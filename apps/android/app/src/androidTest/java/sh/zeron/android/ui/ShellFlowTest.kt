package sh.zeron.android.ui

import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication
import sh.zeron.android.core.NewSessionDraft
import sh.zeron.android.design.ThemeMode
import uniffi.zeron_core.BusyPolicy
import uniffi.zeron_core.SendRequest

/** Shell controls must keep their behavior across menus, sheets and recreation. */
class ShellFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @Test
    fun appearanceSelectionSurvivesRecreation() {
        launch("settings")
        val original = model.appearance.value
        try {
            for (mode in listOf(ThemeMode.Dark, ThemeMode.Light)) {
                findText(mode.name, aboveBottomChrome = true).click()
                await { model.appearance.value.mode == mode }
                device.waitForIdle()
                assertSystemBars(mode)
                launch("settings")
                assertEquals(mode, model.appearance.value.mode)
                findText(mode.name, aboveBottomChrome = true)
                assertSystemBars(mode)
            }
        } finally {
            instrumentation.runOnMainSync { model.setAppearance(original) }
        }
    }

    @Test
    fun renameAlertSavesAndCancels() {
        launch("home")
        val original = requireNotNull(model.row("chat-deploy")).title
        findText(original).longClick()
        requireNotNull(device.wait(Until.findObject(By.text("Rename")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.text("Rename session")), 5_000))
        requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).text = "Renamed from Android"
        device.findObject(By.text("Rename")).click()
        await { model.row("chat-deploy")?.title == "Renamed from Android" }
        assertTrue(device.wait(Until.gone(By.text("Rename session")), 5_000))
        findText("Renamed from Android").longClick()
        requireNotNull(device.wait(Until.findObject(By.text("Rename")), 5_000)).click()
        requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).text = "Discard this"
        device.findObject(By.text("Cancel")).click()
        assertTrue(device.wait(Until.gone(By.text("Rename session")), 5_000))
        assertEquals("Renamed from Android", model.row("chat-deploy")?.title)
        instrumentation.runOnMainSync { model.rename("chat-deploy", original) }
    }

    @Test
    fun projectSheetReturnsSelectionToComposer() {
        launch("new")
        val projects = model.workspace.value!!.projects
        val project = projects.firstOrNull { it.id == model.lastDraft.projectId }
            ?: projects.firstOrNull { it.deviceOnline } ?: projects.first()
        assertTrue(device.wait(Until.hasObject(By.text(project.name)), 5_000))
        // The picker remains usable while the composer owns the keyboard.
        device.findObject(By.text(project.name)).click()
        assertTrue(device.wait(Until.hasObject(By.text("Project")), 5_000))
        findText("No project").click()
        assertTrue(device.wait(Until.gone(By.text("Project")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.text("No project")), 5_000))
        device.findObject(By.text("No project")).click()
        assertTrue(device.wait(Until.hasObject(By.text("Project")), 5_000))
        device.findObject(By.text("Close")).click()
        assertTrue(device.wait(Until.gone(By.text("Project")), 5_000))
        assertTrue(device.hasObject(By.text("No project")))
    }

    @Test
    fun newSessionDraftKeepsItsTargetAfterClose() = checkNewSessionDraft(useSystemBack = false)

    @Test
    fun newSessionDraftKeepsItsTargetAfterSystemBack() = checkNewSessionDraft(useSystemBack = true)

    private fun checkNewSessionDraft(useSystemBack: Boolean) {
        launch("home")
        val original = model.lastDraft
        val project = model.workspace.value!!.projects.first()
        val initial = NewSessionDraft(projectId = project.id, harness = "codex")
        try {
            instrumentation.runOnMainSync { model.lastDraft = initial }
            findText("New session").click()
            findText(project.name).click()
            assertTrue(device.wait(Until.hasObject(By.text("Project")), 5_000))
            findText("No project").click()
            assertTrue(device.wait(Until.gone(By.text("Project")), 5_000))
            val text = "Keep this projectless draft"
            editor().text = text
            device.waitForIdle()
            if (useSystemBack) {
                // Dismiss the keyboard separately from navigating away.
                var keyboardVisible = false
                instrumentation.runOnMainSync {
                    val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
                    keyboardVisible = ViewCompat.getRootWindowInsets(activity.window.decorView)
                        ?.isVisible(WindowInsetsCompat.Type.ime()) == true
                }
                if (keyboardVisible) {
                    device.pressBack()
                    device.waitForIdle()
                }
                device.pressBack()
            } else {
                requireNotNull(device.wait(Until.findObject(By.desc("Close")), 5_000)).click()
            }
            findText("New session").click()
            assertEquals(initial.copy(projectId = null, hostId = project.deviceId), model.lastDraft)
            assertEquals(text, editor().text)
            assertTrue(device.hasObject(By.text("No project")))
            requireNotNull(device.wait(Until.findObject(By.desc("Close")), 5_000)).click()
            findText("New session") // Wait for disposal before restoring test preferences.
        } finally {
            instrumentation.runOnMainSync { model.lastDraft = original }
        }
    }

    @Test
    fun leavingQueuedEditRestoresTheUnsentDraft() {
        launch("home", fast = false, longReply = true)
        // A fresh slow demo keeps the queued message available during navigation.
        instrumentation.runOnMainSync { model.startDemo() }
        val chatId = "chat-deploy"
        val title = requireNotNull(model.row(chatId)).title
        findText(title).click()
        val handle = model.client.value!!.openSession(chatId)
        try {
            handle.send(SendRequest("Keep this turn running", emptyList(), null, BusyPolicy.QUEUE))
            await { handle.composer().live.turnRunning }
            val queued = "This is the queued message"
            handle.send(SendRequest(queued, emptyList(), null, BusyPolicy.QUEUE))
            await { handle.composer().queue.any { it.visibleText == queued } }
            val draft = "My unsent draft, not the queued message"
            editor().text = draft
            requireNotNull(device.wait(Until.findObject(By.desc("Queued message actions")), 5_000)).click()
            requireNotNull(device.wait(Until.findObject(By.text("Edit")), 5_000)).click()
            assertTrue(device.wait(Until.hasObject(By.text("Editing queued message")), 5_000))
            assertEquals(queued, editor().text)
            editor().text = "Discard these queue edits"
            requireNotNull(device.wait(Until.findObject(By.desc("Back").pkg(instrumentation.targetContext.packageName)), 5_000)).click()
            // The outgoing chat stays in the accessibility tree during its exit animation.
            // Wait for disposal before selecting the same title in the session list.
            assertTrue(device.wait(Until.gone(By.clazz("android.widget.EditText")), 5_000))
            assertTrue(device.wait(Until.gone(By.desc("Back").pkg(instrumentation.targetContext.packageName)), 5_000))
            if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
            requireNotNull(device.wait(Until.findObject(By.res("session-$chatId")), 5_000)).click()
            // Streaming accessibility queries can themselves take longer than five seconds.
            // Allow another snapshot after the list-to-chat transition, keeping the text assertion strict.
            assertEquals(draft, editor(timeout = 15_000).text)
            assertFalse(device.hasObject(By.text("Editing queued message")))
            assertEquals(queued, handle.composer().queue.single().visibleText)
        } finally {
            runBlocking { handle.composer().queue.forEach { handle.removeQueued(it.id) } }
            handle.interrupt()
        }
    }

    private fun editor(timeout: Long = 5_000) = requireNotNull(
        device.wait(Until.findObject(By.clazz("android.widget.EditText")), timeout),
    ) { "Missing composer editor after ${timeout}ms" }

    private fun launch(route: String, fast: Boolean = true, longReply: Boolean = false) {
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", fast).putExtra("longreply", longReply).putExtra("route", route))
        assertTrue(device.wait(Until.hasObject(By.pkg(context.packageName)), 20_000))
        await { model.isDemo && model.workspace.value != null }
        device.waitForIdle()
    }

    private fun findText(text: String, aboveBottomChrome: Boolean = false): androidx.test.uiautomator.UiObject2 {
        repeat(5) {
            // Layer-backed scrolling can leave UiAutomation's cached bounds stale.
            if (aboveBottomChrome && Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
            var scrollDown = false
            device.wait(Until.findObject(By.text(text)), 1_000)?.let {
                // Lazy content remains discoverable behind the floating bottom controls.
                val bounds = it.visibleBounds
                if (!aboveBottomChrome || bounds.centerY() in device.displayHeight / 8..device.displayHeight * 3 / 4) return it
                scrollDown = bounds.centerY() < device.displayHeight / 8
            }
            if (aboveBottomChrome) {
                // A short drag avoids flinging a newly visible radio row past the top.
                device.swipe(device.displayWidth / 2, device.displayHeight / 2,
                    device.displayWidth / 2, device.displayHeight * (if (scrollDown) 2 else 1) / 3, 60)
            } else device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4,
                device.displayWidth / 2, device.displayHeight / 3, 20)
            device.waitForIdle()
        }
        error("No visible control: $text")
    }

    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(50)
        }
        fail("Timed out waiting for shell state")
    }

    private fun assertSystemBars(mode: ThemeMode) {
        instrumentation.runOnMainSync {
            val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
            val bars = WindowCompat.getInsetsController(activity.window, activity.window.decorView)
            assertEquals(mode == ThemeMode.Light, bars.isAppearanceLightStatusBars)
            assertEquals(mode == ThemeMode.Light, bars.isAppearanceLightNavigationBars)
        }
    }
}
