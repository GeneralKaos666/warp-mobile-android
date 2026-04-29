# M0 Task 11: APK Install + First-Launch Verify

## APK

- Path: `spikes/vulkan-surface-recreate/android/app/build/outputs/apk/debug/app-debug.apk`
- Size: 3.3 MB
- Built with: `gradle assembleDebug` (AGP 8.7.3, Gradle 9.2.1)
- Embeds: `libvulkan_surface_recreate.so` (arm64-v8a, 706K after Task #13 rebuild)

## Install Results

| Device | Serial | Install | Launch | VkSurfaceKHR |
|--------|--------|---------|--------|--------------|
| S24 Ultra | R5CX10VFFBA | SUCCESS | SUCCESS | created successfully (ANativeWindow=0xb40000716d1f5610) |
| S21+ | RFCNC0WNT9H | SUCCESS | SUCCESS | created successfully (ANativeWindow=0xb400006fc00e98e0) |
| S8 | ce0317133a9ad0190c | SUCCESS | SUCCESS | created successfully (ANativeWindow=0x7c4a3b7010) |

All three devices: APK installs without error, Activity launches without crash,
Rust JNI library loads, `VkSurfaceKHR created successfully` appears in logcat tag `VulkanSpike`.

## Surface Handle

Surface handle uses `ANativeWindow_fromSurface` (NDK public API, available since API 26);
previous `Surface.getNativeHandle()` reflection issue resolved in commit `4aa1fac`.
No reflection code remains in the codebase.

## Script Location

- `spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh <device-serial>`
- Drives 100 rotation cycles, parses `surfaceDestroyed_ts` / `first_frame_presented_ts`
  pairs from logcat, outputs CSV + p50/p95/p99 summary

## References

- Task #13 fix verification: `.omc/m0-artifacts/M0-task13-vulkan-fix-verify.md`
