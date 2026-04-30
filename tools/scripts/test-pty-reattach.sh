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

# Reset rotation to 0 on EXIT or interrupt — keeps device usable if SIGINT/SIGTERM
trap 'adb_cmd shell settings put system user_rotation 0 2>/dev/null || true' EXIT INT TERM

# Launch app
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" >/dev/null 2>&1
sleep 2

# Spawn PTY via broadcast (Service must handle this intent)
# %3N not supported on all platforms; fall back to seconds * 1000
T_SPAWN=$(python3 -c "import time; print(int(time.time()*1000))" 2>/dev/null || echo $(( $(date +%s) * 1000 )))
adb_cmd shell am start-foreground-service -n "${PKG}/.WarpTerminalService" \
    -a dev.warp.mobile.PTY_SPAWN \
    --es cmd "sh" 2>/dev/null || true
sleep 1
adb_cmd shell am start-foreground-service -n "${PKG}/.WarpTerminalService" \
    -a dev.warp.mobile.PTY_WRITE \
    --es data "sleep ${DELAY} && echo ${TOKEN}" 2>/dev/null || true

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
    # Use -v epoch so timestamps are unambiguous seconds since epoch — bypasses
    # timezone, DST, and year-rollover issues that plague MM-DD parsing.
    RAW=$(adb_cmd logcat -d -v epoch 2>/dev/null || true)
    FOUND_LINE=$(printf '%s\n' "$RAW" | grep -F "$LOGCAT_TAG" | grep "$TOKEN" | tail -1 || true)
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

# Parse epoch timestamp from logcat line. With `-v epoch`, format is:
# "1729087425.123  1234  5678 I WarpTerminal:PtyOutput: ..."
# (seconds.millis at line start). No timezone / year ambiguity.
LOGCAT_EPOCH=$(printf '%s\n' "$FOUND" | grep -oE '^[[:space:]]*[0-9]+\.[0-9]+' | head -1 | tr -d '[:space:]' || true)

if [[ -n "$LOGCAT_EPOCH" ]]; then
    # Convert seconds.millis to integer milliseconds since epoch
    T_SEEN=$(python3 -c "print(int(float('$LOGCAT_EPOCH') * 1000))" 2>/dev/null \
        || python3 -c "import time; print(int(time.time()*1000))")
else
    # Fallback: host time at moment we found the token (less accurate)
    T_SEEN=$(python3 -c "import time; print(int(time.time()*1000))")
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
