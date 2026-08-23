package app.lighting.display

import android.os.Build
import android.os.Bundle
import android.util.DisplayMetrics
import android.util.Log
import android.view.Gravity
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import kotlin.concurrent.thread

class DisplayActivity : AppCompatActivity(), SurfaceHolder.Callback {
    companion object {
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
    }

    private lateinit var surface: SurfaceView
    private lateinit var status: TextView
    private lateinit var touchLayer: View
    private var worker: Thread? = null
    @Volatile private var running = false
    @Volatile private var lit: LitSocket? = null
    private val decoder = VideoDecoder()
    private var audio: AudioPlayer? = null
    private var streamW = 0
    private var streamH = 0
    private val touch = TouchMapper { action, x, y ->
        val sock = lit ?: return@TouchMapper
        try {
            sock.write(LitProtocol.MSG_TOUCH, 0, LitProtocol.touchPayload(action, x, y))
        } catch (_: Exception) {
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_display)
        hideSystemUi()
        surface = findViewById(R.id.surface)
        status = findViewById(R.id.status)
        touchLayer = findViewById(R.id.touchLayer)
        status.isClickable = false
        status.isFocusable = false
        surface.holder.addCallback(this)
        touch.attach(touchLayer, surface)
    }

    private fun hideSystemUi() {
        if (Build.VERSION.SDK_INT >= 30) {
            window.insetsController?.let {
                it.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
                it.systemBarsBehavior = WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            }
        } else {
            @Suppress("DEPRECATION")
            window.decorView.systemUiVisibility =
                View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or
                    View.SYSTEM_UI_FLAG_FULLSCREEN or
                    View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
        }
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        startSession()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (streamW > 0 && streamH > 0) {
            letterboxSurface(streamW, streamH)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        stopSession()
    }

    override fun onDestroy() {
        stopSession()
        super.onDestroy()
    }

    private fun startSession() {
        stopSession()
        running = true
        val host = intent.getStringExtra(EXTRA_HOST) ?: "127.0.0.1"
        val port = intent.getIntExtra(EXTRA_PORT, LitProtocol.PORT)
        val metrics = DisplayMetrics()
        @Suppress("DEPRECATION")
        val display = windowManager.defaultDisplay
        display.getRealMetrics(metrics)
        val refresh = display.refreshRate.toInt().coerceIn(30, 120)
        val caps = DeviceCaps.probe()
        worker = thread(name = "lighting-session") {
            try {
                setStatus("连接 $host:$port …")
                val sock = LitSocket(host, port)
                lit = sock
                val hello = LitProtocol.helloJson(
                    caps = caps,
                    w = metrics.widthPixels,
                    h = metrics.heightPixels,
                    maxFps = refresh,
                )
                sock.write(LitProtocol.MSG_HELLO, 0, hello)
                val cfgMsg = sock.read()
                if (cfgMsg.type != LitProtocol.MSG_CONFIG) {
                    throw IllegalStateException("expected config, got ${cfgMsg.type}")
                }
                val cfg = LitProtocol.parseConfig(cfgMsg.payload)
                val hevc = cfg.codec.equals("hevc", true) || cfg.codec.equals("h265", true)
                if (cfg.audioEnabled) {
                    audio = AudioPlayer(cfg.audioSampleRate, cfg.audioChannels)
                }
                setStatus("${cfg.codec} ${cfg.width}×${cfg.height}@${cfg.fps}${if (cfg.audioEnabled) " +音频" else ""} 等待关键帧…")
                var configured = false
                while (running) {
                    val msg = sock.read()
                    when (msg.type) {
                        LitProtocol.MSG_VIDEO -> {
                            val (pts, data) = splitPts(msg.payload)
                            val isCfg = msg.flags and LitProtocol.FLAG_CODEC_CONFIG != 0
                            val key = msg.flags and LitProtocol.FLAG_KEYFRAME != 0
                            if (!configured) {
                                val canInit = isCfg || isCodecConfigNal(data, hevc)
                                if (!canInit) continue
                                decoder.configure(
                                    cfg.codec,
                                    cfg.width,
                                    cfg.height,
                                    data,
                                    surface.holder.surface,
                                )
                                configured = true
                                letterboxSurface(cfg.width, cfg.height)
                                setStatus("${cfg.codec} ${cfg.width}×${cfg.height}@${cfg.fps} ${decoder.activeName}${if (cfg.audioEnabled) " +音频" else ""}  单击/拖动 · 长按右键 · 双指滚动")
                                if (!isCfg) {
                                    decoder.offer(data, codecConfig = false, keyframe = true, ptsUs = pts)
                                }
                                continue
                            }
                            decoder.offer(data, codecConfig = isCfg, keyframe = key, ptsUs = pts)
                        }
                        LitProtocol.MSG_AUDIO -> {
                            val (pts, pcm) = splitPts(msg.payload)
                            audio?.offer(pcm, pts)
                        }
                        LitProtocol.MSG_HEARTBEAT -> sock.write(LitProtocol.MSG_HEARTBEAT)
                        LitProtocol.MSG_ERROR -> setStatus(String(msg.payload, Charsets.UTF_8))
                    }
                }
            } catch (t: Throwable) {
                Log.e("Lighting", "session failed", t)
                if (running) setStatus("断开：${t.message ?: t.javaClass.simpleName}")
            } finally {
                decoder.release()
                audio?.release()
                audio = null
                lit?.close()
                lit = null
            }
        }
    }

    private fun stopSession() {
        touch.cancel()
        running = false
        try {
            lit?.close()
        } catch (_: Exception) {
        }
        worker?.join(400)
        worker = null
        decoder.release()
        audio?.release()
        audio = null
    }

    private fun letterboxSurface(width: Int, height: Int) {
        streamW = width
        streamH = height
        runOnUiThread {
            val parent = surface.parent as? View
            val pw = parent?.width?.takeIf { it > 0 } ?: resources.displayMetrics.widthPixels
            val ph = parent?.height?.takeIf { it > 0 } ?: resources.displayMetrics.heightPixels
            val w: Int
            val h: Int
            if (width <= pw && height <= ph) {
                w = width
                h = height
            } else {
                val scale = minOf(pw.toFloat() / width, ph.toFloat() / height)
                w = (width * scale).toInt().coerceAtLeast(2) and 1.inv()
                h = (height * scale).toInt().coerceAtLeast(2) and 1.inv()
            }
            val lp = (surface.layoutParams as FrameLayout.LayoutParams).apply {
                this.width = w
                this.height = h
                gravity = Gravity.CENTER
            }
            surface.layoutParams = lp
            try {
                surface.holder.setFixedSize(width, height)
            } catch (_: Exception) {
            }
        }
    }

    private fun setStatus(text: String) {
        runOnUiThread { status.text = text }
    }
}
