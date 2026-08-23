package app.lighting.display

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.RectF
import android.os.SystemClock
import android.util.AttributeSet
import android.view.View

/**
 * The "waiting for the PC" beacon: a monitor glyph inside breathing rings.
 * Drawn instead of shipped as an animated asset so it scales to any tablet
 * density, and it only animates while [pulsing] and attached.
 */
class PulseRingView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {

    private val fill = Paint(Paint.ANTI_ALIAS_FLAG).apply { style = Paint.Style.FILL }
    private val stroke = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeCap = Paint.Cap.ROUND
        strokeJoin = Paint.Join.ROUND
    }
    private val rect = RectF()
    private var start = SystemClock.uptimeMillis()

    var pulsing: Boolean = true
        set(value) {
            field = value
            if (value) postInvalidateOnAnimation()
        }

    override fun onAttachedToWindow() {
        super.onAttachedToWindow()
        start = SystemClock.uptimeMillis()
        postInvalidateOnAnimation()
    }

    override fun onDraw(canvas: Canvas) {
        val cx = width / 2f
        val cy = height / 2f
        val base = minOf(width, height) / 2f
        val phase = if (pulsing) {
            ((SystemClock.uptimeMillis() - start) % CYCLE_MS) / CYCLE_MS.toFloat()
        } else {
            0f
        }

        // Two rings expand outward and fade, so the screen reads as "listening".
        for (i in 0 until 2) {
            val t = (phase + i * 0.5f) % 1f
            val radius = base * (0.52f + 0.46f * t)
            fill.color = withAlpha(ACCENT, ((1f - t) * 46).toInt())
            canvas.drawCircle(cx, cy, radius, fill)
        }

        fill.color = withAlpha(ACCENT, 40)
        canvas.drawCircle(cx, cy, base * 0.48f, fill)
        fill.color = withAlpha(ACCENT, 70)
        canvas.drawCircle(cx, cy, base * 0.34f, fill)

        drawMonitor(canvas, cx, cy, base * 0.30f)

        if (pulsing) {
            postInvalidateOnAnimation()
        }
    }

    private fun drawMonitor(canvas: Canvas, cx: Float, cy: Float, size: Float) {
        stroke.color = Color.WHITE
        stroke.strokeWidth = maxOf(2f, size * 0.10f)
        val halfW = size
        val halfH = size * 0.72f
        rect.set(cx - halfW, cy - halfH - size * 0.12f, cx + halfW, cy + halfH - size * 0.12f)
        canvas.drawRoundRect(rect, size * 0.16f, size * 0.16f, stroke)
        canvas.drawLine(cx, rect.bottom, cx, rect.bottom + size * 0.28f, stroke)
        canvas.drawLine(
            cx - size * 0.42f,
            rect.bottom + size * 0.28f,
            cx + size * 0.42f,
            rect.bottom + size * 0.28f,
            stroke,
        )
    }

    private fun withAlpha(color: Int, alpha: Int): Int =
        Color.argb(alpha.coerceIn(0, 255), Color.red(color), Color.green(color), Color.blue(color))

    private companion object {
        const val CYCLE_MS = 2600L
        val ACCENT = Color.parseColor("#7C5CFF")
    }
}
