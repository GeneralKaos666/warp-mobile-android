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

    private val frameCallback = object : Choreographer.FrameCallback {
        override fun doFrame(frameTimeNanos: Long) {
            if (!renderActive) return
            // Magenta clear-color test (M2-S04 AC#2).
            val ok = NativeBridge.renderClearFrame(1.0f, 0.0f, 1.0f, 1.0f)
            if (!ok) {
                // VK_ERROR_OUT_OF_DATE_KHR or transient: skip + retry next vsync.
                Log.d(TAG, "renderClearFrame returned false @ ${SystemClock.uptimeMillis()}")
            }
            // Schedule next frame (Choreographer is one-shot).
            Choreographer.getInstance().postFrameCallback(this)
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
        attachAndStartRender(holder.surface)
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        val ts = SystemClock.uptimeMillis()
        Log.i(TAG, "surfaceChanged_ts=$ts width=$width height=$height")
        // Re-attach: the Rust side replaces any prior swapchain in attach().
        renderActive = false
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        attachAndStartRender(holder.surface)
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        val ts = SystemClock.uptimeMillis()
        Log.i(TAG, "surfaceDestroyed_ts=$ts")
        renderActive = false
        Choreographer.getInstance().removeFrameCallback(frameCallback)
        NativeBridge.renderDetachSurface()
    }

    // ── Internals ────────────────────────────────────────────────────────────

    private fun attachAndStartRender(surface: Surface) {
        val ok = NativeBridge.renderAttachSurface(surface)
        Log.i(TAG, "renderAttachSurface ok=$ok")
        if (ok) {
            renderActive = true
            Choreographer.getInstance().postFrameCallback(frameCallback)
        }
    }

    companion object {
        private const val TAG = "WarpRender"
    }
}
