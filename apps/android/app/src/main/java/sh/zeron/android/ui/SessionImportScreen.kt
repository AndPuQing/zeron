package sh.zeron.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.Alignment
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewModelScope
import androidx.lifecycle.viewmodel.compose.viewModel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import sh.zeron.android.core.AppModel
import uniffi.zeron_core.*

/** Navigation owns this handle, so rotation preserves the selection and batch. */
private class ImportViewModel(client: CoreClient) : ViewModel() {
    private val mutable = MutableStateFlow<SessionImportSnapshot?>(null)
    val state = mutable.asStateFlow()
    val importer = client.sessionImport(object : SessionImportListener {
        override fun onImportChanged(snapshot: SessionImportSnapshot) {
            viewModelScope.launch { mutable.value = snapshot }
        }
    })
    init { mutable.value = importer.snapshot() }
    override fun onCleared() { importer.dismiss(); importer.close() }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SessionImportScreen(model: AppModel, onClose: () -> Unit) {
    val client by model.client.collectAsState()
    val core = client ?: return
    val vm: ImportViewModel = viewModel(key = "session-import-${core.hashCode()}", factory = object : ViewModelProvider.Factory {
        @Suppress("UNCHECKED_CAST")
        override fun <T : ViewModel> create(modelClass: Class<T>): T = ImportViewModel(core) as T
    })
    val snapshot by vm.state.collectAsState()
    val s = snapshot ?: return
    val importer = vm.importer
    // Native snapshots arrive asynchronously; keep edits local so an older
    // query cannot replace newer keystrokes while the user is typing.
    var query by rememberSaveable(s.deviceId) { mutableStateOf(s.query) }
    val workspace by model.workspace.collectAsState()
    LaunchedEffect(workspace?.devices) { importer.updateHosts() }
    var confirmClose by remember { mutableStateOf(false) }
    val close = { if (s.running) confirmClose = true else onClose() }
    BackHandler { close() }
    if (confirmClose) AlertDialog(
        onDismissRequest = { confirmClose = false }, title = { Text("Stop importing?") },
        text = { Text("Sessions already imported will stay in your workspace.") },
        confirmButton = { TextButton(onClick = { importer.dismiss(); onClose() }) { Text("Stop and close") } },
        dismissButton = { TextButton(onClick = { confirmClose = false }) { Text("Keep importing") } },
    )
    var hostMenu by remember { mutableStateOf(false) }
    Scaffold(
        topBar = {
            Column(Modifier.statusBarsPadding()) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Import sessions", style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                    TextButton(onClick = close) { Text("Close") }
                }
            }
        },
        bottomBar = {
            Surface(tonalElevation = 2.dp) {
                Column(Modifier.navigationBarsPadding().imePadding().fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (s.running) {
                        val done = s.results.count { it.phase != SessionImportPhase.PENDING && it.phase != SessionImportPhase.IMPORTING }
                        LinearProgressIndicator(progress = { if (s.results.isEmpty()) 0f else done.toFloat() / s.results.size }, modifier = Modifier.fillMaxWidth())
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text("$done of ${s.results.size} finished", modifier = Modifier.weight(1f))
                            TextButton(enabled = !s.cancelling, onClick = { importer.cancel() }) { Text(if (s.cancelling) "Stopping…" else "Stop import") }
                        }
                    } else {
                        if (s.results.any { it.retryable }) {
                            OutlinedButton(onClick = { importer.start(retry = true) }, modifier = Modifier.fillMaxWidth()) { Text("Retry remaining") }
                        }
                        Button(
                            onClick = { importer.start(retry = false) },
                            enabled = s.selectedCount > 0u && !s.loading,
                            modifier = Modifier.fillMaxWidth(),
                        ) { Text(if (s.selectedCount == 0u) "Select sessions to import" else "Import ${s.selectedCount} selected") }
                    }
                }
            }
        },
    ) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding), contentPadding = PaddingValues(bottom = 16.dp)) {
            item("intro") {
                Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Import Codex and Claude Code history from your computer. Imports are saved in Archived; originals stay on the source device.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Box {
                        OutlinedButton(enabled = !s.running && s.hosts.isNotEmpty(), onClick = { hostMenu = true }, modifier = Modifier.fillMaxWidth()) {
                            Text(s.hosts.firstOrNull { it.id == s.deviceId }?.name ?: "Choose a computer")
                        }
                        DropdownMenu(expanded = hostMenu, onDismissRequest = { hostMenu = false }) {
                            s.hosts.forEach { host ->
                                DropdownMenuItem(
                                    text = { Text(host.name + when { !host.supported -> " · Update required"; !host.online -> " · Offline"; else -> "" }) },
                                    enabled = host.online && host.supported,
                                    onClick = { importer.selectDevice(host.id); hostMenu = false },
                                )
                            }
                        }
                    }
                    if (s.deviceId == null) {
                        Text(if (s.hosts.isEmpty()) "Connect a desktop host to discover sessions." else "Keep a compatible computer online. Update Zerun on the host if needed.", style = MaterialTheme.typography.bodyMedium)
                    } else {
                        val host = s.hosts.firstOrNull { it.id == s.deviceId }
                        if (host?.online == false) Text("This computer is offline. Reconnect it to import.", color = MaterialTheme.colorScheme.error)
                        OutlinedTextField(
                            value = query, onValueChange = { query = it; importer.setQuery(it) },
                            label = { Text("Search sessions or folders") }, singleLine = true, modifier = Modifier.fillMaxWidth(),
                        )
                        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                            listOf(null to "All agents", "codex" to "Codex", "claude-code" to "Claude Code").forEach { (id, label) ->
                                FilterChip(selected = s.provider == id, onClick = { importer.setProvider(id) }, label = { Text(label) })
                            }
                        }
                    }
                }
            }
            if (s.error != null) item("error") {
                ImportNotice(s.error!!, error = true)
            }
            if (s.discoveryError != null) item("discovery-error") {
                ImportNotice(s.discoveryError!!, error = true)
                if (!s.running) TextButton(onClick = { if (s.hasMore) importer.loadMore() else importer.refresh() }, modifier = Modifier.padding(horizontal = 8.dp)) { Text("Retry discovery") }
            }
            s.sourceErrors.forEachIndexed { index, error -> item("source-error-$index") { ImportNotice(error, error = true) } }
            if (s.results.isNotEmpty()) {
                item("results-heading") {
                    val imported = s.results.count { it.phase == SessionImportPhase.IMPORTED || it.phase == SessionImportPhase.ALREADY_MANAGED }
                    Text("Import results · $imported of ${s.results.size} imported", style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(16.dp))
                }
                items(s.results, key = { "result-${it.sourceRef}" }) { result ->
                    ListItem(
                        headlineContent = { Text(result.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                        supportingContent = { Text(result.reason ?: when (result.phase) {
                            SessionImportPhase.PENDING -> "Waiting"
                            SessionImportPhase.IMPORTING -> "Importing…"
                            SessionImportPhase.IMPORTED -> "Imported · Archived"
                            SessionImportPhase.ALREADY_MANAGED -> "Already in Sessions"
                            SessionImportPhase.FAILED -> "Import failed"
                            SessionImportPhase.CANCELLED -> "Cancelled"
                        }, color = if (result.phase == SessionImportPhase.FAILED) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant) },
                    )
                }
                item("results-divider") { HorizontalDivider(Modifier.padding(vertical = 8.dp)) }
            }
            if (s.deviceId != null) item("selection") {
                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Not imported", style = MaterialTheme.typography.titleSmall, modifier = Modifier.weight(1f))
                    TextButton(enabled = !s.running && !s.loading, onClick = { importer.selectVisible() }) { Text("Select visible") }
                    TextButton(enabled = !s.running && !s.loading, onClick = { importer.refresh() }) { Text("Refresh") }
                }
                if (s.selectedCount > 0u) Text("${s.selectedCount} selected · up to ${s.selectionLimit} per import", style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 16.dp))
            }
            items(s.rows, key = { it.sourceRef }) { row ->
                val enabled = row.disabledReason == null && !s.running && !s.loading
                ListItem(
                    modifier = Modifier.toggleable(value = row.selected, enabled = enabled, role = Role.Checkbox, onValueChange = { importer.toggle(row.sourceRef) }),
                    leadingContent = { Checkbox(checked = row.selected, onCheckedChange = null, enabled = enabled) },
                    headlineContent = { Text(row.title.ifBlank { "Untitled session" }, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    supportingContent = {
                        Column {
                            Text(if (row.provider == "codex") "Codex" else "Claude Code", style = MaterialTheme.typography.labelMedium)
                            Text(row.cwd, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            row.disabledReason?.let { Text(it, color = MaterialTheme.colorScheme.error) }
                        }
                    },
                )
            }
            if (s.loading) item("loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator(Modifier.size(24.dp)) } }
            if (!s.loading && s.deviceId != null && s.rows.isEmpty()) item("empty") {
                ImportNotice(when {
                    s.query.isNotBlank() || s.provider != null -> "No matching unimported sessions. Try another search or agent."
                    s.hasMore -> "No unimported sessions on this page. Load more to keep looking."
                    s.discoveryError != null -> "Discovery could not finish. Try again."
                    else -> "No unimported sessions found. Imported sessions are available in Archived."
                })
            }
            if (s.hasMore) item("more") { TextButton(enabled = !s.loading && !s.running, onClick = { importer.loadMore() }, modifier = Modifier.fillMaxWidth()) { Text("Load more sessions") } }
        }
    }
}

@Composable
private fun ImportNotice(text: String, error: Boolean = false) {
    Text(text, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp), style = MaterialTheme.typography.bodyMedium,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
}
