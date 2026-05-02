package dev.warp.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * V1-prep test hook: drive the GhostSuggestController state machine via
 * `adb shell am broadcast` for end-to-end UX verification without a real
 * Anthropic API key.
 *
 * Why: M6-CO1's IME-debounced ghost-text auto-trigger streams suggestions
 * via Haiku and renders them in the AccessoryRow ghostStrip. Verifying
 * the END-USER experience (strip renders, Tab accepts, suffix bytes go
 * to PTY) requires a successful `:DONE:` event from the streaming pipe.
 * On the dev box the saved test API key always returns HTTP 401, so the
 * happy path was code-verified-only until this receiver landed.
 *
 * Drives:
 *   adb shell am broadcast \
 *     -a dev.warp.mobile.GHOST_INJECT_SUGGESTION \
 *     -p dev.warp.mobile \
 *     --es buffer "ls -" \
 *     --es suggestion "ls -la"
 *
 * Same permission gating pattern as the other simulation receivers:
 * release builds require PTY_CONTROL signature permission; debug builds
 * strip it via tools:remove so adb shell broadcasts from uid=2000 can
 * reach the receiver during device tests.
 *
 * Refs:
 *   tools/scripts/test-ghost-suggest.sh — driver that fires this receiver,
 *     dumps the AccessoryRow UI tree, taps the Tab button, and asserts
 *     the suffix bytes reached PTY.
 */
class GhostSuggestSimulationReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context?, intent: Intent?) {
        if (intent == null || context == null) {
            Log.e(TAG, "onReceive: null intent/context")
            return
        }
        when (intent.action) {
            ACTION_GHOST_INJECT_SUGGESTION -> handleInject(context, intent)
            ACTION_GHOST_DISMISS -> handleDismiss(context)
            else -> Log.w(TAG, "onReceive: unknown action ${intent.action}")
        }
    }

    private fun handleInject(context: Context, intent: Intent) {
        val buffer = intent.getStringExtra(EXTRA_BUFFER) ?: ""
        val suggestion = intent.getStringExtra(EXTRA_SUGGESTION) ?: ""
        if (buffer.isEmpty() || suggestion.isEmpty()) {
            Log.e(TAG, "GHOST_INJECT_SUGGESTION: missing 'buffer' or 'suggestion' extra")
            return
        }
        // Ensure the controller has a Context to read AiKeyStore /
        // AiConnectivity from (set by AccessoryRow.onAttachedToWindow,
        // but this receiver may fire before the View attaches — pass
        // through here as defense-in-depth).
        GhostSuggestController.setContext(context.applicationContext)
        GhostSuggestController.injectSimulatedSuggestion(buffer, suggestion)
        Log.i(TAG, "GHOST_INJECT_SUGGESTION ok: buffer=\"$buffer\" suggestion=\"$suggestion\"")
    }

    private fun handleDismiss(context: Context) {
        GhostSuggestController.setContext(context.applicationContext)
        GhostSuggestController.dismissSuggestion()
        Log.i(TAG, "GHOST_DISMISS ok")
    }

    companion object {
        private const val TAG = "WarpGhostSim"
        const val ACTION_GHOST_INJECT_SUGGESTION = "dev.warp.mobile.GHOST_INJECT_SUGGESTION"
        const val ACTION_GHOST_DISMISS = "dev.warp.mobile.GHOST_DISMISS"
        const val EXTRA_BUFFER = "buffer"
        const val EXTRA_SUGGESTION = "suggestion"
    }
}
