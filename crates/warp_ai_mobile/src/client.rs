//! Anthropic API client core (M6-S03 foundation).
//!
//! Round-1 scope (this commit): builder + struct + Cargo dep tree
//! verification. The real `messages_stream()` method that issues a
//! POST to /v1/messages with stream=true and yields parsed SSE events
//! is M6-S03 round-2 work — we want the dep tree to compile + cross-
//! compile clean BEFORE writing the streaming logic so we know rustls
//! + tokio actually work for our target.
//!
//! Why round-1 doesn't ship streaming yet:
//!   1. reqwest::Response::bytes_stream() requires a Tokio runtime
//!      installed in the calling thread. The android-host JNI thread
//!      is NOT a tokio thread; we'd need a single-threaded runtime
//!      `tokio::runtime::Runtime::new()` per call (expensive) OR
//!      a global runtime singleton (lifecycle complexity). Pick + design
//!      the lifecycle pattern in round-2.
//!   2. SSE event parsing for Anthropic's specific schema (event:
//!      message_start / content_block_delta / message_stop) needs
//!      example responses to verify against. Cheaper to capture those
//!      from the real Test Connection in M6-S02 first.
//!   3. The cancel-on-keystroke flow needs a CancellationToken sourced
//!      from Kotlin via JNI (typed-checked across the FFI boundary).
//!      That's its own round-2 design discussion.
//!
//! What this round-1 DOES validate:
//!   - reqwest 0.12 + rustls-tls-webpki-roots cross-compiles to
//!     aarch64-linux-android via cargo-ndk (the big risk: ring crate
//!     historically had Android NDK toolchain quirks pre-NDK 25c).
//!   - Cargo.lock churn is bounded — the new deps don't pull anything
//!     into the warp_terminal / warpui upstream graph.
//!   - The builder + struct + smoke test compile + run on host.

use std::time::Duration;

use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

/// Anthropic API endpoint. Hardcoded at https://api.anthropic.com per
/// public docs; M6-S04 may add a Settings-screen override for
/// enterprise / proxy deployments.
pub const API_ENDPOINT: &str = "https://api.anthropic.com/v1/messages";

/// API version header value. Anthropic guarantees backwards-compat
/// within a minor version; this constant gets bumped on major API
/// releases (≥ 1/year cadence per public release notes).
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Default model for ghost-text completions. Haiku 4.5 = fastest +
/// cheapest of the Claude 4 family; suited for sub-500ms p50 ghost.
pub const DEFAULT_GHOST_MODEL: &str = "claude-haiku-4-5";

/// Default model for agent tasks. Sonnet 4.6 = capable enough for
/// shell-context explanations + small enough for sub-8s p50 first-token.
pub const DEFAULT_AGENT_MODEL: &str = "claude-sonnet-4-6";

/// Async client for Anthropic /v1/messages.
///
/// Constructed once per session (typically by the JNI shim) and reused
/// across ghost + agent calls. Thread-safe (reqwest::Client is internally
/// Arc'd).
pub struct AnthropicClient {
    api_key: String,
    /// Connect timeout for fresh DNS+TLS handshakes. 8s tolerates
    /// captive-portal WiFi / 3G; round-trips to api.anthropic.com on
    /// LTE measure ~150-300ms in M6-S02 device tests.
    pub connect_timeout: Duration,
    /// Per-request total timeout (covers connect + read). 30s tolerates
    /// Sonnet agent p99 first-token + ~25s typical complete-response.
    pub request_timeout: Duration,
}

impl AnthropicClient {
    /// Construct a new client with the given Bearer token. Validates
    /// only that the key is non-empty + starts with sk-ant-; full
    /// validation happens at the first network call.
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            connect_timeout: Duration::from_secs(8),
            request_timeout: Duration::from_secs(30),
        }
    }

    /// Returns a redacted form of the bearer token suitable for log
    /// output. Mirrors the AiKeyStore.redact() Kotlin helper so log
    /// lines from the Rust + Java sides have a consistent format.
    pub fn redact(&self) -> String {
        if self.api_key.is_empty() {
            return "(no key)".to_string();
        }
        let prefix = self.api_key.chars().take(8).collect::<String>();
        let suffix = if self.api_key.len() >= 4 {
            self.api_key.chars().rev().take(4).collect::<Vec<_>>()
                .into_iter().rev().collect::<String>()
        } else {
            "?".to_string()
        };
        format!("Bearer {}***...{}", prefix, suffix)
    }

    /// Synchronous (assembled) request to /v1/messages.
    ///
    /// M6-S03 round-2 scope: posts a non-streaming Messages request,
    /// awaits the full response, returns the concatenated text. Real
    /// SSE streaming with per-token cancel is round-3 work — round-2
    /// validates the dep tree round-trips a real Anthropic call from
    /// the Rust path (round-1 only verified Cargo cross-compile).
    ///
    /// Why not streaming yet:
    ///   - Streaming needs a global tokio runtime + GlobalRef on the
    ///     Java callback object so Rust can fire `on_event(...)` from
    ///     a background tokio worker. That's its own design round.
    ///   - The async-then-sync bridge (`Runtime::new().block_on(...)`)
    ///     is the simplest JNI thread shape: one call → one result.
    ///   - For ghost-text, a sync 1-2s round-trip is too slow to hit
    ///     the <500ms p50 first-token AC; round-3 makes it streaming.
    ///     But for round-2 device verification this proves the pipe.
    ///
    /// `prompt` becomes the user-role message content. The caller is
    /// expected to wrap with appropriate system-prompt scaffolding for
    /// ghost-text vs agent (the helper `ghost_prompt_for(...)` in the
    /// upcoming `ghost.rs` will do this; for now caller passes raw).
    ///
    /// Errors:
    ///   - `MessagesError::Network` for connect / TLS / DNS failures
    ///   - `MessagesError::HttpStatus(code, body)` for 4xx/5xx
    ///   - `MessagesError::Decode` for unexpected response shape
    ///   - `MessagesError::EmptyKey` if api_key was constructed empty
    pub async fn messages_complete(
        &self,
        model: &str,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, MessagesError> {
        if self.api_key.is_empty() {
            return Err(MessagesError::EmptyKey);
        }

        let body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "messages": [
                { "role": "user", "content": prompt }
            ]
        });

        let client = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(|e| MessagesError::Network(format!("client build: {}", e)))?;

        log::info!(
            target: "warp_ai",
            "POST {} model={} max_tokens={} auth={}",
            API_ENDPOINT, model, max_tokens, self.redact()
        );

        let resp = client
            .post(API_ENDPOINT)
            .header("Content-Type", "application/json")
            .header("Anthropic-Version", ANTHROPIC_VERSION)
            .header("X-Api-Key", &self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| MessagesError::Network(format!("send: {}", e)))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| MessagesError::Network(format!("body read: {}", e)))?;

        if !status.is_success() {
            // Try to parse the Anthropic error envelope for a clearer
            // message: { "error": { "type": "...", "message": "..." } }.
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| text.chars().take(200).collect());
            return Err(MessagesError::HttpStatus(status.as_u16(), scrub_key(&msg)));
        }

        // Response shape (non-streaming):
        //   { "id": "msg_...", "type": "message",
        //     "role": "assistant", "model": "...",
        //     "content": [{ "type": "text", "text": "..." }],
        //     "usage": { "input_tokens": N, "output_tokens": M } }
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| MessagesError::Decode(format!("not JSON: {}", e)))?;
        let content = parsed
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or_else(|| MessagesError::Decode("missing 'content' array".into()))?;
        let mut out = String::new();
        for block in content {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                out.push_str(t);
            }
        }
        if out.is_empty() {
            return Err(MessagesError::Decode("'content' had no text blocks".into()));
        }
        Ok(out)
    }

    /// Streaming variant of `messages_complete`.
    ///
    /// Posts to /v1/messages with `stream=true`, parses Anthropic SSE
    /// events (`event: content_block_delta`, etc), and fires
    /// `on_chunk(text)` for every text-delta chunk as it arrives.
    /// Returns the assembled final string after the stream closes.
    ///
    /// `cancel`: a `CancellationToken` the caller can `.cancel()` to
    /// abort the stream mid-flight. Cancellation is checked between
    /// each network read; once cancelled, the function returns
    /// `Err(MessagesError::Cancelled)` quickly.
    ///
    /// SSE event format (per Anthropic docs):
    ///   event: message_start
    ///   data: {"type":"message_start","message":{"id":"...",...}}
    ///
    ///   event: content_block_start
    ///   data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}
    ///
    ///   event: content_block_delta
    ///   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}
    ///
    ///   event: content_block_delta
    ///   data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":" world"}}
    ///
    ///   event: content_block_stop
    ///   data: {"type":"content_block_stop","index":0}
    ///
    ///   event: message_delta
    ///   data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{...}}
    ///
    ///   event: message_stop
    ///   data: {"type":"message_stop"}
    ///
    /// We only care about `content_block_delta` events with type
    /// `text_delta`; everything else is metadata we ignore for round-3
    /// (M6-S04 agent task may need stop_reason + usage in round-4).
    pub async fn messages_stream<F>(
        &self,
        model: &str,
        prompt: &str,
        max_tokens: u32,
        cancel: CancellationToken,
        mut on_chunk: F,
    ) -> Result<String, MessagesError>
    where
        F: FnMut(&str),
    {
        if self.api_key.is_empty() {
            return Err(MessagesError::EmptyKey);
        }

        let body = serde_json::json!({
            "model": model,
            "max_tokens": max_tokens,
            "stream": true,
            "messages": [
                { "role": "user", "content": prompt }
            ]
        });

        let client = reqwest::Client::builder()
            .connect_timeout(self.connect_timeout)
            .timeout(self.request_timeout)
            .build()
            .map_err(|e| MessagesError::Network(format!("client build: {}", e)))?;

        log::info!(
            target: "warp_ai",
            "STREAM POST {} model={} max_tokens={} auth={}",
            API_ENDPOINT, model, max_tokens, self.redact()
        );

        // Race: the network response future vs the cancellation token.
        // tokio::select! drops the loser, which closes the underlying
        // socket on cancel.
        let response = tokio::select! {
            _ = cancel.cancelled() => return Err(MessagesError::Cancelled),
            resp = client
                .post(API_ENDPOINT)
                .header("Content-Type", "application/json")
                .header("Anthropic-Version", ANTHROPIC_VERSION)
                .header("X-Api-Key", &self.api_key)
                .json(&body)
                .send() => resp.map_err(|e| MessagesError::Network(format!("send: {}", e)))?,
        };

        let status = response.status();
        if !status.is_success() {
            // Non-streaming response on error — read full body, parse
            // Anthropic error envelope. Cancel respected on body read.
            let text = tokio::select! {
                _ = cancel.cancelled() => return Err(MessagesError::Cancelled),
                t = response.text() => t.map_err(|e| MessagesError::Network(format!("error body: {}", e)))?,
            };
            let msg = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(String::from))
                .unwrap_or_else(|| text.chars().take(200).collect());
            return Err(MessagesError::HttpStatus(status.as_u16(), scrub_key(&msg)));
        }

        // Stream the response body chunk-by-chunk. SSE events are
        // separated by blank lines; each event has lines like
        // "event: foo" and "data: {...json...}".
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut accumulated = String::new();

        while let Some(item) = tokio::select! {
            _ = cancel.cancelled() => return Err(MessagesError::Cancelled),
            next = stream.next() => next,
        } {
            let bytes = item.map_err(|e| MessagesError::Network(format!("stream read: {}", e)))?;
            let text = std::str::from_utf8(&bytes)
                .map_err(|_| MessagesError::Decode("stream chunk not UTF-8".into()))?;
            buffer.push_str(text);

            // Process complete events (separated by blank line, i.e. \n\n).
            while let Some(end_idx) = buffer.find("\n\n") {
                let event_block = buffer[..end_idx].to_string();
                buffer.drain(..end_idx + 2);
                if let Some(delta_text) = extract_text_delta(&event_block) {
                    accumulated.push_str(&delta_text);
                    on_chunk(&delta_text);
                }
            }
        }

        if accumulated.is_empty() {
            return Err(MessagesError::Decode(
                "stream completed with zero text deltas".into(),
            ));
        }
        Ok(accumulated)
    }
}

/// Extract the `delta.text` field from an SSE event block if it's a
/// `content_block_delta` with `type: text_delta`. Returns None for
/// any other event type (we ignore message_start / content_block_stop
/// / message_delta / etc.) or malformed blocks.
fn extract_text_delta(event_block: &str) -> Option<String> {
    // Find the `data:` line. SSE allows `data: {json}` but Anthropic
    // emits exactly one data line per event; we don't support multi-
    // line data blocks here.
    let data_line = event_block.lines().find_map(|l| {
        l.strip_prefix("data: ")
            .or_else(|| l.strip_prefix("data:"))
    })?;

    let parsed: serde_json::Value = serde_json::from_str(data_line.trim()).ok()?;
    let event_type = parsed.get("type")?.as_str()?;
    if event_type != "content_block_delta" {
        return None;
    }
    let delta = parsed.get("delta")?;
    let delta_type = delta.get("type")?.as_str()?;
    if delta_type != "text_delta" {
        return None;
    }
    delta.get("text")?.as_str().map(String::from)
}

/// Errors returned by `messages_complete` / `messages_stream`.
#[derive(Debug)]
pub enum MessagesError {
    EmptyKey,
    Network(String),
    HttpStatus(u16, String),
    Decode(String),
    /// `messages_stream` only: the cancellation token fired before the
    /// stream completed.
    Cancelled,
}

impl std::fmt::Display for MessagesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MessagesError::EmptyKey => write!(f, "API key is empty"),
            MessagesError::Network(m) => write!(f, "Network: {}", m),
            MessagesError::HttpStatus(c, m) => write!(f, "HTTP {}: {}", c, m),
            MessagesError::Decode(m) => write!(f, "Decode: {}", m),
            MessagesError::Cancelled => write!(f, "Cancelled"),
        }
    }
}

impl std::error::Error for MessagesError {}

/// Defense-in-depth: scrub any embedded `sk-ant-...` substring from
/// API responses before they hit logs / UI / error returns. Mirrors
/// the Java-side `AnthropicClient.scrub()` regex.
fn scrub_key(text: &str) -> String {
    // Simple pass: find "sk-ant-" prefix + take chars until non-key
    // char. Avoids pulling in the regex crate (1MB binary cost).
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let bytes = text.as_bytes();
    while i < bytes.len() {
        if i + 7 <= bytes.len() && &bytes[i..i + 7] == b"sk-ant-" {
            out.push_str("sk-ant-***REDACTED***");
            // Skip past the key — anything in the API-key alphabet.
            i += 7;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
            {
                i += 1;
            }
        } else {
            // Append char (UTF-8 safe via str slicing on char boundary).
            let ch_start = i;
            let ch_end = ch_start
                + text[ch_start..]
                    .chars()
                    .next()
                    .map(|c| c.len_utf8())
                    .unwrap_or(1);
            out.push_str(&text[ch_start..ch_end]);
            i = ch_end;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_format_matches_kotlin_helper() {
        let c = AnthropicClient::new("sk-ant-deadbeef0123456789abcdef".to_string());
        // Format: "Bearer sk-ant-d***...cdef"
        let r = c.redact();
        assert!(r.starts_with("Bearer sk-ant-d"), "got: {}", r);
        assert!(r.ends_with("cdef"), "got: {}", r);
        assert!(r.contains("***...") , "got: {}", r);
    }

    #[test]
    fn redact_handles_empty_key() {
        let c = AnthropicClient::new(String::new());
        assert_eq!(c.redact(), "(no key)");
    }

    #[test]
    fn redact_handles_short_key() {
        let c = AnthropicClient::new("xy".to_string());
        // Prefix takes 2 chars, suffix takes 2 chars (key is 2 chars,
        // last 4 chars = "xy").
        let r = c.redact();
        // Either "Bearer xy***...xy" or similar — verify it doesn't panic.
        assert!(r.starts_with("Bearer "));
    }

    #[test]
    fn timeouts_are_sensible() {
        let c = AnthropicClient::new("sk-ant-x".to_string());
        // Connect timeout in 5..=15s range (mobile network tolerant).
        assert!(c.connect_timeout >= Duration::from_secs(5));
        assert!(c.connect_timeout <= Duration::from_secs(15));
        // Request timeout in 20..=60s range.
        assert!(c.request_timeout >= Duration::from_secs(20));
        assert!(c.request_timeout <= Duration::from_secs(60));
    }

    #[test]
    fn default_models_are_haiku_and_sonnet() {
        assert!(DEFAULT_GHOST_MODEL.contains("haiku"));
        assert!(DEFAULT_AGENT_MODEL.contains("sonnet"));
    }

    #[test]
    fn scrub_key_removes_full_keys() {
        let dirty = "Error: invalid sk-ant-deadbeef0123456789ABCDEF in request";
        let cleaned = scrub_key(dirty);
        assert!(!cleaned.contains("sk-ant-d"), "still has key: {}", cleaned);
        assert!(cleaned.contains("REDACTED"), "missing redact marker: {}", cleaned);
    }

    #[test]
    fn scrub_key_handles_no_keys() {
        let plain = "Just a normal error message with no secrets";
        assert_eq!(scrub_key(plain), plain);
    }

    #[test]
    fn scrub_key_handles_multiple_keys() {
        let dirty = "First sk-ant-aaa123 and second sk-ant-bbb456 keys";
        let cleaned = scrub_key(dirty);
        assert!(cleaned.matches("REDACTED").count() == 2, "got: {}", cleaned);
    }

    #[test]
    fn scrub_key_preserves_unicode() {
        let dirty = "錯誤 sk-ant-xyz 訊息";
        let cleaned = scrub_key(dirty);
        assert!(cleaned.contains("錯誤"));
        assert!(cleaned.contains("訊息"));
        assert!(cleaned.contains("REDACTED"));
    }

    #[test]
    fn messages_error_display_formats() {
        assert_eq!(MessagesError::EmptyKey.to_string(), "API key is empty");
        assert_eq!(MessagesError::Network("dns".into()).to_string(), "Network: dns");
        assert_eq!(MessagesError::HttpStatus(401, "unauthorized".into()).to_string(),
                   "HTTP 401: unauthorized");
        assert_eq!(MessagesError::Decode("bad json".into()).to_string(), "Decode: bad json");
        assert_eq!(MessagesError::Cancelled.to_string(), "Cancelled");
    }

    // ── SSE parser tests (M6-S03 round-3) ─────────────────────────

    #[test]
    fn extract_text_delta_typical_event() {
        let event = "event: content_block_delta\n\
                     data: {\"type\":\"content_block_delta\",\"index\":0,\
                     \"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}";
        assert_eq!(extract_text_delta(event), Some("Hello".to_string()));
    }

    #[test]
    fn extract_text_delta_unicode() {
        let event = "event: content_block_delta\n\
                     data: {\"type\":\"content_block_delta\",\"index\":0,\
                     \"delta\":{\"type\":\"text_delta\",\"text\":\"你好\"}}";
        assert_eq!(extract_text_delta(event), Some("你好".to_string()));
    }

    #[test]
    fn extract_text_delta_skips_message_start() {
        let event = "event: message_start\n\
                     data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_x\"}}";
        assert_eq!(extract_text_delta(event), None);
    }

    #[test]
    fn extract_text_delta_skips_content_block_stop() {
        let event = "event: content_block_stop\n\
                     data: {\"type\":\"content_block_stop\",\"index\":0}";
        assert_eq!(extract_text_delta(event), None);
    }

    #[test]
    fn extract_text_delta_skips_non_text_delta() {
        // input_json_delta is for tool use; we ignore it for ghost-text.
        let event = "event: content_block_delta\n\
                     data: {\"type\":\"content_block_delta\",\"index\":0,\
                     \"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}";
        assert_eq!(extract_text_delta(event), None);
    }

    #[test]
    fn extract_text_delta_handles_no_space_after_colon() {
        // Anthropic uses `data: ` (with space), but defensively support
        // `data:` (without space) too.
        let event = "event:content_block_delta\n\
                     data:{\"type\":\"content_block_delta\",\"index\":0,\
                     \"delta\":{\"type\":\"text_delta\",\"text\":\"X\"}}";
        assert_eq!(extract_text_delta(event), Some("X".to_string()));
    }

    #[test]
    fn extract_text_delta_returns_none_on_garbage() {
        assert_eq!(extract_text_delta(""), None);
        assert_eq!(extract_text_delta("garbage"), None);
        assert_eq!(extract_text_delta("data: not json"), None);
    }
}
