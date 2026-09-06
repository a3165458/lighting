package app.lighting.display

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Paint
import android.util.AttributeSet
import android.view.View
import android.widget.FrameLayout
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * GlideX-style local pointer: a wrap_content hardware layer moved with
 * translationX/Y. Host samples at 1 ms; applying every packet via View.post
 * backs up the UI queue. Take the latest pose and paint it on the next vsync.
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

    private data class Pose(
        val visible: Boolean,
        val x: Int = 0,
        val y: Int = 0,
        val srcW: Int = 1,
        val srcH: Int = 1,
        val surfaceLeft: Int = 0,
        val surfaceTop: Int = 0,
        val surfaceW: Int = 1,
        val surfaceH: Int = 1,
        val bitmap: Bitmap? = null,
        val hotspotX: Int = 0,
        val hotspotY: Int = 0,
    )

    private val pending = AtomicReference<Pose?>()
    private val scheduled = AtomicBoolean(false)
    private val applyOnce = Runnable {
        scheduled.set(false)
        val pose = pending.getAndSet(null) ?: return@Runnable
        applyPose(pose)
        if (pending.get() != null && scheduled.compareAndSet(false, true)) {
            postOnAnimation(this.applyOnce)
        }
    }

    init {
        setLayerType(LAYER_TYPE_HARDWARE, null)
        isClickable = false
        isFocusable = false
        visibility = GONE
    }

    fun hidePointer() {
        submit(Pose(visible = false))
    }

    fun setShape(bitmap: Bitmap, hotspotX: Int, hotspotY: Int) {
        update(
            visible = true,
            x = 0,
            y = 0,
            srcW = 1,
            srcH = 1,
            surfaceLeft = 0,
            surfaceTop = 0,
            surfaceW = 1,
            surfaceH = 1,
            bitmap = bitmap,
            hotspotX = hotspotX,
            hotspotY = hotspotY,
            move = false,
        )
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
        update(
            visible = true,
            x = x,
            y = y,
            srcW = srcW,
            srcH = srcH,
            surfaceLeft = surfaceLeft,
            surfaceTop = surfaceTop,
            surfaceW = surfaceW,
            surfaceH = surfaceH,
            bitmap = null,
            hotspotX = 0,
            hotspotY = 0,
            move = true,
        )
    }

    fun update(
        visible: Boolean,
        x: Int,
        y: Int,
        srcW: Int,
        srcH: Int,
        surfaceLeft: Int,
        surfaceTop: Int,
        surfaceW: Int,
        surfaceH: Int,
        bitmap: Bitmap?,
        hotspotX: Int,
        hotspotY: Int,
        move: Boolean = true,
    ) {
        val cur = pending.get()
        submit(
            Pose(
                visible = visible,
                x = if (move) x else (cur?.x ?: 0),
                y = if (move) y else (cur?.y ?: 0),
                srcW = if (move) srcW else (cur?.srcW ?: 1),
                srcH = if (move) srcH else (cur?.srcH ?: 1),
                surfaceLeft = if (move) surfaceLeft else (cur?.surfaceLeft ?: 0),
                surfaceTop = if (move) surfaceTop else (cur?.surfaceTop ?: 0),
                surfaceW = if (move) surfaceW else (cur?.surfaceW ?: 1),
                surfaceH = if (move) surfaceH else (cur?.surfaceH ?: 1),
                bitmap = bitmap ?: cur?.bitmap,
                hotspotX = if (bitmap != null) hotspotX else (cur?.hotspotX ?: 0),
                hotspotY = if (bitmap != null) hotspotY else (cur?.hotspotY ?: 0),
            ),
        )
    }

    private fun submit(pose: Pose) {
        val prev = pending.getAndSet(pose)
        if (prev?.bitmap != null && prev.bitmap !== pose.bitmap) {
            prev.bitmap.recycle()
        }
        if (scheduled.compareAndSet(false, true)) {
            postOnAnimation(applyOnce)
        }
    }

    private fun applyPose(pose: Pose) {
        if (!pose.visible) {
            showing = false
            visibility = GONE
            translationX = 0f
            translationY = 0f
            return
        }
        var shapeChanged = false
        val incoming = pose.bitmap
        if (incoming != null && incoming !== bmp) {
            val old = bmp
            bmp = incoming
            hotX = pose.hotspotX.toFloat()
            hotY = pose.hotspotY.toFloat()
            if (old != null && old !== incoming && !old.isRecycled) {
                old.recycle()
            }
            shapeChanged = true
        }
        val b = bmp ?: return
        val sw = pose.srcW.coerceAtLeast(1)
        val sh = pose.srcH.coerceAtLeast(1)
        val scaleX = pose.surfaceW.coerceAtLeast(1).toFloat() / sw
        val scaleY = pose.surfaceH.coerceAtLeast(1).toFloat() / sh
        val w = (b.width * scaleX).toInt().coerceAtLeast(1)
        val h = (b.height * scaleY).toInt().coerceAtLeast(1)
        val lp = layoutParams ?: FrameLayout.LayoutParams(w, h)
        if (lp.width != w || lp.height != h) {
            lp.width = w
            lp.height = h
            layoutParams = lp
        }
        translationX = pose.surfaceLeft + pose.x * scaleX - hotX * scaleX
        translationY = pose.surfaceTop + pose.y * scaleY - hotY * scaleY
        showing = true
        if (visibility != VISIBLE) {
            visibility = VISIBLE
        }
        if (shapeChanged) {
            invalidate()
        }
    }

    override fun onDraw(canvas: Canvas) {
        if (!showing) return
        val b = bmp ?: return
        if (b.isRecycled) return
        canvas.drawBitmap(b, null, android.graphics.Rect(0, 0, width, height), paint)
    }
}
