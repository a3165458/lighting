package app.lighting.display

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build
import android.util.Log
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.atomic.AtomicBoolean

class VideoDecoder {
    private var codec: MediaCodec? = null
    private var configured = false
    private var mime: String = MediaFormat.MIMETYPE_VIDEO_AVC
    private val running = AtomicBoolean(false)
    private val skipUntilKey = AtomicBoolean(false)
    private val inputLock = Any()
    private var presentWorker: Thread? = null
    private var fakePtsUs = 0L
    @Volatile var activeName: String = ""
        private set

    fun supportsHevc(): Boolean = hasDecoder(MediaFormat.MIMETYPE_VIDEO_HEVC)
    fun supportsAvc(): Boolean = hasDecoder(MediaFormat.MIMETYPE_VIDEO_AVC)

    private fun hasDecoder(mime: String): Boolean {
        val list = MediaCodecList(MediaCodecList.REGULAR_CODECS)
        return list.codecInfos.any { info ->
            !info.isEncoder && info.supportedTypes.any { it.equals(mime, true) }
        }
    }

    fun configure(codecName: String, width: Int, height: Int, csd: ByteArray?, surface: Surface) {
        release()
        mime = if (codecName.equals("hevc", true) || codecName.equals("h265", true)) {
            MediaFormat.MIMETYPE_VIDEO_HEVC
        } else {
            MediaFormat.MIMETYPE_VIDEO_AVC
        }
        val caps = DeviceCaps.probe()
        val w = (width.coerceAtLeast(16) / caps.alignment * caps.alignment).coerceAtLeast(16)
        val h = (height.coerceAtLeast(16) / caps.alignment * caps.alignment).coerceAtLeast(16)
        val errors = ArrayList<String>()
        for (name in decoderCandidates(mime, w, h, caps)) {
            for (format in formatVariants(w, h, csd, caps, name)) {
                var decoder: MediaCodec? = null
                try {
                    decoder = MediaCodec.createByCodecName(name)
                    decoder.configure(format, surface, null, 0)
                    // AOSP default is SCALE_TO_FIT. Some OEM MediaCodec
                    // defaults to SCALE_TO_FIT_WITH_CROPPING, which GPU-
                    // composites every picture — one refresh GlideX /
                    // Moonlight do not pay (HWC overlay). Must be after
                    // configure(), before start().
                    try {
                        decoder.setVideoScalingMode(MediaCodec.VIDEO_SCALING_MODE_SCALE_TO_FIT)
                    } catch (_: Throwable) {
                    }
                    decoder.start()
                    // setParameters cannot fail configure(); poke the SoC
                    // keys in case this try won without them (try 5 / -1).
                    applyRuntimeLowLatency(decoder, name)
                    codec = decoder
                    configured = true
                    activeName = decoder.name
                    skipUntilKey.set(false)
                    fakePtsUs = 0L
                    running.set(true)
                    presentWorker = Thread({ presentLoop() }, "lighting-present").apply { start() }
                    Log.i(TAG, "decoder ok: ${decoder.name} ${w}x$h $mime soc=${caps.soc} gsi=${caps.gsi}")
                    return
                } catch (t: Throwable) {
                    val msg = "$name: ${t.message}"
                    Log.w(TAG, "decoder failed $msg", t)
                    errors.add(msg)
                    try {
                        decoder?.release()
                    } catch (_: Exception) {
                    }
                }
            }
        }
        throw IllegalStateException("解码失败 0xfffffc0e/UNSUPPORTED。${w}x$h $mime。${errors.joinToString(" | ")}")
    }

    fun offer(
        data: ByteArray,
        codecConfig: Boolean,
        keyframe: Boolean,
        ptsUs: Long,
        offset: Int = 0,
    ) {
        if (!configured || !running.get() || data.size <= offset) return
        if (!codecConfig && skipUntilKey.get() && !keyframe) return
        val pkt = Packet(data, offset, codecConfig, keyframe, ptsUs)
        if (codecConfig || keyframe) {
            skipUntilKey.set(false)
        }
        // Moonlight directSubmit: feed MediaCodec from the TCP reader.
        // The extra decode-thread hop used to sit every picture on a
        // scheduler wakeup (often a full vsync on a loaded pad).
        if (enqueueLocked(pkt, 0L)) {
            return
        }
        // IDR/CSD used to hop a leftover decode thread via queue.offer.
        // This reader then offered the next P while the keyframe still
        // sat on take() — a scheduler slice, and P-before-IDR on a loaded
        // pad. Block like live P-frames so ffmpeg skips *input* instead.
        while (running.get()) {
            if (enqueueLocked(pkt, 8_000L)) return
        }
    }

    fun feed(data: ByteArray, codecConfig: Boolean, keyframe: Boolean) {
        offer(data, codecConfig, keyframe, 0L)
    }

    /**
     * Moonlight/GlideX: present on its own thread. Waiting for output on the
     * input thread meant a decode slower than 8 ms sat until the next AU.
     */
    private fun presentLoop() {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_DISPLAY)
        while (running.get()) {
            val decoder = codec ?: break
            try {
                // Block inside MediaCodec until a picture exists. parkNanos
                // is not woken by the codec and becomes a 1–4 ms scheduler
                // slice on a loaded pad; Moonlight waits on dequeue instead.
                // Timeout (not -1) so release() can stop this thread.
                // 8 ms is one 120 Hz period: dequeue times out just before
                // the next picture and the re-enter is that 1–4 ms slice
                // on every frame. Moonlight direct-submit uses 50 ms.
                drain(decoder, 50_000L)
            } catch (_: IllegalStateException) {
                break
            }
        }
    }

    private fun decoderCandidates(mime: String, width: Int, height: Int, caps: DeviceCaps): List<String> {
        val names = LinkedHashSet<String>()
        val list = MediaCodecList(MediaCodecList.ALL_CODECS)
        val hardware = ArrayList<MediaCodecInfo>()
        val software = ArrayList<MediaCodecInfo>()
        for (info in list.codecInfos) {
            if (info.isEncoder) continue
            if (info.supportedTypes.none { it.equals(mime, true) }) continue
            if (clearlyTooSmall(info, mime, width, height)) continue
            if (DeviceCaps.isSoftwareName(info, info.name)) software.add(info) else hardware.add(info)
        }
        // Moonlight: FEATURE_LowLatency / *.low_latency first. C2 without the
        // feature is often listed above OMX and then holds a decoded frame.
        hardware.sortBy { DeviceCaps.decoderRank(it.name, caps.soc) + lowLatencyScore(it, mime) }
        names.addAll(hardware.map { it.name })
        names.addAll(software.map { it.name })
        if (mime == MediaFormat.MIMETYPE_VIDEO_AVC) {
            names.add("c2.android.avc.decoder")
            names.add("OMX.google.h264.decoder")
        }
        if (mime == MediaFormat.MIMETYPE_VIDEO_HEVC) {
            names.add("c2.android.hevc.decoder")
            names.add("OMX.google.hevc.decoder")
        }
        return names.toList()
    }

    private fun lowLatencyScore(info: MediaCodecInfo, mime: String): Int {
        var score = 0
        val n = info.name.lowercase()
        if (n.contains("low_latency") || n.contains("low-latency")) score -= 20
        if (Build.VERSION.SDK_INT >= 30) {
            try {
                if (info.getCapabilitiesForType(mime)
                        .isFeatureSupported(MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency)
                ) {
                    score -= 15
                }
            } catch (_: Throwable) {
            }
        }
        return score
    }

    private fun formatVariants(
        width: Int,
        height: Int,
        csd: ByteArray?,
        caps: DeviceCaps,
        codecName: String,
    ): List<MediaFormat> {
        val out = ArrayList<MediaFormat>()
        val software = DeviceCaps.isSoftwareDecoderName(codecName)
        // Moonlight setDecoderLowLatencyOptions(tryNumber): most-to-least
        // risky. Try 0 is official + matching SoC vendor key. Dumping every
        // vendor key on try 0 used to fail FEATURE_LowLatency configure().
        // Try 1 still has the SoC key (no operating-rate). Try 2 is KEY +
        // SoC (no FEATURE). Try 6 is pool-only; try -1 is fully bare.
        // max-output-buffers=2 and KEY_PRIORITY=0 ride try 0–6 (vendor-key
        // winners included). C2 reads both at configure(); setParameters
        // after start() does not shrink the pool or switch realtime.
        // lowLatencySafe only widens lastTry to 5 (FEATURE/operating-rate
        // reject fallbacks). Matching SoC keys ride try 0–lastTry even on
        // GSI; try 6 / -1 stay without vendor keys.
        if (!software) {
            val lastTry = if (caps.lowLatencySafe) 5 else 2
            for (tryNumber in 0..lastTry) {
                // Matching SoC keys only. The old GSI crash was dumping
                // every vendor key on try 0. GSI Qualcomm advertises
                // FEATURE_LowLatency, so allowVendor=false let try 0
                // configure() without qti-ext and C2 held a reconstructed
                // picture like a movie — one refresh vs the laptop.
                out.add(buildFormat(width, height, csd, codecName, tryNumber, true))
            }
            // GSI / KEY_LOW_LATENCY reject used to skip to try -1 and
            // keep the default 4-8 output pool — one extra decoded
            // picture vs the laptop. Try 6 is max-output-buffers +
            // PRIORITY only (tryNumber >= 5 skips vendor / FEATURE).
            out.add(buildFormat(width, height, csd, codecName, 6, false))
        }
        out.add(buildFormat(width, height, csd, codecName, -1, false))
        return out
    }

    private fun buildFormat(
        width: Int,
        height: Int,
        csd: ByteArray?,
        codecName: String,
        tryNumber: Int,
        allowVendor: Boolean,
    ): MediaFormat {
        val format = MediaFormat.createVideoFormat(mime, width, height)
        format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 8 * 1024 * 1024)
        try {
            // Unset KEY_FRAME_RATE made some SoCs assume 30 fps and hold frames.
            format.setInteger(MediaFormat.KEY_FRAME_RATE, 120)
        } catch (_: Throwable) {
        }
        if (Build.VERSION.SDK_INT >= 24) {
            try {
                // Unspecified DataSpace on an HDR pad makes HWC GPU-compose
                // + tone-map every picture — one refresh GlideX / Moonlight
                // do not pay. Encoder is SDR BT.709 limited; window colorMode
                // is already DEFAULT. This pins the *buffer*.
                format.setInteger(MediaFormat.KEY_COLOR_STANDARD, MediaFormat.COLOR_STANDARD_BT709)
                format.setInteger(MediaFormat.KEY_COLOR_RANGE, MediaFormat.COLOR_RANGE_LIMITED)
                format.setInteger(MediaFormat.KEY_COLOR_TRANSFER, MediaFormat.COLOR_TRANSFER_SDR_VIDEO)
            } catch (_: Throwable) {
            }
        }
        if (Build.VERSION.SDK_INT >= 31) {
            try {
                // Encoder is IPPP / no B-frames. Default reorder depth is 2
                // pictures (~16–32 ms) even when the SPS already says 0.
                format.setInteger(MediaFormat.KEY_OUTPUT_REORDER_DEPTH, 0)
                // Ask C2 to emit SDR even if VUI looks unspecified after
                // annexb strips timing/HRD. Unsupported → 0 after configure.
                format.setInteger(
                    MediaFormat.KEY_COLOR_TRANSFER_REQUEST,
                    MediaFormat.COLOR_TRANSFER_SDR_VIDEO,
                )
            } catch (_: Throwable) {
            }
        }
        if (tryNumber >= 0) {
            applyLowLatencyOptions(format, codecName, tryNumber, allowVendor)
        }
        if (csd != null && csd.isNotEmpty()) {
            applyCsd(format, csd)
        }
        return format
    }

    /**
     * Moonlight MediaCodecHelper.setDecoderLowLatencyOptions(tryNumber).
     * Try 0 is official keys plus the matching SoC vendor key (Moonlight).
     * Qualcomm C2 advertises FEATURE_LowLatency, so a FEATURE-only try 0
     * used to succeed and never reach qti-ext-dec-low-latency — then the
     * decoder paced at SPS like a movie (a vsync of hold). Try 2 is KEY +
     * SoC (no FEATURE / no operating-rate). A KEY-only try 2 used to
     * configure() and skip try 3–4, so C2 never saw qti-ext. Try 5 / -1
     * stay bare. max-output-buffers=2 and KEY_PRIORITY=0 ride try 0–5
     * so a vendor-key configure still gets a 2-slot realtime pool.
     */
    private fun applyLowLatencyOptions(
        format: MediaFormat,
        codecName: String,
        tryNumber: Int,
        allowVendor: Boolean,
    ) {
        val n = codecName.lowercase()
        val qcom = n.startsWith("omx.qcom") || n.startsWith("c2.qti") || n.contains(".qcom.")
        try {
            // 1 output buffer deadlocks (no ping-pong). Default is often
            // 4-8; Moonlight uses 2. Must be on the MediaFormat that
            // actually configure()s — try 3-5 win on vendor keys after
            // FEATURE/operating-rate reject, and used to keep the large
            // pool (one extra decoded picture vs the laptop). Cannot
            // setParameters this after start(). Try -1 stays bare.
            format.setInteger("max-output-buffers", 2)
            if (Build.VERSION.SDK_INT >= 23) {
                // 0 = realtime / ahead of vsync. C2 reads this at
                // configure(). Bound to try 0–2 it vanished when C2
                // rejected operating-rate, then vendor-key try 3–5
                // won and paced a vsync. setParameters after start()
                // does not switch the realtime bin.
                format.setInteger(MediaFormat.KEY_PRIORITY, 0)
            }
            if (tryNumber < 3) {
                format.setInteger("low-latency", 1)
                if (Build.VERSION.SDK_INT >= 30) {
                    format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
                    // KEY_LOW_LATENCY is "low-latency". FEATURE_LowLatency
                    // writes "feature-low-latency". C2 codecs that advertise
                    // the feature still hold a decoded picture unless the
                    // feature- prefix is on; Moonlight/GlideX set both.
                    // Try 2 omits FEATURE so a reject does not skip KEY.
                    if (tryNumber < 2) {
                        format.setFeatureEnabled(
                            MediaCodecInfo.CodecCapabilities.FEATURE_LowLatency,
                            true,
                        )
                    }
                }
                if (tryNumber < 1 && Build.VERSION.SDK_INT >= 23) {
                    format.setInteger(MediaFormat.KEY_OPERATING_RATE, 32767)
                }
                // Matching SoC keys must be on the format that actually
                // succeeds. A KEY-only try 2 used to configure() and skip
                // try 3–4, so C2 never got qti-ext. Moonlight keeps the
                // SoC key through try 4. Try 5 / -1 stay KEY/bare.
                // Do not return on GSI: FEATURE already won try 0 without
                // qti-ext (one reconstructed picture). Try 6 / -1 pass
                // allowVendor=false and still skip the SoC keys below.
                if (!allowVendor) return
            }
            if (tryNumber < 2 &&
                (!android.os.Build.MANUFACTURER.equals("xiaomi", true) ||
                    Build.VERSION.SDK_INT > 23)
            ) {
                format.setInteger("vdec-lowlatency", 1)
            }
        } catch (_: Throwable) {
        }
        if (Build.VERSION.SDK_INT < 26) return
        try {
            when {
                qcom -> {
                    if (tryNumber < 4) {
                        format.setInteger("vendor.qti-ext-dec-picture-order.enable", 1)
                    }
                    if (tryNumber < 5) {
                        format.setInteger("vendor.qti-ext-dec-low-latency.enable", 1)
                    }
                }
                n.startsWith("omx.hisi") || n.startsWith("c2.hisi") || n.contains("kirin") -> {
                    if (tryNumber < 4) {
                        format.setInteger("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-req", 1)
                        format.setInteger("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-rdy", -1)
                    }
                }
                n.startsWith("omx.exynos") || n.startsWith("c2.exynos") || n.contains(".sec.") || n.contains("samsung") -> {
                    if (tryNumber < 4) {
                        format.setInteger("vendor.rtc-ext-dec-low-latency.enable", 1)
                        // Pixel Tensor is also c2.exynos (Google). sec-ext is
                        // Samsung C2; an unknown vendor key fails configure()
                        // and skips rtc-ext. Only Samsung/SEC get both.
                        if (n.contains(".sec.") || n.contains("samsung") ||
                            android.os.Build.MANUFACTURER.equals("samsung", true)
                        ) {
                            format.setInteger("vendor.sec-ext-dec-low-latency.enable", 1)
                            // Same display-order bypass as Qualcomm
                            // qti-ext-dec-picture-order: SEC otherwise
                            // holds a reconstructed picture for VUI.
                            format.setInteger("vendor.sec-ext-dec-picture-order.enable", 1)
                        }
                    }
                }
                n.startsWith("omx.amlogic") || n.startsWith("c2.amlogic") -> {
                    if (tryNumber < 4) {
                        format.setInteger("vendor.low-latency.enable", 1)
                    }
                }
                n.startsWith("omx.mtk") || n.startsWith("c2.mtk") || n.contains(".mtk.") || n.contains("mediatek") -> {
                    if (tryNumber < 4) {
                        format.setInteger("vendor.mtk.vdec.low.latency", 1)
                        // ByteVU is the display-path sibling of mtk.vdec.
                        // C2 reads it at configure(); the post-start poke
                        // used to land after the decoder already queued a
                        // reconstructed picture (one refresh vs the laptop).
                        // Try 4 stays mtk-ext-only so an unknown ByteVU
                        // key cannot skip the Dimensity fallback.
                        format.setInteger("vendor.mtk.vdec.bytevu.low-latency.enable", 1)
                    }
                    // Dimensity C2 ignores the OMX-era mtk.vdec key.
                    // Without mtk-ext the decoder holds a reconstructed
                    // picture — one refresh vs the laptop. Try 4 keeps
                    // this if mtk.vdec itself was the reject.
                    if (tryNumber < 5) {
                        format.setInteger("vendor.mtk-ext-dec-low-latency.enable", 1)
                    }
                }
            }
        } catch (_: Throwable) {
        }
    }

    /**
     * Vendor low-latency after start(). Configure try 5 / -1 omit these so a
     * reject still succeeds; without the poke, FEATURE_LowLatency C2 holds
     * a decoded picture. Each key is its own bundle: one unknown vendor
     * extra must not skip qti-ext-dec-low-latency.
     */
    private fun applyRuntimeLowLatency(decoder: MediaCodec, codecName: String) {
        fun poke(key: String, value: Int) {
            try {
                val p = android.os.Bundle()
                p.putInt(key, value)
                decoder.setParameters(p)
            } catch (_: Throwable) {
            }
        }
        poke("low-latency", 1)
        if (Build.VERSION.SDK_INT >= 30) {
            poke(MediaFormat.KEY_LOW_LATENCY, 1)
        }
        if (Build.VERSION.SDK_INT >= 23) {
            poke(MediaFormat.KEY_PRIORITY, 0)
        }
        // Configure try 2+ omits vdec-lowlatency. Without this poke,
        // MTK/Amazon C2 holds a decoded picture (Moonlight MediaCodecHelper).
        poke("vdec-lowlatency", 1)
        val n = codecName.lowercase()
        when {
            n.startsWith("omx.qcom") || n.startsWith("c2.qti") || n.contains(".qcom.") -> {
                poke("vendor.qti-ext-dec-picture-order.enable", 1)
                poke("vendor.qti-ext-dec-low-latency.enable", 1)
            }
            n.startsWith("omx.hisi") || n.startsWith("c2.hisi") || n.contains("kirin") -> {
                poke("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-req", 1)
                poke("vendor.hisi-ext-low-latency-video-dec.video-scene-for-low-latency-rdy", -1)
            }
            n.startsWith("omx.exynos") || n.startsWith("c2.exynos") || n.contains(".sec.") || n.contains("samsung") -> {
                poke("vendor.rtc-ext-dec-low-latency.enable", 1)
                if (n.contains(".sec.") || n.contains("samsung") ||
                    android.os.Build.MANUFACTURER.equals("samsung", true)
                ) {
                    poke("vendor.sec-ext-dec-low-latency.enable", 1)
                    poke("vendor.sec-ext-dec-picture-order.enable", 1)
                }
            }
            n.startsWith("omx.amlogic") || n.startsWith("c2.amlogic") -> {
                poke("vendor.low-latency.enable", 1)
            }
            n.startsWith("omx.mtk") || n.startsWith("c2.mtk") || n.contains(".mtk.") || n.contains("mediatek") -> {
                poke("vendor.mtk.vdec.low.latency", 1)
                poke("vendor.mtk-ext-dec-low-latency.enable", 1)
                // Configure try 4+ omits ByteVU so an unknown key can
                // still win on mtk-ext. Poke covers try 4 / 5 / -1.
                poke("vendor.mtk.vdec.bytevu.low-latency.enable", 1)
            }
        }
    }

    private fun enqueueLocked(pkt: Packet, waitUs: Long): Boolean {
        val decoder = codec ?: return false
        val flags = when {
            pkt.codecConfig -> MediaCodec.BUFFER_FLAG_CODEC_CONFIG
            pkt.keyframe -> MediaCodec.BUFFER_FLAG_KEY_FRAME
            else -> 0
        }
        // Never block the TCP reader on dequeueInputBuffer. Wait outside the
        // lock so a 4–16 ms retry cannot HOL-stall the next picture.
        synchronized(inputLock) {
            if (!running.get() || codec !== decoder) return false
            if (enqueue(decoder, pkt, flags, 0L)) return true
        }
        if (waitUs <= 0L) return false
        // Block inside MediaCodec until an input slot exists. Thread.sleep(1)
        // is not woken when present() frees a buffer; at 120 Hz that was a
        // 1–8 ms slice GlideX / Moonlight do not pay.
        val index = try {
            decoder.dequeueInputBuffer(waitUs)
        } catch (_: IllegalStateException) {
            return false
        }
        if (index < 0) return false
        synchronized(inputLock) {
            if (!running.get() || codec !== decoder) {
                try {
                    decoder.queueInputBuffer(index, 0, 0, 0, 0)
                } catch (_: Throwable) {
                }
                return false
            }
            return fillInput(decoder, index, pkt, flags)
        }
    }

    private fun enqueue(
        decoder: MediaCodec,
        pkt: Packet,
        flags: Int,
        waitUs: Long,
    ): Boolean {
        val index = decoder.dequeueInputBuffer(waitUs)
        if (index < 0) return false
        return fillInput(decoder, index, pkt, flags)
    }

    private fun fillInput(
        decoder: MediaCodec,
        index: Int,
        pkt: Packet,
        flags: Int,
    ): Boolean {
        val buf = decoder.getInputBuffer(index) ?: return false
        val size = pkt.data.size - pkt.offset
        if (size <= 0 || buf.remaining() < size) {
            decoder.queueInputBuffer(index, 0, 0, 0, 0)
            return false
        }
        buf.clear()
        buf.put(pkt.data, pkt.offset, size)
        // Moonlight/GlideX: never pace MediaCodec with the host wall clock.
        // Host elapsed-µs as PTS made some SoCs hold frames like a movie.
        fakePtsUs += 1
        decoder.queueInputBuffer(index, 0, size, fakePtsUs, flags)
        return true
    }

    private fun drain(decoder: MediaCodec, waitUs: Long): Boolean {
        val info = MediaCodec.BufferInfo()
        var latest = -1
        var wait = waitUs
        while (true) {
            val idx = decoder.dequeueOutputBuffer(info, wait)
            wait = 0
            if (idx == MediaCodec.INFO_TRY_AGAIN_LATER) {
                break
            }
            if (idx == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED ||
                idx == MediaCodec.INFO_OUTPUT_BUFFERS_CHANGED
            ) {
                continue
            }
            if (idx < 0) break
            if ((info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG) != 0) {
                decoder.releaseOutputBuffer(idx, false)
                continue
            }
            if (latest >= 0) {
                decoder.releaseOutputBuffer(latest, false)
            }
            latest = idx
        }
        if (latest >= 0) {
            // Moonlight FRAME_PACING_MIN_LATENCY (the default): stamp now so
            // SurfaceFlinger drops this buffer if another picture arrives
            // before the next vsync. 0L is MAX_SMOOTHNESS — never drop —
            // and a 60 Hz pad then queued a second picture (one extra
            // refresh vs the PC monitor). `true` reuses the fake 1 µs PTS
            // and some SoCs wait a vsync as if this were a movie.
            decoder.releaseOutputBuffer(latest, System.nanoTime())
            return true
        }
        return false
    }

    fun release() {
        running.set(false)
        presentWorker?.interrupt()
        configured = false
        activeName = ""
        try {
            codec?.stop()
        } catch (_: Exception) {
        }
        try {
            presentWorker?.join(300)
        } catch (_: Exception) {
        }
        presentWorker = null
        try {
            codec?.release()
        } catch (_: Exception) {
        }
        codec = null
    }

    private fun applyCsd(format: MediaFormat, annexb: ByteArray) {
        val nals = splitAnnexB(annexb)
        if (mime == MediaFormat.MIMETYPE_VIDEO_HEVC) {
            val vpsSpsPps = nals.filter { type ->
                val t = hevcType(type)
                t == 32 || t == 33 || t == 34
            }
            val bytes = if (vpsSpsPps.isNotEmpty()) concat(vpsSpsPps) else annexb
            format.setByteBuffer("csd-0", ByteBuffer.wrap(bytes))
            return
        }
        val sps = nals.find { h264Type(it) == 7 }
        val pps = nals.find { h264Type(it) == 8 }
        if (sps != null) {
            format.setByteBuffer("csd-0", ByteBuffer.wrap(sps))
        }
        if (pps != null) {
            format.setByteBuffer("csd-1", ByteBuffer.wrap(pps))
        }
        if (sps == null && pps == null) {
            format.setByteBuffer("csd-0", ByteBuffer.wrap(annexb))
        }
    }

    private fun concat(parts: List<ByteArray>): ByteArray {
        val n = parts.sumOf { it.size }
        val out = ByteArray(n)
        var i = 0
        for (p in parts) {
            p.copyInto(out, i)
            i += p.size
        }
        return out
    }

    private fun h264Type(nal: ByteArray): Int {
        val i = startLen(nal)
        return if (i < nal.size) nal[i].toInt() and 0x1F else 0
    }

    private fun hevcType(nal: ByteArray): Int {
        val i = startLen(nal)
        return if (i < nal.size) (nal[i].toInt() shr 1) and 0x3F else 0
    }

    private fun startLen(nal: ByteArray): Int {
        return when {
            nal.size >= 4 && nal[0] == 0.toByte() && nal[1] == 0.toByte() && nal[2] == 0.toByte() && nal[3] == 1.toByte() -> 4
            nal.size >= 3 && nal[0] == 0.toByte() && nal[1] == 0.toByte() && nal[2] == 1.toByte() -> 3
            else -> 0
        }
    }

    private fun splitAnnexB(data: ByteArray): List<ByteArray> {
        val starts = ArrayList<Int>()
        var i = 0
        while (i + 2 < data.size) {
            if (data[i] == 0.toByte() && data[i + 1] == 0.toByte()) {
                if (data[i + 2] == 1.toByte()) {
                    starts.add(i)
                    i += 3
                    continue
                }
                if (i + 3 < data.size && data[i + 2] == 0.toByte() && data[i + 3] == 1.toByte()) {
                    starts.add(i)
                    i += 4
                    continue
                }
            }
            i++
        }
        if (starts.isEmpty()) return listOf(data)
        val out = ArrayList<ByteArray>()
        for (s in starts.indices) {
            val a = starts[s]
            val b = if (s + 1 < starts.size) starts[s + 1] else data.size
            out.add(data.copyOfRange(a, b))
        }
        return out
    }

    companion object {
        private const val TAG = "LightingDecoder"
    }

    private data class Packet(
        val data: ByteArray,
        val offset: Int,
        val codecConfig: Boolean,
        val keyframe: Boolean,
        val ptsUs: Long,
    )
}

fun preferredCodecs(): List<String> = DeviceCaps.probe().codecs

fun hardwareDecodeLimit(mime: String): Pair<Int, Int> {
    val caps = DeviceCaps.probe()
    return caps.decoderMaxWidth to caps.decoderMaxHeight
}

fun sizeSupported(info: MediaCodecInfo, mime: String, width: Int, height: Int): Boolean {
    return try {
        val caps = info.getCapabilitiesForType(mime).videoCapabilities ?: return true
        caps.isSizeSupported(width, height) || caps.isSizeSupported(height, width)
    } catch (_: Throwable) {
        true
    }
}

/** Some vendors (especially Qualcomm 16-align) report isSizeSupported(1920,1080)=false while configure() works. */
fun clearlyTooSmall(info: MediaCodecInfo, mime: String, width: Int, height: Int): Boolean {
    return try {
        val caps = info.getCapabilitiesForType(mime).videoCapabilities ?: return false
        val maxW = caps.supportedWidths.upper
        val maxH = caps.supportedHeights.upper
        val fits = (width <= maxW && height <= maxH) || (width <= maxH && height <= maxW)
        !fits
    } catch (_: Throwable) {
        false
    }
}

fun isCodecConfigNal(data: ByteArray, hevc: Boolean): Boolean {
    if (data.size < 5 || data[0] != 0.toByte() || data[1] != 0.toByte()) return false
    var i = 0
    if (data.size >= 4 && data[2] == 0.toByte() && data[3] == 1.toByte()) i = 4
    else if (data[2] == 1.toByte()) i = 3
    else return false
    if (i >= data.size) return false
    return if (hevc) {
        val t = (data[i].toInt() shr 1) and 0x3F
        t == 32 || t == 33 || t == 34
    } else {
        val t = data[i].toInt() and 0x1F
        t == 7 || t == 8
    }
}
