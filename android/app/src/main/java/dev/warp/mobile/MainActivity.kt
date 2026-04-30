package dev.warp.mobile

import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.SystemClock
import android.util.Log
import android.view.Choreographer
import android.view.Surface
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.WindowManager
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat

/**
 * MainActivity hosts a SurfaceView that backs the Vulkan swapchain (M2-S04).
 *
 * Lifecycle:
 *   - onCreate: starts the FGS (PTY backend, M1 — unchanged), creates a
 *     SurfaceView and registers as its SurfaceHolder.Callback.
 *   - surfaceCreated: passes the Surface to NativeBridge.renderAttachSurface
 *     which drives ANativeWindow_fromSurface + Vulkan init.
 *   - surfaceChanged: re-attaches with the new dimensions (Vulkan recreates
 *     the swapchain internally on next acquire if needed).
 *   - surfaceDestroyed: tears down Vulkan via renderDetachSurface.
 *
 * Render loop: Choreographer.postFrameCallback drives renderClearFrame at
 * vsync (60Hz on most flagships, 120Hz on S24 Ultra). The frame counter is
 * exported via renderFramesPresented for the test driver.
 *
 * Tag for logcat scraping: "WarpRender" (Kotlin) + "WarpVulkan" (Rust).
 */
class MainActivity : AppCompatActivity(), SurfaceHolder.Callback {

    private var renderActive = false
    // M2-S09: track currently-attached surface dimensions. surfaceChanged is
    // *always* called by Android once after surfaceCreated with the initial
    // dimensions, then again only on actual size/format changes. If we
    // re-attach blindly on every surfaceChanged we double-init the Vulkan
    // pipeline (attach + init_static_grid run twice → ~200ms wasted per
    // rotation). Skip the second redundant call by tracking last-attached
    // dims and only re-attaching when they change.
    private var attachedWidth = -1
    private var attachedHeight = -1
    // M2-S08: when true, doFrame calls renderDrawGridFrame instead of
    // renderClearFrame. Toggled by the START_STATIC_GRID broadcast (driver
    // path) or by intent extras at launch.
    @Volatile
    private var gridMode = false
    @Volatile
    private var gridText: String = "Hello, World"
    @Volatile
    private var gridFontSizePx: Float = 32.0f
    @Volatile
    private var gridRows: Int = 20
    @Volatile
    private var gridCols: Int = 50
    @Volatile
    private var gridCellWPx: Float = 200.0f
    @Volatile
    private var gridCellHPx: Float = 60.0f

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!renderActive) return
            // Magenta clear; if grid mode is active, the Rust side draws on
            // top of the clear within the same render pass. Otherwise this is
            // M2-S04's clear-only path.
            val ok = if (gridMode) {
                NativeBridge.renderDrawGridFrame(1.0f, 0.0f, 1.0f, 1.0f)
            } else {
                NativeBridge.renderClearFrame(1.0f, 0.0f, 1.0f, 1.0f)
            }
            if (!ok) {
                // VK_ERROR_OUT_OF_DATE_KHR or transient: skip + retry next vsync.
                Log.d(TAG, "render frame returned false @ ${SystemClock.uptimeMillis()}")
            }
            // Schedule next frame (Choreographer is one-shot).
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    /**
     * M2-S08: initialize + start the static-grid render path.
     *
     * Called either from `onCreate` (when launched with `--ez grid_mode true`
     * + grid params) or from the START_STATIC_GRID broadcast. The grid init
     * is idempotent on the Rust side, so multiple calls are safe.
     *
     * Logs `static_grid_started rows=… cols=… text=…` for the driver to grep.
     */
    @Synchronized
    fun startStaticGrid(
        text: String,
        fontSizePx: Float,
        rows: Int,
        cols: Int,
        cellWPx: Float,
        cellHPx: Float
    ) {
        gridText = text
        gridFontSizePx = fontSizePx
        gridRows = rows
        gridCols = cols
        gridCellWPx = cellWPx
        gridCellHPx = cellHPx
        if (!renderActive) {
            Log.w(TAG, "startStaticGrid: renderActive=false — surface not yet attached; will retry on surfaceCreated")
            gridMode = true
            return
        }
        val initOk = NativeBridge.renderInitStaticGrid(
            text, fontSizePx, rows, cols, cellWPx, cellHPx
        )
        Log.i(
            TAG,
            "renderInitStaticGrid ok=$initOk text=\"$text\" rows=$rows cols=$cols " +
                "cell=${cellWPx}x${cellHPx}px font_size_px=$fontSizePx"
        )
        if (initOk) {
            gridMode = true
            val stats = NativeBridge.renderStaticGridStats()
            Log.i(TAG, "static_grid_started $stats")
        }
    }

    // M2-S05: the CAPTURE_FRAME broadcast is handled by the manifest-registered
    // [CaptureFrameReceiver] — runtime-registered receivers don't reliably
    // match `am broadcast` from `shell` UID on Android 14+. The receiver
    // calls into [NativeBridge.renderCaptureFrame] directly, which serializes
    // against the Choreographer per-vsync `renderClearFrame` calls via the
    // swapchain mutex inside the Rust crate.

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Keep the screen on while this Activity is in the foreground.
        // Same flag YouTube/Netflix/etc. use — survives Samsung One UI's
        // power-policy overrides that defeat `adb shell svc power stayon`
        // and `wm dismiss-keyguard`. Only effective while Activity is at
        // the top of the stack; system reclaims power management once the
        // user backgrounds us. Fixes M2-S05 round-2 manual unlock loop.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        // POST_NOTIFICATIONS for FGS (M1 carry-over, unchanged).
        if (Build.VERSION.SDK_INT >= 33 &&
            ContextCompat.checkSelfPermission(this, android.Manifest.permission.POST_NOTIFICATIONS)
                != PackageManager.PERMISSION_GRANTED
        ) {
            ActivityCompat.requestPermissions(
                this,
                arrayOf(android.Manifest.permission.POST_NOTIFICATIONS),
                1001
            )
        }

        // Start the FGS so PTY backend is reachable for M1+M2 integration.
        startForegroundService(Intent(this, WarpTerminalService::class.java))

        // SurfaceView for Vulkan rendering (M2-S04).
        val surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)
        setContentView(surfaceView)

        Log.i(TAG, "MainActivity ready ping=${NativeBridge.ping()}")

        // M2-S08: optional grid mode via launch intent extras. Driver uses
        //   am start -n dev.warp.mobile/.MainActivity \
        //     --ez grid_mode true \
        //     --es grid_text "Hello, World" \
        //     --ef grid_font_size_px 32.0 \
        //     --ei grid_rows 20 --ei grid_cols 50 \
        //     --ef grid_cell_w_px 200.0 --ef grid_cell_h_px 60.0
        if (intent.getBooleanExtra("grid_mode", false)) {
            // Text precedence (CJK / space-resilient, mirrors M2-S07
            // CaptureFrameReceiver):
            //   1. `grid_text_b64` extra (base64-encoded UTF-8) — driver-friendly,
            //      avoids `am start --es` losing whitespace/multi-byte chars
            //      when relayed through adb shell.
            //   2. `grid_text` extra (plain string) — works for ASCII tests
            //      without spaces.
            //   3. Default "Hello, World".
            val textB64 = intent.getStringExtra("grid_text_b64")
            val textExtra = intent.getStringExtra("grid_text")
            val text = when {
                textB64 != null -> {
                    try {
                        String(android.util.Base64.decode(textB64, android.util.Base64.DEFAULT), Charsets.UTF_8)
                    } catch (e: Exception) {
                        Log.w(TAG, "grid_text_b64 decode failed (${e.message}); using default")
                        "Hello, World"
                    }
                }
                !textExtra.isNullOrBlank() -> textExtra
                else -> "Hello, World"
            }
            val fontSize = intent.getFloatExtra("grid_font_size_px", 32.0f)
            val rows = intent.getIntExtra("grid_rows", 20)
            val cols = intent.getIntExtra("grid_cols", 50)
            val cellW = intent.getFloatExtra("grid_cell_w_px", 200.0f)
            val cellH = intent.getFloatExtra("grid_cell_h_px", 60.0f)
            Log.i(TAG, "grid_mode requested at launch text=\"$text\" rows=$rows cols=$cols")
            // Mark gridMode so surfaceCreated will init once the surface arrives.
            gridText = text
            gridFontSizePx = fontSize
            gridRows = rows
            gridCols = cols
            gridCellWPx = cellW
            gridCellHPx = cellH
            gridMode = true
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        renderActive = false
        Choreographer.getInstance().removeFrameCallback(frameCallback)
    }

    // ── SurfaceHolder.Callback ───────────────────────────────────────────────

    override fun surfaceCreated(holder: SurfaceHolder) {
        val ts = SystemClock.uptimeMillis()
        Log.i(TAG, "surfaceCreated_ts=$ts")
        // Attach uses the Surface's current dimensions (read inside Rust via
        // ANativeWindow_getWidth/getHeight). We mark attachedWidth=-1 so the
        // first surfaceChanged is treated as a real change and updates our
        // local cache, but we skip the redundant re-attach since this
        // surfaceCreated call already did one.
        attachAndStartRender(holder.surface)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val ts = SystemClock.uptimeMillis()
        Log.i(TAG, "surfaceChanged_ts=$ts width=$width height=$height")
        // M2-S09: skip the redundant double-init. Android calls surfaceChanged
        // exactly once after surfaceCreated with the initial dims (always),
        // then again only on real size/format changes. Re-attaching here on
        // the first call duplicates what surfaceCreated→attachAndStartRender
        // just did and wastes ~80ms on grid_init.
        //
        // Strategy: if renderActive is true and we haven't recorded dims yet
        // (attachedWidth=-1 — fresh attach from surfaceCreated), this is the
        // spurious follow-up surfaceChanged. Just record dims and bail.
        // If dims match the recorded ones, also bail (idempotent).
        // Only re-attach when dims actually changed.
        if (renderActive && attachedWidth == -1 && attachedHeight == -1) {
            // First surfaceChanged after surfaceCreated already attached.
            attachedWidth = width
            attachedHeight = height
            return
        }
        if (renderActive && attachedWidth == width && attachedHeight == height) {
            // No-op: same dims, already attached.
            return
        }
        // Real change: re-attach with fresh dims.
        renderActive = false
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        attachAndStartRender(holder.surface, width, height)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        val ts = SystemClock.uptimeMillis()
        Log.i(TAG, "surfaceDestroyed_ts=$ts")
        renderActive = false
        attachedWidth = -1
        attachedHeight = -1
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        NativeBridge.renderDetachSurface()
    }

    // ── Internals ────────────────────────────────────────────────────────────

    private fun attachAndStartRender(surface: Surface, width: Int = -1, height: Int = -1) {
        val ok = NativeBridge.renderAttachSurface(surface)
        Log.i(TAG, "renderAttachSurface ok=$ok")
        if (ok) {
            // M2-S09: cache dims so the followup surfaceChanged with the same
            // dims becomes a no-op. width=-1/height=-1 means caller didn't
            // know the dims (surfaceCreated path); the followup surfaceChanged
            // will record the real dims on its first run.
            attachedWidth = width
            attachedHeight = height
            renderActive = true
            // M2-S08: if grid mode was requested before surface was ready, do
            // the init now while we have a valid swapchain. The Rust side
            // builds the atlas + pipeline against the current render_pass.
            if (gridMode) {
                val initOk = NativeBridge.renderInitStaticGrid(
                    gridText, gridFontSizePx, gridRows, gridCols, gridCellWPx, gridCellHPx
                )
                Log.i(
                    TAG,
                    "renderInitStaticGrid (post-surfaceCreated) ok=$initOk " +
                        "text=\"$gridText\" rows=$gridRows cols=$gridCols " +
                        "cell=${gridCellWPx}x${gridCellHPx}px font_size_px=$gridFontSizePx"
                )
                if (initOk) {
                    val stats = NativeBridge.renderStaticGridStats()
                    Log.i(TAG, "static_grid_started $stats")
                } else {
                    // Disable grid mode; doFrame will fall back to clear so
                    // we still drive the loop and the driver can detect the
                    // failure via missing static_grid_started line.
                    gridMode = false
                }
            }
            Choreographer.getInstance().postFrameCallback(frameCallback)
        }
    }

    companion object {
        private const val TAG = "WarpRender"
    }
}
