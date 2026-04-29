#!/bin/zsh
# test-fgs-clean-kill.sh — S08 acceptance: no orphan PTY children after kill
#
# PREREQUISITE: S05 (android/ Gradle project + WarpTerminalService) must be
# deployed. WarpTerminalService must spawn at least one PTY child process
# and clean it up when the service is killed (SIGTERM / am kill).
# Until S05 lands this script will fail with pid_before=0 (no process running).
#
# Usage: $0 <device-serial>

set -euo pipefail

if [[ $# -lt 1 ]]; then
    print "Usage: $0 <device-serial>" >&2
    exit 1
fi

DEVICE="$1"
PKG="dev.warp.mobile"
ADB="/Users/iml1s/Library/Android/sdk/platform-tools/adb"

adb_cmd() { "$ADB" -s "$DEVICE" "$@"; }

# Launch app and spawn bash PTY
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
sleep 1
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2
adb_cmd shell am broadcast -a dev.warp.mobile.PTY_SPAWN --es cmd "bash" 2>/dev/null || true
sleep 1

# Capture all warp-related processes before kill
BEFORE_LISTING=$(adb_cmd shell ps -A 2>/dev/null | grep "$PKG" || true)
PID_BEFORE_COUNT=$(print "$BEFORE_LISTING" | grep -c "$PKG" || print 0)

# Kill the app
adb_cmd shell am kill "$PKG" 2>/dev/null || true
sleep 2

# Capture all warp-related processes after kill
AFTER_LISTING=$(adb_cmd shell ps -A 2>/dev/null | grep "$PKG" || true)
PID_AFTER_COUNT=$(print "$AFTER_LISTING" | grep -c "$PKG" || print 0)

ORPHANS=$PID_AFTER_COUNT
PASS=$([[ $ORPHANS -eq 0 ]] && print "true" || print "false")

jq -n \
  --arg  device      "$DEVICE" \
  --argjson pid_before "$PID_BEFORE_COUNT" \
  --argjson pid_after  "$PID_AFTER_COUNT" \
  --argjson orphans    "$ORPHANS" \
  --argjson pass       "$PASS" \
  --arg  after_listing "${AFTER_LISTING:-none}" \
  '{device:$device,pid_before:$pid_before,pid_after:$pid_after,orphans:$orphans,pass:$pass,after_listing:$after_listing}'

[[ "$PASS" == "true" ]]
