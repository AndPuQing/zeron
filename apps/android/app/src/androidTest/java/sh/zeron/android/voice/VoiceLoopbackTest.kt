package sh.zeron.android.voice

import android.Manifest
import android.content.Intent
import android.media.AudioManager
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import org.webrtc.*
import sh.zeron.android.ZeronApplication

/** A real local WebRTC answerer validates the media contract without any provider. */
class VoiceLoopbackTest {
    @Test fun negotiationDefersCaptureUntilActivationAndCloseReleasesAudio() = runBlocking {
        val instrumentation = InstrumentationRegistry.getInstrumentation()
        val context = instrumentation.targetContext
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true))
        val device = UiDevice.getInstance(instrumentation)
        assertTrue(device.wait(Until.hasObject(By.pkg(context.packageName)), 10_000))
        device.waitForIdle()
        val audio = context.getSystemService(AudioManager::class.java)
        val before = audio.activeRecordingConfigurations.size
        val previousMode = audio.mode
        val client = withContext(Dispatchers.Main) { VoicePeer(context, (context.applicationContext as ZeronApplication).model.voice, true).also { it.onFailure = { fail("Unexpected media failure") } } }
        var factory: PeerConnectionFactory? = null
        var server: PeerConnection? = null
        val events = AtomicReference<DataChannel?>()
        try {
            val offer = withContext(Dispatchers.Main) { client.prepare(); client.offer() }
            assertTrue("Loopback needs a network interface with ICE candidates", offer.lineSequence().any { it.startsWith("a=candidate:") })
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
                client.apply(checkNotNull(remote.localDescription).description)
            }
            withTimeout(5_000) { while (events.get()?.state() != DataChannel.State.OPEN) delay(20) }
            assertEquals("oai-events", events.get()?.label())
            assertEquals(PeerConnection.PeerConnectionState.CONNECTED, remote.connectionState())
            assertEquals(before, audio.activeRecordingConfigurations.size)
            withContext(Dispatchers.Main) { client.levels() }
            withContext(Dispatchers.Main) { client.setMuted(false) }
            withTimeout(5_000) { while (audio.activeRecordingConfigurations.size <= before) delay(20) }
            assertEquals(AudioManager.MODE_IN_COMMUNICATION, audio.mode)
            withContext(Dispatchers.Main) { client.setMuted(true) }
        } finally {
            withContext(Dispatchers.Main) {
                client.close()
                events.get()?.close(); events.get()?.dispose()
                server?.close(); server?.dispose()
                factory?.dispose()
            }
        }
        withTimeout(5_000) { while (audio.activeRecordingConfigurations.size != before || audio.mode != previousMode) delay(20) }
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
