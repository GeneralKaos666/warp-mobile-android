#!/usr/bin/env zsh
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
# M3-S11 nit fix (2026-05-01): hardcoded /Users/iml1s/.../adb replaced with
# ${ADB:-$(which adb)} for portability across worker machines.
ADB="${ADB:-$(which adb)}"
if [[ -z "$ADB" || ! -x "$ADB" ]]; then
    print "ERROR: adb not found on PATH and ADB not set; install Android platform-tools or pass ADB=/path/to/adb." >&2
    exit 1
fi
LOGCAT_TAG="WarpTerminal:PtyOutput"
SCRIPT_VERSION="1.0"
GIT_COMMIT="$(git -C "$(dirname "$0")" rev-parse HEAD 2>/dev/null || print 'unknown')"
ARTIFACT_PATH=""

adb_cmd() { "$ADB" -s "$DEVICE" "$@"; }

# Preflight: confirm device is online
DEVICE_STATE=$(adb_cmd get-state 2>/dev/null || print "error")
if [[ "$DEVICE_STATE" != "device" ]]; then
    print "ERROR: device $DEVICE is not ready (state: $DEVICE_STATE). Check USB/WiFi connection." >&2
    exit 2
fi

# Launch app and spawn sh PTY (bash may not be on /system/bin)
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2
adb_cmd shell "am start-foreground-service -n '${PKG}/.WarpTerminalService' -a dev.warp.mobile.PTY_SPAWN --es cmd 'sh'" 2>/dev/null || true
sleep 1

# Send resize via FGS direct (same path as S06 — debug overlay exposes Service)
adb_cmd shell "am start-foreground-service -n '${PKG}/.WarpTerminalService' -a dev.warp.mobile.PTY_RESIZE --ei rows ${ROWS} --ei cols ${COLS}" 2>/dev/null || true
sleep 1

# Write `stty size` — handleWrite appends \n automatically when missing
adb_cmd shell "am start-foreground-service -n '${PKG}/.WarpTerminalService' -a dev.warp.mobile.PTY_WRITE --es data 'stty size'" 2>/dev/null || true

# Wait for stty size output in logcat. Anchor on lines whose PtyOutput message
# is *exactly* two numbers (rows cols) — NOT the date/PID/TID columns of the
# logcat default format, NOT the echo'd 'stty size' command line.
OBSERVED=""
COUNT=0
while [[ $COUNT -lt 15 ]]; do
    RAW=$(adb_cmd logcat -d 2>/dev/null || true)
    # Match: "...WarpTerminal:PtyOutput: <rows> <cols>" at end-of-line.
    LINE=$(printf '%s\n' "$RAW" | grep -E "WarpTerminal:PtyOutput:[[:space:]]+[0-9]+[[:space:]]+[0-9]+[[:space:]]*\$" | tail -1 || true)
    if [[ -n "$LINE" ]]; then
        # Extract just the two trailing numbers
        OBSERVED=$(printf '%s' "$LINE" | grep -oE '[0-9]+[[:space:]]+[0-9]+[[:space:]]*$' | head -1 | sed -E 's/[[:space:]]+$//' | tr -s ' ')
        break
    fi
    COUNT=$(( COUNT + 1 ))
    sleep 1
done

EXPECTED="${ROWS} ${COLS}"
PASS=$([[ "$OBSERVED" == "$EXPECTED" ]] && print "true" || print "false")

jq -n \
  --arg  device         "$DEVICE" \
  --argjson rows         "$ROWS" \
  --argjson cols         "$COLS" \
  --arg  expected        "$EXPECTED" \
  --arg  observed        "${OBSERVED:-none}" \
  --argjson pass         "$PASS" \
  --arg  script_version  "$SCRIPT_VERSION" \
  --arg  git_commit      "$GIT_COMMIT" \
  --arg  artifact_path   "$ARTIFACT_PATH" \
  '{device:$device,rows:$rows,cols:$cols,expected:$expected,observed:$observed,pass:$pass,script_version:$script_version,git_commit:$git_commit,artifact_path:$artifact_path}'

[[ "$PASS" == "true" ]]
