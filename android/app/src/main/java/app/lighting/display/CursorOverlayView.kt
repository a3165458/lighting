package app.lighting.display

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View

/**
 * GlideX-style local pointer: paint on a hardware layer so a move is one
 * invalidate, not a layout pass. Position is in encoded-frame pixels.
 */
class CursorOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : View(context, attrs) {
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private var bmp: Bitmap? = null
    private var hotX = 0f
    private var hotY = 0f
    private var posX = 0f
    private var posY = 0f
    private var scaleX = 1f
    private var scaleY = 1f
    private var originX = 0f
    private var originY = 0f
    @Volatile private var showing = false

    init {
        setLayerType(LAYER_TYPE_HARDWARE, null)
        isClickable = false
        isFocusable = false
        visibility = GONE
    }

    fun hidePointer() {
        showing = false
        post {
            visibility = GONE
            invalidate()
        }
    }

    fun setShape(bitmap: Bitmap, hotspotX: Int, hotspotY: Int) {
        bmp = bitmap
        hotX = hotspotX.toFloat()
        hotY = hotspotY.toFloat()
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
        val sw = srcW.coerceAtLeast(1)
        val sh = srcH.coerceAtLeast(1)
        scaleX = surfaceW.coerceAtLeast(1).toFloat() / sw
        scaleY = surfaceH.coerceAtLeast(1).toFloat() / sh
        originX = surfaceLeft.toFloat()
        originY = surfaceTop.toFloat()
        posX = x.toFloat()
        posY = y.toFloat()
        showing = true
        post {
            if (visibility != VISIBLE) visibility = VISIBLE
            invalidate()
        }
    }

    override fun onDraw(canvas: Canvas) {
        if (!showing) return
        val b = bmp ?: return
        val left = originX + posX * scaleX - hotX * scaleX
        val top = originY + posY * scaleY - hotY * scaleY
        val right = left + b.width * scaleX
        val bottom = top + b.height * scaleY
        canvas.drawBitmap(b, null, android.graphics.RectF(left, top, right, bottom), paint)
    }
}
