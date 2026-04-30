# M2 Kickoff 確認報告

**日期**：2026-04-30 (M2 milestone 正式開始)
**PRD scaffold base**：`51d9ccf` (M1→M2 prd.json transition commit)
**This artifact landed at**：`19b7b5f` — main has since advanced via codex-review fix-up commits
**Plan reference**：`.omc/plans/ralplan-warp-on-mobile.md` §6 M2 (lines 389-502) + Amendment 1+2 D1.5-hybrid
**前置 milestones**：
- M0 close-out CONDITIONAL GO @ commit `24a2c1c` (Vulkan surface recreate p95 < 200ms on Adreno 6xx+; S8/Mali-G71 dropped per Plan Amendment 3)
- M1 close-out CONDITIONAL GO @ commit `f7feb3f` (10/10 stories PASS — PTY/Service plumbing verified on S24 Ultra)

---

## 1. Entry Criteria Satisfied

M2 入場條件全部確認：

| 條件 | 狀態 | 證據 |
|---|---|---|
| M0 Vulkan surface recreate p95 < 200ms on Adreno 6xx+ | **PASS (2/3)** | `.omc/m0-artifacts/M0-vulkan-spike-report.md`:<br>**100-cycle swapchain recreate latency** (the AC-relevant metric): S24 Ultra (Adreno 750) p50=13ms / p95=**18ms** / p99=21ms PASS;  S21+ (Adreno 660) p50=23ms / p95=**28ms** / p99=31ms PASS;  S8 (Mali-G71) p50=190ms / p95=**326ms** / p99=394ms FAIL.<br>**Steady-state single-frame** (separate metric, lead-context-snapshot.md §8): S24 Ultra ~9ms / S21+ ~21ms / S8 ~52ms.<br>**E1 retreat trigger** (M0-tension3-decision.md): "fails on 2+ of 3 devices" — actual = 1/3 fail, trigger **NOT activated**, port proceeds. S8/Mali-G71 dropped from matrix per Plan Amendment 3 (minSdk raised 26→31). |
| M1 PTY/Service plumbing verified (FGS + Activity recreate + resize + clean kill + 30-min stress) | **PASS** | `.omc/m1-artifacts/M1-go-no-go.md` §6 verdict — 5/5 Plan §6 M1 ACs satisfied on S24 Ultra flagship pathway. delta_ms=26 (S06), orphans=0 (S08), pwd 4ms at t=30min (S09). |
| minSdk 31 baseline established | **PASS** | Plan Amendment 3 @ commit `2ccc0f7`. S8/Mali-G71 dropped. |
| warp-src cloned at warp-mobile/m0-facade | **PASS** | `warp-src/` at `ImL1s/warp:warp-mobile/m0-facade` @ commit `afc74ec` (android-activity feature fix, M1-S02). |
| cargo test -p warp-mobile-android-host PASS | **PASS** | 3/3 tests (test_pty_echo_hello, test_drop_reaps_child, test_arc_concurrent_read_kill) — verified 2026-04-30 on this machine. |

---

## 2. 環境基線 (此台機器)

| 工具 | 版本 | 備注 |
|---|---|---|
| Rust | 1.92.0 (ded5c06cf 2025-12-08) | 實際 `rustc --version` 輸出 |
| cargo-ndk | 4.1.2 | 實際 `cargo ndk --version` 輸出 |
| Android NDK | **r28.2 @ `/Users/setsuna-new/development/android-sdk/ndk/28.2.13676358`** (substitute, 限本機 cargo-ndk bring-up) | 專案 `.envrc:1` 標稱 r29 (`29.0.13113456`)；本機未安裝 r29，僅用 r28.2 替代以驅動 cargo-ndk 進行 host 端 Rust 交叉編譯。**範圍限制**：（1）AGP 端 `android/app/build.gradle:89` `ndkVersion '29.0.13113456'` 仍標稱 r29，AGP 只在實際呼叫 native compile 時才檢查（本專案僅 copy 預先 build 好的 .so，不走 AGP 的 native compile path）；（2）任何 NDK API 高於 minSdk 31 baseline 的（如 `AFontMatcher` API 29 OK；NDK 33+ APIs 須 runtime-guard）必須在 source 端做 `android_get_device_api_level()` 或 reflection guard。official ref: developer.android.com/ndk/downloads/, developer.android.com/ndk/guides/using-newer-apis. |
| Android SDK | `/Users/setsuna-new/development/android-sdk/` (本機路徑) | 專案 `.envrc:2` 標稱 `~/Library/Android/sdk/` (project-owner 機器路徑)；本機以 shell env override |
| Java | JDK 21 (`~/development/jdk-21.0.6+7/`) | 本機 PATH；專案 spec 為 OpenJDK 17 — 21 向後兼容，待 Gradle build 驗證 |
| Rust target | `aarch64-apple-darwin` + `aarch64-linux-android` (installed) | `rustup target list --installed` 確認 |
| warp-src branch | `warp-mobile/m0-facade` @ `afc74ec` | per M1-S02 close-out (verified 2026-04-30 on this machine) |

**注意**：`.cargo/config.toml` 為 gitignored 的 machine-absolute paths 檔案。首次 checkout 後需執行 `ANDROID_NDK_ROOT=<your-ndk-path> tools/scripts/setup-cargo-config.sh` 重新生成。Template 在 `.cargo/config.toml.template`。

**機器差異記錄**：M0/M1 開發機為 project owner ImL1s (`/Users/iml1s/...`)；M2 開始於本機 setsuna-new (`/Users/setsuna-new/...`)。NDK 版本、SDK 位置、Java 版本均不同；JDK 21 vs spec 的 17 須在 M2-S02 Gradle build 時驗證。

---

## 3. M2a / M2b Split (Amendment 1+2 D1.5-hybrid)

per `.omc/plans/ralplan-warp-on-mobile.md` §6 M2 lines 393-396:

### M2a — Layer 1 hand-written areas (4-5 weeks 估算)

目標：在 `warp-src/crates/warpui/src/platform/android/` 內實作 4 個 **major rewrite areas**；其餘 32 methods 從 `warpui::platform::headless` derive (22 total reuse + 10 minor patch ≤20 lines each)。

**完整覆蓋分佈**（來自 `.omc/m0-artifacts/M0-platform-trait-delta.md` §6.2 A4 path scoring）：

| 類別 | Method 數 | 處理方式 |
|---|---|---|
| Total reuse | 22 | 直接從 headless 拿來用,0 行新 code |
| Minor patch (≤20 lines each) | 10 | mpsc → ALooper、is_headless 翻 false、open_url → JNI Intent、microphone permission → Android API 等 |
| **Major rewrite (4 areas)** | render_scene + request_frame_capture + FontDB (15) + TextLayoutSystem (2) | 本 milestone 的核心 deliverable |

**4 major rewrite areas + ralplan §6 M2 table 對應**（per ralplan lines 489-502 implementation table — `vulkan.rs` 是 plan 規定的檔名,`render_scene` / `request_frame_capture` 是 method names 而非檔名）：

| Method / Area | 實作檔案 | 說明 |
|---|---|---|
| `WindowContext::render_scene` | `platform/android/vulkan.rs` (per ralplan §6 M2 row #3) | ash Vulkan + ANativeWindow; VkInstance + VkSurfaceKHR (VK_KHR_android_surface) + VkSwapchainKHR + renderpass + present queue。從 M0 spike `spikes/vulkan-surface-recreate/` 的 50-line proof 擴展為 production swapchain。 |
| `WindowContext::request_frame_capture` | `platform/android/vulkan.rs` (readback) | ash readback via vkCmdCopyImageToBuffer + memory mapping → raw RGBA bitmap + PNG。用於 visual regression testing。 |
| `FontDB` (15 methods) | `platform/android/font.rs` (per ralplan §6 M2 row #6) | cosmic-text wrapper + Android system font discovery (AFontMatcher NDK API since API 29 OR `/system/fonts/` scan + bundled assets)。需支援 CJK 字型 (Noto Sans CJK)。 |
| `TextLayoutSystem` (2 methods) | `platform/android/text_layout.rs` (extension; ralplan table uses "etc.") | cosmic-text shaping/layout；與 linux/mac backends 保持 parity。 |

**Window/WindowContext/WindowManager/DispatchDelegate 結構**（minor-patch 區,per ralplan §6 M2 row #2）：
- `platform/android/window.rs` — Window + WindowContext methods (incl render_scene + request_frame_capture call-sites)
- `platform/android/dispatch.rs` — DispatchDelegate (ALooper-based main-thread dispatch)
- `platform/android/mod.rs` — cfg(target_os = "android") dispatch 入口

**M2a 完成門檻** (ralplan §6 M2 AC #1 + #2 + #5):
1. Static 50×20 grid "Hello, World" 以 60fps 渲染於 S24 Ultra (Adreno 730+)，p95 frame time < 16.6ms（30秒持續）
2. Activity.recreate() 100-cycle rotation，swapchain p95 recovery < 200ms，無 black frame > 300ms
3. `cargo doc` for `warpui::platform::android` 存在，module-level doc 解釋設計，≥1 unit test per public function

### M2b — Input, IME, WindowInsets (4-6 weeks 估算)

目標：接受所有 Android 輸入來源，正確處理視窗佈局。

**M2b 完成門檻** (ralplan §6 M2 AC #3 + #4):
3. Soft IME (Gboard English + Pinyin) 每個按鍵進一個 character；composing-text region (Chinese) in-place 更新不閃爍
4. WindowInsets 正確 reserve IME bottom region；全螢幕隱藏 nav bar；rotation 在 1 frame budget 內 re-layout

---

## 4. D1.5-hybrid 架構約束

per ralplan Amendment 2 (`.omc/plans/ralplan-warp-on-mobile.md` §3.3 in lead-context-snapshot.md):

- `warp_terminal → warpui` Cargo edge **保持不變**（D2-lite 已被廢棄；Amendment 1 superseded by Amendment 2）
- `warpui` 內部以 `cfg(target_os = "android")` gates 隔離 Android-specific code，不引入 `font-kit` 或桌面 `winit`
- `crates/warpui/src/platform/android/` 新增，dispatch 在 `platform/mod.rs` 的 `cfg(target_os = "android")` branch
- M2-S03 要求：`ANDROID_NDK_ROOT=$NDK cargo ndk -t arm64-v8a check -p warpui` in `warp-src/` 成功（其他 crates 的 android-incompat errors 屬 out-of-scope）

---

## 5. M2 Story Ledger — 14 個 Stories 及 Phase 分配

| Story | 標題 | Phase | Owner Hint | 狀態 |
|---|---|---|---|---|
| M2-S01 | M2 kickoff doc + Plan section update | Setup | executor (sonnet) | **THIS DOC** |
| M2-S02 | Gradle copy task replacing jniLibs symlink | Setup (M1 carry-over #2) | executor (sonnet) | 待開始 |
| M2-S03 | warpui::platform::android scaffold from headless | **M2a** | executor (opus) | 待開始 |
| M2-S04 | render_scene minimal Vulkan submission via ash + ANativeWindow | **M2a** | executor (opus) | 待開始 |
| M2-S05 | request_frame_capture (ash readback to bitmap) | **M2a** | executor (opus) | 待開始 |
| M2-S06 | TextLayoutSystem 2-method skeleton | **M2a** | executor (opus) | 待開始 |
| M2-S07 | FontDB 15-method cosmic-text + Android system fonts (M2a-font sub-gate) | **M2a** | executor (opus) | 待開始 |
| M2-S08 | Static MxN grid 60fps on flagship (M2a Acceptance #1) | **M2a** | executor (opus) | 待開始 |
| M2-S09 | Activity recreate swapchain p95 < 200ms (M2a Acceptance #2) | **M2a** | executor (opus) | 待開始 |
| M2-S10 | IME glue (commitText/setComposingText/finishComposingText) | **M2b** | executor (opus) | 待開始 |
| M2-S11 | Touch input + gesture mapping | **M2b** | executor (sonnet) | 待開始 |
| M2-S12 | WindowInsets + IME insets reservation (M2b Acceptance #4) | **M2b** | executor (sonnet) | 待開始 |
| M2-S13 | Acquire Pixel 4a or A52s + re-run M1 stories on low-end | M2 carry-over (M1 CO #1) | user-action then lead | 待開始 |
| M2-S14 | M2 close-out integration document | Close-out | executor (sonnet) | 待開始 |

**Phase 說明**：
- **Setup**：環境基礎建設，S01 + S02 必須在 M2a 主幹前完成
- **M2a**：S03-S09，靜態渲染 pipeline 完整 — 從 headless scaffold 到 60fps grid + swapchain recovery
- **M2b**：S10-S12，輸入處理 — IME / touch / WindowInsets
- **M2 carry-over**：S13，Low-end device 補測（Plan Amendment 3 §3 要求，M1 carry-over #1）
- **Close-out**：S14，M2 go/no-go integration doc（per M1-go-no-go.md template）

---

## 6. Exit Criteria (Plan §6 M2 AC 1-5)

per `.omc/plans/ralplan-warp-on-mobile.md` lines 398-402:

| # | Acceptance Criterion | 對應 Story | 量化門檻 |
|---|---|---|---|
| 1 | Static 50×20 glyph atlas "Hello, World" @ 60fps on flagship + mid-tier + replacement low-end Adreno 6xx device；無 Vulkan validation warnings | S08 + S13 | p95 frame time < 16.6ms (30秒); Adreno 6xx+ API 31+ full coverage |
| 2 | Activity.recreate() swapchain recovery < 200ms p95 across 100 rotations；無 black frame > 300ms | S09 | p95 < 200ms (Choreographer diff: surfaceDestroyed → first non-stale-pixel frame) |
| 3 | Soft IME (Gboard English + Pinyin) 每鍵入一 character；composing-text 不閃爍 | S10 | 5 char input events for "hello"; Pinyin 你好 commits without flicker |
| 4 | WindowInsets 正確 reserve IME bottom；全螢幕隱藏 nav bar；rotation re-layout in 1 frame budget | S12 | Visual verification; test-window-insets.sh driver |
| 5 | `cargo doc` for `warpui::platform::android`；module-level doc；≥1 unit test per public function | S03-S07 | `cargo doc` succeeds; doc coverage verified by Codex |

---

## 7. Death-Pit 風險意識 (Top 3)

per `.omc/m2-kickoff.md` §Death-pit awareness + `.omc/plans/ralplan-warp-on-mobile.md` §Pre-mortem:

### 風險 #1 — `Window::draw_frame` Activity recreate 時機問題

**描述**：`warpui::platform::android::Window::draw_frame` 必須將 wgpu/Vulkan surface lifecycle 映射到 Android `SurfaceHolder.Callback` events，而不在 mid-frame 時丟失 rendering state。`onSurfaceCreated` → `onSurfaceChanged` → `onSurfaceDestroyed` 事件順序在不同 OEM 上有差異（Samsung One UI vs AOSP）。

**緩解**：從 M0 spike `spikes/vulkan-surface-recreate/` 擴展。M0 100-cycle p95 結果：S24 Ultra (Adreno 750) 18ms PASS；S21+ (Adreno 660) 28ms PASS；S8 (Mali-G71) 326ms **FAIL**。E1 retreat trigger（`M0-tension3-decision.md` Question E）規定「2+ of 3 devices fail 才觸發」，實際 1/3 fail 故 NOT activated；S8 已從 device matrix dropped (Plan Amendment 3 minSdk 26→31)。S09 story 強制跑 100 rotations 在當前支援的 Adreno 6xx+ device class 上驗收。VK_ERROR_OUT_OF_DATE_KHR 處理必須 explicit（Amendment 2 hardened acceptance）。

**跳脫口**：若 3 天內無法穩定 recreate path，直接 `finishAndRemoveTask()` + restart Activity on surfaceDestroyed（M0 spike有記錄此模式）。

### 風險 #2 — IME composing-text state machine 複雜度

**描述**：Android IME emit `commitText` / `setComposingText` / `finishComposingText` 三種 events，必須正確映射到 terminal cursor + dead-key state。Pinyin 輸入時 composing region 長度在 1-6 字之間動態變化；Gboard 有時會在 `setComposingText` 與 `commitText` 之間插入空的 `finishComposingText`（已知 Gboard bug）。

**緩解**：S10 story 驗收要求 Pinyin 你好 composing region in-place 更新不閃爍，plus "hello" 5 char input。driver `test-ime.sh` 自動化 adb shell input keyboard text 驗證。

**跳脫口**：若 composing-text 在 5 person-days 內無法 stable，先 commit 只到 Latin keystroke (no composing) 作為 M2 PARTIAL，defer CJK IME to M3 with explicit deviation note。

### 風險 #3 — 4 major-rewrite 區域 D1.5-hybrid 中的存活率

**描述**：M0 `M0-platform-trait-delta.md` §6.2 確認 headless backend 中 32 methods 是 total reuse + minor patch（22 + 10）；4 個 major rewrite areas 是 M2 核心 deliverable。但 warpui upstream 持續演進；`warp-mobile/m0-facade` branch 以 `afc74ec` 為基礎，任何 upstream cherry-pick 都可能 conflict 進入這 4 個區域。

**緩解**：M0-platform-trait-delta.md 鎖定了比較的 upstream commit hashes。M2-S03 scaffold story 明確 preserve git history of headless-derived files。每個 story (S03-S07) 在 warp-src branch 上 commit，Codex review 前不 merge to main。

**跳脫口（Companion retreat）**：簽核的 trigger 是 **E1** — 「M0 Vulkan spike fails on 2+ of 3 reference devices」（per `.omc/m0-artifacts/M0-tension3-decision.md` Question E）。M0 Task #8 結果為 1/3 fail（S8 Mali-G71 only），**E1 NOT activated**，port 進入 M2。E2（M2a > 8 weeks）和 E3（cumulative >50% overrun by M3）為 **rejected alternatives**，不是 signed trigger，但作為 **at-risk escalation checkpoint**：若 M2a 在第 8 週仍未達 acceptance #1+#2，lead 觸發 escalation discussion（不是自動 retreat）。ralplan Pre-mortem Scenario A（"M2 WarpUI Android backend stalls"）明列 "12-week budget for M2 with hard checkpoint at week 8"。

---

## 8. M1 Carry-Overs in M2 Scope

per `.omc/m1-artifacts/M1-go-no-go.md` §5 + `.omc/m2-kickoff.md` §M1 carry-overs:

| # | 內容 | 對應 M2 Story | 優先級 |
|---|---|---|---|
| CO-1 | Acquire Pixel 4a / Galaxy A52s API 31 → re-run M1-S06/S07/S08/S09 on low-end | S13 | P0 before M2 close (Amendment 3 §3 requirement) |
| CO-2 | Gradle copy task replacing jniLibs absolute symlink (`android/app/src/main/jniLibs/arm64-v8a/`) | S02 | P0 early M2 (fragile on CI/clean-checkout) |
| CO-3 | android-activity / winit reorganization (redundant dep in warpui/Cargo.toml per Codex S02 review) | S03 (fold into D1.5-hybrid restructuring) | P1 |
| CO-4 | Notification customization (session count + command preview + tap → MainActivity intent) | Out-of-scope M2 → M3 carry-over | Deferred |
| CO-5 | Clippy lint cleanup (7 nits: uninlined format args, let_unit_value) | Out-of-scope M2 → M3 carry-over (non-blocking) | Deferred |

---

## 9. Architecture State at M2 Start

現有 codebase 結構（M1 close 後，per `.omc/handoffs/lead-context-snapshot.md` §8c）：

```
android/                          (Gradle project, minSdk 31 / targetSdk 36 / compileSdk 36)
├── app/build.gradle
├── app/src/main/AndroidManifest.xml
│   ├── FOREGROUND_SERVICE + FOREGROUND_SERVICE_SPECIAL_USE + POST_NOTIFICATIONS
│   ├── MainActivity (LAUNCHER intent)
│   ├── WarpTerminalService (foregroundServiceType=specialUse)
│   └── PtyBroadcastReceiver (4 PTY intent-filters)
└── app/src/main/java/dev/warp/mobile/
    ├── MainActivity.kt
    ├── WarpTerminalService.kt    (FGS lifecycle + PTY broadcast dispatch + read coroutine)
    ├── PtyBroadcastReceiver.kt
    ├── PtyManager.kt             (cmd_id → ptr Map; spawn/write/read/resize/kill/killAll)
    └── NativeBridge.kt           (System.loadLibrary + 6 external funs)

crates/android-host/             (Rust workspace member, cdylib JNI host)
├── Cargo.toml                   (cdylib, jni 0.21, ndk 0.9, log 0.4, android_logger 0.13)
└── src/
    ├── lib.rs                   (6 JNI exports: ping + ptySpawn/Read/Write/Resize/Kill)
    └── pty.rs                   (PtySession: Arc lifetime, AtomicI32 master_fd, AS-safe fork+execve)

tools/scripts/                   (M1 test drivers, all take <serial> as first arg)
├── test-pty-reattach.sh         (S06 — rotation × 5, logcat epoch parse)
├── test-pty-resize.sh           (S07 — PTY_RESIZE broadcast → stty verify)
├── test-fgs-clean-kill.sh       (S08 — am force-stop, orphan detection)
└── test-30min-idle-stress.sh    (S09 — 4 checkpoint snapshots + pwd latency)

spikes/
├── vulkan-surface-recreate/     (M0 50-line Rust+JNI Vulkan lifecycle proof)
└── symlink-jnilibs/             (M0 W^X path validation)

warp-src/                        (gitignored; Warp upstream fork ImL1s/warp)
└── (branch warp-mobile/m0-facade @ afc74ec)
    └── crates/warpui/src/platform/  ← M2 target: add android/ subdirectory here
```

**M2 主要新增**（S03-S12 完成後 — 對齊 ralplan §6 M2 implementation table lines 489-502）：
```
warp-src/crates/warpui/src/platform/android/
├── mod.rs          (cfg dispatch + trait re-exports — ralplan #1)
├── window.rs       (Window + WindowContext + WindowManager methods — ralplan #2; 含 render_scene/request_frame_capture call-sites)
├── dispatch.rs     (DispatchDelegate — ralplan #2; ALooper main-thread dispatch)
├── vulkan.rs       (ash Vulkan + ANativeWindow integration — ralplan #3; production swapchain incl render_scene + request_frame_capture impls)
├── ime.rs          (composing-text state machine via InputConnection JNI — ralplan #4)
├── input.rs        (onTouchEvent → MouseDown/Up/scroll; GestureDetector — ralplan #5)
├── font.rs         (FontDB 15 methods — cosmic-text + Android NDK font discovery — ralplan #6)
└── text_layout.rs  (TextLayoutSystem 2 methods — cosmic-text shaping; extension to ralplan "etc." in row #2)
```

---

## 10. Verifier SOP (unchanged from M1)

`prd.json` `verifierConfig.critic = "codex"` — 每個 worker deliverable 都必須通過 Codex review 後才能將 story 標記為 `passes:true`。

SOP:
1. Worker 完成 deliverable，commit + push to main (warp-src 變更 push to `ImL1s/warp:warp-mobile/m0-facade`)
2. Lead 讀取 artifact，dispatch Codex review：寫 prompt 到 `/tmp/codex-*.md`，再 `omc ask codex --prompt "$(< /tmp/codex-*.md)"`（避免 zsh `()` parse errors）
3. Background dispatch via `run_in_background: true`；verdict 讀自 `.omc/artifacts/ask/codex-*.md`
4. REVISE → follow-on task fix；PASS → mark story `passes:true` in prd.json
5. **Trust but verify**：worker 完成聲明必須 cross-check git diff（M1 lesson learned：fabricated completion 曾發生）

---

## 11. 執行決策

M2 正式開始。M2-S01 (this doc) 完成後，next story 按 priority 順序：

1. **M2-S02** (Gradle copy task) — Setup P0，unblocks clean CI checkout for all subsequent M2a stories
2. **M2-S03** (warpui android scaffold from headless) — M2a foundation，unblocks S04-S09
3. **M2-S04 → S07** (4 hand-written areas) — M2a core，可依 complexity 排序 (render_scene first, then FontDB M2a-font sub-gate, then TextLayoutSystem, then frame capture)
4. **M2-S08 + S09** (M2a acceptance verification) — gate before starting M2b
5. **M2-S10 → S12** (M2b input pipeline) — after M2a gate PASS
6. **M2-S13** (low-end device, user-action dependency) — user action required; track independently
7. **M2-S14** (close-out doc) — after all other stories close

---

*撰寫人：executor@M2-S01 (Claude Sonnet 4.6)*
*下一步：Codex review dispatch for M2-S01 (per prd.json AC#4). On PASS: mark M2-S01.passes:true in prd.json and proceed to M2-S02.*
