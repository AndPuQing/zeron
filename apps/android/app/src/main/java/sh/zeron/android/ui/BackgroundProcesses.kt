package sh.zeron.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import uniffi.zeron_core.BackgroundProcess
import uniffi.zeron_core.BackgroundOutput
import uniffi.zeron_core.CoreClient

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun BackgroundProcesses(client: CoreClient, chatId: String, onDismiss: () -> Unit) {
    var tasks by remember(chatId) { mutableStateOf<List<BackgroundProcess>>(emptyList()) }
    var selected by remember(chatId) { mutableStateOf<String?>(null) }
    var output by remember(chatId) { mutableStateOf<BackgroundOutput?>(null) }
    var stopping by remember(chatId) { mutableStateOf<Set<String>>(emptySet()) }
    var error by remember(chatId) { mutableStateOf<String?>(null) }
    var loaded by remember(chatId) { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val lifecycle = LocalLifecycleOwner.current
    val supported = client.backgroundTasksSupported(chatId)
    LaunchedEffect(client, chatId, selected, lifecycle, supported) {
        output = null
        if (!supported) return@LaunchedEffect
        lifecycle.lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                try {
                    tasks = client.backgroundTasks(chatId)
                    loaded = true
                    stopping = stopping.intersect(tasks.map { it.id }.toSet())
                    val id = selected
                    output = if (id != null && tasks.any { it.id == id }) client.backgroundTaskOutput(chatId, id) else null
                    error = null
                } catch (cancelled: CancellationException) { throw cancelled }
                catch (e: Exception) { error = e.message ?: "Couldn't refresh background processes" }
                delay(1500)
            }
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(24.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Background processes", style = MaterialTheme.typography.titleLarge)
            if (!supported) Text("Update this computer to view and stop background processes.")
            else if (!loaded && error == null) Text("Loading…")
            else if (tasks.isEmpty() && error == null) Text("No background processes reported by this agent.")
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            tasks.forEach { task ->
                Row(Modifier.fillMaxWidth()) {
                    Column(Modifier.weight(1f).clickable { selected = if (selected == task.id) null else task.id }) {
                        Text(task.command, style = MaterialTheme.typography.titleSmall)
                        task.description?.let { Text(it, style = MaterialTheme.typography.bodySmall) }
                        Text("Running for ${((System.currentTimeMillis() - task.startedAtMs).coerceAtLeast(0) / 1000)}s", style = MaterialTheme.typography.bodySmall)
                    }
                    TextButton(enabled = task.id !in stopping, onClick = {
                        stopping = stopping + task.id
                        scope.launch {
                            try {
                                if (!client.stopBackgroundTask(chatId, task.id)) {
                                    stopping = stopping - task.id
                                    tasks = client.backgroundTasks(chatId)
                                }
                            } catch (cancelled: CancellationException) { throw cancelled }
                            catch (e: Exception) { stopping = stopping - task.id; error = e.message }
                        }
                    }) { Text(if (task.id in stopping) "Stopping…" else "Stop") }
                }
                if (selected == task.id) {
                    if (output?.truncated == true) Text("Earlier output not shown", style = MaterialTheme.typography.bodySmall)
                    SelectionContainer { Text(output?.text?.ifBlank { "No output yet" } ?: "Loading output…", style = MaterialTheme.typography.bodySmall) }
                }
            }
            Spacer(Modifier.height(20.dp))
        }
    }
}
