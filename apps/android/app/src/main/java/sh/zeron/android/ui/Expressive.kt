package sh.zeron.android.ui

import androidx.annotation.DrawableRes
import androidx.compose.animation.animateColorAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsTopHeight
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import sh.zeron.android.design.GlassMetrics
import sh.zeron.android.design.GlassSurface
import sh.zeron.android.design.ZIcon
import sh.zeron.android.design.ZIcons
import sh.zeron.android.design.glassControlFill

/** Round glass toolbar control, matching the UIKit glass button. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun TonalCircleButton(
    @DrawableRes icon: Int,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    size: Dp = GlassMetrics.control,
    container: Color = Color.Transparent,
    content: Color = MaterialTheme.colorScheme.onSurface,
) {
    GlassSurface(
        onClick = onClick,
        tint = container.copy(alpha = if (container == Color.Transparent) 0f else 0.16f),
        contentColor = content,
        modifier = modifier.size(size),
    ) {
        Box(contentAlignment = Alignment.Center) { ZIcon(icon, contentDescription, Modifier.size(20.dp), tint = content) }
    }
}

/** Page title and glass actions, with the same 30pt title hierarchy as iOS. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun ScreenHeader(title: String, subtitle: String?, modifier: Modifier = Modifier, actions: @Composable RowScope.() -> Unit = {}) {
    Row(
        modifier.fillMaxWidth().padding(start = 24.dp, end = 16.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.headlineLarge, color = MaterialTheme.colorScheme.onSurface)
            if (subtitle != null) {
                Spacer(Modifier.height(2.dp))
                Text(subtitle, style = MaterialTheme.typography.bodyLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), content = actions)
    }
}

/** A neutral filter capsule with a quiet violet selection. */
@Composable
fun Pill(label: String, selected: Boolean, onClick: () -> Unit, count: Int? = null, @DrawableRes icon: Int? = null) {
    val container by animateColorAsState(
        if (selected) MaterialTheme.colorScheme.primary.copy(alpha = 0.12f) else Color.Transparent,
        MaterialTheme.motionScheme.defaultEffectsSpec(),
        label = "pill",
    )
    val fg = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface
    GlassSurface(onClick = onClick, tint = container, contentColor = fg, modifier = Modifier.semantics { this.selected = selected }) {
        Row(
            Modifier.heightIn(min = 40.dp).padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            icon?.let { ZIcon(it, null, Modifier.size(18.dp)) }
            Text(label, style = MaterialTheme.typography.labelLarge)
            if (count != null && count > 0) {
                Box(
                    Modifier
                        .clip(CircleShape)
                        .background(if (selected) fg.copy(alpha = 0.2f) else MaterialTheme.colorScheme.primary.copy(alpha = 0.14f))
                        .padding(horizontal = 7.dp, vertical = 1.dp),
                ) {
                    Text("$count", style = MaterialTheme.typography.labelMedium, color = if (selected) fg else MaterialTheme.colorScheme.primary)
                }
            }
        }
    }
}

class NavItem(val label: String, @DrawableRes val icon: Int, val selected: Boolean, val onClick: () -> Unit)

/** Bottom navigation as a floating capsule, inset from the screen edges. */
@Composable
fun FloatingNavBar(items: List<NavItem>, modifier: Modifier = Modifier, trailing: (@Composable () -> Unit)? = null) {
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        GlassSurface(
            modifier = Modifier.weight(1f),
        ) {
            Row(Modifier.padding(6.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                for (item in items) {
                    val bg by animateColorAsState(
                        if (item.selected) glassControlFill() else Color.Transparent,
                        MaterialTheme.motionScheme.defaultEffectsSpec(),
                        label = "nav",
                    )
                    Surface(
                        onClick = item.onClick,
                        shape = RoundedCornerShape(30.dp),
                        color = bg,
                        contentColor = if (item.selected) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f).semantics { selected = item.selected; role = Role.Tab },
                    ) {
                        Column(Modifier.padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            ZIcon(item.icon, null, Modifier.size(24.dp))
                            Spacer(Modifier.height(2.dp))
                            Text(item.label, style = if (item.selected) MaterialTheme.typography.labelMediumEmphasized else MaterialTheme.typography.labelMedium)
                        }
                    }
                }
            }
        }
        trailing?.let {
            Spacer(Modifier.width(10.dp))
            it()
        }
    }
}

/**
 * "New session" — the always-there primary action above the nav (the iOS
 * accessory bar), with the live summary of what needs attention.
 */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun NewSessionBar(summary: String?, onClick: () -> Unit, modifier: Modifier = Modifier) {
    GlassSurface(
        onClick = onClick,
        modifier = modifier.fillMaxWidth().padding(horizontal = 16.dp),
    ) {
        Row(Modifier.padding(7.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(
                Modifier.size(34.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary.copy(alpha = 0.12f)),
                contentAlignment = Alignment.Center,
            ) { ZIcon(ZIcons.NewSession, null, Modifier.size(18.dp), tint = MaterialTheme.colorScheme.primary) }
            Spacer(Modifier.width(11.dp))
            Text("New session", style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
            if (summary != null) {
                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(end = 12.dp)) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                    Spacer(Modifier.width(6.dp))
                    Text(summary, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
}

val FloatingChromePadding = PaddingValues(bottom = 176.dp)

/** The page tone behind the status bar once content scrolls under it. */
@Composable
fun BoxScope.StatusBarScrim(scrolled: Boolean) {
    val color by animateColorAsState(
        if (scrolled) MaterialTheme.colorScheme.background else Color.Transparent,
        label = "status-scrim",
    )
    Box(
        Modifier
            .align(Alignment.TopCenter)
            .fillMaxWidth()
            .windowInsetsTopHeight(androidx.compose.foundation.layout.WindowInsets.statusBars)
            .background(color),
    )
}
