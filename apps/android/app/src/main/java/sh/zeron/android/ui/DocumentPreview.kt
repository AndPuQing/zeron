package sh.zeron.android.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.AlertDialog
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.ClipboardManager
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.GeistMono
import sh.zeron.android.core.CloudStore
import sh.zeron.android.transcript.Transcript
import sh.zeron.android.transcript.TranscriptActions
import sh.zeron.android.transcript.TranscriptState
import uniffi.zeron_core.CoreClient
import uniffi.zeron_core.DocumentPreview
import uniffi.zeron_core.DocumentRead
import uniffi.zeron_core.documentLinkPath

/**
 * Characters Copy puts on the clipboard. The clip crosses Binder, whose
 * transaction limit (about 1 MB) a 2 MiB document in UTF-16 would exceed.
 */
private const val COPY_LIMIT = 100_000

/** One document in the preview's back stack: its read, mode and scroll survive a deeper link. */
@Stable
internal class DocumentEntry(val path: String) {
    var document by mutableStateOf<DocumentPreview?>(null)
    var error by mutableStateOf<String?>(null)
    var attempt by mutableIntStateOf(0)
    var source by mutableStateOf(false)
    /** A file beyond the chat's workspace, waiting for the reader's answer. */
    var confirm by mutableStateOf<DocumentRead.NeedsConfirmation?>(null)
    /** Canonical path the reader agreed to open; Retry keeps the same target. */
    var approvedPath: String? = null
    /** The reader, kept while deeper documents are open. */
    var transcript: TranscriptState? = null
    /** Whether the reader last rendered Markdown; null before its first render. */
    var rendered: Boolean? = null

    fun close() {
        transcript?.close()
        transcript = null
        rendered = null
    }
}

/** Host files stay associated with their originating conversation for the whole preview. */
@Composable
fun DocumentPreviewDialog(core: CoreClient, chatId: String, initialPath: String, onDismiss: () -> Unit) {
    val stack = remember(core, chatId, initialPath) { mutableStateListOf(DocumentEntry(initialPath)) }
    DisposableEffect(stack) { onDispose { stack.forEach { it.close() } } }
    val back: () -> Unit = { if (stack.size > 1) stack.removeAt(stack.lastIndex) else onDismiss() }
    // A link to the open document (`guide.md#usage`) stays on it.
    val open: (String) -> Unit = { path -> if (path != stack.last().path) stack.add(DocumentEntry(path)) }
    Dialog(onDismissRequest = back, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surfaceContainerLowest) {
            val entry = stack.last()
            key(entry) {
                DocumentContent(core, chatId, entry, back, open, canGoBack = stack.size > 1)
                // A popped document's reader goes once it leaves the screen.
                DisposableEffect(entry) { onDispose { if (entry !in stack) entry.close() } }
            }
        }
    }
}

@Composable
internal fun DocumentContent(core: CoreClient, chatId: String, entry: DocumentEntry, back: () -> Unit, open: (String) -> Unit, canGoBack: Boolean = false) {
    DialogSystemBars()
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var sheet by remember { mutableStateOf<TextSheet?>(null) }
    var menu by remember { mutableStateOf(false) }
    var details by remember { mutableStateOf(false) }
    LaunchedEffect(entry, entry.attempt) {
        // Back to a loaded document: no second read.
        if (entry.document != null) return@LaunchedEffect
        entry.error = null
        try {
            val approved = entry.approvedPath
            when (val read = core.readDocument(chatId, approved ?: entry.path, approved != null)) {
                is DocumentRead.Ready -> { entry.document = read.document.copy(path = entry.path) }
                is DocumentRead.NeedsConfirmation -> { entry.confirm = read }
            }
        }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { entry.error = CloudStore.message(failure) }
    }
    Column(Modifier.fillMaxSize().safeDrawingPadding(), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(Modifier.fillMaxWidth().heightIn(min = 56.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = back) {
                ZIcon(if (canGoBack) ZIcons.Back else ZIcons.Close, if (canGoBack) "Back" else "Close document")
            }
            Text(entry.path.substringAfterLast('/'), Modifier.weight(1f).padding(horizontal = 8.dp),
                maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
            IconButton(onClick = { entry.document?.let { copyDocument(context, clipboard, it.text) } }, enabled = entry.document != null) {
                ZIcon(ZIcons.Copy, "Copy document")
            }
            Box {
                IconButton(onClick = { menu = true }) { ZIcon(ZIcons.More, "Document options") }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(text = { Text("File details") }, leadingIcon = { ZIcon(ZIcons.Info, null) },
                        onClick = { menu = false; details = true })
                    DropdownMenuItem(text = { Text("Copy path") }, leadingIcon = { ZIcon(ZIcons.Copy, null) },
                        onClick = { menu = false; copyDocument(context, clipboard, entry.approvedPath ?: entry.path) })
                }
            }
        }
        val doc = entry.document
        if (doc?.markdown == true) {
            SingleChoiceSegmentedButtonRow(Modifier.widthIn(max = 360.dp).fillMaxWidth().padding(horizontal = 20.dp, vertical = 8.dp)) {
                listOf("Preview", "Source").forEachIndexed { index, label ->
                    SegmentedButton(
                        selected = entry.source == (index == 1),
                        onClick = { entry.source = index == 1 },
                        shape = SegmentedButtonDefaults.itemShape(index, 2),
                        icon = {},
                        colors = SegmentedButtonDefaults.colors(
                            activeContainerColor = MaterialTheme.colorScheme.secondaryContainer,
                            activeContentColor = MaterialTheme.colorScheme.onSecondaryContainer,
                            inactiveContainerColor = MaterialTheme.colorScheme.surfaceContainerLowest,
                            inactiveContentColor = MaterialTheme.colorScheme.onSurfaceVariant,
                        ),
                    ) { Text(label) }
                }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant.copy(alpha = 0.5f))
        if (doc?.truncated == true) {
            Row(Modifier.widthIn(max = 760.dp).fillMaxWidth().padding(horizontal = 20.dp, vertical = 10.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                ZIcon(ZIcons.Info, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                Text("Showing the first 2 MiB", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        val error = entry.error
        when {
            error != null -> DocumentStatus("Couldn't open document", error, ZIcons.Warning, retry = { entry.attempt++ })
            doc == null -> DocumentStatus(
                if (entry.confirm == null) "Opening document" else "Waiting for permission",
                if (entry.confirm == null) "Reading from the conversation's host…" else "Confirm to read this file.",
                ZIcons.Text, loading = entry.confirm == null)
            doc.text.isEmpty() -> DocumentStatus("This document is empty", "There is no content to display.", ZIcons.Text)
            else -> {
                val transcript = remember(entry) { entry.transcript ?: TranscriptState(startsAtTop = true).also { entry.transcript = it } }
                val markdown = doc.markdown && !entry.source
                LaunchedEffect(transcript, markdown) {
                    if (entry.rendered == markdown) return@LaunchedEffect
                    // Switching Source/Preview starts the other view at its top.
                    entry.rendered = markdown
                    transcript.showDocument(chatId, doc, markdown)
                }
                val scope = rememberCoroutineScope()
                var viewing by remember { mutableStateOf<ImageBitmap?>(null) }
                val actions = remember(transcript) {
                    TranscriptActions(
                        imageCacheScope = "${doc.deviceId}:$chatId:${entry.path}",
                        openUrl = { url ->
                            val target = documentLinkPath(url, entry.path)
                            if (target != null) open(target)
                            else {
                                val uri = Uri.parse(url)
                                when (uri.scheme) {
                                    "https", "http" -> runCatching { CustomTabsIntent.Builder().build().launchUrl(context, uri) }
                                    // Mail, tel and app links go to the system, as in the conversation.
                                    else -> runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
                                }
                            }
                        },
                        loadImage = { loadTranscriptImage(core, doc.deviceId, it) },
                        openImage = { ref -> scope.launch { viewing = loadTranscriptImage(core, doc.deviceId, ref) } },
                        showText = { title, text, mono -> sheet = TextSheet(title, text, mono) },
                    )
                }
                Transcript(transcript, actions, Modifier.weight(1f).widthIn(max = 760.dp).fillMaxWidth().padding(top = 12.dp))
                viewing?.let { ImageViewer(it) { viewing = null } }
            }
        }
    }
    if (details) {
        val device = entry.document?.deviceId?.let { id -> core.devices().firstOrNull { it.id == id }?.name }
        AlertDialog(
            onDismissRequest = { details = false },
            title = { Text("File details") },
            text = {
                Column(Modifier.verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                    Text("Read-only" + (device?.let { " · $it" } ?: ""), style = MaterialTheme.typography.labelMedium)
                    SelectionContainer { Text(entry.approvedPath ?: entry.path, fontFamily = GeistMono, style = MaterialTheme.typography.bodySmall) }
                }
            },
            confirmButton = { TextButton(onClick = { details = false }) { Text("Done") } },
            dismissButton = { TextButton(onClick = { copyDocument(context, clipboard, entry.approvedPath ?: entry.path) }) { Text("Copy path") } },
        )
    }
    sheet?.let { s -> TextSheetHost(s) { sheet = null } }
    entry.confirm?.let { ask ->
        // New hosts defer the read; legacy hosts withhold only the preview.
        val cancel = { entry.confirm = null; back() }
        AlertDialog(
            onDismissRequest = cancel,
            title = { Text("Open this file?") },
            text = { Text("This file is outside the chat's folder.\n\n${ask.deviceName}: ${ask.path}") },
            confirmButton = {
                TextButton(onClick = {
                    entry.confirm = null
                    entry.approvedPath = ask.path
                    entry.attempt++
                }) { Text("Open anyway") }
            },
            dismissButton = { TextButton(onClick = cancel) { Text("Cancel") } },
        )
    }
}

/** Status content stays readable on short screens and with large text. */
@Composable
private fun DocumentStatus(title: String, detail: String, icon: Int, loading: Boolean = false, retry: (() -> Unit)? = null) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(Modifier.widthIn(max = 420.dp).verticalScroll(rememberScrollState()).padding(32.dp),
            horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (loading) CircularProgressIndicator(Modifier.size(28.dp), strokeWidth = 2.dp)
            else ZIcon(icon, null, Modifier.size(36.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(title, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
            Text(detail, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center, color = MaterialTheme.colorScheme.onSurfaceVariant)
            retry?.let { FilledTonalButton(onClick = it) { Text("Try again") } }
        }
    }
}

/** Copy at most [COPY_LIMIT] characters, saying so when the copy is partial or fails. */
private fun copyDocument(context: Context, clipboard: ClipboardManager, text: String) {
    val clipped = clipText(text, COPY_LIMIT)
    val notice = try {
        clipboard.setText(AnnotatedString(clipped))
        if (clipped.length < text.length) "Copied the first ${"%,d".format(COPY_LIMIT)} characters." else "Copied"
    } catch (failure: Exception) {
        "Couldn't copy this document."
    }
    notice?.let { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
}
