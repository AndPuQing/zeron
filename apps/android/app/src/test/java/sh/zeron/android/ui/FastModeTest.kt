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
            val fast = checkNotNull(FastModeOption.find(listOf(option(id, choices, fallback))))
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
        val claude = checkNotNull(FastModeOption.find(listOf(option("fastMode", listOf("off", "on"), "off"))))
        assertEquals(true, claude.enabled(mapOf("fastMode" to "true")))
        assertEquals(false, claude.enabled(mapOf("fastMode" to "false")))
        val codex = checkNotNull(FastModeOption.find(listOf(option("serviceTier", listOf("default", "fast"), "default"))))
        assertEquals(true, codex.enabled(mapOf("serviceTier" to "priority")))
        assertNull(FastModeOption.find(emptyList()))
        assertNull(FastModeOption.find(listOf(option("speed", listOf("fast"), "fast"))))
        assertNull(FastModeOption.find(listOf(option("contextWindow", listOf("200k", "1m"), "200k"))))
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
}
