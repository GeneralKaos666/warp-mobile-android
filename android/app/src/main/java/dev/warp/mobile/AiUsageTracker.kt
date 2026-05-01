package dev.warp.mobile

import android.content.Context
import android.util.Log
import java.io.File
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.concurrent.atomic.AtomicLong
import org.json.JSONObject

/**
 * M6-S06: per-session token usage tracker + opt-in CSV log.
 *
 * Tracks cumulative input/output token counts for the current process
 * lifetime. SettingsActivity reads these counters to show "Cumulative
 * tokens this session" in the cost-warning footer.
 *
 * Persistence: per-request rows are appended to
 *   $PREFIX/var/log/warp-ai-usage.csv
 * iff the M4-S05-extracted bootstrap is present (ie. usr/var/log
 * exists). Without that path we silently skip the disk log — the
 * in-memory counters still work for the SettingsActivity display.
 *
 * Extraction from response: Anthropic non-streaming responses include
 *   { "usage": { "input_tokens": N, "output_tokens": M } }
 * Streaming responses send a final `message_delta` event with usage.
 * For round-1 we extract the count from the FULL non-streaming
 * response body (the synchronous Java AnthropicClient.testConnection
 * path) + the assembled streaming response from messages_stream.
 *
 * NOT thread-safe with regard to disk writes — multiple concurrent
 * ghost-text streams could interleave CSV rows. Acceptable: rows are
 * single-line; worst case is mid-line garbling, not data corruption.
 * AtomicLong counters ARE thread-safe.
 */
object AiUsageTracker {
    private const val LOG_TAG = "WarpAiUsage"
    private const val CSV_FILENAME = "warp-ai-usage.csv"

    private val sessionInputTokens = AtomicLong(0)
    private val sessionOutputTokens = AtomicLong(0)
    private val sessionGhostCalls = AtomicLong(0)
    private val sessionAgentCalls = AtomicLong(0)
    /** Latency p95 sentinels: rolling list of last 100 latencies per kind. */
    private val ghostLatencies = ArrayDeque<Long>()
    private val agentLatencies = ArrayDeque<Long>()

    /**
     * Lock for CSV append + cross-thread mutex so concurrent ghost-text
     * + agent record() calls don't interleave half-rows. Round-3 review
     * MEDIUM #5 — `File.appendText` is not atomic.
     */
    private val csvLock = Any()

    /**
     * Record one completed AI call. `kind` = "ghost" or "agent" (matches
     * the M6-S06 AC token-cap split). `inputTokens` / `outputTokens` come
     * from Anthropic's `usage` field; pass 0 if the response didn't
     * include usage (some streaming intermediate states).
     *
     * Updates in-memory counters AND appends a CSV row (if log path
     * available).
     */
    fun record(
        context: Context,
        kind: String,
        model: String,
        inputTokens: Int,
        outputTokens: Int,
        latencyMs: Long
    ) {
        sessionInputTokens.addAndGet(inputTokens.toLong())
        sessionOutputTokens.addAndGet(outputTokens.toLong())
        when (kind) {
            "ghost" -> {
                sessionGhostCalls.incrementAndGet()
                synchronized(ghostLatencies) {
                    ghostLatencies.addLast(latencyMs)
                    while (ghostLatencies.size > 100) ghostLatencies.removeFirst()
                }
            }
            "agent" -> {
                sessionAgentCalls.incrementAndGet()
                synchronized(agentLatencies) {
                    agentLatencies.addLast(latencyMs)
                    while (agentLatencies.size > 100) agentLatencies.removeFirst()
                }
            }
        }
        appendCsvRow(context, kind, model, inputTokens, outputTokens, latencyMs)

        // Token-cap warning per ralplan §6 M6 #4 (ghost ≤200; agent
        // ≤2000). The output-token check is fixed-cap; the latency
        // figure is logged for context (rolling p95 over last 100).
        // Round-3 review MEDIUM #4: the percentile() read needs the
        // same synchronized() block as the write path.
        val p95 = when (kind) {
            "ghost" -> synchronized(ghostLatencies) { percentile(ghostLatencies, 0.95) }
            "agent" -> synchronized(agentLatencies) { percentile(agentLatencies, 0.95) }
            else -> 0L
        }
        val tokenCap = if (kind == "ghost") 200 else 2000
        if (outputTokens > tokenCap * 1.5) {
            // 50% above cap → log a warning so a future telemetry
            // sweep can flag run-aways.
            Log.w(
                LOG_TAG,
                "token cap exceeded: kind=$kind output=$outputTokens cap=$tokenCap p95latency=$p95"
            )
        }
    }

    /**
     * Try to extract `usage.input_tokens` + `usage.output_tokens` from
     * a non-streaming Anthropic response body. Returns (input, output)
     * pair; (0, 0) on parse failure (silent — caller proceeds without
     * recording).
     */
    fun parseUsageFromBody(body: String): Pair<Int, Int> {
        return try {
            val usage = JSONObject(body).optJSONObject("usage") ?: return 0 to 0
            val input = usage.optInt("input_tokens", 0)
            val output = usage.optInt("output_tokens", 0)
            input to output
        } catch (_: Throwable) {
            0 to 0
        }
    }

    /** Snapshot of cumulative session counters for SettingsActivity display. */
    data class Snapshot(
        val ghostCalls: Long,
        val agentCalls: Long,
        val inputTokens: Long,
        val outputTokens: Long,
        val ghostP95Ms: Long,
        val agentP95Ms: Long,
    )

    fun snapshot(): Snapshot {
        val gp95 = synchronized(ghostLatencies) { percentile(ghostLatencies, 0.95) }
        val ap95 = synchronized(agentLatencies) { percentile(agentLatencies, 0.95) }
        return Snapshot(
            ghostCalls = sessionGhostCalls.get(),
            agentCalls = sessionAgentCalls.get(),
            inputTokens = sessionInputTokens.get(),
            outputTokens = sessionOutputTokens.get(),
            ghostP95Ms = gp95,
            agentP95Ms = ap95,
        )
    }

    /** Reset session counters (UI button). Does NOT truncate CSV. */
    fun resetSession() {
        sessionInputTokens.set(0)
        sessionOutputTokens.set(0)
        sessionGhostCalls.set(0)
        sessionAgentCalls.set(0)
        synchronized(ghostLatencies) { ghostLatencies.clear() }
        synchronized(agentLatencies) { agentLatencies.clear() }
    }

    private fun appendCsvRow(
        context: Context,
        kind: String,
        model: String,
        inputTokens: Int,
        outputTokens: Int,
        latencyMs: Long
    ) {
        // M4-S05 extracted bootstrap puts $PREFIX at this path. If
        // bootstrap hasn't run (M4-S05 not done OR cleared), skip.
        val prefix = "${context.applicationInfo.dataDir}/files/usr"
        val logDir = File("$prefix/var/log")
        if (!logDir.isDirectory) {
            // No usable disk log; in-memory counters still useful.
            return
        }
        // Round-3 MEDIUM #5: serialize concurrent appends so two ghost
        // streams completing within the same millisecond don't interleave
        // partial rows. The lock is process-local (a single-process app)
        // so this is effectively zero contention on the hot path.
        synchronized(csvLock) {
            try {
                val csv = File(logDir, CSV_FILENAME)
                val isNew = !csv.exists()
                csv.appendText(
                    buildString {
                        if (isNew) {
                            append("# Warp AI usage log (M6-S06)\n")
                            append("# timestamp,kind,model,input_tokens,output_tokens,latency_ms\n")
                        }
                        append(timestampUtc())
                        append(',').append(kind)
                        append(',').append(model)
                        append(',').append(inputTokens)
                        append(',').append(outputTokens)
                        append(',').append(latencyMs)
                        append('\n')
                    }
                )
            } catch (e: Throwable) {
                Log.w(LOG_TAG, "CSV append failed: ${e.message}")
            }
        }
    }

    private fun timestampUtc(): String =
        SimpleDateFormat("yyyy-MM-dd'T'HH:mm:ss'Z'", Locale.US).apply {
            timeZone = java.util.TimeZone.getTimeZone("UTC")
        }.format(Date())

    private fun percentile(samples: ArrayDeque<Long>, p: Double): Long {
        if (samples.isEmpty()) return 0L
        val sorted = samples.toLongArray().also { it.sort() }
        val idx = ((sorted.size - 1) * p).toInt().coerceIn(0, sorted.size - 1)
        return sorted[idx]
    }
}
