package dev.warp.mobile

import android.os.Bundle
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast
import androidx.appcompat.app.AppCompatActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * M6-S02: BYOK SettingsActivity.
 *
 * Minimal one-screen settings UI:
 *   - EditText for the Anthropic API key (password-masked)
 *   - Save button: writes to AiKeyStore (EncryptedSharedPreferences)
 *   - Test Connection button: posts a 1-token Haiku completion to
 *     /v1/messages and surfaces the result (Ok / HttpError(401) /
 *     NetworkError) as a Toast + status TextView
 *   - Clear button: forgets the saved key
 *
 * No XML layouts (zero AndroidX nav / Compose deps). Programmatic
 * LinearLayout vertical for build-cycle simplicity. M6-S04 will add
 * a richer Compose-based settings screen if needed.
 *
 * Trigger: launched via `am start -n dev.warp.mobile/.SettingsActivity`
 * from the launcher. (M6-S03 will add a settings icon overlay in the
 * MainActivity AccessoryRow.)
 */
class SettingsActivity : AppCompatActivity() {
    private val LOG_TAG = "WarpSettings"
    private lateinit var keyInput: EditText
    private lateinit var statusText: TextView
    /** M6-S06: cumulative-tokens display TextView (refreshed after Test). */
    private lateinit var usageText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // M6 round-2 security review HIGH #1: FLAG_SECURE prevents OS
        // screenshots, screen recordings, and cast/mirror surfaces from
        // capturing the API key field. Set BEFORE setContentView so it
        // applies to every frame the window renders.
        // Refs: https://developer.android.com/reference/android/view/WindowManager.LayoutParams#FLAG_SECURE
        window.setFlags(
            WindowManager.LayoutParams.FLAG_SECURE,
            WindowManager.LayoutParams.FLAG_SECURE
        )
        // Don't show keyboard input by default — user can tap to focus.
        window.setSoftInputMode(WindowManager.LayoutParams.SOFT_INPUT_STATE_HIDDEN)

        title = "Warp AI · BYOK Settings"

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(dp(16), dp(16), dp(16), dp(16))
        }

        root.addView(label("Anthropic API key (BYOK)"))
        keyInput = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            transformationMethod = PasswordTransformationMethod.getInstance()
            hint = "sk-ant-..."
            setSingleLine(true)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
        }
        root.addView(keyInput, lpMatchWrap())

        // Load existing key (background thread because Keystore key
        // generation can take ~100-300ms on first call).
        // M6 round-2 code-review HIGH #2: lifecycleScope (not GlobalScope)
        // so the coroutine cancels when SettingsActivity is destroyed.
        // Without this, a background load that completed AFTER user pressed
        // back would write to a destroyed Activity's Views.
        lifecycleScope.launch(Dispatchers.IO) {
            val existing = try {
                AiKeyStore.load(this@SettingsActivity)
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "AiKeyStore load failed: ${e.message}")
                null
            }
            withContext(Dispatchers.Main) {
                if (!existing.isNullOrEmpty()) {
                    keyInput.setText(existing)
                    setStatus("Loaded saved key (${AiKeyStore.redact(existing)})")
                }
            }
        }

        val btnRow = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER
            setPadding(0, dp(12), 0, dp(12))
        }
        btnRow.addView(button("Save") { onSave() }, lpButton())
        btnRow.addView(button("Test") { onTest() }, lpButton())
        btnRow.addView(button("Clear") { onClear() }, lpButton())
        root.addView(btnRow, lpMatchWrap())

        statusText = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            setTextColor(0xFFAAAAAA.toInt())
            setPadding(0, dp(12), 0, dp(12))
        }
        root.addView(statusText, lpMatchWrap())

        // M6-S06 cumulative-tokens display.
        usageText = TextView(this).apply {
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setTextColor(0xFFCCCCCC.toInt())
            setPadding(0, dp(20), 0, dp(8))
            // monospace so the number columns line up
            typeface = android.graphics.Typeface.MONOSPACE
        }
        root.addView(usageText, lpMatchWrap())
        refreshUsageDisplay()

        val resetBtn = button("Reset session counters") {
            AiUsageTracker.resetSession()
            refreshUsageDisplay()
            Toast.makeText(this, "Session counters reset", Toast.LENGTH_SHORT).show()
        }
        root.addView(resetBtn, lpMatchWrap())

        // Cost-warning footer per Death-pit #3 in M6-kickoff-confirmed.md
        val costWarning = TextView(this).apply {
            text = "Costs (Anthropic public pricing 2026-Q2):\n" +
                "  Ghost-text via Haiku: ~\$0.005 per completion\n" +
                "  Agent task via Sonnet: ~\$0.05 per task\n" +
                "Heavy use can cost \$1-5/day."
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(0xFF888888.toInt())
            setPadding(0, dp(20), 0, 0)
        }
        root.addView(costWarning, lpMatchWrap())

        setContentView(root)
    }

    override fun onResume() {
        super.onResume()
        // Refresh in case usage changed via other paths (PTY-side AI
        // calls in AccessoryRow update the same singleton).
        if (::usageText.isInitialized) {
            refreshUsageDisplay()
        }
    }

    private fun onSave() {
        val key = keyInput.text.toString().trim()
        if (key.isEmpty()) {
            setStatus("Empty key — nothing saved")
            return
        }
        lifecycleScope.launch(Dispatchers.IO) {
            try {
                AiKeyStore.save(this@SettingsActivity, key)
                Log.i(LOG_TAG, "Saved key (${AiKeyStore.redact(key)})")
                withContext(Dispatchers.Main) {
                    setStatus("Saved (${AiKeyStore.redact(key)})")
                    Toast.makeText(this@SettingsActivity, "API key saved", Toast.LENGTH_SHORT).show()
                }
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "save failed: ${e.message}")
                withContext(Dispatchers.Main) {
                    setStatus("Save failed: ${e.message}")
                }
            }
        }
    }

    private fun onClear() {
        lifecycleScope.launch(Dispatchers.IO) {
            try {
                AiKeyStore.clear(this@SettingsActivity)
                withContext(Dispatchers.Main) {
                    keyInput.setText("")
                    setStatus("Cleared")
                    Toast.makeText(this@SettingsActivity, "API key cleared", Toast.LENGTH_SHORT).show()
                }
            } catch (e: Throwable) {
                Log.e(LOG_TAG, "clear failed: ${e.message}")
            }
        }
    }

    private fun onTest() {
        val key = keyInput.text.toString().trim()
        if (key.isEmpty()) {
            setStatus("Enter a key before testing")
            return
        }
        // M6-S05: short-circuit when offline.
        if (!AiConnectivity.get(this).isOnline()) {
            setStatus("✗ No network — turn off airplane mode and retry")
            return
        }
        setStatus("Testing /v1/messages with model claude-haiku-4-5...")
        lifecycleScope.launch(Dispatchers.IO) {
            val result = AnthropicClient.testConnection(key)
            withContext(Dispatchers.Main) {
                val msg = when (result) {
                    is AnthropicClient.TestResult.Ok -> {
                        // M6-S06: record telemetry for the Test
                        // Connection call. `kind=ghost` because
                        // Test Connection uses the same Haiku
                        // model + 1-token budget.
                        AiUsageTracker.record(
                            this@SettingsActivity,
                            kind = "ghost",
                            model = "claude-haiku-4-5",
                            inputTokens = result.inputTokens,
                            outputTokens = result.outputTokens,
                            latencyMs = result.latencyMs
                        )
                        "✓ OK (${result.latencyMs} ms, in=${result.inputTokens} out=${result.outputTokens} tokens): \"${result.responseText.take(50)}\""
                    }
                    is AnthropicClient.TestResult.HttpError ->
                        "✗ HTTP ${result.code}: ${result.message.take(120)}"
                    is AnthropicClient.TestResult.NetworkError ->
                        "✗ Network: ${result.message.take(120)}"
                    AnthropicClient.TestResult.MissingKey ->
                        "✗ Missing or empty key"
                }
                setStatus(msg)
                refreshUsageDisplay()
                Log.i(LOG_TAG, "test result: $msg")
            }
        }
    }

    /**
     * M6-S06: update the cumulative-tokens TextView at the bottom of
     * Settings. Called after every successful Test Connection + on
     * activity resume. Reads from AiUsageTracker.snapshot().
     */
    private fun refreshUsageDisplay() {
        val s = AiUsageTracker.snapshot()
        usageText.text = buildString {
            append("Session usage (since launch):\n")
            append("  Ghost calls:  ${s.ghostCalls}  (p95 latency ${s.ghostP95Ms}ms; cap 500ms)\n")
            append("  Agent calls:  ${s.agentCalls}  (p95 latency ${s.agentP95Ms}ms; cap 8000ms)\n")
            append("  Input tokens: ${s.inputTokens}\n")
            append("  Output tokens: ${s.outputTokens}\n")
            append("  Reset session counters with the button below.")
        }
    }

    private fun setStatus(text: String) {
        statusText.text = text
    }

    // ── view helpers ────────────────────────────────────────────────

    private fun label(text: String): TextView = TextView(this).apply {
        this.text = text
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
        setPadding(0, dp(8), 0, dp(4))
    }

    private fun button(text: String, onClick: () -> Unit): Button = Button(this).apply {
        this.text = text
        setOnClickListener { onClick() }
    }

    private fun lpMatchWrap() = LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT,
        ViewGroup.LayoutParams.WRAP_CONTENT
    )

    private fun lpButton() = LinearLayout.LayoutParams(
        0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f
    ).apply { setMargins(dp(4), 0, dp(4), 0) }

    private fun dp(value: Int): Int =
        TypedValue.applyDimension(
            TypedValue.COMPLEX_UNIT_DIP, value.toFloat(),
            resources.displayMetrics
        ).toInt()
}
