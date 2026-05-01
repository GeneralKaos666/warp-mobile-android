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
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

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
 * Threading: state mutations go through [updateStateAtomically] which
 * synchronizes the read-then-write composition (round-3 review HIGH —
 * volatile alone can't make `current = current.copy(...)` atomic across
 * UI + IO threads). The Haiku call runs on Dispatchers.IO via [aiScope].
 * [aiScope] is intentionally process-lifetime: this is an `object`
 * singleton + Android process lifecycle is what bounds it.
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

    /**
     * Lock that serializes read-then-write composition on [current].
     * Round-3 review HIGH: a `@Volatile var` only guarantees publication
     * visibility, not atomicity of compound updates. Three threads
     * compose with `current.copy(...)`:
     *   - UI thread: onTextCommitted → appendToBuffer / resetBuffer
     *   - IO thread: debounceJob coroutine → fireSuggestion launch
     *   - IO thread: poll loop → updateState on chunk/done/err
     * Without the lock, two interleaved updates lose one writer's
     * change.
     */
    private val stateLock = Any()
    private var current: SuggestionState =
        SuggestionState(buffer = "", suggestion = "", phase = "")
    @Volatile private var debounceJob: Job? = null
    @Volatile private var enabled: Boolean = true

    /** Toggle auto-suggest on/off. Off means no debounce + no AI calls. */
    fun setEnabled(value: Boolean) {
        enabled = value
        if (!value) {
            cancelActiveStream()
            mutateState { it.copy(suggestion = "", phase = "") }
        }
    }

    fun isEnabled(): Boolean = enabled

    fun snapshot(): SuggestionState = synchronized(stateLock) { current }

    fun register(l: Listener) {
        listeners.add(l)
        l.onSuggestionState(synchronized(stateLock) { current })
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
     * IME backspace / delete: shrink the buffer by N chars. Called
     * from WarpInputConnection.deleteSurroundingText (IME-side delete)
     * and from sendKeyEvent on KEYCODE_DEL (hardware backspace).
     *
     * Without this hook, a user typing "lsx" then backspacing to "ls"
     * would have the controller's buffer stuck at "lsx" — Haiku gets
     * the wrong context, suggestion is wrong, Tab-accept emits the
     * wrong suffix. v1 UX bug.
     *
     * Round-1 simplification: shrinks from the END of the buffer
     * regardless of where the actual cursor is. Mid-word backspaces
     * (cursor in middle of typed text) will diverge from actual PTY
     * state, but the immediate visual feedback is correct (suggestion
     * stays GONE because phase clears) and a follow-up commitText
     * resets the trajectory.
     */
    fun onTextDeleted(charsBeforeCursor: Int) {
        if (!enabled || charsBeforeCursor <= 0) return
        mutateState { state ->
            val newLen = (state.buffer.length - charsBeforeCursor).coerceAtLeast(0)
            val newBuffer = state.buffer.substring(0, newLen)
            // Drop the suggestion + revert phase on edit — same as
            // appendToBuffer's "thinking" except we don't reschedule
            // a fetch (let the user keep editing first; they'll
            // commitText again when done).
            state.copy(buffer = newBuffer, suggestion = "", phase = "")
        }
        cancelActiveStream()
    }

    /**
     * Accept the active suggestion. Returns the bytes to send to PTY,
     * or null if no active suggestion. Caller (AccessoryRow Tab
     * button) handles the byte transmission via PtyBroadcastReceiver.
     *
     * Two-mode emission:
     *   - Suggestion is a continuation of buffer (case-insensitive
     *     prefix match) → emit just the SUFFIX. Common case.
     *   - Suggestion contradicts buffer (LLM rewrote the command) →
     *     prepend Ctrl-U (0x15) to clear the current shell line, then
     *     emit the full suggestion. Avoids the round-3 review HIGH —
     *     "ls -" + "find . -name '*.log'" would otherwise emit
     *     "ls -find . -name '*.log'" (garbled append).
     */
    fun acceptCurrent(): ByteArray? {
        val s = snapshot()
        if (s.suggestion.isEmpty() || s.phase != "ready") return null
        val suffixBytes: ByteArray = if (s.suggestion.startsWith(s.buffer, ignoreCase = true)) {
            s.suggestion.substring(s.buffer.length).toByteArray(Charsets.UTF_8)
        } else {
            // Ctrl-U (0x15) clears from cursor to start-of-line in
            // POSIX shells (bash, zsh, dash, ksh) — safe across all
            // shells we ship in the bootstrap. Then the full
            // suggestion replaces what was on the line.
            byteArrayOf(0x15) + s.suggestion.toByteArray(Charsets.UTF_8)
        }
        mutateState { it.copy(suggestion = "", phase = "") }
        Log.i(LOG_TAG, "accepted; emitting bytes=${suffixBytes.size} (suffix-mode=${s.suggestion.startsWith(s.buffer, ignoreCase = true)})")
        return suffixBytes
    }

    /**
     * Manually clear the suggestion (e.g. on ESC press). Buffer is
     * preserved so the user can keep typing.
     */
    fun dismissSuggestion() {
        cancelActiveStream()
        mutateState { it.copy(suggestion = "", phase = "") }
    }

    private fun appendToBuffer(text: String) {
        mutateState { state ->
            val newBuffer = (state.buffer + text).take(MAX_BUFFER_CHARS)
            state.copy(buffer = newBuffer, suggestion = "", phase = "thinking")
        }
        scheduleSuggestion()
    }

    private fun resetBuffer(reason: String) {
        cancelActiveStream()
        mutateState { SuggestionState(buffer = "", suggestion = "", phase = "") }
        Log.i(LOG_TAG, "buffer reset: $reason")
    }

    /**
     * Atomic state composition. The `transform` runs under [stateLock]
     * so concurrent UI/IO thread updates can't lose each other's
     * changes (round-3 review HIGH closure). Listener fan-out happens
     * AFTER the lock is released to keep the critical section tight.
     */
    private inline fun mutateState(transform: (SuggestionState) -> SuggestionState) {
        val newState: SuggestionState
        synchronized(stateLock) {
            newState = transform(current)
            current = newState
        }
        mainHandler.post {
            for (l in listeners) {
                try {
                    l.onSuggestionState(newState)
                } catch (t: Throwable) {
                    Log.e(LOG_TAG, "listener threw: ${t.message}")
                }
            }
        }
    }

    /**
     * Schedule a Haiku stream after [DEBOUNCE_MS] of typing inactivity.
     * Re-scheduling cancels any prior pending request — both the
     * Kotlin-side debounce coroutine AND the Rust-side stream task.
     *
     * Round-2 v1+1 carry-over closure: previously, typing during an
     * in-flight stream cancelled the Kotlin coroutine but NOT the
     * Rust task. The async task continued the HTTP round-trip (wasting
     * tokens / network / latency budget on a result the user no longer
     * cares about) until completion, with chunks accumulating in a
     * queue nobody polled. Calling cancelActiveStream() here signals
     * the Rust CancellationToken so the streaming task aborts at the
     * next tokio::select! checkpoint — typically within a single
     * SSE chunk frame (~50 ms).
     */
    private fun scheduleSuggestion() {
        debounceJob?.cancel()
        // Cancel any in-flight Rust stream — it's about to be obsolete
        // because the user kept typing. Atomic-claim semantics: the
        // poll loop's finally block will be a no-op if we won the
        // claim here (compareAndSet won't match).
        cancelActiveStream()
        val cur = snapshot()
        if (cur.buffer.length < MIN_BUFFER_CHARS) {
            mutateState { it.copy(suggestion = "", phase = "") }
            return
        }
        debounceJob = aiScope.launch {
            delay(DEBOUNCE_MS)
            // Re-check buffer at fire time — it may have shrunk to
            // below threshold or been reset since the schedule.
            val s = snapshot()
            if (s.buffer.length < MIN_BUFFER_CHARS) return@launch
            fireSuggestion(s.buffer)
        }
    }

    private suspend fun fireSuggestion(buffer: String) {
        val context = contextProvider?.get() ?: run {
            // Round-3 review MEDIUM #3: clear the "thinking" phase so
            // the strip doesn't get stuck showing "thinking…" forever
            // when typing happens before AccessoryRow.onAttachedToWindow
            // has called setContext (e.g., very fast typing on cold start).
            Log.w(LOG_TAG, "no Context available; clearing phase")
            mutateState { it.copy(phase = "") }
            return
        }
        if (!AiConnectivity.get(context).isOnline()) {
            mutateState { it.copy(phase = "offline") }
            return
        }
        val apiKey = try {
            AiKeyStore.load(context)
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "AiKeyStore load failed: ${e.message}")
            null
        }
        if (apiKey.isNullOrBlank()) {
            mutateState { it.copy(phase = "no-key") }
            return
        }

        // NOTE: previously called `cancelActiveStream()` here as a
        // defensive guard against a leftover stream — but that was a
        // self-cancel cascade bug. `cancelActiveStream` ends with
        // `debounceJob?.cancel()` which CANCELS THIS VERY coroutine
        // (we're running INSIDE debounceJob). The CancellationException
        // would then fire at the next delay() suspend point in the
        // poll loop, the finally block would compareAndSet+Free, and
        // the stream would be freed within ~1 ms of starting — before
        // any SSE chunks could arrive. User would never see :DONE:.
        //
        // Round-2 self-pacing fix: scheduleSuggestion() at line 255
        // already calls cancelActiveStream BEFORE this coroutine is
        // launched, so any stale handle is already cleared. This call
        // is redundant + harmful, so it was removed.

        val prompt = "Complete this shell command. Reply with ONLY the " +
            "completed command — no explanation, no quotes, no markdown:\n\n" +
            buffer

        val handle = try {
            NativeBridge.aiGhostStreamStart(apiKey, GHOST_MODEL, prompt, GHOST_MAX_TOKENS)
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "stream start threw: ${e.message}")
            mutateState { it.copy(phase = "error") }
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
                        mutateState { it.copy(suggestion = finalText, phase = "ready") }
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
                        mutateState { it.copy(phase = "error") }
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
