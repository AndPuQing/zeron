package sh.zeron.android.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import sh.zeron.android.core.AppModel
import uniffi.zeron_core.SessionRow

@Composable
fun DeleteSessionDialog(model: AppModel, row: SessionRow, onDismiss: () -> Unit, onDeleted: () -> Unit) {
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    AppAlert("Delete session?", onDismiss = { if (!busy) onDismiss() }, actions = listOf(
        AlertAction("Cancel", enabled = !busy, onClick = onDismiss),
        AlertAction(if (busy) "Deleting…" else "Delete", enabled = !busy, destructive = true) {
            busy = true
            scope.launch {
                try { model.deleteSession(row.id); onDeleted() }
                catch (cancelled: CancellationException) { throw cancelled }
                catch (failure: Exception) { error = failure.message ?: "Couldn't delete this session." }
                finally { busy = false }
            }
        },
    )) {
        Text("Permanently delete “${row.title}”? This cannot be undone." +
            if (row.isCloud) " The session's Cloud machine will also be deleted." else "")
        error?.let { Text(it, color = MaterialTheme.colorScheme.error) }
    }
}
