package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.testTagsAsResourceId
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.ActivityIndicator
import sh.zeron.android.design.LocalDarkTheme
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.glassControlFill
import uniffi.zeron_core.*

internal fun canSwitchAgent(c: ComposerState) = c.host.capabilities.providerSwitch &&
    !c.live.turnRunning && c.sendState != SendState.SENDING && c.sendState != SendState.QUEUED

/** Preserve the current provider's settings, but never carry them into a different provider. */
internal fun modelSelection(config: ChatConfig, harness: String, model: String): ChatConfig =
    if (harness == config.harness) config.copy(model = model)
    else config.copy(harness = harness, model = model, reasoning = null, modelOptions = emptyMap())

@Composable
internal fun SessionModelPicker(client: CoreClient, c: ComposerState, row: SessionRow?, onDismiss: () -> Unit) {
    val current = row?.harness ?: "claude-code"
    var selected by remember { mutableStateOf(current) }
    var providers by remember { mutableStateOf<List<HarnessInfo>>(emptyList()) }
    var models by remember(selected) { mutableStateOf<List<ModelInfo>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var picking by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val switches = canSwitchAgent(c)
    LaunchedEffect(client, c.host.deviceId, switches, current) {
        providers = if (switches) client.listHarnesses(c.host.deviceId).filter { it.offered && it.id != current } else emptyList()
        if (!switches) selected = current
    }
    LaunchedEffect(client, c.host.deviceId, selected) {
        models = client.listModels(c.host.deviceId, selected)
    }
    val tabs = listOf(current to (row?.harnessLabel ?: harnessLabel(current))) + providers.map { it.id to it.label }
    ModelPickerSheet(c.host.name ?: "This computer", onDismiss) {
        if (tabs.size > 1) {
            ModelProviderTabs(tabs, selected, enabled = !picking) { selected = it; error = null }
            Spacer(Modifier.height(10.dp))
        }
        val notice = when {
            !switches -> when {
                c.live.turnRunning -> "You can switch agent when this turn ends."
                c.sendState == SendState.SENDING || c.sendState == SendState.QUEUED -> "You can switch agent after the pending message is delivered."
                else -> "Update this computer's Zerun to switch agent."
            }
            selected != current -> "Your next message continues this chat with ${harnessLabel(selected)}."
            else -> null
        }
        if (notice != null) {
            Row(Modifier.padding(horizontal = 12.dp).fillMaxWidth().clip(RoundedCornerShape(14.dp))
                .background(glassControlFill()).padding(12.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                ZIcon(ZIcons.Info, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Text(notice, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Spacer(Modifier.height(10.dp))
        }
        error?.let { Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(horizontal = 26.dp, vertical = 8.dp)) }
        if (models == null) {
            Box(Modifier.fillMaxWidth().height(160.dp), contentAlignment = Alignment.Center) { ActivityIndicator(Modifier.size(24.dp)) }
        } else if (models.isNullOrEmpty()) {
            Box(Modifier.fillMaxWidth().height(160.dp), contentAlignment = Alignment.Center) {
                Text("No models available", color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        } else {
            key(selected) {
                LazyColumn(Modifier.weight(1f, fill = false).testTag("model-picker-list-$selected"),
                    contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 20.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    items(models.orEmpty(), key = { it.id }) { m ->
                        ModelPickerRow(m, enabled = !picking,
                            isCurrent = selected == current && m.id == (row?.model ?: models?.firstOrNull()?.id)) {
                            val pickedHarness = selected
                            picking = true
                            scope.launch {
                                try {
                                    // Catalog lookup can suspend. Read live state after it, immediately before applying.
                                    if (pickedHarness != current) {
                                        check(client.listHarnesses(c.host.deviceId).any { it.id == pickedHarness && it.offered }) { "This agent is no longer enabled on the computer." }
                                    }
                                    val config = checkNotNull(client.sessionConfig(c.chatId)) { "This chat is no longer available." }
                                    val latest = client.openSession(c.chatId).composer()
                                    check(latest.host.deviceId == c.host.deviceId) { "This chat has moved to a different computer. Reopen the model picker." }
                                    check(pickedHarness == config.harness || canSwitchAgent(latest)) { "Wait for the pending turn or message before switching agent." }
                                    client.setSessionConfig(c.chatId, modelSelection(config, pickedHarness, m.id))
                                    onDismiss()
                                } catch (cancelled: CancellationException) { throw cancelled }
                                catch (failure: Exception) { error = failure.message ?: "Couldn't change the model." }
                                finally { picking = false }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** This picker has the iOS model sheet's two-line header, rather than a navigation bar. */
@Composable
private fun ModelPickerSheet(host: String, onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        sheetMaxWidth = 560.dp,
        shape = RoundedCornerShape(topStart = 34.dp, topEnd = 34.dp),
        // A modal lives in a separate window: use tonal material without sampling the page's backdrop.
        containerColor = if (LocalDarkTheme.current) MaterialTheme.colorScheme.surfaceContainerHighest else MaterialTheme.colorScheme.surfaceContainerLow,
        contentColor = MaterialTheme.colorScheme.onSurface,
        tonalElevation = 0.dp,
        dragHandle = {
            Box(Modifier.padding(top = 8.dp).size(44.dp, 5.dp).clip(CircleShape)
                .background(MaterialTheme.colorScheme.onSurface.copy(alpha = 0.2f)))
        },
    ) {
        Column(Modifier.fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp * 0.78f).dp)
            .semantics { testTagsAsResourceId = true }) {
            Row(Modifier.fillMaxWidth().padding(start = 24.dp, end = 18.dp, top = 16.dp, bottom = 20.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Text("Model", style = MaterialTheme.typography.headlineSmall)
                    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        ZIcon(ZIcons.Monitor, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Text(host, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
                PlainIconButton(ZIcons.Close, "Close model picker", onDismiss,
                    modifier = Modifier.background(glassControlFill(), CircleShape))
            }
            content()
        }
    }
}

/** Keep names when they fit; on a crowded host, only the selected agent keeps its label. */
@Composable
private fun ModelProviderTabs(tabs: List<Pair<String, String>>, selected: String, enabled: Boolean, onSelect: (String) -> Unit) {
    val style = MaterialTheme.typography.titleSmall.copy(fontSize = 14.sp)
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val labels = tabs.map { (_, label) -> if (label == "Claude Code") "Claude" else label }
    val natural = labels.map { label -> with(density) { measurer.measure(label, style, softWrap = false).size.width.toDp() } + 55.dp }
    BoxWithConstraints(Modifier.padding(horizontal = 20.dp).fillMaxWidth().clip(CircleShape).background(glassControlFill())) {
        val available = maxWidth - 6.dp
        val compact = natural.map { it.value }.sum() > available.value
        val widths = tabs.indices.map { if (compact && tabs[it].first != selected) 48.dp else natural[it].coerceAtLeast(48.dp) }
        val extra = ((available.value - widths.map { it.value }.sum()) / tabs.size).coerceAtLeast(0f).dp
        Row(Modifier.horizontalScroll(rememberScrollState()).padding(3.dp), verticalAlignment = Alignment.CenterVertically) {
            tabs.forEachIndexed { index, (id, label) ->
                val active = id == selected
                val bringIntoView = remember(id) { BringIntoViewRequester() }
                LaunchedEffect(active) { if (active) bringIntoView.bringIntoView() }
                val fill = if (!active) Color.Transparent else if (LocalDarkTheme.current) Color.White.copy(alpha = 0.14f) else Color.White
                Row(Modifier.width(widths[index] + extra).heightIn(min = 48.dp).bringIntoViewRequester(bringIntoView)
                    .testTag("model-provider-$id").clip(CircleShape).background(fill)
                    .pressable(enabled, role = Role.Tab) { onSelect(id) }
                    .semantics { contentDescription = label; this.selected = active },
                    horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
                    HarnessMark(id, 18.dp, Modifier.graphicsLayer { alpha = if (!enabled) 0.3f else if (active) 1f else 0.55f })
                    if (!compact || active) {
                        Spacer(Modifier.width(7.dp))
                        Text(labels[index], style = style,
                            color = if (active) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1)
                    }
                }
            }
        }
    }
}

@Composable
private fun ModelPickerRow(model: ModelInfo, enabled: Boolean, isCurrent: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().testTag("model-picker-row-${model.id}").clip(RoundedCornerShape(18.dp))
        .background(if (isCurrent) MaterialTheme.colorScheme.primary.copy(alpha = 0.14f) else Color.Transparent)
        .pressable(enabled, role = Role.RadioButton, onClick = onClick).semantics { selected = isCurrent }
        .heightIn(min = 62.dp).padding(start = 14.dp, end = 16.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
            Text(model.label, style = MaterialTheme.typography.titleMedium.copy(fontSize = 16.sp),
                color = MaterialTheme.colorScheme.onSurface.copy(alpha = if (enabled) 1f else 0.4f))
            model.description?.takeIf { it.isNotBlank() }?.let {
                Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis,
                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = if (enabled) 1f else 0.4f))
            }
        }
        if (isCurrent) ZIcon(ZIcons.Check, null, Modifier.size(20.dp), tint = MaterialTheme.colorScheme.primary)
        else Spacer(Modifier.width(20.dp))
    }
}
