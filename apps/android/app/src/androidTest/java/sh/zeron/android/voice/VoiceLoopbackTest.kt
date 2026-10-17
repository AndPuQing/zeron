package sh.zeron.android.voice

import android.Manifest
import android.media.AudioManager
import androidx.test.platform.app.InstrumentationRegistry
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.webrtc.*

/** A real local WebRTC answerer validates the media contract without any provider. */
class VoiceLoopbackTest {
    @Test fun gatheredOfferConnectsWithEventsAndAudioRemainsDisabled() = runBlocking {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        val audio = context.getSystemService(AudioManager::class.java)
        val before = audio.activeRecordingConfigurations.size
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        val client = withContext(Dispatchers.Main) { NativeVoicePeer(context, scope) { fail("Unexpected media failure") } }
        var factory: PeerConnectionFactory? = null
        var server: PeerConnection? = null
        val events = AtomicReference<DataChannel?>()
        try {
            val offer = withContext(Dispatchers.Main) { client.prepare(); client.offer() }
            factory = withContext(Dispatchers.Main) { PeerConnectionFactory.builder().createPeerConnectionFactory() }
            server = withContext(Dispatchers.Main) {
                checkNotNull(checkNotNull(factory).createPeerConnection(PeerConnection.RTCConfiguration(emptyList()).apply {
                    sdpSemantics = PeerConnection.SdpSemantics.UNIFIED_PLAN
                    bundlePolicy = PeerConnection.BundlePolicy.MAXBUNDLE
                }, object : PeerConnection.Observer {
                    override fun onSignalingChange(state: PeerConnection.SignalingState) = Unit
                    override fun onIceConnectionChange(state: PeerConnection.IceConnectionState) = Unit
                    override fun onIceConnectionReceivingChange(receiving: Boolean) = Unit
                    override fun onIceGatheringChange(state: PeerConnection.IceGatheringState) = Unit
                    override fun onIceCandidate(candidate: IceCandidate) = Unit
                    override fun onIceCandidatesRemoved(candidates: Array<out IceCandidate>) = Unit
                    override fun onAddStream(stream: MediaStream) = Unit
                    override fun onRemoveStream(stream: MediaStream) = Unit
                    override fun onRenegotiationNeeded() = Unit
                    override fun onDataChannel(channel: DataChannel) { events.set(channel) }
                })).also { it.setAudioRecording(false); it.setAudioPlayout(false) }
            }
            val remote = checkNotNull(server)
            withContext(Dispatchers.Main) {
                description(false) { remote.setRemoteDescription(it, SessionDescription(SessionDescription.Type.OFFER, offer)) }
                val answer = checkNotNull(description(true) { remote.createAnswer(it, MediaConstraints()) })
                description(false) { remote.setLocalDescription(it, answer) }
                withTimeout(25_000) { while (remote.iceGatheringState() != PeerConnection.IceGatheringState.COMPLETE) delay(20) }
                client.applyAnswer(checkNotNull(remote.localDescription).description)
            }
            withTimeout(5_000) { while (events.get()?.state() != DataChannel.State.OPEN) delay(20) }
            assertEquals("oai-events", events.get()?.label())
            assertEquals(PeerConnection.PeerConnectionState.CONNECTED, remote.connectionState())
            assertEquals(before, audio.activeRecordingConfigurations.size)
            withContext(Dispatchers.Main) { client.levels() }
        } finally {
            withContext(Dispatchers.Main) {
                client.close()
                events.get()?.close(); events.get()?.dispose()
                server?.close(); server?.dispose()
                factory?.dispose()
            }
            scope.cancel()
        }
        assertEquals(before, audio.activeRecordingConfigurations.size)
    }

    private suspend fun description(create: Boolean, start: (SdpObserver) -> Unit): SessionDescription? = suspendCancellableCoroutine { c ->
        start(object : SdpObserver {
            override fun onCreateSuccess(value: SessionDescription) { if (create && c.isActive) c.resume(value) }
            override fun onSetSuccess() { if (!create && c.isActive) c.resume(null) }
            override fun onCreateFailure(error: String) { if (c.isActive) c.resumeWithException(IllegalStateException("Creating local answer failed")) }
            override fun onSetFailure(error: String) { if (c.isActive) c.resumeWithException(IllegalStateException("Setting local description failed")) }
        })
    }
}
