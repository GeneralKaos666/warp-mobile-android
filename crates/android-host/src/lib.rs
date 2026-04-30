#[cfg(unix)]
pub mod pty;

use jni::objects::{JByteArray, JClass, JObjectArray, JString};
use jni::sys::{jbyteArray, jint, jlong, jshort, jstring};
use jni::JNIEnv;
use std::sync::Arc;

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
