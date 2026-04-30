package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Base64
import android.util.Log

/**
 * M3-S05: terminal-input simulation broadcast receiver.
 *
 * The M3-S05 acceptance test (AC#7) needs to drive raw byte sequences (ESC[31m
 * etc.) into the terminal model so the device-side AC verification doesn't
 * depend on a PTY shell faithfully echoing back the same escape sequences
 * (which depends on tty mode, locale, etc.). This receiver provides a direct
 * path: bytes → `NativeBridge.terminalInputBytes` → Rust streaming parser.
 *
 * The PTY pipeline (M3-S04) remains the production path; this receiver is for
 * device-side test instrumentation only.
 *
 * Usage:
 *   adb shell am broadcast -a dev.warp.mobile.TERM_INJECT_RAW \
 *       -p dev.warp.mobile \
 *       --es cmd_id "test" \
 *       --es bytes_b64 "G1szMW1SRUQbWzMybUdSRUVOG1swbQ=="
 *
 *   adb shell am broadcast -a dev.warp.mobile.TERM_RESET \
 *       -p dev.warp.mobile
 *
 * The base64 encoding sidesteps shell-escaping issues for ESC (0x1b) bytes.
 *
 * ## Web docs consulted (M3-S05, 2026-04-30 → 2026-05-01)
 *
 * - <https://developer.android.com/reference/android/util/Base64>
 * - <https://developer.android.com/reference/android/content/BroadcastReceiver>
 *
 * Same `dev.warp.mobile.permission.PTY_CONTROL` gate as
 * `TouchSimulationReceiver` / `PtyBroadcastReceiver`, so debug builds via
 * the manifest overlay can broadcast from `uid=2000`.
 */
class TerminalSimulationReceiver : BroadcastReceiver() {

    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null) {
            Log.e(TAG, "onReceive: null intent")
            return
        }
        when (intent.action) {
            ACTION_TERM_INJECT_RAW -> handleInjectRaw(intent)
            ACTION_TERM_BLOCKS_DUMP -> handleBlocksDump()
            ACTION_TERM_SCROLLBACK_DUMP -> handleScrollbackDump()
            ACTION_TERM_SET_SCROLL_OFFSET -> handleSetScrollOffset(intent)
            else -> Log.w(TAG, "onReceive: unknown action ${intent.action}")
        }
    }

    /**
     * M3-S09: dump the current scrollback ring state to logcat.
     * Test driver greps `TERM_SCROLLBACK_DUMP info=…`.
     */
    private fun handleScrollbackDump() {
        Log.i(TAG, "TERM_SCROLLBACK_DUMP info=${NativeBridge.terminalScrollbackInfo()}")
    }

    /**
     * M3-S09: drive the viewport scroll offset directly from a broadcast.
     * Useful for repeatable test scenarios that don't depend on synthetic
     * touch swipes (which can vary across OEM ROMs).
     */
    private fun handleSetScrollOffset(intent: Intent) {
        val offset = intent.getIntExtra(EXTRA_OFFSET_ROWS, 0)
        NativeBridge.terminalSetScrollOffset(offset)
        Log.i(TAG, "TERM_SET_SCROLL_OFFSET offset=$offset info=${NativeBridge.terminalScrollbackInfo()}")
    }

    /**
     * M3-S07: dump the current `Vec<Block>` to logcat as a single JSON
     * line. The device-test driver `tools/scripts/test-block-model.sh`
     * greps for the `TERM_BLOCKS_DUMP json=...` line.
     */
    private fun handleBlocksDump() {
        val json = NativeBridge.terminalBlocksDump()
        Log.i(TAG, "TERM_BLOCKS_DUMP json=$json")
    }

    private fun handleInjectRaw(intent: Intent) {
        val cmdId = intent.getStringExtra(EXTRA_CMD_ID) ?: "test"
        val b64 = intent.getStringExtra(EXTRA_BYTES_B64)
        val ascii = intent.getStringExtra(EXTRA_BYTES_ASCII)
        val bytes: ByteArray = when {
            b64 != null -> {
                try {
                    Base64.decode(b64, Base64.DEFAULT)
                } catch (e: IllegalArgumentException) {
                    Log.e(TAG, "TERM_INJECT_RAW: invalid base64 ${e.message}")
                    return
                }
            }
            ascii != null -> {
                // Convenience for ASCII-only payloads (no ESC). Tests should
                // prefer bytes_b64 for ESC sequences.
                ascii.toByteArray(Charsets.UTF_8)
            }
            else -> {
                Log.e(TAG, "TERM_INJECT_RAW: missing bytes_b64 or bytes_ascii extra")
                return
            }
        }
        Log.i(
            TAG,
            "TERM_INJECT_RAW cmd_id=$cmdId bytes=${bytes.size} " +
                "first=${if (bytes.isNotEmpty()) String.format("0x%02x", bytes[0]) else "<empty>"}"
        )
        val ingested = NativeBridge.terminalInputBytes(cmdId, bytes)
        Log.i(TAG, "TERM_INJECT_RAW ingested=$ingested")
        // Surface the post-injection SGR/DCS counters so the test driver can
        // grep them out of logcat without a follow-up call.
        Log.i(TAG, "TERM_INJECT_RAW summary=${NativeBridge.terminalSgrSummary()}")
        // M3-S09 — also surface the scrollback state so the M3-S09 driver can
        // gate on the ring-buffer fill level without a separate broadcast.
        Log.i(TAG, "TERM_INJECT_RAW scrollback=${NativeBridge.terminalScrollbackInfo()}")
    }

    companion object {
        private const val TAG = "WarpTerminal"

        const val ACTION_TERM_INJECT_RAW = "dev.warp.mobile.TERM_INJECT_RAW"
        const val ACTION_TERM_BLOCKS_DUMP = "dev.warp.mobile.TERM_BLOCKS_DUMP"
        // M3-S09 actions.
        const val ACTION_TERM_SCROLLBACK_DUMP = "dev.warp.mobile.TERM_SCROLLBACK_DUMP"
        const val ACTION_TERM_SET_SCROLL_OFFSET = "dev.warp.mobile.TERM_SET_SCROLL_OFFSET"

        const val EXTRA_CMD_ID = "cmd_id"
        const val EXTRA_BYTES_B64 = "bytes_b64"
        const val EXTRA_BYTES_ASCII = "bytes_ascii"
        const val EXTRA_OFFSET_ROWS = "offset_rows"
    }
}
