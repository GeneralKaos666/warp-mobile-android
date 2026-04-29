# M0 Task 13: ANativeWindow Surface Handle Fix — Verify

## Root Cause Fixed

`Surface.getNativeHandle()` is a `@hide` private API removed in Android 12+.
All three test devices (S24 Ultra API 34, S21+ API 33, S8 API 28) threw
`NoSuchMethodException` via reflection, causing native window pointer = 0,
so `vkCreateAndroidSurfaceKHR` silently failed.

## Fix Applied

**Rust (`src/lib.rs`):**
- Added `ndk-sys = "0.6"` dependency (Android-target only)
- JNI signature changed from `nativeSurfaceCreated(nativeWindow: jlong)` to
  `nativeSurfaceCreated(surface: JObject)` — passes `android.view.Surface` as jobject
- Uses `ndk_sys::ANativeWindow_fromSurface(env, surface)` — NDK public API since API 26
- `ANativeWindow*` stored in `SurfaceState` and released via `ANativeWindow_release`
  in `destroy_state()` before `vkDestroySurfaceKHR`

**Kotlin (`MainActivity.kt`):**
- Removed all `Surface.getNativeHandle()` reflection code
- External function signatures: `nativeSurfaceCreated(surface: Surface)`,
  `nativeSurfaceChanged(surface: Surface, width: Int, height: Int)`
- Passes `holder.surface` directly as jobject to JNI

## Three-Device Smoke Test Results

### First Launch (surfaceCreated)

| Device | Serial | ANativeWindow | VkSurfaceKHR |
|--------|--------|--------------|--------------|
| S24 Ultra (API 34) | R5CX10VFFBA | `0xb40000716d1f5610` | created successfully |
| S21+ (API 33) | RFCNC0WNT9H | `0xb400006fc00e98e0` | created successfully |
| S8 (API 28) | ce0317133a9ad0190c | `0x7c4a3b7010` | created successfully |

### Pause → Resume Cycle (surfaceDestroyed → surfaceCreated)

#### S24 Ultra (R5CX10VFFBA)
```
04-29 20:39:22.052 I VulkanSpike: surfaceDestroyed_ts=37354526
04-29 20:39:23.330 I VulkanSpike: VkSurfaceKHR created successfully (ANativeWindow=0xb40000716d1f3c50)
04-29 20:39:23.337 I VulkanSpike: firstNonStaleFrame_ts=37355811
```
Note: new ANativeWindow address after recreate (0xb40000716d1f3c50 vs initial 0xb40000716d1f5610) — correct, surface is a new object.

#### S21+ (RFCNC0WNT9H)
```
04-29 20:39:27.641 I VulkanSpike: surfaceDestroyed_ts=1205787
04-29 20:39:29.232 I VulkanSpike: VkSurfaceKHR created successfully (ANativeWindow=0xb400006fc00e98e0)
04-29 20:39:29.250 I VulkanSpike: firstNonStaleFrame_ts=1207395
```

#### S8 (ce0317133a9ad0190c)
```
04-29 20:39:34.026 I VulkanSpike: surfaceDestroyed_ts=160622693
04-29 20:39:36.394 I VulkanSpike: VkSurfaceKHR created successfully (ANativeWindow=0x7c4a3b7010)
04-29 20:39:36.415 I VulkanSpike: firstNonStaleFrame_ts=160625082
```

All three devices: `surfaceDestroyed_ts` → old state destroyed → new ANativeWindow acquired
→ `VkSurfaceKHR created successfully` → `firstNonStaleFrame_ts`. No errors, no crashes.

## Recovery Times (HOME-key cycles, not rotation)

These are HOME-key pause/resume cycles, which are slower than rotation cycles.
Rotation is the correct measurement scenario for Task #8.

| Device | surfaceDestroyed→firstFrame (ms) |
|--------|----------------------------------|
| S24 Ultra | ~1285 |
| S21+ | ~1608 |
| S8 | ~2389 |

Task #8 rotation cycles expected to be significantly faster (target p95 < 200ms).

## ndk-sys Linkage Note

`ndk-sys = "0.6"` with `cargo-ndk 4.1.2` + NDK r29 linked cleanly without
requiring explicit `-l android`. No additional link flags needed.

## Status

Task #13 fix fully verified. Task #8 (100-cycle rotation measurement) is now unblocked.
