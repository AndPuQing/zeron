package sh.zeron.android.ui

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.ActivityIndicator
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
    AppSheet("Model · ${c.host.name ?: "This computer"}", onDismiss = onDismiss) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                val tabs = listOf(current to (row?.harnessLabel ?: harnessLabel(current))) + providers.map { it.id to it.label }
                tabs.forEach { (id, label) ->
                    AppTextButton(onClick = { selected = id; error = null }, enabled = !picking,
                        modifier = Modifier.testTag("model-provider-$id"),
                        color = if (selected == id) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant) {
                        HarnessMark(id, 16.dp)
                        Spacer(Modifier.width(6.dp))
                        Text(if (selected == id) "✓ $label" else label)
                    }
                }
            }
            if (!switches) Text(when {
                c.live.turnRunning -> "You can switch agent when this turn ends."
                c.sendState == SendState.SENDING || c.sendState == SendState.QUEUED -> "You can switch agent after the pending message is delivered."
                else -> "Update this computer's Zerun to switch agent."
            }, style = MaterialTheme.typography.bodySmall)
            if (selected != current) Text("Your next message continues this chat with ${harnessLabel(selected)}.", style = MaterialTheme.typography.bodySmall)
            error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            if (models == null) ActivityIndicator(Modifier.size(24.dp))
            else LazyColumn(Modifier.weight(1f, fill = false).heightIn(max = 380.dp)) {
                items(models.orEmpty(), key = { it.id }) { m ->
                    MenuRow(m.label, m.description, enabled = !picking,
                        selected = selected == current && m.id == (row?.model ?: models?.firstOrNull()?.id), selectable = true) {
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
