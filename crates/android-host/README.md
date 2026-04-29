# warp-mobile-android-host

Android Service + JNI layer for Warp Mobile (M1 skeleton).

## Structure

```
crates/android-host/
├── Cargo.toml       # crate manifest; crate-type = ["cdylib"]
└── src/
    └── lib.rs       # JNI entry points + logger init
```

## Build

```sh
cargo ndk -t arm64-v8a build -p warp-mobile-android-host
# output: target/aarch64-linux-android/debug/libwarp_mobile_android_host.so
```

Release:

```sh
cargo ndk -t arm64-v8a build -p warp-mobile-android-host --release
# output: target/aarch64-linux-android/release/libwarp_mobile_android_host.so
```

## JNI surface

| Rust symbol | Java signature |
|---|---|
| `Java_dev_warp_mobile_NativeBridge_ping` | `dev.warp.mobile.NativeBridge.ping() -> String` |

## Dependencies

| Crate | Purpose |
|---|---|
| `jni 0.21` | JNI bindings |
| `ndk 0.9` | Android NDK safe wrappers |
| `ndk-sys 0.6` | Raw NDK FFI |
| `log 0.4` | Logging facade |
| `android_logger 0.13` | Routes `log` macros to logcat |

## Build artefact verification

| Build | Output | Size | sha256 |
|---|---|---|---|
| debug, commit `10989b6` (ping only) | `target/aarch64-linux-android/debug/libwarp_mobile_android_host.so` | 16,750,448 bytes (16.7 MB) | `6e6960002e7e5fe9a5b9ee3b81723de4cbe1d4687c1c3a093a651818baf8f219` |
| debug, commit `ef0b06a` (ping + PTY) | `target/aarch64-linux-android/debug/libwarp_mobile_android_host.so` | ~17 MB | `3faa3fb5b07c92a4f6cc92646860551cb2f9f7f44b585c95b0a1837c76e0f36a` |

Symbols confirmed via `llvm-nm -D` after each commit (T = exported).

## M1 scope

This crate is a skeleton. M1 will add:
- Android `Service` lifecycle (start/stop via JNI)
- IPC channel between the Android Service and the Rust host
- Platform abstraction layer under `warpui::platform::android`
