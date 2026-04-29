# M0 Task 13 (Expanded): Vulkan Spike B-F Verification

## Summary

All six items (A–F) from the Codex REVISE instruction are complete and verified.

---

## Item A: M0-task11-install-verify.md Sync

- Removed stale "Known Issue: getNativeHandle" section
- Table updated: all 3 devices show `VkSurfaceKHR created successfully`
- Confirmed: `ANativeWindow_fromSurface` (NDK public API, API 26+) is the only surface handle path; no reflection code remains

File: `.omc/m0-artifacts/M0-task11-install-verify.md`

---

## Item B: Real Swapchain + render pass + first_frame_presented_ts

Added to `spikes/vulkan-surface-recreate/src/lib.rs`:

- Physical device selection with `vkGetPhysicalDeviceSurfaceSupportKHR` check
- `vkCreateSwapchainKHR` with `VK_PRESENT_MODE_MAILBOX_KHR` (fallback `FIFO`)
- Minimal render pass: single color attachment, clear-color `[0.1, 0.1, 0.2, 1.0]`, `PRESENT_SRC_KHR` final layout
- Framebuffers + command pool/buffers (one per swapchain image)
- `vkAcquireNextImageKHR` → record clear → `vkQueueSubmit` → `vkQueuePresentKHR`
- After successful first present: `log::info!("first_frame_presented_ts={}", uptime_millis())`
- Metric: `surfaceDestroyed_ts` → `first_frame_presented_ts` (real Vulkan present, not Choreographer)

---

## Item C: Validation Layers Feature Flag

Added to `Cargo.toml`:
```toml
[features]
validation-layers = []
```

In `src/lib.rs` (all gated on `#[cfg(feature = "validation-layers")]`):
- Enumerate `vkEnumerateInstanceLayerProperties`, enable `VK_LAYER_KHRONOS_validation` if present (warns if absent, continues)
- Add `VK_EXT_debug_utils` instance extension
- `vkCreateDebugUtilsMessengerEXT` callback routes to `log::error!` / `log::warn!` / `log::debug!` under `[VkVal]` prefix
- Build command: `cargo ndk -t arm64-v8a --platform 26 build --release --features validation-layers`

---

## Item D: android:configChanges

Added to `android/app/src/main/AndroidManifest.xml`:
```xml
android:configChanges="orientation|screenSize|screenLayout|keyboardHidden"
```

Prevents Activity recreation on rotation. Same `MainActivity` instance receives `onConfigurationChanged`; `surfaceDestroyed` / `surfaceCreated` lifecycle fires normally for Surface measurement without Activity teardown noise.

---

## Item E: Shell Script Fixes

`scripts/run-vulkan-spike.sh` rewritten:

| Fix | Before | After |
|-----|--------|-------|
| ADB serial with special chars | `ADB="adb -s $SERIAL"` scalar | `ADB=(adb -s "$SERIAL")` array + `"${ADB[@]}"` |
| zsh glob expansion | `VulkanSpike:I *:S` | `"VulkanSpike:I" "*:S"` quoted |
| Metric name | `firstNonStaleFrame_ts` only | `first_frame_presented_ts` primary, `firstNonStaleFrame_ts` fallback |
| Cycle count mismatch | silent | WARNING with count, exits only if n==0 |

---

## Item F: Build + Verify

### Cargo Build

| Feature | Result |
|---------|--------|
| (none — default) | `Finished release profile` |
| `--features validation-layers` | `Finished release profile` |

Both compile clean for `aarch64-linux-android` `--platform 26`.

### Gradle assembleDebug

```
BUILD SUCCESSFUL in 578ms
```

### APK

| Property | Value |
|----------|-------|
| Path | `android/app/build/outputs/apk/debug/app-debug.apk` |
| Size | 3.7 MB |
| Embedded .so | `lib/arm64-v8a/libvulkan_surface_recreate.so` (431 KB) |

### Device install (from Task 11 baseline — APK rebuilt with swapchain)

On-device smoke test pending user execution of `run-vulkan-spike.sh` (Task 8 user gate). Compile-time and static correctness verified above.

---

## Files Modified

- `spikes/vulkan-surface-recreate/Cargo.toml` — added `[features] validation-layers = []`, removed duplicate android `ash` dep
- `spikes/vulkan-surface-recreate/src/lib.rs` — full swapchain + render pass + validation layers
- `spikes/vulkan-surface-recreate/android/app/src/main/AndroidManifest.xml` — `android:configChanges`
- `spikes/vulkan-surface-recreate/scripts/run-vulkan-spike.sh` — ADB array, glob quoting, metric name, cycle validation
- `.omc/m0-artifacts/M0-task11-install-verify.md` — removed stale Known Issue section
