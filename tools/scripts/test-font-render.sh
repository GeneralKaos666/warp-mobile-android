#!/usr/bin/env zsh
# test-font-render.sh <device-serial>
# M2-S07 device verification driver. Installs the warp-mobile APK, launches
# MainActivity, fires the CAPTURE_FRAME_WITH_TEXT broadcast (renders
# "Hello, 世界" via cosmic-text + Android system fonts onto the magenta clear
# frame), pulls the resulting PNG, and verifies the glyph pixel coverage.
#
# Acceptance gates (per .omc/prd.json M2-S07):
#   * "capture_ok ..." line present (M2-S05 schema; reused without
#     modification — see crates/android-host/src/vulkan.rs:1283-1287)
#   * "font_render_ok via=… fonts_loaded=… glyphs_total=… composed_pixels=…"
#     line present in logcat (M2-S07 schema; new in this story)
#   * fonts_loaded > 0 (system fonts discovered via ASystemFontIterator OR
#     /system/fonts dir scan fallback)
#   * glyphs_total >= 7 (Latin "Hello, " = 7 glyphs minimum; CJK 2 glyphs
#     means total >= 9 with CJK fallback)
#   * composed_pixels > 1000 (sanity check that swash actually rasterized
#     bytes — a single 96px glyph has ~1000-3000 pixels of coverage)
#   * Pulled PNG opens cleanly with PIL, dims match
#   * mean RGB has shifted away from pure magenta (i.e. some pixels are
#     white-ish text — mean_r/mean_b drop OR mean_g rises above the
#     M2-S05 baseline)
#   * Validation layer reports zero warnings/errors during the capture
#     submit/present cycle (regression guard for M2-S04/S05)
#
# Logcat tags consumed:
#   WarpRender — Kotlin lifecycle (CAPTURE_FRAME_WITH_TEXT received, etc.)
#   WarpVulkan — Rust render side (capture_ok, font_render_ok, [VkVal])
#   WarpFont   — Rust font discovery side (ASystemFontIterator, etc.)
#
# Usage:
#   ./tools/scripts/test-font-render.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m2-artifacts/M2-S07-result.json
#   /tmp/m2-s07-capture.png  (pulled bitmap; can be inspected manually)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S07-result.json"
LOCAL_PNG="/tmp/m2-s07-capture.png"

# Render parameters — keep large enough so glyphs are clearly visible at a
# 1080×2400 (S24 Ultra portrait) framebuffer. Baseline near vertical-center
# so the text band is unambiguous.
TEXT="Hello, 世界"
# Base64 encode so multi-byte UTF-8 (世界) survives `adb shell am broadcast
# --es` parsing. Without this the JVM-side Intent extras lose codepoints
# above U+007F when passed through adb's shell command line. The receiver
# decodes `text_b64` if present and falls back to `text` otherwise.
TEXT_B64=$(printf '%s' "$TEXT" | base64 | tr -d '\n')
FONT_SIZE_PX="96.0"
BASELINE_X="100.0"
BASELINE_Y="600.0"

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $REPO_ROOT/android && ./gradlew :app:assembleDebug" >&2
    exit 1
fi

# App writes to its own cache dir (only path the app UID can write on
# Android 10+). We extract via `run-as` to /data/local/tmp where adb pull
# can reach it.
APP_CACHE="/data/data/dev.warp.mobile/cache"
APP_PNG="${APP_CACHE}/m2-s07-capture.png"
DEVICE_PNG="/data/local/tmp/m2-s07-capture.png"

mkdir -p "$ARTIFACT_DIR"

ADB=(adb -s "$SERIAL")

echo "=== device: $SERIAL ===" >&2

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

# Defensive grant: if install -g already granted these, this is a no-op.
"${ADB[@]}" shell pm grant "$PACKAGE" android.permission.POST_NOTIFICATIONS 2>&1 >&2 || true

# M2-S04 round-3 lesson: explicit POST_NOTIFICATIONS=granted assertion.
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

# Clean up any stale capture file from a previous run.
"${ADB[@]}" shell run-as "$PACKAGE" rm -f "$APP_PNG" 2>&1 >&2 || true
"${ADB[@]}" shell rm -f "$DEVICE_PNG" 2>&1 >&2 || true
rm -f "$LOCAL_PNG"

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

echo "=== launching $PACKAGE/$ACTIVITY ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" 2>&1 | tail -2 >&2
sleep 1

# M2-S04 round-3 strict focus assertion (Codex round-2 blocker 1).
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
sleep 2

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
echo "=== no post-creation surfaceDestroyed; proceeding to capture ===" >&2

echo "=== triggering CAPTURE_FRAME_WITH_TEXT broadcast ===" >&2
# Fire the M2-S07 broadcast. The receiver runs on the main thread, calls
# NativeBridge.renderCaptureFrameWithText, which:
#   1. M2-S05 readback path → captured RGBA buffer
#   2. font_render::compose_text_on_rgba shapes "Hello, 世界" + composites
#   3. PNG-encode result to APP_PNG
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.CAPTURE_FRAME_WITH_TEXT \
    -p dev.warp.mobile \
    --es path "$APP_PNG" \
    --ef r 1.0 --ef g 0.0 --ef b 1.0 --ef a 1.0 \
    --es text_b64 "$TEXT_B64" \
    --ef font_size_px "$FONT_SIZE_PX" \
    --ef baseline_x "$BASELINE_X" \
    --ef baseline_y "$BASELINE_Y" \
    2>&1 | tail -3 >&2

# Allow more time than M2-S05: font discovery + cosmic-text shaping + swash
# rasterization adds ~1-3 seconds on first run (system fonts mmapped on
# demand). Empirically <2s on flagship; we wait 4s for safety margin.
sleep 4

echo "=== checking for capture_ok + font_render_ok logcat lines ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s07-logcat.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpRender:I" \
    "WarpVulkan:V" \
    "WarpFont:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE"
trap 'rm -f $LOGCAT_FILE 2>/dev/null || true; keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

if ! grep -q "capture_ok" "$LOGCAT_FILE"; then
    echo "ERROR: no 'capture_ok' line in logcat after capture trigger." >&2
    tail -50 "$LOGCAT_FILE" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 6
fi
if ! grep -q "font_render_ok" "$LOGCAT_FILE"; then
    echo "ERROR: no 'font_render_ok' line in logcat — font pipeline did not run." >&2
    tail -80 "$LOGCAT_FILE" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 8
fi

echo "=== pulling capture from device (run-as $PACKAGE) ===" >&2
"${ADB[@]}" shell "run-as $PACKAGE cat $APP_PNG > $DEVICE_PNG" 2>&1 | tail -3 >&2
"${ADB[@]}" pull "$DEVICE_PNG" "$LOCAL_PNG" 2>&1 | tail -3 >&2

if [[ ! -f "$LOCAL_PNG" ]] || [[ ! -s "$LOCAL_PNG" ]]; then
    echo "ERROR: PNG pull failed; $LOCAL_PNG missing or empty." >&2
    "${ADB[@]}" shell run-as "$PACKAGE" ls -la "$APP_CACHE/" 2>&1 >&2 || true
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 7
fi

LOCAL_SIZE=$(stat -f%z "$LOCAL_PNG" 2>/dev/null || stat -c%s "$LOCAL_PNG" 2>/dev/null || echo 0)
echo "=== captured PNG: $LOCAL_PNG (${LOCAL_SIZE} bytes) ===" >&2

echo "=== parsing + verifying ===" >&2
set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" "$LOCAL_PNG" "$LOCAL_SIZE" "$BASELINE_X" "$BASELINE_Y" "$FONT_SIZE_PX" <<'PYEOF'
import sys, re, json

logfile     = sys.argv[1]
serial      = sys.argv[2]
out_json    = sys.argv[3]
png_path    = sys.argv[4]
png_size    = int(sys.argv[5])
baseline_x  = float(sys.argv[6])
baseline_y  = float(sys.argv[7])
font_size_px= float(sys.argv[8])

# capture_ok line — same schema as M2-S05.
capture_re = re.compile(
    r"capture_ok\s+frame=(\d+)\s+ts=(\d+)\s+dims=(\d+)x(\d+)\s+bytes=(\d+)\s+"
    r"mean_rgb=(\d+),(\d+),(\d+)\s+bgra_swizzled=(\w+)"
)
# font_render_ok via=… fonts_loaded=… families_loaded=… glyphs_total=…
# glyphs_missing=… composed_pixels=… mean_rgb_after=R,G,B
# primary_family=… cjk_family=…
font_re = re.compile(
    r"font_render_ok\s+via=(\S+)\s+fonts_loaded=(\d+)\s+families_loaded=(\d+)\s+"
    r"glyphs_total=(\d+)\s+glyphs_missing=(\d+)\s+composed_pixels=(\d+)\s+"
    r"mean_rgb_after=(\d+),(\d+),(\d+)\s+primary_family=(\S+(?:\s+\S+)*?)\s+cjk_family=(.*)"
)
# More forgiving: capture each kv individually if the all-in-one regex fails.
font_kv_re = re.compile(r"font_render_ok\s+(.*)$")

vkval_re      = re.compile(r"\[VkVal\]")
vkval_sev_re  = re.compile(r"\s([VDIWE])/[A-Za-z0-9_-]+\(")
validation_marker_re = re.compile(r"VK_LAYER_KHRONOS_validation enabled")

capture = None
font = None
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
                'mean_r_before': int(m.group(6)),
                'mean_g_before': int(m.group(7)),
                'mean_b_before': int(m.group(8)),
                'bgra_swizzled': m.group(9).lower() == 'true',
            }
            continue
        # Try the full font_render_ok line.
        kv = font_kv_re.search(line)
        if kv:
            payload = kv.group(1)
            # Parse key=value pairs into a dict.
            #
            # Tricky: family names like `Some("SEC CJK SC")` contain spaces,
            # so we must NOT split tokens on whitespace alone. Instead, scan
            # for `key=value` pairs where `value` is either a quoted/`Some(...)`
            # form OR a non-space token followed by another `key=` boundary.
            # Strategy: walk the payload and extract each key=... up to the
            # next `\s+\w+=` (key boundary regex) OR end-of-string.
            kvs = {}
            kv_pat = re.compile(r"(\w+)=(.*?)(?=\s+\w+=|$)")
            for token in kv_pat.finditer(payload):
                kvs[token.group(1)] = token.group(2).strip()
            try:
                # mean_rgb_after looks like "1,2,3" — keep as string here, parse later.
                font = {
                    'via': kvs.get('via'),
                    'fonts_loaded': int(kvs.get('fonts_loaded', 0)),
                    'families_loaded': int(kvs.get('families_loaded', 0)),
                    'glyphs_total': int(kvs.get('glyphs_total', 0)),
                    'glyphs_missing': int(kvs.get('glyphs_missing', 0)),
                    'composed_pixels': int(kvs.get('composed_pixels', 0)),
                    'mean_rgb_after_raw': kvs.get('mean_rgb_after', '0,0,0'),
                    'primary_family': kvs.get('primary_family'),
                    'cjk_family': kvs.get('cjk_family'),
                }
                rgb_after = font['mean_rgb_after_raw'].split(',')
                if len(rgb_after) == 3:
                    font['mean_r_after'] = int(rgb_after[0])
                    font['mean_g_after'] = int(rgb_after[1])
                    font['mean_b_after'] = int(rgb_after[2])
            except Exception as e:
                font = {'parse_error': repr(e), 'raw': payload}
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
if font is None:
    print("FAIL: no 'font_render_ok' line parseable from logcat", file=sys.stderr)
    sys.exit(2)

# PIL verification.
pil_dims = None
pil_mean = None
pil_mode = None
pil_error = None
glyph_pixel_count = 0
band_pixel_count = 0
band_mean = None
try:
    from PIL import Image
    img = Image.open(png_path)
    img.load()
    pil_dims = list(img.size)
    pil_mode = img.mode
    if img.mode != 'RGBA':
        img = img.convert('RGBA')
    band_means = []
    for ch in range(4):
        ch_data = list(img.getdata(band=ch))
        band_means.append(sum(ch_data) / max(1, len(ch_data)))
    pil_mean = {'r': band_means[0], 'g': band_means[1], 'b': band_means[2], 'a': band_means[3]}

    # Glyph-pixel coverage analysis: count pixels where green > 50 (clear
    # color is magenta = 255,0,255 → green=0). Any non-trivial green presence
    # means swash blitted a glyph mask onto that pixel.
    pixels = img.load()
    width, height = img.size
    total = 0
    for y in range(height):
        for x in range(width):
            px = pixels[x, y]
            if len(px) >= 3 and px[1] > 50:
                total += 1
    glyph_pixel_count = total

    # Spot-check a band around the baseline. Text drawn at font_size_px
    # extends roughly [baseline_y - font_size_px, baseline_y + 0.3*font_size_px].
    band_top = max(0, int(baseline_y - font_size_px))
    band_bot = min(height, int(baseline_y + font_size_px * 0.4))
    band_pixel_count_total = 0
    band_glyph_pixel_count = 0
    sr = sg = sb = 0
    for y in range(band_top, band_bot):
        for x in range(width):
            px = pixels[x, y]
            sr += px[0]
            sg += px[1]
            sb += px[2]
            band_pixel_count_total += 1
            # Glyph pixel = green channel > 50 (clear color is magenta = 255,0,255).
            if len(px) >= 3 and px[1] > 50:
                band_glyph_pixel_count += 1
    if band_pixel_count_total > 0:
        band_pixel_count = band_pixel_count_total
        band_mean = {
            'r': sr / band_pixel_count_total,
            'g': sg / band_pixel_count_total,
            'b': sb / band_pixel_count_total,
            'top': band_top,
            'bottom': band_bot,
            'glyph_pixel_count_in_band': band_glyph_pixel_count,
        }
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

# Acceptance gates per .omc/prd.json M2-S07:
#   1. dims match between PIL and Rust log
#   2. fonts_loaded > 0
#   3. glyphs_total >= 7 (Latin "Hello, " has 7 glyphs)
#   4. composed_pixels > 1000
#   5. PNG band around the baseline has shifted away from pure magenta
#      (band_mean.g > 30 → swash glyphs blitted white over magenta)
#   6. Validation layer reports zero W/E lines
dims_match = pil_dims is not None and pil_dims == [capture['width'], capture['height']]
fonts_ok   = font.get('fonts_loaded', 0) > 0
glyphs_ok  = font.get('glyphs_total', 0) >= 7
composed_ok = font.get('composed_pixels', 0) > 1000
# `glyphs_visible`: at least 1000 pixels in the baseline-ish band have a
# green-channel value > 50 (the only way for green to be > 50 is white text
# blended on top of magenta clear color). 1000 is a deliberately loose
# threshold — even one Latin glyph at 96px covers ~700-1500 pixels of mask
# coverage, and CJK glyphs are denser. Below 1000 = no real text rendered.
glyphs_visible = (
    band_mean is not None
    and band_mean.get('glyph_pixel_count_in_band', 0) > 1000
)
file_size_ok = png_size > 0

result = {
    'story': 'M2-S07',
    'device_serial': serial,
    'png_path_local': png_path,
    'png_file_size_bytes': png_size,
    'capture': capture,
    'font_render': font,
    'render_params': {
        'baseline_x': baseline_x,
        'baseline_y': baseline_y,
        'font_size_px': font_size_px,
    },
    'pil_verify': {
        'dims': pil_dims,
        'mode': pil_mode,
        'mean': pil_mean,
        'error': pil_error,
        'glyph_pixel_count': glyph_pixel_count,
        'band': band_mean,
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
        'font_render_ok_seen': True,
        'dims_match': dims_match,
        'fonts_loaded_gt_zero': fonts_ok,
        'glyphs_total_ge_7': glyphs_ok,
        'composed_pixels_gt_1000': composed_ok,
        'glyphs_visible_in_band': glyphs_visible,
        'file_size_ok': file_size_ok,
        'validation_clean_pass': validation_clean,
        'overall_pass': (
            dims_match and fonts_ok and glyphs_ok and composed_ok
            and glyphs_visible and file_size_ok and validation_clean
        ),
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2)

print("\n=== M2-S07 result summary ===")
print(f"device:                     {serial}")
print(f"png_file_size_bytes:        {png_size}")
print(f"rust_log_dims:              {capture['width']}x{capture['height']}")
print(f"rust_log_mean_rgb_before:   {capture['mean_r_before']},{capture['mean_g_before']},{capture['mean_b_before']}")
print(f"font_via:                   {font.get('via')}")
print(f"fonts_loaded:               {font.get('fonts_loaded')}")
print(f"families_loaded:            {font.get('families_loaded')}")
print(f"glyphs_total/missing:       {font.get('glyphs_total')}/{font.get('glyphs_missing')}")
print(f"composed_pixels:            {font.get('composed_pixels')}")
print(f"mean_rgb_after:             {font.get('mean_r_after')},{font.get('mean_g_after')},{font.get('mean_b_after')}")
print(f"primary_family:             {font.get('primary_family')}")
print(f"cjk_family:                 {font.get('cjk_family')}")
if pil_dims:
    print(f"pil_dims:                   {pil_dims[0]}x{pil_dims[1]} mode={pil_mode}")
    print(f"pil_mean:                   R={pil_mean['r']:.1f} G={pil_mean['g']:.1f} B={pil_mean['b']:.1f}")
    print(f"glyph_pixel_count:          {glyph_pixel_count}")
    if band_mean:
        print(
            f"band_mean (rows {band_mean['top']}-{band_mean['bottom']}): "
            f"R={band_mean['r']:.1f} G={band_mean['g']:.1f} B={band_mean['b']:.1f} "
            f"glyph_pixels_in_band={band_mean.get('glyph_pixel_count_in_band')}"
        )
else:
    print(f"pil_error:                  {pil_error}")
print(f"validation_layer_loaded:    {validation_layer_loaded}")
print(f"validation_warn_count:      {warn_count}")
print(f"validation_err_count:       {err_count}")
print(
    f"GATE: dims={dims_match} fonts={fonts_ok} glyphs={glyphs_ok} "
    f"composed={composed_ok} visible={glyphs_visible} clean={validation_clean} "
    f"overall_pass={result['acceptance_gate']['overall_pass']}"
)

# Exit-code matrix, mirroring test-frame-capture.sh family but with M2-S07-
# specific gates:
exit_code = 0
if not validation_layer_loaded:
    exit_code = max(exit_code, 3)
if warn_count > 0 or err_count > 0:
    exit_code = max(exit_code, 4)
if not fonts_ok:
    exit_code = max(exit_code, 20)
if not glyphs_ok:
    exit_code = max(exit_code, 21)
if not composed_ok:
    exit_code = max(exit_code, 22)
if not glyphs_visible:
    exit_code = max(exit_code, 23)
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
"${ADB[@]}" shell svc power stayon false 2>&1 >&2 || true
"${ADB[@]}" shell settings put global stay_on_while_plugged_in "$ORIG_STAY_ON" 2>&1 >&2 || true

exit $PARSE_RC
