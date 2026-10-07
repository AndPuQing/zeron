package sh.zeron.android.ui

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import sh.zeron.android.update.AndroidUpdater
import java.util.Locale

@Composable
fun UpdateDialog(updater: AndroidUpdater) {
    val state by updater.state.collectAsState()
    if (!state.prompt) return
    val activity = LocalContext.current.activity()
    val latest = state.latest
    AlertDialog(
        onDismissRequest = updater::dismiss,
        title = { Text(when {
            state.downloading -> "Downloading update"
            state.checking -> "Checking for updates"
            state.ready -> "Ready to install"
            latest != null -> "Update available"
            state.error != null -> "Update check failed"
            else -> "You're up to date"
        }) },
        text = {
            Column {
                Text("Installed version: ${state.currentVersion}")
                if (latest != null) {
                    Spacer(Modifier.height(8.dp))
                    Text("New version: ${latest.version}")
                }
                if (state.checking || state.downloading) {
                    Spacer(Modifier.height(16.dp))
                    val total = state.totalBytes
                    if (state.downloading && total != null) {
                        LinearProgressIndicator(progress = { (state.downloadedBytes.toFloat() / total).coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
                        Spacer(Modifier.height(8.dp))
                        Text("${mib(state.downloadedBytes)} / ${mib(total)} MB")
                    } else LinearProgressIndicator(Modifier.fillMaxWidth())
                }
                if (state.ready) {
                    Spacer(Modifier.height(12.dp))
                    Text("Android will ask you to confirm the update. If prompted, allow Zerun to install apps, then return here. Your settings and account stay on this device.")
                }
                state.error?.let {
                    Spacer(Modifier.height(12.dp))
                    Text(it, color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = {
            when {
                state.checking || state.downloading -> Unit
                state.ready -> TextButton(onClick = { activity?.let(updater::install) }, enabled = activity != null) { Text("Install") }
                latest != null -> TextButton(onClick = updater::download) { Text(if (state.error == null) "Download" else "Retry download") }
                state.error != null -> TextButton(onClick = { updater.check() }) { Text("Retry") }
                else -> TextButton(onClick = updater::dismiss) { Text("OK") }
            }
        },
        dismissButton = {
            if (latest != null || state.checking || state.downloading || state.error != null) {
                TextButton(onClick = updater::dismiss) { Text(if (state.downloading) "Hide" else "Later") }
            }
        },
    )
}

private fun mib(bytes: Long) = String.format(Locale.ROOT, "%.1f", bytes / (1024.0 * 1024.0))

private fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}
