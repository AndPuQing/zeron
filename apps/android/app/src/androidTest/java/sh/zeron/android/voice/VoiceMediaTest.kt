package sh.zeron.android.voice

import android.Manifest
import android.media.AudioManager
import androidx.test.platform.app.InstrumentationRegistry
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.*

/** Media lifecycle checks use no provider, audio uploads or account credentials. */
class VoiceMediaTest {
    private val context = InstrumentationRegistry.getInstrumentation().targetContext

    @Test fun cancelledOfferIgnoresLateReplyAndMute() = runBlocking {
        val entered = CompletableDeferred<Unit>()
        val release = CompletableDeferred<Unit>()
        val peer = object : FakePeer() {
            override suspend fun offer(): String {
                entered.complete(Unit)
                return withContext(NonCancellable) { release.await(); "late-offer" }
            }
        }
        val call = FakeCall()
        val media = withContext(Dispatchers.Main) { NativeVoiceMedia(context, { fail("unexpected failure") }) { _, _ -> peer }.also { it.call = call } }
        try {
            media.onRequest(request(1u, VoiceMediaOperation.OFFER))
            withTimeout(5_000) { entered.await() }
            withContext(Dispatchers.Main) { media.close(); media.close() }
            release.complete(Unit)
            media.onRequest(request(2u, VoiceMediaOperation.SET_MUTED))
            delay(150)
            withContext(Dispatchers.Main) {
                assertTrue(call.replies.isEmpty())
                assertEquals(1, peer.closes)
                assertEquals(0, peer.activations)
            }
        } finally { release.complete(Unit); withContext(Dispatchers.Main) { media.close() } }
    }

    @Test fun permissionFailureIsReportedAndCloseIsIdempotent() = runBlocking {
        val peer = object : FakePeer() { override suspend fun prepare() { throw NativeVoicePeer.PermissionDenied() } }
        val call = FakeCall()
        val media = withContext(Dispatchers.Main) { NativeVoiceMedia(context, {}) { _, _ -> peer }.also { it.call = call } }
        try {
            media.onRequest(request(1u, VoiceMediaOperation.PREPARE))
            assertEquals(VoiceMediaFailure.PERMISSION_DENIED, withTimeout(5_000) { call.reply.await() })
            media.onRequest(request(2u, VoiceMediaOperation.CLOSE))
            delay(50)
            withContext(Dispatchers.Main) { media.close(); assertEquals(1, peer.closes) }
        } finally { withContext(Dispatchers.Main) { media.close() } }
    }

    @Test fun muteIsImmediateAfterHostActivation() = runBlocking {
        val peer = FakePeer()
        val call = FakeCall()
        val media = withContext(Dispatchers.Main) { NativeVoiceMedia(context, {}) { _, _ -> peer }.also { it.call = call } }
        try {
            withContext(Dispatchers.Main) { media.muteLocally(true); assertFalse(peer.localMute) }
            media.onRequest(request(1u, VoiceMediaOperation.SET_MUTED))
            withTimeout(5_000) { call.reply.await() }
            withContext(Dispatchers.Main) {
                media.muteLocally(true)
                assertTrue(peer.localMute)
                assertEquals(1, peer.activations)
                media.close()
                media.muteLocally(false)
                assertTrue(peer.localMute)
            }
        } finally { withContext(Dispatchers.Main) { media.close() } }
    }

    @Test fun nativeOfferDoesNotOpenMicrophoneAndCanBeRepeated() = runBlocking {
        InstrumentationRegistry.getInstrumentation().uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        val audio = context.getSystemService(AudioManager::class.java)
        val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
        val before = audio.activeRecordingConfigurations.size
        try {
            repeat(2) {
                val peer = withContext(Dispatchers.Main) { NativeVoicePeer(context, scope) { fail("native audio failed") } }
                try {
                    val offer = withContext(Dispatchers.Main) { peer.prepare(); peer.offer() }
                    assertTrue(offer.contains("m=audio"))
                    assertTrue(offer.contains("m=application"))
                    assertTrue(offer.contains("a=ice-ufrag:"))
                    assertTrue(offer.toByteArray().size <= 65_536)
                    assertEquals(before, audio.activeRecordingConfigurations.size)
                } finally { withContext(Dispatchers.Main) { peer.close(); peer.close() } }
            }
            assertEquals(before, audio.activeRecordingConfigurations.size)
        } finally { scope.cancel() }
    }

    private fun request(id: ULong, operation: VoiceMediaOperation) = VoiceMediaRequest(id, operation, null, false)
    private open class FakePeer : VoicePeer {
        var closes = 0
        var activations = 0
        var localMute = false
        override suspend fun prepare() = Unit
        override suspend fun offer() = "offer"
        override suspend fun applyAnswer(answer: String) = Unit
        override fun setMuted(muted: Boolean) { activations++ }
        override fun muteLocally(muted: Boolean) { localMute = muted }
        override fun setSpeaker(enabled: Boolean) = Unit
        override suspend fun levels() = 0.toUShort() to 0.toUShort()
        override fun close() { closes++ }
    }
    private class FakeCall : VoiceCallInterface {
        val replies = mutableListOf<VoiceMediaFailure?>()
        val reply = CompletableDeferred<VoiceMediaFailure?>()
        override fun completeMedia(requestId: ULong, failure: VoiceMediaFailure?, sdp: String?, microphone: UShort, speaker: UShort) { replies.add(failure); reply.complete(failure) }
        override fun setMuted(muted: Boolean) = Unit
        override fun stop() = Unit
    }
}
