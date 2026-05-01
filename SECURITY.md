# Security Policy

## Reporting a vulnerability

If you discover a security issue in Warp on Mobile (Android), please report it via:

- **GitHub Security Advisories**: [Open a draft advisory](https://github.com/ImL1s/warp-mobile-android/security/advisories/new) (preferred — automatically private until publication)
- **Email**: `setsuna@designsharing.org` with subject prefix `[SECURITY] warp-mobile-android`

Please **do not** open a public Issue for security-sensitive reports.

## What to include

- Affected component (renderer, PTY backend, JNI bridge, IME, package manager, etc.)
- Reproduction steps with the exact APK build commit hash
- Impact assessment (information disclosure, privilege escalation, DoS, RCE, etc.)
- Device model + Android version where reproduced

## What to expect

- Acknowledgment within 7 days
- Initial severity assessment within 14 days
- Fix timeline depending on severity (CVSS-based; critical issues prioritized)
- Public disclosure coordinated via GitHub Security Advisory after a fix is shipped

## Scope

In-scope:

- Vulnerabilities in this repository's own code (`crates/`, `android/`, `tools/`)
- Vulnerabilities in this project's build of `warp-src/crates/warpui::platform::android`, `warp_terminal_mobile_facade`
- Issues with the bundled `termux-packages` fork retargeting that introduce security regressions vs upstream

Out-of-scope:

- Upstream `warpdotdev/Warp` issues (report to https://github.com/warpdotdev/warp)
- Upstream `termux/termux-packages` issues (report to https://github.com/termux/termux-packages)
- Android platform issues (report to Google via https://source.android.com/security/bulletin)

## Threat model (current scope)

This project ships a foreground-service-managed PTY + a Vulkan-rendered terminal UI + (M4+) a sandboxed Termux runtime. The threat model assumes:

- **Local app data is trusted** (per Android UID isolation): `/data/data/dev.warp.mobile/files/` is private to the app
- **Network**: Currently no outbound network. M4 will introduce HTTPS for the `apt` package manager (validates against Termux APT signing keys); M6 will introduce HTTPS to `api.anthropic.com` for AI features (BYOK + key stored in Android Keystore).
- **PTY child processes**: Run as the app UID; cannot escape the app sandbox via standard Android security model.
- **APK distribution**: AGPL-3.0 source-form distribution + reproducible-build target for F-Droid (M4-S08); Play Store v3+ requires Google Play Protect.

## Hardening notes

- Validation layer (`libVkLayer_khronos_validation.so`) excluded from release builds (per `M2-S04` design); only debug variant fetches it via SHA-256-pinned download in `android/app/build.gradle`.
- Bootstrap zip (M4+) uses SHA-256 verification at first-launch extraction (atomic `usr.tmp/` → `usr/` rename pattern).
- TerminalSimulationReceiver (debug-only test hook) is permission-gated in the production manifest (`PTY_CONTROL` signature permission); the `tools:remove` debug overlay strips this only in debug builds.

## Acknowledgments

Security researchers who report valid issues will be credited in the GitHub Security Advisory and in a CHANGELOG entry, unless they request anonymity.
