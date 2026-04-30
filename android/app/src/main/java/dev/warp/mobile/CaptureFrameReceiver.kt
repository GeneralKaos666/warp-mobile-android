package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * M2-S05 / M2-S07: manifest-registered broadcast receiver for capture-frame
 * requests.
 *
 * Runtime-registered receivers don't reliably match `am broadcast` from the
 * `shell` UID on Android 14+ (the broadcast queue treats runtime receivers as
 * cached/background-only when reached from outside the registering process).
 * Manifest-declared receivers ARE delivered, so we use one here for the
 * device-driver test path.
 *
 * Bridge contract:
 *   - The receiver invokes [NativeBridge.renderCaptureFrame] (M2-S05) or
 *     [NativeBridge.renderCaptureFrameWithText] (M2-S07) directly depending
 *     on the action.
 *   - That JNI call blocks until `vkQueueWaitIdle` returns; it serializes
 *     against the per-vsync `renderClearFrame` calls via the swapchain mutex
 *     in `crates/android-host/src/vulkan.rs`. So even if the Choreographer is
 *     submitting a frame at the moment we trigger capture, the mutex
 *     guarantees there's no concurrent VkQueue submit on the same queue.
 *   - The receiver runs on the main thread (default for manifest receivers),
 *     same as the Choreographer callback — so contention is naturally
 *     serialized at the Java level too.
 *
 * M2-S05 driver invocation:
 *   adb shell am broadcast \
 *     -a dev.warp.mobile.CAPTURE_FRAME \
 *     -p dev.warp.mobile \
 *     --es path /data/local/tmp/m2-s05-capture.png \
 *     --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0
 *
 * M2-S07 driver invocation:
 *   adb shell am broadcast \
 *     -a dev.warp.mobile.CAPTURE_FRAME_WITH_TEXT \
 *     -p dev.warp.mobile \
 *     --es path /data/local/tmp/m2-s07-capture.png \
 *     --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0 \
 *     --es text "Hello, 世界" \
 *     --ef font_size_px 96.0 \
 *     --ef baseline_x 100.0 --ef baseline_y 600.0
 */
class CaptureFrameReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null) {
            Log.e(TAG, "onReceive: null intent")
            return
        }
        when (intent.action) {
            ACTION_CAPTURE_FRAME -> handleCaptureFrame(intent)
            ACTION_CAPTURE_FRAME_WITH_TEXT -> handleCaptureFrameWithText(intent)
            else -> Log.w(TAG, "onReceive: unknown action ${intent.action}")
        }
    }

    private fun handleCaptureFrame(intent: Intent) {
        val path = intent.getStringExtra("path") ?: run {
            Log.e(TAG, "CAPTURE_FRAME missing 'path' extra")
            return
        }
        val r = intent.getFloatExtra("r", 1.0f)
        val g = intent.getFloatExtra("g", 0.0f)
        val b = intent.getFloatExtra("b", 1.0f)
        val a = intent.getFloatExtra("a", 1.0f)
        Log.i(TAG, "CAPTURE_FRAME received path=$path rgba=$r,$g,$b,$a")
        val ok = NativeBridge.renderCaptureFrame(path, r, g, b, a)
        Log.i(TAG, "renderCaptureFrame ok=$ok path=$path")
    }

    private fun handleCaptureFrameWithText(intent: Intent) {
        val path = intent.getStringExtra("path") ?: run {
            Log.e(TAG, "CAPTURE_FRAME_WITH_TEXT missing 'path' extra")
            return
        }
        // Text precedence (CJK shell-escape resilient):
        //   1. `text_b64` extra (base64-encoded UTF-8 bytes) — driver-friendly,
        //      avoids `am broadcast --es` losing multi-byte chars when relayed
        //      via adb shell.
        //   2. `text` extra (plain string) — works for ASCII tests.
        //   3. Default `"Hello, 世界"` — the M2-S07 acceptance test phrase.
        val textB64 = intent.getStringExtra("text_b64")
        val textExtra = intent.getStringExtra("text")
        val text = when {
            textB64 != null -> {
                try {
                    String(android.util.Base64.decode(textB64, android.util.Base64.DEFAULT), Charsets.UTF_8)
                } catch (e: Exception) {
                    Log.w(TAG, "text_b64 decode failed (${e.message}); falling back to default")
                    DEFAULT_TEXT
                }
            }
            !textExtra.isNullOrBlank() -> textExtra
            else -> DEFAULT_TEXT
        }
        val r = intent.getFloatExtra("r", 1.0f)
        val g = intent.getFloatExtra("g", 0.0f)
        val b = intent.getFloatExtra("b", 1.0f)
        val a = intent.getFloatExtra("a", 1.0f)
        val fontSizePx = intent.getFloatExtra("font_size_px", 96.0f)
        val baselineX = intent.getFloatExtra("baseline_x", 100.0f)
        val baselineY = intent.getFloatExtra("baseline_y", 600.0f)
        Log.i(
            TAG,
            "CAPTURE_FRAME_WITH_TEXT received path=$path text=\"$text\" len=${text.length} " +
                "rgba=$r,$g,$b,$a font_size_px=$fontSizePx baseline=($baselineX,$baselineY)"
        )
        val ok = NativeBridge.renderCaptureFrameWithText(
            path, r, g, b, a, text, fontSizePx, baselineX, baselineY
        )
        Log.i(TAG, "renderCaptureFrameWithText ok=$ok path=$path")
    }

    companion object {
        private const val TAG = "WarpRender"
        const val ACTION_CAPTURE_FRAME = "dev.warp.mobile.CAPTURE_FRAME"
        const val ACTION_CAPTURE_FRAME_WITH_TEXT = "dev.warp.mobile.CAPTURE_FRAME_WITH_TEXT"
        // M2-S07 acceptance phrase (Latin + CJK).
        private const val DEFAULT_TEXT = "Hello, 世界"
    }
}
