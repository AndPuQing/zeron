package sh.zeron.android.voice

import android.app.Application
import android.os.SystemClock
import kotlinx.coroutines.*
import kotlinx.coroutines.flow.*
import sh.zeron.android.BuildConfig
import uniffi.zeron_core.*

/** Ephemeral call data: captions and audio levels never enter preferences or logs. */
data class VoiceUiState(
    val devices: List<DeviceView> = emptyList(),
    val selectedHost: String? = null,
    val selectedStyle: String? = null,
    val styles: List<String> = emptyList(),
    val live: Boolean = false,
    val sessionId: Long = 0,
    val call: VoiceCallState? = null,
    val hostName: String = "",
    val muted: Boolean = false,
    val speaker: Boolean = true,
    val activeSince: Long? = null,
    val endReason: VoiceEndReason? = null,
    val stageOpen: Boolean = false,
    val settingsOpen: Boolean = false,
) {
    val hosts get() = devices.filter { it.online && compatible(it) }
    val available get() = devices.any(::compatible) || live
    val startHost get() = hosts.firstOrNull { it.id == selectedHost } ?: hosts.singleOrNull()
    val orb get() = call?.orb ?: if (live) VoiceOrb.CONNECTING else VoiceOrb.IDLE
    val status: String get() = when {
        !live -> "Call ended"
        call?.phase == null -> "Calling $hostName…"
        call.phase == VoiceCallPhase.CONNECTING -> "Connecting to $hostName…"
        call.phase == VoiceCallPhase.ENDING -> "Ending…"
        muted -> "Muted"
        orb == VoiceOrb.SPEAKING -> "Speaking"
        orb == VoiceOrb.AWAITING_INPUT -> "Codex needs your answer"
        orb == VoiceOrb.WORKING -> "Working on it…"
        else -> "Listening"
    }
    companion object {
        fun compatible(device: DeviceView) = device.isExecutionHost && "voice-client-media-v1" in device.capabilities
    }
}

/** App-wide owner. A generation never outlives its account or revives after hang-up. */
class VoiceController(private val app: Application, private val client: () -> CoreClient?) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val preferences = app.getSharedPreferences("voice", 0)
    private var identity = ""
    private var generation = 0L
    private var handle: VoiceCallInterface? = null
    private var media: NativeVoiceMedia? = null
    private var preview: Job? = null
    private val _state = MutableStateFlow(VoiceUiState())
    val state = _state.asStateFlow()

    fun refresh() {
        val core = client() ?: return
        val owner = core.userId() + "/" + core.orgId()
        if (owner != identity) {
            stop()
            identity = owner
            _state.value = VoiceUiState(selectedHost = preferences.getString("$owner.host", null),
                selectedStyle = preferences.getString("$owner.style", null), styles = defaultVoiceStyles())
        }
        _state.update { it.copy(devices = core.devices().filter { d -> d.isExecutionHost }) }
    }

    fun selectHost(id: String) {
        if (_state.value.live || _state.value.hosts.none { it.id == id }) return
        preferences.edit().putString("$identity.host", id).apply()
        _state.update { it.copy(selectedHost = id) }
    }
    fun selectStyle(style: String?) {
        if (style != null && style !in _state.value.styles) return
        preferences.edit().putString("$identity.style", style).apply()
        _state.update { it.copy(selectedStyle = style) }
    }
    fun openSettings() { refresh(); _state.update { it.copy(settingsOpen = true) } }
    fun closeSettings() { _state.update { it.copy(settingsOpen = false) } }
    fun openStage() { _state.update { it.copy(stageOpen = true, settingsOpen = false) } }
    fun closeStage() { _state.update { it.copy(stageOpen = false) } }
    fun dismissReason() { _state.update { it.copy(endReason = null) } }
    fun permissionDenied() { _state.update { it.copy(endReason = VoiceEndReason.MICROPHONE_DENIED, stageOpen = true, settingsOpen = false) } }

    fun start(): Boolean {
        if (_state.value.live) { openStage(); return true }
        refresh()
        val core = client() ?: return false
        val host = _state.value.startHost ?: return false.also { openSettings() }
        val token = ++generation
        _state.update { it.copy(live = true, sessionId = token, call = null, muted = false, speaker = true, hostName = host.name,
            activeSince = null, endReason = null, stageOpen = true, settingsOpen = false) }
        return try {
            VoiceCallService.start(app)
            val endpoint = NativeVoiceMedia(app, onFailure = { if (generation == token) finish(VoiceEndReason.AUDIO_UNAVAILABLE) })
            media = endpoint
            val listener = object : VoiceSessionListener {
                override fun onVoiceState(state: VoiceCallState) {
                    scope.launch {
                        if (generation != token || !_state.value.live) return@launch
                        _state.update { current -> current.copy(call = state,
                            activeSince = current.activeSince ?: if (state.phase == VoiceCallPhase.ACTIVE) SystemClock.elapsedRealtime() else null,
                            styles = state.voices.ifEmpty { current.styles }) }
                    }
                }
                override fun onVoiceClosed(reason: VoiceEndReason?) {
                    scope.launch { if (generation == token && _state.value.live) finish(reason) }
                }
            }
            handle = core.startVoice(host.id, _state.value.selectedStyle, endpoint, listener)
            endpoint.call = handle
            true
        } catch (_: Exception) {
            finish(VoiceEndReason.AUDIO_UNAVAILABLE)
            false
        }
    }

    fun toggleMute() {
        if (!_state.value.live) return
        val muted = !_state.value.muted
        media?.muteLocally(muted)
        handle?.setMuted(muted)
        _state.update { it.copy(muted = muted) }
    }
    fun toggleSpeaker() {
        val speaker = !_state.value.speaker
        media?.setSpeaker(speaker)
        _state.update { it.copy(speaker = speaker) }
    }
    fun stop() { if (_state.value.live) finish(null) }
    fun stopSession(id: Long) { if (_state.value.sessionId == id) stop() }
    fun reset() { stop(); identity = ""; _state.value = VoiceUiState() }
    private fun finish(reason: VoiceEndReason?) {
        ++generation
        preview?.cancel(); preview = null
        // Local audio is gone before the remote control can be queued.
        media?.close(); media = null
        handle?.stop()
        (handle as? AutoCloseable)?.close()
        handle = null
        _state.update { it.copy(live = false, call = null, activeSince = null, muted = false, endReason = reason,
            stageOpen = it.stageOpen || reason != null) }
        VoiceCallService.stop(app)
    }

    /** Explicit debug/demo UI preview. Never opens an audio device or contacts a provider. */
    fun preview() {
        if (!BuildConfig.DEBUG || client()?.isDemo() != true) return
        stop()
        ++generation
        _state.update { it.copy(live = true, sessionId = generation, hostName = "Demo host", stageOpen = true, settingsOpen = false,
            activeSince = SystemClock.elapsedRealtime() - 83_000, endReason = null, muted = false) }
        preview = scope.launch {
            var frame = 0
            val script = listOf(VoiceOrb.LISTENING to "Open a chat and fix the failing sync test.",
                VoiceOrb.SPEAKING to "I'm starting a session to look at the sync test.",
                VoiceOrb.WORKING to "The session is checking the parser and its fixtures.",
                VoiceOrb.AWAITING_INPUT to "Should I update the fixture or the parser?")
            while (isActive) {
                val (orb, text) = script[(frame / 40) % script.size]
                val words = text.split(" ").take(1 + frame % 40 / 3).joinToString(" ")
                val level = kotlin.math.abs(kotlin.math.sin(frame / 2.3)).toFloat()
                _state.update { it.copy(call = VoiceCallState(VoiceCallPhase.ACTIVE,
                    if (it.muted) VoiceOrb.MUTED else orb, null,
                    if (orb == VoiceOrb.WORKING) VoiceCallWork.WORKING else if (orb == VoiceOrb.AWAITING_INPUT) VoiceCallWork.AWAITING_INPUT else VoiceCallWork.IDLE,
                    it.muted, orb == VoiceOrb.SPEAKING, words, null, "demo-${frame / 40}",
                    if (it.muted) 0f else level, level, emptyList())) }
                frame++; delay(100)
            }
        }
    }
}

fun voiceEndMessage(reason: VoiceEndReason, host: String): String = when (reason) {
    VoiceEndReason.MICROPHONE_DENIED -> "Allow microphone access in Android Settings to talk with Codex."
    VoiceEndReason.AUDIO_UNAVAILABLE -> "Audio is unavailable. Check your microphone and start a new call."
    VoiceEndReason.SIGN_IN_REQUIRED -> "Sign in to Codex with ChatGPT on $host."
    VoiceEndReason.USAGE_UNAVAILABLE -> "Codex usage is currently unavailable on $host."
    VoiceEndReason.BUSY -> "$host is already on a voice call."
    VoiceEndReason.HOST_UNAVAILABLE -> "$host isn't reachable right now."
    VoiceEndReason.HOST_INCOMPATIBLE -> "Update Zerun and enable remote voice on $host."
    VoiceEndReason.CONNECTION_LOST -> "The call dropped. Start a new call to reconnect."
}
