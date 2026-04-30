# M1-S09 30-min Idle Stress Test (FINAL)

**Device**: Galaxy S24 Ultra (R5CX10VFFBA), Android 15, SDK 36
**Run start**: 2026-04-30T03:49:41Z (11:49:41 CST)
**Run end**: 2026-04-30T04:19:56Z (12:19:56 CST)
**Duration**: 30 min idle + spawn/teardown
**Verdict**: **PASS**

## Acceptance Criteria

| Criterion | Threshold | Result | Pass |
|---|---|---|---|
| App process alive at t=30 | alive=1 | alive=1 (PID 24008 throughout) | ✅ |
| FGS state correct at t=30 | isForeground=true | isForeground=true (foregroundId=1, types=SPECIAL_USE) | ✅ |
| No PhantomProcessKiller events on warp app | 0 warp-app anomalies | 0 (system anomalies for OTHER apps don't count) | ✅ |
| pwd response latency | < 500ms | **4ms** (device-side epoch delta) | ✅ |
| 30-min run completes without crash | no crash | run completed, all 4 checkpoints captured | ✅ |

## Interval Snapshots

### t=0 (11:49:48 CST)
```
u0_a1073     24008  1596   18905304 119360 0                   0 S dev.warp.mobile
```
- alive=1, notif=1 (isForeground=true), warp-app anomalies=0

### t=10 (11:59:50 CST)
```
u0_a1073     24008  1596   18636372  92004 0                   0 S dev.warp.mobile
```
- alive=1, notif=1, warp-app anomalies=0
- RSS shrunk 119MB → 92MB (likely PSI memory pressure trim — healthy)

### t=20 (12:09:54 CST)
- alive=1, notif=1, warp-app anomalies=0
- PID 24008 unchanged (Android did NOT recycle the process)

### t=30 (12:19:56 CST)
- alive=1, notif=1, warp-app anomalies=0
- PID 24008 unchanged (full 30-min idle survival)

## pwd Response Latency

Initial test in script crashed on macOS BSD `date +%s%3N` arithmetic (script bug). Reproduced manually via device-side logcat:

```
1777522871.381  24008 24027 D WarpTerminal: PTY_WRITE cmdId=default bytes=4
1777522871.385  24008 24028 I WarpTerminal:PtyOutput: /
```

**Latency: 4 ms** (PTY_WRITE intent → cwd output) — well under 500ms threshold.

## Anomaly Analysis

The script's broad anomaly regex `PhantomProcess|signal [0-9]+|FATAL|crash` over-counts unrelated system events. Manual app-only filter:

```bash
adb logcat -d | grep -E "PhantomProcess|signal [0-9]+|FATAL|crash" \
                | grep -iE "dev\.warp\.mobile|u0a1073|warp_mobile|WarpTerminal"
```

Returns: **0 matches** across full 30-min logcat capture.

The 36 "system_anomalies_total" at t=30 are:
- ~33× `Zygote: Process N exited due to signal 9 (Killed)` for OTHER apps' background processes (normal Android lifecycle, not our app)
- 1× `PhantomProcessRecord {... 23767:sh/u0a413} died` for an entirely different app's shell (u0a413 ≠ our u0a1073)
- 2× Samsung internal services (`SemBatteryUsageStatsProvider: Receive crashed battery data`)

None affect dev.warp.mobile.

## Script Bugs Discovered (M2 carry-overs)

1. **macOS BSD date incompatibility** (line 163): `T_RECV=$(date +%s%3N)` works on Linux GNU coreutils but BSD date returns literal `N`. Arithmetic then fails. Fix: use `python3 -c 'import time; print(int(time.time()*1000))'` consistently (already used for T_SEND on line 147).
2. **Anomaly regex too broad** (line 101): grep matches device-wide events. Fix: filter for `dev.warp.mobile` or `u0a1073` UID.

## Service State Confirmation (post-test)

```
adb shell ps -A | grep dev.warp.mobile
u0_a1073     24008  1596   18636372  83892  0                  0 S dev.warp.mobile
```

App still running at 12:20+ CST. Constant PID 24008 across 30 minutes confirms no process recycling.

## Acceptance Verdict

**M1-S09 PASS** — all five criteria met. Flagship 30-min idle stress demonstrates the L0 Android Host Service + L0 PTY backend handle long-lived sessions correctly with no Android lifecycle interruptions.

Plan §6 M1 acceptance criterion 1 ("Service with FGS, persistent notification, 30-min idle survival on flagship") **fully satisfied** for S24 Ultra. Low-end device (Pixel 4a / Galaxy A52s API 31) re-run deferred to M2 per Plan Amendment 3.
