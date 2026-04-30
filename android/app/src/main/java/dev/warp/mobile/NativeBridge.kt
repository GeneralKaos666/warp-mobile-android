package dev.warp.mobile

import android.view.Surface

object NativeBridge {
    init {
        System.loadLibrary("warp_mobile_android_host")
    }

    external fun ping(): String

    // ── PTY (M1) ─────────────────────────────────────────────────────────────

    external fun ptySpawn(program: String, args: Array<String>, envFlat: Array<String>): Long
    external fun ptyAcquire(ptr: Long): Long
    external fun ptyRelease(ptr: Long)
    external fun ptyRead(ptr: Long, maxBytes: Int): ByteArray?
    external fun ptyWrite(ptr: Long, data: ByteArray): Int
    external fun ptyResize(ptr: Long, rows: Short, cols: Short): Int
    external fun ptyKill(ptr: Long)

    // ── Vulkan render (M2-S04) ───────────────────────────────────────────────
    //
    // Drives the AndroidSwapchain in `crates/android-host/src/vulkan.rs` (which
    // mirrors warp-src/crates/warpui/src/platform/android/vulkan.rs per
    // M2-S04 AC#1). Surface lifecycle is tied to SurfaceHolder.Callback events
    // in MainActivity; render frames are pushed from a Choreographer callback.

    /**
     * Initializes Vulkan on the given Android Surface. Wraps
     * `ANativeWindow_fromSurface` + the M0-spike-validated swapchain creation
     * path. Returns true on success.
     */
    external fun renderAttachSurface(surface: Surface): Boolean

    /** Tears down the swapchain. Idempotent. */
    external fun renderDetachSurface()

    /**
     * Submits a single clear-color frame. RGBA components are floats in
     * [0.0, 1.0]. Returns true on successful vkQueuePresentKHR; false on
     * VK_ERROR_OUT_OF_DATE_KHR (caller may retry next vsync after the
     * swapchain has been recreated internally).
     */
    external fun renderClearFrame(r: Float, g: Float, b: Float, a: Float): Boolean

    /** Cumulative frame count since the last attach. */
    external fun renderFramesPresented(): Long

    /**
     * M2-S05: capture a single frame as PNG at `path`.
     *
     * Renders one clear-color frame (RGBA in [0.0, 1.0]), copies the swapchain
     * image to a host-coherent staging buffer via `vkCmdCopyImageToBuffer`,
     * swizzles BGRA→RGBA if needed, encodes a PNG, and writes it to disk.
     *
     * Returns `true` on success. The Rust side logs `capture_ok frame=<n>
     * ts=<ms> dims=<W>x<H> bytes=<n> mean_rgb=<r>,<g>,<b>` which the device
     * driver greps for.
     *
     * Synchronous — blocks until `vkQueueWaitIdle` completes.
     */
    external fun renderCaptureFrame(
        path: String,
        r: Float,
        g: Float,
        b: Float,
        a: Float
    ): Boolean

    /**
     * M2-S07: capture a single frame as PNG at `path`, with shaped text
     * composited on top.
     *
     * Renders one clear-color frame (RGBA in [0.0, 1.0]), reads it back via
     * the M2-S05 readback pipeline, then:
     *  - Discovers system fonts via `ASystemFontIterator` (NDK API 29+)
     *    or `/system/fonts/` directory scan as fallback.
     *  - Loads them into a `cosmic_text::FontSystem`.
     *  - Shapes `text` (e.g. `"Hello, 世界"`) — Latin from Roboto/Sans-Serif,
     *    CJK fallback from Noto Sans CJK.
     *  - Rasterizes each glyph via swash and alpha-blends the resulting
     *    bitmap onto the captured RGBA buffer at `(baselineX, baselineY)`
     *    in white.
     *  - Encodes the modified buffer as PNG.
     *
     * Returns `true` on success. The Rust side logs `capture_ok` (M2-S05
     * schema) AND `font_render_ok via=… fonts_loaded=… glyphs_total=…
     * composed_pixels=…` which the device driver greps for. The driver
     * additionally checks the resulting PNG for non-magenta pixels in the
     * expected glyph band.
     *
     * Synchronous — blocks until the PNG is fully flushed to disk.
     */
    external fun renderCaptureFrameWithText(
        path: String,
        r: Float,
        g: Float,
        b: Float,
        a: Float,
        text: String,
        fontSizePx: Float,
        baselineX: Float,
        baselineY: Float
    ): Boolean

    // ── Static glyph grid (M2-S08) ──────────────────────────────────────────
    //
    // Drives the GPU pipeline in `crates/android-host/src/static_grid.rs`
    // which mirrors `warp-src/crates/warpui/src/platform/android/static_grid.rs`.
    // The pipeline pre-rasterizes glyphs into a 1024×1024 R8 atlas, builds a
    // per-instance vertex buffer (one entry per glyph in the grid), and
    // draws all `rows × cols × glyphs_per_string` instances in a single
    // `vkCmdDraw` call per frame. Targets 60fps p95<16.6ms on Galaxy S24
    // Ultra (Adreno 750) for the M2a Acceptance #1 gate.

    /**
     * Initializes the static glyph grid. Pre-rasterizes glyphs into the GPU
     * atlas + builds the per-instance vertex buffer + creates the pipeline.
     * Idempotent — calling again replaces any prior grid.
     *
     * Synchronous; call from a non-rendering thread or before starting the
     * Choreographer loop.
     *
     * Returns `true` on success; logs `static_grid_init_ok dt_ms=… text=…
     * rows=… cols=… atlas_glyphs=… instances=…` which the test driver greps.
     */
    external fun renderInitStaticGrid(
        text: String,
        fontSizePx: Float,
        rows: Int,
        cols: Int,
        cellWPx: Float,
        cellHPx: Float
    ): Boolean

    /**
     * Submits one grid frame: clear → draw all instances → present. Returns
     * `true` on successful `vkQueuePresentKHR`. If no grid is initialized,
     * falls back to a clear-color frame.
     *
     * The Rust side logs `present_ok frame=N ts=M` per successful present,
     * the same schema as `renderClearFrame`, so the existing
     * `tools/scripts/test-render-scene.sh` parser is reusable.
     */
    external fun renderDrawGridFrame(r: Float, g: Float, b: Float, a: Float): Boolean

    /** True iff a static grid has been successfully attached. */
    external fun renderStaticGridAttached(): Boolean

    /**
     * Returns "atlas_glyphs=N,glyphs_per_frame=N,rows=N,cols=N,text=…" if a
     * grid is attached, empty string otherwise. Used by the M2-S08 driver to
     * round-trip diagnostic info into the result JSON.
     */
    external fun renderStaticGridStats(): String

    // ── IME composing-text state machine (M2-S10) ───────────────────────────
    //
    // Drives the state machine in `crates/android-host/src/ime.rs` (which
    // mirrors `warp-src/crates/warpui/src/platform/android/ime.rs`). Called
    // from `WarpInputView.WarpInputConnection` overrides on the View's UI
    // thread.
    //
    // All four entry points are thread-safe (Mutex inside the Rust singleton).
    // Logcat tag: `WarpIme` (Rust target). The M2-S10 driver greps these.

    /**
     * `InputConnection.commitText(text, newCursorPosition)` — finalize text
     * into the buffer. If a composing region is active, the region is replaced
     * atomically (Pinyin candidate-pick path); otherwise the text is committed
     * as a Latin keystroke.
     */
    external fun imeCommitText(text: String, newCursorPosition: Int)

    /**
     * `InputConnection.setComposingText(text, newCursorPosition)` — update
     * the in-progress composing region. Empty text clears the region.
     */
    external fun imeSetComposingText(text: String, newCursorPosition: Int)

    /**
     * `InputConnection.finishComposingText()` — clear composing region. If
     * the region is non-empty, emits a `composing_finish` event. If empty
     * (Gboard known issue: spurious calls between setComposingText and
     * commitText), emits an `empty_finish` marker without double-committing.
     */
    external fun imeFinishComposingText()

    /**
     * Returns IME state machine counters as a comma-separated string:
     * "commit_calls=N,set_composing_calls=N,finish_calls=N,events=N,
     *  latin=N,composing_update=N,composing_commit=N,composing_finish=N,
     *  empty_finish=N,is_composing=B,composing_text=S"
     */
    external fun imeStats(): String

    /** Reset IME state (clear counters + composing region). Driver uses
     *  this between sub-tests. */
    external fun imeReset()

    // ── Touch input + gesture mapping (M2-S11) ──────────────────────────────
    //
    // Drives the state machine in `crates/android-host/src/input.rs` (which
    // mirrors `warp-src/crates/warpui/src/platform/android/input.rs`). Called
    // from `WarpInputView.onTouchEvent` + `GestureDetector.SimpleOnGestureListener`
    // on the View's UI thread.
    //
    // Raw down/up arrive from onTouchEvent ACTION_DOWN / ACTION_UP directly,
    // while tap / long_press / scroll arrive from GestureDetector callbacks.
    // VelocityTracker computes instantaneous velocity on the Java side and
    // forwards it alongside the scroll distance.
    //
    // Logcat tag: `WarpInput` (Rust). The M2-S11 driver greps these.

    /** Raw ACTION_DOWN: finger first touches screen. */
    external fun inputTouchDown(x: Float, y: Float)

    /** Raw ACTION_UP: finger lifts from screen. */
    external fun inputTouchUp(x: Float, y: Float)

    /**
     * GestureDetector `onSingleTapConfirmed`: confirmed single tap (fires
     * ~300 ms after ACTION_UP, after double-tap window expires).
     */
    external fun inputTap(x: Float, y: Float)

    /**
     * GestureDetector `onLongPress`: sustained press ≥ long-press timeout.
     * Equivalent to right-click / context-menu trigger.
     */
    external fun inputLongPress(x: Float, y: Float)

    /**
     * GestureDetector `onScroll` + VelocityTracker: drag scroll event.
     *
     * @param x  Current finger X position (px).
     * @param y  Current finger Y position (px).
     * @param dx Distance scrolled on X axis since last scroll event (px).
     * @param dy Distance scrolled on Y axis since last scroll event (px).
     * @param vx Instantaneous X velocity from VelocityTracker (px/s).
     * @param vy Instantaneous Y velocity from VelocityTracker (px/s).
     *           Negative vy = finger moved down (content scrolls up).
     */
    external fun inputScroll(x: Float, y: Float, dx: Float, dy: Float, vx: Float, vy: Float)

    /**
     * Returns input state machine counters as a comma-separated string:
     * "touch_down=N,touch_up=N,tap=N,long_press=N,scroll=N,events=N,
     *  last_down_x=F,last_down_y=F,last_up_x=F,last_up_y=F,
     *  last_scroll_vx=F,last_scroll_vy=F"
     */
    external fun inputStats(): String

    /** Reset input state (clear counters + event queue). Driver uses between sub-tests. */
    external fun inputReset()
}
