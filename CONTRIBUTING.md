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
- **No new dependencies** without discussion. The `warp_terminal` / `warpui` upstream Cargo edges must stay zero-churn (architecture invariant from Plan Amendment 2 / D1.5-hybrid).

## Test requirements

- **New code paths need tests**. Unit tests live under `#[cfg(test)] mod tests` blocks in the relevant source file.
- **Device-side changes** require driver scripts in `tools/scripts/` and result.json artifacts under `.omc/m{N}-artifacts/`.
- **Build verification**: `cargo test` (all packages) must pass. `cargo ndk -t arm64-v8a check` must pass for Android targets.
- **Codex review SOP**: This project uses Codex (OpenAI) as a critic verifier per `.omc/prd.json` `verifierConfig.critic`. Maintainer dispatches Codex review after merge candidate is ready.

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
