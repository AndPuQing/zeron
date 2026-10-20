package sh.zeron.android.ui

import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.input.TextFieldValue
import org.junit.Assert.*
import org.junit.Test
import uniffi.zeron_core.ComposerCommand

class ComposerCommandTest {
    private val goal = ComposerCommand("/goal", "Goal", "objective", "canonical", null)

    @Test fun completionUsesDesktopProseBoundaries() {
        val model = ComposerModel()
        for (text in listOf("    /goal", "\t/goal", "/goal ship it", "```\n/goal", "/goal\t")) {
            model.setText(text)
            assertNull(text, model.completionQuery())
        }
        model.setText("  /go")
        assertEquals("go", model.completionQuery()?.query)
    }

    @Test fun completionPreservesArgumentsAndClosesMenu() {
        val model = ComposerModel()
        model.value = TextFieldValue("/gozzz ship it", TextRange(3))
        model.insertCommand(goal)
        assertEquals("/goal ship it", model.text)
        assertEquals(6, model.value.selection.start)
        assertNull(model.completionQuery())
    }

    @Test fun disabledCommandAndSelectedTextDoNotChangeDraft() {
        val model = ComposerModel("/go")
        model.insertCommand(goal.copy(unavailableReason = "Desktop only"))
        assertEquals("/go", model.text)
        model.value = model.value.copy(selection = TextRange(0, 3))
        assertNull(model.completionQuery())
    }

    @Test fun lateSelectionFromAnotherContextIsIgnored() {
        val model = ComposerModel("/go")
        model.commandContext = "new-host:codex"
        model.insertCommand(goal, "old-host:codex")
        assertEquals("/go", model.text)
    }

    @Test fun completionAddsArgumentSpace() {
        val model = ComposerModel("/go")
        model.insertCommand(goal)
        assertEquals("/goal ", model.text)
        assertNull(model.completionQuery())
    }
    @Test fun menusSeparateSkillsAndRejectOldQueries() {
        val model = ComposerModel("/")
        val skill = ComposerCommand("\$model", "Skill", null, "[\$model](zeron-invoke:7b226b696e64223a22736b696c6c222c226e616d65223a226d6f64656c222c2270617468223a222f70726f6a6563742f534b494c4c2e6d64227d)", null)
        val catalog = uniffi.zeron_core.ComposerCatalog(listOf(goal, skill), null)
        val slash = model.completionQuery()!!
        assertEquals(listOf(goal), model.completionRows(catalog, slash))
        model.setText("\$")
        val dollar = model.completionQuery()!!
        assertTrue(dollar.skills)
        assertEquals(listOf(skill), model.completionRows(catalog, dollar))
        model.insertCommand(goal, expectedQuery = slash)
        assertEquals("\$", model.text)
        model.value = model.value.copy(composition = TextRange(0, 1))
        assertNull(model.completionQuery())
        model.insertCommand(skill, expectedQuery = dollar)
        assertEquals("\$", model.text)
    }

    @Test fun inlineSkillPreservesUnicodePunctuationAndIdentityAcrossDraftRestore() {
        val link = "[\$model](zeron-invoke:7b226b696e64223a22736b696c6c222c226e616d65223a226d6f64656c222c2270617468223a222f70726f6a6563742f534b494c4c2e6d64227d)"
        val skill = ComposerCommand("\$model", "Skill", null, link, null)
        val model = ComposerModel()
        model.value = TextFieldValue("👋 use (\$mo) please", TextRange("👋 use (\$mo".length))
        model.insertCommand(skill)
        assertEquals("👋 use (\$model) please", model.text)
        assertNull(model.completionQuery())
        val encoded = "👋 use ($link) please"
        assertEquals(encoded, model.encodedDraft())
        val restored = ComposerModel()
        restored.setText(model.persistedDraft())
        assertEquals(encoded, restored.encodedDraft())
        restored.commandContext = "old"
        restored.commandContext = "new"
        assertNotNull(restored.selectionError)
        model.setText("/mo")
        model.insertCommand(skill)
        assertEquals("/mo", model.text)
    }

    @Test fun completionKeepsLineBreakAndClosingPunctuation() {
        for (suffix in listOf("\nnext", "\r\nnext", ")", ",")) {
            val model = ComposerModel()
            model.value = TextFieldValue("/go$suffix", TextRange(3))
            model.insertCommand(goal)
            assertEquals("/goal$suffix", model.text)
            assertEquals(5, model.value.selection.start)
            assertNull(model.completionQuery())
        }
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

    @Test fun namespacedActionsWorkAcrossOtherAgentsWithoutInterceptingNativeCommands() = kotlinx.coroutines.runBlocking {
        for (harness in listOf("claude-code", "copilot", "cursor", "devin", "grok", "hermes", "pi", "omp", "opencode", "antigravity", "dsh")) {
            val draft = ComposerModel()
            val actions = mutableListOf<uniffi.zeron_core.MobileComposerAction>()
            draft.localActions = MobileCommandActions(harness, this, { context }) { actions += it }
            val command = modelCommand.copy(token = "/zeron:model", insertion = "/zeron:model")
            assertNull(draft.displayCommand(command).unavailableReason)
            draft.value = TextFieldValue("/zeron:mo keep this", TextRange(9))
            draft.insertCommand(command)
            assertEquals(" keep this", draft.encodedDraft())
            draft.setText("/zeron:model")
            assertTrue(draft.performLocalCommand())
            assertEquals("", draft.text)
            assertEquals(List(2) { uniffi.zeron_core.MobileComposerAction.MODEL }, actions)
            for (native in listOf("/model", "/name", "/fork", "/review", "/compact")) {
                draft.setText("$native native argument")
                assertFalse(draft.performLocalCommand())
                assertEquals("$native native argument", draft.encodedDraft())
            }
            draft.setText("/zeron:rename title")
            assertTrue(draft.performLocalCommand())
            assertEquals("/zeron:rename title", draft.text)
            assertEquals(2, actions.size)
            draft.localActions = MobileCommandActions(harness, this, { context.copy(canFork = false) }) { fail("Fork is unavailable") }
            val fork = command.copy(token = "/zeron:fork", insertion = "/zeron:fork")
            assertNotNull(draft.displayCommand(fork).unavailableReason)
            draft.setText("/zeron:fork")
            draft.insertCommand(fork)
            assertEquals("/zeron:fork", draft.text)
        }
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
