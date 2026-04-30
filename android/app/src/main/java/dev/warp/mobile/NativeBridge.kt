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
}
