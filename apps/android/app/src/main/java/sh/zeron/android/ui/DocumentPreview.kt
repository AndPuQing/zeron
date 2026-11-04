package sh.zeron.android.ui

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.Toast
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.AlertDialog
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
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
private class DocumentEntry(val path: String) {
    var document by mutableStateOf<DocumentPreview?>(null)
    var error by mutableStateOf<String?>(null)
    var attempt by mutableIntStateOf(0)
    var source by mutableStateOf(false)
    /** A file beyond the chat's workspace, waiting for the reader's answer. */
    var confirm by mutableStateOf<DocumentRead.NeedsConfirmation?>(null)
    /** The reader agreed to open it; later reads (Retry) need not ask again. */
    var allowOutside = false
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
        Surface(Modifier.fillMaxSize()) {
            val entry = stack.last()
            key(entry) {
                DocumentContent(core, chatId, entry, back, open)
                // A popped document's reader goes once it leaves the screen.
                DisposableEffect(entry) { onDispose { if (entry !in stack) entry.close() } }
            }
        }
    }
}

@Composable
private fun DocumentContent(core: CoreClient, chatId: String, entry: DocumentEntry, back: () -> Unit, open: (String) -> Unit) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    var sheet by remember { mutableStateOf<TextSheet?>(null) }
    LaunchedEffect(entry, entry.attempt) {
        // Back to a loaded document: no second read.
        if (entry.document != null) return@LaunchedEffect
        entry.error = null
        try {
            when (val read = core.readDocument(chatId, entry.path, entry.allowOutside)) {
                is DocumentRead.Ready -> { entry.document = read.document }
                is DocumentRead.NeedsConfirmation -> { entry.confirm = read }
            }
        }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { entry.error = CloudStore.message(failure) }
    }
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = back) { Text("Back") }
            Text(entry.path.substringAfterLast('/'), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
            entry.document?.let { doc ->
                if (doc.markdown) TextButton(onClick = { entry.source = !entry.source }) { Text(if (entry.source) "Preview" else "Source") }
                TextButton(onClick = { copyDocument(context, clipboard, doc.text) }) { Text("Copy") }
            }
        }
        Text("Read-only · ${entry.path}", Modifier.padding(horizontal = 16.dp, vertical = 4.dp), style = MaterialTheme.typography.labelSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        val doc = entry.document
        val error = entry.error
        when {
            error != null -> Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(error, color = MaterialTheme.colorScheme.error)
                TextButton(onClick = { entry.attempt++ }) { Text("Retry") }
            }
            doc == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { if (entry.confirm == null) CircularProgressIndicator() }
            doc.text.isEmpty() -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("This document is empty.") }
            else -> {
                if (doc.truncated) Text("Preview truncated to the first 2 MiB.", Modifier.padding(16.dp), color = MaterialTheme.colorScheme.error)
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
                Transcript(transcript, actions, Modifier.weight(1f))
                viewing?.let { ImageViewer(it) { viewing = null } }
            }
        }
    }
    sheet?.let { s -> TextSheetHost(s) { sheet = null } }
    entry.confirm?.let { ask ->
        // Nothing of the file has been read yet; Cancel leaves it unread.
        val cancel = { entry.confirm = null; back() }
        AlertDialog(
            onDismissRequest = cancel,
            title = { Text("Open this file?") },
            text = { Text("This file is outside the chat's folder.\n\n${ask.deviceName}: ${ask.path}") },
            confirmButton = {
                TextButton(onClick = {
                    entry.confirm = null
                    entry.allowOutside = true
                    entry.attempt++
                }) { Text("Open anyway") }
            },
            dismissButton = { TextButton(onClick = cancel) { Text("Cancel") } },
        )
    }
}

/** Copy at most [COPY_LIMIT] characters, saying so when the copy is partial or fails. */
private fun copyDocument(context: Context, clipboard: ClipboardManager, text: String) {
    val clipped = clipText(text, COPY_LIMIT)
    val notice = try {
        clipboard.setText(AnnotatedString(clipped))
        if (clipped.length < text.length) "Copied the first ${"%,d".format(COPY_LIMIT)} characters." else null
    } catch (failure: Exception) {
        "Couldn't copy this document."
    }
    notice?.let { Toast.makeText(context, it, Toast.LENGTH_SHORT).show() }
}
