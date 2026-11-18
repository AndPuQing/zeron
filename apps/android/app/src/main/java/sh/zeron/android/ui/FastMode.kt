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
import uniffi.zeron_core.ModelOption

/** Catalog wire values, shared by existing chats and new-session drafts. */
internal data class FastModeOption(val id: String, val on: String, val off: String) {
    fun enabled(values: Map<String, String>): Boolean? = values[id]?.let {
        it == on || (on == "on" && it == "true") || (id == "serviceTier" && it == "priority")
    }
    fun title(values: Map<String, String>): String = when (enabled(values)) {
        true -> "Fast on"
        false -> "Fast off"
        null -> "Fast: Default"
    }
    fun applying(enabled: Boolean?, values: Map<String, String>): Map<String, String> =
        if (enabled == null) values - id else values + (id to if (enabled) on else off)

    companion object {
        /** Same-provider model changes must keep Codex's explicit reset of native/global Fast. */
        fun afterModelChange(values: Map<String, String>, harness: String, previousOptionId: String?): Map<String, String> {
            val id = previousOptionId ?: return values
            if (harness == "codex" && id == "serviceTier" && values[id] == "default") return values
            return values - id
        }

        fun find(options: List<ModelOption>): FastModeOption? = options.firstNotNullOfOrNull { option ->
            val choices = option.choices.map { it.id }
            val on = when {
                option.id in listOf("fastMode", "fast_mode") && "on" in choices -> "on"
                option.id == "fast" && "true" in choices -> "true"
                "fast" in choices -> "fast"
                else -> return@firstNotNullOfOrNull null
            }
            val other = choices.firstOrNull { it != on } ?: return@firstNotNullOfOrNull null
            FastModeOption(option.id, on, option.defaultChoice.takeIf { it != on && it in choices } ?: other)
        }
    }
}

@Composable
internal fun FastModeChip(option: FastModeOption, values: Map<String, String>, onPick: (Boolean?) -> Unit) {
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
