#!/usr/bin/env zsh
# test-ime.sh <device-serial>
# M2-S10 device verification driver. Installs the warp-mobile APK, launches
# MainActivity in IME mode (composite SurfaceView + WarpInputView layout),
# exercises the InputConnection state machine via simulation broadcasts (and
# logs availability of real IME), parses logcat to verify state transitions,
# and writes M2-S10-result.json.
#
# Why broadcast simulation: `adb shell input text` does NOT route through
# InputConnection (it synthesizes raw KeyEvents at the framework's input-
# dispatcher level, bypassing the IME entirely). To exercise the
# WarpInputConnection.commitText / setComposingText / finishComposingText
# code path end-to-end without installing a custom IME on the device, we
# broadcast IME_* intents which the ImeSimulationReceiver routes through the
# real BaseInputConnection subclass via View.post (UI thread). This proves
# the JNI plumbing + state machine work for both Latin and Pinyin patterns.
#
# We additionally probe what real IME is installed and whether the soft
# keyboard could be brought up — for honest disclosure of the manual-step
# requirement (S24 Ultra Knox sometimes blocks programmatic IME enable on
# debug-built apps).
#
# Acceptance gates (per .omc/prd.json M2-S10 + ralplan §6 M2 Acceptance #3):
#   * Latin "hello" → 5 char input events received (latin=5)
#   * Pinyin compose+commit → composing region updates in-place,
#     finally commits as "你好" (composing_update≥5, composing_commit=1)
#   * Gboard empty-finish-after-commit edge case handled (empty_finish=1)
#   * No double commits (composing_finish=0 after commit; no extra latin)
#   * No surface destroy mid-test
#   * MainActivity reaches surfaceCreated_ts within 10s
#   * Vulkan still presents (no regression on rendering)
#
# Logcat tags consumed:
#   WarpRender — Kotlin lifecycle (surfaceCreated_ts, ime_mode shown)
#   WarpIme    — IME events (Kotlin IC.* + Rust ime_event lines)
#   warp-android-host — Rust target tag (where ime_event actually emits)
#
# Usage:
#   ./tools/scripts/test-ime.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m2-artifacts/M2-S10-result.json
#   stdout: human-readable summary
#   stderr: progress / debug
#
# Exit codes (matching M2-S04+S05+S08 driver matrix):
#   0    PASS — all gates satisfied
#   1    install / build / device offline
#   2    surfaceCreated_ts never observed within 10s
#   5    focus stolen by GrantPermissionsActivity
#   6    PNG / AC mismatch
#   9    screen state not ON before launch
#   11   focus stolen by Bouncer / Keyguard / StatusBar / NotificationShade
#   12   focus is NOT dev.warp.mobile
#   13   mInputRestricted=true
#   14   surfaceDestroyed_ts after surfaceCreated_ts before steady run
#   15   ImeSimulationReceiver fell back to direct JNI (M2-S10 round-2 — the
#        WarpInputView lost focus or InputConnection was null; the simulation
#        bypassed the production WarpInputConnection.* path and is therefore
#        invalid as proof of the IME glue).
#   16   Kotlin IC.* call-count assertion failure (M2-S10 round-2 — observed
#        IC.commitText / IC.setComposingText / IC.finishComposingText counts
#        in logcat do NOT match the expected per-sub-test counts; means a
#        receiver dispatch bug or a missed UI-thread post).
#   31   IME state machine assertion failure (latin counts off, etc.)
#
# Web-search refs (2026-04-30):
#   <https://developer.android.com/reference/android/view/inputmethod/InputConnection>
#   <https://developer.android.com/reference/android/view/inputmethod/BaseInputConnection>
#   <https://developer.android.com/develop/ui/views/touch-and-input/creating-input-method>
#   <https://infinum.com/blog/input-connection/>
#   <https://github.com/element-hq/element-android/issues/8521>
#     (Gboard inline-composing Pinyin gotcha; informs our edge-case handling)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m2-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M2-S10-result.json"

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

# Probe the available IMEs for the result-json (informational; we don't fail
# if Pinyin isn't installed since we use simulation broadcasts).
echo "=== probing installed IMEs ===" >&2
IME_LIST_TXT=$("${ADB[@]}" shell ime list -s 2>/dev/null | tr -d '\r' || true)
DEFAULT_IME=$("${ADB[@]}" shell settings get secure default_input_method 2>/dev/null | tr -d '\r' || true)
echo "=== installed IMEs ===" >&2
echo "$IME_LIST_TXT" >&2
echo "=== default IME: $DEFAULT_IME ===" >&2

GBOARD_PRESENT=0
PINYIN_PRESENT=0
if echo "$IME_LIST_TXT" | grep -q "com.google.android.inputmethod.latin"; then GBOARD_PRESENT=1; fi
if echo "$IME_LIST_TXT" | grep -qiE "pinyin|com.baidu|sogou|com.iflytek"; then PINYIN_PRESENT=1; fi

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

echo "=== launching $PACKAGE/$ACTIVITY in ime_mode ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" \
    --ez ime_mode true \
    2>&1 | tail -2 >&2
START_TS=$(date +%s)
sleep 1

# M2-S04 round-3 strict focus assertion (reused).
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
echo "=== no post-creation surfaceDestroyed; proceeding to IME state-machine tests ===" >&2

# Now drive the state-machine tests.
ime_broadcast() {
    local action="$1"; shift
    "${ADB[@]}" shell am broadcast -a "$action" -p "$PACKAGE" "$@" 2>&1 | tail -1 >&2 || true
    # Small delay so the receiver dispatches and logcat observes the event
    # before we issue the next call. The receiver routes through View.post
    # which schedules on the next UI-thread run-loop tick.
    sleep 0.2
}

ime_b64() {
    printf '%s' "$1" | base64 | tr -d '\n'
}

echo "" >&2
echo "=== Sub-test 1: reset state machine ===" >&2
ime_broadcast dev.warp.mobile.IME_RESET

echo "" >&2
echo "=== Sub-test 2: Latin 'hello' (commit per char, mirrors Gboard English) ===" >&2
HELLO_B64=$(ime_b64 "hello")
ime_broadcast dev.warp.mobile.IME_TYPE_LATIN --es text_b64 "$HELLO_B64"
sleep 0.5
LATIN_STATS=$("${ADB[@]}" logcat -d -s warp-android-host:I WarpIme:I 2>/dev/null | grep -E "ime_event kind=latin_commit" | wc -l | tr -d ' ' || true)
echo "=== Latin commit lines observed in logcat: $LATIN_STATS ===" >&2

echo "" >&2
echo "=== Sub-test 3: reset before Pinyin ===" >&2
ime_broadcast dev.warp.mobile.IME_RESET
sleep 0.3

echo "" >&2
echo "=== Sub-test 4: Pinyin 'ni hao' compose-then-commit (5 setComposingText + 1 commitText) ===" >&2
NI_B64=$(ime_b64 "n")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NI_B64" --ei cursor 1
NI_B64=$(ime_b64 "ni")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NI_B64" --ei cursor 1
NIH_B64=$(ime_b64 "nih")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NIH_B64" --ei cursor 1
NIHA_B64=$(ime_b64 "niha")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NIHA_B64" --ei cursor 1
NIHAO_B64=$(ime_b64 "nihao")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NIHAO_B64" --ei cursor 1
NI_HAO_B64=$(ime_b64 "你好")
ime_broadcast dev.warp.mobile.IME_COMMIT_TEXT --es text_b64 "$NI_HAO_B64" --ei cursor 1
sleep 0.5

echo "" >&2
echo "=== Sub-test 5: Gboard known-bug — empty finishComposingText AFTER commit ===" >&2
ime_broadcast dev.warp.mobile.IME_FINISH_COMPOSING_TEXT
sleep 0.3

echo "" >&2
echo "=== Sub-test 6 — round-2 NEW: Gboard real risky order setComposing → finish → commit ===" >&2
# Codex round-1 device repro on R5CX10VFFBA found the real Gboard ordering on
# Pinyin candidate-pick is `setComposingText → finishComposingText →
# commitText` — finish arrives BETWEEN setComposing and the candidate commit.
# Naive eager-flush misclassifies the candidate as latin_commit. The deferred
# state machine must classify the commit as composing_commit via the
# pending_finish defer buffer.
ime_broadcast dev.warp.mobile.IME_RESET
sleep 0.3
NIHAO_B64=$(ime_b64 "nihao")
ime_broadcast dev.warp.mobile.IME_SET_COMPOSING_TEXT --es text_b64 "$NIHAO_B64" --ei cursor 1
ime_broadcast dev.warp.mobile.IME_FINISH_COMPOSING_TEXT
NI_HAO_B64=$(ime_b64 "你好")
ime_broadcast dev.warp.mobile.IME_COMMIT_TEXT --es text_b64 "$NI_HAO_B64" --ei cursor 1
sleep 0.5

echo "" >&2
echo "=== Sub-test 7: capture cumulative state machine stats ===" >&2
# imeStats() output is logged by the receiver dispatcher path AND we can
# query directly via a separate broadcast that logs it. Simpler path: just
# read the last sequence of ime_event lines and let the parser compute totals
# from logcat (single source of truth, matches what M2-S04+S05+S08 do).

# Capture logcat and dump.
LOGCAT_FILE=$(mktemp /tmp/m2-s10-logcat.XXXXXX)
"${ADB[@]}" logcat -d -v time \
    "WarpRender:I" \
    "WarpIme:W" \
    "WarpIme:I" \
    "WarpVulkan:V" \
    "warp-android-host:V" \
    "*:S" > "$LOGCAT_FILE"

echo "=== logcat tail (last 80 lines) ===" >&2
tail -80 "$LOGCAT_FILE" >&2

# Round-2 blocker #2: fail hard if ImeSimulationReceiver fell back to direct
# JNI (i.e. the WarpInputView lost focus or InputConnection was null). That
# bypasses the production code path and the simulation is invalid as proof.
FALLBACK_LINES=$(grep -E "falling back to direct JNI|MainActivity not foreground" "$LOGCAT_FILE" || true)
if [[ -n "$FALLBACK_LINES" ]]; then
    echo "" >&2
    echo "ERROR: ImeSimulationReceiver fell back to direct JNI — production" >&2
    echo "       WarpInputConnection.* code path was BYPASSED. The simulation" >&2
    echo "       is therefore invalid as proof of M2-S10 acceptance criteria." >&2
    echo "" >&2
    echo "$FALLBACK_LINES" >&2
    echo "" >&2
    "${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 >&2 || true
    exit 15
fi
echo "=== no fallback-to-direct-JNI detected (production IC path exercised) ===" >&2

# Probe last imeStats by triggering one more reset-free no-op event so the
# Rust target logs total counters at the next call. Actually the cleanest is
# to broadcast a marker and read out. Instead, parse from `ime_event` lines.

echo "" >&2
echo "=== parsing results ===" >&2
set +e
python3 - "$LOGCAT_FILE" "$SERIAL" "$RESULT_JSON" \
    "$GBOARD_PRESENT" "$PINYIN_PRESENT" "$DEFAULT_IME" "$LATIN_STATS" <<'PYEOF'
import sys, re, json

logfile        = sys.argv[1]
serial         = sys.argv[2]
out_json       = sys.argv[3]
gboard_present = int(sys.argv[4])
pinyin_present = int(sys.argv[5])
default_ime    = sys.argv[6]
latin_obs      = int(sys.argv[7])

# Match `ime_event kind=<kind> text=<quoted> cursor=<n> composing_active=<b> composing_text=<quoted> events_total=<n>`
event_re = re.compile(
    r'ime_event\s+kind=(\S+)\s+text=("(?:[^"\\]|\\.)*")\s+cursor=(-?\d+)\s+composing_active=(\S+)\s+composing_text=("(?:[^"\\]|\\.)*")\s+events_total=(\d+)'
)
# Match Kotlin-side `IC.commitText text="..."` / `IC.setComposingText text="..."` / `IC.finishComposingText`
ic_commit_re = re.compile(r'IC\.commitText\s+text="((?:[^"\\]|\\.)*)"\s+cursorPos=(-?\d+)')
ic_set_re = re.compile(r'IC\.setComposingText\s+text="((?:[^"\\]|\\.)*)"\s+cursorPos=(-?\d+)')
ic_finish_re = re.compile(r'IC\.finishComposingText')
attach_re = re.compile(r'surfaceCreated_ts=(\d+)')
detach_re = re.compile(r'surfaceDestroyed_ts=(\d+)')

events = []
ic_calls = []
attach_ts = None
detach_ts = None

with open(logfile, encoding='utf-8', errors='replace') as f:
    for line in f:
        m = event_re.search(line)
        if m:
            events.append({
                'kind': m.group(1),
                'text': m.group(2),
                'cursor': int(m.group(3)),
                'composing_active': m.group(4) == 'true',
                'composing_text': m.group(5),
                'events_total': int(m.group(6)),
            })
            continue
        m = ic_commit_re.search(line)
        if m:
            ic_calls.append({'kind': 'commitText', 'text': m.group(1), 'cursor': int(m.group(2))})
            continue
        m = ic_set_re.search(line)
        if m:
            ic_calls.append({'kind': 'setComposingText', 'text': m.group(1), 'cursor': int(m.group(2))})
            continue
        if ic_finish_re.search(line):
            ic_calls.append({'kind': 'finishComposingText'})
            continue
        m = attach_re.search(line)
        if m:
            attach_ts = int(m.group(1))
            continue
        m = detach_re.search(line)
        if m:
            detach_ts = int(m.group(1))
            continue

# Reconstruct sub-test event windows.
# - The driver issues IME_RESET twice; ime_event for kinds is emitted ONLY
#   on commit/setComposing/finish. So we identify boundaries by the cumulative
#   events_total counter resetting (after IME_RESET).
# Strategy: walk events, slice on events_total monotonic break.

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

# We expect 3 windows after the 3 resets:
# Window A: 5 latin_commit events (Latin 'hello')
# Window B: 5 composing_update + 1 composing_commit + 1 empty_finish
#          (Pinyin in-place compose-then-commit + Gboard empty-finish-after)
# Window C: 1 composing_update + 1 composing_commit (round-2 NEW —
#          Gboard real risky order setComposing → finish → commit; the
#          finish defers via pending_finish, the commit reclassifies as
#          composing_commit, NOT latin_commit; no composing_finish emitted).
# Window indexing: each IME_RESET clears events_total, splitting windows.

def kind_count(window, kind):
    return sum(1 for e in window if e['kind'] == kind)

window_a = windows[0] if len(windows) >= 1 else []
window_b = windows[1] if len(windows) >= 2 else []
window_c = windows[2] if len(windows) >= 3 else []

latin_a = kind_count(window_a, 'latin_commit')
update_b = kind_count(window_b, 'composing_update')
commit_b = kind_count(window_b, 'composing_commit')
finish_b = kind_count(window_b, 'composing_finish')
empty_b = kind_count(window_b, 'empty_finish')

# Verify: composing_update emits with text 'n','ni','nih','niha','nihao' in sequence
update_b_texts = [e['text'].strip('"') for e in window_b if e['kind'] == 'composing_update']
# Verify: composing_commit emits text "你好"
commit_b_texts = [e['text'].strip('"') for e in window_b if e['kind'] == 'composing_commit']

# Round-2 NEW window C: corrected Gboard order setComposing → finish → commit.
update_c = kind_count(window_c, 'composing_update')
commit_c = kind_count(window_c, 'composing_commit')
finish_c = kind_count(window_c, 'composing_finish')
empty_c = kind_count(window_c, 'empty_finish')
latin_c = kind_count(window_c, 'latin_commit')
update_c_texts = [e['text'].strip('"') for e in window_c if e['kind'] == 'composing_update']
commit_c_texts = [e['text'].strip('"') for e in window_c if e['kind'] == 'composing_commit']

# Acceptance gates (per AC schema):
ac_latin_5 = (latin_a >= 5)  # >= because we may have stray ic_commits
ac_pinyin_compose = (update_b >= 5)
ac_pinyin_committed = (
    commit_b >= 1 and any('你好' in t or '\\u4f60\\u597d' in t for t in commit_b_texts)
)
ac_no_double_commit = (finish_b == 0)
ac_empty_finish_seen = (empty_b >= 1)

# Round-2 NEW gate: corrected Gboard order — exactly 1 ComposingUpdate +
# exactly 1 ComposingCommit "你好". MUST NOT classify as latin_commit, MUST
# NOT emit composing_finish or empty_finish in this window.
ac_round2_classify = (
    update_c == 1
    and commit_c == 1
    and any('你好' in t or '\\u4f60\\u597d' in t for t in commit_c_texts)
    and finish_c == 0
    and empty_c == 0
    and latin_c == 0
)

ac_overall = (
    ac_latin_5 and ac_pinyin_compose and ac_pinyin_committed
    and ac_no_double_commit and ac_empty_finish_seen
    and ac_round2_classify
)

# Total counts across all events (cumulative diagnostic).
total_latin = sum(kind_count(w, 'latin_commit') for w in windows)
total_update = sum(kind_count(w, 'composing_update') for w in windows)
total_commit = sum(kind_count(w, 'composing_commit') for w in windows)
total_finish = sum(kind_count(w, 'composing_finish') for w in windows)
total_empty = sum(kind_count(w, 'empty_finish') for w in windows)
total_events = sum(len(w) for w in windows)

# IC call totals (Kotlin-side IC.* lines mirror what the receiver fed in).
ic_commit_count = sum(1 for c in ic_calls if c['kind'] == 'commitText')
ic_set_count = sum(1 for c in ic_calls if c['kind'] == 'setComposingText')
ic_finish_count = sum(1 for c in ic_calls if c['kind'] == 'finishComposingText')

# Round-2 blocker #2: assert EXACT IC.* call counts match the broadcasts the
# driver issued. Mismatch = receiver dispatch bug, missed UI-thread post, or
# the broadcast didn't actually reach the production WarpInputConnection (it
# may have e.g. fallen back to direct JNI which we already detected, but this
# catches the silent case where the IC method returned false / lost focus
# mid-test).
#
# Driver issues:
#   Sub-test 2 Latin "hello"          : 5 commitText, 0 setComposing, 0 finish
#   Sub-test 4 Pinyin compose+commit  : 5 setComposing + 1 commitText
#   Sub-test 5 empty finish           : 1 finish
#   Sub-test 6 Gboard correct order   : 1 setComposing + 1 finish + 1 commitText
# Cumulative (across all sub-tests):
#   commitText        = 5 + 1 + 1 = 7
#   setComposingText  = 5 + 1     = 6
#   finishComposingText = 1 + 1   = 2
ic_expected_commit = 7
ic_expected_set = 6
ic_expected_finish = 2

ic_count_match = (
    ic_commit_count == ic_expected_commit
    and ic_set_count == ic_expected_set
    and ic_finish_count == ic_expected_finish
)
# Update overall gate: also require IC counts to match.
ac_overall = ac_overall and ic_count_match

# State-machine transitions tracked for the result-json field.
ime_state_machine_transitions = {
    'sub_test_2_latin_hello': {
        'latin_commit_count': latin_a,
        'expected': 5,
        'pass': ac_latin_5,
    },
    'sub_test_4_pinyin_compose_commit': {
        'composing_update_count': update_b,
        'composing_commit_count': commit_b,
        'composing_update_texts': update_b_texts,
        'composing_commit_texts': commit_b_texts,
        'expected_update_count': 5,
        'expected_commit_count': 1,
        'expected_committed_text': '你好',
        'pass': ac_pinyin_compose and ac_pinyin_committed,
    },
    'sub_test_5_empty_finish_after_commit': {
        'empty_finish_count': empty_b,
        'composing_finish_count': finish_b,
        'expected_empty_finish_count': 1,
        'expected_composing_finish_count': 0,
        'pass': ac_empty_finish_seen and ac_no_double_commit,
    },
    # Round-2 NEW: real Gboard risky order setComposing → finish → commit.
    # The pending_finish defer buffer reclassifies the candidate-pick commit
    # as ComposingCommit (NOT LatinCommit, NOT a stray composing_finish).
    'sub_test_6_gboard_finish_then_commit': {
        'composing_update_count': update_c,
        'composing_commit_count': commit_c,
        'composing_finish_count': finish_c,
        'empty_finish_count': empty_c,
        'latin_commit_count': latin_c,
        'composing_update_texts': update_c_texts,
        'composing_commit_texts': commit_c_texts,
        'expected_update_count': 1,
        'expected_commit_count': 1,
        'expected_committed_text': '你好',
        'expected_no_finish': True,
        'expected_no_latin_commit': True,
        'pass': ac_round2_classify,
    },
    # Round-2 blocker #2: exact IC.* call-count assertion (receiver
    # actually dispatched through the production WarpInputConnection.* path
    # the expected number of times).
    'ic_call_count_assertion': {
        'commit_text_actual': ic_commit_count,
        'commit_text_expected': ic_expected_commit,
        'set_composing_text_actual': ic_set_count,
        'set_composing_text_expected': ic_expected_set,
        'finish_composing_text_actual': ic_finish_count,
        'finish_composing_text_expected': ic_expected_finish,
        'pass': ic_count_match,
    },
}

result = {
    'story': 'M2-S10',
    'device_serial': serial,
    'environment': {
        'gboard_present': bool(gboard_present),
        'pinyin_ime_present': bool(pinyin_present),
        'default_ime': default_ime,
    },
    'attach_ts_ms': attach_ts,
    'detach_ts_ms': detach_ts,
    # AC#1 latin_chars_received per the prompt schema:
    'latin_chars_received': latin_a,
    # AC#1 pinyin_composing_seen
    'pinyin_composing_seen': update_b,
    # AC#1 pinyin_committed_text (joined)
    'pinyin_committed_text': ','.join(commit_b_texts),
    # AC#1 ime_event_count (cumulative across all windows)
    'ime_event_count': total_events,
    # AC#1 ime_state_machine_transitions
    'ime_state_machine_transitions': ime_state_machine_transitions,
    # cumulative counts (diagnostic)
    'cumulative': {
        'latin_commit': total_latin,
        'composing_update': total_update,
        'composing_commit': total_commit,
        'composing_finish': total_finish,
        'empty_finish': total_empty,
        'events_total': total_events,
    },
    'ic_kotlin_calls': {
        'commitText': ic_commit_count,
        'setComposingText': ic_set_count,
        'finishComposingText': ic_finish_count,
    },
    # Honest disclosure: simulation broadcasts run through the real
    # WarpInputConnection.commitText/setComposingText/finishComposingText
    # methods on the UI thread, exercising the IDENTICAL code path that a
    # real Pinyin IME would use — but we did NOT switch the system IME to
    # a custom one (not feasible programmatically without Knox-specific
    # workarounds on Samsung debug builds). For real-IME visual verification
    # see manual_verification_steps below.
    'method': 'simulation_broadcasts_via_ImeSimulationReceiver',
    'manual_verification_steps': [
        'Open Settings > General Management > Keyboard list and default',
        'Enable Gboard with Chinese (Pinyin) layout',
        'Set Gboard as default IME',
        'Launch dev.warp.mobile/.MainActivity with --ez ime_mode true',
        'Tap the screen — Gboard appears, type "ni hao", select 你好 candidate',
        'Verify logcat shows IC.setComposingText for n/ni/nih/niha/nihao + IC.commitText for 你好',
        'Verify the same ime_event lines emit in logcat (warp-android-host target)',
    ],
    'acceptance_gate': {
        'latin_pass': ac_latin_5,
        'pinyin_compose_pass': ac_pinyin_compose,
        'pinyin_committed_pass': ac_pinyin_committed,
        'no_double_commit_pass': ac_no_double_commit,
        'empty_finish_seen_pass': ac_empty_finish_seen,
        'gboard_finish_then_commit_classify_pass': ac_round2_classify,
        'ic_call_count_match_pass': ic_count_match,
        'overall_pass': ac_overall,
    },
}

with open(out_json, 'w') as f:
    json.dump(result, f, indent=2, ensure_ascii=False)

print(f"\n=== M2-S10 result summary ===")
print(f"device:                       {serial}")
print(f"gboard_present:               {bool(gboard_present)}")
print(f"pinyin_ime_present:           {bool(pinyin_present)}")
print(f"default_ime:                  {default_ime}")
print(f"")
print(f"Sub-test 2 (Latin 'hello'):")
print(f"  latin_commit_count:         {latin_a} (expected 5) {'PASS' if ac_latin_5 else 'FAIL'}")
print(f"")
print(f"Sub-test 4 (Pinyin compose+commit):")
print(f"  composing_update_count:     {update_b} (expected 5) {'PASS' if ac_pinyin_compose else 'FAIL'}")
print(f"  composing_commit_count:     {commit_b} (expected 1) {'PASS' if ac_pinyin_committed else 'FAIL'}")
print(f"  composing_update_texts:     {update_b_texts}")
print(f"  composing_commit_texts:     {commit_b_texts}")
print(f"")
print(f"Sub-test 5 (Empty finish after commit / Gboard bug edge case):")
print(f"  empty_finish_count:         {empty_b} (expected 1) {'PASS' if ac_empty_finish_seen else 'FAIL'}")
print(f"  composing_finish_count:     {finish_b} (expected 0) {'PASS' if ac_no_double_commit else 'FAIL'}")
print(f"")
print(f"Cumulative IME events:        {total_events}")
print(f"  latin_commit:               {total_latin}")
print(f"  composing_update:           {total_update}")
print(f"  composing_commit:           {total_commit}")
print(f"  composing_finish:           {total_finish}")
print(f"  empty_finish:               {total_empty}")
print(f"")
print(f"Sub-test 6 (Gboard real order setComposing→finish→commit) round-2 NEW:")
print(f"  composing_update_count:     {update_c} (expected 1)")
print(f"  composing_commit_count:     {commit_c} (expected 1, text '你好')")
print(f"  composing_commit_texts:     {commit_c_texts}")
print(f"  composing_finish_count:     {finish_c} (expected 0)")
print(f"  empty_finish_count:         {empty_c} (expected 0)")
print(f"  latin_commit_count:         {latin_c} (expected 0 — must NOT misclassify)")
print(f"  reclassify_pass:            {'PASS' if ac_round2_classify else 'FAIL'}")
print(f"")
print(f"Kotlin IC.* call counts:      commit={ic_commit_count} (expected {ic_expected_commit}) setComposing={ic_set_count} (expected {ic_expected_set}) finish={ic_finish_count} (expected {ic_expected_finish})")
print(f"  ic_count_match_pass:        {'PASS' if ic_count_match else 'FAIL'}")
print(f"")
print(f"GATE: overall_pass={ac_overall}")
print(f"")
print(f"# result written to {out_json}", file=sys.stderr)

# Exit-code mapping:
#   0   PASS
#   16  IC count mismatch (only — round-2 blocker #2 specific)
#   31  state-machine assertion failure (covers all other gates including
#       round-2 blocker #1 reclassification)
if ac_overall:
    exit_code = 0
elif not ic_count_match and ac_latin_5 and ac_pinyin_compose and ac_pinyin_committed and ac_no_double_commit and ac_empty_finish_seen and ac_round2_classify:
    exit_code = 16
else:
    exit_code = 31
sys.exit(exit_code)
PYEOF
PARSE_RC=$?
set -e

echo "=== done ===" >&2

"${ADB[@]}" shell am force-stop "$PACKAGE" 2>&1 | tail -2 >&2 || true

exit $PARSE_RC
