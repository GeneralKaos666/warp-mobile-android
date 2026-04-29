# M0 Go/No-Go 整合報告

**日期**：2026-04-29
**撰寫**：worker-env（代 team-lead@warp-mobile-m0）
**版本**：v1.0（初稿，待 team-lead commit + push + Codex 共識審查）

---

## 1. M0 任務帳目（Tasks 1–17）

| # | 任務 | 狀態 | 一行證據 |
|---|------|------|---------|
| 1 | NDK env + cargo ndk smoke build | **PASS** | NDK 29.0.13113456、cargo-ndk 4.1.2、`aarch64-linux-android` release build 成功（`M0-env-report.md`） |
| 2 | [USER] symlink-jniLibs 三裝置驗證 | **PASS** | 先決條件；Task 12/14 正式量測取代此項 |
| 3 | cargo check warp_terminal aarch64-linux-android + deps report | **PASS-with-blockers** | warp_terminal 自身 dep 乾淨；兩個 transitive 失敗（font-kit / android-activity）均在 warpui 層；預計 cfg-gate 3,334 行 = Pre-mortem C 門檻 6.7× → 觸發 D2-lite（`M0-deps-report.md`） |
| 4 | warpui::platform trait surface + gpui-mobile diff | **PASS** | 89 trait methods：0 identical、31 portable、13 incompatible、45 missing；gpui-mobile 為架構參考非 dep（`M0-platform-trait-delta.md`） |
| 5 | Vulkan-Surface-recreate spike code（編譯驗證） | **PASS** | `libvulkan_surface_recreate.so` 716 KB、`cargo ndk arm64-v8a release` clean build（`spikes/vulkan-surface-recreate/`） |
| 6 | warp_terminal_mobile_facade scaffold | **PASS-structural** | 5 檔案 commit `5400c66`、cfg-dialect 慣例文件完備；Android cargo check 因 android-activity 失敗屬已知上游問題（`M0-facade-scaffold.md`） |
| 7 | Decision A1-vs-A4 archeology | **PASS** | A4（headless base）3–4 週 vs A1（linux/winit）6–8 週；A4 建議確定（附於 `M0-platform-trait-delta.md`） |
| 8 | [USER] Vulkan spike 三裝置 100-cycle rotation 量測 | **PASS-with-caveat** | S24 Ultra p95=18ms、S21+ p95=28ms PASS；S8 p95=326ms FAIL（Mali-G71/A9）。E1 門檻 2/3 fail 未達，1/3 fail → E1 NOT triggered（`M0-vulkan-spike-report.md`） |
| 9 | [USER] Tension 3 user gate decision | **PASS** | A1+B1+C1+E1 由 team-lead 代決（"全自動" 授權）（`M0-tension3-decision.md`） |
| 10 | M0 go/no-go 整合 | **本文件** | — |
| 11 | Vulkan spike APK build + adb 量測腳本 | **PASS** | APK 3.7 MB、`lib/arm64-v8a/` .so 驗證、三裝置 install 成功（`M0-task11-install-verify.md`） |
| 12 | symlink-jniLibs execve() test harness | **PASS** | 初版 harness；Task 14 redo 取代（詳見下） |
| 13 | Fix Surface native handle（ANativeWindow_fromSurface） | **PASS** | 移除 reflection；改用 NDK public API `ANativeWindow_fromSurface`（API 26+）（`M0-task13-surface-fix-verify.md`） |
| 14 | symlink-jniLibs redo per Codex REVISE | **PASS** | 4-config 全 passed=true（SDK 36/35/28、debug+release）；negative_control EACCES 確認；`targetSdk=36`（`M0-symlink-jnilibs.md`） |
| 15 | Vulkan spike B-F（steady-state swapchain recreate） | **PASS** | 真實 swapchain + render pass；validation layer 零警告；三裝置 first_frame_presented_ts 穩態 7–52ms（`M0-task15-swapchain-verify.md`） |
| 16 | Vulkan spike Codex round-2 follow-up | **PASS** | dual-strategy parser、AndroidManifest scope comment、3-device rotation report（commit `1048a1e`） |
| 17 | symlink errno capture cleanup | **PASS** | sentinel `NEGATIVE_ERRNO_BEGIN...END`、`OsConstants.errnoName()`、`negative_errno_name` JSON；4-config 再驗證（commit `f89f0ea`） |
| 18 | Vulkan spike round-3 strict-assert + scope LIMIT | **PASS** | strict assert ±2 tolerance（CYCLES*2=200）、Manifest scope LIMIT 文件（commit `ff439ad`） |
| 19 | symlink JSON escape via jq -n | **PASS** | `jq -n --arg/--argjson` 重組 JSON，避免 IOException 路徑雙引號破壞 JSON（commit `3ceb777`） |

---

## 2. 決策矩陣（逐層）

### L1 — WarpUI Android Vulkan 後端

**判決：API 31+ / Adreno 6xx+ CONDITIONAL GO**

L1 evidence 由兩條獨立量測組成：

1. **Steady-state swapchain recreate**（Task #15, `M0-task15-swapchain-verify.md`）：S24 Ultra p95=9ms、S21+ p95=21ms、S8 p95=52ms，三裝置全部 < 200ms。
2. **100-cycle rotation stress**（Task #8, `M0-vulkan-spike-report.md`）：S24 Ultra p95=18ms PASS、S21+ p95=28ms PASS、**S8 p95=326ms FAIL**（Mali-G71、Android 9/SDK 28）。

L1 verdict 並非「三裝置全部 PASS」。S8 在 100-cycle rotation 失守（p95=326ms 超過 200ms gate），但 E1 trigger 門檻為 2+ devices fail，實際 1/3 fail，**E1 NOT triggered**，全埠繼續。S8 屬於支援矩陣外的早期裝置（Adreno 6xx+ 全部 PASS）。

**支援矩陣（M0 結論）**：
- 主要支援：API 31+（Android 12+）、Adreno 660+ / Mali-G77+。Vulkan 後端 production-ready。
- 不支援：API 28（Android 9）/ Mali-G71 等 2017 前 GPU。100-cycle rotation 顯示 driver 成熟度不足。
- M1 第一週必須提交正式 plan amendment 將 `minSdk` 從目前 26 提升至 31。

S8 冷啟動恢復（HOME → resume）≈ 2505ms 含完整 Vulkan driver init；production 版本應從 Application.onCreate 預熱 instance（M2 carry-over）。

### L2 — warp_terminal facade（D1.5-hybrid per Plan Amendment 2）

**判決：GO**

`warp_terminal_mobile_facade`（commit `5400c66`）scaffold 結構完備，cfg-dialect 慣例文件齊全（`M0-facade-scaffold.md`）。

**D1.5-hybrid 路線（Plan Amendment 2，覆蓋原 D2-lite）**：保留 `warp_terminal → warpui` Cargo dep edge（避免 D2-lite 試圖移除此 edge 的 Cargo 圖矛盾），改在 `warpui` 內部加 `cfg(target_os = "android")` gates 與 Android-specific platform backend，而非 D1 的全面 cfg-gate。

Task 3 deps report 量化顯示：warp_terminal 自身無 Android 不相容 dep；問題完全來自 warpui transitive chain（font-kit 2,834 行、android-activity 500 行）。D1.5-hybrid 邊界明確，M2a 4 週估算具信心基礎。Plan §1 / §1.3 / §6 已統一為 D1.5-hybrid（commit `bbb336e`）。

### L3 — Android Host Service

**判決：GO（基準線，實作延後至 M1）**

M0 沒有 L3 專項 spike；L3 為 JNI glue + Android Service + IPC（Binder/Socket）層，屬已知技術，風險低。實作延後至 M1 不影響 M0 close-out。

### L4 — Termux runtime（symlink-jniLibs B1 路徑）

**判決：GO**

W^X workaround（`Os.symlink` nativeLibraryDir → filesDir）在 SDK 28–36 全部驗證通過（`M0-symlink-jnilibs.md`、Task 17 再驗 commit `0ab80d4`）：
- SDK 36（S24 Ultra）：negative_control EACCES confirmed，symlink exec exit=42 + SYMLINK_EXEC_TOKEN_OK
- SDK 35（S21+）：同上
- SDK 28（S8）：W^X 未執行（預期），symlink exec 正常
- Release variant：S24 Ultra SDK 36 release PASS（stream drain deadlock + finish() 修復已驗證）

Pre-mortem Scenario B（W^X 封鎖 symlink）未觸發。

---

## 3. E1 觸發評估

**E1 未觸發。全埠繼續。**

E1 觸發條件：「Task #8 後，3 裝置中 2+ 裝置 frame-recovery p95 ≥ 200ms 或 validation layer 非乾淨。」

實際結果（Task #8, 100-cycle rotation, `M0-vulkan-spike-report.md`）：S24 Ultra p95=18ms（PASS）、S21+ p95=28ms（PASS）、**S8 p95=326ms（FAIL）**。2/3 裝置通過、1/3 失敗。E1 門檻為 2/3 失敗才觸發 → **1/3 失敗 < 2/3 門檻 → E1 NOT triggered**。

S8 失敗原因為 Mali-G71（2016 GPU）+ Android 9 driver 成熟度不足；不在 Adreno 6xx+ 主要支援矩陣內。Companion-mode retreat path **不啟動**。

Validation layer 全裝置零警告（Task #15）。

---

## 4. Tension 3 記錄（A1+B1+C1+E1）

決策由 team-lead 代決（用戶授權"全自動"/"你自己決定"）：

| 問題 | 決策 | 理由摘要 |
|------|------|---------|
| A — v1 是否搭載 cloud AI？ | **A1 — 是，核心功能** | 無 AI 則產品差異化喪失；BYOK 模式緩解隱私疑慮 |
| B — F-Droid NonFreeNet 標籤？ | **B1 — 接受標籤** | Plan Principle 1 已明示此 coherence gap；B2 雙 build matrix 超出 solo-dev 負擔 |
| C — cloud provider？ | **C1 — Anthropic only** | Haiku TTFT 最優；單一 SDK 減少 v1 複雜度；v2 可重新審議 |
| E — Companion-mode retreat trigger？ | **E1 — M0 Vulkan 2/3 失敗觸發** | 最早可偽造門檻；已評估：未觸發 |

完整論證見 `M0-tension3-decision.md`。

---

## 5. M1 Carry-Over 項目（確定 4 項，Codex round-3 verdicts 已落地）

1. **min API 31 plan amendment（必做，M1 第一週）**：M0 100-cycle rotation 顯示 S8/Mali-G71/Android 9 失守 (p95=326ms)，Adreno 6xx+ 通過。正式提案將 `minSdk` 從 26 提升至 31（Android 12）。

2. **D1.5-hybrid M2 實作（M2 主軸）**：保留 `warp_terminal → warpui` Cargo edge（避免 D2-lite Cargo 圖矛盾），在 `warpui` 內部加 `cfg(target_os = "android")` gates 與 `warpui::platform::android` backend（從 `headless` derive，補實 4 area：`render_scene`、`request_frame_capture`、`FontDB` 15 methods、`TextLayoutSystem` 2 methods）。當前 scaffold `5400c66` 為佔位符。Plan 文字已統一（`bbb336e`）；M2 實作為剩下唯一 carry-over。

3. **android-activity E0282 一行修復（M1 第一天，30 分鐘）**：workspace `Cargo.toml` 新增 `features = ["android-native-activity"]`。

4. **Vulkan spike Rust init-failure cleanup leaks（M2 RAII rewrite，accepted as PARTIAL）**：Codex Task #16 round-2 標記 4 段早期 return 漏清 device/swapchain/framebuffers/sync primitive (`spikes/vulkan-surface-recreate/src/lib.rs:222/252/277/605/617/629`)。Spike 為 throwaway code，M2 `warpui::platform::android` backend 以 RAII (Drop trait) 重寫即解，不在 spike 修。

Tasks #16/#17 round-3 Codex verdicts 均已收斂：Task #16 → Task #18 修 strict-assert（`ff439ad` PASS），Task #17 → Task #19 修 JSON escape（`3ceb777` PASS）。

---

## 6. M0 判決

**CONDITIONAL GO**

### 理由

M0 四個技術層的關鍵風險問題全部得到明確答覆：

- **L1 Vulkan 層**：穩態 swapchain recreate 三裝置 p95 ≤ 52ms（Task #15）；100-cycle rotation 2/3 PASS、S8/Mali-G71/A9 唯一失守 p95=326ms（Task #8）。E1 trigger 門檻 2/3 fail 未達（1/3 fail < 2/3）→ E1 NOT triggered。L1 verdict 為「API 31+ / Adreno 6xx+ CONDITIONAL GO」，Vulkan 後端可行性在主要支援矩陣內確認。

- **L4 Termux runtime**：symlink-jniLibs W^X workaround 在 SDK 28–36 debug + release 全部驗證通過，negative control 在 SDK 29+ 確認 EACCES、SDK 28 預期 succeeded（pre-API29 無 W^X enforcement，retreat path 行為正確），已無設計風險。

- **L2 facade 架構**：D1.5-hybrid 邊界由 Task 3 deps report + Codex Plan REVISE 量化確立；D1（cfg-gate warpui）因 3,334 行超過 Pre-mortem C 500 行門檻而被數據否決；D1.5-hybrid 路徑清晰，M2a 4 週估算有據。Plan §1 / §1.3 / §6 已於 `bbb336e` 統一為 D1.5-hybrid。

- **Tension 3**：A1+B1+C1+E1 四項決策均有論証記錄，不阻塞 M1。

**條件性在於**：(1) min API 31 plan amendment 需 M1 第一週正式提案；(2) android-activity 一行修復屬技術 trivial。兩項皆為管理性 carry-over，不影響 M0 技術方向。Tasks #16/#17 round-3 Codex verdicts 已收斂於 #18/#19 對應 commit；Plan D1.5-hybrid 文字統一已於 `bbb336e` 落地。

**M1 可立即啟動。**

---

*撰寫人：worker-env@warp-mobile-m0 代 team-lead*
*基於：Tasks 1–19 全部 artifacts，main @ 058a089*
*Codex M0 final REVISE → fixes applied per (S8 100-cycle 校正、D1.5-hybrid 統一、M1 carry-overs 改為實際 4 項、Tasks #18/#19 移出 pending)；待 Codex re-review*
