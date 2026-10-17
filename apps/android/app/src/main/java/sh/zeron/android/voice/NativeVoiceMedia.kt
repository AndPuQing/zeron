package sh.zeron.android.voice

import android.content.Context
import kotlinx.coroutines.*
import uniffi.zeron_core.*

internal interface VoicePeer {
    suspend fun prepare()
    suspend fun offer(): String
    suspend fun applyAnswer(answer: String)
    fun setMuted(muted: Boolean)
    fun muteLocally(muted: Boolean)
    fun setSpeaker(enabled: Boolean)
    suspend fun levels(): Pair<UShort, UShort>
    fun close()
}

/** One main-thread media executor per call; closing it invalidates every pending operation. */
internal class NativeVoiceMedia(
    context: Context,
    private val onFailure: () -> Unit,
    peerFactory: ((CoroutineScope, () -> Unit) -> VoicePeer)? = null,
) : VoiceMediaListener {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val peer = peerFactory?.invoke(scope, ::failed) ?: NativeVoicePeer(context, scope, ::failed)
    private val operations = mutableMapOf<ULong, Job>()
    private var closed = false
    private var activated = false
    private var localMute: Boolean? = null
    var call: VoiceCallInterface? = null

    override fun onRequest(request: VoiceMediaRequest) {
        scope.launch {
            if (request.operation == VoiceMediaOperation.CLOSE) { close(); return@launch }
            if (closed) return@launch
            if (operations.size >= 4 || request.requestId in operations) { failed(); return@launch }
            val job = scope.launch(start = CoroutineStart.LAZY) {
                try {
                    var sdp: String? = null
                    var levels: Pair<UShort, UShort> = 0.toUShort() to 0.toUShort()
                    when (request.operation) {
                        VoiceMediaOperation.PREPARE -> peer.prepare()
                        VoiceMediaOperation.OFFER -> sdp = peer.offer()
                        VoiceMediaOperation.APPLY_ANSWER -> peer.applyAnswer(checkNotNull(request.sdp))
                        VoiceMediaOperation.SET_MUTED -> { peer.setMuted(localMute ?: request.muted); activated = true }
                        VoiceMediaOperation.LEVELS -> levels = peer.levels()
                        VoiceMediaOperation.CLOSE -> Unit
                    }
                    if (!closed && isActive) call?.completeMedia(request.requestId, null, sdp, levels.first, levels.second)
                } catch (_: TimeoutCancellationException) {
                    if (!closed) call?.completeMedia(request.requestId, VoiceMediaFailure.UNAVAILABLE, null, 0.toUShort(), 0.toUShort())
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    if (!closed) call?.completeMedia(request.requestId,
                        if (e is NativeVoicePeer.PermissionDenied) VoiceMediaFailure.PERMISSION_DENIED else VoiceMediaFailure.UNAVAILABLE,
                        null, 0.toUShort(), 0.toUShort())
                } finally {
                    operations.remove(request.requestId)
                }
            }
            operations[request.requestId] = job
            job.start()
        }
    }

    fun muteLocally(muted: Boolean) {
        if (closed) return
        // Retain an early mute so an already queued activation cannot reopen the track.
        localMute = muted
        if (activated) peer.muteLocally(muted)
    }
    fun setSpeaker(enabled: Boolean) { if (!closed) peer.setSpeaker(enabled) }
    private fun failed() { if (!closed) { close(); onFailure() } }
    fun close() {
        if (closed) return
        closed = true
        peer.close()
        call = null
        operations.clear()
        scope.cancel()
    }
}
