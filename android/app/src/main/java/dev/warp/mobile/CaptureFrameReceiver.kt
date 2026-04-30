package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * M2-S05: manifest-registered broadcast receiver for `dev.warp.mobile.CAPTURE_FRAME`.
 *
 * Runtime-registered receivers don't reliably match `am broadcast` from the
 * `shell` UID on Android 14+ (the broadcast queue treats runtime receivers as
 * cached/background-only when reached from outside the registering process).
 * Manifest-declared receivers ARE delivered, so we use one here for the
 * device-driver test path.
 *
 * Bridge contract:
 *   - The receiver invokes [NativeBridge.renderCaptureFrame] directly.
 *   - That JNI call blocks until `vkQueueWaitIdle` returns; it serializes
 *     against the per-vsync `renderClearFrame` calls via the swapchain mutex
 *     in `crates/android-host/src/vulkan.rs`. So even if the Choreographer is
 *     submitting a frame at the moment we trigger capture, the mutex
 *     guarantees there's no concurrent VkQueue submit on the same queue.
 *   - The receiver runs on the main thread (default for manifest receivers),
 *     same as the Choreographer callback — so contention is naturally
 *     serialized at the Java level too.
 *
 * Driver invocation:
 *   adb shell am broadcast \
 *     -a dev.warp.mobile.CAPTURE_FRAME \
 *     -p dev.warp.mobile \
 *     --es path /data/local/tmp/m2-s05-capture.png \
 *     --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0
 */
class CaptureFrameReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null) {
            Log.e(TAG, "CAPTURE_FRAME onReceive: null intent")
            return
        }
        val path = intent.getStringExtra("path") ?: run {
            Log.e(TAG, "CAPTURE_FRAME missing 'path' extra")
            return
        }
        val r = intent.getFloatExtra("r", 1.0f)
        val g = intent.getFloatExtra("g", 0.0f)
        val b = intent.getFloatExtra("b", 1.0f)
        val a = intent.getFloatExtra("a", 1.0f)
        Log.i(TAG, "CAPTURE_FRAME received path=$path rgba=$r,$g,$b,$a")
        // Direct JNI call — capture_to_png is fully self-contained on the
        // Rust side and serializes against the choreographer loop via the
        // swapchain mutex.
        val ok = NativeBridge.renderCaptureFrame(path, r, g, b, a)
        Log.i(TAG, "renderCaptureFrame ok=$ok path=$path")
    }

    companion object {
        private const val TAG = "WarpRender"
        const val ACTION_CAPTURE_FRAME = "dev.warp.mobile.CAPTURE_FRAME"
    }
}
