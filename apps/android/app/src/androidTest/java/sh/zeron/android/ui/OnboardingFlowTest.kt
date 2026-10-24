package sh.zeron.android.ui

import android.content.Intent
import android.os.SystemClock
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
import sh.zeron.android.core.OnboardingStep
import sh.zeron.android.design.ThemeMode

/** Exercise the UI in preview mode; never log in or mark a real account as onboarded. */
class OnboardingFlowTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val model get() = (instrumentation.targetContext.applicationContext as ZeronApplication).model

    @Test fun welcomeCanConfigureAppearanceBrowseAgentsAndStartASession() {
        val original = model.appearance.value
        try {
            launch("welcome")
            text("Get started").click()
            text("Make it yours")
            text("Light").click()
            assertEquals(ThemeMode.Light, model.appearance.value.mode)
            text("Continue").click()
            text("Sync your work")
            text("You're exploring the offline demo.")
            text("Continue").click()
            text("Your agents")
            requireNotNull(device.wait(Until.findObject(By.res("provider-claude-code")), 10_000)).click()
            text("Claude Code")
            requireNotNull(device.wait(Until.findObject(By.desc("Back").pkg(instrumentation.targetContext.packageName)), 5_000)).click()
            assertTrue(device.wait(Until.hasObject(By.res("provider-claude-code")), 5_000))
            text("Continue").click()
            text("You're all set")
            text("Start a session").click()
            assertTrue(device.wait(Until.hasObject(By.desc("Close").pkg(instrumentation.targetContext.packageName)), 10_000))
            assertNull(model.onboarding.step.value)
            recreate()
            assertNull(model.onboarding.step.value)
            assertTrue(device.wait(Until.hasObject(By.desc("Close").pkg(instrumentation.targetContext.packageName)), 10_000))
        } finally { reset(original) }
    }

    @Test fun backAndRecreationKeepTheCurrentStepAndAppearance() {
        val original = model.appearance.value
        try {
            launch("appearance")
            text("Make it yours")
            text("Dark").click()
            text("Continue").click()
            text("Sync your work")
            recreate()
            text("Sync your work")
            assertEquals(OnboardingStep.SYNC, model.onboarding.step.value)
            assertEquals(ThemeMode.Dark, model.appearance.value.mode)
            device.pressBack()
            text("Make it yours")
            device.pressBack()
            text("Welcome to Zerun")
            text("Get started")
        } finally { reset(original) }
    }

    private fun reset(appearance: sh.zeron.android.design.Appearance) = instrumentation.runOnMainSync {
        model.setAppearance(appearance)
        model.onboarding.attach("demo/test", true)
    }

    private fun text(value: String) = requireNotNull(device.wait(Until.findObject(By.text(value)), 10_000)) { "Missing text: $value" }

    private fun launch(step: String) {
        val context = instrumentation.targetContext
        context.startActivity(context.packageManager.getLaunchIntentForPackage(context.packageName)!!
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra("demo", true).putExtra("fast", true).putExtra("onboarding", step))
        val wanted = OnboardingStep.valueOf(step.uppercase())
        await { model.isDemo && model.onboarding.step.value == wanted }
        device.waitForIdle()
    }

    private fun recreate() {
        lateinit var old: MainActivity
        instrumentation.runOnMainSync {
            old = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).single() as MainActivity
            old.recreate()
        }
        await {
            var ready = false
            instrumentation.runOnMainSync {
                ready = ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED).any { it !== old }
            }
            ready
        }
        device.waitForIdle()
        if (android.os.Build.VERSION.SDK_INT >= 33) instrumentation.uiAutomation.clearCache()
    }

    private fun await(condition: () -> Boolean) {
        val deadline = SystemClock.uptimeMillis() + 15_000
        while (SystemClock.uptimeMillis() < deadline) {
            if (condition()) return
            SystemClock.sleep(100)
        }
        fail("Timed out waiting for onboarding")
    }
}
