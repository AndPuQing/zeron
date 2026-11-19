package sh.zeron.android.ui

import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.*

class FastModeTest {
    private fun option(id: String, choices: List<String>, fallback: String) =
        ModelOption(id, "Speed", choices.map { ModelOptionChoice(it, it) }, fallback)

    @Test fun catalogEncodingsWriteExplicitOffAndPreserveOtherOptions() {
        val cases = listOf(
            Triple("serviceTier", listOf("default", "fast"), "default"),
            Triple("fastMode", listOf("off", "on"), "off"),
            Triple("fast_mode", listOf("off", "on"), "off"),
            Triple("fast", listOf("false", "true"), "true"),
            Triple("speed", listOf("normal", "fast"), "normal"),
        )
        for ((id, choices, fallback) in cases) {
            val fast = checkNotNull(findFastMode(listOf(option(id, choices, fallback))))
            val initial = mapOf("contextWindow" to "1m")
            assertEquals("Fast: Default", fast.title(initial))
            val enabled = fast.applying(true, initial)
            assertEquals(choices[1], enabled[id])
            assertEquals(true, fast.enabled(enabled))
            val disabled = fast.applying(false, enabled)
            assertEquals(choices[0], disabled[id])
            assertEquals(false, fast.enabled(disabled))
            assertEquals("1m", disabled["contextWindow"])
            assertEquals(initial, fast.applying(null, disabled))
        }
    }

    @Test fun legacyBooleanAndCodexPriorityAreRecognized() {
        val claude = checkNotNull(findFastMode(listOf(option("fastMode", listOf("off", "on"), "off"))))
        assertEquals(true, claude.enabled(mapOf("fastMode" to "true")))
        assertEquals(false, claude.enabled(mapOf("fastMode" to "false")))
        val codex = checkNotNull(findFastMode(listOf(option("serviceTier", listOf("default", "fast"), "default"))))
        assertEquals(true, codex.enabled(mapOf("serviceTier" to "priority")))
        assertNull(findFastMode(emptyList()))
        assertNull(findFastMode(listOf(option("speed", listOf("fast"), "fast"))))
        assertNull(findFastMode(listOf(option("contextWindow", listOf("200k", "1m"), "200k"))))
    }

    @Test fun changingModelsDropsOldFastButPreservesOtherOptions() {
        val config = ChatConfig("supervised", true, "codex", "old", "high",
            mapOf("serviceTier" to "fast", "contextWindow" to "1m"), SandboxLevel.READ_ONLY)
        val changed = modelSelection(config, "codex", "no-fast", "serviceTier")
        assertEquals(mapOf("contextWindow" to "1m"), changed.modelOptions)
        assertEquals(config, modelSelection(config, "codex", "old", "serviceTier"))
        assertTrue(modelSelection(config, "claude-code", "sonnet", "serviceTier").modelOptions.isEmpty())
        assertEquals(config.accessMode, changed.accessMode)
        assertEquals(config.plan, changed.plan)
        val implicit = config.copy(model = null)
        assertEquals(config, modelSelection(implicit, "codex", "old", "serviceTier", "old"))
    }

    @Test fun explicitCodexOffSurvivesModelChangesForChatsAndDrafts() {
        val fast = checkNotNull(findFastMode(listOf(option("serviceTier", listOf("default", "fast"), "fast"))))
        val other = mapOf("contextWindow" to "1m")
        val off = fast.applying(false, fast.applying(true, other))
        val config = ChatConfig("supervised", true, "codex", "old", "high", off, SandboxLevel.READ_ONLY)
        val changed = modelSelection(config, "codex", "another-model", fast.id)
        assertEquals(config.copy(model = "another-model"), changed)
        assertEquals("default", changed.modelOptions["serviceTier"])
        val draft = sh.zeron.android.core.NewSessionDraft(harness = "codex", model = "old", modelOptions = off)
        val draftOptions = fastOptionsAfterModelChange(draft.modelOptions, draft.harness, fast.id)
        assertEquals(changed.modelOptions, draftOptions)
        assertEquals(false, fast.enabled(draftOptions))
        assertEquals(draftOptions, fastOptionsAfterModelChange(draftOptions, "codex", null))
        assertEquals(config.copy(model = "another-model", modelOptions = other),
            changed.copy(modelOptions = fast.applying(null, changed.modelOptions)))
        assertTrue(modelSelection(changed, "claude-code", "sonnet", fast.id).modelOptions.isEmpty())
    }

    @Test fun modelChangeDoesNotTurnInheritanceIntoOffOrChangeOtherProviders() {
        val other = mapOf("contextWindow" to "1m")
        for (value in listOf("fast", "priority")) {
            assertEquals(other, fastOptionsAfterModelChange(other + ("serviceTier" to value), "codex", "serviceTier"))
        }
        assertEquals(other, fastOptionsAfterModelChange(other, "codex", "serviceTier"))
        assertTrue(fastOptionsAfterModelChange(emptyMap(), "codex", "serviceTier").isEmpty())
        assertTrue(fastOptionsAfterModelChange(mapOf("serviceTier" to "default"), "other", "serviceTier").isEmpty())
        assertEquals(other, fastOptionsAfterModelChange(other + ("fastMode" to "off"), "claude-code", "fastMode"))
    }
}
