package sh.zeron.android.core

import org.junit.Assert.*
import org.junit.Test

class OnboardingStateTest {
    private val stored = mutableMapOf<String, String>()
    private fun state() = OnboardingState(stored::get) { key, value ->
        if (value == null) stored.remove(key) else stored[key] = value
    }

    @Test fun syncCannotAdvanceBeforeAuthenticationEvenAfterProcessRecreation() {
        val flow = state()
        flow.attach(null, false)
        flow.begin()
        assertEquals(OnboardingStep.APPEARANCE, flow.step.value)
        flow.next()
        flow.next()
        assertEquals(OnboardingStep.SYNC, flow.step.value)
        val restored = state()
        restored.attach(null, false)
        assertEquals(OnboardingStep.SYNC, restored.step.value)
        restored.finish()
        assertEquals(OnboardingStep.SYNC, restored.step.value)
        restored.attach("org/user", false)
        assertEquals(OnboardingStep.AGENTS, restored.step.value)
    }

    @Test fun completionBelongsToTheAccountAndOrganizationAndSurvivesSigningOut() {
        val flow = state()
        flow.attach("org/user", false)
        flow.next()
        flow.finish()
        flow.signedOut()
        flow.attach("org/user", false)
        assertNull(flow.step.value)
        flow.attach("other-org/user", false)
        assertEquals(OnboardingStep.AGENTS, flow.step.value)
        flow.attach("org/other-user", false)
        assertEquals(OnboardingStep.AGENTS, flow.step.value)
    }

    @Test fun returningToWelcomeDoesNotCompleteTheSignedInSetup() {
        val flow = state()
        flow.attach("org/user", false)
        repeat(3) { flow.back() }
        assertEquals(OnboardingStep.WELCOME, flow.step.value)
        flow.finish()
        state().also {
            it.attach("org/user", false)
            assertEquals(OnboardingStep.AGENTS, it.step.value)
        }
    }

    @Test fun appearanceIsRememberedOnThisPhoneAcrossAccounts() {
        val flow = state()
        flow.attach(null, false)
        flow.begin()
        flow.next()
        flow.signedOut()
        flow.begin()
        assertEquals(OnboardingStep.SYNC, flow.step.value)
    }

    @Test fun signedInProgressResumesOnlyForItsOwner() {
        val flow = state()
        flow.attach("org/user", false)
        flow.next()
        val restored = state()
        restored.attach("org/user", false)
        assertEquals(OnboardingStep.DONE, restored.step.value)
        restored.attach("org/other-user", false)
        assertEquals(OnboardingStep.AGENTS, restored.step.value)
    }

    @Test fun demoAndForcedPreviewsNeverWriteCompletionOrProgress() {
        val flow = state()
        flow.attach("demo/user", true)
        assertNull(flow.step.value)
        flow.attach("demo/user", true, OnboardingStep.WELCOME)
        flow.begin()
        repeat(3) { flow.next() }
        assertEquals(OnboardingStep.DONE, flow.step.value)
        flow.finish()
        assertNull(flow.step.value)
        assertTrue(stored.isEmpty())
        flow.attach("org/user", false, OnboardingStep.DONE)
        flow.finish()
        assertTrue(stored.isEmpty())
    }

    @Test fun aSignedOutPreviewCannotExposeTheAgentsStep() {
        val flow = state()
        flow.attach(null, false, OnboardingStep.DONE)
        assertEquals(OnboardingStep.SYNC, flow.step.value)
    }
}
