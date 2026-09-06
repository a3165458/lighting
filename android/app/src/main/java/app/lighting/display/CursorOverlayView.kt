package app.lighting.display

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View
import android.widget.FrameLayout

/**
 * GlideX-style local pointer: a wrap_content hardware layer moved with
 * translationX/Y so the compositor updates the cursor without redrawing
 * a full-screen overlay (and without blending over the video Surface).
 */
class CursorOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private var bmp: Bitmap? = null
    private var hotX = 0f
    private var hotY = 0f
    @Volatile private var showing = false

    init {
        setLayerType(LAYER_TYPE_HARDWARE, null)
        isClickable = false
        isFocusable = false
        visibility = GONE
    }

    fun hidePointer() {
        showing = false
        visibility = GONE
        translationX = 0f
        translationY = 0f
    }

    fun setShape(bitmap: Bitmap, hotspotX: Int, hotspotY: Int) {
        bmp = bitmap
        hotX = hotspotX.toFloat()
        hotY = hotspotY.toFloat()
        val lp = layoutParams ?: FrameLayout.LayoutParams(
            bitmap.width.coerceAtLeast(1),
            bitmap.height.coerceAtLeast(1),
        )
        if (lp.width != bitmap.width || lp.height != bitmap.height) {
            lp.width = bitmap.width.coerceAtLeast(1)
            lp.height = bitmap.height.coerceAtLeast(1)
            layoutParams = lp
        }
        invalidate()
    }

    fun showAt(
        x: Int,
        y: Int,
        srcW: Int,
        srcH: Int,
        surfaceLeft: Int,
        surfaceTop: Int,
        surfaceW: Int,
        surfaceH: Int,
    ) {
        val b = bmp ?: return
        val sw = srcW.coerceAtLeast(1)
        val sh = srcH.coerceAtLeast(1)
        val scaleX = surfaceW.coerceAtLeast(1).toFloat() / sw
        val scaleY = surfaceH.coerceAtLeast(1).toFloat() / sh
        val w = (b.width * scaleX).toInt().coerceAtLeast(1)
        val h = (b.height * scaleY).toInt().coerceAtLeast(1)
        val lp = layoutParams
        if (lp != null && (lp.width != w || lp.height != h)) {
            lp.width = w
            lp.height = h
            layoutParams = lp
        }
        translationX = surfaceLeft + x * scaleX - hotX * scaleX
        translationY = surfaceTop + y * scaleY - hotY * scaleY
        showing = true
        if (visibility != VISIBLE) {
            visibility = VISIBLE
        }
    }

    override fun onDraw(canvas: Canvas) {
        if (!showing) return
        val b = bmp ?: return
        canvas.drawBitmap(b, null, android.graphics.Rect(0, 0, width, height), paint)
    }
}
