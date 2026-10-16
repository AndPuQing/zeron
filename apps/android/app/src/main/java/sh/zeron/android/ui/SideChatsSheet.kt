package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import sh.zeron.android.design.ActivityIndicator
import sh.zeron.android.design.HarnessMark
import sh.zeron.android.design.LocalDarkTheme
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
    AppSheet("Side chats", onDismiss, actions = {
        if (busy) Box(Modifier.size(44.dp), contentAlignment = Alignment.Center) { ActivityIndicator() }
        else {
            PlainIconButton(ZIcons.Branch, "Fork from last response", onFork, enabled = canFork,
                modifier = Modifier.semantics { if (!canFork) stateDescription = "Needs a completed reply" })
            PlainIconButton(ZIcons.Plus, "New side chat", onNew)
        }
    }) {
        if (parent != null && parent.id != currentId) {
            AppTextButton(onClick = { onOpen(parent.id) }, modifier = Modifier.padding(horizontal = 8.dp)) {
                ZIcon(ZIcons.Back, null, Modifier.size(16.dp))
                Text("Main chat: ${parent.title}", Modifier.padding(start = 8.dp), maxLines = 1, overflow = TextOverflow.Ellipsis,
                    style = MaterialTheme.typography.bodyMedium)
            }
        }
        LazyColumn(Modifier.fillMaxWidth().weight(1f, fill = false).heightIn(min = 220.dp), contentPadding = PaddingValues(bottom = 24.dp)) {
            if (children.isEmpty()) {
                item {
                    Box(Modifier.fillMaxWidth().heightIn(min = 220.dp), contentAlignment = Alignment.Center) {
                        Text("No side chats yet.", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
            }
            items(children, key = { it.id }) { row ->
                val current = row.id == currentId
                val active = if (LocalDarkTheme.current) Color.White.copy(alpha = 0.06f) else Color.White.copy(alpha = 0.9f)
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp).clip(RoundedCornerShape(16.dp))
                        .background(if (current) active else Color.Transparent).pressable { onOpen(row.id) }
                        .semantics { selected = current }.heightIn(min = 62.dp).padding(horizontal = 12.dp, vertical = 9.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(14.dp),
                ) {
                    HarnessMark(row.harness, 20.dp, tint = MaterialTheme.colorScheme.onSurface)
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            Text(row.title, Modifier.weight(1f), style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            StatusLabel(row)
                        }
                        Text(if (current) "Current chat" else row.preview ?: row.timeLabel,
                            style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
        }
    }
}
