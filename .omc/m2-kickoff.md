# M2 Kickoff — `warpui::platform::android` Backend

**Status**: READY TO START (M0 + M1 both CLOSED CONDITIONAL GO)
**Estimated effort**: 8-12 person-weeks
**Plan reference**: `.omc/plans/ralplan-warp-on-mobile.md` §6 M2 (lines 370+, with Amendment 1+2 split into M2a/M2b under D1.5-hybrid)

---

## How to start M2

```
/oh-my-claudecode:ralph M2 milestone — warpui::platform::android backend per ralplan §6 M2 (D1.5-hybrid). Build Android Vulkan rendering on top of M1 PTY/Service infrastructure. 4 hand-written platform areas + headless-derived base. Verifier gate: codex per existing SOP.
```

Or for the autopilot pipeline:

```
/oh-my-claudecode:autopilot
```
(autopilot will detect ralplan plan exists and skip Phase 0+1, drop straight into Phase 2 execution per the skill semantics.)

---

## M2 entry state

**M0 + M1 deliverables already on main**:
- `android/app/` — Gradle project, minSdk 31 / target 36 / compile 36, FGS specialUse Service + Activity skeleton
- `crates/android-host/` — Rust workspace member, cdylib JNI host with PTY backend (Arc<PtySession> + AtomicI32 fd + ANR-safe)
- `tools/scripts/test-pty-{reattach,resize}.sh` + `test-fgs-clean-kill.sh` + `test-30min-idle-stress.sh` — 4 device drivers
- `spikes/vulkan-surface-recreate/` — M0 Vulkan lifecycle spike (50-line proof)
- `spikes/symlink-jnilibs/` — M0 jniLibs symlink path (M2 carry-over: replace with gradle copy task)
- `warp-src/` submodule — Warp upstream fork at `ImL1s/warp:warp-mobile/m0-facade` (commits `5400c66` facade scaffold, `afc74ec` android-activity feature)

**Verified risks retired**:
- L0 Android Service correctness (FGS 30-min flagship survival, isForeground=true)
- L0 PTY backend safety (AS-safe fork+execve, FD_CLOEXEC, robust kill, Arc lifetime, AtomicI32 fd)
- Activity recreate → PTY reattach <1s on flagship
- TIOCSWINSZ resize propagation
- FGS clean kill with no orphan PTY processes
- Vulkan surface recreate <200ms p95 on Adreno 6xx+

---

## M2 acceptance criteria (from Plan §6 M2)

Per ralplan §6 M2 (lines 470+ table):

1. **Static grid wgpu surface**: render a fixed M×N grid of colored cells via `warpui::platform::android` backend deriving from headless. 60fps steady-state on S24 Ultra (Adreno 750), p95 < 16.6ms per frame.

2. **IME glue**: Android InputMethodService surface receives keystrokes from system IME (Gboard, Samsung keyboard); characters propagated to `warpui_core::Window::input` via JNI. Test: type "hello" in Gboard → 5 input events received.

3. **Touch input mapping**: tap → `MouseDown/MouseUp` events with screen coordinates; basic scroll via 2-finger gesture. Test: tap on grid cell → coordinate within cell bounds.

4. **Rotation handling**: portrait ↔ landscape preserves Vulkan surface (recreate path from M0 spike), grid re-flows to new dims, no flicker > 1 frame.

5. **D1.5-hybrid integration**: `warp_terminal` → `warpui` Cargo edge intact (Plan §6 M2 §M2a constraint per Codex review of D1 facade-crate fail). Use `target_os = "android"` cfg gates inside `warpui::platform::android` module deriving from headless backend.

---

## M1 carry-overs to address in M2

(From `.omc/m1-artifacts/M1-go-no-go.md` §5)

1. **Acquire Pixel 4a / Galaxy A52s API 31** — re-run S06/S07/S08/S09 on it before M2 close (Plan Amendment 3 §3 requirement)
2. **Gradle copy task replacing jniLibs symlink** — currently `android/app/src/main/jniLibs/arm64-v8a/libwarp_mobile_android_host.so` is an absolute symlink to `target/aarch64-linux-android/debug/`, fragile on CI/clean-checkout
3. **android-activity / winit reorganization** — `warp-src/crates/warpui/Cargo.toml` explicit android-activity dep is redundant per Codex S02 review; fold into D1.5-hybrid restructuring
4. **Notification customization** — current FGS notification is generic "Warp terminal"; should add session count, command preview, tap → MainActivity intent
5. **Clippy lint cleanup** — `cargo clippy -p warp-mobile-android-host --target aarch64-linux-android -- -D warnings` flags 7 style issues (uninlined format args, let_unit_value on init_logger)

---

## Death-pit awareness for M2

Per ralplan death-pit ranking, M2 is **the #1 risk layer for the entire project** (gpui-mobile is not directly portable to Warp's `warpui_core::platform` trait surface; we must hand-write the Android backend deriving from headless).

**Highest-risk M2 sub-tasks**:
- `warpui::platform::android::Window::draw_frame` — mapping wgpu/Vulkan surface lifecycle to Android `SurfaceHolder.Callback` events without losing rendering state mid-frame
- IME composing-text state machine — Android IME emits `commitText` / `setComposingText` / `finishComposingText` events that must map cleanly to terminal cursor + dead-key state
- 4 hand-written areas (per Plan §6 M2a/M2b split): `Delegate`, `DispatchDelegate`, `WindowManager`, `TextLayoutSystem` — each requires careful Android-specific impl that survives Activity recreate

**Pre-mortem mitigation reference**: ralplan §"Pre-mortem 3 scenarios" — scenarios (a) M2 WarpUI Android backend stalls and (b) Termux runtime F-Droid path also walls are explicitly anticipated. Have escape hatches in mind.

---

## Verifier SOP (continues from M0/M1)

`prd.json` `verifierConfig.critic = "codex"` — every worker deliverable goes through Codex review before story is marked passes:true. Same SOP for M2.

---

## Recommended test device classes

Per Plan Amendment 3 (minSdk 31, Adreno 6xx+ baseline). Forward-looking; specific serials are user-private and live in `~/.claude/projects/.../memory/reference_devices.md`.

- **Primary flagship** (required) — Snapdragon 8 Gen 1+ / Adreno 730+ / API 33+. Examples: Galaxy S22-S24 series, Pixel 7+, OnePlus 11+. M1 verified on this class.
- **Secondary flagship** (recommended) — Snapdragon 888 / Adreno 660 / API 31+. Examples: Galaxy S21+, Pixel 6, OnePlus 9. M0 Vulkan verified on this class.
- **Low-end (M2 carry-over #1)** — Snapdragon 730G / 778G / Adreno 618-642L / API 31+. Examples: Pixel 4a, Galaxy A52s. **Required acquisition** before M2 close to satisfy Plan Amendment 3 §3 low-end coverage.
- **Below-min** — anything below API 31 or Adreno 6xx (e.g., Mali-G71-era, Galaxy S8) is dropped per Amendment 3 (S8 100-cycle p95 = 326ms FAIL).

All driver scripts under `tools/scripts/test-*.sh` take `<serial>` as first arg. Get yours from `adb devices`. Never hardcode serials in tracked code.
