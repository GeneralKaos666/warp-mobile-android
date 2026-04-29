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
    native_window: *mut ndk_sys::ANativeWindow,
    device: ash::Device,
    swapchain_loader: ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    render_pass: vk::RenderPass,
    framebuffers: Vec<vk::Framebuffer>,
    image_views: Vec<vk::ImageView>,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    graphics_queue: vk::Queue,
    image_available: vk::Semaphore,
    render_finished: vk::Semaphore,
    fence: vk::Fence,
    #[cfg(feature = "validation-layers")]
    debug_utils_loader: ash::ext::debug_utils::Instance,
    #[cfg(feature = "validation-layers")]
    debug_messenger: vk::DebugUtilsMessengerEXT,
}

// Safety: ANativeWindow* is ref-counted and thread-safe per NDK contract.
// Vulkan handles are externally synchronized; Mutex guarantees single-threaded access.
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

#[cfg(feature = "validation-layers")]
unsafe extern "system" fn vulkan_debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    _message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _user_data: *mut std::ffi::c_void,
) -> vk::Bool32 {
    if !p_callback_data.is_null() {
        let msg = std::ffi::CStr::from_ptr((*p_callback_data).p_message);
        let text = msg.to_string_lossy();
        if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR) {
            log::error!("[VkVal] {}", text);
        } else if message_severity.contains(vk::DebugUtilsMessageSeverityFlagsEXT::WARNING) {
            log::warn!("[VkVal] {}", text);
        } else {
            log::debug!("[VkVal] {}", text);
        }
    }
    vk::FALSE
}

fn create_vulkan_instance(entry: &ash::Entry) -> Result<ash::Instance, vk::Result> {
    let app_info = vk::ApplicationInfo::default()
        .application_name(c"VulkanSpikeApp")
        .application_version(vk::make_api_version(0, 1, 0, 0))
        .engine_name(c"NoEngine")
        .engine_version(vk::make_api_version(0, 1, 0, 0))
        .api_version(vk::API_VERSION_1_1);

    #[cfg(not(feature = "validation-layers"))]
    let extension_names = vec![
        ash::khr::surface::NAME.as_ptr(),
        ash::khr::android_surface::NAME.as_ptr(),
    ];

    #[cfg(feature = "validation-layers")]
    let extension_names = vec![
        ash::khr::surface::NAME.as_ptr(),
        ash::khr::android_surface::NAME.as_ptr(),
        ash::ext::debug_utils::NAME.as_ptr(),
    ];

    #[cfg(not(feature = "validation-layers"))]
    let layer_names: Vec<*const u8> = vec![];

    #[cfg(feature = "validation-layers")]
    let layer_names = {
        let available = unsafe { entry.enumerate_instance_layer_properties() }
            .unwrap_or_default();
        let khronos_val = c"VK_LAYER_KHRONOS_validation";
        let found = available.iter().any(|l| {
            let name = unsafe { std::ffi::CStr::from_ptr(l.layer_name.as_ptr()) };
            name == khronos_val
        });
        if found {
            log::info!("VK_LAYER_KHRONOS_validation available — enabling");
            vec![khronos_val.as_ptr() as *const u8]
        } else {
            log::warn!("VK_LAYER_KHRONOS_validation not found on device — running without");
            vec![] as Vec<*const u8>
        }
    };

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extension_names)
        .enabled_layer_names(&layer_names);

    unsafe { entry.create_instance(&create_info, None) }
}

#[cfg(feature = "validation-layers")]
fn setup_debug_messenger(
    entry: &ash::Entry,
    instance: &ash::Instance,
) -> (ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT) {
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
    let messenger = unsafe { loader.create_debug_utils_messenger(&create_info, None) }
        .unwrap_or_else(|e| {
            log::warn!("Failed to create debug messenger: {:?}", e);
            vk::DebugUtilsMessengerEXT::null()
        });
    (loader, messenger)
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
    let queue_families = unsafe { instance.get_physical_device_queue_family_properties(phys_device) };
    queue_families.iter().enumerate().find_map(|(i, props)| {
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
    let devices = unsafe { instance.enumerate_physical_devices() }.unwrap_or_default();
    log::info!("physical_device_count={}", devices.len());
    for dev in &devices {
        let props = unsafe { instance.get_physical_device_properties(*dev) };
        let name = unsafe { std::ffi::CStr::from_ptr(props.device_name.as_ptr()) };
        let Some(qf) = find_graphics_queue_family(instance, *dev) else {
            log::info!("device={} no_graphics_queue", name.to_string_lossy());
            continue;
        };
        let supported = unsafe {
            surface_loader.get_physical_device_surface_support(*dev, qf, surface)
        }.unwrap_or(false);
        log::info!("device={} queue_family={} surface_support={}", name.to_string_lossy(), qf, supported);
        if supported {
            return Some((*dev, qf));
        }
    }
    log::error!("no_suitable_physical_device count={}", devices.len());
    None
}

unsafe fn create_device_and_swapchain(
    instance: &ash::Instance,
    surface_loader: &ash::khr::surface::Instance,
    phys_device: vk::PhysicalDevice,
    queue_family: u32,
    surface: vk::SurfaceKHR,
) -> Result<(ash::Device, ash::khr::swapchain::Device, vk::SwapchainKHR, vk::Format, vk::Extent2D, vk::Queue), vk::Result> {
    let queue_priorities = [1.0f32];
    let queue_create_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(queue_family)
        .queue_priorities(&queue_priorities);

    let device_extensions = [ash::khr::swapchain::NAME.as_ptr()];

    let device_create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_create_info))
        .enabled_extension_names(&device_extensions);

    let device = instance.create_device(phys_device, &device_create_info, None)?;
    let queue = device.get_device_queue(queue_family, 0);

    // Choose swapchain surface format
    let formats = surface_loader
        .get_physical_device_surface_formats(phys_device, surface)
        .unwrap_or_default();
    let format = formats
        .iter()
        .find(|f| {
            f.format == vk::Format::B8G8R8A8_UNORM
                || f.format == vk::Format::R8G8B8A8_UNORM
        })
        .or_else(|| formats.first())
        .copied()
        .unwrap_or(vk::SurfaceFormatKHR {
            format: vk::Format::B8G8R8A8_UNORM,
            color_space: vk::ColorSpaceKHR::SRGB_NONLINEAR,
        });

    // Choose present mode: prefer MAILBOX, fall back to FIFO
    let present_modes = surface_loader
        .get_physical_device_surface_present_modes(phys_device, surface)
        .unwrap_or_default();
    let present_mode = if present_modes.contains(&vk::PresentModeKHR::MAILBOX) {
        vk::PresentModeKHR::MAILBOX
    } else {
        vk::PresentModeKHR::FIFO
    };

    let caps = surface_loader.get_physical_device_surface_capabilities(phys_device, surface)?;
    let extent = if caps.current_extent.width != u32::MAX {
        caps.current_extent
    } else {
        vk::Extent2D { width: 1080, height: 2340 }
    };

    let image_count = (caps.min_image_count + 1)
        .min(if caps.max_image_count > 0 { caps.max_image_count } else { u32::MAX });

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
        .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
        .present_mode(present_mode)
        .clipped(true);

    let swapchain_loader = ash::khr::swapchain::Device::new(instance, &device);
    let swapchain = swapchain_loader.create_swapchain(&swapchain_create_info, None)?;

    Ok((device, swapchain_loader, swapchain, format.format, extent, queue))
}

unsafe fn create_render_pass(device: &ash::Device, format: vk::Format) -> Result<vk::RenderPass, vk::Result> {
    let attachment = vk::AttachmentDescription::default()
        .format(format)
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

    device.create_render_pass(&rp_info, None)
}

unsafe fn create_framebuffers(
    device: &ash::Device,
    swapchain_loader: &ash::khr::swapchain::Device,
    swapchain: vk::SwapchainKHR,
    render_pass: vk::RenderPass,
    format: vk::Format,
    extent: vk::Extent2D,
) -> Result<(Vec<vk::ImageView>, Vec<vk::Framebuffer>), vk::Result> {
    let images = swapchain_loader.get_swapchain_images(swapchain)?;
    let mut image_views = Vec::with_capacity(images.len());
    let mut framebuffers = Vec::with_capacity(images.len());

    for image in &images {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(*image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
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

        let attachments = [view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(extent.width)
            .height(extent.height)
            .layers(1);
        let fb = device.create_framebuffer(&fb_info, None)?;
        framebuffers.push(fb);
    }

    Ok((image_views, framebuffers))
}

unsafe fn record_and_present_first_frame(state: &SurfaceState, extent: vk::Extent2D) -> Result<(), vk::Result> {
    let device = &state.device;

    // Acquire image
    let (image_index, _suboptimal) = state.swapchain_loader.acquire_next_image(
        state.swapchain,
        u64::MAX,
        state.image_available,
        vk::Fence::null(),
    )?;

    let cmd_buf = state.command_buffers[image_index as usize];

    // Record clear-color command buffer
    let begin_info = vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    device.begin_command_buffer(cmd_buf, &begin_info)?;

    let clear_values = [vk::ClearValue {
        color: vk::ClearColorValue { float32: [0.1, 0.1, 0.2, 1.0] },
    }];
    let rp_begin = vk::RenderPassBeginInfo::default()
        .render_pass(state.render_pass)
        .framebuffer(state.framebuffers[image_index as usize])
        .render_area(vk::Rect2D { offset: vk::Offset2D::default(), extent })
        .clear_values(&clear_values);
    device.cmd_begin_render_pass(cmd_buf, &rp_begin, vk::SubpassContents::INLINE);
    device.cmd_end_render_pass(cmd_buf);
    device.end_command_buffer(cmd_buf)?;

    // Submit
    let wait_semaphores = [state.image_available];
    let signal_semaphores = [state.render_finished];
    let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
    let cmd_bufs = [cmd_buf];
    let submit_info = vk::SubmitInfo::default()
        .wait_semaphores(&wait_semaphores)
        .wait_dst_stage_mask(&wait_stages)
        .command_buffers(&cmd_bufs)
        .signal_semaphores(&signal_semaphores);
    device.queue_submit(state.graphics_queue, &[submit_info], state.fence)?;

    // Present
    let swapchains = [state.swapchain];
    let image_indices = [image_index];
    let present_info = vk::PresentInfoKHR::default()
        .wait_semaphores(&signal_semaphores)
        .swapchains(&swapchains)
        .image_indices(&image_indices);
    state.swapchain_loader.queue_present(state.graphics_queue, &present_info)?;

    // Wait idle so semaphores/fence are reset for next cycle
    device.queue_wait_idle(state.graphics_queue)?;
    device.reset_fences(&[state.fence])?;
    device.reset_command_pool(state.command_pool, vk::CommandPoolResetFlags::empty())?;

    Ok(())
}

fn destroy_state(s: SurfaceState) {
    unsafe {
        s.device.device_wait_idle().ok();

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
        #[cfg(feature = "validation-layers")]
        if s.debug_messenger != vk::DebugUtilsMessengerEXT::null() {
            s.debug_utils_loader.destroy_debug_utils_messenger(s.debug_messenger, None);
        }
        s.instance.destroy_instance(None);
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

    let native_window = unsafe {
        ndk_sys::ANativeWindow_fromSurface(
            env.get_native_interface() as *mut _,
            surface.as_raw() as *mut _,
        )
    };
    if native_window.is_null() {
        log::error!("ANativeWindow_fromSurface returned null");
        return;
    }
    log::debug!("ANativeWindow acquired: {:p}", native_window);

    let mut state_guard = SURFACE_STATE.lock().unwrap();
    if let Some(old) = state_guard.take() {
        destroy_state(old);
    }

    // --- Vulkan init ---
    let entry = match unsafe { ash::Entry::load() } {
        Ok(e) => e,
        Err(e) => {
            log::error!("Failed to load Vulkan: {:?}", e);
            unsafe { ndk_sys::ANativeWindow_release(native_window) };
            return;
        }
    };

    let instance = match create_vulkan_instance(&entry) {
        Ok(i) => i,
        Err(e) => {
            log::error!("Failed to create VkInstance: {:?}", e);
            unsafe { ndk_sys::ANativeWindow_release(native_window) };
            return;
        }
    };

    #[cfg(feature = "validation-layers")]
    let (debug_utils_loader, debug_messenger) = setup_debug_messenger(&entry, &instance);

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
                #[cfg(feature = "validation-layers")]
                if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }
                instance.destroy_instance(None);
                ndk_sys::ANativeWindow_release(native_window);
            }
            return;
        }
    };

    let surface_loader = ash::khr::surface::Instance::new(&entry, &instance);
    log::info!("surface_loader_ready selecting_physical_device");

    let (phys_device, queue_family) =
        match select_physical_device(&instance, &surface_loader, vk_surface) {
            Some(x) => x,
            None => {
                log::error!("No suitable physical device found");
                unsafe {
                    surface_loader.destroy_surface(vk_surface, None);
                    #[cfg(feature = "validation-layers")]
                    if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                        debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                    }
                    instance.destroy_instance(None);
                    ndk_sys::ANativeWindow_release(native_window);
                }
                return;
            }
        };

    let (device, swapchain_loader, swapchain, format, extent, graphics_queue) = match unsafe {
        create_device_and_swapchain(&instance, &surface_loader, phys_device, queue_family, vk_surface)
    } {
        Ok(x) => x,
        Err(e) => {
            log::error!("Device/swapchain creation failed: {:?}", e);
            unsafe {
                surface_loader.destroy_surface(vk_surface, None);
                #[cfg(feature = "validation-layers")]
                if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }
                instance.destroy_instance(None);
                ndk_sys::ANativeWindow_release(native_window);
            }
            return;
        }
    };

    let render_pass = match unsafe { create_render_pass(&device, format) } {
        Ok(rp) => rp,
        Err(e) => {
            log::error!("RenderPass creation failed: {:?}", e);
            unsafe {
                swapchain_loader.destroy_swapchain(swapchain, None);
                device.destroy_device(None);
                surface_loader.destroy_surface(vk_surface, None);
                #[cfg(feature = "validation-layers")]
                if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }
                instance.destroy_instance(None);
                ndk_sys::ANativeWindow_release(native_window);
            }
            return;
        }
    };

    let (image_views, framebuffers) = match unsafe {
        create_framebuffers(&device, &swapchain_loader, swapchain, render_pass, format, extent)
    } {
        Ok(x) => x,
        Err(e) => {
            log::error!("Framebuffer creation failed: {:?}", e);
            unsafe {
                device.destroy_render_pass(render_pass, None);
                swapchain_loader.destroy_swapchain(swapchain, None);
                device.destroy_device(None);
                surface_loader.destroy_surface(vk_surface, None);
                #[cfg(feature = "validation-layers")]
                if debug_messenger != vk::DebugUtilsMessengerEXT::null() {
                    debug_utils_loader.destroy_debug_utils_messenger(debug_messenger, None);
                }
                instance.destroy_instance(None);
                ndk_sys::ANativeWindow_release(native_window);
            }
            return;
        }
    };

    // Command pool + buffers (one per swapchain image)
    let pool_info = vk::CommandPoolCreateInfo::default()
        .queue_family_index(queue_family)
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER);
    let command_pool = match unsafe { device.create_command_pool(&pool_info, None) } {
        Ok(p) => p,
        Err(e) => {
            log::error!("CommandPool creation failed: {:?}", e);
            return;
        }
    };

    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(framebuffers.len() as u32);
    let command_buffers = match unsafe { device.allocate_command_buffers(&alloc_info) } {
        Ok(b) => b,
        Err(e) => {
            log::error!("CommandBuffer alloc failed: {:?}", e);
            unsafe { device.destroy_command_pool(command_pool, None) };
            return;
        }
    };

    // Synchronization primitives
    let sem_info = vk::SemaphoreCreateInfo::default();
    let fence_info = vk::FenceCreateInfo::default();
    let (image_available, render_finished, fence) = unsafe {
        match (
            device.create_semaphore(&sem_info, None),
            device.create_semaphore(&sem_info, None),
            device.create_fence(&fence_info, None),
        ) {
            (Ok(a), Ok(b), Ok(f)) => (a, b, f),
            _ => {
                log::error!("Sync primitive creation failed");
                device.destroy_command_pool(command_pool, None);
                return;
            }
        }
    };

    let new_state = SurfaceState {
        entry,
        instance,
        surface_loader,
        surface: vk_surface,
        native_window,
        device,
        swapchain_loader,
        swapchain,
        render_pass,
        framebuffers,
        image_views,
        command_pool,
        command_buffers,
        graphics_queue,
        image_available,
        render_finished,
        fence,
        #[cfg(feature = "validation-layers")]
        debug_utils_loader,
        #[cfg(feature = "validation-layers")]
        debug_messenger,
    };

    // Present first frame and record metric
    match unsafe { record_and_present_first_frame(&new_state, extent) } {
        Ok(()) => {
            let frame_ts = uptime_millis();
            log::info!("first_frame_presented_ts={}", frame_ts);
        }
        Err(e) => {
            log::error!("First frame present failed: {:?}", e);
        }
    }

    *state_guard = Some(new_state);
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
