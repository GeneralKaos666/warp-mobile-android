//! M2-S11 — Android touch input state machine (main-repo runtime).
//!
//! Mirrors `warp-src/crates/warpui/src/platform/android/input.rs` (canonical).
//! Cross-workspace duplication is the M2 pattern (M3 unification carry-over per
//! M2-S07/S08 reviews); we keep parity by matching type definitions while adding
//! a process-wide singleton (`global_input()`) for the JNI shim to dispatch into.
//!
//! ## Event flow
//!
//! ```text
//!  WarpInputView (Java)
//!    onTouchEvent(MotionEvent) ──► ACTION_DOWN / ACTION_UP
//!    GestureDetector.SimpleOnGestureListener
//!      onSingleTapConfirmed  ──► NativeBridge.inputTap(x, y)
//!      onLongPress           ──► NativeBridge.inputLongPress(x, y)
//!      onScroll(e1,e2,dx,dy) ──► NativeBridge.inputScroll(x, y, vx, vy)
//!    raw onTouchEvent
//!      ACTION_DOWN           ──► NativeBridge.inputTouchDown(x, y)
//!      ACTION_UP             ──► NativeBridge.inputTouchUp(x, y)
//!   ── JNI ──► input::touch_down / touch_up / tap / long_press / scroll
//!     ── push event ──► global_input().lock().events
//! ```
//!
//! ## Thread-safety
//!
//! Android touch callbacks arrive on the View's UI thread. The driver-side
//! `inputStats` JNI call may arrive from any thread; we wrap [`AndroidInput`]
//! in a `Mutex` so UI-thread events and stats reads are serialized.
//!
//! ## Logcat tag
//!
//! `WarpInput` (Rust target) — every event emits:
//! `input_event kind=<kind> x=<f32> y=<f32> vx=<f32> vy=<f32> events_total=<n>`
//! The M2-S11 driver greps these.

use std::sync::{Mutex, OnceLock};

// ── InputEvent ────────────────────────────────────────────────────────────────

/// One input event derived from Android MotionEvent / GestureDetector.
///
/// Mirrors `warpui::platform::android::input::InputEvent` (canonical).
#[derive(Debug, Clone, PartialEq)]
pub enum InputEvent {
    /// Raw ACTION_DOWN — finger first touches the screen.
    TouchDown { x: f32, y: f32 },
    /// Raw ACTION_UP — finger lifts from screen.
    TouchUp { x: f32, y: f32 },
    /// GestureDetector `onSingleTapConfirmed` — confirmed single tap
    /// (not the start of a double-tap).
    Tap { x: f32, y: f32 },
    /// GestureDetector `onLongPress` — sustained press ≥ ViewConfiguration
    /// long-press timeout (~500 ms). Equivalent to right-click / context-menu.
    LongPress { x: f32, y: f32 },
    /// GestureDetector `onScroll` with VelocityTracker — drag scroll with
    /// instantaneous velocity in pixels/s. `dx`/`dy` are the distance moved
    /// since the previous scroll event (positive dy = finger moved down =
    /// content scrolls up).
    Scroll {
        x: f32,
        y: f32,
        /// Pixels scrolled since last scroll event (X axis).
        dx: f32,
        /// Pixels scrolled since last scroll event (Y axis).
        dy: f32,
        /// Instantaneous X velocity in px/s from VelocityTracker.
        vx: f32,
        /// Instantaneous Y velocity in px/s from VelocityTracker.
        vy: f32,
    },
}

impl InputEvent {
    /// Short tag for logcat grep.
    pub fn kind(&self) -> &'static str {
        match self {
            InputEvent::TouchDown { .. } => "touch_down",
            InputEvent::TouchUp { .. } => "touch_up",
            InputEvent::Tap { .. } => "tap",
            InputEvent::LongPress { .. } => "long_press",
            InputEvent::Scroll { .. } => "scroll",
        }
    }

    pub fn x(&self) -> f32 {
        match self {
            InputEvent::TouchDown { x, .. } => *x,
            InputEvent::TouchUp { x, .. } => *x,
            InputEvent::Tap { x, .. } => *x,
            InputEvent::LongPress { x, .. } => *x,
            InputEvent::Scroll { x, .. } => *x,
        }
    }

    pub fn y(&self) -> f32 {
        match self {
            InputEvent::TouchDown { y, .. } => *y,
            InputEvent::TouchUp { y, .. } => *y,
            InputEvent::Tap { y, .. } => *y,
            InputEvent::LongPress { y, .. } => *y,
            InputEvent::Scroll { y, .. } => *y,
        }
    }

    pub fn vx(&self) -> f32 {
        match self {
            InputEvent::Scroll { vx, .. } => *vx,
            _ => 0.0,
        }
    }

    pub fn vy(&self) -> f32 {
        match self {
            InputEvent::Scroll { vy, .. } => *vy,
            _ => 0.0,
        }
    }
}

// ── InputStats ───────────────────────────────────────────────────────────────

/// Mirrors `warpui::platform::android::input::InputStats` (canonical).
#[derive(Debug, Default, Clone)]
pub struct InputStats {
    pub touch_down_count: u64,
    pub touch_up_count: u64,
    pub tap_count: u64,
    pub long_press_count: u64,
    pub scroll_count: u64,
    pub events_emitted: u64,
    /// x coordinate of last received touch_down event.
    pub last_down_x: f32,
    /// y coordinate of last received touch_down event.
    pub last_down_y: f32,
    /// x coordinate of last received touch_up event.
    pub last_up_x: f32,
    /// y coordinate of last received touch_up event.
    pub last_up_y: f32,
    /// vy of last received scroll event (negative = upward scroll from
    /// downward swipe).
    pub last_scroll_vy: f32,
    /// vx of last received scroll event.
    pub last_scroll_vx: f32,
}

// ── AndroidInput ─────────────────────────────────────────────────────────────

pub struct AndroidInput {
    events: Vec<InputEvent>,
    touch_down_count: u64,
    touch_up_count: u64,
    tap_count: u64,
    long_press_count: u64,
    scroll_count: u64,
    events_emitted: u64,
    last_down_x: f32,
    last_down_y: f32,
    last_up_x: f32,
    last_up_y: f32,
    last_scroll_vx: f32,
    last_scroll_vy: f32,
}

impl AndroidInput {
    pub fn new() -> Self {
        Self {
            events: Vec::with_capacity(32),
            touch_down_count: 0,
            touch_up_count: 0,
            tap_count: 0,
            long_press_count: 0,
            scroll_count: 0,
            events_emitted: 0,
            last_down_x: 0.0,
            last_down_y: 0.0,
            last_up_x: 0.0,
            last_up_y: 0.0,
            last_scroll_vx: 0.0,
            last_scroll_vy: 0.0,
        }
    }

    pub fn touch_down(&mut self, x: f32, y: f32) {
        self.touch_down_count += 1;
        self.last_down_x = x;
        self.last_down_y = y;
        self.push(InputEvent::TouchDown { x, y });
    }

    pub fn touch_up(&mut self, x: f32, y: f32) {
        self.touch_up_count += 1;
        self.last_up_x = x;
        self.last_up_y = y;
        self.push(InputEvent::TouchUp { x, y });
    }

    pub fn tap(&mut self, x: f32, y: f32) {
        self.tap_count += 1;
        self.push(InputEvent::Tap { x, y });
    }

    pub fn long_press(&mut self, x: f32, y: f32) {
        self.long_press_count += 1;
        self.push(InputEvent::LongPress { x, y });
    }

    pub fn scroll(&mut self, x: f32, y: f32, dx: f32, dy: f32, vx: f32, vy: f32) {
        self.scroll_count += 1;
        self.last_scroll_vx = vx;
        self.last_scroll_vy = vy;
        self.push(InputEvent::Scroll { x, y, dx, dy, vx, vy });
    }

    pub fn drain_events(&mut self) -> Vec<InputEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn last_event(&self) -> Option<&InputEvent> {
        self.events.last()
    }

    pub fn stats(&self) -> InputStats {
        InputStats {
            touch_down_count: self.touch_down_count,
            touch_up_count: self.touch_up_count,
            tap_count: self.tap_count,
            long_press_count: self.long_press_count,
            scroll_count: self.scroll_count,
            events_emitted: self.events_emitted,
            last_down_x: self.last_down_x,
            last_down_y: self.last_down_y,
            last_up_x: self.last_up_x,
            last_up_y: self.last_up_y,
            last_scroll_vx: self.last_scroll_vx,
            last_scroll_vy: self.last_scroll_vy,
        }
    }

    fn push(&mut self, event: InputEvent) {
        // events_emitted is the count of all events ever pushed (never reset by
        // drain_events). Use it as the log counter so events_total stays
        // monotonically increasing across drain cycles, as the driver contract
        // requires for window reconstruction via monotonic-break detection.
        let total = self.events_emitted + 1; // +1 because we increment below
        log::info!(
            target: "WarpInput",
            "input_event kind={} x={:.1} y={:.1} vx={:.1} vy={:.1} events_total={}",
            event.kind(),
            event.x(),
            event.y(),
            event.vx(),
            event.vy(),
            total,
        );
        self.events.push(event);
        self.events_emitted += 1;
    }
}

impl Default for AndroidInput {
    fn default() -> Self {
        Self::new()
    }
}

// ── Singleton ────────────────────────────────────────────────────────────────

/// Process-wide input singleton. Initialized lazily on first JNI call.
fn global_input() -> &'static Mutex<AndroidInput> {
    static INPUT: OnceLock<Mutex<AndroidInput>> = OnceLock::new();
    INPUT.get_or_init(|| Mutex::new(AndroidInput::new()))
}

// ── Public entry points (called from JNI in lib.rs) ──────────────────────────

/// Raw ACTION_DOWN.
pub fn touch_down(x: f32, y: f32) {
    if let Ok(mut g) = global_input().lock() {
        g.touch_down(x, y);
    }
}

/// Raw ACTION_UP.
pub fn touch_up(x: f32, y: f32) {
    if let Ok(mut g) = global_input().lock() {
        g.touch_up(x, y);
    }
}

/// GestureDetector `onSingleTapConfirmed`.
pub fn tap(x: f32, y: f32) {
    if let Ok(mut g) = global_input().lock() {
        g.tap(x, y);
    }
}

/// GestureDetector `onLongPress`.
pub fn long_press(x: f32, y: f32) {
    if let Ok(mut g) = global_input().lock() {
        g.long_press(x, y);
    }
}

/// GestureDetector `onScroll` + VelocityTracker velocity.
pub fn scroll(x: f32, y: f32, dx: f32, dy: f32, vx: f32, vy: f32) {
    if let Ok(mut g) = global_input().lock() {
        g.scroll(x, y, dx, dy, vx, vy);
    }
}

/// Returns a comma-separated diagnostic string used by the M2-S11 driver:
///   "touch_down=N,touch_up=N,tap=N,long_press=N,scroll=N,events=N,
///    last_down_x=F,last_down_y=F,last_up_x=F,last_up_y=F,
///    last_scroll_vx=F,last_scroll_vy=F"
pub fn stats_string() -> String {
    let g = match global_input().lock() {
        Ok(g) => g,
        Err(_) => return String::new(),
    };
    let s = g.stats();
    format!(
        "touch_down={},touch_up={},tap={},long_press={},scroll={},events={},\
         last_down_x={:.1},last_down_y={:.1},last_up_x={:.1},last_up_y={:.1},\
         last_scroll_vx={:.1},last_scroll_vy={:.1}",
        s.touch_down_count,
        s.touch_up_count,
        s.tap_count,
        s.long_press_count,
        s.scroll_count,
        s.events_emitted,
        s.last_down_x,
        s.last_down_y,
        s.last_up_x,
        s.last_up_y,
        s.last_scroll_vx,
        s.last_scroll_vy,
    )
}

/// Reset the singleton state. Driver calls between sub-tests.
pub fn reset() {
    if let Ok(mut g) = global_input().lock() {
        *g = AndroidInput::new();
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_down_up_recorded() {
        let mut input = AndroidInput::new();
        input.touch_down(540.0, 1170.0);
        input.touch_up(540.0, 1170.0);
        let stats = input.stats();
        assert_eq!(stats.touch_down_count, 1);
        assert_eq!(stats.touch_up_count, 1);
        assert!((stats.last_down_x - 540.0).abs() < 0.1);
        assert!((stats.last_down_y - 1170.0).abs() < 0.1);
    }

    #[test]
    fn tap_event_recorded() {
        let mut input = AndroidInput::new();
        input.tap(100.0, 200.0);
        let stats = input.stats();
        assert_eq!(stats.tap_count, 1);
        assert_eq!(stats.events_emitted, 1);
    }

    #[test]
    fn long_press_event_recorded() {
        let mut input = AndroidInput::new();
        input.long_press(300.0, 400.0);
        let stats = input.stats();
        assert_eq!(stats.long_press_count, 1);
    }

    #[test]
    fn scroll_event_records_velocity() {
        let mut input = AndroidInput::new();
        // Downward swipe (finger moves down → content scrolls up → vy > 0 px/s).
        input.scroll(540.0, 1000.0, 0.0, -100.0, 0.0, -1200.0);
        let stats = input.stats();
        assert_eq!(stats.scroll_count, 1);
        assert!((stats.last_scroll_vy - (-1200.0)).abs() < 1.0);
    }

    #[test]
    fn drain_clears_events() {
        let mut input = AndroidInput::new();
        input.touch_down(10.0, 20.0);
        input.touch_up(10.0, 20.0);
        let drained = input.drain_events();
        assert_eq!(drained.len(), 2);
        assert!(input.drain_events().is_empty());
    }

    #[test]
    fn singleton_reset_clears_counters() {
        touch_down(1.0, 1.0);
        touch_up(1.0, 1.0);
        let before = stats_string();
        assert!(before.contains("touch_down=") && !before.contains("touch_down=0,"));
        reset();
        let after = stats_string();
        assert!(after.contains("touch_down=0,"), "after reset: {}", after);
        assert!(after.contains("touch_up=0,"), "after reset: {}", after);
    }

    #[test]
    fn stats_string_format() {
        let mut input = AndroidInput::new();
        input.touch_down(540.0, 1170.0);
        input.touch_up(541.0, 1171.0);
        input.scroll(540.0, 900.0, 0.0, -50.0, 0.0, -800.0);
        let s = {
            // Replicate stats_string inline to avoid requiring a real singleton.
            let stats = input.stats();
            format!(
                "touch_down={},touch_up={},tap={},long_press={},scroll={},events={},\
                 last_down_x={:.1},last_down_y={:.1},last_up_x={:.1},last_up_y={:.1},\
                 last_scroll_vx={:.1},last_scroll_vy={:.1}",
                stats.touch_down_count,
                stats.touch_up_count,
                stats.tap_count,
                stats.long_press_count,
                stats.scroll_count,
                stats.events_emitted,
                stats.last_down_x,
                stats.last_down_y,
                stats.last_up_x,
                stats.last_up_y,
                stats.last_scroll_vx,
                stats.last_scroll_vy,
            )
        };
        assert!(s.starts_with("touch_down=1,touch_up=1,tap=0,long_press=0,scroll=1,events=3,"));
        assert!(s.contains("last_down_x=540.0,last_down_y=1170.0"));
        assert!(s.contains("last_scroll_vy=-800.0"));
    }
}
