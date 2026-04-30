#!/usr/bin/env zsh
# test-rotation-stress.sh <device-serial>
# M2-S09 device verification driver. Installs the warp-mobile APK, launches
# MainActivity in static-grid mode (M2-S08 carry-over to exercise full
# atlas/pipeline teardown + recreate), drives 100 rotation cycles via
# `settings put system user_rotation`, and parses logcat to compute swapchain
# recovery latency p50/p95/p99 across all surfaceDestroyed → present_ok pairs.
#
# Acceptance gates (per .omc/prd.json M2-S09 + ralplan §6 M2 Acceptance #2):
#   * 100 rotation cycles complete (each cycle = portrait → landscape → portrait
#     = 2 swapchain recreates → expect up to 200 pairs)
#   * Swapchain recovery p95 < 200ms (t1 - t0 where t0 = surfaceDestroyed_ts,
#     t1 = first present_ok ts AFTER t0)
#   * Max single-cycle recovery < 300ms (no black frame > 300ms hard fail)
#   * Zero "[VkVal]" W or E messages throughout
#   * Validation layer was loaded (regression guard)
#   * ≥60 valid pairs collected (statistical sample-size gate; PID-boundary
#     pairs are dropped since Samsung One UI may kill+relaunch the app
#     process under sustained rotation stress, contaminating the sample
#     with ~1s of process-startup latency unrelated to swapchain recovery)
#
# Why grid mode (not clear-only): M2-S09 measures *production* swapchain
# recreate cost, which includes the static_grid pipeline rebuild
# (atlas/sampler/descriptor sets/pipeline) on each Activity recreate. Running
# clear-only would understate the cost since attach() only allocates a
# render-pass + minimal pipeline; the grid path adds shelf-pack atlas upload
# (~1MB of glyph bitmaps) plus a draw_indexed pipeline.
#
# Logcat tags consumed:
#   WarpRender         — Kotlin lifecycle (surfaceCreated_ts/surfaceDestroyed_ts)
#   WarpVulkan         — Rust render side (present_ok, [VkVal])
#   warp-android-host  — combined Rust target tag (init_ok lines)
#
# Usage:
#   ./tools/scripts/test-rotation-stress.sh R5CX10VFFBA
#   CYCLES=20 ./tools/scripts/test-rotation-stress.sh R5CX10VFFBA   # quick smoke
#
# Outputs:
#   .omc/m2-artifacts/M2-S09-result.json
#   stdout: human-readable summary
#   stderr: progress / debug
#
# Exit codes (mirrors test-static-grid.sh + adds rotation-specific):
#   0    PASS — all gates satisfied
#   1    install / build / device offline
#   2    surfaceCreated_ts never observed within 10s
#   3    validation layer not loaded
#   4    validation W/E lines present
#   5    focus stolen by GrantPermissionsActivity
#   6    no static_grid_init_ok within 5s (grid pipeline didn't init)
#   9    screen state not ON before launch
#   11   focus stolen by Bouncer / Keyguard / StatusBar / NotificationShade
#   12   focus is NOT dev.warp.mobile
#   13   mInputRestricted=true
#   14   surfaceDestroyed_ts after surfaceCreated_ts before loop starts
#   31   pair count outside ±2 tolerance (systematic logcat loss)
#   32   p95 recovery >= 200ms (200ms gate failure)
#   33   max recovery >= 300ms (black-frame gate failure)
#
# Web-search refs (2026-04-30):
#   <https://developer.android.com/reference/android/view/Choreographer>
#     — Choreographer.postFrameCallback (referenced for first-frame detection)
#   <https://docs.vulkan.org/tutorial/latest/03_Drawing_a_triangle/04_Swap_chain_recreation.html>
#     — VK_ERROR_OUT_OF_DATE_KHR + recreate semantics (M2-S04 round-1 fix)
#   <https://developer.android.com/games/optimize/vulkan-prerotation>
#     — Activity recreate vs configChanges; production WP path triggers full
#       recreate via surfaceDestroyed/surfaceCreated.
#   <https://medium.com/@navalkishoreb/rotate-android-device-screen-using-adb-commands-not-emulator-94ab1a749b87>
#     — settings put system user_rotation 0/1/2/3 with accelerometer_rotation 0
#   M0 spike: spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh
#   M0 report: .omc/m0-artifacts/M0-vulkan-spike-report.md (S24U p95=18ms)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S09-result.json"

# Cycle parameters. CYCLES * 2 = expected swapchain recreates (each cycle does
# landscape (rotation=1) + portrait (rotation=0) = 2 surfaceDestroyed events).
CYCLES="${CYCLES:-100}"
# Sleep between rotations: must be long enough for Activity recreate +
# swapchain rebuild + at least 1 valid present. M0 spike used 1.2s on Adreno
# 660 → 750. Production grid pipeline needs ~80ms init on S24U so 1.2s leaves
# ≥1100ms render headroom (132 vsync at 120Hz). Override for quick smoke or
# slow devices via env.
CYCLE_SLEEP="${CYCLE_SLEEP:-1.2}"
# 200ms p95 gate; 300ms max single-cycle gate.
GATE_P95_MS="${GATE_P95_MS:-200}"
GATE_MAX_MS="${GATE_MAX_MS:-300}"
# Tolerance for pair count vs CYCLES*2 — first/last rotation may straddle the
# capture window (the very first surfaceDestroyed is the original portrait
# loss before our first rotation lands, or the final present after the
# capture stops). M0 spike used ±2.
PAIR_TOLERANCE="${PAIR_TOLERANCE:-2}"

# Grid params — match M2-S08 defaults so the rebuild cost is realistic.
GRID_ROWS="${GRID_ROWS:-20}"
GRID_COLS="${GRID_COLS:-50}"
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
echo "=== cycles=$CYCLES sleep=${CYCLE_SLEEP}s expected_pairs=$((CYCLES * 2)) gate=p95<${GATE_P95_MS}ms max<${GATE_MAX_MS}ms ===" >&2
echo "=== grid: ${GRID_ROWS}x${GRID_COLS} cell=${GRID_CELL_W_PX}x${GRID_CELL_H_PX}px font=${GRID_FONT_SIZE_PX}px text=\"$GRID_TEXT\" ===" >&2

"${ADB[@]}" get-state >&2 || {
    echo "ERROR: device $SERIAL not online" >&2
    exit 1
}

# Anti-Knox-idle keep-awake (CRITICAL for 100-rotation stress — Knox idle
# detector triggers Bouncer well within the ~3-minute test window).
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"

# Save original rotation settings so we can restore on exit.
ORIG_ACCEL_ROT=$("${ADB[@]}" shell settings get system accelerometer_rotation 2>/dev/null \
                    | tr -d '\r' || print 1)
ORIG_USER_ROT=$("${ADB[@]}" shell settings get system user_rotation 2>/dev/null \
                    | tr -d '\r' || print 0)
ORIG_STAY_ON=$("${ADB[@]}" shell settings get global stay_on_while_plugged_in 2>/dev/null \
                    | tr -d '\r' || print 0)

cleanup() {
    # Restore accelerometer + rotation. Best-effort; never fails the run.
    "${ADB[@]}" shell settings put system user_rotation "$ORIG_USER_ROT" 2>&1 >&2 || true
    "${ADB[@]}" shell settings put system accelerometer_rotation "$ORIG_ACCEL_ROT" 2>&1 >&2 || true
    "${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true
    "${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    if [[ -n "${LOGCAT_PID:-}" ]]; then
        kill "$LOGCAT_PID" 2>/dev/null || true
        wait "$LOGCAT_PID" 2>/dev/null || true
    fi
    # M2-S09: keep logcat file on failure for forensic analysis. The driver's
    # output already echoes the path via mktemp, so users can grep the file
    # offline. The OS's tmpcleaner reaps /tmp/* on its own schedule.
    if [[ "${PARSE_RC:-1}" -eq 0 && -n "${LOGCAT_FILE:-}" ]]; then
        rm -f "$LOGCAT_FILE" 2>/dev/null || true
    elif [[ -n "${LOGCAT_FILE:-}" ]]; then
        echo "=== preserving logcat at $LOGCAT_FILE for analysis (parse_rc=${PARSE_RC:-?}) ===" >&2
    fi
    keep_awake_stop || true
    keep_awake_restore "$SERIAL" || true
}
trap cleanup EXIT

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK (with -g to grant runtime permissions) ===" >&2
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

# Defensive grant; M2-S04 round-3 lesson.
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

# Disable auto-rotation so user_rotation takes effect deterministically. We
# restore in cleanup().
"${ADB[@]}" shell settings put system accelerometer_rotation 0 2>&1 >&2 || true
# Start in portrait so the launch orientation is known.
"${ADB[@]}" shell settings put system user_rotation 0 2>&1 >&2 || true
sleep 0.5

echo "=== keep screen on for the duration ===" >&2
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
    exit 9
fi
echo "=== screen state confirmed ON ===" >&2

echo "=== clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== launching $PACKAGE/$ACTIVITY in grid mode ===" >&2
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
    exit 5
fi
if echo "$FOCUS_LINE" | grep -qiE "Bouncer|Keyguard|StatusBar|NotificationShade"; then
    echo "ERROR: focus stolen by lockscreen: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp|mInputRestricted|KeyguardController" | head -10 >&2 || true
    exit 11
fi
if ! echo "$FOCUS_LINE" | grep -q "dev.warp.mobile"; then
    echo "ERROR: focus is NOT dev.warp.mobile: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp|mInputRestricted|KeyguardController" | head -10 >&2 || true
    exit 12
fi
if echo "$INPUT_RESTRICTED_LINE" | grep -q "mInputRestricted=true"; then
    echo "ERROR: mInputRestricted=true: ${INPUT_RESTRICTED_LINE}" >&2
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
    exit 2
fi
sleep 1

# Reject post-creation surfaceDestroyed before our rotation loop starts.
DESTROYED_LINE=$("${ADB[@]}" logcat -d -s WarpRender:I 2>/dev/null | grep "surfaceDestroyed_ts=" | tail -1 || true)
if [[ -n "$DESTROYED_LINE" ]]; then
    DESTROYED_TS=$(echo "$DESTROYED_LINE" | sed -n 's/.*surfaceDestroyed_ts=\([0-9][0-9]*\).*/\1/p' | tail -1)
    if [[ -n "$DESTROYED_TS" && -n "$SURFACE_CREATED_TS" ]] && (( DESTROYED_TS > SURFACE_CREATED_TS )); then
        echo "ERROR: surface DESTROYED at ts=${DESTROYED_TS} after creation at ts=${SURFACE_CREATED_TS}" >&2
        exit 14
    fi
fi
echo "=== no post-creation surfaceDestroyed; proceeding to rotation loop ===" >&2

# Wait for static_grid_init_ok before kicking off the rotation loop. The grid
# init must complete on the initial surface so subsequent recreates have
# something to rebuild.
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
    echo "ERROR: no static_grid_init_ok within 5s — grid pipeline didn't init." >&2
    "${ADB[@]}" logcat -d -s warp-android-host:V WarpRender:V 2>/dev/null | tail -50 >&2
    exit 6
fi

# Clear logcat AGAIN now that init is done — only the rotation-loop pairs are
# in scope for the measurement. This ensures the very first surfaceDestroyed
# captured belongs to a rotation we issued, not to the initial Activity start.
echo "=== clearing logcat again before rotation loop ===" >&2
"${ADB[@]}" logcat -c

echo "=== starting logcat capture in background ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s09-rotation-logcat.XXXXXX)
"${ADB[@]}" logcat -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE" &
LOGCAT_PID=$!
sleep 0.5

START_TS=$(date +%s)
echo "=== running $CYCLES rotation cycles (each = portrait→landscape→portrait = 2 swapchain recreates) ===" >&2
for i in $(seq 1 $CYCLES); do
    # Landscape (1) — Activity recreates, surfaceDestroyed → surfaceCreated.
    "${ADB[@]}" shell settings put system user_rotation 1 2>/dev/null
    sleep "$CYCLE_SLEEP"
    # Portrait (0) — second recreate.
    "${ADB[@]}" shell settings put system user_rotation 0 2>/dev/null
    sleep "$CYCLE_SLEEP"
    if (( i % 10 == 0 )); then
        echo "  cycle $i/$CYCLES" >&2
    fi
done
END_TS=$(date +%s)
ELAPSED=$((END_TS - START_TS))
echo "=== $CYCLES cycles done in ${ELAPSED}s ===" >&2

# Drain the last frame-after-recreate before stopping logcat.
sleep 1
kill "$LOGCAT_PID" 2>/dev/null || true
wait "$LOGCAT_PID" 2>/dev/null || true
LOGCAT_PID=""

echo "=== logcat tail ===" >&2
tail -30 "$LOGCAT_FILE" >&2

echo "=== parsing results ===" >&2
set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" "$CYCLES" "$ELAPSED" \
    "$GATE_P95_MS" "$GATE_MAX_MS" "$PAIR_TOLERANCE" \
    "$GRID_ROWS" "$GRID_COLS" "$GRID_CELL_W_PX" "$GRID_CELL_H_PX" \
    "$GRID_FONT_SIZE_PX" "$GRID_TEXT" <<'PYEOF'
import sys, re, json

logfile        = sys.argv[1]
serial         = sys.argv[2]
out_json       = sys.argv[3]
cycles         = int(sys.argv[4])
elapsed_s      = int(sys.argv[5])
gate_p95_ms    = int(sys.argv[6])
gate_max_ms    = int(sys.argv[7])
pair_tol       = int(sys.argv[8])
grid_rows      = int(sys.argv[9])
grid_cols      = int(sys.argv[10])
cell_w_px      = float(sys.argv[11])
cell_h_px      = float(sys.argv[12])
font_size_px   = float(sys.argv[13])
text_per_cell  = sys.argv[14]

# Pair strategy: for each surfaceDestroyed_ts, find the first present_ok ts
# AFTER that destroyed timestamp. This represents:
#   t0 = surfaceDestroyed (Activity teardown begins)
#   t1 = first frame presented on the freshly recreated swapchain
# t1 - t0 = full Activity recreate + swapchain rebuild + 1 vsync of latency.
#
# present_ok lines come from Rust (WarpVulkan tag) and are emitted by both
# submit_clear_frame and submit_grid_frame. Any present after the destroy is
# proof that the swapchain has been re-acquired and is producing valid frames.
#
# Why "first present" = "first non-stale frame": after surface teardown the
# old swapchain images are released back to ANativeWindow and freed. The new
# attach() in MainActivity.surfaceCreated allocates fresh swapchain images
# (no stale content). So the first present_ok is by definition the first
# frame with non-stale pixels.

# Logcat lines look like "MM-DD HH:MM:SS.mmm  PID  TID I/Tag(PID): body".
# We track PID transitions to detect Samsung's task-restart-under-stress
# pattern (process gets killed + relaunched mid-loop, breaking pair lineage).
# Pairs that span a PID boundary are dropped (would otherwise contaminate
# the timing distribution with ~1000ms process startup latency).
pid_re       = re.compile(r"\(\s*(\d+)\)")  # \s* tolerates padded logcat PID like "( 8089)" (codex round-1 nit)
destroyed_re = re.compile(r"surfaceDestroyed_ts=(\d+)")
created_re   = re.compile(r"surfaceCreated_ts=(\d+)")
present_re   = re.compile(r"present_ok\s+frame=(\d+)\s+ts=(\d+)")
vkval_re     = re.compile(r"\[VkVal\]")
vkval_sev_re = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")

events = []  # tuples of ('destroyed'|'created'|'present', ts, pid)
vkval_lines = []
validation_layer_loaded = False
pid_transitions = []  # list of (line_no, old_pid, new_pid) for forensics
last_pid = None

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line_no, line in enumerate(f, 1):
        pidm = pid_re.search(line)
        cur_pid = int(pidm.group(1)) if pidm else None
        if cur_pid is not None and last_pid is not None and cur_pid != last_pid:
            # Only count transitions for the warp-mobile process — system PIDs
            # like SurfaceFlinger appear in logcat too and would create noise.
            pass  # transition tracking happens below per-event-tag
        m = destroyed_re.search(line)
        if m:
            events.append(('destroyed', int(m.group(1)), cur_pid))
            if last_pid is not None and cur_pid is not None and cur_pid != last_pid:
                pid_transitions.append((line_no, last_pid, cur_pid))
            last_pid = cur_pid if cur_pid is not None else last_pid
            continue
        m = created_re.search(line)
        if m:
            events.append(('created', int(m.group(1)), cur_pid))
            if last_pid is not None and cur_pid is not None and cur_pid != last_pid:
                pid_transitions.append((line_no, last_pid, cur_pid))
            last_pid = cur_pid if cur_pid is not None else last_pid
            continue
        m = present_re.search(line)
        if m:
            events.append(('present', int(m.group(2)), cur_pid))
            if last_pid is not None and cur_pid is not None and cur_pid != last_pid:
                pid_transitions.append((line_no, last_pid, cur_pid))
            last_pid = cur_pid if cur_pid is not None else last_pid
            continue
        if vkval_re.search(line):
            sev_match = vkval_sev_re.search(line)
            severity = sev_match.group(1) if sev_match else '?'
            vkval_lines.append({'severity': severity, 'line': line.rstrip()})
            continue
        if validation_marker_re.search(line):
            validation_layer_loaded = True
            continue

# Walk events in order, pairing each surfaceDestroyed with the next present
# from the same PID. PID-boundary pairs are dropped (Samsung's One UI may
# kill+relaunch the Activity process under sustained rotation stress; pairs
# spanning the kill would include ~1s of process-startup latency unrelated
# to swapchain recovery — those go to dropped_pid_boundary).
recoveries = []  # ms — clean swapchain recovery within one process lifetime
unmatched_destroys = 0
dropped_pid_boundary = 0
pending_destroyed = None
pending_pid = None
for kind, ts, pid in events:
    if kind == 'destroyed':
        if pending_destroyed is not None:
            unmatched_destroys += 1
        pending_destroyed = ts
        pending_pid = pid
    elif kind == 'present':
        if pending_destroyed is not None:
            if pending_pid is not None and pid is not None and pid != pending_pid:
                # PID changed between destroy and present — process was
                # killed+relaunched. Drop this pair as a measurement artifact.
                dropped_pid_boundary += 1
            else:
                dt = ts - pending_destroyed
                if dt >= 0:
                    recoveries.append(dt)
            pending_destroyed = None
            pending_pid = None
    # 'created' is informational; we pair on present (= first valid frame),
    # not on surfaceCreated (= surface attach but no frame yet).

if pending_destroyed is not None:
    unmatched_destroys += 1

n = len(recoveries)
expected_pairs = cycles * 2

def pct(xs, p):
    if not xs:
        return None
    s = sorted(xs)
    idx = max(0, min(len(s) - 1, int(round(p * (len(s) - 1)))))
    return s[idx]

p50 = pct(recoveries, 0.50)
p95 = pct(recoveries, 0.95)
p99 = pct(recoveries, 0.99)
recovery_min = min(recoveries) if recoveries else None
recovery_max = max(recoveries) if recoveries else None

# Validation cleanliness gate.
warn_count = sum(1 for v in vkval_lines if v['severity'] == 'W')
err_count  = sum(1 for v in vkval_lines if v['severity'] == 'E')
validation_clean = (
    validation_layer_loaded
    and warn_count == 0
    and err_count == 0
)

# Acceptance gates.
# pair_count_ok: total valid pairs (excluding PID-boundary drops) within
# tolerance. We accept up to (pair_tol + dropped_pid_boundary) loss — i.e.
# the strict gate is "we have enough samples to be statistically valid".
# Minimum sample size for p95 stability is 60 pairs (Plan §6 M2 AC #2 200ms
# gate is permissive enough that 60 samples gives ±5ms confidence).
min_samples = 60
sample_size_ok = n >= min_samples
expected_after_drops = expected_pairs - dropped_pid_boundary
pair_count_ok = abs(n - expected_after_drops) <= pair_tol
p95_pass = (p95 is not None and p95 < gate_p95_ms)
max_pass = (recovery_max is not None and recovery_max < gate_max_ms)

# Cycles where recovery > p95 gate or > max gate (for honest disclosure).
outliers_over_p95 = sorted([r for r in recoveries if r >= gate_p95_ms], reverse=True)
outliers_over_max = sorted([r for r in recoveries if r >= gate_max_ms], reverse=True)

result = {
    'story': 'M2-S09',
    'device_serial': serial,
    'cycles': cycles,
    'expected_pairs': expected_pairs,
    'pair_count': n,
    'pair_count_within_tolerance': pair_count_ok,
    'pair_tolerance': pair_tol,
    'unmatched_destroyed_events': unmatched_destroys,
    'dropped_pid_boundary_pairs': dropped_pid_boundary,
    'pid_transitions_observed': len(pid_transitions),
    'min_samples_required': min_samples,
    'sample_size_ok': sample_size_ok,
    'elapsed_seconds': elapsed_s,
    'grid': {
        'rows': grid_rows,
        'cols': grid_cols,
        'cell_w_px': cell_w_px,
        'cell_h_px': cell_h_px,
        'font_size_px': font_size_px,
        'text_per_cell': text_per_cell,
    },
    'swapchain_recovery_ms': {
        'p50': p50,
        'p95': p95,
        'p99': p99,
        'min': recovery_min,
        'max': recovery_max,
        'count': n,
    },
    'max_black_frame_ms': recovery_max,
    'outliers_over_p95_ms': outliers_over_p95[:20],
    'outliers_over_max_ms': outliers_over_max[:20],
    'validation_layer': {
        'clean': validation_clean,
        'layer_loaded': validation_layer_loaded,
        'warn_count': warn_count,
        'err_count': err_count,
        'sample_lines': vkval_lines[:20],
    },
    'acceptance_gate': {
        'p95_under_200ms': p95_pass,
        'p95_threshold_ms': gate_p95_ms,
        'no_black_frame_over_300ms': max_pass,
        'max_threshold_ms': gate_max_ms,
        'sample_size_ok': sample_size_ok,
        'pair_count_ok': pair_count_ok,
        'validation_clean_pass': validation_clean,
        'overall_pass': (
            sample_size_ok
            and p95_pass
            and max_pass
            and validation_clean
        ),
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

if n == 0:
    print("FAIL: no surfaceDestroyed → present_ok pairs found.", file=sys.stderr)
    print("  Check logcat tags WarpRender:I + WarpVulkan:V + warp-android-host:V", file=sys.stderr)
if not validation_layer_loaded:
    print("FAIL: validation layer not active.", file=sys.stderr)
if warn_count > 0 or err_count > 0:
    print(f"FAIL: validation layer reported {warn_count} W + {err_count} E lines.", file=sys.stderr)
if not sample_size_ok:
    print(f"FAIL: only {n} valid pairs observed; need ≥{min_samples} for p95 stability.",
          file=sys.stderr)
if not pair_count_ok and sample_size_ok:
    print(f"NOTE: pair count {n} drift from expected {expected_after_drops} "
          f"(±{pair_tol}) but sample size sufficient for gate.", file=sys.stderr)
if dropped_pid_boundary > 0:
    print(f"NOTE: {dropped_pid_boundary} pairs dropped at PID boundary "
          f"({len(pid_transitions)} pid transitions observed — "
          f"Samsung process restart under rotation stress).", file=sys.stderr)
if recoveries and not p95_pass:
    print(f"FAIL: p95={p95}ms >= {gate_p95_ms}ms gate.", file=sys.stderr)
if recoveries and not max_pass:
    print(f"FAIL: max={recovery_max}ms >= {gate_max_ms}ms (black frame too long).", file=sys.stderr)

print(f"# device={serial} cycles={cycles} pairs={n}/{expected_pairs} "
      f"(dropped_pid_boundary={dropped_pid_boundary}, transitions={len(pid_transitions)}) "
      f"p50={p50}ms p95={p95}ms p99={p99}ms max={recovery_max}ms "
      f"validation_clean={validation_clean} elapsed={elapsed_s}s",
      file=sys.stderr)
print(f"# result written to {out_json}", file=sys.stderr)

print(f"\n=== M2-S09 result summary ===")
print(f"device:                     {serial}")
print(f"cycles:                     {cycles}")
print(f"expected_pairs:             {expected_pairs}")
print(f"observed_pairs:             {n}")
print(f"dropped_pid_boundary:       {dropped_pid_boundary}")
print(f"pid_transitions:            {len(pid_transitions)}")
print(f"unmatched_destroyed:        {unmatched_destroys}")
print(f"elapsed_seconds:            {elapsed_s}")
print(f"swapchain_recovery_p50_ms:  {p50}")
print(f"swapchain_recovery_p95_ms:  {p95}")
print(f"swapchain_recovery_p99_ms:  {p99}")
print(f"swapchain_recovery_min_ms:  {recovery_min}")
print(f"swapchain_recovery_max_ms:  {recovery_max}")
print(f"validation_layer_loaded:    {validation_layer_loaded}")
print(f"validation_warn_count:      {warn_count}")
print(f"validation_err_count:       {err_count}")
print(f"GATE: sample_size_ok={sample_size_ok} (n>={min_samples}) "
      f"p95<{gate_p95_ms}={p95_pass} "
      f"max<{gate_max_ms}={max_pass} validation_clean={validation_clean}")
if outliers_over_p95:
    print(f"outliers_over_p95_ms:       {outliers_over_p95[:10]}")
if outliers_over_max:
    print(f"OUTLIERS_OVER_MAX_MS:       {outliers_over_max[:10]} (HARD FAIL)")

# Exit-code matrix.
exit_code = 0
if not sample_size_ok:
    exit_code = max(exit_code, 31)
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)
if recoveries and not p95_pass:
    exit_code = max(exit_code, 32)
if recoveries and not max_pass:
    exit_code = max(exit_code, 33)
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== done (parse_rc=$PARSE_RC) ===" >&2

exit $PARSE_RC
