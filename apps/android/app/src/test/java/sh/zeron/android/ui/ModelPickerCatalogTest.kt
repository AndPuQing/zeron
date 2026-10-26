package sh.zeron.android.ui

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.cancelAndJoin
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import org.junit.Assert.*
import org.junit.Test

class ModelPickerCatalogTest {
    @Test fun addingProvidersWhileCurrentModelsLoadDoesNotLoseCurrentCatalog() = runBlocking {
        val currentResult = CompletableDeferred<List<ModelPickerRow>>()
        val currentStarted = CompletableDeferred<Unit>()
        var calls = 0
        val current = ModelPickerTab("claude-code", "Claude") {
            calls++
            currentStarted.complete(Unit)
            currentResult.await()
        }
        val other = ModelPickerRow("other-model", "Other model", null)
        val codex = ModelPickerTab("codex", "Codex") { listOf(other) }
        val catalog = ModelPickerCatalog(this)
        // Simulate LaunchedEffect disposal/restart when provider discovery finishes.
        val effect = launch { catalog.load(listOf(current)) }
        withTimeout(5_000) { currentStarted.await() }
        effect.cancelAndJoin()
        catalog.load(listOf(current, codex))
        awaitRows(catalog, "codex")
        assertEquals(listOf(other), catalog.rows["codex"])
        assertNull(catalog.rows["claude-code"])

        val first = ModelPickerRow("sonnet", "Sonnet", null)
        val second = ModelPickerRow("opus", "Opus", null)
        currentResult.complete(listOf(first, second, first))
        awaitRows(catalog, "claude-code")
        assertEquals(listOf(first, second), catalog.rows["claude-code"])
        catalog.load(listOf(codex, current))
        assertEquals(1, calls)
    }

    @Test fun closingPickerDiscardsPendingResultAndReopeningLoadsAgain() = runBlocking {
        val pending = CompletableDeferred<List<ModelPickerRow>>()
        val started = CompletableDeferred<Unit>()
        val pickerJob = Job(coroutineContext[Job])
        val catalog = ModelPickerCatalog(CoroutineScope(coroutineContext + pickerJob))
        catalog.load(listOf(ModelPickerTab("codex", "Codex") {
            started.complete(Unit)
            pending.await()
        }))
        withTimeout(5_000) { started.await() }
        pickerJob.cancelAndJoin()
        val row = ModelPickerRow("model", "Model", null)
        pending.complete(listOf(row))
        assertNull(catalog.rows["codex"])
        val reopened = ModelPickerCatalog(this)
        reopened.load(listOf(ModelPickerTab("codex", "Codex") { listOf(row) }))
        awaitRows(reopened, "codex")
        assertEquals(listOf(row), reopened.rows["codex"])
    }

    private suspend fun awaitRows(catalog: ModelPickerCatalog, harness: String) {
        withTimeout(5_000) {
            while (catalog.rows[harness] == null) yield()
        }
    }
}
