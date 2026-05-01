package dev.warp.mobile

import android.util.Log
import org.json.JSONObject
import java.io.BufferedReader
import java.io.InputStreamReader
import java.net.HttpURLConnection
import java.net.URL
import java.nio.charset.StandardCharsets

/**
 * M6-S02: minimal Anthropic API client (BYOK + Test Connection).
 *
 * Round-1 scope: blocking HTTPS POST via java.net.HttpsURLConnection
 * (no streaming, no Rust). Sufficient for the SettingsActivity Test
 * Connection button which just needs a 1-token completion to validate
 * the API key.
 *
 * M6-S03/S04 will add the Rust async streaming layer (reqwest +
 * rustls-tls + tokio) for ghost-text + agent paths where SSE
 * streaming + cancel-on-keystroke matter.
 *
 * Refs:
 *   https://docs.claude.com/en/api/messages
 *   https://docs.claude.com/en/api/errors
 */
object AnthropicClient {
    private const val LOG_TAG = "WarpAiClient"
    private const val ENDPOINT = "https://api.anthropic.com/v1/messages"
    private const val ANTHROPIC_VERSION = "2023-06-01"
    /**
     * Connect timeout (TLS + DNS handshake). 8s tolerates slow mobile
     * networks while still failing fast on persistent DNS issues.
     */
    private const val CONNECT_TIMEOUT_MS = 8_000
    /**
     * Read timeout. 1-token Haiku completion is typically <500ms p50;
     * 12s tolerates p99 + network-edge cases.
     */
    private const val READ_TIMEOUT_MS = 12_000

    /** Result of a Test Connection probe. */
    sealed class TestResult {
        data class Ok(val responseText: String, val latencyMs: Long) : TestResult()
        /** HTTP 4xx/5xx with parsed error message from the API body. */
        data class HttpError(val code: Int, val message: String) : TestResult()
        /** Network / TLS / DNS error. */
        data class NetworkError(val message: String) : TestResult()
        /** API key empty or obvious format mismatch. */
        data object MissingKey : TestResult()
    }

    /**
     * Synchronous (blocking) call. MUST be invoked from a background
     * thread (Dispatchers.IO). The SettingsActivity test button uses a
     * coroutine for this.
     *
     * Sends a minimal Haiku request:
     *   POST /v1/messages
     *   { "model": "claude-haiku-4-5", "max_tokens": 1,
     *     "messages": [{"role":"user","content":"hi"}] }
     *
     * Returns Ok if the response is HTTP 200 + parseable JSON with a
     * `content` field. Otherwise returns the appropriate error variant.
     *
     * Note: the model `claude-haiku-4-5` is from the latest Anthropic
     * model family at session knowledge cutoff. If a future API
     * deprecates it, the test will return HttpError(400) with the
     * Anthropic error body — clear signal to update the constant.
     */
    fun testConnection(apiKey: String?): TestResult {
        if (apiKey.isNullOrBlank()) return TestResult.MissingKey
        if (!apiKey.startsWith("sk-ant-")) {
            // Anthropic keys always start with "sk-ant-" — reject obvious
            // typos before the 8s network round-trip. The user can still
            // hit the API with a malformed key if they really want; the
            // server will return 401.
            return TestResult.HttpError(400, "Key doesn't start with 'sk-ant-' — looks malformed")
        }

        val body = JSONObject().apply {
            put("model", "claude-haiku-4-5")
            put("max_tokens", 1)
            put("messages", org.json.JSONArray().apply {
                put(JSONObject().apply {
                    put("role", "user")
                    put("content", "hi")
                })
            })
        }.toString()

        val url = URL(ENDPOINT)
        val t0 = System.currentTimeMillis()
        val conn = url.openConnection() as HttpURLConnection
        try {
            conn.requestMethod = "POST"
            conn.connectTimeout = CONNECT_TIMEOUT_MS
            conn.readTimeout = READ_TIMEOUT_MS
            conn.doOutput = true
            conn.setRequestProperty("Content-Type", "application/json")
            conn.setRequestProperty("Anthropic-Version", ANTHROPIC_VERSION)
            // CRITICAL: log only the redacted form. AiKeyStore.redact()
            // shows `Bearer sk-ant-***...XXXX` so the full key never
            // appears in logcat / bug reports.
            conn.setRequestProperty("X-Api-Key", apiKey)
            Log.i(LOG_TAG, "POST $ENDPOINT auth=${AiKeyStore.redact(apiKey)} body_len=${body.length}")

            conn.outputStream.use { it.write(body.toByteArray(StandardCharsets.UTF_8)) }

            val code = conn.responseCode
            val stream = if (code in 200..299) conn.inputStream else conn.errorStream
            val responseText = stream?.let {
                BufferedReader(InputStreamReader(it, StandardCharsets.UTF_8)).use { r -> r.readText() }
            } ?: ""
            val elapsed = System.currentTimeMillis() - t0
            Log.i(LOG_TAG, "response code=$code elapsedMs=$elapsed body_len=${responseText.length}")

            if (code !in 200..299) {
                // Try to parse the Anthropic error envelope for a clearer
                // message: { "type": "error", "error": { "type": "...",
                // "message": "..." } }
                val msg = try {
                    JSONObject(responseText).optJSONObject("error")?.optString("message", responseText)
                        ?: responseText
                } catch (_: Throwable) {
                    responseText.take(200)
                }
                return TestResult.HttpError(code, msg)
            }

            // Validate response shape: top-level `content` array with at
            // least one `text` entry.
            val parsed = try {
                JSONObject(responseText)
            } catch (e: Throwable) {
                return TestResult.HttpError(code, "200 OK but response not JSON: ${e.message}")
            }
            val content = parsed.optJSONArray("content")
            if (content == null || content.length() == 0) {
                return TestResult.HttpError(code, "200 OK but missing 'content' field")
            }
            val firstText = content.optJSONObject(0)?.optString("text", "") ?: ""
            return TestResult.Ok(firstText, elapsed)
        } catch (e: Throwable) {
            Log.e(LOG_TAG, "testConnection threw: ${e.javaClass.simpleName}: ${e.message}")
            return TestResult.NetworkError("${e.javaClass.simpleName}: ${e.message ?: "(unknown)"}")
        } finally {
            try { conn.disconnect() } catch (_: Throwable) { /* best effort */ }
        }
    }
}
