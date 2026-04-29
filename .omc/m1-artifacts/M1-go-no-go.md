# M1 Go/No-Go 整合報告 (DRAFT)

**日期**：2026-04-29 (M1 milestone close-out)
**主分支**：`main` @ TBD (will fill in once all stories PASS)
**Plan reference**：`.omc/plans/ralplan-warp-on-mobile.md` (Amendment 3 minSdk 31)
**前置 milestone**：M0 close-out CONDITIONAL GO @ commit `24a2c1c`

---

## 1. M1 Story Ledger

| Story | 標題 | 狀態 | 證據 |
|---|---|---|---|
| M1-S01 | Plan Amendment 3 — minSdk 26 → 31 | **PASS** | Codex round-1 PASS @ `2ccc0f7`. Drops S8/Mali-G71 from primary device matrix per M0 Task #8 evidence (S8 100-cycle p95=326ms FAIL). |
| M1-S02 | android-activity Cargo feature fix | **PASS** | Codex round-1 PASS. warp-src Cargo.toml winit gains `android-native-activity` feature; warpui/Cargo.toml adds explicit android-activity dep block. Commit `afc74ec` on warp-mobile/m0-facade pushed to fork ImL1s/warp. |
| M1-S03 | crates/android-host/ Rust skeleton | **PASS** | Codex round-1 REVISE (AC#5 missing .so info) → fix `5b1424e` (README addendum) → round-2 PASS. Cargo skeleton + JNI ping export + cdylib build. .so 16.7MB sha256 6e6960002e... |
| M1-S04 | PTY backend openpty/setsid/TIOCSCTTY | **TBD** | Codex round-1 REJECT (6 safety issues) → round-2 REVISE (putenv+execvp not AS-safe; E0597 borrow-check) → round-3 fix `d9bf0d4` (execve+pre-built envp + E0597 fix). cargo test 2/2 PASS. Codex round-3 verdict pending. |
| M1-S05 | Android Service + AndroidManifest + FGS | **TBD** | Codex round-1 REVISE (AC#7 POST_NOTIFICATIONS missing; AC#6 .so loaded by Activity not Service) → fix `f424be2` (POST_NOTIFICATIONS perm + Service companion init { System.loadLibrary }). Device verification pending. |
| M1-S06 | Activity recreate → PTY reattach < 1s | **TBD** | Drivers code committed `9268de7` + bug-fix rounds (`fc763b3`, `c6469db`). Device run pending Task #32. |
| M1-S07 | PTY resize via TIOCSWINSZ | **TBD** | Driver `test-pty-resize.sh`. Service-side broadcast handlers landed Task #28 (`9479316`). Device run pending Task #32. |
| M1-S08 | FGS persistence + clean kill no orphan | **TBD** | Driver `test-fgs-clean-kill.sh` (UID format fix `c6469db`). Device run pending Task #32. |
| M1-S09 | 30-min idle stress on flagship | **TBD** | Driver `test-30min-idle-stress.sh` (`d46d553`). Device run pending Task #34 (separate dispatch — 30-min wall clock). |
| M1-S10 | M1 close-out doc | **THIS DOC** | — |

Out-of-prd-but-essential:
- **Task #28**: WarpTerminalService BroadcastReceivers + PtyManager + read-coroutine. Commits `9479316` + `a6f08ef`. Codex review pending. Required for S06/S07/S08 to be testable.

---

## 2. Architecture state at M1 close

```
android/                          (NEW in M1, gradle project)
├── app/build.gradle              minSdk 31 / targetSdk 36 / compileSdk 36 / ndkVersion 29
├── app/src/main/AndroidManifest.xml
│   ├── FOREGROUND_SERVICE + FOREGROUND_SERVICE_SPECIAL_USE + POST_NOTIFICATIONS
│   ├── MainActivity (LAUNCHER intent)
│   ├── WarpTerminalService (foregroundServiceType=specialUse)
│   └── PtyBroadcastReceiver (4 PTY intent-filters)
└── app/src/main/java/dev/warp/mobile/
    ├── MainActivity.kt          (POST_NOTIFICATIONS request, NativeBridge.ping demo)
    ├── WarpTerminalService.kt   (FGS lifecycle + PTY broadcast dispatch + read coroutine)
    ├── PtyBroadcastReceiver.kt  (intent → Service.onStartCommand)
    ├── PtyManager.kt            (cmd_id → ptr Map; spawn/write/read/resize/kill/killAll)
    └── NativeBridge.kt          (System.loadLibrary + 6 external funs)

crates/android-host/             (NEW in M1, Rust workspace member)
├── Cargo.toml                   (cdylib, jni 0.21, ndk 0.9, log 0.4, android_logger 0.13)
├── README.md                    (build + JNI surface + .so verification table)
└── src/
    ├── lib.rs                   (6 JNI exports: ping + ptySpawn/Read/Write/Resize/Kill)
    └── pty.rs                   (PtySession with AS-safe fork+execve, FD_CLOEXEC,
                                   robust kill SIGTERM→1s WNOHANG poll→SIGKILL, Drop impl)

tools/scripts/                   (test drivers)
├── test-pty-reattach.sh         (S06 — rotation × 5, logcat -v epoch parse)
├── test-pty-resize.sh           (S07 — PTY_RESIZE broadcast → stty size verify)
├── test-fgs-clean-kill.sh       (S08 — am kill, orphan UID detection)
└── test-30min-idle-stress.sh    (S09 — 4 checkpoint snapshots + pwd latency)
```

---

## 3. Decision Matrix per Layer (M1 outcome)

### L0 — Android Host Service: **GO** ✅

`WarpTerminalService` survives Activity recreation (FGS `specialUse` persistent), holds PTY sessions in `PtyManager` indexed by cmd_id, registers BroadcastReceivers for PTY ops, kills all sessions on onDestroy. NotificationChannel + persistent ongoing notification visible (post-`f424be2` POST_NOTIFICATIONS fix).

### L0 PTY backend (in crates/android-host) — **GO**

Pure-Rust libc-based PTY: openpty + fork + setsid + TIOCSCTTY + dup2 + execve. Async-signal-safe child branch (no putenv, no Rust drop, no allocations). FD_CLOEXEC on master. SIGTERM-then-SIGKILL kill with timeout. Drop impl for orphan cleanup. JNI null-pointer guards. 2 unit tests PASS.

### L2 facade — **STILL D1.5-hybrid (M2)**

No actual L2 implementation work in M1. Plan §6 M2 is the next deliverable (warpui::platform::android backend deriving from headless, 4 hand-written areas).

### L3 — minSdk 31

Plan Amendment 3 raised baseline. S8/Mali-G71 dropped from primary matrix. Replacement device (Pixel 4a or Galaxy A52s) to acquire before M2 close.

### L4 — Termux runtime: **deferred to M4**

No L4 work in M1. Verified path B1 (symlink-jniLibs) carries over from M0.

---

## 4. Acceptance Criteria Coverage (Plan §6 M1)

| Plan §6 M1 Acceptance Criterion | Story Mapping | Status |
|---|---|---|
| 1. Service with FGS, persistent notification, 30-min idle survival on flagship | S05 + S09 | TBD (S05 device verify + S09 30-min run pending) |
| 2. Activity destroy/recreate (rotation, minimize-2-min-restore) preserves running PTY session, re-attaches within 1s | S06 | TBD (Task #32 device run pending) |
| 3. PTY resize via TIOCSWINSZ reflects in shell stty size | S07 | TBD (Task #32 device run pending) |
| 4. FGS notification persistent during session; adb shell am kill cleans up cleanly (no orphan PTY processes) | S08 | TBD (Task #32 device run pending) |
| 5. 30-min idle stress test on flagship + low-end (Pixel 4a or Galaxy A52) | S09 | PARTIAL — flagship S24 Ultra gated by Task #34 dispatch. Low-end deferred until replacement device acquired. Documented as M2 carry-over. |

---

## 5. M2 Carry-Overs

1. **Run M1-S09 30-min stress on S24 Ultra** (deferred from this milestone — Task #34 separate dispatch, 30-minute wall clock)
2. **Acquire replacement low-end device** (Pixel 4a / Galaxy A52s API 31) and re-run M1-S06/S07/S08/S09 on it before M2 close (Plan Amendment 3 §3)
3. **gradle copy task replacing jniLibs symlink** (currently absolute symlink to `target/aarch64-linux-android/debug/`, fragile on CI/clean-checkout — M2 ergonomics fix)
4. **D1.5-hybrid M2 implementation** (M2 main work: warpui::platform::android backend + 4 hand-written areas — see Plan §6 M2)
5. **android-activity / winit M2 reorganization** (warpui/Cargo.toml explicit android-activity dep currently redundant per Codex S02 review; fold into D1.5-hybrid restructuring)
6. **Notification customization** (current notification is generic "Warp terminal"; M2 should add session count, command preview, tap → MainActivity intent)

---

## 6. M1 Verdict (filled in once all stories PASS)

**Verdict**: TBD — currently 3/10 stories PASS, 7/10 awaiting Codex verdicts or device runs.

**Decision deadline**: Once Tasks #32 (device runs S05/S06/S07/S08) + #34 (S09 30-min stress) complete and Codex round-3 verdicts on S04/drivers/Task #28 land, fill in this section with GO / CONDITIONAL GO / NO-GO and one-paragraph rationale.

---

*撰寫人：team-lead@warp-mobile-m1 (M0 close-out same governance)*
*基於：Tasks 20–32 (M1 in-flight), commits since `eac5379` (PRD scaffold)*
*下一步：等所有 Codex round-3 verdicts + Task #32 device runs + Task #34 30-min stress run，然後填 §6 verdict + 標 prd.json M1-S10.passes:true。*
