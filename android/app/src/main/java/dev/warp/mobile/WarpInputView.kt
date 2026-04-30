package dev.warp.mobile

import android.content.Context
import android.text.Spanned
import android.util.AttributeSet
import android.util.Log
import android.view.GestureDetector
import android.view.MotionEvent
import android.view.VelocityTracker
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * M2-S10: a focusable, IME-attaching `View` that does NOT render anything
 * (the SurfaceView next to it owns the rendering). Its sole job is to attract
 * `onCreateInputConnection` calls from the system IME and route Java-side
 * `InputConnection` callbacks (`commitText`, `setComposingText`,
 * `finishComposingText`) into the Rust state machine in
 * `crates/android-host/src/ime.rs` via [NativeBridge].
 *
 * ## Why a separate View?
 *
 * `SurfaceView` does not play well with IME — its surface is on a separate
 * Z-layer (window manager hosted), and the framework's input-routing code
 * paths assume the focused View also owns the visible content. So we use a
 * composite layout: SurfaceView for render + invisible WarpInputView (size
 * 1x1, alpha 0) overlay, with `setFocusableInTouchMode(true)` so it captures
 * focus and the IME attaches to it.
 *
 * ## State machine
 *
 * The Rust side maintains the composing-text buffer (see `ime.rs` for the
 * full state machine). This Kotlin side is essentially a transparent shim:
 * every IC override forwards the args to JNI, then calls super (so that
 * `BaseInputConnection`'s own bookkeeping — relevant for delete/getTextBefore
 * etc. on subsequent IME queries — stays consistent).
 *
 * ## M2-S11 addition: onTouchEvent + GestureDetector
 *
 * WarpInputView is 1×1 px overlaying the SurfaceView; however, in the
 * FrameLayout composite layout it sits at the top of the Z-order and
 * `isClickable = true` means it receives touch events first (before
 * SurfaceView). We override `onTouchEvent` here to:
 *
 *   1. Emit raw `ACTION_DOWN` / `ACTION_UP` → `NativeBridge.inputTouchDown/Up`.
 *   2. Feed the event to a `GestureDetector` for tap/long-press detection.
 *   3. Feed the event to `VelocityTracker` so scroll events carry instantaneous
 *      velocity.
 *
 * **Why WarpInputView gets touch, not SurfaceView**: SurfaceView is on a
 * separate Z-layer (window-manager-hosted surface). Touch dispatch walks the
 * View hierarchy, not the Z-order of surfaces. WarpInputView is a regular View
 * at z-position 1 (added second to FrameLayout), so it sits above SurfaceView
 * for hit-testing purposes. `isClickable=true` ensures it doesn't pass through.
 *
 * **Focus + touch tension**: WarpInputView must have `isFocusableInTouchMode`
 * for IME (S10 requirement). Touch dispatch calls `requestFocus()` on the
 * target view when `isFocusableInTouchMode = true`, which is idempotent here
 * since it already has focus. No conflict.
 *
 * **GestureDetector timing**: `onSingleTapConfirmed` fires ~300 ms after
 * ACTION_UP (double-tap window). Raw `inputTouchDown`/`inputTouchUp` fire
 * immediately for latency-sensitive paths.
 *
 * **VelocityTracker**: initialized on ACTION_DOWN, fed on ACTION_MOVE,
 * velocity computed at ACTION_MOVE and forwarded with each `inputScroll` call.
 * Released on ACTION_UP / ACTION_CANCEL.
 *
 * ## Web docs consulted (M2-S11, 2026-04-30):
 * - <https://developer.android.com/reference/android/view/MotionEvent>
 * - <https://developer.android.com/reference/android/view/GestureDetector>
 * - <https://developer.android.com/reference/android/view/GestureDetector.SimpleOnGestureListener>
 * - <https://developer.android.com/reference/android/view/VelocityTracker>
 * - <https://developer.android.com/training/gestures/detector>
 * ## Web docs consulted (M2-S10, 2026-04-30):
 * - <https://developer.android.com/reference/android/view/inputmethod/InputConnection>
 * - <https://developer.android.com/reference/android/view/inputmethod/BaseInputConnection>
 * - <https://developer.android.com/develop/ui/views/touch-and-input/creating-input-method>
 * - <https://infinum.com/blog/input-connection/>
 */
class WarpInputView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
    defStyleAttr: Int = 0
) : View(context, attrs, defStyleAttr) {

    /**
     * The most recent `WarpInputConnection` returned from
     * `onCreateInputConnection`. Set on the UI thread; read by
     * [ImeSimulationReceiver] (also UI thread via `View.post`). May be null
     * if no IME is currently attached. Held as a strong reference for the
     * driver path; the framework normally drops the IC when the IME detaches
     * but we keep the last one alive for test-injection purposes.
     *
     * In production (M3+) this would be reset to null when the IME detaches
     * (overrride `onInputConnectionClosed` on API 24+). For M2-S10 driver
     * needs we let it linger.
     */
    private var lastInputConnection: WarpInputConnection? = null

    fun warpInputConnectionOrNull(): WarpInputConnection? = lastInputConnection

    // M2-S11: GestureDetector for tap / long-press / scroll detection.
    private val gestureListener = object : GestureDetector.SimpleOnGestureListener() {
        /**
         * Fires ~300 ms after ACTION_UP once the double-tap window expires.
         * More reliable for "single tap intent" than raw ACTION_UP which could
         * be the start of a double-tap.
         */
        override fun onSingleTapConfirmed(e: MotionEvent): Boolean {
            Log.i(TAG, "gesture_tap x=${e.x} y=${e.y}")
            NativeBridge.inputTap(e.x, e.y)
            return true
        }

        /**
         * Long-press: sustained contact ≥ ViewConfiguration.getLongPressTimeout
         * (~500 ms). Equivalent to right-click / context-menu trigger.
         */
        override fun onLongPress(e: MotionEvent) {
            Log.i(TAG, "gesture_long_press x=${e.x} y=${e.y}")
            NativeBridge.inputLongPress(e.x, e.y)
        }

        /**
         * Scroll: drag gesture (finger moves while touching). `distanceX`/`Y`
         * are the pixels moved since the previous call (positive = scrolled
         * up / left). We forward these alongside the instantaneous velocity
         * from `velocityTracker` so the Rust side can drive deceleration.
         *
         * `e1` is the initial ACTION_DOWN; `e2` is the current ACTION_MOVE.
         */
        override fun onScroll(
            e1: MotionEvent?,
            e2: MotionEvent,
            distanceX: Float,
            distanceY: Float
        ): Boolean {
            val vx: Float
            val vy: Float
            val vt = velocityTracker
            if (vt != null) {
                vt.addMovement(e2)
                // Units: pixels per second (default). Positive = moving right/down.
                vt.computeCurrentVelocity(1000)
                vx = vt.xVelocity
                vy = vt.yVelocity
            } else {
                vx = 0f
                vy = 0f
            }
            Log.d(TAG, "gesture_scroll x=${e2.x} y=${e2.y} dx=$distanceX dy=$distanceY vx=$vx vy=$vy")
            NativeBridge.inputScroll(e2.x, e2.y, distanceX, distanceY, vx, vy)
            return true
        }

        // Return true so GestureDetector tracks scroll state from the initial
        // ACTION_DOWN. Without this, onScroll is never called.
        override fun onDown(e: MotionEvent): Boolean = true
    }

    private val gestureDetector = GestureDetector(context, gestureListener)

    // M2-S11: VelocityTracker — initialized on ACTION_DOWN, released on ACTION_UP.
    // The Java VelocityTracker pool is small; always recycle.
    private var velocityTracker: VelocityTracker? = null

    init {
        // Critical for IME attachment:
        //   isFocusable = true        — System.requestFocus may target us.
        //   isFocusableInTouchMode    — touches in our area request focus.
        //   isClickable = true        — also needed on some OEM ROMs.
        // Background is null (no draw cost). The View has size 1x1 in the
        // composite layout (see MainActivity).
        isFocusable = true
        isFocusableInTouchMode = true
        isClickable = true
    }

    /**
     * M2-S11: touch event handler.
     *
     * Routes raw ACTION_DOWN / ACTION_UP to Rust JNI immediately (low latency),
     * and feeds every event through the GestureDetector for higher-level gesture
     * recognition (tap, long-press, scroll).
     *
     * We always return `true` so the View consumes the event and Android does
     * not propagate it further down the view tree.
     */
    override fun onTouchEvent(event: MotionEvent): Boolean {
        // Feed GestureDetector first (needs the raw event).
        gestureDetector.onTouchEvent(event)

        when (event.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                Log.i(TAG, "touch_down x=${event.x} y=${event.y}")
                // Acquire a fresh VelocityTracker for this touch sequence.
                velocityTracker?.recycle()
                velocityTracker = VelocityTracker.obtain()
                velocityTracker!!.addMovement(event)
                NativeBridge.inputTouchDown(event.x, event.y)
            }
            MotionEvent.ACTION_MOVE -> {
                // Feed VelocityTracker so onScroll can query instantaneous velocity.
                velocityTracker?.addMovement(event)
            }
            MotionEvent.ACTION_UP -> {
                Log.i(TAG, "touch_up x=${event.x} y=${event.y}")
                NativeBridge.inputTouchUp(event.x, event.y)
                velocityTracker?.recycle()
                velocityTracker = null
            }
            MotionEvent.ACTION_CANCEL -> {
                Log.d(TAG, "touch_cancel")
                velocityTracker?.recycle()
                velocityTracker = null
            }
        }
        // Returning true is required for GestureDetector to detect scroll and
        // long-press: if we return false on ACTION_DOWN the detector won't
        // receive subsequent ACTION_MOVE events.
        return true
    }

    override fun onCheckIsTextEditor(): Boolean = true

    /**
     * Called by the framework when the IME attaches to this View (typically
     * after our View gains focus + the user taps to bring up the soft
     * keyboard). We populate `outAttrs` with a generic IME_ACTION_NONE +
     * TYPE_CLASS_TEXT so Gboard / Pinyin display normally and not in some
     * specialized mode (PIN, password, search, etc.).
     */
    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection {
        outAttrs.inputType = EditorInfo.TYPE_CLASS_TEXT or EditorInfo.TYPE_TEXT_FLAG_MULTI_LINE
        outAttrs.imeOptions = EditorInfo.IME_ACTION_NONE or EditorInfo.IME_FLAG_NO_EXTRACT_UI
        // Some Pinyin IMEs query the locale; default to system default which
        // pulls the user's preferred locale. We don't override `hintLocales`
        // since the user's installed keyboards drive language detection.
        Log.i(TAG, "onCreateInputConnection inputType=0x${"%x".format(outAttrs.inputType)} imeOptions=0x${"%x".format(outAttrs.imeOptions)}")
        val ic = WarpInputConnection(this, /* fullEditor = */ false)
        lastInputConnection = ic
        return ic
    }

    /**
     * Custom `BaseInputConnection` subclass. `fullEditor=false` means we
     * don't keep a full Editable buffer (we don't have text selection,
     * cursor movement, etc. for a terminal); we only need the composing-
     * region machinery, which is in the Rust state machine.
     *
     * We still call `super.*` so that `BaseInputConnection`'s minimal
     * Editable bookkeeping stays consistent for any subsequent IME queries
     * like `getTextBeforeCursor` / `deleteSurroundingText` (M2-S11+; not
     * exercised in M2-S10).
     */
    class WarpInputConnection(
        view: View,
        fullEditor: Boolean
    ) : BaseInputConnection(view, fullEditor) {

        override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
            val s = text?.toString() ?: ""
            Log.i(
                TAG,
                "IC.commitText text=${quote(s)} cursorPos=$newCursorPosition len=${s.length}"
            )
            NativeBridge.imeCommitText(s, newCursorPosition)
            return super.commitText(text, newCursorPosition)
        }

        override fun setComposingText(
            text: CharSequence?,
            newCursorPosition: Int
        ): Boolean {
            // Strip Spanned styling — Rust state machine deals in plain
            // strings only. This also lets us log a clean version.
            val s = text?.toString() ?: ""
            Log.i(
                TAG,
                "IC.setComposingText text=${quote(s)} cursorPos=$newCursorPosition len=${s.length} spanned=${text is Spanned}"
            )
            NativeBridge.imeSetComposingText(s, newCursorPosition)
            return super.setComposingText(text, newCursorPosition)
        }

        override fun finishComposingText(): Boolean {
            Log.i(TAG, "IC.finishComposingText")
            NativeBridge.imeFinishComposingText()
            return super.finishComposingText()
        }

        // Note: deleteSurroundingText, sendKeyEvent, etc. are NOT routed to
        // Rust in M2-S10 — those land in M2-S11 (touch + key dispatch). The
        // BaseInputConnection super-class default handles them on the local
        // Editable buffer for now so the IME doesn't get confused.

        private fun quote(s: String): String {
            // Compact one-line representation suitable for logcat. Truncates
            // overly long composing strings (Pinyin can sometimes have
            // candidate previews) but for "ni hao" / "你好" cases this is
            // never reached.
            val truncated = if (s.length > 64) s.substring(0, 64) + "…" else s
            return "\"" + truncated.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
        }
    }

    companion object {
        private const val TAG = "WarpIme"
    }
}
