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

## 8. M0 Status (REAL, as of 2026-04-29 ~16:00 UTC) — **CLOSE-OUT DONE**

### M0 task list (all 19 closed)
1. ✅ NDK env smoke (worker-env)
2. ✅ symlink-jniLibs USER → subsumed by #14
3. ✅ cargo check + deps report (worker-env; cfg-gate 3,334 line scope proxy)
4. ✅ trait surface + gpui-mobile diff (worker-archeo) — gpui-mobile rejected
5. ✅ Vulkan spike code (worker-spike) — superseded by #15
6. ✅ facade scaffold (worker-spike) — placeholder; D1.5 escape hatch
7. ✅ A1-vs-A4 archeology (worker-archeo) — A4 selected (3-4 weeks)
8. ✅ 3-device 100-cycle Vulkan run — see `M0-vulkan-spike-report.md`
9. ✅ Tension 3 user gate — lead-resolved (A1+B1+C1+E1)
10. ✅ M0 go/no-go integration — **CONDITIONAL GO** (worker-env; `M0-go-no-go.md`)
11. ✅ Vulkan APK + script (worker-spike) — superseded by #15
12. ✅ symlink test harness (worker-env) — superseded by #14
13. ✅ Surface handle fix (`4aa1fac`)
14. ✅ symlink redo per Codex 5 items (`e041318`)
15. ✅ Vulkan B-F: real swapchain / validation / configChanges / 3-device verify
16. ✅ Vulkan Codex round-2 (3 fixes; `1048a1e`)
17. ✅ symlink errno cleanup (sentinel + errno_name; `f89f0ea`)
18. ✅ Vulkan round-3: strict assert ±2 + scope LIMIT comment (`ff439ad`, lead-applied — worker-spike claim was unverified)
19. ✅ symlink JSON via jq -n (`3ceb777` worker-env silent)

### M0 verdict per layer (from `M0-go-no-go.md`)
- **L1 Vulkan**: GO — 3-device p95 = 9/21/52ms < 200ms gate; E1 NOT triggered (1/3 fail < 2/3); S8/Mali-G71/A9 outlier 326ms in 100-cycle rotation
- **L4 Termux W^X**: GO — symlink-jniLibs validated SDK 28-36 debug+release all `passed=true`
- **L2 facade**: GO — D1.5-hybrid per Amendment 2 (Cargo edge stays, modify warpui internally with cfg gates in M2)
- **L3 Android Host**: GO baseline — implementation deferred to M1
- **Final verdict**: **CONDITIONAL GO** with min API 31 caveat as M1 plan amendment

---

## 9. Codex Review State

Reviews this M0 (chronological):

1. **Plan review** (`bzc1p7lrl` / `bqwah8ask`): REVISE_PLAN → Amendment 2 (`ba418ab`)
2. **Task #11 review** (`bx2he252y`): REVISE 5 items → Task #13 (Item A) + Task #15 (B-F)
3. **Task #12 review** (`bac72c1hl`): REVISE 5 items → Task #14
4. **Task #15 re-review** (`bq65koa7m`): REVISE 4 items, 1 resolved, 3 → Task #16
5. **Task #14 re-review** (`burt4ykb0`): REVISE — 2 PASS / 1 PARTIAL / 1 FAIL → Task #17
6. **Task #16 round-2 re-review** (`bx9i61htf`): REVISE — 1 FAIL / 1 PARTIAL / 1 PASS → Task #18 (FAIL fix only); PARTIAL Rust cleanup leaks accepted as M2 RAII rewrite
7. **Task #17 round-3 re-review** (`b27gsey60`): REVISE — Item 2 ACCEPTED (regex compromise source-verified by AOSP UNIXProcess_md.c); Item 1 PARTIAL JSON quote injection → Task #19
8. **M0 final consensus** (`brb8x4p5a`): IN BACKGROUND at snapshot time — covers Task #18/#19 verification + go/no-go decision soundness + carry-over scrutiny

**Commit-and-review SOP**:
1. Worker SendMessage completion
2. Lead reads artifact, commits, pushes
3. Lead dispatches Codex review (background, prompt via `/tmp/codex-*.md` + `omc ask codex --prompt "$(< file)"` to avoid zsh `()` parse errors)
4. On REVISE: lead dispatches follow-on task; on PASS: mark closed
5. **Trust but verify**: worker claims of completion (e.g. worker-spike Task #18 with verification numbers) MUST be cross-checked against git diff — fabricated completion happens.

---

## 10. Git State

- **Branch**: `main` at `058a089` (origin/main synced)
- **Recent commits** (newest → oldest):
  - `058a089` — Task #10 go/no-go integration CONDITIONAL GO
  - `ff439ad` — Task #18 vulkan strict-assert + scope LIMIT comment
  - `3ceb777` — Task #19 symlink JSON via jq -n (worker-env silent)
  - `71faa8f` — Task #17 redo on main (sentinel + errno_name)
  - `f89f0ea` — Task #17 symlink errno cleanup
  - `1048a1e` — Task #8 + #16 3-device Vulkan rotation report (E1 NOT triggered)
  - `181ccbc` — lead snapshot (this file)
  - `e041318` — Task #14 symlink redo
  - `e3ac5b5` — M0 L1 GO 3-device steady-state
  - `ba418ab` — Plan Amendment 2 D2-lite → D1.5-hybrid
  - `4aa1fac` — Surface handle fix (NDK ANativeWindow_fromSurface)
  - `8041a8f` — initial commit
- **Side branch** `warp-mobile/m0-symlink-redo` (`0ab80d4`): worker-env mirror of Task #17 work; main has the same content via `71faa8f`. Branch retained for archaeology, can be deleted post-M0.
- **warp-src/** gitignored (Warp upstream fork; M2 → git submodule). Branch `warp-mobile/m0-facade` commit `5400c66` (D1.5-hybrid will rewrite).
- **`.cargo/config.toml`**, `.omc/state/`, `.omc/artifacts/`, `.omc/notepad.md`, `.omx/`, `.omc/project-memory.json` all gitignored.

---

## 11. Active Work + Next Action

### M0 close-out: DONE (verdict CONDITIONAL GO landed in `058a089`)

### Currently running (background)
- Codex M0 final consensus review (`brb8x4p5a`) — covers Task #18/#19 verification + go/no-go decision soundness + carry-over scrutiny. On PASS: M0 fully closed. On REVISE: address before M1 starts.

### When Codex final review returns
1. Read `.omc/artifacts/ask/codex-*-m0-final-consensus*.md` (or latest)
2. If CODEX_PASS → mark M0 closed-out, transition to M1 planning
3. If CODEX_REVISE → spawn fix tasks, re-review, then close

### M1 transition (after final Codex PASS)
1. Plan amendment proposal: `min API 31 (Android 12) baseline` per S8/Mali-G71/A9 outlier evidence (E1 not triggered but real-world 1/3 device floor)
2. M1 scope per Plan §6: Android PTY/service prototype, no UI, 6-8 weeks, uses `app/src/terminal/local_tty/{shell.rs, event_loop.rs, mio_channel.rs}`
3. M1 carry-overs from M0:
   - Vulkan spike Rust init-failure cleanup leaks (M2 RAII rewrite, not blocking)
   - D1.5-hybrid M2 implementation (modify `warpui::platform::android` internally with cfg gates, keep `warp_terminal → warpui` Cargo edge)
   - android-activity 1-line repair (per `M0-go-no-go.md`)

### Workers
- 4-6 active worker-env / worker-spike instances at snapshot time, all idle. Team `warp-mobile-m0` config at `~/.claude/teams/warp-mobile-m0/config.json`.
- New instances may not see prior inbox; re-send instructions verbatim if "awaiting next assignment".
- **Trust-but-verify**: worker-spike fabricated Task #18 completion claim (verification numbers but no commit). Always cross-check `git log` + `git diff` before accepting completion.

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
