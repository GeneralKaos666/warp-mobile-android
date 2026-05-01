package dev.warp.mobile

import android.content.Context
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
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.view.inputmethod.InputMethodManager
import android.widget.Toast
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.GlobalScope
import kotlinx.coroutines.launch
import java.io.File
import android.widget.FrameLayout
import androidx.appcompat.app.AppCompatActivity
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import androidx.core.view.ViewCompat
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat

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
    // renderClearFrame. Toggled by intent extras at launch (`--ez grid_mode
    // true`); a `START_STATIC_GRID` BroadcastReceiver was scoped originally
    // but never landed because the launch-extras path covered every M2/M3
    // driver use-case (M3-S11 housekeeping nit fix 2026-05-01).
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

    // M3-S04: when true, doFrame consumes the dirty bit on the Rust
    // TerminalModel and pushes a frame whenever PTY output changed. Falls
    // back to renderClearFrame when no dirty buffer (so vsync keeps ticking
    // for swapchain health). Toggled at launch via --ez terminal_mode true.
    @Volatile
    private var terminalMode = false

    // M2-S10: input focus target for IME attachment. SurfaceView cannot
    // receive `onCreateInputConnection`, so we overlay a 1x1 transparent
    // focusable View on top.
    private var warpInputView: WarpInputView? = null

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!renderActive) return
            // M3-S04 — Choreographer-driven push_frame.
            //
            // Mode precedence (most-specific first):
            //   1. terminalMode → consume the Rust TerminalModel dirty bit;
            //      if dirty, push a frame from the snapshot text. Otherwise
            //      drop through to a clear-frame so vsync keeps the
            //      swapchain healthy (per Choreographer.FrameCallback
            //      contract: <https://developer.android.com/reference/android/view/Choreographer.FrameCallback>).
            //   2. gridMode → static grid (M2-S08 baseline; unchanged).
            //   3. neither → clear-only (M2-S04 baseline).
            //
            // The terminalMode path falls back to gridMode if dirty=0 AND
            // a static grid was previously initialized, so the user always
            // sees text not just a magenta wash between PTY chunks.
            val ok = when {
                terminalMode -> {
                    val pushResult = NativeBridge.terminalTakeDirtyAndPushFrame(
                        gridFontSizePx, gridRows, gridCols, gridCellWPx, gridCellHPx
                    )
                    when (pushResult) {
                        // M3-S08: dirty bit set; the JNI re-initialized the
                        // dynamic_grid + presented one frame.
                        1 -> true
                        // No-dirty fallback: re-present the last dynamic_grid
                        // snapshot so the user keeps seeing the per-cell text
                        // instead of a clear-color frame between PTY chunks.
                        // Black clear matches the bg of the in-flight cells.
                        0 -> NativeBridge.renderDrawDynamicGridFrame(0.0f, 0.0f, 0.0f, 1.0f)
                        else -> false
                    }
                }
                gridMode -> NativeBridge.renderDrawGridFrame(1.0f, 0.0f, 1.0f, 1.0f)
                else -> NativeBridge.renderClearFrame(1.0f, 0.0f, 1.0f, 1.0f)
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
     * Called from `onCreate` when launched with `--ez grid_mode true`
     * (+ grid params). The grid init is idempotent on the Rust side, so
     * repeated calls are safe; a `START_STATIC_GRID` broadcast was scoped
     * but never implemented because the launch-extras driver path covered
     * every M2/M3 use-case (M3-S11 housekeeping nit fix 2026-05-01).
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

        // M4-S05: kick off bootstrap atomic extraction on a background thread.
        // First launch extracts ~50 MB to /data/data/dev.warp.mobile/files/usr;
        // subsequent launches short-circuit on sha-pin match (~instant).
        //
        // Codex round-1 finding 2: first launch must show install progress
        // (per `.omc/prd.json` M4-S05 AC#5). We probe the sha-pin file to
        // detect first-launch vs subsequent: pin file absent = first launch
        // = show indeterminate Toast; pin file present = no UI (sha-pin
        // fast path is ~10ms and shouldn't flash UI).
        val pinFile = File("${applicationInfo.dataDir}/files/.bootstrap-version.json")
        val isFirstLaunch = !pinFile.exists()
        if (isFirstLaunch) {
            Toast.makeText(this, "Installing Termux runtime…", Toast.LENGTH_LONG).show()
        }
        @OptIn(kotlinx.coroutines.DelicateCoroutinesApi::class)
        GlobalScope.launch(Dispatchers.IO) {
            val t0 = System.currentTimeMillis()
            val status = NativeBridge.bootstrapInstall(assets, applicationInfo.dataDir)
            val elapsed = System.currentTimeMillis() - t0
            Log.i(
                "warp.bootstrap",
                "M4-S05 bootstrapInstall: status=$status elapsedMs=$elapsed dataDir=${applicationInfo.dataDir}"
            )
            if (isFirstLaunch) {
                val msg = if (status == 0) {
                    "Termux runtime installed (${elapsed} ms)"
                } else {
                    "Termux runtime install failed: status=$status"
                }
                runOnUiThread {
                    Toast.makeText(this@MainActivity, msg, Toast.LENGTH_LONG).show()
                }
            }
        }

        // Keep the screen on while this Activity is in the foreground.
        // Same flag YouTube/Netflix/etc. use — survives Samsung One UI's
        // power-policy overrides that defeat `adb shell svc power stayon`
        // and `wm dismiss-keyguard`. Only effective while Activity is at
        // the top of the stack; system reclaims power management once the
        // user backgrounds us. Fixes M2-S05 round-2 manual unlock loop.
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        // M2-S12: edge-to-edge — app draws content under system bars.
        // We read insets via ViewCompat.setOnApplyWindowInsetsListener to
        // reserve the bottom region for the IME panel and top for the
        // status bar. Android 15+ enforces edge-to-edge for targetSdk 35+;
        // applying it explicitly here ensures consistent behavior across
        // API 31-36 (Plan Amendment 3 minSdk 31).
        // Ref: https://developer.android.com/develop/ui/views/layout/edge-to-edge
        WindowCompat.setDecorFitsSystemWindows(window, false)

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

        // M2-S10: composite layout = SurfaceView (rendering, full screen)
        // + WarpInputView overlay (1x1, transparent, focusable in touch mode)
        // for IME attachment. SurfaceView's surface is on a separate Z-layer
        // and the framework's IME-routing assumes the focused View also owns
        // the visible content, so we host an invisible focusable View on top.
        val frame = FrameLayout(this)
        val surfaceView = SurfaceView(this)
        surfaceView.holder.addCallback(this)
        frame.addView(
            surfaceView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        )
        warpInputView = WarpInputView(this).apply {
            // alpha=0: transparent so it doesn't obscure the SurfaceView render
            // output (alpha=0 on Android 5+ doesn't skip layout/measurement).
            // M2-S11: MATCH_PARENT so the View covers the full screen and
            // receives all touch events (WarpInputView.onTouchEvent). A 1x1
            // size would only capture taps in the top-left pixel; MATCH_PARENT
            // captures taps anywhere on screen and is still invisible.
            alpha = 0f
        }
        frame.addView(
            warpInputView,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT
            )
        )
        setContentView(frame)
        warpInputView!!.requestFocus()
        // M2-S10: publish the input view to companion object so the
        // ImeSimulationReceiver can route driver broadcasts through the
        // real WarpInputConnection.
        activeWarpInputView = warpInputView

        // M2-S12: WindowInsets listener. Listens on the root FrameLayout
        // so we receive *every* inset change — IME up/down, system bars
        // show/hide (including the fullscreen-toggle below), and rotation.
        //
        // Why root layout vs SurfaceView/WarpInputView: ViewCompat.setOn-
        // ApplyWindowInsetsListener on SurfaceView is unreliable — the
        // SurfaceView surface is on a separate Z-layer and the framework
        // may not propagate window insets to it. The root FrameLayout is a
        // normal View and always receives inset dispatches first per
        // WindowInsets traversal rules (parent → child).
        //
        // IME bottom vs system-bars bottom: we pass max(ime.bottom,
        // sysBars.bottom) so the Rust side always gets the effective bottom
        // reservation. In fullscreen mode sysBars.bottom=0 (nav bar hidden)
        // and ime.bottom reflects only the keyboard height; in non-fullscreen
        // normal mode sysBars.bottom is the nav-bar height and ime.bottom is
        // 0 when the keyboard is hidden — both collapse to the right value.
        //
        // Refs (M2-S12, 2026-04-30; M3-S11 nit fix 2026-05-01 — stale
        // /develop/ui/views/layout/insets/handle-ime-keyboard-visibility URL
        // replaced with the canonical /touch-and-input/keyboard-input/visibility
        // landing page that the Android team currently maintains; the old path
        // 302-redirects but produces a "page not found" inline doc on Android
        // Studio's hover preview):
        //   https://developer.android.com/reference/androidx/core/view/WindowInsetsCompat
        //   https://developer.android.com/develop/ui/views/layout/edge-to-edge
        //   https://developer.android.com/reference/androidx/core/view/ViewCompat#setOnApplyWindowInsetsListener(android.view.View,androidx.core.view.OnApplyWindowInsetsListener)
        //   https://developer.android.com/develop/ui/views/touch-and-input/keyboard-input/visibility
        ViewCompat.setOnApplyWindowInsetsListener(frame) { _, insets ->
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val sysBars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            // bottom = max of IME height and nav-bar height (whichever is larger).
            val effectiveBottom = maxOf(ime.bottom, sysBars.bottom)
            Log.i(
                TAG,
                "window_insets ime.bottom=${ime.bottom} " +
                    "sysBars={top=${sysBars.top} l=${sysBars.left} r=${sysBars.right} b=${sysBars.bottom}} " +
                    "effectiveBottom=$effectiveBottom"
            )
            NativeBridge.setRenderInsets(sysBars.top, sysBars.left, sysBars.right, effectiveBottom)
            insets
        }

        // M2-S12: fullscreen mode — hide nav bar + status bar.
        // Triggered by --ez fullscreen true on launch intent. In fullscreen
        // mode BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE brings them back
        // temporarily on an inward swipe from the edge, then auto-hides.
        //
        // Refs (M2-S12, 2026-04-30):
        //   https://developer.android.com/reference/androidx/core/view/WindowInsetsControllerCompat
        //   https://developer.android.com/develop/ui/views/layout/immersive
        if (intent.getBooleanExtra("fullscreen", false)) {
            val controller = WindowInsetsControllerCompat(window, frame)
            controller.hide(WindowInsetsCompat.Type.systemBars())
            controller.systemBarsBehavior =
                WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            Log.i(TAG, "fullscreen mode applied: systemBars hidden, transient-swipe behavior set")
        }

        Log.i(TAG, "MainActivity ready ping=${NativeBridge.ping()} input_focus=${warpInputView!!.isFocused}")

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

        // M3-S04: optional terminal_mode at launch. Driver uses
        //   am start -n dev.warp.mobile/.MainActivity \
        //     --ez terminal_mode true \
        //     --ef grid_font_size_px 32.0 \
        //     --ei grid_rows 24 --ei grid_cols 80 \
        //     --ef grid_cell_w_px 24.0 --ef grid_cell_h_px 40.0
        //
        // This activates the M3-S04 push_frame path: PTY output → Rust
        // TerminalModel → Vulkan static grid re-init per dirty vsync.
        // Grid params overlap with grid_mode parsing above; if both are
        // set, terminal_mode wins at frame-callback time but both grid
        // params and the Rust resize are applied here.
        if (intent.getBooleanExtra("terminal_mode", false)) {
            terminalMode = true
            // Default grid params optimized for ~24×80 readable text on a
            // flagship 1080×2400 portrait surface (S24 Ultra ~ 3120×1440 in
            // landscape). M3-S08 will tune these via runtime ANativeWindow
            // dims; for M3-S04 we let the launch intent override.
            gridFontSizePx = intent.getFloatExtra("grid_font_size_px", 32.0f)
            gridRows = intent.getIntExtra("grid_rows", 24)
            gridCols = intent.getIntExtra("grid_cols", 80)
            gridCellWPx = intent.getFloatExtra("grid_cell_w_px", 24.0f)
            gridCellHPx = intent.getFloatExtra("grid_cell_h_px", 40.0f)
            // M3-S09: report cell height to the input view so its onScroll /
            // onFling pixel→rows conversion uses the right divisor. Reset
            // any prior scroll state too (rotation / Activity recreate).
            warpInputView?.setCellHeightPx(gridCellHPx)
            warpInputView?.resetScroll()
            // Reshape the Rust model so PTY chunks land in the right grid
            // size from the start (the model's dirty bit will trigger the
            // first push_frame on the next dirty vsync).
            NativeBridge.terminalResize(gridRows, gridCols)
            Log.i(
                TAG,
                "terminal_mode requested rows=$gridRows cols=$gridCols " +
                    "font_size_px=$gridFontSizePx cell=${gridCellWPx}x${gridCellHPx}px"
            )

            // Optional auto-spawn: --es terminal_cmd "/system/bin/sh"
            // with --es terminal_initial_input "echo hello\n". Useful for
            // M3-S04 device verification without needing a separate adb
            // broadcast for PTY_SPAWN. The driver may set both extras to
            // get a one-shot end-to-end smoke test.
            val terminalCmd = intent.getStringExtra("terminal_cmd")
            if (!terminalCmd.isNullOrBlank()) {
                val cmdId = intent.getStringExtra("terminal_cmd_id") ?: "terminal_mode"
                val initialInput = intent.getStringExtra("terminal_initial_input")
                val spawnIntent = Intent(WarpTerminalService.ACTION_SPAWN).apply {
                    setPackage(packageName)
                    putExtra("cmd_id", cmdId)
                    putExtra("program", terminalCmd)
                }
                sendBroadcast(spawnIntent)
                Log.i(TAG, "terminal_mode auto-spawn cmd_id=$cmdId program=$terminalCmd")
                if (!initialInput.isNullOrEmpty()) {
                    // Allow a brief delay so the PTY child has time to start
                    // before we feed it stdin. 200ms is conservative for
                    // sh/bash startup on flagship.
                    warpInputView?.postDelayed({
                        val writeIntent = Intent(WarpTerminalService.ACTION_WRITE).apply {
                            setPackage(packageName)
                            putExtra("cmd_id", cmdId)
                            putExtra("data", initialInput)
                        }
                        sendBroadcast(writeIntent)
                        Log.i(TAG, "terminal_mode auto-input cmd_id=$cmdId bytes=${initialInput.length}")
                    }, 200)
                }
            }
        }

        // M2-S10: optional auto-show IME on launch. Driver uses
        //   am start --ez ime_mode true
        // to request the soft keyboard popup so logcat captures
        // setComposingText/commitText events end-to-end.
        //
        // M3-S11 nit fix (2026-05-01): switched the primary path to
        // `WindowInsetsControllerCompat.show(Type.ime())` per Android 11+
        // (API 30) guidance — this is the future-proof, system-aware way
        // to surface the IME (also tracks Type.ime() insets correctly so
        // the listener registered above forwards `ime.bottom` to the Rust
        // renderer without a re-layout race). The legacy
        // `InputMethodManager.showSoftInput` call is kept as a fallback
        // for stricter OEMs (Samsung Knox blocks the controller path on
        // some debug builds — observed in M2-S12 sub-test 1) and for log
        // parity (driver still greps `showSoftInput shown=…`).
        //
        // Refs:
        //   https://developer.android.com/reference/androidx/core/view/WindowInsetsControllerCompat#show(int)
        //   https://developer.android.com/develop/ui/views/touch-and-input/keyboard-input/visibility
        if (intent.getBooleanExtra("ime_mode", false)) {
            // Wait for the View to be attached to the window before showing
            // the soft keyboard (otherwise InputMethodManager.showSoftInput
            // returns false silently). post() runs after layout pass.
            warpInputView?.post {
                warpInputView?.requestFocus()
                // Primary (API 30+ canonical): WindowInsetsControllerCompat.
                val controllerShown = try {
                    val controller = WindowInsetsControllerCompat(window, warpInputView!!)
                    controller.show(WindowInsetsCompat.Type.ime())
                    true
                } catch (t: Throwable) {
                    Log.w(TAG, "ime_mode: WindowInsetsControllerCompat.show(ime()) threw: ${t.message}")
                    false
                }
                // Fallback (legacy + Knox quirk): InputMethodManager.showSoftInput.
                val imm =
                    getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager
                // SHOW_IMPLICIT is preferred over deprecated SHOW_FORCED on
                // API 30+; we always have a focused View at this point.
                val shown = imm?.showSoftInput(warpInputView, InputMethodManager.SHOW_IMPLICIT) ?: false
                Log.i(
                    TAG,
                    "ime_mode requested: controllerShown=$controllerShown showSoftInput shown=$shown focus=${warpInputView?.isFocused} ime_visible_post_call=${imm?.isAcceptingText}"
                )
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        renderActive = false
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        // M2-S10: clear input-view companion-object pointer; ImeSimulation-
        // Receiver will fall back to direct JNI if a stray broadcast arrives
        // post-destroy.
        if (activeWarpInputView === warpInputView) {
            activeWarpInputView = null
        }
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
            // M3-S08: terminal_mode now uses the per-cell dynamic_grid
            // pipeline driven by `terminalTakeDirtyAndPushFrame`. We do NOT
            // pre-init a static_grid here — the very first PTY chunk's
            // dirty-vsync triggers `init_dynamic_grid` from the real
            // TerminalModel cell snapshot. Until that lands, the
            // Choreographer fallback path renders a black clear (matching
            // the eventual cell bg).
            if (terminalMode) {
                Log.i(
                    TAG,
                    "terminal_mode ready (post-surfaceCreated) " +
                        "rows=$gridRows cols=$gridCols " +
                        "cell=${gridCellWPx}x${gridCellHPx}px font_size_px=$gridFontSizePx"
                )
            }
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

        /**
         * M2-S10: the currently-foregrounded MainActivity's WarpInputView.
         * Set in `onCreate` (after the View is built) and cleared in
         * `onDestroy`. Read by [ImeSimulationReceiver] to forward driver
         * IME-event broadcasts through the real `WarpInputConnection` code
         * path. Volatile because reader and writer are on different threads
         * (broadcast receiver vs UI thread on cold start).
         */
        @Volatile
        var activeWarpInputView: WarpInputView? = null
            private set
    }
}
