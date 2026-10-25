package sh.zeron.android.ui

import android.content.Intent
import android.graphics.Rect
import android.os.SystemClock
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import androidx.lifecycle.ViewModelProvider
import androidx.compose.ui.graphics.ImageBitmap
import kotlinx.coroutines.runBlocking
import org.junit.Assert.*
import org.junit.Test
import org.junit.Rule
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import java.io.File
import sh.zeron.android.ZeronApplication

/** Exercise Android UI against the shared native demo, without opening a real authorization page. */
class ProviderParityFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model
    private val core get() = model.client.value!!

    @get:Rule val failureCapture = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            val folder = File(instrumentation.targetContext.cacheDir, "upstream-flow-failures").apply { mkdirs() }
            device.takeScreenshot(File(folder, "${description.methodName}.png"))
            device.dumpWindowHierarchy(File(folder, "${description.methodName}.xml"))
        }
    }

    @Test fun providersShowAccountsAndCanSwitchTheActiveAccount() {
        launch("settings")
        text("Providers").click()
        val computer = model.workspace.value!!.devices.first { it.isExecutionHost && it.online }.id
        val original = runBlocking { core.deviceAgentAccounts(computer) }.first { it.harness == "claude-code" && it.active }
        val other = runBlocking { core.deviceAgentAccounts(computer) }.first { it.harness == "claude-code" && !it.active && it.switchable }
        try {
            text("Claude Code").click()
            text(original.email ?: original.displayName ?: "Account")
            text(other.email ?: other.displayName ?: "Account").click()
            await { runBlocking { core.deviceAgentAccounts(computer) }.any { it.id == other.id && it.active } }
            text(original.email ?: original.displayName ?: "Account").longClick()
            text("Remove account").click()
            text("Remove account?")
            text("Cancel").click()
            assertEquals(2, runBlocking { core.deviceAgentAccounts(computer) }.count { it.harness == "claude-code" })
        } finally { runBlocking { core.deviceActivateAccount(computer, "claude-code", original.id) } }
    }

    @Test fun chatCanSwitchAgentAndClearsProviderSpecificSettings() {
        launch("chat:chat-deploy")
        val original = core.sessionConfig("chat-deploy")!!
        try {
            await { canSwitchAgent(core.openSession("chat-deploy").composer()) }
            requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).click()
            expandedModelChip(model.row("chat-deploy")!!.modelLabel!!).click()
            stableProviderTab("Codex").click()
            val host = core.openSession("chat-deploy").composer().host.deviceId
            val picked = runBlocking { core.listModels(host, "codex") }.first()
            assertEquals("Browsing an agent's models must not change the chat", original, core.sessionConfig("chat-deploy"))
            text(picked.label).click()
            await { core.sessionConfig("chat-deploy")?.harness == "codex" }
            val config = core.sessionConfig("chat-deploy")!!
            assertEquals(picked.id, config.model)
            assertNull(config.reasoning)
            assertTrue(config.modelOptions.isEmpty())
        } finally { core.setSessionConfig("chat-deploy", original) }
    }

    @Test fun deleteRequiresConfirmationAndCancelKeepsTheChat() {
        launch("chat:chat-deploy")
        requireNotNull(device.wait(Until.findObject(By.desc("More")), 5_000)).click()
        text("Delete").click()
        text("Delete session?")
        text("Cancel").click()
        assertTrue(device.wait(Until.gone(By.text("Delete session?")), 5_000))
        assertNotNull(model.row("chat-deploy"))
    }

    @Test fun recreationKeepsTheRouteTextAndCanonicalSkill() {
        launch("chat:chat-deploy")
        val input = requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 10_000))
        input.click()
        device.waitForIdle()
        input.text = "\$imag"
        requireNotNull(device.wait(Until.findObject(By.descStartsWith("Imagegen")), 10_000)).click()
        await { device.findObject(By.clazz("android.widget.EditText"))?.text?.startsWith("\$imagegen") == true }
        val before = requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)).text
        lateinit var original: sh.zeron.android.MainActivity
        lateinit var draft: ComposerModel
        var encoded = ""
        instrumentation.runOnMainSync {
            original = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single() as sh.zeron.android.MainActivity
            draft = ViewModelProvider(original)[ComposerDrafts::class.java].forClient(core).getValue("chat-deploy")
            encoded = draft.encodedDraft()
            assertTrue(encoded.contains("zeron-invoke:"))
            draft.images.add(StagedImage("rotation-test", "photo.jpg", byteArrayOf(1, 2), ImageBitmap(1, 1)))
            original.recreate()
        }
        await {
            var ready = false
            instrumentation.runOnMainSync {
                ready = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).any { it !== original }
            }
            ready
        }
        val after = requireNotNull(device.wait(Until.findObject(By.clazz("android.widget.EditText")), 10_000))
        assertEquals(before, after.text)
        assertTrue(after.text.startsWith("\$imagegen"))
        requireNotNull(device.wait(Until.findObject(By.desc("More")), 5_000))
        instrumentation.runOnMainSync {
            val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single() as sh.zeron.android.MainActivity
            val restored = ViewModelProvider(activity)[ComposerDrafts::class.java].forClient(core).getValue("chat-deploy")
            assertSame(draft, restored)
            assertEquals(encoded, restored.encodedDraft())
            assertArrayEquals(byteArrayOf(1, 2), restored.images.single().bytes)
            restored.clear()
        }
    }

    @Test fun confirmingDeleteRemovesOnlyTheTemporaryDemoChat() {
        launch("chat:chat-deploy")
        val id = runBlocking { model.createSideChat("chat-deploy") }
        core.renameSession(id, "Temporary deletion test")
        launch("chat:$id")
        requireNotNull(device.wait(Until.findObject(By.desc("More")), 5_000)).click()
        text("Delete").click()
        text("Delete session?")
        text("Delete").click()
        await { model.row(id) == null }
        assertNotNull(model.row("chat-deploy"))
    }

    private fun stableProviderTab(label: String): androidx.test.uiautomator.UiObject2 {
        var previous: Rect? = null
        var movedAt = SystemClock.uptimeMillis()
        lateinit var result: androidx.test.uiautomator.UiObject2
        await {
            var imeVisible = false
            instrumentation.runOnMainSync {
                val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
                imeVisible = ViewCompat.getRootWindowInsets(activity.window.decorView)?.isVisible(WindowInsetsCompat.Type.ime()) == true
            }
            if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
            val tab = device.findObject(By.desc(label)) ?: return@await false
            val bounds = tab.visibleBounds
            if (bounds != previous) { previous = bounds; movedAt = SystemClock.uptimeMillis() }
            if (imeVisible || SystemClock.uptimeMillis() - movedAt < 300) return@await false
            result = tab
            true
        }
        return result
    }

    private fun expandedModelChip(label: String): androidx.test.uiautomator.UiObject2 {
        var previous: Rect? = null
        var movedAt = SystemClock.uptimeMillis()
        lateinit var result: androidx.test.uiautomator.UiObject2
        // Accessibility can become idle before the IME starts animating. Require the chip
        // to be above the keyboard and stationary, then use its current screen bounds.
        await {
            var imeBottom = 0
            instrumentation.runOnMainSync {
                val activity = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single()
                imeBottom = ViewCompat.getRootWindowInsets(activity.window.decorView)?.getInsets(WindowInsetsCompat.Type.ime())?.bottom ?: 0
            }
            if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
            val chip = device.findObject(By.text(label)) ?: return@await false
            val bounds = chip.visibleBounds
            if (bounds != previous) { previous = bounds; movedAt = SystemClock.uptimeMillis() }
            if (imeBottom == 0 || bounds.bottom >= device.displayHeight - imeBottom || SystemClock.uptimeMillis() - movedAt < 300) return@await false
            result = chip
            true
        }
        return result
    }

    private fun text(value: String): androidx.test.uiautomator.UiObject2 {
        return device.wait(Until.findObject(By.text(value)), 10_000) ?: run {
            device.executeShellCommand("screencap -p /sdcard/parity-model-failure.png")
            val tree = java.io.ByteArrayOutputStream()
            device.dumpWindowHierarchy(tree)
            tree.toString("UTF-8").chunked(3000).forEach { android.util.Log.e("ParityHierarchy", it) }
            error("Missing text: $value")
        }
    }
    private fun launch(route: String) {
        var previous: android.app.Activity? = null
        instrumentation.runOnMainSync {
            previous = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).firstOrNull()
        }
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
        assertTrue(device.wait(Until.hasObject(By.pkg(context.packageName)), 20_000))
        await { model.isDemo && model.workspace.value != null }
        await {
            var ready = false
            instrumentation.runOnMainSync {
                ready = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).any { it !== previous && it.intent.getStringExtra("route") == route }
            }
            ready
        }
        device.waitForIdle()
        if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
        if (route.startsWith("chat:")) {
            val row = core.sessionRow(route.removePrefix("chat:"))!!
            text(if (row.parentChatId != null) "Side chat" else row.title)
        }
    }
    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 15_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(100)
        }
        fail("Timed out waiting for provider state")
    }
}
