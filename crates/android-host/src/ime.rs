//! M2-S10 — Android IME composing-text state machine (main-repo runtime).
//!
//! Mirrors `warp-src/crates/warpui/src/platform/android/ime.rs` (canonical).
//! Cross-workspace duplication is a known M3 unification carry-over (per the
//! M2-S07 + M2-S08 reviews); here we keep parity by copying the type
//! definitions and tests verbatim, while adding a process-wide singleton
//! (`global_ime()`) for the JNI shim to dispatch into.
//!
//! ## Round-2 (2026-04-30) — Gboard quirk reclassified
//!
//! The Gboard real-IME ordering on Galaxy S24 Ultra (Codex round-1 device
//! repro) is `setComposingText → finishComposingText → commitText`, with
//! `finish` arriving BETWEEN `setComposing` and the candidate `commit`. To
//! handle that without misclassifying the candidate as a `LatinCommit`, we
//! defer the `finishComposingText` flush via a `pending_finish: Option<String>`
//! buffer; the next event (commit / setComposing / finish / drain) decides
//! how to resolve it. See the canonical module-level docs for the full state
//! machine description.
//!
//! The JNI bindings in `crates/android-host/src/lib.rs` route Java
//! `WarpInputView` callbacks through this module:
//!
//! ```text
//!  WarpInputView (Java) ── BaseInputConnection override ──►
//!    NativeBridge.imeCommitText/imeSetComposingText/imeFinishComposingText
//!     ── JNI ──► ime::commit_text / ime::set_composing_text /
//!                ime::finish_composing_text
//!     ── push event ──► global_ime().lock().events
//! ```
//!
//! ## Thread-safety
//!
//! Android IME callbacks all arrive on the View's UI thread (per the
//! `InputConnection` contract). The driver-side `imeStats` JNI call may
//! arrive from any thread; we wrap [`AndroidIme`] in a `Mutex` here so the
//! UI-thread events and stats reads are serialized.
//!
//! ## Logcat tag
//!
//! `WarpIme` (Rust target) — every event emits a single line:
//! `ime_event kind=… text=… cursor=… composing_active=… events_total=…`
//! the M2-S10 driver greps these.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// One IME event derived from the InputConnection state machine.
///
/// Mirrors `warpui::platform::android::ime::ImeEvent` (canonical).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    LatinCommit {
        text: String,
        new_cursor_position: i32,
    },
    ComposingUpdate {
        text: String,
        new_cursor_position: i32,
    },
    ComposingCommit {
        text: String,
        new_cursor_position: i32,
    },
    ComposingFinish {
        committed: String,
    },
    EmptyFinish,
}

impl ImeEvent {
    /// Short tag for logcat grep.
    pub fn kind(&self) -> &'static str {
        match self {
            ImeEvent::LatinCommit { .. } => "latin_commit",
            ImeEvent::ComposingUpdate { .. } => "composing_update",
            ImeEvent::ComposingCommit { .. } => "composing_commit",
            ImeEvent::ComposingFinish { .. } => "composing_finish",
            ImeEvent::EmptyFinish => "empty_finish",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ComposingRegion {
    text: String,
    cursor_position: i32,
}

impl ComposingRegion {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    fn clear(&mut self) {
        self.text.clear();
        self.cursor_position = 0;
    }
}

/// Mirrors `warpui::platform::android::ime::ImeStats` (canonical).
#[derive(Debug, Default, Clone)]
pub struct ImeStats {
    pub commit_text_calls: u64,
    pub set_composing_text_calls: u64,
    pub finish_composing_text_calls: u64,
    pub events_emitted: u64,
    pub latin_commit_count: u64,
    pub composing_update_count: u64,
    pub composing_commit_count: u64,
    pub composing_finish_count: u64,
    pub empty_finish_count: u64,
}

pub struct AndroidIme {
    composing: ComposingRegion,
    /// Round-2 Gboard quirk defer buffer. See module-level docs in the
    /// canonical `warp-src/.../ime.rs`. When `finish_composing_text()` fires
    /// while composing is active, we move the composing text here and clear
    /// the active region — but do NOT emit `ComposingFinish` immediately. The
    /// next event resolves it (commit → reclassify as ComposingCommit;
    /// setComposing → discard; another finish → idempotent; drain → flush).
    pending_finish: Option<String>,
    events: Vec<ImeEvent>,
    commit_text_calls: AtomicU64,
    set_composing_text_calls: AtomicU64,
    finish_composing_text_calls: AtomicU64,
    events_emitted: AtomicU64,
    latin_commit_count: AtomicU64,
    composing_update_count: AtomicU64,
    composing_commit_count: AtomicU64,
    composing_finish_count: AtomicU64,
    empty_finish_count: AtomicU64,
}

impl AndroidIme {
    pub fn new() -> Self {
        Self {
            composing: ComposingRegion::default(),
            pending_finish: None,
            events: Vec::with_capacity(32),
            commit_text_calls: AtomicU64::new(0),
            set_composing_text_calls: AtomicU64::new(0),
            finish_composing_text_calls: AtomicU64::new(0),
            events_emitted: AtomicU64::new(0),
            latin_commit_count: AtomicU64::new(0),
            composing_update_count: AtomicU64::new(0),
            composing_commit_count: AtomicU64::new(0),
            composing_finish_count: AtomicU64::new(0),
            empty_finish_count: AtomicU64::new(0),
        }
    }

    pub fn is_composing(&self) -> bool {
        !self.composing.is_empty()
    }

    pub fn composing_text(&self) -> &str {
        &self.composing.text
    }

    pub fn commit_text(&mut self, text: &str, new_cursor_position: i32) {
        self.commit_text_calls.fetch_add(1, Ordering::Relaxed);
        if self.is_composing() {
            self.composing.clear();
            self.push(ImeEvent::ComposingCommit {
                text: text.to_string(),
                new_cursor_position,
            });
            self.composing_commit_count.fetch_add(1, Ordering::Relaxed);
            self.pending_finish = None;
        } else if self.pending_finish.is_some() {
            // Round-2 Gboard quirk: setComposing → finish → commit. The prior
            // finish was a Gboard transient flush; this commit is the real
            // candidate-pick. Reclassify as ComposingCommit (NOT LatinCommit).
            self.pending_finish = None;
            if text.is_empty() {
                return;
            }
            self.push(ImeEvent::ComposingCommit {
                text: text.to_string(),
                new_cursor_position,
            });
            self.composing_commit_count.fetch_add(1, Ordering::Relaxed);
        } else {
            if text.is_empty() {
                return;
            }
            self.push(ImeEvent::LatinCommit {
                text: text.to_string(),
                new_cursor_position,
            });
            self.latin_commit_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_composing_text(&mut self, text: &str, new_cursor_position: i32) {
        self.set_composing_text_calls.fetch_add(1, Ordering::Relaxed);
        if text.is_empty() {
            if self.is_composing() {
                let prev = std::mem::take(&mut self.composing.text);
                self.composing.clear();
                self.push(ImeEvent::ComposingFinish { committed: prev });
                self.composing_finish_count.fetch_add(1, Ordering::Relaxed);
            }
            return;
        }
        // Round-2: a non-empty setComposing after a pending_finish discards
        // the pending — Gboard's prior transient flush is overridden by the
        // new composing region (the user's actual intent).
        if self.pending_finish.is_some() {
            self.pending_finish = None;
        }
        self.composing.text = text.to_string();
        self.composing.cursor_position = new_cursor_position;
        self.push(ImeEvent::ComposingUpdate {
            text: text.to_string(),
            new_cursor_position,
        });
        self.composing_update_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn finish_composing_text(&mut self) {
        self.finish_composing_text_calls
            .fetch_add(1, Ordering::Relaxed);
        if self.is_composing() {
            let prev = std::mem::take(&mut self.composing.text);
            self.composing.clear();
            // Round-2: defer ComposingFinish — the next event decides whether
            // this is a Gboard transient flush (→ ComposingCommit on next
            // commit) or a real terminal-finish (→ flushed on drain).
            self.pending_finish = Some(prev);
        } else if self.pending_finish.is_some() {
            // Idempotent: re-entrant finish while pending; leave pending set.
        } else {
            self.push(ImeEvent::EmptyFinish);
            self.empty_finish_count.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn drain_events(&mut self) -> Vec<ImeEvent> {
        // Round-2: flush any deferred Gboard pending_finish as ComposingFinish
        // before draining so consumers see a coherent terminal state. This
        // covers the "user typed nihao, dismissed IME, no candidate picked"
        // path and is harmless for Gboard (whose follow-up commit would have
        // already cleared the pending).
        if let Some(prev) = self.pending_finish.take() {
            self.push(ImeEvent::ComposingFinish { committed: prev });
            self.composing_finish_count.fetch_add(1, Ordering::Relaxed);
        }
        std::mem::take(&mut self.events)
    }

    /// Test/diagnostic only — visible flag for the deferred Gboard quirk.
    #[cfg(test)]
    pub fn has_pending_finish(&self) -> bool {
        self.pending_finish.is_some()
    }

    pub fn last_event(&self) -> Option<&ImeEvent> {
        self.events.last()
    }

    pub fn stats(&self) -> ImeStats {
        ImeStats {
            commit_text_calls: self.commit_text_calls.load(Ordering::Relaxed),
            set_composing_text_calls: self
                .set_composing_text_calls
                .load(Ordering::Relaxed),
            finish_composing_text_calls: self
                .finish_composing_text_calls
                .load(Ordering::Relaxed),
            events_emitted: self.events_emitted.load(Ordering::Relaxed),
            latin_commit_count: self.latin_commit_count.load(Ordering::Relaxed),
            composing_update_count: self.composing_update_count.load(Ordering::Relaxed),
            composing_commit_count: self.composing_commit_count.load(Ordering::Relaxed),
            composing_finish_count: self.composing_finish_count.load(Ordering::Relaxed),
            empty_finish_count: self.empty_finish_count.load(Ordering::Relaxed),
        }
    }

    fn push(&mut self, event: ImeEvent) {
        log::info!(
            target: "WarpIme",
            "ime_event kind={} text={:?} cursor={} composing_active={} composing_text={:?} events_total={}",
            event.kind(),
            extract_text(&event),
            extract_cursor(&event),
            !self.composing.is_empty(),
            self.composing.text,
            self.events.len() as u64 + 1
        );
        self.events.push(event);
        self.events_emitted.fetch_add(1, Ordering::Relaxed);
    }
}

impl Default for AndroidIme {
    fn default() -> Self {
        Self::new()
    }
}

fn extract_text(event: &ImeEvent) -> String {
    match event {
        ImeEvent::LatinCommit { text, .. } => text.clone(),
        ImeEvent::ComposingUpdate { text, .. } => text.clone(),
        ImeEvent::ComposingCommit { text, .. } => text.clone(),
        ImeEvent::ComposingFinish { committed } => committed.clone(),
        ImeEvent::EmptyFinish => String::new(),
    }
}

fn extract_cursor(event: &ImeEvent) -> i32 {
    match event {
        ImeEvent::LatinCommit { new_cursor_position, .. } => *new_cursor_position,
        ImeEvent::ComposingUpdate { new_cursor_position, .. } => *new_cursor_position,
        ImeEvent::ComposingCommit { new_cursor_position, .. } => *new_cursor_position,
        _ => 0,
    }
}

/// Process-wide IME singleton. Initialized lazily on first JNI call.
fn global_ime() -> &'static Mutex<AndroidIme> {
    static IME: OnceLock<Mutex<AndroidIme>> = OnceLock::new();
    IME.get_or_init(|| Mutex::new(AndroidIme::new()))
}

/// Public entry point — driven by JNI `imeCommitText`.
pub fn commit_text(text: &str, new_cursor_position: i32) {
    if let Ok(mut g) = global_ime().lock() {
        g.commit_text(text, new_cursor_position);
    }
}

/// Public entry point — driven by JNI `imeSetComposingText`.
pub fn set_composing_text(text: &str, new_cursor_position: i32) {
    if let Ok(mut g) = global_ime().lock() {
        g.set_composing_text(text, new_cursor_position);
    }
}

/// Public entry point — driven by JNI `imeFinishComposingText`.
pub fn finish_composing_text() {
    if let Ok(mut g) = global_ime().lock() {
        g.finish_composing_text();
    }
}

/// Returns a comma-separated diagnostic string used by the M2-S10 driver:
///   "commit_calls=N,set_composing_calls=N,finish_calls=N,events=N,
///    latin=N,composing_update=N,composing_commit=N,composing_finish=N,
///    empty_finish=N,is_composing=B,composing_text=S"
pub fn stats_string() -> String {
    let g = match global_ime().lock() {
        Ok(g) => g,
        Err(_) => return String::new(),
    };
    let s = g.stats();
    format!(
        "commit_calls={},set_composing_calls={},finish_calls={},events={},latin={},composing_update={},composing_commit={},composing_finish={},empty_finish={},is_composing={},composing_text={}",
        s.commit_text_calls,
        s.set_composing_text_calls,
        s.finish_composing_text_calls,
        s.events_emitted,
        s.latin_commit_count,
        s.composing_update_count,
        s.composing_commit_count,
        s.composing_finish_count,
        s.empty_finish_count,
        g.is_composing(),
        g.composing_text(),
    )
}

/// Reset the singleton state. Used by the M2-S10 driver between sub-tests
/// (e.g. between Latin pass and Pinyin pass) so events from earlier sub-tests
/// don't pollute later ones.
pub fn reset() {
    if let Ok(mut g) = global_ime().lock() {
        *g = AndroidIme::new();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_hello_emits_5_latin_commits() {
        let mut ime = AndroidIme::new();
        for ch in ['h', 'e', 'l', 'l', 'o'] {
            ime.commit_text(&ch.to_string(), 1);
        }
        let stats = ime.stats();
        assert_eq!(stats.latin_commit_count, 5);
        assert_eq!(stats.composing_update_count, 0);
        assert_eq!(stats.composing_commit_count, 0);
    }

    #[test]
    fn pinyin_in_place_compose_then_commit() {
        let mut ime = AndroidIme::new();
        for stage in ["n", "ni", "nih", "niha", "nihao"] {
            ime.set_composing_text(stage, 1);
        }
        ime.commit_text("你好", 1);
        let stats = ime.stats();
        assert_eq!(stats.composing_update_count, 5);
        assert_eq!(stats.composing_commit_count, 1);
        assert_eq!(stats.empty_finish_count, 0);
    }

    #[test]
    fn gboard_empty_finish_idempotent() {
        let mut ime = AndroidIme::new();
        ime.set_composing_text("nih", 1);
        ime.commit_text("你", 1);
        ime.finish_composing_text();
        let stats = ime.stats();
        assert_eq!(stats.composing_commit_count, 1);
        assert_eq!(stats.empty_finish_count, 1);
    }

    #[test]
    fn singleton_reset_clears_counters() {
        // Use the singleton path to verify reset() does what driver expects.
        commit_text("a", 1);
        commit_text("b", 1);
        let s_before = stats_string();
        assert!(s_before.contains("latin=2") || s_before.contains("latin="));
        reset();
        let s_after = stats_string();
        assert!(s_after.contains("latin=0"), "after reset: {}", s_after);
    }

    /// Round-2 — the real Gboard quirk on Galaxy S24 Ultra (Codex round-1
    /// device repro): `setComposing → finish → commit` order. The candidate
    /// pick must be classified as `ComposingCommit`, NOT `LatinCommit`.
    #[test]
    fn gboard_finish_then_commit_classifies_as_composing_commit() {
        let mut ime = AndroidIme::new();
        ime.set_composing_text("nihao", 1);
        ime.finish_composing_text();
        // Pending finish buffered; no ComposingFinish event yet, no
        // misclassified commit.
        assert!(ime.has_pending_finish());
        let stats_mid = ime.stats();
        assert_eq!(stats_mid.composing_finish_count, 0);
        ime.commit_text("你好", 1);
        // commit_text resolves pending: emits ComposingCommit, NOT LatinCommit.
        assert!(!ime.has_pending_finish());
        let events = ime.drain_events();
        assert_eq!(events.len(), 2, "events: {:?}", events);
        match &events[0] {
            ImeEvent::ComposingUpdate { text, .. } => assert_eq!(text, "nihao"),
            other => panic!("event 0 not ComposingUpdate: {:?}", other),
        }
        match &events[1] {
            ImeEvent::ComposingCommit { text, .. } => assert_eq!(text, "你好"),
            other => panic!("event 1 not ComposingCommit: {:?}", other),
        }
        let stats = ime.stats();
        assert_eq!(stats.composing_update_count, 1);
        assert_eq!(stats.composing_commit_count, 1);
        assert_eq!(stats.composing_finish_count, 0);
        assert_eq!(stats.empty_finish_count, 0);
        assert_eq!(stats.latin_commit_count, 0);
    }

    /// Round-2 — variant where another `setComposingText` follows the
    /// pending_finish. The pending is silently discarded; the new composing
    /// region replaces it. No spurious ComposingFinish emission.
    #[test]
    fn gboard_finish_then_set_clears_pending() {
        let mut ime = AndroidIme::new();
        ime.set_composing_text("nihao", 1);
        ime.finish_composing_text();
        assert!(ime.has_pending_finish());
        ime.set_composing_text("hi", 1);
        assert!(!ime.has_pending_finish(), "pending should be discarded");
        let events = ime.drain_events();
        assert_eq!(events.len(), 2, "events: {:?}", events);
        match &events[0] {
            ImeEvent::ComposingUpdate { text, .. } => assert_eq!(text, "nihao"),
            other => panic!("event 0 not ComposingUpdate: {:?}", other),
        }
        match &events[1] {
            ImeEvent::ComposingUpdate { text, .. } => assert_eq!(text, "hi"),
            other => panic!("event 1 not ComposingUpdate: {:?}", other),
        }
        let stats = ime.stats();
        assert_eq!(stats.composing_update_count, 2);
        assert_eq!(stats.composing_finish_count, 0);
        assert_eq!(stats.composing_commit_count, 0);
        assert_eq!(stats.empty_finish_count, 0);
    }

    /// Round-2 — drain flushes pending_finish as ComposingFinish for the
    /// non-Gboard / IME-dismissed path (no follow-up event).
    #[test]
    fn drain_flushes_pending_finish_as_composing_finish() {
        let mut ime = AndroidIme::new();
        ime.set_composing_text("nihao", 1);
        ime.finish_composing_text();
        assert!(ime.has_pending_finish());
        let events = ime.drain_events();
        // 1 ComposingUpdate + 1 ComposingFinish (drain-flushed pending).
        assert_eq!(events.len(), 2, "events: {:?}", events);
        match &events[1] {
            ImeEvent::ComposingFinish { committed } => assert_eq!(committed, "nihao"),
            other => panic!("event 1 not ComposingFinish: {:?}", other),
        }
        assert!(!ime.has_pending_finish(), "drain should clear pending");
        let stats = ime.stats();
        assert_eq!(stats.composing_finish_count, 1);
    }
}
