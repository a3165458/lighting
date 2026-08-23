package app.lighting.display

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaCodecList
import android.media.MediaFormat
import android.os.Build
import android.util.Log
import android.view.Surface
import java.nio.ByteBuffer
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class VideoDecoder {
    private var codec: MediaCodec? = null
    private var configured = false
    private var mime: String = MediaFormat.MIMETYPE_VIDEO_AVC
    private val running = AtomicBoolean(false)
    private val skipUntilKey = AtomicBoolean(false)
    private val queue = ArrayBlockingQueue<Packet>(3)
    private var worker: Thread? = null
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
                    decoder.start()
                    codec = decoder
                    configured = true
                    activeName = decoder.name
                    skipUntilKey.set(false)
                    running.set(true)
                    worker = Thread({ loop() }, "lighting-decode").apply { start() }
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

    fun offer(data: ByteArray, codecConfig: Boolean, keyframe: Boolean, ptsUs: Long) {
        if (!configured || !running.get() || data.isEmpty()) return
        if (!codecConfig && skipUntilKey.get() && !keyframe) return
        val pkt = Packet(data, codecConfig, keyframe, ptsUs)
        if (queue.offer(pkt)) {
            if (keyframe) skipUntilKey.set(false)
            return
        }
        skipUntilKey.set(true)
        if (keyframe) {
            queue.clear()
            if (queue.offer(pkt)) skipUntilKey.set(false)
        }
    }

    fun feed(data: ByteArray, codecConfig: Boolean, keyframe: Boolean) {
        offer(data, codecConfig, keyframe, 0L)
    }

    private fun loop() {
        android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_DISPLAY)
        var frames = 0
        var skips = 0
        var lagSum = 0L
        while (running.get()) {
            val pkt = try {
                queue.poll(4, TimeUnit.MILLISECONDS)
            } catch (_: InterruptedException) {
                break
            } ?: continue
            val decoder = codec ?: continue
            if (!configured) continue
            if (pkt.codecConfig) {
                enqueue(decoder, pkt.data, MediaCodec.BUFFER_FLAG_CODEC_CONFIG, pkt.ptsUs, waitUs = 8_000)
                drain(decoder)
                continue
            }
            if (skipUntilKey.get() && !pkt.keyframe) {
                skips++
                continue
            }
            val flags = if (pkt.keyframe) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0
            val wait = if (pkt.keyframe) 8_000L else 0L
            if (!enqueue(decoder, pkt.data, flags, pkt.ptsUs, wait)) {
                skipUntilKey.set(true)
                skips++
                continue
            }
            if (pkt.keyframe) skipUntilKey.set(false)
            drain(decoder)
            frames++
            if (pkt.ptsUs > 0) {
                val now = System.nanoTime() / 1000
                lagSum += (now - pkt.ptsUs).coerceAtLeast(0)
            }
            if (frames % 120 == 0) {
                val avgLagMs = if (frames > 0) lagSum / frames / 1000 else 0
                Log.i(
                    TAG,
                    "stats decoder=$activeName q=${queue.size} skips=$skips avgLagMs=$avgLagMs",
                )
                skips = 0
                lagSum = 0
                frames = 0
            }
        }
    }

    private fun decoderCandidates(mime: String, width: Int, height: Int, caps: DeviceCaps): List<String> {
        val names = LinkedHashSet<String>()
        val list = MediaCodecList(MediaCodecList.ALL_CODECS)
        val hardware = ArrayList<String>()
        val software = ArrayList<String>()
        for (info in list.codecInfos) {
            if (info.isEncoder) continue
            if (info.supportedTypes.none { it.equals(mime, true) }) continue
            if (clearlyTooSmall(info, mime, width, height)) continue
            val name = info.name
            if (DeviceCaps.isSoftwareName(info, name)) software.add(name) else hardware.add(name)
        }
        hardware.sortBy { DeviceCaps.decoderRank(it, caps.soc) }
        names.addAll(hardware)
        names.addAll(software)
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

    private fun formatVariants(
        width: Int,
        height: Int,
        csd: ByteArray?,
        caps: DeviceCaps,
        codecName: String,
    ): List<MediaFormat> {
        val out = ArrayList<MediaFormat>()
        out.add(buildFormat(width, height, csd, lowLatency = false, operatingRate = false))
        val n = codecName.lowercase()
        val software = n.contains("google") || n.contains("c2.android") || n.contains("software")
        if (caps.lowLatencySafe && !software) {
            out.add(buildFormat(width, height, csd, lowLatency = true, operatingRate = true))
        }
        return out
    }

    private fun buildFormat(
        width: Int,
        height: Int,
        csd: ByteArray?,
        lowLatency: Boolean,
        operatingRate: Boolean,
    ): MediaFormat {
        val format = MediaFormat.createVideoFormat(mime, width, height)
        format.setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, 8 * 1024 * 1024)
        if (operatingRate) {
            try {
                format.setInteger(MediaFormat.KEY_PRIORITY, 0)
                format.setInteger(MediaFormat.KEY_OPERATING_RATE, 60)
            } catch (_: Throwable) {
            }
        }
        if (lowLatency && Build.VERSION.SDK_INT >= 30) {
            try {
                format.setInteger(MediaFormat.KEY_LOW_LATENCY, 1)
            } catch (_: Throwable) {
            }
        }
        if (csd != null && csd.isNotEmpty()) {
            applyCsd(format, csd)
        }
        return format
    }

    private fun enqueue(
        decoder: MediaCodec,
        data: ByteArray,
        flags: Int,
        ptsUs: Long,
        waitUs: Long,
    ): Boolean {
        val index = decoder.dequeueInputBuffer(waitUs)
        if (index < 0) return false
        val buf = decoder.getInputBuffer(index) ?: return false
        if (buf.remaining() < data.size) {
            decoder.queueInputBuffer(index, 0, 0, 0, 0)
            return false
        }
        buf.clear()
        buf.put(data)
        val ts = if (ptsUs > 0) ptsUs else System.nanoTime() / 1000
        decoder.queueInputBuffer(index, 0, data.size, ts, flags)
        return true
    }

    private fun drain(decoder: MediaCodec) {
        val info = MediaCodec.BufferInfo()
        while (true) {
            val idx = decoder.dequeueOutputBuffer(info, 0)
            if (idx >= 0) {
                decoder.releaseOutputBuffer(idx, true)
            } else {
                break
            }
        }
    }

    fun release() {
        running.set(false)
        queue.clear()
        worker?.interrupt()
        try {
            worker?.join(300)
        } catch (_: Exception) {
        }
        worker = null
        configured = false
        activeName = ""
        try {
            codec?.stop()
        } catch (_: Exception) {
        }
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
