package sh.zeron.android.transcript

import android.content.Intent
import android.graphics.Color
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.FrameImage
import uniffi.zeron_core.Rgba
import uniffi.zeron_core.VisualPaint
import uniffi.zeron_core.VisualShape

/** Device tests: real Android pixels, accessibility and touches across the Kotlin/Rust boundary. */
class VisualFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)

    @Test
    fun gpuPixelsKeepTheirChannelsAndPremultipliedAlpha() {
        val image = VisualPainter.image(FrameImage(3u, 1u, byteArrayOf(
            0, 0, -1, -1, // opaque red
            -1, 0, 0, -1, // opaque blue
            0, 0, -128, -128, // premultiplied red, half alpha
        )))!!
        assertEquals(Color.RED, image.getPixel(0, 0))
        assertEquals(Color.BLUE, image.getPixel(1, 0))
        val translucent = image.getPixel(2, 0)
        assertEquals(128, Color.alpha(translucent))
        assertEquals(255, Color.red(translucent))
        assertEquals(0, Color.blue(translucent))
        assertNull(VisualPainter.image(FrameImage(2u, 1u, byteArrayOf(0, 0, 0, 0))))
    }

    @Test
    fun paintUsesLogicalCoordinatesAtDeviceDensity() {
        val paint = VisualPaint("", emptyList(), listOf(
            VisualShape.Rect(2f, 3f, 8f, 6f, 0f, Rgba(1f, 0f, 0f, 1f)),
            VisualShape.Path(listOf(12f, 3f, 18f, 3f, 18f, 9f, 12f, 9f), true,
                Rgba(0f, 0f, 1f, 1f), null, 1f, null),
        ))
        val image = VisualPainter.render(paint, 20f, 12f, 2f, StyleFonts())!!
        assertEquals(40, image.width)
        assertEquals(24, image.height)
        assertEquals(Color.RED, image.getPixel(10, 12))
        assertEquals(Color.BLUE, image.getPixel(30, 12))
        assertEquals(0, Color.alpha(image.getPixel(0, 0)))
    }

    @Test
    fun dashboardControlsDriveTheScript() {
        val context = instrumentation.targetContext
        val intent = context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", "chat:chat-visuals")
        context.startActivity(intent)
        requireNotNull(device.wait(Until.findObject(By.desc("Show table")), 20_000)) {
            "The visual must load from its host and expose native controls"
        }
        device.waitForIdle()
        val toggle = device.findObject(By.desc("Show table"))
        assertFalse(toggle.isChecked)
        // A zero-duration synthetic tap can be lost by a busy native visual.
        toggle.click(100)
        assertTrue(device.wait(Until.hasObject(By.desc("Show table").checked(true)), 5_000))
        device.findObject(By.desc("Show table")).click(100)
        assertTrue(device.wait(Until.hasObject(By.desc("Show table").checked(false)), 5_000))

        val slider = device.wait(Until.findObject(By.desc("Smoothing window")), 5_000)!!
        awaitRange(3f)
        val bounds = slider.visibleBounds
        device.click(bounds.left + (bounds.width() * 0.84).toInt(), bounds.centerY())
        awaitRange(6f)
        device.findObject(By.desc("Reset")).click(100)
        awaitRange(3f)

        val tab = device.wait(Until.findObject(By.desc("30d")), 5_000)!!
        tab.click(100)
        assertTrue(device.wait(Until.hasObject(By.desc("30d").selected(true)), 5_000))

        // A drag changes the slider without handing the gesture to the transcript.
        val track = device.findObject(By.desc("Smoothing window")).visibleBounds
        device.swipe(track.left + track.width() / 3, track.centerY(), track.right - 2, track.centerY(), 20)
        awaitRange(7f)

        // Reattaching after backgrounding retains the script's state.
        device.pressHome()
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        assertTrue(device.wait(Until.hasObject(By.desc("Smoothing window")), 5_000))
        awaitRange(7f)

        // A vertical drag on a slider scrolls rather than changing its value.
        val vertical = device.findObject(By.desc("Smoothing window")).visibleBounds
        device.swipe(vertical.centerX(), vertical.centerY(), vertical.centerX(), vertical.centerY() + 250, 20)
        awaitRange(7f)

        // Offscreen visuals detach their listener and resume the same script when revisited.
        // Scroll to visibility rather than assuming a fixed number of swipes for every layout.
        for (attempt in 0 until 8) {
            device.swipe(30, 450, 30, device.displayHeight - 450, 20)
            if (device.wait(Until.gone(By.desc("Smoothing window")), 500)) break
        }
        assertFalse(device.hasObject(By.desc("Smoothing window")))
        for (attempt in 0 until 8) {
            device.swipe(30, device.displayHeight - 450, 30, 450, 20)
            if (device.wait(Until.hasObject(By.desc("Smoothing window")), 500)) break
        }
        assertTrue(device.wait(Until.hasObject(By.desc("Smoothing window")), 5_000))
        awaitRange(7f)
    }

    private fun awaitRange(value: Float) {
        val deadline = SystemClock.uptimeMillis() + 5_000
        do {
            val root = instrumentation.uiAutomation.rootInActiveWindow
            if (root != null && range(root) == value) return
            SystemClock.sleep(50)
        } while (SystemClock.uptimeMillis() < deadline)
        fail("Expected slider value $value")
    }

    private fun range(node: AccessibilityNodeInfo): Float? {
        if (node.contentDescription?.toString() == "Smoothing window") return node.rangeInfo?.current
        for (i in 0 until node.childCount) node.getChild(i)?.let { child -> range(child)?.let { return it } }
        return null
    }
}
