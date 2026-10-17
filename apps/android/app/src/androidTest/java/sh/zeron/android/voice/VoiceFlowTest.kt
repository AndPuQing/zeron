package sh.zeron.android.voice

import android.Manifest
import android.app.NotificationManager
import android.content.Intent
import android.media.AudioManager
import android.os.Build
import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.defaultVoiceStyles

/** Offline UI and notification checks; preview calls never start an audio device. */
class VoiceFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (context.applicationContext as ZeronApplication).model

    @Test fun previewMinimizesSurvivesRecreationAndEnds() {
        val before = context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size
        launch(preview = true)
        findDescription("Mute microphone").click()
        await { model.voice.state.value.muted }
        findDescription("Unmute microphone")
        val id = model.voice.state.value.sessionId
        findDescription("Back to Zerun").click()
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Codex voice ·")), 5_000))
        instrumentation.runOnMainSync {
            ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).first().recreate()
        }
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Codex voice ·")), 10_000))
        assertEquals(id, model.voice.state.value.sessionId)
        device.findObject(By.textStartsWith("Codex voice ·")).click()
        findDescription("End call").click()
        await { !model.voice.state.value.live }
        findDescription("Back to Zerun").click()
        assertTrue(device.wait(Until.hasObject(By.text("New session")), 5_000))
        assertEquals(before, context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size)
    }

    @Test fun microphoneDenialExplainsRecoveryWithoutStartingACall() {
        launch()
        instrumentation.runOnMainSync { model.voice.permissionDenied() }
        assertTrue(device.wait(Until.hasObject(By.text("Android Settings")), 5_000))
        assertTrue(device.hasObject(By.textContains("Allow microphone access")))
        device.findObject(By.text("Close")).click()
        await { !model.voice.state.value.stageOpen }
        assertFalse(model.voice.state.value.live)
        assertNull(model.voice.state.value.endReason)
    }

    @Test fun voiceSelectionPersistsWhenSettingsReopens() {
        launch()
        val original = model.voice.state.value.selectedStyle
        try {
            findDescription("Voice settings").click()
            assertTrue(device.wait(Until.hasObject(By.text("Codex runs on")), 5_000))
            val choice = defaultVoiceStyles().first()
            val label = choice.replaceFirstChar { it.titlecase() }
            repeat(5) {
                if (!device.hasObject(By.text(label))) device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4, device.displayWidth / 2, device.displayHeight / 3, 20)
            }
            requireNotNull(device.wait(Until.findObject(By.text(label)), 5_000)).click()
            await { model.voice.state.value.selectedStyle == choice }
            findDescription("Close voice settings").click()
            findDescription("Voice settings").click()
            assertEquals(choice, model.voice.state.value.selectedStyle)
            findDescription("Close voice settings").click()
        } finally { instrumentation.runOnMainSync { model.voice.selectStyle(original) } }
    }

    @Test fun notificationMutesAndEndsWhileAppIsBackgrounded() {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.POST_NOTIFICATIONS)
        launch(preview = true)
        val manager = context.getSystemService(NotificationManager::class.java)
        try {
            instrumentation.runOnMainSync { VoiceCallService.start(context) }
            await { manager.activeNotifications.any { it.id == 3461 } }
            device.pressHome()
            manager.activeNotifications.single { it.id == 3461 }.notification.actions[0].actionIntent.send()
            await { model.voice.state.value.muted }
            manager.activeNotifications.single { it.id == 3461 }.notification.actions[1].actionIntent.send()
            await { !model.voice.state.value.live }
            await { manager.activeNotifications.none { it.id == 3461 } }
        } finally { instrumentation.runOnMainSync { model.voice.stop() } }
    }

    private fun launch(preview: Boolean = false) {
        instrumentation.runOnMainSync { model.voice.stop(); model.voice.closeStage(); model.voice.closeSettings() }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("voice-preview", preview)
            .putExtra("route", if (preview) "home" else "settings"))
        if (preview) findDescription("Mute microphone") else findDescription("Voice settings")
        assertTrue(model.isDemo)
        device.waitForIdle()
    }
    private fun findDescription(value: String) = requireNotNull(device.wait(Until.findObject(By.desc(value)), 10_000)) { value }
    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(50)
        }
        fail("Timed out waiting for voice state")
    }
}
