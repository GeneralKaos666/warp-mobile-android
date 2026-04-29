# M0 Task 1: NDK Environment Report

## NDK Path
`/Users/iml1s/Library/Android/sdk/ndk/29.0.13113456`

## cargo-ndk Version
`cargo-ndk 4.1.2`

## Rust Target
`aarch64-linux-android` (pre-installed)

## .envrc Content
```bash
export ANDROID_NDK_ROOT=$HOME/Library/Android/sdk/ndk/29.0.13113456
export ANDROID_HOME=$HOME/Library/Android/sdk
```
Written to: `/Users/iml1s/Documents/mine/warp_termux/.envrc`

## .cargo/config.toml Content
```toml
[target.aarch64-linux-android]
linker = "/Users/iml1s/Library/Android/sdk/ndk/29.0.13113456/toolchains/llvm/prebuilt/darwin-x86_64/bin/aarch64-linux-android33-clang"
```
Written to: `/Users/iml1s/Documents/mine/warp_termux/.cargo/config.toml`

## Smoke Crate Build Log
Crate: `/tmp/cargo-ndk-smoke/smoke` (new --lib)
Target: `arm64-v8a` (aarch64-linux-android), profile: release

```
Building arm64-v8a (aarch64-linux-android)
Compiling smoke v0.1.0 (/private/tmp/cargo-ndk-smoke/smoke)
Finished `release` profile [optimized] target(s) in 0.10s
```

**Result: SUCCESS**

## Warnings
None. Linker resolved cleanly via `.cargo/config.toml` `[target.aarch64-linux-android]` entry.
