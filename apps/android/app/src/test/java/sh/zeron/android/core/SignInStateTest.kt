package sh.zeron.android.core

import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class SignInStateTest {
    private var stored: String? = null
    private fun state() = SignInState(read = { stored }, write = { stored = it })

    @Test fun pendingSignInSurvivesRecreatingTheModel() {
        val original = state().begin()
        assertTrue(state().accept(original))
    }

    @Test fun missingOrWrongStateDoesNotConsumeThePendingSignIn() {
        val signIn = state()
        val original = signIn.begin()
        assertFalse(signIn.accept(null))
        assertFalse(signIn.accept("another-sign-in"))
        assertTrue(signIn.accept(original))
    }

    @Test fun callbackCannotBeReplayedAfterItIsAccepted() {
        val signIn = state()
        val original = signIn.begin()
        assertTrue(signIn.accept(original))
        assertFalse(state().accept(original))
    }

    @Test fun startingAgainInvalidatesThePreviousSignIn() {
        val signIn = state()
        val first = signIn.begin()
        val second = signIn.begin()
        assertNotEquals(first, second)
        assertFalse(signIn.accept(first))
        assertTrue(signIn.accept(second))
    }

    @Test fun signingOutInvalidatesThePendingCallback() {
        val signIn = state()
        val original = signIn.begin()
        signIn.clear()
        assertFalse(signIn.accept(original))
    }
}
