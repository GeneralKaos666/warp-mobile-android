# Lead Context Snapshot — Warp Mobile Android Port

> **For AI instance resuming this project (post-compact or new session)**: read this file FIRST. It captures everything you need to pick up where the previous lead left off, without re-deriving from full conversation history.
>
> **Last updated**: 2026-04-29 by team-lead@warp-mobile-m0 (Claude Opus 4.7, 1M context)

---

## 1. Identity

- **Project**: Warp Terminal Android port + bundled Termux runtime
- **GitHub**: https://github.com/ImL1s/warp-mobile-android (PRIVATE during M0–M2; turn public before v1 alpha per AGPL §6 source-disclosure)
- **License**: AGPL-3.0-only (inherited from `warpdotdev/Warp@d0f045c`)
- **Repo root**: `/Users/iml1s/Documents/mine/warp_termux/`
- **Account**: `ImL1s` (gh logged in)
- **Email**: aa22396584@gmail.com
- **User explicit identity**: ImL1s

---

## 2. User Preferences (CRITICAL — do not violate)

These were established explicitly by the user and apply for the rest of the project:

- **「全自動」** — full-auto governance. Do NOT ping user for: task assignments, Tension 3 / cloud AI decisions, device runs (workers have adb access), spike redos, plan amendments, Codex review responses, repo metadata, commit messages, branch names.
- **「你自己決定」** — lead has authority to commit, push, gh repo create, plan amendments, worker dispatch, Codex re-reviews — without re-asking.
- **Only ping user for**: irreversible operations user must approve (going public, formal v1 release, switching to Companion retreat) OR truly unrecoverable blockers.
- **Language**: 繁體中文 user-facing prose; English for code/identifiers/file paths.
- **Tone**: short, direct, no excessive confirmation gates.

The user already signed off Tension 3 by delegation (see §6 below).

---

## 3. Plan State

### 3.1 RALPLAN consensus
- **Plan file**: `.omc/plans/ralplan-warp-on-mobile.md`
- **Status**: APPROVED (Planner+Architect+Critic 2-iter loop, 13/13 PASS) + 2 amendments
- **Open questions**: `.omc/plans/open-questions.md`

### 3.2 Amendment 1 (D1 → D2-lite)
- Triggered by M0 evidence: cfg-gate 3,334-line scope-proxy > 500 threshold = 6.7×
- Decided D2-lite (facade excludes warpui)
- Now SUPERSEDED by Amendment 2

### 3.3 Amendment 2 (D2-lite → D1.5-hybrid) — current
- Triggered by Codex `CODEX_REVISE_PLAN` review
- D2-lite contradicted Cargo graph (warp_terminal/Cargo.toml:36 deps warpui)
- **Adopted: D1.5-hybrid**
  - Keep `warp_terminal -> warpui` Cargo edge intact
  - Modify `warpui` internally with `cfg(target_os = "android")` gates so it does NOT pull `font-kit` or desktop `winit` on Android
  - Add `crates/warpui/src/platform/android/` derived from `headless`
- Other Amendment 2 corrections: 3,334 wording change ("scope proxy"); M2a 4w → 5-7w + M2a-font sub-gate; M2a acceptance hardened (real swapchain, validation layers, VK_ERROR_OUT_OF_DATE_KHR); device matrix unification (S24 Ultra/S21+/S8); per-package SPDX manifest mandate; solo-dev rhythm budget +1-2w

---

## 4. 5-Layer Architecture (current)

```
L0 Android Host Service     — Activity / Service lifecycle, FGS, JNI shim, IME, clipboard
L1 WarpUI Android backend   — warpui::platform::android (A4 derived from headless), Vulkan+ash+ANativeWindow
                              + 4 hand-written areas: render_scene, request_frame_capture, FontDB (15 methods, cosmic-text wrap), TextLayoutSystem (2 methods)
L2a Terminal Session Engine — crates/warp_terminal + clean deps (warpui/warp_completer/warp_core/warp_util/vte/sum_tree)
L2b Warp Product Logic      — app/src/terminal/... subset + facade crate (D1.5-hybrid: warpui patched, no full facade unless cfg-gate budget exceeded post-M3 archeology)
L3 Termux Runtime+Packages  — fork termux-packages with new $PREFIX, bootstrap zip in APK
```

**Decision A**: A4 (`warpui::platform::headless` derive) — confirmed by Codex archeology (89 trait methods; 85 stubbed in headless; 4 require real impl).
**Decision D**: D1.5-hybrid (modify warpui internally; keep warp_terminal Cargo edge).
**Decision C**: C1 (Anthropic-only cloud, Haiku inline + Sonnet agent).

---

## 5. Connected Devices (adb)

| Device | Serial | Model | Android | SDK | GPU |
|---|---|---|---|---|---|
| S24 Ultra | `R5CX10VFFBA` | SM-S9280 | 16 | 36 | Adreno 750 (Snapdragon 8 Gen 3) |
| S21+ | `RFCNC0WNT9H` | SM-G9960 | 15 | 35 | Adreno 660 (Snapdragon 888) |
| S8 | `ce0317133a9ad0190c` | SM-G950F | 9 | 28 | Mali-G71 |

Workers have adb access. Use these serials for any device test invocation.

---

## 6. Tension 3 Sign-off (lead-resolved)

User delegated via 「全自動」. Document: `.omc/m0-artifacts/M0-tension3-decision.md`

- **A1**: v1 ships cloud AI as core feature
- **B1**: accept F-Droid NonFreeNet anti-feature label
- **C1**: Anthropic only (Haiku + Sonnet)
- **E1**: Companion retreat trigger fires if M0 Vulkan spike fails on 2 of 3 devices (= **DID NOT fire** — see §8)

---

## 7. Environment State

- **Rust**: 1.88.0
- **cargo-ndk**: 4.1.2 (installed)
- **Android NDK**: r29 at `~/Library/Android/sdk/ndk/29.0.13113456` (also r27/r28 present)
- **Android SDK**: `~/Library/Android/sdk/`
- **Java**: OpenJDK 17 (Corretto)
- **Rust target installed**: `aarch64-linux-android`
- **`.envrc`** (committed): exports `ANDROID_NDK_ROOT` + `ANDROID_HOME`. Source via `direnv allow` or `source .envrc` before any cargo-ndk build.
- **`.cargo/config.toml`** (gitignored, contains absolute paths): generated by `tools/scripts/setup-cargo-config.sh`. Run if it doesn't exist.
- **`.cargo/config.toml.template`** (committed): the template that the setup script renders.

---

## 8. M0 Status (REAL, as of 2026-04-29 ~13:00 UTC)

### M0 task list (in team `warp-mobile-m0`, ~/.claude/teams/warp-mobile-m0/)
1. ✅ NDK env smoke (worker-env)
2. ✅ symlink-jniLibs USER (resolved by Task #12 → #14 redo; Task #14 in_progress)
3. ✅ cargo check + deps report (worker-env; cfg-gate 3,334 line scope proxy)
4. ✅ trait surface + gpui-mobile diff (worker-archeo) — gpui-mobile rejected (89 methods × 50% missing)
5. ✅ Vulkan spike code (worker-spike) — superseded by #15 redo
6. ✅ facade scaffold (worker-spike) — placeholder; D1.5 means it's optional escape hatch only, not core
7. ✅ A1-vs-A4 archeology (worker-archeo) — A4 selected (3-4 person-weeks vs A1 6-8w)
8. ✅ 3-device 100-cycle Vulkan run — RESOLVED inside #15 (worker-spike); steady-state 7-52ms p95 << 200ms gate
9. ✅ Tension 3 user gate — lead-resolved (§6 above)
10. 🟡 M0 go/no-go integration — **PENDING** all other tasks closed
11. ✅ Vulkan APK + script (worker-spike) — superseded by #15
12. ✅ symlink test harness (worker-env) — superseded by #14
13. ✅ Surface handle fix (Item A of #15) — committed `4aa1fac`
14. 🟢 **IN PROGRESS** — symlink redo per Codex 5 items: negative control / targetSdk 36 / Os.execv errno / manifest cleanup / release variant. worker-env. Branch: `warp-mobile/m0-symlink-redo`
15. ✅ Vulkan B-F: real swapchain / validation layers / configChanges / shell array fix / 3-device verify
16. 🟢 **IN PROGRESS** — Codex round-2 follow-on (3 small fixes): shell-assert FAIL on n!=expected, src/lib.rs init-failure cleanup paths (Option A drop-guard), AndroidManifest.xml configChanges scope comment. worker-spike.

### M0 L1 GO/NO-GO verdict: **GO**
- Vulkan-Surface-recreate verified on 3 devices steady-state
- All p95 << 200ms (S24 Ultra 7-9ms / S21+ 15-21ms / S8 36-52ms)
- Zero validation errors
- E1 Companion retreat trigger NOT activated

### M0 L4 GO/NO-GO verdict: **PROVISIONAL GO** (Task #14 redo will solidify)
- Three-device symlink-jniLibs PASS (Task #12)
- Codex flagged: missing negative control + targetSdk 36 retest + Os.execv errno + release variant — Task #14 addresses these

---

## 9. Codex Review State

ALL 4 Codex reviews dispatched by lead in this session:

1. **Plan review** (`bzc1p7lrl` then `bqwah8ask`): `CODEX_REVISE_PLAN` — D2-lite Cargo contradiction + 5 other items. → Amendment 2 landed (commit `ba418ab`).
2. **Task #11 review** (`bx2he252y`): `CODEX_REVISE` 5 items. → split into Task #13 Item A (done) + Task #15 Items B-F (done).
3. **Task #12 review** (`bac72c1hl`): `CODEX_REVISE` 5 items. → Task #14 in progress.
4. **Task #15 re-review** (`bq65koa7m`): `CODEX_REVISE` 4 items, 1 resolved by #15 evidence, 3 → Task #16.

**Pending**: Task #14 + #16 completion, then 2 more Codex re-reviews to confirm CODEX_PASS.

**Commit-and-review SOP** (per user instruction "每個 worker 做完事情都給 codex review"):
1. Worker SendMessage completion
2. Lead reads artifact
3. Lead commits + pushes
4. Lead dispatches Codex review of deliverable (background, prompt-via-file to avoid zsh parse error from `()` in prompts — see `/tmp/codex-review-task*.md` pattern)
5. On REVISE: lead dispatches follow-on task to worker; on PASS: mark task closed.

---

## 10. Git State

- **Branch**: `main` (the project's primary branch)
- **Recent commits** (newest → oldest):
  - `e3ac5b5` — M0 L1 GO: 3-device steady-state recreate verified
  - `645d905` — Vulkan B-F (swapchain/validation/lifecycle/shell) + symlink B WIP
  - `ba418ab` — Plan Amendment 2 D2-lite → D1.5-hybrid
  - `4aa1fac` — Surface handle fix (worker-spike, Task #13 Item A)
  - `847eaae` — NOTICE.md backfill repo URL
  - `8041a8f` — initial commit
- **warp-src/** is gitignored (separate Warp upstream fork; M2 will convert to git submodule). It has its own `warp-mobile/m0-facade` branch with commit `5400c66` (facade scaffold per Amendment 1; will be re-architected per Amendment 2 D1.5-hybrid).
- **`.cargo/config.toml`** gitignored (machine-specific path). Template at `.cargo/config.toml.template`.
- **`.omc/state/`**, `.omc/artifacts/`, `.omc/notepad.md`, `.omx/`, `.omc/project-memory.json` all gitignored.

---

## 11. Active Work + Next Action

### Currently running (background, no user blocking)
- worker-env: Task #14 (symlink redo)
- worker-spike: Task #16 (Codex round-2 fixes)

### When workers SendMessage completion
1. Read artifact files
2. `git add -A && git commit && git push origin main`
3. Dispatch Codex review of deliverable (use `/tmp/codex-review-taskN.md` pattern + `omc ask codex --prompt "$(< /tmp/file.md)"` to avoid shell parsing)
4. On Codex PASS for both #14 and #16: write Task #10 (M0 go/no-go integration), mark M0 closed, write `M0-go-no-go.md`
5. Then transition to M1 planning (Android PTY/service prototype, no UI; 6-8 weeks; uses `app/src/terminal/local_tty/{shell.rs, event_loop.rs, mio_channel.rs}` per Plan Section 6 M1)

### Workers idle / re-spawn protocol
- If worker-spike or worker-env idle out (system shows no `Running:`), re-SendMessage the task instructions to wake them. They share team config at `~/.claude/teams/warp-mobile-m0/config.json`.
- New instances may not see prior inbox; re-send instructions verbatim if they say "awaiting next assignment".

---

## 12. Key File Paths Reference

```
PLANS:
.omc/plans/ralplan-warp-on-mobile.md     # consensus plan + 2 amendments
.omc/plans/open-questions.md              # M0 questions (mostly resolved)

HANDOFFS:
.omc/handoffs/team-plan.md                # ralplan → team handoff
.omc/handoffs/lead-context-snapshot.md    # THIS FILE

M0 ARTIFACTS:
.omc/m0-artifacts/M0-env-report.md        # NDK env smoke
.omc/m0-artifacts/M0-deps-report.md       # cargo check + cfg-gate scope-proxy
.omc/m0-artifacts/M0-platform-trait-delta.md  # 89-method gpui-mobile diff + A4 archeology
.omc/m0-artifacts/M0-facade-scaffold.md   # placeholder (D1.5 reframes its role)
.omc/m0-artifacts/M0-task11-install-verify.md # APK install on 3 devices
.omc/m0-artifacts/M0-task13-vulkan-fix-verify.md # Surface handle fix verify
.omc/m0-artifacts/M0-task15-swapchain-verify.md # Real swapchain on 3 devices
.omc/m0-artifacts/M0-symlink-jnilibs.md   # 3-device symlink test (Task #12; pending #14 redo update)
.omc/m0-artifacts/M0-lead-summary-partial.md # mid-M0 lead synthesis (older; superseded by this file for resume purposes)
.omc/m0-artifacts/M0-tension3-decision.md # Lead-resolved Tension 3 sign-off

SPIKES:
spikes/vulkan-surface-recreate/           # Task 5/11/13/15/16 spike crate
spikes/symlink-jnilibs/                   # Task 12/14 spike crate
scripts/run-symlink-test.sh               # symlink test driver
spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh  # Vulkan test driver

TOOLS:
tools/scripts/setup-cargo-config.sh       # render .cargo/config.toml from template

CODEX REVIEW PROMPTS (re-runnable):
/tmp/codex-review-plan.md
/tmp/codex-review-task11.md
/tmp/codex-review-task12.md
/tmp/codex-review-task15.md

CODEX ARTIFACTS:
.omc/artifacts/ask/codex-*.md             # all timestamped Codex review outputs (gitignored)
```

---

## 13. Skills + Tools Used So Far (don't re-discover)

- `oh-my-claudecode:ralplan` (deliberate mode) — already executed; plan APPROVED iter 2
- `oh-my-claudecode:team` — current orchestration mode; team `warp-mobile-m0` active with workers worker-env / worker-spike / worker-archeo
- `oh-my-claudecode:ccg` — used for initial multi-AI research (Codex+Gemini+Claude); already produced
- `omc ask codex --prompt "$(< /tmp/file.md)"` — Codex review pattern (file substitution avoids zsh `()` parse error)
- DO NOT re-invoke `octo:research`, `octo:discover`, `ralplan`, `autopilot`, etc. — they were appropriate for project bootstrap; current state is execution.

---

## 14. Resume Checklist (run on session restart)

1. Read this file fully.
2. `cd /Users/iml1s/Documents/mine/warp_termux && git status && git log --oneline -5`
3. `cat .omc/m0-artifacts/M0-tension3-decision.md` — confirm Tension 3 lead-resolved.
4. Check active workers: `ps aux | grep gradle | grep -v grep` and `ls -t .omc/m0-artifacts/M0-*.md | head -3` to see latest worker output.
5. Check Codex review queue: `ls -t .omc/artifacts/ask/codex-* 2>/dev/null | head -3`.
6. Resume per §11 Active Work + Next Action.

---

## 15. Honest Caveats / Known Issues

- **`spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/arm64-v8a/libvulkan_surface_recreate.so`** is a SYMLINK to `target/aarch64-linux-android/release/libvulkan_surface_recreate.so` (which is gitignored). Fresh clone needs `cargo ndk -t arm64-v8a build --release` before APK assembly works.
- **warp-src/** has its own git history; user-facing source modifications happen there but are NOT tracked in the main repo (will become submodule M2). Branch `warp-mobile/m0-facade` has commit `5400c66`.
- **Plan Amendment 2 means the `warp_terminal_mobile_facade` scaffold (commit `5400c66`) is a placeholder, not the path forward.** Future M2/M3 work modifies `warpui` internally per D1.5-hybrid, not facade-only.
- **Codex review feedback is NOT optional.** User said "每個 worker 做完事情都給 codex review". Always dispatch Codex review after worker SendMessage completion.
- **Conversation token state**: as of this snapshot, conversation is long; user warned 5h API limit + 1h prompt cache means a fresh instance reading from scratch is expensive. This file is the cheap recovery path.

---

End of snapshot.
