package sh.zeron.android.ui

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.width
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp

/** What the composer's trailing button does right now. */
enum class ComposerAction { Send, Queue, Stop }

/** Send / queue / stop: circular controls inside the glass composer. */
@Composable
internal fun ActionButton(action: ComposerAction, hasText: Boolean, onClick: () -> Unit) {
    val haptics = LocalHapticFeedback.current
    AnimatedContent(
        action == ComposerAction.Stop && !hasText,
        transitionSpec = { (scaleIn() + fadeIn()) togetherWith (scaleOut() + fadeOut()) },
        label = "action",
    ) { stop ->
        if (stop) {
            Surface(
                onClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                    onClick()
                },
                shape = CircleShape,
                color = MaterialTheme.colorScheme.onSurface,
                contentColor = MaterialTheme.colorScheme.background,
                modifier = Modifier.size(34.dp),
            ) { Box(contentAlignment = Alignment.Center) { sh.zeron.android.design.ZIcon(sh.zeron.android.design.ZIcons.Stop, "Stop", Modifier.size(16.dp)) } }
        } else {
            Surface(
                onClick = {
                    haptics.performHapticFeedback(HapticFeedbackType.Confirm)
                    onClick()
                },
                enabled = hasText,
                shape = CircleShape,
                color = if (hasText) MaterialTheme.colorScheme.primary else sh.zeron.android.design.glassControlFill(),
                contentColor = if (hasText) Color.White else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.35f),
                modifier = Modifier.size(34.dp),
            ) { Box(contentAlignment = Alignment.Center) { sh.zeron.android.design.ZIcon(sh.zeron.android.design.ZIcons.Send, if (action == ComposerAction.Queue) "Queue" else "Send", Modifier.size(20.dp)) } }
        }
    }
}

/** A context chip in the composer toolbar; `menu` is its (lazily built) choice menu. */
@Composable
fun ContextChip(
    label: String,
    leading: @Composable () -> Unit,
    onClick: () -> Unit,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    menu: @Composable () -> Unit = {},
) {
    Box {
        Surface(
            onClick = onClick,
            shape = RoundedCornerShape(50),
            color = chipContainer(),
            contentColor = tint,
        ) {
            Row(
                Modifier.heightIn(min = 32.dp).padding(start = 10.dp, end = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Box(Modifier.size(16.dp), contentAlignment = Alignment.Center) { leading() }
                Text(label, style = MaterialTheme.typography.labelMedium.copy(fontSize = 13.5.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
        menu()
    }
}

class MenuChoice(
    val label: String,
    val selected: Boolean = false,
    val supporting: String? = null,
    val leading: (@Composable () -> Unit)? = null,
    val onClick: () -> Unit,
)

class MenuSection(val title: String?, val choices: List<MenuChoice>)

class MenuAction(
    val label: String,
    @androidx.annotation.DrawableRes val icon: Int,
    val destructive: Boolean = false,
    val enabled: Boolean = true,
    val supporting: String? = null,
    val onClick: () -> Unit,
)

/** Compact system-style rows: text first, action glyph at the trailing edge. */
@Composable
fun ActionMenu(expanded: Boolean, onDismiss: () -> Unit, actions: List<MenuAction>) {
    AppMenuPopup(expanded, onDismiss) {
        actions.forEachIndexed { i, action ->
            if (i > 0) Hairline()
            MenuRow(action.label, action.supporting, action.enabled, destructive = action.destructive,
                trailing = { sh.zeron.android.design.ZIcon(action.icon, null, Modifier.size(20.dp)) }) {
                onDismiss()
                action.onClick()
            }
        }
    }
}

@Composable
internal fun MenuRow(
    label: String,
    supporting: String? = null,
    enabled: Boolean = true,
    selected: Boolean = false,
    selectable: Boolean = false,
    destructive: Boolean = false,
    leading: (@Composable () -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
    onClick: () -> Unit,
) {
    val color = (if (destructive) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface)
        .copy(alpha = if (enabled) 1f else 0.3f)
    androidx.compose.runtime.CompositionLocalProvider(androidx.compose.material3.LocalContentColor provides color) {
        Row(
            Modifier.fillMaxWidth().pressable(enabled, role = if (selectable) Role.RadioButton else Role.Button, onClick = onClick)
                .then(if (selectable) Modifier.semantics { this.selected = selected } else Modifier)
                .heightIn(min = 46.dp).padding(horizontal = 16.dp, vertical = 11.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            leading?.invoke()
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(label, style = MaterialTheme.typography.bodyLarge, color = color)
                supporting?.let { Text(it, style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = if (enabled) 1f else 0.45f)) }
            }
            trailing?.invoke()
        }
    }
}
