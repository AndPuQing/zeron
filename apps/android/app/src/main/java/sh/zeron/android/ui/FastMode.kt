package sh.zeron.android.ui

import androidx.compose.runtime.*
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Surface
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Alignment
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bolt
import androidx.compose.material3.Icon
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import uniffi.zeron_core.FastModeControl
import uniffi.zeron_core.fastModeEnabled
import uniffi.zeron_core.setFastMode

internal fun FastModeControl.enabled(values: Map<String, String>) = fastModeEnabled(this, values[id])

internal fun FastModeControl.title(values: Map<String, String>): String = when (enabled(values)) {
    true -> "Fast on"
    false -> "Fast off"
    null -> "Fast: Default"
}

internal fun FastModeControl.applying(enabled: Boolean?, values: Map<String, String>) =
    setFastMode(this, enabled, values)

@Composable
internal fun FastModeChip(option: FastModeControl, values: Map<String, String>, onPick: (Boolean?) -> Unit) {
    var open by remember(option.id) { mutableStateOf(false) }
    val selected = option.enabled(values)
    Box {
        Surface(
            onClick = { open = true },
            shape = CircleShape,
            color = if (selected == true) MaterialTheme.colorScheme.primaryContainer else chipContainer(),
            contentColor = if (selected == true) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.size(34.dp),
        ) {
            Box(contentAlignment = Alignment.Center) {
                Icon(Icons.Default.Bolt, option.title(values), Modifier.size(16.dp))
            }
        }
        ChoiceMenu(open, { open = false }, listOf(MenuSection("Fast mode", listOf(
            MenuChoice("Use default", selected == null) { onPick(null) },
            MenuChoice("Off", selected == false) { onPick(false) },
            MenuChoice("On", selected == true) { onPick(true) },
        ))))
    }
}
