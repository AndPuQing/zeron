package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import android.os.Build
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import android.view.accessibility.AccessibilityNodeInfo
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.Rule
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.io.File
import sh.zeron.android.ZeronApplication
import uniffi.zeron_core.CoreException

/** Real navigation and fork operations across Compose, UniFFI and the demo host. */
class SideChatFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @get:Rule val failureCapture = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            val folder = File(instrumentation.targetContext.cacheDir, "side-chat-failures").apply { mkdirs() }
            device.takeScreenshot(File(folder, "${description.methodName}.png"))
            device.dumpWindowHierarchy(File(folder, "${description.methodName}.xml"))
        }
    }

    @Test
    fun forkCopiesHistoryAndForkingAChildCreatesASibling() {
        val source = "chat-deploy"
        launch(source)
        val before = model.children(source).map { it.id }.toSet()
        forkFromMenu()
        await { model.children(source).any { it.id !in before } }
        val fork = model.children(source).single { it.id !in before }
        assertEquals(source, fork.parentChatId)
        assertEquals(model.row(source)?.deviceId, fork.deviceId)
        instrumentation.runOnMainSync { model.rename(fork.id, "Fork navigation test") }
        assertTrue(device.wait(Until.hasObject(By.text("Side chat")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.descStartsWith("Forked from ")), 5_000))
        device.waitForIdle()
        val siblingsBefore = model.children(source).map { it.id }.toSet()
        forkFromMenu()
        await { model.children(source).any { it.id !in siblingsBefore } }
        val sibling = model.children(source).single { it.id !in siblingsBefore }
        assertEquals(source, sibling.parentChatId)
        assertTrue(model.children(fork.id).isEmpty())
        assertTrue(device.wait(Until.hasObject(By.desc("Forked from Fork navigation test")), 5_000))
        // The upstream stack returns from the new sibling to the child that opened it.
        device.findObject(By.desc("Back")).click()
        assertTrue(device.wait(Until.gone(By.desc("Forked from Fork navigation test")), 5_000))
        assertTrue(device.wait(Until.hasObject(By.text("Side chat")), 5_000))
        device.findObject(By.desc("Back")).click()
        awaitMainChat()
        openCompanions()
        requireNotNull(device.wait(Until.findObject(By.text("Fork navigation test")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.text("Side chat")), 5_000))
    }

    @Test
    fun emptySideChatKeepsItsDraftAfterReturningToParent() {
        val source = "chat-veil"
        launch(source)
        val before = model.children(source).map { it.id }.toSet()
        device.findObject(By.desc("More")).click()
        requireNotNull(device.wait(Until.findObject(By.text("New side chat")), 5_000)).click()
        await { model.children(source).any { it.id !in before } }
        val empty = model.children(source).single { it.id !in before }
        assertFalse(model.client.value!!.openSession(empty.id).canFork())
        assertTrue(device.wait(Until.hasObject(By.text("Side chat")), 5_000))
        val editor = requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000))
        editor.text = "Unsent side-chat draft"
        device.findObject(By.desc("Back")).click()
        awaitMainChat()
        assertNotNull(model.row(empty.id))
        assertEquals("Unsent side-chat draft", model.drafts.load(empty.id))
        openCompanions()
        requireNotNull(device.wait(Until.findObject(By.text(if (empty.hasTitle) empty.title else "Side chat")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.text("Side chat")), 5_000))
        assertEquals("Unsent side-chat draft", requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).text)
    }

    private fun forkFromMenu() {
        requireNotNull(device.wait(Until.findObject(By.desc("More")), 5_000)).click()
        requireNotNull(device.wait(Until.findObject(By.text("Fork into side chat")), 5_000)).click()
    }

    private fun openCompanions() {
        // The header remains reachable while a toast covers the composer chips.
        requireNotNull(device.wait(Until.findObject(By.desc("Agents, side chats and terminals")), 5_000)).click()
        assertTrue(device.wait(Until.hasObject(By.text("Side chats")), 5_000))
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
        var previous: android.app.Activity? = null
        instrumentation.runOnMainSync {
            previous = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).firstOrNull()
        }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
        await {
            var ready = false
            instrumentation.runOnMainSync {
                ready = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                    .any { it !== previous && it.intent.getStringExtra("route") == route }
            }
            ready
        }
        device.waitForIdle()
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }

    private fun awaitMainChat() {
        device.waitForIdle()
        val visible = device.wait(Until.hasObject(By.desc("More")), 5_000)
        if (!visible) {
            val labels = device.findObjects(By.clazz("android.widget.TextView")).mapNotNull { runCatching { it.text }.getOrNull() }
            fail("Expected a chat after navigation; visible text: $labels")
        }
        assertTrue(device.wait(Until.gone(By.text("Side chat")), 5_000))
        device.waitForIdle()
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
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
