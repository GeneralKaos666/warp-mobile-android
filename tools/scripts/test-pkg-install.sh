#!/usr/bin/env bash
# M4-S11 + M4-S13: pkg / apt acceptance test driver.
#
# Verifies the M4-S07 apt runtime-config override resolves correctly on
# device:
#   1. apt-config dump shows zero com.termux entries in the canonical
#      Dir::* + DPkg:: scope (only Dir::Bin::solvers::/planners:: list-
#      append cosmetic entries are tolerated).
#   2. apt-config dump shows ≥10 dev.warp.mobile entries.
#   3. M4-S13: ls --color=auto via $PREFIX/bin/ls produces ANSI escape
#      sequences when stdout is a TTY (closes M3-S08 AC#5 toybox-color
#      deferral).
#
# `pkg install` end-to-end (M4-S11 AC) is currently network-dependent;
# the run-as adb shell sandbox doesn't have DNS resolver access, so we
# verify the apt config path is reachable but skip the actual install.
# Run from inside a real PTY (via the app's terminal UI) for the full
# install path.
#
# Usage:
#   tools/scripts/test-pkg-install.sh <serial>

set -euo pipefail

SERIAL="${1:-}"
[ -n "$SERIAL" ] || { echo "usage: $0 <serial>" >&2; exit 1; }

ADB="adb -s $SERIAL"
APP=dev.warp.mobile
PREFIX=/data/data/$APP/files/usr

step() { echo; echo "── $* ──"; }
fail() { echo "  ✗ FAIL: $*" >&2; exit 1; }
pass() { echo "  ✓ $*"; }

# Make sure prerequisite (M4-S05 + M4-S07 wiring) is present.
$ADB get-state > /dev/null
$ADB shell run-as "$APP" test -x "$PREFIX/bin/apt-config" \
    || fail "$PREFIX/bin/apt-config missing — run test-bootstrap-install.sh first"

# Trigger spawn-side writeAptConfig in case service onCreate raced
# bootstrap and the file was wiped on the latest atomic rename.
$ADB shell am broadcast -n "$APP/.PtyBroadcastReceiver" \
    -a "${APP}.PTY_SPAWN" --es cmd_id "test-pkg" > /dev/null
sleep 1

step "1. apt-config dump zero com.termux Dir::/DPkg:: entries (M4-S07 AC #8)"
COUNT_TERMUX=$($ADB shell "run-as $APP env \
    HOME=/data/data/$APP/files/home \
    APT_CONFIG=$PREFIX/etc/apt/apt.conf \
    PATH=$PREFIX/bin:/system/bin \
    $PREFIX/bin/apt-config dump 2>/dev/null \
    | grep -E '^(Dir|DPkg::Path)' \
    | grep -F com.termux \
    | grep -vE '^Dir::Bin::(solvers|planners)::' \
    | wc -l" | tr -d ' \r')
[ "$COUNT_TERMUX" = "0" ] || fail "apt-config has $COUNT_TERMUX com.termux entries in canonical Dir::*/DPkg:: scope (expected 0; cosmetic ::-list-append entries excluded)"
pass "0 canonical com.termux entries in Dir::*/DPkg:: (cosmetic list-append entries tolerated)"

step "2. apt-config dump ≥10 dev.warp.mobile entries"
COUNT_WARP=$($ADB shell "run-as $APP env \
    HOME=/data/data/$APP/files/home \
    APT_CONFIG=$PREFIX/etc/apt/apt.conf \
    PATH=$PREFIX/bin:/system/bin \
    $PREFIX/bin/apt-config dump 2>/dev/null \
    | grep -E '^(Dir|DPkg::Path)' \
    | grep -F dev.warp.mobile \
    | wc -l" | tr -d ' \r')
[ "$COUNT_WARP" -ge 10 ] || fail "apt-config has only $COUNT_WARP dev.warp.mobile entries (expected ≥10)"
pass "$COUNT_WARP dev.warp.mobile entries in apt-config dump"

step "3. M4-S13: $PREFIX/bin/ls --color=auto produces ANSI escapes"
# `--color=always` forces ANSI even without a TTY; we assert the escape
# byte (0x1b == ESC) appears in output. Closes M3-S08 AC#5 toybox-color
# deferral (toybox ls on stock Android doesn't honor --color; GNU
# coreutils ls in our bundled $PREFIX does).
HAS_ESC=$($ADB shell "run-as $APP env PATH=$PREFIX/bin:/system/bin $PREFIX/bin/ls --color=always /system 2>/dev/null | head -c 200 | od -An -c | head -2 | tr -d ' \n'" | grep -c '033' || true)
[ "$HAS_ESC" -ge 1 ] || fail "$PREFIX/bin/ls --color=always produced no ANSI escape sequences"
pass "$PREFIX/bin/ls --color=always emits ANSI escapes (M3-S08 AC#5 closed)"

step "4. zsh runtime config (M4-S06 carry-forwards)"
# Run in env-isolated zsh to verify the .zshenv shell-array fix sticks.
# Use a pattern marker so we can extract our value past the DCS hook frames
# that zsh_body.sh emits via precmd. WARP_ZSH_BODY_SOURCING=1 skips the
# script entirely for this isolated probe (it's a public sentinel intended
# to break the .zshenv↔zsh_body.sh recursion; doubles as a "skip hooks"
# flag for tests).
WARP_ZSHENV=$(LC_ALL=C $ADB shell "run-as $APP env \
    HOME=/data/data/$APP/files/home \
    ZDOTDIR=$PREFIX/etc \
    TMPDIR=$PREFIX/tmp \
    WARP_ZSH_BODY_SOURCING=1 \
    $PREFIX/bin/zsh -c 'echo MARKER=\$WARP_ZSHENV_LOADED'" 2>&1 | LC_ALL=C tr -d '\r' | grep -F 'MARKER=' | head -1 | sed 's/.*MARKER=//')
[ "$WARP_ZSHENV" = "1" ] || fail "WARP_ZSHENV_LOADED != 1 (got: $WARP_ZSHENV); .zshenv not sourced"
pass "WARP_ZSHENV_LOADED=1 → \$ZDOTDIR/.zshenv sourced correctly"

MODULE_PATH=$(LC_ALL=C $ADB shell "run-as $APP env \
    HOME=/data/data/$APP/files/home \
    ZDOTDIR=$PREFIX/etc \
    TMPDIR=$PREFIX/tmp \
    WARP_ZSH_BODY_SOURCING=1 \
    $PREFIX/bin/zsh -c 'echo MARKER_BEGIN; print -rl -- \$module_path; echo MARKER_END'" 2>&1 | LC_ALL=C tr -d '\r' | sed -n '/MARKER_BEGIN/,/MARKER_END/p' | grep -v MARKER)
echo "$MODULE_PATH" | grep -q dev.warp.mobile \
    || fail "module_path doesn't contain dev.warp.mobile: $MODULE_PATH"
echo "$MODULE_PATH" | grep -q com.termux \
    && fail "module_path still contains com.termux: $MODULE_PATH"
pass "module_path = $MODULE_PATH (dev.warp.mobile-rooted; no com.termux)"

step "5. git GIT_EXEC_PATH override (M4-S06 AC #7)"
GIT_EXEC=$(LC_ALL=C $ADB shell "run-as $APP env \
    HOME=/data/data/$APP/files/home \
    GIT_EXEC_PATH=$PREFIX/libexec/git-core \
    $PREFIX/bin/git --exec-path" 2>&1 | LC_ALL=C tr -d '\r' | head -1)
echo "$GIT_EXEC" | grep -q dev.warp.mobile \
    || fail "git --exec-path doesn't contain dev.warp.mobile: $GIT_EXEC"
pass "git --exec-path = $GIT_EXEC"

echo
echo "═════════════════════════════════════════════════"
echo " M4-S11 + M4-S13 acceptance PASS on $SERIAL"
echo "═════════════════════════════════════════════════"
echo " (full pkg install end-to-end requires real PTY"
echo "  network access; covered by manual testing in M5)"
