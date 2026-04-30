#!/usr/bin/env zsh
# test-static-grid.sh <device-serial>
# M2-S08 device verification driver. Installs the warp-mobile APK, launches
# MainActivity in static-grid mode (50×20 cells of "Hello, World"), captures
# logcat for 30 seconds, parses Vulkan present_ok intervals to compute frame
# interval p50/p95/p99 + peak fps, and writes M2-S08-result.json.
#
# Acceptance gates (per .omc/prd.json M2-S08 + ralplan §6 M2 Acceptance #1):
#   * "static_grid_init_ok dt_ms=…" line present (init succeeded)
#   * "present_ok frame=N ts=M" lines logged ≥60 times in 1 second
#   * p95 frame interval < 16.6ms over the 30-second steady run (60fps gate)
#   * Zero "[VkVal]" W or E messages during the steady run
#   * Validation layer was loaded (regression guard)
#
# Logcat tags consumed:
#   WarpRender      — Kotlin lifecycle (surfaceCreated_ts, static_grid_started)
#   WarpVulkan      — Rust render side (present_ok, [VkVal] validation)
#   WarpStaticGrid  — Rust grid pipeline (atlas, init_ok, init_fail)
#
# Usage:
#   ./tools/scripts/test-static-grid.sh R5CX10VFFBA
#   GRID_ROWS=20 GRID_COLS=50 ./tools/scripts/test-static-grid.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m2-artifacts/M2-S08-result.json
#   stdout: human-readable summary
#   stderr: progress / debug
#
# Exit codes (exit-code matrix mirroring test-render-scene.sh):
#   0    PASS — all gates satisfied
#   1    install / build / device offline
#   2    surfaceCreated_ts never observed within 10s
#   3    validation layer not loaded
#   4    validation W/E lines present
#   5    focus stolen by GrantPermissionsActivity
#   6    PNG dim or other AC mismatch (reused for static_grid_init_ok missing)
#   9    screen state not ON before launch
#   11   focus stolen by Bouncer / Keyguard / StatusBar / NotificationShade
#   12   focus is NOT dev.warp.mobile
#   13   mInputRestricted=true
#   14   surfaceDestroyed_ts after surfaceCreated_ts before steady run
#   30   p95 frame interval >= 16.6ms (60fps gate failure)
#
# Web-search refs (2026-04-30):
#   <https://developer.android.com/reference/android/view/Choreographer> — vsync
#   <https://androidperformance.com/en/2025/03/26/Android-Perfetto-05-Chorergrapher/>
#     — perfetto rendering flow
#   <https://docs.vulkan.org/guide/latest/swapchain_semaphore_reuse.html>
#     — per-image semaphore (lessons re-used from M2-S04 round-3)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S08-result.json"
CAPTURE_SECONDS="${CAPTURE_SECONDS:-30}"

# Grid dimensions — defaults match Plan §6 M2 Acceptance #1 (50×20).
# Override via env: GRID_ROWS=10 GRID_COLS=20 ...
GRID_ROWS="${GRID_ROWS:-20}"
GRID_COLS="${GRID_COLS:-50}"
# Cell dims — chosen so 50 cols × 100 px = 5000px wide (wider than display so
# clipped on the right). 20 rows × 110 px = 2200px tall (fits in 2340 height).
GRID_CELL_W_PX="${GRID_CELL_W_PX:-100.0}"
GRID_CELL_H_PX="${GRID_CELL_H_PX:-110.0}"
GRID_FONT_SIZE_PX="${GRID_FONT_SIZE_PX:-20.0}"
GRID_TEXT="${GRID_TEXT:-Hello, World}"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL ===" >&2
echo "=== grid: rows=$GRID_ROWS cols=$GRID_COLS cell=${GRID_CELL_W_PX}x${GRID_CELL_H_PX}px font=${GRID_FONT_SIZE_PX}px text=\"$GRID_TEXT\" ===" >&2

"${ADB[@]}" get-state >&2 || {
    echo "ERROR: device $SERIAL not online" >&2
    exit 1
}

# Anti-Knox-idle keep-awake — see tools/scripts/lib/keep-awake.sh.
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"
trap 'keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK (with -g to grant runtime permissions) ===" >&2
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

# Defensive grant: M2-S04 round-3 lesson.
"${ADB[@]}" shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS 2>&1 >&2 || true

SDK_VERSION=$("${ADB[@]}" shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r' || print 0)
if [[ "${SDK_VERSION}" -ge 33 ]]; then
    PERM_DUMP=$("${ADB[@]}" shell dumpsys package "$PACKAGE" 2>/dev/null | grep -A1 "POST_NOTIFICATIONS" || true)
    if ! echo "$PERM_DUMP" | grep -q "granted=true"; then
        echo "ERROR: POST_NOTIFICATIONS not granted=true after install -g + pm grant." >&2
        echo "$PERM_DUMP" >&2
        exit 4
    fi
    echo "=== POST_NOTIFICATIONS granted=true confirmed (API ${SDK_VERSION}) ===" >&2
fi

echo "=== clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== keep screen on for the duration ===" >&2
ORIG_STAY_ON=$("${ADB[@]}" shell settings get global stay_on_while_plugged_in 2>/dev/null \
                  | tr -d '\r' || print 0)
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 7 2>&1 >&2 || true
"${ADB[@]}" shell svc power stayon true 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
"${ADB[@]}" shell wm dismiss-keyguard 2>&1 >&2 || true
"${ADB[@]}" shell cmd statusbar collapse 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_HOME 2>&1 >&2 || true
sleep 0.5

SCREEN_STATE=$("${ADB[@]}" shell dumpsys display 2>/dev/null \
                  | grep -E "mScreenState=" | head -1 || true)
if ! echo "$SCREEN_STATE" | grep -q "ON"; then
    echo "ERROR: screen state not ON before launch: ${SCREEN_STATE}" >&2
    "${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
    "${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true
    exit 9
fi
echo "=== screen state confirmed ON ===" >&2

echo "=== launching $PACKAGE/$ACTIVITY in grid mode ===" >&2
# Pass grid params through MainActivity intent extras. The text is base64-
# encoded because `am start --es` re-parses on the device side, and a value
# containing spaces (like "Hello, World") would be split across positional
# args even if quoted on the host side. The Kotlin side decodes
# `grid_text_b64` if present, falling back to `grid_text`. See
# CaptureFrameReceiver for the same pattern (M2-S07 lesson).
GRID_TEXT_B64=$(printf '%s' "$GRID_TEXT" | base64 | tr -d '\n')
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" \
    --ez grid_mode true \
    --es grid_text_b64 "$GRID_TEXT_B64" \
    --ef grid_font_size_px "$GRID_FONT_SIZE_PX" \
    --ei grid_rows "$GRID_ROWS" \
    --ei grid_cols "$GRID_COLS" \
    --ef grid_cell_w_px "$GRID_CELL_W_PX" \
    --ef grid_cell_h_px "$GRID_CELL_H_PX" \
    2>&1 | tail -2 >&2
START_TS=$(date +%s)
sleep 1

# M2-S04 round-3 strict focus assertion.
"${ADB[@]}" shell wm dismiss-keyguard 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
sleep 1

WINDOW_DUMP=$("${ADB[@]}" shell dumpsys window 2>/dev/null || true)
FOCUS_LINE=$(echo "$WINDOW_DUMP" | grep "mCurrentFocus" | head -1 || true)
INPUT_RESTRICTED_LINE=$(echo "$WINDOW_DUMP" | grep "mInputRestricted" | head -1 || true)
echo "=== focus probe: $FOCUS_LINE ===" >&2
echo "=== input_restricted probe: $INPUT_RESTRICTED_LINE ===" >&2

if echo "$FOCUS_LINE" | grep -qE "GrantPermissionsActivity|PermissionController"; then
    echo "ERROR: focus stolen by permission UI: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 5
fi
if echo "$FOCUS_LINE" | grep -qiE "Bouncer|Keyguard|StatusBar|NotificationShade"; then
    echo "ERROR: focus stolen by lockscreen / Bouncer / status bar: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp|mInputRestricted|KeyguardController" | head -10 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 11
fi
if ! echo "$FOCUS_LINE" | grep -q "dev.warp.mobile"; then
    echo "ERROR: focus is NOT dev.warp.mobile: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp|mInputRestricted|KeyguardController" | head -10 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 12
fi
if echo "$INPUT_RESTRICTED_LINE" | grep -q "mInputRestricted=true"; then
    echo "ERROR: mInputRestricted=true (keyguard locked input): ${INPUT_RESTRICTED_LINE}" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 13
fi
echo "=== focus confirmed: dev.warp.mobile, input not restricted ===" >&2

# Wait for surfaceCreated_ts.
echo "=== waiting for surfaceCreated_ts (up to 10s) ===" >&2
SURFACE_READY=0
SURFACE_CREATED_TS=""
for i in $(seq 1 20); do
    SURF_LINE=$("${ADB[@]}" logcat -d -s WarpRender:I 2>/dev/null | grep "surfaceCreated_ts=" | tail -1 || true)
    if [[ -n "$SURF_LINE" ]]; then
        SURFACE_READY=1
        SURFACE_CREATED_TS=$(echo "$SURF_LINE" | sed -n 's/.*surfaceCreated_ts=\([0-9][0-9]*\).*/\1/p' | tail -1)
        echo "=== SurfaceView ready after ${i}*0.5s (ts=${SURFACE_CREATED_TS}) ===" >&2
        break
    fi
    sleep 0.5
done
if [[ $SURFACE_READY -ne 1 ]]; then
    echo "ERROR: MainActivity never reached surfaceCreated_ts within 10s." >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp" | head -5 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 2
fi
sleep 1

# Reject post-creation surfaceDestroyed (M2-S04 round-3 blocker 1).
DESTROYED_LINE=$("${ADB[@]}" logcat -d -s WarpRender:I 2>/dev/null | grep "surfaceDestroyed_ts=" | tail -1 || true)
if [[ -n "$DESTROYED_LINE" ]]; then
    DESTROYED_TS=$(echo "$DESTROYED_LINE" | sed -n 's/.*surfaceDestroyed_ts=\([0-9][0-9]*\).*/\1/p' | tail -1)
    if [[ -n "$DESTROYED_TS" && -n "$SURFACE_CREATED_TS" ]] && (( DESTROYED_TS > SURFACE_CREATED_TS )); then
        echo "ERROR: surface DESTROYED at ts=${DESTROYED_TS} after creation at ts=${SURFACE_CREATED_TS}" >&2
        "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
        exit 14
    fi
fi
echo "=== no post-creation surfaceDestroyed; proceeding to render-window ===" >&2

# Wait briefly for static_grid_init_ok before kicking off the steady-state
# capture. The grid init is synchronous (~80-200ms) — give it 5s for safety
# margin (cosmic-text shaping + GPU upload on a slow device).
#
# NOTE: `android_logger` uses a single process-wide tag set at init
# (`warp-android-host`), and `log::info!(target: "WarpStaticGrid", ...)` only
# sets the *target* (visible in the log message body, not the tag). So the
# logcat filter must use the `warp-android-host` tag.
echo "=== waiting for static_grid_init_ok (up to 5s) ===" >&2
GRID_INIT_OK=0
for i in $(seq 1 10); do
    if "${ADB[@]}" logcat -d -s warp-android-host:I 2>/dev/null | grep -q "static_grid_init_ok"; then
        GRID_INIT_OK=1
        echo "=== static_grid_init_ok observed after ${i}*0.5s ===" >&2
        break
    fi
    sleep 0.5
done
if [[ $GRID_INIT_OK -ne 1 ]]; then
    echo "ERROR: no static_grid_init_ok line in logcat within 5s — grid init failed." >&2
    "${ADB[@]}" logcat -d -s warp-android-host:V WarpRender:V 2>/dev/null | tail -50 >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 6
fi

echo "=== capturing logcat for $CAPTURE_SECONDS seconds ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s08-logcat.XXXXXX)
"${ADB[@]}" logcat -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "WarpStaticGrid:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE" &
LOGCAT_PID=$!
trap 'kill $LOGCAT_PID 2>/dev/null || true; rm -f $LOGCAT_FILE 2>/dev/null || true; keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

# Periodic screen-state probe + Bouncer reject during the 30s capture.
PROBE_INTERVAL=5
PROBES=$((CAPTURE_SECONDS / PROBE_INTERVAL))
for i in $(seq 1 $PROBES); do
    sleep $PROBE_INTERVAL
    SCREEN=$("${ADB[@]}" shell dumpsys display 2>/dev/null | grep -E "mScreenState=" | head -1 || true)
    FOCUS=$("${ADB[@]}" shell dumpsys window 2>/dev/null | grep mCurrentFocus | head -1 || true)
    if echo "$SCREEN" | grep -qv "ON"; then
        echo "WARN: screen state changed mid-capture probe#$i: $SCREEN" >&2
    fi
    if echo "$FOCUS" | grep -qiE "Bouncer|Keyguard"; then
        echo "WARN: focus drifted to Bouncer/Keyguard mid-capture probe#$i: $FOCUS" >&2
    fi
done

kill $LOGCAT_PID 2>/dev/null || true
wait $LOGCAT_PID 2>/dev/null || true
END_TS=$(date +%s)

echo "=== captured $((END_TS - START_TS))s of logcat ===" >&2
echo "=== logcat tail ===" >&2
tail -30 "$LOGCAT_FILE" >&2

echo "=== parsing results ===" >&2
set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" "$CAPTURE_SECONDS" \
    "$GRID_ROWS" "$GRID_COLS" "$GRID_CELL_W_PX" "$GRID_CELL_H_PX" \
    "$GRID_FONT_SIZE_PX" "$GRID_TEXT" <<'PYEOF'
import sys, re, json

logfile        = sys.argv[1]
serial         = sys.argv[2]
out_json       = sys.argv[3]
capture_secs   = int(sys.argv[4])
grid_rows      = int(sys.argv[5])
grid_cols      = int(sys.argv[6])
cell_w_px      = float(sys.argv[7])
cell_h_px      = float(sys.argv[8])
font_size_px   = float(sys.argv[9])
text_per_cell  = sys.argv[10]

# Match "WarpVulkan: present_ok frame=<n> ts=<ms>"
present_re = re.compile(r"present_ok\s+frame=(\d+)\s+ts=(\d+)")
# Match "static_grid_init_ok dt_ms=… text=… atlas_glyphs=… instances=…"
init_re = re.compile(
    r"static_grid_init_ok\s+dt_ms=(\d+)\s+text=\"([^\"]+)\"\s+rows=(\d+)\s+cols=(\d+)\s+"
    r"cell=([\d.]+)x([\d.]+)px\s+font_size_px=([\d.]+)\s+"
    r"atlas_glyphs=(\d+)\s+instances=(\d+)"
)
# Validation — same grammar as M2-S04 driver.
vkval_re       = re.compile(r"\[VkVal\]")
vkval_sev_re   = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
attach_re      = re.compile(r"surfaceCreated_ts=(\d+)")
detach_re      = re.compile(r"surfaceDestroyed_ts=(\d+)")
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")

frames = []
vkval_lines = []
attach_ts = None
detach_ts = None
validation_layer_loaded = False
init_info = None

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line in f:
        m = present_re.search(line)
        if m:
            frames.append((int(m.group(1)), int(m.group(2))))
            continue
        m = init_re.search(line)
        if m:
            init_info = {
                'dt_ms': int(m.group(1)),
                'text': m.group(2),
                'rows': int(m.group(3)),
                'cols': int(m.group(4)),
                'cell_w_px': float(m.group(5)),
                'cell_h_px': float(m.group(6)),
                'font_size_px': float(m.group(7)),
                'atlas_glyphs': int(m.group(8)),
                'glyphs_per_frame': int(m.group(9)),
            }
            continue
        if vkval_re.search(line):
            sev_match = vkval_sev_re.search(line)
            severity = sev_match.group(1) if sev_match else '?'
            vkval_lines.append({'severity': severity, 'line': line.rstrip()})
            continue
        if validation_marker_re.search(line):
            validation_layer_loaded = True
            continue
        m = attach_re.search(line)
        if m:
            attach_ts = int(m.group(1))
            continue
        m = detach_re.search(line)
        if m:
            detach_ts = int(m.group(1))
            continue

# Compute frame intervals.
intervals = []
for i in range(1, len(frames)):
    dt = frames[i][1] - frames[i-1][1]
    if dt >= 0:
        intervals.append(dt)

# Compute peak fps in any 1-second sliding window.
ts_list = [f[1] for f in frames]
peak_fps_window = 0
if len(ts_list) >= 2:
    j = 0
    for i in range(len(ts_list)):
        while j < len(ts_list) and ts_list[j] - ts_list[i] <= 1000:
            j += 1
        peak_fps_window = max(peak_fps_window, j - i)

def pct(xs, p):
    if not xs:
        return None
    s = sorted(xs)
    idx = max(0, min(len(s) - 1, int(round(p * (len(s) - 1)))))
    return s[idx]

p50 = pct(intervals, 0.50)
p95 = pct(intervals, 0.95)
p99 = pct(intervals, 0.99)

# Validation cleanliness gate.
warn_count = sum(1 for v in vkval_lines if v['severity'] == 'W')
err_count  = sum(1 for v in vkval_lines if v['severity'] == 'E')
validation_clean = (
    validation_layer_loaded
    and warn_count == 0
    and err_count == 0
)

# 60fps gate: p95 frame interval < 16.6ms.
fps_60_pass = (p95 is not None and p95 < 16.6)
# Also require ≥60 frames in any 1-second window (matches M2-S04 secondary check).
fps_window_pass = peak_fps_window >= 60

result = {
    'story': 'M2-S08',
    'device_serial': serial,
    'capture_seconds': capture_secs,
    'grid': {
        'rows': grid_rows,
        'cols': grid_cols,
        'cell_w_px': cell_w_px,
        'cell_h_px': cell_h_px,
        'font_size_px': font_size_px,
        'text_per_cell': text_per_cell,
        'glyphs_per_frame': init_info['glyphs_per_frame'] if init_info else None,
        'atlas_glyphs': init_info['atlas_glyphs'] if init_info else None,
        'init_dt_ms': init_info['dt_ms'] if init_info else None,
    },
    'frames_observed': len(frames),
    'first_frame_num': frames[0][0] if frames else None,
    'last_frame_num': frames[-1][0] if frames else None,
    'first_ts_ms': frames[0][1] if frames else None,
    'last_ts_ms': frames[-1][1] if frames else None,
    'attach_ts_ms': attach_ts,
    'detach_ts_ms': detach_ts,
    'frame_interval_ms': {
        'p50': p50,
        'p95': p95,
        'p99': p99,
        'count': len(intervals),
    },
    'peak_fps_in_1s_window': peak_fps_window,
    'validation_layer': {
        'clean': validation_clean,
        'layer_loaded': validation_layer_loaded,
        'warn_count': warn_count,
        'err_count': err_count,
        'sample_lines': vkval_lines[:20],
    },
    'acceptance_gate': {
        'static_grid_init_ok_seen': init_info is not None,
        'fps_60_pass': fps_60_pass,
        'fps_window_pass': fps_window_pass,
        'validation_clean_pass': validation_clean,
        'overall_pass': (
            init_info is not None
            and fps_60_pass
            and fps_window_pass
            and validation_clean
        ),
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

if init_info is None:
    print("FAIL: no static_grid_init_ok line — grid pipeline never initialized.",
          file=sys.stderr)
if not validation_layer_loaded:
    print("FAIL: validation layer not active — debug build is silently skipping the gate.",
          file=sys.stderr)
if warn_count > 0 or err_count > 0:
    print(f"FAIL: validation layer reported {warn_count} W + {err_count} E lines.",
          file=sys.stderr)
if not fps_60_pass:
    print(f"FAIL: p95 frame interval {p95}ms >= 16.6ms (60fps gate failure).",
          file=sys.stderr)
if not fps_window_pass:
    print(f"FAIL: peak fps in 1s window {peak_fps_window} < 60.",
          file=sys.stderr)

print(f"# device={serial} frames={len(frames)} peak_fps_1s={peak_fps_window} "
      f"p50={p50}ms p95={p95}ms p99={p99}ms "
      f"validation_layer_loaded={validation_layer_loaded} "
      f"validation_clean={validation_clean} "
      f"fps_60_pass={fps_60_pass}",
      file=sys.stderr)
print(f"# result written to {out_json}", file=sys.stderr)

print(f"\n=== M2-S08 result summary ===")
print(f"device:                    {serial}")
print(f"capture_seconds:           {capture_secs}")
if init_info:
    print(f"grid:                      {init_info['rows']}x{init_info['cols']} "
          f"cell={init_info['cell_w_px']}x{init_info['cell_h_px']}px font_size_px={init_info['font_size_px']}")
    print(f"text_per_cell:             {init_info['text']!r}")
    print(f"atlas_glyphs:              {init_info['atlas_glyphs']}")
    print(f"glyphs_per_frame:          {init_info['glyphs_per_frame']}")
    print(f"init_dt_ms:                {init_info['dt_ms']}")
else:
    print("init_info:                 NOT SEEN (FAIL)")
print(f"frames_observed:           {len(frames)}")
print(f"peak_fps_in_1s_window:     {peak_fps_window}")
print(f"frame_interval_p50_ms:     {p50}")
print(f"frame_interval_p95_ms:     {p95}")
print(f"frame_interval_p99_ms:     {p99}")
print(f"validation_layer_loaded:   {validation_layer_loaded}")
print(f"validation_warn_count:     {warn_count}")
print(f"validation_err_count:      {err_count}")
print(f"GATE: init_seen={init_info is not None} fps_60_pass={fps_60_pass} "
      f"fps_window={fps_window_pass} validation_clean={validation_clean}")

# Exit-code matrix.
exit_code = 0
if init_info is None:
    exit_code = max(exit_code, 6)
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)
if not fps_60_pass:
    exit_code = max(exit_code, 30)
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== done ===" >&2

"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true
"${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
"${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true

exit $PARSE_RC
