# M0 Task 3: cargo check warp_terminal aarch64-linux-android — Deps Report

## Build Log Summary

Two cargo check runs were performed:

**Run 1** (no PKG_CONFIG override):
```
error: failed to run custom build command for `yeslogic-fontconfig-sys v5.0.0`
  → panicked: pkg-config not configured for cross-compilation
```

**Run 2** (with `PKG_CONFIG_ALLOW_CROSS=1` to expose further errors):
```
error[E0583]: file not found for module `activity_impl`
error: Either "game-activity" or "native-activity" must be enabled as features
error[E0412]: cannot find type `PointerImpl` in this scope
error[E0412]: cannot find type `PointersIterImpl` in this scope
error[E0282]: type annotations needed
error: could not compile `android-activity` (lib) due to 5 previous errors
```

No `error[EXXXX]` compiler errors were emitted by Warp's own crates. All failures are in transitive build dependencies.

---

## Failed Dependencies

### 1. `yeslogic-fontconfig-sys v5.0.0`

**Type:** build-script failure (not a compiler error)

**Dependency chain:**
```
warp_terminal
  → warpui (direct dep)
      → warpui_core (direct dep)
          → font-kit (cfg(not(target_family = "wasm")) — Android is NOT wasm, so this fires)
              → yeslogic-fontconfig-sys  ← FAILS: no fontconfig on Android
```

**Why it fails:** `warpui_core/Cargo.toml` unconditionally pulls `font-kit` for all non-wasm targets. Android is not wasm. The warpdotdev `font-kit` fork v0.12.0 unconditionally lists `yeslogic-fontconfig-sys` as a dependency (no Linux-only cfg). `yeslogic-fontconfig-sys`'s build script calls `pkg-config` which panics on cross-compilation without explicit sysroot config.

**Source files using font-kit:**
| File | Lines |
|------|-------|
| `warpui_core/src/fonts/metrics.rs` | 33 |
| `warpui_core/src/fonts/canvas.rs` | 57 |
| `warpui/src/fonts/font_kit.rs` | 168 |
| `warpui/src/fonts/mod.rs` | 15 |
| `warpui/src/windowing/winit/fonts.rs` | 1270 |
| `warpui/src/windowing/winit/fonts/linux.rs` | 369 |
| `warpui/src/windowing/winit/fonts/windows.rs` | 266 |
| `warpui/src/platform/mac/fonts.rs` | 656 |
| **Total** | **2834** |

**cfg-gate lines estimate for font-kit path:** ~2834 lines across 8 files.

Note: the `winit/fonts*` files also depend on `winit` itself, so any Android stub would need to replace both the font-kit backend AND the winit font integration layer.

---

### 2. `android-activity v0.6.0`

**Type:** compiler error (missing feature flag)

**Dependency chain:**
```
warp_terminal
  → warpui (direct dep)
      → winit (workspace dep, warpdotdev fork, rev 7ef01853)
          → android-activity v0.6.0  ← FAILS: needs "game-activity" or "native-activity" feature
```

**Why it fails:** `android-activity` requires one of its two backend features to be selected. `winit` is declared in the workspace without Android-specific features enabled (`winit = { git = ..., rev = ... }` — no features). When `cargo ndk` builds for `aarch64-linux-android`, `android-activity` compiles but the backend feature guard panics.

**Fix required:** Workspace `Cargo.toml` needs `winit` to gain `features = ["android-native-activity"]` or `["android-game-activity"]` when targeting Android. This is a **feature flag change, not a cfg-gate** — 1 line in `Cargo.toml`.

**warpui windowing code size (winit-dependent):**
| Directory | Lines |
|-----------|-------|
| `warpui/src/windowing/` (all `.rs`) | 10,776 |

The windowing directory covers macOS, Linux, Windows, and WASM backends — all gated correctly per-platform already using `cfg(target_os = ...)` and `cfg(target_family = "wasm")`. Android would need a new backend module here.

---

## Per-Dependency cfg-gate Estimate

| Crate | Failure type | Files needing gates | Estimated lines |
|-------|-------------|---------------------|-----------------|
| `yeslogic-fontconfig-sys` (via `font-kit`) | build-script + missing Android font impl | 8 files in `warpui` + `warpui_core` | **~2834** |
| `android-activity` (via `winit`) | missing feature in workspace Cargo.toml | 1 line in `Cargo.toml` + new Android windowing module | **~1 (Cargo.toml) + ~500 (new Android backend stub)** |

**Total cfg-gate estimate: ~3334 lines**

---

## Pre-mortem C Threshold Assessment

**Threshold: 500 lines**

**Result: OVER threshold by ~6.7× (3334 lines estimated)**

The dominant cost is the `font-kit` integration in `warpui` and `warpui_core`. Font rendering is deeply coupled to the existing winit/desktop windowing stack. Gating all of it for Android would require:
1. Modifying `warpui_core/Cargo.toml` to use `cfg(not(any(target_family = "wasm", target_os = "android")))` for `font-kit`
2. Creating Android-specific stubs for `Metrics`, `Canvas`, `FontLoader` traits
3. Creating a new `warpui/src/windowing/android/` module (~500 lines minimum)
4. Patching the warpdotdev `winit` fork to enable `android-native-activity` feature

---

## Recommendation: D2-lite (warp_terminal_mobile_facade)

**Verdict: D2-lite is strongly preferred over D1 (cfg-gate everywhere)**

**Evidence:**

1. **cfg-gate scope is 6.7× over threshold.** The 2834 lines of font-kit integration are not superficial — `warpui` is the full desktop rendering stack (Metal, wgpu, winit). Gating it piecemeal risks correctness regressions on macOS/Linux.

2. **`warp_terminal` direct deps are clean.** `crates/warp_terminal/Cargo.toml` has 0 platform-specific deps. All failures are transitive through `warpui`. This is exactly the isolation point D2-lite exploits.

3. **The `warp_terminal_mobile_facade` pattern is already envisioned.** A thin facade crate that `pub use`s the pure data types and trait definitions from `warp_terminal` without pulling `warpui` avoids the entire font-kit/winit subtree.

4. **android-activity fix is trivial** (1 line in Cargo.toml) but the winit windowing backend replacement is not — D2-lite defers this to a separate Android windowing crate rather than splicing it into the existing desktop windowing module.

5. **D1 risk:** cfg-gating `warpui_core` font-kit usage would require Android stubs for font metrics/canvas that currently have no Android equivalent in the warpdotdev fork. This is undefined scope.

**D2-lite implementation path:**
- `warp_terminal_mobile_facade` depends only on `warp_terminal`, `warp_util`, `vte`, `warp_completer`, `warp_core` (the clean subset)
- Explicitly excludes `warpui` from its dependency graph
- Provides Android-only impls of any `warpui` trait boundaries that `warp_terminal` uses (surface rendering callbacks, font metrics stubs)
- `android-activity` feature flag fix still needed in workspace `Cargo.toml` as a one-liner prerequisite
