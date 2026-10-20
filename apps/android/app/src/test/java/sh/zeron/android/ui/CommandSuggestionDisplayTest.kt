package sh.zeron.android.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Test
import uniffi.zeron_core.ComposerCommand
import uniffi.zeron_core.restoreComposerDraft

class CommandSuggestionDisplayTest {
    /** Mirrors Invocation::link for a file-backed skill; restore round-trips the path. */
    private fun skill(token: String, name: String, path: String): ComposerCommand {
        val json = """{"kind":"skill","name":"$name","path":"$path"}"""
        val payload = json.toByteArray().joinToString("") { "%02x".format(it.toInt() and 0xff) }
        return ComposerCommand(token, "", null, "[\$$name](zeron-invoke:$payload)", null)
    }

    @Test fun skillNamesMatchDesktopPresentation() {
        assertEquals("Imagegen", commandDisplayName(skill("\$imagegen", "imagegen", "/skills/imagegen/SKILL.md")))
        assertEquals("Skill Creator", commandDisplayName(skill("\$skill_creator", "skill_creator", "/skills/skill_creator/SKILL.md")))
        assertEquals("Review Agent", commandDisplayName(skill("\$plugin:review-agent", "plugin:review-agent", "/skills/review-agent/SKILL.md")))
        assertEquals("Review2", commandDisplayName(skill("\$review2", "review2", "/skills/review2/SKILL.md")))
        assertEquals("2fa", commandDisplayName(skill("\$2fa", "2fa", "/skills/2fa/SKILL.md")))
        assertEquals("/compact", commandDisplayName(ComposerCommand("/compact", "", null, "/compact", null)))
    }

    @Test fun sameNameSuffixKeepsHumanNameAndEncodedPath() {
        val first = skill("\$review", "review", "/a/SKILL.md")
        val second = skill("\$review:2", "review", "/b/SKILL.md")
        assertEquals("Review", commandDisplayName(first))
        assertEquals("Review · 2", commandDisplayName(second))
        assertNotEquals(first.insertion, second.insertion)

        val restored = restoreComposerDraft(second.insertion)
        assertEquals("\$review", restored.text)
        assertEquals(second.insertion, restored.selections.single().insertion)
        assertEquals("\$review:2", second.token)
    }

    @Test fun detailMatchesDesktopComposition() {
        val command = ComposerCommand("\$review", "Review changes", "files", "insertion", "Desktop only")
        assertEquals("Review changes · Desktop only · <files>", commandDetail(command))
        assertEquals("", commandDetail(command.copy(description = "", inputHint = null, unavailableReason = null)))
    }
}
