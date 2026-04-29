# M0 Lead Summary (partial — agent-autonomous portion)

**Date**: 2026-04-29  
**Team**: warp-mobile-m0  
**Lead**: team-lead@warp-mobile-m0  
**Status**: 7/7 agent-autonomous tasks PASS; awaiting 3 user-required tasks (#2, #8, #9) and final go/no-go (#10)

---

## 1. Autonomous task results

| # | Task | Worker | Verdict | Artifact |
|---|---|---|---|---|
| 1 | NDK env smoke | worker-env | ✅ PASS | `M0-env-report.md` |
| 3 | cargo check warp_terminal aarch64-linux-android + deps report | worker-env | ⚠️ PASS-with-blockers | `M0-deps-report.md` |
| 4 | warpui::platform trait surface + gpui-mobile diff | worker-archeo | ✅ PASS | `M0-platform-trait-delta.md` |
| 5 | Vulkan-Surface-recreate spike code (compile-only) | worker-spike | ✅ PASS | `spikes/vulkan-surface-recreate/` + `.so` 716KB |
| 6 | warp_terminal_mobile_facade scaffold | worker-spike | ⚠️ PASS-structural | `M0-facade-scaffold.md` + commit `5400c66` on `warp-mobile/m0-facade` |
| 7 | Decision A1-vs-A4 archeology | worker-archeo | ✅ PASS | appended to `M0-platform-trait-delta.md` |

---

## 2. Three convergent findings — Plan amendment required

The three deepest workers (deps, archeology, scaffold) independently converged on the same conclusion. **This invalidates Decision D1 from the consensus plan and forces D2-lite adoption.**

### Finding A — `warp_terminal` itself is clean; `warpui` is the contamination

worker-env Task 3 measurement:
- `warp_terminal/Cargo.toml` direct deps = clean (Critic's assertion confirmed)
- 0 compiler errors emitted from any Warp-authored crate
- All Android build failures are **transitive through `warpui`**
- Two transitive failures:
  1. `font-kit` → `yeslogic-fontconfig-sys` (Android has no fontconfig). 8 files / **2,834 lines** would need cfg-gates and Android font stubs.
  2. `winit` → `android-activity` (missing `game-activity`/`native-activity` feature). 1 line Cargo.toml fix + **~500 lines** new Android windowing backend.
- **Total cfg-gate estimate: ~3,334 lines** — **6.7× the Pre-mortem C threshold of 500.**

### Finding B — `gpui-mobile` is **architecture reference only**, not a Cargo dependency

worker-archeo Task 4 quantification (warp `d0f045c` × gpui-mobile `1d3ec2a`):
- 89 total `warpui_core` trait methods
- identical = 0 (0%)
- portable = 31 (35%)
- incompatible = 13 (15%)
- missing in gpui-mobile = 45 (50%)
- gpui-mobile implements **Zed's `gpui::Platform`** — a different trait family. Not depable. Useful as reference for AndroidWindow / AndroidPlatform implementation patterns.

**A2 (gpui-mobile reuse) — formally rejected by evidence.** ralplan placeholder rejection now has empirical backing.

### Finding C — A4 (headless base) is overwhelmingly correct

worker-archeo Task 7 derive cost analysis:
- A1 (linux/winit base): 6–8 weeks. winit is deeply entangled with all the deps that just failed in Finding A.
- A4 (headless base): **3–4 weeks**. headless already implements 89/89 methods (mostly stubs); only 4 areas need major work:
  - `render_scene` (wgpu + ANativeWindow)
  - `request_frame_capture` (wgpu readback)
  - `FontDB` (15 methods — wrap cosmic-text)
  - `TextLayoutSystem` (2 methods)

**A4 cleanly avoids the font-kit and winit transitive failures from Finding A.** This is not a coincidence — headless was designed precisely to avoid those dependencies.

---

## 3. Required Plan amendment (D1 → D2-lite)

The original plan Section 1.3 Decision D currently chooses **D1 (cfg-gate everywhere)**. Pre-mortem C explicitly defined the abandonment threshold as 500 cfg-gate lines. **Measured value 3,334 lines crossed that threshold by a factor of 6.7 on M0 day 1.**

Plan amendment that must land in `.omc/plans/ralplan-warp-on-mobile.md` before M1 starts:

### Decision D — change to D2-lite (facade excludes warpui)

- **Old (D1)**: cfg-gate `warp_terminal` + `warpui` for Android target
- **New (D2-lite)**: `warp_terminal_mobile_facade` excludes `warpui` from its dep graph entirely. Re-implements the four areas (render_scene, frame capture, FontDB, TextLayoutSystem) using Android-native primitives (Vulkan via `ash` + cosmic-text + ANativeWindow). Re-uses `warp_terminal` (clean) for terminal/block/AI logic.

### Architecture revision — Layer 1 detail

- Layer 1 (WarpUI Android backend) is **NOT a port of warpui's full surface**. It is a **new minimal Android backend** that implements `warpui_core::platform::*` traits directly, copying headless's 85 stub methods, hand-writing the 4 areas above. Total ~3-4 person-weeks (matches Finding C).

### M2 task amendment

M2 was originally scoped as "warpui::platform::android backend, static grid render + IME + touch + rotation" (8-12 weeks). **D2-lite splits it cleaner**:
- M2a (4 weeks): implement the 4 hand-written areas in Layer 1 against Vulkan + cosmic-text + ANativeWindow
- M2b (4-6 weeks): IME + touch + rotation + Surface lifecycle (the Vulkan-spike-style work)
- M2 still 8-12 weeks total but now with cleaner internal milestones

### facade scaffold revision (M3 prep)

Current `crates/warp_terminal_mobile_facade` (commit `5400c66`) re-exports `warp_terminal::*`, which transitively pulls `warpui`. **Per D2-lite the facade must NOT depend on `warpui`.** M3 prep work:
- Remove `warp_terminal` direct dep from facade Cargo.toml
- Add direct deps on the clean subset: `warp_completer`, `warp_core`, `warp_util`, `vte`, `sum_tree` etc. — exactly mirroring `warp_terminal`'s own clean deps
- Re-export only the subset of `warp_terminal` types that don't transitively pull `warpui`
- This is the genuine "facade" work; current scaffold is a placeholder

---

## 4. Vulkan spike — ready for user device tests

`spikes/vulkan-surface-recreate/`:
- ✅ Builds for `aarch64-linux-android` (via `cargo ndk -t arm64-v8a --platform 26 build --release`)
- ✅ `.so` artifact: `target/aarch64-linux-android/release/libvulkan_surface_recreate.so` (716 KB)
- ✅ JNI exports: `nativeSurfaceCreated/Destroyed/Changed` holding `Mutex<Option<SurfaceState>>` of `ash::vk::SurfaceKHR`
- ✅ Android demo app: `android/app/src/main/java/com/warpmobile/spike/MainActivity.kt`
- ✅ README has Choreographer measurement methodology + adb rotation script

User Task #8 trigger condition met. Spike runtime measurement on Pixel 7a + Galaxy A14 + Pixel 9 Pro can begin.

---

## 5. User-required next steps (in order)

| Task | Action | Reads | Writes |
|---|---|---|---|
| #2 | Run symlink-jniLibs `execve()` test on Pixel 7a (Android 14), Galaxy A14 (Android 15), Pixel 9 Pro (Android 16-Beta). Use Termux Android 10 wiki technique. | Plan Pre-mortem B | `M0-symlink-jnilibs.md` |
| #8 | Run Vulkan-Surface-recreate spike 100 cycles on each of 3 devices. Measure Choreographer frame-recovery p95. Target < 200ms. | `spikes/vulkan-surface-recreate/README.md` | `M0-vulkan-spike-report.md` |
| #9 | Answer Tension 3 Questions A-E (cloud AI in v1 vs v2-only). | Plan section line 256-266 | `M0-tension3-decision.md` |
| Plan Amendment | Apply D1 → D2-lite changes above to `ralplan-warp-on-mobile.md` (Decision D revised + Layer 1 revised + M2 split + facade scaffold revision documented) | this doc Section 3 | edits to `ralplan-warp-on-mobile.md` |
| #10 | Integrate all M0-*.md results, write final `M0-go-no-go.md` with sign-off. | all M0-*.md | `M0-go-no-go.md` |

---

## 6. Honest risks for M0 closure

1. **D2-lite scope is wider than D1 in absolute LoC** (write ~3,500 new lines of Layer 1 vs cfg-gate ~3,334 existing lines), but D2-lite is **bounded** (we own the boundary) where D1 was **leaky** (every upstream font/winit change re-enters our cfg-gate). The plan amendment is mandatory; do not delay.
2. **Vulkan spike compiles but is not field-tested.** A "compiles for arm64-v8a" pass is necessary but not sufficient. If 3-device frame-recovery p95 > 200ms, the L1 risk re-elevates and the Companion retreat path (ADR alternative #6) becomes live again.
3. **`android-activity` E0282 is solvable** (1-line workspace Cargo.toml feature flag), but not yet attempted. Worth a 30-minute spike before declaring M1 ready.
4. **`fonts/font-kit` 2,834-line dependency is the real chokepoint**. Most of M2a's "4 weeks for 4 areas" goes to FontDB (cosmic-text wrapping). Underestimating cosmic-text integration is the most likely M2 schedule risk.

---

## 7. Recommendation: pause for plan amendment + user device work

**Do NOT auto-trigger M1.** M0 has produced strong evidence that requires:
- (a) Plan Decision D revision (D1 → D2-lite) committed to `.omc/plans/`
- (b) User completion of Tasks #2, #8, #9
- (c) Final `M0-go-no-go.md` integration with above included

After (a)+(b)+(c), M1 can start with confidence on the new architecture. Estimated additional time before M1: 1-2 days for plan amendment (agent) + 1-3 days for user device tests + decision.
