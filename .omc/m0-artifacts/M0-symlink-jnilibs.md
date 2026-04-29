# M0 Task 12: symlink-jniLibs execve() Three-Device Verification

## Summary

**VERDICT: ALL THREE DEVICES PASS — Pre-mortem Scenario B does NOT fire**

The symlink-jniLibs workaround for Android API 29+ `execve()` restriction is validated across all three target devices including Android 16 (S24 Ultra, sdk=36).

---

## Test Setup

**APK:** `spikes/symlink-jnilibs/app/build/outputs/apk/debug/app-debug.apk`
**Test script:** `scripts/run-symlink-test.sh`
**Binary:** Rust static `hello_exec` compiled for `aarch64-linux-android`, outputs `SYMLINK_EXEC_TOKEN_OK\n` and exits with code 42. Bundled as `jniLibs/arm64-v8a/libhello_exec.so`.

**Workaround tested:**
1. Locate `nativeLibraryDir` (system allowlisted path at install time).
2. Create `<filesDir>/usr/bin/hello_exec` symlink → `nativeLibraryDir/libhello_exec.so`.
3. Call `Runtime.getRuntime().exec(<symlinkPath>)`.
4. Verify exit code == 42 and stdout token == `SYMLINK_EXEC_TOKEN_OK`.

---

## Device Results

### Device 1: Samsung S24 Ultra — R5CX10VFFBA (Android 16, sdk=36)

```json
{"device":"R5CX10VFFBA","android_sdk":36,"exit_code":42,"stdout_token":"SYMLINK_EXEC_TOKEN_OK","errno":null,"passed":true}
```

**Result: PASS**
- Strictest enforcement (Android 16). Symlink exec succeeded.
- No errno, no EACCES, no ENOEXEC.
- Pre-mortem Scenario B does NOT apply on this device.

---

### Device 2: Samsung S21+ — RFCNC0WNT9H (Android 15, sdk=35)

```json
{"device":"RFCNC0WNT9H","android_sdk":35,"exit_code":42,"stdout_token":"SYMLINK_EXEC_TOKEN_OK","errno":null,"passed":true}
```

**Result: PASS**

---

### Device 3: Samsung S8 — ce0317133a9ad0190c (Android 9, sdk=28)

```json
{"device":"ce0317133a9ad0190c","android_sdk":28,"exit_code":42,"stdout_token":"SYMLINK_EXEC_TOKEN_OK","errno":null,"passed":true}
```

**Result: PASS** (baseline, as expected)

---

## Overall Verdict

| Device | Model | Android | SDK | passed |
|--------|-------|---------|-----|--------|
| R5CX10VFFBA | SM-S9280 (S24 Ultra) | 16 | 36 | **true** |
| RFCNC0WNT9H | SM-G9960 (S21+) | 15 | 35 | **true** |
| ce0317133a9ad0190c | SM-G950F (S8) | 9 | 28 | **true** |

**Pre-mortem Scenario B fire signal: NOT triggered.**

The symlink-via-jniLibs pattern works on all tested devices including Android 16. The system loader allowlist for `nativeLibraryDir` continues to permit `execve()` on symlinks pointing to files within that directory.

## Companion Retreat Trigger Note

N/A — S24 Ultra (Android 16) PASSED. Plan B1 (jniLibs symlink workaround) is validated for Termux bootstrap binary execution. ADR alternative #6 is NOT required at this milestone.

## Logcat Anomalies

None observed. No EACCES, ENOEXEC, or SELinux denials in any device's SymlinkExec logcat output.

---

## Artifacts

- APK source: `/Users/iml1s/Documents/mine/warp_termux/spikes/symlink-jnilibs/`
- Test script: `/Users/iml1s/Documents/mine/warp_termux/scripts/run-symlink-test.sh`
- Binary source: `/Users/iml1s/Documents/mine/warp_termux/spikes/symlink-jnilibs/hello-exec/`
