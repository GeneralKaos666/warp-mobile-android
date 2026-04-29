#!/bin/zsh
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

adb_cmd() { "$ADB" -s "$DEVICE" "$@"; }

# Launch app
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2

# Spawn PTY via broadcast (Service must handle this intent)
T_SPAWN=$(date +%s%3N)
adb_cmd shell am broadcast -a dev.warp.mobile.PTY_SPAWN \
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
COUNT=0
while [[ $COUNT -lt 30 ]]; do
    RAW=$(adb_cmd logcat -d 2>/dev/null || true)
    FOUND=$(print "$RAW" | grep "$LOGCAT_TAG" | grep "$TOKEN" | tail -1 || true)
    [[ -n "$FOUND" ]] && break
    COUNT=$(( COUNT + 1 ))
    sleep 1
done

if [[ -z "$FOUND" ]]; then
    print '{"device":"'"$DEVICE"'","t_spawn":'"$T_SPAWN"',"t_expected":'"$T_EXPECTED"',"t_seen":null,"delta_ms":null,"pass":false,"error":"no_token_found"}'
    exit 1
fi

T_SEEN=$(date +%s%3N)
DELTA=$(( T_SEEN > T_EXPECTED ? T_SEEN - T_EXPECTED : T_EXPECTED - T_SEEN ))
PASS=$([[ $DELTA -lt 1000 ]] && print "true" || print "false")

jq -n \
  --arg  device     "$DEVICE" \
  --argjson t_spawn    "$T_SPAWN" \
  --argjson t_expected "$T_EXPECTED" \
  --argjson t_seen     "$T_SEEN" \
  --argjson delta_ms   "$DELTA" \
  --argjson pass       "$PASS" \
  '{device:$device,t_spawn:$t_spawn,t_expected:$t_expected,t_seen:$t_seen,delta_ms:$delta_ms,pass:$pass}'

[[ "$PASS" == "true" ]]
