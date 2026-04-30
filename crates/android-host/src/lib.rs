#[cfg(unix)]
pub mod pty;

#[cfg(target_os = "android")]
mod font_render;

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

// On non-Android Unix targets (host-side cargo test) the Vulkan symbols are
// gated out — JNI bindings are only meaningful on Android.

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
