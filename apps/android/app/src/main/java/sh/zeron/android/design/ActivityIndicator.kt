package sh.zeron.android.design

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.progressSemantics
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.unit.dp

/** The mobile shell's quiet, twelve-spoke activity glyph. */
@Composable
fun ActivityIndicator(modifier: Modifier = Modifier.size(16.dp), color: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    val phase by rememberInfiniteTransition(label = "activity").animateFloat(
        0f, 360f, infiniteRepeatable(tween(1000, easing = LinearEasing)), label = "rotation",
    )
    Canvas(modifier.progressSemantics()) {
        val radius = size.minDimension / 2
        rotate(phase) {
            repeat(12) { i ->
                rotate(i * 30f) {
                    drawLine(color.copy(alpha = color.alpha * (1f - i / 15f)),
                        Offset(center.x, center.y - radius * 0.48f), Offset(center.x, center.y - radius * 0.88f),
                        strokeWidth = radius * 0.2f, cap = StrokeCap.Round)
                }
            }
        }
    }
}
