package sh.zeron.android.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.*
import uniffi.zeron_core.CoreClient

internal fun accessModeLabel(mode: String): String = when (mode) {
    "supervised" -> "Supervised"
    "auto-accept-edits" -> "Auto-accept edits"
    "full-access" -> "Full access"
    else -> mode
}

/** Capability choices come from the same Rust policy used by the host. */
@Composable
internal fun ExecutionChips(
    client: CoreClient, harness: String, deviceId: String?, access: String, plan: Boolean,
    onPick: (String?, Boolean?) -> Unit,
) {
    var menu by remember { mutableStateOf<String?>(null) }
    // Read the workspace's current host capability; reconnecting can upgrade it.
    val caps = runCatching { client.executionCapabilities(harness, deviceId) }.getOrNull()
    val fullAccessColor = warningColor()
    ContextChip(
        accessModeLabel(access), leading = {}, onClick = { menu = "access" },
        tint = if (access == "full-access") fullAccessColor else MaterialTheme.colorScheme.onSurfaceVariant,
    ) {
        ChoiceMenu(menu == "access", { menu = null }, listOf(MenuSection(
            caps?.reason ?: "Permissions · applies next turn",
            (caps?.accessModes ?: listOf("full-access")).map { mode ->
                MenuChoice(
                    accessModeLabel(mode), mode == access,
                    tint = if (mode == "full-access") fullAccessColor else null,
                ) { onPick(mode, null) }
            },
        )))
    }
    ContextChip(if (plan) "Plan on" else "Plan off", leading = {}, onClick = { menu = "plan" }) {
        ChoiceMenu(menu == "plan", { menu = null }, listOf(MenuSection(
            caps?.reason ?: "Plan · applies next turn",
            listOfNotNull(
                MenuChoice("Off", !plan) { onPick(null, false) },
                if (caps?.supportsPlan == true) MenuChoice("On", plan, "Plan before executing") { onPick(null, true) } else null,
            ),
        )))
    }
}
