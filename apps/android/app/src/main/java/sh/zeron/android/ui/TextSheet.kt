package sh.zeron.android.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.material3.Text
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import sh.zeron.android.design.Geist
import sh.zeron.android.design.GeistMono

/** Transcript text opened for selection ("Select text", a tool's details). */
internal data class TextSheet(val title: String, val text: String, val mono: Boolean)

/** Characters a sheet lays out: one selectable Text of megabytes stalls the UI. */
internal const val TEXT_SHEET_LIMIT = 100_000

/** [text] cut to at most [limit] UTF-16 units, never inside a surrogate pair. */
internal fun clipText(text: String, limit: Int): String {
    if (text.length <= limit) return text
    var end = limit
    if (Character.isHighSurrogate(text[end - 1])) end--
    return text.substring(0, end)
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun TextSheetHost(sheet: TextSheet, onDismiss: () -> Unit) {
    val shown = remember(sheet) { clipText(sheet.text, TEXT_SHEET_LIMIT) }
    ModalBottomSheet(onDismissRequest = onDismiss) {
        Text(sheet.title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 24.dp))
        if (shown.length < sheet.text.length) {
            Text(
                "Showing the first ${"%,d".format(TEXT_SHEET_LIMIT)} characters.",
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 4.dp),
            )
        }
        SelectionContainer {
            Text(
                shown,
                fontFamily = if (sheet.mono) GeistMono else Geist,
                style = if (sheet.mono) MaterialTheme.typography.bodySmall else MaterialTheme.typography.bodyLarge,
                modifier = Modifier
                    .fillMaxWidth()
                    .verticalScroll(rememberScrollState())
                    .padding(24.dp),
            )
        }
    }
}
