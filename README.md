# Warp on Mobile (Android port)

Open-source-first port of [Warp Terminal](https://github.com/warpdotdev/Warp) to Android, with a bundled Termux runtime. Targets F-Droid + GitHub Releases as primary distribution; Play Store is a v3+ optional path.

> **Status (2026-04-30)**: **M0 + M1 both CLOSED CONDITIONAL GO**. Plan APPROVED via deliberate-mode RALPLAN consensus + 4 Amendments. M2 (warpui::platform::android backend, 8-12 weeks) ready to start. See [`.omc/handoffs/lead-context-snapshot.md`](.omc/handoffs/lead-context-snapshot.md) and [`.omc/m2-kickoff.md`](.omc/m2-kickoff.md).

## What this is

A solo-dev 12-18 month constrained-beta port of Warp's terminal-with-blocks UX to Android. The phone runs a real Linux user space (forked from `termux-packages` with a project-specific prefix), Warp's block-based UI lives in a custom Vulkan/NDK Layer 1, and AI features use cloud Anthropic API (Haiku for inline ghost-text, Sonnet for agent).

## What this is NOT

- **Not a Termux fork.** We bundle Termux's package collection (`termux-packages`); we do NOT fork the Termux Android app (`termux-app`). The terminal GUI is Warp.
- **Not a thin SSH client.** Termius / Blink Shell already do that well. We run a real local shell on-device.
- **Not a wholesale Compose rewrite.** Warp's `warpui` framework stays; we add an Android backend, not a parallel JVM-side UI.
- **Not Play-Store-first.** F-Droid is the primary distribution target.

## Architecture (5-layer, per Plan Amendments 1-4)

```
L0  Android Host Service     — Activity / Service lifecycle, FGS, JNI shim, IME, clipboard
                               (M1 CLOSED: Service skeleton + PTY plumbing chain Task#28→#33→#35)
L1  WarpUI Android backend   — warpui::platform::android (A4 derived from headless), Vulkan via ash + ANativeWindow
                               (M0 spike CLOSED <200ms p95; M2 main work)
L2a Terminal Session Engine  — crates/warp_terminal + clean deps (M3 scope)
L2b Warp Product Logic       — app/src/terminal/... subset + facade crate under D1.5-hybrid (M3 scope)
L3  Termux Runtime+Packages  — fork termux-packages with new $PREFIX, bootstrap zip in APK
                               (M0 symlink-jniLibs path verified; M4 main work)
```

Cloud AI runs as a separate concern, not a layer. Local llama.cpp is v2+ opt-in.

## Status by milestone

- ✅ **M0** Foundation spike — CLOSED CONDITIONAL GO @ commit `24a2c1c`. L1 Vulkan recreate 7-52ms (all <<200ms gate); L4 PROVISIONAL GO. Evidence in `.omc/m0-artifacts/M0-go-no-go.md`.
- ✅ **M1** Android PTY/Service prototype — CLOSED CONDITIONAL GO @ commit `f7feb3f`. **10/10 stories PASS** on Galaxy S24 Ultra (delta_ms=26 reattach, observed="24 80" resize, orphans=0 clean kill, 30-min idle PID-constant + 4ms pwd). Plan §6 M1 ACs 5/5 satisfied for flagship. Evidence in `.omc/m1-artifacts/M1-go-no-go.md`.
- 🟡 **M2** WarpUI Android backend — READY TO START (8-12 weeks). See [`.omc/m2-kickoff.md`](.omc/m2-kickoff.md).
- 📅 **M3** Warp facade integration (8-12 weeks)
- 📅 **M4** Termux bootstrap + package story (10-16 weeks)
- 📅 **M5** Mobile UX polish (12-16 weeks)
- 📅 **M6** AI integration (Haiku inline + Sonnet agent)

## Repository layout

```
warp_termux/
├── README.md                 ← you are here
├── CLAUDE.md                 ← AI agent entry point — read first if you're an AI session
├── LICENSE-AGPL              ← inherited from warpdotdev/Warp
├── NOTICE.md                 ← upstream attributions, license obligations
├── progress.txt              ← iteration log with lessons learned (M0+M1)
├── Cargo.toml                ← Rust workspace root
├── crates/android-host/      ← Rust JNI host (M1 deliverable: PTY + ping)
├── android/                  ← Gradle project (M1 deliverable: FGS + Service)
├── tools/scripts/            ← Device test drivers (test-pty-*.sh, etc.)
├── spikes/                   ← M0 spike crates (vulkan-surface-recreate, symlink-jnilibs)
├── warp-src/                 ← gitignored — Warp upstream fork (separate git repo)
├── termux-packages/          ← gitignored — Termux fork on warp-mobile/main (M4 runtime)
└── .omc/
    ├── plans/                ← canonical RALPLAN with 4 amendments
    ├── handoffs/             ← lead-context-snapshot.md is the resume entry point
    ├── m0-artifacts/         ← M0 evidence + go/no-go
    ├── m1-artifacts/         ← M1 evidence (S05 evidence, S06-S09 result.json, go/no-go)
    ├── m2-kickoff.md         ← M2 forward-looking dispatch instructions
    ├── m4-artifacts/         ← M4 evidence (S02 fork retarget, future bootstrap zip + sealing)
    └── prd.json              ← M1 stories (10/10 PASS); subsequent milestones auto-generated
```

## Build prerequisites

```
- Rust toolchain (1.88+, with target aarch64-linux-android)
- cargo-ndk (4.1+)
- Android NDK r25c+ (or the bundled r29 path documented in .envrc)
- JDK 17 (for Gradle/Android tooling)
- direnv (recommended) for auto-loading .envrc
```

### Fresh-clone setup (gh clone)

```bash
# 1. Clone main repo
gh repo clone ImL1s/warp-mobile-android
cd warp-mobile-android

# 2. Clone warp-src submodule manually (gitignored — separate fork)
gh repo clone ImL1s/warp warp-src
cd warp-src && git checkout warp-mobile/m0-facade && cd ..

# 2b. Clone termux-packages fork (M4 runtime, gitignored — separate fork)
gh repo clone ImL1s/termux-packages termux-packages
cd termux-packages && git checkout warp-mobile/main && cd ..
# Optional: re-run idempotent retargeting (no-op if already on dev.warp.mobile)
# bash termux-packages/scripts/setup-warp-prefix.sh

# 3. Render local cargo config from template
tools/scripts/setup-cargo-config.sh

# 4. Build native lib for Android
cargo ndk -t arm64-v8a build -p warp-mobile-android-host

# 5. Build APK
cd android && ./gradlew :app:assembleDebug && cd ..

# 6. Install + launch on connected device (requires adb)
adb install -r android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.warp.mobile/.MainActivity

# 7. Trigger PTY spawn (no UI yet — adb-driven only)
adb shell "am start-foreground-service -n 'dev.warp.mobile/.WarpTerminalService' \
  -a dev.warp.mobile.PTY_SPAWN --es cmd 'sh'"

# 8. Read logcat to see PTY output
adb logcat -d | grep WarpTerminal:PtyOutput
```

## License

AGPL-3.0-only. See [`LICENSE-AGPL`](LICENSE-AGPL) and [`NOTICE.md`](NOTICE.md).

## Contributing

This is currently a solo-dev personal project. Contribution guidelines and PR process will be defined post-M2 alpha (when external interest becomes meaningful). For now, the canonical communication channel is `.omc/handoffs/` (stage decisions) and `.omc/m{0,1}-artifacts/` (evidence).

## For AI assistants resuming this project

**If you are an AI instance picking this project up after a context compact, in a new session, or from a fresh `gh clone`**: read [`CLAUDE.md`](CLAUDE.md) first. It is the canonical AI agent entry point and routes to the appropriate handoff document.

The short version:
1. Read `CLAUDE.md` — explains project conventions + points to handoff
2. Read `.omc/handoffs/lead-context-snapshot.md` — full lead state (identity, user prefs, M0+M1 status, M2 ready)
3. Read `.omc/m2-kickoff.md` — if M2 not yet started, this is the dispatch path
4. Read `.omc/plans/ralplan-warp-on-mobile.md` — canonical plan with 4 amendments at top
5. Read `.omc/prd.json` — current milestone story states
6. Read `progress.txt` — iteration log

Designed so you don't need conversation history scrollback to resume.
