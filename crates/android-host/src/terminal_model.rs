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

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

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

/// M3-S09 — maximum number of historical lines retained in the scrollback
/// ring (mirror of facade `render::SCROLLBACK_MAX_LINES`).
pub const SCROLLBACK_MAX_LINES: usize = 1000;

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
    /// Codex round-1 finding #3: non-warp DCS markers and cap-overflow
    /// aborts route here. Consume bytes until ST without bumping
    /// dcs_hook_count or dcs_error_count (cap-overflow callers bump
    /// dcs_error_count BEFORE entering this state).
    DcsIgnoreUntilST,
    /// 7-bit ST handler for the ignore path — distinguished from
    /// `DcsFinish7Bit` so we don't dispatch through `finish_dcs`.
    DcsIgnoreFinish7Bit,
    /// V1-prep iteration 21 (2026-05-02): OSC sequence body consumer.
    /// Used for OSC 0/2 (terminal title), OSC 7 (cwd), OSC 8 (hyperlinks),
    /// OSC 133 (Warp Block-model prompt-mode markers), OSC 9/777
    /// (notifications), etc. Currently consumed silently — Warp's Block
    /// model receives the same metadata via the DCS hook channel
    /// (M3-S07) so OSC 133 is redundant for our pipeline. The previous
    /// terminal_model state machine had no OSC variant, which caused
    /// `\x1b]133;Alocalhost%\x1b\\` to be eaten as `]` (Esc fall-through)
    /// + `133;Alocalhost%` typed as visible glyphs in the grid (user-
    /// reported "133;Alocalhost% 133;B" leak in iteration 20).
    OscString(Vec<u8>),
    /// 7-bit ST handler for OSC: ESC was seen inside the OSC body, so
    /// the next byte is either `\\` (ST → Ground) or anything else
    /// (treat as ESC + that byte by re-entering Esc state).
    OscFinish7Bit(Vec<u8>),
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

// ── M3-S07 — Block aggregation (mirror) ─────────────────────────────────────
//
// The canonical `warp_terminal_mobile_facade::app_terminal::model::{Block,
// BlockList, BlockEvent}` lives in the warp-src facade crate. The mirror
// here is a hand-shaped reimplementation that stays Cargo-edge-free
// (otherwise we'd pull in the entire facade rlib into the cdylib —
// M3-S11 unification scope). The wire shapes are bit-compatible so the
// device-test driver `tools/scripts/test-block-model.sh` produces the
// same JSON dump output regardless of which path runs.

/// Mirror-side Block — the M3-essential subset of upstream
/// `app::terminal::model::Block` per Plan Amendment 5 + AC#1. Wire-compatible
/// with `warp_terminal_mobile_facade::app_terminal::model::block::Block`.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Block {
    id: String,
    /// UNIX millis (the canonical facade uses `SystemTime`; mirror stores
    /// the converted u64 directly so the JNI dump path is zero-cost).
    start_time_unix_ms: u64,
    command: String,
    exit_code: Option<i32>,
    end_time_unix_ms: Option<u64>,
    /// V1-prep: captured output bytes between Preexec and CommandFinished.
    /// Capped at [`Block::OUTPUT_CAP_BYTES`] to bound memory on `find /`-
    /// style commands. Stored as bytes (not String) so partial multi-byte
    /// UTF-8 sequences at the cap boundary don't panic; JSON serialization
    /// does a lossy decode at dump time.
    ///
    /// Replaces the M3-S07-era `output_range_start`/`output_range_end`
    /// fields (cell-stream indices, never populated) — `output: Vec<u8>`
    /// is the model that materialized.
    output: Vec<u8>,
}

impl Block {
    /// Per-block output capture cap. 64 KB is enough for typical command
    /// output (`ls -la`, `git log -10`, `du -sh /`) and keeps total memory
    /// bounded even with thousands of blocks. Beyond this we drop bytes
    /// silently — the user can still see the live output in the grid /
    /// scrollback, just not in the captured Block.output for AI context.
    pub const OUTPUT_CAP_BYTES: usize = 64 * 1024;

    pub fn new_pending(id: String, start_time_unix_ms: u64) -> Self {
        Self {
            id,
            start_time_unix_ms,
            command: String::new(),
            exit_code: None,
            end_time_unix_ms: None,
            output: Vec::new(),
        }
    }

    pub fn set_command(&mut self, command: String) {
        self.command = command;
    }

    pub fn finalize(&mut self, exit_code: i32, end_time_unix_ms: u64) {
        self.exit_code = Some(exit_code);
        self.end_time_unix_ms = Some(end_time_unix_ms);
    }

    /// Append a byte to this block's captured output buffer. No-op once
    /// the buffer reaches [`Block::OUTPUT_CAP_BYTES`]. Called from
    /// [`handle_ground_byte`] when the aggregator is in "capturing" mode
    /// (between Preexec and CommandFinished).
    pub fn append_output_byte(&mut self, b: u8) {
        if self.output.len() < Self::OUTPUT_CAP_BYTES {
            self.output.push(b);
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn command(&self) -> &str {
        &self.command
    }
    pub fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }
    pub fn start_time_unix_ms(&self) -> u64 {
        self.start_time_unix_ms
    }
    pub fn end_time_unix_ms(&self) -> Option<u64> {
        self.end_time_unix_ms
    }
    pub fn output_bytes(&self) -> &[u8] {
        &self.output
    }
}

/// Mirror-side BlockList — `Vec<Block>` aggregator with `to_dump_json` for
/// the JNI debug accessor.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BlockList {
    blocks: Vec<Block>,
}

impl BlockList {
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }
    pub fn push(&mut self, b: Block) {
        self.blocks.push(b);
    }
    pub fn last_mut(&mut self) -> Option<&mut Block> {
        self.blocks.last_mut()
    }
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Mutable slice access — used by V1-prep output capture in
    /// [`handle_ground_byte`] to append stdout bytes to a specific
    /// block by index. Not exposed publicly for general mutation
    /// because the BlockList invariant is "append-only via push +
    /// last_mut for the current block"; index-mutation is allowed
    /// only for this narrow capture path.
    pub fn blocks_mut(&mut self) -> &mut [Block] {
        &mut self.blocks
    }

    /// JSON array of `{id, start_time_unix_ms, command, exit_code,
    /// end_time_unix_ms}`. Wire-format identical to the canonical facade
    /// `BlockList::to_dump_json`.
    pub fn to_dump_json(&self) -> String {
        let mut out = String::with_capacity(64 + 80 * self.blocks.len());
        out.push('[');
        for (i, b) in self.blocks.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            let id = json_escape(&b.id);
            let cmd = json_escape(&b.command);
            let exit = match b.exit_code {
                Some(ec) => format!("{}", ec),
                None => "null".to_string(),
            };
            let end = match b.end_time_unix_ms {
                Some(t) => format!("{}", t),
                None => "null".to_string(),
            };
            // V1-prep: include captured output bytes (UTF-8-lossy decode).
            // The mirror keeps Block.output as Vec<u8> so partial multi-byte
            // sequences at the OUTPUT_CAP_BYTES boundary don't panic. The
            // canonical facade Block doesn't expose this field; the Kotlin
            // consumer (BlockActionsSheet) already does optString("output",
            // "") so absence in the facade-side dump is graceful.
            let output_str = String::from_utf8_lossy(&b.output);
            let output_escaped = json_escape(&output_str);
            out.push_str(&format!(
                "{{\"id\":\"{}\",\"start_time_unix_ms\":{},\"command\":\"{}\",\"exit_code\":{},\"end_time_unix_ms\":{},\"output\":\"{}\"}}",
                id, b.start_time_unix_ms, cmd, exit, end, output_escaped
            ));
        }
        out.push(']');
        out
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// Mirror-side BlockEvent — same enum shape as the canonical facade
/// `app_terminal::model::BlockEvent`. Mirror tests assert against this.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BlockEvent {
    Start { block_id: String, start_time_unix_ms: u64 },
    Preexec { block_id: String, command: String },
    End { block_id: String, exit_code: i32, end_time_unix_ms: u64 },
}

impl BlockEvent {
    pub fn name(&self) -> &'static str {
        match self {
            BlockEvent::Start { .. } => "Start",
            BlockEvent::Preexec { .. } => "Preexec",
            BlockEvent::End { .. } => "End",
        }
    }
    pub fn block_id(&self) -> &str {
        match self {
            BlockEvent::Start { block_id, .. } => block_id,
            BlockEvent::Preexec { block_id, .. } => block_id,
            BlockEvent::End { block_id, .. } => block_id,
        }
    }
}

/// Hand-decoded subset of upstream `DProtoHook` covering only the three
/// variants that drive Block aggregation. The canonical facade
/// (`warp_terminal_mobile_facade::app_terminal::ansi::dcs_hooks`) carries
/// the full ~17 variants; the mirror stays slim by parsing only what M3-S07
/// needs and ignoring everything else.
///
/// Wire format matches upstream's `#[serde(tag = "hook", content = "value")]`
/// adjacent tagging so the same JSON payloads round-trip in both paths.
#[derive(Debug, Deserialize)]
#[serde(tag = "hook", content = "value")]
enum MirrorHook {
    Precmd(MirrorPrecmd),
    Preexec(MirrorPreexec),
    CommandFinished(MirrorCommandFinished),
    /// All other hook variants (Bootstrapped, SSH, InputBuffer, …) parse
    /// into this catchall via `serde::de::IgnoredAny` so the mirror
    /// doesn't error on M5+ payloads.
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize, Default)]
struct MirrorPrecmd {
    #[serde(default)]
    session_id: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct MirrorPreexec {
    #[serde(default)]
    command: String,
}

#[derive(Debug, Deserialize, Default)]
struct MirrorCommandFinished {
    #[serde(default)]
    exit_code: i32,
    /// We accept this field for wire-format completeness (matches upstream
    /// `CommandFinishedValue`) but the mirror aggregator doesn't currently
    /// consume it — the *next* Precmd creates the next Block, so the
    /// hint here is redundant. Tagged `#[allow(dead_code)]` so future
    /// reviewers don't drop the field thinking it's unused; the canonical
    /// facade carries it identically.
    #[serde(default)]
    #[allow(dead_code)]
    next_block_id: String,
}

/// Parse the JSON body from a successful `finish_dcs` decode into a
/// `MirrorHook`. Returns `None` on parse failure (caller bumps
/// `dcs_error_count`).
fn parse_dcs_hook(json: &[u8]) -> Option<MirrorHook> {
    serde_json::from_slice::<MirrorHook>(json).ok()
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

    // ── M3-S07: Block aggregation (mirror) ──────────────────────────────
    blocks: BlockList,
    block_events: Vec<BlockEvent>,
    /// V1-prep: index into `blocks` of the block currently capturing
    /// stdout/stderr bytes. Set on Preexec, cleared on CommandFinished.
    /// `None` means "not in capture mode" — typical between blocks
    /// (after a CommandFinished and before the next Precmd) or at startup.
    /// Used by [`handle_ground_byte`] to also append printable bytes to
    /// the targeted block's output buffer.
    capturing_to_block_idx: Option<usize>,

    // ── M3-S09: scrollback ring + viewport offset (mirror) ─────────────
    scrollback: VecDeque<Vec<Cell>>,
    scroll_offset: usize,

    // ── V1-prep iteration 40 (2026-05-03): UTF-8 continuation buffer ───
    // handle_ground_byte was previously casting every PTY byte to char as
    // Latin-1, splitting multi-byte sequences across cells (你 → ä,½, ).
    // utf8_buf accumulates continuation bytes for the in-progress
    // codepoint; when the lead byte's expected length is reached, the
    // sequence is decoded and written as ONE char to one cell.
    utf8_buf: [u8; 4],
    utf8_len: u8,      // bytes currently buffered (0..=4)
    utf8_expect: u8,   // total bytes expected for the in-progress codepoint
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
                blocks: BlockList::new(),
                block_events: Vec::new(),
                capturing_to_block_idx: None,
                scrollback: VecDeque::with_capacity(SCROLLBACK_MAX_LINES),
                scroll_offset: 0,
                utf8_buf: [0; 4],
                utf8_len: 0,
                utf8_expect: 0,
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

    // ── M3-S09: scrollback + viewport offset accessors (mirror) ─────────

    /// Mirror of `facade::render::TerminalModel::set_scroll_offset`. Updates
    /// the viewport offset (0 = live tail) and sets the dirty flag so the
    /// Choreographer re-inits the GPU grid on the next vsync.
    ///
    /// M3-S09 round-2: returns the actual clamped offset (mirror of facade
    /// signature change) so the JNI export can return it to Kotlin and
    /// `currentScrollOffsetRows` stays in sync with `scrollback.len()`.
    pub fn set_scroll_offset(&self, offset: usize) -> usize {
        let mut state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let clamped = offset.min(state.scrollback.len());
        if clamped != state.scroll_offset {
            state.scroll_offset = clamped;
            self.dirty.store(true, Ordering::Release);
        }
        clamped
    }

    pub fn scroll_offset(&self) -> usize {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.scroll_offset
    }

    pub fn scrollback_len(&self) -> usize {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.scrollback.len()
    }

    pub fn scrollback_max_lines(&self) -> usize {
        SCROLLBACK_MAX_LINES
    }

    pub fn snapshot_text(&self) -> String {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let view = snapshot_view(&state);
        let mut out = String::with_capacity(state.rows * (state.cols + 1));
        for (r, row) in view.iter().enumerate() {
            for cell in row {
                out.push(cell.glyph);
            }
            if r + 1 < state.rows {
                out.push('\n');
            }
        }
        out
    }

    /// M3-S08: clone the full per-cell grid for the dynamic_grid renderer.
    /// Returns `rows × cols` of [`Cell`] preserving glyph + fg/bg/attrs.
    /// Bounded copy (24×80 = 1920 cells × 16 bytes = ~30KB per frame on the
    /// hot path; flagship S24U can absorb this at 60Hz).
    ///
    /// M3-S09: honors `scroll_offset` so a scrolled-up viewport returns the
    /// historical rows from the scrollback ring at the top of the grid.
    pub fn snapshot_cells(&self) -> Vec<Vec<Cell>> {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        snapshot_view(&state)
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

    // ── M3-S07: Block aggregation accessors (mirror) ───────────────────────

    /// Returns the number of Blocks aggregated. Surfaced via JNI for the
    /// `tools/scripts/test-block-model.sh` driver.
    pub fn block_count(&self) -> usize {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.blocks.len()
    }

    /// Serialize the BlockList as JSON; consumed by the JNI accessor
    /// `Java_dev_warp_mobile_NativeBridge_terminalBlocksDump` and the
    /// `test-block-model.sh` device driver.
    pub fn blocks_dump_json(&self) -> String {
        let state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        state.blocks.to_dump_json()
    }

    /// Drain the BlockEvent log for tests / M3-S08 renderer invalidation.
    pub fn drain_block_events(&self) -> Vec<BlockEvent> {
        let mut state = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        std::mem::take(&mut state.block_events)
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
            // V1-prep iteration 21: OSC start. Consume silently until ST
            // (ESC \ or 0x9c or BEL 0x07). Without this, OSC 133 prompt-
            // mode markers + OSC 0/2 title sets + OSC 7 cwd + OSC 8
            // hyperlinks all leaked as visible glyphs in the cell grid.
            b']' => AnsiState::OscString(Vec::new()),
            b'\\' => AnsiState::Ground,
            // 0x40..=0x5F was the catch-all that previously consumed `]`
            // as a no-op. Now `]` is handled above so this range matches
            // only the rest of the C1 controls (e.g. ESC @ = NUL, ESC ^ =
            // PM, ESC _ = APC) which we still discard. Note: PM (^) and
            // APC (_) are like OSC in that they need a string body
            // consumer; for now we drop them on the floor since they're
            // rare enough that the leak isn't user-visible. If users
            // start seeing PM/APC payloads, add OscString-shaped variants
            // for them.
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
                    AnsiState::Csi(buf)
                } else {
                    // Codex round-1 nit: CSI buf cap exceeded → abort.
                    // Bumps dcs_error_count (parser_stats triple is
                    // exposed via JNI; can't add csi_error_count without
                    // breaking the ABI).
                    state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                    log::warn!(
                        target: "WarpTerminalModel",
                        "CSI param buffer cap reached ({} bytes); aborting", CSI_BUF_CAP
                    );
                    AnsiState::Ground
                }
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
            // Bare 0x9c terminates the empty DCS (8-bit ST). Bare ESC must
            // route into the IGNORE 7-bit-ST path; routing through
            // DcsFinish7Bit would call finish_dcs(false, &[]) and bump
            // dcs_hook_count for `ESC P ESC \\` (codex round-2 finding #1).
            (_, 0x9c) => AnsiState::Ground,
            (_, 0x1b) => AnsiState::DcsIgnoreFinish7Bit,
            // Codex round-1 finding #3: route unknown markers to a true
            // ignore-until-ST state instead of DcsRaw, so dcs_hook_count
            // and dcs_error_count both stay unchanged.
            _ => {
                log::trace!(
                    target: "WarpTerminalModel",
                    "non-warp DCS ignored (marker seen_dollar={} byte=0x{:02X})",
                    seen_dollar, b
                );
                AnsiState::DcsIgnoreUntilST
            }
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
                    AnsiState::DcsHex(buf)
                } else {
                    // Codex round-1 nit: cap overflow → abort.
                    state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                    log::warn!(
                        target: "WarpTerminalModel",
                        "DCS hex body cap reached ({} bytes); aborting + ignoring until ST",
                        DCS_BODY_CAP
                    );
                    AnsiState::DcsIgnoreUntilST
                }
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
                    AnsiState::DcsRaw(buf)
                } else {
                    // Codex round-1 nit: cap overflow → abort.
                    state.dcs_error_count = state.dcs_error_count.saturating_add(1);
                    log::warn!(
                        target: "WarpTerminalModel",
                        "DCS raw body cap reached ({} bytes); aborting + ignoring until ST",
                        DCS_BODY_CAP
                    );
                    AnsiState::DcsIgnoreUntilST
                }
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
        AnsiState::DcsIgnoreUntilST => match b {
            // 8-bit ST → return to Ground without touching counters.
            0x9c => AnsiState::Ground,
            // 7-bit ST opens with ESC; next byte must be '\\'.
            0x1b => AnsiState::DcsIgnoreFinish7Bit,
            // Discard everything else.
            _ => AnsiState::DcsIgnoreUntilST,
        },
        AnsiState::DcsIgnoreFinish7Bit => match b {
            b'\\' => AnsiState::Ground,
            // Stay in ignore mode if the ESC wasn't a valid 7-bit ST.
            _ => AnsiState::DcsIgnoreUntilST,
        },
        // V1-prep iteration 21: OSC body consumer. Spec: OSC = ESC ] ;
            // payload ; ST. ST may be ESC \\ (7-bit, two bytes), 0x9c
            // (8-bit, one byte), or BEL 0x07 (xterm extension, common in
            // OSC 0/2/7/8/133). Consume bytes silently — Warp's Block
            // model already gets the same metadata via DCS hooks
            // (M3-S07), so OSC 133 is redundant for our pipeline.
        AnsiState::OscString(mut buf) => match b {
            // 8-bit ST or BEL — both terminate the OSC and return to ground.
            0x9c | 0x07 => AnsiState::Ground,
            // 7-bit ST starts with ESC; the next byte decides.
            0x1b => AnsiState::OscFinish7Bit(buf),
            // Cap the buffer to avoid unbounded growth from a malformed
            // sequence with no terminator. 4 KB matches the practical
            // OSC 8 hyperlink upper bound.
            _ => {
                if buf.len() < 4096 {
                    buf.push(b);
                }
                AnsiState::OscString(buf)
            }
        },
        AnsiState::OscFinish7Bit(buf) => match b {
            b'\\' => AnsiState::Ground,
            // ESC was followed by something other than `\\` — re-enter
            // OscString with the discarded ESC + this byte appended.
            // (In practice this case is rare; most ESC mid-OSC indicates
            // a stray ESC and the OSC was supposed to end at the prior
            // 0x07 / 0x9c.)
            _ => {
                let mut next = buf;
                if next.len() < 4096 {
                    next.push(0x1b);
                    if next.len() < 4096 {
                        next.push(b);
                    }
                }
                AnsiState::OscString(next)
            }
        },
    };
}

fn handle_ground_byte(state: &mut TerminalState, b: u8) {
    // V1-prep: capture printable + LF/CR bytes into the current block's
    // output buffer if we're between Preexec and CommandFinished. Skip
    // truly invisible control bytes (NUL, BEL, BS, DEL) so the captured
    // output is shell-readable; \n and \r ARE captured because they're
    // structural to multi-line output.
    if let Some(idx) = state.capturing_to_block_idx {
        let capturable = match b {
            0x00 | 0x07 | 0x08 | 0x7F => false,
            // Tab is structural — capture so columns line up in the
            // recorded output (e.g. `ls -la` aligned columns).
            b'\t' | b'\n' | b'\r' => true,
            // Other C0 controls (0x01..=0x1F minus the structural ones
            // above) are typically terminal control codes that aren't
            // meaningful in the captured-text view (e.g. ^G bell).
            c if c < 0x20 => false,
            _ => true,
        };
        if capturable {
            if let Some(block) = state.blocks.blocks_mut().get_mut(idx) {
                block.append_output_byte(b);
            }
        }
    }

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
            // V1-prep iteration 40 (2026-05-03): UTF-8-aware codepoint
            // assembly. Pre-iter-40 this branch did `b as char` which
            // sliced multi-byte sequences across cells (你 = E4 BD A0
            // → 'ä', '½', ' ' visible mojibake). Per-byte algorithm:
            //
            //  - 0x00..=0x7F    ASCII fast-path: write immediately,
            //                   reset any partial-sequence buffer.
            //  - 0xC2..=0xF4    UTF-8 lead: store len, start buffer.
            //  - 0x80..=0xBF    Continuation: append; emit when full.
            //  - other (0x80..=0xC1, 0xF5..=0xFF) - invalid lead;
            //                   write U+FFFD replacement char and reset.
            //
            // On a complete sequence, decode via str::from_utf8 (cheap,
            // returns the canonical char). On invalid continuation, emit
            // U+FFFD and restart from the offending byte. This mirrors
            // the WHATWG UTF-8 decoder's "byte-stream-replacement" mode
            // adapted to a per-byte streaming PTY parser.
            //
            // Note: cell width handling stays single-cell for now even
            // for fullwidth Han chars. Wide-char (East Asian Wide /
            // Fullwidth) layout requires moving the cursor by 2 columns
            // and marking the trailing cell as continuation; deferred to
            // iter-41 (the renderer needs the cell-width metadata too).
            let mut to_emit: Option<char> = None;
            let mut reset_then_buffer_lead = false;
            if state.utf8_expect == 0 {
                // Not in the middle of a sequence.
                if b < 0x80 {
                    to_emit = Some(b as char);
                } else if (0xC2..=0xDF).contains(&b) {
                    state.utf8_expect = 2;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                } else if (0xE0..=0xEF).contains(&b) {
                    state.utf8_expect = 3;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                } else if (0xF0..=0xF4).contains(&b) {
                    state.utf8_expect = 4;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                } else {
                    to_emit = Some('\u{FFFD}');
                }
            } else if (0x80..=0xBF).contains(&b) {
                state.utf8_buf[state.utf8_len as usize] = b;
                state.utf8_len += 1;
                if state.utf8_len == state.utf8_expect {
                    let bytes = &state.utf8_buf[..state.utf8_len as usize];
                    to_emit = match std::str::from_utf8(bytes) {
                        Ok(s) => s.chars().next(),
                        Err(_) => Some('\u{FFFD}'),
                    };
                    state.utf8_len = 0;
                    state.utf8_expect = 0;
                }
            } else {
                // In-progress sequence interrupted by non-continuation
                // byte: emit replacement char for the partial sequence,
                // then re-process `b` as a fresh lead.
                to_emit = Some('\u{FFFD}');
                state.utf8_len = 0;
                state.utf8_expect = 0;
                reset_then_buffer_lead = true;
            }

            if let Some(ch) = to_emit {
                let row = state.cursor.row.min(state.rows - 1);
                let col = state.cursor.col.min(state.cols - 1);
                state.cells[row][col] = Cell {
                    glyph: ch,
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

            if reset_then_buffer_lead {
                // Re-enter the dispatch with the same `b` but state now
                // shows utf8_expect == 0; recurse via tail-call style.
                // Bound recursion via the simple invariant that a single
                // byte cannot trigger another reset (fresh state).
                if b < 0x80 {
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
                } else if (0xC2..=0xDF).contains(&b) {
                    state.utf8_expect = 2;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                } else if (0xE0..=0xEF).contains(&b) {
                    state.utf8_expect = 3;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                } else if (0xF0..=0xF4).contains(&b) {
                    state.utf8_expect = 4;
                    state.utf8_buf[0] = b;
                    state.utf8_len = 1;
                }
                // else: another invalid lead — drop silently to avoid
                // a cascade of replacement chars on a stream of garbage.
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

    let mut i = 0;
    while i < codes.len() {
        let code = codes[i];
        match code {
            0 => {
                state.cur_fg = DEFAULT_FG;
                state.cur_bg = DEFAULT_BG;
                state.cur_attrs = 0;
                i += 1;
            }
            1 => { state.cur_attrs |= ATTR_BOLD; i += 1; }
            2 => { state.cur_attrs |= ATTR_DIM; i += 1; }
            3 => { state.cur_attrs |= ATTR_ITALIC; i += 1; }
            4 => { state.cur_attrs |= ATTR_UNDERLINE; i += 1; }
            7 => { state.cur_attrs |= ATTR_REVERSE; i += 1; }
            22 => { state.cur_attrs &= !(ATTR_BOLD | ATTR_DIM); i += 1; }
            23 => { state.cur_attrs &= !ATTR_ITALIC; i += 1; }
            24 => { state.cur_attrs &= !ATTR_UNDERLINE; i += 1; }
            27 => { state.cur_attrs &= !ATTR_REVERSE; i += 1; }
            30..=37 => {
                state.cur_fg = ANSI_STANDARD_COLORS[(code - 30) as usize];
                i += 1;
            }
            // Codex round-1 finding #2: extended-color escapes consume
            // 5 codes (38;2;R;G;B) or 3 codes (38;5;N) so subsequent SGR
            // codes are not corrupted by the operands. M3-S05 baseline
            // doesn't apply truecolor/256-color (deferred to M3-S08
            // dynamic palette).
            38 | 48 => {
                if i + 1 < codes.len() {
                    match codes[i + 1] {
                        2 => {
                            log::trace!(
                                target: "WarpTerminalModel",
                                "truecolor SGR {};2;R;G;B ignored (M3-S05 baseline; M3-S08 dynamic palette)",
                                code
                            );
                            i += 5;
                        }
                        5 => {
                            log::trace!(
                                target: "WarpTerminalModel",
                                "256-color SGR {};5;N ignored (M3-S05 baseline; M3-S08 dynamic palette)",
                                code
                            );
                            i += 3;
                        }
                        _ => {
                            log::trace!(
                                target: "WarpTerminalModel",
                                "unknown extended SGR escape {};{} (consumed 38/48 only)",
                                code, codes[i + 1]
                            );
                            i += 1;
                        }
                    }
                } else {
                    i += 1;
                }
            }
            39 => { state.cur_fg = DEFAULT_FG; i += 1; }
            40..=47 => {
                state.cur_bg = ANSI_STANDARD_COLORS[(code - 40) as usize];
                i += 1;
            }
            49 => { state.cur_bg = DEFAULT_BG; i += 1; }
            90..=97 => {
                state.cur_fg = ANSI_BRIGHT_COLORS[(code - 90) as usize];
                i += 1;
            }
            100..=107 => {
                state.cur_bg = ANSI_BRIGHT_COLORS[(code - 100) as usize];
                i += 1;
            }
            _ => {
                log::trace!(
                    target: "WarpTerminalModel",
                    "SGR code {} not recognized in M3-S05 baseline", code
                );
                i += 1;
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

    // M3-S07 — Block aggregation. Hand-decode the M3-essential variants
    // (Precmd / Preexec / CommandFinished) and drive the BlockList.
    // Other variants (Bootstrapped, SSH, …) parse into MirrorHook::Other
    // and are no-ops here.
    if let Some(hook) = parse_dcs_hook(&json_bytes) {
        handle_block_hook(state, hook);
    } else {
        log::trace!(
            target: "WarpTerminalModel",
            "block aggregator: hook JSON did not match MirrorHook schema (ignored)"
        );
    }
}

/// M3-S07 mirror — Block aggregation reducer. Functional twin of the
/// canonical facade `render::handle_dcs_hook`.
fn handle_block_hook(state: &mut TerminalState, hook: MirrorHook) {
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    match hook {
        MirrorHook::Precmd(p) => {
            let id = match p.session_id {
                Some(sid) => format!("session-{}-{}", sid, state.blocks.len()),
                None => format!("manual-{:016x}", state.blocks.len() as u64),
            };
            state
                .blocks
                .push(Block::new_pending(id.clone(), now_ms));
            push_block_event(
                state,
                BlockEvent::Start {
                    block_id: id,
                    start_time_unix_ms: now_ms,
                },
            );
        }
        MirrorHook::Preexec(p) => {
            if let Some(b) = state.blocks.last_mut() {
                b.set_command(p.command.clone());
                let id = b.id().to_string();
                push_block_event(
                    state,
                    BlockEvent::Preexec {
                        block_id: id,
                        command: p.command,
                    },
                );
                // V1-prep: start capturing stdout/stderr bytes into
                // this block's output buffer. handle_ground_byte will
                // append from now until the next CommandFinished.
                state.capturing_to_block_idx = Some(state.blocks.len() - 1);
            } else {
                log::warn!(
                    target: "WarpTerminalModel",
                    "Preexec received with no pending block; ignoring (command={:?})",
                    p.command
                );
            }
        }
        MirrorHook::CommandFinished(p) => {
            if let Some(b) = state.blocks.last_mut() {
                b.finalize(p.exit_code, now_ms);
                let id = b.id().to_string();
                push_block_event(
                    state,
                    BlockEvent::End {
                        block_id: id,
                        exit_code: p.exit_code,
                        end_time_unix_ms: now_ms,
                    },
                );
                // V1-prep: stop capturing. The next block (if any) will
                // start capturing on its own Preexec.
                state.capturing_to_block_idx = None;
            } else {
                log::warn!(
                    target: "WarpTerminalModel",
                    "CommandFinished received with no pending block; ignoring (exit_code={})",
                    p.exit_code
                );
            }
        }
        MirrorHook::Other => {}
    }
}

/// Push a [`BlockEvent`] onto the bounded log + emit logcat.
fn push_block_event(state: &mut TerminalState, ev: BlockEvent) {
    log::info!(
        target: "WarpTerminalModel",
        "block_event name={} block_id={}",
        ev.name(),
        ev.block_id(),
    );
    const BLOCK_EVENT_CAP: usize = 128;
    if state.block_events.len() >= BLOCK_EVENT_CAP {
        state.block_events.remove(0);
    }
    state.block_events.push(ev);
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

/// M3-S09 mirror — see facade `render::snapshot_view`. Builds rows × cols
/// snapshot honoring `state.scroll_offset` (0 = live tail; >0 = scrolled
/// into history). Padding for missing top rows is blank cells.
fn snapshot_view(state: &TerminalState) -> Vec<Vec<Cell>> {
    if state.scroll_offset == 0 {
        return state.cells.clone();
    }
    let rows = state.rows;
    let cols = state.cols;
    let scrollback_len = state.scrollback.len();
    let total_stream_len = scrollback_len + rows;
    let last_visible = total_stream_len
        .saturating_sub(1)
        .saturating_sub(state.scroll_offset);
    let first_visible = last_visible.saturating_sub(rows.saturating_sub(1));

    let mut out: Vec<Vec<Cell>> = Vec::with_capacity(rows);
    for r in 0..rows {
        let idx = first_visible + r;
        if idx < scrollback_len {
            if let Some(row) = state.scrollback.get(idx) {
                let mut clone = row.clone();
                if clone.len() < cols {
                    clone.resize(cols, Cell::blank());
                } else if clone.len() > cols {
                    clone.truncate(cols);
                }
                out.push(clone);
            } else {
                out.push(vec![Cell::blank(); cols]);
            }
        } else {
            let live_idx = idx - scrollback_len;
            if live_idx < rows {
                let row = &state.cells[live_idx];
                let mut clone = row.clone();
                if clone.len() < cols {
                    clone.resize(cols, Cell::blank());
                } else if clone.len() > cols {
                    clone.truncate(cols);
                }
                out.push(clone);
            } else {
                out.push(vec![Cell::blank(); cols]);
            }
        }
    }
    out
}

fn scroll_up(state: &mut TerminalState) {
    if state.rows < 2 {
        for c in 0..state.cols {
            state.cells[0][c] = Cell::blank();
        }
        return;
    }
    // M3-S09: push the departing row into the bounded scrollback ring.
    let departing = state.cells.remove(0);
    state.scrollback.push_back(departing);
    while state.scrollback.len() > SCROLLBACK_MAX_LINES {
        state.scrollback.pop_front();
    }
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

/// M3-S08: process-wide accessor returning the full cell grid for the
/// dynamic_grid renderer.
pub fn snapshot_cells() -> Vec<Vec<Cell>> {
    global_model().snapshot_cells()
}

/// V1-prep iteration 32: process-wide accessor for cursor row/col. Used by
/// `terminalTakeDirtyAndPushFrame` to bottom-anchor short content under the
/// composer instead of leaving the active prompt row pinned at the top of
/// the viewport with empty rows below it (the user's "反著來" complaint).
pub fn cursor_position() -> Cursor {
    global_model().cursor()
}

pub fn dims() -> (usize, usize) {
    global_model().dims()
}

pub fn resize(rows: usize, cols: usize) {
    global_model().resize(rows, cols);
}

/// M3-S07: process-wide accessor for the JNI `terminalBlocksDump` export.
pub fn blocks_dump_json() -> String {
    global_model().blocks_dump_json()
}

// ── M3-S09: scrollback global accessors ─────────────────────────────────────

/// Set the global terminal model's viewport offset. Surfaced for the JNI
/// `terminalSetScrollOffset` export.
///
/// M3-S09 round-2: returns the actual clamped offset (after Rust clamps to
/// `scrollback.len()`) so the JNI export can hand it back to the Kotlin
/// caller, preventing top-boundary state drift in `currentScrollOffsetRows`.
pub fn set_scroll_offset(offset: usize) -> usize {
    global_model().set_scroll_offset(offset)
}

/// Get the current viewport scroll offset (0 = live tail).
pub fn scroll_offset() -> usize {
    global_model().scroll_offset()
}

/// Number of lines currently held in the scrollback ring.
pub fn scrollback_len() -> usize {
    global_model().scrollback_len()
}

/// Configured scrollback cap (constant [`SCROLLBACK_MAX_LINES`]).
pub fn scrollback_max_lines() -> usize {
    global_model().scrollback_max_lines()
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

    /// V1-prep iteration 21 regression gate: OSC 133 prompt-mode markers
    /// + OSC 0/2 title sets MUST be consumed silently — none of their
    /// payload bytes should ever appear in the rendered cell grid. Before
    /// the OSC handler landed, zsh's `\x1b]133;Alocalhost%\x1b\\` leaked as
    /// "133;Alocalhost%" rendered glyphs (see iteration-20 screenshots).
    #[test]
    fn osc_133_and_title_are_consumed_silently() {
        let m = TerminalModel::new(2, 32);
        // OSC 133;A (start prompt) ESC \\ then literal "%"
        // OSC 0;tab title BEL terminator
        // Then a literal "x" sentinel.
        let bytes: Vec<u8> = b"\x1b]133;A\x1b\\%\x1b]0;localhost\x07x".to_vec();
        m.ingest_pty_bytes(&bytes);
        let snap = m.snapshot_text();
        // Visible glyphs MUST be exactly "%x" — no "133", no "0;", no
        // "Alocalhost", no "localhost".
        assert!(!snap.contains("133"), "OSC 133 payload leaked: {:?}", snap);
        assert!(!snap.contains("Alocalhost"), "OSC 133 payload leaked: {:?}", snap);
        assert!(!snap.contains("localhost"), "OSC 0 title payload leaked: {:?}", snap);
        let trimmed = snap.replace(' ', "").replace('\n', "").replace('\u{0}', "");
        assert_eq!(trimmed, "%x", "expected only '%x' to render, got {:?}", snap);
    }

    /// OSC 8 hyperlink (commonly emitted by GNU ls with --color=auto when
    /// terminal advertises file-clickable support). Format:
    ///   ESC ] 8 ; <params> ; <uri> ESC \\ <text> ESC ] 8 ; ; ESC \\
    /// The text in the middle is normal payload and SHOULD render; the
    /// envelope must be silent.
    #[test]
    fn osc_8_hyperlink_envelope_silent_text_visible() {
        let m = TerminalModel::new(1, 32);
        let bytes: Vec<u8> =
            b"\x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\".to_vec();
        m.ingest_pty_bytes(&bytes);
        let snap = m.snapshot_text();
        assert!(snap.contains("click"), "expected 'click' to render, got {:?}", snap);
        assert!(!snap.contains("8;"), "OSC 8 envelope leaked: {:?}", snap);
        assert!(!snap.contains("example.com"), "OSC 8 URI leaked: {:?}", snap);
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

    // ── M3-S05 round-2: codex finding mirror tests ─────────────────────

    /// Mirror of facade test — codex round-1 finding #2: truecolor SGR
    /// must not parse RGB operands as independent codes.
    #[test]
    fn sgr_truecolor_skips_rgb_operands() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1b[38;2;255;128;0;1mX");
        let cell = m.cell(0, 0).unwrap();
        // Without the fix: `2` sets ATTR_DIM, `0` resets attrs (clearing
        // the bold from the trailing `1`). With the fix: 38;2;R;G;B are
        // consumed as operands, only the trailing `1` applies bold.
        assert_eq!(cell.glyph, 'X');
        assert_eq!(cell.fg, DEFAULT_FG, "truecolor must not apply to fg");
        assert_eq!(cell.attrs & ATTR_BOLD, ATTR_BOLD, "trailing 1 must apply bold");
        assert_eq!(cell.attrs & ATTR_DIM, 0, "operand 2 in 38;2 must NOT set dim");
    }

    /// Mirror of facade test — 256-color must skip the index operand.
    #[test]
    fn sgr_256color_skips_index_operand() {
        let m = TerminalModel::new(2, 16);
        // ESC[38;5;31;1m — 31 standalone is RED fg, but as the 256-color
        // index it must not bleed into cur_fg.
        m.ingest_pty_bytes(b"\x1b[38;5;31;1mY");
        let cell = m.cell(0, 0).unwrap();
        assert_eq!(cell.fg, DEFAULT_FG, "256-color index must not bleed into fg");
        assert_eq!(cell.attrs & ATTR_BOLD, ATTR_BOLD, "trailing 1 must apply bold");
    }

    /// Mirror — codex round-1 finding #3: unknown DCS marker `$x` must
    /// not bump `dcs_hook_count` or `dcs_error_count`.
    #[test]
    fn dcs_unknown_marker_ignored_without_counter_bumps() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1bP$xnonsense\x9c");
        let (sgr, hooks, errs) = m.parser_stats();
        assert_eq!(sgr, 0);
        assert_eq!(hooks, 0, "unknown marker must NOT bump dcs_hook_count");
        assert_eq!(errs, 0, "unknown marker must NOT bump dcs_error_count");
        // Parser back at Ground.
        m.ingest_pty_bytes(b"after");
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'a');
        assert_eq!(m.cell(0, 4).unwrap().glyph, 'r');
    }

    /// Mirror — same ignore behavior for 7-bit ST.
    #[test]
    fn dcs_unknown_marker_ignored_with_7bit_st() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1bP$xstuff\x1b\\done");
        let (sgr, hooks, errs) = m.parser_stats();
        assert_eq!(sgr, 0);
        assert_eq!(hooks, 0);
        assert_eq!(errs, 0);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'd');
        assert_eq!(m.cell(0, 3).unwrap().glyph, 'e');
    }

    /// Mirror — codex round-2 finding #1: `ESC P ESC \\` must NOT bump
    /// any counters. Pre-fix the empty DCS routed through `DcsFinish7Bit`
    /// → `finish_dcs(false, &[])` and the mirror's `finish_dcs` bumped
    /// `dcs_hook_count`. Post-fix: route to `DcsIgnoreFinish7Bit`.
    #[test]
    fn dcs_empty_no_marker_with_7bit_st_no_counter_bumps() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1bP\x1b\\after");
        let (sgr, hooks, errs) = m.parser_stats();
        assert_eq!(sgr, 0);
        assert_eq!(hooks, 0);
        assert_eq!(errs, 0);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'a');
        assert_eq!(m.cell(0, 4).unwrap().glyph, 'r');
    }

    /// Mirror — codex round-2 finding #1: `ESC P $ ESC \\` (no marker
    /// after `$`) must NOT bump any counters either.
    #[test]
    fn dcs_dollar_only_with_7bit_st_no_counter_bumps() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(b"\x1bP$\x1b\\next");
        let (sgr, hooks, errs) = m.parser_stats();
        assert_eq!(sgr, 0);
        assert_eq!(hooks, 0);
        assert_eq!(errs, 0);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'n');
        assert_eq!(m.cell(0, 3).unwrap().glyph, 't');
    }

    /// Mirror — codex round-1 nit: DCS body cap overflow must abort with
    /// `dcs_error_count` bump and recover to Ground.
    #[test]
    fn dcs_hex_body_cap_overflow_aborts_and_recovers() {
        let m = TerminalModel::new(2, 16);
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1bP$d");
        let payload_size = DCS_BODY_CAP + 6 * 1024;
        frame.extend(std::iter::repeat(b'a').take(payload_size));
        frame.push(0x9c);
        m.ingest_pty_bytes(&frame);

        let (_sgr, hooks, errs) = m.parser_stats();
        assert_eq!(hooks, 0, "no hook should dispatch when cap is hit");
        assert_eq!(errs, 1, "exactly one error recorded for cap overflow");

        m.ingest_pty_bytes(b"after");
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'a');
        assert_eq!(m.cell(0, 4).unwrap().glyph, 'r');
    }

    // ── M3-S07 NEW: Block aggregation mirror tests ─────────────────────

    /// Helper — build a hex-encoded DCS frame for a JSON payload (mirrors
    /// the upstream `warp_send_json_message` wire format at
    /// `zsh_body.sh:90`).
    fn build_dcs_frame_local(json: &str) -> Vec<u8> {
        let mut frame = Vec::new();
        frame.extend_from_slice(b"\x1bP$d");
        for &b in json.as_bytes() {
            frame.extend_from_slice(format!("{:02x}", b).as_bytes());
        }
        frame.push(0x9c);
        frame
    }

    /// Mirror AC#3 #1 — Three Precmd+Preexec+CommandFinished triplets via
    /// real DCS frames produce three Blocks with the right command +
    /// exit_code.
    #[test]
    fn block_aggregation_three_triplets_via_dcs_frames_mirror() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/d","ps1":"$","session_id":7}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"ls"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-7-1"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/d","ps1":"$","session_id":7}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"whoami"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-7-2"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/d","ps1":"$","session_id":7}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"false"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"CommandFinished","value":{"exit_code":1,"next_block_id":"session-7-3"}}"#,
        ));

        assert_eq!(m.block_count(), 3);
        let json = m.blocks_dump_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let arr = v.as_array().expect("array");
        assert_eq!(arr.len(), 3);
        assert_eq!(arr[0]["command"], "ls");
        assert_eq!(arr[0]["exit_code"], 0);
        assert_eq!(arr[1]["command"], "whoami");
        assert_eq!(arr[1]["exit_code"], 0);
        assert_eq!(arr[2]["command"], "false");
        assert_eq!(arr[2]["exit_code"], 1);
    }

    /// Mirror AC#3 #2 — Bootstrapped + Clear hooks do NOT push Blocks.
    /// Confirms `MirrorHook::Other` catchall works.
    #[test]
    fn block_aggregation_ignores_non_block_hooks_mirror() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Bootstrapped","value":{"histfile":"/h/.zsh_history","shell":"zsh","home_dir":"/h","path":"/u/b","editor":"vim","aliases":"","abbreviations":"","function_names":"","env_var_names":"","builtins":"","keywords":"","shell_version":"5.9"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Clear","value":{}}"#,
        ));

        assert_eq!(m.block_count(), 0);
        let (_sgr, hooks, _errs) = m.parser_stats();
        assert_eq!(hooks, 2, "both hooks decoded");
    }

    /// Mirror AC#3 #3 — Preexec without prior Precmd is dropped (no Block
    /// pushed, no panic).
    #[test]
    fn block_aggregation_preexec_without_precmd_is_dropped_mirror() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"orphan"}}"#,
        ));
        assert_eq!(m.block_count(), 0);
        assert_eq!(m.drain_block_events().len(), 0);
        let (_sgr, hooks, _errs) = m.parser_stats();
        assert_eq!(hooks, 1);
    }

    /// Mirror AC#3 #4 — `blocks_dump_json` shape matches the canonical
    /// facade dump format (id / start_time_unix_ms / command / exit_code
    /// / end_time_unix_ms).
    #[test]
    fn blocks_dump_json_shape_matches_canonical_facade() {
        let m = TerminalModel::new(2, 16);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/x","ps1":"$","session_id":99}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"echo hello"}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-99-1"}}"#,
        ));

        let json = m.blocks_dump_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let entry = &v[0];
        for key in [
            "id",
            "start_time_unix_ms",
            "command",
            "exit_code",
            "end_time_unix_ms",
            "output",
        ] {
            assert!(entry.get(key).is_some(), "missing key: {}", key);
        }
        assert_eq!(entry["command"], "echo hello");
        assert_eq!(entry["exit_code"], 0);
        assert_eq!(entry["output"], "");
    }

    /// V1-prep: Block.output captures bytes between Preexec and
    /// CommandFinished. Verifies the capture state machine + cap.
    #[test]
    fn block_output_captures_stdout_between_preexec_and_finished() {
        let m = TerminalModel::new(4, 32);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/x","ps1":"$","session_id":1}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"echo hi"}}"#,
        ));
        // Output bytes that should be captured.
        m.ingest_pty_bytes(b"hello world\n");
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-1-1"}}"#,
        ));
        // Bytes after CommandFinished should NOT be captured into block 0.
        m.ingest_pty_bytes(b"prompt$");

        let json = m.blocks_dump_json();
        let v: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(v[0]["command"], "echo hi");
        assert_eq!(v[0]["output"], "hello world\n");
    }

    /// V1-prep: capture cap is enforced; bytes beyond OUTPUT_CAP_BYTES
    /// are silently dropped (the live grid still shows them, only the
    /// captured Block.output is bounded).
    #[test]
    fn block_output_cap_drops_overflow_bytes() {
        let m = TerminalModel::new(4, 32);
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Precmd","value":{"pwd":"/x","ps1":"$","session_id":2}}"#,
        ));
        m.ingest_pty_bytes(&build_dcs_frame_local(
            r#"{"hook":"Preexec","value":{"command":"yes"}}"#,
        ));
        // Push more than OUTPUT_CAP_BYTES (64 KB) of printable bytes.
        let cap = Block::OUTPUT_CAP_BYTES;
        let payload = vec![b'a'; cap + 1024];
        m.ingest_pty_bytes(&payload);

        // Block.output should be exactly OUTPUT_CAP_BYTES, not cap + 1024.
        let blocks_snapshot = {
            let s = m.inner.lock().unwrap();
            s.blocks.blocks().to_vec()
        };
        assert_eq!(
            blocks_snapshot[0].output_bytes().len(),
            cap,
            "output should be capped at OUTPUT_CAP_BYTES"
        );
    }

    // ── M3-S09: scrollback + viewport offset (mirror) ──────────────────

    #[test]
    fn scrollback_collects_evicted_rows_mirror() {
        let m = TerminalModel::new(2, 8);
        m.ingest_pty_bytes(b"AA\r\nBB\r\nCC\r\nDD");
        assert_eq!(m.scrollback_len(), 2);
        assert_eq!(m.cell(0, 0).unwrap().glyph, 'C');
        assert_eq!(m.cell(1, 0).unwrap().glyph, 'D');
        assert_eq!(m.scroll_offset(), 0);
    }

    #[test]
    fn scrollback_evicts_when_at_cap_mirror() {
        let m = TerminalModel::new(2, 4);
        let mut payload = String::new();
        for i in 0..(SCROLLBACK_MAX_LINES + 5) {
            payload.push_str(&format!("L{}\n", i % 10));
        }
        m.ingest_pty_bytes(payload.as_bytes());
        assert_eq!(m.scrollback_len(), SCROLLBACK_MAX_LINES);
        assert_eq!(m.scrollback_max_lines(), SCROLLBACK_MAX_LINES);
    }

    #[test]
    fn scroll_offset_shifts_viewport_into_history_mirror() {
        let m = TerminalModel::new(2, 8);
        m.ingest_pty_bytes(b"AA\r\nBB\r\nCC\r\nDD");
        assert_eq!(m.scrollback_len(), 2);

        let snap_live = m.snapshot_cells();
        assert_eq!(snap_live[0][0].glyph, 'C');
        assert_eq!(snap_live[1][0].glyph, 'D');

        m.set_scroll_offset(1);
        let snap = m.snapshot_cells();
        assert_eq!(snap[0][0].glyph, 'B');
        assert_eq!(snap[1][0].glyph, 'C');

        m.set_scroll_offset(2);
        let snap = m.snapshot_cells();
        assert_eq!(snap[0][0].glyph, 'A');
        assert_eq!(snap[1][0].glyph, 'B');

        // Over-scroll clamps.
        // M3-S09 round-2: returns the clamped value so the JNI export can
        // hand it back to Kotlin (no top-boundary drift).
        let clamped = m.set_scroll_offset(999);
        assert_eq!(clamped, 2);
        assert_eq!(m.scroll_offset(), 2);

        let zero = m.set_scroll_offset(0);
        assert_eq!(zero, 0);
        let snap = m.snapshot_cells();
        assert_eq!(snap[0][0].glyph, 'C');
        assert_eq!(snap[1][0].glyph, 'D');
    }

    #[test]
    fn scrollback_holds_at_least_1000_lines_mirror() {
        let m = TerminalModel::new(2, 8);
        for _ in 0..1500 {
            m.ingest_pty_bytes(b"line\n");
        }
        assert!(m.scrollback_len() >= 1000);
        assert!(m.scrollback_len() <= SCROLLBACK_MAX_LINES);
        let clamped_at_cap = m.set_scroll_offset(1000);
        assert_eq!(clamped_at_cap, m.scrollback_len().min(1000));
        assert_eq!(m.scroll_offset(), m.scrollback_len().min(1000));
        // Round-2 boundary check — over-scroll past the cap returns the cap.
        let over = m.set_scroll_offset(SCROLLBACK_MAX_LINES + 500);
        assert_eq!(over, m.scrollback_len());
        assert!(over <= SCROLLBACK_MAX_LINES);
    }
}
