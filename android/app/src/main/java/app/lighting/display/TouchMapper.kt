package app.lighting.display

import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.util.Log
import android.view.MotionEvent
import android.view.View
import android.view.ViewConfiguration
import kotlin.math.abs
import kotlin.math.hypot

/**
 * One finger = mouse. Tap = click. Drag = left-button drag.
 * Long-press without moving = right click. Two fingers = scroll.
 */
class TouchMapper(private val send: (action: Int, x: Int, y: Int) -> Unit) {
    companion object {
        const val LEFT_DOWN = 0
        const val MOVE = 1
        const val LEFT_UP = 2
        const val CANCEL = 3
        const val RIGHT_DOWN = 4
        const val RIGHT_UP = 5
        const val WHEEL = 6
        const val HWHEEL = 7
        private const val TAG = "LightingTouch"
        private const val LONG_PRESS_MS = 480L
        private const val MOVE_MIN_INTERVAL_MS = 8L
        private const val WHEEL_PX = 28f
    }

    private val handler = Handler(Looper.getMainLooper())
    private var slop = 32f
    private var video: View? = null
    private var overlay: View? = null
    private var downX = 0f
    private var downY = 0f
    private var lastSendMs = 0L
    private var leftDown = false
    private var scrolling = false
    private var rightClicked = false
    private var scrollX = 0f
    private var scrollY = 0f
    private var lastCx = 0f
    private var lastCy = 0f
    private var lastNorm = 32767 to 32767
    private var sentCount = 0
    private var lastHandledSeq = -1
    private var lastHandledAction = -1

    private val longPress = Runnable {
        if (!leftDown && !scrolling && !rightClicked) {
            val (nx, ny) = lastNorm
            emit(RIGHT_DOWN, nx, ny)
            emit(RIGHT_UP, nx, ny)
            rightClicked = true
        }
    }

    fun attach(overlay: View, video: View) {
        this.video = video
        this.overlay = overlay
        slop = ViewConfiguration.get(overlay.context).scaledTouchSlop.toFloat().coerceAtLeast(16f)
        overlay.isClickable = true
        overlay.isFocusable = true
        overlay.isFocusableInTouchMode = true
        overlay.setOnTouchListener { view, event ->
            view.parent?.requestDisallowInterceptTouchEvent(true)
            try {
                onWindowTouch(event)
            } catch (err: Throwable) {
                Log.w(TAG, "overlay touch failed", err)
            }
            true
        }
    }
    fun onWindowTouch(event: MotionEvent) {
        try {
            val seq = event.eventTime.toInt() xor event.actionMasked
            val action = event.actionMasked
            if (seq == lastHandledSeq && action == lastHandledAction) return
            lastHandledSeq = seq
            lastHandledAction = action
            Log.i(TAG, "in action=" + action + " pointers=" + event.pointerCount)
            onTouch(event)
        } catch (err: Throwable) {
            Log.w(TAG, "onTouch failed", err)
        }
    }

    fun cancel() {
        handler.removeCallbacks(longPress)
        if (leftDown) {
            emit(LEFT_UP, lastNorm.first, lastNorm.second)
            leftDown = false
        }
        scrolling = false
        rightClicked = false
    }

    private fun onTouch(e: MotionEvent) {
        val (px, py) = local(e, e.actionIndex.coerceAtLeast(0))
        lastNorm = norm(px, py)

        when (e.actionMasked) {
            MotionEvent.ACTION_HOVER_MOVE, MotionEvent.ACTION_HOVER_ENTER -> {
                emit(MOVE, lastNorm.first, lastNorm.second)
            }
            MotionEvent.ACTION_BUTTON_PRESS -> {
                emit(LEFT_DOWN, lastNorm.first, lastNorm.second)
                leftDown = true
            }
            MotionEvent.ACTION_BUTTON_RELEASE -> {
                emit(LEFT_UP, lastNorm.first, lastNorm.second)
                leftDown = false
            }
            MotionEvent.ACTION_DOWN -> {
                downX = px
                downY = py
                leftDown = false
                scrolling = false
                rightClicked = false
                handler.removeCallbacks(longPress)
                handler.postDelayed(longPress, LONG_PRESS_MS)
                emit(MOVE, lastNorm.first, lastNorm.second)
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                handler.removeCallbacks(longPress)
                if (leftDown) {
                    emit(LEFT_UP, lastNorm.first, lastNorm.second)
                    leftDown = false
                }
                scrolling = e.pointerCount >= 2
                if (scrolling) {
                    lastCx = centroidX(e)
                    lastCy = centroidY(e)
                    scrollX = 0f
                    scrollY = 0f
                }
            }
            MotionEvent.ACTION_MOVE -> {
                if (scrolling && e.pointerCount >= 2) {
                    val cx = centroidX(e)
                    val cy = centroidY(e)
                    scrollX += cx - lastCx
                    scrollY += cy - lastCy
                    lastCx = cx
                    lastCy = cy
                    flushWheel()
                } else if (!rightClicked) {
                    val dist = hypot((px - downX).toDouble(), (py - downY).toDouble()).toFloat()
                    if (!leftDown && dist > slop) {
                        handler.removeCallbacks(longPress)
                        emit(LEFT_DOWN, lastNorm.first, lastNorm.second)
                        leftDown = true
                    }
                    val now = SystemClock.uptimeMillis()
                    if (now - lastSendMs >= MOVE_MIN_INTERVAL_MS) {
                        lastSendMs = now
                        emit(MOVE, lastNorm.first, lastNorm.second)
                    }
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (e.pointerCount <= 2) {
                    scrolling = false
                    scrollX = 0f
                    scrollY = 0f
                }
            }
            MotionEvent.ACTION_UP -> {
                handler.removeCallbacks(longPress)
                when {
                    scrolling -> scrolling = false
                    rightClicked -> rightClicked = false
                    leftDown -> {
                        emit(MOVE, lastNorm.first, lastNorm.second)
                        emit(LEFT_UP, lastNorm.first, lastNorm.second)
                        leftDown = false
                    }
                    else -> {
                        emit(LEFT_DOWN, lastNorm.first, lastNorm.second)
                        emit(LEFT_UP, lastNorm.first, lastNorm.second)
                    }
                }
            }
            MotionEvent.ACTION_CANCEL -> {
                handler.removeCallbacks(longPress)
                if (leftDown) {
                    emit(CANCEL, lastNorm.first, lastNorm.second)
                    leftDown = false
                }
                scrolling = false
                rightClicked = false
            }
        }
    }

    private fun emit(action: Int, x: Int, y: Int) {
        sentCount++
        if (action != MOVE || sentCount % 15 == 1) {
            Log.i(TAG, "send action=$action x=$x y=$y n=$sentCount")
        }
        send(action, x, y)
    }

    private fun local(e: MotionEvent, index: Int): Pair<Float, Float> {
        val v = video
        val ov = overlay
        val count = e.pointerCount
        val rawX: Float
        val rawY: Float
        if (count <= 0) {
            rawX = e.x
            rawY = e.y
        } else {
            val idx = index.coerceIn(0, count - 1)
            rawX = e.getX(idx)
            rawY = e.getY(idx)
        }
        if (v == null || ov == null || v.width <= 0 || v.height <= 0) {
            return rawX to rawY
        }
        val overlayLoc = IntArray(2)
        val videoLoc = IntArray(2)
        ov.getLocationOnScreen(overlayLoc)
        v.getLocationOnScreen(videoLoc)
        val x = rawX + overlayLoc[0] - videoLoc[0]
        val y = rawY + overlayLoc[1] - videoLoc[1]
        return x to y
    }

    private fun norm(px: Float, py: Float): Pair<Int, Int> {
        val v = video
        val w = (v?.width ?: 1).coerceAtLeast(1)
        val h = (v?.height ?: 1).coerceAtLeast(1)
        val x = ((px / w) * 65535f).toInt().coerceIn(0, 65535)
        val y = ((py / h) * 65535f).toInt().coerceIn(0, 65535)
        return x to y
    }

    private fun flushWheel() {
        if (abs(scrollY) >= WHEEL_PX && abs(scrollY) >= abs(scrollX)) {
            val notches = (scrollY / WHEEL_PX).toInt()
            if (notches != 0) {
                emit(WHEEL, -notches * 120, 0)
                scrollY -= notches * WHEEL_PX
                scrollX = 0f
            }
        } else if (abs(scrollX) >= WHEEL_PX) {
            val notches = (scrollX / WHEEL_PX).toInt()
            if (notches != 0) {
                emit(HWHEEL, notches * 120, 0)
                scrollX -= notches * WHEEL_PX
                scrollY = 0f
            }
        }
    }

    private fun centroidX(e: MotionEvent): Float {
        var s = 0f
        for (i in 0 until e.pointerCount) s += local(e, i).first
        return s / e.pointerCount
    }

    private fun centroidY(e: MotionEvent): Float {
        var s = 0f
        for (i in 0 until e.pointerCount) s += local(e, i).second
        return s / e.pointerCount
    }
}




