package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Surface
import androidx.compose.material3.SwipeToDismissBox
import androidx.compose.material3.SwipeToDismissBoxValue
import androidx.compose.material3.Text
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.material3.rememberSwipeToDismissBoxState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.Color
import androidx.compose.foundation.layout.heightIn
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.LocalDarkTheme
import sh.zeron.android.design.ProjectColors
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.ChatIndicator
import uniffi.zeron_core.ConnectivityState
import uniffi.zeron_core.PullRequestState
import uniffi.zeron_core.SessionRow
import uniffi.zeron_core.WorkspaceSnapshot

private enum class Filter(val label: String) { All("All"), NeedsYou("Needs you"), Working("Working"), Pinned("Pinned"), Archived("Archived") }

/** "1 needs you" / "2 working" — the live summary beside New session. */
fun liveSummary(ws: WorkspaceSnapshot): String? {
    val (working, awaiting) = liveCounts(ws)
    return when {
        awaiting > 0 -> "$awaiting needs you"
        working > 0 -> "$working working"
        else -> null
    }
}

private fun frontRows(ws: WorkspaceSnapshot): List<SessionRow> {
    val seen = HashSet<String>()
    return (ws.front.pinned + ws.front.sections.flatMap { it.sessions } + ws.front.recent).filter { seen.add(it.id) }
}

private fun liveCounts(ws: WorkspaceSnapshot): Pair<Int, Int> {
    val rows = frontRows(ws)
    return rows.count { it.indicator == ChatIndicator.WORKING } to rows.count { it.indicator == ChatIndicator.AWAITING_INPUT }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SessionsScreen(model: AppModel, onOpen: (String) -> Unit, onImport: () -> Unit) {
    val workspace by model.workspace.collectAsState()
    val connectivity by model.connectivity.collectAsState()
    var filter by rememberSaveable { mutableStateOf(Filter.All) }
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    var refreshing by remember { mutableStateOf(false) }
    val pull = rememberPullToRefreshState()
    val ws = workspace
    val counts = remember(ws) { ws?.let { liveCounts(it) } ?: (0 to 0) }

    val subtitle = when {
        connectivity?.state == ConnectivityState.OFFLINE -> "Offline"
        connectivity?.state == ConnectivityState.RECONNECTING -> "Reconnecting…"
        model.isDemo -> "Demo workspace"
        else -> model.accountDetail.takeIf { it.isNotEmpty() }
    }

    val archive: (SessionRow) -> Unit = { row ->
        if (row.archived) model.unarchive(row.id) else model.archive(row.id)
        scope.launch {
            if (snackbar.showSnackbar(if (row.archived) "Unarchived" else "Archived", actionLabel = "Undo", withDismissAction = true) == SnackbarResult.ActionPerformed) {
                if (row.archived) model.archive(row.id) else model.unarchive(row.id)
            }
        }
    }

    val list = androidx.compose.foundation.lazy.rememberLazyListState()
    val density = androidx.compose.ui.platform.LocalDensity.current
    Box(Modifier.fillMaxSize()) {
        sh.zeron.android.design.WallpaperHero(
            model.wallpaper,
            scrollFade = {
                if (list.firstVisibleItemIndex > 0) 0f
                else 1f - list.firstVisibleItemScrollOffset / (with(density) { 400.dp.toPx() })
            },
        )
        PullToRefreshBox(
            isRefreshing = refreshing,
            onRefresh = {
                refreshing = true
                scope.launch {
                    model.refresh()
                    refreshing = false
                }
            },
            state = pull,
            modifier = Modifier.fillMaxSize(),
            indicator = {
                if (refreshing || pull.distanceFraction > 0f) {
                    Box(Modifier.align(Alignment.TopCenter).padding(WindowInsets.statusBars.asPaddingValues()).padding(top = 12.dp).size(32.dp),
                        contentAlignment = Alignment.Center) { sh.zeron.android.design.ActivityIndicator(Modifier.size(20.dp)) }
                }
            },
        ) {
            LazyColumn(
                state = list,
                contentPadding = PaddingValues(top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding() + 8.dp, bottom = 128.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                item("header") {
                    ScreenHeader("Sessions", subtitle) {
                        TonalCircleButton(ZIcons.ArrowDown, "Import", onClick = onImport)
                        if (connectivity?.state == ConnectivityState.OFFLINE) {
                            TonalCircleButton(
                                ZIcons.Offline, "Offline", onClick = { scope.launch { model.refresh() } },
                                container = MaterialTheme.colorScheme.errorContainer, content = MaterialTheme.colorScheme.onErrorContainer,
                            )
                        }
                    }
                }
                item("filters") {
                    Row(
                        Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        for (f in Filter.entries) {
                            Pill(
                                f.label,
                                filter == f,
                                onClick = { filter = f },
                                count = when (f) {
                                    Filter.NeedsYou -> counts.second
                                    Filter.Working -> counts.first
                                    else -> null
                                },
                            )
                        }
                    }
                }
                if (ws == null) return@LazyColumn
                val front = ws.front
                when (filter) {
                    Filter.All -> {
                        if (front.pinned.isNotEmpty()) group("pinned", "Pinned", front.pinned, model, onOpen, archive)
                        for (section in front.sections) {
                            group(section.id, section.name, section.sessions, model, onOpen, archive, collapsed = section.collapsed) {
                                model.setSectionCollapsed(section.id, !section.collapsed)
                            }
                        }
                        if (front.recent.isNotEmpty()) group("recent", "Recent", front.recent, model, onOpen, archive)
                    }
                    Filter.NeedsYou -> group("f", null, frontRows(ws).filter { it.indicator == ChatIndicator.AWAITING_INPUT }, model, onOpen, archive)
                    Filter.Working -> group("f", null, frontRows(ws).filter { it.indicator == ChatIndicator.WORKING }, model, onOpen, archive)
                    Filter.Pinned -> group("f", null, front.pinned, model, onOpen, archive)
                    Filter.Archived -> group("f", null, ws.archived, model, onOpen, archive)
                }
                val empty = when (filter) {
                    Filter.All -> front.pinned.isEmpty() && front.sections.isEmpty() && front.recent.isEmpty()
                    Filter.NeedsYou -> counts.second == 0
                    Filter.Working -> counts.first == 0
                    Filter.Pinned -> front.pinned.isEmpty()
                    Filter.Archived -> ws.archived.isEmpty()
                }
                if (empty) item("empty") { EmptyState(filter) }
            }
        }
        AppSnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 180.dp))
        StatusBarScrim(scrolled = list.firstVisibleItemIndex > 0 || list.firstVisibleItemScrollOffset > 0)
    }
}

private fun LazyListScope.group(
    id: String,
    title: String?,
    rows: List<SessionRow>,
    model: AppModel,
    onOpen: (String) -> Unit,
    archive: (SessionRow) -> Unit,
    collapsed: Boolean = false,
    onToggle: (() -> Unit)? = null,
) {
    if (title != null) {
        item("h-$id") {
            Row(
                Modifier.animateItem().fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 16.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(title, style = MaterialTheme.typography.titleSmallEmphasized, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.width(8.dp))
                Text("${rows.size}", style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Spacer(Modifier.weight(1f))
                if (onToggle != null) {
                    IconButton(onClick = onToggle, modifier = Modifier.size(32.dp)) {
                        ZIcon(if (collapsed) ZIcons.ChevronDown else ZIcons.ChevronUp, if (collapsed) "Expand" else "Collapse", Modifier.size(18.dp))
                    }
                }
            }
        }
    } else {
        item("h-$id") { Spacer(Modifier.height(8.dp)) }
    }
    if (collapsed) return
    itemsIndexed(rows, key = { _, r -> "$id/${r.id}" }) { i, row ->
        Box(Modifier.animateItem().padding(horizontal = 8.dp)) {
            SwipeableSessionRow(row, i, rows.size, model, onOpen, archive)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun SwipeableSessionRow(
    row: SessionRow,
    index: Int,
    count: Int,
    model: AppModel,
    onOpen: (String) -> Unit,
    archive: (SessionRow) -> Unit,
) {
    val state = rememberSwipeToDismissBoxState()
    val shape = RoundedCornerShape(16.dp)
    SwipeToDismissBox(
        state = state,
        enableDismissFromStartToEnd = false,
        onDismiss = { if (it == SwipeToDismissBoxValue.EndToStart) archive(row) },
        backgroundContent = {
            val revealing = state.dismissDirection == SwipeToDismissBoxValue.EndToStart
            Surface(Modifier.fillMaxSize(), shape = shape, color = if (revealing) MaterialTheme.colorScheme.tertiaryContainer else androidx.compose.ui.graphics.Color.Transparent) {
                Box(Modifier.padding(end = 28.dp), contentAlignment = Alignment.CenterEnd) {
                    if (revealing) ZIcon(ZIcons.Archive, if (row.archived) "Unarchive" else "Archive", tint = MaterialTheme.colorScheme.onTertiaryContainer)
                }
            }
        },
    ) {
        SessionItem(row, index, count, model, onOpen, archive)
    }
}

/** The list card colour: paper-white in light, raised in dark. */
@Composable
fun cardColor() = if (LocalDarkTheme.current) Color(0xFF111113) else Color.White

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SessionItem(
    row: SessionRow,
    index: Int,
    count: Int,
    model: AppModel,
    onOpen: (String) -> Unit,
    archive: (SessionRow) -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    var deleting by remember { mutableStateOf(false) }
    val haptics = LocalHapticFeedback.current
    Box {
        Row(
            Modifier.fillMaxWidth().testTag("session-${row.id}").clip(RoundedCornerShape(16.dp))
                .pressable(onLongClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                    menu = true
                }, onClick = { onOpen(row.id) })
                .heightIn(min = 62.dp).padding(horizontal = 12.dp, vertical = 9.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(14.dp),
        ) {
            HarnessMark(row.harness, 20.dp, tint = MaterialTheme.colorScheme.onSurface)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(row.title, style = if (row.unseen) MaterialTheme.typography.titleMediumEmphasized else MaterialTheme.typography.titleMedium,
                        maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                    StatusLabel(row)
                }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    Box(Modifier.weight(1f)) {
                        val parentId = row.parentChatId
                        if (parentId != null) {
                            Text("Side chat of ${model.row(parentId)?.title ?: "main chat"}",
                                style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1, overflow = TextOverflow.Ellipsis)
                        } else Subline(row)
                    }
                    row.pullRequest?.let { PrBadge(it.number, it.state) }
                }
            }
        }
        ActionMenu(
            menu,
            { menu = false },
            listOfNotNull(
                if (row.parentChatId == null) MenuAction(if (row.pinned) "Unpin" else "Pin", ZIcons.Pin) { model.setPinned(row.id, !row.pinned) } else null,
                MenuAction("Rename", ZIcons.Rename) { renaming = true },
                MenuAction(if (row.archived) "Unarchive" else "Archive", ZIcons.Archive) { archive(row) },
                MenuAction("Delete", ZIcons.Delete, destructive = true) { deleting = true },
            ),
        )
    }
    if (renaming) RenameDialog(row.title, onDismiss = { renaming = false }) { model.rename(row.id, it) }
    if (deleting) DeleteSessionDialog(model, row, onDismiss = { deleting = false }, onDeleted = { deleting = false })
}

/** Project monogram + name, then the branch — the desktop sidebar subline. */
@Composable
private fun Subline(row: SessionRow) {
    val muted = MaterialTheme.colorScheme.onSurfaceVariant
    Row(verticalAlignment = Alignment.CenterVertically) {
        val project = row.project
        if (project != null) {
            ProjectTile(project.name, project.colorIndex.toInt(), 14.dp)
            Spacer(Modifier.width(6.dp))
        }
        Text(
            project?.name ?: row.deviceName ?: "No project",
            style = MaterialTheme.typography.bodySmall,
            color = muted,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f, fill = false),
        )
        row.branch?.takeIf { it.isNotEmpty() }?.let { branch ->
            Spacer(Modifier.width(10.dp))
            ZIcon(ZIcons.Branch, null, Modifier.size(14.dp), tint = muted.copy(alpha = 0.7f))
            Spacer(Modifier.width(3.dp))
            Text(branch, style = MaterialTheme.typography.bodySmall, color = muted.copy(alpha = 0.7f), maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
fun PrBadge(number: ULong, state: PullRequestState) {
    val tone = when (state) {
        PullRequestState.OPEN -> successColor()
        PullRequestState.MERGED -> MaterialTheme.colorScheme.primary
        PullRequestState.CLOSED -> MaterialTheme.colorScheme.error
    }
    Surface(shape = RoundedCornerShape(8.dp), color = tone.copy(alpha = 0.12f), contentColor = tone) {
        Row(Modifier.padding(horizontal = 6.dp, vertical = 2.dp), verticalAlignment = Alignment.CenterVertically) {
            ZIcon(ZIcons.PullRequest, null, Modifier.size(13.dp))
            Spacer(Modifier.width(3.dp))
            Text("$number", style = MaterialTheme.typography.labelMediumEmphasized)
        }
    }
}

@Composable
fun RenameDialog(current: String, onDismiss: () -> Unit, onRename: (String) -> Unit) {
    var text by remember { mutableStateOf(current) }
    AppAlert(
        title = "Rename session", onDismiss = onDismiss,
        actions = listOf(
            AlertAction("Cancel", onClick = onDismiss),
            AlertAction("Rename", enabled = text.isNotBlank()) { onRename(text.trim()); onDismiss() },
        ),
    ) {
        AppTextField(text, { text = it }, singleLine = true, modifier = Modifier.fillMaxWidth())
    }
}

@Composable
private fun EmptyState(filter: Filter) {
    Column(Modifier.fillMaxWidth().padding(48.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        ZIcon(ZIcons.Chat, null, Modifier.size(40.dp), tint = MaterialTheme.colorScheme.outline)
        Spacer(Modifier.height(12.dp))
        Text(
            when (filter) {
                Filter.All -> "No sessions yet"
                Filter.NeedsYou -> "Nothing needs you"
                Filter.Working -> "No agents working"
                Filter.Pinned -> "Nothing pinned"
                Filter.Archived -> "Nothing archived"
            },
            style = MaterialTheme.typography.titleLarge,
        )
    }
}
