use ash::vk;
use jni::objects::{JClass, JObject};
use jni::JNIEnv;
use std::sync::Mutex;

static SURFACE_STATE: Mutex<Option<SurfaceState>> = Mutex::new(None);

struct SurfaceState {
    #[allow(dead_code)]
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    // Keep ANativeWindow alive until VkSurfaceKHR is destroyed
    native_window: *mut ndk_sys::ANativeWindow,
}

// Safety: ANativeWindow* is ref-counted and thread-safe per NDK contract.
unsafe impl Send for SurfaceState {}

fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("VulkanSpike"),
    );
}

#[cfg(target_os = "android")]
fn uptime_millis() -> i64 {
    let mut ts = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1000 + ts.tv_nsec as i64 / 1_000_000
}

#[cfg(not(target_os = "android"))]
fn uptime_millis() -> i64 { 0 }

unsafe fn create_surface_from_native_window(
    entry: &ash::Entry,
    instance: &ash::Instance,
    native_window: *mut ndk_sys::ANativeWindow,
) -> Result<vk::SurfaceKHR, vk::Result> {
    let android_surface_loader = ash::khr::android_surface::Instance::new(entry, instance);
    let create_info = vk::AndroidSurfaceCreateInfoKHR::default()
        .window(native_window as *mut ash::vk::ANativeWindow);
    android_surface_loader.create_android_surface(&create_info, None)
}

fn create_vulkan_instance(entry: &ash::Entry) -> Result<ash::Instance, vk::Result> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"VulkanSpikeApp")
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(c"NoEngine")
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::API_VERSION_1_1);

    let extension_names = [
        ash::khr::surface::NAME.as_ptr(),
        ash::khr::android_surface::NAME.as_ptr(),
    ];

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names);

    unsafe { entry.create_instance(&create_info, None) }
}

fn destroy_state(s: SurfaceState) {
    unsafe {
        s.surface_loader.destroy_surface(s.surface, None);
        s.instance.destroy_instance(None);
        // Release the ANativeWindow strong ref acquired by ANativeWindow_fromSurface
        ndk_sys::ANativeWindow_release(s.native_window);
    }
}

#[no_mangle]
pub extern "system" fn Java_com_warpmobile_spike_MainActivity_nativeSurfaceCreated(
    env: JNIEnv,
    _class: JClass,
    surface: JObject,
) {
    init_logging();
    let ts = uptime_millis();
    log::info!("surfaceCreated_ts={}", ts);

    // Acquire ANativeWindow from Java Surface object via NDK public API (API 26+)
    let native_window = unsafe {
        ndk_sys::ANativeWindow_fromSurface(env.get_native_interface() as *mut _, surface.as_raw() as *mut _)
    };

    if native_window.is_null() {
        log::error!("ANativeWindow_fromSurface returned null — Surface may be invalid");
        return;
    }
    log::debug!("ANativeWindow acquired: {:p}", native_window);

    let mut state = SURFACE_STATE.lock().unwrap();

    if let Some(old) = state.take() {
        destroy_state(old);
    }

    let entry = match unsafe { ash::Entry::load() } {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to load Vulkan entry: {:?}", e);
            unsafe { ndk_sys::ANativeWindow_release(native_window) };
            return;
        }
    };

    let instance = match create_vulkan_instance(&entry) {
        Ok(i) => i,
        Err(e) => {
            log::error!("Failed to create Vulkan instance: {:?}", e);
            unsafe { ndk_sys::ANativeWindow_release(native_window) };
            return;
        }
    };

    let vk_surface = match unsafe {
        create_surface_from_native_window(&entry, &instance, native_window)
    } {
        Ok(s) => {
            log::info!("VkSurfaceKHR created successfully (ANativeWindow={:p})", native_window);
            s
        }
        Err(e) => {
            log::error!("VkSurfaceKHR creation failed: {:?}", e);
            unsafe {
                instance.destroy_instance(None);
                ndk_sys::ANativeWindow_release(native_window);
            }
            return;
        }
    };

    let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);

    *state = Some(SurfaceState {
        entry,
        instance,
        surface_loader,
        surface: vk_surface,
        native_window,
    });
}

#[no_mangle]
pub extern "system" fn Java_com_warpmobile_spike_MainActivity_nativeSurfaceDestroyed(
    _env: JNIEnv,
    _class: JClass,
) {
    let ts = uptime_millis();
    log::info!("surfaceDestroyed_ts={}", ts);
    let mut state = SURFACE_STATE.lock().unwrap();
    if let Some(s) = state.take() {
        destroy_state(s);
        log::debug!("Surface state destroyed on surfaceDestroyed");
    }
}

#[no_mangle]
pub extern "system" fn Java_com_warpmobile_spike_MainActivity_nativeSurfaceChanged(
    env: JNIEnv,
    class: JClass,
    surface: JObject,
    _width: i32,
    _height: i32,
) {
    log::debug!("nativeSurfaceChanged — delegating to surfaceCreated");
    Java_com_warpmobile_spike_MainActivity_nativeSurfaceCreated(env, class, surface);
}
