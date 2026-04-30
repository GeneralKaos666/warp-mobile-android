//! M3-S04 — Terminal model + Vulkan push_frame harness (main-repo runtime mirror).
//!
//! This module mirrors `warp-src/crates/warp_terminal_mobile_facade/src/render.rs`
//! (the canonical [`TerminalModel`] data shape) and adds a process-wide
//! singleton that the JNI exports drive. It is the **runtime path** for
//! M3-S04 — the canonical facade copy is what M3-S05/S07 will extend with
//! ANSI parsing + Block aggregation; the JNI side here ingests bytes,
//! coalesces dirty state, and pushes frames through the existing static
//! grid pipeline.
//!
//! Cross-workspace mirror policy (M3-S11 unification scope):
//!   * Canonical (warp-src facade) — the data shape extended by M3-S05+
//!   * Runtime mirror (this file) — what the JNI cdylib actually compiles
//!     into the .so shipped to the device. The mirror is kept in sync at
//!     M3 close-out per the same policy as `font_render.rs` / `static_grid.rs`
//!     / `ime.rs` / `input.rs`.
//!
//! The mirror contains only the facade-shaped data + dirty-bit accessors —
//! the actual GPU work is forwarded to `crate::vulkan::{init_static_grid,
//! submit_grid_frame}` which already exists in the main-repo runtime path.
//!
//! ## Web-search references (M3-S04, 2026-04-30)
//!
//! - **Choreographer-driven invalidate / dirty-bit pattern**:
//!   <https://developer.android.com/reference/android/view/Choreographer.FrameCallback>
//!   <https://developer.android.com/reference/android/view/View#invalidate()>
//!   The Choreographer side reads-and-clears the dirty bit once per vsync,
//!   which coalesces multiple PTY chunks into a single grid re-init.
//! - **JNI byte-array passing for streaming buffers**:
//!   <https://docs.oracle.com/javase/8/docs/technotes/guides/jni/spec/functions.html#GetByteArrayRegion>
//!   <https://developer.android.com/training/articles/perf-jni>
//!   The JNI export uses `jni::JNIEnv::convert_byte_array` (one copy via
//!   GetByteArrayRegion) rather than the GetPrimitiveArrayCritical pin'd
//!   pointer, because we hold the `Mutex` for the full ingest path. Pinning
//!   the JVM heap during a Mutex hold is the documented anti-pattern in the
//!   perf-jni guide.
//! - **termux-app `TerminalEmulator.append`**:
//!   <https://github.com/termux/termux-app/blob/master/terminal-emulator/src/main/java/com/termux/terminal/TerminalEmulator.java>
//!   Same coarse contract — bytes in, grid mutated, dirty bit raised — but
//!   the parser lives here in Rust rather than Kotlin.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Default grid size when the renderer hasn't reported the surface dimensions
/// yet. M3-S04 baseline picks 24×80 to match a classic VT100; M3-S08 will
/// resize this from `ANativeWindow_getWidth/Height` once the surface attaches.
pub const DEFAULT_ROWS: usize = 24;
pub const DEFAULT_COLS: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub glyph: char,
    pub fg: u32,
    pub bg: u32,
    pub attrs: u8,
}

impl Cell {
    pub const fn blank() -> Self {
        Self {
            glyph: ' ',
            fg: 0xFFFFFFFFu32,
            bg: 0x000000FFu32,
            attrs: 0,
        }
    }
    pub const fn glyph(c: char) -> Self {
        Self {
            glyph: c,
            fg: 0xFFFFFFFFu32,
            bg: 0x000000FFu32,
            attrs: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

pub struct TerminalModel {
    inner: Mutex<TerminalState>,
    dirty: AtomicBool,
}

struct TerminalState {
    rows: usize,
    cols: usize,
    cells: Vec<Vec<Cell>>,
    cursor: Cursor,
    bytes_ingested: u64,
}

impl TerminalModel {
    pub fn new(rows: usize, cols: usize) -> Self {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let cells = (0..rows)
            .map(|_| vec![Cell::blank(); cols])
            .collect::<Vec<_>>();
        Self {
            inner: Mutex::new(TerminalState {
                rows,
                cols,
                cells,
                cursor: Cursor::default(),
                bytes_ingested: 0,
            }),
            dirty: AtomicBool::new(false),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_ROWS, DEFAULT_COLS)
    }

    pub fn resize(&self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        let mut state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if state.rows == rows && state.cols == cols {
            return;
        }
        let mut new_cells: Vec<Vec<Cell>> = (0..rows).map(|_| vec![Cell::blank(); cols]).collect();
        for r in 0..rows.min(state.rows) {
            for c in 0..cols.min(state.cols) {
                new_cells[r][c] = state.cells[r][c];
            }
        }
        state.cells = new_cells;
        state.rows = rows;
        state.cols = cols;
        if state.cursor.row >= rows {
            state.cursor.row = rows - 1;
        }
        if state.cursor.col >= cols {
            state.cursor.col = cols - 1;
        }
        self.dirty.store(true, Ordering::Release);
    }

    pub fn ingest_pty_bytes(&self, bytes: &[u8]) -> usize {
        let mut state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.bytes_ingested = state.bytes_ingested.saturating_add(bytes.len() as u64);
        let text = String::from_utf8_lossy(bytes);
        for ch in text.chars() {
            match ch {
                '\u{0}' => {}
                '\u{8}' => {
                    if state.cursor.col > 0 {
                        state.cursor.col -= 1;
                    }
                }
                '\t' => {
                    let cols = state.cols;
                    let next = ((state.cursor.col / 8) + 1) * 8;
                    state.cursor.col = next.min(cols.saturating_sub(1));
                }
                '\n' => {
                    state.cursor.row += 1;
                    if state.cursor.row >= state.rows {
                        scroll_up(&mut state);
                        state.cursor.row = state.rows - 1;
                    }
                }
                '\r' => {
                    state.cursor.col = 0;
                }
                '\u{1b}' => {
                    log::trace!(
                        target: "WarpTerminalModel",
                        "ESC byte ignored at row={} col={} (M3-S05 parser pending)",
                        state.cursor.row, state.cursor.col
                    );
                }
                printable if !printable.is_control() => {
                    let row = state.cursor.row.min(state.rows - 1);
                    let col = state.cursor.col.min(state.cols - 1);
                    state.cells[row][col] = Cell::glyph(printable);
                    state.cursor.col += 1;
                    if state.cursor.col >= state.cols {
                        state.cursor.col = 0;
                        state.cursor.row += 1;
                        if state.cursor.row >= state.rows {
                            scroll_up(&mut state);
                            state.cursor.row = state.rows - 1;
                        }
                    }
                }
                _ => {}
            }
        }
        self.dirty.store(true, Ordering::Release);
        bytes.len()
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Non-destructive dirty bit peek — used by `terminalModelStats` so the
    /// stats accessor doesn't accidentally swallow a pending Choreographer
    /// re-init. Returns the current dirty state without clearing.
    pub fn peek_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    pub fn snapshot_text(&self) -> String {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut out = String::with_capacity(state.rows * (state.cols + 1));
        for (r, row) in state.cells.iter().enumerate() {
            for cell in row {
                out.push(cell.glyph);
            }
            if r + 1 < state.rows {
                out.push('\n');
            }
        }
        out
    }

    pub fn dims(&self) -> (usize, usize) {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        (state.rows, state.cols)
    }

    pub fn cursor(&self) -> Cursor {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.cursor
    }

    pub fn bytes_ingested(&self) -> u64 {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.bytes_ingested
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<Cell> {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.cells.get(row).and_then(|r| r.get(col)).copied()
    }
}

fn scroll_up(state: &mut TerminalState) {
    if state.rows < 2 {
        for c in 0..state.cols {
            state.cells[0][c] = Cell::blank();
        }
        return;
    }
    state.cells.remove(0);
    state.cells.push(vec![Cell::blank(); state.cols]);
}

// ── Process-wide singleton ─────────────────────────────────────────────────

static GLOBAL_MODEL: OnceLock<TerminalModel> = OnceLock::new();

/// Returns the process-wide [`TerminalModel`]. Initialized lazily on first
/// access with the default 24×80 dimensions; the Choreographer-side
/// `terminalModelResize` JNI call will reshape it once the renderer reports
/// the surface dimensions.
pub fn global_model() -> &'static TerminalModel {
    GLOBAL_MODEL.get_or_init(TerminalModel::new_default)
}

/// Convenience: ingest bytes into the process-wide model. JNI export entry
/// point (`Java_dev_warp_mobile_NativeBridge_terminalInputBytes`).
pub fn ingest_pty_bytes(bytes: &[u8]) -> usize {
    global_model().ingest_pty_bytes(bytes)
}

/// Read-and-clear the dirty bit on the process-wide model.
pub fn take_dirty() -> bool {
    global_model().take_dirty()
}

/// Take a snapshot of the process-wide model as a single text string.
pub fn snapshot_text() -> String {
    global_model().snapshot_text()
}

/// Returns the current dimensions as `(rows, cols)`.
pub fn dims() -> (usize, usize) {
    global_model().dims()
}

/// Resize the process-wide model.
pub fn resize(rows: usize, cols: usize) {
    global_model().resize(rows, cols);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingest_then_snapshot_round_trips_plain_text() {
        let m = TerminalModel::new(2, 8);
        m.ingest_pty_bytes(b"hello\r\nworld");
        assert!(m.take_dirty());
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'h');
        assert_eq!(m.cell(1, 0).unwrap().glyph, 'w');
        let snap = m.snapshot_text();
        assert!(snap.contains("hello"));
        assert!(snap.contains("world"));
        assert!(!m.take_dirty(), "second take_dirty returns false");
    }

    #[test]
    fn dirty_bit_coalesces() {
        let m = TerminalModel::new(2, 8);
        m.ingest_pty_bytes(b"a");
        m.ingest_pty_bytes(b"b");
        m.ingest_pty_bytes(b"c");
        // Three ingests but only one dirty take.
        assert!(m.take_dirty());
        assert!(!m.take_dirty());
    }

    #[test]
    fn resize_preserves_visible_cells() {
        let m = TerminalModel::new(2, 8);
        m.ingest_pty_bytes(b"hello");
        m.resize(2, 16);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'h');
        assert_eq!(m.cell(0, 4).unwrap().glyph, 'o');
        assert_eq!(m.cell(0, 15).unwrap().glyph, ' ');
        assert_eq!(m.dims(), (2, 16));
    }

    #[test]
    fn global_model_is_singleton() {
        // Two accesses return the same model.
        let r1 = global_model() as *const _;
        let r2 = global_model() as *const _;
        assert_eq!(r1, r2);
    }
}
