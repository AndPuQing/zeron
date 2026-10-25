package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.KeyModifiers
import uniffi.zeron_core.TerminalKey

/** The real navigation opens a host shell and returns to its retained composer. */
class CompanionFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @Test fun terminalRunsOnTheChatHostAndAppearsInCompanions() {
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", "chat:chat-deploy"))
        assertTrue(device.wait(Until.hasObject(By.desc("More")), 20_000))
        requireNotNull(device.findObject(By.desc("More"))).click()
        requireNotNull(device.wait(Until.findObject(By.text("Terminal")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.desc("Terminal")), 10_000))
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (model.terminals.tabs("chat-deploy").none { it.isLive } && SystemClock.uptimeMillis() < deadline) SystemClock.sleep(100)
        val tab = model.terminals.tabs("chat-deploy").first { it.isLive }
        try {
            assertEquals(model.row("chat-deploy")!!.deviceId, tab.ref.deviceId)
            tab.session.typeText("echo companion-regression", KeyModifiers(false, false, false))
            tab.session.press(TerminalKey.Enter, KeyModifiers(false, false, false))
            val outputDeadline = SystemClock.uptimeMillis() + 10_000
            while (!tab.session.text(false).contains("companion-regression") && SystemClock.uptimeMillis() < outputDeadline) SystemClock.sleep(100)
            assertTrue(tab.session.text(false).contains("companion-regression"))
            requireNotNull(device.findObject(By.desc("Back"))).click()
            requireNotNull(device.wait(Until.findObject(By.desc("Agents, side chats and terminals")), 5_000)).click()
            assertTrue(device.wait(Until.hasObject(By.text("Terminals")), 5_000))
            assertTrue(device.wait(Until.hasObject(By.text(tab.name)), 5_000))
        } finally { instrumentation.runOnMainSync { model.terminals.close(tab) } }
    }
}
