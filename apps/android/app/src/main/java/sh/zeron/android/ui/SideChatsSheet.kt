package sh.zeron.android.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.SessionRow

@Composable
fun SideChatsSheet(
    parent: SessionRow?,
    children: List<SessionRow>,
    currentId: String,
    busy: Boolean,
    canFork: Boolean,
    onDismiss: () -> Unit,
    onNew: () -> Unit,
    onFork: () -> Unit,
    onOpen: (String) -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
    ) {
        Column(Modifier.fillMaxWidth().heightIn(max = 640.dp)) {
            Text("Side chats", style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(horizontal = 24.dp))
            if (parent != null && parent.id != currentId) {
                TextButton(onClick = { onOpen(parent.id) }, modifier = Modifier.padding(horizontal = 12.dp)) {
                    ZIcon(ZIcons.Back, null, Modifier.size(18.dp))
                    Text("Main chat: ${parent.title}", Modifier.padding(start = 8.dp), maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            } else {
                Spacer(Modifier.height(12.dp))
            }
            Row(
                Modifier.fillMaxWidth().padding(horizontal = 24.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                FilledTonalButton(
                    onClick = onFork,
                    enabled = canFork && !busy,
                    modifier = Modifier.weight(1f).semantics { contentDescription = "Fork from last response" },
                ) {
                    if (busy) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                    else ZIcon(ZIcons.Branch, null, Modifier.size(18.dp))
                    Text(if (busy) "Forking…" else "Fork", Modifier.padding(start = 8.dp))
                }
                FilledTonalButton(onClick = onNew, enabled = !busy, modifier = Modifier.weight(1f)) {
                    ZIcon(ZIcons.Plus, null, Modifier.size(18.dp))
                    Text("New side chat", Modifier.padding(start = 8.dp))
                }
            }
            Text(
                if (canFork) "Fork copies the conversation through the last completed reply."
                else "A completed reply is needed to fork this chat.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp),
            )
            if (children.isEmpty()) {
                Text(
                    "No side chats yet.",
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(24.dp),
                )
            } else {
                LazyColumn(
                    Modifier.weight(1f, fill = false),
                    contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(4.dp),
                ) {
                    items(children, key = { it.id }) { row ->
                        val current = row.id == currentId
                        Surface(
                            onClick = { onOpen(row.id) },
                            shape = RoundedCornerShape(16.dp),
                            color = if (current) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerLow,
                            modifier = Modifier.fillMaxWidth().semantics { selected = current },
                        ) {
                            Row(
                                Modifier.heightIn(min = 72.dp).padding(16.dp),
                                horizontalArrangement = Arrangement.spacedBy(12.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                HarnessMark(row.harness, 24.dp)
                                Column(Modifier.weight(1f)) {
                                    Text(row.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                    Text(
                                        if (current) "Current chat" else row.preview ?: row.timeLabel,
                                        style = MaterialTheme.typography.bodySmall,
                                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                    )
                                }
                                StatusLabel(row)
                            }
                        }
                    }
                }
            }
        }
    }
}
