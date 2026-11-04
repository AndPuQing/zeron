package sh.zeron.android.ui

import android.content.Intent
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.transcript.Transcript
import sh.zeron.android.transcript.TranscriptActions
import sh.zeron.android.transcript.TranscriptState
import uniffi.zeron_core.CoreClient
import uniffi.zeron_core.DocumentPreview
import uniffi.zeron_core.documentLinkPath

/** Host files stay associated with their originating conversation for the whole preview. */
@Composable
fun DocumentPreviewDialog(core: CoreClient, chatId: String, initialPath: String, onDismiss: () -> Unit) {
    var paths by remember(core, chatId, initialPath) { mutableStateOf(listOf(initialPath)) }
    val back = { if (paths.size > 1) paths = paths.dropLast(1) else onDismiss() }
    Dialog(onDismissRequest = back, properties = DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Surface(Modifier.fillMaxSize()) {
            key(core, chatId, paths.last()) {
                DocumentContent(core, chatId, paths.last(), back, { paths = paths + it })
            }
        }
    }
}

@Composable
private fun DocumentContent(core: CoreClient, chatId: String, path: String, back: () -> Unit, open: (String) -> Unit) {
    var document by remember { mutableStateOf<DocumentPreview?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var attempt by remember { mutableIntStateOf(0) }
    var source by remember { mutableStateOf(false) }
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    LaunchedEffect(core, chatId, path, attempt) {
        error = null
        document = null
        try { document = core.readDocument(chatId, path) }
        catch (cancelled: CancellationException) { throw cancelled }
        catch (failure: Exception) { error = failure.message ?: "Couldn't read this document." }
    }
    Column(Modifier.fillMaxSize().safeDrawingPadding()) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = back) { Text("Back") }
            Text(path.substringAfterLast('/'), Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
            document?.let { doc ->
                if (doc.markdown) TextButton(onClick = { source = !source }) { Text(if (source) "Preview" else "Source") }
                TextButton(onClick = { clipboard.setText(AnnotatedString(doc.text)) }) { Text("Copy") }
            }
        }
        Text("Read-only · $path", Modifier.padding(horizontal = 16.dp, vertical = 4.dp), style = MaterialTheme.typography.labelSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        val doc = document
        when {
            error != null -> Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(error!!, color = MaterialTheme.colorScheme.error)
                TextButton(onClick = { attempt++ }) { Text("Retry") }
            }
            doc == null -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
            doc.text.isEmpty() -> Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) { Text("This document is empty.") }
            else -> {
                if (doc.truncated) Text("Preview truncated to the first 2 MiB.", Modifier.padding(16.dp), color = MaterialTheme.colorScheme.error)
                key(source) {
                    val transcript = remember { TranscriptState().also { it.following = false } }
                    val scope = rememberCoroutineScope()
                    var viewing by remember { mutableStateOf<ImageBitmap?>(null) }
                    DisposableEffect(transcript) {
                        transcript.engine.setDocument(chatId, doc, doc.markdown && !source)
                        onDispose { transcript.close() }
                    }
                    val actions = remember(transcript) {
                        TranscriptActions(
                            imageCacheScope = "${doc.deviceId}:$chatId:$path",
                            openUrl = { url ->
                                val target = documentLinkPath(url, path)
                                if (target != null) open(target)
                                else {
                                    val uri = Uri.parse(url)
                                    when (uri.scheme) {
                                        "https", "http" -> CustomTabsIntent.Builder().build().launchUrl(context, uri)
                                        "mailto" -> runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, uri)) }
                                        else -> Unit
                                    }
                                }
                            },
                            loadImage = { loadTranscriptImage(core, doc.deviceId, it) },
                            openImage = { ref -> scope.launch { viewing = loadTranscriptImage(core, doc.deviceId, ref) } },
                            showText = { _, text, _ -> clipboard.setText(AnnotatedString(text)) },
                        )
                    }
                    Transcript(transcript, actions, Modifier.weight(1f))
                    viewing?.let { ImageViewer(it) { viewing = null } }
                }
            }
        }
    }
}
