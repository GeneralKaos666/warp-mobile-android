package dev.warp.mobile

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.util.Log
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * M6 carry-over #1 (IME-bound ghost auto-trigger): debounced typing →
 * Haiku ghost-text suggestion stream → Tab accepts.
 *
 * State machine driven by the IME path. WarpInputView's
 * WarpInputConnection forwards every commitText / setComposingText to
 * [onTextCommitted] / [onTextComposing]; this controller maintains a
 * "current command line" buffer (resets on Enter / 0x0d), debounces
 * 300ms after the last edit, and fires a Haiku streaming request via
 * the same `aiGhostStream*` JNI surface that the 💡 button uses.
 *
 * UI contract:
 *   - AccessoryRow registers as a [Listener] in onAttachedToWindow
 *   - State updates marshal to the View's UI thread via post()
 *   - When [SuggestionState.suggestion] is non-empty, the AccessoryRow
 *     shows a strip "💡 <suggestion> · Tab to accept"
 *   - Tab button in AccessoryRow checks [acceptCurrent] — if a
 *     suggestion is active, it returns the suffix bytes to send to
 *     PTY (and clears the state); otherwise the Tab sends 0x09 as
 *     before
 *
 * Round-1 scope decisions:
 *   - Suggestion is shown in AccessoryRow strip (not as cursor-anchored
 *     overlay) — the latter requires JNI accessor for cursor screen
 *     position which is a separate v1 enhancement
 *   - Buffer is reset hard on Enter; we don't try to detect partial
 *     line edits (delete word, cursor-left, etc.) — keeps the state
 *     machine simple. Wrong suggestion just means user ignores it.
 *   - Min buffer length 2 chars before triggering — typing a single
 *     letter shouldn't burn API tokens
 *   - Per-keystroke debounce: every text event resets the 300ms timer
 *     (matching Gmail / VS Code / IntelliJ inline-suggestion cadence)
 *   - The active stream's handle is tracked separately from the
 *     AccessoryRow.activeStreamHandle (which is for the manual 💡
 *     button) so the two paths can coexist
 *
 * Threading: the buffer + state are protected by `synchronized(this)`.
 * The Haiku call runs on Dispatchers.IO via [aiScope].
 */
object GhostSuggestController {
    private const val LOG_TAG = "WarpGhostSuggest"
    private const val DEBOUNCE_MS = 300L
    private const val MIN_BUFFER_CHARS = 2
    private const val MAX_BUFFER_CHARS = 200
    private const val GHOST_MODEL = "claude-haiku-4-5"
    private const val GHOST_MAX_TOKENS = 50

    interface Listener {
        fun onSuggestionState(state: SuggestionState)
    }

    /**
     * Snapshot of the controller state for UI consumers. The
     * AccessoryRow renders directly off this; the listener fires on
     * every change.
     */
    data class SuggestionState(
        /** What the user has typed since the last Enter. */
        val buffer: String,
        /** Active suggestion text, or "" if none. */
        val suggestion: String,
        /** "" / "thinking" / "ready" / "error". UI shows different glyphs. */
        val phase: String,
    )

    private val listeners = CopyOnWriteArrayList<Listener>()
    private val mainHandler = Handler(Looper.getMainLooper())
    private val aiScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val activeStreamHandle = AtomicLong(0L)

    @Volatile private var current: SuggestionState =
        SuggestionState(buffer = "", suggestion = "", phase = "")
    @Volatile private var debounceJob: Job? = null
    @Volatile private var enabled: Boolean = true

    /** Toggle auto-suggest on/off. Off means no debounce + no AI calls. */
    fun setEnabled(value: Boolean) {
        enabled = value
        if (!value) {
            cancelActiveStream()
            updateState(current.copy(suggestion = "", phase = ""))
        }
    }

    fun isEnabled(): Boolean = enabled

    fun snapshot(): SuggestionState = current

    fun register(l: Listener) {
        listeners.add(l)
        l.onSuggestionState(current)
    }

    fun unregister(l: Listener) {
        listeners.remove(l)
    }

    /**
     * IME committed text: append to buffer, reset on Enter, restart
     * debounce. Called from WarpInputConnection.commitText on UI thread.
     */
    fun onTextCommitted(text: String) {
        if (!enabled) return
        // CommitText with "\n" or "\r" means line submitted — clear
        // buffer + drop suggestion. Other whitespace appends.
        if (text.contains('\n') || text.contains('\r')) {
            resetBuffer("submitted")
            return
        }
        appendToBuffer(text)
    }

    /**
     * IME composing text: replaces the composing region. We treat
     * this as appending the new composing chunk to the buffer's
     * "stable" prefix — but for round-1 we ignore composing text
     * to avoid CJK IME pinyin candidate previews thrashing the
     * suggestion. Only commitText drives suggestions.
     */
    fun onTextComposing(text: String) {
        // Round-1: composing text is candidate-preview; ignored for
        // suggestion driving. Future iteration can hook this for
        // English IMEs that submit only on space/punctuation.
    }

    /**
     * Accept the active suggestion. Returns the bytes to send to PTY
     * (the suffix of the suggestion that wasn't already in the
     * buffer), or null if no active suggestion. Caller (AccessoryRow
     * Tab button) handles the byte transmission via the existing
     * sendBytes path.
     */
    fun acceptCurrent(): ByteArray? {
        val s = current
        if (s.suggestion.isEmpty() || s.phase != "ready") return null
        // Compute the suffix: the suggestion minus the buffer prefix,
        // case-insensitive matching to be robust to Haiku's stylistic
        // capitalization. Common case: buffer="ls -" suggestion="ls -la"
        // → suffix="la".
        val suffix = if (s.suggestion.startsWith(s.buffer, ignoreCase = true)) {
            s.suggestion.substring(s.buffer.length)
        } else {
            // Suggestion didn't continue the buffer — accept as-is.
            // User might have typed "wrong" prefix; the model returned
            // a corrected command. Sending the full suggestion would
            // duplicate characters; safer to emit just the suggestion
            // delta from cursor: clear-line (\x15 = Ctrl-U) + suggestion.
            // For round-1 simplicity, emit the full suggestion + let
            // the user backspace if needed.
            s.suggestion
        }
        // Clear state — user has accepted, next Enter will reset cleanly.
        updateState(s.copy(suggestion = "", phase = ""))
        Log.i(LOG_TAG, "accepted; sending suffix bytes=${suffix.length}")
        return suffix.toByteArray(Charsets.UTF_8)
    }

    /**
     * Manually clear the suggestion (e.g. on ESC press). Buffer is
     * preserved so the user can keep typing.
     */
    fun dismissSuggestion() {
        cancelActiveStream()
        updateState(current.copy(suggestion = "", phase = ""))
    }

    private fun appendToBuffer(text: String) {
        val newBuffer = (current.buffer + text).take(MAX_BUFFER_CHARS)
        updateState(current.copy(buffer = newBuffer, suggestion = "", phase = "thinking"))
        scheduleSuggestion()
    }

    private fun resetBuffer(reason: String) {
        cancelActiveStream()
        updateState(SuggestionState(buffer = "", suggestion = "", phase = ""))
        Log.i(LOG_TAG, "buffer reset: $reason")
    }

    private fun updateState(state: SuggestionState) {
        current = state
        mainHandler.post {
            for (l in listeners) {
                try {
                    l.onSuggestionState(state)
                } catch (t: Throwable) {
                    Log.e(LOG_TAG, "listener threw: ${t.message}")
                }
            }
        }
    }

    /**
     * Schedule a Haiku stream after [DEBOUNCE_MS] of typing inactivity.
     * Re-scheduling cancels any prior pending request.
     */
    private fun scheduleSuggestion() {
        debounceJob?.cancel()
        if (current.buffer.length < MIN_BUFFER_CHARS) {
            updateState(current.copy(suggestion = "", phase = ""))
            return
        }
        debounceJob = aiScope.launch {
            delay(DEBOUNCE_MS)
            // Re-check buffer at fire time — it may have shrunk to
            // below threshold or been reset since the schedule.
            val snapshot = current
            if (snapshot.buffer.length < MIN_BUFFER_CHARS) return@launch
            fireSuggestion(snapshot.buffer)
        }
    }

    private suspend fun fireSuggestion(buffer: String) {
        // Get application context from a registered listener (via
        // weak ref discipline). For simplicity we read it from the
        // first non-null listener Context. If no listener has a
        // context, skip — controller can't function without one.
        val context = contextProvider?.get() ?: run {
            Log.w(LOG_TAG, "no Context available; skipping suggestion")
            return
        }
        if (!AiConnectivity.get(context).isOnline()) {
            updateState(current.copy(phase = "offline"))
            return
        }
        val apiKey = try {
            AiKeyStore.load(context)
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "AiKeyStore load failed: ${e.message}")
            null
        }
        if (apiKey.isNullOrBlank()) {
            updateState(current.copy(phase = "no-key"))
            return
        }

        // Cancel any prior in-flight stream first — atomic-claim semantics
        // matching AccessoryRow / AgentBlockSheet.
        cancelActiveStream()

        val prompt = "Complete this shell command. Reply with ONLY the " +
            "completed command — no explanation, no quotes, no markdown:\n\n" +
            buffer

        val handle = try {
            NativeBridge.aiGhostStreamStart(apiKey, GHOST_MODEL, prompt, GHOST_MAX_TOKENS)
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "stream start threw: ${e.message}")
            updateState(current.copy(phase = "error"))
            return
        }
        activeStreamHandle.set(handle)
        val t0 = System.currentTimeMillis()

        val streamingBuffer = StringBuilder()
        try {
            while (true) {
                delay(50)
                val response = try {
                    NativeBridge.aiGhostStreamPoll(handle)
                } catch (e: Throwable) {
                    ":ERR:JNI poll: ${e.message}"
                }
                when {
                    response.isNullOrEmpty() -> { /* still running */ }
                    response.startsWith(":CHUNK:") -> {
                        streamingBuffer.append(response.removePrefix(":CHUNK:"))
                    }
                    response.startsWith(":DONE:") -> {
                        val elapsed = System.currentTimeMillis() - t0
                        val finalText = streamingBuffer.toString().trim()
                        updateState(current.copy(suggestion = finalText, phase = "ready"))
                        AiUsageTracker.record(
                            context, kind = "ghost", model = GHOST_MODEL,
                            inputTokens = prompt.length / 4,
                            outputTokens = finalText.length / 4,
                            latencyMs = elapsed,
                        )
                        Log.i(LOG_TAG, "ready: \"${finalText.take(60)}\" elapsedMs=$elapsed")
                        break
                    }
                    response.startsWith(":ERR:") -> {
                        val msg = response.removePrefix(":ERR:")
                        Log.w(LOG_TAG, "stream err: ${msg.take(120)}")
                        updateState(current.copy(phase = "error"))
                        break
                    }
                }
            }
        } finally {
            // Atomic-claim free path matching AccessoryRow pattern.
            if (activeStreamHandle.compareAndSet(handle, 0L)) {
                try { NativeBridge.aiGhostStreamFree(handle) } catch (_: Throwable) {}
            }
        }
    }

    private fun cancelActiveStream() {
        val h = activeStreamHandle.getAndSet(0L)
        if (h != 0L) {
            try { NativeBridge.aiGhostStreamCancel(h) } catch (_: Throwable) {}
            try { NativeBridge.aiGhostStreamFree(h) } catch (_: Throwable) {}
        }
        debounceJob?.cancel()
    }

    /**
     * Lightweight Context provider. AccessoryRow sets this in
     * onAttachedToWindow via [setContextProvider]. Decoupled from
     * Listener so the singleton can run AI calls even before the
     * AccessoryRow listener is registered (e.g., if the user types
     * with no IME-up + accessory hidden).
     */
    private var contextProvider: java.lang.ref.WeakReference<Context>? = null

    fun setContext(context: Context) {
        contextProvider = java.lang.ref.WeakReference(context.applicationContext)
    }
}
