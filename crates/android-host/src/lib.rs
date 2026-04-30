#[cfg(unix)]
pub mod pty;

#[cfg(target_os = "android")]
mod font_render;

// M2-S10 IME state machine. Pure Rust + atomics; no Vulkan / NDK deps so we
// build it on host targets too (allows `cargo test -p warp-mobile-android-host`
// to exercise the state-machine tests without cross-compilation).
pub mod ime;

// M2-S11 touch input state machine. Same host-build policy as IME — no NDK
// deps so `cargo test -p warp-mobile-android-host` exercises the unit tests
// without cross-compilation.
pub mod input;

#[cfg(target_os = "android")]
mod static_grid;

// M3-S04: terminal model. Pure Rust + atomics (mirrors the canonical facade
// `warp_terminal_mobile_facade::render::TerminalModel`); built on host targets
// too so `cargo test -p warp-mobile-android-host` exercises ingest/dirty/snapshot.
pub mod terminal_model;

#[cfg(target_os = "android")]
mod vulkan;

use jni::objects::{JByteArray, JClass, JObjectArray, JString};
use jni::sys::{jbyteArray, jint, jlong, jshort, jstring};
use jni::JNIEnv;
use std::sync::Arc;

#[cfg(target_os = "android")]
use jni::objects::JObject;
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jfloat, JNI_FALSE, JNI_TRUE};

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ping(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    init_logger();
    log::info!(target: "android-host", "ping called");
    env.new_string("Rust ping ok from arm64-v8a")
        .expect("failed to create Java string")
        .into_raw()
}

// ── PTY JNI bindings ────────────────────────────────────────────────────────

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptySpawn(
    mut env: JNIEnv,
    _class: JClass,
    program: JString,
    args_array: JObjectArray,
    env_flat: JObjectArray,
) -> jlong {
    init_logger();
    let program_str: String = match env.get_string(&program) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };

    // Extract args from JObjectArray
    let args_len = env.get_array_length(&args_array).unwrap_or(0);
    let mut args_owned: Vec<String> = Vec::with_capacity(args_len as usize);
    for i in 0..args_len {
        if let Ok(elem) = env.get_object_array_element(&args_array, i) {
            let jstr: jni::objects::JString = elem.into();
            let s: String = env.get_string(&jstr)
                .map(|j| String::from(j))
                .unwrap_or_default();
            if !s.is_empty() { args_owned.push(s); }
        }
    }
    let args_refs: Vec<&str> = args_owned.iter().map(|s| s.as_str()).collect();

    // Extract env pairs from flat ["KEY=VALUE", ...] array
    let env_len = env.get_array_length(&env_flat).unwrap_or(0);
    let mut env_owned: Vec<(String, String)> = Vec::with_capacity(env_len as usize);
    for i in 0..env_len {
        if let Ok(elem) = env.get_object_array_element(&env_flat, i) {
            let jstr: jni::objects::JString = elem.into();
            let kv: String = env.get_string(&jstr)
                .map(|j| String::from(j))
                .unwrap_or_default();
            if let Some(eq) = kv.find('=') {
                env_owned.push((kv[..eq].to_string(), kv[eq + 1..].to_string()));
            }
        }
    }
    let env_refs: Vec<(&str, &str)> = env_owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    match pty::spawn_pty(&program_str, &args_refs, &env_refs) {
        Ok(session) => {
            // Arc refcount=1 owned by Java's PtyManager map
            let ptr = Arc::into_raw(Arc::new(session)) as jlong;
            log::info!(target: "android-host", "ptySpawn ok ptr={}", ptr);
            ptr
        }
        Err(e) => {
            log::error!(target: "android-host", "ptySpawn failed: {}", e);
            0
        }
    }
}

/// Increment Arc refcount. Called under PtyManager map lock before ptyRead.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyAcquire(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) -> jlong {
    if ptr == 0 { return 0; }
    unsafe { Arc::increment_strong_count(ptr as *const pty::PtySession) };
    ptr
}

/// Decrement Arc refcount. Called in finally after ptyRead completes.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyRelease(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 { return; }
    unsafe { Arc::decrement_strong_count(ptr as *const pty::PtySession) };
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyRead(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    max_bytes: jint,
) -> jbyteArray {
    if ptr == 0 {
        return std::ptr::null_mut();
    }
    // Safety: caller holds an Arc ref (via ptyAcquire) for the duration of this call
    let session = unsafe { &*(ptr as *const pty::PtySession) };
    let cap = max_bytes.max(0) as usize;
    let mut buf = vec![0u8; cap];
    match session.read(&mut buf) {
        Ok(n) => {
            let slice = &buf[..n];
            match env.byte_array_from_slice(slice) {
                Ok(arr) => arr.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
        Err(_) => std::ptr::null_mut(),
    }
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyWrite(
    env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    data: JByteArray,
) -> jint {
    if ptr == 0 {
        return -1;
    }
    let session = unsafe { &*(ptr as *const pty::PtySession) };
    let bytes: Vec<u8> = match env.convert_byte_array(&data) {
        Ok(b) => b,
        Err(_) => return -1,
    };
    match session.write(&bytes) {
        Ok(n) => n as jint,
        Err(e) => -(e.raw_os_error().unwrap_or(1) as jint),
    }
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyResize(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
    rows: jshort,
    cols: jshort,
) -> jint {
    if ptr == 0 {
        return -1;
    }
    let session = unsafe { &*(ptr as *const pty::PtySession) };
    match session.resize(rows as u16, cols as u16) {
        Ok(()) => 0,
        Err(e) => -(e.raw_os_error().unwrap_or(1) as jint),
    }
}

#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_ptyKill(
    _env: JNIEnv,
    _class: JClass,
    ptr: jlong,
) {
    if ptr == 0 {
        return;
    }
    let session = unsafe { &*(ptr as *const pty::PtySession) };
    // Close fd eagerly so concurrent reads return EBADF immediately
    session.kill_eager();
    session.kill().ok();
    // Decrement the Arc refcount held by the Java map (spawned with Arc::into_raw)
    unsafe { Arc::decrement_strong_count(ptr as *const pty::PtySession) };
}

// ── Vulkan / render JNI bindings (M2-S04) ───────────────────────────────────

/// Attaches an Android `Surface` (from `SurfaceHolder.getSurface()`) to the
/// Vulkan swapchain. Returns `true` on success.
///
/// Wraps `ANativeWindow_fromSurface` + the M0-spike-validated initialization
/// path in `vulkan.rs`.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderAttachSurface(
    env: JNIEnv,
    _class: JClass,
    surface: JObject,
) -> jboolean {
    init_logger();
    if surface.is_null() {
        log::error!(target: "warp-android-host", "renderAttachSurface: null Surface");
        return JNI_FALSE;
    }
    // SAFETY: env.get_native_interface() returns a valid JNIEnv* per JNI spec;
    // ANativeWindow_fromSurface returns a refcounted ANativeWindow* (we own the
    // ref and pass ownership into vulkan::attach).
    let native_window = unsafe {
        ndk_sys::ANativeWindow_fromSurface(
            env.get_native_interface() as *mut _,
            surface.as_raw() as *mut _,
        )
    };
    if native_window.is_null() {
        log::error!(target: "warp-android-host",
            "renderAttachSurface: ANativeWindow_fromSurface returned null");
        return JNI_FALSE;
    }
    // SAFETY: ownership transfers to vulkan::attach.
    let ok = unsafe { vulkan::attach(native_window) };
    if ok { JNI_TRUE } else { JNI_FALSE }
}

/// Detaches the Vulkan swapchain. Idempotent.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderDetachSurface(
    _env: JNIEnv,
    _class: JClass,
) {
    init_logger();
    vulkan::detach();
}

/// Submits a single clear-color frame. Returns `true` on successful
/// `vkQueuePresentKHR`. The clear color components are floats in [0.0, 1.0].
///
/// For M2-S04 the JNI side typically passes magenta `(1.0, 0.0, 1.0, 1.0)`.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderClearFrame(
    _env: JNIEnv,
    _class: JClass,
    r: jfloat,
    g: jfloat,
    b: jfloat,
    a: jfloat,
) -> jboolean {
    let ok = vulkan::submit_clear_frame([r, g, b, a]);
    if ok { JNI_TRUE } else { JNI_FALSE }
}

/// Returns the cumulative number of frames presented since the last attach.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderFramesPresented(
    _env: JNIEnv,
    _class: JClass,
) -> jlong {
    vulkan::frames_presented() as jlong
}

/// M2-S05: capture a single frame as PNG to the given file path.
///
/// Renders one magenta clear frame, copies the swapchain image to a host-coherent
/// staging buffer via `vkCmdCopyImageToBuffer`, swizzles BGRA→RGBA if needed,
/// and encodes a PNG via the `png` crate.
///
/// Returns `true` on success. Logs a `capture_ok` line with frame number,
/// dimensions, mean RGB, and bytes written; the test driver greps for this.
///
/// Synchronous — blocks the calling thread until `vkQueueWaitIdle` completes.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderCaptureFrame(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    r: jfloat,
    g: jfloat,
    b: jfloat,
    a: jfloat,
) -> jboolean {
    init_logger();
    let path_str: String = match env.get_string(&path) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(target: "warp-android-host",
                "renderCaptureFrame: could not extract path JString: {:?}", e);
            return JNI_FALSE;
        }
    };
    let out_path = std::path::PathBuf::from(&path_str);
    match vulkan::capture_to_png([r, g, b, a], &out_path) {
        Some(_) => JNI_TRUE,
        None => JNI_FALSE,
    }
}

/// M2-S07: capture a single frame as PNG with shaped text composited on top.
///
/// 1. Renders one magenta clear frame and reads back the swapchain image into
///    a host-coherent staging buffer (M2-S05 pipeline).
/// 2. Constructs a cosmic-text `FontSystem` populated from `/system/fonts/`
///    via `ASystemFontIterator` (NDK API 29+) — see
///    `crates/android-host/src/font_render.rs` for the full discovery pipeline.
/// 3. Shapes `text` (typically `"Hello, 世界"`) and rasterizes each glyph via
///    swash; alpha-blends every glyph onto the captured RGBA buffer at
///    `(baseline_x, baseline_y)` using `font_size_px`.
/// 4. Encodes the modified buffer as PNG.
///
/// Returns `true` on success. Logs `capture_ok` (M2-S05 schema) AND a new
/// `font_render_ok` line carrying the M2-S07 counters (fonts_loaded,
/// glyphs_total, composed_pixels, mean_rgb_after, primary/cjk family). The
/// test driver greps these to verify glyph coverage.
///
/// Synchronous — blocks the calling thread until the PNG file is fully
/// flushed to disk.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderCaptureFrameWithText(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    r: jfloat,
    g: jfloat,
    b: jfloat,
    a: jfloat,
    text: JString,
    font_size_px: jfloat,
    baseline_x: jfloat,
    baseline_y: jfloat,
) -> jboolean {
    init_logger();
    let path_str: String = match env.get_string(&path) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(
                target: "warp-android-host",
                "renderCaptureFrameWithText: could not extract path JString: {:?}",
                e
            );
            return JNI_FALSE;
        }
    };
    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(
                target: "warp-android-host",
                "renderCaptureFrameWithText: could not extract text JString: {:?}",
                e
            );
            return JNI_FALSE;
        }
    };
    let out_path = std::path::PathBuf::from(&path_str);
    match vulkan::capture_to_png_with_text(
        [r, g, b, a],
        &out_path,
        &text_str,
        font_size_px,
        baseline_x,
        baseline_y,
    ) {
        Some(_) => JNI_TRUE,
        None => JNI_FALSE,
    }
}

// ── M2-S08: static glyph grid renderer JNI bindings ────────────────────────

/// M2-S08: initialize the static-grid GPU pipeline.
///
/// Builds the glyph atlas + per-instance vertex buffer + Vulkan pipeline once.
/// All expensive work (cosmic-text shaping, swash rasterization, GPU upload,
/// pipeline creation) happens synchronously in this call. Subsequent
/// `renderDrawGridFrame` calls are pure GPU draws with zero per-frame
/// allocations.
///
/// Returns `true` on success. The Rust side logs `static_grid_init_ok dt_ms=…
/// text=… rows=… cols=… atlas_glyphs=… instances=…` which the test driver
/// greps for.
///
/// Idempotent: if a grid is already attached, the old one is destroyed first.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderInitStaticGrid(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
    font_size_px: jfloat,
    rows: jint,
    cols: jint,
    cell_w_px: jfloat,
    cell_h_px: jfloat,
) -> jboolean {
    init_logger();
    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(target: "warp-android-host",
                "renderInitStaticGrid: could not extract text JString: {:?}", e);
            return JNI_FALSE;
        }
    };
    if rows <= 0 || cols <= 0 {
        log::error!(target: "warp-android-host",
            "renderInitStaticGrid: invalid grid dims rows={} cols={}", rows, cols);
        return JNI_FALSE;
    }
    let ok = vulkan::init_static_grid(
        &text_str,
        font_size_px,
        rows as u32,
        cols as u32,
        cell_w_px,
        cell_h_px,
    );
    if ok { JNI_TRUE } else { JNI_FALSE }
}

/// M2-S08: submit one grid frame (clear + grid draw + present).
///
/// Returns `true` on successful `vkQueuePresentKHR`. If no grid has been
/// initialized, the call falls back to a clear-color frame.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderDrawGridFrame(
    _env: JNIEnv,
    _class: JClass,
    r: jfloat,
    g: jfloat,
    b: jfloat,
    a: jfloat,
) -> jboolean {
    let ok = vulkan::submit_grid_frame([r, g, b, a]);
    if ok { JNI_TRUE } else { JNI_FALSE }
}

/// Returns true if a static grid is currently attached.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderStaticGridAttached(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if vulkan::static_grid_attached() { JNI_TRUE } else { JNI_FALSE }
}

/// Returns a comma-separated diagnostic string:
///   "atlas_glyphs=N,glyphs_per_frame=N,rows=N,cols=N,text=…"
/// or empty string if no grid attached. Used by the driver to round-trip
/// diagnostic info into the result JSON.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_renderStaticGridStats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let s = match vulkan::static_grid_stats() {
        Some((atlas, ppf, rows, cols, text)) => format!(
            "atlas_glyphs={},glyphs_per_frame={},rows={},cols={},text={}",
            atlas, ppf, rows, cols, text
        ),
        None => String::new(),
    };
    env.new_string(s)
        .expect("failed to create Java string")
        .into_raw()
}

// On non-Android Unix targets (host-side cargo test) the Vulkan symbols are
// gated out — JNI bindings are only meaningful on Android.

// ── M2-S10: IME composing-text JNI bindings ─────────────────────────────────
//
// Drives `crates/android-host/src/ime.rs::global_ime()` (which mirrors the
// canonical state machine in `warp-src/crates/warpui/src/platform/android/
// ime.rs` per M2-S10 AC#1). Java side is `WarpInputView.WarpInputConnection`
// in `android/app/src/main/java/dev/warp/mobile/WarpInputView.kt`.
//
// JNI call thread: the View's UI thread (per Android InputConnection contract).
// All three event entry points are guarded by a process-wide `Mutex` inside
// `ime::global_ime()` so the driver-side `imeStats` query (potentially on a
// different thread) is serialized against UI-thread updates.

/// M2-S10: Java `WarpInputView.WarpInputConnection.commitText` → Rust state.
///
/// `text` may be empty (some IMEs send empty commits as no-ops); the state
/// machine handles that internally without emitting LatinCommit events for
/// empty Latin commits.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_imeCommitText(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
    new_cursor_position: jint,
) {
    init_logger();
    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(target: "WarpIme", "imeCommitText: get_string failed: {:?}", e);
            return;
        }
    };
    ime::commit_text(&text_str, new_cursor_position as i32);
}

/// M2-S10: Java `WarpInputView.WarpInputConnection.setComposingText` → Rust.
///
/// Empty `text` while a region is active is treated as a finish (clears the
/// region without inserting); empty + no active region is a no-op.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_imeSetComposingText(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
    new_cursor_position: jint,
) {
    init_logger();
    let text_str: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(e) => {
            log::error!(target: "WarpIme", "imeSetComposingText: get_string failed: {:?}", e);
            return;
        }
    };
    ime::set_composing_text(&text_str, new_cursor_position as i32);
}

/// M2-S10: Java `WarpInputView.WarpInputConnection.finishComposingText` → Rust.
///
/// If the composing region is empty (Gboard known issue: spurious empty
/// finishComposingText between setComposingText and commitText), emits an
/// `EmptyFinish` marker rather than double-committing.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_imeFinishComposingText(
    _env: JNIEnv,
    _class: JClass,
) {
    init_logger();
    ime::finish_composing_text();
}

/// M2-S10: returns the current IME stats as a comma-separated string. Driver
/// queries this between sub-tests to read counters without parsing logcat.
///
/// Schema:
///   `commit_calls=N,set_composing_calls=N,finish_calls=N,events=N,
///    latin=N,composing_update=N,composing_commit=N,composing_finish=N,
///    empty_finish=N,is_composing=B,composing_text=S`
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_imeStats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let s = ime::stats_string();
    env.new_string(s)
        .expect("failed to create Java string")
        .into_raw()
}

/// M2-S10: reset the IME state (clear counters + composing region). Driver
/// calls this between Latin and Pinyin sub-tests so counters are independent.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_imeReset(
    _env: JNIEnv,
    _class: JClass,
) {
    init_logger();
    ime::reset();
}

// ── M2-S11: Touch input + gesture JNI bindings ──────────────────────────────
//
// Drives `crates/android-host/src/input.rs::global_input()` (which mirrors the
// canonical state machine in `warp-src/crates/warpui/src/platform/android/
// input.rs` per M2-S11 AC#1). Java side is `WarpInputView` in
// `android/app/src/main/java/dev/warp/mobile/WarpInputView.kt`.
//
// JNI call thread: View's UI thread (Android touch dispatch contract).
// All five event entry points + the stats/reset pair are guarded by the
// process-wide `Mutex` inside `input::global_input()`.

/// M2-S11: Java `WarpInputView.onTouchEvent ACTION_DOWN` → Rust state.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputTouchDown(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
) {
    init_logger();
    input::touch_down(x, y);
}

/// M2-S11: Java `WarpInputView.onTouchEvent ACTION_UP` → Rust state.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputTouchUp(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
) {
    init_logger();
    input::touch_up(x, y);
}

/// M2-S11: Java `WarpInputView.onTouchEvent ACTION_CANCEL` → Rust state.
///
/// Emits `InputEvent::TouchCancel` to close the open touch-down sequence when
/// Android cancels the gesture (e.g. a parent View intercepts the event stream,
/// or the window loses focus). Without this, Rust state believes the finger is
/// still down after an intercepted DOWN.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputTouchCancel(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
) {
    init_logger();
    input::touch_cancel(x, y);
}

/// M2-S11: Java `GestureDetector.onSingleTapConfirmed` → Rust state.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputTap(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
) {
    init_logger();
    input::tap(x, y);
}

/// M2-S11: Java `GestureDetector.onLongPress` → Rust state.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputLongPress(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
) {
    init_logger();
    input::long_press(x, y);
}

/// M2-S11: Java `GestureDetector.onScroll` + VelocityTracker → Rust state.
///
/// `dx`/`dy`: distance moved since last scroll event (from `onScroll` distanceX/Y).
/// `vx`/`vy`: instantaneous velocity in px/s from `VelocityTracker` (computed
/// at ACTION_MOVE time on Java side and forwarded here).
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputScroll(
    _env: JNIEnv,
    _class: JClass,
    x: jfloat,
    y: jfloat,
    dx: jfloat,
    dy: jfloat,
    vx: jfloat,
    vy: jfloat,
) {
    init_logger();
    input::scroll(x, y, dx, dy, vx, vy);
}

/// M2-S11: returns current input stats as a comma-separated string:
///   "touch_down=N,touch_up=N,tap=N,long_press=N,scroll=N,events=N,
///    last_down_x=F,last_down_y=F,last_up_x=F,last_up_y=F,
///    last_scroll_vx=F,last_scroll_vy=F"
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputStats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let s = input::stats_string();
    env.new_string(s)
        .expect("failed to create Java string")
        .into_raw()
}

/// M2-S11: reset the input state (clear counters + events). Driver calls
/// between sub-tests so each sub-test's counters are independent.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_inputReset(
    _env: JNIEnv,
    _class: JClass,
) {
    init_logger();
    input::reset();
}

// ── M2-S12: WindowInsets render area ────────────────────────────────────────
//
// Called from `ViewCompat.setOnApplyWindowInsetsListener` in MainActivity
// whenever system insets change (IME up/down, system bars show/hide, rotation).
// For M2-S12 we store + log; M3 grid rendering will consume the effective
// viewport to avoid overlap with the IME panel or status bar.

/// Process-global render insets in physical pixels.
/// Updated atomically from the UI thread (single-writer via main thread).
static RENDER_INSETS_TOP: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);
static RENDER_INSETS_LEFT: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);
static RENDER_INSETS_RIGHT: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);
static RENDER_INSETS_BOTTOM: std::sync::atomic::AtomicI32 =
    std::sync::atomic::AtomicI32::new(0);

/// M2-S12: store the current effective render insets reported from
/// `ViewCompat.setOnApplyWindowInsetsListener` in MainActivity.
///
/// Insets are in physical pixels (same coordinate space as ANativeWindow).
/// `bottom` is `max(ime.bottom, sysBars.bottom)` from the Kotlin side so
/// it represents whichever reservation is larger (keyboard or nav bar).
///
/// For M2-S12 this is a store + log only. M3 will read these atomics when
/// computing the effective render viewport for the block grid.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_setRenderInsets(
    _env: JNIEnv,
    _class: JClass,
    top: jni::sys::jint,
    left: jni::sys::jint,
    right: jni::sys::jint,
    bottom: jni::sys::jint,
) {
    use std::sync::atomic::Ordering;
    init_logger();
    RENDER_INSETS_TOP.store(top, Ordering::Relaxed);
    RENDER_INSETS_LEFT.store(left, Ordering::Relaxed);
    RENDER_INSETS_RIGHT.store(right, Ordering::Relaxed);
    RENDER_INSETS_BOTTOM.store(bottom, Ordering::Relaxed);
    log::info!(
        target: "WarpRender",
        "render_insets top={} left={} right={} bottom={}",
        top, left, right, bottom
    );
}

// ── M3-S04: Terminal model + push_frame JNI bindings ───────────────────────
//
// Pipeline: PTY bytes (M1 backend) → terminalInputBytes JNI → facade-shaped
// TerminalModel.ingest_pty_bytes → Choreographer-side terminalPushFrame JNI
// → vulkan::push_frame (which chains init_static_grid + submit_grid_frame).
//
// Java side flow:
//   1. WarpTerminalService.kt PTY read coroutine forwards every chunk to
//      `NativeBridge.terminalInputBytes(cmdId, bytes)`. This is fire-and-forget;
//      the model handles its own dirty bit.
//   2. MainActivity Choreographer per-vsync callback calls
//      `NativeBridge.terminalTakeDirtyAndPushFrame(font_size_px, rows, cols,
//      cell_w_px, cell_h_px)`. If the model is dirty, it snapshots the text
//      and re-inits the GPU grid + submits a frame; otherwise it falls back
//      to `renderClearFrame` so the loop keeps presenting at vsync rate.
//
// Logcat tag: `WarpTerminalModel` (Rust). Test drivers grep these.

/// M3-S04: Java `WarpTerminalService.startReadLoop` → Rust state.
///
/// Forwards the PTY chunk to the process-global [`terminal_model::TerminalModel`].
/// Returns the number of bytes ingested (always equals `bytes.len`) so the
/// Java side can spot-check the round-trip count via stats.
///
/// `cmd_id` is forwarded for logging only — the M3-S04 baseline routes ALL
/// PTY chunks into the single global model. M3 future work may key per-tab.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalInputBytes(
    mut env: JNIEnv,
    _class: JClass,
    cmd_id: JString,
    data: JByteArray,
) -> jint {
    init_logger();
    // cmd_id is informational; if extraction fails we still process bytes.
    let cmd_id_str: String = match env.get_string(&cmd_id) {
        Ok(s) => s.into(),
        Err(_) => String::from("<unknown>"),
    };
    let bytes: Vec<u8> = match env.convert_byte_array(&data) {
        Ok(b) => b,
        Err(e) => {
            log::error!(
                target: "WarpTerminalModel",
                "terminalInputBytes: convert_byte_array failed: {:?}", e
            );
            return -1;
        }
    };
    let n = terminal_model::ingest_pty_bytes(&bytes);
    log::debug!(
        target: "WarpTerminalModel",
        "terminalInputBytes cmd_id={} bytes={} ingested={}",
        cmd_id_str, bytes.len(), n
    );
    n as jint
}

/// M3-S04: Choreographer-driven push_frame.
///
/// If the model dirty bit is set, snapshots the current text + drives the
/// Vulkan static-grid pipeline (re-init + submit). Returns:
///   *  1 → frame pushed successfully
///   *  0 → no dirty buffer; caller should fall back to clear-frame
///   * -1 → init/submit failed
///
/// The grid params (`font_size_px`, `rows`, `cols`, `cell_w_px`, `cell_h_px`)
/// mirror `renderInitStaticGrid`. They are passed per-call rather than stored
/// in a global because they may differ per orientation / Activity recreate.
///
/// Tagged `WarpTerminalModel` for log scraping; the M3-S04 driver greps
/// `terminal_push_frame ok=… text_len=…` to verify the pipeline ran.
#[cfg(target_os = "android")]
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalTakeDirtyAndPushFrame(
    _env: JNIEnv,
    _class: JClass,
    font_size_px: jfloat,
    rows: jint,
    cols: jint,
    cell_w_px: jfloat,
    cell_h_px: jfloat,
) -> jint {
    if !terminal_model::take_dirty() {
        return 0;
    }
    if rows <= 0 || cols <= 0 {
        log::error!(
            target: "WarpTerminalModel",
            "terminalTakeDirtyAndPushFrame: invalid grid dims rows={} cols={}", rows, cols
        );
        return -1;
    }
    let snap = terminal_model::snapshot_text();
    if snap.is_empty() {
        log::warn!(
            target: "WarpTerminalModel",
            "terminalTakeDirtyAndPushFrame: empty snapshot; skipping"
        );
        return 0;
    }
    // M3-S04 baseline: the existing static_grid pipeline renders ONE text
    // string replicated across every grid cell (M2-S08 acceptance). It
    // doesn't natively support per-row text. We pick the first non-blank
    // row of the snapshot as the renderable text — this proves the
    // PTY→model→renderer pipeline end-to-end without requiring the M3-S05
    // dynamic-grid extension.
    //
    // M3-S05 will introduce per-cell color attrs which mandates the
    // dynamic-grid extension; at that point the renderer can consume
    // multi-row snapshots directly. For M3-S04 the single-line projection
    // is the defensible baseline.
    let render_text: String = snap
        .split('\n')
        .map(|line| line.trim_end_matches(' '))
        .find(|line| !line.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            // All-blank snapshot — render a placeholder dot so the static
            // grid pipeline stays valid (it rejects zero-instance buffers).
            ".".to_string()
        });
    let init_ok = vulkan::init_static_grid(
        &render_text,
        font_size_px,
        rows as u32,
        cols as u32,
        cell_w_px,
        cell_h_px,
    );
    if !init_ok {
        log::error!(
            target: "WarpTerminalModel",
            "terminalTakeDirtyAndPushFrame: init_static_grid failed; dropping frame"
        );
        return -1;
    }
    let present_ok = vulkan::submit_grid_frame([1.0, 0.0, 1.0, 1.0]);
    log::info!(
        target: "WarpTerminalModel",
        "terminal_push_frame ok={} render_text_len={} snapshot_len={} rows={} cols={} render_text={:?}",
        present_ok, render_text.len(), snap.len(), rows, cols, render_text
    );
    if present_ok { 1 } else { -1 }
}

/// M3-S04: returns terminal model dimensions/cursor/byte-count as a CSV
/// string for the device driver to read out without parsing logcat.
///
/// Schema:
///   `rows=N,cols=N,cursor_row=N,cursor_col=N,bytes_ingested=N,dirty=B`
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalModelStats(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let model = terminal_model::global_model();
    let (rows, cols) = model.dims();
    let cur = model.cursor();
    let bytes = model.bytes_ingested();
    // Non-destructive peek so the stats accessor doesn't accidentally swallow
    // a pending Choreographer re-init.
    let dirty = model.peek_dirty();
    let s = format!(
        "rows={},cols={},cursor_row={},cursor_col={},bytes_ingested={},dirty={}",
        rows, cols, cur.row, cur.col, bytes, dirty
    );
    env.new_string(s)
        .expect("failed to create Java string")
        .into_raw()
}

/// M3-S05: returns SGR/DCS parser counters as a CSV string. Used by the
/// device-side AC#7 driver to verify that ANSI color sequences and DCS
/// frames were actually parsed (rather than getting silently dropped).
///
/// Schema:
///   `sgr_apply_count=N,dcs_hook_count=N,dcs_error_count=N,cur_fg=0xRRGGBBAA,cur_bg=0xRRGGBBAA,cur_attrs=0xNN`
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalSgrSummary(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let model = terminal_model::global_model();
    let (sgr, hooks, errs) = model.parser_stats();
    let (fg, bg, attrs) = model.current_attrs();
    let s = format!(
        "sgr_apply_count={},dcs_hook_count={},dcs_error_count={},cur_fg=0x{:08X},cur_bg=0x{:08X},cur_attrs=0x{:02X}",
        sgr, hooks, errs, fg, bg, attrs
    );
    env.new_string(s)
        .expect("failed to create Java string")
        .into_raw()
}

/// M3-S07: returns the current `Vec<Block>` as a JSON array. Each entry
/// has `{id, start_time_unix_ms, command, exit_code, end_time_unix_ms}`
/// — wire-format identical to the canonical facade `BlockList::to_dump_json`.
///
/// Consumed by `tools/scripts/test-block-model.sh` to gate M3 Acceptance
/// #3 (start_time/command/exit_code populated correctly).
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalBlocksDump(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let json = terminal_model::blocks_dump_json();
    env.new_string(json)
        .expect("failed to create Java string")
        .into_raw()
}

/// M3-S04: resize the terminal model. Called from the Java side when the
/// surface dimensions change (e.g. rotation, IME show/hide). The grid is
/// reshaped; existing in-bounds cells are preserved.
#[allow(non_snake_case)]
#[no_mangle]
pub extern "C" fn Java_dev_warp_mobile_NativeBridge_terminalResize(
    _env: JNIEnv,
    _class: JClass,
    rows: jint,
    cols: jint,
) {
    init_logger();
    if rows <= 0 || cols <= 0 {
        log::error!(
            target: "WarpTerminalModel",
            "terminalResize: invalid dims rows={} cols={}", rows, cols
        );
        return;
    }
    terminal_model::resize(rows as usize, cols as usize);
    log::info!(
        target: "WarpTerminalModel",
        "terminal_resize rows={} cols={}", rows, cols
    );
}

// ── Logger ──────────────────────────────────────────────────────────────────

#[cfg(target_os = "android")]
fn init_logger() {
    let _ = android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("warp-android-host"),
    );
}

#[cfg(not(target_os = "android"))]
fn init_logger() {}
