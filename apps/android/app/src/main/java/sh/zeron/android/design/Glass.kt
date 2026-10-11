package sh.zeron.android.design

import android.os.Build
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.compositeOver
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.dp
import com.kyant.backdrop.backdrops.LayerBackdrop
import com.kyant.backdrop.backdrops.layerBackdrop
import com.kyant.backdrop.backdrops.rememberLayerBackdrop
import com.kyant.backdrop.drawBackdrop
import com.kyant.backdrop.effects.blur
import com.kyant.backdrop.effects.lens
import com.kyant.backdrop.highlight.Highlight
import com.kyant.backdrop.shadow.Shadow

/** Keep in sync with Glass.Metrics in the UIKit shell (points ↔ dp). */
object GlassMetrics {
    val control = 44.dp
    val composerRadius = 25.dp
    val cardRadius = 26.dp
    val capsule = RoundedCornerShape(50)
}

private val LocalGlassBackdrop = staticCompositionLocalOf<LayerBackdrop?> { null }

/**
 * Capture the page once, then let sibling controls sample that layer.
 * A source never samples itself: inline controls use the tonal fallback.
 * No bitmap readback or per-frame CPU blur is involved.
 */
@Composable
fun GlassScene(
    modifier: Modifier = Modifier,
    background: @Composable BoxScope.(source: Modifier) -> Unit,
    content: @Composable BoxScope.() -> Unit,
) {
    val backdrop = rememberLayerBackdrop()
    Box(modifier) {
        CompositionLocalProvider(LocalGlassBackdrop provides null) {
            background(Modifier.layerBackdrop(backdrop))
        }
        CompositionLocalProvider(LocalGlassBackdrop provides backdrop) { content() }
    }
}

/** Quiet foreground controls inside glass, matching Palette.controlFill. */
@Composable
fun glassControlFill(): Color = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.075f)

/** Glass belongs to the controls; child text and icons are painted sharply. */
@Composable
fun GlassSurface(
    modifier: Modifier = Modifier,
    shape: Shape = GlassMetrics.capsule,
    tint: Color = Color.Transparent,
    contentColor: Color = MaterialTheme.colorScheme.onSurface,
    onClick: (() -> Unit)? = null,
    enabled: Boolean = true,
    content: @Composable () -> Unit,
) {
    val backdrop = LocalGlassBackdrop.current
    val dark = LocalDarkTheme.current
    val interactions = remember { MutableInteractionSource() }
    val pressed by interactions.collectIsPressedAsState()
    val scale by animateFloatAsState(
        if (pressed) 0.97f else 1f,
        MaterialTheme.motionScheme.fastSpatialSpec(),
        label = "glass-press",
    )
    // Android 10/11 cannot blur a RenderNode: use a legible opaque material.
    val optical = backdrop != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
    val material = if (dark) Color(0xFF19191C) else Color.White
    val wash = material.copy(alpha = if (dark) 0.72f else 0.64f)
    val edge = Color.White.copy(alpha = if (dark) 0.15f else 0.65f)
    val optics = if (optical) Modifier.drawBackdrop(
        backdrop = backdrop,
        shape = { shape },
        effects = {
            blur(18.dp.toPx())
            lens(10.dp.toPx(), 14.dp.toPx())
        },
        highlight = { Highlight(width = 0.75.dp, alpha = if (dark) 0.45f else 0.7f) },
        shadow = { Shadow(radius = 12.dp, color = Color.Black.copy(alpha = if (dark) 0.2f else 0.08f)) },
        onDrawSurface = { drawRect(wash); drawRect(tint) },
    ) else Modifier
    val surfaceModifier = modifier
        .graphicsLayer { scaleX = scale; scaleY = scale }
        .then(optics)
    val fill = if (optical) Color.Transparent else tint.compositeOver(material)
    val border = if (optical) null else BorderStroke(0.5.dp, edge)
    if (onClick == null) {
        Surface(surfaceModifier, shape = shape, color = fill, contentColor = contentColor, border = border, content = content)
    } else {
        Surface(
            onClick = onClick,
            enabled = enabled,
            modifier = surfaceModifier,
            shape = shape,
            color = fill,
            contentColor = contentColor,
            border = border,
            interactionSource = interactions,
            content = content,
        )
    }
}
