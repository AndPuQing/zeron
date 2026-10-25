package sh.zeron.android.ui

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.*
import org.junit.Test

/** Uses the actual mobile native library for persisted reference validation. */
class ComposerSelectionTest {
    private fun skillLink(path: String): String {
        val json = """{"kind":"skill","name":"review","path":"$path"}"""
        val hex = json.toByteArray().joinToString("") { "%02x".format(it) }
        return "[\$review](zeron-invoke:$hex)"
    }

    private fun wire(): String = skillLink("/b/SKILL.md") + " check this"

    @Test fun insertingSameNameSkillAfterRestorePreservesBothIdentities() {
        val draft = ComposerModel()
        draft.setText(wire())
        draft.value = TextFieldValue("\$ " + draft.text, TextRange(1))
        draft.insertCommand(uniffi.zeron_core.ComposerCommand(
            token = "\$review", description = "", inputHint = null,
            insertion = skillLink("/a/SKILL.md"), unavailableReason = null,
        ))
        assertEquals("\$review:2 \$review check this", draft.text)
        assertEquals(skillLink("/a/SKILL.md") + " " + wire(), draft.encoded())
        val reopened = ComposerModel()
        reopened.setText(draft.persistedDraft())
        assertEquals(draft.encoded(), reopened.encoded())
    }

    @Test fun constructorRestoresCanonicalDraftFromStorage() {
        val original = ComposerModel()
        original.setText(wire())
        val restored = ComposerModel(original.persistedDraft())
        assertEquals(original.text, restored.text)
        assertEquals(original.encoded(), restored.encoded())
    }

    @Test fun contextChangesRemainInvalidAfterRestoringDraft() {
        val draft = ComposerModel()
        draft.commandContext = "host:agent:project-a"
        draft.setText(wire())
        assertNull(draft.selectionError)
        draft.commandContext = "host:agent:project-b"
        assertNotNull(draft.selectionError)
        val reopened = ComposerModel()
        reopened.setText(draft.persistedDraft())
        assertNotNull(reopened.selectionError)
        reopened.value = TextFieldValue("check this", TextRange(10))
        assertNull(reopened.selectionError)
    }

    @Test fun prependingTextPreservesIdentity() {
        val draft = ComposerModel()
        draft.setText(wire())
        draft.value = TextFieldValue("Please use " + draft.text)
        assertTrue(draft.encoded().contains("zeron-invoke:"))
        val reopened = ComposerModel()
        reopened.setText(draft.persistedDraft())
        assertEquals(draft.encoded(), reopened.encoded())
    }
}
