# Handoff: ralplan-consensus → team-prd

## Decided
- **Direction**: Port `warpdotdev/Warp@d0f045c` to Android with bundled Termux runtime; open-source first (F-Droid + GitHub Releases primary, Play Store v3+ optional).
- **Architecture**: 5-layer — L0 Android Host Service / L1 WarpUI Android backend (`warpui::platform::android`) / L2a Terminal Session Engine (`crates/warp_terminal` + clean deps) / L2b Warp Product Logic (`app/` curated subset + facade) / L3 Termux Runtime (fork termux-packages with new prefix).
- **MVP timeline**: 7-phase, 13-18 month constrained beta. M0 expanded to 4-5 person-weeks with 9 tasks owning L1 risk early (Vulkan-Surface-recreate spike + trait diff + facade scaffold).
- **AI strategy**: Cloud Anthropic API first (Haiku inline + Sonnet agent); local llama.cpp Qwen-1.5B-Q4_K_M as v2+ opt-in.
- **License**: AGPL-3.0-only main + MIT for `warpui_core`/`warpui` (inherits from upstream); termux-packages GPL stays compatible per AGPL §13.

## Rejected
- **A1 fork termux-app + AI plugin** — A path was viable but doesn't unlock Warp's block UX.
- **A2 use gpui-mobile directly** — gpui-mobile targets Zed's GPUI, not Warp's `warpui_core`; docs.rs admits "full implementations coming soon"; trait incompatibility unverified must be confirmed in M0 (task 4).
- **A3 from-scratch Compose terminal** — abandons Warp UX leverage; defeats project purpose.
- **D2 deep refactor warp_terminal upfront** — too much disruption; chose D1 (cfg-gate) + facade hybrid promoted to M0 primitive.
- **ADR-Companion** (phone pairs to desktop Warp via SSH) — kept as documented retreat trigger if M0 spikes fail.

## Risks (carried forward)
1. **L1 WarpUI Android backend** (#1 death pit): gpui-mobile incompatible, must self-implement `warpui::platform::android` deriving from `linux` or `headless`. M0 task 5+7 owns this.
2. **L2 Warp Core split cleanliness**: `app/` crate (NOT `warp_terminal`) is tangled with `mio`/`nix`/`ai`/`feature_flag`. Pre-mortem C threshold: cfg-gate count ≤ 500 lines or convert to facade-crate. M0 task 6 scaffolds the facade.
3. **L3 PTY/lifecycle**: existing `app/src/terminal/local_tty/{shell.rs, event_loop.rs, mio_channel.rs}` must be derived/replaced for Android, integration with `event_loop` reactor is M1's hardest sub-task.
4. **L4 Termux**: F-Droid path bypasses Play Store W^X — symlink-jniLibs trick may break on Android 16+ (Pre-mortem B). M0 task 2 verifies on three devices.
5. **AGPL §7 + Anthropic BYOK ToS** coherence with F-Droid NonFreeNet — lawyer review pre-v1 ship (Follow-up).

## Files
- `/Users/iml1s/Documents/mine/warp_termux/.omc/plans/ralplan-warp-on-mobile.md` — final consensus plan (approved iter 2)
- `/Users/iml1s/Documents/mine/warp_termux/.omc/plans/open-questions.md` — open spike targets per milestone
- `/Users/iml1s/Documents/mine/warp_termux/warp-src/` — Warp source repo at commit `d0f045c`, branch master, clean

## Remaining for next stage (team-prd → team-exec for M0)
- **Agent-autonomous** (this batch):
  - Task 1: Install `cargo-ndk` + export `ANDROID_NDK_ROOT`/`ANDROID_HOME`
  - Task 3: `cargo check --target aarch64-linux-android -p warp_terminal` + write `M0-deps-report.md` quantifying cfg-gate scope
  - Task 4+7: `warpui::platform` trait surface enumerate + diff against `gpui-mobile` exports + Decision A1-vs-A4 evaluation → `M0-platform-trait-delta.md`
  - Task 5a: Write Vulkan-Surface-recreate spike code (50-line Rust+JNI standalone holding `VkSurfaceKHR` across `onPause`/`onResume`+rotation) under `spikes/vulkan-surface-recreate/`
  - Task 6: Scaffold empty `crates/warp_terminal_mobile_facade/` with cfg-dialect pre-declared per Pre-mortem C #5
- **User-required** (deferred to next batch):
  - Task 2: Run `execve()` symlink-jniLibs on Android 14/15/16-Beta on Pixel 7a + Galaxy A14 + Pixel 9 Pro
  - Task 5b: Run Vulkan spike on those three devices, measure frame-recovery p95 < 200ms
  - Task 8: Tension 3 user gate Questions A-E (cloud AI in v1 vs v2-only) → `M0-tension3-decision.md`
  - Task 9: M0 go/no-go integration after user device tests + decision

## Test gates for M0 close
1. `M0-deps-report.md` cfg-gate estimate ≤ 500 lines
2. Vulkan spike code compiles for `aarch64-linux-android`
3. `crates/warp_terminal_mobile_facade` `cargo check --target aarch64-linux-android` passes
4. Trait diff document committed with pinned commit hashes
5. (User) device-tests pass on three devices
6. (User) Tension 3 decision committed
