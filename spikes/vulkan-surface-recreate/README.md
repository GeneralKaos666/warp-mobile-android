# Vulkan Surface Recreate Spike

This spike validates VkSurfaceKHR recreation lifecycle on Android during screen rotation
and app backgrounding. It measures frame-recovery time: the interval between
`surfaceDestroyed` and the first Choreographer frame callback after `surfaceCreated`.

## Build

Prerequisites:
- `cargo-ndk` installed: `cargo install cargo-ndk`
- Android NDK r25+ (set `ANDROID_NDK_ROOT`)
- Rust target: `rustup target add aarch64-linux-android`

```bash
export ANDROID_NDK_ROOT=$HOME/Library/Android/sdk/ndk/29.0.13113456
cd spikes/vulkan-surface-recreate
cargo ndk -t arm64-v8a --platform 26 build --release
```

Output `.so` is at:
`target/aarch64-linux-android/release/libvulkan_surface_recreate.so`

## Build APK

After building the `.so`, build the installable APK:

```bash
# Step 1: build .so (if not already done)
export ANDROID_NDK_ROOT=$HOME/Library/Android/sdk/ndk/29.0.13113456
cargo ndk -t arm64-v8a --platform 26 build --release

# Step 2: copy .so into jniLibs
mkdir -p android/app/src/main/jniLibs/arm64-v8a
cp target/aarch64-linux-android/release/libvulkan_surface_recreate.so \
   android/app/src/main/jniLibs/arm64-v8a/

# Step 3: assemble debug APK (requires ANDROID_HOME set)
export ANDROID_HOME=$HOME/Library/Android/sdk
cd android && gradle assembleDebug
```

APK output: `android/app/build/outputs/apk/debug/app-debug.apk`

Requires: Android SDK platform-34, build-tools 35+, AGP 8.7.3, Gradle 9.x.
Downloads dependencies from Maven Central on first run.

## Deploy to Device

```bash
# Install
adb -s <serial> install -r android/app/build/outputs/apk/debug/app-debug.apk

# Launch
adb -s <serial> shell am start -n com.warpmobile.spike/.MainActivity
```

The app shows a black `SurfaceView`. Rotating the device triggers
`surfaceDestroyed` → `surfaceCreated` cycles. Logcat tag: `VulkanSpike`.

## Automated 100-Cycle Measurement

Use `scripts/run-vulkan-spike.sh` to drive 100 rotation cycles and compute
frame-recovery statistics automatically:

```bash
./scripts/run-vulkan-spike.sh <device-serial>
```

### Prerequisites

- Device screen must be **ON and unlocked** (the script cannot unlock the screen)
- Auto-rotate must be **enabled** in device display settings
- `adb` in PATH, device authorized

### Output

The script outputs CSV to stdout and a summary line to stderr:

```
device,cycle,recovery_ms
R5CX10VFFBA,1,87
R5CX10VFFBA,2,94
...
# device=R5CX10VFFBA count=100 p50=91ms p95=143ms p99=178ms passed=true (threshold: p95<200ms)
```

Save results to a file:

```bash
./scripts/run-vulkan-spike.sh R5CX10VFFBA > results-s24.csv
```

### How recovery time is measured

| Event | Log line | Timestamp source |
|-------|----------|-----------------|
| Surface destroyed | `VulkanSpike: surfaceDestroyed_ts=<ms>` | `SystemClock.uptimeMillis()` |
| First frame after recreate | `VulkanSpike: firstNonStaleFrame_ts=<ms>` | `SystemClock.uptimeMillis()` |

Recovery = `firstNonStaleFrame_ts - surfaceDestroyed_ts` (ms).
Both timestamps use the same clock source, so the diff is meaningful.

### Target

**p95 < 200 ms** over 100 cycles = PASS.

## Expected Behavior

- `surfaceCreated`: Vulkan entry loaded, instance created, `VkSurfaceKHR` allocated via
  `VK_KHR_android_surface`. Previous surface/instance destroyed first if present.
- `surfaceDestroyed`: Surface and instance torn down immediately; no dangling handles.
- `surfaceChanged`: Delegates to `surfaceCreated` to recreate with new dimensions.

## Known Limitations

- No swapchain or render pass: this spike only tests surface lifecycle, not rendering.
- Instance is recreated on every `surfaceCreated`. For production, instance should be
  persistent and only the swapchain/surface recreated.
- `getNativeHandle()` reflection fails on Android 12+ (API 31+). The native window
  pointer passed to Rust is `0`, so `VkSurfaceKHR` creation will fail silently.
  Fix: replace with NDK `ANativeWindow_fromSurface` called from a JNI function
  that accepts `jobject surface`. Frame-recovery timing is unaffected (measured
  in Kotlin via `SystemClock.uptimeMillis()`).
- Rotation-based cycling requires device to be unlocked; headless automation is
  not possible without root or UiAutomator instrumentation.
