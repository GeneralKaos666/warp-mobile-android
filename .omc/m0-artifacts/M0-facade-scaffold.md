# M0 Facade Scaffold Artifact

## Branch & Commit

- Branch: `warp-mobile/m0-facade`
- Commit hash: `5400c66`
- Files committed (5):
  - `crates/warp_terminal_mobile_facade/Cargo.toml`
  - `crates/warp_terminal_mobile_facade/src/lib.rs`
  - `crates/warp_terminal_mobile_facade/src/terminal.rs`
  - `crates/warp_terminal_mobile_facade/src/blocks.rs`
  - `crates/warp_terminal_mobile_facade/src/ai.rs`

## cargo check Results

### Host (macOS) — FAIL (expected limitation)

**Result:** Build failed — `warpui` build script requires Metal Toolchain.

**Error:**
```
thread panicked at crates/warpui/build.rs:92:5:
error compiling metal shaders to .air; error: cannot execute tool 'metal' due to missing Metal Toolchain;
use: xcodebuild -downloadComponent MetalToolchain
```

**Why expected:** `warp_terminal` depends on `warpui`, which compiles Metal shaders at build time. This requires Xcode Metal Toolchain not present in this CI/dev environment. This is an upstream `warp_terminal` limitation, not a facade crate defect.

### Android (aarch64-linux-android) — FAIL (expected limitation)

**Result:** `cargo ndk -t arm64-v8a check -p warp_terminal_mobile_facade` failed.

**Error:**
```
error[E0282]: type annotations needed
error: could not compile `android-activity` (lib) due to 5 previous errors
```

**Why expected:** `warp_terminal` pulls in `android-activity` crate which has compilation errors against the current NDK/Rust toolchain. This is a known upstream dependency issue tracked in Task #3 (deps report). The facade crate itself has no Android-specific code yet (M2+ work).

## cfg-Dialect Convention for M2+

The facade uses a two-level cfg pattern for future Android-specific implementations:

```rust
// Module file (e.g., src/terminal.rs):

#[cfg(target_os = "android")]
pub use crate::android::terminal::*;

#[cfg(not(target_os = "android"))]
pub use warp_terminal::*;          // pass-through on host/macOS/Linux

#[cfg(target_os = "android")]
mod android {
    pub mod terminal {
        // TODO: M2+ — replace with Android-specific impl
        pub use warp_terminal::*;  // identical until M2 adds Android variants
    }
}
```

### Rules for M2+ contributors

1. **Never modify `warp_terminal` directly** — add Android variants inside `mod android` only.
2. **Add new modules** to `src/lib.rs` + create matching `src/<module>.rs` following the same pattern.
3. **Conflict scope** is bounded: upstream changes to `warp_terminal` only require updating `pub use warp_terminal::*` re-exports, not the Android-specific impl code.
4. **Feature flags** (`#[cfg(feature = "android-native")]`) may be layered on top of `target_os` checks in M2+ for optional capabilities (e.g., hardware keyboard detection).
