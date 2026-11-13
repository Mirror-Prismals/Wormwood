//! A Vulkan surface/swapchain renderer. One frame in flight for diagnostics.
use super::{vk::*, window::Window};
use serde_json::{Value, json};
use std::{ffi::CStr, path::PathBuf, ptr};
fn check(code: VkResult, operation: &str) -> Result<(), String> {
    if code == VK_SUCCESS {
        Ok(())
    } else {
        Err(format!("{operation}: VkResult {code}"))
    }
}
pub(super) struct Client {
    pub window: Window,
    water: Option<super::water::vulkan::Client>,
    water_frames: Vec<super::water::data::DrawFrame>,
    reflection_vegetation: Option<super::vegetation::vulkan::Client>,
    reflection_plants_active: bool,
    vegetation: Option<super::vegetation::vulkan::Client>,
    vegetation_active: bool,
    memory_properties: VkPhysicalDeviceMemoryProperties,
    api: Api,
    instance: VkInstance,
    surface: VkSurfaceKHR,
    device: VkDevice,
    queue: VkQueue,
    swapchain: VkSwapchainKHR,
    views: Vec<VkImageView>,
    render_pass: VkRenderPass,
    layout: VkPipelineLayout,
    pipeline: VkPipeline,
    world_pipeline:VkPipeline,
    overlay_pass:VkRenderPass,
    world_active:bool,
    world_buffer:VkBuffer,
    world_memory:VkDeviceMemory,
    world_pool:VkDescriptorPool,
    world_set:VkDescriptorSet,
    texture_layout: VkDescriptorSetLayout,
    texture_pool: VkDescriptorPool,
    texture_set: VkDescriptorSet,
    texture_buffer: VkBuffer,
    texture_memory: VkDeviceMemory,
    texture_capacity: usize,
    vertex_buffer: VkBuffer,
    vertex_memory: VkDeviceMemory,
    vertices: Vec<f32>,
    vertex_count: u32,
    recorded: Option<(u32, [f32; 4], bool)>,
    framebuffers: Vec<VkFramebuffer>,
    pool: VkCommandPool,
    commands: Vec<VkCommandBuffer>,
    timestamp_pool: VkQueryPool,
    timestamp_period: f64,
    timestamp_mask: u64,
    gpu_log: Option<super::performance::GpuLog>,
    gpu_serial: u64,
    images: Vec<VkImage>,
    available: VkSemaphore,
    rendered: VkSemaphore,
    read_buffer: VkBuffer,
    read_memory: VkDeviceMemory,
    extent: VkExtent2D,
    format: VkFormat,
    depth: VkImage,
    depth_memory: VkDeviceMemory,
    depth_view: VkImageView,
    pub(super) captured: bool,
    pub(super) monitor_capture: Option<Vec<u8>>,
    pub(super) validate_triangle_center: bool,
    pub(super) clear: [f32; 4],
    capture: PathBuf,
    pub info: Value,
}
impl Client {
    pub unsafe fn new(
        data: &[f32; 24],
        _root: &std::path::Path,
        output: &std::path::Path,
        workstation: bool,
    ) -> Result<Self, String> {
        #[cfg(not(target_os = "ios"))]
        let path = std::env::var("WORMWOOD_VULKAN_LIBRARY").unwrap_or_else(|_| {
            if cfg!(any(target_os = "macos", target_os = "ios")) {
                "/opt/homebrew/lib/libvulkan.dylib"
            } else if cfg!(target_os = "windows") {
                "vulkan-1.dll"
            } else {
                "libvulkan.so.1"
            }
            .into()
        });
        #[cfg(not(target_os = "ios"))]
        let api = Api::new(&path).map_err(|e| e.to_string())?;
        #[cfg(target_os = "ios")]
        let path = "linked MoltenVK 1.4.2";
        #[cfg(target_os = "ios")]
        let api =
            Api::from_library(libloading::os::unix::Library::this()).map_err(|e| e.to_string())?;
        let window = Window::with_loader(
            false,
            c"Wormwood — Vulkan",
            Some(api.vkGetInstanceProcAddr),
            workstation,
        )?;
        let mut c = Self {
            window,
            water: None,
            water_frames: Vec::new(),
            reflection_vegetation: None,
            reflection_plants_active: false,
            vegetation: None,
            vegetation_active: false,
            memory_properties: Default::default(),
            api,
            instance: ptr::null_mut(),
            surface: ptr::null_mut(),
            device: ptr::null_mut(),
            queue: ptr::null_mut(),
            swapchain: ptr::null_mut(),
            views: vec![],
            render_pass: ptr::null_mut(),
            layout: ptr::null_mut(),
            pipeline: ptr::null_mut(),
            world_pipeline:ptr::null_mut(),overlay_pass:ptr::null_mut(),world_active:false,
            world_buffer:ptr::null_mut(),world_memory:ptr::null_mut(),world_pool:ptr::null_mut(),world_set:ptr::null_mut(),
            texture_layout: ptr::null_mut(),
            texture_pool: ptr::null_mut(),
            texture_set: ptr::null_mut(),
            texture_buffer: ptr::null_mut(),
            texture_memory: ptr::null_mut(),
            texture_capacity: if workstation {
                crate::scene_assets::SCENE_BYTES.max(super::workstation::STORAGE_BYTES + crate::scene_assets::TABLE.len() + crate::scene_assets::FINISHES.len())
            } else {
                crate::scene_assets::SCENE_BYTES
            },
            vertex_buffer: ptr::null_mut(),
            vertex_memory: ptr::null_mut(),
            vertices: Vec::new(),
            vertex_count: 0,
            recorded: None,
            framebuffers: vec![],
            pool: ptr::null_mut(),
            commands: vec![],
            timestamp_pool: ptr::null_mut(),
            timestamp_period: 0.,
            timestamp_mask: 0,
            gpu_log: None,
            gpu_serial: 0,
            images: Vec::new(),
            available: ptr::null_mut(),
            rendered: ptr::null_mut(),
            read_buffer: ptr::null_mut(),
            read_memory: ptr::null_mut(),
            extent: Default::default(),
            format: 0,
            depth: ptr::null_mut(),
            depth_memory: ptr::null_mut(),
            depth_view: ptr::null_mut(),
            captured: false,
            monitor_capture: None,
            validate_triangle_center: true,
            clear: [0.035, 0.045, 0.065, 1.],
            capture: output.join("triangle-Vulkan.ppm"),
            info: Value::Null,
        };
        #[cfg(not(target_os = "ios"))]
        let mut extensions = {
            let required = c
                .window
                .symbol::<unsafe extern "C" fn(*mut u32) -> *const *const i8>(
                    b"glfwGetRequiredInstanceExtensions\0",
                )?;
            let mut n = 0;
            let names = required(&mut n);
            if names.is_null() || n == 0 {
                return Err("GLFW has no Vulkan surface extensions".into());
            }
            std::slice::from_raw_parts(names, n as usize).to_vec()
        };
        #[cfg(target_os = "ios")]
        let mut extensions = vec![c"VK_KHR_surface".as_ptr(), c"VK_EXT_metal_surface".as_ptr()];
        // The desktop Vulkan loader and directly linked MoltenVK expose
        // different extension sets. Portability enumeration belongs to the
        // loader and must not be unconditionally requested on iPhone.
        let mut extension_count = 0;
        check(
            (c.api.vkEnumerateInstanceExtensionProperties)(
                ptr::null(),
                &mut extension_count,
                ptr::null_mut(),
            ),
            "enumerate instance extensions",
        )?;
        if extension_count > 4096 {
            return Err("Vulkan instance extension count exceeds bound".into());
        }
        let mut available = vec![VkExtensionProperties::default(); extension_count as usize];
        check(
            (c.api.vkEnumerateInstanceExtensionProperties)(
                ptr::null(),
                &mut extension_count,
                available.as_mut_ptr(),
            ),
            "enumerate instance extensions",
        )?;
        available.truncate(extension_count as usize);
        let supports = |name: &CStr| {
            available
                .iter()
                .any(|e| CStr::from_ptr(e.extensionName.as_ptr()) == name)
        };
        for name in &extensions {
            let name = CStr::from_ptr(*name);
            if !supports(name) {
                return Err(format!(
                    "required Vulkan surface extension unavailable: {}",
                    name.to_string_lossy()
                ));
            }
        }
        let portability = cfg!(any(target_os = "macos", target_os = "ios"))
            && supports(c"VK_KHR_portability_enumeration");
        if portability {
            extensions.push(c"VK_KHR_portability_enumeration".as_ptr());
        }
        if cfg!(any(target_os = "macos", target_os = "ios"))
            && supports(c"VK_KHR_get_physical_device_properties2")
        {
            extensions.push(c"VK_KHR_get_physical_device_properties2".as_ptr());
        }
        let extension_names: Vec<String> = extensions
            .iter()
            .map(|p| CStr::from_ptr(*p).to_string_lossy().into_owned())
            .collect();
        let app = VkApplicationInfo {
            sType: VK_STRUCTURE_TYPE_APPLICATION_INFO,
            pApplicationName: c"Wormwood".as_ptr(),
            pEngineName: c"Wormwood".as_ptr(),
            apiVersion: 1 << 22,
            ..Default::default()
        };
        let create = VkInstanceCreateInfo {
            sType: VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
            pApplicationInfo: &app,
            flags: if portability {
                VK_INSTANCE_CREATE_ENUMERATE_PORTABILITY_BIT_KHR
            } else {
                0
            },
            enabledExtensionCount: extensions.len() as u32,
            ppEnabledExtensionNames: extensions.as_ptr(),
            ..Default::default()
        };
        check(
            (c.api.vkCreateInstance)(&create, ptr::null(), &mut c.instance),
            &format!(
                "vkCreateInstance (requested extensions: {})",
                extension_names.join(", ")
            ),
        )?;
        #[cfg(not(target_os = "ios"))]
        {
            let surface = c.window.symbol::<unsafe extern "C" fn(
                VkInstance,
                *mut std::ffi::c_void,
                *const VkAllocationCallbacks,
                *mut VkSurfaceKHR,
            ) -> VkResult>(b"glfwCreateWindowSurface\0")?;
            check(
                surface(c.instance, c.window.handle, ptr::null(), &mut c.surface),
                "glfwCreateWindowSurface",
            )?;
        }
        #[cfg(target_os = "ios")]
        {
            let create = VkMetalSurfaceCreateInfoEXT {
                sType: VK_STRUCTURE_TYPE_METAL_SURFACE_CREATE_INFO_EXT,
                pLayer: c.window.metal_layer().cast(),
                ..Default::default()
            };
            check(
                (c.api.vkCreateMetalSurfaceEXT)(c.instance, &create, ptr::null(), &mut c.surface),
                "vkCreateMetalSurfaceEXT",
            )?;
        }
        let mut count = 0;
        check(
            (c.api.vkEnumeratePhysicalDevices)(c.instance, &mut count, ptr::null_mut()),
            "enumerate physical devices",
        )?;
        if count == 0 {
            return Err("no Vulkan physical devices".into());
        }
        let mut physical = vec![ptr::null_mut(); count as usize];
        check(
            (c.api.vkEnumeratePhysicalDevices)(c.instance, &mut count, physical.as_mut_ptr()),
            "enumerate physical devices",
        )?;
        let mut chosen = None;
        for device in physical {
            let mut count = 0;
            (c.api.vkGetPhysicalDeviceQueueFamilyProperties)(device, &mut count, ptr::null_mut());
            let mut families = vec![VkQueueFamilyProperties::default(); count as usize];
            (c.api.vkGetPhysicalDeviceQueueFamilyProperties)(
                device,
                &mut count,
                families.as_mut_ptr(),
            );
            for (index, family) in families.iter().enumerate() {
                let mut supported = 0;
                check(
                    (c.api.vkGetPhysicalDeviceSurfaceSupportKHR)(
                        device,
                        index as u32,
                        c.surface,
                        &mut supported,
                    ),
                    "surface support",
                )?;
                if family.queueFlags & VK_QUEUE_GRAPHICS_BIT != 0 && supported != 0 {
                    chosen = Some((device, index as u32, family.timestampValidBits));
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        let (physical, family, timestamp_bits) =
            chosen.ok_or("no graphics queue supporting presentation")?;
        let mut properties = VkPhysicalDeviceProperties::default();
        (c.api.vkGetPhysicalDeviceProperties)(physical, &mut properties);
        let mut count = 0;
        check(
            (c.api.vkEnumerateDeviceExtensionProperties)(
                physical,
                ptr::null(),
                &mut count,
                ptr::null_mut(),
            ),
            "device extensions",
        )?;
        let mut available = vec![VkExtensionProperties::default(); count as usize];
        check(
            (c.api.vkEnumerateDeviceExtensionProperties)(
                physical,
                ptr::null(),
                &mut count,
                available.as_mut_ptr(),
            ),
            "device extensions",
        )?;
        let mut device_extensions = vec![c"VK_KHR_swapchain".as_ptr()];
        if available.iter().any(|e| {
            CStr::from_ptr(e.extensionName.as_ptr()).to_bytes() == b"VK_KHR_portability_subset"
        }) {
            device_extensions.push(c"VK_KHR_portability_subset".as_ptr());
        }
        let priority = 1.0;
        let queues = VkDeviceQueueCreateInfo {
            sType: VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
            queueFamilyIndex: family,
            queueCount: 1,
            pQueuePriorities: &priority,
            ..Default::default()
        };
        let create = VkDeviceCreateInfo {
            sType: VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
            queueCreateInfoCount: 1,
            pQueueCreateInfos: &queues,
            enabledExtensionCount: device_extensions.len() as u32,
            ppEnabledExtensionNames: device_extensions.as_ptr(),
            ..Default::default()
        };
        check(
            (c.api.vkCreateDevice)(physical, &create, ptr::null(), &mut c.device),
            "vkCreateDevice",
        )?;
        (c.api.vkGetDeviceQueue)(c.device, family, 0, &mut c.queue);
        if super::performance::enabled() {
            let period = f64::from(properties.limits.timestampPeriod);
            if (1..=64).contains(&timestamp_bits) && period.is_finite() && period > 0. {
                let create = VkQueryPoolCreateInfo {
                    sType: VK_STRUCTURE_TYPE_QUERY_POOL_CREATE_INFO,
                    queryType: VK_QUERY_TYPE_TIMESTAMP,
                    queryCount: 2,
                    ..Default::default()
                };
                let mut pool = ptr::null_mut();
                if (c.api.vkCreateQueryPool)(c.device, &create, ptr::null(), &mut pool)
                    == VK_SUCCESS
                {
                    c.timestamp_pool = pool;
                    c.timestamp_period = period;
                    c.timestamp_mask = if timestamp_bits == 64 {
                        u64::MAX
                    } else {
                        (1u64 << timestamp_bits) - 1
                    };
                }
            }
            c.gpu_log = Some(super::performance::GpuLog::new(
                output,
                "Vulkan",
                workstation,
                !c.timestamp_pool.is_null(),
                "synchronous_timestamp",
                if cfg!(target_os = "macos") {
                    "Vulkan TOP/BOTTOM timestamp interval around graphics commands, before screenshot copy; MoltenVK counter boundaries are not independently validated as render-pass boundaries."
                } else {
                    "Graphics commands including shadows, reflection and water, before screenshot copy; excludes presentation, includes acquire synchronization"
                },
            ));
            if let Some(log) = &mut c.gpu_log {
                log.set_query_context(json!({
                    "timestamp_period_ns":c.timestamp_period,
                    "timestamp_valid_bits":timestamp_bits,
                    "timestamp_mask":c.timestamp_mask,
                    "device":CStr::from_ptr(properties.deviceName.as_ptr()).to_string_lossy(),
                    "vendor_id":properties.vendorID,"device_id":properties.deviceID,
                    "driver_version":properties.driverVersion,"api_version":properties.apiVersion,
                    "queue_family":family,
                    "requested_begin_stage":"TOP_OF_PIPE","requested_end_stage":"BOTTOM_OF_PIPE",
                    "interpretation":"Raw GPU query values; a short interval is retained without substituting CPU acquire, submission, presentation or idle time."
                }));
            }
        }
        let mut caps = VkSurfaceCapabilitiesKHR::default();
        check(
            (c.api.vkGetPhysicalDeviceSurfaceCapabilitiesKHR)(physical, c.surface, &mut caps),
            "surface capabilities",
        )?;
        let (w, h) = c.window.dimensions()?;
        c.extent = if caps.currentExtent.width != u32::MAX {
            caps.currentExtent
        } else {
            VkExtent2D {
                width: w.clamp(caps.minImageExtent.width, caps.maxImageExtent.width),
                height: h.clamp(caps.minImageExtent.height, caps.maxImageExtent.height),
            }
        };
        let mut count = 0;
        check(
            (c.api.vkGetPhysicalDeviceSurfaceFormatsKHR)(
                physical,
                c.surface,
                &mut count,
                ptr::null_mut(),
            ),
            "surface formats",
        )?;
        if count == 0 {
            return Err("no Vulkan surface formats".into());
        }
        let mut formats = vec![VkSurfaceFormatKHR::default(); count as usize];
        check(
            (c.api.vkGetPhysicalDeviceSurfaceFormatsKHR)(
                physical,
                c.surface,
                &mut count,
                formats.as_mut_ptr(),
            ),
            "surface formats",
        )?;
        let format = *formats
            .iter()
            .find(|f| {
                matches!(
                    f.format,
                    VK_FORMAT_B8G8R8A8_UNORM | VK_FORMAT_R8G8B8A8_UNORM
                )
            })
            .ok_or("no supported 8-bit readback surface format")?;
        c.format = format.format;
        let usage = VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
        if caps.supportedUsageFlags & usage != usage {
            return Err("surface cannot support required color/readback usage".into());
        }
        let mut image_count = caps.minImageCount + 1;
        if caps.maxImageCount != 0 {
            image_count = image_count.min(caps.maxImageCount);
        }
        let swap = VkSwapchainCreateInfoKHR {
            sType: VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR,
            surface: c.surface,
            minImageCount: image_count,
            imageFormat: format.format,
            imageColorSpace: format.colorSpace,
            imageExtent: c.extent,
            imageArrayLayers: 1,
            imageUsage: usage,
            imageSharingMode: VK_SHARING_MODE_EXCLUSIVE,
            preTransform: caps.currentTransform,
            compositeAlpha: 1 << caps.supportedCompositeAlpha.trailing_zeros(),
            presentMode: VK_PRESENT_MODE_FIFO_KHR,
            clipped: 1,
            ..Default::default()
        };
        check(
            (c.api.vkCreateSwapchainKHR)(c.device, &swap, ptr::null(), &mut c.swapchain),
            "create swapchain",
        )?;
        let mut count = 0;
        check(
            (c.api.vkGetSwapchainImagesKHR)(c.device, c.swapchain, &mut count, ptr::null_mut()),
            "swapchain images",
        )?;
        let mut images = vec![ptr::null_mut(); count as usize];
        check(
            (c.api.vkGetSwapchainImagesKHR)(c.device, c.swapchain, &mut count, images.as_mut_ptr()),
            "swapchain images",
        )?;
        for image in &images {
            let create = VkImageViewCreateInfo {
                sType: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                image: *image,
                viewType: VK_IMAGE_VIEW_TYPE_2D,
                format: format.format,
                subresourceRange: VkImageSubresourceRange {
                    aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
                    levelCount: 1,
                    layerCount: 1,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut view = ptr::null_mut();
            check(
                (c.api.vkCreateImageView)(c.device, &create, ptr::null(), &mut view),
                "create image view",
            )?;
            c.views.push(view);
        }
        let depth_info = VkImageCreateInfo {
            sType: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
            imageType: VK_IMAGE_TYPE_2D,
            format: VK_FORMAT_D32_SFLOAT,
            extent: VkExtent3D {
                width: c.extent.width,
                height: c.extent.height,
                depth: 1,
            },
            mipLevels: 1,
            arrayLayers: 1,
            samples: VK_SAMPLE_COUNT_1_BIT,
            tiling: VK_IMAGE_TILING_OPTIMAL,
            usage: VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
            ..Default::default()
        };
        check(
            (c.api.vkCreateImage)(c.device, &depth_info, ptr::null(), &mut c.depth),
            "depth image",
        )?;
        let mut depth_requirements = VkMemoryRequirements::default();
        (c.api.vkGetImageMemoryRequirements)(c.device, c.depth, &mut depth_requirements);
        let mut depth_properties = VkPhysicalDeviceMemoryProperties::default();
        (c.api.vkGetPhysicalDeviceMemoryProperties)(physical, &mut depth_properties);
        c.memory_properties = depth_properties;
        let depth_type = (0..depth_properties.memoryTypeCount)
            .find(|i| {
                depth_requirements.memoryTypeBits & (1 << i) != 0
                    && depth_properties.memoryTypes[*i as usize].propertyFlags
                        & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT
                        != 0
            })
            .ok_or("no depth device memory")?;
        let depth_alloc = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            allocationSize: depth_requirements.size,
            memoryTypeIndex: depth_type,
            ..Default::default()
        };
        check(
            (c.api.vkAllocateMemory)(c.device, &depth_alloc, ptr::null(), &mut c.depth_memory),
            "depth allocation",
        )?;
        check(
            (c.api.vkBindImageMemory)(c.device, c.depth, c.depth_memory, 0),
            "bind depth",
        )?;
        let depth_view = VkImageViewCreateInfo {
            sType: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
            image: c.depth,
            viewType: VK_IMAGE_VIEW_TYPE_2D,
            format: VK_FORMAT_D32_SFLOAT,
            subresourceRange: VkImageSubresourceRange {
                aspectMask: VK_IMAGE_ASPECT_DEPTH_BIT,
                levelCount: 1,
                layerCount: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        check(
            (c.api.vkCreateImageView)(c.device, &depth_view, ptr::null(), &mut c.depth_view),
            "depth view",
        )?;
        let color = VkAttachmentDescription {
            format: format.format,
            samples: VK_SAMPLE_COUNT_1_BIT,
            loadOp: VK_ATTACHMENT_LOAD_OP_CLEAR,
            storeOp: VK_ATTACHMENT_STORE_OP_STORE,
            stencilLoadOp: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencilStoreOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
            finalLayout: VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
            ..Default::default()
        };
        let reference = VkAttachmentReference {
            attachment: 0,
            layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let depth_reference = VkAttachmentReference {
            attachment: 1,
            layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let depth_attachment = VkAttachmentDescription {
            format: VK_FORMAT_D32_SFLOAT,
            samples: VK_SAMPLE_COUNT_1_BIT,
            loadOp: VK_ATTACHMENT_LOAD_OP_CLEAR,
            storeOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            stencilLoadOp: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencilStoreOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
            finalLayout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
            ..Default::default()
        };
        let attachments = [color, depth_attachment];
        let subpass = VkSubpassDescription {
            pipelineBindPoint: VK_PIPELINE_BIND_POINT_GRAPHICS,
            colorAttachmentCount: 1,
            pColorAttachments: &reference,
            pDepthStencilAttachment: &depth_reference,
            ..Default::default()
        };
        let dependencies = [
            VkSubpassDependency {
                srcSubpass: VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                dstStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                dstAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT
                    | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                ..Default::default()
            },
            VkSubpassDependency {
                srcSubpass: 0,
                dstSubpass: VK_SUBPASS_EXTERNAL as u32,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                dstStageMask: VK_PIPELINE_STAGE_TRANSFER_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                dstAccessMask: VK_ACCESS_TRANSFER_READ_BIT,
                ..Default::default()
            },
        ];
        let pass = VkRenderPassCreateInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            attachmentCount: 2,
            pAttachments: attachments.as_ptr(),
            subpassCount: 1,
            pSubpasses: &subpass,
            dependencyCount: 2,
            pDependencies: dependencies.as_ptr(),
            ..Default::default()
        };
        check(
            (c.api.vkCreateRenderPass)(c.device, &pass, ptr::null(), &mut c.render_pass),
            "create render pass",
        )?;
        if workstation {
            let overlay_attachments=[VkAttachmentDescription{loadOp:VK_ATTACHMENT_LOAD_OP_LOAD,initialLayout:VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,..color},depth_attachment];
            let mut overlay_dependencies=dependencies;
            overlay_dependencies[0].srcAccessMask=VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT;
            overlay_dependencies[0].dstAccessMask|=VK_ACCESS_COLOR_ATTACHMENT_READ_BIT;
            let overlay=VkRenderPassCreateInfo{pAttachments:overlay_attachments.as_ptr(),pDependencies:overlay_dependencies.as_ptr(),..pass};
            check((c.api.vkCreateRenderPass)(c.device,&overlay,ptr::null(),&mut c.overlay_pass),"create world overlay pass")?;
        }
        let bindings = [
            VkDescriptorSetLayoutBinding {
                binding: 0,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                descriptorCount: 1,
                stageFlags: VK_SHADER_STAGE_FRAGMENT_BIT,
                ..Default::default()
            },
            VkDescriptorSetLayoutBinding {
                binding: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                descriptorCount: 1,
                stageFlags: VK_SHADER_STAGE_VERTEX_BIT,
                ..Default::default()
            },
        ];
        let descriptor = VkDescriptorSetLayoutCreateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            bindingCount: bindings.len() as u32,
            pBindings: bindings.as_ptr(),
            ..Default::default()
        };
        check(
            (c.api.vkCreateDescriptorSetLayout)(
                c.device,
                &descriptor,
                ptr::null(),
                &mut c.texture_layout,
            ),
            "scene/texture layout",
        )?;
        let layout = VkPipelineLayoutCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
            setLayoutCount: 1,
            pSetLayouts: &c.texture_layout,
            ..Default::default()
        };
        check(
            (c.api.vkCreatePipelineLayout)(c.device, &layout, ptr::null(), &mut c.layout),
            "pipeline layout",
        )?;
        let vertex_source: &[u8] = if workstation {
            include_bytes!(concat!(env!("OUT_DIR"), "/workstation.vert.spv"))
        } else {
            include_bytes!(concat!(env!("OUT_DIR"), "/triangle.vert.spv"))
        };
        let styled_fragment = if workstation {
            crate::workstation::style_shader::vulkan_fragment(_root)?
        } else {
            None
        };
        let fragment_source: &[u8] = if let Some(source) = styled_fragment.as_deref() {
            source
        } else if workstation {
            include_bytes!(concat!(env!("OUT_DIR"), "/workstation.frag.spv"))
        } else {
            include_bytes!(concat!(env!("OUT_DIR"), "/triangle.frag.spv"))
        };
        let vertex = c.shader(vertex_source)?;
        let fragment = match c.shader(fragment_source) {
            Ok(s) => s,
            Err(e) => {
                (c.api.vkDestroyShaderModule)(c.device, vertex, ptr::null());
                return Err(e);
            }
        };
        let stages = [
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_VERTEX_BIT,
                module: vertex,
                pName: c"main".as_ptr(),
                ..Default::default()
            },
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_FRAGMENT_BIT,
                module: fragment,
                pName: c"main".as_ptr(),
                ..Default::default()
            },
        ];
        let vertex_input = VkPipelineVertexInputStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            ..Default::default()
        };
        let assembly = VkPipelineInputAssemblyStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
            topology: VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..Default::default()
        };
        let viewport = VkViewport {
            x: 0.0,
            y: 0.0,
            width: c.extent.width as f32,
            height: c.extent.height as f32,
            minDepth: 0.0,
            maxDepth: 1.0,
        };
        let scissor = VkRect2D {
            offset: Default::default(),
            extent: c.extent,
        };
        let viewport_state = VkPipelineViewportStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
            viewportCount: 1,
            pViewports: &viewport,
            scissorCount: 1,
            pScissors: &scissor,
            ..Default::default()
        };
        let raster = VkPipelineRasterizationStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
            polygonMode: VK_POLYGON_MODE_FILL,
            cullMode: VK_CULL_MODE_NONE,
            frontFace: VK_FRONT_FACE_COUNTER_CLOCKWISE,
            lineWidth: 1.0,
            ..Default::default()
        };
        let samples = VkPipelineMultisampleStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
            rasterizationSamples: VK_SAMPLE_COUNT_1_BIT,
            ..Default::default()
        };
        let attachment = VkPipelineColorBlendAttachmentState {
            blendEnable: 1,
            srcColorBlendFactor: VK_BLEND_FACTOR_SRC_ALPHA,
            dstColorBlendFactor: VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
            colorBlendOp: VK_BLEND_OP_ADD,
            srcAlphaBlendFactor: VK_BLEND_FACTOR_ONE,
            dstAlphaBlendFactor: VK_BLEND_FACTOR_ONE_MINUS_SRC_ALPHA,
            alphaBlendOp: VK_BLEND_OP_ADD,
            colorWriteMask: VK_COLOR_COMPONENT_R_BIT
                | VK_COLOR_COMPONENT_G_BIT
                | VK_COLOR_COMPONENT_B_BIT
                | VK_COLOR_COMPONENT_A_BIT,
            ..Default::default()
        };
        let blend = VkPipelineColorBlendStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_COLOR_BLEND_STATE_CREATE_INFO,
            attachmentCount: 1,
            pAttachments: &attachment,
            ..Default::default()
        };
        let depth_state = VkPipelineDepthStencilStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
            depthTestEnable: 1,
            depthWriteEnable: 1,
            depthCompareOp: VK_COMPARE_OP_LESS,
            ..Default::default()
        };
        let pipeline = VkGraphicsPipelineCreateInfo {
            sType: VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
            stageCount: 2,
            pStages: stages.as_ptr(),
            pVertexInputState: &vertex_input,
            pInputAssemblyState: &assembly,
            pViewportState: &viewport_state,
            pRasterizationState: &raster,
            pMultisampleState: &samples,
            pColorBlendState: &blend,
            pDepthStencilState: &depth_state,
            layout: c.layout,
            renderPass: c.render_pass,
            ..Default::default()
        };
        let result = (c.api.vkCreateGraphicsPipelines)(
            c.device,
            ptr::null_mut(),
            1,
            &pipeline,
            ptr::null(),
            &mut c.pipeline,
        );
        (c.api.vkDestroyShaderModule)(c.device, vertex, ptr::null());
        (c.api.vkDestroyShaderModule)(c.device, fragment, ptr::null());
        check(result, "create graphics pipeline")?;
        if workstation {
            let world_vertex=c.shader(include_bytes!(concat!(env!("OUT_DIR"),"/triangle.vert.spv")))?;
            let world_fragment=c.shader(include_bytes!(concat!(env!("OUT_DIR"),"/triangle.frag.spv")))?;
            let world_stages=[VkPipelineShaderStageCreateInfo{module:world_vertex,..stages[0]},VkPipelineShaderStageCreateInfo{module:world_fragment,..stages[1]}];
            let world=VkGraphicsPipelineCreateInfo{pStages:world_stages.as_ptr(),..pipeline};
            let result=(c.api.vkCreateGraphicsPipelines)(c.device,ptr::null_mut(),1,&world,ptr::null(),&mut c.world_pipeline);
            (c.api.vkDestroyShaderModule)(c.device,world_vertex,ptr::null());(c.api.vkDestroyShaderModule)(c.device,world_fragment,ptr::null());
            check(result,"create live world pipeline")?;
        }
        for view in &c.views {
            let fb_attachments = [*view, c.depth_view];
            let fb = VkFramebufferCreateInfo {
                sType: VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
                renderPass: c.render_pass,
                attachmentCount: 2,
                pAttachments: fb_attachments.as_ptr(),
                width: c.extent.width,
                height: c.extent.height,
                layers: 1,
                ..Default::default()
            };
            let mut framebuffer = ptr::null_mut();
            check(
                (c.api.vkCreateFramebuffer)(c.device, &fb, ptr::null(), &mut framebuffer),
                "framebuffer",
            )?;
            c.framebuffers.push(framebuffer);
        }
        let buffer = VkBufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            size: c.extent.width as u64 * c.extent.height as u64 * 4,
            usage: VK_BUFFER_USAGE_TRANSFER_DST_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            ..Default::default()
        };
        check(
            (c.api.vkCreateBuffer)(c.device, &buffer, ptr::null(), &mut c.read_buffer),
            "readback buffer",
        )?;
        let mut requirements = VkMemoryRequirements::default();
        (c.api.vkGetBufferMemoryRequirements)(c.device, c.read_buffer, &mut requirements);
        let mut memory = VkPhysicalDeviceMemoryProperties::default();
        (c.api.vkGetPhysicalDeviceMemoryProperties)(physical, &mut memory);
        let flags = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        let index = (0..memory.memoryTypeCount)
            .find(|i| {
                requirements.memoryTypeBits & (1 << i) != 0
                    && memory.memoryTypes[*i as usize].propertyFlags & flags == flags
            })
            .ok_or("no coherent host-visible readback memory")?;
        let allocation = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            allocationSize: requirements.size,
            memoryTypeIndex: index,
            ..Default::default()
        };
        check(
            (c.api.vkAllocateMemory)(c.device, &allocation, ptr::null(), &mut c.read_memory),
            "allocate readback",
        )?;
        check(
            (c.api.vkBindBufferMemory)(c.device, c.read_buffer, c.read_memory, 0),
            "bind readback",
        )?;
        let texture = VkBufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            size: c.texture_capacity as u64,
            usage: VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            ..Default::default()
        };
        check(
            (c.api.vkCreateBuffer)(c.device, &texture, ptr::null(), &mut c.texture_buffer),
            "player texture buffer",
        )?;
        let mut req = VkMemoryRequirements::default();
        (c.api.vkGetBufferMemoryRequirements)(c.device, c.texture_buffer, &mut req);
        let memindex = (0..memory.memoryTypeCount)
            .find(|i| {
                req.memoryTypeBits & (1 << i) != 0
                    && memory.memoryTypes[*i as usize].propertyFlags & flags == flags
            })
            .ok_or("no player texture memory")?;
        let alloc = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            allocationSize: req.size,
            memoryTypeIndex: memindex,
            ..Default::default()
        };
        check(
            (c.api.vkAllocateMemory)(c.device, &alloc, ptr::null(), &mut c.texture_memory),
            "player texture memory",
        )?;
        check(
            (c.api.vkBindBufferMemory)(c.device, c.texture_buffer, c.texture_memory, 0),
            "bind player texture",
        )?;
        let vertices = VkBufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            size: crate::scene_limits::SCENE_BUFFER_BYTES,
            usage: VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            ..Default::default()
        };
        check(
            (c.api.vkCreateBuffer)(c.device, &vertices, ptr::null(), &mut c.vertex_buffer),
            "scene vertex buffer",
        )?;
        let mut req = VkMemoryRequirements::default();
        (c.api.vkGetBufferMemoryRequirements)(c.device, c.vertex_buffer, &mut req);
        let memindex = (0..memory.memoryTypeCount)
            .find(|i| {
                req.memoryTypeBits & (1 << i) != 0
                    && memory.memoryTypes[*i as usize].propertyFlags & flags == flags
            })
            .ok_or("no coherent scene vertex memory")?;
        let alloc = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            allocationSize: req.size,
            memoryTypeIndex: memindex,
            ..Default::default()
        };
        check(
            (c.api.vkAllocateMemory)(c.device, &alloc, ptr::null(), &mut c.vertex_memory),
            "scene vertex memory",
        )?;
        check(
            (c.api.vkBindBufferMemory)(c.device, c.vertex_buffer, c.vertex_memory, 0),
            "bind scene vertices",
        )?;
        let size = VkDescriptorPoolSize {
            type_: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
            descriptorCount: 2,
        };
        let pool = VkDescriptorPoolCreateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
            maxSets: 1,
            poolSizeCount: 1,
            pPoolSizes: &size,
            ..Default::default()
        };
        check(
            (c.api.vkCreateDescriptorPool)(c.device, &pool, ptr::null(), &mut c.texture_pool),
            "scene descriptor pool",
        )?;
        let alloc = VkDescriptorSetAllocateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            descriptorPool: c.texture_pool,
            descriptorSetCount: 1,
            pSetLayouts: &c.texture_layout,
            ..Default::default()
        };
        check(
            (c.api.vkAllocateDescriptorSets)(c.device, &alloc, &mut c.texture_set),
            "scene descriptor set",
        )?;
        let buffers = [
            VkDescriptorBufferInfo {
                buffer: c.texture_buffer,
                offset: 0,
                range: c.texture_capacity as u64,
            },
            VkDescriptorBufferInfo {
                buffer: c.vertex_buffer,
                offset: 0,
                range: crate::scene_limits::SCENE_BUFFER_BYTES,
            },
        ];
        let descriptors = [
            VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: c.texture_set,
                dstBinding: 0,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                pBufferInfo: &buffers[0],
                ..Default::default()
            },
            VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: c.texture_set,
                dstBinding: 1,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                pBufferInfo: &buffers[1],
                ..Default::default()
            },
        ];
        (c.api.vkUpdateDescriptorSets)(
            c.device,
            descriptors.len() as u32,
            descriptors.as_ptr(),
            0,
            ptr::null(),
        );
        c.texture(&[1, 0, 0, 0, 1, 0, 0, 0, 255, 255, 255, 255])?;
        let pool = VkCommandPoolCreateInfo {
            sType: VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
            queueFamilyIndex: family,
            ..Default::default()
        };
        check(
            (c.api.vkCreateCommandPool)(c.device, &pool, ptr::null(), &mut c.pool),
            "command pool",
        )?;
        let allocation = VkCommandBufferAllocateInfo {
            sType: VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            commandPool: c.pool,
            level: VK_COMMAND_BUFFER_LEVEL_PRIMARY,
            commandBufferCount: images.len() as u32,
            ..Default::default()
        };
        c.commands = vec![ptr::null_mut(); images.len()];
        check(
            (c.api.vkAllocateCommandBuffers)(c.device, &allocation, c.commands.as_mut_ptr()),
            "allocate commands",
        )?;
        c.images = images;
        c.set_batch(std::slice::from_ref(data))?;
        let semaphore = VkSemaphoreCreateInfo {
            sType: VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO,
            ..Default::default()
        };
        check(
            (c.api.vkCreateSemaphore)(c.device, &semaphore, ptr::null(), &mut c.available),
            "acquire semaphore",
        )?;
        check(
            (c.api.vkCreateSemaphore)(c.device, &semaphore, ptr::null(), &mut c.rendered),
            "render semaphore",
        )?;
        c.info = json!({"api":"Vulkan","library":path,"instance_extensions":extension_names,"device":CStr::from_ptr(properties.deviceName.as_ptr()).to_string_lossy(),"api_version":properties.apiVersion,"width":c.extent.width,"height":c.extent.height,"readback_verified":false});
        if workstation {
            c.set_workstation(&super::workstation::initial(_root)?)?;
        }
        Ok(c)
    }
    pub unsafe fn set_workstation(&mut self, rows: &[[f32; 4]]) -> Result<(), String> {
        let complete=crate::workstation::deferred_floor::complete(rows)?;
        let rows=complete.as_ref();
        let size = std::mem::size_of_val(rows);
        if size > super::workstation::STORAGE_BYTES {
            return Err("workstation optical data exceeds storage".into());
        }
        self.wait()?;
        let mut mapped = ptr::null_mut();
        check(
            (self.api.vkMapMemory)(
                self.device,
                self.texture_memory,
                0,
                super::workstation::STORAGE_BYTES as u64,
                0,
                &mut mapped,
            ),
            "map workstation optics",
        )?;
        for (offset, range) in super::workstation::upload_ranges(rows) {
            std::ptr::copy_nonoverlapping(range.as_ptr().cast::<u8>(),
                mapped.cast::<u8>().add(offset * 16), std::mem::size_of_val(range));
        }
        (self.api.vkUnmapMemory)(self.device, self.texture_memory);
        Ok(())
    }
    pub unsafe fn table_eye(&mut self, view:&Value)->Result<(),String>{
        let eye=crate::scene_assets::table_eye(view)?;self.wait()?;let mut mapped=ptr::null_mut();
        check((self.api.vkMapMemory)(self.device,self.scene_memory(),0,self.scene_capacity() as u64,0,&mut mapped),"map table camera")?;
        std::ptr::copy_nonoverlapping(eye.as_ptr().cast::<u8>(),mapped.cast::<u8>().add(self.scene_capacity()-crate::scene_assets::FINISHES.len()-crate::scene_assets::TABLE.len()),16);
        let light=crate::scene_assets::exterior_light(view);
        std::ptr::copy_nonoverlapping(light.as_ptr().cast::<u8>(),mapped.cast::<u8>().add(self.scene_capacity()-crate::scene_assets::FINISHES.len()-crate::scene_assets::TABLE.len()+crate::scene_assets::EXTERIOR_OFFSET),32);
        (self.api.vkUnmapMemory)(self.device,self.scene_memory());Ok(())
    }
    pub unsafe fn monitor_image(&mut self, image: &[u8]) -> Result<(), String> {
        self.wait()?;
        let offset = crate::scene_assets::SCREEN_OFFSET;
        let mut mapped = ptr::null_mut();
        check((self.api.vkMapMemory)(self.device,self.scene_memory(),0,crate::scene_assets::SCENE_BYTES as u64,0,&mut mapped),"map computer screen")?;
        std::ptr::copy_nonoverlapping(image.as_ptr(),mapped.cast::<u8>().add(offset),image.len());
        (self.api.vkUnmapMemory)(self.device,self.scene_memory());
        Ok(())
    }
    pub unsafe fn texture(&mut self, bytes: &[u8]) -> Result<(), String> {
        let mut packed = crate::scene_assets::scene_bytes(bytes)?;
        if self.texture_capacity > packed.len() {
            packed.resize(self.texture_capacity,0);
            let finishes = crate::scene_assets::FINISHES;
            let table=crate::scene_assets::TABLE;
            packed[self.texture_capacity-finishes.len()-table.len()..self.texture_capacity-finishes.len()].copy_from_slice(table);
            packed[self.texture_capacity-finishes.len()..].copy_from_slice(finishes);
        }
        let bytes = packed.as_slice();
        self.wait()?;
        let mut mapped = ptr::null_mut();
        check(
            (self.api.vkMapMemory)(
                self.device,
                self.texture_memory,
                0,
                bytes.len() as u64,
                0,
                &mut mapped,
            ),
            "map player texture",
        )?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast::<u8>(), bytes.len());
        (self.api.vkUnmapMemory)(self.device, self.texture_memory);
        Ok(())
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        if triangles.len() > crate::scene_limits::MAX_SCENE_TRIANGLES {
            return Err(format!("mesh exceeds {} triangles",crate::scene_limits::MAX_SCENE_TRIANGLES));
        }
        self.wait()?;
        self.vertices.clear();
        self.vertices.reserve(triangles.len() * 24);
        for triangle in triangles {
            for vertex in 0..3 {
                self.vertices
                    .extend_from_slice(&triangle[vertex * 4..vertex * 4 + 4]);
                self.vertices
                    .extend_from_slice(&triangle[12 + vertex * 4..16 + vertex * 4]);
            }
        }
        if !self.vertices.is_empty() {
            let mut mapped = ptr::null_mut();
            check(
                (self.api.vkMapMemory)(
                    self.device,
                    self.vertex_memory,
                    0,
                    (self.vertices.len() * 4) as u64,
                    0,
                    &mut mapped,
                ),
                "map scene vertices",
            )?;
            ptr::copy_nonoverlapping(
                self.vertices.as_ptr(),
                mapped.cast::<f32>(),
                self.vertices.len(),
            );
            (self.api.vkUnmapMemory)(self.device, self.vertex_memory);
        }
        self.vertex_count = (triangles.len() * 3) as u32;
        self.record_commands()
    }
    unsafe fn record_commands(&mut self) -> Result<(), String> {
        let state = (self.vertex_count, self.clear, !self.captured);
        if self.recorded == Some(state) {
            return Ok(());
        }
        check(
            (self.api.vkResetCommandPool)(self.device, self.pool, 0),
            "reset command pool",
        )?;
        for (i, cmd) in self.commands.iter().enumerate() {
            let begin = VkCommandBufferBeginInfo {
                sType: VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
                ..Default::default()
            };
            check(
                (self.api.vkBeginCommandBuffer)(*cmd, &begin),
                "begin commands",
            )?;
            if !self.timestamp_pool.is_null() {
                // Cached command buffers must reset their queries on every submission.
                (self.api.vkCmdResetQueryPool)(*cmd, self.timestamp_pool, 0, 2);
                (self.api.vkCmdWriteTimestamp)(
                    *cmd,
                    VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                    self.timestamp_pool,
                    0,
                );
            }
            if self.vegetation_active {
                if let Some(client) = &self.vegetation {
                    client.shadow(*cmd);
                }
            }
            if self.reflection_plants_active {
                self.reflection_vegetation.as_ref().unwrap().shadow(*cmd);
            }
            let clear = VkClearValue {
                color: VkClearColorValue {
                    float32: self.clear,
                },
            };
            let clears = [
                clear,
                VkClearValue {
                    depthStencil: VkClearDepthStencilValue {
                        depth: 1.,
                        stencil: 0,
                    },
                },
            ];
            let pass = VkRenderPassBeginInfo {
                sType: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
                renderPass: if self.water_frames.is_empty() {
                    self.render_pass
                } else {
                    self.water.as_ref().unwrap().scene_pass()
                },
                framebuffer: if self.water_frames.is_empty() {
                    self.framebuffers[i]
                } else {
                    self.water.as_ref().unwrap().scene_framebuffer(false)
                },
                renderArea: VkRect2D {
                    offset: VkOffset2D { x: 0, y: 0 },
                    extent: self.extent,
                },
                clearValueCount: 2,
                pClearValues: clears.as_ptr(),
                ..Default::default()
            };
            if self.water_frames.iter().any(|f| f.optics.planar_reflection) {
                let water = self.water.as_ref().unwrap();
                let reflected = VkRenderPassBeginInfo {
                    framebuffer: water.scene_framebuffer(true),
                    ..pass
                };
                (self.api.vkCmdBeginRenderPass)(*cmd, &reflected, VK_SUBPASS_CONTENTS_INLINE);
                (self.api.vkCmdBindPipeline)(*cmd, VK_PIPELINE_BIND_POINT_GRAPHICS, self.scene_pipeline());
                let set = water.reflection_set();
                (self.api.vkCmdBindDescriptorSets)(
                    *cmd,
                    VK_PIPELINE_BIND_POINT_GRAPHICS,
                    self.layout,
                    0,
                    1,
                    &set,
                    0,
                    ptr::null(),
                );
                (self.api.vkCmdDraw)(*cmd, water.reflection_count, 1, 0, 0);
                if self.reflection_plants_active {
                    self.reflection_vegetation.as_ref().unwrap().color(
                        *cmd,
                        self.extent.width,
                        self.extent.height,
                    );
                }
                (self.api.vkCmdEndRenderPass)(*cmd);
            }
            (self.api.vkCmdBeginRenderPass)(*cmd, &pass, VK_SUBPASS_CONTENTS_INLINE);
            (self.api.vkCmdBindPipeline)(*cmd, VK_PIPELINE_BIND_POINT_GRAPHICS, self.scene_pipeline());
            (self.api.vkCmdBindDescriptorSets)(
                *cmd,
                VK_PIPELINE_BIND_POINT_GRAPHICS,
                self.layout,
                0,
                1,
                &self.scene_set(),
                0,
                ptr::null(),
            );
            (self.api.vkCmdDraw)(*cmd, self.vertex_count-if self.world_active{6}else{0}, 1, 0, 0);
            if self.vegetation_active {
                if let Some(client) = &self.vegetation {
                    client.color(*cmd, self.extent.width, self.extent.height);
                }
            }
            (self.api.vkCmdEndRenderPass)(*cmd);
            if !self.water_frames.is_empty() {
                let range = VkImageSubresourceRange {
                    aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
                    levelCount: 1,
                    layerCount: 1,
                    ..Default::default()
                };
                let barrier = VkImageMemoryBarrier {
                    sType: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                    oldLayout: VK_IMAGE_LAYOUT_UNDEFINED,
                    newLayout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                    dstAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                    srcQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    dstQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    image: self.images[i],
                    subresourceRange: range,
                    ..Default::default()
                };
                (self.api.vkCmdPipelineBarrier)(
                    *cmd,
                    VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT,
                    VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                    0,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    1,
                    &barrier,
                );
                let water = self.water.as_mut().unwrap();
                let attachments = water.scene_attachments(
                    self.views[i],
                    self.format,
                    self.water_frames.iter().any(|f| f.optics.planar_reflection),
                );
                water.encode_batch(*cmd, &self.water_frames, attachments, i)?;
                let barrier = VkImageMemoryBarrier {
                    sType: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                    oldLayout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
                    newLayout: VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                    srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                    dstAccessMask: VK_ACCESS_TRANSFER_READ_BIT,
                    srcQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    dstQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    image: self.images[i],
                    subresourceRange: range,
                    ..Default::default()
                };
                (self.api.vkCmdPipelineBarrier)(
                    *cmd,
                    VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                    VK_PIPELINE_STAGE_TRANSFER_BIT,
                    0,
                    0,
                    ptr::null(),
                    0,
                    ptr::null(),
                    1,
                    &barrier,
                );
            }
            if self.world_active {
                let overlay=VkRenderPassBeginInfo{renderPass:self.overlay_pass,framebuffer:self.framebuffers[i],..pass};
                (self.api.vkCmdBeginRenderPass)(*cmd,&overlay,VK_SUBPASS_CONTENTS_INLINE);
                (self.api.vkCmdBindPipeline)(*cmd,VK_PIPELINE_BIND_POINT_GRAPHICS,self.pipeline);
                (self.api.vkCmdBindDescriptorSets)(*cmd,VK_PIPELINE_BIND_POINT_GRAPHICS,self.layout,0,1,&self.texture_set,0,ptr::null());
                (self.api.vkCmdDraw)(*cmd,6,1,self.vertex_count-6,0);
                (self.api.vkCmdEndRenderPass)(*cmd);
            }
            if !self.timestamp_pool.is_null() {
                (self.api.vkCmdWriteTimestamp)(
                    *cmd,
                    VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
                    self.timestamp_pool,
                    1,
                );
            }
            // Diagnostic capture is one-time work. Interactive frames should
            // not copy the entire fullscreen swapchain image back to the CPU.
            if !self.captured {
                let region = VkBufferImageCopy {
                    imageSubresource: VkImageSubresourceLayers {
                        aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
                        layerCount: 1,
                        ..Default::default()
                    },
                    imageExtent: VkExtent3D {
                        width: self.extent.width,
                        height: self.extent.height,
                        depth: 1,
                    },
                    ..Default::default()
                };
                (self.api.vkCmdCopyImageToBuffer)(
                    *cmd,
                    self.images[i],
                    VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                    self.read_buffer,
                    1,
                    &region,
                );
                let host_read = VkBufferMemoryBarrier {
                    sType: VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
                    srcAccessMask: VK_ACCESS_TRANSFER_WRITE_BIT,
                    dstAccessMask: VK_ACCESS_HOST_READ_BIT,
                    srcQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    dstQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                    buffer: self.read_buffer,
                    offset: 0,
                    size: self.extent.width as u64 * self.extent.height as u64 * 4,
                    ..Default::default()
                };
                (self.api.vkCmdPipelineBarrier)(
                    *cmd,
                    VK_PIPELINE_STAGE_TRANSFER_BIT,
                    VK_PIPELINE_STAGE_HOST_BIT,
                    0,
                    0,
                    ptr::null(),
                    1,
                    &host_read,
                    0,
                    ptr::null(),
                );
            }
            let barrier = VkImageMemoryBarrier {
                sType: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT | VK_ACCESS_TRANSFER_READ_BIT,
                oldLayout: VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL,
                newLayout: VK_IMAGE_LAYOUT_PRESENT_SRC_KHR,
                srcQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                dstQueueFamilyIndex: VK_QUEUE_FAMILY_IGNORED as u32,
                image: self.images[i],
                subresourceRange: VkImageSubresourceRange {
                    aspectMask: VK_IMAGE_ASPECT_COLOR_BIT,
                    levelCount: 1,
                    layerCount: 1,
                    ..Default::default()
                },
                ..Default::default()
            };
            (self.api.vkCmdPipelineBarrier)(
                *cmd,
                VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT | VK_PIPELINE_STAGE_TRANSFER_BIT,
                VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT,
                0,
                0,
                ptr::null(),
                0,
                ptr::null(),
                1,
                &barrier,
            );
            check((self.api.vkEndCommandBuffer)(*cmd), "end commands")?;
        }
        self.recorded = Some(state);
        Ok(())
    }
    unsafe fn shader(&self, bytes: &[u8]) -> Result<VkShaderModule, String> {
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        let create = VkShaderModuleCreateInfo {
            sType: VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
            codeSize: bytes.len(),
            pCode: words.as_ptr(),
            ..Default::default()
        };
        let mut module = ptr::null_mut();
        check(
            (self.api.vkCreateShaderModule)(self.device, &create, ptr::null(), &mut module),
            "shader module",
        )?;
        Ok(module)
    }
    fn scene_buffer(&self)->VkBuffer{if self.world_active{self.world_buffer}else{self.texture_buffer}}
    fn scene_memory(&self)->VkDeviceMemory{if self.world_active{self.world_memory}else{self.texture_memory}}
    fn scene_capacity(&self)->usize{if self.world_active{crate::scene_assets::SCENE_BYTES}else{self.texture_capacity}}
    fn scene_pipeline(&self)->VkPipeline{if self.world_active{self.world_pipeline}else{self.pipeline}}
    fn scene_set(&self)->VkDescriptorSet{if self.world_active{self.world_set}else{self.texture_set}}
    pub unsafe fn set_world_backdrop(&mut self,_root:&std::path::Path,enabled:bool)->Result<bool,String>{
        let changed=self.world_active!=enabled;
        if enabled && self.world_pipeline.is_null(){return Err("live world requires a workstation surface".into());}
        self.wait()?;
        if enabled && self.world_buffer.is_null(){
        let flags=VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT|VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
        let texture = VkBufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
            size: crate::scene_assets::SCENE_BYTES as u64,
            usage: VK_BUFFER_USAGE_STORAGE_BUFFER_BIT,
            sharingMode: VK_SHARING_MODE_EXCLUSIVE,
            ..Default::default()
        };
        check(
            (self.api.vkCreateBuffer)(self.device, &texture, ptr::null(), &mut self.world_buffer),
            "player texture buffer",
        )?;
        let mut req = VkMemoryRequirements::default();
        (self.api.vkGetBufferMemoryRequirements)(self.device, self.world_buffer, &mut req);
        let memindex = (0..self.memory_properties.memoryTypeCount)
            .find(|i| {
                req.memoryTypeBits & (1 << i) != 0
                    && self.memory_properties.memoryTypes[*i as usize].propertyFlags & flags == flags
            })
            .ok_or("no player texture memory")?;
        let alloc = VkMemoryAllocateInfo {
            sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
            allocationSize: req.size,
            memoryTypeIndex: memindex,
            ..Default::default()
        };
        check(
            (self.api.vkAllocateMemory)(self.device, &alloc, ptr::null(), &mut self.world_memory),
            "player texture memory",
        )?;
        check(
            (self.api.vkBindBufferMemory)(self.device, self.world_buffer, self.world_memory, 0),
            "bind player texture",
        )?;
        let size = VkDescriptorPoolSize {
            type_: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
            descriptorCount: 2,
        };
        let pool = VkDescriptorPoolCreateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
            maxSets: 1,
            poolSizeCount: 1,
            pPoolSizes: &size,
            ..Default::default()
        };
        check(
            (self.api.vkCreateDescriptorPool)(self.device, &pool, ptr::null(), &mut self.world_pool),
            "scene descriptor pool",
        )?;
        let alloc = VkDescriptorSetAllocateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            descriptorPool: self.world_pool,
            descriptorSetCount: 1,
            pSetLayouts: &self.texture_layout,
            ..Default::default()
        };
        check(
            (self.api.vkAllocateDescriptorSets)(self.device, &alloc, &mut self.world_set),
            "scene descriptor set",
        )?;
        let buffers = [
            VkDescriptorBufferInfo {
                buffer: self.world_buffer,
                offset: 0,
                range: crate::scene_assets::SCENE_BYTES as u64,
            },
            VkDescriptorBufferInfo {
                buffer: self.vertex_buffer,
                offset: 0,
                range: crate::scene_limits::SCENE_BUFFER_BYTES,
            },
        ];
        let descriptors = [
            VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: self.world_set,
                dstBinding: 0,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                pBufferInfo: &buffers[0],
                ..Default::default()
            },
            VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: self.world_set,
                dstBinding: 1,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                pBufferInfo: &buffers[1],
                ..Default::default()
            },
        ];
        (self.api.vkUpdateDescriptorSets)(
            self.device,
            descriptors.len() as u32,
            descriptors.as_ptr(),
            0,
            ptr::null(),
        );

            let bytes=crate::scene_assets::scene_bytes(&[1,0,0,0,1,0,0,0,255,255,255,255])?;
            let mut mapped=ptr::null_mut();
            check((self.api.vkMapMemory)(self.device,self.world_memory,0,bytes.len() as u64,0,&mut mapped),"map live world assets")?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(),mapped.cast(),bytes.len());
            (self.api.vkUnmapMemory)(self.device,self.world_memory);
        }
        if self.world_active!=enabled {self.vegetation.take();self.reflection_vegetation.take();self.water.take();self.water_frames.clear();self.world_active=enabled;self.recorded=None;}
        Ok(changed)
    }
    pub unsafe fn set_water(
        &mut self,
        root: &std::path::Path,
        packets: &[crate::water::presentation::Packet],
        view: &Value,
        triangles: &[[f32; 24]],
    ) -> Result<(), String> {
        self.table_eye(view)?;
        let cloud_frame = crate::base_system::layer_four::cloud_system::read_frame(view)?;
        let bytes: Vec<u8> = cloud_frame
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        self.wait()?;
        let mut mapped = ptr::null_mut();
        check(
            (self.api.vkMapMemory)(
                self.device,
                self.scene_memory(),
                0,
                crate::scene_assets::SCENE_BYTES as u64,
                0,
                &mut mapped,
            ),
            "map cloud frame",
        )?;
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            mapped
                .cast::<u8>()
                .add(crate::scene_assets::CLOUD_FRAME_OFFSET),
            bytes.len(),
        );
        (self.api.vkUnmapMemory)(self.device, self.scene_memory());

        self.wait()?;
        self.water_frames.clear();
        self.recorded = None;
        if packets.is_empty() {
            return Ok(());
        }
        self.water_frames = super::scene::water_frames(
            packets,
            view,
            self.extent.width,
            self.extent.height,
            false,
        )?;
        if self.water.is_none() {
            let api = &self.api;
            let instance = self.instance;
            self.water = Some(super::water::vulkan::Client::new(
                root,
                self.device,
                &self.memory_properties,
                &mut |name| (api.vkGetInstanceProcAddr)(instance, name.as_ptr()),
                super::water::vulkan::ShaderBinaries {
                    surface_vertex: include_bytes!(concat!(env!("OUT_DIR"), "/water.vert.spv")),
                    surface_fragment: include_bytes!(concat!(env!("OUT_DIR"), "/water.frag.spv")),
                    composite_vertex: include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/water-composite.vert.spv"
                    )),
                    composite_fragment: include_bytes!(concat!(
                        env!("OUT_DIR"),
                        "/water-composite.frag.spv"
                    )),
                },
            )?);
        }
        let scene_buffer=self.scene_buffer();
        let scene_buffer = self.scene_buffer();
        let water = self.water.as_mut().unwrap();
        water.prepare_scene(self.format, self.extent.width, self.extent.height)?;
        if let Some(frame) = self
            .water_frames
            .iter()
            .find(|f| f.optics.planar_reflection)
        {
            water.reflected_geometry(
                &super::scene::reflected_batch(triangles, frame)?,
                self.texture_layout,
                scene_buffer,
            )?;
        }
        Ok(())
    }
    pub unsafe fn set_vegetation(
        &mut self,
        _root: &std::path::Path,
        packet: Option<crate::vegetation::packet::Packet>,
    ) -> Result<(), String> {
        self.wait()?;
        self.vegetation_active = packet.is_some();
        self.recorded = None;
        self.reflection_plants_active = false;
        if let (Some(packet), Some(frame)) = (
            &packet,
            self.water_frames
                .iter()
                .find(|f| f.optics.planar_reflection),
        ) {
            let reflected = super::scene::reflected_plants(packet, frame)?;
            if self.reflection_vegetation.is_none() {
                let api = &self.api;
                let instance = self.instance;
                self.reflection_vegetation = Some(super::vegetation::vulkan::Client::new(
                    self.device,
                    self.scene_buffer(),
                    &self.memory_properties,
                    self.render_pass,
                    &mut |name| (api.vkGetInstanceProcAddr)(instance, name.as_ptr()),
                    [
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.vert.spv")),
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.frag.spv")),
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.shadow.spv")),
                    ],
                )?);
            }
            self.reflection_vegetation
                .as_mut()
                .unwrap()
                .set_packet(&reflected)?;
            self.reflection_plants_active = true;
        }

        if let Some(packet) = packet {
            if self.vegetation.is_none() {
                let api = &self.api;
                let instance = self.instance;
                self.vegetation = Some(super::vegetation::vulkan::Client::new(
                    self.device,
                    self.scene_buffer(),
                    &self.memory_properties,
                    self.render_pass,
                    &mut |name| (api.vkGetInstanceProcAddr)(instance, name.as_ptr()),
                    [
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.vert.spv")),
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.frag.spv")),
                        include_bytes!(concat!(env!("OUT_DIR"), "/vegetation.shadow.spv")),
                    ],
                )?);
            }
            self.vegetation.as_mut().unwrap().set_packet(&packet)?;
        }
        Ok(())
    }
    pub unsafe fn draw(&mut self) -> Result<(), String> {
        if self
            .recorded
            .is_none_or(|state| state != (self.vertex_count, self.clear, !self.captured))
        {
            self.wait()?;
            self.record_commands()?;
        }
        let mut image = 0;
        let result = (self.api.vkAcquireNextImageKHR)(
            self.device,
            self.swapchain,
            1_000_000_000,
            self.available,
            ptr::null_mut(),
            &mut image,
        );
        if result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR {
            return Err(format!("acquire swapchain image: {result}"));
        }
        let stage = VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT;
        let submit = VkSubmitInfo {
            sType: VK_STRUCTURE_TYPE_SUBMIT_INFO,
            waitSemaphoreCount: 1,
            pWaitSemaphores: &self.available,
            pWaitDstStageMask: &stage,
            commandBufferCount: 1,
            pCommandBuffers: &self.commands[image as usize],
            signalSemaphoreCount: 1,
            pSignalSemaphores: &self.rendered,
            ..Default::default()
        };
        self.gpu_serial = self.gpu_serial.wrapping_add(1);
        check(
            (self.api.vkQueueSubmit)(self.queue, 1, &submit, ptr::null_mut()),
            "queue submit",
        )?;
        if let Some(log) = &mut self.gpu_log {
            log.submitted(self.gpu_serial);
        }
        let present = VkPresentInfoKHR {
            sType: VK_STRUCTURE_TYPE_PRESENT_INFO_KHR,
            waitSemaphoreCount: 1,
            pWaitSemaphores: &self.rendered,
            swapchainCount: 1,
            pSwapchains: &self.swapchain,
            pImageIndices: &image,
            ..Default::default()
        };
        let result = (self.api.vkQueuePresentKHR)(self.queue, &present);
        if result != VK_SUCCESS && result != VK_SUBOPTIMAL_KHR {
            if let Some(log) = &mut self.gpu_log {
                log.missing(self.gpu_serial);
            }
            return Err(format!("queue present: {result}"));
        }
        if let Err(error) = self.wait() {
            if let Some(log) = &mut self.gpu_log {
                log.missing(self.gpu_serial);
            }
            return Err(error);
        }
        if let Some(log) = &mut self.gpu_log {
            if self.timestamp_pool.is_null() {
                log.missing(self.gpu_serial);
            } else {
                // The existing frame wait covers query execution; never add a query wait.
                // Each 64-bit result is followed by its 64-bit availability value.
                let mut timestamps = [0u64; 4];
                let result = (self.api.vkGetQueryPoolResults)(
                    self.device,
                    self.timestamp_pool,
                    0,
                    2,
                    std::mem::size_of_val(&timestamps),
                    timestamps.as_mut_ptr().cast(),
                    16,
                    VK_QUERY_RESULT_64_BIT | VK_QUERY_RESULT_WITH_AVAILABILITY_BIT,
                );
                log.observe_query(self.gpu_serial, json!({
                    "swapchain_image":image,"vk_result":result,
                    "begin_ticks":timestamps[0],"begin_available":timestamps[1],
                    "end_ticks":timestamps[2],"end_available":timestamps[3]
                }));
                if result == VK_SUCCESS && timestamps[1] != 0 && timestamps[3] != 0 {
                    let ticks = timestamps[2].wrapping_sub(timestamps[0]) & self.timestamp_mask;
                    let milliseconds = ticks as f64 * self.timestamp_period / 1_000_000.;
                    if milliseconds.is_finite() {
                        log.record(self.gpu_serial, milliseconds);
                    } else {
                        log.missing(self.gpu_serial);
                    }
                } else {
                    log.missing(self.gpu_serial);
                }
            }
        }
        if !self.captured {
            let size = self.extent.width as usize * self.extent.height as usize * 4;
            let mut mapped = ptr::null_mut();
            check(
                (self.api.vkMapMemory)(
                    self.device,
                    self.read_memory,
                    0,
                    size as u64,
                    0,
                    &mut mapped,
                ),
                "map readback",
            )?;
            let bytes = std::slice::from_raw_parts(mapped.cast::<u8>(), size).to_vec();
            (self.api.vkUnmapMemory)(self.device, self.read_memory);
            let center =
                ((self.extent.height / 2 * self.extent.width + self.extent.width / 2) * 4) as usize;
            if self.validate_triangle_center && bytes[center..center + 3].iter().all(|v| *v < 25) {
                return Err("Vulkan readback did not contain the triangle at its center".into());
            }
            let mut ppm =
                format!("P6\n{} {}\n255\n", self.extent.width, self.extent.height).into_bytes();
            for p in bytes.chunks_exact(4) {
                if self.format == VK_FORMAT_B8G8R8A8_UNORM {
                    ppm.extend_from_slice(&[p[2], p[1], p[0]]);
                } else {
                    ppm.extend_from_slice(&p[..3]);
                }
            }
            if let Some(monitor) = &mut self.monitor_capture {
                *monitor = crate::scene_assets::monitor_image(&ppm)?;
            } else {
                std::fs::write(&self.capture, ppm).map_err(|e| e.to_string())?;
            }
            self.captured = true;
            self.info["readback_verified"] = json!(self.validate_triangle_center);
        }
        Ok(())
    }
    pub unsafe fn wait(&self) -> Result<(), String> {
        check((self.api.vkDeviceWaitIdle)(self.device), "device idle")
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            if !self.device.is_null() {
                let _ = (self.api.vkDeviceWaitIdle)(self.device);
                if let Some(log) = &mut self.gpu_log {
                    log.finish();
                }
                self.vegetation.take();
                self.reflection_vegetation.take();
                self.water.take();
                if !self.available.is_null() {
                    (self.api.vkDestroySemaphore)(self.device, self.available, ptr::null());
                }
                if !self.rendered.is_null() {
                    (self.api.vkDestroySemaphore)(self.device, self.rendered, ptr::null());
                }
                if !self.pool.is_null() {
                    (self.api.vkDestroyCommandPool)(self.device, self.pool, ptr::null());
                }
                if !self.timestamp_pool.is_null() {
                    (self.api.vkDestroyQueryPool)(self.device, self.timestamp_pool, ptr::null());
                }
                for fb in &self.framebuffers {
                    (self.api.vkDestroyFramebuffer)(self.device, *fb, ptr::null());
                }
                if !self.pipeline.is_null() {
                    (self.api.vkDestroyPipeline)(self.device, self.pipeline, ptr::null());
                }
                if !self.layout.is_null() {
                    (self.api.vkDestroyPipelineLayout)(self.device, self.layout, ptr::null());
                }
                if !self.texture_pool.is_null() {
                    (self.api.vkDestroyDescriptorPool)(self.device, self.texture_pool, ptr::null());
                }
                if !self.texture_layout.is_null() {
                    (self.api.vkDestroyDescriptorSetLayout)(
                        self.device,
                        self.texture_layout,
                        ptr::null(),
                    );
                }
                if !self.vertex_buffer.is_null() {
                    (self.api.vkDestroyBuffer)(self.device, self.vertex_buffer, ptr::null());
                }
                if !self.vertex_memory.is_null() {
                    (self.api.vkFreeMemory)(self.device, self.vertex_memory, ptr::null());
                }
                if !self.texture_buffer.is_null() {
                    (self.api.vkDestroyBuffer)(self.device, self.texture_buffer, ptr::null());
                }
                if !self.world_pool.is_null(){(self.api.vkDestroyDescriptorPool)(self.device,self.world_pool,ptr::null());}
                if !self.world_buffer.is_null(){(self.api.vkDestroyBuffer)(self.device,self.world_buffer,ptr::null());}
                if !self.world_memory.is_null(){(self.api.vkFreeMemory)(self.device,self.world_memory,ptr::null());}
                if !self.world_pipeline.is_null(){(self.api.vkDestroyPipeline)(self.device,self.world_pipeline,ptr::null());}
                if !self.overlay_pass.is_null(){(self.api.vkDestroyRenderPass)(self.device,self.overlay_pass,ptr::null());}
                if !self.texture_memory.is_null() {
                    (self.api.vkFreeMemory)(self.device, self.texture_memory, ptr::null());
                }
                if !self.render_pass.is_null() {
                    (self.api.vkDestroyRenderPass)(self.device, self.render_pass, ptr::null());
                }
                for view in &self.views {
                    (self.api.vkDestroyImageView)(self.device, *view, ptr::null());
                }
                if !self.read_buffer.is_null() {
                    (self.api.vkDestroyBuffer)(self.device, self.read_buffer, ptr::null());
                }
                if !self.read_memory.is_null() {
                    (self.api.vkFreeMemory)(self.device, self.read_memory, ptr::null());
                }
                if !self.depth_view.is_null() {
                    (self.api.vkDestroyImageView)(self.device, self.depth_view, ptr::null());
                }
                if !self.depth.is_null() {
                    (self.api.vkDestroyImage)(self.device, self.depth, ptr::null());
                }
                if !self.depth_memory.is_null() {
                    (self.api.vkFreeMemory)(self.device, self.depth_memory, ptr::null());
                }
                if !self.swapchain.is_null() {
                    (self.api.vkDestroySwapchainKHR)(self.device, self.swapchain, ptr::null());
                }
                (self.api.vkDestroyDevice)(self.device, ptr::null());
            }
            if !self.surface.is_null() {
                (self.api.vkDestroySurfaceKHR)(self.instance, self.surface, ptr::null());
            }
            if !self.instance.is_null() {
                (self.api.vkDestroyInstance)(self.instance, ptr::null());
            }
        }
    }
}
