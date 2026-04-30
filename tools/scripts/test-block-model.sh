#!/usr/bin/env zsh
# test-block-model.sh <device-serial>
#
# M3-S07 device verification driver. Installs the warp-mobile APK, launches
# MainActivity in terminal_mode, injects 3 synthetic DCS hook triplets
# (Precmd + Preexec + CommandFinished — mirroring the upstream zsh_body.sh
# wire format) for ls / whoami / false (exit 0/0/1), then asserts the
# Block aggregator produced 3 Blocks with the right command + exit_code.
#
# Why synthetic injection? S24 Ultra ships with /system/bin/sh (mksh) — no
# zsh. M3-S06 deferred real zsh-hook execution to M5 (Termux). For M3-S07
# we drive the parser/aggregator directly via TerminalSimulationReceiver
# TERM_INJECT_RAW (the M3-S05 pattern).
#
# Acceptance gates (per .omc/prd.json M3-S07 AC#6,7):
#   * block_count == 3
#   * commands match ["ls", "whoami", "false"] in arrival order
#   * exit_codes match [0, 0, 1] in arrival order
#   * dcs_error_count == 0 (parser saw no malformed payloads)
#
# Usage:
#   ./tools/scripts/test-block-model.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m3-artifacts/M3-S07-result.json
#   /tmp/m3-s07-logcat.txt          (filtered logcat)
#   /tmp/m3-s07-blocks-dump.json    (raw blocks JSON from logcat)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m3-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M3-S07-result.json"
LOGCAT_OUT="/tmp/m3-s07-logcat.txt"
BLOCKS_DUMP="/tmp/m3-s07-blocks-dump.json"

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

# Anti-Knox-idle keep-awake.
source "$SCRIPT_DIR/lib/keep-awake.sh"
keep_awake_setup "$SERIAL"
keep_awake_start "$SERIAL"
trap 'keep_awake_stop || true; keep_awake_restore "$SERIAL" || true' EXIT

echo "=== install APK ===" >&2
"${ADB[@]}" install -r -g "$APK" 2>&1 | tail -3 >&2

echo "=== force-stop existing app ===" >&2
"${ADB[@]}" shell am force-stop "$PACKAGE" || true

echo "=== launch MainActivity in terminal_mode ===" >&2
"${ADB[@]}" shell am start -n "${PACKAGE}/${ACTIVITY}" --ez terminal_mode true
sleep 4

PID=$("${ADB[@]}" shell pidof "$PACKAGE" | tr -d '\r')
if [[ -z "$PID" ]]; then
    echo "ERROR: ${PACKAGE} did not start" >&2
    exit 1
fi
echo "=== app PID: $PID ===" >&2

# Clear logcat.
"${ADB[@]}" logcat -c

# ── Inject 3 DCS triplets ──────────────────────────────────────────────────
#
# Each triplet matches the upstream zsh_body.sh emission pattern:
#   Precmd → Preexec → CommandFinished
# The aggregator pushes a Block on Precmd, fills command on Preexec, and
# finalizes exit_code on CommandFinished.

inject_dcs() {
    local label="$1"
    local json="$2"
    local b64
    b64=$(python3 -c "
import base64, sys
json_payload = sys.argv[1].encode()
hex_body = ''.join(f'{b:02x}' for b in json_payload).encode()
frame = b'\x1bP\$d' + hex_body + b'\x9c'
print(base64.b64encode(frame).decode())
" "$json")
    "${ADB[@]}" shell am broadcast \
        -a dev.warp.mobile.TERM_INJECT_RAW \
        -p "$PACKAGE" \
        --es cmd_id "$label" \
        --es bytes_b64 "$b64" \
        > /dev/null
    sleep 0.3
}

echo "=== triplet 1: ls (exit 0) ===" >&2
inject_dcs "t1-precmd" '{"hook":"Precmd","value":{"pwd":"/data/data/dev.warp.mobile/files","ps1":"$","session_id":42}}'
inject_dcs "t1-preexec" '{"hook":"Preexec","value":{"command":"ls"}}'
inject_dcs "t1-cmdfin" '{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-42-1"}}'

echo "=== triplet 2: whoami (exit 0) ===" >&2
inject_dcs "t2-precmd" '{"hook":"Precmd","value":{"pwd":"/data/data/dev.warp.mobile/files","ps1":"$","session_id":42}}'
inject_dcs "t2-preexec" '{"hook":"Preexec","value":{"command":"whoami"}}'
inject_dcs "t2-cmdfin" '{"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"session-42-2"}}'

echo "=== triplet 3: false (exit 1) ===" >&2
inject_dcs "t3-precmd" '{"hook":"Precmd","value":{"pwd":"/data/data/dev.warp.mobile/files","ps1":"$","session_id":42}}'
inject_dcs "t3-preexec" '{"hook":"Preexec","value":{"command":"false"}}'
inject_dcs "t3-cmdfin" '{"hook":"CommandFinished","value":{"exit_code":1,"next_block_id":"session-42-3"}}'

# ── Dump current Vec<Block> via TERM_BLOCKS_DUMP broadcast ────────────────
echo "=== request TERM_BLOCKS_DUMP ===" >&2
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_BLOCKS_DUMP \
    -p "$PACKAGE" \
    > /dev/null
sleep 1

# ── Pull logcat ────────────────────────────────────────────────────────────
echo "=== pull logcat ===" >&2
"${ADB[@]}" logcat -d > "$LOGCAT_OUT" 2>&1 || true

# Extract the JSON dump line.
DUMP_LINE=$(grep "TERM_BLOCKS_DUMP json=" "$LOGCAT_OUT" | tail -1 || true)
if [[ -z "$DUMP_LINE" ]]; then
    echo "ERROR: TERM_BLOCKS_DUMP line not found in logcat" >&2
    tail -50 "$LOGCAT_OUT" >&2
    exit 1
fi

# Strip everything before json= and any trailing carriage return.
echo "$DUMP_LINE" | sed -nE 's/.*TERM_BLOCKS_DUMP json=(.*)$/\1/p' | tr -d '\r' > "$BLOCKS_DUMP"
echo "    blocks_dump bytes: $(wc -c < "$BLOCKS_DUMP" | tr -d ' ')" >&2
echo "    blocks_dump preview: $(head -c 400 "$BLOCKS_DUMP")" >&2

# Count Block aggregation events in logcat.
BLOCK_EVENT_LINES=$(grep -c "block_event " "$LOGCAT_OUT" || true)
echo "    block_event lines in logcat: $BLOCK_EVENT_LINES" >&2

# DCS parser counters from terminalSgrSummary (delivered after each
# TERM_INJECT_RAW broadcast).
LAST_SUMMARY=$(grep "TERM_INJECT_RAW summary=" "$LOGCAT_OUT" | tail -1 || true)
DCS_HOOK_COUNT=$(echo "$LAST_SUMMARY" | sed -nE 's/.*dcs_hook_count=([0-9]+).*/\1/p')
DCS_ERROR_COUNT=$(echo "$LAST_SUMMARY" | sed -nE 's/.*dcs_error_count=([0-9]+).*/\1/p')
echo "    last dcs_hook_count: ${DCS_HOOK_COUNT:-missing}" >&2
echo "    last dcs_error_count: ${DCS_ERROR_COUNT:-missing}" >&2

# Parse the JSON dump.
BLOCK_COUNT=$(python3 -c "
import json, sys
try:
    with open('$BLOCKS_DUMP') as f:
        data = json.load(f)
    print(len(data))
except Exception as e:
    print('parse_err:' + str(e), file=sys.stderr)
    print(0)
" 2>&1)

if [[ "$BLOCK_COUNT" =~ ^parse_err: ]]; then
    echo "ERROR: failed to parse blocks dump JSON" >&2
    cat "$BLOCKS_DUMP" >&2
    exit 1
fi

# Build per-block command/exit_code arrays for the result.
COMMANDS_JSON=$(python3 -c "
import json
with open('$BLOCKS_DUMP') as f:
    data = json.load(f)
print(json.dumps([b.get('command', '') for b in data]))
")
EXIT_CODES_JSON=$(python3 -c "
import json
with open('$BLOCKS_DUMP') as f:
    data = json.load(f)
print(json.dumps([b.get('exit_code', None) for b in data]))
")
COMMAND_MATCH=$(python3 -c "
import json
expected = ['ls', 'whoami', 'false']
with open('$BLOCKS_DUMP') as f:
    data = json.load(f)
got = [b.get('command', '') for b in data]
print(json.dumps([(i < len(got) and got[i] == expected[i]) for i in range(len(expected))]))
")
EXIT_CODE_MATCH=$(python3 -c "
import json
expected = [0, 0, 1]
with open('$BLOCKS_DUMP') as f:
    data = json.load(f)
got = [b.get('exit_code', None) for b in data]
print(json.dumps([(i < len(got) and got[i] == expected[i]) for i in range(len(expected))]))
")

# ── Gate ───────────────────────────────────────────────────────────────────

GATE_PASS="false"
ALL_CMD_MATCH=$(echo "$COMMAND_MATCH" | python3 -c "import json,sys; print('true' if all(json.load(sys.stdin)) else 'false')")
ALL_EXIT_MATCH=$(echo "$EXIT_CODE_MATCH" | python3 -c "import json,sys; print('true' if all(json.load(sys.stdin)) else 'false')")

if [[ "$BLOCK_COUNT" == "3" \
   && "$ALL_CMD_MATCH" == "true" \
   && "$ALL_EXIT_MATCH" == "true" \
   && "${DCS_ERROR_COUNT:-1}" == "0" ]]; then
    GATE_PASS="true"
fi

# ── Result JSON ────────────────────────────────────────────────────────────

cat > "$RESULT_JSON" <<EOF
{
  "story": "M3-S07",
  "device_serial": "$SERIAL",
  "block_count": $BLOCK_COUNT,
  "expected_block_count": 3,
  "commands": $COMMANDS_JSON,
  "expected_commands": ["ls", "whoami", "false"],
  "command_match": $COMMAND_MATCH,
  "exit_codes": $EXIT_CODES_JSON,
  "expected_exit_codes": [0, 0, 1],
  "exit_code_match": $EXIT_CODE_MATCH,
  "evidence": {
    "blocks_dump_path": "$BLOCKS_DUMP",
    "logcat": "$LOGCAT_OUT",
    "block_event_lines_in_logcat": $BLOCK_EVENT_LINES,
    "last_dcs_summary_line": "${LAST_SUMMARY}",
    "dcs_hook_count": "${DCS_HOOK_COUNT:-missing}",
    "dcs_error_count": "${DCS_ERROR_COUNT:-missing}"
  },
  "gate": {
    "overall_pass": $GATE_PASS,
    "criteria": "block_count == 3 AND commands == [ls,whoami,false] AND exit_codes == [0,0,1] AND dcs_error_count == 0"
  }
}
EOF

echo "" >&2
echo "=== M3-S07 result (gate: $GATE_PASS) ===" >&2
cat "$RESULT_JSON" >&2

if [[ "$GATE_PASS" != "true" ]]; then
    echo "" >&2
    echo "=== last 30 logcat lines ===" >&2
    tail -30 "$LOGCAT_OUT" >&2
    exit 1
fi
