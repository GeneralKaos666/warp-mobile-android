#!/bin/zsh
set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "Usage: $0 <device-serial> [debug|release]" >&2
    exit 1
fi
DEVICE_SERIAL="$1"
VARIANT="${2:-debug}"
SCRIPT_DIR="${0:a:h}"
PROJECT_DIR="${SCRIPT_DIR}/../spikes/symlink-jnilibs"
if [[ "$VARIANT" == "release" ]]; then
    APK_PATH="${PROJECT_DIR}/app/build/outputs/apk/release/app-release.apk"
else
    APK_PATH="${PROJECT_DIR}/app/build/outputs/apk/debug/app-debug.apk"
fi
PKG="dev.warp.symlinktest"
ACTIVITY="${PKG}/.MainActivity"
LOG_TAG="SymlinkExec"
ADB_PATH="/Users/iml1s/Library/Android/sdk/platform-tools/adb"

adb_cmd() {
    "${ADB_PATH}" -s "${DEVICE_SERIAL}" "$@"
}

ANDROID_SDK=$(adb_cmd shell getprop ro.build.version.sdk 2>/dev/null | tr -d '\r' || echo "0")
echo "[symlink-test] device=${DEVICE_SERIAL} sdk=${ANDROID_SDK} variant=${VARIANT}" >&2

echo "[symlink-test] installing APK..." >&2
adb_cmd install -r "${APK_PATH}" >/dev/null 2>&1 || adb_cmd install "${APK_PATH}" >/dev/null 2>&1

adb_cmd shell am force-stop "${PKG}" 2>/dev/null || true
adb_cmd logcat -c 2>/dev/null || true
adb_cmd shell am start -n "${ACTIVITY}" >/dev/null 2>&1

echo "[symlink-test] waiting for result..." >&2
RESULT_LINE=""
COUNT=0
while [[ $COUNT -lt 25 ]]; do
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
    echo '{"device":"'"${DEVICE_SERIAL}"'","android_sdk":'"${ANDROID_SDK}"',"variant":"'"${VARIANT}"'","negative_control_failed":false,"symlink_passed":false,"errno":"no_result_line","passed":false}'
    exit 1
fi

echo "[symlink-test] raw result: ${RESULT_LINE}" >&2

NEG_FAILED=$(echo "$RESULT_LINE" | grep -oE 'negative_control_failed=(true|false)' | cut -d= -f2 || echo "false")
SYMLINK_PASSED=$(echo "$RESULT_LINE" | grep -oE 'symlink_passed=(true|false)' | cut -d= -f2 || echo "false")
EXIT_CODE=$(echo "$RESULT_LINE" | grep -oE 'result_exit=[-0-9]+' | cut -d= -f2 || echo "-1")
STDOUT_TOKEN=$(echo "$RESULT_LINE" | grep -oE 'stdout_token=[^ ]+' | cut -d= -f2 || echo "")
# Extract full errno message using sentinel delimiters (avoids whitespace truncation)
NEG_ERRNO=$(echo "$RESULT_LINE" | sed -n 's/.*NEGATIVE_ERRNO_BEGIN\(.*\)NEGATIVE_ERRNO_END.*/\1/p' || echo "none")
[[ -z "$NEG_ERRNO" ]] && NEG_ERRNO="none"
NEG_ERRNO_NAME=$(echo "$RESULT_LINE" | grep -oE 'negative_errno_name=[^ ]+' | cut -d= -f2 || echo "none")
SYMLINK_ERRNO=$(echo "$RESULT_LINE" | grep -oE 'symlink_errno=[^ ]+' | cut -d= -f2 || echo "none")

# SDK >= 29: W^X enforced; negative control must fail AND symlink must pass.
# SDK < 29: restriction not present; only check symlink.
if [[ "$ANDROID_SDK" -ge 29 ]]; then
    if [[ "$NEG_FAILED" == "true" && "$SYMLINK_PASSED" == "true" ]]; then
        PASSED="true"
    else
        PASSED="false"
    fi
else
    PASSED="$SYMLINK_PASSED"
fi

JSON='{"device":"'"${DEVICE_SERIAL}"'","android_sdk":'"${ANDROID_SDK}"',"variant":"'"${VARIANT}"'","negative_control_failed":'"${NEG_FAILED}"',"negative_errno":"'"${NEG_ERRNO}"'","negative_errno_name":"'"${NEG_ERRNO_NAME}"'","symlink_passed":'"${SYMLINK_PASSED}"',"symlink_errno":"'"${SYMLINK_ERRNO}"'","exit_code":'"${EXIT_CODE}"',"stdout_token":"'"${STDOUT_TOKEN}"'","passed":'"${PASSED}"'}'
echo "$JSON"

if [[ "$PASSED" == "true" ]]; then
    exit 0
else
    exit 1
fi
