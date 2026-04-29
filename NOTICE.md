# NOTICE — Third-Party Attribution

This project is a derivative work that ports [Warp Terminal](https://github.com/warpdotdev/Warp) to Android with a bundled Termux runtime. It depends on, derives from, or references the following third-party projects. All license obligations of upstream projects are inherited and propagated.

---

## Primary upstreams (forked or directly bundled)

### Warp Terminal — `warpdotdev/Warp`
- **License**: AGPL-3.0-only (workspace), MIT (`warpui_core` / `warpui` crates)
- **Upstream**: https://github.com/warpdotdev/Warp
- **Pinned commit**: `d0f045c01bacbd845a631d07da30f277cfd2b98d` (2026-04-28)
- **Usage**: Source-level fork for the terminal+block+agent core. Long-lived `mobile/main` branch periodically rebased onto upstream master. Layer 2a (`crates/warp_terminal` + clean deps) is reused largely as-is; Layer 2b (`app/...` subset) is wrapped in `warp_terminal_mobile_facade`.

### termux-packages — `termux/termux-packages`
- **License**: GPL-3.0-or-later (collection); individual packages have their own licenses (preserved per-package).
- **Upstream**: https://github.com/termux/termux-packages
- **Usage** (M4 onwards): Forked + retargeted to a new `$PREFIX` matching this project's package name. Bootstrap zip bundled in APK. Per-package source/patches will be re-published with the binary distribution per AGPL §6 + GPL Corresponding Source obligations.

### gpui-mobile — `itsbalamurali/gpui-mobile`
- **License**: TBD (verify upstream LICENSE before any direct reuse)
- **Upstream**: https://github.com/itsbalamurali/gpui-mobile
- **Usage**: **Architecture reference only**, NOT a Cargo dependency. M0 trait diff (see `.omc/m0-artifacts/M0-platform-trait-delta.md`) confirmed gpui-mobile targets Zed's `gpui::Platform` trait family, not Warp's `warpui_core::platform::*`. We study its `AndroidWindow` / `AndroidPlatform` patterns when implementing our own `warpui::platform::android` backend.

---

## Secondary references / inspirations

### Termux app — `termux/termux-app`
- **License**: GPL-3.0-only
- **Upstream**: https://github.com/termux/termux-app
- **Usage**: Reference for Android terminal app patterns (TerminalView/TerminalSession/TerminalEmulator), shell hook conventions, package management UX. **Not bundled, not forked.** Architecture parallels documented in `.omc/plans/`.

### Zed — `zed-industries/zed`
- **License**: GPL-3.0-or-later (collection); various sub-licenses
- **Upstream**: https://github.com/zed-industries/zed
- **Usage**: Origin of GPUI framework that Warp's `warpui` is derived from. We do not depend on Zed directly.

### Anthropic Claude API
- **License**: Proprietary (Anthropic ToS)
- **Usage** (v1+, pending Tension 3 user-gate decision): Cloud AI provider for inline ghost-text (Haiku) and agent (Sonnet). User-supplied API key (BYOK). User accepts Anthropic ToS at first use; this app does not redistribute Anthropic's service, only consumes it as a client. AGPL §13 not triggered.

---

## License obligation summary

This work as a whole is distributed under **AGPL-3.0-only**. Per AGPL §6:
- Object-code (APK/AAB) distribution must be accompanied by the corresponding source.
- Source repository: https://github.com/ImL1s/warp-mobile-android (private during M0-M2; will turn public before v1 alpha release per AGPL §6 source-disclosure obligation when binaries are distributed).
- Each released APK `versionCode` corresponds to an exact tag in the source repository.

Per AGPL §13: combination with GPL-3.0 work (Termux runtime / termux-packages) is explicitly permitted.

Per AGPL §7: no further restrictions are added beyond AGPL-3.0. This NOTICE file fulfills the source-disclosure requirement of AGPL §4 (preserve copyright notices and add notice of changes).

---

## Modifications

This project introduces the following modifications relative to upstream `warpdotdev/Warp@d0f045c`:

- New crate `crates/warp_terminal_mobile_facade/` (Android-only re-export layer; commit `5400c66` on branch `warp-mobile/m0-facade` of the warp-src submodule). Will be refactored per Decision D2-lite (Plan Amendment 1) before M2.
- Future: New module `crates/warpui/src/platform/android/` (Layer 1 backend, A4 derived from `headless`).

Detailed change manifest will be auto-generated per release into `CHANGES-vs-upstream.md`.
