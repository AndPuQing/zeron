package sh.zeron.android.ui

import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.*

class SessionModelPickerTest {
    private val config = ChatConfig("supervised", true, "codex", "old", "high", mapOf("contextWindow" to "1m"), SandboxLevel.READ_ONLY)
    private fun state() = ComposerState(
        "chat", 0uL, "Chat", HostInfo("host", "Computer", true, HostCapabilities(true, true, true, true, true, true, true, true)),
        LiveStatus(ChatIndicator.IDLE, false, null, false, false), emptyList(), emptyList(), null, false,
        RoomState(true, null, false), null, null, null, null, null,
    )

    @Test fun changingAgentClearsProviderSpecificOptionsButKeepsPermissions() {
        val selected = modelSelection(config, "claude-code", "sonnet")
        assertEquals("claude-code", selected.harness)
        assertEquals("sonnet", selected.model)
        assertNull(selected.reasoning)
        assertTrue(selected.modelOptions.isEmpty())
        assertEquals(SandboxLevel.READ_ONLY, selected.sandbox)
        assertEquals("supervised", selected.accessMode)
        assertTrue(selected.plan)
    }

    @Test fun sameAgentKeepsReasoningAndOptions() {
        assertEquals(config.copy(model = "new"), modelSelection(config, "codex", "new"))
    }

    @Test fun runningAndPendingMessagesCannotSwitchAgentEvenIfMenuWasOpenedEarlier() {
        val c = state()
        assertTrue(canSwitchAgent(c))
        assertFalse(canSwitchAgent(c.copy(live = c.live.copy(turnRunning = true))))
        for (pending in listOf(SendState.SENDING, SendState.QUEUED)) assertFalse(canSwitchAgent(c.copy(sendState = pending)))
        assertFalse(canSwitchAgent(c.copy(host = c.host.copy(capabilities = c.host.capabilities.copy(providerSwitch = false)))))
    }
}
