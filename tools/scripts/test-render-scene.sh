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

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK ===" >&2
"${ADB[@]}" install -r "$APK" 2>&1 | tail -3 >&2

echo "=== clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== keep screen on for the duration ===" >&2
# Stay-on while plugged: 1=AC, 2=USB, 4=wireless. 7=all. Restored after run.
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 7 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
sleep 0.5

echo "=== launching $PACKAGE/$ACTIVITY ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" 2>&1 | tail -2 >&2
START_TS=$(date +%s)
sleep 2

echo "=== capturing logcat for $CAPTURE_SECONDS seconds ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s04-logcat.XXXXXX)
# Quote filter spec so zsh doesn't glob-expand '*:S'.
"${ADB[@]}" logcat -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE" &
LOGCAT_PID=$!
trap "kill $LOGCAT_PID 2>/dev/null; rm -f $LOGCAT_FILE" EXIT

sleep "$CAPTURE_SECONDS"

kill $LOGCAT_PID 2>/dev/null || true
wait $LOGCAT_PID 2>/dev/null || true
END_TS=$(date +%s)

echo "=== captured $((END_TS - START_TS))s of logcat ===" >&2
echo "=== logcat tail ===" >&2
tail -30 "$LOGCAT_FILE" >&2

echo "=== parsing results ===" >&2

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
validation_clean = warn_count == 0 and err_count == 0

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

print(f"# device={serial} frames={len(frames)} peak_fps_1s={peak_fps_window} "
      f"p50={p50}ms p95={p95}ms p99={p99}ms validation_clean={validation_clean} "
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
print(f"validation_warn_count:     {warn_count}")
print(f"validation_err_count:      {err_count}")
print(f"GATE:                      fps_60_pass={fps_60_pass} validation_clean={validation_clean}")
PYEOF
PARSE_RC=$?

echo "=== done ===" >&2

# Try to gracefully stop the activity so the device isn't left rendering.
"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true
# Restore stay-on default (0).
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 0 2>&1 >&2 || true

exit $PARSE_RC
