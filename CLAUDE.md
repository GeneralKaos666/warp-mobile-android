# Warp on Mobile — AI Agent Entry Point

> **You are an AI assistant (Claude Code, Cursor, GitHub Copilot, etc.) opening this project.** This file is the canonical entry point. Read it first.

## What is this project

Open-source-first port of [Warp Terminal](https://github.com/warpdotdev/Warp) to Android, with a bundled Termux runtime. AGPL-3.0-only. Solo-dev 12-18 month timeline. F-Droid + GitHub Releases primary distribution.

See [`README.md`](README.md) for product description and architecture overview.

## Current milestone state (2026-04-30)

- **M0** (foundation spike): CLOSED CONDITIONAL GO @ commit `24a2c1c`
- **M1** (Android PTY/Service prototype): CLOSED CONDITIONAL GO @ commit `f7feb3f`, **10/10 stories PASS**
- **M2** (warpui::platform::android backend, 8-12 weeks): READY TO START

To verify currency: `git log --oneline -5` and check `.omc/prd.json` `passes` fields.

## How to resume / pick up work

**Read in this order**:

1. **This file** (`CLAUDE.md`) — you are here
2. [`.omc/handoffs/lead-context-snapshot.md`](.omc/handoffs/lead-context-snapshot.md) — **authoritative session resume document**
   - §1-7: identity, user preferences, conventions
   - §8: M0 status (CLOSED)
   - §8b: M1 status (CLOSED 10/10 stories PASS — table with file:line evidence)
   - §8c: M2 ready-to-start state + dispatch instructions
3. [`.omc/m2-kickoff.md`](.omc/m2-kickoff.md) — if M2 not yet dispatched, this is the forward-looking kickoff doc
4. [`.omc/plans/ralplan-warp-on-mobile.md`](.omc/plans/ralplan-warp-on-mobile.md) — canonical plan with 4 amendments at top
5. [`.omc/prd.json`](.omc/prd.json) — current milestone story states
6. [`progress.txt`](progress.txt) — iteration log with lessons learned

**Do NOT** attempt to derive state from full conversation history. The handoff doc is designed to be read cold.

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
# Verify all 10 M1 stories PASS
jq -r '.stories | map(select(.passes == true)) | length' .omc/prd.json   # → 10
jq -r '.stories | length' .omc/prd.json                                   # → 10

# Confirm latest M1 close-out commits
git log --oneline -10

# Build sanity check
cargo test -p warp-mobile-android-host                                    # → 3 passed

# Connected devices (your serials will differ)
adb devices                                                                # → at least one API 31+ Adreno 6xx+ device for M2 work
```

## What you should NOT do

- Don't skip reading `.omc/handoffs/lead-context-snapshot.md` and start fresh — you'll re-derive months of decisions and risk getting them wrong.
- Don't switch to a Compose UI — `warpui::platform::android` derived from headless is the chosen path (Plan §6 M2; Decisions A4 + D1.5-hybrid).
- Don't fork `termux-app` — we bundle `termux-packages` only.
- Don't ping the user for routine decisions — see "User governance preferences" above.
- Don't commit `.cargo/config.toml` (machine-absolute paths). The template at `.cargo/config.toml.template` is the source of truth; run `tools/scripts/setup-cargo-config.sh` to render.
- Don't commit `warp-src/` (it's a separate git repo, gitignored).

## How to start M2

When ready to begin M2:

```
/oh-my-claudecode:ralph M2 milestone — warpui::platform::android backend per ralplan §6 M2 (D1.5-hybrid). Build Android Vulkan rendering on top of M1 PTY/Service infrastructure. 4 hand-written platform areas + headless-derived base. Verifier gate: codex per existing SOP.
```

Or with autopilot (auto-detects ralplan plan, skips Phase 0+1):

```
/oh-my-claudecode:autopilot
```

The PRD scaffold for M2 stories will be auto-generated by the ralph/autopilot skill and refined per the just-in-time milestone planning workflow described in [`.omc/m2-kickoff.md`](.omc/m2-kickoff.md).

---

*Last updated: 2026-04-30 by team-lead@warp-mobile-m1 (Claude Opus 4.7, 1M context) — M0+M1 close-out + M2 ready*
