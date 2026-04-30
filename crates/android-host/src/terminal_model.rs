//! M3-S04/M3-S05 — Terminal model + Vulkan push_frame harness (main-repo runtime mirror).
//!
//! This module mirrors `warp-src/crates/warp_terminal_mobile_facade/src/render.rs`
//! (the canonical [`TerminalModel`] data shape) and adds a process-wide
//! singleton that the JNI exports drive. It is the **runtime path** for
//! M3-S04/S05 — the canonical facade copy is what M3-S05 extends with
//! ANSI/DCS streaming parsing; the JNI side here ingests bytes, coalesces
//! dirty state, and pushes frames through the existing static grid pipeline.
//!
//! Cross-workspace mirror policy (M3-S11 unification scope):
//!   * Canonical (warp-src facade) — the data shape extended by M3-S05+
//!   * Runtime mirror (this file) — what the JNI cdylib actually compiles
//!     into the .so shipped to the device. The mirror is kept in sync at
//!     M3 close-out per the same policy as `font_render.rs` / `static_grid.rs`
//!     / `ime.rs` / `input.rs`.
//!
//! ## What M3-S05 changes vs M3-S04 mirror
//!
//! * Replaced the M3-S04 "ESC byte ignored" branch with a streaming ANSI/DCS
//!   state machine matching the canonical facade implementation. SGR codes
//!   30-37 / 40-47 / 90-97 / 100-107 / 0 / 1 / 4 etc. update per-cell `fg`
//!   `bg` `attrs`. CSI cursor pos / erase line / erase display / cursor
//!   movement parsed.
//! * DCS hook frame parser (`ESC P $ d <hex_chars> ST`) recognized; hex
//!   body is decoded and **logged with logcat tag `WarpTerminalModel`**.
//!   The mirror does NOT deserialize into `DProtoHook` (that requires the
//!   facade dep, which the M3-S11 unification will sort out). Instead it
//!   logs the decoded JSON for device-side verification — this is sufficient
//!   for AC#7 (the device test verifies SGR colors, not full DProtoHook
//!   plumbing; that's covered by the facade-side unit tests).
//!
//! ## Web-search references (M3-S05, 2026-04-30 → 2026-05-01)
//!
//! - **ECMA-48 §5.6 DCS framing**:
//!   <https://ecma-international.org/wp-content/uploads/ECMA-48_5th_edition_june_1991.pdf>
//!   <https://wezfurlong.org/ecma48/04-coding.html>
//! - **xterm Device-Control functions**:
//!   <https://invisible-island.net/xterm/ctlseqs/ctlseqs-contents.html>
//! - **DEC ANSI parser canonical state diagram**:
//!   <https://vt100.net/emu/dec_ansi_parser>
//! - **Rust streaming-parser pattern (anstyle-parse / vtparse)**:
//!   <https://docs.rs/vtparse/latest/vtparse/>
//! - **ANSI SGR color codes**:
//!   <https://en.wikipedia.org/wiki/ANSI_escape_code>
//!
//! Carry-forward citations from M3-S04:
//!
//! - **Choreographer-driven invalidate / dirty-bit pattern**:
//!   <https://developer.android.com/reference/android/view/Choreographer.FrameCallback>
//!   <https://developer.android.com/reference/android/view/View#invalidate()>
//! - **JNI byte-array passing for streaming buffers**:
//!   <https://docs.oracle.com/javase/8/docs/technotes/guides/jni/spec/functions.html#GetByteArrayRegion>
//!   <https://developer.android.com/training/articles/perf-jni>

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Default grid size when the renderer hasn't reported the surface dimensions.
pub const DEFAULT_ROWS: usize = 24;
pub const DEFAULT_COLS: usize = 80;

// ── ANSI 16-color palette (mirrors facade render.rs) ────────────────────────

pub const ANSI_STANDARD_COLORS: [u32; 8] = [
    0x000000FF, // black
    0xCC0000FF, // red
    0x4E9A06FF, // green
    0xC4A000FF, // yellow
    0x3465A4FF, // blue
    0x75507BFF, // magenta
    0x06989AFF, // cyan
    0xD3D7CFFF, // white
];

pub const ANSI_BRIGHT_COLORS: [u32; 8] = [
    0x555753FF, // bright black
    0xEF2929FF, // bright red
    0x8AE234FF, // bright green
    0xFCE94FFF, // bright yellow
    0x729FCFFF, // bright blue
    0xAD7FA8FF, // bright magenta
    0x34E2E2FF, // bright cyan
    0xEEEEECFF, // bright white
];

pub const DEFAULT_FG: u32 = 0xFFFFFFFFu32;
pub const DEFAULT_BG: u32 = 0x000000FFu32;

pub const ATTR_BOLD: u8 = 1 << 0;
pub const ATTR_ITALIC: u8 = 1 << 1;
pub const ATTR_UNDERLINE: u8 = 1 << 2;
pub const ATTR_DIM: u8 = 1 << 3;
pub const ATTR_REVERSE: u8 = 1 << 4;

/// Warp DCS hex-encoded JSON marker — identical to upstream
/// `app/src/terminal/model/ansi/dcs_hooks.rs:14`. M3-S03 v2 extracted this
/// into the facade; the mirror duplicates the constant to avoid the Cargo
/// edge to facade.
pub const HEX_ENCODED_JSON_MARKER: u8 = b'd';
/// Warp DCS unencoded (raw) JSON marker — used only on WSL paths.
pub const UNENCODED_JSON_MARKER: u8 = b'f';

const DCS_BODY_CAP: usize = 64 * 1024;
const CSI_BUF_CAP: usize = 256;

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
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: 0,
        }
    }
    pub const fn glyph(c: char) -> Self {
        Self {
            glyph: c,
            fg: DEFAULT_FG,
            bg: DEFAULT_BG,
            attrs: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
enum AnsiState {
    Ground,
    Esc,
    EscIntermediate,
    Csi(Vec<u8>),
    DcsHex(Vec<u8>),
    DcsRaw(Vec<u8>),
    DcsAwaitMarker { seen_dollar: bool },
    DcsFinish7Bit { is_hex: bool, body: Vec<u8> },
}

impl Default for AnsiState {
    fn default() -> Self {
        Self::Ground
    }
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

    parser: AnsiState,
    cur_fg: u32,
    cur_bg: u32,
    cur_attrs: u8,
    sgr_apply_count: u64,
    dcs_hook_count: u64,
    dcs_error_count: u64,
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
                parser: AnsiState::Ground,
                cur_fg: DEFAULT_FG,
                cur_bg: DEFAULT_BG,
                cur_attrs: 0,
                sgr_apply_count: 0,
                dcs_hook_count: 0,
                dcs_error_count: 0,
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

        for &b in bytes {
            advance_state_machine(&mut state, b);
        }

        self.dirty.store(true, Ordering::Release);
        bytes.len()
    }

    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

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

    /// M3-S05: returns active fg/bg/attrs for the next printable cell.
    pub fn current_attrs(&self) -> (u32, u32, u8) {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        (state.cur_fg, state.cur_bg, state.cur_attrs)
    }

    /// M3-S05: returns running stats `(sgr_apply_count, dcs_hook_count,
    /// dcs_error_count)`. Surfaced through `terminalSgrSummary` JNI for the
    /// device-side AC#7 test driver.
    pub fn parser_stats(&self) -> (u64, u64, u64) {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        (
            state.sgr_apply_count,
            state.dcs_hook_count,
            state.dcs_error_count,
        )
    }
}

// ── Streaming ANSI/DCS state machine helpers (mirror of facade render.rs) ──

fn advance_state_machine(state: &mut TerminalState, b: u8) {
    let parser = std::mem::replace(&mut state.parser, AnsiState::Ground);
    state.parser = match parser {
        AnsiState::Ground => match b {
            0x1b => AnsiState::Esc,
            0x9c => AnsiState::Ground,
            _ => {
                handle_ground_byte(state, b);
                AnsiState::Ground
            }
        },
        AnsiState::Esc => match b {
            b'[' => AnsiState::Csi(Vec::new()),
            b'P' => AnsiState::DcsAwaitMarker { seen_dollar: false },
            b'\\' => AnsiState::Ground,
            0x40..=0x5F => AnsiState::Ground,
            0x20..=0x2F => AnsiState::EscIntermediate,
            _ => AnsiState::Ground,
        },
        AnsiState::EscIntermediate => match b {
            0x20..=0x2F => AnsiState::EscIntermediate,
            _ => AnsiState::Ground,
        },
        AnsiState::Csi(mut buf) => {
            if (0x30..=0x3F).contains(&b) || (0x20..=0x2F).contains(&b) {
                if buf.len() < CSI_BUF_CAP {
                    buf.push(b);
                }
                AnsiState::Csi(buf)
            } else if (0x40..=0x7E).contains(&b) {
                dispatch_csi(state, &buf, b);
                AnsiState::Ground
            } else {
                AnsiState::Ground
            }
        }
        AnsiState::DcsAwaitMarker { seen_dollar } => match (seen_dollar, b) {
            (false, b'$') => AnsiState::DcsAwaitMarker { seen_dollar: true },
            (true, b) if b == HEX_ENCODED_JSON_MARKER => AnsiState::DcsHex(Vec::new()),
            (true, b) if b == UNENCODED_JSON_MARKER => AnsiState::DcsRaw(Vec::new()),
            (_, 0x9c) => AnsiState::Ground,
            (_, 0x1b) => AnsiState::DcsFinish7Bit {
                is_hex: false,
                body: Vec::new(),
            },
            _ => AnsiState::DcsRaw(Vec::new()),
        },
        AnsiState::DcsHex(mut buf) => match b {
            0x9c => {
                finish_dcs(state, true, &buf);
                AnsiState::Ground
            }
            0x1b => AnsiState::DcsFinish7Bit { is_hex: true, body: buf },
            b'0'..=b'9' | b'A'..=b'F' | b'a'..=b'f' => {
                if buf.len() < DCS_BODY_CAP {
                    buf.push(b);
                }
                AnsiState::DcsHex(buf)
            }
            _ => {
                state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                log::warn!(
                    target: "WarpTerminalModel",
                    "DCS hex body aborted: non-hex byte 0x{:02X} after {} chars",
                    b, buf.len()
                );
                AnsiState::Ground
            }
        },
        AnsiState::DcsRaw(mut buf) => match b {
            0x9c => {
                finish_dcs(state, false, &buf);
                AnsiState::Ground
            }
            0x1b => AnsiState::DcsFinish7Bit { is_hex: false, body: buf },
            _ => {
                if buf.len() < DCS_BODY_CAP {
                    buf.push(b);
                }
                AnsiState::DcsRaw(buf)
            }
        },
        AnsiState::DcsFinish7Bit { is_hex, body } => match b {
            b'\\' => {
                finish_dcs(state, is_hex, &body);
                AnsiState::Ground
            }
            _ => {
                state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                log::warn!(
                    target: "WarpTerminalModel",
                    "DCS aborted: expected ESC \\\\ but got ESC 0x{:02X}", b
                );
                AnsiState::Ground
            }
        },
    };
}

fn handle_ground_byte(state: &mut TerminalState, b: u8) {
    match b {
        0x00 => {}
        0x07 => {}
        0x08 => {
            if state.cursor.col > 0 {
                state.cursor.col -= 1;
            }
        }
        b'\t' => {
            let cols = state.cols;
            let next = ((state.cursor.col / 8) + 1) * 8;
            state.cursor.col = next.min(cols.saturating_sub(1));
        }
        b'\n' => {
            state.cursor.row += 1;
            if state.cursor.row >= state.rows {
                scroll_up(state);
                state.cursor.row = state.rows - 1;
            }
        }
        b'\r' => {
            state.cursor.col = 0;
        }
        c if c < 0x20 => {}
        c if c == 0x7F => {}
        _ => {
            let row = state.cursor.row.min(state.rows - 1);
            let col = state.cursor.col.min(state.cols - 1);
            state.cells[row][col] = Cell {
                glyph: b as char,
                fg: state.cur_fg,
                bg: state.cur_bg,
                attrs: state.cur_attrs,
            };
            state.cursor.col += 1;
            if state.cursor.col >= state.cols {
                state.cursor.col = 0;
                state.cursor.row += 1;
                if state.cursor.row >= state.rows {
                    scroll_up(state);
                    state.cursor.row = state.rows - 1;
                }
            }
        }
    }
}

fn dispatch_csi(state: &mut TerminalState, params: &[u8], final_byte: u8) {
    match final_byte {
        b'm' => apply_sgr(state, params),
        b'H' | b'f' => {
            let (row1, col1) = parse_two_params(params, 1, 1);
            let row = row1.saturating_sub(1).min(state.rows.saturating_sub(1));
            let col = col1.saturating_sub(1).min(state.cols.saturating_sub(1));
            state.cursor.row = row;
            state.cursor.col = col;
        }
        b'K' => {
            let mode = parse_one_param(params, 0);
            erase_line(state, mode);
        }
        b'J' => {
            let mode = parse_one_param(params, 0);
            erase_display(state, mode);
        }
        b'A' => {
            let n = parse_one_param(params, 1).max(1);
            state.cursor.row = state.cursor.row.saturating_sub(n);
        }
        b'B' => {
            let n = parse_one_param(params, 1).max(1);
            state.cursor.row = (state.cursor.row + n).min(state.rows.saturating_sub(1));
        }
        b'C' => {
            let n = parse_one_param(params, 1).max(1);
            state.cursor.col = (state.cursor.col + n).min(state.cols.saturating_sub(1));
        }
        b'D' => {
            let n = parse_one_param(params, 1).max(1);
            state.cursor.col = state.cursor.col.saturating_sub(n);
        }
        _ => {
            log::trace!(
                target: "WarpTerminalModel",
                "CSI unknown final 0x{:02X} params={:?}", final_byte, params
            );
        }
    }
}

fn parse_two_params(params: &[u8], default_a: usize, default_b: usize) -> (usize, usize) {
    let s = std::str::from_utf8(params).unwrap_or("");
    let mut iter = s.split(';');
    let a = iter
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(default_a);
    let b = iter
        .next()
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(default_b);
    (a, b)
}

fn parse_one_param(params: &[u8], default: usize) -> usize {
    let s = std::str::from_utf8(params).unwrap_or("");
    let head = s.split(';').next().unwrap_or("");
    if head.is_empty() {
        default
    } else {
        head.parse::<usize>().unwrap_or(default)
    }
}

fn apply_sgr(state: &mut TerminalState, params: &[u8]) {
    let s = std::str::from_utf8(params).unwrap_or("");
    let codes: Vec<u32> = if s.is_empty() {
        vec![0]
    } else {
        s.split(';')
            .map(|p| {
                if p.is_empty() {
                    0
                } else {
                    p.parse::<u32>().unwrap_or(0)
                }
            })
            .collect()
    };

    for &code in &codes {
        match code {
            0 => {
                state.cur_fg = DEFAULT_FG;
                state.cur_bg = DEFAULT_BG;
                state.cur_attrs = 0;
            }
            1 => state.cur_attrs |= ATTR_BOLD,
            2 => state.cur_attrs |= ATTR_DIM,
            3 => state.cur_attrs |= ATTR_ITALIC,
            4 => state.cur_attrs |= ATTR_UNDERLINE,
            7 => state.cur_attrs |= ATTR_REVERSE,
            22 => state.cur_attrs &= !(ATTR_BOLD | ATTR_DIM),
            23 => state.cur_attrs &= !ATTR_ITALIC,
            24 => state.cur_attrs &= !ATTR_UNDERLINE,
            27 => state.cur_attrs &= !ATTR_REVERSE,
            30..=37 => {
                state.cur_fg = ANSI_STANDARD_COLORS[(code - 30) as usize];
            }
            39 => state.cur_fg = DEFAULT_FG,
            40..=47 => {
                state.cur_bg = ANSI_STANDARD_COLORS[(code - 40) as usize];
            }
            49 => state.cur_bg = DEFAULT_BG,
            90..=97 => {
                state.cur_fg = ANSI_BRIGHT_COLORS[(code - 90) as usize];
            }
            100..=107 => {
                state.cur_bg = ANSI_BRIGHT_COLORS[(code - 100) as usize];
            }
            _ => {
                log::trace!(
                    target: "WarpTerminalModel",
                    "SGR code {} not recognized in M3-S05 baseline", code
                );
            }
        }
    }
    state.sgr_apply_count = state.sgr_apply_count.saturating_add(1);
    log::debug!(
        target: "WarpTerminalModel",
        "sgr_color codes={:?} fg=0x{:08X} bg=0x{:08X} attrs=0x{:02X}",
        codes, state.cur_fg, state.cur_bg, state.cur_attrs
    );
}

fn erase_line(state: &mut TerminalState, mode: usize) {
    let row = state.cursor.row.min(state.rows.saturating_sub(1));
    let col = state.cursor.col.min(state.cols.saturating_sub(1));
    let cols = state.cols;
    match mode {
        0 => {
            for c in col..cols {
                state.cells[row][c] = Cell::blank();
            }
        }
        1 => {
            for c in 0..=col {
                state.cells[row][c] = Cell::blank();
            }
        }
        2 => {
            for c in 0..cols {
                state.cells[row][c] = Cell::blank();
            }
        }
        _ => {}
    }
}

fn erase_display(state: &mut TerminalState, mode: usize) {
    let rows = state.rows;
    let cols = state.cols;
    match mode {
        0 => {
            erase_line(state, 0);
            let start_row = state.cursor.row + 1;
            for r in start_row..rows {
                for c in 0..cols {
                    state.cells[r][c] = Cell::blank();
                }
            }
        }
        1 => {
            for r in 0..state.cursor.row {
                for c in 0..cols {
                    state.cells[r][c] = Cell::blank();
                }
            }
            erase_line(state, 1);
        }
        2 => {
            for r in 0..rows {
                for c in 0..cols {
                    state.cells[r][c] = Cell::blank();
                }
            }
        }
        _ => {}
    }
}

/// Decode + log a finished DCS body. The mirror does NOT deserialize into
/// `DProtoHook` — it only logs the JSON bytes for device-side verification.
/// Full DProtoHook plumbing lives in the canonical facade module
/// (`warp-src/crates/warp_terminal_mobile_facade/src/render.rs`); the
/// device-side AC#7 test only checks SGR rendering, not Block aggregation
/// (which is M3-S07).
fn finish_dcs(state: &mut TerminalState, is_hex: bool, body: &[u8]) {
    let json_bytes: Vec<u8> = if is_hex {
        match hex_decode(body) {
            Ok(v) => v,
            Err(e) => {
                state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                log::warn!(
                    target: "WarpTerminalModel",
                    "DCS hex decode failed: {} (body_len={})",
                    e, body.len()
                );
                return;
            }
        }
    } else {
        body.to_vec()
    };

    state.dcs_hook_count = state.dcs_hook_count.saturating_add(1);
    log::info!(
        target: "WarpTerminalModel",
        "dcs_hook seq={} body_len={} json={:?}",
        state.dcs_hook_count,
        json_bytes.len(),
        // Truncate large bodies in the log so logcat stays readable.
        String::from_utf8_lossy(&json_bytes[..json_bytes.len().min(256)])
    );
}

fn hex_decode(hex_chars: &[u8]) -> Result<Vec<u8>, &'static str> {
    if hex_chars.len() % 2 != 0 {
        return Err("odd-length hex body");
    }
    let mut out = Vec::with_capacity(hex_chars.len() / 2);
    for pair in hex_chars.chunks_exact(2) {
        let hi = hex_to_nibble(pair[0])?;
        let lo = hex_to_nibble(pair[1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn hex_to_nibble(b: u8) -> Result<u8, &'static str> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err("non-hex byte in DCS body"),
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

pub fn global_model() -> &'static TerminalModel {
    GLOBAL_MODEL.get_or_init(TerminalModel::new_default)
}

pub fn ingest_pty_bytes(bytes: &[u8]) -> usize {
    global_model().ingest_pty_bytes(bytes)
}

pub fn take_dirty() -> bool {
    global_model().take_dirty()
}

pub fn snapshot_text() -> String {
    global_model().snapshot_text()
}

pub fn dims() -> (usize, usize) {
    global_model().dims()
}

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
        let r1 = global_model() as *const _;
        let r2 = global_model() as *const _;
        assert_eq!(r1, r2);
    }

    // ── M3-S05 mirror tests ────────────────────────────────────────────

    #[test]
    fn sgr_red_then_reset_paints_cells() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1b[31mRED\x1b[0m green");
        let cell_r = m.cell(0, 0).unwrap();
        let cell_g = m.cell(0, 4).unwrap();
        assert_eq!(cell_r.glyph, 'R');
        assert_eq!(cell_r.fg, ANSI_STANDARD_COLORS[1]);
        assert_eq!(cell_g.glyph, 'g');
        assert_eq!(cell_g.fg, DEFAULT_FG);
        let (sgr_count, _hooks, errs) = m.parser_stats();
        assert_eq!(sgr_count, 2);
        assert_eq!(errs, 0);
    }

    #[test]
    fn sgr_bright_colors_apply() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1b[91;104mX");
        let cell = m.cell(0, 0).unwrap();
        assert_eq!(cell.fg, ANSI_BRIGHT_COLORS[1]); // bright red
        assert_eq!(cell.bg, ANSI_BRIGHT_COLORS[4]); // bright blue
    }

    /// Mirror of the canonical facade test
    /// `dcs_streaming_emits_preexec_hook` — uses the same hex DCS frame
    /// encoder so the wire format is verified end-to-end at this layer too.
    #[test]
    fn dcs_streaming_increments_hook_count() {
        let m = TerminalModel::new(2, 16);
        let json = r#"{"hook":"Preexec","value":{"command":"ls"}}"#;
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1bP$d");
        for &b in json.as_bytes() {
            frame.extend_from_slice(format!("{:02x}", b).as_bytes());
        }
        frame.push(0x9c);
        m.ingest_pty_bytes(&frame);
        let (_sgr, hooks, errs) = m.parser_stats();
        assert_eq!(hooks, 1, "exactly one DCS hook decoded");
        assert_eq!(errs, 0);
    }

    #[test]
    fn dcs_malformed_hex_resets_to_ground() {
        let m = TerminalModel::new(2, 16);
        // Non-hex byte in body.
        m.ingest_pty_bytes(b"\x1bP$d7b22!");
        m.ingest_pty_bytes(b"after");
        let (_sgr, hooks, errs) = m.parser_stats();
        assert_eq!(hooks, 0);
        assert_eq!(errs, 1);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'a');
        assert_eq!(m.cell(0, 4).unwrap().glyph, 'r');
    }

    #[test]
    fn csi_cursor_position_works() {
        let m = TerminalModel::new(8, 16);
        m.ingest_pty_bytes(b"\x1b[5;3HX");
        assert_eq!(m.cell(4, 2).unwrap().glyph, 'X');
    }
}
