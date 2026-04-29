#!/usr/bin/env zsh
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

# Get app UID for orphan detection
APP_UID=$(adb_cmd shell dumpsys package "$PKG" 2>/dev/null \
    | grep -oE 'userId=[0-9]+' | head -1 | grep -oE '[0-9]+' || print "")

# Launch app and spawn bash PTY
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
sleep 1
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2
adb_cmd shell am broadcast -n "${PKG}/.PtyBroadcastReceiver" -a dev.warp.mobile.PTY_SPAWN --es cmd "bash" 2>/dev/null || true
sleep 1

# Check notification is present before kill
NOTIF_BEFORE=$(adb_cmd shell dumpsys notification --noredact 2>/dev/null \
    | grep -c "Warp terminal" || print 0)

# Snapshot all processes before kill (full ps with PID/PPID/USER/NAME)
BEFORE_PS=$(adb_cmd shell ps -A -o PID,PPID,USER,NAME 2>/dev/null || true)

# Count warp-related processes before kill
PID_BEFORE_COUNT=$(print "$BEFORE_PS" | grep -c "$PKG" || print 0)

# FAIL if app wasn't running (script header line 7)
if [[ "$PID_BEFORE_COUNT" -eq 0 ]]; then
    jq -n \
      --arg  device         "$DEVICE" \
      --argjson pid_before   0 \
      --argjson pid_after    0 \
      --argjson orphans      0 \
      --argjson pass         "false" \
      --arg  after_listing   "n/a" \
      --argjson notif_before 0 \
      --argjson notif_after  0 \
      --arg  script_version  "$SCRIPT_VERSION" \
      --arg  git_commit      "$GIT_COMMIT" \
      --arg  artifact_path   "$ARTIFACT_PATH" \
      '{device:$device,pid_before:$pid_before,pid_after:$pid_after,orphans:$orphans,pass:$pass,after_listing:$after_listing,notif_before:$notif_before,notif_after:$notif_after,error:"app_not_running_before_kill",script_version:$script_version,git_commit:$git_commit,artifact_path:$artifact_path}'
    exit 1
fi

# Kill the app
adb_cmd shell am kill "$PKG" 2>/dev/null || true
sleep 2

# Check notification is absent after kill
NOTIF_AFTER=$(adb_cmd shell dumpsys notification --noredact 2>/dev/null \
    | grep -c "Warp terminal" || print 0)

# Snapshot all processes after kill
AFTER_PS=$(adb_cmd shell ps -A -o PID,PPID,USER,NAME 2>/dev/null || true)

# Count any remaining warp package processes
PID_AFTER_PKG=$(print "$AFTER_PS" | grep -c "$PKG" || print 0)

# Count orphan children by UID (processes with same user but not the main package name)
ORPHAN_BY_UID=0
if [[ -n "$APP_UID" ]]; then
    ORPHAN_BY_UID=$(print "$AFTER_PS" | awk -v uid="u0_a${APP_UID}" '$3 == uid' | grep -cv "$PKG" || print 0)
fi

AFTER_LISTING=$(print "$AFTER_PS" | grep "$PKG" || print "none")
ORPHANS=$(( PID_AFTER_PKG + ORPHAN_BY_UID ))

PASS="true"
[[ $ORPHANS -gt 0 ]] && PASS="false"
[[ $NOTIF_BEFORE -eq 0 ]] && PASS="false"
[[ $NOTIF_AFTER -gt 0 ]] && PASS="false"

jq -n \
  --arg  device         "$DEVICE" \
  --argjson pid_before   "$PID_BEFORE_COUNT" \
  --argjson pid_after    "$PID_AFTER_PKG" \
  --argjson orphans      "$ORPHANS" \
  --argjson pass         "$PASS" \
  --arg  after_listing   "${AFTER_LISTING:-none}" \
  --argjson notif_before "$NOTIF_BEFORE" \
  --argjson notif_after  "$NOTIF_AFTER" \
  --arg  script_version  "$SCRIPT_VERSION" \
  --arg  git_commit      "$GIT_COMMIT" \
  --arg  artifact_path   "$ARTIFACT_PATH" \
  '{device:$device,pid_before:$pid_before,pid_after:$pid_after,orphans:$orphans,pass:$pass,after_listing:$after_listing,notif_before:$notif_before,notif_after:$notif_after,script_version:$script_version,git_commit:$git_commit,artifact_path:$artifact_path}'

[[ "$PASS" == "true" ]]
