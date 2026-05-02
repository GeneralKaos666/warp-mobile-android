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

## Threat model (current scope, post-M6)

This project ships a foreground-service-managed PTY + a Vulkan-rendered terminal UI + a sandboxed Termux runtime + a BYOK AI integration. The threat model assumes:

- **Local app data is trusted** (per Android UID isolation): `/data/data/dev.warp.mobile/files/` is private to the app.
- **Outbound network — apt** (M4): HTTPS to `packages-cf.termux.dev`. Validates against Termux APT signing keys at install time. Bootstrap zip integrity verified via M4-S08 reproducibility pin (`tools/scripts/m4-bootstrap-snapshot.sha256`).
- **Outbound network — Anthropic** (M6 — shipped): HTTPS to `api.anthropic.com` for ghost-text + agent task. **BYOK** — the user provides their own Anthropic API key, stored in [`EncryptedSharedPreferences`](https://developer.android.com/topic/security/data) backed by an AES256-GCM master key in the Android Keystore (alias `warp-ai-key-v1`). Key never lands in plaintext on disk; logcat redaction via `AiKeyStore.redact()` + defensive `scrub()` regex for `sk-ant-*` patterns. `SettingsActivity` uses `FLAG_SECURE` so the key-entry surface is excluded from screenshots / casts. `AgentBlockSheet` also uses `FLAG_SECURE` (the streamed AI response can reflect terminal output containing secrets).
- **PTY child processes**: Run as the app UID; cannot escape the app sandbox via standard Android security model.
- **Block.output capture** (v1-prep): The mirror Block model (M3 + Block.output v1-prep addition) records stdout/stderr bytes between Preexec/CommandFinished into a 64 KB-capped buffer per block, included in the `terminalBlocksDump` JSON returned via JNI. This output is forwarded to the AI agent (M6-S04 round-2) as `<output>` XML-tagged content for the "Explain this block" flow. Users running commands that print secrets (`cat .env`, `env`) will have those bytes in the AI prompt — the BYOK system preamble marks the content as DATA but this is defense-in-depth; sensitive output should not be sent to AI agents.
- **Clipboard**: Copy / Paste paths use `EXTRA_IS_SENSITIVE` on Android 13+ so the system clipboard preview suppresses the first line. Terminal output may contain secrets (env vars, `.env` content, AWS keys) so the sensitive flag is non-negotiable on those paths.
- **APK distribution**: AGPL-3.0 source-form distribution + reproducible-build target for F-Droid (M4-S08, re-verified in v1-prep — see `.omc/m4-artifacts/M4-S08-reproducibility-verify.json`). Play Store v3+ requires Google Play Protect.

## Hardening notes

- Validation layer (`libVkLayer_khronos_validation.so`) excluded from release builds (per `M2-S04` design); only debug variant fetches it via SHA-256-pinned download in `android/app/build.gradle`.
- Bootstrap zip (M4+) uses SHA-256 verification at first-launch extraction (atomic `usr.tmp/` → `usr/` rename pattern). The upstream Termux apt snapshot is pinned in `tools/scripts/m4-bootstrap-snapshot.sha256`; build-bootstrap.sh refuses to proceed on drift.
- TerminalSimulationReceiver (debug-only test hook) is permission-gated in the production manifest (`PTY_CONTROL` signature permission); the `tools:remove` debug overlay strips this only in debug builds.
- BYOK API key never enters Intent extras / Broadcast payloads. `AiGhostStreamStart` JNI call passes the key directly to Rust memory via JString; no Intent/Bundle intermediary. SettingsActivity is `exported="false"` (in-app `⚙` button is the only entry).
- Release `.so` is stripped (`[profile.release] strip = "symbols"`) — debug + non-dynamic symbol tables removed. JNI export resolution unaffected (dynamic symbol table preserved).
- CI gates (every PR): `cargo audit --deny warnings` (RUSTSEC advisories), `cargo deny check` (license + supply-chain). License allowlist enforces AGPL-3.0 compatibility on every dep; source allowlist restricts to crates.io + the explicit `[sources] allow-git` entries (currently `warpdotdev/cosmic-text` + `ImL1s/warp` only).
- Dependabot weekly cargo + GitHub-actions PRs; monthly Gradle PRs. All bumps go through the same CI gates above.

## Acknowledgments

Security researchers who report valid issues will be credited in the GitHub Security Advisory and in a CHANGELOG entry, unless they request anonymity.
