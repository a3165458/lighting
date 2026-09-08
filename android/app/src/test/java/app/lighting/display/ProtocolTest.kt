package app.lighting.display

import org.junit.Assert.assertNull
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ProtocolTest {
    @Test
    fun surface_api_gates_match_android_api_levels() {
        assertFalse(DisplayApis.supportsDirectSurfacePosition(32))
        assertTrue(DisplayApis.supportsDirectSurfacePosition(33))
        assertFalse(DisplayApis.supportsFrameRateChangeStrategy(30))
        assertTrue(DisplayApis.supportsFrameRateChangeStrategy(31))
    }

    @Test
    fun oversized_cursor_shape_is_rejected() {
        val width = 257
        val height = 1
        val payload = ByteArray(14 + width * height * 4)
        payload[0] = 3
        payload[10] = (width ushr 8).toByte()
        payload[11] = width.toByte()
        payload[12] = (height ushr 8).toByte()
        payload[13] = height.toByte()

        assertNull(parseCursor(payload))
    }

    @Test
    fun overflowing_cursor_dimensions_are_rejected() {
        val payload = ByteArray(14)
        payload[0] = 3
        payload[10] = 0xff.toByte()
        payload[11] = 0xff.toByte()
        payload[12] = 0xff.toByte()
        payload[13] = 0xff.toByte()

        assertNull(parseCursor(payload))
    }
}
