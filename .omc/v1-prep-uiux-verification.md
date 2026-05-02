# v1-prep UI/UX Verification — Honest Status (2026-05-02)

User-driven full UI/UX verification surfaced **3 ship blockers** that
aren't fixable as carry-over polish. Pre-existing issues that all
prior "device-verified" device-test runs side-stepped because they
used `--ez terminal_mode true` + simulation broadcasts, never the
plain launcher path that real users will take.

This document is the honest accounting. The 35-commit v1-prep arc
landed real infrastructure (CI, release script, APK shrink, license
metadata, etc.) — but **the actual end-user experience cannot ship
v1.0 until these 3 issues are resolved**.

---

## §1. The 3 Ship Blockers

### Blocker #1: Default launcher path → magenta surface, no terminal

**Symptom**: User taps the app icon from launcher → MainActivity
launches → SurfaceView shows the Vulkan magenta clear color from
M2-S04 setup → no PTY, no terminal grid, no usable content.

**Root cause**: `MainActivity.kt:468` gates terminal-mode + auto-spawn
behind `intent.getBooleanExtra("terminal_mode", false)`. The launcher's
`<action android:name="android.intent.action.MAIN" />` Intent doesn't
pass any extras → terminal_mode = false → no PTY auto-spawn.

**Evidence**: `/tmp/warp-verify/01-cold-start.png` — full magenta surface
under the action bar. No terminal output. No keyboard. Just magenta.

**Fix scope**: 1 file change. Either always auto-spawn `$PREFIX/bin/zsh`
on MainActivity onCreate when no terminal_cmd is provided, OR show a
"Tap to start terminal" UI on the magenta clear color.

### Blocker #2: Even with `terminal_mode=true`, grid sizing is broken

**Symptom**: Cold-start with `--ez terminal_mode true --es terminal_cmd /system/bin/sh`
+ initial echo input → PTY spawns + outputs visible BUT text is
clipped to a 1-line band at the very top of the screen, partially
hidden under the action bar.

**Root cause**: `terminal_mode requested rows=24 cols=80
font_size_px=32.0 cell=24.0x40.0px` (per logcat). Grid dimensions:
- 80 cols × 24 px/cell = 1920 px wide → exceeds 1080 px screen
- 24 rows × 40 px/cell = 960 px tall → only top ~40% of screen

**Evidence**: `/tmp/warp-verify/04-sh-pty.png` — top edge shows
`:/  $ echo` clipped under "Warp Mobile" action bar. Most of the
screen is black/empty.

**Fix scope**: 1 file change. `MainActivity.kt` lines ~474 should
compute rows/cols from `displayMetrics.widthPixels / cellWidthPx`
instead of hardcoding 80×24. Or use the `grid_cell_h_px` extra
the test drivers already pass.

### Blocker #3: zsh PTY spawn dies within ~10 ms

**Symptom**: Spawning `$PREFIX/bin/zsh` via `PtyManager.spawn` →
`spawn ok` logged → `read loop ended` ~10-13 ms later. zsh died
before producing any output.

**Reproducer** (multiple variants all fail identically):
- args=[] (interactive zsh) → dies in 13 ms
- args=[] + PTY_RESIZE rows=30 cols=80 (sent ~800 ms after spawn) → dies before resize arrives
- terminal_initial_input "echo zsh_alive\n" → dies before write reaches it

**Negative control**: `/system/bin/sh` (mksh) under identical
PtyManager spawn → outputs `:/ $ echo` to PTY → renders.

**Cross-check**: zsh runs FINE under `run-as` with the same env
(PATH, ZDOTDIR, HOME, TERM set) AND fires the M4-S06 DCS hooks
correctly (Bootstrapped + Precmd + CommandFinished, exit 0).

So the zsh BINARY is fine, the .zshenv is fine, the env is fine
— the failure is specific to the PtyManager spawn path under
interactive mode.

**Hypothesis**: zsh's interactive startup reads from stdin and gets
EOF or EIO on the PTY slave for some Bionic-specific reason. Not
fully diagnosed.

**Diagnostic next steps**:
- Run zsh under PTY with `script -F` style strace to capture syscalls
- Compare PTY slave behavior macOS vs Bionic (Linux read() returns 0 on EOF; macOS returns EIO; Bionic should be Linux-like but may differ)
- Try with TIOCSWINSZ baked into spawn_pty (currently called separately as resize)
- Try with explicit `setpgid` and `tcsetpgrp` calls in spawn_pty (zsh may need to be the foreground process group of the PTY)

**Fix scope**: unknown until diagnosed. Could be 1-line fix (add
TIOCSWINSZ in spawn_pty) or could need rethinking the PTY setup.

---

## §2. What WAS Verified UIUX-OK This Iteration

Despite the 3 blockers above, several v1-prep features ARE genuinely
working:

| Feature | Verified | Evidence |
|---|---|---|
| Cold-start (no crash) | ✓ | logcat clean, no FATAL |
| FGS started + receivers registered | ✓ | "WarpTerminalService created" log |
| Vulkan surface created + presenting | ✓ | `attach ok extent=1080x2340 images=5` + `present_ok frame=2..28` |
| Bootstrap zip extracted | ✓ | "sha-pin match (221216544d0b8b3d) — usr/ already current" |
| .zshenv written | ✓ | "writeWarpZshenv: ... already current" |
| apt.conf written | ✓ | "writeAptConfig: ... already current" |
| AccessoryRow renders 18 buttons | ✓ | Screenshots 05/06/07 confirm: ESC/TAB/CTRL/ALT/↑↓←→/14 punctuation/Copy All/Paste/📋/⚙/💡/🤖/🎤 all visible after horizontal scroll |
| Gboard summons + remains usable | ✓ | Screenshot 03/04/05 |
| /system/bin/sh PTY spawn + render | ✓ | Screenshot 04 shows `:/  $ echo` rendered glyphs |
| PtyOutput logging | ✓ | "WarpTerminal:PtyOutput: echo" / ":/ $ " |
| Block aggregator (DCS injection path) | ✓ | M3-S07 driver PASS |
| Reproducible bootstrap zip | ✓ | M4-S08 byte-identical re-verify |
| AGPL license + cargo deny | ✓ | CI green |

---

## §3. Recommended path

**Recommended**: implement Plan A from prior message — fix all 3
blockers before tagging anything. Estimated 1-3 hours of focused
work.

The CI infrastructure, release pipeline, APK shrink, license
metadata, etc. ALL stay valid. Only the launch UX needs the 3 fixes.

**NOT recommended**: shipping `v1.0.0-rc1` with these blockers
present. Any user installing from GitHub Releases would tap the
icon and see magenta. Soak feedback would be 100% "app doesn't
launch / blank screen" — useless signal.

---

## §4. Artifacts

Screenshots in `/tmp/warp-verify/`:

- `01-cold-start.png` — magenta surface (Blocker #1 evidence)
- `02-terminal-mode.png` — terminal_mode=true, but no terminal_cmd → black grid
- `03-zsh-running.png` — zsh attempted, dies within 10ms → black
- `04-sh-pty.png` — sh works, text clipped at top (Blocker #2 evidence)
- `05-row-left.png` / `06-row-mid.png` / `07-row-right.png` — AccessoryRow scroll states confirming all 18 buttons render

---

*Filed by automated /loop iteration 2026-05-02 after user invoked
`/autopilot 全部驗證好` + `你要確定 UIUX 每一個功能都能用` + `不是
一片黑的都不能操作或是殘缺哦`. The earlier "v1.0 ready" status was
optimistic; this re-verification is the corrective.*
