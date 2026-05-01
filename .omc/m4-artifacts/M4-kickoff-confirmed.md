# M4 Kickoff 確認報告

**日期**：2026-05-01 (M4 milestone 正式開始)
**PRD scaffold base**：`9c62a75` (M4 prd.json 15-story scaffold landed at 9c62a75)
**Plan reference**：`.omc/plans/ralplan-warp-on-mobile.md` §6 M4 (lines 442-450 for ACs + lines 548-560 for implementation table)
**前置 milestones**：
- M0 close-out CONDITIONAL GO @ commit `24a2c1c` (Vulkan surface recreate p95 < 200ms on Adreno 6xx+)
- M1 close-out CONDITIONAL GO @ commit `f7feb3f` (10/10 stories PASS — PTY/FGS pipeline on S24 Ultra)
- M2 close-out CONDITIONAL GO @ commit `0506c35` (12/14 stories CODEX_PASS — warpui::platform::android backend; M2-S13 low-end user-deferred)
- M3 close-out CONDITIONAL GO @ commit `8ec75c8` (12/12 stories PASS — warp_terminal_mobile_facade real impl; DCS/Block pipeline; 60fps scroll; APK 7.4MB; 27 codex rounds total)

---

## 1. Entry Criteria Satisfied

M4 入場條件確認：

| 條件 | 狀態 | 證據 |
|---|---|---|
| M3 CONDITIONAL GO @ `8ec75c8` | **PASS** | `.omc/m3-artifacts/M3-go-no-go.md` §6 verdict — 12/12 stories CODEX_PASS. All 5 Plan §6 M3 ACs satisfied on flagship pathway (AC#1 glyph_quads=995; AC#2 p95=13ms/peak_fps=144; AC#3 block_count=3/exit_codes=[0,0,1]; AC#4 release 7.4MB; AC#5 3 conflicts/10 commits Pre-mortem C NOT TRIPPED). |
| PTY→facade→renderer pipeline ready (M1+M2+M3) | **PASS** | End-to-end pipeline verified: `WarpTerminalService.kt` PTY → `NativeBridge.terminalInputBytes` JNI → `warp_terminal_mobile_facade::render.rs` adapter → `warpui::platform::android::Window::push_frame`. M3-S04 warp-src `71f5c73`, M3-S08 dynamic_grid `6dedd95`. |
| warp_terminal_mobile_facade real impl (M3-S02..S07) | **PASS** | 7 modules (lib, terminal, blocks, ai, app_context, feature_flag, ssh_noop) + render.rs + extracted `app_terminal::` sub-tree (ansi/dcs_hooks + model/block + model/blocks). All compiled Android `aarch64-linux-android` via cargo ndk. |
| zsh_body.sh APK asset in place (M3-S06) | **PASS** | `assets/warp/zsh_body.sh` in APK (65K, `M3-S10-result.json:top_10_contributors[rank=4]`). Readable from PTY context. Hook execution deferred; M4-S06 closes this by spawning `$PREFIX/bin/zsh`. |
| APK size headroom for bootstrap zip (M3-S10) | **PASS** | Release APK 7.4MB (90.7% under 80MB gate). **Two distinct gates** (codex M4-S01 round-1 math fix): (a) Single APK ≤80MB → 7.4MB APK + bundled-bootstrap scenario fits up to 50MB zip = 57.4MB total, **22.6MB headroom under 80MB**; (b) Combined APK + auxiliary bootstrap-zip ≤120MB → if APK stays at 7.4MB and bootstrap is auxiliary download, 50MB zip = 57.4MB combined, **62.6MB headroom under 120MB**. Either gate strategy passes. **Canonical strategy** (per ralplan §6 M3 AC#4 literal text + §7 Death-pit #1 below): zip is auxiliary F-Droid asset → APK stays small (7.4MB+5MB delta = ~12.5MB). |
| cargo test -p warp-mobile-android-host PASS | **PASS** | 45/45 tests (M3 final state — M1 baseline 3 + M2 additions + M3-S05 DCS parser + M3-S09 scroll + M3-S11 emoji smoke). |
| Block model + DCS hook pipeline (M3-S07) | **PASS** | `block_count=3; commands=[ls,whoami,false]; exit_codes=[0,0,1]; dcs_error_count=0` via synthetic injection on S24 Ultra `R5CX10VFFBA`. Upstream `d943f1c`. |

### §1a — Device Matrix at M4 Start

**Device matrix** (unchanged from M3; user directive 「先跳過便宜手機」 2026-04-30 remains in effect):

| Serial | 機型 | SoC / GPU | API | 角色 |
|---|---|---|---|---|
| `R5CX10VFFBA` | Galaxy S24 Ultra | Snapdragon 8 Gen 3 / Adreno 750 | API 36 | **Primary flagship — M4 P0 gate device** |
| `RFCNC0WNT9H` | Galaxy S21+ | Snapdragon 888 / Adreno 660 | API 31 | Mid-tier — optional/deferred; available, not mandated |
| `RFCY71LAFYE` | Galaxy S25 | Snapdragon 8 Elite / Adreno 750 | API 36 | Secondary flagship — supplementary; available |
| `25c027b4...` | Samsung Note 9 | Snapdragon 845 / Adreno 630 | SDK 29 | **Below minSdk 31 baseline — NOT used** |

**Termux build environment note (CRITICAL)**: M4-S03 bootstrap zip build requires a **Linux/Docker environment** (`termux-packages/scripts/build-bootstraps.sh` uses Docker internally). This Mac dev machine (`/Users/setsuna-new/...`) **cannot build the bootstrap zip natively**. See §7 Death-pit #2 for full mitigation. All other M4 stories (APK packaging, JNI extraction, pkg UX, F-Droid metadata) are buildable on this machine.

---

## 2. Architecture State at M4 Start (post-M3)

### 2.1 Layer Stack

```
L3: Termux runtime ($PREFIX layout)        ← M4 NEW — not yet present
L2: warp_terminal_mobile_facade            ← M3 real impl; M4 adds pkg.rs
    ├── app_terminal::* (extracted)         ← M3-S03/S05/S07
    ├── render.rs (PTY → push_frame)        ← M3-S04
    └── blocks.rs (Block aggregation)       ← M3-S07
L2a: warp_terminal (clean, untouched)      ← D1.5-hybrid invariant; M4 no-touch
L1: warpui::platform::android (M2 + M3 dynamic_grid)
L0: PTY/FGS (WarpTerminalService + PtyManager)  ← M1 carry-forward intact
```

### 2.2 Post-M3 codebase snapshot

```
android/app/src/main/java/dev/warp/mobile/
├── MainActivity.kt              M3-S11 edits: doc URL fix:284; ime_mode WindowInsetsControllerCompat:447-468; stale comments removed
├── NativeBridge.kt              M3 adds: terminalInputBytes + terminalBlocksDump + setScrollOffset + renderPushFrameDynamic + renderInitDynamicGrid
├── WarpTerminalService.kt       M1 carry-forward — M4-S06 changes spawnPty default from /system/bin/sh to $PREFIX/bin/zsh
├── PtyManager.kt                M1 carry-forward
├── PtyBroadcastReceiver.kt      M1 carry-forward
├── CaptureFrameReceiver.kt      M2-S05
├── ImeSimulationReceiver.kt     M2-S10
└── TouchSimulationReceiver.kt   M2-S11

crates/android-host/src/
├── lib.rs                       ~48 JNI exports — M4 adds bootstrapInstall + (pkg progress channel if needed)
├── pty.rs                       M1 baseline unchanged
├── dynamic_grid.rs              M3-S08 mirror (Option C divergence — M4-S12 Option D resolves)
├── terminal_model.rs            M3-S04/S07 mirror (Option C divergence — M4-S12 Option D resolves)
├── font_render.rs               M2-S07 mirror (Option C divergence — M4-S12 Option D resolves)
├── static_grid.rs               M2-S08 mirror (Option C divergence — M4-S12 Option D resolves)
├── ime.rs                       M2-S10 mirror (Option C divergence — M4-S12 Option D resolves)
└── input.rs                     M2-S11 mirror (Option C divergence — M4-S12 Option D resolves)

warp-src/crates/warp_terminal_mobile_facade/src/    (warp-src @ 94bf0ff after M3-S09)
├── lib.rs                  Session::spawn/write/read public API
├── terminal.rs             Session lifecycle impl
├── blocks.rs               BlockList + terminalBlocksDump JNI bridge
├── ai.rs                   AI provider stub (all Unsupported)
├── app_context.rs          AppContext mobile shim
├── feature_flag.rs         FeatureFlag shim (terminal=true, ai=false, blocks=true)
├── ssh_noop.rs             SSH provider (all Unsupported)
├── render.rs               PTY bytes → terminal model → Window::push_frame adapter
└── app_terminal/           Extraction of app::terminal::model::*
    ├── mod.rs
    ├── model/block.rs      extracted from app/src/terminal/model/block.rs:286
    ├── model/blocks.rs     extracted from app/src/terminal/model/blocks.rs:239
    └── ansi/dcs_hooks.rs   extracted from app/src/terminal/model/ansi/dcs_hooks.rs:1,14,407,487

tools/scripts/              (all take <serial> as first arg)
├── [M1] test-pty-{reattach,resize}.sh; test-fgs-clean-kill.sh; test-30min-idle-stress.sh
├── [M2] test-render-scene.sh; test-frame-capture{,-stress}.sh; test-font-render.sh;
│        test-static-grid.sh; test-rotation-stress.sh; test-ime.sh; test-touch.sh; test-window-insets.sh
├── [M3] test-ansi-color.sh; test-dynamic-grid.sh; test-block-model.sh; test-scroll.sh
└── [M4 — to be written] test-bootstrap-install.sh; test-pkg-install.sh
```

### 2.3 M4 新增 (S02-S14 完成後預期)

```
ImL1s/termux-packages (fork, new repo, branch warp-mobile/main)
├── scripts/setup-warp-prefix.sh           $PREFIX retargeting
└── scripts/build-bootstraps.sh            produces bootstrap-aarch64.zip (Linux/Docker required)

android/app/src/main/assets/warp/
├── bootstrap/bootstrap-aarch64.zip        ~30-50MB compressed
├── bootstrap/version.json                 {sha256, build_date, package_list, prefix}
└── zsh_body.sh                            already present (M3-S06)

android/app/src/main/cpp/
└── bootstrap_install.c                    atomic extraction: usr.tmp/ → usr/; kill-recovery

crates/warp_terminal_mobile_facade/src/
└── pkg.rs                                 NEW — pkg/apt subprocess wrapper; PkgProgress events

metadata/
└── dev.warp.mobile.yml                    F-Droid build recipe; bootstrap zip hash-pin

fastlane/metadata/android/en-US/           F-Droid GUI metadata

warp-src/crates/ (if M4-S12 succeeds)
└── warp_terminal_mobile_facade_android_link/   Option D shared-rlib (resolves 6 mirror dups)
```

---

## 3. M4 Work-Domain Table

per `.omc/plans/ralplan-warp-on-mobile.md` lines 548-560 (§6 M4 implementation table):

| # | 工作域 | ralplan §6 M4 table row | 主要 file/path | Phase | Stories |
|---|---|---|---|---|---|
| 1 | Fork + $PREFIX retargeting | Row #1 — fork termux-packages; search-replace $PREFIX `/data/data/com.termux/files/usr` → `/data/data/dev.warp.mobile/files/usr` | `ImL1s/termux-packages` fork; `scripts/setup-warp-prefix.sh` | Bootstrap Build | S02 |
| 2 | Bootstrap zip build | Row #2 — `build-bootstraps.sh` with package list bash/zsh/coreutils/findutils/apt/pkg/git | `bootstrap-aarch64.zip` (~30-50MB compressed) | Bootstrap Build | S03 |
| 3 | APK asset packaging + extraction | Row #3 — atomic-extract JNI shim; Gradle asset staging | `android/app/src/main/cpp/bootstrap_install.c` (NEW); `android/app/src/main/assets/warp/bootstrap/` | Asset Packaging / Runtime Integration | S04, S05 |
| 4 | PTY spawn → $PREFIX/bin/zsh | Extension of M3-S06 deferral closure | `WarpTerminalService.kt` spawnPty env; zsh_body.sh hooks | Runtime Integration | S06 |
| 5 | pkg install UX | Row #4 — subprocess apt; progress channel | `crates/warp_terminal_mobile_facade/src/pkg.rs` (NEW) | pkg UX | S07 |
| 6 | Reproducibility + F-Droid | Row #5 — reproducible-build manifest; bootstrap zip as auxiliary asset | `metadata/dev.warp.mobile.yml`; `fastlane/metadata/android/en-US/` | F-Droid | S08, S09 |
| 7 | M4 acceptance device tests | ralplan §6 M4 verification step | `tools/scripts/test-bootstrap-install.sh`; `test-pkg-install.sh` | Acceptance | S10, S11 |
| 8 | M3 carry-overs | M3-S11 Option D; M3-S08 AC#5/#6 deferrals; emoji raster | carry-forward items from M3 | M3 Carry-overs | S12, S13, S14 |

---

## 4. ralplan §6 M4 Acceptance Criteria (5 ACs with Quantified Gates)

per `.omc/plans/ralplan-warp-on-mobile.md` lines 442-450:

| # | Acceptance Criterion | 量化門檻 | 對應 Story | 說明 |
|---|---|---|---|---|
| 1 | APK ships a bootstrap zip (~30-50MB compressed) for `aarch64`; on first launch extracts to `/data/data/dev.warp.mobile/files/usr` atomically (no partial state on kill mid-extract); subsequent launches are instant | first-launch extraction completes without crash; `files/usr/` populated; **`version.json` SHA256 matches expected bootstrap zip hash (`sha256_match=true`)**; first-launch extraction time recorded (target <30s on S24U flagship; budget gate <60s); subsequent launch <2s with no re-extraction; kill-mid-extract → clean recovery on next launch | S04 + S05 + S10 | Device: S24 Ultra `R5CX10VFFBA`; driver `tools/scripts/test-bootstrap-install.sh` (codex M4-S01 round-1 fix: SHA256 verification gate added) |
| 2 | `pkg install git python` from within the app installs the forked termux-packages builds with our prefix; `git --version` and `python3 --version` execute correctly post-install | `git --version` returns 2.x; `python3 --version` returns 3.x; install exits 0; git clone of a small public repo succeeds | S07 + S11 | ralplan §6 M4 verification step: "Fresh-install app → run `pkg install git && git clone ...` end-to-end without error" |
| 3 | Bootstrap zip is reproducible: rebuilding the fork at the same commit produces a byte-identical zip (within deterministic-tooling allowances) | `sha256(build1) == sha256(build2)` for two consecutive builds at same commit; `SOURCE_DATE_EPOCH` fixed; tar entry ordering deterministic | S08 | Deferred to CI if Docker build env unavailable on dev machine (see §7 Death-pit #2) |
| 4 | Upgrade path: app v1.0 → v1.1 with bootstrap-zip-content changes → installed packages migrate via reinstall manifest, no user data loss | **Reinstall manifest gate (codex M4-S01 round-1 fix)**: (a) install v1.0 + run `pkg install git python` (records to `$PREFIX/var/lib/dpkg/status` as packages.installed manifest); (b) sideload v1.1 APK with new bootstrap zip (different sha256 in `version.json`); (c) onLaunch detects sha256 mismatch + triggers re-extract usr/; (d) replay manifest: re-run `pkg install` for each package; (e) verify packages restored AND `$PREFIX/home/<user>/` preserved (test files unchanged via sha256 round-trip) AND `$PREFIX/var/lib/apt/lists/` reset (cache invalidation correct) | S05 + S10 + new S10b sub-test | This AC is NOT covered by sub-test 3+4 of S10 (those are kill-recovery + corrupt-recovery — different from upgrade migration). New sub-test required: `test-bootstrap-upgrade.sh` |
| 5 | F-Droid metadata + recipe handles the bootstrap zip as part of reproducible build (or as a separate-source asset with hash-pin) | `metadata/dev.warp.mobile.yml` passes `fdroid readmeta` validation; bootstrap zip SHA256 present in recipe; License field = AGPL-3.0-only | S09 | F-Droid validate run |

---

## 5. 15-Story Ledger with Phase Assignment

| Story | 標題 | Phase | Owner Hint | 狀態 |
|---|---|---|---|---|
| **M4-S01** | M4 kickoff doc + Plan section update + M3 carry-overs absorption note | **Bootstrap Build** | executor (sonnet) | **THIS DOC** |
| **M4-S02** | Fork termux-packages with $PREFIX retargeting | **Bootstrap Build** | executor (opus) | 待開始 |
| **M4-S03** | Bootstrap zip build (bash + zsh + coreutils + findutils + apt + pkg + git) | **Bootstrap Build** | executor (opus) | 待開始 |
| **M4-S04** | APK asset packaging for bootstrap zip + version-pin file | **Asset Packaging** | executor (sonnet) | 待開始 |
| **M4-S05** | First-launch atomic extraction (JNI shim with usr.tmp → usr rename + kill-mid-extract recovery) | **Runtime Integration** | executor (opus) | 待開始 |
| **M4-S06** | PTY spawn uses $PREFIX/bin/zsh instead of /system/bin/sh (closes M3-S06 hook execution deferral) | **Runtime Integration** | executor (sonnet) | 待開始 |
| **M4-S07** | pkg install UX with progress + cancellation (subprocess to forked apt) | **pkg UX** | executor (opus) | 待開始 |
| **M4-S08** | Bootstrap zip reproducibility (byte-identical rebuilds) | **F-Droid** | executor (sonnet) | 待開始 |
| **M4-S09** | F-Droid metadata + reproducible-build manifest | **F-Droid** | executor (sonnet) | 待開始 |
| **M4-S10** | Acceptance #1 device test — first-launch extraction + subsequent-launch instant + kill-recovery | **Acceptance** | executor (opus) | 待開始 |
| **M4-S11** | Acceptance #2 device test — pkg install git python end-to-end on S24U | **Acceptance** | executor (opus) | 待開始 |
| **M4-S12** | M3 carry-over: Option D shared-rlib API split (resolves 6 cross-workspace dups) | **M3 Carry-overs** | executor (opus) | 待開始 |
| **M4-S13** | M3-S08 deferral closure: colored ls -la /system with GNU coreutils ls --color=auto | **M3 Carry-overs** | executor (sonnet) | 待開始 |
| **M4-S14** | M3-S11 carry-forward: live emoji raster smoke (closes SubpixelMask emoji classifier-only deferral) | **M3 Carry-overs** | executor (sonnet) | 待開始 |
| **M4-S15** | M4 close-out integration document | **Close-out** | executor (sonnet) | 待開始 |

**Phase 說明**:
- **Bootstrap Build** (S01-S03): kickoff doc + termux-packages fork + bootstrap zip artifact. S02 (fork + retarget) must precede S03 (build zip). S03 is **Linux/Docker required** — see Death-pit #2.
- **Asset Packaging** (S04): Gradle task stages the zip into APK. Depends on S03 zip artifact (or a placeholder zip for APK pipeline smoke). Can begin APK pipeline structure before S03 zip is available.
- **Runtime Integration** (S05-S06): S05 atomic extraction JNI shim; S06 switches PTY spawn to `$PREFIX/bin/zsh`. S06 depends on S05 (zsh must be installed before spawning). Sequential: S05 → S06.
- **pkg UX** (S07): `pkg.rs` subprocess wrapper. Depends on S05+S06 (working $PREFIX with apt available). Can be developed offline with a mock apt subprocess.
- **F-Droid** (S08-S09): S08 reproducibility and S09 metadata. S08 requires S03 (real zip for hash comparison). S09 can begin earlier as metadata structure is independent of zip.
- **Acceptance** (S10-S11): device integration tests. S10 depends on S04+S05; S11 depends on S07+S06. Both gate on S24 Ultra `R5CX10VFFBA`.
- **M3 Carry-overs** (S12-S14): S12 (Option D rlib) is architectural; S13 (colored ls) and S14 (emoji) both require S06 (working zsh in $PREFIX). S13+S14 can proceed in parallel after S06 closes.
- **Close-out** (S15): after all other stories close. Mirrors M3-go-no-go.md template.

---

## 6. Architecture Invariants

per ralplan Amendment 2 + D1.5-hybrid constraint:

### 6.1 D1.5-hybrid constraint (unchanged from M3)

```
warp_terminal_mobile_facade       ← M4 adds pkg.rs; architecture unchanged
    ├── depends on warp_terminal  ← clean Layer 2a; NOT modified in M4
    ├── depends on warpui         ← via platform::android::Window::push_frame
    └── provides Session + AppContext + FeatureFlag + SSH-noop + pkg (NEW)

android-host/ (cdylib JNI root)   ← M4 adds bootstrapInstall JNI export
    └── 6 Option C mirror files   ← carry-forward; M4-S12 Option D resolves if successful
```

**Invariant**: `warp_terminal → warpui` Cargo edge stays. `warp_terminal` crate is NOT modified by M4. M4 introduces L3 (Termux runtime) below the existing layers — it is a deployment concern, not a crate dependency change.

### 6.2 Termux as L3 layer (NEW in M4)

M4 introduces Termux runtime as Layer 3 — the on-device $PREFIX that provides the POSIX environment. Key layout:

```
/data/data/dev.warp.mobile/files/
├── usr/                              $PREFIX — installed by M4-S05 extraction
│   ├── bin/zsh                      spawned by M4-S06 (replaces /system/bin/sh)
│   ├── bin/bash
│   ├── bin/ls                       GNU coreutils ls (closes M3-S08 AC#5 toybox-color)
│   ├── bin/git
│   ├── bin/python3
│   ├── bin/pkg, bin/apt             package management (M4-S07)
│   ├── lib/                         shared libraries
│   ├── etc/                         config; ZDOTDIR for zsh_body.sh (M4-S06)
│   │   └── (zsh_body.sh hooks here after M4-S06 wiring)
│   └── var/lib/apt/                 apt lists
├── usr.tmp/                         atomic extraction staging (M4-S05)
│   └── (removed after successful extraction)
├── home/                            user home (preserved on upgrade)
└── warp/zsh_body.sh                 already present (M3-S06 extraction target)
```

**Cargo.lock zero churn constraint**: M4 introduction of L3 is purely on-device deployment. No new Cargo deps on `warp_terminal` or `warpui` upstream edges. `pkg.rs` may use `std::process::Command` for subprocess; no new crates expected.

### 6.3 Option D shared-rlib (M4-S12 — if successful)

```
warp_terminal_mobile_facade_android_link (NEW rlib in warp-src/crates/)
    ├── exposes warpui::platform::android::* types as public API
    └── consumed by both warpui (canonical) and android-host (mirror)
        → removes 6 Option C mirror files (~5800 LOC sync debt)
```

**Risk**: if Option D introduces Metal transitive deps that break `cargo ndk check` (the original M2-S04 blocker pattern), document failure + escalate to milestone scope review. Do NOT force Option D past this blocker — Option C divergence is an acceptable permanent architectural artifact of D1.5-hybrid per M3-S11 rationale.

---

## 7. Death-Pit Top-3

per `.omc/plans/ralplan-warp-on-mobile.md` §Pre-mortem + M4 scope analysis:

### 死坑 #1 — Bootstrap zip 30-50MB inflation past 80MB APK combined budget

**描述**: ralplan §6 M4 #1 specifies bootstrap zip ~30-50MB compressed. M3-S10 measured release APK at 7.4MB — leaving **73MB headroom** before the 80MB single-APK gate. However, ralplan §6 M3 AC#4 actually defines the gate as: release APK ≤80MB **excluding** bootstrap zip; combined APK + bootstrap zip ≤120MB total. So the real gate is:

- Single APK (release .apk file): must stay ≤80MB (currently 7.4MB — 72.6MB margin)
- **Delivery strategy decision required**: Is the bootstrap zip **bundled inside the APK** (inflating APK to 37-57MB, still under 80MB but consuming most of the margin) OR shipped as a **separate F-Droid auxiliary asset** (APK stays 7.4MB; bootstrap zip is a separate download)?

**量化預警**:
- If zip is bundled in APK: 7.4MB + 30-50MB zip = 37-57MB APK → PASS on single-APK ≤80MB gate
- If zip reaches 75MB+ uncompressed content (many packages added): APK could exceed 80MB gate
- Per ralplan §6 M3 AC#4 literal text: bootstrap zip is a "separate F-Droid auxiliary asset" — this is the **canonical delivery choice** (APK stays small; zip is aux download). But M3-S10 `bootstrap_already_in_apk:true` means M3 was measuring the bundled scenario.
- **M4-S04 must formally commit to delivery strategy** and document in M4-kickoff §9 or M4-S04 result.json before bootstrap zip build begins.

**緩解**:
1. Default to separate F-Droid auxiliary asset delivery per ralplan canonical text — APK stays at 7.4MB + ~3-5MB growth from new code; bootstrap zip is aux download
2. If bundled approach chosen: enforce a hard limit on package list (7 packages in ralplan table row #2 only; no scope creep); measure compressed size before committing
3. M4-S04 documents the chosen strategy in `version.json` and M4-S04 result.json

### 死坑 #2 — termux-packages Docker build env not available on Mac dev machine

**描述**: `termux-packages/scripts/build-bootstraps.sh` requires Docker to run the Termux cross-compilation container (`termux/termux-docker`). This Mac dev machine (`/Users/setsuna-new/...`) does NOT have Docker installed as a standard part of the development environment. The docker build runs a full cross-compilation toolchain (NDK r25c, Bionic libc headers, package build scripts) that cannot execute on macOS without Docker or a Linux VM.

**Concrete implication for M4-S03**: M4-S03 (bootstrap zip build) is **not executable on this machine without environment setup**. M4-S03 executor MUST:
1. Check if Docker is installed: `docker --version`
2. If NOT available: document the build env requirement in M4-S03 result.json (`build_env_available: false`); defer artifact production to CI or a Linux session
3. Do NOT claim "bootstrap-aarch64.zip produced" without actually producing and hashing the artifact

**M4-S02** (fork + $PREFIX retargeting) is purely git operations and script edits — macOS compatible. M4-S02 can complete on this machine.

**緩解**:
1. M4-S03 executor first runs `docker --version` and documents availability
2. If Docker unavailable: M4-S03 produces the fork, documents the build script invocation, records the expected package list, and marks AC as "deferred to Linux CI session" — Codex review dispatched on this documentation deliverable
3. CI path: GitHub Actions with `ubuntu-latest` runner + Docker can run `build-bootstraps.sh`; set up `.github/workflows/build-bootstrap.yml` as part of M4-S03 deliverable
4. cherry-pick velocity for `ImL1s/termux-packages` fork is a **separate budget** from `warp-src` — upstream `termux/termux-packages` commits at a different cadence than `warpdotdev/Warp`. Record conflict count separately in M4 cherry-pick dry-run (M4-S15 close-out).

### 死坑 #3 — Cherry-pick velocity now hits termux-packages fork too

**描述**: M3 managed cherry-pick velocity for `warp-src` (warp upstream at `warpdotdev/Warp`). M4 introduces a second upstream dependency: `termux/termux-packages`. The fork (`ImL1s/termux-packages`) must track upstream termux-packages for security updates (package CVEs, toolchain updates). Unlike `warpdotdev/Warp` (one team's controlled commits), `termux/termux-packages` is a large community project with hundreds of packages updating continuously.

**量化預警**:
- `termux/termux-packages` has ~4000+ package recipes; our fork only needs ~7 packages for M4-S03
- Commit frequency: upstream averages 50-100 commits/day (package updates)
- The $PREFIX retargeting in M4-S02 (search-replace across build scripts) will conflict with every upstream commit that touches the same files
- Cherry-pick strategy must be **selective**: only cherry-pick commits touching our 7-package subset + core build scripts; not the entire stream
- Separate budget tracking required from `warp-src` cherry-pick budget

**緩解**:
1. M4-S02 executor documents which core scripts were modified (setup-warp-prefix.sh + build-bootstraps.sh + PREFIX constants); creates a "conflict footprint" file at M4 kickoff
2. Upstream tracking strategy: `git merge upstream/master -- <specific packages>` rather than full cherry-pick; only absorb commits touching bash/zsh/coreutils/findutils/apt/pkg/git recipes
3. Record separate `termux_packages_cherry_pick_conflict_count` in M4-S15 close-out alongside `warp_src_cherry_pick_conflict_count`
4. If upstream termux-packages tracking exceeds 1hr/month maintenance budget: pin to a known stable tag (e.g., after a security release) rather than tracking HEAD

---

## 8. M3 → M4 Carry-Overs

per `.omc/m3-artifacts/M3-go-no-go.md` §5 + prd.json M4-S01 ACs:

### 8.1 M3-S06 hook execution deferral (zsh PATH) → M4-S06

**狀態**: M3-S06 ships `zsh_body.sh` as APK asset (65K at `assets/warp/zsh_body.sh`). Actual DCS preexec + CommandFinished hook firing in a live zsh session deferred because no zsh on stock Android (`/system/bin/sh` is mksh). **M4-S06 closes this** by switching `WarpTerminalService.spawnPty` from `/system/bin/sh` to `$PREFIX/bin/zsh` after M4-S05 extraction lands `$PREFIX/bin/zsh`.

**Verification**: M4-S06 acceptance requires `echo $0` returns `zsh` in PTY; zsh_body.sh hooks fire (DCS preexec/CommandFinished observed in logcat); M3 Acceptance #3 Block model pipeline closes end-to-end in a real zsh session (not synthetic injection).

### 8.2 M3-S08 toybox-color deferral → M4-S13

**狀態**: Android stock `/system/bin/ls` (toybox) does NOT emit ANSI colors even with `--color=always`. M3-S08 verified per-cell rendering via synthetic SGR injection only. **M4-S13 closes this** by running `ls -la --color=auto /system` via Termux GNU coreutils `ls` (installed in M4-S03 bootstrap as part of `coreutils-gnu`). Verification: SGR escapes visible in PTY output (`od -c` showing `\033[` sequences); on-screen rendering shows blue dirs + green executables per ralplan §6 M3 AC#1 literal text.

### 8.3 M3-S08 Linux pixel-similarity gate deferral → M4 (or M5 if reference render not feasible)

**狀態**: M3-S08 deferred the ≥95% pixel-similarity gate to Linux reference render because producing a matching font/cell-size reference was out of scope. **M4-S13** will produce a screenshot of colored `ls -la /system` output. Pixel-similarity against a Linux golden PNG remains aspirational; if the reference render tooling is not available in M4, this gate migrates to M5 (Mobile UX layer). M4-S13 must produce the screenshot artifact and document whether the pixel-similarity sub-gate was evaluated.

### 8.4 M3-S11 Option D shared-rlib API split → M4-S12

**狀態**: 6 cross-workspace mirror files (~5800 LOC sync debt) documented as Option C divergence in M3-S11 (`M3-S11-result.json:ac_status.ac1_unify_4_m2_era_duplicates.rationale`). The architectural fix (Option D: new `warp_terminal_mobile_facade_android_link` rlib in warp-src/crates/ that exposes `warpui::platform::android::*` types publicly) is the correct M4 refactor. **M4-S12** implements Option D or confirms permanent Option C if Option D introduces Metal transitive deps (per M2-S04 blocker pattern). If M4-S12 is blocked: add to M5 scope as a formal architectural artifact with named "D1.5-hybrid permanent divergence" label.

| Main file | warp-src canonical | LOC (main) | Option C since |
|---|---|---|---|
| `crates/android-host/src/font_render.rs` | `warp-src/crates/warpui/src/platform/android/font.rs` | 651 | M2-S07 / M3-S11 |
| `crates/android-host/src/static_grid.rs` | `warp-src/crates/warpui/src/platform/android/static_grid.rs` | 1182 | M2-S08 / M3-S11 |
| `crates/android-host/src/ime.rs` | `warp-src/crates/warpui/src/platform/android/ime.rs` | 516 | M2-S10 / M3-S11 |
| `crates/android-host/src/input.rs` | `warp-src/crates/warpui/src/platform/android/input.rs` | 506 | M2-S11 / M3-S11 |
| `crates/android-host/src/terminal_model.rs` | `warp-src/.../facade/src/render.rs` + `warpui/dynamic_grid.rs` | 1551 | M3-S04/S07 |
| `crates/android-host/src/dynamic_grid.rs` | `warp-src/crates/warpui/src/platform/android/dynamic_grid.rs` | ~1500 | M3-S08 |

**Total sync debt**: ~5906 LOC across 6 files. Every M4+ story that modifies warpui/platform/android/ or facade/render.rs must manually sync the mirror file. This debt grows at ~200-400 LOC/milestone.

### 8.5 M3-S11 SubpixelMask emoji raster smoke → M4-S14

**狀態**: M3-S11 fixed the emoji classifier path in `crates/android-host/src/lib.rs:1208-1340` (classifier-only; device blit path deferred). **M4-S14 closes this** by running `echo 🎉` through the PTY with the M4 Termux runtime providing a real bash/zsh, then capturing a screenshot to verify the emoji glyph renders correctly (not tofu). Requires M4-S06 (working $PREFIX/bin/zsh) and M4-S05 (bootstrap installed).

---

## 9. Architecture State at M4 Start — $PREFIX Layout Being Introduced

The defining architectural change in M4 is the introduction of the Termux $PREFIX on-device. The full $PREFIX tree after M4-S03 bootstrap build + M4-S05 extraction:

```
/data/data/dev.warp.mobile/files/usr/   ($PREFIX; atomic extraction from APK asset)
├── bin/
│   ├── zsh              → WarpTerminalService default shell (M4-S06)
│   ├── bash
│   ├── ls               → GNU coreutils ls --color=auto (closes M3-S08 AC#5)
│   ├── find
│   ├── git
│   ├── python3
│   ├── apt
│   └── pkg
├── lib/
│   ├── libz.so.1        → shared libs required by bootstrap packages
│   └── (per-package .so files)
├── etc/
│   ├── (zsh startup files; ZDOTDIR → here per M4-S06 environment wiring)
│   └── apt/             → apt sources.list pointing to ImL1s/termux-packages
├── var/
│   └── lib/apt/lists/   → package index cache
├── share/
│   └── termux/          → termux-specific helper scripts
└── version.json         → {sha256, build_date, package_list, prefix} — version pin
```

**Key difference from Termux app**: Our `$PREFIX` is `/data/data/dev.warp.mobile/files/usr` NOT `/data/data/com.termux/files/usr`. All M4-S02 $PREFIX retargeting work (search-replace + audit) must be validated end-to-end by M4-S05 extraction and M4-S06 PTY spawn before claiming success.

---

## 10. Verifier SOP (Codex + M3-S12 lesson: lead-dispatched only)

`prd.json` `verifierConfig.critic = "codex"` — 每個 worker deliverable 必須通過 Codex review 後才能將 story 標記為 `passes:true`。

### SOP:

1. Worker 完成 deliverable，commit + push to main (warp-src 變更 push to `ImL1s/warp:warp-mobile/m0-facade`; termux-packages 變更 push to `ImL1s/termux-packages:warp-mobile/main`)
2. **Lead (not worker)** reads artifact + dispatches Codex review：write prompt to `/tmp/codex-M4-S0x-review.md`, then `omc ask codex --prompt "$(< /tmp/codex-M4-S0x-review.md)"` (avoid zsh `()` parse errors)
3. Background dispatch via `run_in_background: true`; verdict read from `.omc/artifacts/ask/codex-*.md`
4. REVISE → follow-on task fix (new commit, NOT amend); PASS → lead marks story `passes:true` in prd.json
5. **M3 SOP reaffirmed (27 codex rounds, 0 worker self-dispatches)**: Worker MUST NOT dispatch Codex review before the fix is committed. The canonical M4 sequence: Worker delivers → Lead reads → Lead dispatches Codex → Codex verdict → Lead flips `passes:true`.
6. **Exhaustive sweep discipline (from M2-S14 / M3-S01 memory note)**: When codex finds a class of error, grep ALL instances first; fix all in one pass; re-grep pre-commit. Avoids 4-5 round codex cycles seen in M2-S14 and M3-S01.
7. **M3-S03 Pre-mortem C lesson**: If a story hits a structural mismatch (not just budget overrun), stop and report to lead — do NOT push harder or paper over. Plan amendment is faster than 10 failed executor rounds.
8. **M4-S03 specific**: If Docker build env unavailable, the deliverable is the documentation of the build env requirement + CI workflow, not the zip artifact. Codex review on documentation deliverable is valid.

---

## 11. Execution Decision — Bootstrap Build First

M4 正式開始。M4-S01 (this doc) 完成後，next story 按 priority 順序：

### Bootstrap Build phase dispatch order (S02 → S03, sequential; S04 can overlap with S03):

1. **M4-S02** (fork + $PREFIX retargeting) — Bootstrap Build P0. Fork `termux/termux-packages` to `ImL1s/termux-packages` on branch `warp-mobile/main`; search-replace $PREFIX; produce `scripts/setup-warp-prefix.sh`; document conflict footprint. Owner: executor (opus).
   - 先決條件：macOS git + GitHub CLI available + **`gh auth status` returns authenticated user with public_repo scope** (codex M4-S01 round-1 finding #3: forking is an irreversible "going public" operation per CLAUDE.md user governance — pre-authorized by user via `/autopilot M4` directive, but worker MUST verify `gh auth status` before fork; if unauthenticated, surface as user-blocker requesting `gh auth login` and STOP); no Docker required for M4-S02
   - 完成門檻：`ImL1s/termux-packages` fork exists on GitHub (verify via `gh repo view ImL1s/termux-packages`); `scripts/setup-warp-prefix.sh` runs on macOS; `grep -r 'com.termux' -- '*.sh' | wc -l` returns 0 in our fork
   - **Blocker path** (gh auth unavailable): write `M4-S02-blocker.md` documenting the auth requirement; pause autopilot; user runs `gh auth login`; M4-S02 resumes

2. **M4-S03** (bootstrap zip build) — Bootstrap Build P0 after S02. Executor MUST first check Docker availability: `docker --version`. If unavailable: produce CI workflow at `.github/workflows/build-bootstrap.yml` targeting `ubuntu-latest`; document expected zip artifact; Codex review on CI + documentation. If Docker available: run `build-bootstraps.sh` with 7-package list; produce `bootstrap-aarch64.zip` with SHA256. Owner: executor (opus).
   - **HARD CONSTRAINT**: Do NOT claim zip produced without `sha256sum bootstrap-aarch64.zip` evidence
   - 完成門檻 (no Docker): CI workflow exists; expected artifact path documented; `docker --version` failure recorded as evidence
   - 完成門檻 (Docker available): `bootstrap-aarch64.zip` exists; `sha256sum` hash recorded; size 30-50MB validated

3. **M4-S04** (APK asset packaging) — Asset Packaging. Gradle copy task stages bootstrap zip into APK; version-pin file created. Can begin staging the Gradle task structure before S03 zip is available (use placeholder). Owner: executor (sonnet).
   - 先決條件：S03 zip artifact OR placeholder; S03 delivery strategy decision documented
   - 完成門檻：`unzip -l app-debug.apk | grep bootstrap-aarch64.zip` passes; `version.json` in APK assets

### Runtime Integration phase (S05 → S06, sequential):

4. **M4-S05** (atomic extraction JNI shim) — depends on S04 (zip in APK). Implements `bootstrap_install.c`: extract to `usr.tmp/` → verify SHA256 → rename → version-pin. Kill-mid-extract recovery. Owner: executor (opus).
   - 完成門檻：S24 Ultra `R5CX10VFFBA` first-launch shows extraction; kill-recovery test passes; subsequent launch instant

5. **M4-S06** (PTY spawn → $PREFIX/bin/zsh) — depends on S05 (zsh in $PREFIX). Changes `WarpTerminalService.spawnPty` default; wires `ZDOTDIR`/`PATH`/`HOME` env. Closes M3-S06 hook execution deferral. Owner: executor (sonnet).
   - 完成門檻：`echo $0` returns `zsh`; M3-S06 DCS hooks fire in logcat

### pkg UX phase (S07, after S06):

6. **M4-S07** (pkg install UX) — depends on S06 (working $PREFIX/bin/zsh + apt). Owner: executor (opus).
   - 完成門檻：`pkg install python` happy path; progress events to UI; cancellation works

### F-Droid + Reproducibility phase (S08-S09, overlapping with S05-S07):

7. **M4-S08** (reproducibility) — depends on S03 (real zip). Can proceed in parallel with S05-S07. Owner: executor (sonnet).
8. **M4-S09** (F-Droid metadata) — independent of runtime; can begin after S01. Owner: executor (sonnet).

### Acceptance phase (S10-S11, after Runtime Integration):

9. **M4-S10** (bootstrap install acceptance) — depends on S04+S05. S24 Ultra device test.
10. **M4-S11** (pkg install acceptance) — depends on S07+S06. S24 Ultra device test.

### M3 Carry-overs + Close-out:

11. **M4-S12** (Option D rlib) — architectural refactor; can proceed independently after S01. If blocked, scope to M5.
12. **M4-S13** (colored ls) + **M4-S14** (emoji smoke) — both depend on S06 (working zsh in $PREFIX). Can parallel after S06.
13. **M4-S15** (close-out doc) — after all other stories close.

**Critical dependency sequence**: S02 → S03 → S04 → S05 → S06 → S07 → S11. S10 branches from S05. S13/S14 branch from S06.

**M4 timeline estimate per ralplan §6 M4**: 10-16 weeks (solo). Bootstrap Build + Runtime Integration 4-6 weeks (Docker build env is the wildcard). pkg UX 2-3 weeks. F-Droid + Reproducibility 2-3 weeks. Acceptance + M3 carry-overs + Close-out 2-4 weeks.

**Honest flag**: M4 is the first milestone where **external tooling constraints** (Docker build env, termux-packages CI infrastructure) are a genuine blocking risk independent of code complexity. The Mac dev machine can execute all Rust/Kotlin/Gradle work but cannot produce the bootstrap zip without Docker. This is documented as Death-pit #2 and must be resolved in M4-S03 before the Acceptance phase can begin.

---

*撰寫人：executor@M4-S01 (Claude Sonnet 4.6)*
*下一步：Codex review dispatch for M4-S01 (per prd.json M4-S01 AC#5 + §10 SOP). On PASS: lead marks M4-S01.passes:true in prd.json and dispatches M4-S02 to executor (opus).*

---

## 12. M4-S03 Status — Docker-deferred CI workflow (2026-05-01)

**M4-S03 state**: Docker-deferred CI workflow path complete. Bootstrap zip NOT produced locally (Docker/colima absent on Mac dev machine — verified `docker --version` → command not found).

**Deliverables committed**:
- `.github/workflows/build-bootstrap.yml` — GitHub Actions workflow targeting `ubuntu-latest`; encodes `./scripts/run-docker.sh ./scripts/build-bootstraps.sh --architectures aarch64` invocation against `ImL1s/termux-packages:warp-mobile/main`; verifies 0 `com.termux` in build files; size-gates 30-50MB; emits SHA256 + `bootstrap-metadata.json`.
- `tools/scripts/m4-bootstrap-packages.txt` — 7 packages: bash, zsh, coreutils-gnu, findutils, apt, pkg, git (per ralplan §6 M4 #2).
- `.omc/m4-artifacts/M4-S03-result.json` — AC status: ac1+ac3 = DEFERRED_TO_CI; ac2+ac4 = PASS. Honest deferral.

**Trigger command for lead** (after M4-S02 fork is on GitHub):
```bash
gh workflow run build-bootstrap.yml -f arch=aarch64
gh run list --workflow=build-bootstrap.yml --limit=1
# gh run download <run-id> -n bootstrap-aarch64
```

**Next gate**: Codex review on CI workflow + documentation (dispatched by lead per §10 SOP). On PASS: lead marks M4-S03.passes:true in prd.json and dispatches M4-S04 (APK asset packaging — can begin with placeholder zip while CI zip is in-flight).
