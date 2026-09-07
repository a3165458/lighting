package app.lighting.display

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.PorterDuff
import android.graphics.Rect
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.AttributeSet
import android.view.Surface
import android.view.SurfaceControl
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.widget.FrameLayout
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference

/**
 * GlideX-style local pointer as its own SurfaceView (HWC overlay plane).
 *
 * A regular View on top of the decoder SurfaceView forces SurfaceFlinger to
 * GPU-compose every video frame while the pointer is visible. Keep pose
 * updates as translationX/Y and only lockCanvas when the shape changes.
 * Move-only poses also punch SurfaceControl.Transaction.setPosition from
 * the control thread so the overlay does not sit on the next Choreographer
 * vsync (8–16 ms of pointer lag vs GlideX).
 */
class CursorOverlayView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : SurfaceView(context, attrs), SurfaceHolder.Callback {
    private val paint = Paint(Paint.FILTER_BITMAP_FLAG)
    private var bmp: Bitmap? = null
    private var hotX = 0f
    private var hotY = 0f
    @Volatile private var showing = false
    @Volatile private var surfaceReady = false
    @Volatile private var lastTx = Float.NaN
    @Volatile private var lastTy = Float.NaN
    // Overlay planes ignore the activity mode and sit at 60 Hz unless we
    // pin the SurfaceControl. display.refreshRate is that 60 until then.
    @Volatile private var peakHz = 120f

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
    // createAsync jumps Choreographer's sync barrier. A default main
    // Handler waits for the next vsync traversal; translationX then lands
    // a refresh late and SurfaceView.updateSurface snaps the punch back.
    private val ui = if (Build.VERSION.SDK_INT >= 28) {
        Handler.createAsync(Looper.getMainLooper())
    } else {
        Handler(Looper.getMainLooper())
    }
    private val applyOnce: Runnable = Runnable { drainPose() }

    private fun drainPose() {
        // Consume every pose that landed while applyPose ran. Re-posting
        // to the Looper let Choreographer commit a stale translationX and
        // SurfaceView.updateSurface snapped the overlay back a vsync —
        // the pointer sat behind the laptop after setPosition had already
        // punched the new spot.
        while (true) {
            var pose = pending.getAndSet(null)
            if (pose == null) {
                scheduled.set(false)
                if (pending.get() != null && scheduled.compareAndSet(false, true)) {
                    continue
                }
                return
            }
            while (true) {
                val next = pending.getAndSet(null) ?: break
                pose = next
            }
            applyPose(pose)
        }
    }

    init {
        // Must run before attach: later calls do not move the overlay plane.
        setZOrderMediaOverlay(true)
        holder.setFormat(PixelFormat.TRANSLUCENT)
        holder.addCallback(this)
        isClickable = false
        isFocusable = false
        visibility = GONE
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        surfaceReady = true
        hintOverlayFrameRate()
        paintShape()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        surfaceReady = true
        hintOverlayFrameRate()
        paintShape()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
    }

    fun setPeakRefreshHz(hz: Float) {
        peakHz = hz.coerceIn(30f, 120f)
        hintOverlayFrameRate()
    }

    /**
     * Overlay planes default to 60 Hz even after the activity locked peak
     * refresh. SurfaceControl.Transaction then waits 16 ms; GlideX does not.
     * Pin the Surface *and* its SurfaceControl to the mode lockPeakRefresh
     * chose — display.refreshRate on this overlay is still 60 until then.
     */
    private fun hintOverlayFrameRate() {
        if (Build.VERSION.SDK_INT < 30) return
        val hz = peakHz
        val s = holder.surface
        if (s.isValid) {
            try {
                if (Build.VERSION.SDK_INT >= 31) {
                    s.setFrameRate(
                        hz,
                        Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                        Surface.CHANGE_FRAME_RATE_ALWAYS,
                    )
                } else {
                    s.setFrameRate(hz, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT)
                }
            } catch (_: Throwable) {
            }
        }
        DisplayApis.requestViewFrameRate(this, hz)
        if (Build.VERSION.SDK_INT < 31) return
        try {
            val sc = surfaceControl
            if (!sc.isValid) return
            // Same OVERRIDE_CHILDREN as the decoder SurfaceView: PROPAGATE
            // lets a child BLAST layer latch at 60 Hz and the pointer sits
            // one refresh behind the laptop.
            val tx = SurfaceControl.Transaction()
                .setFrameRate(
                    sc,
                    hz,
                    Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                    Surface.CHANGE_FRAME_RATE_ALWAYS,
                )
            DisplayApis.overrideChildrenFrameRate(tx, sc)
            // SurfaceView default BufferQueue is 3. The extra slot
            // holds a decoded picture until the next vsync — one
            // refresh vs the laptop. 2 is ping-pong; 1 tears.
            // Public on API 34; reflection no-ops on older builds.
            DisplayApis.setBufferMaxCount(tx, sc, 2)

            tx.apply()
        } catch (_: Throwable) {
        }
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
        // Punch the overlay plane now. A default main Handler waits
        // on Choreographer's sync barrier, so translationX landed on
        // the next vsync and SurfaceView.updateSurface snapped the
        // punch back. createAsync jumps that barrier: View coords
        // catch up before traversal, so a HUD/layout cannot snap the
        // pointer a refresh behind the laptop. Show/hide/resize/shape
        // still drain applyPose on the same handler.
        applySurfacePosition(pose)
        if (scheduled.compareAndSet(false, true)) {
            ui.postAtFrontOfQueue(applyOnce)
        }
    }

    /**
     * API 29+: SurfaceFlinger position without waiting for the View
     * traversal. Returns false if the View path must run first (show/hide
     * / resize / first frame).
     */
    private fun applySurfacePosition(pose: Pose): Boolean {
        if (Build.VERSION.SDK_INT < 29) return false
        if (!pose.visible || !showing || !surfaceReady) return false
        if (visibility != VISIBLE) return false
        val b = bmp ?: return false
        if (pose.bitmap != null && pose.bitmap !== b) return false
        val sw = pose.srcW.coerceAtLeast(1).toFloat()
        val sh = pose.srcH.coerceAtLeast(1).toFloat()
        val scaleX = pose.surfaceW.coerceAtLeast(1).toFloat() / sw
        val scaleY = pose.surfaceH.coerceAtLeast(1).toFloat() / sh
        val w = (b.width * scaleX).toInt().coerceAtLeast(1)
        val h = (b.height * scaleY).toInt().coerceAtLeast(1)
        val lp = layoutParams
        if (lp != null && (lp.width != w || lp.height != h)) return false
        val x = pose.surfaceLeft + pose.x * scaleX - hotX * scaleX
        val y = pose.surfaceTop + pose.y * scaleY - hotY * scaleY
        if (x == lastTx && y == lastTy) return true
        return try {
            val sc = surfaceControl
            if (!sc.isValid) return false
            // Hint peak Hz once in hintOverlayFrameRate. CHANGE_FRAME_RATE_ALWAYS
            // on every pose made SurfaceFlinger treat a 1 ms move as a refresh
            // switch — the pointer sat a vsync behind the laptop. GlideX only
            // punches position here.
            SurfaceControl.Transaction().setPosition(sc, x, y).apply()
            lastTx = x
            lastTy = y
            true
        } catch (_: Throwable) {
            false
        }
    }

    private fun applyPose(pose: Pose) {
        if (!pose.visible) {
            showing = false
            visibility = GONE
            translationX = 0f
            translationY = 0f
            lastTx = Float.NaN
            lastTy = Float.NaN
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
        val sizeChanged = lp.width != w || lp.height != h
        if (sizeChanged) {
            lp.width = w
            lp.height = h
            layoutParams = lp
        }
        val tx = pose.surfaceLeft + pose.x * scaleX - hotX * scaleX
        val ty = pose.surfaceTop + pose.y * scaleY - hotY * scaleY
        // Punch owns lastTx on API 29+. update() carries the current
        // bitmap on every move (`bitmap ?: cur?.bitmap`), so pose.bitmap
        // is almost never null after the first shape — a stale applyPose
        // then rewrote translationX from an older pose after submit had
        // already punched a newer one. SurfaceView.updateSurface snapped
        // the pointer a refresh behind the laptop. Unchanged size/shape:
        // sync View to the punched coords. Show / hide / resize / shape
        // still assign from the pose.
        if (Build.VERSION.SDK_INT >= 29 &&
            !sizeChanged &&
            !shapeChanged &&
            showing &&
            !lastTx.isNaN() &&
            !lastTy.isNaN()
        ) {
            if (translationX != lastTx) translationX = lastTx
            if (translationY != lastTy) translationY = lastTy
        } else {
            translationX = tx
            translationY = ty
            lastTx = tx
            lastTy = ty
        }
        showing = true
        if (visibility != VISIBLE) {
            visibility = VISIBLE
        }
        if (shapeChanged) {
            paintShape()
        }
    }

    private fun paintShape() {
        if (!showing || !surfaceReady) return
        val b = bmp ?: return
        if (b.isRecycled) return
        val canvas: Canvas = try {
            holder.lockCanvas() ?: return
        } catch (_: Throwable) {
            return
        }
        try {
            canvas.drawColor(Color.TRANSPARENT, PorterDuff.Mode.CLEAR)
            canvas.drawBitmap(b, null, Rect(0, 0, canvas.width, canvas.height), paint)
        } finally {
            try {
                holder.unlockCanvasAndPost(canvas)
            } catch (_: Throwable) {
            }
        }
    }
}
