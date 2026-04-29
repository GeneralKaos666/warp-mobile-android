# RALPLAN: Warp Terminal on Android (Open-Source First Port)

> **Mode**: DELIBERATE consensus plan (high-risk: 12-18 month porting, GPL fork, multi-platform native, AGPL compliance)
> **Status**: APPROVED iter 2 (Planner+Architect+Critic) → **M0 partial findings amended (2026-04-29 evening)**
> **Upstream commit referenced**: `warpdotdev/Warp@d0f045c` (just-released 2026-04-28)
> **Author**: Planner agent (RALPLAN-DR deliberate); **Amendment 1 by team-lead@warp-mobile-m0** post-M0 autonomous tasks
> **Date**: 2026-04-29

---

## ⚠️ Amendment 2 (2026-04-29 night) — Codex review revised D2-lite to D1.5-hybrid; 6 follow-on corrections

**Tl;dr**: Codex independent review (`.omc/artifacts/ask/codex-...12-38-36-474Z.md`, verdict `CODEX_REVISE_PLAN`) caught a logic gap that the deliberate Planner+Architect+Critic loop missed: D2-lite as written contradicts itself ("exclude `warpui` from facade dep graph" + "reuse `crates/warp_terminal`" cannot both hold, because `crates/warp_terminal/Cargo.toml:36` directly depends on `warpui` and the source uses `warpui::keymap::Keystroke`, `warpui::platform::OperatingSystem`, `warpui::units::Lines` types). Adoption: **D1.5-hybrid** (modify `warpui` internally with Android target_os cfg gates so it does NOT pull `font-kit` or desktop `winit` on Android, and add `warpui::platform::android` derived from `headless`; keep `warp_terminal -> warpui` Cargo edge intact).

**Six corrections** committed in this Amendment:

1. **Decision D revised D2-lite → D1.5-hybrid** (Section 1.3 Decision D below). Keep `warp_terminal -> warpui` Cargo dep; modify `warpui/Cargo.toml` to make `font-kit` and desktop `winit` deps `cfg(not(target_os = "android"))`; add `crates/warpui/src/platform/android/` derived from `headless`. Facade crate (`warp_terminal_mobile_facade`) becomes optional, only used if specific app-layer cfg-gates exceed budget after M3 archeology.

2. **3,334-line wording correction** (`Amendment 1 Tl;dr` above). The 3,334 figure is a "scope proxy" (existing-files-needing-isolation + new-Android-backend-LOC estimate), not literal cfg-gate count. Math: 2,834 (font-kit-touching files in `warpui` + `warpui_core`) + 500 (estimated new Android windowing backend LOC) = 3,334 ÷ 500 threshold = 6.7×. The threshold trigger remains valid; the language was misleading.

3. **M2a 4 weeks → 5-7 weeks; new M2a-font sub-gate** (Section 5 M2 + Section 6 M2 below). FontDB-via-cosmic-text was over-optimistically estimated. `headless/app.rs:46-47` actually injects `platform::test::FontDB` (empty glyphs, zero advance, empty layout). Real desktop path is `warpui/src/windowing/winit/fonts.rs` ~1,270 lines covering fontdb ID mapping, fallback chains, glyph rasterization, line/text layout. cosmic-text wraps the basics but not "cleanly absorb 15 methods". M2a-font becomes a discrete gate: must produce CJK text rendering at 60fps on flagship before M2a-render proceeds.

4. **M2a acceptance criteria hardened** (Section 5 M2). Real swapchain + render pass + validation-layers-clean + `VK_ERROR_OUT_OF_DATE_KHR`/`VK_SUBOPTIMAL_KHR` recovery paths + surface destroy/create with proper fence/thread shutdown sequencing. The current Vulkan spike's `VkSurface`-only validation is insufficient for "L1 risk verified at M0" claim.

5. **Device matrix unification** (search-and-replace across plan). The original Pixel 4a/7a/9 Pro/Galaxy A14 references at lines 179-183, 305, 395, 402 must be replaced with the actual three-device matrix (S24 Ultra/S21+/S8). M2 close gate adds: at least one Pixel/AOSP/Tensor lane device must be sourced before M2 close, NOT deferred to v1-release backfill (Codex flagged single-OEM Samsung-only as a real risk).

6. **License per-package SPDX manifest + GPLv2-only-static-link prohibition** (M4 + Cross-cutting). The `.omc/handoffs/team-plan.md:8` "AGPL + GPL Termux compatible" claim is too coarse. APK bundles include hundreds of Termux packages with heterogeneous licenses; M4 must produce a per-package SPDX manifest + source-offer URL + explicit prohibition: NO GPLv2-only binary statically linked into AGPL JNI module (only dynamic / external-process boundaries permitted for GPLv2-only).

7. **Solo-dev rhythm budget** (Section 5 + Section 6). M0+M1+M2 = 18-25 weeks continuous high-risk Rust+Vulkan+Android work. Add: 1-week merge-maintenance buffer between M1↔M2, hard-stop reflection point after M2a (decide continue vs Companion retreat), 2-week burnout buffer after M3 before M4 begins. Total budget moves from 13-18 → 14-20 months for v1 constrained beta. Per Plan Principle 5 risk-first ordering: post-M2a stop is the highest-leverage retreat opportunity.

---

## ⚠️ Amendment 1 (2026-04-29) — D1 invalidated by M0 evidence

**Tl;dr**: M0 worker-env Task 3 measured cfg-gate scope at **3,334 lines** — **6.7× the Pre-mortem C 500-line threshold**. Decision D1 (cfg-gate everywhere) is **formally invalidated**. Amendment 1 first adopted **D2-lite** (`warp_terminal_mobile_facade` excludes `warpui` from its dep graph). Amendment 2 (below) revised this to **D1.5-hybrid** after Codex review found a Cargo graph contradiction in D2-lite. **The currently active decision is D1.5-hybrid** (see Amendment 2 above for details, Section 1.3 Decision D for table, Section 6 M2 for milestones).

**Three convergent findings from M0** (all artifacts under `.omc/m0-artifacts/`):
1. **`warp_terminal` itself clean; `warpui` is the contamination** — cfg-gate 3,334 lines distributed across 8 files / `font-kit` (~2,834 lines) + `winit`+`android-activity` (~500 lines).
2. **`gpui-mobile` formally rejected by evidence** — 89 trait methods × {0% identical, 35% portable, 15% incompatible, 50% missing}. gpui-mobile implements Zed's `gpui::Platform`, not Warp's `warpui_core::platform::*`.
3. **A4 (`headless` base) confirmed ~3-4 person-weeks** (vs A1 `linux` 6-8 weeks). 89/89 methods stubbed in `headless`; only 4 areas need real work.

**Device matrix update**: original plan named Pixel 7a (Android 14) + Galaxy A14 (Android 15) + Pixel 9 Pro (Android 16-Beta). User has connected three Samsung Galaxy devices forming a dialectically tight tier matrix (each SDK step +7):
- **Galaxy S24 Ultra SM-S928x** (Snapdragon 8 Gen 3, **Android 16 production, SDK 36**, arm64-v8a) → strictest-W^X-policy-yet flagship path (production Android 16, more stable than original plan's Pixel 9 Pro Beta)
- **Galaxy S21+ SM-G996x** (**Android 15, SDK 35**, arm64-v8a) → mid-tier modern-OS daily-driver tier (Snapdragon 888 / Exynos 2100; covers the original plan's Pixel 7a + Galaxy A14 mid-tier slot in one device)
- **Galaxy S8 SM-G950F** (**Android 9, SDK 28**, arm64-v8a) → `targetSdkVersion 28` retreat baseline (Pre-mortem B mitigation validation; pre-W^X-strict era reference)
- Bonus: same-OEM (Samsung OneUI) reduces SELinux/Bionic variance vs original Pixel/Samsung mix; multi-vendor diff deferred to v1-release backfill.

---

## Section 1: RALPLAN-DR Summary

### 1.1 Principles (5)

1. **Open-source distribution first** — GitHub Releases + F-Droid is the primary channel. Play Store is a v3+ optional path, NOT a launch dependency. This sidesteps Android W^X enforcement on `targetSdkVersion 29+` for distributable binaries. *Coherence gap acknowledged*: v1 ships with proprietary cloud AI dependency (Anthropic API); F-Droid users must self-supply API key or disable AI; this is a known coherence gap relative to the "open-source first" stance and will surface as F-Droid's NonFreeNet anti-feature label.
2. **Cloud AI before local LLM** — Claude Haiku (inline ghost-text) + Sonnet (agent) ship in MVP. Local llama.cpp Qwen2.5-Coder-1.5B Q4_K_M is v2+ feature, opt-in only, RAM-gated to ≥6GB devices, mmap-backed.
3. **Self-implement `warpui::platform::android`** — `gpui-mobile` is for Zed's GPUI, NOT Warp's `warpui`. Cross-port from Linux backend, do NOT take a dependency on `gpui-mobile`. *(Note: `warpui_core::platform/` only contains app/file_picker/keyboard/menu/wasm shims — actual platform backends live in `crates/warpui/src/platform/{linux,mac,windows,wasm,headless}`.)*
4. **Fork-and-narrow over rebuild** — Maintain a long-lived fork of `warpdotdev/Warp` and `termux-packages` rather than recreating the wheel. Cherry-pick upstream into a `mobile/main` branch periodically. Stay GPL-clean.
5. **Verify the riskiest layer first** — Death-pit ranking dictates execution order: M0 (now expanded) validates F-Droid + NDK build **plus the L1 renderer's #1 risk via a 2-day Vulkan-Surface-recreate spike + a `warpui::platform` trait diff + an empty `warp_terminal_mobile_facade` scaffold**, M1 validates PTY+lifecycle on Bionic, M2 implements the full renderer on top of M0's de-risked surface. Termux runtime (L3) is intentionally late because the F-Droid path defuses it. **Enforcement narrative**: Architect raised that the original M0 was PTY-plumbing-only and renderer was deferred to M2, masking the #1 risk for 8-10 weeks. The revised M0 (4-5 person-weeks) means M0 itself validates L1's top risk, not M2.

### 1.2 Decision Drivers (top 3)

1. **AGPL-3.0 license obligation** — Workspace `Cargo.toml#L25-28` declares AGPL-3.0-only on the Warp client crate. Source disclosure is mandatory; this constrains how we structure proprietary integrations (none planned), pin dependencies, and how we publish. F-Droid alignment with AGPL is excellent; Play Store is murky for AGPL apps, reinforcing principle #1.
2. **Solo-dev sustainability over 12-18 months** — A one-person crew cannot implement a from-scratch GPU compositor + IME + PTY + agent runtime + package store in 12 months. Every major decision must trade against scope. Prefer fork+derive over rewrite, prefer cloud AI over self-hosted, prefer 2 of 3 mobile UX features over all of them.
3. **Android process kill economy** — PhantomProcessKiller (`DEFAULT_MAX_PHANTOM_PROCESSES = 32`) silently culls children of FGS. FGS protects only the app process priority, not its descendants. Every PTY+package operation must be designed knowing children can vanish. This shapes IPC, recovery, and the bootstrap-zip-in-APK choice.

### 1.3 Viable Options (sub-decisions)

#### Decision A: How to render Warp UI on Android?

| Option | Pros | Cons |
|---|---|---|
| **A1: Self-implement `warpui::platform::android` deriving from `linux` backend** *(chosen, pending M0 archeology)* | Direct compatibility with Warp's existing `Delegate`/`DispatchDelegate`/`FontDB`/`TextLayoutSystem`/`Window`/`WindowContext`/`WindowManager` traits; no third-party black box; we control the Vulkan glue code. | High effort (8-12 weeks for static grid, more for parity). Vulkan-on-Android lifecycle (Surface lost on app pause/rotate) is well-known but tedious. IME glue is hostile. |
| **A2: Depend on `itsbalamurali/gpui-mobile`** *(rejected)* | Promised future Android backend; could in theory delegate platform layer. | Targets Zed's GPUI, not Warp's `warpui`. Trait surfaces differ. docs.rs admits "full iOS/Android implementations coming soon" with only momentum scrolling currently exposed. Adopting it would make us co-maintainers of an upstream we don't control, AND we'd still have to bridge to `warpui`. |
| **A3: Replace renderer with Compose / Jetpack** *(rejected)* | Familiar Android stack; massive ecosystem. | Throws away Warp's entire UI architecture (`warpui` is the product); we'd be rewriting Warp, not porting it. 12-18 months becomes 36+ months. License confusion (mixing JVM-side Android with Warp's AGPL Rust core). |
| **A4: Derive from `warpui::platform::headless` (existing minimal backend)** *(chosen by Amendment 1; A4 confirmed by M0 archeology)* | No X11/Wayland/Cocoa baggage; smallest trait surface to wrap. M0 worker-archeo confirmed: 85/89 trait methods are already stubbed in `headless`; only 4 areas need real implementations (`render_scene`, `request_frame_capture`, `FontDB` 15 methods, `TextLayoutSystem` 2 methods). Estimate **3-4 person-weeks** vs A1's 6-8 weeks. Cleanly avoids the font-kit + winit transitive failures that invalidated D1. | The 4 hand-written areas are non-trivial: `FontDB` requires wrapping `cosmic-text` + Android system fonts, `render_scene` requires `ash` Vulkan glue + `ANativeWindow` integration. Estimated risk concentrated in cosmic-text wrapping (highest schedule variance). |

**If only A1 viable**: Currently A1 is the working assumption pending M0 archeology that re-evaluates A4 (headless) and `wasm` (which may already carry mobile hints). A2 invalidated by trait incompatibility (verifiable by `grep`-ing Warp's `crates/warpui/src/platform/mod.rs` traits vs gpui-mobile's exports), A3 invalidated by scope explosion. Final A1-vs-A4 selection becomes an M0 deliverable.

#### Decision B: How to ship `$PREFIX` for shells/binaries?

| Option | Pros | Cons |
|---|---|---|
| **B1: Bundle bootstrap zip in APK, extract to app private dir, fork `termux-packages` with project's package name as new prefix** *(chosen)* | Self-contained; no Play Store policy entanglement at v1; mirrors Termux's proven model. Allows `targetSdkVersion 28` on F-Droid path to retain `execve()` from writable home. | Fork maintenance debt (every Termux package needs prefix retargeting). Bootstrap zip adds ~20-50MB to APK. Symlink-trick or `targetSdk 28` may break on future Android (16+) — has to be monitored. |
| **B2: Depend on Termux app via Intent/RPC** *(rejected)* | Zero packaging work; Termux's package ecosystem stays current. | Cannot ship to F-Droid as a dependency on another F-Droid app for shells (loose coupling = poor UX). Cross-app FGS doesn't transfer process priority. Distribution becomes "install Termux first" — fails the "open the app and it works" UX bar. |
| **B3: Build only essential binaries (zsh, bash, curl, git) statically into APK, no `$PREFIX` filesystem** *(rejected)* | Smaller surface; no Termux fork. | No package install story (`pkg install` impossible). Loses huge chunk of value prop (people use Warp partly because their dev env is there). Effectively becomes a fancy `ssh` client. |

#### Decision C: Where does AI live in MVP?

| Option | Pros | Cons |
|---|---|---|
| **C1: Cloud-only (Anthropic API: Haiku ghost-text, Sonnet agent), local LLM v2+** *(chosen)* | Zero on-device VRAM/RAM concerns; quality is guaranteed; small APK; no llama.cpp NDK build pain at MVP. | Requires user API key; offline = no AI. Latency on poor networks. AGPL+API doesn't conflict, but BYOK UX is a hurdle. |
| **C2: Local llama.cpp at MVP** *(rejected at MVP, reconsidered v2+)* | Offline; no API key needed; "free". | Qwen2.5-Coder-1.5B Q4_K_M is ~1.2GB; OOM-killer trips on 4GB-RAM mid-tier devices despite FGS (LMK kills under memory pressure regardless of FGS); inference is too slow on most non-flagship NPUs for ghost-text UX (sub-200ms target). NDK build of llama.cpp is non-trivial. |
| **C3: Hybrid from day 1 (local 1.5B + cloud Sonnet)** *(rejected)* | Best of both. | Doubles testing matrix at MVP. M0-M3 already overcommitted. Adds NDK build, RAM gating, model download UX, mmap setup — every one a multi-week task. Defer to v2+. |

#### Decision D: How to handle Warp's tangled core (warp_terminal depending on warpui, TerminalModel pulling AI/FeatureFlag/AppContext/SSH)?

> **Amendment 1 (2026-04-29 evening)**: D1 invalidated by M0 evidence (scope-proxy 3,334 LoC > 500 threshold). **D2-lite was chosen.**
>
> **Amendment 2 (2026-04-29 night)**: Codex review revised D2-lite to **D1.5-hybrid** because D2-lite contradicted Cargo graph (`warp_terminal/Cargo.toml:36` directly imports `warpui`; source uses `warpui::keymap::Keystroke`, `warpui::platform::OperatingSystem`, `warpui::units::Lines` types). **D1.5-hybrid is the chosen option.**

| Option | Pros | Cons |
|---|---|---|
| **D1: Surgical cfg-gating (`#[cfg(not(target_os = "android"))]`) at the dependency edges, with progressive extraction of pure-Rust facade as M3 progresses** *(rejected by M0 evidence)* | Keeps fork tractable; lets us merge upstream changes; doesn't require a clean architecture pre-condition. | **Empirically blown out**: M0 worker-env Task 3 measured 3,334 cfg-gate lines (font-kit ~2,834 lines + winit/android-activity ~500 lines), 6.7× Pre-mortem C threshold (500). Code becomes uglier than estimated; every upstream font/winit change re-enters our cfg-gate. |
| **D2-lite: `warp_terminal_mobile_facade` excludes `warpui` entirely from its dep graph; Layer 1 self-implements 4 areas** *(superseded by Amendment 2 → D1.5-hybrid)* | Originally chosen in Amendment 1 because warp_terminal's direct deps were thought clean. Codex review found `warp_terminal/Cargo.toml:36` directly deps on `warpui` and source uses `warpui::keymap::Keystroke`/`warpui::platform::OperatingSystem`/`warpui::units::Lines` — Cargo graph contradiction. | Removing the warp_terminal→warpui edge would require either upstreaming a refactor (rejected as D2) or carrying a heavy fork patch. |
| **D1.5-hybrid: keep `warp_terminal → warpui` Cargo edge; modify `warpui` internally with `cfg(target_os = "android")` gates on `font-kit` and desktop `winit` deps; add `crates/warpui/src/platform/android/` derived from `headless`; Layer 1's 4 hand-written areas land inside the new `warpui::platform::android` module** *(chosen, Amendment 2)* | Resolves D2-lite Cargo contradiction. cfg-gate scope is bounded (`warpui/Cargo.toml` + new `platform/android/` module, NOT 3,334 lines like D1). 85/89 methods still derive from `headless`. Facade crate (`warp_terminal_mobile_facade`) becomes optional escape hatch only. M2a still 4 person-weeks. | Modifying `warpui` internally means our fork now has a non-trivial patch on top of upstream Warp. Upstream re-merge cost: medium (touches dep manifest + new platform module, but each on a clear boundary). |
| **D2: Refactor warp_terminal into a clean platform-agnostic core crate first, upstream the refactor PR** *(rejected)* | Cleaner. Upstream might even take the PR. | Refactoring someone else's just-open-sourced AGPL codebase before they have stable contributor process is a classic time-sink. Could spend 6 months on PR review cycles. Solo dev cannot afford it. |
| **D3: Extract a tiny terminal-only sub-crate, ignore rest of Warp, write our own block/agent layer** *(rejected)* | Minimal binary; no AGPL drag from Warp's heavy crates. | Defeats the entire point of "porting Warp" — we'd just be making another mobile terminal. The blocks + agent UX are the moat. |

**D1 → D1.5-hybrid migration steps (M2 prep, supersedes original D2-lite migration list)**:
1. `crates/warpui/Cargo.toml`: gate `font-kit` and desktop `winit` deps under `[target.'cfg(not(target_os = "android"))'.dependencies]`. Add `[target.'cfg(target_os = "android")'.dependencies]` block for Android-specific replacements (`cosmic-text`, `ash`, `ndk-sys`).
2. `crates/warpui/src/platform/android/` (new): copy `headless` mod, hand-write the 4 areas (`render_scene` Vulkan via `ash` + `ANativeWindow`; `request_frame_capture` ash readback; `FontDB` 15 methods via `cosmic-text`; `TextLayoutSystem` 2 methods). Aim for `cargo ndk -t arm64-v8a check -p warpui` to pass with `target_os = "android"` cfg active.
3. `warp_terminal -> warpui` Cargo edge stays intact. `warp_terminal_mobile_facade` (commit `5400c66`) demoted to optional escape hatch only — used if specific app-layer cfg-gates exceed budget after M3 archeology.
4. M2 split per Section 6 below (M2a 4 weeks for the 4 areas, M2b 4-6 weeks for IME/lifecycle/perf).

---

## Section 2: Pre-mortem (3 scenarios, deliberate-mode required)

### Scenario A: M2 WarpUI Android backend stuck (Vulkan/IME/rotation)

**Narrative**: It is 2026-09 (5 months in). M0 and M1 went smoothly — the F-Droid build pipeline works, NDK r25c with `cargo-ndk` builds clean, the headless Android service can `openpty()` + `setsid()` + `TIOCSCTTY` and survive Activity recreation, **and the M0 Vulkan-Surface-recreate spike validated the basic lifecycle on 3 devices**. M2 begins: deriving `warpui::platform::android` from `linux` (or `headless` per M0 archeology). Within four weeks we have a Vulkan surface and a static glyph atlas painting "Hello" to the screen. Then production-scale issues surface that the M0 spike's 50-line scope could not catch. Rotation triggers a Surface destroy/recreate cycle that races with the render thread under real frame load. Soft-IME (Gboard) shows on top of our framebuffer because we missed `WindowInsets`. Latin input works; Chinese composition (`InputConnection.setComposingText`) doesn't paint. By 2026-12 we are 7 months in with a renderer that crashes on rotation 30% of the time under load and cannot accept Chinese input. Solo-dev burnout looms.

**Early warning signals**:
- Week 2 of M2: Vulkan validation layer prints `VK_ERROR_OUT_OF_DATE_KHR` on every device-orientation change.
- Week 3 of M2: any `Activity.recreate()` causes >1s black frame.
- Week 4 of M2: `InputConnection` callbacks fire but no glyph appears for composing text.
- Cross-cutting: Pixel 4a (low-end) shows 8fps even on idle; Pixel 8 Pro is fine. Performance gap >5x.

**Prevention/mitigation**:
1. **Spike Vulkan-Surface-recreate in M0**, NOT M2. Before committing to M2's full backend, prove a 50-line Rust+JNI standalone that holds a `VkSurfaceKHR` across `onPause`/`onResume` and rotation on 3 reference devices (Pixel 7a, Galaxy A14, Pixel 9 Pro). Two-day timebox per device; output frame-recovery-time-ms numbers in `M0-vulkan-spike-report.md`. If it fails, fall back to GLES 3.2 backend (Warp's `warpui` likely has a fallback path; verify [unverified]).
2. **Vendor a known-good Android Vulkan lifecycle template**, e.g., Khronos's `vulkan-samples/api/swapchain_recreation`. Don't invent.
3. **IME spike before week 5 of M2**: build a "type Chinese, see it render" demo on a stub renderer (ASCII-only) before integrating with Warp's text layout. CJK composition is high-risk; isolate it.
4. **Set a 12-week budget for M2 with a hard checkpoint at week 8**: if Vulkan + IME + rotation are not all green at week 8, escalate (cancel/redesign or accept GLES fallback).
5. **Have a "Compose host shell" backup plan**: if `warpui::platform::android` proves intractable, render text-only via Compose + JNI, deferring Warp's blocks UI to v3. Demote this from a primary plan to a "kill-switch" — only invoked if mitigation 1-4 fail.

### Scenario B: L3 Termux runtime — F-Droid path hits a wall

**Narrative**: It is 2027-02 (10 months in). M2 is shipping, M3 (Warp core integration) is mid-stream, and we begin M4 (Termux bootstrap). We fork `termux-packages`, change `$PREFIX` to `/data/data/io.warp.mobile/files/usr`, and rebuild the bootstrap zip. On Pixel 7 (Android 14) it works. On a Pixel 9 (Android 16) shipping mid-2026, `execve()` of files in writable app home is silently blocked even with `targetSdkVersion 28` — Google has tightened the symlink-jniLibs trick. F-Droid policy is unchanged but devices on the latest Android can't run our shells. Without shells, the entire app is decoration. We've already shipped two betas advertising "real bash on Android."

**Early warning signals**:
- M0 spike: any new Android Quarterly Platform Release / Beta blocks `system_linker_exec` arbitrary `$PREFIX`.
- Pre-M4: Termux community discussions about Android 16 changes (watch `termux/termux-app` issues monthly).
- M4 week 2: `termux-exec` integration gives `EACCES` on at least one device with current public Android beta.
- Cross-cutting: Google's Play Integrity API begins flagging W^X-bypassing apps in any context (would predict a future enforcement wave).

**Prevention/mitigation**:
1. **M0 includes "on the latest Android Beta channel device, verify mutable-prefix shell spawn"** — not just on stable Android 14/15. Buy/borrow a Pixel running the latest Beta from day 1.
2. **Maintain a quarterly review of Android platform release notes**, with explicit checks for: PROT_EXEC restrictions, `/data/data` filesystem mounts changes, jniLibs symlink trick status, `system_linker_exec` fate, `seccomp-bpf` defaults. Block release of any version that is regressed.
3. **Diversify the shell-spawn mechanism**:
   - Primary: writable `$PREFIX` + `system_linker_exec` (Termux model).
   - Fallback A: Static-link a single-binary busybox+bash+coreutils, ship inside `lib/<abi>/libshell.so` (jniLibs are `PROT_EXEC` by Android contract, even on `targetSdk 35+`), exec via `dlopen + dlsym(main)` trick. Loses package install but preserves shell.
   - Fallback B (radical): Move shell execution to a server-side sandbox via Tailscale/SSH, mobile is just a glass terminal. Loses offline, regains future-proofing.
4. **Define a shell health canary in `Stability` test suite** that runs on every Android Developer Preview within 48h of release, fails the CI if shell spawn breaks. Must be automated.
5. **Communicate openly in release notes about platform-specific shell paths.** Set user expectation that "Android 16+" might require a non-default install method.

### Scenario C: L2b Warp Product Logic (the `app` crate) pulls too many desktop deps that can't be cleanly cfg-gated

**Narrative**: It is 2026-08 (4 months in). M2's renderer is rendering. We begin M3 (integrate Warp's product logic). **Reality check** (verified by `grep` on upstream `d0f045c`): `crates/warp_terminal/Cargo.toml` direct deps are CLEAN — only `warpui`, `warp_completer`, `warp_core`, `warp_util`, `vte`, etc. The tangle lives in the **`app` crate**: `app/Cargo.toml` declares `mio = "1.1.1"` (event loop with Bionic-incompatible `signal` semantics), `nix.workspace = true` (some PTY ioctls glibc-only), `ai.workspace = true` (pulls in `tokio-rustls`/cloud-AI plumbing), and `feature_flag.workspace = true` (process-global state, file-watch-based on `notify`). On top of that, the `app` crate's `terminal/` module re-exports types from `warp_terminal` and crosses into `app::ai` / `app::ssh` / `app::feature_flag` / `app::app_context`. The compiler errors when targeting `aarch64-linux-android -p app` are immediate and cascading. Cfg-gating each one creates a fractal of `#[cfg(not(target_os = "android"))]` inside `app/`. By 2026-11 (7 months) we have 2,000+ cfg lines in `app/`. Every upstream merge from `warpdotdev/Warp` causes 50+ merge conflicts in `app/`. Cherry-pick velocity drops to one commit per day.

**Why this matters for layer labelling**: the original draft conflated `warp_terminal` and `app` as the same "tangled core". They are not. **Layer 2a (Terminal Session Engine)** is the clean side: `crates/warp_terminal` + `crates/warpui` + `crates/warp_core` + `crates/warp_completer` + `crates/warp_util`. **Layer 2b (Warp Product Logic)** is the tangled side: a curated subset of `app/src/terminal/...` modules + `app::ai` + `app::feature_flag` + `app::ssh` + `app::app_context`, and our new `crates/warp_terminal_mobile_facade` that re-exposes a thin API to the Android JNI layer.

**Early warning signals**:
- M3 week 2: `cargo build --target aarch64-linux-android -p app` produces >100 errors (note: `-p app`, not `-p warp_terminal`; `warp_terminal` itself likely builds clean once `warpui::platform::android` is stubbed).
- M3 week 4: cfg-gate count in `app/src/terminal/**` crosses 500 lines.
- M3 week 6: a single upstream merge takes >4 hours, mostly in `app/`.
- Cross-cutting: `cargo expand --target aarch64-linux-android -p app` shows entire `ai`/`ssh`/`feature_flag` modules excluded — meaning entire features are silently disabled on Android.

**Prevention/mitigation**:
1. **Compile-test M3 dependencies in M0**, not M3. As part of the M0 spike, run `cargo check --target aarch64-linux-android` against (a) a tiny Rust binary importing `warp_terminal` (clean side; expected to mostly work once `warpui::platform::android` stub exists) and (b) the `app` crate (tangled side; expected to fail loudly). Quantify the dependency leakage on the `app` side specifically. If it shows >50 unbuildable deps in `app`, the facade-crate detour is mandatory, not optional.
2. **Adopt the "Android facade crate" pattern**: scaffold `crates/warp_terminal_mobile_facade` in M0 (empty crate, cfg-dialect pre-declared). The facade re-exports a minimal subset of `app::terminal::*` API and provides Android-native implementations of `AppContext`, `FeatureFlag`, `SSH`, etc. The facade crate carries the cfg lines; `warp_terminal` stays untouched; `app` gets surgical cfg-gates only at the dependency edges that the facade can't bypass.
3. **Quarantine SSH and AI to separate crates from day 1**. They are the highest-leverage things to defer (SSH can be v3+; AI can be feature-flagged in via Anthropic SDK without going through Warp's existing AI integration in `app::ai`).
4. **Set a hard merge-conflict budget**: if upstream cherry-picks against `app/` consistently take >2 hours, declare divergence and switch to "feature-pull-only" (we cherry-pick named features, not bulk merges). Document this trade-off.
5. **Pre-write the cfg dialect**: agree that we use ONE form: `#[cfg(target_os = "android")]` for additions, `#[cfg(not(target_os = "android"))]` for removals. No mixing with `platform_family` or other gates. Makes mass `sed` operations possible if we need to shift the gate. The empty M0 facade crate must commit this convention in its `lib.rs` doc-comment so the dialect is locked from day 1.

---

## Section 3: Expanded Test Plan (deliberate-mode required)

### 3.1 Unit Tests (Rust crate level)

| Crate | Coverage Target | Key Invariants |
|---|---|---|
| `warpui::platform::android` (new) | 70% of public functions | `Window` lifecycle ops are idempotent under `onSurfaceDestroyed/Created` interleaving; `FontDB` returns same handle for same locale; `DispatchDelegate` posts to main thread without panicking under recursive dispatch. |
| `warp_terminal_mobile_facade` (new, scaffolded in M0) | 80% | Facade calls do not silently no-op on Android (every gated function logs its skip path); `AppContext` mock returns deterministic data for tests; `SSH` no-op provider returns error not panic. |
| `app::terminal::local_tty::{shell, event_loop, mio_channel}` (cfg-gated for Android in M1) | 90% on PTY ops | `openpty()`, `setsid()`, `TIOCSCTTY`, `dup2(stdio)`, `TIOCSWINSZ` all return success on Bionic; `SIGCHLD` handler reaps without zombies; resize is monotonic; existing reactor wakeup semantics preserved. |
| Bootstrap-zip extractor (new, in JNI shim) | 85% | Atomic-rename install (extract to `usr.tmp`, atomic rename to `usr`); checksum verification before activation; rollback on partial extract; no-op if version matches. |
| Block detector (DCS parser, modified) | 100% on hot path | `ESC P $ d ... 0x9c` detection is byte-stream-safe (handles split reads); JSON metadata parse failure gracefully degrades to "no block"; never deadlocks on malformed input. |

**Testing tooling**: `cargo test --target x86_64-linux-android` running on host x86 Android emulator avoids the slow ARM device cycle for pure-logic tests. `nix::pty::openpty` is verified on Bionic via a single device test, then unit tests use a fake-PTY trait.

### 3.2 Integration Tests (PTY+UI loopback, no full device)

| Test Suite | What it does | Why |
|---|---|---|
| **PTY-Renderer loopback** | Spawn a fake PTY (in-memory pipe pair), feed `ls -la` style ANSI output, assert renderer's grid state after each chunk. Tests scrollback truncation, line-wrap, BCE (background color erase) handling. | Catches the 80% of bugs that aren't device-specific. |
| **DCS block hook** | Inject a synthetic `ESC P $ d {"command":"foo","start_time":...} 0x9c` sequence into the PTY stream, assert that the block manager creates a `Block` with correct metadata and bounds. | Validates Warp's block UX on Android without needing real zsh. |
| **Activity-recreate session continuity** | Boot the service, spawn a long-running command (`sleep 60 && echo done`), simulate Activity destroy+recreate at t=10s, assert that on rebind the existing session is reattached and the eventual `done` output is captured. | Death-pit risk #3. |
| **IME composition harness** | A JUnit test driving an in-process `InputConnection` mock against a fake renderer, scripting Pinyin composition events, asserting glyph count + composing-region updates. | Mostly catches regressions; not perfect coverage but blocks CJK breakage. |
| **Bootstrap update lifecycle** | Test extract → activate → upgrade → rollback. Use a synthetic 1MB zip. Assert no partial state on simulated kill mid-extract. | Code path for app updates that change `$PREFIX` contents. |

**Tooling**: Robolectric for JVM-side, `cargo test` with feature-gated mocks for Rust side. CI runs on Linux runners, no Android emulator needed.

### 3.3 End-to-End Tests (3-device matrix)

| Device class | Hardware exemplar | Android version | Coverage Goal |
|---|---|---|---|
| **Low-end** | Pixel 4a (4GB RAM, Snapdragon 730G) | 14 | App launches without OOM; 5-line cmd executes; no AI; 30fps minimum at 80x24 grid. |
| **Mid-tier** | Pixel 7a (8GB RAM, Tensor G2) | 15 | Full happy path: zsh launches, runs `git status`, blocks render with metadata, AI ghost-text from Anthropic API in <500ms p50. |
| **Flagship** | Pixel 9 Pro (16GB RAM, Tensor G4) | 16 (Beta) | Same as mid-tier + local LLM (v2+) inference at <3s/100tok; no PhantomProcessKiller deaths under sustained 5-minute usage. |

**E2E happy path script**:
1. Cold launch → app shows shell prompt within 2s.
2. Type `echo hello` → output appears within 100ms after Enter.
3. Type partial command → ghost-text appears (cloud AI mode) within 500ms.
4. Run `for i in {1..10}; do echo $i; sleep 1; done` → all 10 lines render; rotate device mid-stream → no lost output.
5. Background app → wait 2 minutes → foreground → session intact, history preserved.
6. Open second tab → independent PTY spawned → both run concurrently.
7. Trigger an "agent" task ("explain `du -sh *`") → Sonnet response renders in <8s.

**Explicit fail criteria (per step)**:
- **Step 1 fail** = prompt not visible within 2.0s of `am start` issued (measured via UIAutomator until-found timeout).
- **Step 2 fail** = `echo hello` output text not present in renderer state within 100ms after the Enter `keyevent` injection.
- **Step 3 fail** = no ghost-text element appearing within 500ms p50 across 10 trials, OR any single trial >1500ms.
- **Step 4 fail** = byte-diff between captured PTY output stream and reference stream baseline (recorded once per device class at known-good commit) is ≠ 0 — even one missing line/byte is a fail. Specifically, capture `master` PTY bytes to `step4-actual.bin`, compare via `cmp` to `step4-reference.bin` per device; any non-zero diff fails the gate.
- **Step 5 fail** = on resume, scrollback line count differs from pre-background snapshot, OR running shell PID differs (session was killed and restarted).
- **Step 6 fail** = either tab's PTY output interleaves bytes from the other tab (shared FD bug), OR tab-2 spawn time >3s after the second-tab gesture.
- **Step 7 fail** = no agent block visible within 8s p50 across 5 trials, OR Sonnet response has zero tokens (API error not surfaced to user).

**Runner**: Android Espresso + UIAutomator. Recordings stored as artifacts. No flaky-test tolerance: all 7 steps must pass on all 3 devices for release gating; any single fail-criterion above blocks release.

### 3.4 Observability

| Metric | Source | Target | Action on Breach |
|---|---|---|---|
| **FGS survival** | Foreground Service ANR / kill events from `ApplicationExitInfo` | <0.5% sessions killed | Ship hotfix increasing notification importance; investigate vendor-specific Doze. |
| **PTY resize consistency** | Custom logger comparing `TIOCSWINSZ` SET vs `tcgetwinsize` GET after each resize | 100% match | Bug — ship fix before next release. |
| **Activity-recreate recovery** | Custom event "session_reattached_ms" measuring time from `onResume` to live PTY data flow | p95 < 500ms | Optimize service binding; investigate kill-restart vs warm rebind. |
| **OOM-killer triggers** | `ApplicationExitInfo.REASON_LOW_MEMORY` count | <1% sessions | Reduce baseline RAM; reconsider local LLM availability rules. |
| **Crash reporting** | Sentry (open-source self-hosted preferred for AGPL alignment, or Crashlytics) | Crash-free sessions >99.5% | Standard triage. |
| **PhantomProcessKiller events** | Logcat scan for `phantom` strings, count per session | Zero on flagship/mid-tier; <5% on low-end | If non-zero on mid-tier, redesign IPC to consolidate child processes. |
| **AI latency (cloud)** | Application timer wrapping Anthropic SDK calls | p50 < 500ms ghost-text, < 8s agent | Optimize prompt size; switch model tier if Haiku is overloaded. |
| **Bootstrap install rate** | Counter of successful first-launch bootstrap extractions | >99% | Investigate device-specific `EACCES`. |

**Distribution**: All metrics aggregated to a privacy-preserving counter sink (likely Plausible-style minimal telemetry for AGPL compatibility). User can opt out; defaults to opt-in only on beta channel.

---

## Section 4: ADR (Architecture Decision Record)

### Decision (1-line)

**Port Warp to Android via 5-layer fork (Warp + Termux) + self-implemented `warpui::platform::android` backend + bundled runtime + open-source-first distribution (GitHub Releases / F-Droid primary, Play Store deferred to v3+).**

### Drivers (top 3)

1. **AGPL-3.0 license + just-released open source posture**: We can legally fork; we must publish source; F-Droid is the most license-aligned distribution channel; Play Store with AGPL has unsettled history. Drives "open-source first" principle.
2. **Solo-dev sustainability**: The project must be tractable for one engineer for 12-18 months. Drives "fork-and-narrow" over "rewrite", "cloud AI before local", and "verify the riskiest layer first" via M0 spike.
3. **Android process lifecycle hostility**: PhantomProcessKiller, FGS limits, scoped storage, W^X enforcement, IME composition complexity. Drives the death-pit ranking (L1 renderer first, L3 runtime defused via F-Droid path).

### Alternatives considered (≥3)

| Alternative | Why rejected |
|---|---|
| **Termux fork + AI plugin** (start from Termux, add Warp-style blocks/agent on top) | Termux is Android-native and solid for runtime, but its UI is plain `XTerm`. Building Warp's blocks/agent UX without Warp's `warpui` rendering stack means rebuilding 80% of Warp's value prop. Estimated 18-30 months to match v1 feature-parity, longer than porting Warp itself. Loses access to upstream Warp evolution. |
| **From-scratch Compose terminal + custom AI** | Cleanest Android integration but throws away every Warp innovation. Would be "yet another mobile terminal with AI", not "Warp on Android". Doesn't satisfy the project's stated goal. Also risks feature drift from upstream Warp. |
| **Wave Terminal port** (Wave is open-source TS/React + Go) | Wave's renderer is web-based (xterm.js), so Android port = WebView wrapper. WebView terminal performance is mediocre; IME in WebView is notoriously bad on Android; Go runtime + WebView + Android service stack is a different complexity tax. Doesn't deliver Warp's UX. |
| **Cloud-rendered Warp** (run Warp on a remote server, mobile is just a viewer) | Latency, requires server infra (defeats solo-dev), AGPL would obligate publishing the server too. Loses offline. |
| **Warp via Linux terminal emulator on Android (chroot/proot)** | proot is slow, hostile to graphics, and `ldd` mismatches between chroot-glibc and host-Bionic create endless surprises. Has been done for Linux desktops on Android (Userland, Andronix) — UX is bad, not Warp. |
| **Warp Companion** (phone pairs to desktop Warp via SSH/Drive; blocks rendered as native Compose UI proxying input/output to/from desktop) | **Pros**: 3-4 month scope (vs 12-18); no GPU compositor / Vulkan / W^X / IME rabbit holes; no fork-of-Warp maintenance; AGPL §13 cleanly avoided because we ship a thin client to upstream Warp, not a derivative; no bootstrap zip / Termux fork. **Cons**: requires desktop Warp always-on; offline use compromised (cannot run shell when desktop unreachable); phone-side UX is from-scratch (cannot reuse Warp UX inside Compose proxy); Warp Drive sync becomes a hard dependency. **Why rejected**: Principle 1 (open-source-first) implicitly assumes standalone offline use — the companion path makes the phone a dumb terminal that mandates a desktop running Warp. This contradicts "open the app and it works" UX bar and forecloses the F-Droid AGPL story (the value prop on F-Droid is a self-contained app, not a remote control). Kept here as a documented fallback — if M0 spikes fail catastrophically, "demote to companion mode for v1" is a known retreat path. |

### Why chosen

- **Aligns license**: AGPL fork is permitted; F-Droid is AGPL-friendly; we publish source, period.
- **Aligns scope**: 5-layer architecture is decomposable; each layer can be isolated and tested.
- **Aligns risk**: Death-pit ranking puts the most-likely-to-kill-the-project work (L1 renderer) first; L3 (Termux) is defused by the F-Droid path.
- **Aligns motivation**: Real Warp on Android is a flagship use of Warp's just-released open source posture; even a partial v1 is interesting and shippable.
- **Aligns dependency direction**: We control our forks; upstream changes flow in via cherry-pick on our schedule, not theirs.

### Consequences

**Positive**
- We own the entire stack and can ship without external blockers.
- F-Droid distribution with AGPL is cleanest open-source story possible.
- Bootstrap-zip + fork of `termux-packages` gives us a real package ecosystem at v1, not just "a fancy SSH client".
- Cloud AI lets us ship MVP without wrestling NDK + llama.cpp + RAM gating.
- Self-implementing `warpui::platform::android` keeps us off third-party hot paths (no `gpui-mobile` debt).

**Negative**
- **Fork maintenance debt forever**. Upstream Warp evolves; we cherry-pick. Conflicts grow over time.
- **Solo-dev burnout risk**. 12-18 months minimum, more likely 18-30 months for a polished v1.
- **AGPL constrains** future commercial paths (no proprietary plugins without dual-license negotiation upstream).
- **Cloud AI requires user BYOK** — friction for non-developer users (probably acceptable given target audience).
- **Android W^X future may invalidate F-Droid path** — see pre-mortem Scenario B.
- **F-Droid will list under NonFreeNet anti-feature label** because v1 ships with optional dependency on Anthropic API. Acceptable for pragmatic shipping but reduces "open-source purity" perception.

### Follow-ups (deferred)

- **Play Store distribution (v3+)**: After establishing F-Droid presence and resolving any W^X-related concerns; will require additional engineering to produce a Play-compatible variant (no writable executable code, possibly via static-bundled-busybox path).
- **Local LLM support (v2+)**: Qwen2.5-Coder-1.5B Q4_K_M with mmap, RAM gating to ≥6GB devices, opt-in only.
- **iOS port**: AGPL is incompatible with App Store distribution (Apple's terms conflict with AGPL §7 on imposed restrictions). Defer indefinitely or only if Warp's license shifts.
- **Warp Drive / Warp Cloud sync**: Proprietary upstream service. Defer to "best effort, optional integration if Warp's API contract stabilizes".
- **MCP server hosting on device**: After M6 ships, integrate MCP host capabilities. Not on critical path.
- **SSH client**: M5+ feature; cfg-gated out at M0-M4. SSH on Android has scope: key storage (Keystore vs file), passphrase UX.
- **Multi-user / profiles**: v3+ feature.
- **AGPL §7 (no further restrictions) vs Anthropic BYOK ToS lawyer review pre-v1 ship**: Anthropic's ToS imposes export-control + age + acceptable-use restrictions on API consumers; AGPL §7 forbids the licensee from imposing further restrictions on downstream users. We need a lawyer to confirm whether shipping a BYOK config (where the *user's own* API key invokes Anthropic ToS, not ours) creates an AGPL §7 conflict. Block v1 release on this opinion.

### Tension 3 user decision required at M0 close

**Tension 3** (open-source-first vs cloud AI dependency) cannot be resolved by Planner/Architect/Critic alone — it is a product-strategy gate. At M0 close (week 4-5) the user must answer:

- [ ] **Question A**: Does v1 ship with cloud AI (Anthropic BYOK) as a *core* feature, or as an *opt-in* feature that defaults off?
- [ ] **Question B**: If v1 ships with cloud AI core, do we accept F-Droid's NonFreeNet anti-feature label and the AGPL §7 lawyer review path?
- [ ] **Question C**: If v1 ships AI as opt-in only (or postpones AI to v2), does the value-prop story hold? (i.e., is "Warp blocks UX on Android without AI" still worth 12-18 months of solo work?)
- [ ] **Question D**: If lawyer review (Follow-ups #4 above) concludes BYOK creates an AGPL §7 conflict, what is the fallback? Defer AI to v2 entirely / dual-license negotiation upstream / pivot to local-LLM-only?
- [ ] **Question E**: At what point does the "Warp Companion" alternative (rejected above) get reconsidered? Define the trigger condition (e.g., "if M0 Vulkan spike fails on 2 of 3 reference devices").

**Output of gate**: a 1-page `M0-tension3-decision.md` committed to repo before M1 begins.

---

## Section 5: M0–M6 Acceptance Criteria

### M0 — F-Droid + L1 renderer-risk feasibility spike (4-5 person-weeks)

**Goal**: Validate that the open-source distribution path is technically sound AND that the L1 renderer's #1 risk (Vulkan-Surface-recreate lifecycle) is empirically de-risked before committing to the architecture. Per Principle 5, M0 must verify the riskiest layer first — that means M0 owns the renderer spike, not M2.

1. On a test fixture Android project with `targetSdkVersion 28` and the symlink-to-jniLibs trick, successfully `execve()` a binary written to writable app home on Android 14, 15, and the latest 16 Beta — verified by adb log of process start and exit code.
2. `cargo-ndk` with NDK r25c+ produces a stripped `aarch64-linux-android` `.so` (>=1MB toy Rust crate using `nix` for `openpty`) with no missing-symbol errors against API level 26.
3. Compile-test of `warp_terminal` AND of `app` separately (`cargo check --target aarch64-linux-android -p warp_terminal` and `-p app`, no link) produces a quantified count of unbuildable deps + cfg-gating estimate **per crate** so we can see the clean-vs-tangled split; report committed to repo as `M0-deps-report.md` with one section per crate.
4. F-Droid metadata (`metadata/<id>.yml` + reproducible build recipe) produces a build that matches a local `gradle assembleRelease` byte-for-byte (or with documented variation rationale).
5. **(NEW) Vulkan-Surface-recreate spike, 2-day timebox per device**: a 50-line Rust+JNI standalone app holds a `VkSurfaceKHR` across `onPause`/`onResume` and rotation on **3 reference devices** — Pixel 7a (mid-tier), Galaxy A14 (low-end Mali), Pixel 9 Pro (flagship). Verification: `M0-vulkan-spike-report.md` committed with per-device frame-recovery-time-ms numbers (`onPause`→`onResume` swapchain re-acquire + first valid frame), and a pass/fail/conditional verdict.
6. **(NEW) `warpui::platform` trait diff, 2-day timebox**: enumerate the trait surface that an Android backend must satisfy by reading `crates/warpui/src/platform/mod.rs` (Delegate / DispatchDelegate / FontDB / TextLayoutSystem / Window / WindowContext / WindowManager). Diff against `gpui-mobile`'s exports at a pinned commit and against Warp's existing `linux` / `headless` / `wasm` backends to lock the derive base. Verification: `M0-platform-trait-delta.md` committed with hash references to the upstream commits compared.
7. **(NEW) Empty `crates/warp_terminal_mobile_facade` scaffold, 1-week timebox**: scaffold an empty crate with `Cargo.toml`, `src/lib.rs`, doc-comment locking the cfg dialect (`#[cfg(target_os = "android")]` for additions, `#[cfg(not(target_os = "android"))]` for removals — no `platform_family` mixing per Pre-mortem C #5), and a placeholder `Session::spawn`/`Session::write`/`Session::read` API stub. Verification: scaffold commit hash recorded in `M0-go-no-go.md`.
8. **(NEW) Tension 3 user decision gate**: the user answers Questions A-E (see ADR "Tension 3" subsection) and commits `M0-tension3-decision.md`. Without this, M1 does not start.
9. Documented decision: "M1 starts" or "M0 expanded by N weeks" or "project pivots/cancels". No bleed into M1 without explicit gate.

### M1 — Android PTY/service prototype, no UI (6-8 weeks)

**Goal**: Prove the systems plumbing — PTY + lifecycle — independent of UI.

1. Pure-Rust+JNI service spawns `bash -c 'echo hello'` via `openpty()` + `setsid()` + `TIOCSCTTY` + `dup2(stdio)` on Pixel 7 emulator, captures `hello\n` from PTY master, asserts process exits cleanly with `SIGCHLD` reaped (no zombie).
2. Activity destroy + recreate (rotation, minimize-2-min-restore) preserves a running `sleep 60 && echo done` session — re-attached PTY emits `done` to the new Activity binding within 1s of `done` actually firing.
3. PTY resize via `TIOCSWINSZ` reflects in shell's `stty size` output (verified by sending `stty size > /tmp/x; cat /tmp/x` → expected dimensions).
4. FGS notification persistent during session; on `adb shell am kill <package>` the service self-terminates cleanly (no orphan PTY processes).
5. Stress test: 30-minute idle session on flagship + low-end Pixel 4a, no crashes, no PhantomProcessKiller events on flagship; documented behavior on low-end.

### M2 — `warpui::platform::android` backend (8-12 weeks; **Amendment 1+2**: split into M2a + M2b under D1.5-hybrid)

**Goal**: Get pixels to screen, accept input, survive the Android lifecycle. (Builds on M0's de-risked Vulkan lifecycle spike; M2's job is to scale from the 50-line spike to a production-grade backend.)

> **Amendment 1+2 split (D1.5-hybrid)**:
> - **M2a (4 weeks)** — Implement Layer 1's 4 hand-written areas inside `crates/warpui/src/platform/android/` (per Amendment 2): `render_scene` (`ash` Vulkan + `ANativeWindow`), `request_frame_capture` (ash readback), `FontDB` (15 methods, `cosmic-text` wrapper + Android system fonts), `TextLayoutSystem` (2 methods). Derive other 85 trait methods from `warpui::platform::headless`. `warp_terminal → warpui` Cargo edge stays.
> - **M2b (4-6 weeks)** — IME (`InputConnection`), touch + gesture, rotation + Surface lifecycle scaling, `WindowInsets`, validation-layer cleanup, performance tuning across device matrix.
> - Combined: still 8-12 weeks but with cleaner internal milestone gate at M2a→M2b transition.

1. Static glyph atlas (50x20 grid of "Hello, World") renders at 60fps on **Galaxy S24 Ultra** (flagship), 60fps on **Galaxy S21+** (mid-tier Android 15), with **Galaxy S8** (Android 9 SDK 28 retreat baseline) achieving at least 30fps or documented graceful degradation; no Vulkan validation warnings.
2. `Activity.recreate()` (rotation + low-memory simulated kill) recovers swapchain within 200ms (instrumented via `Choreographer.postFrameCallback` timestamp diff: t0 = `surfaceDestroyed` callback; t1 = first frame where `pixels != stale_pixels`; assert `(t1 - t0) < 200ms` p95 across 100 rotations); no black frame >300ms.
3. Soft IME (Gboard English + Pinyin) inputs one character per keystroke on a stub editable region; composing-text region (Chinese) updates in-place without flicker.
4. `WindowInsets` correctly reserves bottom region for IME; full-screen mode hides nav bar; rotation re-lays out within one frame budget.
5. `cargo doc` for `warpui::platform::android` exists with module-level doc explaining the Linux-derived (or headless-derived per M0 archeology) design and points of divergence; >=1 unit test per public function.

### M3 — Warp minimal terminal/session integration (8-12 weeks)

**Goal**: The thing actually looks and feels like Warp on a phone.

1. PTY stream from M1 service feeds the M2 renderer via the cfg-gated `app::terminal::*` and `crates/warp_terminal` minimal subset; running `ls -la /system` produces correctly colored, line-wrapped output (verified vs Linux baseline).
2. Scrollback ≥1000 lines; touch-drag scrolls smoothly (60fps on Galaxy S24 Ultra + S21+, 30fps on Galaxy S8 or graceful degrade documented); two-finger flick momentum scroll behaves natively.
3. Block detection from DCS hook (`ESC P $ d ... 0x9c`) creates `Block` objects on the model side, with `start_time`, `command`, and `exit_code` correctly populated (verified by inspecting model state after running 3 sample commands in zsh with the bootstrap hook).
4. App size (release APK **excluding** the bootstrap zip, which is shipped as a separate F-Droid auxiliary asset per M4) ≤80MB; combined APK + bootstrap zip ≤120MB total.
5. Code freeze-and-merge dry-run: cherry-pick the latest 10 upstream Warp commits onto our `mobile/main` branch — record total time; if >2hr, flag scope concern.

### M4 — Termux bootstrap + package story (10-16 weeks)

**Goal**: A real $PREFIX with installable packages.

1. APK ships a bootstrap zip (~30-50MB compressed) for `aarch64`; on first launch extracts to `/data/data/<id>/files/usr` atomically (no partial state on kill mid-extract); subsequent launches are instant.
2. `pkg install git python` from within the app installs the forked `termux-packages` builds with our prefix; `git --version` and `python3 --version` execute correctly post-install.
3. Bootstrap zip is reproducible: rebuilding the fork at the same commit produces a byte-identical zip (within deterministic-tooling allowances).
4. Upgrade path: app v1.0 → v1.1 with bootstrap-zip-content changes → installed packages migrate via reinstall manifest, no user data loss.
5. F-Droid metadata + recipe handles the bootstrap zip as part of reproducible build (or as a separate-source asset with hash-pin).

### M5 — Mobile UX layer (12-16 weeks)

**Goal**: It's not a desktop terminal squeezed onto a phone.

1. Selection: long-press starts a touch-drag selection; copy via accessory menu; selection preserved across scroll.
2. Accessory row (Esc, Tab, Ctrl, Alt, arrow keys, common symbols) above IME, customizable (last 20 commands' symbols pinned dynamically).
3. Block gestures: tap-block to focus, long-press for menu (copy, re-run, share), swipe-right to bookmark.
4. IME edge cases tested: switching keyboard mid-composition (Pinyin → Cangjie); voice input → text; clipboard paste of multi-line content correctly streamed to PTY without dropping characters.
5. UX review with **≥5 external testers** (TestFlight equivalent on F-Droid via direct APK install). Recruitment criteria: **≥3 of the 5 must be currently-active mobile developers** (defined as: shipped at least one app to a public store in the past 12 months, OR have ≥6 months of professional Android/iOS development experience). Aggregate "would daily-drive" sentiment **≥3/5** measured via Likert questions pinned verbatim:
   - Q1: "I would use this app on my phone for real terminal work at least once a week." (1=strongly disagree, 5=strongly agree)
   - Q2: "Block detection / command history makes mobile shell easier than a plain terminal app." (1-5)
   - Q3: "IME handling (especially CJK if applicable) feels native, not janky." (1-5)
   - Q4: "Rotation, app switching, and FGS notifications worked without losing my session." (1-5)
   - Q5: "Overall, I would recommend this to a developer friend on Android." (1-5)
   - Aggregate score = mean of Q1+Q5 across all testers. ≥3.0 passes. Raw responses + redacted free-text feedback committed to `M5-ux-review.md`.

### M6 — AI integration (cloud-first; open-ended) (~8-10 weeks within overall budget)

**Goal**: Ghost-text + agent both work, on real Anthropic API.

1. Inline ghost-text via Claude Haiku: typing partial command produces grayed suggestion within 500ms p50, accept-with-Tab inserts.
2. Agent task: select a command + "explain" → Claude Sonnet returns explanation in <8s p50; rendered in a side-panel block.
3. BYOK UX: settings screen accepts Anthropic API key; key stored in Android Keystore; "test connection" button validates with a 1-token completion.
4. Costs: agent task ≤2000 tokens p95; ghost-text ≤200 tokens p95; documented in user-facing settings.
5. Fallback: on network unavailable, AI features visibly disabled with non-blocking notice; rest of app remains functional.

---

## Section 6: Implementation Plan Detail (per milestone)

### M0 — F-Droid + L1 Renderer-Risk Feasibility Spike

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Set up `cargo-ndk` + NDK r25c+ + `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android` targets in CI | `.github/workflows/build.yml`, `rust-toolchain.toml` | `cargo build --target aarch64-linux-android` completes in CI |
| 2 | Stand up minimal AndroidManifest with FGS + `targetSdkVersion 28`; write JNI shim spawning busybox | `android/app/src/main/AndroidManifest.xml`, `android/app/src/main/cpp/jni_shim.c` | `adb shell am start -n <id>` boots; logcat shows shim init |
| 3 | Verify symlink-to-jniLibs trick works on Android 14/15/16-Beta to mark `$PREFIX/bin/*` files executable from writable home | `android/app/src/main/jniLibs/<abi>/`, install script | `adb shell run-as <id> ./files/usr/bin/sh -c 'echo ok'` returns `ok` on all 3 OS versions |
| 4 | `cargo check --target aarch64-linux-android -p warp_terminal` AND `-p app` separately; quantify failures per crate (clean vs tangled split) | `M0-deps-report.md` (committed, per-crate sections) | Report exists with breakdown: per-crate total crates, unbuildable count, cfg-gate estimate; explicit "warp_terminal-clean / app-tangled" attribution |
| 5 | Write F-Droid metadata + reproducible-build recipe; produce a hash-locked APK | `metadata/io.warp.mobile.yml`, `fastlane/metadata/android/` | F-Droid build server parity test |
| 6 | **(NEW)** Vulkan-Surface-recreate spike: 50-line Rust+JNI standalone holding `VkSurfaceKHR` across `onPause`/`onResume`+rotation; run on Pixel 7a + Galaxy A14 + Pixel 9 Pro | `spikes/vulkan-surface-recreate/` (new dir; standalone Cargo project + minimal AndroidManifest) | `M0-vulkan-spike-report.md` committed: per-device frame-recovery-time-ms (`onPause`→first valid frame), pass/fail/conditional verdict, validation-layer log excerpts. 2 days/device timebox. |
| 7 | **(NEW)** `warpui::platform` trait diff: enumerate Delegate / DispatchDelegate / FontDB / TextLayoutSystem / Window / WindowContext / WindowManager surface from `crates/warpui/src/platform/mod.rs`; diff vs gpui-mobile pinned commit; diff vs `linux` / `headless` / `wasm` backends to lock derive base | `M0-platform-trait-delta.md` (committed) | Report includes: upstream pinned commit hashes, trait-by-trait surface diff table, recommendation on derive base (A1 linux vs A4 headless vs hybrid). 2-day timebox. |
| 8 | **(NEW)** Scaffold empty `crates/warp_terminal_mobile_facade` crate with cfg-dialect doc-comment per Pre-mortem C #5 | `crates/warp_terminal_mobile_facade/Cargo.toml`, `crates/warp_terminal_mobile_facade/src/lib.rs` | `cargo build -p warp_terminal_mobile_facade` succeeds host-side; `cargo build --target aarch64-linux-android -p warp_terminal_mobile_facade` succeeds; scaffold commit hash recorded in `M0-go-no-go.md`. 1-week timebox. |
| 9 | **(NEW)** Tension 3 user-decision gate (Questions A-E from ADR Tension 3 subsection) | `M0-tension3-decision.md` (committed) | User signs off all 5 questions; without this, M1 does not start. |

**Verification step (overall)**: `M0-go-no-go.md` decision document committed referencing all artifacts above; explicit gate to M1.
**Effort (solo)**: **4-5 person-weeks** (was 2-3 in iteration 1; expanded to absorb Vulkan spike + trait diff + facade scaffold + Tension 3 gate per Architect Principle 5 enforcement).
**Dependencies**: None (project start). Requires access to 3 reference Android devices (Pixel 7a, Galaxy A14, Pixel 9 Pro).

### M1 — Android PTY/Service Prototype

**Integration strategy note** (revised per Critic IMPORTANT #5): Rather than fabricating a fresh `crates/mobile_pty` crate divorced from the existing reactor, M1 derives from / cfg-gates the upstream `app/src/terminal/local_tty/{shell.rs, event_loop.rs, mio_channel.rs}` modules. The Android backend lives behind `#[cfg(target_os = "android")]` switches inside those existing files (where possible) plus a small new `local_tty/android.rs` sibling for Bionic-specific glue. This keeps cherry-pick velocity high (Pre-mortem C mitigation #1) and avoids creating a parallel PTY universe that drifts from upstream.

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Cfg-gate Bionic-compatible PTY ops in existing `app::terminal::local_tty::shell` (`openpty + fork + setsid + TIOCSCTTY + dup2 + execve`); add `#[cfg(target_os = "android")]` siblings only where the Linux/macOS path won't compile | `app/src/terminal/local_tty/shell.rs` (cfg-gated), `app/src/terminal/local_tty/android.rs` (new, only if necessary) | Unit test `spawn_echo_hello` passes on emulator targeting `aarch64-linux-android` |
| 2 | Reactor integration: extend the existing `event_loop.rs` mio reactor to handle Android-specific signal delivery (SIGCHLD reaping under FGS); document integration with `mio_channel.rs` cross-thread wakeup | `app/src/terminal/local_tty/event_loop.rs` (cfg-extended), `app/src/terminal/local_tty/mio_channel.rs` (audit only) | `M1-reactor-integration.md` documents the Android reactor flow; CI runs reactor stress test |
| 3 | JNI bridge between Android FGS and the cfg-gated `local_tty` reactor; `Service.onStartCommand` → spawn via reactor; `onDestroy` → SIGTERM children | `android/app/src/main/cpp/pty_jni.c`, `android/app/src/main/java/io/warp/mobile/PtyService.kt` | `am start-foreground-service` boots; `adb shell ps` shows child shells |
| 4 | Lifecycle handlers: `onPause/onResume/onConfigurationChanged` rebind without restart; persist session via `WorkManager` if needed | `android/app/src/main/java/io/warp/mobile/MainActivity.kt` | Rotation test passes (acceptance criterion #2) |
| 5 | PTY resize: `TIOCSWINSZ` from grid dims; SIGWINCH delivered to child | `app/src/terminal/local_tty/resize.rs` (new sibling, cfg-shared) | `stty size` round-trip test |
| 6 | Logcat-based assertions for SIGCHLD reaping (no zombies after 10 spawn+exit cycles) | `app/src/terminal/local_tty/tests/zombie_test.rs` | CI gate |

**Verification step**: `adb logcat | grep PtyService` shows clean spawn/exit sequence; `ps -A | grep <id>` shows no zombies after stress test. `cargo build --target aarch64-linux-android -p app --features android-pty-only` builds the cfg-gated reactor without pulling in `app::ai`/`app::ssh`/`app::feature_flag` (those stay in M3 facade scope).
**Effort (solo)**: 6-8 person-weeks.
**Dependencies**: M0 (NDK + FGS proven; facade-crate scaffold + cfg-dialect committed).

### M2 — `warpui::platform::android` Backend

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Fork `crates/warpui/src/platform/{linux or headless per M0 archeology}/` → `crates/warpui/src/platform/android/`; cfg-gate dispatch in `crates/warpui/src/platform/mod.rs` | `crates/warpui/src/platform/mod.rs`, `crates/warpui/src/platform/android/mod.rs` | `cargo build --target aarch64-linux-android -p warpui` succeeds |
| 2 | Implement `Window`, `WindowContext`, `WindowManager`, `DispatchDelegate` deriving from M0-selected base behavior | `crates/warpui/src/platform/android/window.rs`, `crates/warpui/src/platform/android/dispatch.rs`, etc. | Headless render test renders 50x20 atlas |
| 3 | Vulkan + ANativeWindow integration: scale M0's 50-line spike to production swapchain lifecycle (recreate on `onSurfaceChanged`, multi-buffer, validation-layer-clean) | `crates/warpui/src/platform/android/vulkan.rs` | Rotation-in-loop test (100 rotations); no validation warnings; `Choreographer` instrumentation confirms acceptance #2 (<200ms p95 swapchain recovery) |
| 4 | IME via `InputConnection` JNI: composing text → `setMarkedText` equivalent in renderer | `android/app/src/main/java/io/warp/mobile/WarpInputView.kt`, `crates/warpui/src/platform/android/ime.rs` | Pinyin test (acceptance #3) |
| 5 | Touch input → `Window` event dispatch; gesture recognition (tap, drag, momentum-flick) | `crates/warpui/src/platform/android/input.rs` | Touch-scroll test passes |
| 6 | `FontDB` — load Noto Sans CJK + monospace fonts from app assets | `crates/warpui/src/platform/android/font.rs`, `android/app/src/main/assets/fonts/` | CJK glyph render test |

**Verification step**: Demo app renders interactive 50x20 grid + accepts CJK input, runs at 60fps on flagship + 30fps on low-end. Recorded video.
**Effort (solo)**: 8-12 person-weeks.
**Dependencies**: M0 (build pipeline + Vulkan-spike report + trait-diff report locking the derive base), M1 (lifecycle understanding + reactor integration pattern).

### M3 — Warp Product Logic Integration (Layer 2b)

**Layer recap** (per Pre-mortem C revision): M3 wires the **clean Layer 2a** (`crates/warp_terminal` + `crates/warpui` + `crates/warp_core` + `crates/warp_completer` + `crates/warp_util`) into the **tangled Layer 2b** (`app/src/terminal/...` + `app::ai` + `app::feature_flag` + `app::ssh` + `app::app_context`) via the M0-scaffolded `crates/warp_terminal_mobile_facade`. The facade absorbs the cfg-gates so `warp_terminal` itself stays untouched and `app/` gets only edge-of-dependency cuts.

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Flesh out `crates/warp_terminal_mobile_facade` (scaffolded in M0) — expose minimal API: `Session::spawn`, `Session::write`, `Session::read`, plus mobile shims for `AppContext`, `FeatureFlag`, and a no-op `SSH` provider | `crates/warp_terminal_mobile_facade/src/lib.rs`, `crates/warp_terminal_mobile_facade/src/{app_context,feature_flag,ssh_noop}.rs` | `cargo doc` shows API; unit tests stub all desktop deps; facade builds for `aarch64-linux-android` |
| 2 | cfg-gate `app::terminal::*` desktop-only paths at the `app` crate edges; primary file is `app/src/terminal/model/session.rs` (verified to exist in `warpdotdev/Warp@d0f045c`) and its dependency edges into `app::ai` / `app::feature_flag` / `app::ssh` / `app::app_context` | `app/src/terminal/model/session.rs`, `app/src/terminal/mod.rs`, `app/Cargo.toml` (mio/nix/ai cfg-gates), `app/build.rs` | `cargo build --target aarch64-linux-android -p app` succeeds with cfg-gate count < 500 lines (Pre-mortem C signal threshold) |
| 3 | Wire facade output → M2's renderer via `warpui::platform::android::Window::push_frame` | `crates/warp_terminal_mobile_facade/src/render.rs` | Live `ls -la` test (acceptance #1) |
| 4 | Implement DCS hook parser; ship modified `zsh_body.sh` in bootstrap zip | `crates/warp_terminal/src/dcs.rs` (or wherever upstream parses DCS — verify in M0 archeology if file path differs), `app/assets/bundled/bootstrap/zsh_body.sh` | DCS detection test (acceptance #3) |
| 5 | Cherry-pick dry-run from `warpdotdev/Warp@HEAD` onto `mobile/main`; record conflict count separately for `warp_terminal/` (expected low) vs `app/` (expected high) | (ops task, no code) | `git cherry-pick` log; per-crate merge time recorded; trips Pre-mortem C #4 budget if `app/` >2hr |

**Verification step**: Run `git status`, `ls -la`, `du -sh *` end-to-end; verify blocks render with metadata; APK ≤80MB excluding bootstrap zip / ≤120MB including.
**Effort (solo)**: 8-12 person-weeks.
**Dependencies**: M2 renderer, M1 PTY service (cfg-gated reactor in `app::terminal::local_tty`), M0 facade scaffold + per-crate deps report.

### M4 — Termux Bootstrap + Package Ecosystem

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Fork `termux-packages`, retarget `$PREFIX` to `/data/data/io.warp.mobile/files/usr` (search-replace + audit) | `termux-packages/` (fork repo) | `bash scripts/setup-android-sdk.sh && ./build-package.sh bash` produces correct paths |
| 2 | Build core bootstrap zip: `bash`, `zsh`, `coreutils`, `findutils`, `apt`, `pkg`, `git` | `termux-packages/scripts/build-bootstraps.sh` | Hashed bootstrap-aarch64.zip artifact in CI |
| 3 | Atomic-extract installer in JNI shim: `usr.tmp/` → `usr/` rename, version-pin file | `android/app/src/main/cpp/bootstrap_install.c` | Kill-mid-extract test (acceptance #1) |
| 4 | `pkg install` UX: subprocess to forked `apt`; progress to UI via async channel | `crates/warp_terminal_mobile_facade/src/pkg.rs` | `pkg install python` happy path |
| 5 | Reproducible-build manifest for F-Droid: bootstrap zip listed as auxiliary artifact with SHA256 | `metadata/io.warp.mobile.yml` | F-Droid validate run |

**Verification step**: Fresh-install app → run `pkg install git && git clone ...` end-to-end without error.
**Effort (solo)**: 10-16 person-weeks.
**Dependencies**: M3 (PTY can host package processes); M0 (executable-from-home strategy proven).

### M5 — Mobile UX Layer

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Selection state machine + touch handlers; copy/paste integration with `ClipboardManager` | `crates/warpui/src/platform/android/selection.rs` | Long-press select + copy round-trip |
| 2 | Accessory row `KeyboardAccessoryView` above IME, with dynamic symbol-pinning | `android/app/src/main/java/io/warp/mobile/AccessoryRow.kt` | Visual + interaction test |
| 3 | Block gesture recognizer: tap, long-press, swipe-right; haptic feedback | `crates/warp_terminal_mobile_facade/src/gestures.rs` | Manual test on devices |
| 4 | Voice input + clipboard-paste streaming buffer (avoids dropping chars in long pastes) | `crates/warpui/src/platform/android/ime.rs` | 10K-char paste test |
| 5 | TestFlight-equivalent beta channel via F-Droid Beta repo or direct APK; usability survey | `metadata/io.warp.mobile-beta.yml` | ≥5 external testers reports |

**Verification step**: 5 testers daily-drive the app for ≥1 week; ≥3/5 average rating.
**Effort (solo)**: 12-16 person-weeks.
**Dependencies**: M2-M4 (full stack working).

### M6 — AI Integration (cloud-first)

| # | Task | File / Path | Verification |
|---|---|---|---|
| 1 | Anthropic SDK integration via `anthropic-sdk-rust` or HTTP+`reqwest`; BYOK flow | `crates/warp_ai_mobile/src/client.rs`, `android/app/src/main/java/io/warp/mobile/SettingsActivity.kt` | "Test API key" returns 1-token completion |
| 2 | Ghost-text via Haiku: streaming + debounce + cancel-on-keystroke | `crates/warp_ai_mobile/src/ghost.rs` | Latency p50 <500ms |
| 3 | Agent task UI: side-panel block with streaming output | `crates/warp_terminal_mobile_facade/src/agent_block.rs` | "explain `du -sh`" test |
| 4 | Offline graceful degrade: hide AI controls, show non-blocking banner | `crates/warp_ai_mobile/src/connectivity.rs` | Airplane-mode test |
| 5 | Token-usage display in settings + opt-in telemetry sink | `android/app/src/main/java/io/warp/mobile/UsageActivity.kt` | UI test |

**Verification step**: Ghost-text + agent both demonstrably working on real Anthropic API key during E2E.
**Effort (solo)**: 8-10 person-weeks.
**Dependencies**: M3-M5 (UI + blocks ready to host AI surfaces).

---

## Section 7: Open Questions / Spike Targets

Items tagged [unverified] that must be empirically validated before or during the corresponding milestone. Questions tracked in `.omc/plans/open-questions.md` upon plan acceptance.

1. **[M0] Does `nix::pty::openpty` work on Bionic API 26+ without modification?** Empirical: write 30-line Rust that calls `nix::pty::openpty`, build for `aarch64-linux-android`, run on Pixel emulator + real device. Expected: works (Bionic implements `posix_openpt` + `grantpt` + `unlockpt` + `ptsname_r`). Confirm.
2. **[M0] Does `termux-exec system_linker_exec` work for arbitrary `$PREFIX` on the latest Android Beta channel?** Empirical: Termux-fork toy build with our prefix, run on Pixel 9 with Android 16 Beta. Critical pre-mortem Scenario B trigger.
3. **[M2] Can `warpui::platform::android` derive cleanly from `linux` (A1) or `headless` (A4) or `wasm` backend, or does Vulkan-on-Android require fundamental architectural changes?** Empirical: M0 trait-diff archeology + 2-day Vulkan-Surface-recreate spike per device; if all derive paths fail, implications for M2 timeline.
4. **[M3] Does Warp's `crates/warp_terminal` actually compile to `aarch64-linux-android` once `warpui::platform::android` stub is in place?** Empirical: `cargo check -p warp_terminal --target aarch64-linux-android` in M0; binary reproducibility in M3.
5. **[Cross] What is Warp's default fallback rendering backend (does `warpui` have a CPU rasterizer / GLES path) if Vulkan-on-Android proves unviable?** Source-archeology task: grep `crates/warpui/src/platform/linux/render*` AND `crates/warpui/src/platform/headless/` AND `crates/warpui/src/platform/wasm/` for non-Vulkan backends. Also check whether `wasm/` has any `is_mobile_device` hint.
6. **[M2] How does `warpui::FontDB` consume `.ttf`/`.otf` files at runtime?** Source-archeology + spike: pull the chosen-derive-base font loader (linux/headless/wasm per M0 archeology), see if it's `freetype-rs` (works on Android NDK) or system-specific (e.g., FontConfig — won't work).
7. **[M3] How tangled is `app/src/terminal/model/session.rs`'s dependency topology on `app::ai` / `app::feature_flag` / `app::app_context` / `app::ssh`? Can we cfg-gate cleanly or do we need the facade-crate detour?** *(Note: previous draft referenced `terminal_model.rs` which does NOT exist — actual files in `app/src/terminal/model/` per `warpdotdev/Warp@d0f045c` are `session.rs`, `blockgrid.rs`, `blocks.rs`, etc.)* Empirical: M0 per-crate deps report (`-p warp_terminal` clean side, `-p app` tangled side).
8. **[M4] Are `pkg`/`apt` (Termux-flavored) reliable enough as on-device package managers for our use case, or should we ship a smaller "no apt, only static binaries" v1?** Test by surveying Termux user reports for crash/install-failure rates on Android 14+; if >5% on flagship, reconsider.
9. **[M5] Does `Activity.recreate()` work cleanly with `WindowInsets` for IME, or do we need our own IME insets handling?** Spike during M2.
10. **[M6] Is Anthropic API rate-limiting consumer-friendly enough for a BYOK app where users have variable-tier API keys?** Practical: review API docs for free-tier RPM limits; design backoff strategy.
11. **[Cross] AGPL-3.0 + Anthropic API SDK terms compatibility**: nothing problematic expected (Anthropic's SDK is MIT/Apache typically), but verify before shipping.
12. **[Cross] How will we handle the `warp_terminal` -> `warpui` dependency cleanly?** Currently `crates/warp_terminal` re-exports `warpui`; on Android we have a stubbed `warpui::platform::android` that may or may not satisfy all `warp_terminal`'s usages. Worth a M3 sub-spike.
13. **[M4] Does the F-Droid build server tolerate large bootstrap zips as auxiliary artifacts, or do we need to extract-on-first-run from a downloaded asset?** Read F-Droid policy doc; pilot.
14. **[M0 archeology] Read `crates/warpui/src/platform/headless/` and `crates/warpui/src/platform/wasm/` (especially any `is_mobile_device` predicate if it exists), then decide derive base for `warpui::platform::android`.** Outputs: `M0-platform-trait-delta.md` recommendation comparing A1 (linux), A4 (headless), and a wasm-hybrid; final A1-vs-A4 selection.
15. **[M0/Cross] Tension 3 user-decision gate (Questions A-E from ADR Tension 3 subsection)** — does v1 ship with cloud AI as core feature or opt-in only; F-Droid NonFreeNet acceptance; AGPL §7 lawyer review path; companion-mode retreat trigger. Output: `M0-tension3-decision.md`.
16. **[Cross/Legal] AGPL §7 (no further restrictions) vs Anthropic BYOK ToS lawyer review pre-v1 ship** — confirm whether shipping a BYOK config (where the *user's own* API key invokes Anthropic ToS, not ours) creates an AGPL §7 conflict. Block v1 release on this opinion.

---

## Document Status

This is the Planner's initial RALPLAN-DR draft. **Next steps in consensus loop**:
1. Architect to steelman this plan (challenge M2's renderer feasibility, Termux fork sustainability, AGPL stance).
2. CCG fan-out for tri-model review.
3. (User-set high-risk gate) GPT-5.5 Pro reviewer to find blind spots before Architect's steelman lands.
4. Critic verdict loop until consensus.
5. Final ADR signed and `start-work` handoff.

**Open questions tracked at**: `.omc/plans/open-questions.md` (to be populated on plan acceptance).
