//! Borrowed-device Vulkan water encoding. Parent owns device/queue/command
//! buffers, opaque/reflected scene passes, barriers, fences and presentation.
#![allow(non_snake_case, non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use super::super::vk::*;
use super::data::DrawFrame;
use std::{
    ffi::{CStr, CString, c_void},
    path::Path,
    ptr,
};
fn check(r: VkResult, name: &str) -> Result<(), String> {
    if r == VK_SUCCESS {
        Ok(())
    } else {
        Err(format!("water Vulkan {name}: {r}"))
    }
}
macro_rules! functions{($($name:ident:($($arg:ty),*)$(->$ret:ty)?;)+)=>{
struct Api{$($name:unsafe extern "system" fn($($arg),*)$(->$ret)?,)+}
impl Api{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->PFN_vkVoidFunction)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(stringify!($name)).unwrap();let p=resolve(&name).ok_or_else(||format!("water Vulkan {} unavailable",name.to_string_lossy()))?;std::mem::transmute_copy::<_,unsafe extern "system" fn($($arg),*)$(->$ret)?>(&p)},)+})}}
}}
functions! {
 vkCreateShaderModule:(VkDevice,*const VkShaderModuleCreateInfo,*const VkAllocationCallbacks,*mut VkShaderModule)->VkResult;vkDestroyShaderModule:(VkDevice,VkShaderModule,*const VkAllocationCallbacks);
 vkCreateDescriptorSetLayout:(VkDevice,*const VkDescriptorSetLayoutCreateInfo,*const VkAllocationCallbacks,*mut VkDescriptorSetLayout)->VkResult;vkDestroyDescriptorSetLayout:(VkDevice,VkDescriptorSetLayout,*const VkAllocationCallbacks);
 vkCreatePipelineLayout:(VkDevice,*const VkPipelineLayoutCreateInfo,*const VkAllocationCallbacks,*mut VkPipelineLayout)->VkResult;vkDestroyPipelineLayout:(VkDevice,VkPipelineLayout,*const VkAllocationCallbacks);
 vkCreateRenderPass:(VkDevice,*const VkRenderPassCreateInfo,*const VkAllocationCallbacks,*mut VkRenderPass)->VkResult;vkDestroyRenderPass:(VkDevice,VkRenderPass,*const VkAllocationCallbacks);
 vkCreateGraphicsPipelines:(VkDevice,VkPipelineCache,u32,*const VkGraphicsPipelineCreateInfo,*const VkAllocationCallbacks,*mut VkPipeline)->VkResult;vkDestroyPipeline:(VkDevice,VkPipeline,*const VkAllocationCallbacks);
 vkCreateSampler:(VkDevice,*const VkSamplerCreateInfo,*const VkAllocationCallbacks,*mut VkSampler)->VkResult;vkDestroySampler:(VkDevice,VkSampler,*const VkAllocationCallbacks);
 vkCreateBuffer:(VkDevice,*const VkBufferCreateInfo,*const VkAllocationCallbacks,*mut VkBuffer)->VkResult;vkDestroyBuffer:(VkDevice,VkBuffer,*const VkAllocationCallbacks);vkGetBufferMemoryRequirements:(VkDevice,VkBuffer,*mut VkMemoryRequirements);vkBindBufferMemory:(VkDevice,VkBuffer,VkDeviceMemory,VkDeviceSize)->VkResult;
 vkAllocateMemory:(VkDevice,*const VkMemoryAllocateInfo,*const VkAllocationCallbacks,*mut VkDeviceMemory)->VkResult;vkFreeMemory:(VkDevice,VkDeviceMemory,*const VkAllocationCallbacks);vkMapMemory:(VkDevice,VkDeviceMemory,VkDeviceSize,VkDeviceSize,VkMemoryMapFlags,*mut *mut c_void)->VkResult;vkUnmapMemory:(VkDevice,VkDeviceMemory);
 vkCreateImage:(VkDevice,*const VkImageCreateInfo,*const VkAllocationCallbacks,*mut VkImage)->VkResult;vkDestroyImage:(VkDevice,VkImage,*const VkAllocationCallbacks);vkGetImageMemoryRequirements:(VkDevice,VkImage,*mut VkMemoryRequirements);vkBindImageMemory:(VkDevice,VkImage,VkDeviceMemory,VkDeviceSize)->VkResult;vkCreateImageView:(VkDevice,*const VkImageViewCreateInfo,*const VkAllocationCallbacks,*mut VkImageView)->VkResult;vkDestroyImageView:(VkDevice,VkImageView,*const VkAllocationCallbacks);
 vkCreateFramebuffer:(VkDevice,*const VkFramebufferCreateInfo,*const VkAllocationCallbacks,*mut VkFramebuffer)->VkResult;vkDestroyFramebuffer:(VkDevice,VkFramebuffer,*const VkAllocationCallbacks);
 vkCreateDescriptorPool:(VkDevice,*const VkDescriptorPoolCreateInfo,*const VkAllocationCallbacks,*mut VkDescriptorPool)->VkResult;vkResetDescriptorPool:(VkDevice,VkDescriptorPool,VkDescriptorPoolResetFlags)->VkResult;vkDestroyDescriptorPool:(VkDevice,VkDescriptorPool,*const VkAllocationCallbacks);vkAllocateDescriptorSets:(VkDevice,*const VkDescriptorSetAllocateInfo,*mut VkDescriptorSet)->VkResult;vkUpdateDescriptorSets:(VkDevice,u32,*const VkWriteDescriptorSet,u32,*const VkCopyDescriptorSet);
 vkCmdBeginRenderPass:(VkCommandBuffer,*const VkRenderPassBeginInfo,VkSubpassContents);vkCmdEndRenderPass:(VkCommandBuffer);vkCmdBindPipeline:(VkCommandBuffer,VkPipelineBindPoint,VkPipeline);vkCmdSetViewport:(VkCommandBuffer,u32,u32,*const VkViewport);vkCmdSetScissor:(VkCommandBuffer,u32,u32,*const VkRect2D);vkCmdBindDescriptorSets:(VkCommandBuffer,VkPipelineBindPoint,VkPipelineLayout,u32,u32,*const VkDescriptorSet,u32,*const u32);vkCmdBindVertexBuffers:(VkCommandBuffer,u32,u32,*const VkBuffer,*const VkDeviceSize);vkCmdBindIndexBuffer:(VkCommandBuffer,VkBuffer,VkDeviceSize,VkIndexType);vkCmdDrawIndexed:(VkCommandBuffer,u32,u32,u32,i32,u32);vkCmdDraw:(VkCommandBuffer,u32,u32,u32,u32);
}
pub(crate) struct ShaderBinaries<'a> {
    pub surface_vertex: &'a [u8],
    pub surface_fragment: &'a [u8],
    pub composite_vertex: &'a [u8],
    pub composite_fragment: &'a [u8],
}
#[derive(Clone, Copy)]
pub(crate) struct VkAttachments {
    pub opaque_color_view: VkImageView,
    pub opaque_depth_view: VkImageView,
    pub reflection_view: Option<VkImageView>,
    pub output_view: VkImageView,
    pub color_format: VkFormat,
    pub depth_format: VkFormat,
}
#[derive(Default)]
struct Buffer {
    handle: VkBuffer,
    memory: VkDeviceMemory,
    size: u64,
}
#[derive(Default)]
struct Image {
    handle: VkImage,
    memory: VkDeviceMemory,
    view: VkImageView,
}
#[derive(Default)]
struct Draw {
    vertex: Buffer,
    index: Buffer,
    uniform: Buffer,
    set: VkDescriptorSet,
}
#[derive(Default)]
struct Slot {
    color: Image,
    depth: Image,
    water_fbo: VkFramebuffer,
    output_fbo: VkFramebuffer,
    output_view: VkImageView,
    extent: (u32, u32),
    pool: VkDescriptorPool,
    capacity: usize,
    draws: Vec<Draw>,
}
pub(crate) struct Client {
    api: Api,
    device: VkDevice,
    memory: VkPhysicalDeviceMemoryProperties,
    shaders: [VkShaderModule; 4],
    set_layout: VkDescriptorSetLayout,
    composite_set_layout: VkDescriptorSetLayout,
    layout: VkPipelineLayout,
    composite_layout: VkPipelineLayout,
    water_pass: VkRenderPass,
    composite_pass: VkRenderPass,
    pipeline: VkPipeline,
    composite: VkPipeline,
    sampler: VkSampler,
    depth_sampler: VkSampler,
    format: VkFormat,
    reversed: bool,
    slots: Vec<Slot>,
    scene_images: [Image; 4],
    scene_fbos: [VkFramebuffer; 2],
    scene_pass: VkRenderPass,
    scene_extent: (u32, u32),
    reflection_buffer: Buffer,
    reflection_pool: VkDescriptorPool,
    reflection_set: VkDescriptorSet,
    pub reflection_count: u32,
}
impl Client {
    pub unsafe fn new(
        _root: &Path,
        device: VkDevice,
        memory: &VkPhysicalDeviceMemoryProperties,
        resolve: &mut dyn FnMut(&CStr) -> PFN_vkVoidFunction,
        binaries: ShaderBinaries<'_>,
    ) -> Result<Self, String> {
        if device.is_null() {
            return Err("water Vulkan borrowed device required".into());
        }
        let api = Api::load(resolve)?;
        let mut s = Self {
            api,
            device,
            memory: *memory,
            shaders: [ptr::null_mut(); 4],
            set_layout: ptr::null_mut(),
            composite_set_layout: ptr::null_mut(),
            layout: ptr::null_mut(),
            composite_layout: ptr::null_mut(),
            water_pass: ptr::null_mut(),
            composite_pass: ptr::null_mut(),
            pipeline: ptr::null_mut(),
            composite: ptr::null_mut(),
            sampler: ptr::null_mut(),
            depth_sampler: ptr::null_mut(),
            format: VK_FORMAT_UNDEFINED,
            reversed: false,
            slots: Vec::new(),
            scene_images: std::array::from_fn(|_| Image::default()),
            scene_fbos: [ptr::null_mut(); 2],
            scene_pass: ptr::null_mut(),
            scene_extent: (0, 0),
            reflection_buffer: Buffer::default(),
            reflection_pool: ptr::null_mut(),
            reflection_set: ptr::null_mut(),
            reflection_count: 0,
        };
        for (i, bytes) in [
            binaries.surface_vertex,
            binaries.surface_fragment,
            binaries.composite_vertex,
            binaries.composite_fragment,
        ]
        .into_iter()
        .enumerate()
        {
            s.shaders[i] = s.shader(bytes)?;
        }
        let binding = |binding, kind, stages| VkDescriptorSetLayoutBinding {
            binding,
            descriptorType: kind,
            descriptorCount: 1,
            stageFlags: stages,
            ..Default::default()
        };
        let bindings = [
            binding(
                0,
                VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
            binding(
                1,
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
            binding(
                2,
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
            binding(
                3,
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
        ];
        s.set_layout = s.descriptor_layout(&bindings)?;
        let bindings = [
            binding(
                0,
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
            binding(
                1,
                VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                VK_SHADER_STAGE_FRAGMENT_BIT,
            ),
        ];
        s.composite_set_layout = s.descriptor_layout(&bindings)?;
        for (set, out) in [
            (s.set_layout, &mut s.layout),
            (s.composite_set_layout, &mut s.composite_layout),
        ] {
            let desc = VkPipelineLayoutCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                setLayoutCount: 1,
                pSetLayouts: &set,
                ..Default::default()
            };
            check(
                (s.api.vkCreatePipelineLayout)(device, &desc, ptr::null(), out),
                "pipeline layout",
            )?;
        }
        for (filter, out) in [
            (VK_FILTER_LINEAR, &mut s.sampler),
            (VK_FILTER_NEAREST, &mut s.depth_sampler),
        ] {
            let desc = VkSamplerCreateInfo {
                sType: VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
                magFilter: filter,
                minFilter: filter,
                mipmapMode: VK_SAMPLER_MIPMAP_MODE_NEAREST,
                addressModeU: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                addressModeV: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                addressModeW: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                maxLod: 1.,
                ..Default::default()
            };
            check(
                (s.api.vkCreateSampler)(device, &desc, ptr::null(), out),
                "sampler",
            )?;
        }
        Ok(s)
    }
    unsafe fn shader(&self, bytes: &[u8]) -> Result<VkShaderModule, String> {
        if bytes.len() % 4 != 0 || bytes.len() < 20 {
            return Err("water Vulkan SPIR-V malformed".into());
        }
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
            .collect();
        if words[0] != 0x07230203 {
            return Err("water shader SPIR-V magic mismatch".into());
        }
        let desc = VkShaderModuleCreateInfo {
            sType: VK_STRUCTURE_TYPE_SHADER_MODULE_CREATE_INFO,
            codeSize: bytes.len(),
            pCode: words.as_ptr(),
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateShaderModule)(self.device, &desc, ptr::null(), &mut result),
            "shader module",
        )?;
        Ok(result)
    }
    unsafe fn descriptor_layout(
        &self,
        bindings: &[VkDescriptorSetLayoutBinding],
    ) -> Result<VkDescriptorSetLayout, String> {
        let desc = VkDescriptorSetLayoutCreateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_LAYOUT_CREATE_INFO,
            bindingCount: bindings.len() as u32,
            pBindings: bindings.as_ptr(),
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateDescriptorSetLayout)(self.device, &desc, ptr::null(), &mut result),
            "descriptor layout",
        )?;
        Ok(result)
    }
    fn memory_type(&self, bits: u32, flags: u32) -> Result<u32, String> {
        (0..self.memory.memoryTypeCount)
            .find(|i| {
                bits & (1 << i) != 0
                    && self.memory.memoryTypes[*i as usize].propertyFlags & flags == flags
            })
            .ok_or("water Vulkan required memory type unavailable".into())
    }
    unsafe fn buffer(&self, size: u64, usage: u32) -> Result<Buffer, String> {
        let mut b = Buffer::default();
        let result = (|| {
            let desc = VkBufferCreateInfo {
                sType: VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
                size: size.max(4),
                usage,
                sharingMode: VK_SHARING_MODE_EXCLUSIVE,
                ..Default::default()
            };
            check(
                (self.api.vkCreateBuffer)(self.device, &desc, ptr::null(), &mut b.handle),
                "buffer",
            )?;
            let mut req = VkMemoryRequirements::default();
            (self.api.vkGetBufferMemoryRequirements)(self.device, b.handle, &mut req);
            let desc = VkMemoryAllocateInfo {
                sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                allocationSize: req.size,
                memoryTypeIndex: self.memory_type(
                    req.memoryTypeBits,
                    VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT,
                )?,
                ..Default::default()
            };
            check(
                (self.api.vkAllocateMemory)(self.device, &desc, ptr::null(), &mut b.memory),
                "buffer memory",
            )?;
            check(
                (self.api.vkBindBufferMemory)(self.device, b.handle, b.memory, 0),
                "bind buffer",
            )?;
            b.size = size.max(4);
            Ok(())
        })();
        if let Err(e) = result {
            self.destroy_buffer(&mut b);
            return Err(e);
        }
        Ok(b)
    }
    unsafe fn destroy_buffer(&self, b: &mut Buffer) {
        if !b.handle.is_null() {
            (self.api.vkDestroyBuffer)(self.device, b.handle, ptr::null());
        }
        if !b.memory.is_null() {
            (self.api.vkFreeMemory)(self.device, b.memory, ptr::null());
        }
        *b = Buffer::default();
    }
    unsafe fn upload(&self, b: &Buffer, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        if bytes.len() as u64 > b.size {
            return Err("water Vulkan upload exceeds buffer".into());
        }
        let mut mapped = ptr::null_mut();
        check(
            (self.api.vkMapMemory)(self.device, b.memory, 0, bytes.len() as u64, 0, &mut mapped),
            "map buffer",
        )?;
        if mapped.is_null() {
            (self.api.vkUnmapMemory)(self.device, b.memory);
            return Err("water Vulkan mapped pointer missing".into());
        }
        ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast(), bytes.len());
        (self.api.vkUnmapMemory)(self.device, b.memory);
        Ok(())
    }
    unsafe fn image(
        &self,
        width: u32,
        height: u32,
        format: VkFormat,
        depth: bool,
    ) -> Result<Image, String> {
        let mut image = Image::default();
        let result = (|| {
            let desc = VkImageCreateInfo {
                sType: VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
                imageType: VK_IMAGE_TYPE_2D,
                format,
                extent: VkExtent3D {
                    width,
                    height,
                    depth: 1,
                },
                mipLevels: 1,
                arrayLayers: 1,
                samples: VK_SAMPLE_COUNT_1_BIT,
                tiling: VK_IMAGE_TILING_OPTIMAL,
                usage: if depth {
                    VK_IMAGE_USAGE_DEPTH_STENCIL_ATTACHMENT_BIT | VK_IMAGE_USAGE_SAMPLED_BIT
                } else {
                    VK_IMAGE_USAGE_COLOR_ATTACHMENT_BIT | VK_IMAGE_USAGE_SAMPLED_BIT
                },
                sharingMode: VK_SHARING_MODE_EXCLUSIVE,
                initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
                ..Default::default()
            };
            check(
                (self.api.vkCreateImage)(self.device, &desc, ptr::null(), &mut image.handle),
                "private image",
            )?;
            let mut req = VkMemoryRequirements::default();
            (self.api.vkGetImageMemoryRequirements)(self.device, image.handle, &mut req);
            let desc = VkMemoryAllocateInfo {
                sType: VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
                allocationSize: req.size,
                memoryTypeIndex: self
                    .memory_type(req.memoryTypeBits, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT)?,
                ..Default::default()
            };
            check(
                (self.api.vkAllocateMemory)(self.device, &desc, ptr::null(), &mut image.memory),
                "image memory",
            )?;
            check(
                (self.api.vkBindImageMemory)(self.device, image.handle, image.memory, 0),
                "bind image",
            )?;
            let desc = VkImageViewCreateInfo {
                sType: VK_STRUCTURE_TYPE_IMAGE_VIEW_CREATE_INFO,
                image: image.handle,
                viewType: VK_IMAGE_VIEW_TYPE_2D,
                format,
                subresourceRange: VkImageSubresourceRange {
                    aspectMask: if depth {
                        VK_IMAGE_ASPECT_DEPTH_BIT
                    } else {
                        VK_IMAGE_ASPECT_COLOR_BIT
                    },
                    baseMipLevel: 0,
                    levelCount: 1,
                    baseArrayLayer: 0,
                    layerCount: 1,
                },
                ..Default::default()
            };
            check(
                (self.api.vkCreateImageView)(self.device, &desc, ptr::null(), &mut image.view),
                "image view",
            )?;
            Ok(())
        })();
        if let Err(e) = result {
            self.destroy_image(&mut image);
            return Err(e);
        }
        Ok(image)
    }
    unsafe fn destroy_image(&self, image: &mut Image) {
        if !image.view.is_null() {
            (self.api.vkDestroyImageView)(self.device, image.view, ptr::null());
        }
        if !image.handle.is_null() {
            (self.api.vkDestroyImage)(self.device, image.handle, ptr::null());
        }
        if !image.memory.is_null() {
            (self.api.vkFreeMemory)(self.device, image.memory, ptr::null());
        }
        *image = Image::default();
    }
    unsafe fn render_pass(&self, format: VkFormat, water: bool) -> Result<VkRenderPass, String> {
        let color = VkAttachmentDescription {
            format,
            samples: VK_SAMPLE_COUNT_1_BIT,
            loadOp: if water {
                VK_ATTACHMENT_LOAD_OP_CLEAR
            } else {
                VK_ATTACHMENT_LOAD_OP_DONT_CARE
            },
            storeOp: VK_ATTACHMENT_STORE_OP_STORE,
            stencilLoadOp: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencilStoreOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initialLayout: if water {
                VK_IMAGE_LAYOUT_UNDEFINED
            } else {
                VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL
            },
            finalLayout: if water {
                VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL
            } else {
                VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL
            },
            ..Default::default()
        };
        let depth = VkAttachmentDescription {
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
        let attachments = if water {
            vec![color, depth]
        } else {
            vec![color]
        };
        let color = VkAttachmentReference {
            attachment: 0,
            layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let depth = VkAttachmentReference {
            attachment: 1,
            layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let subpass = VkSubpassDescription {
            pipelineBindPoint: VK_PIPELINE_BIND_POINT_GRAPHICS,
            colorAttachmentCount: 1,
            pColorAttachments: &color,
            pDepthStencilAttachment: if water { &depth } else { ptr::null() },
            ..Default::default()
        };
        let dependencies = [
            VkSubpassDependency {
                srcSubpass: VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                dstStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT
                    | VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT | VK_ACCESS_SHADER_READ_BIT,
                dstAccessMask: VK_ACCESS_SHADER_READ_BIT
                    | VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT
                    | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                ..Default::default()
            },
            VkSubpassDependency {
                srcSubpass: 0,
                dstSubpass: VK_SUBPASS_EXTERNAL as u32,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT,
                dstStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT,
                dstAccessMask: VK_ACCESS_SHADER_READ_BIT,
                ..Default::default()
            },
        ];
        let desc = VkRenderPassCreateInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            attachmentCount: attachments.len() as u32,
            pAttachments: attachments.as_ptr(),
            subpassCount: 1,
            pSubpasses: &subpass,
            dependencyCount: 2,
            pDependencies: dependencies.as_ptr(),
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateRenderPass)(self.device, &desc, ptr::null(), &mut result),
            "render pass",
        )?;
        Ok(result)
    }
    unsafe fn opaque_pass(&self, format: VkFormat) -> Result<VkRenderPass, String> {
        let color = VkAttachmentDescription {
            format,
            samples: VK_SAMPLE_COUNT_1_BIT,
            loadOp: VK_ATTACHMENT_LOAD_OP_CLEAR,
            storeOp: VK_ATTACHMENT_STORE_OP_STORE,
            stencilLoadOp: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencilStoreOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
            finalLayout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            ..Default::default()
        };
        let depth = VkAttachmentDescription {
            format: VK_FORMAT_D32_SFLOAT,
            samples: VK_SAMPLE_COUNT_1_BIT,
            loadOp: VK_ATTACHMENT_LOAD_OP_CLEAR,
            storeOp: VK_ATTACHMENT_STORE_OP_STORE,
            stencilLoadOp: VK_ATTACHMENT_LOAD_OP_DONT_CARE,
            stencilStoreOp: VK_ATTACHMENT_STORE_OP_DONT_CARE,
            initialLayout: VK_IMAGE_LAYOUT_UNDEFINED,
            finalLayout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_READ_ONLY_OPTIMAL,
            ..Default::default()
        };
        let attachments = vec![color, depth];
        let color = VkAttachmentReference {
            attachment: 0,
            layout: VK_IMAGE_LAYOUT_COLOR_ATTACHMENT_OPTIMAL,
        };
        let depth = VkAttachmentReference {
            attachment: 1,
            layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let subpass = VkSubpassDescription {
            pipelineBindPoint: VK_PIPELINE_BIND_POINT_GRAPHICS,
            colorAttachmentCount: 1,
            pColorAttachments: &color,
            pDepthStencilAttachment: &depth,
            ..Default::default()
        };
        let dependencies = [
            VkSubpassDependency {
                srcSubpass: VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                dstStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT
                    | VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT | VK_ACCESS_SHADER_READ_BIT,
                dstAccessMask: VK_ACCESS_SHADER_READ_BIT
                    | VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT
                    | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                ..Default::default()
            },
            VkSubpassDependency {
                srcSubpass: 0,
                dstSubpass: VK_SUBPASS_EXTERNAL as u32,
                srcStageMask: VK_PIPELINE_STAGE_COLOR_ATTACHMENT_OUTPUT_BIT
                    | VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                dstStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                srcAccessMask: VK_ACCESS_COLOR_ATTACHMENT_WRITE_BIT
                    | VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                dstAccessMask: VK_ACCESS_SHADER_READ_BIT,
                ..Default::default()
            },
        ];
        let desc = VkRenderPassCreateInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
            attachmentCount: attachments.len() as u32,
            pAttachments: attachments.as_ptr(),
            subpassCount: 1,
            pSubpasses: &subpass,
            dependencyCount: 2,
            pDependencies: dependencies.as_ptr(),
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateRenderPass)(self.device, &desc, ptr::null(), &mut result),
            "render pass",
        )?;
        Ok(result)
    }
    unsafe fn pipeline(
        &self,
        pass: VkRenderPass,
        layout: VkPipelineLayout,
        water: bool,
        reversed: bool,
    ) -> Result<VkPipeline, String> {
        let offset = if water { 0 } else { 2 };
        let stages = [
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_VERTEX_BIT,
                module: self.shaders[offset],
                pName: c"main".as_ptr(),
                ..Default::default()
            },
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_FRAGMENT_BIT,
                module: self.shaders[offset + 1],
                pName: c"main".as_ptr(),
                ..Default::default()
            },
        ];
        let binding = VkVertexInputBindingDescription {
            binding: 0,
            stride: 96,
            inputRate: VK_VERTEX_INPUT_RATE_VERTEX,
        };
        let attributes: Vec<_> = (0..6)
            .map(|i| VkVertexInputAttributeDescription {
                location: i,
                binding: 0,
                format: VK_FORMAT_R32G32B32A32_SFLOAT,
                offset: i * 16,
            })
            .collect();
        let vertex = VkPipelineVertexInputStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            vertexBindingDescriptionCount: if water { 1 } else { 0 },
            pVertexBindingDescriptions: if water { &binding } else { ptr::null() },
            vertexAttributeDescriptionCount: if water { 6 } else { 0 },
            pVertexAttributeDescriptions: if water {
                attributes.as_ptr()
            } else {
                ptr::null()
            },
            ..Default::default()
        };
        let assembly = VkPipelineInputAssemblyStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_INPUT_ASSEMBLY_STATE_CREATE_INFO,
            topology: VK_PRIMITIVE_TOPOLOGY_TRIANGLE_LIST,
            ..Default::default()
        };
        let viewport = VkPipelineViewportStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_VIEWPORT_STATE_CREATE_INFO,
            viewportCount: 1,
            scissorCount: 1,
            ..Default::default()
        };
        let raster = VkPipelineRasterizationStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_RASTERIZATION_STATE_CREATE_INFO,
            polygonMode: VK_POLYGON_MODE_FILL,
            cullMode: VK_CULL_MODE_NONE,
            frontFace: VK_FRONT_FACE_COUNTER_CLOCKWISE,
            lineWidth: 1.,
            ..Default::default()
        };
        let samples = VkPipelineMultisampleStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_MULTISAMPLE_STATE_CREATE_INFO,
            rasterizationSamples: VK_SAMPLE_COUNT_1_BIT,
            ..Default::default()
        };
        let depth = VkPipelineDepthStencilStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_DEPTH_STENCIL_STATE_CREATE_INFO,
            depthTestEnable: if water { 1 } else { 0 },
            depthWriteEnable: if water { 1 } else { 0 },
            depthCompareOp: if reversed {
                VK_COMPARE_OP_GREATER_OR_EQUAL
            } else {
                VK_COMPARE_OP_LESS_OR_EQUAL
            },
            maxDepthBounds: 1.,
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
        let states = [VK_DYNAMIC_STATE_VIEWPORT, VK_DYNAMIC_STATE_SCISSOR];
        let dynamic = VkPipelineDynamicStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_DYNAMIC_STATE_CREATE_INFO,
            dynamicStateCount: 2,
            pDynamicStates: states.as_ptr(),
            ..Default::default()
        };
        let desc = VkGraphicsPipelineCreateInfo {
            sType: VK_STRUCTURE_TYPE_GRAPHICS_PIPELINE_CREATE_INFO,
            stageCount: 2,
            pStages: stages.as_ptr(),
            pVertexInputState: &vertex,
            pInputAssemblyState: &assembly,
            pViewportState: &viewport,
            pRasterizationState: &raster,
            pMultisampleState: &samples,
            pDepthStencilState: if water { &depth } else { ptr::null() },
            pColorBlendState: &blend,
            pDynamicState: &dynamic,
            layout,
            renderPass: pass,
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateGraphicsPipelines)(
                self.device,
                ptr::null_mut(),
                1,
                &desc,
                ptr::null(),
                &mut result,
            ),
            "graphics pipeline",
        )?;
        Ok(result)
    }
    unsafe fn framebuffer(
        &self,
        pass: VkRenderPass,
        views: &[VkImageView],
        width: u32,
        height: u32,
    ) -> Result<VkFramebuffer, String> {
        let desc = VkFramebufferCreateInfo {
            sType: VK_STRUCTURE_TYPE_FRAMEBUFFER_CREATE_INFO,
            renderPass: pass,
            attachmentCount: views.len() as u32,
            pAttachments: views.as_ptr(),
            width,
            height,
            layers: 1,
            ..Default::default()
        };
        let mut result = ptr::null_mut();
        check(
            (self.api.vkCreateFramebuffer)(self.device, &desc, ptr::null(), &mut result),
            "framebuffer",
        )?;
        Ok(result)
    }
    unsafe fn destroy_slot(&self, slot: &mut Slot) {
        for f in [&mut slot.water_fbo, &mut slot.output_fbo] {
            if !f.is_null() {
                (self.api.vkDestroyFramebuffer)(self.device, *f, ptr::null());
                *f = ptr::null_mut();
            }
        }
        self.destroy_image(&mut slot.color);
        self.destroy_image(&mut slot.depth);
        for d in &mut slot.draws {
            self.destroy_buffer(&mut d.vertex);
            self.destroy_buffer(&mut d.index);
            self.destroy_buffer(&mut d.uniform);
        }
        slot.draws.clear();
        if !slot.pool.is_null() {
            (self.api.vkDestroyDescriptorPool)(self.device, slot.pool, ptr::null());
            slot.pool = ptr::null_mut();
        }
        slot.extent = (0, 0);
        slot.capacity = 0;
        slot.output_view = ptr::null_mut();
    }
    /// Parent has waited this frame_slot's fence. Sampled inputs are already in
    /// SHADER_READ_ONLY / DEPTH_STENCIL_READ_ONLY layouts; output is in COLOR_ATTACHMENT
    /// layout and cannot alias scene_color. All slots must be idle on format/depth
    /// convention changes, and before release. No command is submitted here.
    pub unsafe fn encode_batch(
        &mut self,
        command: VkCommandBuffer,
        frames: &[DrawFrame],
        a: VkAttachments,
        frame_slot: usize,
    ) -> Result<(), String> {
        let Some(first) = frames.first() else {
            return Ok(());
        };
        if command.is_null()
            || a.opaque_color_view.is_null()
            || a.opaque_depth_view.is_null()
            || a.output_view.is_null()
            || a.output_view == a.opaque_color_view
            || frame_slot >= 8
        {
            return Err("water Vulkan borrowed inputs/slot invalid".into());
        }
        for f in frames {
            f.validate()?;
            if f.camera.width != first.camera.width
                || f.camera.height != first.camera.height
                || f.camera.reversed_z != first.camera.reversed_z
            {
                return Err("water Vulkan batch camera mismatch".into());
            }
            if f.optics.planar_reflection && a.reflection_view.is_none() {
                return Err("water Vulkan actual reflection missing".into());
            }
        }
        if self.format != a.color_format
            || self.reversed != first.camera.reversed_z
            || self.pipeline.is_null()
        {
            let mut slots = std::mem::take(&mut self.slots);
            for slot in &mut slots {
                self.destroy_slot(slot);
            }
            self.slots = slots;
            for p in [&mut self.pipeline, &mut self.composite] {
                if !p.is_null() {
                    (self.api.vkDestroyPipeline)(self.device, *p, ptr::null());
                    *p = ptr::null_mut();
                }
            }
            for p in [&mut self.water_pass, &mut self.composite_pass] {
                if !p.is_null() {
                    (self.api.vkDestroyRenderPass)(self.device, *p, ptr::null());
                    *p = ptr::null_mut();
                }
            }
            self.water_pass = self.render_pass(VK_FORMAT_R8G8B8A8_UNORM, true)?;
            self.composite_pass = self.render_pass(a.color_format, false)?;
            self.pipeline =
                self.pipeline(self.water_pass, self.layout, true, first.camera.reversed_z)?;
            self.composite = self.pipeline(
                self.composite_pass,
                self.composite_layout,
                false,
                first.camera.reversed_z,
            )?;
            self.format = a.color_format;
            self.reversed = first.camera.reversed_z;
        }
        while self.slots.len() <= frame_slot {
            self.slots.push(Slot::default());
        }
        let mut slot = std::mem::take(&mut self.slots[frame_slot]);
        let result = self.encode_slot(command, frames, a, &mut slot);
        self.slots[frame_slot] = slot;
        result
    }
    unsafe fn encode_slot(
        &self,
        command: VkCommandBuffer,
        frames: &[DrawFrame],
        a: VkAttachments,
        slot: &mut Slot,
    ) -> Result<(), String> {
        let width = frames[0].camera.width;
        let height = frames[0].camera.height;
        if slot.extent != (width, height) {
            self.destroy_slot(slot);
            slot.color = self.image(width, height, VK_FORMAT_R8G8B8A8_UNORM, false)?;
            slot.depth = self.image(width, height, VK_FORMAT_D32_SFLOAT, true)?;
            slot.water_fbo = self.framebuffer(
                self.water_pass,
                &[slot.color.view, slot.depth.view],
                width,
                height,
            )?;
            slot.extent = (width, height);
        }
        if slot.output_view != a.output_view || slot.output_fbo.is_null() {
            if !slot.output_fbo.is_null() {
                (self.api.vkDestroyFramebuffer)(self.device, slot.output_fbo, ptr::null());
                slot.output_fbo = ptr::null_mut();
            }
            slot.output_fbo =
                self.framebuffer(self.composite_pass, &[a.output_view], width, height)?;
            slot.output_view = a.output_view;
        }
        let count = frames.len();
        if slot.pool.is_null() || slot.capacity < count {
            if !slot.pool.is_null() {
                (self.api.vkDestroyDescriptorPool)(self.device, slot.pool, ptr::null());
                slot.pool = ptr::null_mut();
            }
            let sizes = [
                VkDescriptorPoolSize {
                    type_: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                    descriptorCount: count as u32,
                },
                VkDescriptorPoolSize {
                    type_: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                    descriptorCount: (3 * count + 2) as u32,
                },
            ];
            let desc = VkDescriptorPoolCreateInfo {
                sType: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
                maxSets: (count + 1) as u32,
                poolSizeCount: 2,
                pPoolSizes: sizes.as_ptr(),
                ..Default::default()
            };
            check(
                (self.api.vkCreateDescriptorPool)(self.device, &desc, ptr::null(), &mut slot.pool),
                "frame descriptor pool",
            )?;
            slot.capacity = count;
        } else {
            check(
                (self.api.vkResetDescriptorPool)(self.device, slot.pool, 0),
                "reset frame descriptors",
            )?;
        }
        let mut layouts = vec![self.set_layout; count];
        layouts.push(self.composite_set_layout);
        let desc = VkDescriptorSetAllocateInfo {
            sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
            descriptorPool: slot.pool,
            descriptorSetCount: layouts.len() as u32,
            pSetLayouts: layouts.as_ptr(),
            ..Default::default()
        };
        let mut sets = vec![ptr::null_mut(); layouts.len()];
        check(
            (self.api.vkAllocateDescriptorSets)(self.device, &desc, sets.as_mut_ptr()),
            "frame descriptors",
        )?;
        while slot.draws.len() < count {
            slot.draws.push(Draw::default());
        }
        for (i, frame) in frames.iter().enumerate() {
            let vertices = frame.vertex_bytes_borrowed();
            let indices = frame.index_bytes_borrowed();
            let uniform = super::uniform_bytes(frame, -1.)?;
            let d = &mut slot.draws[i];
            for (buffer, bytes, usage) in [
                (
                    &mut d.vertex,
                    vertices,
                    VK_BUFFER_USAGE_VERTEX_BUFFER_BIT,
                ),
                (
                    &mut d.index,
                    indices,
                    VK_BUFFER_USAGE_INDEX_BUFFER_BIT,
                ),
                (
                    &mut d.uniform,
                    uniform.as_slice(),
                    VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT,
                ),
            ] {
                if buffer.handle.is_null() || buffer.size < (bytes.len() as u64).max(4) {
                    let next = self.buffer(bytes.len() as u64, usage)?;
                    self.destroy_buffer(buffer);
                    *buffer = next;
                }
                self.upload(buffer, bytes)?;
            }
            d.set = sets[i];
            let buffer = VkDescriptorBufferInfo {
                buffer: d.uniform.handle,
                offset: 0,
                range: 1296,
            };
            let images = [
                VkDescriptorImageInfo {
                    sampler: self.sampler,
                    imageView: a.opaque_color_view,
                    imageLayout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                },
                VkDescriptorImageInfo {
                    sampler: self.sampler,
                    imageView: a.reflection_view.unwrap_or(a.opaque_color_view),
                    imageLayout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
                },
                VkDescriptorImageInfo {
                    sampler: self.depth_sampler,
                    imageView: a.opaque_depth_view,
                    imageLayout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_READ_ONLY_OPTIMAL,
                },
            ];
            let mut writes = vec![VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: d.set,
                dstBinding: 0,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                pBufferInfo: &buffer,
                ..Default::default()
            }];
            for (j, image) in images.iter().enumerate() {
                writes.push(VkWriteDescriptorSet {
                    sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                    dstSet: d.set,
                    dstBinding: (j + 1) as u32,
                    descriptorCount: 1,
                    descriptorType: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                    pImageInfo: image,
                    ..Default::default()
                });
            }
            (self.api.vkUpdateDescriptorSets)(
                self.device,
                writes.len() as u32,
                writes.as_ptr(),
                0,
                ptr::null(),
            );
        }
        let composite = sets[count];
        let images = [
            VkDescriptorImageInfo {
                sampler: self.sampler,
                imageView: a.opaque_color_view,
                imageLayout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            },
            VkDescriptorImageInfo {
                sampler: self.sampler,
                imageView: slot.color.view,
                imageLayout: VK_IMAGE_LAYOUT_SHADER_READ_ONLY_OPTIMAL,
            },
        ];
        let writes: Vec<_> = images
            .iter()
            .enumerate()
            .map(|(i, image)| VkWriteDescriptorSet {
                sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                dstSet: composite,
                dstBinding: i as u32,
                descriptorCount: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                pImageInfo: image,
                ..Default::default()
            })
            .collect();
        (self.api.vkUpdateDescriptorSets)(
            self.device,
            writes.len() as u32,
            writes.as_ptr(),
            0,
            ptr::null(),
        );
        let viewport = VkViewport {
            x: 0.,
            y: 0.,
            width: width as f32,
            height: height as f32,
            minDepth: 0.,
            maxDepth: 1.,
        };
        let area = VkRect2D {
            offset: VkOffset2D { x: 0, y: 0 },
            extent: VkExtent2D { width, height },
        };
        let clears = [
            VkClearValue {
                color: VkClearColorValue { float32: [0.; 4] },
            },
            VkClearValue {
                depthStencil: VkClearDepthStencilValue {
                    depth: if self.reversed { 0. } else { 1. },
                    stencil: 0,
                },
            },
        ];
        let begin = VkRenderPassBeginInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            renderPass: self.water_pass,
            framebuffer: slot.water_fbo,
            renderArea: area,
            clearValueCount: 2,
            pClearValues: clears.as_ptr(),
            ..Default::default()
        };
        (self.api.vkCmdBeginRenderPass)(command, &begin, VK_SUBPASS_CONTENTS_INLINE);
        (self.api.vkCmdSetViewport)(command, 0, 1, &viewport);
        (self.api.vkCmdSetScissor)(command, 0, 1, &area);
        (self.api.vkCmdBindPipeline)(command, VK_PIPELINE_BIND_POINT_GRAPHICS, self.pipeline);
        for (i, frame) in frames.iter().enumerate() {
            if frame.indices.is_empty() {
                continue;
            }
            let d = &slot.draws[i];
            (self.api.vkCmdBindDescriptorSets)(
                command,
                VK_PIPELINE_BIND_POINT_GRAPHICS,
                self.layout,
                0,
                1,
                &d.set,
                0,
                ptr::null(),
            );
            (self.api.vkCmdBindVertexBuffers)(command, 0, 1, &d.vertex.handle, &0);
            (self.api.vkCmdBindIndexBuffer)(command, d.index.handle, 0, VK_INDEX_TYPE_UINT32);
            (self.api.vkCmdDrawIndexed)(command, frame.indices.len() as u32, 1, 0, 0, 0);
        }
        (self.api.vkCmdEndRenderPass)(command);
        let begin = VkRenderPassBeginInfo {
            sType: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
            renderPass: self.composite_pass,
            framebuffer: slot.output_fbo,
            renderArea: area,
            ..Default::default()
        };
        (self.api.vkCmdBeginRenderPass)(command, &begin, VK_SUBPASS_CONTENTS_INLINE);
        (self.api.vkCmdSetViewport)(command, 0, 1, &viewport);
        (self.api.vkCmdSetScissor)(command, 0, 1, &area);
        (self.api.vkCmdBindPipeline)(command, VK_PIPELINE_BIND_POINT_GRAPHICS, self.composite);
        (self.api.vkCmdBindDescriptorSets)(
            command,
            VK_PIPELINE_BIND_POINT_GRAPHICS,
            self.composite_layout,
            0,
            1,
            &composite,
            0,
            ptr::null(),
        );
        (self.api.vkCmdDraw)(command, 3, 1, 0, 0);
        (self.api.vkCmdEndRenderPass)(command);
        Ok(())
    }
    pub unsafe fn encode(
        &mut self,
        command: VkCommandBuffer,
        frame: &DrawFrame,
        a: VkAttachments,
        frame_slot: usize,
    ) -> Result<(), String> {
        self.encode_batch(command, std::slice::from_ref(frame), a, frame_slot)
    }
    pub unsafe fn prepare_scene(
        &mut self,
        format: VkFormat,
        width: u32,
        height: u32,
    ) -> Result<(), String> {
        if self.scene_extent == (width, height) {
            return Ok(());
        }
        if self.scene_pass.is_null() {
            self.scene_pass = self.opaque_pass(format)?;
        }
        for i in 0..2 {
            if !self.scene_fbos[i].is_null() {
                (self.api.vkDestroyFramebuffer)(self.device, self.scene_fbos[i], ptr::null());
                self.scene_fbos[i] = ptr::null_mut();
            }
        }
        let mut old = std::mem::replace(
            &mut self.scene_images,
            std::array::from_fn(|_| Image::default()),
        );
        for image in &mut old {
            self.destroy_image(image);
        }
        for i in 0..4 {
            self.scene_images[i] = self.image(
                width,
                height,
                if i % 2 == 0 {
                    format
                } else {
                    VK_FORMAT_D32_SFLOAT
                },
                i % 2 == 1,
            )?;
        }
        for i in 0..2 {
            self.scene_fbos[i] = self.framebuffer(
                self.scene_pass,
                &[
                    self.scene_images[i * 2].view,
                    self.scene_images[i * 2 + 1].view,
                ],
                width,
                height,
            )?;
        }
        self.scene_extent = (width, height);
        Ok(())
    }
    pub fn scene_pass(&self) -> VkRenderPass {
        self.scene_pass
    }
    pub fn scene_framebuffer(&self, reflection: bool) -> VkFramebuffer {
        self.scene_fbos[reflection as usize]
    }
    pub fn scene_attachments(
        &self,
        output: VkImageView,
        format: VkFormat,
        reflection: bool,
    ) -> VkAttachments {
        VkAttachments {
            opaque_color_view: self.scene_images[0].view,
            opaque_depth_view: self.scene_images[1].view,
            reflection_view: reflection.then_some(self.scene_images[2].view),
            output_view: output,
            color_format: format,
            depth_format: VK_FORMAT_D32_SFLOAT,
        }
    }
    pub fn reflection_set(&self) -> VkDescriptorSet {
        self.reflection_set
    }
    pub unsafe fn reflected_geometry(
        &mut self,
        triangles: &[[f32; 24]],
        layout: VkDescriptorSetLayout,
        texture: VkBuffer,
    ) -> Result<(), String> {
        if self.reflection_buffer.handle.is_null() {
            self.reflection_buffer =
                self.buffer(crate::scene_limits::REFLECTION_BUFFER_BYTES, VK_BUFFER_USAGE_STORAGE_BUFFER_BIT)?;
            let size = VkDescriptorPoolSize {
                type_: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                descriptorCount: 2,
            };
            check(
                (self.api.vkCreateDescriptorPool)(
                    self.device,
                    &VkDescriptorPoolCreateInfo {
                        sType: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
                        maxSets: 1,
                        poolSizeCount: 1,
                        pPoolSizes: &size,
                        ..Default::default()
                    },
                    ptr::null(),
                    &mut self.reflection_pool,
                ),
                "reflection scene pool",
            )?;
            check(
                (self.api.vkAllocateDescriptorSets)(
                    self.device,
                    &VkDescriptorSetAllocateInfo {
                        sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
                        descriptorPool: self.reflection_pool,
                        descriptorSetCount: 1,
                        pSetLayouts: &layout,
                        ..Default::default()
                    },
                    &mut self.reflection_set,
                ),
                "reflection scene set",
            )?;
            let buffers = [
                VkDescriptorBufferInfo {
                    buffer: self.reflection_buffer.handle,
                    offset: 0,
                    range: crate::scene_limits::REFLECTION_BUFFER_BYTES,
                },
                VkDescriptorBufferInfo {
                    buffer: texture,
                    offset: 0,
                    range: crate::scene_assets::SCENE_BYTES as u64,
                },
            ];
            let writes: Vec<_> = buffers
                .iter()
                .enumerate()
                .map(|(i, buffer)| VkWriteDescriptorSet {
                    sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                    dstSet: self.reflection_set,
                    dstBinding: i as u32,
                    descriptorCount: 1,
                    descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                    pBufferInfo: buffer,
                    ..Default::default()
                })
                .collect();
            (self.api.vkUpdateDescriptorSets)(self.device, 2, writes.as_ptr(), 0, ptr::null());
        }
        let mut bytes = Vec::with_capacity(triangles.len() * 96);
        for triangle in triangles {
            for i in 0..3 {
                for n in triangle[i * 4..i * 4 + 4]
                    .iter()
                    .chain(triangle[12 + i * 4..16 + i * 4].iter())
                {
                    bytes.extend(n.to_ne_bytes());
                }
            }
        }
        self.upload(&self.reflection_buffer, &bytes)?;
        self.reflection_count = (triangles.len() * 3) as u32;
        Ok(())
    }
    pub unsafe fn release(&mut self) {
        for fbo in &mut self.scene_fbos {
            if !fbo.is_null() {
                (self.api.vkDestroyFramebuffer)(self.device, *fbo, ptr::null());
                *fbo = ptr::null_mut();
            }
        }
        if !self.scene_pass.is_null() {
            (self.api.vkDestroyRenderPass)(self.device, self.scene_pass, ptr::null());
            self.scene_pass = ptr::null_mut();
        }
        let mut images = std::mem::replace(
            &mut self.scene_images,
            std::array::from_fn(|_| Image::default()),
        );
        for image in &mut images {
            self.destroy_image(image);
        }
        if !self.reflection_pool.is_null() {
            (self.api.vkDestroyDescriptorPool)(self.device, self.reflection_pool, ptr::null());
            self.reflection_pool = ptr::null_mut();
        }
        let mut buffer = std::mem::take(&mut self.reflection_buffer);
        self.destroy_buffer(&mut buffer);
        let mut slots = std::mem::take(&mut self.slots);
        for s in &mut slots {
            self.destroy_slot(s);
        }
        for p in [&mut self.pipeline, &mut self.composite] {
            if !p.is_null() {
                (self.api.vkDestroyPipeline)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.water_pass, &mut self.composite_pass] {
            if !p.is_null() {
                (self.api.vkDestroyRenderPass)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.layout, &mut self.composite_layout] {
            if !p.is_null() {
                (self.api.vkDestroyPipelineLayout)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.set_layout, &mut self.composite_set_layout] {
            if !p.is_null() {
                (self.api.vkDestroyDescriptorSetLayout)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
        for p in [&mut self.sampler, &mut self.depth_sampler] {
            if !p.is_null() {
                (self.api.vkDestroySampler)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
        for p in &mut self.shaders {
            if !p.is_null() {
                (self.api.vkDestroyShaderModule)(self.device, *p, ptr::null());
                *p = ptr::null_mut();
            }
        }
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            self.release();
        }
    }
}
