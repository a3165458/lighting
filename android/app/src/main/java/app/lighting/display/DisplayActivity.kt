package app.lighting.display

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.AttributeSet
import android.util.DisplayMetrics
import android.util.Log
import android.view.Gravity
import android.view.MotionEvent
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AppCompatActivity
import java.util.concurrent.ArrayBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

class DisplayActivity : AppCompatActivity(), SurfaceHolder.Callback {
    companion object {
        const val EXTRA_HOST = "host"
        const val EXTRA_PORT = "port"
        const val EXTRA_ERROR = "error"
        const val EXTRA_ERROR_HINT = "error_hint"
        const val EXTRA_ERROR_DETAIL = "error_detail"
        private const val RECONNECT_ATTEMPTS = 7
        private const val RECONNECT_BUDGET_MS = 12_000L
        private const val HUD_HIDE_MS = 2800L
    }

    private lateinit var surface: SurfaceView
    private var videoSurface: Surface? = null
    private lateinit var status: TextView
    private lateinit var statusReason: TextView
    private lateinit var statusBar: PassThroughBar
    private lateinit var touchLayer: View
    private lateinit var reconnectLayer: View
    private var worker: Thread? = null
    @Volatile private var running = false
    @Volatile private var sessionGen = 0
    @Volatile private var awaitingManual = false
    @Volatile private var lit: LitSocket? = null
    @Volatile private var everVideo = false
    @Volatile private var lastError: String? = null
    @Volatile private var lastFail: UserFacingError? = null
    private val decoder = VideoDecoder()
    private var audio: AudioPlayer? = null
    private var streamW = 0
    private var streamH = 0
    private val hideHud = Runnable {
        if (!awaitingManual) {
            statusBar.visibility = View.GONE
        }
    }
    private val outbound = ArrayBlockingQueue<ByteArray>(128)
    @Volatile private var senderRunning = false
    private var sender: Thread? = null
    private val touch = TouchMapper { action, x, y ->
        if (awaitingManual) {
            if (action == TouchMapper.LEFT_UP) {
                runOnUiThread { requestManualReconnect() }
            }
            return@TouchMapper
        }
        val payload = LitProtocol.touchPayload(action, x, y)
        android.util.Log.i("LightingTouch", "queue action=" + action + " x=" + x + " y=" + y + " lit=" + (lit != null))
        if (!outbound.offer(payload)) {
            outbound.poll()
            outbound.offer(payload)
        }
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_display)
        hideSystemUi()
        surface = findViewById(R.id.surface)
        status = findViewById(R.id.status)
        statusReason = findViewById(R.id.statusReason)
        statusBar = findViewById(R.id.statusBar)
        touchLayer = findViewById(R.id.touchLayer)
        reconnectLayer = findViewById(R.id.reconnectLayer)
        status.isClickable = false
        status.isFocusable = false
        statusReason.isClickable = false
        statusReason.isFocusable = false
        statusBar.consumeTouches = false
        statusBar.isClickable = false
        statusBar.isFocusable = false
        reconnectLayer.isClickable = false
        reconnectLayer.isFocusable = false
        reconnectLayer.setOnClickListener(null)
        surface.isClickable = false
        surface.isFocusable = false
        // SurfaceView sits under the overlay; z-order media overlay keeps HUD/touch above.
        surface.setZOrderMediaOverlay(false)
        surface.holder.addCallback(this)
        touchLayer.bringToFront()
        reconnectLayer.bringToFront()
        statusBar.bringToFront()
        touch.attach(touchLayer, surface)
        if (surface.holder.surface.isValid) {
            bindVideoSurface(surface.holder.surface)
            startSession()
        }
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

    override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
        if (awaitingManual) return super.dispatchTouchEvent(ev)
        try {
            touch.onWindowTouch(ev)
        } catch (err: Throwable) {
            android.util.Log.w("LightingTouch", "dispatch failed", err)
        }
        return super.dispatchTouchEvent(ev)
    }

    override fun dispatchGenericMotionEvent(ev: MotionEvent): Boolean {
        if (!awaitingManual && (ev.source and android.view.InputDevice.SOURCE_MOUSE) != 0) {
            touch.onWindowTouch(ev)
            return true
        }
        return super.dispatchGenericMotionEvent(ev)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        bindVideoSurface(holder.surface)
        startSession()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (streamW > 0 && streamH > 0) {
            letterboxSurface(streamW, streamH)
        }
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        stopSession()
        // SurfaceHolder owns the surface — do not release it.
        videoSurface = null
    }

    private fun bindVideoSurface(surfaceObj: Surface?) {
        if (surfaceObj == null || !surfaceObj.isValid) return
        videoSurface = surfaceObj
    }

    override fun onDestroy() {
        statusBar.removeCallbacks(hideHud)
        senderRunning = false
        sender?.interrupt()
        sender = null
        outbound.clear()
        stopSession()
        super.onDestroy()
    }

    override fun finish() {
        val fail = lastFail
        if (!everVideo && fail != null) {
            setResult(
                Activity.RESULT_OK,
                Intent()
                    .putExtra(EXTRA_ERROR, fail.primary)
                    .putExtra(EXTRA_ERROR_HINT, fail.hint)
                    .putExtra(EXTRA_ERROR_DETAIL, fail.detail),
            )
        } else if (!everVideo && !lastError.isNullOrBlank()) {
            setResult(Activity.RESULT_OK, Intent().putExtra(EXTRA_ERROR, lastError))
        }
        super.finish()
    }

    private fun requestManualReconnect() {
        if (!awaitingManual) return
        startSession()
    }

    private fun rememberFail(error: Throwable, host: String): String {
        val mapped = ConnectCopy.fromThrowable(error, host)
        lastFail = mapped
        lastError = mapped.primary
        return mapped.primary
    }

    private fun startSender() {
        if (senderRunning) return
        senderRunning = true
        sender = thread(name = "lighting-touch") {
            while (senderRunning && !Thread.currentThread().isInterrupted) {
                val payload = try {
                    outbound.poll(200, TimeUnit.MILLISECONDS) ?: continue
                } catch (_: InterruptedException) {
                    break
                }
                val sock = lit
                if (sock == null) {
                    Thread.sleep(20)
                    outbound.offer(payload)
                    continue
                }
                try {
                    sock.write(LitProtocol.MSG_TOUCH, 0, payload)
                    android.util.Log.i("LightingTouch", "wrote bytes=" + payload.size)
                } catch (t: Exception) {
                    android.util.Log.w("LightingTouch", "send failed", t)
                }
            }
        }
    }

    private fun startSession() {
        stopSession()
        startSender()
        val gen = ++sessionGen
        running = true
        showManualReconnect(false)
        val host = intent.getStringExtra(EXTRA_HOST)?.ifBlank { null } ?: ConnectCopy.USB_HOST
        val port = intent.getIntExtra(EXTRA_PORT, LitProtocol.PORT)
        val metrics = DisplayMetrics()
        @Suppress("DEPRECATION")
        val display = windowManager.defaultDisplay
        display.getRealMetrics(metrics)
        val refresh = display.refreshRate.toInt().coerceIn(30, 120)
        val caps = DeviceCaps.probe()
        worker = thread(name = "lighting-session") {
            var fails = 0
            var windowStart = 0L
            while (running && sessionGen == gen) {
                setHud(
                    if (fails == 0) ConnectCopy.connectingLabel(host) else "重连中",
                    reason = null,
                    keep = true,
                )
                val reachedVideo = try {
                    runSessionOnce(host, port, metrics, refresh, caps, gen)
                } catch (t: Throwable) {
                    Log.e("Lighting", "session failed", t)
                    val primary = rememberFail(t, host)
                    if (running && sessionGen == gen) {
                        setHud("重连中", reason = primary, keep = true)
                    }
                    false
                } finally {
                    releaseStream()
                }
                if (!running || sessionGen != gen) break
                if (reachedVideo) {
                    fails = 0
                    windowStart = 0L
                    lastError = null
                    lastFail = null
                }
                fails++
                if (windowStart == 0L) {
                    windowStart = SystemClock.uptimeMillis()
                }
                val spent = SystemClock.uptimeMillis() - windowStart
                if (fails > RECONNECT_ATTEMPTS || spent >= RECONNECT_BUDGET_MS) {
                    setHud("已断开 · 点此重连", reason = lastError, keep = true)
                    showManualReconnect(true)
                    break
                }
                val jitter = (SystemClock.uptimeMillis() % 201).toInt()
                sleepBackoff(reconnectBackoffMs(fails - 1, jitter), gen)
            }
        }
    }

    /**
     * @return true if Config arrived and video started (used to reset retry streak).
     */
    private fun runSessionOnce(
        host: String,
        port: Int,
        metrics: DisplayMetrics,
        refresh: Int,
        caps: DeviceCaps,
        gen: Int,
    ): Boolean {
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
        ConnectHistory.remember(this, cfg.hostName, host, port)
        val hevc = cfg.codec.equals("hevc", true) || cfg.codec.equals("h265", true)
        if (cfg.audioEnabled) {
            audio = AudioPlayer(cfg.audioSampleRate, cfg.audioChannels)
        }
        setHud("${cfg.codec} ${cfg.width}×${cfg.height} 等待关键帧…", reason = null, keep = true)
        var configured = false
        var reachedVideo = false
        try {
            while (running && sessionGen == gen) {
                val msg = sock.read()
                when (msg.type) {
                    LitProtocol.MSG_VIDEO -> {
                        val (pts, data) = splitPts(msg.payload)
                        val isCfg = msg.flags and LitProtocol.FLAG_CODEC_CONFIG != 0
                        val key = msg.flags and LitProtocol.FLAG_KEYFRAME != 0
                        if (!configured || isCfg) {
                            val canInit = isCfg || isCodecConfigNal(data, hevc)
                            if (!canInit) continue
                            decoder.configure(
                                cfg.codec,
                                cfg.width,
                                cfg.height,
                                data,
                                (videoSurface ?: surface.holder.surface),
                            )
                            configured = true
                            reachedVideo = true
                            everVideo = true
                            lastError = null
                            lastFail = null
                            letterboxSurface(cfg.width, cfg.height)
                            setHud("${cfg.codec} ${cfg.width}×${cfg.height}@${cfg.fps}", reason = null, keep = false)
                            if (!isCfg) {
                                decoder.offer(data, codecConfig = false, keyframe = true, ptsUs = pts)
                            }
                            continue
                        }
                        decoder.offer(data, codecConfig = false, keyframe = key, ptsUs = pts)
                    }
                    LitProtocol.MSG_AUDIO -> {
                        val (pts, pcm) = splitPts(msg.payload)
                        audio?.offer(pcm, pts)
                    }
                    LitProtocol.MSG_HEARTBEAT -> sock.write(LitProtocol.MSG_HEARTBEAT)
                    LitProtocol.MSG_ERROR -> {
                        val remote = String(msg.payload, Charsets.UTF_8)
                        val primary = if (ConnectCopy.hasPortJargon(remote)) {
                            "连接中断，请检查数据线或重新点开始"
                        } else {
                            remote
                        }
                        lastError = primary
                        lastFail = UserFacingError(primary, "", remote)
                        setHud(primary, reason = null, keep = true)
                    }
                }
            }
        } catch (t: Throwable) {
            if (!running || sessionGen != gen) return reachedVideo
            if (reachedVideo) {
                Log.w("Lighting", "socket dropped after video", t)
                rememberFail(t, host)
                return true
            }
            throw t
        }
        return reachedVideo
    }

    private fun releaseStream() {
        decoder.release()
        audio?.release()
        audio = null
        try {
            lit?.close()
        } catch (_: Exception) {
        }
        lit = null
    }

    private fun stopSession() {
        touch.cancel()
        sessionGen++
        running = false
        awaitingManual = false
        try {
            lit?.close()
        } catch (_: Exception) {
        }
        worker?.interrupt()
        worker?.join(400)
        worker = null
        releaseStream()
        showManualReconnect(false)
    }

    private fun sleepBackoff(ms: Long, gen: Int) {
        val end = SystemClock.uptimeMillis() + ms
        while (running && sessionGen == gen && SystemClock.uptimeMillis() < end) {
            try {
                Thread.sleep(50)
            } catch (_: InterruptedException) {
                return
            }
        }
    }

    private fun showManualReconnect(enabled: Boolean) {
        awaitingManual = enabled
        runOnUiThread {
            if (isFinishing || isDestroyed) return@runOnUiThread
            statusBar.consumeTouches = enabled
            statusBar.isClickable = enabled
            statusBar.isFocusable = enabled
            status.isClickable = false
            status.isFocusable = false
            statusReason.isClickable = false
            statusReason.isFocusable = false
            reconnectLayer.visibility = if (enabled) View.VISIBLE else View.GONE
            reconnectLayer.isClickable = enabled
            reconnectLayer.isFocusable = enabled
            if (enabled) {
                statusBar.setOnClickListener { requestManualReconnect() }
                reconnectLayer.setOnClickListener { requestManualReconnect() }
            } else {
                statusBar.setOnClickListener(null)
                reconnectLayer.setOnClickListener(null)
                statusBar.isClickable = false
                statusBar.isFocusable = false
                reconnectLayer.isClickable = false
                reconnectLayer.isFocusable = false
            }
        }
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
            touchLayer.bringToFront()
            reconnectLayer.bringToFront()
            statusBar.bringToFront()
        }
    }

    private fun setHud(text: String, reason: String?, keep: Boolean) {
        runOnUiThread {
            if (isFinishing || isDestroyed) return@runOnUiThread
            status.text = text
            val detail = reason?.takeIf { it.isNotBlank() && !ConnectCopy.hasPortJargon(it) }
                ?: reason?.takeIf { it.isNotBlank() }?.let { "没检测到电脑，请检查数据线是否支持传数据" }
            if (detail == null) {
                statusReason.text = ""
                statusReason.visibility = View.GONE
            } else {
                statusReason.text = detail
                statusReason.visibility = View.VISIBLE
            }
            statusBar.visibility = View.VISIBLE
            statusBar.removeCallbacks(hideHud)
            if (!keep) {
                statusBar.postDelayed(hideHud, HUD_HIDE_MS)
            }
        }
    }
}

/** Top HUD that lets touches reach the video touch layer unless manual reconnect is armed. */
class PassThroughBar @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : LinearLayout(context, attrs) {
    var consumeTouches = false

    override fun dispatchTouchEvent(ev: MotionEvent): Boolean {
        if (!consumeTouches) return false
        return super.dispatchTouchEvent(ev)
    }
}
