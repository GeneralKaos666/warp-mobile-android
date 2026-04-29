#!/bin/zsh
set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <device-serial>" >&2
    exit 1
fi
DEVICE_SERIAL="$1"
SCRIPT_DIR="${0:a:h}"
PROJECT_DIR="${SCRIPT_DIR}/../spikes/symlink-jnilibs"
APK_PATH="${PROJECT_DIR}/app/build/outputs/apk/debug/app-debug.apk"
PKG="dev.warp.symlinktest"
ACTIVITY="${PKG}/.MainActivity"
LOG_TAG="SymlinkExec"
ADB_PATH="/Users/iml1s/Library/Android/sdk/platform-tools/adb"

adb_cmd() {
    "${ADB_PATH}" -s "${DEVICE_SERIAL}" "$@"
}

# Get Android SDK version
ANDROID_SDK=$(adb_cmd shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r' || echo "0")

echo "[symlink-test] device=${DEVICE_SERIAL} sdk=${ANDROID_SDK}" >&2

# Install APK
echo "[symlink-test] installing APK..." >&2
adb_cmd install -r "${APK_PATH}" >/dev/null 2>&1 || adb_cmd install "${APK_PATH}" >/dev/null 2>&1

# Clear logcat
adb_cmd logcat -c 2>/dev/null || true

# Launch activity
adb_cmd shell am start -n "${ACTIVITY}" >/dev/null 2>&1

# Read logcat for up to 8 seconds, looking for RESULT line
echo "[symlink-test] waiting for result..." >&2
RESULT_LINE=""
COUNT=0
while [[ $COUNT -lt 8 ]]; do
    RAW=$(adb_cmd logcat -d -s "${LOG_TAG}:I" 2>/dev/null || true)
    RESULT_LINE=$(echo "$RAW" | grep "RESULT:" | tail -1 || true)
    if [[ -n "$RESULT_LINE" ]]; then
        break
    fi
    COUNT=$(( COUNT + 1 ))
    sleep 1
done

if [[ -z "$RESULT_LINE" ]]; then
    echo "[symlink-test] no RESULT line found, dumping logcat:" >&2
    adb_cmd logcat -d -s "${LOG_TAG}:I" >&2 || true
    echo '{"device":"'"${DEVICE_SERIAL}"'","android_sdk":'"${ANDROID_SDK}"',"exit_code":-99,"stdout_token":"","errno":"no_result_line","passed":false}'
    exit 1
fi

echo "[symlink-test] raw result: ${RESULT_LINE}" >&2

# Parse fields
EXIT_CODE=$(echo "$RESULT_LINE" | grep -oE 'result_exit=[-0-9]+' | cut -d= -f2 || echo "-1")
STDOUT_TOKEN=$(echo "$RESULT_LINE" | grep -oE 'stdout_token=[^ ]+' | cut -d= -f2 || echo "")
ERRNO_RAW=$(echo "$RESULT_LINE" | grep -oE 'errno=[^ ]+' | cut -d= -f2 || echo "null")
PASSED=$(echo "$RESULT_LINE" | grep -oE 'passed=(true|false)' | cut -d= -f2 || echo "false")

if [[ "$ERRNO_RAW" == "null" ]]; then
    ERRNO_JSON="null"
else
    ERRNO_JSON="\"${ERRNO_RAW}\""
fi

JSON='{"device":"'"${DEVICE_SERIAL}"'","android_sdk":'"${ANDROID_SDK}"',"exit_code":'"${EXIT_CODE}"',"stdout_token":"'"${STDOUT_TOKEN}"'","errno":'"${ERRNO_JSON}"',"passed":'"${PASSED}"'}'
echo "$JSON"

if [[ "$PASSED" == "true" ]]; then
    exit 0
else
    exit 1
fi
