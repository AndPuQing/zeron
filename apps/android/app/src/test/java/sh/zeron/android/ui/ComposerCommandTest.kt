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

class MobileLocalCommandTest {
    private val modelCommand = ComposerCommand("/model", "Model", null, "/model", "Desktop only")
    private val context = uniffi.zeron_core.MobileComposerContext(true, false, true, true, false, false)

    @Test fun localMenuSelectionPreservesRemainingDraftAndDoesNotInsertReference() = kotlinx.coroutines.runBlocking {
        val draft = ComposerModel()
        val actions = mutableListOf<uniffi.zeron_core.MobileComposerAction>()
        draft.localActions = MobileCommandActions("codex", this, { context }) { actions += it }
        draft.value = TextFieldValue("/mo keep this", TextRange(3))
        assertNull(draft.displayCommand(modelCommand).unavailableReason)
        draft.insertCommand(modelCommand)
        assertEquals(" keep this", draft.text)
        assertEquals(" keep this", draft.encodedDraft())
        assertEquals(listOf(uniffi.zeron_core.MobileComposerAction.MODEL), actions)
    }

    @Test fun directSubmitRejectsArgumentsAndFailureRestoresTheCommand() = kotlinx.coroutines.runBlocking {
        val draft = ComposerModel("/rename title")
        var called = false
        draft.localActions = MobileCommandActions("codex", this, { context }) { called = true; error("offline") }
        assertTrue(draft.performLocalCommand())
        assertFalse(called)
        assertEquals("/rename title", draft.text)
        draft.setText("/model")
        assertTrue(draft.performLocalCommand())
        assertTrue(called)
        assertEquals("/model", draft.text)
        assertEquals("offline", draft.localError)
    }

    @Test fun pendingAndContextRestrictionsBlockActionsWithoutConsumingText() = kotlinx.coroutines.runBlocking {
        val draft = ComposerModel("/fork")
        var called = false
        draft.localActions = MobileCommandActions("codex", this, { context.copy(canFork = false) }) { called = true }
        assertTrue(draft.performLocalCommand())
        assertFalse(called)
        assertEquals("/fork", draft.text)
        draft.localActions = MobileCommandActions("claude-code", this, { context }) { called = true }
        assertFalse(draft.performLocalCommand())
        assertNotNull(draft.displayCommand(modelCommand).unavailableReason)
    }

    @Test fun pendingActionCannotBeTriggeredTwiceOrOverwriteEditsOnFailure() = kotlinx.coroutines.runBlocking {
        val draft = ComposerModel("/model")
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        var calls = 0
        draft.localActions = MobileCommandActions("codex", this, { context }) { calls++; gate.await(); error("failed") }
        draft.performLocalCommand()
        draft.setText("/model")
        draft.performLocalCommand()
        assertEquals(1, calls)
        draft.setText("new text")
        gate.complete(Unit)
        kotlinx.coroutines.yield()
        assertEquals("new text", draft.text)
    }
}
