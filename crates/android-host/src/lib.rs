#[cfg(unix)]
pub mod pty;

use jni::objects::{JByteArray, JClass, JObjectArray, JString};
use jni::sys::{jbyteArray, jint, jlong, jshort, jstring};
use jni::JNIEnv;

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
    cmd: JString,
    _env_array: JObjectArray,
) -> jlong {
    init_logger();
    let cmd_str: String = match env.get_string(&cmd) {
        Ok(s) => s.into(),
        Err(_) => return 0,
    };
    match pty::spawn_pty(&cmd_str, &[], &[]) {
        Ok(session) => {
            let ptr = Box::into_raw(Box::new(session)) as jlong;
            log::info!(target: "android-host", "ptySpawn ok ptr={}", ptr);
            ptr
        }
        Err(e) => {
            log::error!(target: "android-host", "ptySpawn failed: {}", e);
            0
        }
    }
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
    let session = unsafe { Box::from_raw(ptr as *mut pty::PtySession) };
    session.kill().ok();
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
