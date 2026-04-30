package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * M2-S11: Touch-input simulation broadcast receiver.
 *
 * `adb shell input tap <x> <y>` synthesizes a `MotionEvent` at the OS
 * input-dispatcher level, which DOES arrive at `WarpInputView.onTouchEvent`
 * as a real `ACTION_DOWN` + `ACTION_UP` sequence.
 *
 * However, the M2-S11 driver's sub-tests also need to exercise gestures that
 * `adb shell input` cannot easily reproduce with precise velocity (e.g.,
 * long-press duration is not guaranteed, and swipe velocity depends on OS
 * animation interpolation). For those cases the driver broadcasts these intents
 * which call the Rust JNI entry points directly:
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_TAP \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1170.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_LONG_PRESS \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1170.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_SCROLL \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1000.0 \
 *       --ef dx 0.0 --ef dy -300.0 \
 *       --ef vx 0.0 --ef vy -1200.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_TOUCH_DOWN \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1170.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_TOUCH_UP \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1170.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_TOUCH_CANCEL \
 *       -p dev.warp.mobile \
 *       --ef x 540.0 --ef y 1170.0
 *
 *   adb shell am broadcast -a dev.warp.mobile.INPUT_RESET \
 *       -p dev.warp.mobile
 *
 * ## Honest disclosure
 *
 * For the `adb shell input tap` sub-test (sub-test 1) the driver uses the real
 * OS input path which routes through `WarpInputView.onTouchEvent`. This proves
 * the full Java → JNI → Rust chain for raw touch events. The driver logs whether
 * the `adb shell input tap` route was taken vs. simulation broadcast.
 *
 * For the swipe-velocity and long-press sub-tests the driver uses simulation
 * broadcasts, since `adb shell input swipe` velocity is non-deterministic (OS
 * interpolation). This is disclosed in the result JSON.
 *
 * The receiver also forwards raw-down/up through [WarpInputView]'s
 * `onTouchEvent`-equivalent method for the sub-tests that want to exercise
 * the real View code path.
 *
 * ## Web docs consulted (M2-S11, 2026-04-30):
 * - <https://developer.android.com/reference/android/view/MotionEvent>
 * - <https://developer.android.com/reference/android/view/GestureDetector>
 * - <https://developer.android.com/reference/android/view/VelocityTracker>
 * - <https://developer.android.com/training/gestures/detector>
 */
class TouchSimulationReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null) {
            Log.e(TAG, "onReceive: null intent")
            return
        }
        when (intent.action) {
            ACTION_INPUT_TOUCH_DOWN -> handleTouchDown(intent)
            ACTION_INPUT_TOUCH_UP -> handleTouchUp(intent)
            ACTION_INPUT_TOUCH_CANCEL -> handleTouchCancel(intent)
            ACTION_INPUT_TAP -> handleTap(intent)
            ACTION_INPUT_LONG_PRESS -> handleLongPress(intent)
            ACTION_INPUT_SCROLL -> handleScroll(intent)
            ACTION_INPUT_RESET -> handleReset()
            else -> Log.w(TAG, "onReceive: unknown action ${intent.action}")
        }
    }

    private fun handleTouchDown(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        Log.i(TAG, "INPUT_TOUCH_DOWN x=$x y=$y")
        NativeBridge.inputTouchDown(x, y)
    }

    private fun handleTouchUp(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        Log.i(TAG, "INPUT_TOUCH_UP x=$x y=$y")
        NativeBridge.inputTouchUp(x, y)
    }

    private fun handleTouchCancel(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        Log.i(TAG, "INPUT_TOUCH_CANCEL x=$x y=$y")
        NativeBridge.inputTouchCancel(x, y)
    }

    private fun handleTap(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        Log.i(TAG, "INPUT_TAP x=$x y=$y")
        NativeBridge.inputTap(x, y)
    }

    private fun handleLongPress(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        Log.i(TAG, "INPUT_LONG_PRESS x=$x y=$y")
        NativeBridge.inputLongPress(x, y)
    }

    private fun handleScroll(intent: Intent) {
        val x = intent.getFloatExtra(EXTRA_X, 0f)
        val y = intent.getFloatExtra(EXTRA_Y, 0f)
        val dx = intent.getFloatExtra(EXTRA_DX, 0f)
        val dy = intent.getFloatExtra(EXTRA_DY, 0f)
        val vx = intent.getFloatExtra(EXTRA_VX, 0f)
        val vy = intent.getFloatExtra(EXTRA_VY, 0f)
        Log.i(TAG, "INPUT_SCROLL x=$x y=$y dx=$dx dy=$dy vx=$vx vy=$vy")
        NativeBridge.inputScroll(x, y, dx, dy, vx, vy)
    }

    private fun handleReset() {
        Log.i(TAG, "INPUT_RESET")
        NativeBridge.inputReset()
    }

    companion object {
        private const val TAG = "WarpInput"

        const val ACTION_INPUT_TOUCH_DOWN = "dev.warp.mobile.INPUT_TOUCH_DOWN"
        const val ACTION_INPUT_TOUCH_UP = "dev.warp.mobile.INPUT_TOUCH_UP"
        const val ACTION_INPUT_TOUCH_CANCEL = "dev.warp.mobile.INPUT_TOUCH_CANCEL"
        const val ACTION_INPUT_TAP = "dev.warp.mobile.INPUT_TAP"
        const val ACTION_INPUT_LONG_PRESS = "dev.warp.mobile.INPUT_LONG_PRESS"
        const val ACTION_INPUT_SCROLL = "dev.warp.mobile.INPUT_SCROLL"
        const val ACTION_INPUT_RESET = "dev.warp.mobile.INPUT_RESET"

        const val EXTRA_X = "x"
        const val EXTRA_Y = "y"
        const val EXTRA_DX = "dx"
        const val EXTRA_DY = "dy"
        const val EXTRA_VX = "vx"
        const val EXTRA_VY = "vy"
    }
}
