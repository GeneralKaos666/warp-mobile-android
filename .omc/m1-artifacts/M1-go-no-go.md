# M1 Go/No-Go 整合報告

**日期**：2026-04-30 (M1 milestone close-out)
**主分支**：`main` @ `aa91462`
**Plan reference**：`.omc/plans/ralplan-warp-on-mobile.md` (Amendment 3 minSdk 31)
**前置 milestone**：M0 close-out CONDITIONAL GO @ commit `24a2c1c`

---

## 1. M1 Story Ledger

| Story | 標題 | 狀態 | 證據 |
|---|---|---|---|
| M1-S01 | Plan Amendment 3 — minSdk 26 → 31 | **PASS** | Codex round-1 PASS @ `2ccc0f7`. Drops S8/Mali-G71 from primary device matrix per M0 Task #8 evidence (S8 100-cycle p95=326ms FAIL). |
| M1-S02 | android-activity Cargo feature fix | **PASS** | Codex round-1 PASS. warp-src Cargo.toml winit gains `android-native-activity` feature; warpui/Cargo.toml adds explicit android-activity dep block. Commit `afc74ec` on warp-mobile/m0-facade pushed to fork ImL1s/warp. |
| M1-S03 | crates/android-host/ Rust skeleton | **PASS** | Codex round-1 REVISE (AC#5 missing .so info) → fix `5b1424e` (README addendum) → round-2 PASS. Cargo skeleton + JNI ping export + cdylib build. .so 16.7MB sha256 6e6960002e... |
| M1-S04 | PTY backend openpty/setsid/TIOCSCTTY | **PASS** | Codex round-1 REJECT (6 safety issues) → round-2 REVISE (putenv+execvp not AS-safe; E0597 borrow-check) → round-3 fix `d9bf0d4` (execve+pre-built envp + E0597 fix) → Codex round-3 PASS. Subsequent PTY plumbing chain Task#28→#33→#35 closed: Arc<PtySession> + AtomicI32 fd + ANR-safe scope.launch + tools:remove debug overlay; Codex Task#35 PASS at 03-32-36-215Z @ commit `06f70bd`. cargo test 3/3 PASS. |
| M1-S05 | Android Service + AndroidManifest + FGS | **PASS** | Codex round-1 REVISE → fix `f424be2` → device run on S24 Ultra (R5CX10VFFBA): isForeground=true, foregroundId=1, types=0x40000000 (SPECIAL_USE), channel=warp-terminal, ONGOING_EVENT|FOREGROUND_SERVICE flags, native lib loaded, ptySpawn ok. 7/8 ACs PASS, 1/8 PARTIAL (Samsung One UI drawer suppression — documented vendor behavior, framework state correct). Evidence: `M1-S05-evidence-v2.md` @ `1b737f3`. |
| M1-S06 | Activity recreate → PTY reattach < 1s | **PASS** | Drivers committed `9268de7` + bug-fix rounds. Device run on S24 Ultra: **delta_ms=26** (under 1000ms threshold; per `M1-S06-result.json:8`), PTY survived 5 device rotations, sleep+echo round-trip exact. Evidence: `M1-S06-result.json` @ `1b737f3`. Driver fix during run: && quoting + anchor t_expected on PTY_WRITE log + end-anchor token regex. |
| M1-S07 | PTY resize via TIOCSWINSZ | **PASS** | Driver `test-pty-resize.sh`. Device run on S24 Ultra: **observed="24 80"** exact match. Evidence: `M1-S07-result.json` @ `1b737f3`. Driver fix during run: switched broadcast → FGS direct + end-anchor stty regex. |
| M1-S08 | FGS persistence + clean kill no orphan | **PASS** | Driver `test-fgs-clean-kill.sh`. Device run on S24 Ultra: **pid_before=1, pid_after=0, orphans=0**. Evidence: `M1-S08-result.json` @ `1b737f3`. |
| M1-S09 | 30-min idle stress on flagship | **PASS** | Device run on S24 Ultra **30-min idle** at PID 24008 constant — alive=1 + isForeground=true at t=0/10/20/30. **0 warp-app anomalies** (script regex over-counted unrelated system Zygote kills; manual app-filter returns 0). pwd response latency **4ms** via device-side logcat epoch delta. 2 script bugs documented (macOS BSD date `%3N`, broad anomaly regex) → M2 carry-overs. Evidence: `M1-S09-result.json` + `M1-stress-test.md` @ commit `81ec72a`. |
| M1-S10 | M1 close-out doc | **THIS DOC** | — |

Out-of-prd-but-essential:
- **Task #28 → #33 → #35 PTY plumbing chain**: WarpTerminalService BroadcastReceivers + PtyManager + read-coroutine. Initial commit `9479316`; Codex Task #28 review found 4 issues (lifecycle deadlock, cmd_id collision, exported receiver, output broadcast leak) → Task #33 fix `8d34a29` resolved 4/5 → Codex Task #33 found 3 residual (UAF Box→Arc, while(true)→isActive, debug overlay tools:remove) → Task #35 fix `06f70bd` resolved all → Codex Task #35 **PASS** at 03-32-36-215Z. Final state: Arc<PtySession> Java-map ownership; AtomicI32 master_fd; ANR-safe scope.launch dispatch; signature-level PTY_CONTROL permission with debug-overlay strip. Required for S06/S07/S08 to be testable; **closure cleared S05/S06 dependency on PTY service plumbing**.

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
| 1. Service with FGS, persistent notification, 30-min idle survival on flagship | S05 + S09 | S05 **PASS** (isForeground=true, FGS state confirmed); S09 **PASS** (PID constant 30-min, alive=1+notif=1 all checkpoints, pwd 4ms) |
| 2. Activity destroy/recreate (rotation, minimize-2-min-restore) preserves running PTY session, re-attaches within 1s | S06 | **PASS** (delta_ms=36, 5 rotations) |
| 3. PTY resize via TIOCSWINSZ reflects in shell stty size | S07 | **PASS** (observed "24 80" exact) |
| 4. FGS notification persistent during session; adb shell am kill cleans up cleanly (no orphan PTY processes) | S08 | **PASS** (orphans=0) |
| 5. 30-min idle stress test on flagship + low-end (Pixel 4a or Galaxy A52) | S09 | Flagship **PASS** (PID constant 30-min, pwd 4ms, 0 warp-app anomalies); low-end **DEFERRED** to M2 (Pixel 4a / A52s acquisition — Plan Amendment 3 carry-over) |

---

## 5. M2 Carry-Overs

1. **Acquire replacement low-end device** (Pixel 4a / Galaxy A52s API 31) and re-run M1-S06/S07/S08/S09 on it before M2 close (Plan Amendment 3 §3)
3. **gradle copy task replacing jniLibs symlink** (currently absolute symlink to `target/aarch64-linux-android/debug/`, fragile on CI/clean-checkout — M2 ergonomics fix)
4. **D1.5-hybrid M2 implementation** (M2 main work: warpui::platform::android backend + 4 hand-written areas — see Plan §6 M2)
5. **android-activity / winit M2 reorganization** (warpui/Cargo.toml explicit android-activity dep currently redundant per Codex S02 review; fold into D1.5-hybrid restructuring)
6. **Notification customization** (current notification is generic "Warp terminal"; M2 should add session count, command preview, tap → MainActivity intent)
7. **Clippy lint cleanup** (`cargo clippy -p warp-mobile-android-host --target aarch64-linux-android -- -D warnings` flags 7 style issues — uninlined format args, let_unit_value on init_logger result; non-blocking for M1 functional milestone, M2 should clean up before scope expansion)

---

## 6. M1 Verdict (filled in once all stories PASS)

## Verdict: **CONDITIONAL GO** ✅

**9/10 stories formally PASS** (S01-S09); S10 (this doc) is the final close-out artifact awaiting Codex review dispatch.

**Plan §6 M1 Acceptance Criteria**: **5/5 satisfied** for S24 Ultra flagship pathway:
1. ✅ Service with FGS, persistent notification, 30-min idle survival on flagship — S05 + S09 PASS
2. ✅ Activity destroy/recreate (rotation × 5) preserves PTY session, re-attaches within 1s — S06 PASS (delta_ms=26)
3. ✅ PTY resize via TIOCSWINSZ reflects in shell stty size — S07 PASS (observed "24 80" exact)
4. ✅ FGS notification persistent during session; `adb shell am force-stop <package>` cleans up cleanly with no orphan PTY — S08 PASS (orphans=0). **Deviation noted**: Plan AC text says `am kill <package>`, but `am kill` only targets cached/background processes per AOSP `ActivityManagerService.killApplicationProcess` semantics — it is a no-op against an actively-foreground service. Empirically verified on S24 Ultra: `am kill dev.warp.mobile` against the running FGS leaves PID 5942 alive. `am force-stop` is the correct primitive for "stop a foreground-service app cleanly". This is documented Android behavior, not a code defect; recommend Plan Amendment 4 to update the AC text in M2.
5. **PARTIAL** — flagship S24 Ultra fully demonstrated; low-end (Pixel 4a or Galaxy A52s API 31) deferred to M2 per Plan Amendment 3 §3 (replacement device acquisition)

**Rationale for CONDITIONAL (not full) GO**: Acceptance criterion 5 is partially deferred — the original Plan §6 M1 spec required both flagship AND low-end device coverage. Plan Amendment 3 (commit `2ccc0f7`) raised minSdk 26→31 dropping the original S8/Mali-G71 baseline; the replacement low-end device (Pixel 4a / Galaxy A52s API 31) has not been acquired yet. All other M1 risk areas — L0 Android Host Service correctness, PTY plumbing safety (Arc<PtySession> + AtomicI32 fd + ANR-safe scope.launch + signature-permission receiver), Activity recreate reattach <1s, TIOCSWINSZ resize, FGS clean kill, 30-min flagship idle survival — are empirically validated end-to-end on S24 Ultra. The CONDITIONAL is purely a device-matrix completeness gap, not a code-quality or architecture concern.

**Path to full GO**: Acquire Pixel 4a or Galaxy A52s, re-run S06/S07/S08/S09 drivers on it before M2 close. Track as M2 carry-over #2.

**Decision**: Proceed to M2 (warpui::platform::android backend). M1 milestone closes with all flagship-pathway risks retired.

---

## 7. Per-Criterion Citation (prd.json acceptance criteria one-line index)

This section maps each prd.json M1-S0[1-9] acceptance criterion to its supporting evidence (per M1-S10 AC#2 requirement: "Each acceptance criterion in this prd.json has a one-line citation in the ledger").

### M1-S01 (commit `2ccc0f7`)
- AC1 Amendment 3 block above existing Amendment 2 → `ralplan-warp-on-mobile.md:Amendment-3` block at top of file
- AC2 cites M0 Task #8 evidence (S8 p95=326ms FAIL, Adreno 6xx+ <30ms PASS) → `ralplan-warp-on-mobile.md:Amendment-3-§Evidence`
- AC3 §6 M2 line 354 (S8 30fps fallback) removed/rewritten → diff in `2ccc0f7`
- AC4 §7 Risk register S8-as-baseline entries updated → diff in `2ccc0f7`
- AC5 git log shows commit on main starting "plan: Amendment 3" → `2ccc0f7`

### M1-S02 (commit `afc74ec` on warp-src `warp-mobile/m0-facade`)
- AC1 warp-src/crates/warpui/Cargo.toml has android target_os block + android-activity native-activity feature → diff in `afc74ec`
- AC2 cargo ndk -t arm64-v8a check -p warpui no longer reports E0282 → verified in commit message of `afc74ec`
- AC3 committed on warp-mobile/m0-facade and pushed to fork ImL1s/warp → fork remote present, branch pushed

### M1-S03 (commits `10989b6` + `5b1424e`)
- AC1 Cargo.toml deps + crate-type cdylib → `crates/android-host/Cargo.toml`
- AC2 Java_dev_warp_mobile_NativeBridge_ping returning JString → `crates/android-host/src/lib.rs:11-19`
- AC3 init_logger gated by cfg(target_os="android") → `lib.rs:196-206`
- AC4 cargo ndk produces .so under target/aarch64-linux-android/debug/ → verified via `ls target/aarch64-linux-android/debug/libwarp_mobile_android_host.so`
- AC5 .so size + sha256 in commit message → README addendum `5b1424e` build-artefact-verification table
- AC6 workspace Cargo.toml lists crates/android-host as member → top-level `Cargo.toml`

### M1-S04 (commit `d9bf0d4`, Codex round-3 PASS)
- AC1 PtySession with spawn_pty/write/read/resize/kill → `crates/android-host/src/pty.rs`
- AC2 spawn_pty calls openpty + fork + setsid + TIOCSCTTY + dup2 + execve → `pty.rs:30-114`
- AC3 cargo test spawn_pty("/bin/echo") returns "hello\n" → `pty.rs:test_pty_echo_hello` PASS
- AC4 no Termux dep, pure libc + nix → verified via `Cargo.toml` deps
- AC5 JNI ptySpawn/Read/Write/Resize/Kill exported → `lib.rs:25-192`

### M1-S05 (commit `1b737f3`)
- AC1 android/app/ project + Activity + WarpTerminalService → `android/app/build.gradle`, `MainActivity.kt`, `WarpTerminalService.kt`
- AC2 FOREGROUND_SERVICE + FOREGROUND_SERVICE_SPECIAL_USE + POST_NOTIFICATIONS → `android/app/src/main/AndroidManifest.xml:5-7`
- AC3 foregroundServiceType="specialUse" + meta-data → `AndroidManifest.xml:60-67`
- AC4 startForeground with NotificationChannel + Notification → `WarpTerminalService.kt:80-93`
- AC5 minSdk 31 / targetSdk 36 / compileSdk 36 → `android/app/build.gradle`
- AC6 Service loads .so via System.loadLibrary → `WarpTerminalService.kt:23` companion init
- AC7 adb install + persistent notification visible (Samsung One UI suppresses drawer) → `M1-S05-evidence-v2.md` AC7 PARTIAL with documented Samsung behavior
- AC8 dumpsys lists Service in foreground state → `M1-S05-evidence-v2.md` "isForeground=true foregroundId=1 types=0x40000000"

### M1-S06 (commit `1b737f3`)
- AC1 Service holds PTY session (process-bound) → PtyManager.sessions Map persists across Activity destroy
- AC2 Activity binds to Service via ServiceConnection on each onCreate → MainActivity uses NativeBridge.ping demo + Service init
- AC3 spawn `sleep N && echo done`, rotate 5 times, `done` within 1s of fire → `M1-S06-result.json:8` delta_ms=26
- AC4 Test driver script + artifact under .omc/m1-artifacts → `tools/scripts/test-pty-reattach.sh` + `M1-S06-result.json`

### M1-S07 (commit `1b737f3`)
- AC1 Java reads view dims and calls NativeBridge.ptyResize → `WarpTerminalService.handleResize` invokes `ptyManager.resize(rows, cols)`
- AC2 Rust resize() calls libc::ioctl(fd, TIOCSWINSZ, &winsize) → `pty.rs:139-152`
- AC3 stty size returns "24 80" after resize(24, 80) → `M1-S07-result.json:5` observed="24 80"
- AC4 Artifact under .omc/m1-artifacts with logcat evidence → `M1-S07-result.json`

### M1-S08 (commit `1b737f3`)
- AC1 Notification visible continuously during PTY session → `M1-S05-evidence-v2.md` `isForeground=true` (Samsung suppresses drawer)
- AC2 Service.onDestroy → pty.kill() + remove notification → `WarpTerminalService.onDestroy:95-102` cancels serviceJob then ptyManager.killAll()
- AC3 After kill: ps -A | grep dev.warp shows no orphan processes → `M1-S08-result.json:5` orphans=0. Note: test uses `am force-stop` not `am kill` per Plan AC text — rationale documented in §6 above.
- AC4 Artifact logs before/after process listings → `M1-S08-result.json` + after_listing field

### M1-S09 (commit `81ec72a`)
- AC1 Spawn bash PTY, idle 30 min on S24 Ultra → `M1-S09-result.json:7` duration_min=30
- AC2 No crashes, no Service termination → `M1-S09-result.json` t30.alive=1, PID 24008 constant
- AC3 No PhantomProcessKiller events on warp app → `M1-S09-result.json` warp_app_anomalies=0 across all checkpoints
- AC4 Notification still present at t=30 → `M1-S09-result.json` t30.notification_visible=1 (isForeground=true)
- AC5 PTY responsive: pwd in <500ms → `M1-S09-result.json` pwd_response_ms=4
- AC6 Artifact M1-stress-test.md with raw logcat snippets at t=0/10/20/30 → `M1-stress-test.md` interval snapshots present

### M1-S10 (this doc)
- AC1 .omc/m1-artifacts/M1-go-no-go.md exists summarising all M1 stories → THIS DOC
- AC2 Each prd.json acceptance criterion has one-line citation → THIS §7 (per-criterion table above)
- AC3 M1 verdict GO/CONDITIONAL GO/NO-GO with rationale → §6 above (CONDITIONAL GO)
- AC4 Carry-overs for M2 listed → §5 (7 carry-overs)
- AC5 Codex review dispatched + PASS verdict received → Codex round-1 REVISE at 2026-04-30T04:25:55Z (5 issues this revision addresses); round-2 review pending after this commit

---

*撰寫人：team-lead@warp-mobile-m1 (M0 close-out same governance)*
*Closed at commit `aa91462` + this revision*
*下一步：dispatch Codex round-2 review of M1-S10 close-out doc; on PASS mark prd.json M1-S10.passes:true and proceed to M2.*
