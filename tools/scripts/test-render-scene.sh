#!/usr/bin/env zsh
# test-render-scene.sh <device-serial>
# M2-S04 device verification driver. Installs the warp-mobile APK, launches
# MainActivity, captures logcat for ~60 seconds, and parses Vulkan present
# events to compute frame interval p50/p95/p99.
#
# Acceptance gates (per .omc/prd.json M2-S04):
#   * "present_ok frame=N ts=M" lines logged ≥60 times in 1 second
#   * No "[VkVal]" warnings or errors during the 60-second steady run
#
# Logcat tags consumed:
#   WarpRender   — Kotlin lifecycle (surfaceCreated_ts, etc.)
#   WarpVulkan   — Rust render side (present_ok, [VkVal] validation)
#
# Usage:
#   ./tools/scripts/test-render-scene.sh 25c027b4fe1c7ece
#
# Outputs:
#   .omc/m2-artifacts/M2-S04-result.json
#   stdout: human-readable summary
#   stderr: progress / debug

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S04-result.json"
CAPTURE_SECONDS="${CAPTURE_SECONDS:-60}"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL ===" >&2

# Verify device is online and authorized.
"${ADB[@]}" get-state >&2 || {
    echo "ERROR: device $SERIAL not online" >&2
    exit 1
}

# Anti-Knox-idle keep-awake — see tools/scripts/lib/keep-awake.sh.
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"
trap 'keep_awake_stop; keep_awake_restore "$SERIAL"' EXIT

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK (with -g to grant runtime permissions) ===" >&2
# Round-2 (Codex blocker 3): `-g` auto-grants runtime permissions on install.
# Without it, on API 33+ MainActivity.onCreate calls
# requestPermissions(POST_NOTIFICATIONS) which spawns GrantPermissionsActivity
# and STEALS FOCUS from the SurfaceView. Codex round-1 reproduction on
# RFCY71LAFYE got 83 frames vs claimed 7,418 — exactly because of this race.
# Belt-and-suspenders: explicit pm grant after install too, in case `-g` fails
# silently on some API levels.
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

# Defensive grant: if install -g already granted these, this is a no-op.
"${ADB[@]}" shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS 2>&1 >&2 || true

# Round-3 (Codex blocker 2): explicit POST_NOTIFICATIONS=granted assertion.
# pm grant exit-code is unreliable (returns 0 on some no-op cases even when
# permission stays denied). Codex round-2 reproduced focus theft after
# revoking the grant — the previous wait-for-surfaceCreated_ts heuristic did
# not catch it because surfaceCreated still fired *behind* the dialog.
# Robust check: dumpsys package then grep for granted=true on API 33+.
SDK_VERSION=$("${ADB[@]}" shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r' || print 0)
if [[ "${SDK_VERSION}" -ge 33 ]]; then
    PERM_DUMP=$("${ADB[@]}" shell dumpsys package "$PACKAGE" 2>/dev/null | grep -A1 "POST_NOTIFICATIONS" || true)
    if ! echo "$PERM_DUMP" | grep -q "granted=true"; then
        echo "ERROR: POST_NOTIFICATIONS not granted=true after install -g + pm grant." >&2
        echo "       This means GrantPermissionsActivity will steal focus from" >&2
        echo "       SurfaceView and the M2-S04 capture will be invalid." >&2
        echo "       dumpsys output:" >&2
        echo "$PERM_DUMP" >&2
        exit 4
    fi
    echo "=== POST_NOTIFICATIONS granted=true confirmed (API ${SDK_VERSION}) ===" >&2
fi

echo "=== clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== keep screen on for the duration ===" >&2
# Capture original stay-on so we can restore it on exit (S24 Ultra hardening).
ORIG_STAY_ON=$("${ADB[@]}" shell settings get global stay_on_while_plugged_in 2>/dev/null \
                  | tr -d '\r' || print 0)
# Stay-on while plugged: 1=AC, 2=USB, 4=wireless. 7=all. Restored after run.
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 7 2>&1 >&2 || true
"${ADB[@]}" shell svc power stayon true 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
"${ADB[@]}" shell wm dismiss-keyguard 2>&1 >&2 || true
# S24 Ultra (One UI) parks focus on NotificationShade after WAKEUP+settings
# changes; explicit collapse + HOME ensures app focus isn't stolen by the
# notification panel before MainActivity launches.
"${ADB[@]}" shell cmd statusbar collapse 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_HOME 2>&1 >&2 || true
sleep 0.5

# Probe screen state — if not ON, fail fast.
SCREEN_STATE=$("${ADB[@]}" shell dumpsys display 2>/dev/null \
                  | grep -E "mScreenState=" | head -1 || true)
if ! echo "$SCREEN_STATE" | grep -q "ON"; then
    echo "ERROR: screen state not ON before launch: ${SCREEN_STATE}" >&2
    "${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
    "${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true
    exit 9
fi
echo "=== screen state confirmed ON ===" >&2

echo "=== launching $PACKAGE/$ACTIVITY ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" 2>&1 | tail -2 >&2
START_TS=$(date +%s)
sleep 1

# Round-3 (Codex blocker 2): explicit focus check post-launch. If a permission
# dialog or other system overlay stole focus, fail fast with the actual focus
# window so we can diagnose. surfaceCreated_ts can fire BEHIND the dialog, so
# the prior wait-only heuristic produced false positives.
FOCUS_LINE=$("${ADB[@]}" shell dumpsys window 2>/dev/null | grep "mCurrentFocus" | head -1 || true)
if echo "$FOCUS_LINE" | grep -qE "GrantPermissionsActivity|PermissionController"; then
    echo "ERROR: focus stolen by permission UI: ${FOCUS_LINE}" >&2
    echo "       The capture would not be valid (SurfaceView is not foreground)." >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 5
fi

# Round-2 (Codex blocker 3): fail-fast sanity check — wait up to 10 seconds
# for MainActivity to log `surfaceCreated_ts=`. If it doesn't appear, the
# activity never reached the SurfaceHolder.Callback, almost always because
# something stole focus (permission dialog, secure-screen, lock, etc.).
echo "=== waiting for surfaceCreated_ts (up to 10s) ===" >&2
SURFACE_READY=0
for i in $(seq 1 20); do
    if "${ADB[@]}" logcat -d -s WarpRender:I 2>/dev/null \
            | grep -q "surfaceCreated_ts="; then
        SURFACE_READY=1
        echo "=== SurfaceView ready after ${i}*0.5s ===" >&2
        break
    fi
    sleep 0.5
done
if [[ $SURFACE_READY -ne 1 ]]; then
    echo "ERROR: MainActivity never reached surfaceCreated_ts within 10s." >&2
    echo "       Most common cause: a permission dialog stole focus." >&2
    echo "       Check 'adb shell dumpsys window | grep mCurrentFocus' and" >&2
    echo "       ensure POST_NOTIFICATIONS is granted (this script tried)." >&2
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp" | head -5 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 2
fi
sleep 1

echo "=== capturing logcat for $CAPTURE_SECONDS seconds ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s04-logcat.XXXXXX)
# Quote filter spec so zsh doesn't glob-expand '*:S'.
"${ADB[@]}" logcat -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE" &
LOGCAT_PID=$!
# Merge logcat cleanup with keep_awake cleanup (defined earlier).
trap 'kill $LOGCAT_PID 2>/dev/null; rm -f $LOGCAT_FILE; keep_awake_stop; keep_awake_restore "$SERIAL"' EXIT

sleep "$CAPTURE_SECONDS"

kill $LOGCAT_PID 2>/dev/null || true
wait $LOGCAT_PID 2>/dev/null || true
END_TS=$(date +%s)

echo "=== captured $((END_TS - START_TS))s of logcat ===" >&2
echo "=== logcat tail ===" >&2
tail -30 "$LOGCAT_FILE" >&2

echo "=== parsing results ===" >&2

# Round-4 (Codex round-3 nit): with `set -euo pipefail`, the parser exiting
# non-zero would bypass the cleanup paths below (force-stop, stay-on
# restore). Disable errexit just around the parser invocation so we can
# capture PARSE_RC into a variable and decide policy ourselves.
set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" "$CAPTURE_SECONDS" <<'PYEOF'
import sys, re, json, statistics

logfile        = sys.argv[1]
serial         = sys.argv[2]
out_json       = sys.argv[3]
capture_secs   = int(sys.argv[4])

# Match "WarpVulkan: present_ok frame=<n> ts=<ms>"
present_re = re.compile(r"present_ok\s+frame=(\d+)\s+ts=(\d+)")
# Match validation warnings/errors (we tag them with [VkVal] in Rust).
# `adb logcat -v time` formats lines as:
#   MM-DD HH:MM:SS.mmm <SEV>/<TAG>(<PID>): <message>
# Extract the single-letter severity right before the `/<TAG>(`.
vkval_re       = re.compile(r"\[VkVal\]")
vkval_sev_re   = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
attach_re      = re.compile(r"surfaceCreated_ts=(\d+)")
detach_re      = re.compile(r"surfaceDestroyed_ts=(\d+)")

frames = []   # list of (frame_num, ts_ms)
vkval_lines = []
attach_ts = None
detach_ts = None
# Round-2 (Codex blocker 4c): require that the validation layer was actually
# loaded during this run. Without this, an APK without the validation layer
# would run with zero [VkVal] messages and the driver would happily report
# `validation_clean=true` — the exact false-positive the round-1 reviewer
# flagged. The Rust side logs "VK_LAYER_KHRONOS_validation enabled" when it
# successfully enables the layer (see crates/android-host/src/vulkan.rs).
validation_layer_loaded = False
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line in f:
        m = present_re.search(line)
        if m:
            frames.append((int(m.group(1)), int(m.group(2))))
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

# Compute frame intervals (ms between consecutive present_ok timestamps).
intervals = []
for i in range(1, len(frames)):
    dt = frames[i][1] - frames[i-1][1]
    if dt >= 0:
        intervals.append(dt)

# Compute "frames in any 1-second sliding window" peak (the AC says "≥60 frames
# in 1 second" — measured here as max over sliding 1000ms window).
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

# Validation cleanliness gate: any W/E lines fail the gate.
warn_count = sum(1 for v in vkval_lines if v['severity'] == 'W')
err_count  = sum(1 for v in vkval_lines if v['severity'] == 'E')
# Round-2 (Codex blocker 4c): require BOTH zero W/E lines AND that the layer
# was actually loaded. An APK without the validation layer .so would pass the
# old W/E-zero check trivially (no lines at all) — that was the false
# positive Codex flagged.
validation_clean = (
    validation_layer_loaded
    and warn_count == 0
    and err_count == 0
)

# Acceptance: ≥60 frames in any 1-second window.
fps_60_pass = peak_fps_window >= 60

result = {
    'story': 'M2-S04',
    'device_serial': serial,
    'capture_seconds': capture_secs,
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
        'fps_60_pass': fps_60_pass,
        'validation_clean_pass': validation_clean,
        'overall_pass': fps_60_pass and validation_clean,
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

if not validation_layer_loaded:
    print("FAIL: validation layer not active — your debug build is silently "
          "skipping the M2-S04 hard gate. Ensure libVkLayer_khronos_validation.so "
          "is packaged in jniLibs (run android/gradlew :app:fetchValidationLayer "
          "or rebuild :app:assembleDebug after the layer .so is in place).",
          file=sys.stderr)
if warn_count > 0 or err_count > 0:
    print(f"FAIL: validation layer reported {warn_count} W + {err_count} E "
          f"lines (validation_clean=false). Review sample_lines in result.json.",
          file=sys.stderr)
if not fps_60_pass:
    print(f"FAIL: peak fps in 1s window {peak_fps_window} < 60 (performance gate).",
          file=sys.stderr)

print(f"# device={serial} frames={len(frames)} peak_fps_1s={peak_fps_window} "
      f"p50={p50}ms p95={p95}ms p99={p99}ms "
      f"validation_layer_loaded={validation_layer_loaded} "
      f"validation_clean={validation_clean} "
      f"fps_60_pass={fps_60_pass}", file=sys.stderr)
print(f"# result written to {out_json}", file=sys.stderr)

# Summary table on stdout.
print(f"\n=== M2-S04 result summary ===")
print(f"device:                    {serial}")
print(f"capture_seconds:           {capture_secs}")
print(f"frames_observed:           {len(frames)}")
print(f"peak_fps_in_1s_window:     {peak_fps_window}")
print(f"frame_interval_p50_ms:     {p50}")
print(f"frame_interval_p95_ms:     {p95}")
print(f"frame_interval_p99_ms:     {p99}")
print(f"validation_layer_loaded:   {validation_layer_loaded}")
print(f"validation_warn_count:     {warn_count}")
print(f"validation_err_count:      {err_count}")
print(f"GATE:                      fps_60_pass={fps_60_pass} validation_clean={validation_clean}")

# Round-3 (Codex round-2 blocker 3): exit non-zero on ANY gate failure, not
# just the validation-layer-loaded check. Round-2 only failed when the layer
# was absent — a real validation W/E or an FPS regression would still exit 0
# and the result.json would silently report overall_pass=false to nobody.
#
# Round-4 (Codex round-3 blocker): layer-absent must exit 3, not 4. Earlier
# round-3 used `not validation_clean` for the exit-4 case, but
# validation_clean is computed as `layer_loaded AND no W/E lines`, which
# means a missing layer would falsely upgrade the exit code to 4. Codex's
# repro fed a synthetic "60fps, no marker" log and got `no_layer_rc=4`
# instead of the documented 3. Decouple: exit 4 only on actual W/E lines.
exit_code = 0
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)              # blocker 4c (round-1)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)              # blocker 3 (round-2) — only on real W/E
if not fps_60_pass:
    exit_code = max(exit_code, 5)              # blocker 3 (round-2)
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== done ===" >&2

# Try to gracefully stop the activity so the device isn't left rendering.
"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true
"${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
# Restore stay-on default to user's original.
"${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true

exit $PARSE_RC
