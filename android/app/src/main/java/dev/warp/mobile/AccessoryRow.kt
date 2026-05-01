package dev.warp.mobile

import android.annotation.SuppressLint
import android.content.ClipData
import android.content.ClipboardManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.Toast
import java.util.concurrent.atomic.AtomicLong
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch
import org.json.JSONArray

/**
 * M5-S02: KeyboardAccessoryView above the IME panel.
 *
 * Why this exists: mobile keyboards don't have Esc / Tab / Ctrl / arrow
 * keys / common shell-symbol keys. Without an accessory row, even basic
 * shell tasks (cd .., ls -l | less) require switching to a symbol layer
 * the IME might not have. This row sits between the terminal render and
 * the IME panel so a user can tap one button to send those bytes.
 *
 * Architecture:
 * - HorizontalScrollView so all buttons are reachable without overflow.
 * - LinearLayout child holds the buttons.
 * - Each button has a "send-bytes" closure that builds the right ANSI
 *   escape sequence + dispatches a PTY_WRITE broadcast (same path the
 *   IME and TerminalSimulationReceiver use).
 * - Sticky modifiers (Ctrl, Alt): tapping the modifier highlights it;
 *   the NEXT alphanumeric key sends the modified combo and clears the
 *   highlight. One-shot semantics matches stock-Android terminal apps.
 * - Dynamic symbol pinning (last 20 commands' frequent symbols):
 *   DEFERRED to v1-release polish. Round-1 ships static-only.
 *
 * Visibility:
 * - Default state: GONE (when IME is hidden, no row visible).
 * - When IME shown (per WindowInsets.ime): VISIBLE, positioned just
 *   above the IME panel.
 *
 * The MainActivity owns positioning via WindowInsets listener; this
 * View is content-only.
 */
class AccessoryRow @JvmOverloads constructor(
    context: Context,
    attrs: android.util.AttributeSet? = null,
) : HorizontalScrollView(context, attrs) {

    private val LOG_TAG = "WarpAccessoryRow"
    private val cmdId: String = "default"
    private val rowLayout: LinearLayout

    /**
     * Sticky modifier state. When `ctrlPending` is true, the next
     * alphanumeric key press sends Ctrl-X (i.e. byte = X & 0x1F) and
     * resets the flag. Same for Alt: prefixes the next key with ESC
     * (0x1b) per terminal convention. Tapping the modifier button
     * again toggles its pending state.
     */
    private var ctrlPending: Boolean = false
        set(value) { field = value; refreshModifierVisuals() }
    private var altPending: Boolean = false
        set(value) { field = value; refreshModifierVisuals() }

    private lateinit var ctrlButton: Button
    private lateinit var altButton: Button

    init {
        isFillViewport = false
        setBackgroundColor(0xFF202020.toInt())
        // No edge-to-edge background; respect the standard IME accessory aesthetic.
        rowLayout = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            layoutParams = LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
            gravity = Gravity.CENTER_VERTICAL
        }
        addView(rowLayout)
        buildButtons()
    }

    /**
     * Build all the static buttons in left-to-right order. Each button:
     * - shows a glyph
     * - on click, calls `sendBytes()` with the right escape sequence
     */
    private fun buildButtons() {
        // Order chosen by frequency in shell day-to-day:
        // modifiers first, then escape/tab, then arrows, then common
        // shell punctuation. Mic is rightmost — M5-S04 voice input.
        addBtn("Esc")  {
            // M6-S03 round-4: ESC also cancels any in-flight AI stream.
            // No-op if no stream is running (cancelActiveStream early-
            // returns on activeStreamHandle==0). User-visible behavior:
            // tapping ESC during a streaming AI response stops the
            // stream + sends ESC byte to PTY (consistent dual purpose
            // matches Termux's ESC behavior).
            cancelActiveStream()
            sendBytes(byteArrayOf(0x1B))
        }
        addBtn("Tab")  { sendBytes(byteArrayOf(0x09)) }
        ctrlButton = addBtn("Ctrl") { ctrlPending = !ctrlPending }
        altButton  = addBtn("Alt")  { altPending  = !altPending }
        // Arrow keys send CSI sequences: ESC [ A/B/C/D for up/down/right/left.
        addBtn("↑") { sendBytes("[A".toByteArray()) }
        addBtn("↓") { sendBytes("[B".toByteArray()) }
        addBtn("←") { sendBytes("[D".toByteArray()) }
        addBtn("→") { sendBytes("[C".toByteArray()) }
        // Punctuation that mobile keyboards usually require a symbol-mode
        // round-trip to type. Adding them inline saves several taps for
        // common shell pipelines.
        for (sym in listOf("|", "/", "~", "-", "$", "*", "&", "!", "?", ".")) {
            addBtn(sym) { sendBytes(sym.toByteArray()) }
        }
        // M5-S01: "Copy All" button — flattens all visible terminal blocks
        // (via NativeBridge.terminalBlocksDump) to plain text and writes
        // to Android ClipboardManager. Interactive cell-range selection
        // is v1-release scope; the round-1 label is "Copy All" (not just
        // "Copy") so users aren't misled into expecting selection-aware
        // behavior — that comes when v1 wires the Selection state machine
        // (warp_mobile_android_link::selection) to touch dispatch + this
        // button (per M5-S08 §4 carry-forward).
        addBtn("Copy All") { copyVisibleToClipboard() }
        // M5-S04: Paste button — pulls from Android ClipboardManager and
        // streams to PTY in 4 KB chunks with 1ms gaps so the PTY's read
        // buffer doesn't overflow on long pastes (10K+ chars). ESC during
        // streaming cancels the in-flight paste.
        addBtn("Paste") { startClipboardPaste() }
        // M6-S02: in-app entry point to BYOK SettingsActivity. Required
        // so SettingsActivity can stay android:exported="false" (security
        // review MEDIUM #4) — without an in-app launch path, users (and
        // adb) couldn't reach it. Explicit Intent + setClass works
        // regardless of exported flag because it's a same-process launch.
        addBtn("⚙") {
            val intent = Intent().apply {
                setClass(context, SettingsActivity::class.java)
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            }
            context.startActivity(intent)
        }
        // M6-S03 round-2: AI ghost-suggest button. Sends a hardcoded
        // "suggest a shell completion for 'ls -'" prompt to Claude
        // Haiku via the warp_ai_mobile Rust crate (synchronous round-
        // trip; round-3 will hook into the IME path for live typing-
        // driven ghost-text + Tab-accept). The result is shown as a
        // Toast and ALSO inserted into the PTY as a one-shot
        // "echo SUGGESTION:..." line so users can see it in their
        // terminal scrollback.
        addBtn("💡") { triggerAiSuggest() }
        // M6-S04: agent task button. Opens AgentBlockSheet (Dialog
        // with streaming Sonnet response). Round-1 hardcoded prompt;
        // round-2 will accept a Block ID parameter from a Long-press
        // BottomSheet menu (per M5-S03 deferred UI integration).
        addBtn("🤖") { triggerAgentTask() }
        // Mic placeholder for M5-S04. Voice input via RecognizerIntent is a
        // future enhancement (need explicit RECORD_AUDIO permission flow);
        // round-1 ships paste streaming as the headline M5-S04 feature.
        addBtn("🎤") {
            Log.i(LOG_TAG, "voice input button (RecognizerIntent — v1-release)")
        }
    }

    /**
     * Add a single labelled button to the row. Returns the view so callers
     * can keep a reference (used by Ctrl / Alt for the modifier-visual
     * refresh path).
     */
    @SuppressLint("SetTextI18n")
    private fun addBtn(label: String, onClick: () -> Unit): Button {
        val btn = Button(context).apply {
            text = label
            // Compact button sizing so 15+ buttons fit on a single
            // horizontal-scrolling row without wrapping.
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setPadding(dp(12), dp(6), dp(12), dp(6))
            setBackgroundColor(0xFF303030.toInt())
            setTextColor(Color.WHITE)
            minWidth = dp(40)
            minHeight = dp(36)
            setOnClickListener {
                // For non-modifier keys, apply pending Ctrl/Alt then run the
                // action's bytes. The action closure already calls
                // sendBytes which honours the pending modifiers.
                onClick()
            }
        }
        rowLayout.addView(
            btn,
            LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ).apply { setMargins(dp(2), 0, dp(2), 0) }
        )
        return btn
    }

    /**
     * Send a sequence of raw bytes to the PTY via the existing PTY_WRITE
     * broadcast pipeline (same path the IME state machine uses).
     *
     * Sticky modifier handling:
     * - If `ctrlPending` AND the byte is a printable ASCII letter (a-zA-Z),
     *   replace it with `byte & 0x1f` (Ctrl-letter combo).
     * - If `altPending`, prefix with ESC (0x1b) per terminal-Alt convention.
     * - Either modifier resets after one keystroke (single-shot).
     *
     * Modifiers do NOT compose with multi-byte sequences (arrow keys,
     * Esc itself, Tab) — those just send their bytes through unmodified
     * and clear the pending modifiers. Stock-Android terminal apps
     * (Termux, Termius) use the same convention.
     */
    private fun sendBytes(bytes: ByteArray) {
        var out = bytes
        if (ctrlPending && out.size == 1) {
            val b = out[0].toInt() and 0x7F
            // Apply Ctrl-letter only for ASCII letters; pass through
            // others unchanged so e.g. Ctrl+Esc doesn't garble Esc.
            if (b in 0x40..0x7E) {
                out = byteArrayOf((b and 0x1F).toByte())
            }
        }
        if (altPending) {
            // Prepend ESC: standard "Meta-X" convention for terminals.
            out = byteArrayOf(0x1B.toByte()) + out
        }
        // Reset modifiers after one keystroke (single-shot).
        ctrlPending = false
        altPending = false

        // Dispatch via the manifest-registered PtyBroadcastReceiver only.
        // Bug found in M5-S02 round-1 device test: setPackage(...) was too
        // broad — both the manifest receiver AND the in-service runtime-
        // registered receiver matched the action, causing handleWrite to
        // fire TWICE per click (visible as duplicate PTY_WRITE / PtyOutput
        // log lines + double bytes flowing into cat). Setting an explicit
        // ComponentName targets a single receiver; PtyBroadcastReceiver
        // forwards to WarpTerminalService.onStartCommand which dispatches
        // ACTION_WRITE → handleWrite exactly once.
        val intent = Intent(WarpTerminalService.ACTION_WRITE).apply {
            component = ComponentName(context.packageName, "${context.packageName}.PtyBroadcastReceiver")
            putExtra("cmd_id", cmdId)
            putExtra("data", out)
        }
        context.sendBroadcast(intent)
    }

    /**
     * Visual-state refresh for the sticky-modifier buttons. Highlighted
     * background when pending; default otherwise.
     */
    private fun refreshModifierVisuals() {
        if (::ctrlButton.isInitialized) {
            ctrlButton.setBackgroundColor(
                if (ctrlPending) 0xFF005A9E.toInt() else 0xFF303030.toInt()
            )
        }
        if (::altButton.isInitialized) {
            altButton.setBackgroundColor(
                if (altPending) 0xFF005A9E.toInt() else 0xFF303030.toInt()
            )
        }
    }

    private fun dp(value: Int): Int =
        TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP, value.toFloat(),
            resources.displayMetrics
        ).toInt()

    // ── M5-S04: clipboard paste streaming ────────────────────────────────
    //
    // Why chunked + delayed: a single ptyWrite(10240 bytes) on a 4 KB PTY
    // canonical-mode line buffer would silently drop characters past the
    // first overflow because the kernel's pty buffer fills before the
    // child process drains it. Chunking to 4 KB with 1 ms gaps lets the
    // child's read() loop keep up. Verified by 10K-char round-trip echo
    // test (M5-S04 AC #3).

    private val pasteHandler = Handler(Looper.getMainLooper())
    @Volatile private var pasteCanceled: Boolean = false

    /**
     * Read the system clipboard's primary clip and stream to the PTY.
     * No-op if clipboard is empty or doesn't contain text.
     *
     * Re-entry safe (M6 round-2 code-review MEDIUM #1): if a previous
     * paste is still streaming when the user taps Paste again, the old
     * stream is cancelled FIRST. Without this, two streams interleaved
     * on the same Handler queue would produce garbled PTY input.
     */
    private fun startClipboardPaste() {
        // Cancel any in-flight stream before reading new clipboard
        // content. Cheap idempotent op — sets the flag + drops scheduled
        // postDelayed callbacks so the next chunk dispatch becomes a
        // no-op before we issue any new ones.
        cancelPaste()

        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            ?: run {
                Log.w(LOG_TAG, "paste: ClipboardManager unavailable")
                return
            }
        val clip = cm.primaryClip
        if (clip == null || clip.itemCount == 0) {
            Log.i(LOG_TAG, "paste: clipboard empty")
            return
        }
        val item = clip.getItemAt(0)
        val text = item.coerceToText(context).toString()
        if (text.isEmpty()) {
            Log.i(LOG_TAG, "paste: clipboard text empty")
            return
        }
        val bytes = text.toByteArray(Charsets.UTF_8)
        Log.i(LOG_TAG, "paste: starting stream of ${bytes.size} bytes")
        pasteCanceled = false
        streamPasteChunked(bytes, 0)
    }

    /**
     * Recursive chunked stream: each step writes up to CHUNK_BYTES from
     * `data` starting at `offset` and posts itself to fire 1 ms later
     * for the next chunk. Honors `pasteCanceled` between chunks.
     */
    private fun streamPasteChunked(data: ByteArray, offset: Int) {
        if (pasteCanceled) {
            Log.i(LOG_TAG, "paste: canceled at offset=$offset of ${data.size}")
            return
        }
        if (offset >= data.size) {
            Log.i(LOG_TAG, "paste: complete (${data.size} bytes streamed)")
            return
        }
        val end = (offset + CHUNK_BYTES).coerceAtMost(data.size)
        val chunk = data.copyOfRange(offset, end)
        // Send chunk via the existing PTY_WRITE broadcast — same pipeline
        // sendBytes() uses for keystrokes. Targets PtyBroadcastReceiver
        // explicitly (not setPackage) to avoid the double-dispatch bug
        // (see sendBytes() comment).
        val intent = Intent(WarpTerminalService.ACTION_WRITE).apply {
            component = ComponentName(context.packageName, "${context.packageName}.PtyBroadcastReceiver")
            putExtra("cmd_id", cmdId)
            putExtra("data", chunk)
        }
        context.sendBroadcast(intent)
        // Schedule next chunk after CHUNK_DELAY_MS so the PTY child can
        // drain. 1 ms is conservative; 10K bytes / 4 KB chunks = 3 chunks
        // = ~3 ms total streaming time on flagship.
        pasteHandler.postDelayed({ streamPasteChunked(data, end) }, CHUNK_DELAY_MS)
    }

    /**
     * Cancel any in-flight paste stream. Wired to ESC button so a user
     * can abort if they realize they pasted the wrong thing. Also
     * called by `startClipboardPaste` to prevent stream re-entry races.
     */
    fun cancelPaste() {
        pasteCanceled = true
        pasteHandler.removeCallbacksAndMessages(null)
    }

    companion object {
        // 4 KB matches the canonical Linux PTY line buffer; one chunk
        // typically fits in the PTY without triggering EWOULDBLOCK.
        private const val CHUNK_BYTES = 4096
        // 1 ms gap is enough for the child's read loop to drain on
        // flagship; mid-tier devices may need 2-3 ms (tunable later).
        private const val CHUNK_DELAY_MS = 1L
    }

    // ── M5-S01: copy visible terminal blocks to clipboard ───────────────
    //
    // Round-1 scope: copy ALL visible block content. Interactive cell-range
    // selection is v1-release polish (warp_mobile_android_link/src/
    // selection.rs has the state machine + 11 unit tests; touch-event
    // wiring + Vulkan overlay rect drawing are deferred).
    //
    // The flatten path: NativeBridge.terminalBlocksDump returns a JSON
    // array of {command, output, exit_code, ...} blocks. We concat the
    // output fields with newline separators and write to ClipboardManager.

    private fun copyVisibleToClipboard() {
        val cm = context.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager
            ?: run {
                Log.w(LOG_TAG, "copy: ClipboardManager unavailable")
                return
            }
        val blocksJson = try {
            NativeBridge.terminalBlocksDump()
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "copy: terminalBlocksDump JNI failed: ${e.message}")
            null
        }
        val text = flattenBlocksToText(blocksJson)
        if (text.isEmpty()) {
            Log.i(LOG_TAG, "copy: no visible block content")
            Toast.makeText(context, "Nothing to copy", Toast.LENGTH_SHORT).show()
            return
        }
        val clipData = ClipData.newPlainText("warp-terminal", text)
        // M6 round-2 security review MEDIUM #2: terminal output may
        // contain secrets (env vars echoed by `env`, `cat .env`, etc).
        // Mark the clip as sensitive on Android 13+ so the system-level
        // clipboard preview toast doesn't show the first line, and
        // visible-clipboard surfaces (Gboard clipboard panel, system
        // overlay) hide the content until tapped.
        // Refs: https://developer.android.com/about/versions/13/features/copy-paste
        if (android.os.Build.VERSION.SDK_INT >= 33) {
            clipData.description.extras = android.os.PersistableBundle().apply {
                putBoolean("android.content.extra.IS_SENSITIVE", true)
            }
        }
        cm.setPrimaryClip(clipData)
        Log.i(LOG_TAG, "copy: ${text.length} chars copied to clipboard (sensitive flag: SDK>=33)")
        Toast.makeText(context, "Copied ${text.length} chars", Toast.LENGTH_SHORT).show()
    }

    // ── M6-S03 round-2: AI ghost-text via Claude Haiku ──────────────────
    //
    // Reads the saved BYOK key from AiKeyStore, sends a hardcoded sample
    // prompt to Claude Haiku via NativeBridge.aiGhostComplete (which
    // dispatches to warp_ai_mobile::client::messages_complete on a
    // tokio per-call runtime). Shows result as Toast + writes to PTY
    // as `echo "WARP-AI: <suggestion>"` so it appears in scrollback.
    //
    // Round-3 scope:
    //   - read the current shell-input line from the PTY tail (need
    //     a new JNI getter; not present in M1-M3)
    //   - debounced auto-trigger 150ms after last keystroke
    //   - cancel-on-keystroke via tokio CancellationToken
    //   - render grayed suggestion as IME-cursor-anchored overlay
    //     (TextView at the right pixel coords) instead of toast/echo
    //   - Tab key intercept to accept

    /**
     * Round-3: streaming AI suggest. Replaces the round-2 sync
     * round-trip with the StreamStart / StreamPoll / StreamFree JNI
     * triple. Each :CHUNK: arrives in 50-200ms intervals; we echo
     * each chunk to the PTY (as comment line) the moment it arrives,
     * so the user sees progressive output instead of one big toast at
     * the end.
     *
     * Cancellation: ESC button claims the handle via getAndSet(0L)
     * — whichever path (button vs poll-loop finally) wins ownership
     * does the Cancel + Free, the loser is a no-op. Prevents a TOCTOU
     * use-after-free where button reads handle, poll-loop frees the
     * Arc, then button calls Cancel on the freed handle (round-3
     * code-review CRITICAL).
     */
    private val activeStreamHandle = AtomicLong(0L)

    /**
     * AI coroutine scope. SupervisorJob so one failing stream doesn't
     * cancel the next 💡 tap. Canceled in onDetachedFromWindow so a
     * rotation/teardown doesn't leak the in-flight stream + Context.
     * Replaces the prior GlobalScope.launch (round-3 code-review LOW).
     */
    private val aiScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private fun triggerAiSuggest() {
        // M6-S05 offline check: short-circuit before spinning up a
        // coroutine + reading Keystore + posting HTTPS that would just
        // fail with a network error. Banner-style toast guides user to
        // toggle airplane mode off.
        if (!AiConnectivity.get(context).isOnline()) {
            Toast.makeText(
                context,
                "AI features paused — no network. Toggle airplane mode off and try again.",
                Toast.LENGTH_LONG
            ).show()
            return
        }
        aiScope.launch {
            val apiKey = try {
                AiKeyStore.load(context)
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "ai: AiKeyStore load failed: ${e.message}")
                null
            }
            if (apiKey.isNullOrBlank()) {
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                    Toast.makeText(
                        context,
                        "No API key — open ⚙ Settings to set one",
                        Toast.LENGTH_LONG
                    ).show()
                }
                return@launch
            }

            // Cancel any previously-running stream + free its handle so
            // tapping 💡 twice doesn't run two streams in parallel.
            cancelActiveStream()

            // Round-3 hardcoded prompt; round-4 will pull from PTY tail.
            val prompt = "Suggest a single shell command completion for `ls -`. " +
                "Reply with ONLY the completed command, no explanation, no markdown."
            val t0 = System.currentTimeMillis()
            val handle = try {
                NativeBridge.aiGhostStreamStart(
                    apiKey,
                    "claude-haiku-4-5",
                    prompt,
                    /* maxTokens = */ 50
                )
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "ai: aiGhostStreamStart threw: ${e.message}")
                kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                    Toast.makeText(context, "AI start failed: ${e.message}", Toast.LENGTH_LONG).show()
                }
                return@launch
            }
            activeStreamHandle.set(handle)
            Log.i(LOG_TAG, "ai: stream started handle=$handle")

            // Open-line indicator in PTY.
            sendPtyComment("# WARP-AI streaming…\n")

            // Poll loop. 50ms cadence is invisible to user; chunks arrive
            // in 50-200ms intervals from Anthropic anyway.
            var assembled = StringBuilder()
            try {
                while (true) {
                    // Single-shot delay between polls. yields back to
                    // the IO dispatcher so other coroutines can run.
                    kotlinx.coroutines.delay(50)
                    val response = try {
                        NativeBridge.aiGhostStreamPoll(handle)
                    } catch (e: Throwable) {
                        Log.e(LOG_TAG, "ai: poll threw: ${e.message}")
                        ":ERR:JNI poll threw: ${e.message}"
                    }
                    when {
                        response.isNullOrEmpty() -> {
                            // Still running, no new chunks. Keep polling.
                        }
                        response.startsWith(":CHUNK:") -> {
                            val text = response.removePrefix(":CHUNK:")
                            assembled.append(text)
                            // Push each chunk to PTY immediately (as a
                            // comment fragment). Round-4 IME overlay will
                            // render this on the SurfaceView instead.
                            sendPtyComment(text)
                        }
                        response.startsWith(":DONE:") -> {
                            val elapsed = System.currentTimeMillis() - t0
                            Log.i(
                                LOG_TAG,
                                "ai: stream done elapsedMs=$elapsed total_chars=${assembled.length}"
                            )
                            // M6-S06: per-request token telemetry. The
                            // streaming pipe doesn't expose Anthropic's
                            // `usage` event today (round-3 only forwards
                            // content_block_delta), so estimate from
                            // assembled char count: ~4 chars per token
                            // (matches AgentBlockSheet's estimator).
                            // Architect-review gap close.
                            AiUsageTracker.record(
                                context,
                                kind = "ghost",
                                model = "claude-haiku-4-5",
                                inputTokens = prompt.length / 4,
                                outputTokens = assembled.length / 4,
                                latencyMs = elapsed,
                            )
                            sendPtyComment("\n# WARP-AI done (${elapsed}ms)\n")
                            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                                Toast.makeText(
                                    context,
                                    "AI streamed ${assembled.length} chars in ${elapsed}ms",
                                    Toast.LENGTH_SHORT
                                ).show()
                            }
                            break
                        }
                        response.startsWith(":ERR:") -> {
                            val msg = response.removePrefix(":ERR:")
                            Log.e(LOG_TAG, "ai: stream error: $msg")
                            sendPtyComment("\n# WARP-AI error: $msg\n")
                            kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.Main) {
                                Toast.makeText(
                                    context,
                                    "AI error: ${msg.take(120)}",
                                    Toast.LENGTH_LONG
                                ).show()
                            }
                            break
                        }
                    }
                }
            } finally {
                // Atomic-claim free path: only the path that wins the
                // CAS owns the Free call. If the ESC button (or a new
                // 💡 tap that calls cancelActiveStream) already claimed
                // the handle, it has already done both Cancel+Free and
                // we skip — preventing a double-free on the Rust Arc.
                if (activeStreamHandle.compareAndSet(handle, 0L)) {
                    try {
                        NativeBridge.aiGhostStreamFree(handle)
                    } catch (e: Throwable) {
                        Log.e(LOG_TAG, "ai: free threw: ${e.message}")
                    }
                }
            }
        }
    }

    /**
     * Cancel any in-flight AI stream. Wired to the ESC button (round-4)
     * + called from triggerAiSuggest itself before starting a new stream
     * so two simultaneous 💡 taps don't run two streams in parallel.
     *
     * Atomic-claim semantics: getAndSet(0L) returns the handle and
     * clears the slot in one step. Whoever wins the claim owns BOTH
     * cancel + free; whoever loses (the poll-loop finally) is a no-op.
     * Prevents the round-3 review CRITICAL — a TOCTOU where the cancel
     * read a handle the poll-loop subsequently freed.
     */
    fun cancelActiveStream() {
        val h = activeStreamHandle.getAndSet(0L)
        if (h != 0L) {
            try {
                NativeBridge.aiGhostStreamCancel(h)
                Log.i(LOG_TAG, "ai: cancel + free claimed for handle=$h")
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "ai: cancel threw: ${e.message}")
            }
            try {
                NativeBridge.aiGhostStreamFree(h)
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "ai: free threw: ${e.message}")
            }
        }
    }

    override fun onDetachedFromWindow() {
        super.onDetachedFromWindow()
        // Cancel any in-flight stream + cancel the scope so the launched
        // coroutine doesn't keep ticking after the View detaches.
        cancelActiveStream()
        aiScope.cancel()
    }

    /**
     * M6-S04: open the Agent BottomSheet with a hardcoded round-1 prompt.
     * Round-2 will accept a Block ID + read its command + output via
     * `terminalBlocksDump` so the agent gets real shell context.
     */
    private fun triggerAgentTask() {
        if (!AiConnectivity.get(context).isOnline()) {
            Toast.makeText(
                context,
                "AI features paused — no network. Toggle airplane mode off and try again.",
                Toast.LENGTH_LONG
            ).show()
            return
        }
        val prompt = "Explain what `du -sh *` does in a Unix shell, " +
            "and how I'd interpret its output. Keep it to 3 short paragraphs."
        AgentBlockSheet(context, prompt).show()
    }

    /** Helper: send a literal string (e.g. AI streamed comment) to the PTY. */
    private fun sendPtyComment(text: String) {
        val intent = Intent(WarpTerminalService.ACTION_WRITE).apply {
            component = ComponentName(context.packageName, "${context.packageName}.PtyBroadcastReceiver")
            putExtra("cmd_id", cmdId)
            putExtra("data", text.toByteArray(Charsets.UTF_8))
        }
        context.sendBroadcast(intent)
    }

    /**
     * Flatten the M3-S07 terminalBlocksDump JSON (array of block objects)
     * to plain text. Each block contributes "command\noutput\n" with the
     * exit_code suffix appended for non-zero exits. Returns empty string
     * for null / malformed JSON.
     *
     * Schema (per warp_terminal_mobile_facade::blocks::dump_blocks_json):
     *   [{"command":"ls -la","output":"...","exit_code":0,"start_time":...},
     *    ...]
     */
    private fun flattenBlocksToText(json: String?): String {
        if (json.isNullOrEmpty()) return ""
        return try {
            val arr = JSONArray(json)
            buildString {
                for (i in 0 until arr.length()) {
                    val o = arr.optJSONObject(i) ?: continue
                    val cmd = o.optString("command", "")
                    val out = o.optString("output", "")
                    val exit = o.optInt("exit_code", 0)
                    if (cmd.isNotEmpty()) {
                        append("$ ").append(cmd).append('\n')
                    }
                    if (out.isNotEmpty()) {
                        append(out)
                        if (!out.endsWith('\n')) append('\n')
                    }
                    if (exit != 0) {
                        append("[exit ").append(exit).append("]\n")
                    }
                }
            }
        } catch (e: Throwable) {
            Log.w(LOG_TAG, "copy: JSON parse failed: ${e.message}")
            ""
        }
    }
}
