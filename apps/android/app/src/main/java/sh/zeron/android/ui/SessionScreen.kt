package sh.zeron.android.ui

import uniffi.zeron_core.MobileComposerAction

import android.content.Intent
import android.graphics.BitmapFactory
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.outlined.Archive
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import sh.zeron.android.design.ZIcon
import kotlinx.coroutines.delay
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.ActivityIndicator
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import sh.zeron.android.core.AppModel
import sh.zeron.android.design.Geist
import sh.zeron.android.design.GeistMono
import sh.zeron.android.design.GlassScene
import sh.zeron.android.design.ZIcons
import sh.zeron.android.transcript.Transcript
import sh.zeron.android.transcript.TranscriptActions
import sh.zeron.android.transcript.TranscriptState
import uniffi.zeron_core.ComposerState
import uniffi.zeron_core.ChatIndicator
import uniffi.zeron_core.ConnectivityState
import uniffi.zeron_core.SendState
import uniffi.zeron_core.SessionHandle

private data class TextSheet(val title: String, val text: String, val mono: Boolean)
private data class SideChatError(val title: String, val message: String)

@OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun SessionScreen(
    model: AppModel, chatId: String, onBack: () -> Unit,
    onOpen: (String) -> Unit = {}, onAgentAccounts: (String, String) -> Unit = { _, _ -> },
    onNavigate: (MobileComposerAction) -> Unit = {}, draft: ComposerModel = remember(chatId) { ComposerModel() },
    onOpenVoice: () -> Unit = {},
    onVoiceSettings: () -> Unit = {},
    onOpenSideChat: (String) -> Unit = onOpen,
    onOpenTerminal: (chatId: String, terminalId: String?) -> Unit = { _, _ -> },
    onOpenSubagent: (parentId: String, docId: String, title: String, running: Boolean) -> Unit = { _, _, _, _ -> },
    /** Debug launch extra: open a companion once the session is up (`companions`, `terminal`, `side-chat:<id>`, `subagent:<tool id>`). */
    openCompanion: String? = null,
    /** An agent's logins (an error card's "Log in": `signIn` starts one at once). */
    onOpenAccounts: (AccountsTarget, signIn: Boolean) -> Unit = { _, _ -> },
    onOpenBilling: () -> Unit = {},
    /** Companions open beside the session (the large-screen inspector) instead of over it. */
    onShowCompanions: ((CompanionSection?) -> Unit)? = null,
) {
    val client by model.client.collectAsState()
    val core = client ?: return
    val handle: SessionHandle = remember(core, chatId) { runCatching { core.openSession(chatId) }.getOrNull() } ?: run {
        Column(Modifier.statusBarsPadding().padding(24.dp)) {
            TonalCircleButton(ZIcons.Back, "Back", onClick = onBack)
            Text("This session isn't available.", Modifier.padding(top = 24.dp))
        }
        return
    }
    val transcript = remember(core, chatId) { TranscriptState() }
    val pixelScale = LocalDensity.current.density
    DisposableEffect(core, handle, transcript, pixelScale) {
        transcript.engine.setVisualSource(core, handle.composer().host.deviceId, pixelScale)
        transcript.engine.attach(core, chatId)
        // Agent visuals run their scripts from the chat's host.
        model.row(chatId)?.deviceId?.let { transcript.setVisualSource(core, it, maxOf(2f, pixelScale)) }
        handle.setViewAttached(true)
        core.markSeen(chatId)
        model.markSeen(chatId)
        model.openChats.add(chatId)
        onDispose {
            model.openChats.remove(chatId)
            handle.setViewAttached(false)
            transcript.close()
            // What arrived while it was open has been seen too.
            model.markSeen(chatId)

        }
    }

    // Composer-facing state: re-pulled on this chat's events.
    var composer by remember(handle) { mutableStateOf(handle.composer()) }
    var canFork by remember(handle) { mutableStateOf(handle.canFork()) }
    val workspace by model.workspace.collectAsState()
    val connectivity by model.connectivity.collectAsState()
    LaunchedEffect(model, handle, chatId) {
        model.sessionEvents.collect {
            if (it == chatId) {
                composer = handle.composer()
                canFork = handle.canFork()
            }
        }
    }
    // Upload rings on pending thumbnails follow the escort.
    LaunchedEffect(composer.transferProgress) { transcript.uploadProgress = composer.transferProgress }
    val row = remember(workspace, chatId) { model.row(chatId) }
    val parentId = row?.parentChatId ?: chatId
    val parent = remember(workspace, parentId) { model.row(parentId) }
    val children = remember(workspace, parentId) { model.children(parentId) }
    val sideChatBusy by model.sideChatBusy.collectAsState()
    var showSideChats by rememberSaveable(chatId) { mutableStateOf(false) }
    var sideChatError by remember(chatId) { mutableStateOf<SideChatError?>(null) }
    val scope = rememberCoroutineScope()
    val createSideChat: (Boolean) -> Unit = { fork ->
        if (!sideChatBusy) {
            scope.launch {
                try {
                    val id = if (fork) model.forkSideChat(chatId) else model.createSideChat(parentId)
                    showSideChats = false
                    onOpen(id)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    sideChatError = SideChatError(
                        if (fork) "Couldn't fork this chat" else "Couldn't create a side chat",
                        e.message?.removePrefix("reason=")?.takeIf { it.isNotBlank() } ?: "Please try again.",
                    )
                }
            }
        }
    }

    val context = LocalContext.current
    var sheet by remember(chatId) { mutableStateOf<TextSheet?>(null) }
    // The action set closes over the execution target, so a late workspace
    // snapshot or an agent switch rebuilds it instead of pointing at a stale row.
    val actionTarget = "${row?.harness.orEmpty()}\u0000${composer.host.deviceId}"
    var viewing by remember(chatId) { mutableStateOf<ImageBitmap?>(null) }
    val actions = remember(core, chatId, onOpen, onAgentAccounts, actionTarget, onOpenSubagent) {
        suspend fun load(ref: String): ImageBitmap? {
            val device = handle.composer().host.deviceId
            return runCatching {
                val bytes = core.readAttachment(device, ref)
                withContext(Dispatchers.Default) { BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }
            }.getOrNull()
        }
        val open: (String) -> Unit = { ref -> scope.launch { load(ref)?.let { viewing = it } } }
        TranscriptActions(
            openUrl = { url ->
                val uri = Uri.parse(url)
                if (uri.scheme == "zerun" && uri.host == "chat") {
                    uri.pathSegments.singleOrNull()?.takeIf { it.isNotBlank() }?.let(onOpen)
                } else if (uri.scheme == "zeron-preview") {
                    uri.getQueryParameter("ref")?.let(open)
                } else if (uri.scheme == "http" || uri.scheme == "https") {
                    CustomTabsIntent.Builder().build().launchUrl(context, uri)
                } else {
                    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
                }
            },
            loadImage = ::load,
            showText = { title, text, mono -> sheet = TextSheet(title, text, mono) },
            openImage = open,
            openSubagent = { doc, title, running -> onOpenSubagent(chatId, doc, title, running) },
            perform = { action, label ->
                if (action == "sign-in") {
                    val device = handle.composer().host.deviceId
                    val agent = model.row(chatId)?.harness
                    if (device.isNotBlank() && agent != null) onAgentAccounts(device, agent)
                    else sheet = TextSheet(label, "The computer or agent for this chat isn't available yet.", false)
                }
            },
        )
    }

    val project = row?.project?.name ?: "No project"
    val subtitle = if (row?.parentChatId != null) "Side chat of ${parent?.title ?: "main chat"}"
        else composer.host.name?.let { "$project @ $it" } ?: project
    var overflow by remember(chatId) { mutableStateOf(false) }
    var deleting by rememberSaveable(chatId) { mutableStateOf(false) }
    // Text handed to this chat (a terminal's output) lands after what's typed.
    DisposableEffect(chatId) {
        model.composerSinks[chatId] = { text ->
            draft.appendText(text)
            model.drafts.save(chatId, draft.persistedDraft())
        }
        onDispose { model.composerSinks.remove(chatId) }
    }

    // Companions: subagents, side chats (a side chat's siblings live with its parent), terminals.
    val sideParentId = row?.parentChatId
    // `companion model-picker` (screenshots): the model picker opens once the session is up.
    val openPicker = remember(chatId) { openCompanion == "model-picker" }
    val isCloud = row?.isCloud == true
    val cloudOutOfCredits = isCloud && model.cloud.outOfCredits
    var nudges by remember { mutableIntStateOf(0) }
    LaunchedEffect(isCloud) { if (isCloud && model.cloud.info == null) model.cloud.refresh() }
    val companions = rememberCompanions(model, chatId, includeSideChats = sideParentId == null)
    var companionsSheet by remember { mutableStateOf<CompanionSection?>(null) }
    var showCompanions by remember { mutableStateOf(false) }
    val view = androidx.compose.ui.platform.LocalView.current
    fun openTerminal() = onOpenTerminal(chatId, model.terminals.tabs(chatId).firstOrNull { it.isLive }?.ref?.id)
    val initialCompanion = remember(chatId) { openCompanion }
    LaunchedEffect(chatId) {
        val which = initialCompanion ?: return@LaunchedEffect
        delay(800)
        when {
            which == "companions" -> onShowCompanions?.invoke(null) ?: run { showCompanions = true }
            which == "terminal" -> onOpenTerminal(chatId, null)
            which.startsWith("side-chat:") -> onOpenSideChat(which.removePrefix("side-chat:"))
            which.startsWith("subagent:") -> {
                val tool = which.removePrefix("subagent:")
                model.sessionSubagents(chatId).firstOrNull { it.id == tool || it.docId?.endsWith(tool) == true }?.let { a ->
                    a.docId?.let { onOpenSubagent(chatId, it, a.title, a.state == uniffi.zeron_core.SubagentState.RUNNING) }
                }
            }
        }
    }

    val clipboard = LocalClipboardManager.current

    val density = LocalDensity.current
    var headerHeight by remember { mutableStateOf(88.dp) }
    var footerHeight by remember { mutableStateOf(80.dp) }
    LaunchedEffect(footerHeight) {
        // The last row can clear the floating composer, queue and input panel.
        transcript.bottomInset = footerHeight.value + 12f
    }
    GlassScene(
        modifier = Modifier.fillMaxSize().imePadding(),
        background = { source ->
            Box(source.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
                Transcript(transcript, actions, Modifier.fillMaxSize().padding(top = headerHeight))
            }
        },
    ) {
        Box(
            Modifier.align(Alignment.TopCenter)
                .onSizeChanged { headerHeight = with(density) { it.height.toDp() } },
        ) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .statusBarsPadding()
                    .padding(horizontal = 12.dp, vertical = 8.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                TonalCircleButton(ZIcons.Back, "Back", onClick = onBack)
                Column(
                    Modifier.weight(1f).padding(horizontal = 12.dp).heightIn(min = 48.dp)
                        .clickable(enabled = row?.parentChatId != null, onClickLabel = "Open main chat") { onOpen(parentId) },
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = androidx.compose.foundation.layout.Arrangement.Center,
                ) {
                    Text(composer.title, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMediumEmphasized)
                    Text(
                        subtitle,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        style = MaterialTheme.typography.bodySmall.copy(fontSize = 12.sp),
                        color = if (row?.parentChatId != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 1.dp),
                    )
                }
                VoiceEntry(model, Modifier.padding(end = 8.dp))
                val sideChatTone = if (children.any { it.indicator == ChatIndicator.WORKING || it.indicator == ChatIndicator.AWAITING_INPUT })
                    MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant
                GlassSurface(onClick = { showSideChats = true }, contentColor = sideChatTone) {
                    Row(Modifier.heightIn(min = 44.dp).widthIn(min = 44.dp).padding(horizontal = 10.dp).semantics { contentDescription = "Side chats" },
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = androidx.compose.foundation.layout.Arrangement.spacedBy(4.dp, Alignment.CenterHorizontally)) {
                        if (sideChatBusy) ActivityIndicator(Modifier.size(20.dp), color = sideChatTone)
                        else sh.zeron.android.design.ZIcon(ZIcons.Branch, null, Modifier.size(24.dp))
                        if (children.isNotEmpty()) Text(children.size.toString(), style = MaterialTheme.typography.labelMedium)
                    }
                }
                Spacer(Modifier.width(8.dp))
                Box {
                    TonalCircleButton(ZIcons.More, "More", onClick = { overflow = true })
                    ActionMenu(
                        overflow,
                        { overflow = false },
                        listOfNotNull(
                            MenuAction("Terminal", ZIcons.Terminal) { openTerminal() },
                            if (row?.parentChatId == null) MenuAction("New side chat", ZIcons.Plus, enabled = !sideChatBusy) { createSideChat(false) } else null,
                            MenuAction(
                                "Fork to side chat",
                                ZIcons.Branch,
                                enabled = canFork && !sideChatBusy,
                                supporting = when {
                                    sideChatBusy -> "Forking…"
                                    !canFork -> "Needs a completed reply"
                                    else -> "Through the last completed reply"
                                },
                            ) { createSideChat(true) },
                            MenuAction("Copy transcript", ZIcons.Copy) { transcript.frame?.let { clipboard.setText(AnnotatedString(it.plainText())) } },
                            row?.takeIf { it.parentChatId == null }?.let { r -> MenuAction(if (r.pinned) "Unpin" else "Pin", ZIcons.Pin) { model.setPinned(chatId, !r.pinned) } },
                            row?.let { MenuAction(if (it.archived) "Unarchive" else "Archive", ZIcons.Archive) { if (it.archived) model.unarchive(chatId) else model.archive(chatId); onBack() } },
                            row?.let { MenuAction("Delete", ZIcons.Delete, destructive = true) { deleting = true } },
                        ),
                    )
                }
            }
        }
        val showJump by remember { derivedStateOf { !transcript.following && transcript.distanceFromBottom > 400f } }
        androidx.compose.animation.AnimatedVisibility(
            showJump,
            enter = fadeIn() + scaleIn(),
            exit = fadeOut() + scaleOut(),
            modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = footerHeight + 12.dp),
        ) {
            TonalCircleButton(ZIcons.ArrowDown, "Jump to latest", onClick = { transcript.scrollToBottom() })
        }
        Column(
            Modifier.align(Alignment.BottomCenter).fillMaxWidth()
                .onSizeChanged { footerHeight = with(density) { it.height.toDp() } }
                .navigationBarsPadding(),
        ) {
            if (!companions.isEmpty) CompanionStrip(companions, onTap = { section ->
                onShowCompanions?.invoke(section) ?: run { companionsSheet = section; showCompanions = true }
            }, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
            Banner(composer, connectivity?.state, onRetry = { runCatching { handle.retryDelivery() } })
            composer.openInput?.let { QuestionPanel(it) { answers -> runCatching { handle.respondInput(it.requestId, answers) } } }
            Composer(model, core, handle, composer, row, transcript, draft, canFork, sideChatBusy,
                    onNavigate = onNavigate, onArchived = onBack, onSideChat = { fork ->
                        val id = if (fork) model.forkSideChat(chatId) else model.createSideChat(parentId)
                        onOpen(id)
                    })
        }
    }

    viewing?.let { ImageViewer(it, onDismiss = { viewing = null }) }
    if (showSideChats) {
        SideChatsSheet(
            parent, children, chatId, sideChatBusy, canFork,
            onDismiss = { showSideChats = false },
            onNew = { createSideChat(false) },
            onFork = { createSideChat(true) },
            onOpen = { id -> showSideChats = false; onOpen(id) },
        )
    }

    sideChatError?.let { error ->
        AppAlert(error.title, onDismiss = { sideChatError = null },
            actions = listOf(AlertAction("OK") { sideChatError = null })) { Text(error.message) }
    }

    if (deleting) row?.let { r ->
        DeleteSessionDialog(model, r, onDismiss = { deleting = false }, onDeleted = { deleting = false; onBack() })
    }

    if (showCompanions) {
        CompanionsSheet(
            model,
            chatId,
            title = if (row?.parentChatId != null) "Side chat" else composer.title,
            initialSection = companionsSheet,
            onDismiss = {
                showCompanions = false
                companionsSheet = null
            },
            onOpenSubagent = { a -> a.docId?.let { onOpenSubagent(chatId, it, a.title, a.state == uniffi.zeron_core.SubagentState.RUNNING) } },
            onOpenSideChat = onOpenSideChat,
            onOpenTerminal = { tab -> onOpenTerminal(chatId, tab?.ref?.id) },
        )
    }

    sheet?.let { s ->
        AppSheet(s.title, onDismiss = { sheet = null }) {
            SelectionContainer {
                Text(
                    s.text,
                    fontFamily = if (s.mono) GeistMono else Geist,
                    style = if (s.mono) MaterialTheme.typography.bodySmall else MaterialTheme.typography.bodyLarge,
                    modifier = Modifier
                        .fillMaxWidth()
                        .verticalScroll(rememberScrollState())
                        .padding(24.dp),
                )
            }
        }
    }
}

@Composable
private fun Banner(c: ComposerState, connectivity: ConnectivityState?, onRetry: () -> Unit, cloud: sh.zeron.android.core.CloudStore? = null, chatId: String = "", deviceId: String? = null) {
    // Reconnect countdowns tick each second.
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    val retryAt = c.room.retryAtMs
    if (!c.room.connected && retryAt != null) {
        LaunchedEffect(retryAt) {
            while (true) {
                now = System.currentTimeMillis()
                delay(1_000)
            }
        }
    }
    // A Cloud chat whose send can't land says why: out of credits the notice
    // above already does; a failed machine is named.
    if (cloud != null && c.sendState == SendState.FAILED) {
        LaunchedEffect(Unit) { cloud.refreshSessions() }
        if (cloud.outOfCredits) return
        cloud.sessionError(chatId, deviceId)?.let { return StatusBanner("Cloud machine: $it", "Retry" to onRetry) }
    }
    val text = when {
        c.sendState == SendState.FAILED -> "Not delivered"
        c.sendState == SendState.QUEUED -> "${c.host.name ?: "Host"} is offline — will send when it's back"
        connectivity == ConnectivityState.OFFLINE -> "You're offline"
        !c.room.connected && c.room.retryAtMs != null -> "Reconnecting…"
        else -> null
    } ?: return
    StatusBanner(text, if (c.sendState == SendState.FAILED) "Retry" to onRetry else null)
}

/** Whose logins an error card's "Log in" opens: Cloud's for a Cloud chat, else the host's. */
fun signInTarget(model: AppModel, chatId: String): AccountsTarget? {
    val row = model.row(chatId) ?: return null
    val harness = row.harness ?: "claude-code"
    if (row.isCloud) return sh.zeron.android.core.CloudStore.agents.firstOrNull { it.harness == harness }?.let { AccountsTarget.Cloud(it) }
    val label = row.harnessLabel ?: uniffi.zeron_core.agentLoginRules(harness).name.ifEmpty { harness }
    return AccountsTarget.Device(row.deviceId, model.deviceName(row.deviceId), harness, label)
}

/** "Out of Cloud credits" with its one way forward; a refused send shakes it. */
@Composable
private fun CloudCreditsNotice(nudges: Int, onAddCredits: () -> Unit) {
    val shake = remember { androidx.compose.animation.core.Animatable(0f) }
    val view = androidx.compose.ui.platform.LocalView.current
    LaunchedEffect(nudges) {
        if (nudges == 0) return@LaunchedEffect
        view.performHapticFeedback(android.view.HapticFeedbackConstants.REJECT)
        for (x in listOf(-8f, 8f, -5f, 5f, -2f, 0f)) shake.animateTo(x, androidx.compose.animation.core.tween(50))
    }
    Surface(
        shape = androidx.compose.foundation.shape.RoundedCornerShape(50),
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        modifier = Modifier
            .padding(horizontal = 16.dp, vertical = 6.dp)
            .graphicsLayer { translationX = shake.value * density },
    ) {
        Row(Modifier.padding(start = 16.dp, end = 6.dp, top = 4.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            ZIcon(ZIcons.Warning, null, Modifier.size(18.dp))
            androidx.compose.foundation.layout.Spacer(Modifier.size(10.dp))
            Text(sh.zeron.android.core.CloudStore.OUT_OF_CREDITS, style = MaterialTheme.typography.labelLarge, modifier = Modifier.weight(1f))
            androidx.compose.material3.Button(onClick = onAddCredits, contentPadding = androidx.compose.foundation.layout.PaddingValues(horizontal = 14.dp)) { Text("Add credits") }
        }
    }
}
