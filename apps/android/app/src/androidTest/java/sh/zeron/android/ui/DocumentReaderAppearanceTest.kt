package sh.zeron.android.ui

import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Color
import android.os.SystemClock
import androidx.activity.compose.setContent
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.MainActivity
import sh.zeron.android.ZeronApplication
import sh.zeron.android.design.Appearance
import sh.zeron.android.design.ThemeMode
import sh.zeron.android.design.ZeronTheme
import uniffi.zeron_core.DocumentPreview
import java.io.File

/** Native UI captures with deterministic document contents; no host or credentials are used. */
class DocumentReaderAppearanceTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val context get() = instrumentation.targetContext

    @Test fun readerModesDetailsAndStates() {
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", "chat:chat-deploy"))
        val model = (context.applicationContext as ZeronApplication).model
        var activity: MainActivity? = null
        await {
            activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).filterIsInstance<MainActivity>().firstOrNull()
            activity != null && model.client.value != null
        }
        val path = "/workspace/docs/getting-started-with-remote-documents.md"
        val text = """
            # A calmer place to read

            Open documents from your computer, right here on your phone.

            ## Keep your context

            - **Preview** for reading, **Source** for the original text.
            - Follow links to related documents.
            - Go back without losing your place.

            > Your files stay on their host. This view is read-only.

            ## A small example

            ```kotlin
            val document = "guide.md"
            println(document)
            ```

            [Continue reading](next.md)
        """.trimIndent()
        val doc = DocumentPreview(path, text, true, false, "host", "/workspace")
        val entry = DocumentEntry(path).also { it.document = doc }
        var theme by mutableStateOf(ThemeMode.Light)
        instrumentation.runOnMainSync {
            activity!!.setContent {
                ZeronTheme(Appearance(theme)) {
                    Dialog(onDismissRequest = {}, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
                        Surface(color = MaterialTheme.colorScheme.surfaceContainerLowest) {
                            DocumentContent(model.client.value!!, "chat-deploy", entry, {}, {})
                        }
                    }
                }
            }
        }
        try {
            await { (entry.transcript?.frame?.rowCount() ?: 0u) > 0u }
            awaitDocumentPaint(dark = false)
            capture("document-android-preview-light")
            requireNotNull(device.wait(Until.findObject(By.desc("Document options")), 5_000)).click()
            requireNotNull(device.wait(Until.findObject(By.text("File details")), 5_000)).click()
            assertTrue(device.wait(Until.hasObject(By.text(path)), 5_000))
            capture("document-android-details")
            requireNotNull(device.wait(Until.findObject(By.text("Done")), 5_000)).click()
            var revision = 0uL
            instrumentation.runOnMainSync { revision = entry.transcript!!.frame!!.revision(); theme = ThemeMode.Dark }
            requireNotNull(device.wait(Until.findObject(By.text("Source")), 5_000)).click()
            await { entry.source && entry.transcript!!.frame!!.revision() > revision }
            awaitDocumentPaint(dark = true)
            capture("document-android-source-dark")
            instrumentation.runOnMainSync {
                theme = ThemeMode.Light
                entry.document = null
                entry.error = "The host is offline. Reconnect it and try again."
            }
            assertTrue(device.wait(Until.hasObject(By.text("Couldn't open document")), 5_000))
            capture("document-android-error")
            instrumentation.runOnMainSync { entry.error = null; entry.document = doc.copy(text = "") }
            assertTrue(device.wait(Until.hasObject(By.text("This document is empty")), 5_000))
            capture("document-android-empty")
        } finally {
            instrumentation.runOnMainSync { activity!!.setContent {}; entry.close() }
        }
    }

    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 15_000
        var ready = false
        while (!ready && SystemClock.uptimeMillis() < deadline) {
            instrumentation.runOnMainSync { ready = condition() }
            if (!ready) SystemClock.sleep(50)
        }
        assertTrue("Reader UI did not settle", ready)
    }
    private fun capture(name: String) {
        device.waitForIdle()
        val folder = File(context.cacheDir, "execution-screenshots").apply { mkdirs() }
        assertTrue(device.takeScreenshot(File(folder, "$name.png")))
    }

    /** Layout arrives before the canvas is painted (and before its reveal animation). */
    private fun awaitDocumentPaint(dark: Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (SystemClock.uptimeMillis() < deadline) {
            val raw = instrumentation.uiAutomation.takeScreenshot()
            if (raw != null) {
                val bitmap = raw.copy(Bitmap.Config.ARGB_8888, false)
                raw.recycle()
                var ink = 0
                // Only the reading area: the toolbar must not make an empty page pass.
                for (y in bitmap.height * 3 / 10 until bitmap.height * 4 / 5 step 3) {
                    for (x in bitmap.width / 10 until bitmap.width * 9 / 10 step 3) {
                        val pixel = bitmap.getPixel(x, y)
                        val r = Color.red(pixel); val g = Color.green(pixel); val b = Color.blue(pixel)
                        if (if (dark) r > 160 && g > 160 && b > 160 else r < 160 && g < 160 && b < 160) ink++
                    }
                }
                bitmap.recycle()
                if (ink > 50) return
            }
            SystemClock.sleep(100)
        }
        fail("The document canvas stayed blank")
    }
}
