package sh.zeron.android.core

import java.net.URI
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeout
import uniffi.zeron_core.CoreClient
import uniffi.zeron_core.DeviceLoginInfo
import uniffi.zeron_core.DeviceLoginPollInfo

data class AgentTarget(val deviceId: String, val deviceName: String, val harness: String, val label: String) {
    fun matches(other: AgentTarget) = deviceId == other.deviceId && harness == other.harness
}

data class AgentSignInState(
    val target: AgentTarget,
    val loginId: String? = null,
    val url: String? = null,
    val browserRequest: Int = 0,
    val browserPending: Boolean = false,
    val pasteCode: Boolean = false,
    val submitting: Boolean = false,
    val message: String? = null,
    val error: String? = null,
    val completed: Boolean = false,
) {
    val busy get() = error == null && !completed
}

/** The RPC boundary, also used by deterministic sign-in lifecycle tests. */
internal interface AgentLoginClient {
    suspend fun start(target: AgentTarget, provider: String?, doneUrl: String): DeviceLoginInfo
    suspend fun poll(target: AgentTarget, loginId: String, doneUrl: String): DeviceLoginPollInfo
    suspend fun complete(target: AgentTarget, loginId: String, code: String)
    fun cancel(target: AgentTarget, loginId: String)
}

internal class CoreAgentLoginClient(private val client: CoreClient) : AgentLoginClient {
    override suspend fun start(target: AgentTarget, provider: String?, doneUrl: String) =
        client.deviceLoginStart(target.deviceId, target.harness, provider, doneUrl)
    override suspend fun poll(target: AgentTarget, loginId: String, doneUrl: String) =
        client.deviceLoginPoll(target.deviceId, loginId, doneUrl)
    override suspend fun complete(target: AgentTarget, loginId: String, code: String) {
        client.deviceLoginComplete(target.deviceId, loginId, code)
    }
    override fun cancel(target: AgentTarget, loginId: String) = client.deviceLoginCancel(target.deviceId, loginId)
}

internal fun agentBrowserUrl(value: String): Boolean = runCatching {
    val uri = URI(value)
    uri.userInfo == null && !uri.host.isNullOrEmpty() &&
        (uri.scheme.equals("https", true) || (uri.scheme.equals("http", true) &&
            uri.host.lowercase() in setOf("localhost", "127.0.0.1", "[::1]")))
}.getOrDefault(false)

/** App-owned so returning from a browser, or recreating an Activity, keeps the same sign-in. */
class DeviceSignIn internal constructor(
    private val scope: CoroutineScope,
    private val client: () -> AgentLoginClient?,
    private val timeoutMs: Long = 600_000,
    private val pollMs: Long = 1_500,
) {
    private val _state = MutableStateFlow<AgentSignInState?>(null)
    val state = _state.asStateFlow()
    private var generation = 0
    private var request = 0
    private var job: Job? = null
    private var code: CompletableDeferred<String>? = null

    fun start(target: AgentTarget, provider: String?) {
        val api = client() ?: return
        cancel()
        val version = generation
        _state.value = AgentSignInState(target)
        job = scope.launch {
            var loginId: String? = null
            var succeeded = false
            fun update(change: (AgentSignInState) -> AgentSignInState) {
                if (generation == version) _state.value = _state.value?.let(change)
            }
            fun open(url: String?) {
                if (url.isNullOrEmpty() || _state.value?.url != null) return
                check(agentBrowserUrl(url)) { "The computer returned an invalid sign-in link." }
                update { it.copy(url = url, browserRequest = ++request, browserPending = true) }
            }
            try {
                withTimeout(timeoutMs) {
                    // Capture the remote id even if the page is closed while StartAgentLogin is in flight.
                    val started = withContext(NonCancellable) {
                        api.start(target, provider, DONE_URL).also { loginId = it.loginId }
                    }
                    if (!isActive || generation != version) throw CancellationException()
                    val pasted = CompletableDeferred<String>()
                    code = pasted
                    update { it.copy(loginId = started.loginId, pasteCode = started.pasteCode) }
                    open(started.url)
                    if (started.pasteCode) {
                        api.complete(target, started.loginId, pasted.await())
                    } else {
                        while (true) {
                            delay(pollMs)
                            val reply = api.poll(target, started.loginId, DONE_URL)
                            if (generation != version) throw CancellationException()
                            when (reply.status) {
                                "done" -> break
                                "error" -> error(reply.message ?: "The sign-in failed.")
                                "pending" -> {
                                    open(reply.url)
                                    update { it.copy(message = reply.message) }
                                }
                                else -> error("The computer returned an unknown sign-in status.")
                            }
                        }
                    }
                    succeeded = true
                }
                update { it.copy(completed = true, browserPending = false, submitting = false) }
            } catch (_: TimeoutCancellationException) {
                update { it.copy(error = "The sign-in timed out. Try again.", browserPending = false) }
            } catch (cancelled: CancellationException) {
                throw cancelled
            } catch (error: Exception) {
                update { it.copy(error = error.message ?: "Couldn't sign in.", browserPending = false, submitting = false) }
            } finally {
                if (!succeeded) loginId?.let { runCatching { api.cancel(target, it) } }
                if (generation == version) code = null
            }
        }
    }

    fun submitCode(value: String) {
        val current = _state.value ?: return
        val pending = code ?: return
        if (!current.busy || !current.pasteCode || current.submitting || value.isBlank()) return
        _state.value = current.copy(submitting = true)
        pending.complete(value.trim())
    }

    fun browserOpened(request: Int) {
        _state.value?.takeIf { it.browserRequest == request }?.let { _state.value = it.copy(browserPending = false) }
    }

    fun browserFailed() {
        val current = _state.value ?: return
        cancel()
        _state.value = current.copy(error = "Couldn't open the sign-in page. Try again.", browserPending = false)
    }

    /** A deep link only returns to the app; the computer's poll remains authoritative. */
    fun browserReturned() {
        _state.value?.takeIf { it.busy }?.let { _state.value = it.copy(message = "Finishing sign-in…") }
    }

    fun cancel(target: AgentTarget? = null) {
        if (target != null && _state.value?.target?.matches(target) != true) return
        generation++
        job?.cancel()
        job = null
        code = null
        _state.value = null
    }

    companion object { const val DONE_URL = "zerun-dev://signed-in" }
}
