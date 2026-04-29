# Warp on Mobile (Android port)

Open-source-first port of [Warp Terminal](https://github.com/warpdotdev/Warp) to Android, with a bundled Termux runtime. Targets F-Droid + GitHub Releases as primary distribution; Play Store is a v3+ optional path.

> **Status**: M0 spike phase (2026-04-29). Plan APPROVED via deliberate-mode RALPLAN consensus (Planner+Architect+Critic, 2 iterations, 13/13 PASS), amended once post-M0 evidence (Decision D1 invalidated → D2-lite chosen). See [`.omc/plans/ralplan-warp-on-mobile.md`](.omc/plans/ralplan-warp-on-mobile.md).

## What this is

A solo-dev 12-18 month constrained-beta port of Warp's terminal-with-blocks UX to Android. The phone runs a real Linux user space (forked from `termux-packages` with a project-specific prefix), Warp's block-based UI lives in a custom Vulkan/NDK Layer 1, and AI features use cloud Anthropic API (Haiku for inline ghost-text, Sonnet for agent — pending Tension 3 user-gate decision).

## What this is NOT

- **Not a Termux fork.** We bundle Termux's package collection (`termux-packages`); we do NOT fork the Termux Android app (`termux-app`). The terminal GUI is Warp.
- **Not a thin SSH client.** Termius / Blink Shell already do that well. We run a real local shell on-device.
- **Not a wholesale Compose rewrite.** Warp's `warpui` framework stays; we add an Android backend, not a parallel JVM-side UI.
- **Not Play-Store-first.** F-Droid is the primary distribution target.

## Architecture (5-layer, per Plan Amendment 1)

```
L0  Android Host Service     — Activity / Service lifecycle, FGS, JNI shim, IME, clipboard
L1  WarpUI Android backend   — warpui::platform::android (A4 derived from headless), Vulkan via ash + ANativeWindow
L2a Terminal Session Engine  — crates/warp_terminal + clean deps (warp_completer / warp_core / warp_util / vte / sum_tree)
L2b Warp Product Logic       — app/src/terminal/... subset + facade crate (D2-lite isolation)
L3  Termux Runtime+Packages  — fork termux-packages with new $PREFIX, bootstrap zip in APK
```

Cloud AI runs as a separate concern, not a layer. Local llama.cpp is v2+ opt-in.

## Repository layout

```
warp_termux/
├── README.md ← you are here
├── LICENSE-AGPL ← inherited from warpdotdev/Warp
├── NOTICE.md ← upstream attributions, license obligations, modifications log
├── docs/ ← architecture summary, contributing guide
├── warp-src/ ← warpdotdev/Warp fork (git submodule, pinned d0f045c)
├── spikes/ ← M0+ spike crates (Vulkan-Surface-recreate, symlink-jniLibs)
├── tools/scripts/ ← cross-device test runners (run-vulkan-spike.sh etc.)
└── .omc/
    ├── plans/ ← APPROVED implementation plan + open questions (tracked)
    ├── handoffs/ ← stage-to-stage decision context (tracked)
    └── m0-artifacts/ ← M0 evidence artifacts (deps report, trait delta, spike results, etc.)
```

## Status by milestone

- ✅ **M0 autonomous portion (7/9)** — env smoke, deps report, trait diff, A1-vs-A4 archeology, Vulkan spike code, facade scaffold all complete. See `.omc/m0-artifacts/M0-lead-summary-partial.md`.
- 🟡 **M0 user-gated (2/9)** — symlink-jniLibs runtime test on 3 devices (S24 Ultra Android 16 / S21+ Android 15 / S8 Android 9), Vulkan spike runtime measurement, Tension 3 user-gate decision. Test harnesses in `spikes/` + `tools/scripts/`.
- ⏸ **M0 close (1/9)** — go/no-go integration pending user-gated tasks.
- 📅 **M1+ — M6** — see plan section 6.

## Key M0 findings (Plan Amendment 1)

1. `cargo check --target aarch64-linux-android -p warp_terminal` showed transitive failures via `warpui` (font-kit + winit/android-activity), measured at **3,334 cfg-gate lines — 6.7× the Pre-mortem C 500-line threshold**. Decision D1 (cfg-gate everywhere) is empirically invalidated. Adopted **D2-lite** (`warp_terminal_mobile_facade` excludes `warpui` from its dep graph; Layer 1 self-implements 4 areas).
2. `gpui-mobile` is **NOT** a usable Cargo dependency. It targets Zed's `gpui::Platform` trait family, not Warp's `warpui_core::platform::*`. 89 trait methods analyzed: 0% identical, 35% portable, 15% incompatible, 50% missing. Reference for AndroidWindow/AndroidPlatform patterns only.
3. **A4 (`headless` base) confirmed** as Layer 1 derive base. 85/89 trait methods already stubbed in `warpui::platform::headless`; only 4 areas need real implementation: `render_scene`, `request_frame_capture`, `FontDB` (15 methods, wrap cosmic-text), `TextLayoutSystem` (2 methods). Estimate **3-4 person-weeks** for Layer 1 vs A1's 6-8 weeks.

## Build prerequisites

```
- Rust toolchain (1.88+, with target aarch64-linux-android)
- cargo-ndk (4.1+)
- Android NDK r25c+ (or the bundled r29 path documented in .envrc)
- JDK 17 (for Gradle/Android tooling)
- direnv (recommended) for auto-loading .envrc
```

## License

AGPL-3.0-only. See [`LICENSE-AGPL`](LICENSE-AGPL) and [`NOTICE.md`](NOTICE.md).

## Contributing

This is currently a solo-dev personal project. Contribution guidelines and PR process will be defined post-M2 alpha (when external interest becomes meaningful). For now, the canonical communication channel is `.omc/handoffs/` (stage decisions) and `.omc/m0-artifacts/` (evidence).
