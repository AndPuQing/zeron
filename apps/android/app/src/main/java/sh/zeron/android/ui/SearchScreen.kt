package sh.zeron.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import kotlinx.coroutines.delay
import kotlinx.coroutines.CancellationException
import uniffi.zeron_core.TranscriptSearchHit
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.unit.dp
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons

@Composable
fun SearchScreen(model: AppModel, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val client by model.client.collectAsState()
    val workspace by model.workspace.collectAsState()
    var query by remember { mutableStateOf("") }
    var messages by remember { mutableStateOf(false) }
    val hosts = workspace?.devices.orEmpty().filter { it.isExecutionHost }
    var host by remember { mutableStateOf<String?>(null) }
    val selectedHost = hosts.firstOrNull { it.id == host } ?: hosts.firstOrNull()
    var hostMenu by remember { mutableStateOf(false) }
    var hits by remember { mutableStateOf<List<TranscriptSearchHit>>(emptyList()) }
    var error by remember { mutableStateOf<String?>(null) }
    var loading by remember { mutableStateOf(false) }
    LaunchedEffect(client, query, messages, selectedHost?.id) {
        hits = emptyList(); error = null; loading = false
        val core = client
        val device = selectedHost?.id
        if (messages && query.isNotBlank() && core != null && device != null) {
            loading = true
            try {
                delay(250)
                do {
                    val page = core.searchTranscripts(device, query.trim())
                    hits = page.hits
                    if (page.indexing) delay(500)
                } while (page.indexing)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (e: Exception) { error = "Message search unavailable: ${e.message}" }
            finally { loading = false }
        }
    }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    var newSection by remember { mutableStateOf<String?>(null) }
    val actions = RowActions(
        model = model,
        onOpen = onOpen,
        archive = { model.archive(it.id) },
        sections = workspace?.front?.sections.orEmpty().map { it.id to it.name },
        newSection = { newSection = it },
    )
    val results = remember(query, workspace) {
        val q = query.trim()
        if (q.isEmpty()) workspace?.front?.recent.orEmpty() else client?.searchIncludingChildren(q, 60u)?.map { it.session }.orEmpty()
    }
    LazyColumn(
        Modifier.fillMaxSize().imePadding(),
        contentPadding = PaddingValues(bottom = 32.dp),
        verticalArrangement = Arrangement.spacedBy(ListItemDefaults.SegmentedGap),
    ) {
        item {
            Row(Modifier.statusBarsPadding().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                TonalCircleButton(ZIcons.Back, "Back", onClick = onBack)
                Spacer(Modifier.width(10.dp))
                // The search field: a full pill.
                Surface(shape = RoundedCornerShape(50), color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.weight(1f)) {
                    Row(Modifier.heightIn(min = 56.dp).padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                        ZIcon(ZIcons.Search, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.width(12.dp))
                        Box(Modifier.weight(1f)) {
                            if (query.isEmpty()) Text(if (messages) "Search message text" else "Sessions, projects, branches", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge)
                            BasicTextField(
                                query,
                                { query = it },
                                singleLine = true,
                                textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                                cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                                modifier = Modifier.fillMaxWidth().focusRequester(focus),
                            )
                        }
                    }
                }
            }
        }
        item {
            Row(Modifier.padding(horizontal = 20.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(messages, { messages = true }, label = { Text("Messages") })
                FilterChip(!messages, { messages = false }, label = { Text("Sessions") })
            }
            if (messages) {
                Box(Modifier.padding(horizontal = 20.dp)) {
                    TextButton(onClick = { hostMenu = true }) { Text(selectedHost?.name ?: "No computer available") }
                    DropdownMenu(hostMenu, { hostMenu = false }) {
                        hosts.forEach { device -> DropdownMenuItem(text = { Text(device.name) }, onClick = { host = device.id; hostMenu = false }) }
                    }
                }
                Text("Searches history stored on this computer. Unsynced history may be missing.", modifier = Modifier.padding(horizontal = 28.dp), style = MaterialTheme.typography.bodySmall)
                if (loading) Text("Searching…", Modifier.padding(28.dp))
                error?.let { Text(it, Modifier.padding(28.dp), color = MaterialTheme.colorScheme.error) }
            }
        }
        item {
            Text(
                if (messages) "${hits.size} message results" else if (query.isBlank()) "Recent" else "${results.size} results",
                style = MaterialTheme.typography.titleSmallEmphasized,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(start = 28.dp, top = 8.dp, bottom = 6.dp),
            )
        }
        if (messages) itemsIndexed(hits, key = { _, h -> "${h.chatId}:${h.messageId}" }) { _, hit ->
            Column(Modifier.fillMaxWidth().clickable {
                model.messageSearchTarget = hit.chatId to hit.messageId
                onOpen(hit.chatId)
            }.padding(horizontal = 28.dp, vertical = 12.dp)) {
                Text(model.row(hit.chatId)?.title ?: "Conversation", style = MaterialTheme.typography.titleSmall)
                Text((if (hit.fromUser) "You: " else "Agent: ") + hit.snippet, style = MaterialTheme.typography.bodyMedium)
            }
        }
        if (!messages) itemsIndexed(results, key = { _, r -> r.id }) { i, row ->
            Box(Modifier.padding(horizontal = 16.dp)) {
                SessionItem(row, i, results.size, actions)
            }
        }
    }
    newSection?.let { chatId ->
        TextPromptDialog("New section", "", "Create", ZIcons.Folder, onDismiss = { newSection = null }) { name ->
            model.createSection(name)?.let { id -> model.move(chatId, id) }
        }
    }
}
