package sh.zeron.android.core

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.*
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.DeviceLoginInfo
import uniffi.zeron_core.DeviceLoginPollInfo

@OptIn(ExperimentalCoroutinesApi::class)
class DeviceSignInTest {
    private val target = AgentTarget("computer", "My computer", "codex", "Codex")
    private class Fake : AgentLoginClient {
        var reply = DeviceLoginInfo("login", "https://example.com/auth", false)
        var starting: CompletableDeferred<DeviceLoginInfo>? = null
        val replies = ArrayDeque<DeviceLoginPollInfo>()
        val cancelled = mutableListOf<String>()
        val codes = mutableListOf<String>()
        var completeError = false
        override suspend fun start(target: AgentTarget, provider: String?, doneUrl: String) = starting?.await() ?: reply
        override suspend fun poll(target: AgentTarget, loginId: String, doneUrl: String) =
            replies.removeFirstOrNull() ?: DeviceLoginPollInfo("pending", null, null)
        override suspend fun complete(target: AgentTarget, loginId: String, code: String) {
            codes += code
            if (completeError) error("Invalid code")
        }
        override fun cancel(target: AgentTarget, loginId: String) { cancelled += loginId }
    }

    @Test fun browserReturnDoesNotCompleteLoginAndRequestIsConsumedOnce() = runTest {
        val api = Fake()
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        val request = flow.state.value!!.browserRequest
        flow.browserOpened(request)
        flow.browserReturned()
        assertFalse(flow.state.value!!.completed)
        assertFalse(flow.state.value!!.browserPending)
        api.replies += DeviceLoginPollInfo("done", null, null)
        advanceTimeBy(1_500); runCurrent()
        assertTrue(flow.state.value!!.completed)
        assertTrue(api.cancelled.isEmpty())
    }

    @Test fun delayedUrlOpensOnceAndLeavingCancelsTheRemoteLogin() = runTest {
        val api = Fake().apply {
            reply = reply.copy(url = "")
            replies += DeviceLoginPollInfo("pending", "Device code: 1234", "https://example.com/code")
            replies += DeviceLoginPollInfo("pending", null, "https://example.com/code")
        }
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        assertNull(flow.state.value!!.url)
        advanceTimeBy(1_500); runCurrent()
        assertEquals("Device code: 1234", flow.state.value!!.message)
        val request = flow.state.value!!.browserRequest
        flow.browserOpened(request)
        advanceTimeBy(1_500); runCurrent()
        assertEquals(request, flow.state.value!!.browserRequest)
        assertFalse(flow.state.value!!.browserPending)
        flow.cancel(target); runCurrent()
        assertNull(flow.state.value)
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun lateStartIsCancelledWithoutOverwritingNewLogin() = runTest {
        val old = Fake().apply { starting = CompletableDeferred() }
        val next = Fake().apply { reply = reply.copy(loginId = "next") }
        var api = old
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        api = next
        flow.start(target.copy(deviceId = "other"), null); runCurrent()
        old.starting!!.complete(old.reply); runCurrent()
        assertEquals(listOf("login"), old.cancelled)
        assertEquals("next", flow.state.value!!.loginId)
        flow.cancel(); runCurrent()
        assertEquals(listOf("next"), next.cancelled)
    }

    @Test fun timeoutShowsAnErrorAndCancelsRemoteLogin() = runTest {
        val api = Fake()
        val flow = DeviceSignIn(this, { api }, timeoutMs = 5_000)
        flow.start(target, null)
        advanceUntilIdle()
        assertTrue(flow.state.value!!.error!!.contains("timed out"))
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun timedOutStartStillCleansUpTheLateRemoteId() = runTest {
        val api = Fake().apply { starting = CompletableDeferred() }
        val flow = DeviceSignIn(this, { api }, timeoutMs = 5_000)
        flow.start(target, null); runCurrent()
        advanceTimeBy(6_000); runCurrent()
        api.starting!!.complete(api.reply); runCurrent()
        assertTrue(flow.state.value!!.error!!.contains("timed out"))
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun pasteCodeIsTrimmedAndSubmittedOnlyOnce() = runTest {
        val api = Fake().apply { reply = reply.copy(pasteCode = true) }
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        flow.submitCode(" ")
        assertFalse(flow.state.value!!.submitting)
        flow.submitCode(" code "); flow.submitCode("second"); runCurrent()
        assertEquals(listOf("code"), api.codes)
        assertTrue(flow.state.value!!.completed)
        assertTrue(api.cancelled.isEmpty())
    }

    @Test fun invalidCodeShowsErrorAndCleansUp() = runTest {
        val api = Fake().apply { reply = reply.copy(pasteCode = true); completeError = true }
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        flow.submitCode("bad"); runCurrent()
        assertEquals("Invalid code", flow.state.value!!.error)
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun cancellingAnotherPageDoesNotCancelThisTarget() = runTest {
        val api = Fake()
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        flow.cancel(target.copy(deviceId = "other")); runCurrent()
        assertTrue(flow.state.value!!.busy)
        flow.cancel(target); runCurrent()
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun invalidBrowserLinkNeverOpensAndCancelsLogin() = runTest {
        val api = Fake().apply { reply = reply.copy(url = "file:///etc/passwd") }
        val flow = DeviceSignIn(this, { api })
        flow.start(target, null); runCurrent()
        assertNotNull(flow.state.value!!.error)
        assertNull(flow.state.value!!.url)
        assertEquals(listOf("login"), api.cancelled)
    }

    @Test fun browserLinksRequireHttpsOrLocalOauthCallback() {
        listOf("https://example.com/auth", "http://localhost:1234/auth", "http://127.0.0.1:1234/", "http://[::1]:1234/").forEach { assertTrue(it, agentBrowserUrl(it)) }
        listOf("http://example.com/auth", "https://user:secret@example.com/", "javascript:alert(1)", "intent://host", "https://", "https://example.com\n").forEach { assertFalse(it, agentBrowserUrl(it)) }
    }
}
