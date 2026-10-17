package sh.zeron.android.ui

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
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
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
fun SessionScreen(model: AppModel, chatId: String, onBack: () -> Unit, onOpen: (String) -> Unit) {
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
        handle.setViewAttached(true)
        core.markSeen(chatId)
        onDispose {
            handle.setViewAttached(false)
            transcript.close()
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
    val actions = remember(core, chatId, onOpen) {
        TranscriptActions(
            openUrl = { url ->
                val uri = Uri.parse(url)
                if (uri.scheme == "zerun" && uri.host == "chat") {
                    uri.pathSegments.singleOrNull()?.takeIf { it.isNotBlank() }?.let(onOpen)
                } else if (uri.scheme == "http" || uri.scheme == "https") {
                    CustomTabsIntent.Builder().build().launchUrl(context, uri)
                } else {
                    runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
                }
            },
            loadImage = { ref ->
                val device = handle.composer().host.deviceId
                runCatching {
                    val bytes = core.readAttachment(device, ref)
                    withContext(Dispatchers.Default) { BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }
                }.getOrNull()
            },
            showText = { title, text, mono -> sheet = TextSheet(title, text, mono) },
        )
    }

    val project = row?.project?.name ?: "No project"
    val subtitle = if (row?.parentChatId != null) "Side chat of ${parent?.title ?: "main chat"}"
        else composer.host.name?.let { "$project @ $it" } ?: project
    var overflow by remember(chatId) { mutableStateOf(false) }
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
                            row?.let { MenuAction("Archive", ZIcons.Archive) { model.archive(chatId); onBack() } },
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
            Banner(composer, connectivity?.state, onRetry = { runCatching { handle.retryDelivery() } })
            composer.openInput?.let { QuestionPanel(it) { answers -> runCatching { handle.respondInput(it.requestId, answers) } } }
            Composer(model, core, handle, composer, row, transcript)
        }
    }

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
private fun Banner(c: ComposerState, connectivity: ConnectivityState?, onRetry: () -> Unit) {
    val text = when {
        c.sendState == SendState.FAILED -> "Not delivered"
        c.sendState == SendState.QUEUED -> "${c.host.name ?: "Host"} is offline — will send when it's back"
        connectivity == ConnectivityState.OFFLINE -> "You're offline"
        !c.room.connected && c.room.retryAtMs != null -> "Reconnecting…"
        else -> null
    } ?: return
    StatusBanner(text, if (c.sendState == SendState.FAILED) "Retry" to onRetry else null)
}
