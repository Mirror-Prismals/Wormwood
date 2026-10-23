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
    framebuffers: Vec<VkFramebuffer>,
    pool: VkCommandPool,
    commands: Vec<VkCommandBuffer>,
    images: Vec<VkImage>,
    available: VkSemaphore,
    rendered: VkSemaphore,
    read_buffer: VkBuffer,
    read_memory: VkDeviceMemory,
    extent: VkExtent2D,
    format: VkFormat,
    captured: bool,
    pub(super) clear: [f32; 4],
    capture: PathBuf,
    pub info: Value,
}
impl Client {
    pub unsafe fn new(
        data: &[f32; 24],
        _root: &std::path::Path,
        output: &std::path::Path,
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
        let window =
            Window::with_loader(false, c"Wormwood — Vulkan", Some(api.vkGetInstanceProcAddr))?;
        let mut c = Self {
            window,
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
            framebuffers: vec![],
            pool: ptr::null_mut(),
            commands: vec![],
            images: Vec::new(),
            available: ptr::null_mut(),
            rendered: ptr::null_mut(),
            read_buffer: ptr::null_mut(),
            read_memory: ptr::null_mut(),
            extent: Default::default(),
            format: 0,
            captured: false,
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
        if cfg!(any(target_os = "macos", target_os = "ios")) {
            extensions.push(c"VK_KHR_portability_enumeration".as_ptr());
            extensions.push(c"VK_KHR_get_physical_device_properties2".as_ptr());
        }
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
            flags: if cfg!(any(target_os = "macos", target_os = "ios")) {
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
            "vkCreateInstance",
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
                    chosen = Some((device, index as u32));
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        let (physical, family) = chosen.ok_or("no graphics queue supporting presentation")?;
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
        let subpass = VkSubpassDescription {
            pipelineBindPoint: VK_PIPELINE_BIND_POINT_GRAPHICS,
            colorAttachmentCount: 1,
            pColorAttachments: &reference,
            ..Default::default()
        };
        let dependencies = [
            VkSubpassDependency {
                srcSubpass: VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                dstStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                dstAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                ..Default::default()
            },
            VkSubpassDependency {
                srcSubpass: 0,
                dstSubpass: VK_SUBPASS_EXTERNAL as u32,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                dstStageMask: VK_PIPELINE_STAGE_TRANSFER_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                dstAccessMask: VK_ACCESS_TRANSFER_READ_BIT,
                ..Default::default()
            },
        ];
        let pass = VkRenderPassCreateInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            attachmentCount: 1,
            pAttachments: &color,
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
        let push = VkPushConstantRange {
            stageFlags: VK_SHADER_STAGE_VERTEX_BIT,
            offset: 0,
            size: 96,
        };
        let layout = VkPipelineLayoutCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
            pushConstantRangeCount: 1,
            pPushConstantRanges: &push,
            ..Default::default()
        };
        check(
            (c.api.vkCreatePipelineLayout)(c.device, &layout, ptr::null(), &mut c.layout),
            "pipeline layout",
        )?;
        let vertex = c.shader(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/triangle.vert.spv"
        )))?;
        let fragment = match c.shader(include_bytes!(concat!(
            env!("OUT_DIR"),
            "/triangle.frag.spv"
        ))) {
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
        for view in &c.views {
            let fb = VkFramebufferCreateInfo {
                sType: VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
                renderPass: c.render_pass,
                attachmentCount: 1,
                pAttachments: view,
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
        c.info = json!({"api":"Vulkan","library":path,"device":CStr::from_ptr(properties.deviceName.as_ptr()).to_string_lossy(),"api_version":properties.apiVersion,"width":c.extent.width,"height":c.extent.height,"readback_verified":false});
        Ok(c)
    }
    pub unsafe fn set_batch(&mut self, triangles: &[[f32; 24]]) -> Result<(), String> {
        self.wait()?;
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
            let clear = VkClearValue {
                color: VkClearColorValue {
                    float32: self.clear,
                },
            };
            let pass = VkRenderPassBeginInfo {
                sType: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
                renderPass: self.render_pass,
                framebuffer: self.framebuffers[i],
                renderArea: VkRect2D {
                    offset: VkOffset2D { x: 0, y: 0 },
                    extent: self.extent,
                },
                clearValueCount: 1,
                pClearValues: &clear,
                ..Default::default()
            };
            (self.api.vkCmdBeginRenderPass)(*cmd, &pass, VK_SUBPASS_CONTENTS_INLINE);
            (self.api.vkCmdBindPipeline)(*cmd, VK_PIPELINE_BIND_POINT_GRAPHICS, self.pipeline);
            for data in triangles {
                (self.api.vkCmdPushConstants)(
                    *cmd,
                    self.layout,
                    VK_SHADER_STAGE_VERTEX_BIT,
                    0,
                    96,
                    data.as_ptr().cast(),
                );
                (self.api.vkCmdDraw)(*cmd, 3, 1, 0, 0);
            }
            (self.api.vkCmdEndRenderPass)(*cmd);
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
            let barrier = VkImageMemoryBarrier {
                sType: VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
                srcAccessMask: VK_ACCESS_TRANSFER_READ_BIT,
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
                VK_PIPELINE_STAGE_TRANSFER_BIT,
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
    pub unsafe fn draw(&mut self) -> Result<(), String> {
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
        check(
            (self.api.vkQueueSubmit)(self.queue, 1, &submit, ptr::null_mut()),
            "queue submit",
        )?;
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
            return Err(format!("queue present: {result}"));
        }
        self.wait()?;
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
            if bytes[center..center + 3].iter().all(|v| *v < 25) {
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
            std::fs::write(&self.capture, ppm).map_err(|e| e.to_string())?;
            self.captured = true;
            self.info["readback_verified"] = json!(true);
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
                if !self.available.is_null() {
                    (self.api.vkDestroySemaphore)(self.device, self.available, ptr::null());
                }
                if !self.rendered.is_null() {
                    (self.api.vkDestroySemaphore)(self.device, self.rendered, ptr::null());
                }
                if !self.pool.is_null() {
                    (self.api.vkDestroyCommandPool)(self.device, self.pool, ptr::null());
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
