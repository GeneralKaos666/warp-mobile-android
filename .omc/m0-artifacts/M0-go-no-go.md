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
| 8 | [USER] Vulkan spike 三裝置 frame-recovery 量測 | **PASS** | S24 Ultra p95≈9ms、S21+ p95≈21ms、S8 p95≈52ms；全部 < 200ms 門檻（`M0-task15-swapchain-verify.md`） |
| 9 | [USER] Tension 3 user gate decision | **PASS** | A1+B1+C1+E1 由 team-lead 代決（"全自動" 授權）（`M0-tension3-decision.md`） |
| 10 | M0 go/no-go 整合 | **本文件** | — |
| 11 | Vulkan spike APK build + adb 量測腳本 | **PASS** | APK 3.7 MB、`lib/arm64-v8a/` .so 驗證、三裝置 install 成功（`M0-task11-install-verify.md`） |
| 12 | symlink-jniLibs execve() test harness | **PASS** | 初版 harness；Task 14 redo 取代（詳見下） |
| 13 | Fix Surface native handle（ANativeWindow_fromSurface） | **PASS** | 移除 reflection；改用 NDK public API `ANativeWindow_fromSurface`（API 26+）（`M0-task13-surface-fix-verify.md`） |
| 14 | symlink-jniLibs redo per Codex REVISE | **PASS** | 4-config 全 passed=true（SDK 36/35/28、debug+release）；negative_control EACCES 確認；`targetSdk=36`（`M0-symlink-jnilibs.md`） |
| 15 | Vulkan spike B-F items（swapchain/validation/lifecycle/shell） | **PASS** | 真實 swapchain + render pass；validation layer 零警告；三裝置 first_frame_presented_ts 正常（`M0-task15-swapchain-verify.md`） |
| 16 | Vulkan spike Codex round-2 follow-up fixes | **PASS** | strict-assert cycle count、init-failure cleanup paths、configChanges scope comment（commit `1048a1e`） |
| 17 | symlink errno capture cleanup（Codex round-2 PARTIAL/FAIL） | **PASS** | sentinel `NEGATIVE_ERRNO_BEGIN...END`、`OsConstants.errnoName()`、`negative_errno_name` JSON 欄位；4-config 再驗證 EACCES/succeeded（commit `0ab80d4`） |

---

## 2. 決策矩陣（逐層）

### L1 — WarpUI Android Vulkan 後端

**判決：GO（含注意事項）**

三裝置穩態 swapchain recreate p95：S24 Ultra ≈ 9ms、S21+ ≈ 21ms、S8 ≈ 52ms，全部遠低於 200ms 目標門檻（`M0-task15-swapchain-verify.md`）。驗證層零錯誤、零警告，`first_frame_presented_ts` 在所有裝置正確發出。

**注意事項（min API 31 提案）**：S8（Mali-G71、Android 9/SDK 28）p95 為 52ms，屬可接受但為三裝置最慢。Mali-G71 屬 OpenGL ES 3.2 世代 GPU，Vulkan 1.0 支援完整但驅動成熟度低於 Adreno 6xx/7xx。若 M2 穩態目標更嚴格（< 20ms），建議向 team-lead 提交「min API 31 plan amendment」——Adreno 619+（API 30）以上裝置均達 21ms 以下。此為 M1 carry-over 議題，不封鎖 M0。

S8 冷啟動恢復（HOME → resume）≈ 2505ms，含完整 Vulkan driver init；此屬實作特性非缺陷，正式量產版應從 Application.onCreate 預熱 Vulkan instance。

### L2 — warp_terminal facade（D1.5-hybrid per Plan Amendment 2）

**判決：GO**

`warp_terminal_mobile_facade`（commit `5400c66`）scaffold 結構完備，cfg-dialect 慣例文件齊全（`M0-facade-scaffold.md`）。Plan Amendment 2 確立 D2-lite：facade 在 M2 起排除 warpui dep 圖，改依賴 warp_terminal 的乾淨子集（`warp_completer`、`warp_core`、`vte` 等）；warpui 內部以 `cfg(target_os = "android")` gates 修改，而非 D1 的全面 cfg-gate。

Task 3 deps report 量化顯示：warp_terminal 自身無 Android 不相容 dep；問題完全來自 warpui transitive chain（font-kit 2,834 行、android-activity 500 行）。D2-lite 邊界明確，M2a 4 週估算具信心基礎。

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

實際結果：S24 Ultra p95 ≈ 9ms（PASS）、S21+ p95 ≈ 21ms（PASS）、S8 p95 ≈ 52ms（PASS）。3/3 裝置通過，觸發門檻為 2/3 失敗 = **1/3 失敗 < 2/3 門檻**，E1 **不觸發**。

S8 Mali-G71 p95 52ms 屬硬體/驅動代際因素，非設計缺陷。不計為失敗。

Companion-mode retreat path **不啟動**。

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

## 5. M1 Carry-Over 項目

1. **Codex round-3 re-review 待結果**：Tasks #16（Vulkan spike）與 #17（symlink errno）各自有 `codex-rereview-task16.md` 和 `codex-rereview-task17.md` 評審任務在進行中。最終 CODEX_PASS/REVISE 判決應在 M0 close-out 前落地；若任一為 REVISE，新任務進入 M1 backlog。

2. **min API 31 plan amendment 提案**：S8（API 28、Mali-G71）p95 52ms 屬可接受，但若 M2 目標更嚴格，需正式提案將 `minSdk` 從 26 提升至 31（Android 12）。M1 第一週提案。

3. **D1.5-hybrid M2 實作**：facade 真正排除 warpui dep（移除 `warp_terminal` direct dep，改依賴乾淨子集）。當前 scaffold commit `5400c66` 為佔位符；M2 起需正式 cfg-gate。

4. **android-activity E0282 一行修復**：workspace `Cargo.toml` 新增 `features = ["android-native-activity"]`。30 分鐘 spike，建議 M1 第一天確認。

5. **Tasks #18、#19 待處理**：#18（Vulkan strict-assert + stale comment）、#19（symlink JSON escape via jq -n）為輕量清理任務，可在 M1 初期作為 warm-up 完成。

---

## 6. M0 判決

**CONDITIONAL GO**

### 理由

M0 四個技術層的關鍵風險問題全部得到明確答覆：

- **L1 Vulkan 層**：三裝置穩態 p95 均低於 200ms，validation layer 零錯誤，swapchain recreate lifecycle 正常，E1 retreat trigger 未觸發。Vulkan 後端可行性確認。

- **L4 Termux runtime**：symlink-jniLibs W^X workaround 在 SDK 28–36 debug + release 全部驗證通過，negative control 確認 EACCES，此為 Termux binary 執行的唯一可行路徑，已無設計風險。

- **L2 facade 架構**：D2-lite 邊界由 Task 3 deps report 量化確立，D1（cfg-gate warpui）因 3,334 行超過 Pre-mortem C 500 行門檻而被數據否決；D2-lite 路徑清晰，M2a 4 週估算有據。

- **Tension 3**：A1+B1+C1+E1 四項決策均有論証記錄，不阻塞 M1。

條件性在於：Codex round-3 對 Tasks #16/17 的最終 CODEX_PASS 待確認，以及 min API 31 提案需在 M1 第一週決議。這兩項屬管理性 carry-over，不影響技術方向。

**M1 可在上述 carry-over 確認後立即啟動。**

---

*撰寫人：worker-env@warp-mobile-m0 代 team-lead*
*基於：Tasks 1–17 全部 artifacts，branch warp-mobile/m0-symlink-redo @ 0ab80d4*
