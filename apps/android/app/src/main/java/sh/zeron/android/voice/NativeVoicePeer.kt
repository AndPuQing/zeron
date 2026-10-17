package sh.zeron.android.voice

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.media.AudioAttributes
import android.media.MediaRecorder
import androidx.core.content.ContextCompat
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import org.webrtc.*
import org.webrtc.audio.JavaAudioDeviceModule

/** Android's audio endpoint; credentials, tools and canonical transcripts stay on the host. */
internal class NativeVoicePeer(
    private val context: Context,
    private val scope: CoroutineScope,
    private val onFailure: () -> Unit,
) : VoicePeer {
    class PermissionDenied : Exception()
    private var factory: PeerConnectionFactory? = null
    private var audio: JavaAudioDeviceModule? = null
    private var source: AudioSource? = null
    private var peer: PeerConnection? = null
    private var channel: DataChannel? = null
    private var track: AudioTrack? = null
    private var closed = false
    private var activated = false
    private val route = VoiceAudioRoute(context) { fail() }

    override suspend fun prepare() {
        check(!closed && peer == null)
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            throw PermissionDenied()
        }
        initialize(context)
        audio = JavaAudioDeviceModule.builder(context)
            .setAudioSource(MediaRecorder.AudioSource.VOICE_COMMUNICATION)
            .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_VOICE_COMMUNICATION)
                .setContentType(AudioAttributes.CONTENT_TYPE_SPEECH).build())
            .setEnableVolumeLogger(false)
            .setAudioRecordErrorCallback(object : JavaAudioDeviceModule.AudioRecordErrorCallback {
                override fun onWebRtcAudioRecordInitError(errorMessage: String) = fail()
                override fun onWebRtcAudioRecordStartError(errorCode: JavaAudioDeviceModule.AudioRecordStartErrorCode, errorMessage: String) = fail()
                override fun onWebRtcAudioRecordError(errorMessage: String) = fail()
            })
            .setAudioTrackErrorCallback(object : JavaAudioDeviceModule.AudioTrackErrorCallback {
                override fun onWebRtcAudioTrackInitError(errorMessage: String) = fail()
                override fun onWebRtcAudioTrackStartError(errorCode: JavaAudioDeviceModule.AudioTrackStartErrorCode, errorMessage: String) = fail()
                override fun onWebRtcAudioTrackError(errorMessage: String) = fail()
            }).createAudioDeviceModule()
        factory = PeerConnectionFactory.builder().setAudioDeviceModule(audio).createPeerConnectionFactory()
        val config = PeerConnection.RTCConfiguration(emptyList()).apply {
            sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
            bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
        }
        val connection = checkNotNull(factory?.createPeerConnection(config, observer))
        peer = connection
        // Negotiation must not start either Android audio device before lease confirmation.
        connection.setAudioRecording(false)
        connection.setAudioPlayout(false)
        source = factory?.createAudioSource(MediaConstraints())
        track = factory?.createAudioTrack("voice-audio", source).also { it?.setEnabled(false) }
        checkNotNull(connection.addTrack(checkNotNull(track), listOf("voice")))
        channel = checkNotNull(connection.createDataChannel("oai-events", DataChannel.Init().apply { ordered = true }))
        channel?.registerObserver(object : DataChannel.Observer {
            override fun onBufferedAmountChange(previousAmount: Long) = Unit
            override fun onStateChange() {
                scope.launch {
                    if (!closed && channel?.state() in listOf(DataChannel.State.CLOSING, DataChannel.State.CLOSED)) fail()
                }
            }
            override fun onMessage(buffer: DataChannel.Buffer) = Unit
        })
    }

    override suspend fun offer(): String {
        val connection = connection()
        val description: SessionDescription = sdp { connection.createOffer(it, MediaConstraints().apply {
            mandatory.add(MediaConstraints.KeyValuePair("OfferToReceiveAudio", "true"))
        }) }
        setDescription { connection.setLocalDescription(it, description) }
        waitUntil { connection.iceGatheringState() == PeerConnection.IceGatheringState.COMPLETE }
        return checkNotNull(connection.localDescription?.description).also { check(it.toByteArray().size <= 65_536) }
    }

    override suspend fun applyAnswer(answer: String) {
        check(answer.isNotEmpty() && answer.toByteArray().size <= 65_536)
        val connection = connection()
        setDescription { connection.setRemoteDescription(it, SessionDescription(SessionDescription.Type.ANSWER, answer)) }
        waitUntil { connection.connectionState() == PeerConnection.PeerConnectionState.CONNECTED && channel?.state() == DataChannel.State.OPEN }
    }

    override fun setMuted(muted: Boolean) {
        val connection = connection()
        check(connection.connectionState() == PeerConnection.PeerConnectionState.CONNECTED)
        if (!activated) {
            route.activate()
            activated = true
            connection.setAudioPlayout(true)
            connection.setAudioRecording(true)
        }
        track?.setEnabled(!muted)
    }
    override fun muteLocally(muted: Boolean) { if (!closed && activated) track?.setEnabled(!muted) }
    override fun setSpeaker(enabled: Boolean) = route.setSpeaker(enabled)

    override suspend fun levels(): Pair<UShort, UShort> {
        val connection = connection()
        val report: RTCStatsReport = suspendCancellableCoroutine { continuation ->
            connection.getStats { if (continuation.isActive) continuation.resume(it) }
        }
        var microphone = 0.0
        var speaker = 0.0
        for (stat in report.statsMap.values) {
            val level = (stat.members["audioLevel"] as? Number)?.toDouble() ?: continue
            if (stat.type == "media-source") microphone = maxOf(microphone, level)
            if (stat.type == "inbound-rtp") speaker = maxOf(speaker, level)
        }
        return (microphone.coerceIn(0.0, 1.0) * 65535).toInt().toUShort() to
            (speaker.coerceIn(0.0, 1.0) * 65535).toInt().toUShort()
    }

    override fun close() {
        if (closed) return
        closed = true
        track?.setEnabled(false)
        peer?.setAudioRecording(false)
        peer?.setAudioPlayout(false)
        channel?.unregisterObserver()
        channel?.close()
        peer?.close()
        channel?.dispose(); channel = null
        peer?.dispose(); peer = null
        track?.dispose(); track = null
        source?.dispose(); source = null
        factory?.dispose(); factory = null
        audio?.release(); audio = null
        route.close()
        activated = false
    }

    private fun connection(): PeerConnection = checkNotNull(peer).also { check(!closed) }
    private fun fail() { scope.launch { yield(); if (!closed) onFailure() } }
    private suspend fun waitUntil(ready: () -> Boolean) = withTimeout(25_000) {
        while (!ready()) { check(!closed); delay(20) }
        check(!closed)
    }
    private suspend fun sdp(start: (SdpObserver) -> Unit): SessionDescription = suspendCancellableCoroutine { c ->
        val done = AtomicBoolean()
        c.invokeOnCancellation { done.set(true) }
        start(object : SdpObserver {
            override fun onCreateSuccess(description: SessionDescription) { if (done.compareAndSet(false, true)) c.resume(description) }
            override fun onCreateFailure(error: String) { if (done.compareAndSet(false, true)) c.resumeWithException(IllegalStateException("SDP offer failed")) }
            override fun onSetSuccess() = Unit
            override fun onSetFailure(error: String) = Unit
        })
    }
    private suspend fun setDescription(start: (SdpObserver) -> Unit): Unit = suspendCancellableCoroutine { c ->
        val done = AtomicBoolean()
        c.invokeOnCancellation { done.set(true) }
        start(object : SdpObserver {
            override fun onCreateSuccess(description: SessionDescription) = Unit
            override fun onCreateFailure(error: String) = Unit
            override fun onSetSuccess() { if (done.compareAndSet(false, true)) c.resume(Unit) }
            override fun onSetFailure(error: String) { if (done.compareAndSet(false, true)) c.resumeWithException(IllegalStateException("SDP description failed")) }
        })
    }
    private val observer = object : PeerConnection.Observer {
        override fun onSignalingChange(state: PeerConnection.SignalingState) = Unit
        override fun onIceConnectionChange(state: PeerConnection.IceConnectionState) {
            if (state in listOf(PeerConnection.IceConnectionState.FAILED, PeerConnection.IceConnectionState.DISCONNECTED, PeerConnection.IceConnectionState.CLOSED)) fail()
        }
        override fun onConnectionChange(state: PeerConnection.PeerConnectionState) {
            if (state == PeerConnection.PeerConnectionState.FAILED) fail()
        }
        override fun onIceConnectionReceivingChange(receiving: Boolean) = Unit
        override fun onIceGatheringChange(state: PeerConnection.IceGatheringState) = Unit
        override fun onIceCandidate(candidate: IceCandidate) = Unit
        override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>) = Unit
        override fun onAddStream(stream: MediaStream) = Unit
        override fun onRemoveStream(stream: MediaStream) = Unit
        override fun onDataChannel(channel: DataChannel) { scope.launch { channel.close(); channel.dispose() } }
        override fun onRenegotiationNeeded() = Unit
    }
    companion object {
        private var initialized = false
        @Synchronized private fun initialize(context: Context) {
            if (!initialized) {
                PeerConnectionFactory.initialize(PeerConnectionFactory.InitializationOptions.builder(context).createInitializationOptions())
                Logging.enableLogToDebugOutput(Logging.Severity.LS_NONE)
                initialized = true
            }
        }
    }
}
