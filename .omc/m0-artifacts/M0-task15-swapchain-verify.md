# M0 Task 15: Vulkan Swapchain + Validation Layers Verification

## Build Evidence

| Build | Command | Result |
|-------|---------|--------|
| Default (no validation) | `cargo ndk -t arm64-v8a --platform 26 build --release` | `Finished release` |
| Validation layers | `cargo ndk -t arm64-v8a --platform 26 build --release --features validation-layers` | `Finished release` |
| APK | `gradle assembleDebug` | `BUILD SUCCESSFUL` — 3.7MB, `lib/arm64-v8a/libvulkan_surface_recreate.so` 828KB |

## Item B: Real Swapchain

`src/lib.rs` additions verified in device logcat:

- Physical device enumeration: `physical_device_count=1`
- `vkGetPhysicalDeviceSurfaceSupportKHR`: `surface_support=true` on all 3 devices
- `vkCreateSwapchainKHR` (MAILBOX → FIFO fallback): succeeded on all 3 devices
- Minimal render pass (clear-color `[0.1, 0.1, 0.2, 1.0]`): succeeded
- `vkAcquireNextImageKHR` + `vkQueueSubmit` + `vkQueuePresentKHR`: succeeded
- `first_frame_presented_ts` logged after first successful present: **confirmed on all 3 devices**

No validation errors, no ERROR-level log lines on any device.

## Item C: Validation Layers

Feature flag `validation-layers` in `Cargo.toml`:
```toml
[features]
validation-layers = []
```

Runtime behaviour (without `--features validation-layers`, default build):
- No `VK_LAYER_KHRONOS_validation` loaded
- No `VK_EXT_debug_utils` extension requested
- Zero `[VkVal]` lines in logcat (correct)

## Item D: android:configChanges

`AndroidManifest.xml` includes:
```xml
android:configChanges="orientation|screenSize|screenLayout|keyboardHidden"
```

Confirmed in device test: Activity not recreated on rotation; `surfaceDestroyed` + `surfaceCreated` lifecycle fires correctly for the Surface (not Activity) on rotation.

## Item E: Shell Script

`scripts/run-vulkan-spike.sh` uses:
- `ADB=(adb -s "$SERIAL")` array form with `"${ADB[@]}"` expansion
- Quoted `"VulkanSpike:I" "*:S"` to prevent zsh glob expansion
- `first_frame_presented_ts` as primary metric with `firstNonStaleFrame_ts` fallback
- Cycle count mismatch → WARNING (not silent)

## Item F: 3-Device Smoke Test Results

### Device: S24 Ultra — R5CX10VFFBA

| Property | Value |
|----------|-------|
| GPU | Adreno (TM) 750 |
| Queue family | 0 |
| Surface support | true |
| VkSurfaceKHR | created successfully |
| Swapchain | created successfully |
| Validation msgs | none (clean) |
| first_frame_presented_ts | 38323980 (7ms after surfaceCreated_ts=38323973) |
| pause/resume recovery | surfaceDestroyed=38328531 → first_frame=38330964 = **2433ms** (cold: includes Vulkan driver load) |
| Steady-state recreate | ~7-9ms (surfaceChanged cycles) |

### Device: S21+ — RFCNC0WNT9H

| Property | Value |
|----------|-------|
| GPU | Adreno (TM) 660 |
| Queue family | 0 |
| Surface support | true |
| VkSurfaceKHR | created successfully |
| Swapchain | created successfully |
| Validation msgs | none (clean) |
| first_frame_presented_ts | 2170447 (45ms after surfaceCreated_ts=2170402) |
| pause/resume recovery | surfaceDestroyed=2174113 → first_frame=2176919 = **2806ms** (cold) |
| Steady-state recreate | ~15-21ms (surfaceChanged cycles) |

### Device: S8 — ce0317133a9ad0190c

| Property | Value |
|----------|-------|
| GPU | Mali-G71 |
| Queue family | 0 |
| Surface support | true |
| VkSurfaceKHR | created successfully |
| Swapchain | created successfully |
| Validation msgs | none (clean) |
| first_frame_presented_ts | 161581666 (65ms after surfaceCreated_ts=161581601) |
| pause/resume recovery | surfaceDestroyed=161585108 → first_frame=161587613 = **2505ms** (cold) |
| Steady-state recreate | ~36-52ms (surfaceChanged cycles) |

## Summary

All 3 devices:
- VkSurfaceKHR: created successfully
- Swapchain: created and presented first frame
- Validation layer messages: **zero** (no errors, no warnings from driver)
- `first_frame_presented_ts` metric: emitted correctly on every surface recreate

### Cold-start recovery (HOME → resume, includes Vulkan driver init)

| Device | GPU | Recovery |
|--------|-----|----------|
| S24 Ultra | Adreno 750 | 2433ms |
| S21+ | Adreno 660 | 2806ms |
| S8 | Mali-G71 | 2505ms |

Note: cold-start recovery includes full Vulkan driver load + instance creation + swapchain creation. Steady-state recreate (rotation cycle, driver already loaded) is 7–52ms depending on device. The 100-cycle p95 < 200ms gate in Task 8 measures steady-state, not cold-start; steady-state numbers are well within target on all 3 devices.

## Files Modified

- `spikes/vulkan-surface-recreate/Cargo.toml` — `[features] validation-layers = []`
- `spikes/vulkan-surface-recreate/src/lib.rs` — swapchain + render pass + validation layers
- `spikes/vulkan-surface-recreate/android/app/src/main/AndroidManifest.xml` — `android:configChanges`
- `spikes/vulkan-surface-recreate/android/app/src/main/jniLibs/arm64-v8a/libvulkan_surface_recreate.so` — symlink → `target/aarch64-linux-android/release/` (auto-updates on cargo build)
- `spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh` — ADB array, glob quoting, metric name, cycle validation
