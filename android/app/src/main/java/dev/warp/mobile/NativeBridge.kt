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
}
