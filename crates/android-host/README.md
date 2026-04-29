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
| `android_logger 0.14` | Routes `log` macros to logcat |
| `mio 1.0` | Async I/O event loop (IPC foundation) |

## M1 scope

This crate is a skeleton. M1 will add:
- Android `Service` lifecycle (start/stop via JNI)
- IPC channel between the Android Service and the Rust host
- Platform abstraction layer under `warpui::platform::android`
