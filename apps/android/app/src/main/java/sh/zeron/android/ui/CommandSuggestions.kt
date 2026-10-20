package sh.zeron.android.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.CancellationException
import sh.zeron.android.design.GlassSurface
import uniffi.zeron_core.ComposerCatalog

/** Fetch once per opening/context; typing filters locally without stale query results. */
@Composable
fun CommandSuggestions(
    model: ComposerModel,
    chatId: String,
    harness: String?,
    deviceId: String,
    load: suspend () -> ComposerCatalog,
) {
    val query = model.slashQuery()?.second ?: return
    var catalog by remember(chatId, harness, deviceId) { mutableStateOf<ComposerCatalog?>(null) }
    var error by remember(chatId, harness, deviceId) { mutableStateOf<String?>(null) }
    var retry by remember { mutableStateOf(0) }
    LaunchedEffect(chatId, harness, deviceId, retry) {
        catalog = null
        error = null
        try {
            catalog = load()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            error = e.message ?: "Could not load commands"
        }
    }
    val rows = catalog?.commands?.map(model::displayCommand)
        ?.sortedWith(compareBy({ it.unavailableReason != null }, { it.token }))?.filter {
        query.isEmpty() || it.token.drop(1).contains(query, ignoreCase = true) || it.description.contains(query, ignoreCase = true)
    }.orEmpty()
    val selectionContext = model.commandContext
    val scroll = rememberScrollState()
    LaunchedEffect(query) { scroll.scrollTo(0) }
    GlassSurface(
        shape = RoundedCornerShape(18.dp),
        modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp).testTag("slash-menu"),
    ) {
        Column(Modifier.heightIn(max = 280.dp).verticalScroll(scroll).padding(6.dp)) {
            rows.forEach { command ->
                val enabled = command.unavailableReason == null
                Column(
                    Modifier.fillMaxWidth().alpha(if (enabled) 1f else 0.55f)
                        .clickable(enabled = enabled) { model.insertCommand(command, selectionContext) }
                        .testTag("slash-${command.token}")
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                ) {
                    Text(command.token + (command.inputHint?.let { "  $it" } ?: ""), style = MaterialTheme.typography.bodyLarge)
                    Text(command.unavailableReason ?: command.description, style = MaterialTheme.typography.bodySmall)
                }
            }
            val warning = error ?: catalog?.warning
            if (warning != null) {
                Text(warning, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(12.dp))
                TextButton(onClick = { retry++ }) { Text("Retry loading commands") }
            } else if (rows.isEmpty()) {
                Text(if (catalog == null) "Loading commands…" else "No matching commands", modifier = Modifier.padding(12.dp))
            }
        }
    }
}
