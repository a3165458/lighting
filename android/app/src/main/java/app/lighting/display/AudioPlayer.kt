package app.lighting.display

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicLong

class AudioPlayer(sampleRate: Int, channels: Int) {
    private val track: AudioTrack
    private val queue = ArrayBlockingQueue<ByteArray>(24)
    private val running = AtomicBoolean(true)
    val lastPtsUs = AtomicLong(0)
    private val worker: Thread

    init {
        val ch = if (channels >= 2) AudioFormat.CHANNEL_OUT_STEREO else AudioFormat.CHANNEL_OUT_MONO
        val min = AudioTrack.getMinBufferSize(sampleRate, ch, AudioFormat.ENCODING_PCM_16BIT)
        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_MEDIA)
            .setContentType(AudioAttributes.CONTENT_TYPE_MOVIE)
            .build()
        val format = AudioFormat.Builder()
            .setSampleRate(sampleRate)
            .setChannelMask(ch)
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .build()
        val builder = AudioTrack.Builder()
            .setAudioAttributes(attrs)
            .setAudioFormat(format)
            .setBufferSizeInBytes((min * 4).coerceAtLeast(sampleRate / 5 * 4))
            .setTransferMode(AudioTrack.MODE_STREAM)
        if (Build.VERSION.SDK_INT >= 26) {
            builder.setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
        }
        track = builder.build()
        track.play()
        worker = Thread({
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_AUDIO)
            while (running.get()) {
                val chunk = try {
                    queue.poll(8, TimeUnit.MILLISECONDS)
                } catch (_: InterruptedException) {
                    break
                } ?: continue
                var off = 0
                while (off < chunk.size && running.get()) {
                    val n = track.write(chunk, off, chunk.size - off)
                    if (n <= 0) break
                    off += n
                }
                if (track.playState != AudioTrack.PLAYSTATE_PLAYING) {
                    try {
                        track.play()
                    } catch (_: Exception) {
                    }
                }
            }
        }, "lighting-audio").apply { start() }
    }

    fun offer(pcm: ByteArray, ptsUs: Long) {
        if (pcm.isEmpty() || !running.get()) return
        lastPtsUs.set(ptsUs)
        if (!queue.offer(pcm)) {
            queue.poll()
            queue.offer(pcm)
        }
    }

    fun release() {
        running.set(false)
        worker.interrupt()
        try {
            worker.join(200)
        } catch (_: Exception) {
        }
        try {
            track.pause()
            track.flush()
            track.release()
        } catch (_: Exception) {
        }
    }
}

fun splitPts(payload: ByteArray): Pair<Long, ByteArray> {
    if (payload.size < 8) return 0L to payload
    var pts = 0L
    for (i in 0 until 8) {
        pts = (pts shl 8) or (payload[i].toLong() and 0xFF)
    }
    return pts to payload.copyOfRange(8, payload.size)
}
