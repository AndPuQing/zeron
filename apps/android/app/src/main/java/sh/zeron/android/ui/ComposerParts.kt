package sh.zeron.android.ui

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import sh.zeron.android.design.GeistMono
import sh.zeron.android.design.GlassMetrics
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import uniffi.zeron_core.FileMatch

/**
 * The composer, shared by sessions and the new-session page. One glass
 * surface with two states: a resting one-line capsule — [+] Message… [↑] —
 * and, while focused or holding a draft, a card with staged photos and the
 * full-width prompt above a toolbar of context chips.
 */
@Composable
fun ComposerSurface(
    model: ComposerModel,
    placeholder: String,
    action: ComposerAction,
    onAction: () -> Unit,
    modifier: Modifier = Modifier,
    alwaysCard: Boolean = false,
    attach: Boolean = true,
    focusRequester: FocusRequester = remember { FocusRequester() },
    chips: @Composable RowScope.() -> Unit = {},
) {
    var focused by remember { mutableStateOf(false) }
    val card = alwaysCard || focused || model.hasContent
    val selectionError = model.selectionError
    GlassSurface(
        shape = RoundedCornerShape(GlassMetrics.composerRadius),
        modifier = modifier.fillMaxWidth(),
    ) {
        Column(Modifier.animateContentSize(MaterialTheme.motionScheme.defaultSpatialSpec())) {
            (selectionError ?: model.localError)?.let {
                Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
            }
            AnimatedVisibility(model.images.isNotEmpty() && card) { StagedThumbnails(model) }
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.heightIn(min = 56.dp)) {
                if (!card) {
                    Spacer(Modifier.width(4.dp))
                    AttachButton(model, attach)
                } else {
                    Spacer(Modifier.width(20.dp))
                }
                Box(Modifier.weight(1f).padding(vertical = if (card) 16.dp else 8.dp)) {
                    if (model.text.isEmpty()) {
                        Text(placeholder, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
                    }
                    BasicTextField(
                        model.value,
                        { model.value = it },
                        textStyle = MaterialTheme.typography.bodyLarge.copy(color = MaterialTheme.colorScheme.onSurface),
                        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
                        maxLines = if (card) 8 else 2,
                        modifier = Modifier
                            .fillMaxWidth()
                            .focusRequester(focusRequester)
                            .onFocusChanged { focused = it.isFocused },
                    )
                }
                if (!card) {
                    Spacer(Modifier.width(8.dp))
                    ActionButton(action, model.hasContent && selectionError == null, onAction)
                    Spacer(Modifier.width(8.dp))
                } else {
                    Spacer(Modifier.width(16.dp))
                }
            }
            AnimatedVisibility(card, enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically()) {
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    modifier = Modifier.padding(start = 4.dp, end = 8.dp, bottom = 8.dp),
                ) {
                    AttachButton(model, attach)
                    Row(
                        Modifier
                            .weight(1f)
                            .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
                            .drawWithContent {
                                drawContent()
                                // Chips scrolling under the send button fade out.
                                val fade = 24.dp.toPx()
                                drawRect(
                                    Brush.horizontalGradient(listOf(Color.Black, Color.Transparent), startX = size.width - fade, endX = size.width),
                                    topLeft = Offset(size.width - fade, 0f),
                                    blendMode = BlendMode.DstIn,
                                )
                            }
                            .horizontalScroll(rememberScrollState()),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                        content = chips,
                    )
                    Spacer(Modifier.width(8.dp))
                    ActionButton(action, model.hasContent && selectionError == null, onAction)
                }
            }
        }
    }
}

/** Staged photos above the prompt, each removable. */
@Composable
private fun StagedThumbnails(model: ComposerModel) {
    Row(
        Modifier.horizontalScroll(rememberScrollState()).padding(start = 12.dp, end = 12.dp, top = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        for (image in model.images) {
            Box(Modifier.size(72.dp)) {
                Image(image.thumb, "Attached image", Modifier.fillMaxSize().clip(RoundedCornerShape(16.dp)), contentScale = ContentScale.Crop)
                Surface(
                    onClick = { model.images.remove(image) },
                    shape = CircleShape,
                    color = MaterialTheme.colorScheme.inverseSurface.copy(alpha = 0.85f),
                    contentColor = MaterialTheme.colorScheme.inverseOnSurface,
                    modifier = Modifier.align(Alignment.TopEnd).padding(4.dp).size(22.dp),
                ) {
                    Box(contentAlignment = Alignment.Center) { ZIcon(ZIcons.Close, "Remove image", Modifier.size(14.dp)) }
                }
            }
        }
    }
}

/** [+] → Photos / Files / Paste image, staged into the composer. */
@Composable
private fun AttachButton(model: ComposerModel, enabled: Boolean) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var open by remember { mutableStateOf(false) }
    fun add(uris: List<Uri>) {
        scope.launch {
            for (uri in uris.take(Staging.MAX_IMAGES - model.images.size)) Staging.stage(context, uri)?.let { model.images.add(it) }
        }
    }
    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(Staging.MAX_IMAGES)) { add(it) }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { add(it) }
    Box {
        PlainIconButton(ZIcons.Plus, "Attach", onClick = { open = true }, enabled = enabled && model.images.size < Staging.MAX_IMAGES, tint = MaterialTheme.colorScheme.onSurfaceVariant)
        val clip = if (open) context.getSystemService(android.content.ClipboardManager::class.java)?.primaryClip else null
        val pasted = clip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.uri
            ?.takeIf { context.contentResolver.getType(it)?.startsWith("image/") == true }
        ActionMenu(
            open,
            { open = false },
            listOfNotNull(
                MenuAction("Photos", ZIcons.Image) { photos.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                MenuAction("Files", ZIcons.Folder) { files.launch(arrayOf("image/*")) },
                pasted?.let { uri -> MenuAction("Paste image", ZIcons.Copy) { add(listOf(uri)) } },
            ),
        )
    }
}

/** Files matching the `@query` being typed, shown above the composer. */
@Composable
fun MentionSuggestions(model: ComposerModel, search: suspend (String) -> List<FileMatch>, modifier: Modifier = Modifier) {
    val query = model.activeQuery()?.second
    var files by remember { mutableStateOf<List<FileMatch>>(emptyList()) }
    LaunchedEffect(query) {
        if (query == null) {
            files = emptyList()
            return@LaunchedEffect
        }
        delay(120)
        files = runCatching { search(query) }.getOrDefault(emptyList()).take(5)
    }
    AnimatedVisibility(query != null && files.isNotEmpty(), enter = fadeIn() + expandVertically(), exit = fadeOut() + shrinkVertically(), modifier = modifier) {
        GlassSurface(
            shape = RoundedCornerShape(18.dp),
            modifier = Modifier.fillMaxWidth().padding(bottom = 8.dp),
        ) {
            Column(Modifier.padding(6.dp)) {
                files.forEach { file ->
                    val name = file.path.trimEnd('/').substringAfterLast('/')
                    val dir = file.path.trimEnd('/').substringBeforeLast('/', "")
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(18.dp))
                            .clickable { model.insertMention(file) }
                            .padding(horizontal = 14.dp, vertical = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        ZIcon(if (file.isDir) ZIcons.Folder else ZIcons.Text, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Spacer(Modifier.width(12.dp))
                        Column {
                            Text(name, style = MaterialTheme.typography.bodyLargeEmphasized, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (dir.isNotEmpty()) {
                                Text(dir, style = MaterialTheme.typography.bodySmall, fontFamily = GeistMono, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** System-style choice groups: a checkmark, never a filled selection tile. */
@Composable
fun ChoiceMenu(expanded: Boolean, onDismiss: () -> Unit, sections: List<MenuSection>) {
    AppMenuPopup(expanded, onDismiss) {
        sections.filter { it.choices.isNotEmpty() }.forEachIndexed { group, section ->
            if (group > 0) Box(Modifier.fillMaxWidth().size(6.dp).background(sh.zeron.android.design.glassControlFill()))
            section.title?.let { title ->
                Text(title, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp, bottom = 6.dp))
            }
            section.choices.forEachIndexed { index, choice ->
                if (index > 0) Hairline(Modifier.padding(start = 44.dp))
                MenuRow(choice.label, choice.supporting, selected = choice.selected, selectable = true,
                    leading = { Box(Modifier.size(16.dp)) { if (choice.selected) ZIcon(ZIcons.Check, null, Modifier.size(16.dp)) } },
                    trailing = choice.leading,
                ) {
                    choice.onClick()
                    onDismiss()
                }
            }
        }
    }
}
