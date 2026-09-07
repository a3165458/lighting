package app.lighting.display

import android.os.Build
import android.view.Surface
import android.view.SurfaceControl
import android.view.SurfaceView
import android.view.View
import android.view.WindowManager

/**
 * Display-path helpers. Some are @hide (missing from android.jar and
 * fail assembleDebug if called directly); [requestViewFrameRate] is
 * public API 35 but every caller needs the same SDK gate + try/catch.
 */
internal object DisplayApis {
    fun setPreferredRefreshRange(lp: WindowManager.LayoutParams, hz: Float) {
        try {
            val cls = lp.javaClass
            cls.getField("preferredMinRefreshRate").setFloat(lp, hz)
            cls.getField("preferredMaxRefreshRate").setFloat(lp, hz)
        } catch (_: Throwable) {
        }
    }

    fun overrideChildrenFrameRate(tx: SurfaceControl.Transaction, sc: SurfaceControl) {
        try {
            val strategy = SurfaceControl::class.java
                .getField("FRAME_RATE_SELECTION_STRATEGY_OVERRIDE_CHILDREN")
                .getInt(null)
            SurfaceControl.Transaction::class.java
                .getMethod(
                    "setFrameRateSelectionStrategy",
                    SurfaceControl::class.java,
                    Int::class.javaPrimitiveType,
                )
                .invoke(tx, sc, strategy)
        } catch (_: Throwable) {
        }
    }

    fun setBufferMaxCount(tx: SurfaceControl.Transaction, sc: SurfaceControl, count: Int) {
        try {
            SurfaceControl.Transaction::class.java
                .getMethod(
                    "setBufferMaxCount",
                    SurfaceControl::class.java,
                    Int::class.javaPrimitiveType,
                )
                .invoke(tx, sc, count)
        } catch (_: Throwable) {
        }
    }

    /**
     * Producer-side BufferQueue cap. SurfaceView defaults to 3 slots;
     * MediaCodec then holds a decoded picture until the next vsync —
     * one refresh vs the laptop. 2 is ping-pong; 1 tears.
     * Must run before MediaCodec.configure connects the producer.
     * setBufferMaxCount is the consumer/layer twin; BLAST keeps a
     * child queue the SurfaceControl cap misses (API 31–33 pads).
     */
    fun capDecoderBuffers(view: SurfaceView?, surface: Surface, count: Int) {
        setMaxDequeuedBufferCount(surface, count)
        if (view != null) {
            tryBlastMaxDequeued(view, count)
        }
    }

    fun setMaxDequeuedBufferCount(surface: Surface, count: Int) {
        // Public on API 34. Some API 31–33 builds ship the same hidden
        // method; the old SDK_INT < 34 return left a 3-slot queue on
        // Android 12/13 tablets (one extra glass frame vs GlideX).
        invokeIntMethod(surface, "setMaxDequeuedBufferCount", count)
    }

    /**
     * BLASTBufferQueue (API 31+) is the SurfaceView child the
     * Surface.setMaxDequeuedBufferCount call never sees. Hidden, so
     * reflect; missing method is a no-op.
     */
    private fun tryBlastMaxDequeued(view: SurfaceView, count: Int) {
        var cls: Class<*>? = view.javaClass
        while (cls != null && cls != Any::class.java) {
            for (field in cls.declaredFields) {
                val name = field.name
                if (!name.contains("Blast") && !name.contains("BLAST")) {
                    continue
                }
                try {
                    field.isAccessible = true
                    val bbq = field.get(view) ?: continue
                    invokeIntMethod(bbq, "setMaxDequeuedBufferCount", count)
                } catch (_: Throwable) {
                }
            }
            cls = cls.superclass
        }
    }

    private fun invokeIntMethod(target: Any, method: String, value: Int) {
        val cls = target.javaClass
        try {
            cls.getMethod(method, Int::class.javaPrimitiveType).invoke(target, value)
            return
        } catch (_: Throwable) {
        }
        try {
            val m = cls.getDeclaredMethod(method, Int::class.javaPrimitiveType)
            m.isAccessible = true
            m.invoke(target, value)
        } catch (_: Throwable) {
        }
    }

    /**
     * Android 15 ViewRoot only counts a View vote. Surface.setFrameRate
     * plus SurfaceControl is not enough: VRR still latches 60 and every
     * picture waits a vsync the laptop does not. GlideX / game windows
     * vote the View at the panel peak.
     */
    fun requestViewFrameRate(view: View, hz: Float) {
        if (Build.VERSION.SDK_INT < 35) return
        try {
            view.setRequestedFrameRate(hz)
        } catch (_: Throwable) {
        }
    }
}
