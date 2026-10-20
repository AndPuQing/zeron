package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

/** Both composer entries share the desktop-style command menu and canonical insertion. */
class CommandMenuFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)

    @Test
    fun chatAndNewSessionMenusAlignAndInsertTheSameSkill() {
        launch("chat:chat-deploy")
        val chatEditor = editor()
        chatEditor.click()
        chatEditor.text = "/"
        val first = requireRow("Imagegen.")
        device.waitForIdle()

        val density = instrumentation.targetContext.resources.displayMetrics.density
        assertTrue("one-line touch row", first.visibleBounds.height() >= (48 * density).toInt())
        val firstDescription = first.contentDescription ?: ""
        assertFalse(firstDescription.contains("SKILL.md"))
        assertFalse(firstDescription.contains("/Users/"))
        assertInsetMenu(first)
        assertAboveKeyboardAndInput(first, chatEditor)
        val chatWidth = first.visibleBounds.width()
        val chatLeft = first.visibleBounds.left
        val chatRight = first.visibleBounds.right

        chatEditor.text = "/imag"
        assertTrue(device.wait(Until.gone(By.descStartsWith("Openai Docs")), 5_000))
        val filtered = requireRow("Imagegen.")
        filtered.click()
        assertTrue(chatEditor.text.startsWith("\$imagegen"))

        chatEditor.text = "/compact"
        val compact = requireRow("/compact")
        assertTrue((compact.contentDescription ?: "").startsWith("/compact"))
        compact.click()
        assertTrue(chatEditor.text.startsWith("/compact"))

        launch("new")
        val newEditor = editor()
        newEditor.click()
        newEditor.text = "/"
        val newRow = requireRow("Imagegen.")
        device.waitForIdle()
        assertInsetMenu(newRow)
        assertTrue("new-session menu keeps the same inset width",
            abs(chatWidth - newRow.visibleBounds.width()) <= 2)
        assertEquals("new-session menu left edge", chatLeft, newRow.visibleBounds.left)
        assertEquals("new-session menu right edge", chatRight, newRow.visibleBounds.right)
        assertAboveKeyboardAndInput(newRow, newEditor)
    }

    private fun requireRow(descriptionPrefix: String): UiObject2 =
        requireNotNull(device.wait(Until.findObject(By.descStartsWith(descriptionPrefix)), 10_000)) {
            "No completion row starting with $descriptionPrefix"
        }

    private fun assertInsetMenu(row: UiObject2) {
        val inset = (12 * instrumentation.targetContext.resources.displayMetrics.density).toInt() - 1
        assertTrue("menu is not full width", row.visibleBounds.left >= inset)
        assertTrue("menu is not full width", row.visibleBounds.right <= device.displayWidth - inset)
    }

    private fun assertAboveKeyboardAndInput(row: UiObject2, editor: UiObject2) {
        assertTrue("menu stays above the composer", row.visibleBounds.bottom <= editor.visibleBounds.top)
        val ime = awaitIme()
        assertTrue("the software keyboard must be visible", ime > 0)
        assertTrue("menu stays above the keyboard", row.visibleBounds.bottom <= device.displayHeight - ime)
    }

    /** Fails instead of treating a hidden or floating-toolbar-only IME as success. */
    private fun awaitIme(): Int {
        val deadline = SystemClock.uptimeMillis() + 8_000
        while (SystemClock.uptimeMillis() < deadline) {
            var bottom = 0
            instrumentation.runOnMainSync {
                val activity = ActivityLifecycleMonitorRegistry.getInstance()
                    .getActivitiesInStage(Stage.RESUMED).single()
                val insets = ViewCompat.getRootWindowInsets(activity.window.decorView)
                if (insets?.isVisible(WindowInsetsCompat.Type.ime()) == true) {
                    bottom = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
                }
            }
            if (bottom > 0) return bottom
            SystemClock.sleep(100)
        }
        error("Software keyboard did not become visible")
    }

    private fun editor(): UiObject2 =
        requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 10_000))

    private fun launch(route: String) {
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
        assertTrue(device.wait(Until.hasObject(By.pkg(context.packageName)), 20_000))
        val deadline = SystemClock.uptimeMillis() + 15_000
        while (!device.hasObject(By.clazz("android.widget.EditText"))) {
            if (SystemClock.uptimeMillis() > deadline) error("Composer did not appear")
            SystemClock.sleep(50)
        }
        device.waitForIdle()
    }
}
