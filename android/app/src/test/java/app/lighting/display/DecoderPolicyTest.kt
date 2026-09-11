package app.lighting.display

import org.junit.Assert.*
import org.junit.Test

class DecoderPolicyTest {
    @Test
    fun decoder_wait_has_a_deadline_so_audio_cannot_be_blocked_forever() {
        var now = 0L
        val failure = assertThrows(DecoderUnavailable::class.java) {
            submitDecoderInput(DecodeMode(1920, 1152, 60), { true }, { now }) { waitUs ->
                now += waitUs * 1000
                false
            }
        }
        assertEquals(DecodeMode(1920, 1152, 60), failure.mode)
        assertTrue(now in 250_000_000L..260_000_000L)
    }

    @Test
    fun decoder_wait_exits_on_cancel_or_success() {
        var attempts = 0
        submitDecoderInput(DecodeMode(1280, 720, 30), { true }, { 0L }) { ++attempts == 2 }
        assertEquals(2, attempts)
        submitDecoderInput(DecodeMode(1280, 720, 30), { false }, { 0L }) {
            fail("cancelled session must not submit to the decoder")
            false
        }
    }

    @Test
    fun independent_dimension_maxima_do_not_advertise_a_square_decoder() {
        val mode = DecoderPolicy.selectMode(1920, 1920, 2) { w, h, fps ->
            w <= 1920 && h <= 1080 && fps <= 60
        }
        assertEquals(DecodeMode(1920, 1080, 60), mode)
    }

    @Test
    fun frame_rate_belongs_to_the_selected_size() {
        val mode = DecoderPolicy.selectMode(2560, 1600, 2) { w, h, fps ->
            if (w > 1920 || h > 1080) fps <= 30 else fps <= 60
        }
        assertEquals(DecodeMode(2560, 1600, 30), mode)
    }

    @Test
    fun modes_are_aligned_and_verified_in_their_actual_orientation() {
        val mode = DecoderPolicy.selectMode(1088, 1920, 16) { w, h, fps ->
            w <= 1080 && h <= 1920 && w % 16 == 0 && h % 16 == 0 && fps <= 60
        }
        assertEquals(DecodeMode(1072, 1920, 60), mode)
    }

    @Test
    fun unsupported_or_invalid_capabilities_do_not_claim_hardware_decode() {
        assertNull(DecoderPolicy.selectMode(1920, 1920, 2) { _, _, _ -> false })
        assertNull(DecoderPolicy.selectMode(0, 0, 2) { _, _, _ -> true })
    }

    @Test
    fun hardware_session_excludes_software_and_secure_candidates() {
        assertFalse(DecoderPolicy.allowsDecoder("OMX.qcom.video.decoder.avc.secure", false, false, true))
        assertFalse(DecoderPolicy.allowsDecoder("vendor.decoder", false, true, true))
        assertFalse(DecoderPolicy.allowsDecoder("c2.android.avc.decoder", true, false, true))
        assertTrue(DecoderPolicy.allowsDecoder("OMX.qcom.video.decoder.avc", false, false, true))
        assertTrue(DecoderPolicy.allowsDecoder("c2.android.avc.decoder", true, false, false))
        assertFalse(DecoderPolicy.allowsDecoder("vendor.secure", false, false, false))
    }

    @Test
    fun failed_1920_by_1152_stream_retries_smaller_and_eventually_stops() {
        val recovery = DecoderRecovery()
        assertEquals(DecodeMode(1920, 1080, 30), recovery.next(DecodeMode(1920, 1152, 60)))
        assertEquals(DecodeMode(1280, 720, 30), recovery.next(DecodeMode(1800, 1080, 30)))
        assertEquals(DecodeMode(960, 540, 30), recovery.next(DecodeMode(1200, 720, 30)))
        assertNull(recovery.next(DecodeMode(900, 540, 30)))
    }

    @Test
    fun old_host_ignoring_reduced_limits_does_not_cause_infinite_retry() {
        val recovery = DecoderRecovery()
        repeat(3) { assertNotNull(recovery.next(DecodeMode(1920, 1152, 60))) }
        assertNull(recovery.next(DecodeMode(1920, 1152, 60)))
    }

    @Test
    fun retries_do_not_upscale_a_small_or_portrait_stream() {
        assertNull(DecoderRecovery().next(DecodeMode(640, 360, 30)))
        assertEquals(DecodeMode(720, 1280, 30), DecoderRecovery().next(DecodeMode(1080, 1920, 30)))
    }

    @Test
    fun retry_caps_are_sent_for_both_codecs_without_changing_cached_caps() {
        val caps = DeviceCaps(
            "google", "unknown", "TrebleDroid", "bengal", "qcom", true,
            listOf("avc", "hevc"), 1920, 1920, 60, true, 2, false,
            DeviceCaps.CodecLimit(1920, 1920, 60, true, "OMX.qcom.video.decoder.avc"),
            DeviceCaps.CodecLimit(1920, 1088, 60, true, "OMX.qcom.video.decoder.hevc"),
        )
        val retry = caps.withDecodeCeiling(DecodeMode(1280, 720, 30))
        assertEquals(1280, retry.decoderMaxWidth)
        assertEquals(720, retry.decoderMaxHeight)
        assertEquals(30, retry.decoderMaxFps)
        assertEquals(720, retry.avc!!.height)
        assertEquals(720, retry.hevc!!.height)
        assertEquals(30, retry.avc!!.fps)
        assertTrue(retry.hwDecode)
        assertEquals(1920, caps.avc!!.height)
    }
}
