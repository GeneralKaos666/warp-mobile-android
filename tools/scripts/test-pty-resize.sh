#!/bin/zsh
# test-pty-resize.sh — S07 acceptance: PTY resize propagated to child process
#
# PREREQUISITE: S05 (android/ Gradle project + WarpTerminalService) must be
# deployed. WarpTerminalService must:
#   1. Spawn a bash PTY on launch (or on PTY_SPAWN broadcast)
#   2. Implement a BroadcastReceiver for action dev.warp.mobile.PTY_RESIZE
#      with extras rows (int) and cols (int) that calls NativeBridge.ptyResize()
#   3. Log PTY stdout lines to logcat tag WarpTerminal:PtyOutput
# Until S05 lands this script will fail with "no stty size output found".
#
# Usage: $0 <device-serial> [rows=24] [cols=80]

set -euo pipefail

if [[ $# -lt 1 ]]; then
    print "Usage: $0 <device-serial> [rows] [cols]" >&2
    exit 1
fi

DEVICE="$1"
ROWS="${2:-24}"
COLS="${3:-80}"
PKG="dev.warp.mobile"
ADB="/Users/iml1s/Library/Android/sdk/platform-tools/adb"
LOGCAT_TAG="WarpTerminal:PtyOutput"

adb_cmd() { "$ADB" -s "$DEVICE" "$@"; }

# Launch app and spawn bash PTY
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2
adb_cmd shell am broadcast -a dev.warp.mobile.PTY_SPAWN --es cmd "bash" 2>/dev/null || true
sleep 1

# Send resize broadcast
adb_cmd shell am broadcast \
    -a dev.warp.mobile.PTY_RESIZE \
    --ei rows "$ROWS" \
    --ei cols "$COLS" 2>/dev/null || true
sleep 1

# Write stty size to PTY stdin via broadcast
adb_cmd shell am broadcast \
    -a dev.warp.mobile.PTY_INPUT \
    --es data "stty size\n" 2>/dev/null || true

# Wait for stty size output in logcat
OBSERVED=""
COUNT=0
while [[ $COUNT -lt 15 ]]; do
    RAW=$(adb_cmd logcat -d 2>/dev/null || true)
    LINE=$(print "$RAW" | grep "$LOGCAT_TAG" | grep -E '^[0-9]+ [0-9]+$' | tail -1 || true)
    if [[ -n "$LINE" ]]; then
        OBSERVED=$(print "$LINE" | grep -oE '[0-9]+ [0-9]+' | tail -1)
        break
    fi
    COUNT=$(( COUNT + 1 ))
    sleep 1
done

EXPECTED="${ROWS} ${COLS}"
PASS=$([[ "$OBSERVED" == "$EXPECTED" ]] && print "true" || print "false")

jq -n \
  --arg  device   "$DEVICE" \
  --argjson rows  "$ROWS" \
  --argjson cols  "$COLS" \
  --arg  expected "$EXPECTED" \
  --arg  observed "${OBSERVED:-none}" \
  --argjson pass  "$PASS" \
  '{device:$device,rows:$rows,cols:$cols,expected:$expected,observed:$observed,pass:$pass}'

[[ "$PASS" == "true" ]]
