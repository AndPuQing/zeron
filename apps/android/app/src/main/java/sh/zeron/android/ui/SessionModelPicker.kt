package sh.zeron.android.ui

import androidx.compose.runtime.*
import sh.zeron.android.design.ZIcons
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
    var policy by remember(c.chatId) { mutableStateOf(client.sessionConfig(c.chatId)) }
    var policyError by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(c.revision) { policy = client.sessionConfig(c.chatId) }
    val switches = canSwitchAgent(c)
    var providers by remember(client, c.host.deviceId) { mutableStateOf<List<HarnessInfo>>(emptyList()) }
    LaunchedEffect(client, c.host.deviceId, switches, current) {
        providers = if (switches) client.listHarnesses(c.host.deviceId).filter { it.offered && it.id != current } else emptyList()
    }
    val notice = when {
        c.live.turnRunning -> "You can switch agent when this turn ends."
        c.sendState == SendState.SENDING || c.sendState == SendState.QUEUED -> "You can switch agent after the pending message is delivered."
        !switches -> "Update this computer's Zerun to switch agent."
        else -> null
    }
    val available = listOf(current to (row?.harnessLabel ?: harnessLabel(current))) + providers.map { it.id to it.label }
    val tabs = available.map { (harness, label) ->
        ModelPickerTab(harness, label, footer = if (harness != current) "Your next message continues this chat with $label." else notice) {
            client.listModels(c.host.deviceId, harness).map { ModelPickerRow(it.id, it.label, it.description) }
        }
    }
    ModelPickerSheet(tabs, current, row?.model, host = c.host.name ?: "This computer", hostIcon = ZIcons.Laptop,
        settings = {
            ExecutionChips(client, current, c.host.deviceId, policy?.accessMode ?: "full-access", policy?.plan ?: false) { access, plan ->
                runCatching {
                    val next = checkNotNull(client.sessionConfig(c.chatId)).let { it.copy(accessMode = access ?: it.accessMode, plan = plan ?: it.plan) }
                    client.setSessionConfig(c.chatId, next)
                    policy = next
                }.onFailure { policyError = it.message }
            }
            policyError?.let { androidx.compose.material3.Text(it) }
        },
        onDismiss = onDismiss, onPick = { harness, picked ->
            // Catalog lookup can suspend. Check the current target and pending work again at commit time.
            if (harness != current) {
                check(client.listHarnesses(c.host.deviceId).any { it.id == harness && it.offered }) { "This agent is no longer enabled on the computer." }
            }
            val config = checkNotNull(client.sessionConfig(c.chatId)) { "This chat is no longer available." }
            val latest = client.openSession(c.chatId).composer()
            check(latest.host.deviceId == c.host.deviceId) { "This chat has moved to a different computer. Reopen the model picker." }
            check(harness == config.harness || canSwitchAgent(latest)) { "Wait for the pending turn or message before switching agent." }
            client.setSessionConfig(c.chatId, modelSelection(config, harness, picked.id))
        })
}
