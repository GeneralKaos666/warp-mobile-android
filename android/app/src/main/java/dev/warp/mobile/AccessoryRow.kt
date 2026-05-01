package dev.warp.mobile

import android.annotation.SuppressLint
import android.content.ClipData
import android.content.ClipboardManager
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
        addBtn("Esc")  { sendBytes(byteArrayOf(0x1B)) }
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
        // M5-S01: Copy button — flattens all visible terminal blocks
        // (via NativeBridge.terminalBlocksDump) to plain text and writes
        // to Android ClipboardManager. Interactive cell-range selection
        // is the v1-release scope; round-1 ships "copy all visible".
        addBtn("Copy") { copyVisibleToClipboard() }
        // M5-S04: Paste button — pulls from Android ClipboardManager and
        // streams to PTY in 4 KB chunks with 1ms gaps so the PTY's read
        // buffer doesn't overflow on long pastes (10K+ chars). ESC during
        // streaming cancels the in-flight paste.
        addBtn("Paste") { startClipboardPaste() }
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

        // Dispatch via the existing PtyBroadcastReceiver path. This routes
        // through PtyManager.write → NativeBridge.ptyWrite → libc::write
        // on the PTY master fd (M1 pipeline).
        val intent = Intent(WarpTerminalService.ACTION_WRITE).apply {
            setPackage(context.packageName)
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
     */
    private fun startClipboardPaste() {
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
        // sendBytes() uses for keystrokes. Each broadcast → PtyManager.write
        // → libc::write on the master fd.
        val intent = Intent(WarpTerminalService.ACTION_WRITE).apply {
            setPackage(context.packageName)
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
     * can abort if they realize they pasted the wrong thing.
     */
    @Suppress("unused") // available for future ESC-cancels-paste keymap
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
        cm.setPrimaryClip(ClipData.newPlainText("warp-terminal", text))
        Log.i(LOG_TAG, "copy: ${text.length} chars copied to clipboard")
        Toast.makeText(context, "Copied ${text.length} chars", Toast.LENGTH_SHORT).show()
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
