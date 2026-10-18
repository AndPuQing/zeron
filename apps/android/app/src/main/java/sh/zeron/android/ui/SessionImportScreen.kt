package sh.zeron.android.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
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
import sh.zeron.android.design.ActivityIndicator
import sh.zeron.android.design.GlassScene
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
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

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
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
    // Keep edits local: a delayed native snapshot must not replace new keystrokes.
    var query by rememberSaveable(s.deviceId) { mutableStateOf(s.query) }
    val workspace by model.workspace.collectAsState()
    LaunchedEffect(workspace?.devices) { importer.updateHosts() }
    var confirmClose by remember { mutableStateOf(false) }
    val close = { if (s.running) confirmClose = true else onClose() }
    BackHandler { close() }
    if (confirmClose) AppAlert("Stop importing?", onDismiss = { confirmClose = false }, actions = listOf(
        AlertAction("Keep importing") { confirmClose = false },
        AlertAction("Stop and close") { importer.dismiss(); onClose() },
    )) { Text("Completed imports will stay in Archived.") }

    var hostMenu by remember { mutableStateOf(false) }
    var agentMenu by remember { mutableStateOf(false) }
    val density = LocalDensity.current
    var headerHeight by remember { mutableStateOf(240.dp) }
    var footerHeight by remember { mutableStateOf(100.dp) }
    GlassScene(
        modifier = Modifier.fillMaxSize().imePadding(),
        background = { source ->
            LazyColumn(
                source.fillMaxSize().background(MaterialTheme.colorScheme.background),
                contentPadding = PaddingValues(top = headerHeight + 8.dp, bottom = footerHeight + 12.dp),
            ) {
                if (s.deviceId == null) item("no-host") {
                    ImportNotice(if (s.hosts.isEmpty()) "No computers connected" else "No compatible computer online")
                } else if (s.hosts.firstOrNull { it.id == s.deviceId }?.online == false) item("offline") {
                    ImportNotice("Computer offline", error = true)
                }
                s.error?.let { item("error") { ImportNotice(it, error = true) } }
                s.discoveryError?.let { error -> item("discovery-error") {
                    ImportNotice(error, error = true)
                    if (!s.running) TextButton(onClick = { if (s.hasMore) importer.loadMore() else importer.refresh() }, modifier = Modifier.padding(horizontal = 12.dp)) { Text("Retry discovery") }
                } }
                s.sourceErrors.forEachIndexed { index, error -> item("source-error-$index") { ImportNotice(error, error = true) } }
                if (s.results.isNotEmpty()) {
                    item("results-heading") {
                        val imported = s.results.count { it.phase == SessionImportPhase.IMPORTED || it.phase == SessionImportPhase.ALREADY_MANAGED }
                        SectionHeader("Import results · $imported of ${s.results.size} imported")
                    }
                    items(s.results, key = { "result-${it.sourceRef}" }) { result ->
                        val failed = result.phase == SessionImportPhase.FAILED
                        ListItem(
                            colors = ListItemDefaults.colors(containerColor = Color.Transparent),
                            leadingContent = { ZIcon(when (result.phase) {
                                SessionImportPhase.IMPORTED, SessionImportPhase.ALREADY_MANAGED -> ZIcons.Check
                                SessionImportPhase.FAILED -> ZIcons.Warning
                                SessionImportPhase.CANCELLED -> ZIcons.Close
                                else -> ZIcons.Queue
                            }, null, Modifier.size(20.dp), tint = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant) },
                            headlineContent = { Text(result.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                            supportingContent = { Text(result.reason ?: when (result.phase) {
                                SessionImportPhase.PENDING -> "Waiting"
                                SessionImportPhase.IMPORTING -> "Importing…"
                                SessionImportPhase.IMPORTED -> "Imported · Archived"
                                SessionImportPhase.ALREADY_MANAGED -> "Already imported"
                                SessionImportPhase.FAILED -> "Import failed"
                                SessionImportPhase.CANCELLED -> "Cancelled"
                            }, color = if (failed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant) },
                        )
                    }
                }
                if (s.deviceId != null) item("selection") {
                    SectionHeader("Not imported", trailing = {
                        TextButton(enabled = !s.running && !s.loading, onClick = { importer.selectVisible() }) { Text("Select visible") }
                    })
                }
                items(s.rows, key = { it.sourceRef }) { row ->
                    val enabled = row.disabledReason == null && !s.running && !s.loading
                    Row(
                        Modifier.padding(horizontal = 12.dp).fillMaxWidth().clip(RoundedCornerShape(16.dp))
                            .toggleable(value = row.selected, enabled = enabled, role = Role.Checkbox, onValueChange = { importer.toggle(row.sourceRef) })
                            .padding(horizontal = 12.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(14.dp),
                    ) {
                        HarnessMark(row.provider, 20.dp, Modifier.alpha(if (row.disabledReason == null) 1f else 0.45f)
                            .semantics { contentDescription = agentLabel(row.provider) })
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                            Text(row.title.ifBlank { "Untitled session" }, style = MaterialTheme.typography.bodyLargeEmphasized,
                                color = if (row.disabledReason == null) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 2, overflow = TextOverflow.Ellipsis)
                            Text(row.cwd, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            row.disabledReason?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        }
                        val check = if (row.selected) MaterialTheme.colorScheme.primary else Color.Transparent
                        Box(Modifier.size(22.dp).clip(CircleShape).background(check)
                            .border(if (row.selected) 0.dp else 1.dp, MaterialTheme.colorScheme.outlineVariant, CircleShape)
                            .alpha(if (row.disabledReason == null) 1f else 0.35f), contentAlignment = Alignment.Center) {
                            if (row.selected) ZIcon(ZIcons.Check, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onPrimary)
                        }
                    }
                }
                if (s.loading) item("loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { ActivityIndicator(Modifier.size(24.dp)) } }
                if (!s.loading && s.deviceId != null && s.rows.isEmpty()) item("empty") {
                    ImportNotice(when {
                        s.query.isNotBlank() || s.provider != null -> "No matching sessions"
                        s.hasMore -> "No unimported sessions on this page"
                        s.discoveryError != null -> "Discovery failed"
                        else -> "No unimported sessions"
                    })
                }
                if (s.hasMore) item("more") {
                    Box(Modifier.fillMaxWidth().padding(12.dp), contentAlignment = Alignment.Center) {
                        TextButton(enabled = !s.loading && !s.running, onClick = { importer.loadMore() }) { Text("Load more sessions") }
                    }
                }
            }
        },
    ) {
        Column(Modifier.align(Alignment.TopCenter).fillMaxWidth()
            .onSizeChanged { headerHeight = with(density) { it.height.toDp() } }
            .statusBarsPadding().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(Modifier.fillMaxWidth().padding(vertical = 8.dp), verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Import sessions", style = MaterialTheme.typography.headlineLarge, modifier = Modifier.weight(1f))
                TonalCircleButton(ZIcons.Refresh, "Refresh", onClick = { if (!s.running && !s.loading) importer.refresh() })
                TonalCircleButton(ZIcons.Close, "Close", onClick = close)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Box(Modifier.weight(1f)) {
                    ImportPicker(s.hosts.firstOrNull { it.id == s.deviceId }?.name ?: "Computer",
                        enabled = !s.running && s.hosts.isNotEmpty(), onClick = { hostMenu = true },
                        leading = { ZIcon(ZIcons.Laptop, null, Modifier.size(18.dp)) })
                    AppMenuPopup(hostMenu, { hostMenu = false }) {
                        s.hosts.forEachIndexed { index, host ->
                            if (index > 0) Hairline()
                            MenuRow(host.name, supporting = when { !host.supported -> "Update required"; !host.online -> "Offline"; else -> null },
                                enabled = host.online && host.supported, selected = host.id == s.deviceId, selectable = true,
                                leading = { ZIcon(ZIcons.Laptop, null, Modifier.size(20.dp)) },
                                trailing = { if (host.id == s.deviceId) ZIcon(ZIcons.Check, null, Modifier.size(18.dp)) }) {
                                importer.selectDevice(host.id); hostMenu = false
                            }
                        }
                    }
                }
                if (s.deviceId != null) Box(Modifier.weight(1f)) {
                    ImportPicker(agentLabel(s.provider), enabled = true, onClick = { agentMenu = true }, leading = {
                        if (s.provider == null) ZIcon(ZIcons.Context, null, Modifier.size(18.dp)) else HarnessMark(s.provider, 18.dp)
                    })
                    ChoiceMenu(agentMenu, { agentMenu = false }, listOf(MenuSection(null,
                        listOf<String?>(null, "codex", "claude-code", "dsh", "pi", "opencode").map { id ->
                            MenuChoice(agentLabel(id), selected = s.provider == id, leading = {
                                if (id == null) ZIcon(ZIcons.Context, null, Modifier.size(20.dp)) else HarnessMark(id, 20.dp)
                            }) { importer.setProvider(id) }
                        },
                    )))
                }
            }
            if (s.deviceId != null) GlassSurface(modifier = Modifier.fillMaxWidth()) {
                Row(Modifier.heightIn(min = 48.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    ZIcon(ZIcons.Search, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(12.dp))
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) Text("Search sessions", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                        BasicTextField(query, { query = it; importer.setQuery(it) }, singleLine = true,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary), modifier = Modifier.fillMaxWidth())
                    }
                }
            }
            Spacer(Modifier.height(0.dp))
        }
        Column(Modifier.align(Alignment.BottomCenter).fillMaxWidth()
            .onSizeChanged { footerHeight = with(density) { it.height.toDp() } }
            .navigationBarsPadding().padding(horizontal = 16.dp, vertical = 12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (s.running) GlassSurface(shape = RoundedCornerShape(26.dp), modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    val done = s.results.count { it.phase != SessionImportPhase.PENDING && it.phase != SessionImportPhase.IMPORTING }
                    LinearProgressIndicator(progress = { if (s.results.isEmpty()) 0f else done.toFloat() / s.results.size }, modifier = Modifier.fillMaxWidth())
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("$done of ${s.results.size} finished", modifier = Modifier.weight(1f))
                        TextButton(enabled = !s.cancelling, onClick = { importer.cancel() }) { Text(if (s.cancelling) "Stopping…" else "Stop import") }
                    }
                }
            } else {
                if (s.results.any { it.retryable }) ImportAction("Retry remaining", ZIcons.Refresh, enabled = !s.loading, onClick = { importer.start(retry = true) })
                ImportAction(if (s.selectedCount == 0u) "Import" else "Import ${s.selectedCount} selected", ZIcons.ArrowDown,
                    enabled = s.selectedCount > 0u && !s.loading, primary = true, onClick = { importer.start(retry = false) })
            }
        }
    }
}

@Composable
private fun ImportPicker(label: String, enabled: Boolean, onClick: () -> Unit, leading: @Composable () -> Unit) {
    GlassSurface(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.heightIn(min = 44.dp).padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            leading()
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            ZIcon(ZIcons.ChevronDown, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ImportAction(label: String, icon: Int, enabled: Boolean, primary: Boolean = false, onClick: () -> Unit) {
    val tone = if (primary && enabled) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
    GlassSurface(onClick = onClick, enabled = enabled, modifier = Modifier.fillMaxWidth(),
        tint = if (primary && enabled) tone.copy(alpha = 0.14f) else Color.Transparent, contentColor = tone) {
        Row(Modifier.fillMaxWidth().heightIn(min = 52.dp).padding(horizontal = 20.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp, Alignment.CenterHorizontally), verticalAlignment = Alignment.CenterVertically) {
            ZIcon(icon, null, Modifier.size(20.dp))
            Text(label, style = MaterialTheme.typography.bodyLargeEmphasized)
        }
    }
}

private fun agentLabel(provider: String?) = when (provider) {
    "codex" -> "Codex"
    "claude-code" -> "Claude Code"
    "dsh" -> "DeepSeek Harness"
    "pi" -> "Pi"
    "opencode" -> "OpenCode"
    else -> "All agents"
}

@Composable
private fun ImportNotice(text: String, error: Boolean = false) {
    Text(text, modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp), style = MaterialTheme.typography.bodyMedium,
        color = if (error) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant)
}
