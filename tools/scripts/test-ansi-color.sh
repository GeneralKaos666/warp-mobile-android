#!/usr/bin/env zsh
# test-ansi-color.sh <device-serial>
#
# M3-S05 device verification driver. Installs the warp-mobile APK, launches
# MainActivity in terminal_mode, injects raw ANSI byte sequences via
# TerminalSimulationReceiver, and asserts the streaming ANSI/DCS state
# machine parsed them correctly.
#
# Acceptance gates (per .omc/prd.json M3-S05 AC#7):
#   * `sgr_color codes=...` lines appear in logcat tagged WarpTerminalModel
#   * `sgr_apply_count` > 0 in NativeBridge.terminalSgrSummary() output
#   * `dcs_hook_count` > 0 after injecting a hex-encoded DCS Preexec frame
#   * `dcs_error_count` == 0 (no malformed payloads)
#   * Screenshot captured for visual evidence
#
# Logcat tags consumed:
#   WarpTerminalModel — Rust streaming parser (sgr_color, dcs_hook, …)
#   WarpTerminal      — Java side (TERM_INJECT_RAW, summary)
#
# Usage:
#   ./tools/scripts/test-ansi-color.sh R5CX10VFFBA
#
# Outputs:
#   .omc/m3-artifacts/M3-S05-result.json
#   /tmp/m3-s05-color-test.png  (screenshot)
#   /tmp/m3-s05-logcat.txt      (filtered logcat)

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
APK="$REPO_ROOT/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="dev.warp.mobile"
ACTIVITY=".MainActivity"
ARTIFACT_DIR="$REPO_ROOT/.omc/m3-artifacts"
RESULT_JSON="$ARTIFACT_DIR/M3-S05-result.json"
SCREENSHOT="/tmp/m3-s05-color-test.png"
LOGCAT_OUT="/tmp/m3-s05-logcat.txt"

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

# Clear logcat to keep our results focused.
"${ADB[@]}" logcat -c

# ── Sub-test 1: SGR color codes ────────────────────────────────────────────
#
# Send: ESC[31mRED ESC[32mGREEN ESC[34mBLUE ESC[0m end
# Bytes (hex): 1b 5b 33 31 6d 52 45 44 1b 5b 33 32 6d 47 52 45 45 4e 1b 5b 33 34 6d 42 4c 55 45 1b 5b 30 6d 20 65 6e 64
# Base64: G1szMW1SRUQbWzMybUdSRUVOG1szNG1CTFVFG1swbSBlbmQ=

echo "=== sub-test 1: SGR color codes ===" >&2
SGR_BYTES_B64=$(python3 -c "import base64; print(base64.b64encode(b'\x1b[31mRED\x1b[32mGREEN\x1b[34mBLUE\x1b[0m end').decode())")
echo "    SGR bytes_b64=$SGR_BYTES_B64" >&2
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_INJECT_RAW \
    -p "$PACKAGE" \
    --es cmd_id "sgr_test" \
    --es bytes_b64 "$SGR_BYTES_B64" \
    > /dev/null
sleep 1

# ── Sub-test 2: DCS hex Preexec hook ───────────────────────────────────────
#
# Build a real DCS frame matching zsh_body.sh:90 wire format:
#   ESC P $ d <hex> 0x9c
# Payload: {"hook":"Preexec","value":{"command":"ls -la /system"}}

echo "=== sub-test 2: DCS hex Preexec hook ===" >&2
DCS_BYTES_B64=$(python3 -c "
import base64
json_payload = b'{\"hook\":\"Preexec\",\"value\":{\"command\":\"ls -la /system\"}}'
hex_body = ''.join(f'{b:02x}' for b in json_payload).encode()
frame = b'\x1bP\$d' + hex_body + b'\x9c'
print(base64.b64encode(frame).decode())
")
echo "    DCS bytes_b64=$DCS_BYTES_B64" >&2
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_INJECT_RAW \
    -p "$PACKAGE" \
    --es cmd_id "dcs_test" \
    --es bytes_b64 "$DCS_BYTES_B64" \
    > /dev/null
sleep 1

# ── Sub-test 3: DCS hex CommandFinished hook ───────────────────────────────
#
# Payload: {"hook":"CommandFinished","value":{"exit_code":0,"next_block_id":"precmd-42-1"}}

echo "=== sub-test 3: DCS hex CommandFinished hook ===" >&2
CMDFIN_BYTES_B64=$(python3 -c "
import base64
json_payload = b'{\"hook\":\"CommandFinished\",\"value\":{\"exit_code\":0,\"next_block_id\":\"precmd-42-1\"}}'
hex_body = ''.join(f'{b:02x}' for b in json_payload).encode()
frame = b'\x1bP\$d' + hex_body + b'\x9c'
print(base64.b64encode(frame).decode())
")
"${ADB[@]}" shell am broadcast \
    -a dev.warp.mobile.TERM_INJECT_RAW \
    -p "$PACKAGE" \
    --es cmd_id "cmdfin_test" \
    --es bytes_b64 "$CMDFIN_BYTES_B64" \
    > /dev/null
sleep 1

# ── Capture screenshot ─────────────────────────────────────────────────────
echo "=== capture screenshot ===" >&2
"${ADB[@]}" exec-out screencap -p > "$SCREENSHOT"
SHOT_BYTES=$(wc -c < "$SCREENSHOT" | tr -d ' ')
echo "    screenshot bytes=$SHOT_BYTES path=$SCREENSHOT" >&2

# ── Pull logcat ────────────────────────────────────────────────────────────
echo "=== pull logcat ===" >&2
"${ADB[@]}" logcat -d > "$LOGCAT_OUT" 2>&1 || true

# Filter for our tags. Keep both WarpTerminalModel (Rust parser logs) and
# WarpTerminal (Java side TERM_INJECT_RAW echoes).
SGR_LOG_LINES=$(grep -c "sgr_color\|TERM_INJECT_RAW" "$LOGCAT_OUT" || true)
DCS_LOG_LINES=$(grep -c "dcs_hook" "$LOGCAT_OUT" || true)
DCS_ERR_LINES=$(grep -c "DCS .* aborted\|DCS .* failed" "$LOGCAT_OUT" || true)

echo "    sgr_color lines: $SGR_LOG_LINES" >&2
echo "    dcs_hook  lines: $DCS_LOG_LINES" >&2
echo "    dcs_error lines: $DCS_ERR_LINES" >&2

# Extract terminalSgrSummary snippet from the logcat for evidence.
SUMMARY_LINE=$(grep "TERM_INJECT_RAW summary=" "$LOGCAT_OUT" | tail -1 || true)
echo "    last summary: $SUMMARY_LINE" >&2

# Parse final SGR/DCS counts from the summary.
if [[ -n "$SUMMARY_LINE" ]]; then
    SGR_COUNT=$(echo "$SUMMARY_LINE" | sed -nE 's/.*sgr_apply_count=([0-9]+).*/\1/p')
    HOOK_COUNT=$(echo "$SUMMARY_LINE" | sed -nE 's/.*dcs_hook_count=([0-9]+).*/\1/p')
    ERR_COUNT=$(echo "$SUMMARY_LINE" | sed -nE 's/.*dcs_error_count=([0-9]+).*/\1/p')
    CUR_FG=$(echo "$SUMMARY_LINE" | sed -nE 's/.*cur_fg=(0x[0-9A-Fa-f]+).*/\1/p')
else
    SGR_COUNT="missing"
    HOOK_COUNT="missing"
    ERR_COUNT="missing"
    CUR_FG="missing"
fi

# ── Gate ───────────────────────────────────────────────────────────────────

GATE_PASS="false"
if [[ -n "$SGR_COUNT" && "$SGR_COUNT" != "missing" && "$SGR_COUNT" -ge 4 \
   && -n "$HOOK_COUNT" && "$HOOK_COUNT" != "missing" && "$HOOK_COUNT" -ge 2 \
   && "$ERR_COUNT" == "0" ]]; then
    GATE_PASS="true"
fi

# ── Result JSON ────────────────────────────────────────────────────────────

cat > "$RESULT_JSON" <<EOF
{
  "story": "M3-S05",
  "device_serial": "$SERIAL",
  "subtests": {
    "sgr_color_injection": {
      "sgr_apply_count": "$SGR_COUNT",
      "expected": ">=4 (RED/GREEN/BLUE/reset)",
      "logcat_sgr_lines": $SGR_LOG_LINES
    },
    "dcs_preexec_hook": {
      "dcs_hook_count": "$HOOK_COUNT",
      "expected": ">=2 (Preexec + CommandFinished)",
      "logcat_dcs_lines": $DCS_LOG_LINES
    },
    "dcs_error_free": {
      "dcs_error_count": "$ERR_COUNT",
      "expected": "0",
      "logcat_err_lines": $DCS_ERR_LINES
    }
  },
  "evidence": {
    "screenshot": "$SCREENSHOT",
    "screenshot_bytes": $SHOT_BYTES,
    "logcat": "$LOGCAT_OUT",
    "summary_line": "${SUMMARY_LINE}",
    "cur_fg_after": "$CUR_FG"
  },
  "gate": {
    "overall_pass": $GATE_PASS,
    "criteria": "sgr_apply_count >= 4 AND dcs_hook_count >= 2 AND dcs_error_count == 0"
  }
}
EOF

echo "" >&2
echo "=== M3-S05 result (gate: $GATE_PASS) ===" >&2
cat "$RESULT_JSON" >&2

if [[ "$GATE_PASS" != "true" ]]; then
    echo "" >&2
    echo "=== last 30 logcat lines ===" >&2
    tail -30 "$LOGCAT_OUT" >&2
    exit 1
fi
