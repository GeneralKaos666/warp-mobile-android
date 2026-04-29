#!/usr/bin/env zsh
# run-vulkan-spike.sh <device-serial>
# Installs Vulkan spike APK, drives 100 pause/resume cycles via screen rotation,
# parses VulkanSpike logcat for surfaceDestroyed_ts and first_frame_presented_ts,
# outputs CSV + p50/p95/p99 summary.
#
# Usage:
#   ./scripts/run-vulkan-spike.sh R5CX10VFFBA
#
# Requirements:
#   - adb in PATH and device authorized
#   - Device screen must be ON and unlocked (script cannot unlock)
#   - APK already built at android/app/build/outputs/apk/debug/app-debug.apk

set -euo pipefail

SERIAL="${1:?Usage: $0 <device-serial>}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SPIKE_DIR="$(dirname "$SCRIPT_DIR")"
APK="$SPIKE_DIR/android/app/build/outputs/apk/debug/app-debug.apk"
PACKAGE="com.warpmobile.spike"
ACTIVITY=".MainActivity"
CYCLES=100
CYCLE_SLEEP=1.2   # seconds between rotation commands

if [[ ! -f "$APK" ]]; then
    echo "ERROR: APK not found at $APK" >&2
    echo "Build with: cd $SPIKE_DIR/android && gradle assembleDebug" >&2
    exit 1
fi

# Use array to avoid word-splitting on serial that may contain special chars
ADB=(adb -s "$SERIAL")

echo "=== Installing APK on $SERIAL ===" >&2
"${ADB[@]}" install -r "$APK" >&2

echo "=== Launching activity ===" >&2
"${ADB[@]}" shell am start -n "$PACKAGE/$ACTIVITY" >&2
sleep 2

echo "=== Clearing logcat ===" >&2
"${ADB[@]}" logcat -c

echo "=== Starting logcat capture in background ===" >&2
LOGCAT_FILE=$(mktemp /tmp/vulkan-spike-logcat.XXXXXX)
# Quote filter spec to prevent zsh glob expansion of '*:S'
"${ADB[@]}" logcat -v time "VulkanSpike:I" "*:S" > "$LOGCAT_FILE" &
LOGCAT_PID=$!
trap "kill $LOGCAT_PID 2>/dev/null; rm -f $LOGCAT_FILE" EXIT

sleep 1

echo "=== Running $CYCLES rotation cycles ===" >&2
echo "NOTE: Device must be unlocked. Auto-rotation must be enabled." >&2

# Disable auto-rotation so user_rotation setting takes effect
"${ADB[@]}" shell settings put system accelerometer_rotation 0 2>/dev/null || true

for i in $(seq 1 $CYCLES); do
    # Landscape (1) -> portrait (0)
    "${ADB[@]}" shell settings put system user_rotation 1
    sleep $CYCLE_SLEEP
    "${ADB[@]}" shell settings put system user_rotation 0
    sleep $CYCLE_SLEEP
    [[ $((i % 10)) -eq 0 ]] && echo "  cycle $i/$CYCLES" >&2
done

echo "=== Restoring portrait ===" >&2
"${ADB[@]}" shell settings put system user_rotation 0
sleep 0.5

# Stop logcat capture
kill $LOGCAT_PID 2>/dev/null || true
wait $LOGCAT_PID 2>/dev/null || true

echo "=== Parsing results ===" >&2

# Parse paired surfaceDestroyed_ts / first_frame_presented_ts lines.
# Falls back to firstNonStaleFrame_ts for builds without swapchain (pre-B-item builds).
# Log format: MM-DD HH:MM:SS.mmm  PID  PID I VulkanSpike: <key>=<val>
python3 - "$LOGCAT_FILE" "$SERIAL" "$CYCLES" <<'PYEOF'
import sys, re, csv

logfile  = sys.argv[1]
serial   = sys.argv[2]
expected = int(sys.argv[3])

destroyed_ts = None
results = []

with open(logfile) as f:
    for line in f:
        m = re.search(r'surfaceDestroyed_ts=(\d+)', line)
        if m:
            destroyed_ts = int(m.group(1))
            continue
        # Accept both swapchain metric and Choreographer fallback
        m = re.search(r'(?:first_frame_presented_ts|firstNonStaleFrame_ts)=(\d+)', line)
        if m and destroyed_ts is not None:
            frame_ts = int(m.group(1))
            recovery = frame_ts - destroyed_ts
            results.append(recovery)
            destroyed_ts = None

if not results:
    print(f"ERROR: no paired timestamps found in {logfile}", file=sys.stderr)
    print("Check VulkanSpike logcat for:", file=sys.stderr)
    print("  surfaceDestroyed_ts=<ms>", file=sys.stderr)
    print("  first_frame_presented_ts=<ms>  OR  firstNonStaleFrame_ts=<ms>", file=sys.stderr)
    sys.exit(1)

n = len(results)
if n != expected:
    print(f"WARNING: expected {expected} cycles but found {n} paired timestamps — "
          f"some cycles may have been missed or logcat lines dropped", file=sys.stderr)
    if n == 0:
        sys.exit(1)

writer = csv.writer(sys.stdout)
writer.writerow(["device", "cycle", "recovery_ms"])
for i, r in enumerate(results, 1):
    writer.writerow([serial, i, r])

results_sorted = sorted(results)
p50 = results_sorted[int(n * 0.50)]
p95 = results_sorted[int(n * 0.95)]
p99 = results_sorted[int(n * 0.99)] if n >= 100 else results_sorted[-1]
passed = p95 < 200

print(f"# device={serial} count={n} p50={p50}ms p95={p95}ms p99={p99}ms "
      f"passed={passed} (threshold: p95<200ms)",
      file=sys.stderr)
PYEOF
