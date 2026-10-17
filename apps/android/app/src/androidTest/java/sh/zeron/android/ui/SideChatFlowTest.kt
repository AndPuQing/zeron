package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.CoreException

/** Real navigation and fork operations across Compose, UniFFI and the demo host. */
class SideChatFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @Test
    fun forkCopiesHistoryAndForkingAChildCreatesASibling() {
        val source = "chat-deploy"
        launch(source)
        val before = model.children(source).map { it.id }.toSet()
        device.findObject(By.desc("More")).click()
        requireNotNull(device.wait(Until.findObject(By.text("Fork to side chat")), 5_000)).click()
        await { model.children(source).any { it.id !in before } }
        val fork = model.children(source).single { it.id !in before }
        assertEquals(source, fork.parentChatId)
        assertEquals(model.row(source)?.deviceId, fork.deviceId)
        instrumentation.runOnMainSync { model.rename(fork.id, "Fork navigation test") }
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Side chat of ")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.descStartsWith("Forked from ")), 5_000))
        device.waitForIdle()

        // The seam returns to the original page rather than launching another app.
        val seam = findDescription(instrumentation.uiAutomation.rootInActiveWindow, "Forked from ")!!
        assertTrue(seam.performAction(AccessibilityNodeInfo.ACTION_CLICK))
        awaitMainChat()

        device.findObject(By.desc("Side chats")).click()
        assertTrue(device.wait(Until.hasObject(By.text("Side chats")), 5_000))
        requireNotNull(device.wait(Until.findObject(By.text("Fork navigation test")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Side chat of ")), 5_000))
        assertTrue(device.wait(Until.gone(By.text("Side chats")), 5_000))
        device.waitForIdle()
        val siblingsBefore = model.children(source).map { it.id }.toSet()
        device.findObject(By.desc("More")).click()
        requireNotNull(device.wait(Until.findObject(By.text("Fork to side chat")), 5_000)).click()
        await { model.children(source).any { it.id !in siblingsBefore } }
        val sibling = model.children(source).single { it.id !in siblingsBefore }
        assertEquals(source, sibling.parentChatId)
        assertTrue(model.children(fork.id).isEmpty())
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Side chat of ")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.text(sibling.title)), 5_000))
        assertTrue(device.wait(Until.hasObject(By.desc("Forked from Fork navigation test")), 5_000))
        device.waitForIdle()

        // Switching siblings replaces the child page: one Back returns to the parent.
        device.pressBack()
        awaitMainChat()
        instrumentation.runOnMainSync { model.rename(sibling.id, "Sibling navigation test") }
        device.findObject(By.desc("Side chats")).click()
        requireNotNull(device.wait(Until.findObject(By.text("Sibling navigation test")), 5_000)).click()
        assertTrue(device.wait(Until.gone(By.text("Side chats")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Side chat of ")), 5_000))
        device.waitForIdle()
        device.findObject(By.desc("Back")).click()
        awaitMainChat()
    }

    @Test
    fun emptySideChatDisablesForkAndReturnsToParent() {
        val source = "chat-veil"
        launch(source)
        val before = model.children(source).map { it.id }.toSet()
        device.findObject(By.desc("Side chats")).click()
        requireNotNull(device.wait(Until.findObject(By.desc("New side chat")), 5_000)).click()
        await { model.children(source).any { it.id !in before } }
        val empty = model.children(source).single { it.id !in before }
        assertEquals(source, empty.parentChatId)
        assertFalse(model.client.value!!.openSession(empty.id).canFork())
        assertTrue(device.wait(Until.hasObject(By.textStartsWith("Side chat of ")), 5_000))

        device.findObject(By.desc("More")).click()
        assertTrue(device.wait(Until.hasObject(By.text("Needs a completed reply")), 5_000))
        assertFalse(device.hasObject(By.text("Pin")))
        assertFalse(device.hasObject(By.text("New side chat")))
        val disabled = device.findObject(By.text("Fork to side chat")).visibleBounds
        device.click(disabled.centerX(), disabled.centerY())
        device.waitForIdle()
        assertTrue(device.hasObject(By.text("Needs a completed reply")))
        assertEquals(before.size + 1, model.children(source).size)
        device.pressBack()
        requireNotNull(device.wait(Until.findObject(By.textStartsWith("Side chat of ")), 5_000)).click()
        awaitMainChat()
    }

    @Test
    fun searchFindsExistingChildren() {
        startRoute("search")
        requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).text = "timing"
        assertTrue(model.isDemo)
        assertTrue(device.wait(Until.hasObject(By.text("Side chat: veil timing")), 5_000))
        assertTrue(device.hasObject(By.textStartsWith("Side chat of ")))
    }

    @Test
    fun failedForkReleasesTheBusyGuard() {
        launch("chat-deploy")
        runBlocking {
            try {
                model.forkSideChat("missing-chat")
                fail("Unknown sources must fail")
            } catch (_: CoreException.NotFound) {
                assertFalse(model.sideChatBusy.value)
            }
            val id = model.forkSideChat("chat-deploy")
            assertEquals("chat-deploy", model.row(id)?.parentChatId)
            assertFalse(model.sideChatBusy.value)
        }
    }

    private fun launch(chatId: String) {
        startRoute("chat:$chatId")
        assertTrue(device.wait(Until.hasObject(By.desc("More")), 20_000))
        assertTrue("UI tests must use the offline demo workspace", model.isDemo)
    }

    private fun startRoute(route: String) {
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
    }

    private fun awaitMainChat() {
        device.waitForIdle()
        val visible = device.wait(Until.hasObject(By.desc("More")), 5_000)
        if (!visible) {
            val labels = device.findObjects(By.clazz("android.widget.TextView")).mapNotNull { runCatching { it.text }.getOrNull() }
            fail("Expected a chat after navigation; visible text: $labels")
        }
        assertTrue(device.wait(Until.gone(By.textStartsWith("Side chat of ")), 5_000))
    }

    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 10_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(50)
        }
        fail("Timed out waiting for the side chat registry")
    }

    private fun findDescription(node: AccessibilityNodeInfo?, prefix: String): AccessibilityNodeInfo? {
        node ?: return null
        if (node.contentDescription?.startsWith(prefix) == true) return node
        for (i in 0 until node.childCount) findDescription(node.getChild(i), prefix)?.let { return it }
        return null
    }
}
