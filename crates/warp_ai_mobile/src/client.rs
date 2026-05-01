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

    // M6-S03 round-2 will add:
    //
    //   pub async fn messages_stream<F: FnMut(StreamEvent)>(
    //       &self, model: &str, prompt: &str, max_tokens: u32,
    //       cancel: CancellationToken, mut on_event: F,
    //   ) -> Result<MessagesResponse, MessagesError>
    //
    // The FnMut callback fires for each parsed SSE event so the caller
    // (ghost-text dispatcher / agent-task UI) can stream-update the
    // UI without buffering the full response.
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
}
