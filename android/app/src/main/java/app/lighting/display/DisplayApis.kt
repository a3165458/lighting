package app.lighting.display

import android.view.SurfaceControl
import android.view.WindowManager

/**
 * Framework display APIs that exist on-device but are @hide, so they are
 * missing from the public compileSdk android.jar and fail assembleDebug.
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
}
