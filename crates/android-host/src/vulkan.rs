//! Android Vulkan render harness (M2-S04 runtime path).
//!
//! This module is the **runtime harness** that the JNI layer drives. It
//! mirrors the canonical Vulkan implementation in
//! `warp-src/crates/warpui/src/platform/android/vulkan.rs` so we have an
//! end-to-end runtime path on device for M2-S04 verification without yet
//! solving the cross-workspace Cargo dependency between this crate and
//! `warpui` (warp-src has workspace.package inheritance the main repo lacks;
//! unifying them is M3 scope).
//!
//! Both implementations are derived from the M0 spike at
//! `spikes/vulkan-surface-recreate/src/lib.rs` which validated lifecycle on
//! Adreno 6xx+ (S24 Ultra Adreno 750 p95=18ms, S21+ Adreno 660 p95=28ms over
//! 100 swapchain recreates).
//!
//! ## Design
//!
//! - Single process-wide `Mutex<Option<VulkanSurface>>` holds the entire
//!   Vulkan state. JNI exports drive lifecycle via
//!   `surface_attach`/`surface_detach`/`render_clear`.
//! - VK_LAYER_KHRONOS_validation enabled in debug builds; clean steady run is
//!   a hard M2-S04 acceptance gate.
//! - VK_ERROR_OUT_OF_DATE_KHR / VK_SUBOPTIMAL_KHR triggers swapchain recreate
//!   per Plan Amendment 2 hardened acceptance.
//! - Present mode FIFO (vsync-locked); image count = min_image_count + 1
//!   (typically 2-3 on Adreno).
//!
//! ## Web-search references consulted (2026-04-30):
//! - ash 0.38 swapchain example:
//!   <https://github.com/ash-rs/ash/blob/0.38.0/ash-examples/src/bin/triangle.rs>
//! - VK_KHR_android_surface man page:
//!   <https://registry.khronos.org/vulkan/specs/latest/man/html/VK_KHR_android_surface.html>
//! - ANativeWindow_fromSurface NDK reference:
//!   <https://developer.android.com/ndk/reference/group/a-native-window#anativewindow_fromsurface>
//! - VK_ERROR_OUT_OF_DATE_KHR handling pattern from ash examples + Vulkan
//!   spec swapchain VUID-vkAcquireNextImageKHR-semaphore-01779.

#![cfg(target_os = "android")]

use std::sync::Mutex;

use ash::vk;

/// Whether to enable validation layers. Default ON in debug builds.
const VALIDATION_LAYERS: bool = cfg!(debug_assertions);

#[allow(dead_code)] // some fields kept for ordered RAII teardown.
struct VulkanSurface {
    entry: ash::Entry,
    instance: ash::Instance,
    surface_loader: ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    native_window: *mut ndk_sys::ANativeWindow,
    phys_device: vk::PhysicalDevice,
    queue_family: u32,
    device: ash::Device,
    swapchain_loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    surface_format: vk::Format,
    extent: vk::Extent2D,
    render_pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    image_views: Vec<vk::ImageView>,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    graphics_queue: vk::Queue,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    fence: vk::Fence,
    debug_utils_loader: Option<ash::ext::debug_utils::Instance>,
    debug_messenger: vk::DebugUtilsMessengerEXT,
}

// SAFETY: ANativeWindow* is ref-counted (NDK contract). Vulkan handles are
// externally synchronized; Mutex guarantees single-threaded access.
unsafe impl Send for VulkanSurface {}

static SURFACE_STATE: Mutex<Option<VulkanSurface>> = Mutex::new(None);
static FRAME_COUNTER: Mutex<u64> = Mutex::new(0);

fn uptime_millis() -> i64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime accepts a valid out-pointer.
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as i64 * 1000 + ts.tv_nsec as i64 / 1_000_000
}

unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if !p_callback_data.is_null() {
        let msg = std::ffi::CStr::from_ptr((*p_callback_data).p_message);
        let text = msg.to_string_lossy();
        // Tag prefix `[VkVal]` is parsed by tools/scripts/test-render-scene.sh
        // to detect any non-debug validation messages.
        if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
            log::error!(target: "WarpVulkan", "[VkVal] {}", text);
        } else if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
            log::warn!(target: "WarpVulkan", "[VkVal] {}", text);
        } else {
            log::debug!(target: "WarpVulkan", "[VkVal] {}", text);
        }
    }
    vk::FALSE
}

fn create_vulkan_instance(entry: &ash::Entry) -> Result<ash::Instance, vk::Result> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"warp-mobile")
        .application_version(vk::make_api_version(0, 0, 1, 0))
        .engine_name(c"warp-android-host")
        .engine_version(vk::make_api_version(0, 0, 1, 0))
        .api_version(vk::API_VERSION_1_1);

    let mut extension_names: Vec<*const u8> = vec![
        ash::khr::surface::NAME.as_ptr(),
        ash::khr::android_surface::NAME.as_ptr(),
    ];
    if VALIDATION_LAYERS {
        extension_names.push(ash::ext::debug_utils::NAME.as_ptr());
    }

    let layer_names: Vec<*const u8> = if VALIDATION_LAYERS {
        // SAFETY: enumerate_instance_layer_properties is safe with a loaded entry.
        let available =
            unsafe { entry.enumerate_instance_layer_properties() }.unwrap_or_default();
        let khronos_val = c"VK_LAYER_KHRONOS_validation";
        let found = available.iter().any(|l| {
            // SAFETY: layer_name is always a NUL-terminated C string per spec.
            let name = unsafe { std::ffi::CStr::from_ptr(l.layer_name.as_ptr()) };
            name == khronos_val
        });
        if found {
            log::info!(target: "WarpVulkan", "VK_LAYER_KHRONOS_validation enabled");
            vec![khronos_val.as_ptr() as *const u8]
        } else {
            // Round-2 (Codex blocker 4b): in debug builds, validation layer is
            // a HARD M2-S04 acceptance gate. Silently warning + continuing led
            // to a false-positive PASS where the test driver saw zero
            // validation lines and reported `validation_clean=true` regardless.
            // Production (release) builds skip the layer entirely (above
            // VALIDATION_LAYERS check).
            log::error!(target: "WarpVulkan",
                "VK_LAYER_KHRONOS_validation NOT available — debug build packaging is broken");
            panic!("VK_LAYER_KHRONOS_validation layer required in debug builds; \
                    ensure libVkLayer_khronos_validation.so is packaged in jniLibs \
                    (run android/gradlew :app:fetchValidationLayer or set ANDROID_VALIDATION_LAYER_SO)");
        }
    } else {
        vec![]
    };

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names)
        .enabled_layer_names(&layer_names);
    // SAFETY: pointer arrays outlive this call.
    unsafe { entry.create_instance(&create_info, None) }
}

fn setup_debug_messenger(
    entry: &ash::Entry,
    instance: &ash::Instance,
) -> (
    Option<ash::ext::debug_utils::Instance>,
    vk::DebugUtilsMessengerEXT,
) {
    if !VALIDATION_LAYERS {
        return (None, vk::DebugUtilsMessengerEXT::null());
    }
    let loader = ash::ext::debug_utils::Instance::new(entry, instance);
    let create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR
                | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
                | vk::DebugUtilsMessageSeverityFlagsEXT::INFO,
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE,
        )
        .pfn_user_callback(Some(vulkan_debug_callback));
    // SAFETY: loader/instance live as long as the messenger.
    let messenger = unsafe { loader.create_debug_utils_messenger(&create_info, None) }
        .unwrap_or_else(|e| {
            log::warn!(target: "WarpVulkan", "create_debug_utils_messenger failed: {:?}", e);
            vk::DebugUtilsMessengerEXT::null()
        });
    (Some(loader), messenger)
}

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

fn find_graphics_queue_family(
    instance: &ash::Instance,
    phys_device: vk::PhysicalDevice,
) -> Option<u32> {
    // SAFETY: phys_device owned by instance.
    let queue_families =
        unsafe { instance.get_physical_device_queue_family_properties(phys_device) };
    queue_families
        .iter()
        .enumerate()
        .find_map(|(i, props)| {
            if props.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                Some(i as u32)
            } else {
                None
            }
        })
}

fn select_physical_device(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
) -> Option<(vk::PhysicalDevice, u32)> {
    // SAFETY: enumerate_physical_devices has no preconditions.
    let devices = unsafe { instance.enumerate_physical_devices() }.unwrap_or_default();
    log::info!(target: "WarpVulkan", "physical_device_count={}", devices.len());
    for dev in &devices {
        // SAFETY: dev came from enumerate_physical_devices.
        let props = unsafe { instance.get_physical_device_properties(*dev) };
        let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
        let Some(qf) = find_graphics_queue_family(instance, *dev) else {
            log::info!(target: "WarpVulkan",
                "device={} no_graphics_queue", name.to_string_lossy());
            continue;
        };
        let supported =
            unsafe { surface_loader.get_physical_device_surface_support(*dev, qf, surface) }
                .unwrap_or(false);
        log::info!(target: "WarpVulkan",
            "device={} queue_family={} surface_support={}",
            name.to_string_lossy(), qf, supported);
        if supported {
            return Some((*dev, qf));
        }
    }
    log::error!(target: "WarpVulkan",
        "no_suitable_physical_device count={}", devices.len());
    None
}

unsafe fn create_swapchain_and_dependents(
    surface_loader: &ash::khr::surface::Instance,
    phys_device: vk::PhysicalDevice,
    queue_family: u32,
    surface: vk::SurfaceKHR,
    device: &ash::Device,
    swapchain_loader: &ash::khr::swapchain::Device,
    old_swapchain: vk::SwapchainKHR,
) -> Result<
    (
        vk::SwapchainKHR,
        vk::Format,
        vk::Extent2D,
        vk::RenderPass,
        Vec<vk::ImageView>,
        Vec<vk::Framebuffer>,
        vk::CommandPool,
        Vec<vk::CommandBuffer>,
    ),
    vk::Result,
> {
    let formats = surface_loader
        .get_physical_device_surface_formats(phys_device, surface)
        .unwrap_or_default();
    let format = formats
        .iter()
        .find(|f| {
            f.format == vk::Format::B8G8R8A8_UNORM || f.format == vk::Format::R8G8B8A8_UNORM
        })
        .or_else(|| formats.first())
        .copied()
        .unwrap_or(vk::SurfaceFormatKHR {
            format: vk::Format::B8G8R8A8_UNORM,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        });

    // FIFO per Plan §6 M2 row #3 + M2-S04 AC: vsync-locked, no tearing.
    let present_mode = vk::PresentModeKHR::FIFO;

    let caps = surface_loader.get_physical_device_surface_capabilities(phys_device, surface)?;
    let extent = if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D {
            width: 1080,
            height: 2400,
        }
    };

    let image_count = (caps.min_image_count + 1)
        .min(if caps.max_image_count > 0 {
            caps.max_image_count
        } else {
            u32::MAX
        });

    // Pick a supported composite-alpha mode. Adreno 830 (S25) does not support
    // OPAQUE — we must query supportedCompositeAlpha and pick one of
    // {OPAQUE, INHERIT, PRE_MULTIPLIED, POST_MULTIPLIED}. INHERIT is the
    // safest fallback on Android since the SurfaceFlinger handles compositing.
    let composite_alpha = if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::OPAQUE) {
        vk::CompositeAlphaFlagsKHR::OPAQUE
    } else if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::INHERIT) {
        vk::CompositeAlphaFlagsKHR::INHERIT
    } else if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED) {
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED
    } else if caps.supported_composite_alpha.contains(vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED) {
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED
    } else {
        // Defensive — spec guarantees at least one mode supported, so this
        // path is unreachable on a conformant driver.
        log::warn!(target: "WarpVulkan",
            "no composite-alpha mode advertised; defaulting to INHERIT");
        vk::CompositeAlphaFlagsKHR::INHERIT
    };
    log::info!(target: "WarpVulkan",
        "composite_alpha selected: {:?} (supported_mask=0x{:x})",
        composite_alpha, caps.supported_composite_alpha.as_raw());

    let swapchain_create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(format.format)
        .image_color_space(format.color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(caps.current_transform)
        .composite_alpha(composite_alpha)
        .present_mode(present_mode)
        .clipped(true)
        .old_swapchain(old_swapchain);

    let swapchain = swapchain_loader.create_swapchain(&swapchain_create_info, None)?;

    // Render pass — clear → present_src layout.
    let attachment = vk::AttachmentDescription::default()
        .format(format.format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);
    let color_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
    let color_refs = [color_ref];
    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_refs);
    let dependency = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::empty())
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE);
    let attachments = [attachment];
    let subpasses = [subpass];
    let dependencies = [dependency];
    let rp_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    let render_pass = device.create_render_pass(&rp_info, None)?;

    // Framebuffers.
    let images = swapchain_loader.get_swapchain_images(swapchain)?;
    let mut image_views = Vec::with_capacity(images.len());
    let mut framebuffers = Vec::with_capacity(images.len());
    for image in &images {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(*image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format.format)
            .components(vk::ComponentMapping::default())
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .base_mip_level(0)
                    .level_count(1)
                    .base_array_layer(0)
                    .layer_count(1),
            );
        let view = device.create_image_view(&view_info, None)?;
        image_views.push(view);
        let attach = [view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attach)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        let fb = device.create_framebuffer(&fb_info, None)?;
        framebuffers.push(fb);
    }

    // Command pool + buffers (one per swapchain image).
    let pool_info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    let command_pool = device.create_command_pool(&pool_info, None)?;
    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(framebuffers.len() as u32);
    let command_buffers = device.allocate_command_buffers(&alloc_info)?;

    Ok((
        swapchain,
        format.format,
        extent,
        render_pass,
        image_views,
        framebuffers,
        command_pool,
        command_buffers,
    ))
}

fn init_surface(
    native_window: *mut ndk_sys::ANativeWindow,
) -> Result<VulkanSurface, vk::Result> {
    // SAFETY: Entry::load loads the system Vulkan loader; safe.
    let entry = unsafe { ash::Entry::load() }.map_err(|e| {
        log::error!(target: "WarpVulkan", "ash::Entry::load failed: {:?}", e);
        vk::Result::ERROR_INITIALIZATION_FAILED
    })?;

    let instance = create_vulkan_instance(&entry)?;
    let (debug_utils_loader, debug_messenger) = setup_debug_messenger(&entry, &instance);

    let surface = match unsafe {
        create_surface_from_native_window(&entry, &instance, native_window)
    } {
        Ok(s) => s,
        Err(e) => {
            // SAFETY: we own these handles on this failure path.
            unsafe {
                if let Some(loader) = &debug_utils_loader {
                    if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                        loader.destroy_debug_utils_messenger(debug_messenger, None);
                    }
                }
                instance.destroy_instance(None);
            }
            return Err(e);
        }
    };

    let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
    let (phys_device, queue_family) =
        match select_physical_device(&instance, &surface_loader, surface) {
            Some(x) => x,
            None => {
                unsafe {
                    surface_loader.destroy_surface(surface, None);
                    if let Some(loader) = &debug_utils_loader {
                        if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                            loader.destroy_debug_utils_messenger(debug_messenger, None);
                        }
                    }
                    instance.destroy_instance(None);
                }
                return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
            }
        };

    // Logical device.
    let queue_priorities = [1.0f32];
    let queue_create_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&queue_priorities);
    let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];
    let device_create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_create_info))
        .enabled_extension_names(&device_extensions);
    let device = unsafe { instance.create_device(phys_device, &device_create_info, None) }?;
    let graphics_queue = unsafe { device.get_device_queue(queue_family, 0) };

    let swapchain_loader = ash::khr::swapchain::Device::new(&instance, &device);

    let (
        swapchain,
        format,
        extent,
        render_pass,
        image_views,
        framebuffers,
        command_pool,
        command_buffers,
    ) = unsafe {
        create_swapchain_and_dependents(
            &surface_loader,
            phys_device,
            queue_family,
            surface,
            &device,
            &swapchain_loader,
            vk::SwapchainKHR::null(),
        )
    }?;

    let sem_info = vk::SemaphoreCreateInfo::default();
    let fence_info = vk::FenceCreateInfo::default();
    let image_available = unsafe { device.create_semaphore(&sem_info, None) }?;
    let render_finished = unsafe { device.create_semaphore(&sem_info, None) }?;
    let fence = unsafe { device.create_fence(&fence_info, None) }?;

    Ok(VulkanSurface {
        entry,
        instance,
        surface_loader,
        surface,
        native_window,
        phys_device,
        queue_family,
        device,
        swapchain_loader,
        swapchain,
        surface_format: format,
        extent,
        render_pass,
        framebuffers,
        image_views,
        command_pool,
        command_buffers,
        graphics_queue,
        image_available,
        render_finished,
        fence,
        debug_utils_loader,
        debug_messenger,
    })
}

unsafe fn recreate_swapchain(s: &mut VulkanSurface) -> Result<(), vk::Result> {
    let t0 = uptime_millis();
    s.device.device_wait_idle()?;

    for fb in s.framebuffers.drain(..) {
        s.device.destroy_framebuffer(fb, None);
    }
    for iv in s.image_views.drain(..) {
        s.device.destroy_image_view(iv, None);
    }
    s.device.destroy_render_pass(s.render_pass, None);
    s.device.free_command_buffers(s.command_pool, &s.command_buffers);
    s.device.destroy_command_pool(s.command_pool, None);

    let old = s.swapchain;
    let (
        swapchain,
        format,
        extent,
        render_pass,
        image_views,
        framebuffers,
        command_pool,
        command_buffers,
    ) = create_swapchain_and_dependents(
        &s.surface_loader,
        s.phys_device,
        s.queue_family,
        s.surface,
        &s.device,
        &s.swapchain_loader,
        old,
    )?;
    s.swapchain_loader.destroy_swapchain(old, None);

    s.swapchain = swapchain;
    s.surface_format = format;
    s.extent = extent;
    s.render_pass = render_pass;
    s.image_views = image_views;
    s.framebuffers = framebuffers;
    s.command_pool = command_pool;
    s.command_buffers = command_buffers;

    let t1 = uptime_millis();
    log::info!(target: "WarpVulkan",
        "recreate_swapchain ok dt_ms={} extent={}x{}",
        t1 - t0, extent.width, extent.height);
    Ok(())
}

/// Outcome of one frame's record-submit-present cycle.
///
/// Round-2 fix (Codex blockers 1+2): in ash 0.38, `acquire_next_image` and
/// `queue_present` return `Ok((idx, suboptimal))` and `Ok(suboptimal)` — the
/// suboptimal bool is NOT folded into `Err(SUBOPTIMAL_KHR)`. Plan Amendment 2
/// SUBOPTIMAL_KHR handling requires us to extract that bool and recreate the
/// swapchain BEFORE the next frame.
///
/// Refs:
///   <https://docs.rs/ash/latest/ash/khr/swapchain/struct.Device.html>
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameOutcome {
    /// Present succeeded; swapchain still optimal — keep going.
    Presented,
    /// Present succeeded BUT swapchain is suboptimal (e.g. orientation changed
    /// mid-frame). Recreate before the next frame.
    PresentedSuboptimal,
    /// Present or acquire returned OUT_OF_DATE; swapchain must be recreated.
    OutOfDate,
}

unsafe fn record_and_present_clear(
    s: &mut VulkanSurface,
    clear_rgba: [f32; 4],
) -> Result<FrameOutcome, vk::Result> {
    let device = &s.device;

    // ── Acquire ─────────────────────────────────────────────────────────────
    // ash 0.38: returns Ok((u32, bool)) where the bool is `suboptimal`.
    let (image_index, acquire_suboptimal) = match s.swapchain_loader.acquire_next_image(
        s.swapchain,
        u64::MAX,
        s.image_available,
        vk::Fence::null(),
    ) {
        Ok(pair) => pair,
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
            // No present happened — fence/cmd-pool are still clean from prior
            // frame. Return early; caller will recreate.
            return Ok(FrameOutcome::OutOfDate);
        }
        Err(e) => return Err(e),
    };

    let cmd_buf = s.command_buffers[image_index as usize];
    let begin_info = vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    device.begin_command_buffer(cmd_buf, &begin_info)?;

    let clear_values = [vk::ClearValue {
        color: vk::ClearColorValue {
            float32: clear_rgba,
        },
    }];
    let rp_begin = vk::RenderPassBeginInfo::default()
        .render_pass(s.render_pass)
        .framebuffer(s.framebuffers[image_index as usize])
        .render_area(vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent: s.extent,
        })
        .clear_values(&clear_values);
    device.cmd_begin_render_pass(cmd_buf, &rp_begin, vk::SubpassContents::INLINE);
    device.cmd_end_render_pass(cmd_buf);
    device.end_command_buffer(cmd_buf)?;

    // ── Submit ──────────────────────────────────────────────────────────────
    let wait_semaphores = [s.image_available];
    let signal_semaphores = [s.render_finished];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let cmd_bufs = [cmd_buf];
    let submit_info = vk::SubmitInfo::default()
        .wait_semaphores(&wait_semaphores)
        .wait_dst_stage_mask(&wait_stages)
        .command_buffers(&cmd_bufs)
        .signal_semaphores(&signal_semaphores);
    device.queue_submit(s.graphics_queue, &[submit_info], s.fence)?;

    // ── Present ─────────────────────────────────────────────────────────────
    // ash 0.38: queue_present returns Ok(suboptimal: bool) on success, or
    // Err(ERROR_OUT_OF_DATE_KHR) on swapchain mismatch.
    let swapchains = [s.swapchain];
    let image_indices = [image_index];
    let present_info = vk::PresentInfoKHR::default()
        .wait_semaphores(&signal_semaphores)
        .swapchains(&swapchains)
        .image_indices(&image_indices);
    let present_result = s
        .swapchain_loader
        .queue_present(s.graphics_queue, &present_info);

    // Round-2 fix (Codex blocker 2): on present-error path, the submit
    // already occurred so the fence is signaled and the command pool is
    // dirty. We MUST drain the queue + reset fence + reset command pool
    // before returning, otherwise the next submit (post-recreate) reuses a
    // signaled fence → undefined behavior.
    let present_suboptimal = match present_result {
        Ok(suboptimal) => suboptimal,
        Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
            // Drain + reset before recreate.
            device.queue_wait_idle(s.graphics_queue)?;
            device.reset_fences(&[s.fence])?;
            device.reset_command_pool(s.command_pool, vk::CommandPoolResetFlags::empty())?;
            return Ok(FrameOutcome::OutOfDate);
        }
        Err(e) => {
            // Same cleanup as above so the next attempt has a clean state.
            // Errors other than OUT_OF_DATE are unrecoverable for this frame
            // but should not poison subsequent attempts.
            let _ = device.queue_wait_idle(s.graphics_queue);
            let _ = device.reset_fences(&[s.fence]);
            let _ = device.reset_command_pool(s.command_pool, vk::CommandPoolResetFlags::empty());
            return Err(e);
        }
    };

    // Successful present path — drain the queue, reset fence + pool for the
    // next frame.
    device.queue_wait_idle(s.graphics_queue)?;
    device.reset_fences(&[s.fence])?;
    device.reset_command_pool(s.command_pool, vk::CommandPoolResetFlags::empty())?;

    if acquire_suboptimal || present_suboptimal {
        Ok(FrameOutcome::PresentedSuboptimal)
    } else {
        Ok(FrameOutcome::Presented)
    }
}

fn destroy_surface(s: VulkanSurface) {
    // SAFETY: ordered RAII teardown — wait_idle, per-surface, device, instance, native_window.
    unsafe {
        let _ = s.device.device_wait_idle();
        for fb in &s.framebuffers {
            s.device.destroy_framebuffer(*fb, None);
        }
        for iv in &s.image_views {
            s.device.destroy_image_view(*iv, None);
        }
        s.device.destroy_semaphore(s.image_available, None);
        s.device.destroy_semaphore(s.render_finished, None);
        s.device.destroy_fence(s.fence, None);
        s.device.destroy_command_pool(s.command_pool, None);
        s.swapchain_loader.destroy_swapchain(s.swapchain, None);
        s.device.destroy_render_pass(s.render_pass, None);
        s.device.destroy_device(None);
        s.surface_loader.destroy_surface(s.surface, None);
        if let Some(loader) = &s.debug_utils_loader {
            if s.debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                loader.destroy_debug_utils_messenger(s.debug_messenger, None);
            }
        }
        s.instance.destroy_instance(None);
        ndk_sys::ANativeWindow_release(s.native_window);
        let _ = s.entry;
    }
}

// ---------------------------------------------------------------------------
// Public-ish API used by JNI exports
// ---------------------------------------------------------------------------

/// Attaches an `ANativeWindow*` (typically obtained via
/// `ANativeWindow_fromSurface`).
///
/// # Safety
///
/// `native_window` must be a valid pointer with one outstanding ref count
/// owned by the caller. Ownership transfers to the swapchain (released on
/// detach).
pub(crate) unsafe fn attach(native_window: *mut ndk_sys::ANativeWindow) -> bool {
    if native_window.is_null() {
        log::error!(target: "WarpVulkan", "attach: null native_window");
        return false;
    }
    let mut state = match SURFACE_STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(old) = state.take() {
        log::info!(target: "WarpVulkan", "replacing prior VulkanSurface");
        destroy_surface(old);
    }
    match init_surface(native_window) {
        Ok(s) => {
            log::info!(target: "WarpVulkan",
                "attach ok extent={}x{} images={}",
                s.extent.width, s.extent.height, s.framebuffers.len());
            *state = Some(s);
            // Reset frame counter for the new attach.
            if let Ok(mut c) = FRAME_COUNTER.lock() {
                *c = 0;
            }
            true
        }
        Err(e) => {
            log::error!(target: "WarpVulkan", "init_surface failed: {:?}", e);
            ndk_sys::ANativeWindow_release(native_window);
            false
        }
    }
}

pub(crate) fn detach() {
    let mut state = match SURFACE_STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if let Some(s) = state.take() {
        destroy_surface(s);
        log::info!(target: "WarpVulkan", "detach ok");
    }
}

/// Submits a single clear-color frame. Returns `true` on successful present.
pub(crate) fn submit_clear_frame(clear_rgba: [f32; 4]) -> bool {
    let mut state = match SURFACE_STATE.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let Some(s) = state.as_mut() else {
        return false;
    };
    match unsafe { record_and_present_clear(s, clear_rgba) } {
        Ok(FrameOutcome::Presented) => {
            let n = if let Ok(mut c) = FRAME_COUNTER.lock() {
                *c += 1;
                *c
            } else {
                0
            };
            let ts = uptime_millis();
            // Tag parsed by tools/scripts/test-render-scene.sh: "present_ok frame=<n> ts=<ms>".
            log::info!(target: "WarpVulkan", "present_ok frame={} ts={}", n, ts);
            true
        }
        Ok(FrameOutcome::PresentedSuboptimal) => {
            // Round-2 (Codex blocker 1): swapchain reports suboptimal but the
            // present succeeded. Count the frame, then schedule a recreate
            // before the next acquire so the new orientation/scale takes
            // effect cleanly on the next vsync.
            let n = if let Ok(mut c) = FRAME_COUNTER.lock() {
                *c += 1;
                *c
            } else {
                0
            };
            let ts = uptime_millis();
            log::info!(target: "WarpVulkan", "present_ok frame={} ts={}", n, ts);
            log::warn!(target: "WarpVulkan", "swapchain suboptimal — recreating before next frame");
            if let Err(e) = unsafe { recreate_swapchain(s) } {
                log::error!(target: "WarpVulkan", "recreate after suboptimal failed: {:?}", e);
            }
            true
        }
        Ok(FrameOutcome::OutOfDate) => {
            log::warn!(target: "WarpVulkan", "swapchain out-of-date — recreating");
            if let Err(e) = unsafe { recreate_swapchain(s) } {
                log::error!(target: "WarpVulkan", "recreate failed: {:?}", e);
            }
            false
        }
        Err(e) => {
            log::error!(target: "WarpVulkan", "present failed: {:?}", e);
            false
        }
    }
}

#[allow(dead_code)]
pub(crate) fn frames_presented() -> u64 {
    FRAME_COUNTER.lock().map(|g| *g).unwrap_or(0)
}
