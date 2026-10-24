package sh.zeron.android.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.core.AgentTarget
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.ActivityIndicator
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.*
import kotlin.math.roundToInt

@Composable
fun ProvidersScreen(model: AppModel, initialDevice: String?, initialHarness: String?, signIn: Boolean, onBack: () -> Unit, embedded: Boolean = false) {
    val client by model.client.collectAsState()
    val workspace by model.workspace.collectAsState()
    val hosts = workspace?.devices.orEmpty().filter { it.isExecutionHost }
    var device by rememberSaveable { mutableStateOf(initialDevice ?: model.providersDevice) }
    var harness by rememberSaveable { mutableStateOf(initialHarness) }
    var choosing by remember { mutableStateOf(false) }
    val host = hosts.firstOrNull { it.id == device }
    LaunchedEffect(hosts.map { it.id }) {
        if (device == null || (initialDevice == null && host == null)) {
            device = (hosts.firstOrNull { it.online } ?: hosts.firstOrNull())?.id
        }
    }
    LaunchedEffect(device) { if (device != null) model.providersDevice = device }
    val back = {
        if (harness != null && initialHarness == null) harness = null else onBack()
    }
    BackHandler(onBack = back)
    val id = device
    val selected = harness
    val core = client
    Column(Modifier.fillMaxSize().then(if (embedded) Modifier else Modifier.statusBarsPadding().navigationBarsPadding()).imePadding()) {
        if (!embedded || selected != null) {
            ScreenHeader(if (selected == null) "Providers" else harnessLabel(selected), host?.name) {
                TonalCircleButton(ZIcons.Back, "Back", onClick = back)
            }
        }
        if (id == null || core == null) {
            Text("No computers yet", style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(24.dp))
            Text("Install Zerun on a computer to run agents and manage their accounts here.", modifier = Modifier.padding(horizontal = 24.dp))
        } else if (selected != null) {
            key(core, id, selected) {
                AgentAccounts(model, core, AgentTarget(id, host?.name ?: "this computer", selected, harnessLabel(selected)), host?.online == true, signIn)
            }
        } else {
            Box(Modifier.padding(horizontal = 20.dp)) {
                GroupedRow(0, 1, onClick = { if (hosts.isNotEmpty()) choosing = true },
                    modifier = Modifier.testTag("providers-device"),
                    leadingContent = { IconTile(ZIcons.Laptop) },
                    supportingContent = { Text(if (host?.online == true) "Online" else "Offline") },
                    trailingContent = { ZIcon(ZIcons.ChevronDown, null, Modifier.size(14.dp)) },
                ) { Text(host?.name ?: "Computer unavailable") }
                ChoiceMenu(choosing, { choosing = false }, listOf(MenuSection("Device", hosts.map { h ->
                    MenuChoice(h.name, h.id == id, if (h.online) null else "Offline") { device = h.id }
                })))
            }
            key(core, id) {
                ProviderList(core, id, host?.name ?: "this computer", host?.online == true) { harness = it }
            }
        }
    }
}

@Composable
private fun ProviderList(client: CoreClient, device: String, name: String, online: Boolean, onAccounts: (String) -> Unit) {
    val scope = rememberCoroutineScope()
    var list by remember { mutableStateOf<List<HarnessInfo>?>(null) }
    var accounts by remember { mutableStateOf<List<DeviceAccountInfo>?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    var revision by remember { mutableIntStateOf(0) }
    LaunchedEffect(client, device, online, revision) {
        if (!online) return@LaunchedEffect
        try {
            list = client.listHarnesses(device)
            accounts = client.deviceAgentAccounts(device)
            error = null
        }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "Couldn't load accounts." }
    }
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        item { Text("Agents", style = MaterialTheme.typography.labelLarge) }
        if (list == null) item { Text(if (online) "Loading agents…" else "$name is offline.") }
        item {
            val rows = list.orEmpty().sortedWith(compareBy<HarnessInfo> { !it.installed }.thenBy { it.label })
            val enabled = rows.count { it.offered }
            Column {
                rows.forEachIndexed { index, h ->
                    val rules = agentLoginRules(h.id)
                    val mine = accounts.orEmpty().filter { it.harness == h.id }
                    val active = mine.firstOrNull { it.active }
                    val summary = when {
                        !h.installed -> "Not installed"
                        !online -> "Offline"
                        accounts == null -> error ?: "Installed"
                        active != null -> accountTitle(active) + if (mine.size > 1) " · ${mine.size} accounts" else ""
                        mine.isNotEmpty() -> "${mine.size} accounts"
                        rules.signsIn -> "No account"
                        else -> "Installed"
                    }
                    GroupedRow(index, rows.size,
                        onClick = if (h.installed && (rules.signsIn || mine.isNotEmpty())) ({ onAccounts(h.id) }) else null,
                        modifier = Modifier.testTag("provider-${h.id}"),
                        leadingContent = { HarnessMark(h.id, 22.dp) },
                        supportingContent = { Text(summary) },
                        trailingContent = {
                            if (busy == h.id) ActivityIndicator(Modifier.size(20.dp))
                            else Switch(h.offered, enabled = online && h.installed && busy == null && !(h.offered && enabled <= 1),
                                modifier = Modifier.testTag("provider-toggle-${h.id}"), onCheckedChange = { on ->
                                    busy = h.id
                                    scope.launch {
                                        try { list = client.setHarnessEnabled(device, h.id, on); error = null }
                                        catch (cancelled: CancellationException) { throw cancelled }
                                        catch (failure: Exception) { error = failure.message ?: "Couldn't update the agent." }
                                        finally { busy = null }
                                    }
                                })
                        },
                    ) { Text(h.label) }
                }
            }
        }
        error?.let { message -> item { Text(message, color = MaterialTheme.colorScheme.error) } }
        item { AppTextButton(onClick = { revision++ }, enabled = online && busy == null) { Text("Refresh") } }
        item { Text("Agents and their logins live on $name. Open an agent to sign in or switch accounts.", style = MaterialTheme.typography.bodySmall) }
    }
}

private fun accountTitle(a: DeviceAccountInfo) = a.email ?: a.displayName ?: if (a.apiKey) "API key" else "Account"

@Composable
private fun AgentAccounts(model: AppModel, client: CoreClient, target: AgentTarget, online: Boolean, autoSignIn: Boolean) {
    val scope = rememberCoroutineScope()
    val rules = remember(target.harness) { agentLoginRules(target.harness) }
    val allSignIn by model.agentSignIn.state.collectAsState()
    val signIn = allSignIn?.takeIf { it.target.matches(target) }
    var accounts by remember { mutableStateOf<List<DeviceAccountInfo>?>(null) }
    var loading by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf<DeviceAccountInfo?>(null) }
    var code by remember { mutableStateOf("") }
    var revision by remember { mutableIntStateOf(0) }
    var autoStarted by rememberSaveable { mutableStateOf(false) }
    val activity = LocalContext.current.loginActivity()
    DisposableEffect(target.deviceId, target.harness) {
        onDispose { if (activity?.isChangingConfigurations != true) model.agentSignIn.cancel(target) }
    }
    LaunchedEffect(client, target, online, revision) {
        if (!online) return@LaunchedEffect
        loading = true
        try { accounts = client.deviceAgentAccounts(target.deviceId); error = null }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "Couldn't load accounts." }
        finally { loading = false }
    }
    LaunchedEffect(signIn?.completed) { if (signIn?.completed == true) { code = ""; revision++ } }
    LaunchedEffect(autoSignIn, online) {
        if (autoSignIn && online && !autoStarted) {
            autoStarted = true
            if (signIn == null) rules.options.firstOrNull()?.let { model.agentSignIn.start(target, it.provider) }
        }
    }
    val mine = accounts.orEmpty().filter { it.harness == target.harness }
    val enabled = online && !loading && busy == null && signIn?.busy != true
    LazyColumn(contentPadding = PaddingValues(20.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        item { Text("Accounts on ${target.deviceName}", style = MaterialTheme.typography.labelLarge) }
        if (!online) item { Text("${target.deviceName} is offline. Connect it to manage its accounts.") }
        if (loading && accounts == null) item { ActivityIndicator(Modifier.size(22.dp)) }
        item {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                mine.forEach { account ->
                    Column {
                        GroupedRow(0, 1, modifier = Modifier.testTag("account-${account.id}"),
                            leadingContent = { HarnessMark(target.harness, 22.dp) },
                            supportingContent = {
                                val option = if (rules.options.size > 1) rules.options.firstOrNull { it.provider == account.provider }?.label else null
                                val detail = listOfNotNull(account.planLabel, account.organization, option).joinToString(" · ")
                                if (detail.isNotEmpty()) Text(detail)
                            },
                            trailingContent = {
                                when {
                                    busy == account.id -> ActivityIndicator(Modifier.size(20.dp))
                                    account.active -> Text("In use", color = successColor(), modifier = Modifier.testTag("account-in-use"))
                                    account.switchable && rules.switches -> AppTextButton(onClick = {
                                        busy = account.id
                                        scope.launch {
                                            try { accounts = client.deviceActivateAccount(target.deviceId, target.harness, account.id); error = null }
                                            catch (cancelled: CancellationException) { throw cancelled }
                                            catch (failure: Exception) { error = failure.message ?: "Couldn't switch accounts." }
                                            finally { busy = null }
                                        }
                                    }, enabled = enabled, modifier = Modifier.testTag("account-switch-${account.id}")) { Text("Switch") }
                                }
                            },
                        ) { Text(accountTitle(account)) }
                        if (rules.reportsUsage) account.usage.take(3).forEach { window -> UsageWindow(window) }
                        account.usageError?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
                        if (account.switchable && rules.switches) AppTextButton(onClick = { removing = account }, enabled = enabled) { Text("Remove account", color = MaterialTheme.colorScheme.error) }
                    }
                }
            }
        }
        error?.let { message -> item { Text(message, color = MaterialTheme.colorScheme.error) } }
        signIn?.error?.let { message -> item { Text(message, color = MaterialTheme.colorScheme.error) } }
        if (signIn?.busy == true) item {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp), modifier = Modifier.testTag("agent-sign-in-waiting")) {
                SelectionContainer { Text(signIn.message ?: "Finish signing in in the browser…") }
                if (signIn.pasteCode) {
                    Text("Authorization code", style = MaterialTheme.typography.labelMedium)
                    AppTextField(code, { code = it }, modifier = Modifier.testTag("agent-sign-in-code"), singleLine = true)
                    AppButton(onClick = { model.agentSignIn.submitCode(code); code = "" }, enabled = code.isNotBlank() && !signIn.submitting) { Text(if (signIn.submitting) "Signing in…" else "Continue") }
                }
                signIn.url?.let { url ->
                    val context = LocalContext.current
                    AppTextButton(onClick = {
                        runCatching { openAgentBrowser(context, url) }.onFailure { model.agentSignIn.browserFailed() }
                    }) { Text("Open sign-in page") }
                }
                AppTextButton(onClick = { model.agentSignIn.cancel(target); code = "" }) { Text("Cancel sign-in") }
            }
        }
        if (mine.isEmpty() || !rules.keepsOneLogin) {
            rules.options.forEach { option -> item {
                AppTextButton(onClick = { code = ""; model.agentSignIn.start(target, option.provider) }, enabled = enabled,
                    modifier = Modifier.testTag("agent-add-${option.provider.orEmpty()}")) {
                    Text(if (mine.isEmpty()) option.firstTitle else option.moreTitle)
                }
            } }
        }
        item { AppTextButton(onClick = { revision++ }, enabled = enabled) { Text("Refresh accounts") } }
        item { Text(listOfNotNull(rules.note, "Account changes affect new sessions on ${target.deviceName}. Signing in runs on that computer; finish in the browser here.").joinToString("\n\n"), style = MaterialTheme.typography.bodySmall) }
    }
    removing?.let { account ->
        AppAlert("Remove account?", onDismiss = { removing = null }, actions = listOf(
            AlertAction("Cancel") { removing = null },
            AlertAction("Remove", destructive = true) {
                removing = null
                busy = account.id
                scope.launch {
                    try { accounts = client.deviceForgetAccount(target.deviceId, target.harness, account.id); error = null }
                    catch (cancelled: CancellationException) { throw cancelled }
                    catch (failure: Exception) { error = failure.message ?: "Couldn't remove the account." }
                    finally { busy = null }
                }
            },
        )) { Text("Remove ${accountTitle(account)} from ${target.deviceName}?") }
    }
}

@Composable
private fun UsageWindow(window: UsageWindowInfo) {
    val used = window.usedFraction.takeIf { it.isFinite() }?.coerceIn(0f, 1f) ?: 0f
    Column(Modifier.padding(horizontal = 16.dp, vertical = 5.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        val remaining = window.resetsAtMs?.let { ((it - System.currentTimeMillis()).coerceAtLeast(0) / 60_000) }
        Text(listOfNotNull("${window.label} · ${(used * 100).roundToInt()}%", remaining?.let { "resets in ${if (it >= 60) "${it / 60}h" else "${it}m"}" }).joinToString(" · "), style = MaterialTheme.typography.bodySmall)
        LinearProgressIndicator(progress = { used }, modifier = Modifier.fillMaxWidth(), color = if (used > 0.9f) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary)
    }
}

private fun Context.loginActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.loginActivity()
    else -> null
}

internal fun openAgentBrowser(context: Context, url: String) {
    androidx.browser.customtabs.CustomTabsIntent.Builder().build().launchUrl(context, android.net.Uri.parse(url))
}
