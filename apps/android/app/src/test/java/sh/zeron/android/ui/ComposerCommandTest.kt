package sh.zeron.android.ui

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.ComposerCommand

class ComposerCommandTest {
    private val goal = ComposerCommand("/goal", "Goal", "objective", "canonical", null)

    @Test fun onlyLeadingSlashOpensMenu() {
        val model = ComposerModel()
        for (text in listOf("    /goal", "\t/goal", "ask /goal", "/goal ship it", "```\n/goal", "/goal\t")) {
            model.setText(text)
            assertNull(text, model.slashQuery())
        }
        model.setText("  /go")
        assertEquals("go", model.slashQuery()?.second)
    }

    @Test fun completionPreservesArgumentsAndClosesMenu() {
        val model = ComposerModel()
        model.value = TextFieldValue("/gozzz ship it", TextRange(3))
        model.insertCommand(goal)
        assertEquals("/goal ship it", model.text)
        assertEquals(6, model.value.selection.start)
        assertNull(model.slashQuery())
    }

    @Test fun disabledCommandAndSelectedTextDoNotChangeDraft() {
        val model = ComposerModel("/go")
        model.insertCommand(goal.copy(unavailableReason = "Desktop only"))
        assertEquals("/go", model.text)
        model.value = model.value.copy(selection = TextRange(0, 3))
        assertNull(model.slashQuery())
    }

    @Test fun completionAddsArgumentSpace() {
        val model = ComposerModel("/go")
        model.insertCommand(goal)
        assertEquals("/goal ", model.text)
        assertNull(model.slashQuery())
    }
}
