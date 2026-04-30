package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * M2-S10: IME simulation broadcast receiver.
 *
 * `adb shell input text 'hello'` does NOT route through `InputConnection` —
 * the `input` command synthesizes raw `KeyEvent`s at the framework's input
 * dispatcher level, which bypasses the IME entirely. To exercise the
 * `WarpInputView.WarpInputConnection.commitText / setComposingText /
 * finishComposingText` code path end-to-end through the JNI without requiring
 * a real Pinyin IME on the device, the M2-S10 driver issues these broadcasts:
 *
 *   adb shell am broadcast -a dev.warp.mobile.IME_COMMIT_TEXT \
 *       -p dev.warp.mobile \
 *       --es text "hello" \
 *       --ei cursor 1
 *
 *   adb shell am broadcast -a dev.warp.mobile.IME_SET_COMPOSING_TEXT \
 *       -p dev.warp.mobile \
 *       --es text "ni" \
 *       --ei cursor 1
 *
 *   adb shell am broadcast -a dev.warp.mobile.IME_FINISH_COMPOSING_TEXT \
 *       -p dev.warp.mobile
 *
 *   adb shell am broadcast -a dev.warp.mobile.IME_RESET \
 *       -p dev.warp.mobile
 *
 * The receiver routes the call to the real `BaseInputConnection` subclass on
 * the active `WarpInputView` so the **identical** code path exercised by a
 * real Gboard/Pinyin IME is hit. There is no shortcut to JNI: every call
 * goes through `WarpInputConnection.commitText/setComposingText/...` first,
 * which logs the event and forwards to JNI.
 *
 * If MainActivity is not in the foreground (and the input view therefore not
 * available), the receiver falls back to calling `NativeBridge.imeXxx`
 * directly so the state machine still receives the events. The fallback is
 * logged so the driver can detect this case.
 *
 * The text is base64-encoded via `text_b64` extra (preferred) to handle
 * multi-byte UTF-8 (CJK) cleanly — `am broadcast --es` mangles spaces and
 * sometimes non-ASCII when relayed through `adb shell`.
 */
class ImeSimulationReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null) {
            Log.e(TAG, "onReceive: null intent")
            return
        }
        when (intent.action) {
            ACTION_IME_COMMIT_TEXT -> handleCommit(intent)
            ACTION_IME_SET_COMPOSING_TEXT -> handleSetComposing(intent)
            ACTION_IME_FINISH_COMPOSING_TEXT -> handleFinishComposing()
            ACTION_IME_RESET -> handleReset()
            ACTION_IME_TYPE_LATIN -> handleTypeLatin(intent)
            else -> Log.w(TAG, "onReceive: unknown action ${intent.action}")
        }
    }

    private fun handleCommit(intent: Intent) {
        val text = extractText(intent)
        val cursor = intent.getIntExtra(EXTRA_CURSOR, 1)
        Log.i(TAG, "IME_COMMIT_TEXT text=${quote(text)} cursor=$cursor")
        forwardCommit(text, cursor)
    }

    private fun handleSetComposing(intent: Intent) {
        val text = extractText(intent)
        val cursor = intent.getIntExtra(EXTRA_CURSOR, 1)
        Log.i(TAG, "IME_SET_COMPOSING_TEXT text=${quote(text)} cursor=$cursor")
        forwardSetComposing(text, cursor)
    }

    private fun handleFinishComposing() {
        Log.i(TAG, "IME_FINISH_COMPOSING_TEXT")
        forwardFinishComposing()
    }

    private fun handleReset() {
        Log.i(TAG, "IME_RESET")
        NativeBridge.imeReset()
    }

    /**
     * Convenience: types a Latin string char-by-char as separate `commitText`
     * calls, mirroring how Gboard's English layout drives the IME during
     * normal typing. Driver invocation:
     *
     *   adb shell am broadcast -a dev.warp.mobile.IME_TYPE_LATIN \
     *       -p dev.warp.mobile \
     *       --es text "hello"
     */
    private fun handleTypeLatin(intent: Intent) {
        val text = extractText(intent)
        Log.i(TAG, "IME_TYPE_LATIN text=${quote(text)} len=${text.length}")
        for (codePoint in text.codePoints()) {
            val s = String(Character.toChars(codePoint))
            forwardCommit(s, 1)
        }
    }

    /**
     * Forward the call through the active WarpInputView's InputConnection so
     * the identical code path as a real IME is exercised. Falls back to a
     * direct JNI call if the input view isn't available.
     */
    private fun forwardCommit(text: String, cursor: Int) {
        val view = MainActivity.activeWarpInputView
        if (view != null) {
            view.post {
                val ic = view.warpInputConnectionOrNull()
                if (ic != null) {
                    ic.commitText(text, cursor)
                } else {
                    Log.w(TAG, "WarpInputConnection unavailable; falling back to direct JNI")
                    NativeBridge.imeCommitText(text, cursor)
                }
            }
        } else {
            Log.w(TAG, "MainActivity not foreground; falling back to direct JNI")
            NativeBridge.imeCommitText(text, cursor)
        }
    }

    private fun forwardSetComposing(text: String, cursor: Int) {
        val view = MainActivity.activeWarpInputView
        if (view != null) {
            view.post {
                val ic = view.warpInputConnectionOrNull()
                if (ic != null) {
                    ic.setComposingText(text, cursor)
                } else {
                    Log.w(TAG, "WarpInputConnection unavailable; falling back to direct JNI")
                    NativeBridge.imeSetComposingText(text, cursor)
                }
            }
        } else {
            Log.w(TAG, "MainActivity not foreground; falling back to direct JNI")
            NativeBridge.imeSetComposingText(text, cursor)
        }
    }

    private fun forwardFinishComposing() {
        val view = MainActivity.activeWarpInputView
        if (view != null) {
            view.post {
                val ic = view.warpInputConnectionOrNull()
                if (ic != null) {
                    ic.finishComposingText()
                } else {
                    Log.w(TAG, "WarpInputConnection unavailable; falling back to direct JNI")
                    NativeBridge.imeFinishComposingText()
                }
            }
        } else {
            Log.w(TAG, "MainActivity not foreground; falling back to direct JNI")
            NativeBridge.imeFinishComposingText()
        }
    }

    private fun extractText(intent: Intent): String {
        // text_b64 first (CJK / space-resilient via base64), then text plain.
        val b64 = intent.getStringExtra(EXTRA_TEXT_B64)
        if (b64 != null) {
            return try {
                String(
                    android.util.Base64.decode(b64, android.util.Base64.DEFAULT),
                    Charsets.UTF_8
                )
            } catch (e: Exception) {
                Log.w(TAG, "text_b64 decode failed (${e.message}); falling back to text")
                intent.getStringExtra(EXTRA_TEXT) ?: ""
            }
        }
        return intent.getStringExtra(EXTRA_TEXT) ?: ""
    }

    private fun quote(s: String): String {
        val truncated = if (s.length > 64) s.substring(0, 64) + "…" else s
        return "\"" + truncated.replace("\\", "\\\\").replace("\"", "\\\"") + "\""
    }

    companion object {
        private const val TAG = "WarpIme"
        const val ACTION_IME_COMMIT_TEXT = "dev.warp.mobile.IME_COMMIT_TEXT"
        const val ACTION_IME_SET_COMPOSING_TEXT = "dev.warp.mobile.IME_SET_COMPOSING_TEXT"
        const val ACTION_IME_FINISH_COMPOSING_TEXT = "dev.warp.mobile.IME_FINISH_COMPOSING_TEXT"
        const val ACTION_IME_RESET = "dev.warp.mobile.IME_RESET"
        const val ACTION_IME_TYPE_LATIN = "dev.warp.mobile.IME_TYPE_LATIN"

        const val EXTRA_TEXT = "text"
        const val EXTRA_TEXT_B64 = "text_b64"
        const val EXTRA_CURSOR = "cursor"
    }
}
