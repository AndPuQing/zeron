package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.CancellationException
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.glassControlFill
import uniffi.zeron_core.ComposerCatalog
import uniffi.zeron_core.ComposerCommand

/** Fetch once per opening/context; typing filters locally without stale query results. */
@Composable
fun CommandSuggestions(
    model: ComposerModel,
    chatId: String,
    harness: String?,
    deviceId: String,
    modifier: Modifier = Modifier,
    load: suspend () -> ComposerCatalog,
) {
    val query = model.completionQuery() ?: return
    val kind = if (query.skills) "skills" else "commands"
    val tag = if (query.skills) "skill" else "slash"
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
    val rows = catalog?.let { model.completionRows(it, query) }.orEmpty()
    val selectionContext = model.commandContext
    val scroll = rememberScrollState()
    LaunchedEffect(query) { scroll.scrollTo(0) }
    GlassSurface(
        shape = RoundedCornerShape(12.dp),
        modifier = modifier.fillMaxWidth().padding(bottom = 8.dp).testTag("$tag-menu"),
    ) {
        // Six 48 dp touch rows plus the 4 dp card inset; iOS's 280 grows by the
        // extra touch-row height. The inset sits outside the scroller so a
        // seventh row cannot peek through as a clipped sliver.
        Column(Modifier.heightIn(max = 296.dp).padding(4.dp).verticalScroll(scroll)) {
            rows.forEach { command ->
                val enabled = command.unavailableReason == null
                CommandSuggestionRow(
                    title = commandDisplayName(command),
                    detail = commandDetail(command),
                    icon = if (command.token.startsWith("$")) ZIcons.Magic else ZIcons.Command,
                    enabled = enabled,
                    onClick = { model.insertCommand(command, selectionContext, query) },
                    testTag = "$tag-${command.token}",
                )
            }
            val warning = error ?: catalog?.warning
            if (warning != null) {
                CommandSuggestionRow(
                    title = "Retry loading $kind",
                    detail = warning,
                    icon = ZIcons.Refresh,
                    enabled = true,
                    onClick = { retry++ },
                    testTag = "$tag-status",
                )
            } else if (rows.isEmpty()) {
                CommandSuggestionRow(
                    title = if (catalog == null) "Loading $kind…" else "No matching $kind",
                    detail = "",
                    icon = ZIcons.Command,
                    enabled = false,
                    onClick = {},
                    testTag = "$tag-status",
                )
            }
        }
    }
}

/** One desktop-style completion row: icon, medium name and muted inline detail. */
@Composable
private fun CommandSuggestionRow(
    title: String,
    detail: String,
    icon: Int,
    enabled: Boolean,
    onClick: () -> Unit,
    testTag: String,
) {
    val interaction = remember { MutableInteractionSource() }
    val pressed by interaction.collectIsPressedAsState()
    val titleStyle = MaterialTheme.typography.labelMedium
    val detailStyle = MaterialTheme.typography.bodySmall.copy(fontSize = 12.5.sp, lineHeight = 16.sp)
    BoxWithConstraints(Modifier.fillMaxWidth()) {
        val titleMax = maxWidth * 0.55f
        Row(
            Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .clip(RoundedCornerShape(7.dp))
                .background(if (pressed && enabled) glassControlFill() else Color.Transparent)
                .clickable(
                    enabled = enabled,
                    role = Role.Button,
                    interactionSource = interaction,
                    indication = null,
                    onClick = onClick,
                )
                .semantics(mergeDescendants = true) {
                    contentDescription = if (detail.isEmpty()) title else "$title. $detail"
                    if (!enabled) disabled()
                }
                .testTag(testTag)
                .alpha(if (enabled) 1f else 0.5f)
                .padding(horizontal = 8.dp, vertical = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            ZIcon(icon, null, Modifier.size(16.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(
                title,
                style = titleStyle,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = if (detail.isEmpty()) Modifier.weight(1f) else Modifier.widthIn(max = titleMax),
            )
            if (detail.isNotEmpty()) {
                Text(
                    detail,
                    style = detailStyle,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
            }
        }
    }
}

/** Desktop `skill_display_name`, plus the catalog's `:N` same-name disambiguation. */
internal fun commandDisplayName(command: ComposerCommand): String {
    val token = command.token
    if (!token.startsWith("$")) return token
    val suffix = disambiguationSuffix(token)
    if (suffix != null) {
        val base = canonicalToken(command) ?: return token
        if (token == "$base:$suffix") return "${desktopSkillName(base)} · $suffix"
    }
    return desktopSkillName(token)
}

internal fun commandDetail(command: ComposerCommand): String = buildList {
    if (command.description.isNotEmpty()) add(command.description)
    command.unavailableReason?.takeIf { it.isNotEmpty() }?.let { add(it) }
    command.inputHint?.takeIf { it.isNotEmpty() }?.let { add("<$it>") }
}.joinToString(" · ")

private fun desktopSkillName(token: String): String =
    token.drop(1).substringAfterLast(':').split(Regex("[-_\\s]+"))
        .filter { it.isNotEmpty() }
        .joinToString(" ") { word -> word.first().uppercase() + word.drop(1).lowercase() }

/** Only a trailing numeric `:N` (N >= 2) can be a catalog suffix; verify its base. */
private fun disambiguationSuffix(token: String): String? {
    val colon = token.lastIndexOf(':')
    if (colon < 0) return null
    val suffix = token.substring(colon + 1)
    if (suffix.isEmpty() || suffix.any { !it.isDigit() }) return null
    return suffix.takeIf { (it.toIntOrNull() ?: 0) >= 2 }
}

private fun canonicalToken(command: ComposerCommand): String? {
    val restored = uniffi.zeron_core.restoreComposerDraft(command.insertion)
    if (restored.selections.size != 1) return null
    if (restored.selections[0].insertion != command.insertion) return null
    if (!restored.text.startsWith("$")) return null
    return restored.text
}
