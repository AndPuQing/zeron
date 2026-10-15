package sh.zeron.android.transcript

import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.DashPathEffect
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Color
import uniffi.zeron_core.FrameImage
import uniffi.zeron_core.Rgba
import uniffi.zeron_core.VisualPaint
import uniffi.zeron_core.VisualShape
import java.nio.ByteBuffer
import kotlin.math.ceil

/** Rasterizes the shared worker's display lists on that worker, never on the UI thread. */
internal object VisualPainter {
    private fun color(c: Rgba) = Color.argb(c.a, c.r, c.g, c.b)

    fun render(paint: VisualPaint, width: Float, height: Float, scale: Float, fonts: StyleFonts): Bitmap? {
        if (width <= 0 || height <= 0 || (paint.shapes.isEmpty() && paint.runs.isEmpty())) return null
        val bitmap = Bitmap.createBitmap(ceil(width * scale).toInt(), ceil(height * scale).toInt(), Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.scale(scale, scale)
        val brush = Paint(Paint.ANTI_ALIAS_FLAG).apply {
            strokeJoin = Paint.Join.ROUND
            strokeCap = Paint.Cap.ROUND
        }
        for (shape in paint.shapes) {
            when (shape) {
                is VisualShape.Rect -> {
                    brush.style = Paint.Style.FILL
                    brush.color = color(shape.color)
                    val r = minOf(shape.radius, shape.w / 2, shape.h / 2).coerceAtLeast(0f)
                    canvas.drawRoundRect(shape.x, shape.y, shape.x + shape.w, shape.y + shape.h, r, r, brush)
                }
                is VisualShape.Path -> {
                    if (shape.points.size < 4) continue
                    val path = Path().apply {
                        moveTo(shape.points[0], shape.points[1])
                        for (i in 2 until shape.points.size - 1 step 2) lineTo(shape.points[i], shape.points[i + 1])
                        if (shape.closed) close()
                    }
                    shape.fill?.let {
                        brush.style = Paint.Style.FILL
                        brush.color = color(it)
                        canvas.drawPath(path, brush)
                    }
                    shape.stroke?.let {
                        brush.style = Paint.Style.STROKE
                        brush.strokeWidth = shape.lineWidth
                        brush.color = color(it)
                        brush.pathEffect = shape.dash?.takeIf { dash -> dash > 0 }?.let { dash ->
                            DashPathEffect(floatArrayOf(dash, dash), 0f)
                        }
                        canvas.drawPath(path, brush)
                        brush.pathEffect = null
                    }
                }
            }
        }
        for (run in paint.runs) {
            val start = run.start.toLong()
            val end = start + run.len.toLong()
            if (end > paint.text.length) continue
            val font = fonts[run.style] ?: continue
            font.color = color(run.color)
            canvas.drawText(paint.text, start.toInt(), end.toInt(), run.x, run.baseline, font)
        }
        return bitmap
    }

    /** Android bitmaps store premultiplied RGBA bytes; the core sends premultiplied BGRA. */
    fun image(frame: FrameImage): Bitmap? {
        val w = frame.width.toLong()
        val h = frame.height.toLong()
        if (w <= 0 || h <= 0 || w * h * 4 != frame.bgra.size.toLong()) return null
        val rgba = frame.bgra.copyOf()
        for (i in rgba.indices step 4) {
            val blue = rgba[i]
            rgba[i] = rgba[i + 2]
            rgba[i + 2] = blue
        }
        return Bitmap.createBitmap(w.toInt(), h.toInt(), Bitmap.Config.ARGB_8888).apply {
            copyPixelsFromBuffer(ByteBuffer.wrap(rgba))
        }
    }
}
