package sh.zeron.android.voice

import android.Manifest
import android.content.Intent
import android.media.AudioManager
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import kotlinx.coroutines.*
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.VoiceEndReason

/** Offline regressions exercise the upstream controller and real WebRTC peer. */
class VoiceMediaTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val controller get() = (context.applicationContext as ZeronApplication).model.voice

    @Test fun oldCallCallbacksCannotEndOrOverwriteTheNextCall() {
        launch()
        instrumentation.runOnMainSync {
            assertTrue(controller.preview())
            val oldGeneration = generation()
            val oldState = requireNotNull(controller.snapshot.value.state).copy(chatId = "stale-call")
            controller.stop()
            assertTrue(controller.preview())
            val next = controller.snapshot.value
            try {
                controller.receive(oldState, oldGeneration)
                controller.closed(VoiceEndReason.CONNECTION_LOST, oldGeneration)
                assertEquals(next, controller.snapshot.value)
                assertTrue(controller.live)
                // The active generation still receives changes and can end normally.
                controller.receive(oldState.copy(chatId = "current-call"), generation())
                assertEquals("current-call", controller.snapshot.value.chatId)
                controller.closed(VoiceEndReason.CONNECTION_LOST, generation())
                assertFalse(controller.live)
            } finally { controller.stop(); controller.dismissNotice(); controller.dismissEndReason() }
        }
    }

    @Test fun nativeOfferDoesNotCaptureAndClosePreventsReuse() = runBlocking {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        launch()
        val audio = context.getSystemService(AudioManager::class.java)
        val before = audio.activeRecordingConfigurations.size
        repeat(2) {
            val peer = withContext(Dispatchers.Main) { VoicePeer(context, controller, true) }
            try {
                val offer = withContext(Dispatchers.Main) { peer.prepare(); peer.offer() }
                assertTrue(offer.contains("m=audio"))
                assertTrue(offer.contains("m=application"))
                assertTrue(offer.contains("a=ice-ufrag:"))
                assertTrue(offer.toByteArray().size <= 65_536)
                assertEquals(before, audio.activeRecordingConfigurations.size)
            } finally { withContext(Dispatchers.Main) { peer.close(); peer.close() } }
            withContext(Dispatchers.Main) {
                try { peer.prepare(); fail("Closed peer was reused") }
                catch (failure: VoicePeer.Failure) { assertEquals(VoicePeer.Failure.Kind.Closed, failure.kind) }
            }
        }
        withTimeout(5_000) { while (audio.activeRecordingConfigurations.size != before) delay(20) }
    }

    private fun generation(): Long = VoiceController::class.java.getDeclaredField("generation").let {
        it.isAccessible = true
        it.getLong(controller)
    }

    private fun launch() {
        instrumentation.runOnMainSync { controller.stop() }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", "home"))
        assertTrue(UiDevice.getInstance(instrumentation).wait(Until.hasObject(By.desc("New session")), 10_000))
        instrumentation.waitForIdleSync()
    }
}
