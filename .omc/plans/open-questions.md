# Open Questions / Spike Targets

## ralplan-warp-on-mobile - 2026-04-29

Items tagged [unverified] from the upstream synthesis that must be empirically validated before or during the corresponding milestone.

### M0 — F-Droid + L1 Renderer-Risk Feasibility Spike

> **Amendment 1 (2026-04-29 evening)**: M0 autonomous portion (7/7) complete. Status updates inline below. Device matrix updated to **Galaxy S24 Ultra (Android 16, SDK 36) + Galaxy S21+ (Android 15, SDK 35) + Galaxy S8 (Android 9, SDK 28)** — three connected Samsung devices forming SDK 28/35/36 tight dialectic matrix.

- [ ] Does `nix::pty::openpty` work on Bionic API 26+ without modification? — Drives M1 PTY plumbing; failure forces custom syscall path
- [ ] Does `termux-exec system_linker_exec` work for arbitrary `$PREFIX` on Android 16 (S24 Ultra)? — Pre-mortem Scenario B trigger; failure invalidates the F-Droid distribution path. **(USER task #2)**
- [ ] Does the F-Droid build server tolerate large bootstrap zips as auxiliary artifacts, or do we need extract-on-first-run from a downloaded asset? — Affects M4 packaging decisions
- [x] Will `cargo check --target aarch64-linux-android -p warp_terminal` AND `-p app` separately produce tractable counts of unbuildable deps? — **RESOLVED: M0 task #3 measured 3,334 cfg-gate lines (font-kit ~2,834 + winit/android-activity ~500). 6.7× Pre-mortem C threshold. D1 invalidated; D2-lite chosen. See `M0-deps-report.md`.**
- [ ] **(USER #8)** Vulkan-Surface-recreate spike runtime on Galaxy S24 Ultra (Android 16) + Galaxy S21+ (Android 15) + Galaxy S8 (Android 9 SDK 28) — Spike `.so` 716KB pre-built per task #5; user runs 100 cycles per device, measures `Choreographer.postFrameCallback` frame-recovery p95 < 200ms. Output: `M0-vulkan-spike-report.md`. **Three-device matrix complete.**
- [x] **(NEW)** `warpui::platform` trait diff archeology — **RESOLVED: M0 task #4 measured 89 methods across `Delegate/DispatchDelegate/FontDB/TextLayoutSystem/Window/WindowContext/WindowManager`; gpui-mobile delta = 0% identical / 35% portable / 15% incompatible / 50% missing. A2 invalidated. See `M0-platform-trait-delta.md`.**
- [x] **(NEW)** Scaffold empty `crates/warp_terminal_mobile_facade` with cfg-dialect locked — **PARTIAL: M0 task #6 committed scaffold (commit `5400c66` on `warp-mobile/m0-facade` branch) but `cargo check` fails on transitive `warp_terminal` deps (host: Metal Toolchain; Android: `android-activity` E0282). Per Amendment 1 D2-lite, facade scaffold needs M3-prep refactor: drop `warp_terminal` direct dep, add clean subset deps only. Plan acceptance #3 will pass after refactor.**
- [ ] **(USER #9)** Tension 3 user-decision gate (Questions A-E from ADR Tension 3 subsection) — does v1 ship cloud AI as core or opt-in; F-Droid NonFreeNet acceptance; AGPL §7 lawyer review path; companion-mode retreat trigger. Output: `M0-tension3-decision.md`.

### M2 — `warpui::platform::android` Backend
- [x] Can `warpui::platform::android` derive cleanly from `linux` (A1) or `headless` (A4) or `wasm` backend? — **RESOLVED by Amendment 1: A4 (`headless` base) chosen. M0 archeology Task 7 confirms 85/89 methods stubbed in `headless`; only 4 areas need real work (`render_scene`, `request_frame_capture`, `FontDB` 15 methods, `TextLayoutSystem` 2 methods). Estimated 3-4 person-weeks for Layer 1.**
- [ ] What is Warp's default fallback rendering backend (CPU rasterizer / GLES path) if Vulkan-on-Android proves unviable? — Source-archeology task spanning `crates/warpui/src/platform/{linux,headless,wasm}/`
- [ ] How does `warpui::FontDB` consume `.ttf`/`.otf` files at runtime (freetype-rs vs FontConfig)? — FontConfig won't work on Android
- [ ] Does `Activity.recreate()` work cleanly with `WindowInsets` for IME, or do we need our own IME insets handling? — IME edge case risk

### M3 — Warp Product Logic Integration (Layer 2b)
- [ ] Does Warp's `crates/warp_terminal` (Layer 2a clean side) actually compile to `aarch64-linux-android` once `warpui::platform::android` stub is in place? — Validates whole architecture; expected mostly-clean per Pre-mortem C revision
- [ ] How tangled is `app/src/terminal/model/session.rs`'s dependency topology on `app::ai` / `app::feature_flag` / `app::app_context` / `app::ssh`? Can we cfg-gate cleanly or do we need the facade-crate detour? — Pre-mortem Scenario C; *(note: previous draft referenced `terminal_model.rs` which does NOT exist; actual files in `app/src/terminal/model/` per `warpdotdev/Warp@d0f045c` are `session.rs`, `blockgrid.rs`, `blocks.rs`, etc.)*
- [ ] How will we handle the `warp_terminal` -> `warpui` dependency cleanly given our stubbed `warpui::platform::android`? — Sub-spike during M3

### M4 — Termux Bootstrap
- [ ] Are `pkg`/`apt` (Termux-flavored) reliable enough as on-device package managers, or should we ship a smaller "no apt, only static binaries" v1? — Survey Termux user reports; if >5% crash rate on flagship, reconsider

### M6 — AI Integration
- [ ] Is Anthropic API rate-limiting consumer-friendly enough for a BYOK app where users have variable-tier API keys? — Affects backoff strategy and UX

### Cross-cutting
- [ ] AGPL-3.0 + Anthropic API SDK terms compatibility (expected fine, but verify before shipping)
- [ ] Privacy-preserving telemetry sink choice (Sentry self-hosted vs Plausible-style minimal counter) for AGPL alignment
- [ ] **(NEW)** AGPL §7 (no further restrictions) vs Anthropic BYOK ToS lawyer review pre-v1 ship — confirm whether shipping a BYOK config (where the user's own API key invokes Anthropic ToS, not ours) creates an AGPL §7 conflict. Block v1 release on this opinion.
- [ ] **(NEW)** F-Droid NonFreeNet anti-feature label acceptance — v1 ships with optional Anthropic API dependency; this is a known coherence gap with Principle 1 (open-source-first). Confirm acceptable at Tension 3 user gate (M0).
- [ ] **(NEW)** Companion-mode retreat trigger — at what point does the rejected "Warp Companion" alternative get reconsidered? Define in `M0-tension3-decision.md` (e.g., "if M0 Vulkan spike fails on 2 of 3 reference devices").
