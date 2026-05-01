package dev.warp.mobile

import android.app.Activity
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
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.GlobalScope
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
class SettingsActivity : Activity() {
    private val LOG_TAG = "WarpSettings"
    private lateinit var keyInput: EditText
    private lateinit var statusText: TextView

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
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
        @OptIn(kotlinx.coroutines.DelicateCoroutinesApi::class)
        GlobalScope.launch(Dispatchers.IO) {
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

        // Cost-warning footer per Death-pit #3 in M6-kickoff-confirmed.md
        val costWarning = TextView(this).apply {
            text = "Costs (Anthropic public pricing 2026-Q2):\n" +
                "  Ghost-text via Haiku: ~\$0.005 per completion\n" +
                "  Agent task via Sonnet: ~\$0.05 per task\n" +
                "Heavy use can cost \$1-5/day. Token usage tracked\n" +
                "in M6-S06 (cumulative this session shown here)."
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setTextColor(0xFF888888.toInt())
            setPadding(0, dp(24), 0, 0)
        }
        root.addView(costWarning, lpMatchWrap())

        setContentView(root)
    }

    private fun onSave() {
        val key = keyInput.text.toString().trim()
        if (key.isEmpty()) {
            setStatus("Empty key — nothing saved")
            return
        }
        @OptIn(kotlinx.coroutines.DelicateCoroutinesApi::class)
        GlobalScope.launch(Dispatchers.IO) {
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
        @OptIn(kotlinx.coroutines.DelicateCoroutinesApi::class)
        GlobalScope.launch(Dispatchers.IO) {
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
        setStatus("Testing /v1/messages with model claude-haiku-4-5...")
        @OptIn(kotlinx.coroutines.DelicateCoroutinesApi::class)
        GlobalScope.launch(Dispatchers.IO) {
            val result = AnthropicClient.testConnection(key)
            withContext(Dispatchers.Main) {
                val msg = when (result) {
                    is AnthropicClient.TestResult.Ok ->
                        "✓ OK (${result.latencyMs} ms): \"${result.responseText.take(60)}\""
                    is AnthropicClient.TestResult.HttpError ->
                        "✗ HTTP ${result.code}: ${result.message.take(120)}"
                    is AnthropicClient.TestResult.NetworkError ->
                        "✗ Network: ${result.message.take(120)}"
                    AnthropicClient.TestResult.MissingKey ->
                        "✗ Missing or empty key"
                }
                setStatus(msg)
                Log.i(LOG_TAG, "test result: $msg")
            }
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
