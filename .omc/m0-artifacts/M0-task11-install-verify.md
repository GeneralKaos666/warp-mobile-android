# M0 Task 11: APK Install + First-Launch Verify

## APK

- Path: `spikes/vulkan-surface-recreate/android/app/build/outputs/apk/debug/app-debug.apk`
- Size: 3.3 MB
- Built with: `gradle assembleDebug` (AGP 8.7.3, Gradle 9.2.1)
- Embeds: `libvulkan_surface_recreate.so` (arm64-v8a, 716K)

## Install Results

| Device | Serial | Install | Launch | surfaceCreated_ts logged |
|--------|--------|---------|--------|--------------------------|
| S24 Ultra | R5CX10VFFBA | SUCCESS | SUCCESS | YES (ts=36678796) |
| S21+ | RFCNC0WNT9H | SUCCESS | SUCCESS | YES (ts=505825) |
| S8 | ce0317133a9ad0190c | SUCCESS | SUCCESS | YES (ts=159920830) |

All three devices: APK installs without error, Activity launches without crash,
Rust JNI library loads (`System.loadLibrary("vulkan_surface_recreate")` succeeds),
`surfaceCreated_ts` appears in logcat tag `VulkanSpike`.

## Known Issue: getNativeHandle Reflection Failure

All three devices emit:
```
W VulkanSpike: getNativeHandle failed: java.lang.NoSuchMethodException: android.view.Surface.getNativeHandle []
```

`android.view.Surface.getNativeHandle()` is not accessible via reflection on these
API levels (Android 12+). The native window pointer passed to Rust is therefore `0`.

**Impact on Task #8 measurement:** The `surfaceDestroyed_ts` / `firstNonStaleFrame_ts`
logcat lines are emitted from Kotlin (using `SystemClock.uptimeMillis()`), not from the
Rust JNI side — so frame-recovery timing measurement is unaffected by this issue.
The Vulkan surface creation itself will fail silently (null window), but the timing
scaffolding works.

**Fix before full 100-cycle measurement:** Replace `getNativeHandle` reflection with
NDK `ANativeWindow_fromSurface` via a proper JNI call, or use a `SurfaceView` and
pass the surface to a Rust JNI function that accepts `jobject` and calls
`ANativeWindow_fromSurface` internally.

## Script Location

- `spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh <device-serial>`
- Drives 100 rotation cycles, parses `surfaceDestroyed_ts` / `firstNonStaleFrame_ts`
  pairs from logcat, outputs CSV + p50/p95/p99 summary

## Update (Task #13)

**VkSurfaceKHR creation 'silent failure' caveat resolved.** Task #13 replaced
the failing `getNativeHandle()` reflection with `ANativeWindow_fromSurface()` (NDK
public API, API 26+). All three devices now log `VkSurfaceKHR created successfully`
with non-null ANativeWindow pointers. Task #8 is now unblocked.
See `.omc/m0-artifacts/M0-task13-surface-fix-verify.md` for full verification log.
