#!/usr/bin/env zsh
# test-frame-capture.sh <device-serial>
# M2-S05 device verification driver. Installs the warp-mobile APK, launches
# MainActivity, fires the CAPTURE_FRAME broadcast, pulls the resulting PNG,
# and verifies dimensions + mean RGB matches expected magenta clear color.
#
# Acceptance gates (per .omc/prd.json M2-S05):
#   * "capture_ok frame=N ts=M dims=WxH bytes=B mean_rgb=R,G,B" line in logcat
#   * Pulled PNG opens cleanly with PIL, dims match
#   * mean R > 200, mean G < 50, mean B > 200 (magenta = 0xFF00FF)
#   * Validation layer reports zero warnings/errors during capture
#
# Logcat tags consumed:
#   WarpRender   — Kotlin lifecycle (CAPTURE_FRAME received, etc.)
#   WarpVulkan   — Rust render side (capture_ok, [VkVal] validation)
#
# Usage:
#   ./tools/scripts/test-frame-capture.sh RFCY71LAFYE
#
# Outputs:
#   .omc/m2-artifacts/M2-S05-result.json
#   /tmp/m2-s05-capture.png  (pulled bitmap; can be inspected manually)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S05-result.json"
LOCAL_PNG="/tmp/m2-s05-capture.png"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

# App writes to its own cache dir (only path the app UID can write on
# Android 10+). We extract via `run-as` to /data/local/tmp where adb pull
# can reach it.
APP_CACHE="/data/data/dev.warp.mobile/cache"
APP_PNG="${APP_CACHE}/m2-s05-capture.png"
DEVICE_PNG="/data/local/tmp/m2-s05-capture.png"

mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL ===" >&2

"${ADB[@]}" get-state >&2 || {
    echo "ERROR: device $SERIAL not online" >&2
    exit 1
}

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK (with -g to grant runtime permissions) ===" >&2
# Round-2 lesson from M2-S04: -g auto-grants runtime permissions; without it
# the POST_NOTIFICATIONS dialog steals focus from SurfaceView and we never
# reach surfaceCreated.
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

# Defensive grant: if install -g already granted these, this is a no-op.
"${ADB[@]}" shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS 2>&1 >&2 || true

# Round-3 M2-S04 lesson: explicit POST_NOTIFICATIONS=granted assertion.
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

# Clean up any stale capture file from a previous run so we don't accidentally
# pull a phantom and report "PASS" when this run's capture actually failed.
# App-private file: must use run-as. /data/local/tmp file: shell can rm directly.
"${ADB[@]}" shell run-as "$PACKAGE" rm -f "$APP_PNG" 2>&1 >&2 || true
"${ADB[@]}" shell rm -f "$DEVICE_PNG" 2>&1 >&2 || true
rm -f "$LOCAL_PNG"

echo "=== clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== keep screen on for the duration ===" >&2
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 7 2>&1 >&2 || true
"${ADB[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
sleep 0.5

echo "=== launching $PACKAGE/$ACTIVITY ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" 2>&1 | tail -2 >&2
sleep 1

# M2-S04 lesson: focus check post-launch.
FOCUS_LINE=$("${ADB[@]}" shell dumpsys window 2>/dev/null | grep "mCurrentFocus" | head -1 || true)
if echo "$FOCUS_LINE" | grep -qE "GrantPermissionsActivity|PermissionController"; then
    echo "ERROR: focus stolen by permission UI: ${FOCUS_LINE}" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 5
fi

# Wait for surfaceCreated_ts.
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
    "${ADB[@]}" shell dumpsys window 2>/dev/null | grep -E "mCurrentFocus|mFocusedApp" | head -5 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 2
fi
# Let the render path stabilize for a few frames before triggering capture.
sleep 2

echo "=== triggering CAPTURE_FRAME broadcast ===" >&2
# Send the broadcast. The manifest-registered CaptureFrameReceiver picks it
# up and calls NativeBridge.renderCaptureFrame. The receiver runs on the main
# thread (default for manifest receivers), so it serializes naturally with
# the Choreographer per-vsync render loop via the swapchain mutex on the
# Rust side.
#
# IMPORTANT: write target is the app-private cache dir, not /data/local/tmp.
# On Android 10+, app UIDs cannot write to /data/local/tmp (only `shell` UID
# can). The app cache dir is reachable via `run-as` for `adb pull`.
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.CAPTURE_FRAME \
    -p dev.warp.mobile \
    --es path "$APP_PNG" \
    --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0 \
    2>&1 | tail -3 >&2

# Wait for the capture to land — the choreographer pause + Vulkan submit +
# wait_idle + PNG encode is well under 1s on flagship.
sleep 2

echo "=== checking for capture_ok logcat line ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s05-logcat.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE"
trap "rm -f $LOGCAT_FILE" EXIT

if ! grep -q "capture_ok" "$LOGCAT_FILE"; then
    echo "ERROR: no 'capture_ok' line in logcat after capture trigger." >&2
    echo "=== logcat tail ===" >&2
    tail -50 "$LOGCAT_FILE" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 6
fi

echo "=== pulling capture from device (run-as $PACKAGE) ===" >&2
# Two-step: copy from app-private cache to /data/local/tmp via run-as cat
# (which streams the file with the app's UID), then adb pull from there.
"${ADB[@]}" shell "run-as $PACKAGE cat $APP_PNG > $DEVICE_PNG" 2>&1 | tail -3 >&2
"${ADB[@]}" pull "$DEVICE_PNG" "$LOCAL_PNG" 2>&1 | tail -3 >&2

if [[ ! -f "$LOCAL_PNG" ]] || [[ ! -s "$LOCAL_PNG" ]]; then
    echo "ERROR: PNG pull failed; $LOCAL_PNG missing or empty." >&2
    echo "  app cache state:" >&2
    "${ADB[@]}" shell run-as "$PACKAGE" ls -la "$APP_CACHE/" 2>&1 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 7
fi

LOCAL_SIZE=$(stat -f%z "$LOCAL_PNG" 2>/dev/null || stat -c%s "$LOCAL_PNG" 2>/dev/null || echo 0)
echo "=== captured PNG: $LOCAL_PNG (${LOCAL_SIZE} bytes) ===" >&2

echo "=== parsing + verifying ===" >&2

set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" "$LOCAL_PNG" "$LOCAL_SIZE" <<'PYEOF'
import sys, re, json

logfile     = sys.argv[1]
serial      = sys.argv[2]
out_json    = sys.argv[3]
png_path    = sys.argv[4]
png_size    = int(sys.argv[5])

# Parse capture_ok line:
#   "capture_ok frame=<n> ts=<ms> dims=<W>x<H> bytes=<B> mean_rgb=<R>,<G>,<B> bgra_swizzled=<bool>"
capture_re = re.compile(
    r"capture_ok\s+frame=(\d+)\s+ts=(\d+)\s+dims=(\d+)x(\d+)\s+bytes=(\d+)\s+"
    r"mean_rgb=(\d+),(\d+),(\d+)\s+bgra_swizzled=(\w+)"
)
vkval_re      = re.compile(r"\[VkVal\]")
vkval_sev_re  = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")

capture = None
vkval_lines = []
validation_layer_loaded = False

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line in f:
        m = capture_re.search(line)
        if m:
            capture = {
                'frame_num': int(m.group(1)),
                'ts_ms': int(m.group(2)),
                'width': int(m.group(3)),
                'height': int(m.group(4)),
                'bytes_logged': int(m.group(5)),
                'mean_r': int(m.group(6)),
                'mean_g': int(m.group(7)),
                'mean_b': int(m.group(8)),
                'bgra_swizzled': m.group(9).lower() == 'true',
            }
            continue
        if vkval_re.search(line):
            sev_match = vkval_sev_re.search(line)
            severity = sev_match.group(1) if sev_match else '?'
            vkval_lines.append({'severity': severity, 'line': line.rstrip()})
            continue
        if validation_marker_re.search(line):
            validation_layer_loaded = True

if capture is None:
    print("FAIL: no 'capture_ok' line parseable from logcat", file=sys.stderr)
    sys.exit(1)

# Verify PNG via PIL (provides independent dim + mean RGB cross-check).
pil_dims = None
pil_mean = None
pil_mode = None
pil_error = None
try:
    from PIL import Image
    img = Image.open(png_path)
    img.load()
    pil_dims = list(img.size)  # (width, height)
    pil_mode = img.mode
    if img.mode != 'RGBA':
        img = img.convert('RGBA')
    # Compute per-channel mean.
    band_means = []
    for ch in range(4):
        ch_data = list(img.getdata(band=ch))
        band_means.append(sum(ch_data) / max(1, len(ch_data)))
    pil_mean = {'r': band_means[0], 'g': band_means[1], 'b': band_means[2], 'a': band_means[3]}
except Exception as e:
    pil_error = repr(e)

# Validation cleanliness: zero W/E lines AND layer was loaded.
warn_count = sum(1 for v in vkval_lines if v['severity'] == 'W')
err_count  = sum(1 for v in vkval_lines if v['severity'] == 'E')
validation_clean = (
    validation_layer_loaded
    and warn_count == 0
    and err_count == 0
)

# Acceptance gates per .omc/prd.json M2-S05:
#   * dims match between PIL and Rust log
#   * NOT all-black (mean_r > 200, mean_g < 50, mean_b > 200 for magenta)
#   * file size > 0
dims_match = pil_dims is not None and pil_dims == [capture['width'], capture['height']]
not_black  = capture['mean_r'] > 200 and capture['mean_b'] > 200
magenta_ok = (
    capture['mean_r'] > 200 and
    capture['mean_g'] < 50  and
    capture['mean_b'] > 200
)
file_size_ok = png_size > 0

result = {
    'story': 'M2-S05',
    'device_serial': serial,
    'png_path_local': png_path,
    'png_file_size_bytes': png_size,
    'capture': capture,
    'pil_verify': {
        'dims': pil_dims,
        'mode': pil_mode,
        'mean': pil_mean,
        'error': pil_error,
    },
    'validation_layer': {
        'clean': validation_clean,
        'layer_loaded': validation_layer_loaded,
        'warn_count': warn_count,
        'err_count': err_count,
        'sample_lines': vkval_lines[:20],
    },
    'acceptance_gate': {
        'capture_ok_seen': True,
        'dims_match': dims_match,
        'not_black': not_black,
        'magenta_ok': magenta_ok,
        'file_size_ok': file_size_ok,
        'validation_clean_pass': validation_clean,
        'overall_pass': (
            dims_match and not_black and magenta_ok and file_size_ok
            and validation_clean
        ),
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

print("\n=== M2-S05 result summary ===")
print(f"device:                    {serial}")
print(f"png_file_size_bytes:       {png_size}")
print(f"rust_log_dims:             {capture['width']}x{capture['height']}")
print(f"rust_log_mean_rgb:         {capture['mean_r']},{capture['mean_g']},{capture['mean_b']}")
print(f"rust_log_bgra_swizzled:    {capture['bgra_swizzled']}")
if pil_dims:
    print(f"pil_dims:                  {pil_dims[0]}x{pil_dims[1]} mode={pil_mode}")
    print(f"pil_mean:                  R={pil_mean['r']:.1f} G={pil_mean['g']:.1f} B={pil_mean['b']:.1f} A={pil_mean['a']:.1f}")
else:
    print(f"pil_error:                 {pil_error}")
print(f"validation_layer_loaded:   {validation_layer_loaded}")
print(f"validation_warn_count:     {warn_count}")
print(f"validation_err_count:      {err_count}")
print(f"GATE: dims_match={dims_match} magenta_ok={magenta_ok} validation_clean={validation_clean} overall_pass={result['acceptance_gate']['overall_pass']}")

# Exit-code matrix mirrors test-render-scene.sh:
exit_code = 0
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)
if not magenta_ok:
    exit_code = max(exit_code, 5)
if not dims_match:
    exit_code = max(exit_code, 6)
if not file_size_ok:
    exit_code = max(exit_code, 7)
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== done ===" >&2

# Always cleanup the device-side capture so a stale file doesn't poison a
# future run.
"${ADB[@]}" shell run-as "$PACKAGE" rm -f "$APP_PNG" 2>&1 >&2 || true
"${ADB[@]}" shell rm -f "$DEVICE_PNG" 2>&1 >&2 || true
"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true
"${ADB[@]}" shell settings put global stay_on_while_plugged_in 0 2>&1 >&2 || true

exit $PARSE_RC
