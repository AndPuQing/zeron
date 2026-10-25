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
import org.junit.Rule
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.io.File
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.defaultVoiceStyles

/** Offline UI and notification checks; preview calls never start an audio device. */
class VoiceFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (context.applicationContext as ZeronApplication).model

    @get:Rule val failureCapture = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            val folder = File(instrumentation.targetContext.cacheDir, "upstream-flow-failures").apply { mkdirs() }
            device.takeScreenshot(File(folder, "${description.methodName}.png"))
            device.dumpWindowHierarchy(File(folder, "${description.methodName}.xml"))
        }
    }

    @Test fun previewMinimizesSurvivesRecreationAndEnds() {
        val before = context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size
        launch(preview = true)
        findDescription("Mute microphone").click()
        await { model.voice.snapshot.value.muted }
        findDescription("Unmute microphone")
        val since = model.voice.snapshot.value.activeSince
        findDescription("Back to Zerun").click()
        assertTrue(device.wait(Until.hasObject(By.descStartsWith("Voice call,")), 5_000))
        instrumentation.runOnMainSync {
            ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).first().recreate()
        }
        assertTrue(device.wait(Until.hasObject(By.descStartsWith("Voice call,")), 10_000))
        assertEquals(since, model.voice.snapshot.value.activeSince)
        device.findObject(By.descStartsWith("Voice call,")).click()
        findDescription("End call").click()
        await { !model.voice.snapshot.value.live }
        assertTrue(device.wait(Until.hasObject(By.desc("New session")), 5_000))
        assertEquals(before, context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size)
    }

    @Test fun microphoneDenialExplainsRecoveryWithoutStartingAudio() {
        val before = context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size
        launch(preview = true)
        instrumentation.runOnMainSync {
            model.voice.closed(uniffi.zeron_core.VoiceEndReason.MICROPHONE_DENIED, generation())
        }
        assertTrue(device.wait(Until.hasObject(By.textContains("needs microphone access")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.text("Open Settings")), 5_000))
        assertFalse(model.voice.live)
        assertEquals(before, context.getSystemService(AudioManager::class.java).activeRecordingConfigurations.size)
        findDescription("Back to Zerun").click()
        await { model.voice.snapshot.value.endReason == null }
    }

    @Test fun voiceSelectionPersistsWhenSettingsReopens() {
        launch()
        val original = model.voice.selectedStyle.value
        try {
            repeat(6) {
                if (device.findObjects(By.text("Voice")).isEmpty()) device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4, device.displayWidth / 2, device.displayHeight / 3, 30)
            }
            device.findObjects(By.text("Voice")).last().click()
            assertTrue(device.wait(Until.hasObject(By.text("Codex runs on")), 5_000))
            val current = VoiceController.styleLabel(original)
            repeat(6) {
                if (!device.hasObject(By.text(current))) device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4, device.displayWidth / 2, device.displayHeight / 3, 20)
            }
            text(current).click()
            val choice = defaultVoiceStyles().first()
            val label = choice.replaceFirstChar { it.titlecase() }
            requireNotNull(device.wait(Until.findObject(By.text(label)), 5_000)).click()
            await { model.voice.selectedStyle.value == choice }
            device.pressBack()
            device.findObjects(By.text("Voice")).last().click()
            assertEquals(choice, model.voice.selectedStyle.value)
            device.pressBack()
        } finally { instrumentation.runOnMainSync { model.voice.selectStyle(original) } }
    }

    @Test fun notificationMutesAndEndsWhileAppIsBackgrounded() {
        instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.RECORD_AUDIO)
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.grantRuntimePermission(context.packageName, Manifest.permission.POST_NOTIFICATIONS)
        launch(preview = true)
        val manager = context.getSystemService(NotificationManager::class.java)
        try {
            instrumentation.runOnMainSync { VoiceCallService.start(context) }
            await { manager.activeNotifications.any { it.id == 0x766f } }
            device.pressHome()
            manager.activeNotifications.single { it.id == 0x766f }.notification.actions.single { it.title.toString().equals("Mute", ignoreCase = true) }.actionIntent.send()
            await { model.voice.snapshot.value.muted }
            manager.activeNotifications.single { it.id == 0x766f }.notification.actions.first { it.title.toString().contains("hang", ignoreCase = true) || it.title.toString().contains("end", ignoreCase = true) }.actionIntent.send()
            await { !model.voice.snapshot.value.live }
            await { manager.activeNotifications.none { it.id == 0x766f } }
        } finally { instrumentation.runOnMainSync { model.voice.stop() } }
    }

    private fun generation(): Long = VoiceController::class.java.getDeclaredField("generation").let {
        it.isAccessible = true
        it.getLong(model.voice)
    }

    private fun text(value: String) = requireNotNull(device.wait(Until.findObject(By.text(value)), 10_000)) { value }

    private fun launch(preview: Boolean = false) {
        instrumentation.runOnMainSync { model.voice.stop(); model.voice.forced = true }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true)
            .putExtra("route", if (preview) "home" else "settings"))
        if (preview) findDescription("New session") else text("Providers")
        assertTrue(model.isDemo)
        device.waitForIdle()
        if (preview) {
            instrumentation.runOnMainSync { assertTrue(model.voice.preview()); model.voice.requestStage() }
            findDescription("Mute microphone")
        }
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
