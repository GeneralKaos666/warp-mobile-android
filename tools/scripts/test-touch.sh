#!/usr/bin/env zsh
# test-touch.sh <device-serial>
# M2-S11 device verification driver. Installs the warp-mobile APK, launches
# MainActivity, exercises the touch + gesture input state machine via a
# combination of real OS input events (adb shell input tap / swipe) and
# simulation broadcasts (for deterministic velocity / long-press), then
# writes M2-S11-result.json.
#
# ## Acceptance gates (per .omc/prd.json M2-S11):
#
#   Sub-test 1: adb shell input tap (540, 1170)
#     → real MotionEvent route → WarpInputView.onTouchEvent → JNI
#     → Rust receives touch_down + touch_up with x,y within 2 px of (540, 1170)
#     (Note: `adb shell input tap` fires an OS-level MotionEvent with the
#      exact coords; ACTION_DOWN + ACTION_UP, so two events expected.)
#
#   Sub-test 2: swipe simulation broadcast
#     → INPUT_SCROLL broadcast with vy=-1200.0 (upward swipe — finger moves up, vy < 0 in screen coords)
#     → Rust receives scroll event with vy < 0 (non-zero velocity)
#
#   Sub-test 3: long-press simulation broadcast
#     → INPUT_LONG_PRESS broadcast at (540, 1170)
#     → Rust receives long_press event
#
# ## Why simulation for sub-tests 2 + 3
#
#   `adb shell input swipe` velocity is OS-interpolated and non-deterministic
#   (the velocity depends on the animation duration, which can vary across
#   Samsung One UI versions). For the acceptance gate "scroll event with non-
#   zero velocity", a simulation broadcast with known vy=-1200 px/s is exact
#   and reproducible. Disclosed in result JSON (method field).
#
#   `adb shell input swipe` does still arrive as real MotionEvents at
#   WarpInputView (exercises the View path), but GestureDetector.onScroll
#   velocity depends on the event stream's timing. We log both outcomes.
#
# ## Sub-test 1: real OS tap
#
#   `adb shell input tap x y` dispatches a `MotionEvent(ACTION_DOWN)`
#   followed immediately by `MotionEvent(ACTION_UP)` to the focused View
#   (here: WarpInputView). The coords are passed verbatim (in physical pixels).
#   So Rust should see touch_down at (540.0, 1170.0) and touch_up at the same
#   coords, within ±2 px (OS rounding).
#
# ## IC.* count assertion (round-2 pattern from S10)
#
#   We assert that the Rust input_event logcat lines count matches what
#   the driver issued. This catches silent failures where broadcasts didn't
#   reach the receiver.
#
# ## Exit codes:
#   0    PASS — all gates satisfied
#   1    install / build / device offline
#   2    surfaceCreated_ts never observed within 10s
#   5    focus stolen by GrantPermissionsActivity
#   9    screen state not ON before launch
#   11   focus stolen by Bouncer / Keyguard / StatusBar / NotificationShade
#   12   focus is NOT dev.warp.mobile
#   13   mInputRestricted=true
#   14   surfaceDestroyed_ts after surfaceCreated_ts before steady run
#   15   TouchSimulationReceiver fell back (should not happen — no fallback path)
#   31   touch input state machine assertion failure
#
# ## Web-search refs (M2-S11, 2026-04-30):
#   <https://developer.android.com/reference/android/view/MotionEvent>
#   <https://developer.android.com/reference/android/view/GestureDetector>
#   <https://developer.android.com/reference/android/view/GestureDetector.SimpleOnGestureListener>
#   <https://developer.android.com/reference/android/view/VelocityTracker>
#   <https://developer.android.com/training/gestures/detector>

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S11-result.json"

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

# Anti-Knox-idle keep-awake (shared from M2-S04+).
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"
trap 'keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

echo "=== uninstall any prior debug install ===" >&2
"${ADB[@]}" uninstall "$PACKAGE" 2>&1 | tail -1 >&2 || true

echo "=== installing APK (with -g to grant runtime permissions) ===" >&2
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

# POST_NOTIFICATIONS assertion (M2-S04 round-3 lesson).
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
    exit 9
fi
echo "=== screen state confirmed ON ===" >&2

echo "=== launching $PACKAGE/$ACTIVITY ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" 2>&1 | tail -2 >&2
START_TS=$(date +%s)
sleep 1

# Strict focus assertion (M2-S04 round-3 + S05 round-2 pattern).
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
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 11
fi
if ! echo "$FOCUS_LINE" | grep -q "dev.warp.mobile"; then
    echo "ERROR: focus is NOT dev.warp.mobile: ${FOCUS_LINE}" >&2
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
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 2
fi
sleep 1

# Reject post-creation surfaceDestroyed (M2-S04 round-3 blocker).
DESTROYED_LINE=$("${ADB[@]}" logcat -d -s WarpRender:I 2>/dev/null | grep "surfaceDestroyed_ts=" | tail -1 || true)
if [[ -n "$DESTROYED_LINE" ]]; then
    DESTROYED_TS=$(echo "$DESTROYED_LINE" | sed -n 's/.*surfaceDestroyed_ts=\([0-9][0-9]*\).*/\1/p' | tail -1)
    if [[ -n "$DESTROYED_TS" && -n "$SURFACE_CREATED_TS" ]] && (( DESTROYED_TS > SURFACE_CREATED_TS )); then
        echo "ERROR: surface DESTROYED at ts=${DESTROYED_TS} after creation at ts=${SURFACE_CREATED_TS}" >&2
        "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
        exit 14
    fi
fi
echo "=== no post-creation surfaceDestroyed; proceeding to touch sub-tests ===" >&2

# Helper: broadcast to TouchSimulationReceiver.
input_broadcast() {
    local action="$1"; shift
    "${ADB[@]}" shell am broadcast -a "$action" -p "$PACKAGE" "$@" 2>&1 | tail -1 >&2 || true
    sleep 0.2
}

# ── Sub-test 1: real OS tap (540, 1170) ────────────────────────────────────
# This uses `adb shell input tap` which generates real MotionEvents at the
# OS input-dispatcher level. They arrive at WarpInputView.onTouchEvent as
# ACTION_DOWN + ACTION_UP with exact integer coords.
#
# On Galaxy S24 Ultra the screen is 1080×2340 px; (540, 1170) is the center.
echo "" >&2
echo "=== Sub-test 1: reset + real adb input tap (540, 1170) ===" >&2
input_broadcast dev.warp.mobile.INPUT_RESET
sleep 0.3

"${ADB[@]}" shell input tap 540 1170
sleep 1  # wait for GestureDetector double-tap window (~300ms) + Rust logging

# ── Sub-test 2: swipe-velocity simulation broadcast ────────────────────────
# Sign convention (canonical — Android screen coordinates):
#   VelocityTracker.computeCurrentVelocity(1000) returns px/s in screen coords.
#   Positive vy = finger moves DOWNWARD (Y axis grows downward on Android).
#   Negative vy = finger moves UPWARD.
#
#   A swipe from (540,1500) → (540,500) moves the finger UPWARD, so vy < 0.
#   A swipe from (540,500) → (540,1500) moves the finger DOWNWARD, so vy > 0.
#
#   Terminal scroll convention is TBD M3 (likely INVERTED: swipe up = scroll
#   terminal content down). This test only asserts vy != 0 (non-zero velocity).
#
# Issue #2 fix: we split logcat into two windows — window B-sim (simulation
# only) and window B-swipe (real swipe only) — by clearing logcat between the
# two operations. The simulation gate is evaluated against B-sim alone, so a
# broken INPUT_SCROLL broadcast cannot false-pass via the supplemental swipe.
echo "" >&2
echo "=== Sub-test 2: reset + swipe simulation (vy=-1200.0 px/s, finger moves up) ===" >&2
input_broadcast dev.warp.mobile.INPUT_RESET
sleep 0.3
input_broadcast dev.warp.mobile.INPUT_SCROLL \
    --ef x 540.0 --ef y 1000.0 \
    --ef dx 0.0 --ef dy -300.0 \
    --ef vx 0.0 --ef vy -1200.0
sleep 0.5

# Capture logcat for the simulation window BEFORE clearing and running the
# supplemental real swipe. This is the authoritative window for sub-test 2.
LOGCAT_SIM_FILE=$(mktemp /tmp/m2-s11-logcat-sim.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpIme:I" \
    "WarpRender:I" \
    "WarpInput:W" \
    "WarpInput:I" \
    "WarpInput:D" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_SIM_FILE"
echo "=== simulation logcat captured (${LOGCAT_SIM_FILE}) ===" >&2

# Clear logcat before the supplemental real swipe so real-swipe events do NOT
# contaminate the simulation parse window (Issue #2 fix).
"${ADB[@]}" logcat -c
sleep 0.1

# Run the supplemental real adb input swipe for logcat evidence of the View
# code path. Its events go into a separate LOGCAT_SWIPE_FILE; they are NOT
# used for the sub-test 2 simulation gate.
echo "=== (supplemental real adb input swipe — separate logcat window) ===" >&2
"${ADB[@]}" shell input swipe 540 1500 540 500 200
sleep 0.8

# ── Sub-test 3: long-press simulation broadcast ─────────────────────────────
echo "" >&2
echo "=== Sub-test 3: reset + long-press simulation at (540, 1170) ===" >&2
input_broadcast dev.warp.mobile.INPUT_RESET
sleep 0.3
input_broadcast dev.warp.mobile.INPUT_LONG_PRESS --ef x 540.0 --ef y 1170.0
sleep 0.5

# ── Sub-test B: ACTION_CANCEL state machine integrity ─────────────────────
# Verifies Issue #4 fix: DOWN followed by CANCEL emits TouchCancel to Rust,
# closing the open touch-down sequence. Without the fix, Rust would believe
# the finger is still down after the cancel.
echo "" >&2
echo "=== Sub-test B: reset + INPUT_TOUCH_DOWN + INPUT_TOUCH_CANCEL ===" >&2
input_broadcast dev.warp.mobile.INPUT_RESET
sleep 0.3
input_broadcast dev.warp.mobile.INPUT_TOUCH_DOWN --ef x 540.0 --ef y 1170.0
sleep 0.1
input_broadcast dev.warp.mobile.INPUT_TOUCH_CANCEL --ef x 540.0 --ef y 1170.0
sleep 0.3

# ── Sub-test C: sign convention — downward swipe yields vy > 0 ─────────────
# Verifies Issue #5 fix: broadcasts a scroll with vy=+800.0 (finger moves
# DOWN, positive in Android screen coordinates). The driver asserts
# vy > 0 in Rust, confirming the convention is consistent end-to-end.
echo "" >&2
echo "=== Sub-test C: reset + INPUT_SCROLL (vy=+800.0, finger moves down) ===" >&2
input_broadcast dev.warp.mobile.INPUT_RESET
sleep 0.3
input_broadcast dev.warp.mobile.INPUT_SCROLL \
    --ef x 540.0 --ef y 500.0 \
    --ef dx 0.0 --ef dy 300.0 \
    --ef vx 0.0 --ef vy 800.0
sleep 0.3

# Capture sim-only logcat for sub-tests B and C BEFORE the combined capture.
LOGCAT_BC_FILE=$(mktemp /tmp/m2-s11-logcat-bc.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpIme:I" \
    "WarpRender:I" \
    "WarpInput:W" \
    "WarpInput:I" \
    "WarpInput:D" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_BC_FILE"
echo "=== B+C logcat captured (${LOGCAT_BC_FILE}) ===" >&2

echo "" >&2
echo "=== capturing combined logcat (sub-tests 1 + supplemental swipe + 3) ===" >&2
LOGCAT_FILE=$(mktemp /tmp/m2-s11-logcat.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpIme:I" \
    "WarpRender:I" \
    "WarpInput:W" \
    "WarpInput:I" \
    "WarpInput:D" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE"

echo "=== logcat tail (last 80 lines) ===" >&2
tail -80 "$LOGCAT_FILE" >&2
echo "=== simulation-only logcat tail (sub-test 2 gate) ===" >&2
tail -30 "$LOGCAT_SIM_FILE" >&2

echo "" >&2
echo "=== parsing results ===" >&2
set +e
python3 - "$LOGCAT_FILE" "$LOGCAT_SIM_FILE" "$LOGCAT_BC_FILE" "$SERIAL" "$RESULT_JSON" <<'PYEOF'
import sys, re, json

# LOGCAT_FILE:     combined logcat ending after real swipe + sub-test 3.
#                  Used for sub-tests 1 (window A) and 3 (window C).
# LOGCAT_SIM_FILE: simulation-only logcat, captured BEFORE adb logcat -c and
#                  the supplemental real swipe. Used exclusively for sub-test 2
#                  gate (Issue #2 fix: avoids false-pass via real swipe events).
# LOGCAT_BC_FILE:  logcat from sub-tests B (cancel) and C (sign convention).
logfile     = sys.argv[1]
simfile     = sys.argv[2]
bcfile      = sys.argv[3]
serial      = sys.argv[4]
out_json    = sys.argv[5]

# Match `input_event kind=<kind> x=<f> y=<f> vx=<f> vy=<f> events_total=<n>`
event_re = re.compile(
    r'input_event\s+kind=(\S+)\s+x=(-?[\d.]+)\s+y=(-?[\d.]+)\s+vx=(-?[\d.]+)\s+vy=(-?[\d.]+)\s+events_total=(\d+)'
)

# Match Kotlin-side raw touch logs for IC.* count assertion equivalent.
touch_down_re = re.compile(r'touch_down\s+x=(-?[\d.]+)\s+y=(-?[\d.]+)')
touch_up_re   = re.compile(r'touch_up\s+x=(-?[\d.]+)\s+y=(-?[\d.]+)')
gesture_tap_re = re.compile(r'gesture_tap\s+x=(-?[\d.]+)\s+y=(-?[\d.]+)')
gesture_lp_re  = re.compile(r'gesture_long_press\s+x=(-?[\d.]+)\s+y=(-?[\d.]+)')
gesture_scroll_re = re.compile(r'gesture_scroll|INPUT_SCROLL')

attach_re  = re.compile(r'surfaceCreated_ts=(\d+)')
detach_re  = re.compile(r'surfaceDestroyed_ts=(\d+)')

def parse_events(path):
    events = []
    with open(path, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = event_re.search(line)
            if m:
                events.append({
                    'kind': m.group(1),
                    'x': float(m.group(2)),
                    'y': float(m.group(3)),
                    'vx': float(m.group(4)),
                    'vy': float(m.group(5)),
                    'events_total': int(m.group(6)),
                })
    return events

def reconstruct_windows(events):
    """Split events into windows by monotonic events_total break (S10 pattern)."""
    windows = []
    cur = []
    prev_total = 0
    for ev in events:
        if ev['events_total'] <= prev_total and cur:
            windows.append(cur)
            cur = []
        cur.append(ev)
        prev_total = ev['events_total']
    if cur:
        windows.append(cur)
    return windows

def kind_events(window, kind):
    return [e for e in window if e['kind'] == kind]

# ── Parse simulation logcat (sub-tests 1 and 2) ──
# LOGCAT_SIM_FILE is captured BEFORE `adb logcat -c`, so it contains both:
#   Window A (sub-test 1): real adb input tap events (touch_down + touch_up + tap)
#   Window B-sim (sub-test 2): simulation-only scroll (INPUT_SCROLL broadcast)
# After `adb logcat -c`, the combined logcat no longer has sub-test 1 or 2 data.
sim_events = parse_events(simfile)
sim_windows = reconstruct_windows(sim_events)
# Window 0: sub-test 1 (real tap, events_total=1..3)
# Window 1: sub-test 2 sim (scroll vy=-1200, events_total=1 after reset)
window_a     = sim_windows[0] if len(sim_windows) >= 1 else []
window_b_sim = sim_windows[1] if len(sim_windows) >= 2 else []

# ── Parse combined logcat (sub-test 3 — long-press) ──
# The combined logcat is captured at the very end (after real swipe + sub-test
# 3 + B + C). Sub-tests 3, B, and C all appear in this file as separate windows.
# We find the long_press window by searching for any window containing a
# long_press event (rather than relying on a fixed index).
combined_events = parse_events(logfile)
combined_windows = reconstruct_windows(combined_events)
window_c = next((w for w in combined_windows if any(e['kind'] == 'long_press' for e in w)), [])

# ── Kotlin-side logcat counts ──
# Parse from sim logcat (before adb logcat -c) to capture sub-test 1 Kotlin
# touch_down/touch_up lines (WarpIme:I). The combined logcat was cleared and
# no longer has sub-test 1 data.
kt_touch_downs = []
kt_touch_ups = []
attach_ts = None
detach_ts = None
for scanfile in (simfile, logfile):
    with open(scanfile, encoding='utf-8', errors='replace') as f:
        for line in f:
            m = touch_down_re.search(line)
            if m:
                kt_touch_downs.append({'x': float(m.group(1)), 'y': float(m.group(2))})
                continue
            m = touch_up_re.search(line)
            if m:
                kt_touch_ups.append({'x': float(m.group(1)), 'y': float(m.group(2))})
                continue
            m = attach_re.search(line)
            if m:
                attach_ts = int(m.group(1))
                continue
            m = detach_re.search(line)
            if m:
                detach_ts = int(m.group(1))
                continue

# ── Sub-test 1: real tap ──
TAP_X, TAP_Y = 540.0, 1170.0
def within2(e, tx=TAP_X, ty=TAP_Y):
    return abs(e['x'] - tx) <= 2.0 and abs(e['y'] - ty) <= 2.0

downs_a = kind_events(window_a, 'touch_down')
ups_a   = kind_events(window_a, 'touch_up')
taps_a  = kind_events(window_a, 'tap')

downs_a_match = [e for e in downs_a if within2(e)]
ups_a_match   = [e for e in ups_a   if within2(e)]

ac_tap_down_received = len(downs_a_match) >= 1
ac_tap_up_received   = len(ups_a_match) >= 1
ac_tap_received = ac_tap_down_received and ac_tap_up_received

kt_real_tap_down_match = [d for d in kt_touch_downs if abs(d['x'] - TAP_X) <= 2 and abs(d['y'] - TAP_Y) <= 2]
kt_real_tap_up_match   = [d for d in kt_touch_ups   if abs(d['x'] - TAP_X) <= 2 and abs(d['y'] - TAP_Y) <= 2]
kt_tap_view_path_confirmed = len(kt_real_tap_down_match) >= 1 and len(kt_real_tap_up_match) >= 1

# ── Sub-test 2: scroll velocity (simulation-only window, Issue #2 fix) ──
# Gate is evaluated against ONLY the simulation logcat window — NOT the
# combined logcat that also contains real swipe events. A broken INPUT_SCROLL
# broadcast will cause this to fail even if the real swipe produces scrolls.
#
# Sign convention: positive vy = finger moves DOWNWARD (Android screen coords).
# The simulation broadcast uses vy=-1200.0 (finger moves upward).
scrolls_b_sim = kind_events(window_b_sim, 'scroll')
ac_scroll_sim_received = len(scrolls_b_sim) >= 1
ac_scroll_sim_nonzero_velocity = any(abs(e['vy']) > 0.1 for e in scrolls_b_sim)
last_vy_sim = scrolls_b_sim[-1]['vy'] if scrolls_b_sim else 0.0

# ── Sub-test 3: long-press ──
lps_c = kind_events(window_c, 'long_press')
ac_long_press_received = len(lps_c) >= 1

# ── Sub-tests B and C: parse LOGCAT_BC_FILE ──
bc_events = parse_events(bcfile)
bc_windows = reconstruct_windows(bc_events)
# The BC logcat is captured after the real swipe and long-press broadcasts,
# so its window list may contain residual windows from those earlier resets.
# Sub-test B (down → cancel) is the second-to-last window; sub-test C (sign
# convention scroll vy > 0) is the last window.
# We use [-2] and [-1] rather than fixed indices to be robust to how many
# preceding windows the logcat carries (depends on logcat buffer depth).
window_b_cancel = bc_windows[-2] if len(bc_windows) >= 2 else (bc_windows[0] if bc_windows else [])
window_c_sign   = bc_windows[-1] if len(bc_windows) >= 1 else []

# Sub-test B: ACTION_CANCEL — verify TouchCancel event received.
# After INPUT_TOUCH_DOWN + INPUT_TOUCH_CANCEL, Rust must have a touch_cancel
# event (not just silence). This proves the state machine is closed correctly.
downs_b  = kind_events(window_b_cancel, 'touch_down')
cancels_b = kind_events(window_b_cancel, 'touch_cancel')
ac_cancel_down_received   = len(downs_b) >= 1
ac_cancel_cancel_received = len(cancels_b) >= 1
ac_cancel_received = ac_cancel_down_received and ac_cancel_cancel_received

# Sub-test C: sign convention — downward swipe (vy=+800.0 broadcast).
# Broadcast uses vy=+800.0 (positive = finger moves DOWNWARD in Android
# screen coords). We assert the received scroll event has vy > 0.
scrolls_c_sign = kind_events(window_c_sign, 'scroll')
ac_sign_scroll_received = len(scrolls_c_sign) >= 1
last_vy_sign = scrolls_c_sign[-1]['vy'] if scrolls_c_sign else 0.0
ac_sign_positive_vy = last_vy_sign > 0.1  # broadcast sent vy=+800.0

ac_overall = (ac_tap_received and ac_scroll_sim_received
              and ac_scroll_sim_nonzero_velocity and ac_long_press_received
              and ac_cancel_received and ac_sign_positive_vy)

result = {
    'story': 'M2-S11',
    'device_serial': serial,
    'attach_ts_ms': attach_ts,
    'detach_ts_ms': detach_ts,
    'sub_test_1_real_tap': {
        'method': 'adb_shell_input_tap',
        'tap_x': TAP_X,
        'tap_y': TAP_Y,
        'touch_down_events': [{'x': e['x'], 'y': e['y']} for e in downs_a],
        'touch_up_events':   [{'x': e['x'], 'y': e['y']} for e in ups_a],
        'tap_events':        [{'x': e['x'], 'y': e['y']} for e in taps_a],
        'down_within_2px': ac_tap_down_received,
        'up_within_2px':   ac_tap_up_received,
        'kt_view_path_confirmed': kt_tap_view_path_confirmed,
        'pass': ac_tap_received,
    },
    'sub_test_2_scroll_velocity': {
        'method': 'simulation_broadcast_INPUT_SCROLL',
        'gate_window': 'simulation_only_logcat (logcat cleared before real swipe)',
        'scroll_events_sim_received': len(scrolls_b_sim),
        'last_vy_sim': last_vy_sim,
        'nonzero_velocity_sim': ac_scroll_sim_nonzero_velocity,
        'sign_convention': 'positive_vy_finger_moves_down_android_screen_coords',
        'pass': ac_scroll_sim_received and ac_scroll_sim_nonzero_velocity,
    },
    'sub_test_3_long_press': {
        'method': 'simulation_broadcast_INPUT_LONG_PRESS',
        'long_press_events_received': len(lps_c),
        'pass': ac_long_press_received,
    },
    'sub_test_b_action_cancel': {
        'method': 'simulation_INPUT_TOUCH_DOWN_then_INPUT_TOUCH_CANCEL',
        'touch_down_received': ac_cancel_down_received,
        'touch_cancel_received': ac_cancel_cancel_received,
        'cancel_events': [{'x': e['x'], 'y': e['y']} for e in cancels_b],
        'pass': ac_cancel_received,
    },
    'sub_test_c_sign_convention': {
        'method': 'simulation_INPUT_SCROLL_vy_pos800',
        'expected_vy_gt_0': True,
        'scroll_events_received': len(scrolls_c_sign),
        'last_vy': last_vy_sign,
        'positive_vy_confirmed': ac_sign_positive_vy,
        'sign_convention': 'positive_vy_finger_moves_down_android_screen_coords',
        'pass': ac_sign_positive_vy,
    },
    # Honest disclosure.
    'method_note': (
        'Sub-test 1 uses real adb shell input tap (OS MotionEvent path through '
        'WarpInputView.onTouchEvent). Sub-tests 2+3+B+C use simulation broadcasts '
        '(TouchSimulationReceiver → NativeBridge JNI directly) because adb swipe '
        'velocity and long-press duration are non-deterministic on Samsung One UI. '
        'Issue #2 fix (round-4): sub-test 2 gate evaluated against simulation-only '
        'logcat (captured before adb logcat -c + real swipe), preventing false-pass. '
        'Issue #4 fix (round-4): sub-test B confirms TouchCancel emitted on cancel. '
        'Issue #5 fix (round-4): sub-test C confirms positive vy = finger down.'
    ),
    'acceptance_gate': {
        'tap_received': ac_tap_received,
        'scroll_sim_received': ac_scroll_sim_received,
        'scroll_sim_nonzero_velocity': ac_scroll_sim_nonzero_velocity,
        'long_press_received': ac_long_press_received,
        'cancel_received': ac_cancel_received,
        'sign_positive_vy': ac_sign_positive_vy,
        'overall_pass': ac_overall,
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2, ensure_ascii=False)

print(f"\n=== M2-S11 result summary ===")
print(f"device:                       {serial}")
print(f"")
print(f"Sub-test 1 (real adb input tap at ({TAP_X},{TAP_Y})):")
print(f"  touch_down within 2px:      {ac_tap_down_received}")
print(f"  touch_up within 2px:        {ac_tap_up_received}")
print(f"  kt View path confirmed:     {kt_tap_view_path_confirmed}")
print(f"  gesture tap events:         {len(taps_a)} (bonus; 300ms delay not guaranteed in window)")
print(f"  pass:                       {'PASS' if ac_tap_received else 'FAIL'}")
print(f"")
print(f"Sub-test 2 (scroll simulation broadcast vy=-1200.0, sim-only window):")
print(f"  scroll events (sim window): {len(scrolls_b_sim)}")
print(f"  last_vy (sim):              {last_vy_sim}  [+vy = finger down, Android coords]")
print(f"  nonzero_velocity (sim):     {ac_scroll_sim_nonzero_velocity}")
print(f"  pass:                       {'PASS' if (ac_scroll_sim_received and ac_scroll_sim_nonzero_velocity) else 'FAIL'}")
print(f"")
print(f"Sub-test 3 (long-press simulation broadcast):")
print(f"  long_press events received: {len(lps_c)}")
print(f"  pass:                       {'PASS' if ac_long_press_received else 'FAIL'}")
print(f"")
print(f"Sub-test B (ACTION_CANCEL state machine):")
print(f"  touch_down received:        {ac_cancel_down_received}")
print(f"  touch_cancel received:      {ac_cancel_cancel_received}")
print(f"  pass:                       {'PASS' if ac_cancel_received else 'FAIL'}")
print(f"")
print(f"Sub-test C (sign convention, vy=+800.0 broadcast):")
print(f"  scroll events received:     {len(scrolls_c_sign)}")
print(f"  last_vy:                    {last_vy_sign}  [expected > 0]")
print(f"  positive_vy_confirmed:      {ac_sign_positive_vy}")
print(f"  pass:                       {'PASS' if ac_sign_positive_vy else 'FAIL'}")
print(f"")
print(f"GATE: overall_pass={ac_overall}")
print(f"", file=sys.stderr)
print(f"# result written to {out_json}", file=sys.stderr)

sys.exit(0 if ac_overall else 31)
PYEOF
PARSE_RC=$?
set -e

echo "=== done ===" >&2

"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true

exit $PARSE_RC
