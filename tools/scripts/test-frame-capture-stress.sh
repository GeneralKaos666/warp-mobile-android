#!/usr/bin/env zsh
# test-frame-capture-stress.sh <device-serial> [count]
#
# M2-S05 round-2 tight-loop stress test (Codex round-1 blocker 2 verifier).
# Fires N back-to-back CAPTURE_FRAME broadcasts (default 10) within ~2s and
# verifies:
#   * All N captures produce a `capture_ok` line in logcat (zero Vk(TIMEOUT))
#   * No validation warnings/errors accumulate
#   * No "previously acquired" / "two images" WSI errors
#
# Codex round-1 repro: 6 captures → validation errors at #2 → Vk(TIMEOUT) at #4.
# After fix (queue_present_khr after each capture), all N must succeed cleanly.
#
# Usage:
#   ./tools/scripts/test-frame-capture-stress.sh R5CX10VFFBA 10
#
# Outputs:
#   .omc/m2-artifacts/M2-S05-stress-result.json
#   /tmp/m2-s05-stress-logcat.txt  (full validation lines for inspection)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial> [count]}"
COUNT="${2:-10}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S05-stress-result.json"
LOGCAT_OUT="/tmp/m2-s05-stress-logcat.txt"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

APP_CACHE="/data/data/dev.warp.mobile/cache"
APP_PNG_BASE="${APP_CACHE}/m2-s05-stress"  # we'll suffix with -N.png per capture
mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL  count: $COUNT ===" >&2
"${ADB[@]}" get-state >&2 || { echo "ERROR: device $SERIAL not online" >&2; exit 1; }

echo "=== uninstall + reinstall to start fresh ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2
"${ADB[@]}" shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS 2>&1 >&2 || true

# POST_NOTIFICATIONS gate.
SDK_VERSION=$("${ADB[@]}" shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r' || print 0)
if [[ "${SDK_VERSION}" -ge 33 ]]; then
    PERM_DUMP=$("${ADB[@]}" shell dumpsys package "$PACKAGE" 2>/dev/null \
                  | grep -A1 "POST_NOTIFICATIONS" || true)
    if ! echo "$PERM_DUMP" | grep -q "granted=true"; then
        echo "ERROR: POST_NOTIFICATIONS not granted=true." >&2
        exit 4
    fi
fi

# Clean stale captures.
for i in $(seq 1 "$COUNT"); do
    "${ADB[@]}" shell run-as "$PACKAGE" rm -f "${APP_PNG_BASE}-${i}.png" 2>&1 >&2 || true
done
"${ADB[@]}" logcat -c

echo "=== keep screen on + dismiss notification shade + keyguard ===" >&2
# Capture original stay-on so we can restore it on exit (S24 Ultra hardening).
ORIG_STAY_ON=$("${ADB[@]}" shell settings get global stay_on_while_plugged_in 2>/dev/null \
                  | tr -d '\r' || print 0)
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

# Probe screen state — if not ON, fail fast (per user mandate "screen MUST
# STAY AWAKE through the entire test run").
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
sleep 1

FOCUS_LINE=$("${ADB[@]}" shell dumpsys window 2>/dev/null | grep "mCurrentFocus" | head -1 || true)
if echo "$FOCUS_LINE" | grep -qE "GrantPermissionsActivity|PermissionController"; then
    echo "ERROR: focus stolen by permission UI: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 5
fi

# Wait for SurfaceView ready.
echo "=== waiting for surfaceCreated_ts ===" >&2
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
    echo "ERROR: surfaceCreated_ts not seen within 10s." >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 2
fi
sleep 2

T_START=$(($(date +%s%N) / 1000000))
echo "=== firing $COUNT CAPTURE_FRAME broadcasts back-to-back ===" >&2
for i in $(seq 1 "$COUNT"); do
    APP_PNG="${APP_PNG_BASE}-${i}.png"
    "${ADB[@]}" shell am broadcast \
        -a dev.warp.mobile.CAPTURE_FRAME \
        -p dev.warp.mobile \
        --es path "$APP_PNG" \
        --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0 \
        2>&1 | tail -1 >&2
    # Probe screen every 5 broadcasts to detect screen-off during the loop
    # (per user mandate "screen MUST STAY AWAKE").
    if (( i % 5 == 0 )); then
        ST=$("${ADB[@]}" shell dumpsys display 2>/dev/null \
                  | grep -E "mScreenState=" | head -1 || true)
        if ! echo "$ST" | grep -q "ON"; then
            echo "ERROR: screen state went non-ON at broadcast $i: $ST" >&2
            "${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
            "${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true
            exit 10
        fi
    fi
done
T_BROADCAST_DONE=$(($(date +%s%N) / 1000000))

# Wait for all captures to land. Each capture is < 200ms typical on flagship;
# allow generous padding.
sleep 3
T_DONE=$(($(date +%s%N) / 1000000))

# Final screen state check post-loop.
ST_END=$("${ADB[@]}" shell dumpsys display 2>/dev/null \
            | grep -E "mScreenState=" | head -1 || true)
echo "=== final screen state: $ST_END ===" >&2
if ! echo "$ST_END" | grep -q "ON"; then
    echo "ERROR: screen state went non-ON before logcat collection: $ST_END" >&2
fi

echo "=== broadcast batch took $((T_BROADCAST_DONE - T_START))ms ===" >&2
echo "=== total elapsed (incl. settle) $((T_DONE - T_START))ms ===" >&2

echo "=== collecting logcat ===" >&2
"${ADB[@]}" logcat -d -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_OUT"

set +e
python3 - "$LOGCAT_OUT" "$SERIAL" "$RESULT_JSON" "$COUNT" "$T_START" "$T_DONE" <<'PYEOF'
import sys, re, json

logfile     = sys.argv[1]
serial      = sys.argv[2]
out_json    = sys.argv[3]
target_n    = int(sys.argv[4])
t_start_ms  = int(sys.argv[5])
t_done_ms   = int(sys.argv[6])

capture_re = re.compile(
    r"capture_ok\s+frame=(\d+)\s+ts=(\d+)\s+dims=(\d+)x(\d+)\s+bytes=(\d+)\s+"
    r"mean_rgb=(\d+),(\d+),(\d+)\s+bgra_swizzled=(\w+)"
)
vkval_re      = re.compile(r"\[VkVal\]")
vkval_sev_re  = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")
timeout_re    = re.compile(r"Vk\(TIMEOUT\)|VK_TIMEOUT|TIMEOUT", re.IGNORECASE)
acquired_re   = re.compile(r"already.*acquired|previously.*acquired", re.IGNORECASE)

captures = []
vkval_lines = []
timeout_lines = []
acquired_lines = []
validation_layer_loaded = False

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line in f:
        m = capture_re.search(line)
        if m:
            captures.append({
                'frame_num':     int(m.group(1)),
                'ts_ms':         int(m.group(2)),
                'width':         int(m.group(3)),
                'height':        int(m.group(4)),
                'bytes_logged':  int(m.group(5)),
                'mean_r':        int(m.group(6)),
                'mean_g':        int(m.group(7)),
                'mean_b':        int(m.group(8)),
                'bgra_swizzled': m.group(9).lower() == 'true',
            })
            continue
        if vkval_re.search(line):
            sev_match = vkval_sev_re.search(line)
            severity = sev_match.group(1) if sev_match else '?'
            vkval_lines.append({'severity': severity, 'line': line.rstrip()})
            if timeout_re.search(line):
                timeout_lines.append(line.rstrip())
            if acquired_re.search(line):
                acquired_lines.append(line.rstrip())
            continue
        if validation_marker_re.search(line):
            validation_layer_loaded = True
        # Also check non-VkVal lines for direct TIMEOUT errors from Rust side
        if timeout_re.search(line) and 'WarpVulkan' in line:
            timeout_lines.append(line.rstrip())

warn_count = sum(1 for v in vkval_lines if v['severity'] == 'W')
err_count  = sum(1 for v in vkval_lines if v['severity'] == 'E')
validation_clean = (
    validation_layer_loaded
    and warn_count == 0
    and err_count == 0
)

# Each capture is one magenta clear; verify all dims/means consistent.
captures_count = len(captures)
all_succeeded  = (captures_count == target_n)
all_magenta    = all(
    c['mean_r'] > 200 and c['mean_g'] < 50 and c['mean_b'] > 200
    for c in captures
)
no_timeouts    = len(timeout_lines) == 0
no_acquired    = len(acquired_lines) == 0

result = {
    'story': 'M2-S05-round-2-stress',
    'device_serial': serial,
    'target_count': target_n,
    'observed_count': captures_count,
    'elapsed_ms': t_done_ms - t_start_ms,
    'captures': captures,
    'validation_layer': {
        'clean': validation_clean,
        'layer_loaded': validation_layer_loaded,
        'warn_count': warn_count,
        'err_count': err_count,
        'sample_lines': vkval_lines[:30],
    },
    'wsi_errors': {
        'timeout_count':       len(timeout_lines),
        'timeout_samples':     timeout_lines[:5],
        'acquired_count':      len(acquired_lines),
        'acquired_samples':    acquired_lines[:5],
    },
    'acceptance_gate': {
        'all_captures_succeeded':    all_succeeded,
        'all_magenta':               all_magenta,
        'validation_clean_pass':     validation_clean,
        'no_vk_timeout':             no_timeouts,
        'no_already_acquired':       no_acquired,
        'overall_pass': (
            all_succeeded and all_magenta and validation_clean
            and no_timeouts and no_acquired
        ),
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

print()
print("=== M2-S05 round-2 stress result ===")
print(f"device:                  {serial}")
print(f"target / observed:       {target_n} / {captures_count}")
print(f"elapsed:                 {t_done_ms - t_start_ms}ms total ({t_done_ms - t_start_ms - 3000}ms broadcast)")
print(f"validation_layer:        loaded={validation_layer_loaded} W={warn_count} E={err_count}")
print(f"wsi: timeouts={len(timeout_lines)} acquired_warnings={len(acquired_lines)}")
print(f"all_magenta:             {all_magenta}")
print(f"GATE overall_pass:       {result['acceptance_gate']['overall_pass']}")

if not result['acceptance_gate']['overall_pass']:
    print()
    print("=== first 5 vkval_lines ===")
    for v in vkval_lines[:5]:
        print(f"  [{v['severity']}] {v['line']}")
    if timeout_lines:
        print("=== timeout samples ===")
        for t in timeout_lines[:3]:
            print(f"  {t}")
    if acquired_lines:
        print("=== acquired_warnings samples ===")
        for a in acquired_lines[:3]:
            print(f"  {a}")

# Exit-code matrix
exit_code = 0
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)
if not all_succeeded:
    exit_code = max(exit_code, 5)
if timeout_lines:
    exit_code = max(exit_code, 6)
if acquired_lines:
    exit_code = max(exit_code, 7)
if not all_magenta:
    exit_code = max(exit_code, 8)
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== cleanup ===" >&2
for i in $(seq 1 "$COUNT"); do
    "${ADB[@]}" shell run-as "$PACKAGE" rm -f "${APP_PNG_BASE}-${i}.png" 2>&1 >&2 || true
done
"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -1 >&2 || true
"${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
"${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true

exit $PARSE_RC
