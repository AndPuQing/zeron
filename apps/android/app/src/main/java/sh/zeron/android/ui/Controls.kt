package sh.zeron.android.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.ProvideTextStyle
import androidx.compose.material3.Surface
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntRect
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.Popup
import androidx.compose.ui.window.PopupPositionProvider
import androidx.compose.ui.window.PopupProperties
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.glassControlFill

/** A quiet pressed wash, shared by lists and controls instead of a ripple. */
fun Modifier.pressable(
    enabled: Boolean = true,
    role: Role = Role.Button,
    onLongClick: (() -> Unit)? = null,
    onClick: () -> Unit,
): Modifier = composed {
    val source = remember { MutableInteractionSource() }
    val pressed by source.collectIsPressedAsState()
    val wash by animateColorAsState(if (pressed) glassControlFill() else Color.Transparent, label = "pressed")
    background(wash).combinedClickable(
        enabled = enabled,
        role = role,
        interactionSource = source,
        indication = null,
        onLongClick = onLongClick,
        onClick = onClick,
    )
}

@Composable
fun PlainIconButton(
    @DrawableRes icon: Int,
    label: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    tint: Color = MaterialTheme.colorScheme.onSurface,
) {
    Box(
        modifier.size(44.dp).clip(CircleShape).pressable(enabled = enabled, onClick = onClick)
            .semantics { contentDescription = label },
        contentAlignment = Alignment.Center,
    ) {
        ZIcon(icon, null, Modifier.size(20.dp), tint = tint.copy(alpha = if (enabled) tint.alpha else 0.3f))
    }
}

@Composable
fun AppTextButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    color: Color = MaterialTheme.colorScheme.primary,
    content: @Composable RowScope.() -> Unit,
) {
    CompositionLocalProvider(LocalContentColor provides color.copy(alpha = if (enabled) color.alpha else 0.3f)) {
        ProvideTextStyle(MaterialTheme.typography.bodyLarge) {
            Row(
                modifier.heightIn(min = 44.dp).clip(RoundedCornerShape(10.dp)).pressable(enabled = enabled, onClick = onClick)
                    .padding(horizontal = 12.dp),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
                content = content,
            )
        }
    }
}

@Composable
fun AppButton(
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    container: Color = MaterialTheme.colorScheme.primary,
    foreground: Color = Color.White,
    content: @Composable RowScope.() -> Unit,
) {
    Surface(
        modifier = modifier,
        shape = CircleShape,
        color = if (enabled) container else glassControlFill(),
        contentColor = if (enabled) foreground else MaterialTheme.colorScheme.onSurfaceVariant.copy(alpha = 0.4f),
    ) {
        ProvideTextStyle(MaterialTheme.typography.bodyLargeEmphasized) {
            Row(
                Modifier.heightIn(min = 46.dp).pressable(enabled = enabled, onClick = onClick).padding(horizontal = 20.dp),
                horizontalArrangement = Arrangement.Center,
                verticalAlignment = Alignment.CenterVertically,
                content = content,
            )
        }
    }
}

/** Inset grouped cells use a shared fill and a hairline, with no gaps. */
@Composable
fun GroupedRow(
    index: Int,
    count: Int,
    modifier: Modifier = Modifier,
    onClick: (() -> Unit)? = null,
    leadingContent: (@Composable () -> Unit)? = null,
    supportingContent: (@Composable () -> Unit)? = null,
    trailingContent: (@Composable () -> Unit)? = null,
    content: @Composable () -> Unit,
) {
    Surface(modifier, shape = segmentShape(index, count, outer = 12.dp), color = cardColor()) {
        Column {
            Row(
                Modifier.fillMaxWidth().then(if (onClick != null) Modifier.pressable(onClick = onClick) else Modifier)
                    .heightIn(min = 50.dp).padding(horizontal = 16.dp, vertical = 11.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                leadingContent?.invoke()
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    ProvideTextStyle(MaterialTheme.typography.titleMedium) { content() }
                    if (supportingContent != null) {
                        CompositionLocalProvider(LocalContentColor provides MaterialTheme.colorScheme.onSurfaceVariant) {
                            ProvideTextStyle(MaterialTheme.typography.bodySmall, supportingContent)
                        }
                    }
                }
                trailingContent?.invoke()
            }
            if (index < count - 1) Hairline(Modifier.padding(start = if (leadingContent != null) 50.dp else 16.dp))
        }
    }
}

@Composable
fun Hairline(modifier: Modifier = Modifier) {
    Box(modifier.fillMaxWidth().height(0.5.dp).background(MaterialTheme.colorScheme.outlineVariant))
}

/** System sheet mechanics with the same compact navigation header as iOS. */
@Composable
fun AppSheet(
    title: String,
    onDismiss: () -> Unit,
    actions: @Composable RowScope.() -> Unit = {},
    content: @Composable ColumnScope.() -> Unit,
) {
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        sheetMaxWidth = 560.dp,
        shape = RoundedCornerShape(topStart = 26.dp, topEnd = 26.dp),
        containerColor = MaterialTheme.colorScheme.background,
        contentColor = MaterialTheme.colorScheme.onSurface,
        tonalElevation = 0.dp,
        dragHandle = {
            Box(Modifier.padding(top = 8.dp, bottom = 4.dp).size(32.dp, 5.dp).clip(CircleShape)
                .background(MaterialTheme.colorScheme.onSurface.copy(alpha = 0.18f)))
        },
    ) {
        Column(Modifier.fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp * 0.85f).dp)) {
            Box(Modifier.fillMaxWidth().heightIn(min = 52.dp)) {
                AppTextButton(onClick = onDismiss, modifier = Modifier.align(Alignment.CenterStart).padding(start = 8.dp)) { Text("Close") }
                Text(title, style = MaterialTheme.typography.bodyLargeEmphasized, modifier = Modifier.align(Alignment.Center).padding(horizontal = 96.dp), maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
                Row(Modifier.align(Alignment.CenterEnd).padding(end = 8.dp), verticalAlignment = Alignment.CenterVertically, content = actions)
            }
            content()
        }
    }
}

/** Popups use their own window; a regular material avoids sampling another window's layer. */
@Composable
fun AppMenuPopup(expanded: Boolean, onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    if (!expanded) return
    val density = androidx.compose.ui.platform.LocalDensity.current
    val margin = with(density) { 12.dp.roundToPx() }
    val position = remember(margin) {
        object : PopupPositionProvider {
            override fun calculatePosition(anchorBounds: IntRect, windowSize: IntSize, layoutDirection: LayoutDirection, popupContentSize: IntSize): IntOffset {
                val x = if (layoutDirection == LayoutDirection.Ltr) anchorBounds.right - popupContentSize.width else anchorBounds.left
                val below = anchorBounds.bottom + margin / 2
                val above = anchorBounds.top - popupContentSize.height - margin / 2
                return IntOffset(
                    x.coerceIn(margin, (windowSize.width - popupContentSize.width - margin).coerceAtLeast(margin)),
                    (if (below + popupContentSize.height <= windowSize.height - margin) below else above)
                        .coerceIn(margin, (windowSize.height - popupContentSize.height - margin).coerceAtLeast(margin)),
                )
            }
        }
    }
    Popup(popupPositionProvider = position, onDismissRequest = onDismiss, properties = PopupProperties(focusable = true)) {
        Surface(
            modifier = Modifier.padding(12.dp).widthIn(max = (LocalConfiguration.current.screenWidthDp - 48).dp).width(292.dp),
            shape = RoundedCornerShape(14.dp),
            color = cardColor(),
            shadowElevation = 16.dp,
        ) {
            Column(Modifier.heightIn(max = (LocalConfiguration.current.screenHeightDp * 0.65f).dp)
                .verticalScroll(rememberScrollState()), content = content)
        }
    }
}

data class AlertAction(val label: String, val enabled: Boolean = true, val destructive: Boolean = false, val onClick: () -> Unit)

@Composable
fun AppAlert(
    title: String,
    onDismiss: () -> Unit,
    actions: List<AlertAction>,
    content: @Composable ColumnScope.() -> Unit = {},
) {
    Dialog(onDismissRequest = onDismiss, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Surface(Modifier.padding(24.dp).widthIn(max = 300.dp), shape = RoundedCornerShape(14.dp), color = cardColor()) {
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Text(title, style = MaterialTheme.typography.bodyLargeEmphasized, textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 8.dp))
                ProvideTextStyle(MaterialTheme.typography.bodySmall.copy(textAlign = TextAlign.Center)) {
                    Column(Modifier.fillMaxWidth().heightIn(max = (LocalConfiguration.current.screenHeightDp * 0.5f).dp)
                        .verticalScroll(rememberScrollState()).padding(start = 16.dp, end = 16.dp, bottom = 20.dp),
                        horizontalAlignment = Alignment.CenterHorizontally, content = content)
                }
                Hairline()
                Row(Modifier.fillMaxWidth()) {
                    actions.forEachIndexed { i, action ->
                        if (i > 0) Box(Modifier.width(0.5.dp).height(46.dp).background(MaterialTheme.colorScheme.outlineVariant))
                        AppTextButton(action.onClick, Modifier.weight(1f), enabled = action.enabled,
                            color = if (action.destructive) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.primary) {
                            Text(action.label, textAlign = TextAlign.Center)
                        }
                    }
                }
            }
        }
    }
}

@Composable
fun AppTextField(
    value: String,
    onValueChange: (String) -> Unit,
    modifier: Modifier = Modifier,
    singleLine: Boolean = false,
    minLines: Int = 1,
    textStyle: TextStyle = MaterialTheme.typography.bodyLarge,
) {
    BasicTextField(
        value, onValueChange,
        singleLine = singleLine, minLines = minLines,
        textStyle = textStyle.copy(color = MaterialTheme.colorScheme.onSurface),
        cursorBrush = SolidColor(MaterialTheme.colorScheme.primary),
        modifier = modifier.clip(RoundedCornerShape(10.dp)).background(glassControlFill()).padding(horizontal = 12.dp, vertical = 10.dp),
    )
}

@Composable
fun AppSnackbarHost(state: SnackbarHostState, modifier: Modifier = Modifier) {
    SnackbarHost(state, modifier) { data ->
        sh.zeron.android.design.GlassSurface(shape = RoundedCornerShape(22.dp), modifier = Modifier.padding(horizontal = 16.dp)) {
            Row(Modifier.padding(start = 16.dp, end = 8.dp).heightIn(min = 46.dp),
                verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(data.visuals.message, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f, fill = false).padding(vertical = 10.dp))
                data.visuals.actionLabel?.let { label -> AppTextButton(onClick = { data.performAction() }) { Text(label) } }
                if (data.visuals.withDismissAction) PlainIconButton(sh.zeron.android.design.ZIcons.Close, "Dismiss", { data.dismiss() })
            }
        }
    }
}
