package app.lighting.display

import android.os.Build
import android.view.SurfaceControl
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
