package app.lighting.display

import org.json.JSONArray
import org.json.JSONObject
import java.io.DataInputStream
import java.io.DataOutputStream
import java.io.EOFException
import java.net.InetSocketAddress
import java.net.Socket

object LitProtocol {
    const val PORT = 17400
    const val MSG_HELLO: Byte = 1
    const val MSG_CONFIG: Byte = 2
    const val MSG_VIDEO: Byte = 3
    const val MSG_TOUCH: Byte = 4
    const val MSG_HEARTBEAT: Byte = 5
    const val MSG_ERROR: Byte = 6
    const val MSG_AUDIO: Byte = 7
    const val FLAG_KEYFRAME: Int = 1
    const val FLAG_CODEC_CONFIG: Int = 2
    private val MAGIC = byteArrayOf('L'.code.toByte(), 'I'.code.toByte(), 'T'.code.toByte(), '1'.code.toByte())

    data class Message(val type: Byte, val flags: Int, val payload: ByteArray)

    fun read(input: DataInputStream): Message {
        val hdr = ByteArray(12)
        input.readFully(hdr)
        if (hdr[0] != MAGIC[0] || hdr[1] != MAGIC[1] || hdr[2] != MAGIC[2] || hdr[3] != MAGIC[3]) {
            throw IllegalStateException("bad magic")
        }
        val type = hdr[4]
        val flags = hdr[5].toInt() and 0xFF
        val len = ((hdr[8].toInt() and 0xFF) shl 24) or
            ((hdr[9].toInt() and 0xFF) shl 16) or
            ((hdr[10].toInt() and 0xFF) shl 8) or
            (hdr[11].toInt() and 0xFF)
        if (len < 0 || len > 16 * 1024 * 1024) {
            throw IllegalStateException("bad length $len")
        }
        val payload = ByteArray(len)
        if (len > 0) {
            input.readFully(payload)
        }
        return Message(type, flags, payload)
    }

    fun write(output: DataOutputStream, type: Byte, flags: Int, payload: ByteArray) {
        val hdr = ByteArray(12)
        MAGIC.copyInto(hdr)
        hdr[4] = type
        hdr[5] = flags.toByte()
        val len = payload.size
        hdr[8] = (len ushr 24).toByte()
        hdr[9] = (len ushr 16).toByte()
        hdr[10] = (len ushr 8).toByte()
        hdr[11] = len.toByte()
        output.write(hdr)
        if (payload.isNotEmpty()) {
            output.write(payload)
        }
        output.flush()
    }

    fun helloJson(caps: DeviceCaps, w: Int, h: Int, maxFps: Int): ByteArray {
        val obj = JSONObject()
        obj.put("protocol", 1)
        obj.put("device", "${caps.manufacturer} ${caps.model}".trim())
        obj.put("screenWidth", w)
        obj.put("screenHeight", h)
        obj.put("maxFps", maxFps.coerceAtMost(caps.decoderMaxFps).coerceIn(24, 120))
        val arr = JSONArray()
        caps.codecs.forEach { arr.put(it) }
        obj.put("codecs", arr)
        obj.put("wantAudio", true)
        obj.put("decoderMaxWidth", caps.decoderMaxWidth)
        obj.put("decoderMaxHeight", caps.decoderMaxHeight)
        obj.put("decoderMaxFps", caps.decoderMaxFps)
        obj.put("hwDecode", caps.hwDecode)
        obj.put("alignment", caps.alignment)
        obj.put("soc", caps.soc)
        obj.put("gsi", caps.gsi)
        obj.put("brand", caps.brand)
        fun putLimit(key: String, limit: DeviceCaps.CodecLimit?) {
            if (limit == null) return
            val o = JSONObject()
            o.put("width", limit.width)
            o.put("height", limit.height)
            o.put("fps", limit.fps)
            o.put("hw", limit.hw)
            o.put("name", limit.name)
            obj.put(key, o)
        }
        putLimit("avcLimit", caps.avc)
        putLimit("hevcLimit", caps.hevc)
        return obj.toString().toByteArray(Charsets.UTF_8)
    }

    fun parseConfig(payload: ByteArray): StreamConfig {
        val obj = JSONObject(String(payload, Charsets.UTF_8))
        return StreamConfig(
            width = obj.getInt("width"),
            height = obj.getInt("height"),
            fps = obj.getInt("fps"),
            codec = obj.getString("codec"),
            bitrateKbps = obj.getInt("bitrateKbps"),
            audioEnabled = obj.optBoolean("audioEnabled", false),
            audioSampleRate = obj.optInt("audioSampleRate", 48000),
            audioChannels = obj.optInt("audioChannels", 2),
        )
    }

    fun touchPayload(action: Int, x: Int, y: Int, extra: Int = 0): ByteArray {
        val p = ByteArray(8)
        val xx = x and 0xFFFF
        val yy = y and 0xFFFF
        val zz = extra and 0xFFFF
        p[0] = action.toByte()
        p[1] = 0
        p[2] = (xx ushr 8).toByte()
        p[3] = xx.toByte()
        p[4] = (yy ushr 8).toByte()
        p[5] = yy.toByte()
        p[6] = (zz ushr 8).toByte()
        p[7] = zz.toByte()
        return p
    }
}

data class StreamConfig(
    val width: Int,
    val height: Int,
    val fps: Int,
    val codec: String,
    val bitrateKbps: Int,
    val audioEnabled: Boolean = false,
    val audioSampleRate: Int = 48000,
    val audioChannels: Int = 2,
)

class LitSocket(host: String, port: Int, connectTimeoutMs: Int = 1_500) : AutoCloseable {
    val socket: Socket = Socket().apply {
        tcpNoDelay = true
        keepAlive = true
        try {
            connect(InetSocketAddress(host, port), connectTimeoutMs)
        } catch (t: Throwable) {
            try {
                close()
            } catch (_: Exception) {
            }
            throw t
        }
    }
    val input = DataInputStream(socket.getInputStream())
    val output = DataOutputStream(socket.getOutputStream())
    private val writeLock = Any()

    fun read(): LitProtocol.Message = LitProtocol.read(input)
    fun write(type: Byte, flags: Int = 0, payload: ByteArray = ByteArray(0)) {
        synchronized(writeLock) {
            LitProtocol.write(output, type, flags, payload)
        }
    }

    override fun close() {
        try {
            socket.close()
        } catch (_: Exception) {
        }
    }
}

fun isEof(t: Throwable): Boolean = t is EOFException || t.message?.contains("Broken pipe", true) == true

/** `failIndex` 0 = first retry after a drop. Includes caller-supplied jitter (0–200ms). */
fun reconnectBackoffMs(failIndex: Int, jitterMs: Int): Long {
    val base = when {
        failIndex <= 0 -> 650L
        failIndex == 1 -> 900L
        failIndex == 2 -> 1300L
        failIndex == 3 -> 1800L
        else -> 2400L
    }
    return base + jitterMs.coerceIn(0, 200)
}

@Suppress("UNUSED_PARAMETER")
fun describeConnectError(error: Throwable, host: String, port: Int): String =
    ConnectCopy.fromThrowable(error, host).primary
