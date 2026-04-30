#!/usr/bin/env zsh
# tools/scripts/lib/keep-awake.sh — shared keep-awake helper
#
# Usage:
#   source tools/scripts/lib/keep-awake.sh
#   keep_awake_setup R5CX10VFFBA   # at start of driver
#   keep_awake_start R5CX10VFFBA   # spawn heartbeat
#   # ... test runs ...
#   keep_awake_stop                # in trap / cleanup
#   keep_awake_restore R5CX10VFFBA # restore original settings
#
# Why heartbeat: Samsung S24 Ultra has Knox Secure Folder as a device admin
# (`com.samsung.knox.securefolder`) which enforces lockscreen even after
# `cmd lock_settings set-disabled true`. Knox runs an independent idle
# detector that triggers Bouncer (lock-screen unlock UI) regardless of
# `screen_off_timeout`, `svc power stayon`, or `FLAG_KEEP_SCREEN_ON`.
# A periodic no-op input event resets Knox's idle timer.
#
# Lock-screen settings that we override (and restore on exit):
#   * settings system screen_off_timeout
#   * settings secure lock_screen_lock_after_timeout
#   * settings system aod_mode (Samsung Always-on Display)
#   * svc power stayon

# Globals populated by keep_awake_setup; consumed by keep_awake_restore.
typeset -g KEEPAWAKE_ORIG_SCREEN_OFF=""
typeset -g KEEPAWAKE_ORIG_LOCK_AFTER=""
typeset -g KEEPAWAKE_ORIG_AOD=""
typeset -g KEEPAWAKE_HEARTBEAT_PID=""

# Save current settings + apply max values.
keep_awake_setup() {
    local serial="$1"
    local adb=(adb -s "$serial")

    KEEPAWAKE_ORIG_SCREEN_OFF=$("${adb[@]}" shell settings get system screen_off_timeout 2>/dev/null \
                                | tr -d '\r' || print 60000)
    KEEPAWAKE_ORIG_LOCK_AFTER=$("${adb[@]}" shell settings get secure lock_screen_lock_after_timeout 2>/dev/null \
                                | tr -d '\r' || print 5000)
    KEEPAWAKE_ORIG_AOD=$("${adb[@]}" shell settings get system aod_mode 2>/dev/null \
                                | tr -d '\r' || print 1)

    "${adb[@]}" shell settings put system screen_off_timeout 2147483647 2>&1 >&2 || true
    "${adb[@]}" shell settings put secure lock_screen_lock_after_timeout 2147483647 2>&1 >&2 || true
    "${adb[@]}" shell settings put system aod_mode 0 2>&1 >&2 || true
    "${adb[@]}" shell svc power stayon true 2>&1 >&2 || true
    "${adb[@]}" shell input keyevent KEYCODE_WAKEUP 2>&1 >&2 || true
    "${adb[@]}" shell wm dismiss-keyguard 2>&1 >&2 || true

    echo "=== keep_awake_setup: timeout/lock/aod overridden, originals=${KEEPAWAKE_ORIG_SCREEN_OFF}/${KEEPAWAKE_ORIG_LOCK_AFTER}/${KEEPAWAKE_ORIG_AOD} ===" >&2
}

# Spawn a background heartbeat that re-asserts wake every 20s.
# Defeats Samsung Knox's independent idle detector that re-triggers
# Bouncer regardless of system settings.
keep_awake_start() {
    local serial="$1"
    (
        while true; do
            sleep 20
            adb -s "$serial" shell input keyevent KEYCODE_WAKEUP 2>/dev/null
            adb -s "$serial" shell wm dismiss-keyguard 2>/dev/null
            adb -s "$serial" shell svc power stayon true 2>/dev/null
        done
    ) &
    KEEPAWAKE_HEARTBEAT_PID=$!
    echo "=== keep_awake_start: heartbeat pid=${KEEPAWAKE_HEARTBEAT_PID} (every 20s) ===" >&2
}

# Stop the heartbeat (idempotent).
keep_awake_stop() {
    if [[ -n "${KEEPAWAKE_HEARTBEAT_PID}" ]]; then
        kill "${KEEPAWAKE_HEARTBEAT_PID}" 2>/dev/null || true
        wait "${KEEPAWAKE_HEARTBEAT_PID}" 2>/dev/null || true
        echo "=== keep_awake_stop: heartbeat killed ===" >&2
        KEEPAWAKE_HEARTBEAT_PID=""
    fi
}

# Restore device settings to pre-test values.
keep_awake_restore() {
    local serial="$1"
    local adb=(adb -s "$serial")

    [[ -n "${KEEPAWAKE_ORIG_SCREEN_OFF}" ]] && \
        "${adb[@]}" shell settings put system screen_off_timeout "${KEEPAWAKE_ORIG_SCREEN_OFF}" 2>&1 >&2 || true
    [[ -n "${KEEPAWAKE_ORIG_LOCK_AFTER}" ]] && \
        "${adb[@]}" shell settings put secure lock_screen_lock_after_timeout "${KEEPAWAKE_ORIG_LOCK_AFTER}" 2>&1 >&2 || true
    [[ -n "${KEEPAWAKE_ORIG_AOD}" ]] && \
        "${adb[@]}" shell settings put system aod_mode "${KEEPAWAKE_ORIG_AOD}" 2>&1 >&2 || true
    "${adb[@]}" shell svc power stayon false 2>&1 >&2 || true

    echo "=== keep_awake_restore: settings restored ===" >&2
}
