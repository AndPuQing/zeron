package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.After
import org.junit.Test
import java.io.File
import sh.zeron.android.ZeronApplication

/** Real model picker, configuration writes and screenshots against the isolated demo. */
class ExecutionPolicyFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val context get() = instrumentation.targetContext
    private val model get() = (context.applicationContext as ZeronApplication).model

    @Test fun executionControlsPersistAndRender() {
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", "chat:chat-deploy")
            .putExtra("companion", "model-picker"))
        // Use the existing companion entry point to open the real settings sheet.
        // A coordinate click during IME/composer movement can miss the model chip;
        // the toolbar's duplicate permission label then lets the test run on the
        // wrong surface, where Plan is horizontally off-screen on small devices.
        requireNotNull(device.wait(Until.findObject(By.desc("Close")), 15_000)) {
            "Model settings sheet did not open"
        }
        text("Model")
        text("Full access")
        capture("execution-settings")
        text("Full access").click()
        text("Supervised")
        capture("execution-permissions")
        text("Supervised").click()
        val core = model.client.value!!
        await { core.sessionConfig("chat-deploy")?.accessMode == "supervised" }
        text("Plan off").click()
        text("On")
        capture("execution-plan")
        text("On").click()
        await { core.sessionConfig("chat-deploy")?.plan == true }
        assertEquals("supervised", core.sessionConfig("chat-deploy")?.accessMode)
        text("Plan on")
        capture("execution-selected")
        requireNotNull(device.wait(Until.findObject(By.desc("Close")), 5_000)).click()
        await { !device.hasObject(By.desc("Close")) }
        assertTrue(core.sessionConfig("chat-deploy")!!.plan)
        assertEquals("supervised", core.sessionConfig("chat-deploy")!!.accessMode)

        // Also cover the ordinary toolbar entry point. Opening the sheet takes
        // focus away from this empty editor; that must not dispose the sheet.
        requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).click()
        val label = model.row("chat-deploy")!!.modelLabel!!
        var lastBounds: android.graphics.Rect? = null
        var stableSince = SystemClock.uptimeMillis()
        await {
            val bounds = device.findObject(By.text(label))?.visibleBounds ?: return@await false
            if (lastBounds != bounds) { lastBounds = bounds; stableSince = SystemClock.uptimeMillis() }
            SystemClock.uptimeMillis() - stableSince > 500
        }
        text(label).click()
        requireNotNull(device.wait(Until.findObject(By.desc("Close")), 5_000)) {
            "Model settings sheet must survive the editor losing focus"
        }
        text("Model")
        text("Supervised")
        text("Plan on")
        capture("execution-reopened")
    }

    @After fun recordFinalUi() {
        val folder = File(context.cacheDir, "execution-screenshots").apply { mkdirs() }
        device.takeScreenshot(File(folder, "execution-final.png"))
        device.dumpWindowHierarchy(File(folder, "execution-final.xml"))
    }

    private fun text(value: String) = requireNotNull(device.wait(Until.findObject(By.text(value)), 10_000)) { "Missing text: $value" }
    private fun await(predicate: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (!predicate()) {
            check(SystemClock.uptimeMillis() < deadline) { "Timed out waiting for UI/configuration" }
            SystemClock.sleep(100)
        }
    }
    private fun capture(name: String) {
        device.waitForIdle()
        val folder = File(context.cacheDir, "execution-screenshots").apply { mkdirs() }
        assertTrue(device.takeScreenshot(File(folder, "$name.png")))
    }
}
