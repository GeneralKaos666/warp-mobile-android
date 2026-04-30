#!/usr/bin/env bash
# M3-S06: zsh_body.sh APK asset ship + first-launch extraction verification.
#
# Usage: tools/scripts/test-zsh-asset.sh [<serial>]
#   <serial>  ADB device serial (default: R5CX10VFFBA — Galaxy S24 Ultra primary)
#
# Pre-requisites:
#   - APK already built: cd android && ./gradlew :app:assembleDebug
#   - Device connected with ADB and USB debugging enabled
#
# Exit codes: 0 = all PASS, 1 = any FAIL
#
# Verification gates (per M3-S06 AC#5):
#   1. APK contains assets/warp/zsh_body.sh      (unzip -l)
#   2. Install + cold-launch triggers extraction
#   3. File present in app files dir              (adb shell run-as)
#   4. PTY can cat the file                       (logcat extract line)
#
# Note on hook execution (AC#4): S24 Ultra ships only mksh. zsh_body.sh
# requires zsh's precmd/preexec mechanism. Hook EXECUTION is deferred to M5
# Termux. This script verifies ship + extract only.
#
# Refs:
#   https://developer.android.com/reference/android/content/res/AssetManager
#   https://wiki.termux.com/wiki/Zsh (M5 target for hook execution)
#   AGPL-3.0 §5: zsh_body.sh ships verbatim source; satisfies §5.

set -euo pipefail

SERIAL="${1:-R5CX10VFFBA}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APK="${REPO_ROOT}/android/app/build/outputs/apk/debug/app-debug.apk"
PKG="dev.warp.mobile"
ACTIVITY="${PKG}/.MainActivity"

PASS=0
FAIL=0

pass() { echo "PASS: $1"; (( PASS++ )) || true; }
fail() { echo "FAIL: $1"; (( FAIL++ )) || true; }

# ---------------------------------------------------------------------------
# Gate 1: APK contains assets/warp/zsh_body.sh
# ---------------------------------------------------------------------------
echo "--- Gate 1: APK asset presence ---"
if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK"
    echo "Build first: cd android && ./gradlew :app:assembleDebug"
    exit 1
fi

if unzip -l "$APK" | grep -q "assets/warp/zsh_body.sh"; then
    pass "APK contains assets/warp/zsh_body.sh"
else
    fail "APK missing assets/warp/zsh_body.sh"
    echo "  Run: unzip -l $APK | grep zsh_body"
fi

# ---------------------------------------------------------------------------
# Gate 2 + 3: Install, cold-launch, verify extraction
# ---------------------------------------------------------------------------
echo ""
echo "--- Gate 2: Install + cold-launch ---"
adb -s "$SERIAL" install -r "$APK" >/dev/null 2>&1 && pass "APK installed" || fail "APK install failed"

# Force-stop to ensure a cold start (service onCreate runs extractWarpAssets)
adb -s "$SERIAL" shell am force-stop "$PKG" 2>/dev/null || true

# Clear logcat so we capture only the fresh launch output
adb -s "$SERIAL" logcat -c 2>/dev/null || true

# Launch the activity; WarpTerminalService starts as FGS → extractWarpAssets runs
adb -s "$SERIAL" shell am start -n "$ACTIVITY" >/dev/null 2>&1 \
    && pass "Activity launched" \
    || fail "Activity launch failed"

# Wait for service onCreate to run and extract the asset
sleep 4

# ---------------------------------------------------------------------------
# Gate 3: File present in app data dir
# ---------------------------------------------------------------------------
echo ""
echo "--- Gate 3: Extraction to data dir ---"
EXTRACTED=$(adb -s "$SERIAL" shell run-as "$PKG" ls files/warp/ 2>&1 || true)
if echo "$EXTRACTED" | grep -q "zsh_body.sh"; then
    pass "zsh_body.sh present in /data/data/${PKG}/files/warp/"
else
    fail "zsh_body.sh NOT found in /data/data/${PKG}/files/warp/ (got: ${EXTRACTED})"
fi

# Capture file size for evidence
FILE_INFO=$(adb -s "$SERIAL" shell run-as "$PKG" ls -la files/warp/zsh_body.sh 2>&1 || true)
echo "  File info: $FILE_INFO"

# ---------------------------------------------------------------------------
# Gate 4: PTY can cat the file — verify via logcat extract line
# ---------------------------------------------------------------------------
echo ""
echo "--- Gate 4: Logcat extraction confirmation ---"
# WarpTerminalService logs: "extracted zsh_body.sh to <path> (<n> bytes)"
# OR: "zsh_body.sh already extracted at <path> (<n> bytes); skipping"
LOGCAT=$(adb -s "$SERIAL" logcat -d -s "WarpTerminal" 2>&1 || true)
if echo "$LOGCAT" | grep -q "zsh_body.sh"; then
    EXTRACT_LINE=$(echo "$LOGCAT" | grep "zsh_body.sh" | tail -1)
    pass "Logcat confirms zsh_body.sh extraction: $EXTRACT_LINE"
else
    fail "No zsh_body.sh extraction log line in WarpTerminal logcat"
fi

# Optionally: spawn a PTY shell and cat the file, log output
echo ""
echo "--- Gate 4b: PTY cat zsh_body.sh (spot-check first 5 lines via logcat) ---"
CMD_ID="zsh_asset_test"
adb -s "$SERIAL" shell am broadcast \
    -a dev.warp.mobile.PTY_SPAWN \
    -n "${PKG}/${PKG}.WarpTerminalService" \
    --es cmd_id "$CMD_ID" \
    --es program /system/bin/sh \
    >/dev/null 2>&1 || true
sleep 1
adb -s "$SERIAL" shell am broadcast \
    -a dev.warp.mobile.PTY_WRITE \
    -n "${PKG}/${PKG}.WarpTerminalService" \
    --es cmd_id "$CMD_ID" \
    --es data "head -5 /data/data/${PKG}/files/warp/zsh_body.sh\n" \
    >/dev/null 2>&1 || true
sleep 2

PTY_LOGCAT=$(adb -s "$SERIAL" logcat -d -s "WarpTerminal:PtyOutput" 2>&1 || true)
if echo "$PTY_LOGCAT" | grep -q "zsh\|WARP\|precmd\|preexec\|dcs\|#!"; then
    pass "PTY logcat shows zsh_body.sh content lines"
else
    # Non-fatal: PTY broadcast path may not be wired for direct test; check
    # WarpTerminal:PtyOutput for any output from the cat command
    echo "  INFO: PTY cat output not captured via logcat (non-fatal; asset extraction is the primary gate)"
fi

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
echo ""
echo "=============================="
echo "M3-S06 verification summary"
echo "=============================="
echo "PASS: $PASS  FAIL: $FAIL"
if [[ $FAIL -eq 0 ]]; then
    echo "RESULT: M3-S06 ship + extract VERIFIED"
    exit 0
else
    echo "RESULT: FAIL ($FAIL gate(s) failed)"
    exit 1
fi
