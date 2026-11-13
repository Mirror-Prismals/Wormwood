//! Borrowed-device Vulkan water encoding. Parent owns device/queue/command
//! buffers, opaque/reflected scene passes, barriers, fences and presentation.
#![allow(non_snake_case, non_upper_case_globals, unsafe_op_in_unsafe_fn)]
use super::super::vk::*;
use crate::vegetation::packet::Packet;
use std::{
    ffi::{CStr, CString, c_void},
    path::Path,
    ptr,
};
fn check(r: VkResult, name: &str) -> Result<(), String> {
    if r == VK_SUCCESS {
        Ok(())
    } else {
        Err(format!("vegetation Vulkan {name}: {r}"))
    }
}
macro_rules! functions{($($name:ident:($($arg:ty),*)$(->$ret:ty)?;)+)=>{
struct Api{$($name:unsafe extern "system" fn($($arg),*)$(->$ret)?,)+}
impl Api{unsafe fn load(resolve:&mut dyn FnMut(&CStr)->PFN_vkVoidFunction)->Result<Self,String>{Ok(Self{$($name:{let name=CString::new(stringify!($name)).unwrap();let p=resolve(&name).ok_or_else(||format!("vegetation Vulkan {} unavailable",name.to_string_lossy()))?;std::mem::transmute_copy::<_,unsafe extern "system" fn($($arg),*)$(->$ret)?>(&p)},)+})}}
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

pub(crate) struct Client {
    api: Api,
    device: VkDevice,
    memory: VkPhysicalDeviceMemoryProperties,
    shaders: [VkShaderModule; 3],
    set_layout: VkDescriptorSetLayout,
    layout: VkPipelineLayout,
    color: VkPipeline,
    shadow: VkPipeline,
    shadow_pass: VkRenderPass,
    shadow_fbo: VkFramebuffer,
    depth: Image,
    sampler: VkSampler,
    pool: VkDescriptorPool,
    sets: [VkDescriptorSet; 2],
    uniforms: [Buffer; 2],
    vertex: Buffer,
    index: Buffer,
    impostors: Buffer,
    count: u32,
}
impl Client {
    pub unsafe fn new(
        device: VkDevice,
        materials: VkBuffer,
        memory: &VkPhysicalDeviceMemoryProperties,
        pass: VkRenderPass,
        resolve: &mut dyn FnMut(&CStr) -> PFN_vkVoidFunction,
        binaries: [&[u8]; 3],
    ) -> Result<Self, String> {
        let api = Api::load(resolve)?;
        let mut s = Self {
            api,
            device,
            memory: *memory,
            shaders: [ptr::null_mut(); 3],
            set_layout: ptr::null_mut(),
            layout: ptr::null_mut(),
            color: ptr::null_mut(),
            shadow: ptr::null_mut(),
            shadow_pass: ptr::null_mut(),
            shadow_fbo: ptr::null_mut(),
            depth: Image::default(),
            sampler: ptr::null_mut(),
            pool: ptr::null_mut(),
            sets: [ptr::null_mut(); 2],
            uniforms: std::array::from_fn(|_| Buffer::default()),
            vertex: Buffer::default(),
            index: Buffer::default(),
            impostors: Buffer::default(),
            count: 0,
        };
        for (i, bytes) in binaries.into_iter().enumerate() {
            s.shaders[i] = s.shader(bytes)?;
        }
        let bindings = [
            VkDescriptorSetLayoutBinding {
                binding: 0,
                descriptorType: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                descriptorCount: 1,
                stageFlags: VK_SHADER_STAGE_VERTEX_BIT | VK_SHADER_STAGE_FRAGMENT_BIT,
                ..Default::default()
            },
            VkDescriptorSetLayoutBinding {
                binding: 1,
                descriptorType: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                descriptorCount: 1,
                stageFlags: VK_SHADER_STAGE_FRAGMENT_BIT,
                ..Default::default()
            },
        ];
        let mut bindings = bindings.to_vec();
        bindings.push(VkDescriptorSetLayoutBinding {
            binding: 2,
            descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
            descriptorCount: 1,
            stageFlags: VK_SHADER_STAGE_FRAGMENT_BIT,
            ..Default::default()
        });
        bindings.push(VkDescriptorSetLayoutBinding{binding:3,descriptorType:VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,descriptorCount:1,stageFlags:VK_SHADER_STAGE_FRAGMENT_BIT,..Default::default()});
        s.set_layout = s.descriptor_layout(&bindings)?;
        check(
            (s.api.vkCreatePipelineLayout)(
                device,
                &VkPipelineLayoutCreateInfo {
                    sType: VK_STRUCTURE_TYPE_PIPELINE_LAYOUT_CREATE_INFO,
                    setLayoutCount: 1,
                    pSetLayouts: &s.set_layout,
                    ..Default::default()
                },
                ptr::null(),
                &mut s.layout,
            ),
            "vegetation layout",
        )?;
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
        let reference = VkAttachmentReference {
            attachment: 0,
            layout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let subpass = VkSubpassDescription {
            pipelineBindPoint: VK_PIPELINE_BIND_POINT_GRAPHICS,
            pDepthStencilAttachment: &reference,
            ..Default::default()
        };
        let dependencies = [
            VkSubpassDependency {
                srcSubpass: VK_SUBPASS_EXTERNAL as u32,
                dstSubpass: 0,
                srcStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                dstStageMask: VK_PIPELINE_STAGE_EARLY_FRAGMENT_TESTS_BIT,
                srcAccessMask: VK_ACCESS_SHADER_READ_BIT,
                dstAccessMask: VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                ..Default::default()
            },
            VkSubpassDependency {
                srcSubpass: 0,
                dstSubpass: VK_SUBPASS_EXTERNAL as u32,
                srcStageMask: VK_PIPELINE_STAGE_LATE_FRAGMENT_TESTS_BIT,
                dstStageMask: VK_PIPELINE_STAGE_FRAGMENT_SHADER_BIT,
                srcAccessMask: VK_ACCESS_DEPTH_STENCIL_ATTACHMENT_WRITE_BIT,
                dstAccessMask: VK_ACCESS_SHADER_READ_BIT,
                ..Default::default()
            },
        ];
        check(
            (s.api.vkCreateRenderPass)(
                device,
                &VkRenderPassCreateInfo {
                    sType: VK_STRUCTURE_TYPE_RENDER_PASS_CREATE_INFO,
                    attachmentCount: 1,
                    pAttachments: &depth,
                    subpassCount: 1,
                    pSubpasses: &subpass,
                    dependencyCount: 2,
                    pDependencies: dependencies.as_ptr(),
                    ..Default::default()
                },
                ptr::null(),
                &mut s.shadow_pass,
            ),
            "vegetation sunlight pass",
        )?;
        s.shadow = s.pipeline(s.shadow_pass, s.layout, true)?;
        s.color = s.pipeline(pass, s.layout, false)?;
        s.depth = s.image(2048, 2048, VK_FORMAT_D32_SFLOAT, true)?;
        s.shadow_fbo = s.framebuffer(s.shadow_pass, &[s.depth.view], 2048, 2048)?;
        check(
            (s.api.vkCreateSampler)(
                device,
                &VkSamplerCreateInfo {
                    sType: VK_STRUCTURE_TYPE_SAMPLER_CREATE_INFO,
                    magFilter: VK_FILTER_NEAREST,
                    minFilter: VK_FILTER_NEAREST,
                    mipmapMode: VK_SAMPLER_MIPMAP_MODE_NEAREST,
                    addressModeU: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                    addressModeV: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                    addressModeW: VK_SAMPLER_ADDRESS_MODE_CLAMP_TO_EDGE,
                    maxLod: 1.,
                    ..Default::default()
                },
                ptr::null(),
                &mut s.sampler,
            ),
            "vegetation sampler",
        )?;
        s.vertex = s.buffer(98304 * 64, VK_BUFFER_USAGE_VERTEX_BUFFER_BIT)?;
        s.index = s.buffer(98304 * 4, VK_BUFFER_USAGE_INDEX_BUFFER_BIT)?;
        for i in 0..2 {
            s.uniforms[i] = s.buffer(192, VK_BUFFER_USAGE_UNIFORM_BUFFER_BIT)?;
        }
        let tree_bytes=&crate::vegetation::impostor::BYTES[8..];
        s.impostors=s.buffer(tree_bytes.len()as u64,VK_BUFFER_USAGE_STORAGE_BUFFER_BIT)?;
        s.upload(&s.impostors,tree_bytes)?;
        let sizes = [
            VkDescriptorPoolSize {
                type_: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                descriptorCount: 4,
            },
            VkDescriptorPoolSize {
                type_: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                descriptorCount: 2,
            },
            VkDescriptorPoolSize {
                type_: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                descriptorCount: 2,
            },
        ];
        check(
            (s.api.vkCreateDescriptorPool)(
                device,
                &VkDescriptorPoolCreateInfo {
                    sType: VK_STRUCTURE_TYPE_DESCRIPTOR_POOL_CREATE_INFO,
                    maxSets: 2,
                    poolSizeCount: sizes.len() as u32,
                    pPoolSizes: sizes.as_ptr(),
                    ..Default::default()
                },
                ptr::null(),
                &mut s.pool,
            ),
            "vegetation pool",
        )?;
        let layouts = [s.set_layout; 2];
        check(
            (s.api.vkAllocateDescriptorSets)(
                device,
                &VkDescriptorSetAllocateInfo {
                    sType: VK_STRUCTURE_TYPE_DESCRIPTOR_SET_ALLOCATE_INFO,
                    descriptorPool: s.pool,
                    descriptorSetCount: 2,
                    pSetLayouts: layouts.as_ptr(),
                    ..Default::default()
                },
                s.sets.as_mut_ptr(),
            ),
            "vegetation sets",
        )?;
        for i in 0..2 {
            let buffer = VkDescriptorBufferInfo {
                buffer: s.uniforms[i].handle,
                offset: 0,
                range: 192,
            };
            let image = VkDescriptorImageInfo {
                sampler: s.sampler,
                imageView: s.depth.view,
                imageLayout: VK_IMAGE_LAYOUT_DEPTH_STENCIL_READ_ONLY_OPTIMAL,
            };
            let materials_info = VkDescriptorBufferInfo {
                buffer: materials,
                offset: 0,
                range: crate::scene_assets::SCENE_BYTES as u64,
            };
            let tree_info=VkDescriptorBufferInfo{buffer:s.impostors.handle,offset:0,range:tree_bytes.len()as u64};
            let writes = [
                VkWriteDescriptorSet{sType:VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,dstSet:s.sets[i],dstBinding:3,descriptorCount:1,descriptorType:VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,pBufferInfo:&tree_info,..Default::default()},
                VkWriteDescriptorSet {
                    sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                    dstSet: s.sets[i],
                    dstBinding: 2,
                    descriptorCount: 1,
                    descriptorType: VK_DESCRIPTOR_TYPE_STORAGE_BUFFER,
                    pBufferInfo: &materials_info,
                    ..Default::default()
                },
                VkWriteDescriptorSet {
                    sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                    dstSet: s.sets[i],
                    dstBinding: 0,
                    descriptorCount: 1,
                    descriptorType: VK_DESCRIPTOR_TYPE_UNIFORM_BUFFER,
                    pBufferInfo: &buffer,
                    ..Default::default()
                },
                VkWriteDescriptorSet {
                    sType: VK_STRUCTURE_TYPE_WRITE_DESCRIPTOR_SET,
                    dstSet: s.sets[i],
                    dstBinding: 1,
                    descriptorCount: 1,
                    descriptorType: VK_DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
                    pImageInfo: &image,
                    ..Default::default()
                },
            ];
            (s.api.vkUpdateDescriptorSets)(
                device,
                writes.len() as u32,
                writes.as_ptr(),
                0,
                ptr::null(),
            );
        }
        Ok(s)
    }
    pub unsafe fn set_packet(&mut self, packet: &Packet) -> Result<(), String> {
        self.upload(&self.vertex, &packet.vertex_bytes())?;
        self.upload(&self.index, &packet.index_bytes())?;
        let color = packet.scene.bytes()?;
        let mut shadow = color.clone();
        shadow[..64].copy_from_slice(&color[64..128]);
        self.upload(&self.uniforms[0], &color)?;
        self.upload(&self.uniforms[1], &shadow)?;
        self.count = packet.indices.len() as u32;
        Ok(())
    }
    unsafe fn bind(&self, command: VkCommandBuffer, shadow: bool, width: u32, height: u32) {
        (self.api.vkCmdBindPipeline)(
            command,
            VK_PIPELINE_BIND_POINT_GRAPHICS,
            if shadow { self.shadow } else { self.color },
        );
        (self.api.vkCmdBindDescriptorSets)(
            command,
            VK_PIPELINE_BIND_POINT_GRAPHICS,
            self.layout,
            0,
            1,
            &self.sets[shadow as usize],
            0,
            ptr::null(),
        );
        (self.api.vkCmdSetViewport)(
            command,
            0,
            1,
            &VkViewport {
                x: 0.,
                y: 0.,
                width: width as f32,
                height: height as f32,
                minDepth: 0.,
                maxDepth: 1.,
            },
        );
        (self.api.vkCmdSetScissor)(
            command,
            0,
            1,
            &VkRect2D {
                offset: VkOffset2D { x: 0, y: 0 },
                extent: VkExtent2D { width, height },
            },
        );
        (self.api.vkCmdBindVertexBuffers)(command, 0, 1, &self.vertex.handle, &0);
        (self.api.vkCmdBindIndexBuffer)(command, self.index.handle, 0, VK_INDEX_TYPE_UINT32);
        (self.api.vkCmdDrawIndexed)(command, self.count, 1, 0, 0, 0);
    }
    pub unsafe fn shadow(&self, command: VkCommandBuffer) {
        let clear = VkClearValue {
            depthStencil: VkClearDepthStencilValue {
                depth: 1.,
                stencil: 0,
            },
        };
        (self.api.vkCmdBeginRenderPass)(
            command,
            &VkRenderPassBeginInfo {
                sType: VK_STRUCTURE_TYPE_RENDER_PASS_BEGIN_INFO,
                renderPass: self.shadow_pass,
                framebuffer: self.shadow_fbo,
                renderArea: VkRect2D {
                    offset: VkOffset2D { x: 0, y: 0 },
                    extent: VkExtent2D {
                        width: 2048,
                        height: 2048,
                    },
                },
                clearValueCount: 1,
                pClearValues: &clear,
                ..Default::default()
            },
            VK_SUBPASS_CONTENTS_INLINE,
        );
        self.bind(command, true, 2048, 2048);
        (self.api.vkCmdEndRenderPass)(command);
    }
    pub unsafe fn color(&self, command: VkCommandBuffer, width: u32, height: u32) {
        self.bind(command, false, width, height);
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
    unsafe fn pipeline(
        &self,
        pass: VkRenderPass,
        layout: VkPipelineLayout,
        shadow: bool,
    ) -> Result<VkPipeline, String> {
        let stages = [
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_VERTEX_BIT,
                module: self.shaders[0],
                pName: c"main".as_ptr(),
                ..Default::default()
            },
            VkPipelineShaderStageCreateInfo {
                sType: VK_STRUCTURE_TYPE_PIPELINE_SHADER_STAGE_CREATE_INFO,
                stage: VK_SHADER_STAGE_FRAGMENT_BIT,
                module: self.shaders[if shadow { 2 } else { 1 }],
                pName: c"main".as_ptr(),
                ..Default::default()
            },
        ];
        let binding = VkVertexInputBindingDescription {
            binding: 0,
            stride: 64,
            inputRate: VK_VERTEX_INPUT_RATE_VERTEX,
        };
        let attributes: Vec<_> = [
            (VK_FORMAT_R32G32B32_SFLOAT, 0),
            (VK_FORMAT_R32G32B32_SFLOAT, 12),
            (VK_FORMAT_R32G32_SFLOAT, 24),
            (VK_FORMAT_R32G32B32_SFLOAT, 32),
            (VK_FORMAT_R32G32B32A32_SFLOAT, 44),
            (VK_FORMAT_R32_SFLOAT, 60),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (format, offset))| VkVertexInputAttributeDescription {
            location: i as u32,
            binding: 0,
            format,
            offset,
        })
        .collect();
        let vertex = VkPipelineVertexInputStateCreateInfo {
            sType: VK_STRUCTURE_TYPE_PIPELINE_VERTEX_INPUT_STATE_CREATE_INFO,
            vertexBindingDescriptionCount: 1,
            pVertexBindingDescriptions: &binding,
            vertexAttributeDescriptionCount: 6,
            pVertexAttributeDescriptions: attributes.as_ptr(),
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
            depthTestEnable: 1,
            depthWriteEnable: 1,
            depthCompareOp: VK_COMPARE_OP_LESS,
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
            attachmentCount: if shadow { 0 } else { 1 },
            pAttachments: if shadow { ptr::null() } else { &attachment },
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
            pDepthStencilState: &depth,
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
}
impl Drop for Client {
    fn drop(&mut self) {
        unsafe {
            for pipeline in [self.color, self.shadow] {
                if !pipeline.is_null() {
                    (self.api.vkDestroyPipeline)(self.device, pipeline, ptr::null());
                }
            }
            if !self.shadow_fbo.is_null() {
                (self.api.vkDestroyFramebuffer)(self.device, self.shadow_fbo, ptr::null());
            }
            if !self.shadow_pass.is_null() {
                (self.api.vkDestroyRenderPass)(self.device, self.shadow_pass, ptr::null());
            }
            if !self.pool.is_null() {
                (self.api.vkDestroyDescriptorPool)(self.device, self.pool, ptr::null());
            }
            if !self.sampler.is_null() {
                (self.api.vkDestroySampler)(self.device, self.sampler, ptr::null());
            }
            let mut trees=std::mem::take(&mut self.impostors);
            self.destroy_buffer(&mut trees);
            let mut image = std::mem::take(&mut self.depth);
            self.destroy_image(&mut image);
            let mut buffers = vec![
                std::mem::take(&mut self.vertex),
                std::mem::take(&mut self.index),
            ];
            for b in &mut self.uniforms {
                buffers.push(std::mem::take(b));
            }
            for mut b in buffers {
                self.destroy_buffer(&mut b);
            }
            if !self.layout.is_null() {
                (self.api.vkDestroyPipelineLayout)(self.device, self.layout, ptr::null());
            }
            if !self.set_layout.is_null() {
                (self.api.vkDestroyDescriptorSetLayout)(self.device, self.set_layout, ptr::null());
            }
            for shader in self.shaders {
                if !shader.is_null() {
                    (self.api.vkDestroyShaderModule)(self.device, shader, ptr::null());
                }
            }
        }
    }
}
