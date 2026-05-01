#!/usr/bin/env bash
# M4-S10: bootstrap install acceptance test driver.
#
# Runs the M4-S05 atomic-extraction acceptance scenarios end-to-end on a
# connected Android device. Uses the SAME app data dir + zip layout that
# the production app does — no synthetic test fixtures.
#
# Scenarios covered (per .omc/prd.json M4-S05 acceptance):
#   1. Clean first launch — pin file + usr/ removed; expect status=0,
#      elapsed < 30s flagship target, sha-pin file written.
#   2. Subsequent launch (sha-pin fast path) — expect status=0,
#      elapsed < 2s, "sha-pin match" log.
#   3. Kill-mid-extract recovery — leave usr.tmp/ junk + remove pin;
#      expect bootstrap detects + re-extracts cleanly.
#   4. M4-S05 AC#7 (M4-S03 RUNPATH carry-forward) — env -u
#      LD_LIBRARY_PATH $PREFIX/bin/{zsh,bash,git} --version all return 0
#      (proves patchelf RUNPATH retargeting in M4-S03 build-bootstrap.sh
#      is reflected on the extracted binaries).
#
# Usage:
#   tools/scripts/test-bootstrap-install.sh <serial>
# Example:
#   tools/scripts/test-bootstrap-install.sh R5CX10VFFBA

set -euo pipefail

SERIAL="${1:-}"
if [ -z "$SERIAL" ]; then
    echo "usage: $0 <serial>" >&2
    echo "  example: $0 R5CX10VFFBA" >&2
    exit 1
fi

ADB="adb -s $SERIAL"
APP=dev.warp.mobile
PREFIX=/data/data/$APP/files/usr

step() { echo; echo "── $* ──"; }
fail() { echo "  ✗ FAIL: $*" >&2; exit 1; }
pass() { echo "  ✓ $*"; }

# Sanity: device + app present.
$ADB get-state > /dev/null || fail "device $SERIAL not connected"
$ADB shell pm list packages "$APP" | grep -q "package:$APP" \
    || fail "$APP not installed (run: ./gradlew :app:installDebug)"

step "1. Clean first launch (rm pin + usr/ + usr.tmp/)"
$ADB shell am force-stop "$APP"
$ADB shell run-as "$APP" rm -f files/.bootstrap-version.json
$ADB shell run-as "$APP" rm -rf files/usr files/usr.tmp
$ADB logcat -c
$ADB shell am start -n "$APP/.MainActivity" > /dev/null
# Wait up to 60s for bootstrap to complete (target <30s).
for i in $(seq 1 60); do
    if $ADB logcat -d -s warp.bootstrap | grep -q "M4-S05 bootstrapInstall:"; then
        break
    fi
    sleep 1
done
LOG_LINE=$($ADB logcat -d -s warp.bootstrap | grep -E "M4-S05 bootstrapInstall:" | tail -1)
[ -n "$LOG_LINE" ] || fail "no bootstrapInstall log line within 60s"
ELAPSED_MS=$(echo "$LOG_LINE" | sed -nE 's/.*elapsedMs=([0-9]+).*/\1/p')
STATUS=$(echo "$LOG_LINE" | sed -nE 's/.*status=([0-9]+).*/\1/p')
[ "$STATUS" = "0" ] || fail "bootstrapInstall returned status=$STATUS (expected 0)"
[ -n "$ELAPSED_MS" ] && [ "$ELAPSED_MS" -lt 30000 ] \
    || fail "first launch elapsedMs=$ELAPSED_MS exceeded 30s flagship target"
pass "first launch status=0 elapsedMs=$ELAPSED_MS (target <30000)"

# Verify pin file written + usr/ populated.
$ADB shell run-as "$APP" test -f files/.bootstrap-version.json \
    || fail "pin file not written"
PIN_SHA=$($ADB shell run-as "$APP" cat files/.bootstrap-version.json \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['sha256'])")
pass "pin file sha=${PIN_SHA:0:16}..."
$ADB shell run-as "$APP" test -x files/usr/bin/zsh \
    || fail "$PREFIX/bin/zsh not executable post-extraction"
pass "$PREFIX/bin/zsh present + executable"

step "2. Subsequent launch (sha-pin fast path)"
$ADB shell am force-stop "$APP"
sleep 1
$ADB logcat -c
$ADB shell am start -n "$APP/.MainActivity" > /dev/null
sleep 4
LOG_LINE=$($ADB logcat -d -s warp.bootstrap warp-android-host | grep -E "M4-S05 bootstrapInstall:|sha-pin match" | tail -2)
echo "$LOG_LINE" | grep -q "sha-pin match" \
    || fail "expected 'sha-pin match' log line; got: $LOG_LINE"
ELAPSED_MS=$($ADB logcat -d -s warp.bootstrap | grep -E "M4-S05 bootstrapInstall:" | tail -1 | sed -nE 's/.*elapsedMs=([0-9]+).*/\1/p')
[ "$ELAPSED_MS" -lt 2000 ] \
    || fail "sha-pin fast path elapsedMs=$ELAPSED_MS exceeded 2s gate"
pass "sha-pin fast path elapsedMs=$ELAPSED_MS (target <2000)"

step "3. Kill-mid-extract recovery (junk usr.tmp/ + remove pin)"
$ADB shell am force-stop "$APP"
$ADB shell run-as "$APP" rm -f files/.bootstrap-version.json
$ADB shell "run-as $APP mkdir -p files/usr.tmp"
$ADB shell "run-as $APP touch files/usr.tmp/.partial"
$ADB shell run-as "$APP" test -f files/usr.tmp/.partial \
    || fail "couldn't seed usr.tmp junk"
$ADB logcat -c
$ADB shell am start -n "$APP/.MainActivity" > /dev/null
# Wait for re-extract.
for i in $(seq 1 60); do
    if $ADB logcat -d -s warp.bootstrap | grep -q "M4-S05 bootstrapInstall: status=0"; then
        break
    fi
    sleep 1
done
$ADB shell run-as "$APP" test ! -e files/usr.tmp \
    || fail "usr.tmp/ should have been wiped on recovery"
$ADB shell run-as "$APP" test -x files/usr/bin/zsh \
    || fail "$PREFIX/bin/zsh missing after kill-recovery"
$ADB shell run-as "$APP" test -f files/.bootstrap-version.json \
    || fail "pin file not written after recovery"
pass "kill-recovery: usr.tmp wiped; usr/bin/zsh restored; pin file present"

step "4. M4-S05 AC#7 / M4-S03 RUNPATH carry-forward (LD_LIBRARY_PATH UNSET)"
for bin in zsh bash git; do
    OUT=$($ADB shell "run-as $APP env -u LD_LIBRARY_PATH $PREFIX/bin/$bin --version 2>&1" | head -1 | tr -d '\r')
    [ -n "$OUT" ] || fail "$PREFIX/bin/$bin --version produced no output (probably missing dynamic libs)"
    pass "env -u LD_LIBRARY_PATH $PREFIX/bin/$bin --version → $OUT"
done

echo
echo "═════════════════════════════════════════════════"
echo " M4-S10 acceptance test PASS on $SERIAL"
echo "═════════════════════════════════════════════════"
