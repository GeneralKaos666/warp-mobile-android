# M3 Kickoff 確認報告

**日期**：2026-04-30 (M3 milestone 正式開始)
**PRD scaffold base**：`fdece64` (M3 prd.json 12-story scaffold landed at fdece64)
**Plan reference**：`.omc/plans/ralplan-warp-on-mobile.md` §6 M3 (lines 404-412 for ACs + lines 504-518 for implementation table)
**前置 milestones**：
- M0 close-out CONDITIONAL GO @ commit `24a2c1c` (Vulkan surface recreate p95 < 200ms on Adreno 6xx+)
- M1 close-out CONDITIONAL GO @ commit `f7feb3f` (10/10 stories PASS — PTY/FGS pipeline on S24 Ultra)
- M2 close-out CONDITIONAL GO @ commit `0506c35` (12/14 stories CODEX_PASS — warpui::platform::android backend verified; M2-S14 Codex round-3 PASS; only M2-S13 low-end deferred per user choice)

---

## 1. Entry Criteria Satisfied

M3 入場條件確認：

| 條件 | 狀態 | 證據 |
|---|---|---|
| M2 CONDITIONAL GO @ `0506c35` | **PASS** | `.omc/m2-artifacts/M2-go-no-go.md` §6 verdict — 12/14 stories CODEX_PASS. All 5 Plan §6 M2 ACs satisfied on flagship pathway (AC#1 grid p95=9ms; AC#2 swapchain p95=155ms; AC#3 IME Gboard quirk; AC#4 WindowInsets; AC#5 cargo doc). |
| warpui::platform::android backend ready (4 hand-written areas verified) | **PASS** | `warp-src/crates/warpui/src/platform/android/` @ warp-src `d7616e5`: `vulkan.rs` 88,690 bytes (render_scene @ `window.rs:313` + request_frame_capture @ `window.rs:325`); `font.rs` 39,821 bytes (FontDB 15 methods; ASystemFontIterator NDK API 29+ @ `font.rs:169`); `text_layout.rs` 20,017 bytes (TextLayoutSystem 2 methods @ `text_layout.rs:428,471`); `static_grid.rs` 45,408 bytes (11k-instance vkCmdDraw pipeline). |
| M1 PTY plumbing ready (S04+ chain proven) | **PASS** | `.omc/m1-artifacts/M1-go-no-go.md` §6 — M1-S06 delta_ms=26, M1-S08 orphans=0, M1-S09 pwd 4ms at t=30min. `WarpTerminalService.kt` + `PtyManager.kt` + `NativeBridge.kt` M1 carry-forward intact in M2 close state. |
| warp_terminal_mobile_facade scaffold in place (M0-S08) | **PASS (with caveat)** | Scaffolded under **`warp-src/crates/warp_terminal_mobile_facade/`** (NOT main repo) per M0 implementation table task #8. **Already contains `lib.rs`, `terminal.rs`, `blocks.rs`, `ai.rs`** (worker M3-S01 review found this on M0-archeology re-check). `cargo build -p warp_terminal_mobile_facade` currently FAILS through warpui's Metal toolchain build script (matches M0-facade-scaffold.md:18 documented gap; resolves on Android target via cargo ndk). M3-S02 extracts/wires existing scaffolding rather than starting from zero. |
| cargo test -p warp-mobile-android-host PASS | **PASS** | 18/18 tests (M2 final state — M2-S10 host tests 10/10 + M1 baseline 3 + M2-S06 text_layout 2 + M2-S11/S12 additions) per M2-S12 close-out evidence. |
| M2-S13 low-end device gate | **USER CHOICE — DEFERRED** | Per user directive 「先跳過便宜手機」 2026-04-30: Pixel 4a / Galaxy A52s acquisition is NOT a P0 blocker for M3 start. This is an **explicit user decision** (not an overlooked obligation). See §1a below for full rationale. |

### §1a — M2-S13 User Deferral (「先跳過便宜手機」 2026-04-30)

**User directive**: 2026-04-30 — 「直接開始 先跳過便宜手機」 (proceed to M3 without M2-S13 Pixel 4a / A52s gate).

**Framing**: This is a deliberate product decision made by the project owner with full awareness of the tradeoff. It is NOT a forgotten obligation or a process shortcut. Plan Amendment 3 §3 originally listed low-end device validation as a requirement, but the user has chosen to defer it indefinitely with the understanding that M3 proceeds on a **flagship-only verification path**.

**Device matrix for M3**:
- **Primary flagship (P0 mandatory)**: Galaxy S24 Ultra `R5CX10VFFBA` / Adreno 750 / API 36 — all M3 acceptance stories verified on this device
- **Mid-tier (optional / deferred)**: Galaxy S21+ `RFCNC0WNT9H` / Adreno 660 / API 31 — online and connected; per prd.json scope.out "deferred to post-M3 polish; flagship-only for M3"; can be used for supplementary runs at executor discretion but not mandated
- **Secondary flagship (Galaxy S25 `RFCY71LAFYE`)**: Used in M2-S04 codex reproduction; available if needed
- **Low-end (Pixel 4a / Galaxy A52s API 31)**: Remains an **M3 carry-over option** (no longer P0); can be absorbed by M3-S11 or surfaced as M4 carry-over depending on hardware acquisition timing
- **Below-baseline**: Note 9 (serial `25c027b4...`) / SDK 29 — below minSdk 31 baseline per Plan Amendment 3; NOT used in any M3 verification

**M3 close-out impact**: M3 will likely close CONDITIONAL GO (same pattern as M1 and M2) on the device-matrix gap. This is acceptable given the flagship-only directive.

---

## 2. 環境基線 (此台機器)

| 工具 | 版本 | 備注 |
|---|---|---|
| Rust | 1.92.0 (ded5c06cf 2025-12-08) | `rustc --version` 確認 |
| cargo-ndk | 4.1.2 | `cargo ndk --version` 確認 |
| Android NDK | **r28.2 @ `/Users/setsuna-new/development/android-sdk/ndk/28.2.13676358`** (substitute for spec r29) | 同 M2 — AGP 標稱 `android/app/build.gradle:89` ndkVersion `29.0.13113456`；本機僅 r28.2 可用；僅用於 cargo-ndk `check/build`，不走 AGP native compile path |
| Android SDK | `/Users/setsuna-new/development/android-sdk/` | `.envrc:2` 標稱 `~/Library/Android/sdk/`；本機以 shell env override |
| Java | JDK 21 (`~/development/jdk-21.0.6+7/`) | spec 為 OpenJDK 17；M2 Gradle build 驗證 JDK 21 兼容 |
| Rust target | `aarch64-apple-darwin` + `aarch64-linux-android` (installed) | `rustup target list --installed` 確認 |
| warp-src branch | `warp-mobile/m0-facade` @ `d7616e5` | M2 close-out state; M3-S02 may advance this |

**Connected devices (adb devices)**：

| Serial | 機型 | SoC / GPU | API | 角色 |
|---|---|---|---|---|
| `R5CX10VFFBA` | Galaxy S24 Ultra | Snapdragon 8 Gen 3 / Adreno 750 | API 36 | **Primary flagship — M3 P0 gate device** |
| `RFCNC0WNT9H` | Galaxy S21+ | Snapdragon 888 / Adreno 660 | API 31 | Mid-tier — optional/deferred; available, not mandated (per prd.json:23 "deferred to post-M3 polish; flagship-only for M3") |
| `RFCY71LAFYE` | Galaxy S25 | Snapdragon 8 Elite / Adreno 750 | API 36 | Secondary flagship — used in M2-S04 codex repro |
| `25c027b4...` | Samsung Note 9 | Snapdragon 845 / Adreno 630 | SDK 29 | **Below minSdk 31 baseline — NOT used** |

**機器差異記錄**：M0/M1 開發機為 project owner ImL1s (`/Users/iml1s/...`)；M2+ 開發機為本機 setsuna-new (`/Users/setsuna-new/...`)。NDK/SDK/Java 版本差異同 M2-kickoff-confirmed.md §2 所記。

---

## 3. M3 Scope per ralplan §6 M3 + Layer 2b D1.5-hybrid Recap

### Layer 2b 整合概覽

**ralplan §6 M3 lines 504-507** (Layer recap):

> M3 wires the **clean Layer 2a** (`crates/warp_terminal` + `crates/warpui` + `crates/warp_core` + `crates/warp_completer` + `crates/warp_util`) into the **tangled Layer 2b** (`app/src/terminal/...` + `app::ai` + `app::feature_flag` + `app::ssh` + `app::app_context`) via the M0-scaffolded `crates/warp_terminal_mobile_facade`. The facade absorbs the cfg-gates so `warp_terminal` itself stays untouched and `app/` gets only edge-of-dependency cuts.

M3 目標用一句話：**PTY stream (M1) → terminal model (warp_terminal + facade) → renderer (M2 warpui Android backend)** 這個 end-to-end pipeline，使 `ls -la /system` 產生正確著色、line-wrapped 輸出，並且 Block detection 從 DCS hook 運作。

### M3 的 6 個工作域

| 工作域 | ralplan §6 M3 table row | 主要 file/path | Phase |
|---|---|---|---|
| 1. Facade real impl | Row #1 — `Session::spawn/write/read` + AppContext/FeatureFlag/SSH shims | `warp-src/crates/warp_terminal_mobile_facade/src/{lib,terminal,blocks,ai}.rs` (existing M0 scaffold) + new `{app_context,feature_flag,ssh_noop}.rs` | Foundation (S02) |
| 2. cfg-gate app::terminal::* | Row #2 — desktop-only path gates at app crate edges | `warp-src/app/src/terminal/model/session.rs`, `warp-src/app/src/terminal/mod.rs`, `warp-src/app/Cargo.toml`, `warp-src/app/build.rs` | Foundation (S03) |
| 3. Facade → warpui wiring | Row #3 — `render.rs` adapter: PTY bytes → model cells → `Window::push_frame` | `warp-src/crates/warp_terminal_mobile_facade/src/render.rs` (NEW in M3-S04) | Foundation (S04) |
| 4. DCS hook parser | Row #4 — ESC P $ d ... 0x9c parser; **already exists in upstream warp** at `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` (4 anchor refs: lines 1, 14, 407, 487; dispatch at `warp-src/app/src/terminal/model/ansi/mod.rs:771`) | M3-S05 = extract / route through facade, NOT discover from scratch | DCS+Block (S05) |
| 5. zsh_body.sh DCS hook | Row #4 — modified bootstrap script **already exists** at `warp-src/app/assets/bundled/bootstrap/zsh_body.sh` (lines 80, 254, 301 emit hex JSON DCS) | M3-S06 = ship existing asset, NOT write new | DCS+Block (S06) |
| 6. Block model | Row #4 extension — `Block` struct aggregation | **app-layer** model at `warp-src/app/src/terminal/model/block.rs:286` (`Block`) + `warp-src/app/src/terminal/model/blocks.rs:239` (`BlockList`); `warp_terminal` only carries `BlockId`/`BlockIndex` newtypes — S07 wires Block model through facade, no `warp_terminal` modification | DCS+Block (S07) |

### Layer 2b 架構邊界

- `warp_terminal → warpui` Cargo edge **保持不變**（D1.5-hybrid constraint, Amendment 2）
- `warp_terminal_mobile_facade` absorbs all cfg-gates; `warp_terminal` itself stays untouched
- `app/` 只在 edge-of-dependency 點 cut（Pre-mortem C: <500 lines diff threshold）
- M3 **不** 引入 `font-kit`、桌面 `winit`、`wgpu`、proprietary Warp Cloud sync

---

## 4. ralplan §6 M3 Acceptance Criteria (5 ACs with Quantified Gates)

per `.omc/plans/ralplan-warp-on-mobile.md` lines 408-412:

| # | Acceptance Criterion | 量化門檻 | 對應 Story | 說明 |
|---|---|---|---|---|
| 1 | PTY stream feeds M2 renderer via cfg-gated `app::terminal::*` + `crates/warp_terminal` minimal subset; running `ls -la /system` produces **correctly colored, line-wrapped output** (verified vs Linux baseline) | ≥95% pixel similarity to Linux baseline golden PNG; ANSI 16-color dirs=blue + executables=green; line-wrap at column boundary | S08 | Device: S24 Ultra `R5CX10VFFBA`; driver `tools/scripts/test-ls-la.sh` |
| 2 | **Scrollback ≥1000 lines**; touch-drag scrolls smoothly (**60fps on Galaxy S24 Ultra + S21+**); two-finger flick momentum scroll behaves natively. *(Amendment 3: S8 30fps fallback removed.)* | Ring buffer capacity ≥1000 lines; p95 frame interval during scroll <16.6ms on S24 Ultra; S21+ supplementary (not P0 gate) | S09 | Low-end Pixel 4a/A52s deferred per M2-S13 user choice |
| 3 | Block detection from DCS hook (`ESC P $ d ... 0x9c`) creates `Block` objects on the model side, with `start_time`, `command`, and `exit_code` correctly populated (verified by inspecting model state after running **3 sample commands** in zsh with the bootstrap hook) | 3 Block entries for `ls`, `whoami`, `false` — exit_code 0/0/1; command fields match; start_time non-zero | S07 + S05 + S06 | Driver: `tools/scripts/test-block-model.sh <serial>` |
| 4 | App size (release APK **excluding** bootstrap zip, which is shipped as a separate F-Droid auxiliary asset per M4) **≤80MB**; combined APK + bootstrap zip **≤120MB** total | `du -h app-release-unsigned.apk` ≤80MB; combined ≤120MB | S10 | Validation layer .so already excluded from release (M2-S04 round-3 gitignore) |
| 5 | Code freeze-and-merge dry-run: **cherry-pick latest 10 upstream Warp commits** onto our `mobile/main` branch — record total time; if **>2hr**, flag scope concern | Cherry-pick time <2hr for app/ conflicts = green; ≥2hr = Pre-mortem C #4 trip (scope concern + propose facade widening) | S11 | Per-crate conflict count recorded separately: `warp_terminal/` (expected low) vs `app/` (expected high) |

---

## 5. 12-Story Ledger with Phase Assignment

| Story | 標題 | Phase | Owner Hint | 狀態 |
|---|---|---|---|---|
| **M3-S01** | M3 kickoff doc + Plan section update + M2-S13 deferral note | **Foundation** | executor (sonnet) | **THIS DOC** |
| **M3-S02** | warp_terminal_mobile_facade real impl (Session API + AppContext + FeatureFlag + SSH-noop) | **Foundation** | executor (opus) | 待開始 |
| **M3-S03** | cfg-gate app::terminal::* desktop-only paths (Pre-mortem C threshold <500 lines) | **Foundation** | executor (opus) | 待開始 |
| **M3-S04** | Facade → warpui Android push_frame wiring (PTY bytes → terminal model → renderer) | **Foundation** | executor (opus) | 待開始 |
| **M3-S05** | DCS hook parser implementation (ESC P $ d ... 0x9c) | **DCS + Block** | executor (opus) | 待開始 |
| **M3-S06** | Bootstrap zsh_body.sh DCS hook ship + APK asset integration | **DCS + Block** | executor (sonnet) | 待開始 |
| **M3-S07** | Block model — start_time / command / exit_code (M3 Acceptance #3) | **DCS + Block** | executor (opus) | 待開始 |
| **M3-S08** | Live ls -la /system colored + line-wrapped on S24 Ultra (M3 Acceptance #1) | **Acceptance** | executor (opus) | 待開始 |
| **M3-S09** | Scrollback ≥1000 lines + 60fps touch-drag scroll (M3 Acceptance #2 flagship) | **Acceptance** | executor (opus) | 待開始 |
| **M3-S10** | APK size budget — release ≤80MB / combined ≤120MB (M3 Acceptance #4) | **Acceptance** | executor (sonnet) | 待開始 |
| **M3-S11** | Cross-workspace dup unification + cherry-pick dry-run (M3 Acceptance #5 + M2 carry-overs) | **Carry-overs** | executor (opus) | 待開始 |
| **M3-S12** | M3 close-out integration document | **Close-out** | executor (sonnet) | 待開始 |

**Phase 說明**：
- **Foundation** (S01-S04)：kickoff doc + facade real impl + cfg-gate app/ + facade→warpui wiring。S02-S04 必須依序完成，因為 S03 依賴 S02 的 facade API，S04 依賴 S02+S03 的結合。
- **DCS + Block** (S05-S07)：DCS parser + zsh_body.sh hook ship + Block model 三者是垂直 slice，可在 Foundation 完成後並行探索，但 S07 (Block model) depends on S05 (parser) + S06 (zsh hook)。
- **Acceptance** (S08-S10)：M3 acceptance 驗收，S08+S09 依賴 Foundation + DCS+Block pipeline 完整；S10 (APK size) 可在 S02-S04 基本 compile 通過後即開始量測。
- **Carry-overs** (S11)：cross-workspace dup unification + cherry-pick dry-run。S11 can unblock partially before S08-S10 close, but cherry-pick dry-run is cleanest after S08-S10 establish final file structure.
- **Close-out** (S12)：M3 go/no-go integration document — after all other stories close.

---

## 6. D1.5-hybrid 架構約束 (warp_terminal → warpui edge 保持不動)

per ralplan Amendment 2 + `.omc/m2-artifacts/M2-kickoff-confirmed.md` §4:

### 6.1 Cargo dependency graph invariants

```
warp_terminal_mobile_facade
    ├── depends on warp_terminal (clean Layer 2a, untouched in M3)
    ├── depends on warpui (via platform::android::Window::push_frame)
    └── provides Session + AppContext + FeatureFlag + SSH-noop to app/ consumer

app/ (Layer 2b, tangled)
    ├── consumes warp_terminal_mobile_facade (NOT direct warp_terminal edges on Android)
    ├── cfg(target_os = "android") gates route desktop-only deps through facade shims
    └── cfg-gate diff budget: <500 lines (Pre-mortem C threshold)
```

**Invariant**: `warp_terminal → warpui` Cargo edge stays. `warp_terminal` crate itself is NOT modified by M3 — DCS parser (S05) is extracted from `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` and routed through the facade, NOT added to `warp_terminal`. D2-lite has been superseded by Amendment 2 and is NOT revived.

### 6.2 cfg-gate dialect (consistent with Amendment 2 + Pre-mortem C #5)

```rust
// Mobile path — Android-specific impl
#[cfg(target_os = "android")]
fn foo() { /* facade shim */ }

// Desktop-only — excluded on Android
#[cfg(not(target_os = "android"))]
fn bar() { /* ai/ssh/feature_flag dispatch */ }
```

**Prohibited**: mixing `platform_family`, `unix`, `windows` in the same gate expression as `target_os = "android"` without explicit justification. Keep the dialect consistent throughout M3 to maintain cherry-pick velocity.

### 6.3 Facade widening escape hatch

If M3-S03 cfg-gate diff ≥500 lines → do NOT add more gates. Instead, **widen the facade**: move more `app/` surface area behind facade shim methods. This keeps `app/` diff bounded and shifts complexity to the facade (which is explicitly designed to absorb it).

---

## 7. Death-Pit Top-3

per `.omc/plans/ralplan-warp-on-mobile.md` §Pre-mortem + M3 scope analysis:

### 死坑 #1 — Pre-mortem C: cfg-gate budget overshoot (app/ diff >500 lines)

**描述**：M3-S03 要求 cfg-gate `app::terminal::*` desktop-only paths so `cargo build --target aarch64-linux-android -p app` succeeds. The 5 dependency edges to cut (app::ai / app::feature_flag / app::ssh / app::app_context / mio/nix paths) may require more than 500 lines of `#[cfg(...)]` annotations if `warp-src/app/src/terminal/model/session.rs` has deep nested call chains into these deps.

**量化預警**：
- `git diff main warp-mobile/m0-facade -- '*.rs' | grep -E '^\+\s*#\[cfg' | wc -l` measurement in S03
- If ≥500 lines: Pre-mortem C #4 trip — surface as scope concern, do NOT add more gates, propose facade widening instead
- If ≥1000 lines: escalate to architect agent (this is a structural refactor, not an executor task)

**緩解**：M3-S02 (facade real impl) completes first; facade API designed to absorb the 5 dependency edges cleanly before S03 starts gating. ralplan Implementation table row #2 lines 510-511 explicitly bounds the gate count.

### 死坑 #2 — Pre-mortem cherry-pick velocity: app/ conflict resolution >2hr

**描述**：M3-S11 cherry-pick dry-run from `warpdotdev/Warp@HEAD` onto our `warp-mobile/m0-facade` or `warp-mobile/main` will expose how much semantic drift has accumulated since our branch point (M0 `afc74ec`). The `warp-src/app/` layer is the danger zone — it's the tangled Layer 2b that already has OUR cfg-gates; any upstream changes to `warp-src/app/src/terminal/model/session.rs` or adjacent files will produce conflicts. If `warp-src/app/` conflicts take >2hr to resolve, this is a signal that the upstream HEAD has drifted beyond our cherry-pick recovery budget.

**量化預警**：
- `git cherry-pick` total time tracked; per-crate conflict count recorded
- `warp_terminal/` conflict count expected LOW (we didn't touch it)
- `app/` conflict count expected HIGH
- >2hr = Pre-mortem C #4 trip + scope concern + propose **upstream Warp HEAD pin** (freeze to a known-compatible HEAD, stop cherry-picking moving target)

**緩解**：Pin upstream Warp HEAD in `warp-src/` at a stable commit before M3-S11. Cherry-pick is a dry-run for M3-S11 (no permanent merge); the result is diagnostic, not prescriptive.

### 死坑 #3 — DCS extraction & cfg-gating risk (NOT format-undocumented; codex round-1 archeology confirmed parser exists)

**描述 (UPDATED post codex round-1 archeology)**：DCS hook parser **already exists upstream** at `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` (codex M0-archeology re-check confirmed: lines 1, 14, 407, 487; dispatch at `warp-src/app/src/terminal/model/ansi/mod.rs:771`). `warp-src/app/assets/bundled/bootstrap/zsh_body.sh` already emits hex JSON DCS hooks at lines 80, 254, 301. **M3-S05 reframed**: extract + route the existing parser through `warp_terminal_mobile_facade`, NOT discover the format from scratch. **M3-S06 reframed**: ship existing `zsh_body.sh` asset, NOT write new hooks. Death-pit downgraded from "format undocumented" to "extraction & cfg-gating risk". The existing parser likely has tight coupling to `app::terminal::model::*` desktop-only paths, so executor must:
1. Read existing `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` end-to-end to map dependency edges
2. Identify which `app::*` types/traits the parser uses; route them through `warp_terminal_mobile_facade` shims (consistent with S03)
3. Validate extraction by feeding `warp-src/app/assets/bundled/bootstrap/zsh_body.sh` DCS emissions through the facade-wrapped parser; assert Block events match expected boundaries

**量化預警**：
- Existing parser at `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` has tight `app::*` desktop-only deps → wrap behind facade or carve out with cfg-gates (consistent with S03 strategy)
- If payload format has changed across upstream versions → pin to the specific warp-src commit hash where format was established

**緩解**：
1. Read `warp-src/app/src/terminal/model/ansi/dcs_hooks.rs` (lines 1, 14, 407, 487) + `ansi/mod.rs:771` dispatch as the canonical references — they ARE the format spec
2. Use `warp-src/app/assets/bundled/bootstrap/zsh_body.sh` (lines 80, 254, 301) as live test fixtures for the extracted parser
3. Cross-check ECMA-48 §5.6 DCS framing only as protocol-level sanity (warp uses ESC P $ d ... ST per ralplan §6 M3 row #4); upstream impl is authoritative

---

## 8. M2 → M3 Carry-Overs

per `.omc/m2-artifacts/M2-go-no-go.md` §5 + prd.json M3-S11 ACs:

### 8.1 Cross-workspace duplication unification (overdue — carry-forward since M2-S07/S08/S10/S11)

**狀態**：M3-S11 P0 item within carry-overs phase. This has been deferred since S07 (font_render.rs) and S08 (static_grid.rs), then again from S10/S11 (ime.rs / input.rs). It is **overdue** and must be resolved in M3.

| 重複檔案對 | main repo | warp-src | 來源 story |
|---|---|---|---|
| `font_render.rs` / `font.rs` | `crates/android-host/src/font_render.rs` | `warp-src/crates/warpui/src/platform/android/font.rs` | M2-S07 nit |
| `static_grid.rs` | `crates/android-host/src/static_grid.rs` | `warp-src/crates/warpui/src/platform/android/static_grid.rs` | M2-S08 nit |
| `ime.rs` | `crates/android-host/src/ime.rs` | `warp-src/crates/warpui/src/platform/android/ime.rs` | M2-S10 nit |
| `input.rs` | `crates/android-host/src/input.rs` | `warp-src/crates/warpui/src/platform/android/input.rs` | M2-S11 nit |

**Resolution path** (prd.json M3-S11 AC#1):
- Option A: main consumes warp-src via Cargo.toml path dep (single source of truth in warp-src; main deletes mirror copies)
- Option B: warp-src is the only copy; main deletes all `crates/android-host/src/{font_render,static_grid,ime,input}.rs`
- Document decision rationale in S11 result artifact

### 8.2 Hardcoded `/Users/iml1s/...` adb path in test-pty-reattach.sh

`tools/scripts/test-pty-reattach.sh` — relic from M0/M1 dev machine ImL1s. Should use `command -v adb` instead. **Fix required in M3-S11** (per prd.json M3-S11 AC#5).

### 8.3 M2 cleanup nits (absorbed into M3-S11)

| # | 內容 | 位置 |
|---|---|---|
| 8.3.1 | Stale handle-ime-keyboard-visibility doc URL | `M2-S12-result.json:10` |
| 8.3.2 | `WindowInsetsControllerCompat.show(Type.ime())` for ime_mode test hook | `MainActivity.kt` ime_mode launch path |
| 8.3.3 | `START_STATIC_GRID` broadcast receiver comment with no impl | `MainActivity.kt` static-grid launch path |
| 8.3.4 | SubpixelMask / Color rasterize_glyph branches — emoji smoke test missing | `warp-src/crates/warpui/src/platform/android/font.rs:904` |
| 8.3.5 | Clippy lint cleanup (7+ nits: uninlined format args, let_unit_value) | `cargo clippy -p warp-mobile-android-host` |
| 8.3.6 | android-activity / winit reorganization re-check | `warp-src/crates/warpui/Cargo.toml` |
| 8.3.7 | CJK fallback span hack → file upstream cosmic-text PR | `warp-src/crates/warpui/src/platform/android/font.rs` other.rs emulation |

### 8.4 Functional carry-overs from M2 §5.1 → M3 in-scope

1. **Terminal session integration** (M3 main work — S02-S04)
2. **Block-based UI** (M3 S05-S07)
3. **Notification customization** (M2 CO-4, deferred again) — generic "Warp terminal" notification; M3 should add session count + command preview + tap → MainActivity intent IF S11 capacity permits; else M4 carry-over

---

## 9. Architecture State at M3 Start (post-M2)

### 9.1 Overall codebase structure

```
android/                                      (Gradle project, minSdk 31 / targetSdk 36 / compileSdk 36)
├── app/build.gradle
├── app/src/main/AndroidManifest.xml
│   ├── FOREGROUND_SERVICE + FOREGROUND_SERVICE_SPECIAL_USE + POST_NOTIFICATIONS
│   ├── MainActivity (LAUNCHER intent)
│   ├── WarpTerminalService (foregroundServiceType=specialUse)
│   └── PtyBroadcastReceiver (4 PTY intent-filters)
└── app/src/main/java/dev/warp/mobile/
    ├── MainActivity.kt              22,501 bytes — composite layout: FrameLayout → SurfaceView (Vulkan) + WarpInputView
    │                                  - FLAG_KEEP_SCREEN_ON:160; WindowInsets listener:247-262; fullscreen nav-bar hide:268-272
    ├── WarpInputView.kt             15,499 bytes — GestureDetector:105-122; VelocityTracker pre-detector; WarpInputConnection
    ├── NativeBridge.kt              13,095 bytes — 32 external funs (PTY×8 + Render×10 + IME×5 + Input×8 + Insets×1)
    ├── WarpTerminalService.kt        7,862 bytes — M1 FGS lifecycle + PTY broadcast dispatch (M3 wiring target)
    ├── PtyManager.kt                 2,455 bytes — M1 cmd_id → ptr Map; spawn/write/read/resize/kill
    ├── PtyBroadcastReceiver.kt         541 bytes — M1 carry-forward
    ├── CaptureFrameReceiver.kt       5,454 bytes — M2-S05 capture broadcast harness
    ├── ImeSimulationReceiver.kt      7,861 bytes — M2-S10 IME testing harness
    └── TouchSimulationReceiver.kt    6,197 bytes — M2-S11 simulation broadcasts

crates/android-host/                          (Rust workspace member, cdylib JNI host)
├── Cargo.toml                               (cdylib; jni 0.21; ndk 0.9; log 0.4; android_logger 0.13)
└── src/
    ├── lib.rs                               32 JNI exports (lines 35-788) — M3 wiring adds terminalInputBytes + terminalBlocksDump
    ├── pty.rs                               PtySession M1 baseline
    ├── font_render.rs                       M2-S07 duplicate of warp-src font.rs (M3-S11 unification target)
    ├── static_grid.rs                       M2-S08 duplicate of warp-src static_grid.rs (M3-S11 unification target)
    ├── ime.rs                               M2-S10 duplicate of warp-src ime.rs (M3-S11 unification target)
    └── input.rs                             M2-S11 duplicate of warp-src input.rs (M3-S11 unification target)

warp-src/                                    (gitignored; ImL1s/warp:warp-mobile/m0-facade @ d7616e5)
└── crates/
    ├── warp_terminal/src/                   (clean Layer 2a — NOT modified by M3; DCS parser is in app/ not here)
    ├── warpui/src/platform/android/         (M2 complete: 9 modules, 4 major-rewrite areas verified)
    │   ├── mod.rs / window.rs / dispatch.rs / vulkan.rs (render_scene:window.rs:313 / capture:window.rs:325)
    │   └── ime.rs / input.rs / font.rs (ASystemFontIterator:font.rs:169) / text_layout.rs / static_grid.rs
    ├── warp_terminal_mobile_facade/         (M0 scaffold — already has 4 .rs files; M3-S02 extracts/wires)
    │   └── src/{lib,terminal,blocks,ai}.rs  (existing M0 stub state)
    └── app/                                 (Layer 2b — DCS parser + zsh hooks ALREADY HERE)
        ├── src/terminal/model/ansi/dcs_hooks.rs   (M3-S05 extract/route through facade)
        ├── src/terminal/model/ansi/mod.rs:771     (DCS dispatch; S05 reuse)
        └── assets/bundled/bootstrap/zsh_body.sh   (M3-S06 ship existing — already emits hex JSON DCS at lines 80, 254, 301)

tools/scripts/                               (all take <serial> as first arg)
├── [M1] test-pty-{reattach,resize}.sh; test-fgs-clean-kill.sh; test-30min-idle-stress.sh
├── [M2] test-render-scene.sh; test-frame-capture{,-stress}.sh; test-font-render.sh;
│        test-static-grid.sh; test-rotation-stress.sh; test-ime.sh; test-touch.sh; test-window-insets.sh
└── [M3 — to be written] test-ls-la.sh; test-scroll.sh; test-block-model.sh
```

### 9.2 M3 主要新增 (S02-S11 完成後預期)

```
warp-src/crates/warp_terminal_mobile_facade/src/
├── lib.rs              (M0 placeholder → real Session::spawn/write/read API — S02)
├── terminal.rs         (M0 stub — S02 may extend or supersede)
├── blocks.rs           (M0 stub — S07 Block model integration)
├── ai.rs               (M0 stub — S02 may neutralize for mobile)
├── app_context.rs      (NEW S02 — AppContext mobile shim)
├── feature_flag.rs     (NEW S02 — FeatureFlag shim: terminal=true, ai=false, blocks=true)
├── ssh_noop.rs         (NEW S02 — SSH provider returning Unsupported)
└── render.rs           (NEW S04 — PTY bytes → model → Window::push_frame adapter)

warp-src/app/src/terminal/model/ansi/
└── dcs_hooks.rs        (EXISTING upstream — S05 extract/route through facade; DO NOT create a new DCS module under warp_terminal — DCS lives in app/ via facade extraction)

warp-src/app/assets/bundled/bootstrap/
└── zsh_body.sh         (EXISTING upstream — S06 ship/integrate existing asset; already emits DCS hooks at lines 80, 254, 301)

tools/scripts/
├── test-ls-la.sh       (S08 driver — PTY → renderer → frame capture → golden PNG diff)
├── test-scroll.sh      (S09 driver — 5s scroll stress; p95 frame interval)
└── test-block-model.sh (S07 driver — 3 commands; assert Block entries; result.json)
```

---

## 10. Verifier SOP (Codex + M2-S11 lesson: lead-dispatched only)

`prd.json` `verifierConfig.critic = "codex"` — 每個 worker deliverable 必須通過 Codex review 後才能將 story 標記為 `passes:true`。

### SOP:

1. Worker 完成 deliverable，commit + push to main (warp-src 變更 push to `ImL1s/warp:warp-mobile/m0-facade`)
2. **Lead (not worker)** reads artifact + dispatches Codex review：write prompt to `/tmp/codex-M3-S0x-review.md`, then `omc ask codex --prompt "$(< /tmp/codex-M3-S0x-review.md)"` (avoid zsh `()` parse errors)
3. Background dispatch via `run_in_background: true`; verdict read from `.omc/artifacts/ask/codex-*.md`
4. REVISE → follow-on task fix (new commit, NOT amend); PASS → lead marks story `passes:true` in prd.json
5. **M2-S11 lesson (process anomaly, worker self-dispatched on pre-fix state)**: Worker MUST NOT dispatch Codex review before the fix is committed. The canonical M3 sequence is: Worker delivers → Lead reads → Lead dispatches Codex → Codex verdict → Lead flips `passes:true`. Any deviation from this must be flagged.
6. **Trust but verify**: worker completion claims must be cross-checked via `git diff` before lead dispatches Codex. Fabricated completion claims have occurred (M1 lesson learned).

### Web search protocol (per user 「搭配 codex 和網路搜索相關文檔和資訊」):

Executors for M3-S02, S03, S04, S05 **must** web-search relevant docs before implementation:
- S02: warp upstream AppContext/FeatureFlag interface patterns; mobile shim precedents
- S03: Rust cfg attribute composition; warp upstream platform-cfg pattern
- S05: DCS sequence spec ECMA-48 §5.6; xterm DCS; warp upstream DCS hook format (search for any OSS mentions)

Cite searched doc URLs in the codex review prompt.

---

## 11. 執行決策 — Foundation Phase First

M3 正式開始。M3-S01 (this doc) 完成後，next story 按 priority 順序：

### Foundation phase dispatch order (S02 → S03 → S04, sequential):

1. **M3-S02** (facade real impl) — Foundation P0. Implements `Session::spawn/write/read` + AppContext + FeatureFlag + SSH-noop. Must complete before S03 (S03 cuts `app/` edges INTO the facade). Owner: executor (opus).
   - 先決條件：read `warp-src/crates/warp_terminal_mobile_facade/src/lib.rs` (M0 scaffold); web search warp AppContext/FeatureFlag patterns
   - 完成門檻：`cargo doc --no-deps -p warp_terminal_mobile_facade` PASS; `cargo ndk -t arm64-v8a check -p warp_terminal_mobile_facade` PASS; ≥1 unit test stubs all desktop deps

2. **M3-S03** (cfg-gate app::terminal::*) — Foundation P0 after S02. Gates `app::` edges; budget <500 lines. Owner: executor (opus).
   - 先決條件：S02 complete (facade API surface fixed); web search warp upstream terminal/session.rs cfg pattern
   - 完成門檻：`cargo build --target aarch64-linux-android -p app` zero errors; gate count <500 lines measured and recorded

3. **M3-S04** (facade → warpui wiring) — Foundation P0 after S03. Wires `render.rs` adapter: PTY bytes → model cells → `Window::push_frame`. Owner: executor (opus).
   - 先決條件：S02 (Session API) + S03 (app/ builds android) + M2 `warpui::platform::android::Window::push_frame` verified
   - 完成門檻：`cargo ndk -t arm64-v8a build -p warp-mobile-android-host` PASS; APK installs without crash; PTY bytes visible at NativeBridge

### DCS+Block phase (S05-S07, after Foundation):

4. **M3-S05** (DCS parser) → 5. **M3-S06** (zsh_body.sh) → 6. **M3-S07** (Block model) — sequential; S07 depends on S05+S06

### Acceptance phase (S08-S10, after DCS+Block):

7. **M3-S08** (ls -la colored) + **M3-S09** (scrollback 60fps) — these verify the full pipeline end-to-end; M3-S08 implicitly validates S04 push_frame wiring works with real PTY output
8. **M3-S10** (APK size budget) — can start partial measurement after S02-S04 land

### Carry-overs + Close-out:

9. **M3-S11** (cross-workspace dup unification + cherry-pick dry-run) — after S08-S10 establish final file structure; unification is cleanest when all M3 source files are stable
10. **M3-S12** (close-out doc) — after all other stories close; mirrors M2-go-no-go.md template

**M3 timeline estimate per ralplan §6 M3**: 8-12 weeks (same as M2). Foundation phase 2-3 weeks. DCS+Block 2-3 weeks (dependency-heavy on warp upstream archeology). Acceptance 2-3 weeks (device driver writing + iteration). Carry-overs + Close-out 1-2 weeks.

**Honest dependency flag**: M3 is **more dependency-heavy on warp upstream** than M2 was. M2 added new files to warp-src; M3 must MODIFY existing `warp-src/app/` Layer 2b files (DCS parser, Block model, terminal session glue) plus add facade/host wiring — `warp_terminal` (Layer 2a) remains clean/untouched per Amendment 2 invariant, with cherry-pick conflict count expected low there. Cherry-pick velocity (Death-pit #2) is a genuine risk on the `warp-src/app/` side. If cherry-pick conflicts in `warp-src/app/` consistently exceed budget, the upstream Warp HEAD must be pinned earlier rather than later.

---

*撰寫人：executor@M3-S01 (Claude Sonnet 4.6)*
*下一步：Codex review dispatch for M3-S01 (per prd.json M3-S01 AC#4 + §10 SOP). On PASS: lead marks M3-S01.passes:true in prd.json and dispatches M3-S02 to executor (opus).*
