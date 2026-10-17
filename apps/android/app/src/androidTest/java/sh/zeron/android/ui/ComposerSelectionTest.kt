package sh.zeron.android.ui

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.*
import org.junit.Test

/** Uses the actual mobile native library for persisted reference validation. */
class ComposerSelectionTest {
    private fun wire(): String {
        val json = """{"kind":"skill","name":"review","path":"/b/SKILL.md"}"""
        val hex = json.toByteArray().joinToString("") { "%02x".format(it) }
        return "[\$review](zeron-invoke:$hex) check this"
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
