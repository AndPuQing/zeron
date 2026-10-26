package sh.zeron.android.ui

import android.content.Intent
import android.os.Build
import android.os.SystemClock
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.compose.ui.graphics.ImageBitmap
import androidx.lifecycle.ViewModelProvider
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
import sh.zeron.android.MainActivity
import sh.zeron.android.ZeronApplication

/** Exercise the upstream phone stack and tablet shell against the native offline demo. */
class UpstreamNavigationTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val context = instrumentation.targetContext
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (context.applicationContext as ZeronApplication).model

    @get:Rule val failureCapture = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            capture("failure-${description.methodName}")
            val folder = File(context.cacheDir, "upstream-navigation")
            device.dumpWindowHierarchy(File(folder, "failure-${description.methodName}.xml"))
        }
    }

    @Test fun phoneSearchBarActionsAndBackStack() {
        launch("home")
        capture("phone-sessions")
        description("Settings").click()
        text("Providers").click()
        text("Claude Code")
        description("Back").click()
        assertTrue(device.wait(Until.gone(By.text("Claude Code")), 5_000))
        text("Providers")
        description("Back").click()
        awaitPhoneHome()
        description("Search sessions").click()
        editor().text = "Streaming veil"
        text(model.row("chat-veil")!!.title)
        // Settle the IME before tapping the in-app navigation control.
        dismissKeyboard()
        description("Back").click()
        awaitPhoneHome()
        description("New session").click()
        editor()
        capture("phone-new-session")
        description("Close").click()
        awaitPhoneHome()
        description("Settings").click()
        scrollTo("Archived sessions").click()
        text("Archived")
        description("Back").click()
        assertTrue(device.wait(Until.gone(By.text("Archived")), 5_000))
        text("Archived sessions")
        // Settings restores its scroll position; the upstream Back row is above the fold.
        device.pressBack()
        assertTrue(device.wait(Until.hasObject(By.desc("New session")), 5_000))
    }

    @Test fun collapsedNewSessionFabKeepsAccessibleAction() {
        launch("home")
        description("New session")
        device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4,
            device.displayWidth / 2, device.displayHeight / 3, 30)
        assertTrue(device.wait(Until.gone(By.text("New session")), 5_000))
        description("New session").click()
        editor()
        description("Close").click()
        awaitPhoneHome()
    }

    @Test fun tabletSettingsDialogAndInspectorKeepTheSelectedChat() {
        try {
            device.executeShellCommand("wm size 3000x2000")
            launch("chat:chat-deploy")
            description("More")
            capture("tablet-before-settings")
            assertTrue(device.wait(Until.hasObject(By.desc("Session options")), 10_000))
            val draft = composer()
            instrumentation.runOnMainSync { draft.setText("Keep the tablet draft") }
            capture("tablet-chat")
            description("Settings").click()
            text("Providers").click()
            text("Claude Code")
            description("Back").click()
            assertTrue(device.wait(Until.gone(By.text("Claude Code")), 5_000))
            text("Providers")
            capture("tablet-settings-dialog")
            description("Back").click()
            assertTrue(device.wait(Until.gone(By.text("Providers")), 5_000))
            assertEquals("Keep the tablet draft", editor().text)
            description("Agents, side chats and terminals").click()
            text("Side chats")
            text("Terminals")
            capture("tablet-inspector")
            description("Close").click()
            assertTrue(device.wait(Until.gone(By.text("Terminals")), 5_000))
            assertEquals("Keep the tablet draft", editor().text)
        } finally {
            device.executeShellCommand("wm size reset")
            device.waitForIdle()
        }
    }

    @Test fun changingWindowSizeKeepsCanonicalDraftAndAttachments() {
        launch("chat:chat-deploy")
        description("More")
        val draft = composer()
        val payload = """{"kind":"skill","name":"review","path":"/tmp/navigation/SKILL.md"}"""
        val skill = "[\$review](zeron-invoke:${payload.toByteArray().joinToString("") { "%02x".format(it) }})"
        instrumentation.runOnMainSync {
            draft.setText("Use $skill")
            draft.images.add(StagedImage("window-test", "photo.jpg", byteArrayOf(1, 2), ImageBitmap(1, 1)))
        }
        val encoded = draft.encodedDraft()
        try {
            device.executeShellCommand("wm size 3000x2000")
            assertTrue(device.wait(Until.hasObject(By.desc("Session options")), 15_000))
            description("More")
            assertSame(draft, composer())
            instrumentation.runOnMainSync {
                assertEquals(encoded, draft.encodedDraft())
                assertEquals(1, draft.images.size)
            }
            device.executeShellCommand("wm size reset")
            assertTrue(device.wait(Until.gone(By.desc("Session options")), 15_000))
            description("More")
            assertSame(draft, composer())
            instrumentation.runOnMainSync {
                assertEquals(encoded, draft.encodedDraft())
                assertArrayEquals(byteArrayOf(1, 2), draft.images.single().bytes)
                draft.clear()
            }
        } finally {
            device.executeShellCommand("wm size reset")
            device.waitForIdle()
        }
    }

    private fun awaitPhoneHome() {
        // The underlying search bar appears before outgoing pages finish their transition.
        assertTrue(device.wait(Until.gone(By.desc("Close").pkg(context.packageName)), 5_000))
        assertTrue(device.wait(Until.gone(By.desc("Back").pkg(context.packageName)), 5_000))
        assertTrue(device.wait(Until.gone(By.clazz("android.widget.EditText").pkg(context.packageName)), 5_000))
        text("Search sessions")
        device.waitForIdle()
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }

    private fun composer(): ComposerModel {
        lateinit var result: ComposerModel
        instrumentation.runOnMainSync {
            val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single() as MainActivity
            result = ViewModelProvider(activity)[ComposerDrafts::class.java].forClient(model.client.value).getValue("chat-deploy")
        }
        return result
    }

    private fun dismissKeyboard() {
        instrumentation.runOnMainSync {
            val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
            WindowCompat.getInsetsController(activity.window, activity.window.decorView).hide(WindowInsetsCompat.Type.ime())
        }
        val deadline = SystemClock.uptimeMillis() + 5_000
        while (SystemClock.uptimeMillis() < deadline) {
            var visible = false
            instrumentation.runOnMainSync {
                val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
                visible = ViewCompat.getRootWindowInsets(activity.window.decorView)?.isVisible(WindowInsetsCompat.Type.ime()) == true
            }
            if (!visible) return
            SystemClock.sleep(50)
        }
        error("Keyboard did not close before navigating back")
    }

    private fun launch(route: String) {
        var previous: android.app.Activity? = null
        instrumentation.runOnMainSync {
            previous = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).firstOrNull()
        }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
        val deadline = SystemClock.uptimeMillis() + 15_000
        while (SystemClock.uptimeMillis() < deadline) {
            var ready = false
            instrumentation.runOnMainSync {
                ready = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                    .any { it !== previous && it.intent.getStringExtra("route") == route }
            }
            if (ready && model.isDemo && model.workspace.value != null) break
            SystemClock.sleep(100)
        }
        device.waitForIdle()
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }

    private fun description(value: String) = stable(if (value == "Settings") By.descStartsWith("Settings,") else By.desc(value), value)
    private fun text(value: String) = stable(By.text(value), value)

    private fun stable(selector: androidx.test.uiautomator.BySelector, label: String): androidx.test.uiautomator.UiObject2 {
        device.waitForIdle()
        // Compose and IME transitions may replace a node after accessibility first
        // reports it. Tap the current button only once its bounds have settled.
        var previous: android.graphics.Rect? = null
        var movedAt = SystemClock.uptimeMillis()
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
            val button = device.findObject(selector)
            val bounds = runCatching { button?.visibleBounds }.getOrNull()
            if (bounds == null || bounds != previous) {
                previous = bounds
                movedAt = SystemClock.uptimeMillis()
            } else if (SystemClock.uptimeMillis() - movedAt >= 300) {
                return requireNotNull(button)
            }
            SystemClock.sleep(50)
        }
        error("Missing or moving control: $label")
    }
    private fun editor() = requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 10_000))
    private fun scrollTo(value: String): androidx.test.uiautomator.UiObject2 {
        repeat(8) {
            if (device.hasObject(By.text(value))) return text(value)
            device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4, device.displayWidth / 2, device.displayHeight / 3, 30)
            device.waitForIdle()
        }
        return text(value)
    }
    private fun capture(name: String) {
        device.waitForIdle()
        val folder = File(context.cacheDir, "upstream-navigation").apply { mkdirs() }
        device.takeScreenshot(File(folder, "$name.png"))
    }
}
