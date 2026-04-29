#!/usr/bin/env zsh
# test-30min-idle-stress.sh — M1-S09 acceptance: 30-min idle PTY stress test
#
# PREREQUISITE: M1-S05 (WarpTerminalService + JNI PTY) and M1-S06 (PTY reattach)
# must be deployed. Service must handle broadcast dev.warp.mobile.PTY_SPAWN and
# log PTY output under logcat tag WarpTerminal:PtyOutput.
#
# Usage: $0 <device-serial>
# Output: JSON result to stdout + .omc/m1-artifacts/M1-stress-test.md artifact

set -uo pipefail

if [[ $# -lt 1 ]]; then
    print "Usage: $0 <device-serial>" >&2
    exit 1
fi

DEVICE="$1"
PKG="dev.warp.mobile"
ADB="${ADB_PATH:-/Users/iml1s/Library/Android/sdk/platform-tools/adb}"
REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
ARTIFACT_DIR="${REPO_ROOT}/.omc/m1-artifacts"
ARTIFACT_FILE="${ARTIFACT_DIR}/M1-stress-test.md"
LOGCAT_TAG="WarpTerminal:PtyOutput"
PID_FILE="/tmp/stress_pid_$$"
LOGCAT_FULL="/tmp/logcat_full_$$.txt"
SCRIPT_VERSION="1.0"
GIT_COMMIT="$(git -C "${REPO_ROOT}" rev-parse HEAD 2>/dev/null || print 'unknown')"

adb_cmd() { "$ADB" -s "$DEVICE" "$@"; }

log() { print "[$(date '+%H:%M:%S')] $*" >&2; }

# Preflight: confirm device is online
DEVICE_STATE=$(adb_cmd get-state 2>/dev/null || print "error")
if [[ "$DEVICE_STATE" != "device" ]]; then
    print "ERROR: device $DEVICE is not ready (state: $DEVICE_STATE). Check USB/WiFi connection." >&2
    exit 2
fi

mkdir -p "$ARTIFACT_DIR"

# ── partial-artifact trap ────────────────────────────────────────────────────
write_partial_artifact() {
    log "Writing partial artifact on exit..."
    {
        print "# M1-S09 30-min Idle Stress Test (PARTIAL / INTERRUPTED)"
        print ""
        print "Device: \`${DEVICE}\`  "
        print "Run time: $(date -u '+%Y-%m-%dT%H:%M:%SZ')  "
        print "Pass: **INCOMPLETE**"
        print ""
        print "## Interval Snapshots (captured so far)"
        print ""
        for interval in t0 t10 t20 t30; do
            local alive_var="SNAP_${interval}_ALIVE"
            local notif_var="SNAP_${interval}_NOTIF"
            local anom_var="SNAP_${interval}_ANOMALIES"
            local ps_var="SNAP_${interval}_PS"
            local logcat_var="SNAP_${interval}_LOGCAT"
            if [[ -n "${(P)alive_var+x}" ]]; then
                print "### $interval"
                print "- alive: ${(P)alive_var}"
                print "- notification_visible: ${(P)notif_var}"
                print "- anomalies: ${(P)anom_var}"
                print ""
                print "\`\`\`"
                cat "${(P)ps_var}" 2>/dev/null || print "(no ps output)"
                print "\`\`\`"
                print ""
                print "#### logcat snippet ($interval)"
                print "\`\`\`"
                tail -20 "${(P)logcat_var}" 2>/dev/null || print "(no logcat)"
                print "\`\`\`"
                print ""
            fi
        done
    } > "$ARTIFACT_FILE"
}

trap 'write_partial_artifact' EXIT INT TERM

# ── snapshot helper ──────────────────────────────────────────────────────────
take_snapshot() {
    local label="$1"
    local logcat_file="/tmp/logcat_${label}_$$.txt"
    local ps_file="/tmp/ps_${label}_$$.txt"

    adb_cmd logcat -d > "$logcat_file" 2>/dev/null || true
    adb_cmd shell ps -A 2>/dev/null | grep "$PKG" > "$ps_file" 2>/dev/null || true

    local alive=0
    [[ -s "$ps_file" ]] && alive=1

    local notif=0
    adb_cmd shell dumpsys notification --noredact 2>/dev/null \
        | grep -q "Warp terminal" && notif=1 || true

    local anomalies=0
    anomalies=$(adb_cmd logcat -d 2>/dev/null \
        | grep -cE "PhantomProcess|signal [0-9]+|FATAL|crash" 2>/dev/null || true)

    eval "SNAP_${label}_ALIVE=$alive"
    eval "SNAP_${label}_NOTIF=$notif"
    eval "SNAP_${label}_ANOMALIES=$anomalies"
    eval "SNAP_${label}_LOGCAT=$logcat_file"
    eval "SNAP_${label}_PS=$ps_file"

    log "[$label] alive=$alive notif=$notif anomalies=$anomalies"
}

# ── main ─────────────────────────────────────────────────────────────────────
log "Starting 30-min idle stress test on $DEVICE"

# Launch app and spawn PTY
adb_cmd shell am force-stop "$PKG" 2>/dev/null || true
sleep 1
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${PKG}/.MainActivity" > /dev/null 2>&1
sleep 3

log "Spawning bash PTY via broadcast..."
adb_cmd shell am broadcast -a dev.warp.mobile.PTY_SPAWN \
    --es cmd "bash" 2>/dev/null || true
sleep 2

# t=0 snapshot
T0=$(date +%s%3N)
take_snapshot "t0"

# ── 10-minute intervals ──────────────────────────────────────────────────────
log "Sleeping 600s (t=0 → t=10)..."
sleep 600
take_snapshot "t10"

log "Sleeping 600s (t=10 → t=20)..."
sleep 600
take_snapshot "t20"

log "Sleeping 600s (t=20 → t=30)..."
sleep 600
take_snapshot "t30"

# ── pwd response latency test ────────────────────────────────────────────────
log "Sending 'pwd' command to PTY..."
adb_cmd logcat -c 2>/dev/null || true
T_SEND=$(date +%s%3N)
adb_cmd shell am broadcast -a dev.warp.mobile.PTY_WRITE \
    --es data "pwd\n" 2>/dev/null || true

PWD_RESPONSE_MS="-1"
COUNT=0
while [[ $COUNT -lt 20 ]]; do
    RAW=$(adb_cmd logcat -d 2>/dev/null || true)
    FOUND=$(print "$RAW" | grep "$LOGCAT_TAG" | grep "/" | tail -1 || true)
    if [[ -n "$FOUND" ]]; then
        T_RECV=$(date +%s%3N)
        PWD_RESPONSE_MS=$(( T_RECV - T_SEND ))
        break
    fi
    COUNT=$(( COUNT + 1 ))
    sleep 0.5
done

log "pwd response time: ${PWD_RESPONSE_MS}ms"

# ── final anomaly scan ───────────────────────────────────────────────────────
ANOMALY_FILE="/tmp/anomaly_final_$$.txt"
adb_cmd logcat -d 2>/dev/null \
    | grep -E "PhantomProcess|signal [0-9]+|FATAL|crash" > "$ANOMALY_FILE" 2>/dev/null || true
FINAL_ANOMALIES=$(wc -l < "$ANOMALY_FILE" | tr -d ' ')

# ── overall pass/fail ────────────────────────────────────────────────────────
PASS="true"
[[ "$SNAP_t30_ALIVE" -eq 0 ]] && PASS="false"
[[ "$SNAP_t30_NOTIF" -eq 0 ]] && PASS="false"
[[ "$FINAL_ANOMALIES" -gt 0 ]] && PASS="false"
# pwd latency: -1 means no response; >= 500 is also a fail (PRD says <500ms)
[[ "$PWD_RESPONSE_MS" -eq -1 || "$PWD_RESPONSE_MS" -ge 500 ]] && PASS="false"

# ── write full artifact (overrides partial trap output) ──────────────────────
trap - EXIT INT TERM
{
    print "# M1-S09 30-min Idle Stress Test"
    print ""
    print "Device: \`${DEVICE}\`  "
    print "Run time: $(date -u '+%Y-%m-%dT%H:%M:%SZ')  "
    print "Pass: **${PASS}**"
    print ""
    print "## Interval Snapshots"
    print ""
    for interval in t0 t10 t20 t30; do
        eval "local_alive=\$SNAP_${interval}_ALIVE"
        eval "local_notif=\$SNAP_${interval}_NOTIF"
        eval "local_anom=\$SNAP_${interval}_ANOMALIES"
        eval "local_ps=\$SNAP_${interval}_PS"
        eval "local_logcat=\$SNAP_${interval}_LOGCAT"
        print "### $interval"
        print "- alive: $local_alive"
        print "- notification_visible: $local_notif"
        print "- anomalies: $local_anom"
        print ""
        print "\`\`\`"
        cat "$local_ps" 2>/dev/null || print "(no ps output)"
        print "\`\`\`"
        print ""
        print "#### logcat snippet ($interval)"
        print "\`\`\`"
        tail -20 "$local_logcat" 2>/dev/null || print "(no logcat)"
        print "\`\`\`"
        print ""
    done
    print "## pwd Response Latency"
    print ""
    print "Response time: **${PWD_RESPONSE_MS}ms** (threshold: <500ms)"
    print ""
    print "## Final Anomaly Scan"
    print ""
    print "\`\`\`"
    cat "$ANOMALY_FILE" 2>/dev/null || print "(no anomalies)"
    print "\`\`\`"
} > "$ARTIFACT_FILE"

log "Artifact written to $ARTIFACT_FILE"

# ── JSON output ──────────────────────────────────────────────────────────────
jq -n \
  --arg  device             "$DEVICE" \
  --argjson t0_alive        "$SNAP_t0_ALIVE" \
  --argjson t0_notif        "$SNAP_t0_NOTIF" \
  --argjson t0_anomalies    "$SNAP_t0_ANOMALIES" \
  --argjson t10_alive       "$SNAP_t10_ALIVE" \
  --argjson t10_notif       "$SNAP_t10_NOTIF" \
  --argjson t10_anomalies   "$SNAP_t10_ANOMALIES" \
  --argjson t20_alive       "$SNAP_t20_ALIVE" \
  --argjson t20_notif       "$SNAP_t20_NOTIF" \
  --argjson t20_anomalies   "$SNAP_t20_ANOMALIES" \
  --argjson t30_alive       "$SNAP_t30_ALIVE" \
  --argjson t30_notif       "$SNAP_t30_NOTIF" \
  --argjson t30_anomalies   "$SNAP_t30_ANOMALIES" \
  --argjson pwd_response_ms "$PWD_RESPONSE_MS" \
  --argjson pass            "$PASS" \
  --arg  script_version     "$SCRIPT_VERSION" \
  --arg  git_commit         "$GIT_COMMIT" \
  --arg  artifact_path      "$ARTIFACT_FILE" \
  '{
    device: $device,
    t0:  {alive: $t0_alive,  notification_visible: $t0_notif,  anomalies: $t0_anomalies},
    t10: {alive: $t10_alive, notification_visible: $t10_notif, anomalies: $t10_anomalies},
    t20: {alive: $t20_alive, notification_visible: $t20_notif, anomalies: $t20_anomalies},
    t30: {alive: $t30_alive, notification_visible: $t30_notif, anomalies: $t30_anomalies},
    pwd_response_ms: $pwd_response_ms,
    pass: $pass,
    script_version: $script_version,
    git_commit: $git_commit,
    artifact_path: $artifact_path
  }'

[[ "$PASS" == "true" ]]
