package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.isActive
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.TranscriptSearchHit

@Composable
fun SearchScreen(model: AppModel, onBack: () -> Unit, onOpen: (String) -> Unit) {
    val client by model.client.collectAsState()
    val workspace by model.workspace.collectAsState()
    var query by remember { mutableStateOf("") }
    var messages by remember { mutableStateOf(false) }
    val hosts = workspace?.devices.orEmpty().filter { it.isExecutionHost }
    var host by remember { mutableStateOf<String?>(null) }
    val selectedHost = hosts.firstOrNull { it.id == host } ?: hosts.firstOrNull { it.online } ?: hosts.firstOrNull()
    var hostMenu by remember { mutableStateOf(false) }
    var hits by remember { mutableStateOf<List<TranscriptSearchHit>>(emptyList()) }
    var error by remember { mutableStateOf(false) }
    var loading by remember { mutableStateOf(false) }
    var indexing by remember { mutableStateOf(false) }
    var retry by remember { mutableIntStateOf(0) }
    LaunchedEffect(client, query, messages, selectedHost?.id, selectedHost?.online, retry) {
        hits = emptyList(); error = false; loading = false; indexing = false
        val core = client
        val device = selectedHost?.id
        if (messages && query.isNotBlank() && core != null && device != null && selectedHost?.online == true) {
            loading = true
            try {
                delay(250)
                do {
                    val page = core.searchTranscripts(device, query.trim())
                    currentCoroutineContext().ensureActive()
                    hits = page.hits
                    indexing = page.indexing
                    if (page.indexing) delay(500)
                } while (page.indexing)
            } catch (cancelled: CancellationException) { throw cancelled }
            catch (_: Exception) { error = true }
            finally { if (currentCoroutineContext().isActive) { loading = false; indexing = false } }
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
    val results = remember(query, workspace, client) {
        val q = query.trim()
        if (q.isEmpty()) workspace?.front?.recent.orEmpty() else client?.searchIncludingChildren(q, 60u)?.map { it.session }.orEmpty()
    }
    Column(Modifier.fillMaxSize().imePadding()) {
        Row(Modifier.statusBarsPadding().padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            TonalCircleButton(ZIcons.Back, "Back", onClick = onBack)
            Spacer(Modifier.width(10.dp))
            Surface(shape = RoundedCornerShape(50), color = MaterialTheme.colorScheme.surfaceContainerHigh, modifier = Modifier.weight(1f)) {
                Row(Modifier.heightIn(min = 56.dp).padding(start = 16.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    ZIcon(ZIcons.Search, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.width(10.dp))
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) Text(if (messages) "Search messages" else "Search sessions", color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                        BasicTextField(
                            query, { query = it }, singleLine = true,
                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                            cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                            modifier = Modifier.fillMaxWidth().focusRequester(focus).semantics { contentDescription = if (messages) "Search messages" else "Search sessions" },
                        )
                    }
                    if (query.isNotEmpty()) IconButton(onClick = { query = "" }) { ZIcon(ZIcons.Close, "Clear search", Modifier.size(18.dp)) }
                    else Spacer(Modifier.width(12.dp))
                }
            }
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FilterChip(!messages, { messages = false }, label = { Text("Sessions") }, modifier = Modifier.weight(1f))
            FilterChip(messages, { messages = true }, label = { Text("Messages") }, modifier = Modifier.weight(1f))
        }
        if (messages) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 20.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Search in", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Box(Modifier.weight(1f)) {
                    TextButton(onClick = { hostMenu = true }, enabled = hosts.isNotEmpty()) {
                        ZIcon(ZIcons.Laptop, null, Modifier.size(16.dp))
                        Spacer(Modifier.width(6.dp))
                        Text(selectedHost?.name ?: "No computer", maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
                        Spacer(Modifier.width(6.dp))
                        ZIcon(ZIcons.ChevronDown, null, Modifier.size(14.dp))
                    }
                    DropdownMenu(hostMenu, { hostMenu = false }) {
                        hosts.forEach { device -> DropdownMenuItem(text = { Text(device.name) }, leadingIcon = {
                            Box(Modifier.size(8.dp).background(if (device.online) successColor() else MaterialTheme.colorScheme.outline, CircleShape)
                                .semantics { contentDescription = if (device.online) "Online" else "Offline" })
                        }, trailingIcon = { if (device.id == selectedHost?.id) ZIcon(ZIcons.Check, null, Modifier.size(18.dp)) }, onClick = { host = device.id; hostMenu = false }) }
                    }
                }
            }
            Text("Saved history on this computer", Modifier.padding(horizontal = 20.dp, vertical = 2.dp), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        LazyColumn(Modifier.weight(1f).fillMaxWidth(), contentPadding = PaddingValues(top = 12.dp, bottom = 32.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (messages) {
                item("message-status") {
                    when {
                        selectedHost == null -> SearchNotice("Connect a computer", "Message search uses the history saved on a connected computer.")
                        !selectedHost.online -> SearchNotice("Computer is unavailable", "Reconnect ${selectedHost.name} or select another computer.")
                        error -> SearchNotice("Couldn't search messages", "Check that this computer is connected and running the latest Zerun.", onRetry = { retry++ })
                        query.isBlank() -> SearchNotice("Find a message", "Search words or phrases from your conversations. Tap a result to jump to the message.")
                        loading -> Row(Modifier.padding(horizontal = 24.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                            Text(if (indexing) "Searching saved history…" else "Searching…", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        hits.isEmpty() -> SearchNotice("No matching messages", "Try fewer words or choose another computer. History that hasn't synced may be missing.")
                        else -> Text("${hits.size} ${if (hits.size == 1) "message" else "messages"}", Modifier.padding(horizontal = 24.dp), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                itemsIndexed(hits, key = { _, h -> "${h.chatId}:${h.messageId}" }) { _, hit ->
                    Surface(onClick = { model.messageSearchTarget = hit.chatId to hit.messageId; onOpen(hit.chatId) }, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp), shape = RoundedCornerShape(16.dp), color = MaterialTheme.colorScheme.surfaceContainerLow) {
                        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Text(model.row(hit.chatId)?.title ?: "Conversation", style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                                ZIcon(ZIcons.ChevronRight, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                            Text(if (hit.fromUser) "You" else "Agent", style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                            Text(highlightSearchSnippet(hit.snippet, query), style = MaterialTheme.typography.bodyMedium, maxLines = 4, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
            } else {
                item { Text(if (query.isBlank()) "Recent" else "${results.size} results", Modifier.padding(horizontal = 24.dp), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                itemsIndexed(results, key = { _, r -> r.id }) { i, row -> Box(Modifier.padding(horizontal = 16.dp)) { SessionItem(row, i, results.size, actions) } }
            }
        }
    }
    newSection?.let { chatId ->
        TextPromptDialog("New section", "", "Create", ZIcons.Folder, onDismiss = { newSection = null }) { name -> model.createSection(name)?.let { id -> model.move(chatId, id) } }
    }
}

@Composable
private fun SearchNotice(title: String, detail: String, onRetry: (() -> Unit)? = null) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 28.dp, vertical = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Text(detail, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        onRetry?.let { TextButton(onClick = it) { Text("Try again") } }
    }
}

private fun highlightSearchSnippet(snippet: String, query: String) = buildAnnotatedString {
    append(snippet)
    val needle = query.trim()
    if (needle.isNotEmpty()) {
        var start = snippet.indexOf(needle, ignoreCase = true)
        while (start >= 0) {
            addStyle(SpanStyle(fontWeight = FontWeight.SemiBold), start, start + needle.length)
            start = snippet.indexOf(needle, start + needle.length, ignoreCase = true)
        }
    }
}
