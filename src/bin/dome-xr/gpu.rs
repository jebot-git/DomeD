//! Vulkan DMA-BUF import and GPU panel effects. No host mapping of image pixels.
use super::Panel;
use ash::{vk, Device, Instance};
use dome::xr_gpu::Received;
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    os::fd::{AsRawFd, IntoRawFd},
    sync::Arc,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn extensions(instance: &Instance, physical: vk::PhysicalDevice) -> Option<Vec<*const i8>> {
    let required = [
        ash::khr::external_memory_fd::NAME,
        ash::ext::external_memory_dma_buf::NAME,
        ash::ext::image_drm_format_modifier::NAME,
        ash::ext::queue_family_foreign::NAME,
        ash::ext::physical_device_drm::NAME,
        ash::khr::image_format_list::NAME,
    ];
    let available = unsafe {
        instance
            .enumerate_device_extension_properties(physical)
            .ok()?
    };
    if !required.iter().all(|name| {
        available
            .iter()
            .any(|p| unsafe { CStr::from_ptr(p.extension_name.as_ptr()) } == *name)
    }) {
        return None;
    }
    Some(required.iter().map(|n| n.as_ptr()).collect())
}
struct Imported {
    device: Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    descriptor: vk::DescriptorSet,
    pool: vk::DescriptorPool,
    frame: Arc<Received>,
}
impl Drop for Imported {
    fn drop(&mut self) {
        unsafe {
            if self.descriptor != vk::DescriptorSet::null() {
                let _ = self
                    .device
                    .free_descriptor_sets(self.pool, &[self.descriptor]);
            }
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
            // Frame's socket closes only after the imported GPU resources are gone.
        }
    }
}
pub struct Renderer {
    device: Device,
    instance: Instance,
    physical: vk::PhysicalDevice,
    queue_family: u32,
    format: vk::Format,
    descriptor_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    fence: vk::Fence,
    imported: HashMap<u64, Imported>,
}
impl Renderer {
    pub unsafe fn new(
        instance: &Instance,
        device: &Device,
        physical: vk::PhysicalDevice,
        queue_family: u32,
        format: vk::Format,
    ) -> Result<Self> {
        // Initialize nullable handles so errors are cleaned up by Drop.
        let mut r = Self {
            device: device.clone(),
            instance: instance.clone(),
            physical,
            queue_family,
            format,
            descriptor_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            sampler: vk::Sampler::null(),
            render_pass: vk::RenderPass::null(),
            layout: vk::PipelineLayout::null(),
            pipeline: vk::Pipeline::null(),
            fence: vk::Fence::null(),
            imported: HashMap::new(),
        };
        r.descriptor_layout = device.create_descriptor_set_layout(
            &vk::DescriptorSetLayoutCreateInfo::default().bindings(&[
                vk::DescriptorSetLayoutBinding::default()
                    .binding(0)
                    .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::FRAGMENT),
            ]),
            None,
        )?;
        r.descriptor_pool = device.create_descriptor_pool(
            &vk::DescriptorPoolCreateInfo::default()
                .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
                .max_sets(128)
                .pool_sizes(&[vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 128,
                }]),
            None,
        )?;
        r.sampler = device.create_sampler(
            &vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .max_lod(0.),
            None,
        )?;
        let attachments = [vk::AttachmentDescription::default()
            .format(format)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::DONT_CARE)
            .store_op(vk::AttachmentStoreOp::STORE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
        let refs = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&refs)];
        r.render_pass = device.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&attachments)
                .subpasses(&subpasses),
            None,
        )?;
        r.layout = device.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&[r.descriptor_layout])
                .push_constant_ranges(&[vk::PushConstantRange {
                    stage_flags: vk::ShaderStageFlags::FRAGMENT,
                    offset: 0,
                    size: 64,
                }]),
            None,
        )?;
        let vertex = ash::util::read_spv(&mut std::io::Cursor::new(include_bytes!(
            "shaders/panel.vert.spv"
        )))?;
        let fragment = ash::util::read_spv(&mut std::io::Cursor::new(include_bytes!(
            "shaders/panel.frag.spv"
        )))?;
        let vs = device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vertex), None)?;
        let fs = match device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&fragment), None)
        {
            Ok(s) => s,
            Err(e) => {
                device.destroy_shader_module(vs, None);
                return Err(e.into());
            }
        };
        let main = CString::new("main")?;
        let stages = [
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::VERTEX)
                .module(vs)
                .name(&main),
            vk::PipelineShaderStageCreateInfo::default()
                .stage(vk::ShaderStageFlags::FRAGMENT)
                .module(fs)
                .name(&main),
        ];
        let vertex_state = vk::PipelineVertexInputStateCreateInfo::default();
        let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
        let viewport = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);
        let raster = vk::PipelineRasterizationStateCreateInfo::default()
            .polygon_mode(vk::PolygonMode::FILL)
            .cull_mode(vk::CullModeFlags::NONE)
            .line_width(1.);
        let ms = vk::PipelineMultisampleStateCreateInfo::default()
            .rasterization_samples(vk::SampleCountFlags::TYPE_1);
        let blends = [vk::PipelineColorBlendAttachmentState::default()
            .color_write_mask(vk::ColorComponentFlags::RGBA)];
        let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blends);
        let dynamic = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
        let pipeline = device.create_graphics_pipelines(
            vk::PipelineCache::null(),
            &[vk::GraphicsPipelineCreateInfo::default()
                .stages(&stages)
                .vertex_input_state(&vertex_state)
                .input_assembly_state(&assembly)
                .viewport_state(&viewport)
                .rasterization_state(&raster)
                .multisample_state(&ms)
                .color_blend_state(&blend)
                .dynamic_state(&dynamic)
                .layout(r.layout)
                .render_pass(r.render_pass)],
            None,
        );
        device.destroy_shader_module(vs, None);
        device.destroy_shader_module(fs, None);
        r.pipeline = pipeline.map_err(|(partial, e)| {
            for p in partial {
                device.destroy_pipeline(p, None);
            }
            e
        })?[0];
        r.fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
        Ok(r)
    }
    unsafe fn import(&self, frame: Arc<Received>) -> Result<Imported> {
        let info = &frame.info;
        let mut drm = vk::PhysicalDeviceDrmPropertiesEXT::default();
        self.instance.get_physical_device_properties2(
            self.physical,
            &mut vk::PhysicalDeviceProperties2::default().push_next(&mut drm),
        );
        if drm.has_render == 0
            || nix::sys::stat::major(info.device) != drm.render_major as u64
            || nix::sys::stat::minor(info.device) != drm.render_minor as u64
        {
            return Err("Compositor and OpenXR use different GPUs".into());
        }
        // DRM ARGB8888 is BGRA in little-endian byte order.
        if info.fourcc != u32::from_le_bytes(*b"AR24") {
            return Err("Unsupported DMA-BUF format/modifier".into());
        }
        let format = vk::Format::B8G8R8A8_UNORM;
        let handle_type = vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT;
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default();
        self.instance.get_physical_device_format_properties2(
            self.physical,
            format,
            &mut vk::FormatProperties2::default().push_next(&mut list),
        );
        let mut modifiers = vec![
            vk::DrmFormatModifierPropertiesEXT::default();
            list.drm_format_modifier_count as usize
        ];
        let mut list = vk::DrmFormatModifierPropertiesListEXT::default()
            .drm_format_modifier_properties(&mut modifiers);
        self.instance.get_physical_device_format_properties2(
            self.physical,
            format,
            &mut vk::FormatProperties2::default().push_next(&mut list),
        );
        if !modifiers.iter().any(|m| {
            m.drm_format_modifier == info.modifier
                && m.drm_format_modifier_plane_count == 1
                && m.drm_format_modifier_tiling_features.contains(
                    vk::FormatFeatureFlags::SAMPLED_IMAGE
                        | vk::FormatFeatureFlags::SAMPLED_IMAGE_FILTER_LINEAR,
                )
        }) {
            return Err("DMA-BUF layout does not support filtered single-plane sampling".into());
        }
        let mut modifier_query = vk::PhysicalDeviceImageDrmFormatModifierInfoEXT::default()
            .drm_format_modifier(info.modifier)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let mut external_query =
            vk::PhysicalDeviceExternalImageFormatInfo::default().handle_type(handle_type);
        let query = vk::PhysicalDeviceImageFormatInfo2::default()
            .format(format)
            .ty(vk::ImageType::TYPE_2D)
            .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .push_next(&mut modifier_query)
            .push_next(&mut external_query);
        let mut external_props = vk::ExternalImageFormatProperties::default();
        let mut props = vk::ImageFormatProperties2::default().push_next(&mut external_props);
        self.instance.get_physical_device_image_format_properties2(
            self.physical,
            &query,
            &mut props,
        )?;
        if !external_props
            .external_memory_properties
            .external_memory_features
            .contains(vk::ExternalMemoryFeatureFlags::IMPORTABLE)
        {
            return Err("DMA-BUF format cannot be imported".into());
        }
        let planes = [vk::SubresourceLayout {
            offset: info.offset as u64,
            row_pitch: info.stride as u64,
            ..Default::default()
        }];
        let mut modifier = vk::ImageDrmFormatModifierExplicitCreateInfoEXT::default()
            .drm_format_modifier(info.modifier)
            .plane_layouts(&planes);
        let mut external = vk::ExternalMemoryImageCreateInfo::default().handle_types(handle_type);
        let image = self.device.create_image(
            &vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D {
                    width: info.width,
                    height: info.height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .tiling(vk::ImageTiling::DRM_FORMAT_MODIFIER_EXT)
                .usage(vk::ImageUsageFlags::SAMPLED)
                .sharing_mode(vk::SharingMode::EXCLUSIVE)
                .push_next(&mut modifier)
                .push_next(&mut external),
            None,
        )?;
        let mut imported = Imported {
            device: self.device.clone(),
            image,
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
            descriptor: vk::DescriptorSet::null(),
            pool: self.descriptor_pool,
            frame,
        };
        let req = self.device.get_image_memory_requirements(image);
        let fd = imported.frame.fd.try_clone()?;
        let mut fd_props = vk::MemoryFdPropertiesKHR::default();
        let api = ash::khr::external_memory_fd::Device::new(&self.instance, &self.device);
        api.get_memory_fd_properties(handle_type, fd.as_raw_fd(), &mut fd_props)?;
        let bits = req.memory_type_bits & fd_props.memory_type_bits;
        let memory_type = (0..32)
            .find(|i| bits & (1 << i) != 0)
            .ok_or("No DMA-BUF memory type")?;
        let mut import = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(handle_type)
            .fd(fd.as_raw_fd());
        let mut dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        imported.memory = self.device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(memory_type)
                .push_next(&mut import)
                .push_next(&mut dedicated),
            None,
        )?;
        // Vulkan takes ownership only after successful allocation.
        let _ = fd.into_raw_fd();
        self.device.bind_image_memory(image, imported.memory, 0)?;
        imported.view = self.device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(color_range()),
            None,
        )?;
        imported.descriptor = self.device.allocate_descriptor_sets(
            &vk::DescriptorSetAllocateInfo::default()
                .descriptor_pool(self.descriptor_pool)
                .set_layouts(&[self.descriptor_layout]),
        )?[0];
        let images = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(imported.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        self.device.update_descriptor_sets(
            &[vk::WriteDescriptorSet::default()
                .dst_set(imported.descriptor)
                .dst_binding(0)
                .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .image_info(&images)],
            &[],
        );
        Ok(imported)
    }
    unsafe fn prepare(&mut self, id: u64, frame: Arc<Received>) -> Result<()> {
        if self
            .imported
            .get(&id)
            .is_none_or(|i| i.frame.info.serial != frame.info.serial)
        {
            // The previous draw's fence has completed before replacing this import.
            self.imported.remove(&id);
            let imported = self.import(frame)?;
            self.imported.insert(id, imported);
        }
        Ok(())
    }
    pub fn invalidate(&mut self, id: u64) {
        self.imported.remove(&id);
    }
    pub fn retain(&mut self, ids: &[u64]) {
        self.imported.retain(|id, _| ids.contains(id));
    }
    pub unsafe fn draw(
        &mut self,
        id: u64,
        frame: Arc<Received>,
        panel: &mut Panel,
        queue: vk::Queue,
        command: vk::CommandBuffer,
        opacity: u8,
        border: u8,
        focused: bool,
        stereo: bool,
        accent: [u8; 3],
    ) -> Result<()> {
        self.prepare(id, frame.clone())?;
        let index = panel.chain.acquire_image()?;
        panel.chain.wait_image(openxr::Duration::INFINITE)?;
        let result = self.draw_target(
            id,
            frame,
            panel.images[index as usize],
            panel.width,
            panel.height,
            queue,
            command,
            opacity,
            border,
            focused,
            stereo,
            accent,
        );
        let released = panel.chain.release_image();
        result?;
        released?;
        Ok(())
    }
    unsafe fn draw_target(
        &mut self,
        id: u64,
        frame: Arc<Received>,
        target: vk::Image,
        width: u32,
        height: u32,
        queue: vk::Queue,
        command: vk::CommandBuffer,
        opacity: u8,
        border: u8,
        focused: bool,
        stereo: bool,
        accent: [u8; 3],
    ) -> Result<()> {
        self.prepare(id, frame)?;
        let source = &self.imported[&id];
        let info = &source.frame.info;
        let view = self.device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(target)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(self.format)
                .subresource_range(color_range()),
            None,
        )?;
        let framebuffer = match self.device.create_framebuffer(
            &vk::FramebufferCreateInfo::default()
                .render_pass(self.render_pass)
                .attachments(&[view])
                .width(width)
                .height(height)
                .layers(1),
            None,
        ) {
            Ok(f) => f,
            Err(e) => {
                self.device.destroy_image_view(view, None);
                return Err(e.into());
            }
        };
        let result = (|| -> Result<()> {
            self.device.reset_fences(&[self.fence])?;
            self.device
                .reset_command_buffer(command, vk::CommandBufferResetFlags::empty())?;
            self.device.begin_command_buffer(
                command,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
            let acquire = vk::ImageMemoryBarrier::default()
                .image(source.image)
                .subresource_range(color_range())
                .old_layout(vk::ImageLayout::GENERAL)
                .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .src_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .dst_queue_family_index(self.queue_family)
                .dst_access_mask(vk::AccessFlags::SHADER_READ);
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[acquire],
            );
            let area = vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: vk::Extent2D { width, height },
            };
            self.device.cmd_begin_render_pass(
                command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(framebuffer)
                    .render_area(area),
                vk::SubpassContents::INLINE,
            );
            self.device
                .cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, self.pipeline);
            self.device.cmd_set_viewport(
                command,
                0,
                &[vk::Viewport {
                    x: 0.,
                    y: 0.,
                    width: width as f32,
                    height: height as f32,
                    min_depth: 0.,
                    max_depth: 1.,
                }],
            );
            self.device.cmd_set_scissor(command, 0, &[area]);
            self.device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::GRAPHICS,
                self.layout,
                0,
                &[source.descriptor],
                &[],
            );
            let viewport = info.viewport;
            let output_height = viewport.map_or(info.logical_height as f32, |v| v.height);
            let params: [f32; 16] = [
                info.logical_width as f32,
                output_height,
                info.logical_height as f32,
                f32::from(info.y_inverted),
                opacity as f32 / 255.,
                border as f32,
                f32::from(focused),
                f32::from(stereo),
                accent[0] as f32 / 255.,
                accent[1] as f32 / 255.,
                accent[2] as f32 / 255.,
                viewport.map_or(0., |v| v.band()),
                viewport.map_or(0., |v| v.offset),
                viewport.map_or(0., |v| v.max_offset()),
                f32::from(self.format == vk::Format::R8G8B8A8_SRGB),
                0.,
            ];
            let bytes = std::slice::from_raw_parts(params.as_ptr().cast::<u8>(), 64);
            self.device.cmd_push_constants(
                command,
                self.layout,
                vk::ShaderStageFlags::FRAGMENT,
                0,
                bytes,
            );
            self.device.cmd_draw(command, 3, 1, 0, 0);
            self.device.cmd_end_render_pass(command);
            let release = acquire
                .old_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                .new_layout(vk::ImageLayout::GENERAL)
                .src_queue_family_index(self.queue_family)
                .dst_queue_family_index(vk::QUEUE_FAMILY_FOREIGN_EXT)
                .src_access_mask(vk::AccessFlags::SHADER_READ)
                .dst_access_mask(vk::AccessFlags::empty());
            self.device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::FRAGMENT_SHADER,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                vk::DependencyFlags::empty(),
                &[],
                &[],
                &[release],
            );
            self.device.end_command_buffer(command)?;
            self.device.queue_submit(
                queue,
                &[vk::SubmitInfo::default().command_buffers(&[command])],
                self.fence,
            )?;
            // This fence covers this panel only; never idle the entire GPU queue.
            self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
            Ok(())
        })();
        self.device.destroy_framebuffer(framebuffer, None);
        self.device.destroy_image_view(view, None);
        result
    }
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            self.imported.clear();
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.descriptor_layout, None);
        }
    }
}
fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1)
}

#[cfg(all(test, feature = "nested"))]
mod tests {
    use super::*;
    use smithay::{
        backend::{
            allocator::{
                gbm::{GbmAllocator, GbmBufferFlags, GbmDevice},
                Buffer,
            },
            egl::{EGLContext, EGLDevice, EGLDisplay},
            renderer::{gles::GlesRenderer, Bind, Frame, Renderer as _},
        },
        utils::{Rectangle, Transform},
    };
    use std::{
        io::Read,
        os::{fd::AsRawFd, unix::net::UnixListener},
    };

    /// Executes the real GLES -> DMA-BUF -> Unix FD -> Vulkan shader path without
    /// a headset. Readback is used only here to assert the rendered test pixels.
    #[test]
    #[ignore = "requires a render node and Vulkan DMA-BUF import"]
    fn hardware_dmabuf_render() -> Result<()> {
        unsafe {
            let entry = ash::Entry::load()?;
            let instance = entry.create_instance(
                &vk::InstanceCreateInfo::default().application_info(
                    &vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_1),
                ),
                None,
            )?;
            let physical = instance
                .enumerate_physical_devices()?
                .into_iter()
                .find(|p| extensions(&instance, *p).is_some())
                .ok_or("No DMA-BUF GPU")?;
            let ext = extensions(&instance, physical).unwrap();
            let family = instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .unwrap() as u32;
            let device = instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default()
                    .enabled_extension_names(&ext)
                    .queue_create_infos(&[vk::DeviceQueueCreateInfo::default()
                        .queue_family_index(family)
                        .queue_priorities(&[1.])]),
                None,
            )?;
            let queue = device.get_device_queue(family, 0);
            let mut drm_props = vk::PhysicalDeviceDrmPropertiesEXT::default();
            instance.get_physical_device_properties2(
                physical,
                &mut vk::PhysicalDeviceProperties2::default().push_next(&mut drm_props),
            );
            let path =
                std::path::PathBuf::from(format!("/dev/dri/renderD{}", drm_props.render_minor));
            let egl_device = EGLDevice::enumerate()?
                .find(|d| d.render_device_path().ok().as_ref() == Some(&path))
                .ok_or("No matching EGL GPU")?;
            let display = EGLDisplay::new(egl_device)?;
            let context = EGLContext::new(&display)?;
            let mut gl = GlesRenderer::new(context)?;
            let file = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)?;
            let mut allocator = GbmAllocator::new(
                GbmDevice::new(file)?,
                GbmBufferFlags::RENDERING | GbmBufferFlags::LINEAR,
            );
            let mut dma = dome::xr_gpu::allocate_capture(&mut gl, &mut allocator, 96, 64)?;
            let mut target = gl.bind(&mut dma)?;
            let mut frame = gl.render(&mut target, (96, 64).into(), Transform::Normal)?;
            frame.clear(
                [0.25, 0.5, 0.75, 1.].into(),
                &[Rectangle::from_size((96, 64).into())],
            )?;
            // Distinct upper and lower stripes catch inverted image interpretation.
            frame.clear(
                [1., 0., 0., 1.].into(),
                &[Rectangle::new((0, 0).into(), (96, 16).into())],
            )?;
            frame.clear(
                [0., 1., 0., 1.].into(),
                &[Rectangle::new((0, 48).into(), (96, 16).into())],
            )?;
            for x in 0..96 {
                let c = if x % 2 == 0 {
                    [0., 0., 0., 1.]
                } else {
                    [1., 1., 1., 1.]
                };
                frame.clear(c.into(), &[Rectangle::new((x, 28).into(), (1, 2).into())])?;
            }
            frame.finish()?.wait()?;
            drop(target);
            let info = dome::xr_gpu::FrameInfo {
                version: 1,
                serial: 1,
                device: nix::sys::stat::makedev(
                    drm_props.render_major as u64,
                    drm_props.render_minor as u64,
                ),
                width: 96,
                height: 64,
                logical_width: 48,
                logical_height: 32,
                fourcc: u32::from_le_bytes(*b"AR24"),
                modifier: dma.format().modifier.into(),
                stride: dma.strides().next().unwrap(),
                offset: dma.offsets().next().unwrap(),
                y_inverted: dma.y_inverted(),
                viewport: None,
            };
            let path =
                std::env::temp_dir().join(format!("dome-gpu-render-{}.sock", std::process::id()));
            let _ = std::fs::remove_file(&path);
            let listener = UnixListener::bind(&path)?;
            let producer = std::thread::spawn(move || {
                let (mut s, _) = listener.accept().unwrap();
                let mut id = [0; 8];
                s.read_exact(&mut id).unwrap();
                dome::xr_gpu::send_frame(&mut s, &info, dma.handles().next().unwrap().as_raw_fd())
                    .unwrap();
                assert_eq!(s.read(&mut [0]).unwrap(), 0);
            });
            let mut received = Arc::new(dome::xr_gpu::receive(&path, 1)?);
            let format = vk::Format::R8G8B8A8_SRGB;
            let mut renderer = Renderer::new(&instance, &device, physical, family, format)?;
            let output = device.create_image(
                &vk::ImageCreateInfo::default()
                    .image_type(vk::ImageType::TYPE_2D)
                    .format(format)
                    .extent(vk::Extent3D {
                        width: 96,
                        height: 64,
                        depth: 1,
                    })
                    .mip_levels(1)
                    .array_layers(1)
                    .samples(vk::SampleCountFlags::TYPE_1)
                    .tiling(vk::ImageTiling::OPTIMAL)
                    .usage(
                        vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    ),
                None,
            )?;
            let props = instance.get_physical_device_memory_properties(physical);
            let req = device.get_image_memory_requirements(output);
            let ty = (0..props.memory_type_count)
                .find(|i| req.memory_type_bits & (1 << i) != 0)
                .unwrap();
            let memory = device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            )?;
            device.bind_image_memory(output, memory, 0)?;
            let pool = device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .queue_family_index(family)
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
                None,
            )?;
            let command = device.allocate_command_buffers(
                &vk::CommandBufferAllocateInfo::default()
                    .command_pool(pool)
                    .command_buffer_count(1),
            )?[0];
            let buffer = device.create_buffer(
                &vk::BufferCreateInfo::default()
                    .size(96 * 64 * 4)
                    .usage(vk::BufferUsageFlags::TRANSFER_DST),
                None,
            )?;
            let req = device.get_buffer_memory_requirements(buffer);
            let ty = (0..props.memory_type_count)
                .find(|i| {
                    req.memory_type_bits & (1 << i) != 0
                        && props.memory_types[*i as usize].property_flags.contains(
                            vk::MemoryPropertyFlags::HOST_VISIBLE
                                | vk::MemoryPropertyFlags::HOST_COHERENT,
                        )
                })
                .unwrap();
            let host = device.allocate_memory(
                &vk::MemoryAllocateInfo::default()
                    .allocation_size(req.size)
                    .memory_type_index(ty),
                None,
            )?;
            device.bind_buffer_memory(buffer, host, 0)?;
            let read_pixels = |device: &Device| -> Result<Vec<u8>> {
                device.reset_command_buffer(command, vk::CommandBufferResetFlags::empty())?;
                device.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default())?;
                let barrier = vk::ImageMemoryBarrier::default()
                    .image(output)
                    .subresource_range(color_range())
                    .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
                    .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
                device.cmd_pipeline_barrier(
                    command,
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                device.cmd_copy_image_to_buffer(
                    command,
                    output,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D {
                            width: 96,
                            height: 64,
                            depth: 1,
                        })],
                );
                device.end_command_buffer(command)?;
                let fence = device.create_fence(&vk::FenceCreateInfo::default(), None)?;
                device.queue_submit(
                    queue,
                    &[vk::SubmitInfo::default().command_buffers(&[command])],
                    fence,
                )?;
                device.wait_for_fences(&[fence], true, u64::MAX)?;
                device.destroy_fence(fence, None);
                let ptr = device.map_memory(host, 0, 96 * 64 * 4, vk::MemoryMapFlags::empty())?;
                let result = std::slice::from_raw_parts(ptr.cast::<u8>(), 96 * 64 * 4).to_vec();
                device.unmap_memory(host);
                Ok(result)
            };
            renderer.draw_target(
                1,
                received.clone(),
                output,
                96,
                64,
                queue,
                command,
                255,
                0,
                false,
                false,
                [0; 3],
            )?;
            let pixels = read_pixels(&device)?;
            let at = |x: usize, y: usize| &pixels[(y * 96 + x) * 4..(y * 96 + x) * 4 + 4];
            assert_eq!(at(48, 8), [255, 0, 0, 255], "top orientation");
            assert_eq!(at(48, 56), [0, 255, 0, 255], "bottom orientation");
            for x in 2..94 {
                assert_eq!(
                    at(x, 28),
                    if x % 2 == 0 {
                        &[0, 0, 0, 255]
                    } else {
                        &[255, 255, 255, 255]
                    },
                    "one-pixel detail"
                );
            }
            for (a, b) in at(48, 32).iter().zip([64u8, 128, 191, 255]) {
                assert!(
                    (*a as i16 - b as i16).abs() <= 1,
                    "channel/gamma {:?}",
                    at(48, 32)
                );
            }
            renderer.draw_target(
                1,
                received.clone(),
                output,
                96,
                64,
                queue,
                command,
                128,
                1,
                true,
                false,
                [0, 255, 0],
            )?;
            let pixels = read_pixels(&device)?;
            let center = &pixels[(32 * 96 + 48) * 4..(32 * 96 + 48) * 4 + 4];
            for (a, b) in center.iter().zip([32u8, 64, 96, 128]) {
                assert!((*a as i16 - b as i16).abs() <= 1, "opacity {center:?}");
            }
            assert_ne!(&pixels[0..4], center, "border/focus effect");
            renderer.draw_target(
                1,
                received.clone(),
                output,
                96,
                64,
                queue,
                command,
                255,
                0,
                true,
                true,
                [0, 255, 0],
            )?;
            let pixels = read_pixels(&device)?;
            let seam = &pixels[(32 * 96 + 48) * 4..(32 * 96 + 48) * 4 + 4];
            assert!(
                seam[1] > 190 && seam[0] < 40,
                "separate stereo focus edge {seam:?}"
            );
            renderer.invalidate(1);
            let f = Arc::get_mut(&mut received).unwrap();
            f.info.serial += 1;
            f.info.viewport = Some(dome::context_menu::Viewport::new(32., 24., 12.));
            renderer.draw_target(
                1,
                received.clone(),
                output,
                96,
                48,
                queue,
                command,
                255,
                0,
                false,
                false,
                [0; 3],
            )?;
            let pixels = read_pixels(&device)?;
            assert_eq!(
                &pixels[(4 * 96 + 4) * 4..(4 * 96 + 4) * 4 + 4],
                &[31, 37, 45, 255],
                "logical popup scroll band"
            );
            let body = &pixels[(24 * 96 + 48) * 4..(24 * 96 + 48) * 4 + 4];
            for (a, b) in body.iter().zip([64u8, 128, 191, 255]) {
                assert!((*a as i16 - b as i16).abs() <= 1, "popup body {body:?}");
            }
            drop(renderer);
            drop(received);
            producer.join().unwrap();
            std::fs::remove_file(path)?;
            device.destroy_buffer(buffer, None);
            device.free_memory(host, None);
            device.destroy_image(output, None);
            device.free_memory(memory, None);
            device.destroy_command_pool(pool, None);
            device.destroy_device(None);
            instance.destroy_instance(None);
            Ok(())
        }
    }
}
