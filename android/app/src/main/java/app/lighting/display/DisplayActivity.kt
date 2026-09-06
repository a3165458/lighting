package app.lighting.display

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.AttributeSet
import android.util.DisplayMetrics
import android.util.Log
import android.view.MotionEvent
import android.graphics.PixelFormat
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.WindowInsets
import android.view.WindowInsetsController
import android.view.WindowManager
import android.widget.LinearLayout
import android.widget.TextView
import java.nio.ByteBuffer
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
        private const val HUD_HIDE_MS = 400L
    }

    private lateinit var surface: SurfaceView
    private var videoSurface: Surface? = null
    private lateinit var status: TextView
    private lateinit var statusReason: TextView
    private lateinit var statusBar: PassThroughBar
    private lateinit var reconnectLayer: View
    private lateinit var cursorOverlay: CursorOverlayView
    private var cursorBitmap: Bitmap? = null
    private var cursorHotX = 0
    private var cursorHotY = 0
    @Volatile private var controlLit: LitSocket? = null
    private var controlReader: Thread? = null
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
    private var streamFps = 60
    private var panelFps = 60
    private val hideHud = Runnable {
        if (!awaitingManual) {
            statusBar.visibility = View.GONE
        }
    }
    private val outbound = ArrayBlockingQueue<ByteArray>(2)
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
        if (!outbound.offer(payload)) {
            outbound.poll()
            outbound.offer(payload)
        }
    }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        if (Build.VERSION.SDK_INT >= 30) {
            try {
                window.setPreferMinimalPostProcessing(true)
            } catch (_: Throwable) {
            }
        }
        // Lock peak Hz before the first vsync. After setContentView the
        // compositor has already picked 60 Hz on many pads.
        lockPeakRefresh()
        setContentView(R.layout.activity_display)
        hideSystemUi()
        surface = findViewById(R.id.surface)
        status = findViewById(R.id.status)
        statusReason = findViewById(R.id.statusReason)
        statusBar = findViewById(R.id.statusBar)
        reconnectLayer = findViewById(R.id.reconnectLayer)
        cursorOverlay = findViewById(R.id.cursorOverlay)
        cursorOverlay.isClickable = false
        cursorOverlay.isFocusable = false
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
        // Touch on the SurfaceView itself. A fullscreen transparent overlay
        // forces SurfaceFlinger to GPU-compose the video (one extra vsync
        // on many pads). Moonlight/GlideX keep the decoder surface uncovered.
        surface.isClickable = true
        surface.isFocusable = true
        surface.setZOrderMediaOverlay(false)
        // OPAQUE lets HWC punch the video overlay through. RGBX_8888 forced GPU composition.
        surface.holder.setFormat(PixelFormat.OPAQUE)
        surface.holder.addCallback(this)
        reconnectLayer.bringToFront()
        statusBar.bringToFront()
        touch.attach(surface, surface)
        if (surface.holder.surface.isValid) {
            applySurfaceFrameRate(surface.holder.surface)
            bindVideoSurface(surface.holder.surface)
            startSession()
        }
    }

    /**
     * GlideX / Moonlight: run the activity at the panel's peak Hz.
     * `preferredRefreshRate = display.refreshRate` is a no-op when the
     * system is already sitting at 60 Hz on a 90/120 Hz pad.
     */
    private fun lockPeakRefresh(): Int {
        @Suppress("DEPRECATION")
        val display = windowManager.defaultDisplay
        val peak = peakDisplayMode(display)
        val hz = (peak?.refreshRate ?: display.refreshRate).toInt().coerceIn(30, 120)
        panelFps = hz
        if (Build.VERSION.SDK_INT >= 23) {
            try {
                val lp = window.attributes
                if (peak != null) {
                    lp.preferredDisplayModeId = peak.modeId
                }
                val peakHz = peak?.refreshRate ?: hz.toFloat()
                lp.preferredRefreshRate = peakHz
                if (Build.VERSION.SDK_INT >= 31) {
                    try {
                        lp.preferredMinRefreshRate = peakHz
                        lp.preferredMaxRefreshRate = peakHz
                    } catch (_: Throwable) {
                    }
                }
                window.attributes = lp
            } catch (_: Throwable) {
            }
        }
        return hz
    }

    private fun peakDisplayMode(display: android.view.Display): android.view.Display.Mode? {
        if (Build.VERSION.SDK_INT < 23) return null
        return try {
            val cur = display.mode
            val modes = display.supportedModes
            val same = modes.filter {
                it.physicalWidth == cur.physicalWidth && it.physicalHeight == cur.physicalHeight
            }
            val pool = if (same.isNotEmpty()) same else modes.toList()
            val atLeast120 = pool.filter { it.refreshRate >= 119.5f }
            atLeast120.minByOrNull { it.refreshRate } ?: pool.maxByOrNull { it.refreshRate }
        } catch (_: Throwable) {
            null
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
        // Hint peak Hz before MediaCodec attaches. Waiting until the first
        // picture (letterboxSurface) lets SurfaceFlinger lock 60 Hz.
        applySurfaceFrameRate(holder.surface)
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
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_DISPLAY)
            while (senderRunning && !Thread.currentThread().isInterrupted) {
                val payload = try {
                    outbound.poll(200, TimeUnit.MILLISECONDS) ?: continue
                } catch (_: InterruptedException) {
                    break
                }
                val sock = controlLit ?: lit
                if (sock == null) {
                    Thread.sleep(20)
                    outbound.offer(payload)
                    continue
                }
                try {
                    sock.write(LitProtocol.MSG_TOUCH, 0, payload)
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
        windowManager.defaultDisplay.getRealMetrics(metrics)
        val refresh = lockPeakRefresh()
        val caps = DeviceCaps.probe()
        worker = thread(name = "lighting-session") {
            android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_DISPLAY)
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
        // Open the GlideX-style control socket immediately so HID is live
        // before the host finishes starting ffmpeg.
        thread(name = "lighting-control-open") {
            openControlPlane(host, port, gen)
        }
        val cfgMsg = sock.read()
        if (cfgMsg.type != LitProtocol.MSG_CONFIG) {
            throw IllegalStateException("expected config, got ${cfgMsg.type}")
        }
        val cfg = LitProtocol.parseConfig(cfgMsg.payload)
        streamFps = cfg.fps.coerceIn(24, 120)
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
                            // Size the SurfaceView buffers before the codec
                            // attaches. setFixedSize after start() makes
                            // SurfaceFlinger rebuild the queue (one extra
                            // glass frame; Moonlight does this first).
                            try {
                                surface.holder.setFixedSize(cfg.width, cfg.height)
                            } catch (_: Throwable) {
                            }
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
                    LitProtocol.MSG_CURSOR -> applyCursor(parseCursor(msg.payload))
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
        cursorOverlay.hidePointer()
        try {
            controlLit?.close()
        } catch (_: Exception) {
        }
        controlLit = null
        controlReader?.interrupt()
        controlReader = null
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

    private fun defaultCursorBitmap(): Bitmap {
        val w = 24
        val h = 24
        val bmp = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
        val px = IntArray(w * h)
        for (y in 0 until h) {
            for (x in 0 until w) {
                val on = x <= y && x + y <= 22 && x < 10
                val edge = on && (x == 0 || x == y || x + y == 22 || x == 9)
                px[y * w + x] = when {
                    edge -> 0xFF111111.toInt()
                    on -> 0xFFFFFFFF.toInt()
                    else -> 0
                }
            }
        }
        bmp.setPixels(px, 0, w, 0, 0, w, h)
        return bmp
    }

    private fun openControlPlane(host: String, port: Int, gen: Int) {
        try {
            val sock = LitSocket(host, port, 800, recvBytes = 16 * 1024, sendBytes = 16 * 1024)
            sock.write(LitProtocol.MSG_HELLO, 0, LitProtocol.controlHelloJson())
            controlLit = sock
            controlReader = thread(name = "lighting-cursor") {
                android.os.Process.setThreadPriority(android.os.Process.THREAD_PRIORITY_URGENT_DISPLAY)
                try {
                    while (running && sessionGen == gen) {
                        val msg = sock.read()
                        if (msg.type == LitProtocol.MSG_CURSOR) {
                            applyCursor(parseCursor(msg.payload))
                        }
                    }
                } catch (t: Throwable) {
                    if (running && sessionGen == gen) {
                        Log.w("Lighting", "control plane dropped", t)
                    }
                }
            }
        } catch (t: Throwable) {
            Log.w("Lighting", "control plane unavailable; cursor stays on video socket", t)
            controlLit = null
        }
    }

    private fun applyCursor(update: CursorUpdate?) {
        if (update == null) return
        if (!update.visible) {
            cursorOverlay.hidePointer()
            return
        }
        var incoming: Bitmap? = null
        var hotX = cursorHotX
        var hotY = cursorHotY
        val shape = update.bgra
        if (shape != null && update.width > 0 && update.height > 0) {
            incoming = Bitmap.createBitmap(update.width, update.height, Bitmap.Config.ARGB_8888)
            incoming.copyPixelsFromBuffer(ByteBuffer.wrap(shape))
            hotX = update.hotspotX
            hotY = update.hotspotY
            cursorBitmap = incoming
            cursorHotX = hotX
            cursorHotY = hotY
        } else if (cursorBitmap == null) {
            incoming = defaultCursorBitmap()
            hotX = 1
            hotY = 1
            cursorBitmap = incoming
            cursorHotX = hotX
            cursorHotY = hotY
        }
        cursorOverlay.update(
            visible = true,
            x = update.x,
            y = update.y,
            srcW = streamW.coerceAtLeast(1),
            srcH = streamH.coerceAtLeast(1),
            surfaceLeft = surface.left,
            surfaceTop = surface.top,
            surfaceW = surface.width.coerceAtLeast(1),
            surfaceH = surface.height.coerceAtLeast(1),
            bitmap = incoming,
            hotspotX = hotX,
            hotspotY = hotY,
        )
    }

    /**
     * Hint the panel refresh, but do not mark the surface as a fixed-rate
     * movie. FIXED_SOURCE made SurfaceFlinger wait a vsync; GlideX /
     * Moonlight present as soon as the buffer is released.
     */
    private fun applySurfaceFrameRate(surfaceObj: Surface) {
        if (Build.VERSION.SDK_INT < 30 || !surfaceObj.isValid) return
        val hz = panelFps.coerceAtLeast(streamFps).toFloat().coerceAtLeast(30f)
        try {
            if (Build.VERSION.SDK_INT >= 31) {
                surfaceObj.setFrameRate(
                    hz,
                    Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                    Surface.CHANGE_FRAME_RATE_ALWAYS,
                )
            } else {
                surfaceObj.setFrameRate(
                    hz,
                    Surface.FRAME_RATE_COMPATIBILITY_DEFAULT,
                )
            }
        } catch (_: Throwable) {
        }
    }

    private fun letterboxSurface(width: Int, height: Int) {
        streamW = width
        streamH = height
        runOnUiThread {
            // Keep MATCH_PARENT. Switching layoutParams from match_parent to
            // explicit pixels (or setFixedSize after MediaCodec.start) makes
            // SurfaceFlinger rebuild the buffer queue and can fire
            // surfaceDestroyed -> stopSession on the first picture.
            applySurfaceFrameRate(surface.holder.surface)
            // Overlay z-order is setZOrderMediaOverlay, set in the view
            // ctor. bringToFront() on that SurfaceView can rebuild it and
            // drop the first picture into surfaceDestroyed.
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
