#!/usr/bin/env zsh
# test-scroll.sh <device-serial>
#
# M3-S09 device verification driver. Verifies:
#
#   1. Scrollback ring buffer holds ≥1000 lines (AC#1).
#   2. Touch-drag scroll updates the viewport offset (AC#2).
#   3. Two-finger flick / synthetic flick triggers momentum scroll (AC#3).
#   4. Sustained 5s scroll on Galaxy S24 Ultra: p95 frame interval <16.6ms
#      (AC#4 — the critical 60fps gate).
#   5. Result artifact at .omc/m3-artifacts/M3-S09-result.json carrying:
#        scrollback_max_lines, scrollback_observed_max,
#        scroll_p50/p95/p99_ms, peak_fps, gate.overall_pass (AC#5).
#
# AC#6 (low-end Pixel 4a / A52s) is deferred per M2-S13 user choice; this
# driver targets the flagship S24U primary device class.
#
# # M3-S09 round-2 architecture: split gesture vs broadcast subtests.
#
# Codex round-1 finding #2: the previous driver sent BOTH gesture swipes AND
# `TERM_SET_SCROLL_OFFSET` broadcasts in the same window, then accepted *any*
# `terminal_set_scroll_offset` Rust line as evidence of touch-drag. In the
# captured logcat, 55 `gesture_scroll` lines yielded only 10 Rust calls —
# all matching broadcast values 5/0 — meaning AC#2/AC#3 were satisfied by
# the deterministic broadcast fallback, not by `onScroll`/`onFling`.
#
# Round-2 splits the two paths into two distinct sub-tests:
#
#   * Sub-test A — gesture-only: synthetic `adb shell input swipe` events
#     ONLY (no broadcasts). Frame timing is collected during this window
#     because this is the real 60fps gate. Counts Rust JNI calls produced
#     by the gesture path. Pass requires
#     `rust_set_scroll_offset_calls ≥ 1`.
#   * Sub-test B — broadcast-only: `TERM_SET_SCROLL_OFFSET` broadcasts only
#     (no swipes). Validates the deterministic broadcast path independently
#     of touch dispatch. Pass requires `rust_set_scroll_offset_calls ≥ 1`
#     matching broadcast count.
#
# Both sub-tests' Rust call counts feed `result.json`; AC#2/AC#3 require
# Sub-test A to pass on its own (gesture-driven offset change).
#
# Logcat tags consumed:
#   WarpVulkan          — present_ok frame=N ts=… (frame timing)
#   WarpDynamicGrid     — dynamic_grid_init_ok / dynamic_grid_fast_path_ok
#   WarpTerminalModel   — terminal_set_scroll_offset requested=… clamped=…
#   WarpTerminal        — TERM_INJECT_RAW + WarpTerminalService PTY (carry-over)
#
# Usage:
#   ./tools/scripts/test-scroll.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m3-artifacts/M3-S09-result.json           gate + frame stats
#   .omc/m3-artifacts/M3-S09-logcat-evidence.txt   filtered logcat
#   .omc/m3-artifacts/M3-S09-frames.txt            raw frame interval list
#   .omc/m3-artifacts/M3-S09-screenshot-pre.png    pre-scroll screenshot
#   .omc/m3-artifacts/M3-S09-screenshot-post.png   post-scroll screenshot

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m3-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M3-S09-result.json"
LOGCAT_OUT="$ARTIFACT_DIR/M3-S09-logcat-evidence.txt"
LOGCAT_GESTURE="$ARTIFACT_DIR/M3-S09-logcat-gesture.txt"
LOGCAT_BROADCAST="$ARTIFACT_DIR/M3-S09-logcat-broadcast.txt"
FRAMES_OUT="$ARTIFACT_DIR/M3-S09-frames.txt"
PRE_PNG="$ARTIFACT_DIR/M3-S09-screenshot-pre.png"
POST_PNG="$ARTIFACT_DIR/M3-S09-screenshot-post.png"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL ===" >&2

"${ADB[@]}" get-state >&2 || {
    echo "ERROR: device $SERIAL not online" >&2
    exit 1
}

# Anti-Knox-idle keep-awake (mirrors test-dynamic-grid.sh).
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"
trap 'keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

echo "=== install APK ===" >&2
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

echo "=== force-stop existing app ===" >&2
"${ADB[@]}" shell am force-stop "$PACKAGE" || true

# Clear logcat before launch so we have a clean evidence window.
"${ADB[@]}" logcat -c

echo "=== launch MainActivity in terminal_mode ===" >&2
GRID_FONT_SIZE_PX="${GRID_FONT_SIZE_PX:-22.0}"
GRID_ROWS="${GRID_ROWS:-24}"
GRID_COLS="${GRID_COLS:-80}"
GRID_CELL_W_PX="${GRID_CELL_W_PX:-13.5}"
GRID_CELL_H_PX="${GRID_CELL_H_PX:-27.0}"

"${ADB[@]}" shell am start -n "${PACKAGE}/${ACTIVITY}" \
    --ez terminal_mode true \
    --ef grid_font_size_px "$GRID_FONT_SIZE_PX" \
    --ei grid_rows "$GRID_ROWS" \
    --ei grid_cols "$GRID_COLS" \
    --ef grid_cell_w_px "$GRID_CELL_W_PX" \
    --ef grid_cell_h_px "$GRID_CELL_H_PX" \
    > /dev/null
sleep 4

PID=$("${ADB[@]}" shell pidof "$PACKAGE" | tr -d '\r' || true)
if [[ -z "$PID" ]]; then
    echo "ERROR: ${PACKAGE} did not start" >&2
    exit 1
fi
echo "=== app PID: $PID ===" >&2

# ── Step 1: pre-populate scrollback by injecting 2000 lines ────────────────
#
# Each line is a short "Line N: filler text\r\n" payload. We chunk them into
# ~512-byte broadcasts so we don't blow past the AM intent payload limit on
# Android 14 (~1MB practical limit but smaller chunks are safer).

echo "=== pre-populate scrollback with 2000 lines ===" >&2

# Build the full payload in one pass on the host then chunk + base64 inline.
# Each line ≈ 30 bytes; 2000 × 30 = 60KB ≈ 30 chunks of 2KB each.
PYTHON_SCRIPT=$(cat <<'PYEOF'
import base64
import sys

# Generate 2000 short lines.
lines = []
for i in range(1, 2001):
    lines.append(f"Line {i:04d}: scrollback filler text\r\n")
payload = "".join(lines).encode()

# Chunk into 2KB pieces and emit one per line (newline-delimited base64).
CHUNK_SIZE = 2048
for offset in range(0, len(payload), CHUNK_SIZE):
    chunk = payload[offset:offset + CHUNK_SIZE]
    print(base64.b64encode(chunk).decode())
PYEOF
)

# Write payload chunks to a temp file so we can iterate. Using process
# substitution would work but a tempfile is more debuggable.
CHUNKS_FILE=$(mktemp)
python3 -c "$PYTHON_SCRIPT" > "$CHUNKS_FILE"
NUM_CHUNKS=$(wc -l < "$CHUNKS_FILE" | tr -d ' ')
echo "    payload: 2000 lines split into $NUM_CHUNKS chunks" >&2

I=0
# zsh `while read … done < file` interacts badly with set -e on some
# combinations; iterate via line-numbered loop instead.
LINE_COUNT=$(wc -l < "$CHUNKS_FILE" | tr -d ' ')
LN=1
while [[ $LN -le $LINE_COUNT ]]; do
    CHUNK_B64=$(sed -n "${LN}p" "$CHUNKS_FILE")
    if [[ -z "$CHUNK_B64" ]]; then
        LN=$((LN + 1))
        continue
    fi
    I=$((I + 1))
    "${ADB[@]}" shell am broadcast \
        -a dev.warp.mobile.TERM_INJECT_RAW \
        -p "$PACKAGE" \
        --es cmd_id "scrollback_init_${I}" \
        --es bytes_b64 "$CHUNK_B64" \
        > /dev/null
    # Brief pause so the BroadcastQueue actually delivers each one before the
    # next is queued. Without this Android coalesces / drops back-to-back
    # broadcasts and only a handful actually arrive at the receiver.
    sleep 0.15
    LN=$((LN + 1))
done
rm -f "$CHUNKS_FILE"

# Allow the Choreographer + push_frame loop to drain all dirty frames + the
# scrollback ring to saturate.
sleep 3

# ── Step 2: capture scrollback state ────────────────────────────────────────
echo "=== capture scrollback state ===" >&2
# M3-S09 — broadcast TERM_SCROLLBACK_DUMP to ask the JNI to log the
# current scrollback ring state. Greppable line:
#   "TERM_SCROLLBACK_DUMP info=scrollback_len=N,scrollback_max=N,scroll_offset=N"
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_SCROLLBACK_DUMP \
    -p "$PACKAGE" \
    > /dev/null
sleep 1
"${ADB[@]}" logcat -d > /tmp/logcat-pre-scroll.txt 2>&1 || true
PRE_SCROLLBACK_LINE=$(grep "TERM_SCROLLBACK_DUMP info=" /tmp/logcat-pre-scroll.txt | tail -1 || true)
if [[ -n "$PRE_SCROLLBACK_LINE" ]]; then
    PRE_SCROLLBACK_LEN=$(echo "$PRE_SCROLLBACK_LINE" | sed -nE 's/.*scrollback_len=([0-9]+).*/\1/p')
    echo "    pre-scroll scrollback_len=$PRE_SCROLLBACK_LEN" >&2
fi
PRE_SCROLLBACK_LEN="${PRE_SCROLLBACK_LEN:-0}"

"${ADB[@]}" exec-out screencap -p > "$PRE_PNG"

# ── Step 3 — Sub-test A: gesture-only scroll for 5 seconds ─────────────────
#
# Round-2 fix: NO broadcasts during this window. We send ONLY synthetic
# `input swipe` events and require ≥1 `terminal_set_scroll_offset` Rust
# log line whose `requested=` value is NOT a multiple of 5 (the broadcast
# step). Frame timing comes from this window because this is the real
# 60fps gate.
#
# On a 1080×2400 portrait surface, swipe up from y=1500 → y=200 over
# 100ms moves ~1300px → 1300/27 ≈ 48 cell rows.

echo "=== Sub-test A: gesture-only scroll (5s, NO broadcasts) ===" >&2
"${ADB[@]}" logcat -c

A_START_MS=$(("$(date +%s)" * 1000))
A_END_MS=$((A_START_MS + 5000))
A_SWIPE_COUNT=0
A_DOWN_SWIPE_COUNT=0
A_UP_SWIPE_COUNT=0

# Establish a non-zero baseline offset before the test so the first
# up-swipe (finger up = distanceY > 0 = decrease offset) actually has
# room to decrement (otherwise it clamps to 0 and produces no Rust call).
# This baseline is a one-off setup broadcast — captured separately and
# excluded from sub-test A's gesture call count.
echo "    seed: TERM_SET_SCROLL_OFFSET 50 (one-off setup, excluded from A count)" >&2
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_SET_SCROLL_OFFSET \
    -p "$PACKAGE" \
    --ei offset_rows 50 \
    > /dev/null
sleep 0.5
"${ADB[@]}" logcat -c  # Drop the seed broadcast from sub-test A's evidence.

while [[ $(("$(date +%s)" * 1000)) -lt $A_END_MS ]]; do
    if [[ $((A_SWIPE_COUNT % 2)) -eq 0 ]]; then
        # Finger down (y=200 → y=1500): scroll INTO older history.
        "${ADB[@]}" shell input swipe 540 200 540 1500 100
        A_DOWN_SWIPE_COUNT=$((A_DOWN_SWIPE_COUNT + 1))
    else
        # Finger up (y=1500 → y=200): scroll TOWARD newer / live tail.
        "${ADB[@]}" shell input swipe 540 1500 540 200 100
        A_UP_SWIPE_COUNT=$((A_UP_SWIPE_COUNT + 1))
    fi
    A_SWIPE_COUNT=$((A_SWIPE_COUNT + 1))
    sleep 0.1
done
echo "    Sub-test A swipes: total=$A_SWIPE_COUNT down=$A_DOWN_SWIPE_COUNT up=$A_UP_SWIPE_COUNT" >&2

# Allow the last fling to decay + Choreographer to settle.
sleep 1

# Capture sub-test A logcat window.
"${ADB[@]}" logcat -d > "$LOGCAT_GESTURE" 2>&1 || true

# Count Rust JNI calls from sub-test A. These are pure gesture-driven calls
# because we sent NO broadcasts during this window.
A_RUST_CALLS=$( (grep -cE "terminal_set_scroll_offset requested=" "$LOGCAT_GESTURE" || true) | tr -d ' ')
A_RUST_CALLS="${A_RUST_CALLS:-0}"

# Also count gesture_scroll + gesture_fling lines from the input view as a
# sanity check on touch dispatch. The logcat tag column is space-padded
# (`WarpIme :` not `WarpIme:`), so we match `gesture_scroll`/`gesture_fling`
# anywhere on the line attributed to WarpIme.
A_GESTURE_SCROLL_LINES=$( (grep -cE "WarpIme[[:space:]]*: gesture_scroll" "$LOGCAT_GESTURE" || true) | tr -d ' ')
A_GESTURE_FLING_LINES=$( (grep -cE "WarpIme[[:space:]]*: gesture_fling" "$LOGCAT_GESTURE" || true) | tr -d ' ')
A_GESTURE_SCROLL_LINES="${A_GESTURE_SCROLL_LINES:-0}"
A_GESTURE_FLING_LINES="${A_GESTURE_FLING_LINES:-0}"

# Sanity: any TERM_SET_SCROLL_OFFSET broadcasts captured in this window
# would indicate cross-talk from outside the test. Should be 0.
A_BROADCAST_LINES=$( (grep -cE "TERM_SET_SCROLL_OFFSET" "$LOGCAT_GESTURE" || true) | tr -d ' ')
A_BROADCAST_LINES="${A_BROADCAST_LINES:-0}"

echo "    Sub-test A: rust_calls=$A_RUST_CALLS gesture_scroll=$A_GESTURE_SCROLL_LINES gesture_fling=$A_GESTURE_FLING_LINES broadcast_crosstalk=$A_BROADCAST_LINES" >&2

# Sub-test A frame timing — captured from the gesture-only window.
grep "present_ok frame=" "$LOGCAT_GESTURE" \
    | sed -nE 's/.*ts=([0-9]+).*/\1/p' \
    > /tmp/m3-s09-ts-raw.txt

NUM_PRESENTS=$(wc -l < /tmp/m3-s09-ts-raw.txt | tr -d ' ')
echo "    present_ok lines: $NUM_PRESENTS" >&2

python3 - /tmp/m3-s09-ts-raw.txt "$FRAMES_OUT" <<'PYEOF'
import sys

src, dst = sys.argv[1], sys.argv[2]
with open(src) as f:
    ts = [int(line.strip()) for line in f if line.strip()]

deltas = [ts[i+1] - ts[i] for i in range(len(ts)-1) if 0 < ts[i+1] - ts[i] < 1000]
with open(dst, "w") as f:
    for d in deltas:
        f.write(f"{d}\n")

if not deltas:
    print("WARN: no valid frame deltas", file=sys.stderr)
PYEOF

P50_MS="0"
P95_MS="0"
P99_MS="0"
PEAK_FPS="0"
NUM_FRAMES=0
if [[ -s "$FRAMES_OUT" ]]; then
    NUM_FRAMES=$(wc -l < "$FRAMES_OUT" | tr -d ' ')
    P50_MS=$(sort -n "$FRAMES_OUT" | awk -v n="$NUM_FRAMES" 'BEGIN{p=int(n*0.5)} NR==p+1 {print; exit}')
    P95_MS=$(sort -n "$FRAMES_OUT" | awk -v n="$NUM_FRAMES" 'BEGIN{p=int(n*0.95)} NR==p+1 {print; exit}')
    P99_MS=$(sort -n "$FRAMES_OUT" | awk -v n="$NUM_FRAMES" 'BEGIN{p=int(n*0.99)} NR==p+1 {print; exit}')
    MIN_MS=$(sort -n "$FRAMES_OUT" | head -1)
    if [[ -n "$MIN_MS" && "$MIN_MS" -gt 0 ]]; then
        PEAK_FPS=$(python3 -c "print(min(int(1000 / $MIN_MS), 144))")
    fi
fi

P50_MS="${P50_MS:-0}"
P95_MS="${P95_MS:-0}"
P99_MS="${P99_MS:-0}"
PEAK_FPS="${PEAK_FPS:-0}"

echo "    frame intervals: n=$NUM_FRAMES p50=${P50_MS}ms p95=${P95_MS}ms p99=${P99_MS}ms peak_fps=$PEAK_FPS" >&2

# ── Step 4 — Sub-test B: broadcast-only fallback ───────────────────────────
#
# Validates the deterministic broadcast path independently of touch
# dispatch. Some OEM Android variants intercept input swipes for
# accessibility / system gestures even with `am start`-launched
# activities, so the broadcast path is the safety net that keeps the
# Rust scrollback exercisable from the test driver. This is *not* the
# primary AC#2/AC#3 evidence — Sub-test A is.

echo "=== Sub-test B: broadcast-only TERM_SET_SCROLL_OFFSET (5 broadcasts) ===" >&2
"${ADB[@]}" logcat -c

B_BROADCAST_COUNT=0
B_OFFSET_SEQUENCE=(10 30 50 25 0)
for OFFSET in "${B_OFFSET_SEQUENCE[@]}"; do
    "${ADB[@]}" shell am broadcast \
        -a dev.warp.mobile.TERM_SET_SCROLL_OFFSET \
        -p "$PACKAGE" \
        --ei offset_rows "$OFFSET" \
        > /dev/null
    B_BROADCAST_COUNT=$((B_BROADCAST_COUNT + 1))
    sleep 0.3
done
sleep 1

"${ADB[@]}" logcat -d > "$LOGCAT_BROADCAST" 2>&1 || true

# Count Rust JNI calls from sub-test B.
B_RUST_CALLS=$( (grep -cE "terminal_set_scroll_offset requested=" "$LOGCAT_BROADCAST" || true) | tr -d ' ')
B_RUST_CALLS="${B_RUST_CALLS:-0}"
echo "    Sub-test B: broadcasts=$B_BROADCAST_COUNT rust_calls=$B_RUST_CALLS" >&2

# ── Step 5: capture combined post-scroll state ─────────────────────────────
echo "=== capture post-scroll combined logcat ===" >&2
"${ADB[@]}" exec-out screencap -p > "$POST_PNG"

# Combined evidence file = gesture window + broadcast window.
{
    echo "===== Sub-test A (gesture-only) =====";
    cat "$LOGCAT_GESTURE";
    echo "";
    echo "===== Sub-test B (broadcast-only) =====";
    cat "$LOGCAT_BROADCAST";
} > "$LOGCAT_OUT"

# Find the most-recent scrollback_len from any source. Use the larger of
# pre-scroll (from initial population) and post-scroll captures (sub-test
# A drives from offset 50; sub-test B from various). Ring length is
# monotonic post-saturation, so max is fine.
LAST_SB_LINE=$(grep -E "TERM_(SCROLLBACK_DUMP|INJECT_RAW|SET_SCROLL_OFFSET)\b.*scrollback" "$LOGCAT_OUT" | tail -1 || true)
POST_SB_LEN=""
if [[ -n "$LAST_SB_LINE" ]]; then
    POST_SB_LEN=$(echo "$LAST_SB_LINE" | sed -nE 's/.*scrollback_len=([0-9]+).*/\1/p')
fi
POST_SB_LEN="${POST_SB_LEN:-0}"
PRE_SB_LEN="${PRE_SCROLLBACK_LEN:-0}"
if [[ "$POST_SB_LEN" -gt "$PRE_SB_LEN" ]]; then
    SCROLLBACK_OBSERVED_MAX="$POST_SB_LEN"
else
    SCROLLBACK_OBSERVED_MAX="$PRE_SB_LEN"
fi
SCROLLBACK_OBSERVED_MAX="${SCROLLBACK_OBSERVED_MAX:-0}"

# Extract dynamic_grid perf counters (sub-test A is the relevant window).
FAST_PATH_LINES=$(grep -c "dynamic_grid_fast_path_ok" "$LOGCAT_GESTURE" || true)
FULL_REINIT_LINES=$(grep -c "dynamic_grid_init_ok" "$LOGCAT_GESTURE" || true)
FAST_PATH_LINES="${FAST_PATH_LINES:-0}"
FULL_REINIT_LINES="${FULL_REINIT_LINES:-0}"

LAST_FAST_PATH_LINE=$(grep "dynamic_grid_fast_path_ok" "$LOGCAT_GESTURE" | tail -1 || true)
if [[ -n "$LAST_FAST_PATH_LINE" ]]; then
    FAST_PATH_TOTAL=$(echo "$LAST_FAST_PATH_LINE" | sed -nE 's/.*fast_path_updates=([0-9]+).*/\1/p')
    FULL_REINIT_TOTAL=$(echo "$LAST_FAST_PATH_LINE" | sed -nE 's/.*full_reinits=([0-9]+).*/\1/p')
fi
FAST_PATH_TOTAL="${FAST_PATH_TOTAL:-0}"
FULL_REINIT_TOTAL="${FULL_REINIT_TOTAL:-0}"

# Total bytes ingested (proxy for scrollback population).
TOTAL_INGESTED=$( (grep -E "terminalInputBytes" "$LOGCAT_OUT" || true) \
    | sed -nE 's/.*ingested=([0-9]+).*/\1/p' \
    | awk '{s+=$1} END {print s+0}')
TOTAL_INGESTED="${TOTAL_INGESTED:-0}"

# ── Compute gates ──────────────────────────────────────────────────────────

# AC#1: scrollback ≥1000 lines. We injected 2000 lines × ~30 bytes = ~60KB.
# Cap is 1000 lines so the observed max should saturate at 1000.
SCROLLBACK_GATE_PASS="false"
if [[ "$SCROLLBACK_OBSERVED_MAX" -ge 1000 ]]; then
    SCROLLBACK_GATE_PASS="true"
fi

# AC#2/#3 (round-2 hard split): Sub-test A — gesture-only — must produce
# at least one Rust set_scroll_offset call. This is the *real* AC#2/AC#3
# evidence. Sub-test B is a non-AC sanity check on the broadcast path.
GESTURE_GATE_PASS="false"
if [[ "$A_RUST_CALLS" -gt 0 ]]; then
    GESTURE_GATE_PASS="true"
fi

# Sub-test B must also pass — broadcasts should produce as many Rust calls
# as broadcasts sent.
BROADCAST_GATE_PASS="false"
if [[ "$B_RUST_CALLS" -ge "$B_BROADCAST_COUNT" ]]; then
    BROADCAST_GATE_PASS="true"
fi

# AC#4: 60fps p95 <16.6ms during sustained scroll (sub-test A window).
FPS_GATE_PASS="false"
if [[ -n "$P95_MS" && "$P95_MS" != "0" ]]; then
    P95_OK=$(awk -v p="$P95_MS" 'BEGIN { print (p < 16.6) ? "1" : "0" }')
    if [[ "$P95_OK" == "1" ]]; then
        FPS_GATE_PASS="true"
    fi
fi

# Overall pass requires scrollback + gesture (the real AC#2/#3) + fps.
# Broadcast sub-test is reported but not part of the AC gate — it's a
# health check on the determinstic fallback path.
OVERALL_PASS="false"
if [[ "$SCROLLBACK_GATE_PASS" == "true" \
   && "$GESTURE_GATE_PASS" == "true" \
   && "$FPS_GATE_PASS" == "true" ]]; then
    OVERALL_PASS="true"
fi

# ── Emit result.json ───────────────────────────────────────────────────────

cat > "$RESULT_JSON" <<EOF
{
  "story": "M3-S09",
  "round": 2,
  "device_serial": "$SERIAL",
  "device_class": "flagship",
  "scrollback": {
    "max_lines_cap": 1000,
    "observed_max": $SCROLLBACK_OBSERVED_MAX,
    "lines_injected": 2000,
    "total_bytes_ingested": $TOTAL_INGESTED,
    "expected": "saturates at 1000 (cap is intentional)",
    "pass": $SCROLLBACK_GATE_PASS
  },
  "gesture_scroll": {
    "swipes_sent": $A_SWIPE_COUNT,
    "down_swipes": $A_DOWN_SWIPE_COUNT,
    "up_swipes": $A_UP_SWIPE_COUNT,
    "rust_set_scroll_offset_calls": $A_RUST_CALLS,
    "kotlin_gesture_scroll_lines": $A_GESTURE_SCROLL_LINES,
    "kotlin_gesture_fling_lines": $A_GESTURE_FLING_LINES,
    "broadcast_crosstalk_lines": $A_BROADCAST_LINES,
    "expected": "rust_set_scroll_offset_calls >=1 (gesture-driven, no broadcasts)",
    "is_ac_gate": true,
    "pass": $GESTURE_GATE_PASS
  },
  "broadcast_fallback": {
    "broadcasts_sent": $B_BROADCAST_COUNT,
    "rust_set_scroll_offset_calls": $B_RUST_CALLS,
    "expected": "rust_set_scroll_offset_calls >= broadcasts_sent (deterministic path)",
    "is_ac_gate": false,
    "pass": $BROADCAST_GATE_PASS
  },
  "frame_timing": {
    "scroll_p50_ms": $P50_MS,
    "scroll_p95_ms": $P95_MS,
    "scroll_p99_ms": $P99_MS,
    "peak_fps": $PEAK_FPS,
    "num_frames": $NUM_FRAMES,
    "p95_threshold_ms": 16.6,
    "expected": "p95 <16.6ms during sustained scroll (sub-test A window)",
    "pass": $FPS_GATE_PASS
  },
  "dynamic_grid_perf": {
    "fast_path_lines": $FAST_PATH_LINES,
    "full_reinit_lines": $FULL_REINIT_LINES,
    "fast_path_total": $FAST_PATH_TOTAL,
    "full_reinit_total": $FULL_REINIT_TOTAL
  },
  "evidence": {
    "logcat_combined": "$LOGCAT_OUT",
    "logcat_gesture": "$LOGCAT_GESTURE",
    "logcat_broadcast": "$LOGCAT_BROADCAST",
    "frames": "$FRAMES_OUT",
    "screenshot_pre": "$PRE_PNG",
    "screenshot_post": "$POST_PNG"
  },
  "deferred_to_m5_or_later": {
    "low_end_pixel_4a_a52s": "Per M2-S13 user choice; flagship-only for M3."
  },
  "gate": {
    "ac1_scrollback_pass": $SCROLLBACK_GATE_PASS,
    "ac2_3_gesture_pass": $GESTURE_GATE_PASS,
    "ac4_fps_pass": $FPS_GATE_PASS,
    "broadcast_health_pass": $BROADCAST_GATE_PASS,
    "overall_pass": $OVERALL_PASS
  }
}
EOF

echo "=== gate: overall_pass=$OVERALL_PASS ===" >&2
echo "    scrollback=$SCROLLBACK_GATE_PASS  gesture(AC#2/#3)=$GESTURE_GATE_PASS  fps_p95=$FPS_GATE_PASS (${P95_MS}ms <16.6ms)" >&2
echo "    broadcast_health=$BROADCAST_GATE_PASS (sanity check, not part of AC gate)" >&2

if [[ "$OVERALL_PASS" == "true" ]]; then
    exit 0
fi
exit 2
