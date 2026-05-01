# Warp on Mobile — AI Agent Entry Point

> **You are an AI assistant (Claude Code, Cursor, GitHub Copilot, etc.) opening this project.** This file is the canonical entry point. Read it first.

## What is this project

Open-source-first port of [Warp Terminal](https://github.com/warpdotdev/Warp) to Android, with a bundled Termux runtime. AGPL-3.0-only. Solo-dev 12-18 month timeline. F-Droid + GitHub Releases primary distribution.

See [`README.md`](README.md) for product description and architecture overview.

## Current milestone state (2026-05-01)

- **M0** (foundation spike): CLOSED CONDITIONAL GO @ commit `24a2c1c`
- **M1** (Android PTY/Service prototype): CLOSED CONDITIONAL GO @ commit `f7feb3f`, **10/10 stories PASS**
- **M2** (warpui::platform::android backend): CLOSED CONDITIONAL GO @ commit `0506c35`, **12/14 stories PASS** (M2-S13 user-deferred per 「先跳過便宜手機」)
- **M3** (Layer 2b integration: facade + DCS + Block + dynamic_grid): **CLOSED CONDITIONAL GO** @ commit `8ec75c8`, **12/12 stories PASS** (27 codex rounds; Plan Amendment 5 cfg-gate→extraction pivot)
- **M4** (Termux runtime: zsh + GNU coreutils + APT): READY TO START

To verify currency: `git log --oneline -5` and check `.omc/prd.json` `passes` fields.

## How to resume / pick up work

**Read in this order**:

1. **This file** (`CLAUDE.md`) — you are here
2. [`.omc/m3-artifacts/M3-go-no-go.md`](.omc/m3-artifacts/M3-go-no-go.md) — **authoritative M3 close-out document** (519 lines; CONDITIONAL GO verdict; all 12 stories ledger; per-layer GO/CONDITIONAL/NO-GO; M4 carry-overs)
3. [`.omc/m3-artifacts/M3-kickoff-confirmed.md`](.omc/m3-artifacts/M3-kickoff-confirmed.md) — M3 kickoff doc (5-round codex PASSed)
4. [`.omc/m2-artifacts/M2-go-no-go.md`](.omc/m2-artifacts/M2-go-no-go.md) — M2 close-out (508 lines; 12/14 PASS)
5. [`.omc/handoffs/lead-context-snapshot.md`](.omc/handoffs/lead-context-snapshot.md) — earlier session resume doc (M0+M1 era; superseded for M2+ by go-no-go docs)
6. [`.omc/plans/ralplan-warp-on-mobile.md`](.omc/plans/ralplan-warp-on-mobile.md) — canonical plan with **5 amendments** at top (Amendment 5 = M3 cfg-gate→extraction pivot, 2026-04-30)
7. [`.omc/prd.json`](.omc/prd.json) — current milestone story states (M3-S01..S12 all `passes:true`)
8. [`progress.txt`](progress.txt) — iteration log with lessons learned

**Do NOT** attempt to derive state from full conversation history. The M3 close-out doc + kickoff doc are designed to be read cold.

## User governance preferences (from CLAUDE.md global instructions)

These were established by the user explicitly and apply project-wide:

- **「全自動」** — full-auto governance. Do NOT ping user for: task assignments, strategy decisions, device runs (workers have adb access), spike redos, plan amendments, Codex review responses, repo metadata, commit messages, branch names.
- **「你自己決定」** — lead has authority to commit, push, run gh repo create, plan amendments, worker dispatch, Codex re-reviews — without re-asking.
- **Only ping user for**: irreversible operations user must approve (going public, formal v1 release, switching to Companion retreat) OR truly unrecoverable blockers.
- **Language**: 繁體中文 user-facing prose; English for code/identifiers/file paths/commit messages.
- **Tone**: short, direct, no excessive confirmation gates.

## Verifier SOP

`.omc/prd.json` has `verifierConfig.critic = "codex"`. **Every worker deliverable goes through Codex review** before story is marked passes:true. Use `omc ask codex --prompt "$(< /tmp/codex-*.md)"` (write prompt to file first to avoid zsh `()` parse errors). Background dispatch via `run_in_background: true`. Read verdict from `.omc/artifacts/ask/codex-*.md`.

## Project conventions

- **Branching**: main only. All commits push direct to main. No feature branches.
- **warp-src is gitignored**: it's a separate git repo (Warp upstream fork at `ImL1s/warp:warp-mobile/m0-facade`). Clone manually if needed (see README "Fresh-clone setup").
- **`.omc/` partial gitignore**: `plans/`, `handoffs/`, `m0-artifacts/`, `m1-artifacts/` tracked; `state/`, `artifacts/`, `notepad.md`, `project-memory.json` gitignored.
- **OMC orchestration runtime state** in `.omc/state/`: clear via `/oh-my-claudecode:cancel` when milestone closes.
- **Codex review prompts**: write to `/tmp/codex-*.md` first, dispatch via `omc ask codex --prompt "$(< file)"`.
- **Driver scripts use `am force-stop` not `am kill`** (Plan Amendment 4): `am kill` is no-op against running FGS per AOSP semantics.
- **Recommended test device classes** (per Plan Amendment 3 minSdk 31, Adreno 6xx+ baseline):
  - **Primary flagship** — Snapdragon 8 Gen 1+ / Adreno 730+ / API 33+ (e.g., Galaxy S24 Ultra, Pixel 7+)
  - **Secondary flagship** — Snapdragon 888 / Adreno 660 / API 31+ (e.g., Galaxy S21+, Pixel 6)
  - **Low-end (M2 carry-over)** — Snapdragon 730G/778G / Adreno 618-642L / API 31+ (e.g., Pixel 4a, Galaxy A52s)
  - **Below-min** — anything below API 31 / Adreno 6xx (e.g., Mali-G71-era, Galaxy S8) is dropped per Amendment 3.
  - Pass `<serial>` as first arg to all driver scripts (`tools/scripts/test-*.sh <serial>`); never hardcode serials.

## Available OMC tools (oh-my-claudecode plugin)

If `oh-my-claudecode` plugin is installed:
- `/oh-my-claudecode:ralph` — until-done loop, no state cleanup
- `/oh-my-claudecode:autopilot` — full lifecycle (Phase 0 expansion → Phase 5 cleanup); skips Phase 0+1 if ralplan exists
- `/oh-my-claudecode:cancel` — clean state and exit
- Worker agents: `executor`, `deep-executor`, `planner`, `architect`, `critic`, `verifier`, `analyst`

If you don't have the OMC plugin, you can still:
- Read all the docs in `.omc/`
- Run device tests via `tools/scripts/test-*.sh`
- Build via `cargo ndk -t arm64-v8a build` + `cd android && ./gradlew :app:assembleDebug`
- Run `omc ask codex` directly if you have Codex CLI

## Quick verification commands

```bash
# Verify all 12 M3 stories PASS
jq -r '.stories | map(select(.passes == true)) | length' .omc/prd.json   # → 12
jq -r '.stories | length' .omc/prd.json                                   # → 12

# Confirm latest M3 close-out commits
git log --oneline -10

# Build sanity check
cargo test -p warp-mobile-android-host                                    # → 45 passed (M3-S11 added 3 emoji smoke tests)
cargo test -p warp_terminal_mobile_facade --manifest-path warp-src/Cargo.toml  # → 73 passed

# Release APK size verification (M3-S10 baseline 7.4MB)
cd android && ./gradlew :app:assembleRelease
du -h app/build/outputs/apk/release/app-release-unsigned.apk              # → 7.4M

# Connected devices (your serials will differ)
adb devices                                                                # → Galaxy S24 Ultra R5CX10VFFBA primary
```

## What you should NOT do

- Don't skip reading `.omc/handoffs/lead-context-snapshot.md` and start fresh — you'll re-derive months of decisions and risk getting them wrong.
- Don't switch to a Compose UI — `warpui::platform::android` derived from headless is the chosen path (Plan §6 M2; Decisions A4 + D1.5-hybrid).
- Don't fork `termux-app` — we bundle `termux-packages` only.
- Don't ping the user for routine decisions — see "User governance preferences" above.
- Don't commit `.cargo/config.toml` (machine-absolute paths). The template at `.cargo/config.toml.template` is the source of truth; run `tools/scripts/setup-cargo-config.sh` to render.
- Don't commit `warp-src/` (it's a separate git repo, gitignored).

## How to start M4

When ready to begin M4 (Termux runtime: zsh + GNU coreutils + APT):

```
/oh-my-claudecode:ralph M4 milestone — Termux runtime integration per ralplan §6 M4. Bundle termux-packages (zsh + GNU coreutils + APT package manager) as APK assets; first-launch extraction; PTY spawn uses Termux shell instead of /system/bin/sh. Closes M3 carry-overs (M3-S06 hook execution, M3-S08 toybox color, M3-S08 Linux pixel similarity, M3-S11 Option D shared-rlib API split). Verifier gate: codex per existing SOP.
```

Or with autopilot (auto-detects ralplan plan, skips Phase 0+1):

```
/oh-my-claudecode:autopilot
```

**M4 entry criteria** (per M3-go-no-go.md §5 + §6):
- M3 CLOSED CONDITIONAL GO @ commit `8ec75c8` (12/12 stories PASS)
- Release APK 7.4MB (M3-S10 baseline; ~73MB headroom for Termux bundle under 80MB gate, or ~113MB under 120MB combined)
- Cherry-pick budget intact (Pre-mortem C #4 NOT TRIPPED at M3-S11; ~3-5 min/commit estimated)
- 6 cross-workspace dups (~5800 LOC) documented as Option C divergence; Option D shared-rlib API split scheduled for M4 refactor

**M4 deliverables (per ralplan §6 M4)**:
1. Termux bootstrap zip bundling (zsh + coreutils-gnu + bash + grep + find + sed + awk + ...)
2. APK asset packaging similar to M3-S06 zsh_body.sh pattern
3. First-launch extraction to `/data/data/dev.warp.mobile/files/termux/`
4. PTY spawn uses `$PREFIX/bin/zsh` instead of `/system/bin/sh`
5. F-Droid metadata for v1 release prep
6. Bootstrap zip reproducibility (deterministic build)
7. Option D shared-rlib API split (resolves the 6 cross-workspace dups from M3-S11)
8. Re-run M3-S05 colored ls -la /system AC against real GNU coreutils ls --color=auto (closes M3-S08 AC#5 deferral)
9. Live emoji raster smoke (closes M3-S11 carry-forward)

---

*Last updated: 2026-05-01 by team-lead@warp-mobile-m3 (Claude Opus 4.7, 1M context) — M2+M3 close-out + M4 ready*
