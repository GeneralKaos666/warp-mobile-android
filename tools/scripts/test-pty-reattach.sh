#!/usr/bin/env zsh
# test-pty-reattach.sh — S06 acceptance: PTY session survives rotation
#
# PREREQUISITE: S05 (android/ Gradle project + WarpTerminalService) must be
# deployed to the device and the Service must support a broadcast receiver
# action dev.warp.mobile.PTY_SPAWN that spawns a child process and echoes
# PTY_REATTACH_TOKEN_OK to logcat tag WarpTerminal:PtyOutput after a delay.
# Until S05 lands this script will fail with "no PTY_REATTACH_TOKEN_OK found".
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
LOGCAT_TAG="WarpTerminal:PtyOutput"
TOKEN="PTY_REATTACH_TOKEN_OK"
DELAY=10
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

# Reset rotation to 0 on exit
trap 'adb_cmd shell settings put system user_rotation 0 2>/dev/null || true' EXIT

# Launch app
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2

# Spawn PTY via broadcast (Service must handle this intent)
T_SPAWN=$(date +%s%3N)
adb_cmd shell am broadcast -n "${PKG}/.PtyBroadcastReceiver" \
    -a dev.warp.mobile.PTY_SPAWN \
    --es cmd "sleep ${DELAY} && echo ${TOKEN}" 2>/dev/null || true

# Rotate device 5 times while PTY runs
for i in {1..5}; do
    ROTATION=$(( (i % 2) ))
    adb_cmd shell settings put system user_rotation "$ROTATION" 2>/dev/null || true
    sleep 1.5
done

T_EXPECTED=$(( T_SPAWN + DELAY * 1000 ))

# Wait for token with tolerance
FOUND=""
FOUND_LINE=""
COUNT=0
while [[ $COUNT -lt 30 ]]; do
    RAW=$(adb_cmd logcat -d 2>/dev/null || true)
    FOUND_LINE=$(print "$RAW" | grep "$LOGCAT_TAG" | grep "$TOKEN" | tail -1 || true)
    if [[ -n "$FOUND_LINE" ]]; then
        FOUND="$FOUND_LINE"
        break
    fi
    COUNT=$(( COUNT + 1 ))
    sleep 1
done

if [[ -z "$FOUND" ]]; then
    jq -n \
      --arg  device         "$DEVICE" \
      --argjson t_spawn      "$T_SPAWN" \
      --argjson t_expected   "$T_EXPECTED" \
      --arg  script_version  "$SCRIPT_VERSION" \
      --arg  git_commit      "$GIT_COMMIT" \
      --arg  artifact_path   "$ARTIFACT_PATH" \
      '{device:$device,t_spawn:$t_spawn,t_expected:$t_expected,t_seen:null,delta_ms:null,pass:false,error:"no_token_found",script_version:$script_version,git_commit:$git_commit,artifact_path:$artifact_path}'
    exit 1
fi

# Parse timestamp from logcat line (format: MM-DD HH:MM:SS.mmm)
# e.g. "04-29 17:23:45.123  1234  5678 I WarpTerminal:PtyOutput: ..."
LOGCAT_TS=$(print "$FOUND" | grep -oE '^[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]+' | head -1 || true)

if [[ -n "$LOGCAT_TS" ]]; then
    # Convert MM-DD HH:MM:SS.mmm to epoch ms using python3
    YEAR=$(date +%Y)
    T_SEEN=$(python3 -c "
import datetime, calendar
ts = '${YEAR}-${LOGCAT_TS}'
# ts format: YYYY-MM-DD HH:MM:SS.mmm
dt = datetime.datetime.strptime(ts, '%Y-%m-%d %H:%M:%S.%f')
epoch_ms = int(calendar.timegm(dt.timetuple()) * 1000 + dt.microsecond // 1000)
print(epoch_ms)
" 2>/dev/null || date +%s%3N)
else
    T_SEEN=$(date +%s%3N)
fi

DELTA=$(( T_SEEN > T_EXPECTED ? T_SEEN - T_EXPECTED : T_EXPECTED - T_SEEN ))
PASS=$([[ $DELTA -lt 1000 ]] && print "true" || print "false")

jq -n \
  --arg  device         "$DEVICE" \
  --argjson t_spawn      "$T_SPAWN" \
  --argjson t_expected   "$T_EXPECTED" \
  --argjson t_seen       "$T_SEEN" \
  --argjson delta_ms     "$DELTA" \
  --argjson pass         "$PASS" \
  --arg  script_version  "$SCRIPT_VERSION" \
  --arg  git_commit      "$GIT_COMMIT" \
  --arg  artifact_path   "$ARTIFACT_PATH" \
  '{device:$device,t_spawn:$t_spawn,t_expected:$t_expected,t_seen:$t_seen,delta_ms:$delta_ms,pass:$pass,script_version:$script_version,git_commit:$git_commit,artifact_path:$artifact_path}'

[[ "$PASS" == "true" ]]
