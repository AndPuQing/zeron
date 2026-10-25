package sh.zeron.android.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.outlined.Home
import sh.zeron.android.design.ActivityIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import sh.zeron.android.design.GeistMono
import sh.zeron.android.design.LocalDarkTheme
import sh.zeron.android.design.ProjectColors
import sh.zeron.android.design.glassControlFill
import uniffi.zeron_core.ChatIndicator
import uniffi.zeron_core.SessionRow
import uniffi.zeron_core.projectColorIndex

/**
 * Continuous outer corners for grouped settings on both platforms.
 */
fun segmentShape(index: Int, count: Int, outer: Dp = 20.dp, inner: Dp = 0.dp): Shape {
    val top = if (index == 0) outer else inner
    val bottom = if (index == count - 1) outer else inner
    return RoundedCornerShape(topStart = top, topEnd = top, bottomStart = bottom, bottomEnd = bottom)
}

/** A section title above a group. */
@Composable
fun SectionHeader(text: String, modifier: Modifier = Modifier, trailing: @Composable (() -> Unit)? = null) {
    Box(modifier.fillMaxWidth().padding(start = 20.dp, end = 12.dp, top = 20.dp, bottom = 8.dp)) {
        Text(
            text,
            style = MaterialTheme.typography.labelLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.align(Alignment.CenterStart),
        )
        trailing?.let { Box(Modifier.align(Alignment.CenterEnd)) { it() } }
    }
}

/** Project monogram tile, toned by the core's stable project color. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun ProjectTile(name: String?, colorIndex: Int, size: Dp = 40.dp, modifier: Modifier = Modifier) {
    val tone = ProjectColors.color(colorIndex, LocalDarkTheme.current)
    val shape = if (name == null) CircleShape else RoundedCornerShape(size * (3f / 13f))
    Box(
        modifier.size(size).clip(shape).background(tone.copy(alpha = 0.08f)),
        contentAlignment = Alignment.Center,
    ) {
        if (name != null) {
            val fontSize = (size.value * if (size < 28.dp) 0.62f else 0.42f).sp
            Text(
                name.trim().take(1).uppercase(),
                color = tone.copy(alpha = 0.85f),
                fontFamily = GeistMono,
                fontWeight = FontWeight.Medium,
                fontSize = fontSize,
                style = androidx.compose.ui.text.TextStyle(
                    lineHeight = fontSize,
                    lineHeightStyle = androidx.compose.ui.text.style.LineHeightStyle(
                        androidx.compose.ui.text.style.LineHeightStyle.Alignment.Center,
                        androidx.compose.ui.text.style.LineHeightStyle.Trim.Both,
                    ),
                ),
            )
        } else {
            sh.zeron.android.design.ZIcon(sh.zeron.android.design.ZIcons.Home, null, Modifier.size(size * 0.5f), tint = tone)
        }
    }
}

fun SessionRow.colorIndex(): Int = (project?.colorIndex ?: projectColorIndex("home")).toInt()

@Composable
fun successColor(): Color = if (LocalDarkTheme.current) Color(0xFF34D399) else Color(0xFF15803D)

/** Live status at the trailing edge of a session row (desktop wording). */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun StatusLabel(row: SessionRow) {
    @Composable
    fun label(text: String, color: Color, dot: Boolean) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (dot) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(color))
            } else {
                sh.zeron.android.design.ZIcon(sh.zeron.android.design.ZIcons.Check, null, Modifier.size(15.dp), tint = color)
            }
            Spacer(Modifier.width(5.dp))
            Text(text, style = MaterialTheme.typography.labelMedium, color = color)
        }
    }
    when (row.indicator) {
        ChatIndicator.WORKING -> Row(verticalAlignment = Alignment.CenterVertically) {
            ActivityIndicator(Modifier.size(12.dp))
            Spacer(Modifier.width(5.dp))
            Text("Working", style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
        ChatIndicator.AWAITING_INPUT -> label("Input", MaterialTheme.colorScheme.primary, dot = true)
        ChatIndicator.ERRORED -> label("Failed", MaterialTheme.colorScheme.error, dot = true)
        ChatIndicator.COMPLETED, ChatIndicator.IDLE -> if (row.indicator == ChatIndicator.COMPLETED && row.unseen) {
            label("Done", successColor(), dot = false)
        } else Row(verticalAlignment = Alignment.CenterVertically) {
            if (row.unseen) {
                Box(Modifier.size(7.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                Spacer(Modifier.width(5.dp))
            }
            Text(row.timeLabel, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.outline)
        }
    }
}

@Composable
fun chipContainer(): Color = glassControlFill()

/** Segmented list shapes, with a lone item rounded like a whole group. */
@Composable
fun segmentedShapes(index: Int, count: Int): androidx.compose.material3.ListItemShapes =
    if (count == 1) {
        androidx.compose.material3.ListItemDefaults.shapes(shape = RoundedCornerShape(24.dp))
    } else {
        androidx.compose.material3.ListItemDefaults.segmentedShapes(index, count)
    }

/** Inside a full-screen dialog: status and navigation bar icons that read on the page (dark on light). */
@Composable
fun DialogSystemBars(lightIcons: Boolean = LocalDarkTheme.current) {
    val view = androidx.compose.ui.platform.LocalView.current
    androidx.compose.runtime.SideEffect {
        val window = (view.parent as? androidx.compose.ui.window.DialogWindowProvider)?.window ?: return@SideEffect
        androidx.core.view.WindowCompat.getInsetsController(window, view).apply {
            isAppearanceLightStatusBars = !lightIcons
            isAppearanceLightNavigationBars = !lightIcons
        }
    }
}

@Composable
fun composerContainer(): Color =
    if (LocalDarkTheme.current) MaterialTheme.colorScheme.surfaceContainerHigh else MaterialTheme.colorScheme.surfaceContainerLowest

@Composable
fun HarnessTile(row: uniffi.zeron_core.SessionRow) {
    Box(Modifier.size(40.dp), contentAlignment = Alignment.Center) {
        sh.zeron.android.design.HarnessMark(row.harness, 24.dp, tint = MaterialTheme.colorScheme.onSurface)
    }
}
