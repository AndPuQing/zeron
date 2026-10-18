package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
import android.os.Build
import java.io.File
import org.junit.Rule
import org.junit.rules.TestWatcher
import org.junit.runner.Description
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.UiObject2
import androidx.test.uiautomator.Until
import org.junit.Assert.*
import org.junit.Test
import sh.zeron.android.ZeronApplication

/** Exercises the native chooser, UniFFI controller and DemoHost publication. */
class SessionImportFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @get:Rule
    val captureFailure = object : TestWatcher() {
        override fun failed(error: Throwable, description: Description) {
            val directory = instrumentation.targetContext.getExternalFilesDir(null) ?: return
            device.takeScreenshot(File(directory, "${description.methodName}.png"))
            device.dumpWindowHierarchy(File(directory, "${description.methodName}.xml"))
        }
    }

    @Test
    fun entrySelectionFiltersAndImportSurviveRecreation() {
        launchFresh("home")
        find("Import").click()
        find("Import sessions")
        find("Improve import navigation").click()
        find("Import 1 selected")
        assertFalse(device.hasObject(By.text("Already in Sessions")))
        find("All agents").click()
        find("Claude Code").click()
        await { !device.hasObject(By.text("Improve import navigation")) }
        find("Import 1 selected") // Hidden selections stay selected.
        find("Claude Code").click()
        find("All agents").click()
        try {
            device.setOrientationLeft()
            find("Import 1 selected")
        } finally {
            device.setOrientationNatural()
        }
        find("Import 1 selected").click()
        await { rows().any { it.title == "Improve import navigation" } }
        find("Import results · 1 of 1 imported")
        find("Refresh").click()
        find("Not imported")
        await { !device.hasObject(By.text("Improve import navigation")) }
        find("Close").click()
        await { device.hasObject(By.text("Sessions")) }
        find("Archived").click()
        find("Improve import navigation")
        assertTrue(rows().any { it.title == "Improve import navigation" && it.archived })
    }

    @Test
    fun rapidSearchInputKeepsEveryCharacterAndFiltersSessions() {
        launchFresh("import")
        val search = device.wait(Until.findObject(By.clazz("android.widget.EditText")), 5_000)
            ?: error("Missing import search")
        search.click()
        instrumentation.sendStringSync("deployment")
        await {
            device.findObject(By.clazz("android.widget.EditText"))?.text == "deployment" &&
                device.hasObject(By.text("Review the deployment workflow")) &&
                !device.hasObject(By.text("Improve import navigation"))
        }
        device.pressBack()
        find("Review the deployment workflow").click()
        find("Import 1 selected")
    }

    @Test
    fun paginatedSelectionKeepsSuccessesAndRetriesTheRemainingSession() {
        launchFresh("import")
        find("Select visible").click()
        find("Load more sessions").click()
        find("Polish the mobile layout").click()
        find("Import 3 selected").click()
        await { rows().count { it.title == "Improve import navigation" || it.title == "Review the deployment workflow" } == 2 }
        find("Import results · 2 of 3 imported")
        find("Retry remaining").click()
        await { rows().any { it.title == "Polish the mobile layout" } }
        find("Import results · 3 of 3 imported")
        assertEquals(1, rows().count { it.title == "Improve import navigation" })
        assertEquals(1, rows().count { it.title == "Review the deployment workflow" })
        assertEquals(1, rows().count { it.title == "Polish the mobile layout" })
        find("Close").click()
        find("Import").click()
        find("Import sessions")
        assertFalse(device.hasObject(By.text("Improve import navigation")))
        assertFalse(device.hasObject(By.text("Review the deployment workflow")))
        find("Load more sessions").click()
        assertTrue(device.wait(Until.gone(By.text("Load more sessions")), 5_000))
        assertFalse(device.hasObject(By.text("Polish the mobile layout")))
    }

    @Test
    fun newHarnessDropdownFiltersImportIntoArchivedAndDisappearOnRescan() {
        launchFresh("import")
        find("Load more sessions").click()
        val fixtures = listOf(
            "DeepSeek Harness" to "Review the import pipeline",
            "GitHub Copilot" to "Preserve the Copilot workspace",
            "Cursor" to "Continue the SDK checkpoint",
            "Grok" to "Restore the Grok conversation",
            "Hermes" to "Resume the Hermes ACP session",
            "Pi" to "Trace the session tree",
            "OpenCode" to "Verify native session forks",
        )
        var current = "All agents"
        for ((agent, title) in fixtures) {
            find(current).click()
            find(agent).click()
            find(title).click()
            current = agent
        }
        find("Import ${fixtures.size} selected").click()
        await { fixtures.all { (_, title) -> rows().any { it.title == title && it.archived } } }
        find("Import results · ${fixtures.size} of ${fixtures.size} imported")
        find("Refresh").click()
        find("Load more sessions").click()
        await { !device.hasObject(By.text("Load more sessions")) }
        assertFalse(device.hasObject(By.text("Verify native session forks")))
    }

    private fun rows() = model.workspace.value?.let { ws ->
        ws.front.pinned + ws.front.sections.flatMap { it.sessions } + ws.front.recent + ws.archived
    }.orEmpty()

    private fun launchFresh(route: String) {
        val context = instrumentation.targetContext
        // A fresh host per test makes partial failure/retry deterministic.
        if (model.client.value != null) instrumentation.runOnMainSync {
            model.client.value?.shutdown()
            model.startDemo()
        }
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("route", route))
        await { model.isDemo && model.workspace.value != null }
        find(if (route == "home") "Sessions" else "Import sessions")
    }

    private fun find(text: String): UiObject2 {
        repeat(8) {
            clearCache()
            device.wait(Until.findObject(By.text(text)), 750)?.let { return it }
            device.findObject(By.desc(text))?.let { return it }
            device.swipe(device.displayWidth / 2, device.displayHeight * 3 / 4,
                device.displayWidth / 2, device.displayHeight / 3, 25)
        }
        // Results can put the controls above the current viewport.
        repeat(8) {
            clearCache()
            device.swipe(device.displayWidth / 2, device.displayHeight / 3,
                device.displayWidth / 2, device.displayHeight * 3 / 4, 25)
            device.wait(Until.findObject(By.text(text)), 500)?.let { return it }
            device.findObject(By.desc(text))?.let { return it }
        }
        error("No visible text: $text")
    }
    private fun clearCache() {
        if (Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }
    private fun await(predicate: () -> Boolean) {
        val deadline = SystemClock.elapsedRealtime() + 15_000
        while (true) {
            clearCache()
            if (predicate()) break
            check(SystemClock.elapsedRealtime() < deadline) { "Timed out waiting for import state" }
            SystemClock.sleep(50)
        }
        device.waitForIdle()
    }
}
