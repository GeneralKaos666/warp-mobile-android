# Contributing to Warp on Mobile (Android)

Thanks for considering a contribution. This is a solo-developer project on a 12-18 month constrained-beta timeline; coordination via Issues first is strongly preferred over surprise PRs.

## Before opening a PR

1. **Read the milestone plan**: [`.omc/plans/ralplan-warp-on-mobile.md`](.omc/plans/ralplan-warp-on-mobile.md) — canonical plan with 5 amendments. Each milestone (M0-M6) has explicit acceptance criteria. Make sure your contribution maps to a story in `.omc/prd.json` or open an Issue to propose a new story.

2. **File an Issue first** describing what you want to change. The maintainer will respond with scope alignment, applicable milestone, and any pre-mortem concerns. PRs without prior Issue discussion are likely to be closed in favor of an Issue thread.

3. **Match the existing patterns**:
   - Test scripts go in `tools/scripts/test-*.sh` and accept device serial as `$1`. Never hardcode serials or absolute paths.
   - Rust code in the workspace uses `cargo ndk -t arm64-v8a check` for Android target verification.
   - Java/Kotlin uses standard AndroidX + Coroutines patterns; no new framework dependencies without discussion.

## Coding standards

- **Rust**: `cargo fmt` + `cargo clippy --target aarch64-linux-android` clean.
- **Kotlin**: Match existing Kotlin style in `android/app/src/main/java/dev/warp/mobile/*.kt`.
- **No emojis in code** unless explicitly requested (per project convention).
- **No new dependencies** without discussion. The `warp_terminal` / `warpui` upstream Cargo edges must stay zero-churn (D1.5-hybrid invariant — see Plan Amendment 5 §3 cfg-gate→extraction pivot for the architectural rationale).
- **Vulkan-warpui rendering, not Compose** (Plan Decision A4) — new UI surfaces follow the existing programmatic LinearLayout/Dialog pattern (AccessoryRow, BlockActionsSheet, AgentBlockSheet, SettingsActivity). Compose proposals belong in v3+ scope.
- **Flagship-first device class** — Adreno 6xx+ / API 31+ baseline (Plan Amendment 3). Sub-flagship-only code paths get deferred.

## Test requirements

- **New code paths need tests**. Unit tests live under `#[cfg(test)] mod tests` blocks in the relevant source file.
- **Device-side changes** require driver scripts in `tools/scripts/` and result.json artifacts under `.omc/m{N}-artifacts/`.
- **Build verification**: `cargo test` (all packages) must pass. `cargo ndk -t arm64-v8a check` must pass for Android targets.
- **CI gates** (auto-enforced on every PR — see [`.github/workflows/test.yml`](.github/workflows/test.yml)):
  - 157 host tests (66 host + 18 ai client + 73 facade)
  - `cargo audit --deny warnings` — no known-vulnerable or unmaintained crates
  - `cargo deny check` — license allowlist (AGPL-3.0 compatible only) + supply-chain source allowlist (per [`deny.toml`](deny.toml))
  - Android Kotlin compile smoke
- **Codex review SOP**: This project uses Codex (OpenAI) as a critic verifier per `.omc/prd.json` `verifierConfig.critic`. Maintainer dispatches Codex review after merge candidate is ready.

## Dependencies

If you're adding a Cargo or Gradle dep:

- **License must be permissive** (MIT / Apache-2.0 / BSD / ISC / Unicode-3.0 / etc.) per `deny.toml` `[licenses] allow`. GPL-only and proprietary licenses are AGPL-incompatible and will fail CI.
- **Source must be in the allowlist**: crates.io for Cargo deps, or the explicit git allowlist in `deny.toml` `[sources] allow-git` (currently `warpdotdev/cosmic-text` + `ImL1s/warp` forks only).
- **Run `cargo audit` + `cargo deny check` locally** before opening the PR — the CI gates them anyway, but local pre-check saves a round-trip.
- **Dependabot auto-PRs** weekly for patch/minor bumps + monthly for Gradle. If your manual add changes a dep that dependabot is tracking, mention it in the PR body so the maintainer can pause dependabot if needed.

## Reporting bugs

- Use GitHub Issues with the `bug` label.
- Include: device model, Android version (`adb shell getprop ro.build.version.release`), `adb logcat -d -s WarpTerminal:V` excerpt, and the exact reproduction steps.
- For UI bugs, attach a screenshot via `adb -s <serial> exec-out screencap -p > screenshot.png`.

## License

By contributing, you agree your code is licensed under AGPL-3.0-only (see [`LICENSE-AGPL`](LICENSE-AGPL)). This is a copyleft license — derivative works must also be AGPL-3.0. If your employer requires CLA assignment, please raise this in an Issue before contributing.

## Code of conduct

Be respectful. Technical disagreement is fine; personal attacks are not. The maintainer reserves the right to close any Issue/PR that violates this norm without further explanation.

## Acknowledgments

This project builds on:

- [warpdotdev/Warp](https://github.com/warpdotdev/Warp) — the upstream Warp Terminal (AGPL-3.0)
- [termux/termux-packages](https://github.com/termux/termux-packages) — the package ecosystem (M4+)
- [pop-os/cosmic-text](https://github.com/pop-os/cosmic-text) — text shaping/layout
- [ash-rs/ash](https://github.com/ash-rs/ash) — Vulkan bindings

See [`NOTICE.md`](NOTICE.md) for full attribution.
