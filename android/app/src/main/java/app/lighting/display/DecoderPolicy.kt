package app.lighting.display

data class DecodeMode(val width: Int, val height: Int, val fps: Int)

class DecoderUnavailable(val mode: DecodeMode, detail: String) :
    IllegalStateException("无法启动或维持流畅解码（${mode.width}×${mode.height}@${mode.fps}）。$detail")

object DecoderPolicy {
    /** The three independent capability maxima are not a supported mode. */
    fun selectMode(
        maxWidth: Int,
        maxHeight: Int,
        alignment: Int,
        supports: (Int, Int, Int) -> Boolean,
    ): DecodeMode? {
        if (maxWidth < 128 || maxHeight < 128) return null
        val align = alignment.coerceAtLeast(2)
        val sizes = linkedSetOf(maxWidth to maxHeight)
        for ((w, h) in listOf(
            3840 to 2160, 2560 to 1600, 2560 to 1440, 1920 to 1200,
            1920 to 1080, 1600 to 900, 1280 to 800, 1280 to 720,
            960 to 540, 640 to 360,
        )) {
            sizes.add(w.coerceAtMost(maxWidth) to h.coerceAtMost(maxHeight))
            sizes.add(h.coerceAtMost(maxWidth) to w.coerceAtMost(maxHeight))
        }
        return sizes.map { (w, h) -> w / align * align to h / align * align }
            .distinct()
            .filter { (w, h) -> w >= 128 && h >= 128 }
            .mapNotNull { (w, h) ->
                val fps = listOf(120, 90, 60, 45, 30, 24).firstOrNull { supports(w, h, it) }
                fps?.let { DecodeMode(w, h, it) }
            }
            .maxWithOrNull(compareBy<DecodeMode> { it.width.toLong() * it.height }.thenBy { it.fps })
    }

    fun allowsDecoder(name: String, software: Boolean, requiresSecure: Boolean, hardwareRequired: Boolean): Boolean =
        !name.endsWith(".secure", ignoreCase = true) && !requiresSecure && (!hardwareRequired || !software)
}

/** Per connection attempt group; never mutate the cached device capabilities. */
class DecoderRecovery {
    private var nextTier = 0
    private val tiers = listOf(DecodeMode(1920, 1080, 30), DecodeMode(1280, 720, 30), DecodeMode(960, 540, 30))

    fun next(failed: DecodeMode): DecodeMode? {
        while (nextTier < tiers.size) {
            val tier = tiers[nextTier++]
            val cap = if (failed.height > failed.width) tier.copy(width = tier.height, height = tier.width) else tier
            // Never retry the same workload, even if the peer ignores our Hello limits.
            if (failed.width > cap.width || failed.height > cap.height || failed.fps > cap.fps) {
                return cap.copy(
                    width = minOf(failed.width, cap.width),
                    height = minOf(failed.height, cap.height),
                    fps = minOf(failed.fps, cap.fps),
                )
            }
        }
        return null
    }
}

fun DeviceCaps.withDecodeCeiling(ceiling: DecodeMode): DeviceCaps {
    fun clamp(limit: DeviceCaps.CodecLimit): DeviceCaps.CodecLimit {
        val portrait = limit.height > limit.width
        val long = maxOf(ceiling.width, ceiling.height)
        val short = minOf(ceiling.width, ceiling.height)
        return limit.copy(
            width = minOf(limit.width, if (portrait) short else long),
            height = minOf(limit.height, if (portrait) long else short),
            fps = minOf(limit.fps, ceiling.fps),
        )
    }
    val fallback = clamp(DeviceCaps.CodecLimit(decoderMaxWidth, decoderMaxHeight, decoderMaxFps, hwDecode, ""))
    return copy(
        decoderMaxWidth = fallback.width,
        decoderMaxHeight = fallback.height,
        decoderMaxFps = fallback.fps,
        avc = avc?.let(::clamp),
        hevc = hevc?.let(::clamp),
    )
}

/** Keep a failed decoder from holding the shared video/audio reader indefinitely. */
fun submitDecoderInput(
    mode: DecodeMode,
    running: () -> Boolean,
    nowNanos: () -> Long = System::nanoTime,
    enqueue: (Long) -> Boolean,
) {
    val start = nowNanos()
    if (!running()) return
    if (enqueue(0L)) return
    while (running()) {
        if (nowNanos() - start >= 250_000_000L) {
            throw DecoderUnavailable(mode, "解码输入持续阻塞，正在降低参数重试")
        }
        if (enqueue(8_000L)) return
    }
}
