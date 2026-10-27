//! Stereo Vulkan rendering of the locally imported Precipice scene.
use ash::{vk, Device, Instance};
use dome::{
    xr_backdrop::{Asset, Presentation},
    Config, Pose, Scene,
};
use std::{collections::HashMap, ffi::CString, path::Path};
#[path = "backdrop_sound.rs"]
mod sound;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Buffer {
    device: Device,
    handle: vk::Buffer,
    memory: vk::DeviceMemory,
}
impl Drop for Buffer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_buffer(self.handle, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
struct Texture {
    device: Device,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
}
impl Drop for Texture {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}
struct Target {
    device: Device,
    framebuffer: vk::Framebuffer,
    view: vk::ImageView,
    _depth: Texture,
}
impl Drop for Target {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_framebuffer(self.framebuffer, None);
            self.device.destroy_image_view(self.view, None);
        }
    }
}
struct Draw {
    descriptor: vk::DescriptorSet,
    first: u32,
    count: u32,
    mode: u32,
    cutoff: f32,
    motions: Vec<usize>,
    uv_scroll: [f32; 2],
}

pub struct Renderer {
    device: Device,
    memory: vk::PhysicalDeviceMemoryProperties,
    format: vk::Format,
    pool: vk::CommandPool,
    command: vk::CommandBuffer,
    fence: vk::Fence,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    pipelines: Vec<vk::Pipeline>,
    descriptor_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    vertices: Option<Buffer>,
    indices: Option<Buffer>,
    particles: Option<Buffer>,
    particle_descriptors: Vec<vk::DescriptorSet>,
    particle_periods: Vec<f32>,
    pub clock: dome::xr_backdrop::Clock,
    sound: Option<sound::Player>,
    textures: Vec<Texture>,
    draws: Vec<Draw>,
    targets: HashMap<(vk::Image, u32, u32), Target>,
    asset: Asset,
    pub frames: u64,
    pub suspended_frames: u64,
}
impl Renderer {
    pub unsafe fn new(
        instance: &Instance,
        device: &Device,
        physical: vk::PhysicalDevice,
        family: u32,
        queue: vk::Queue,
        format: vk::Format,
        directory: &Path,
    ) -> Result<Self> {
        let (asset, vertices) = Asset::load(directory)?;
        let mut r = Self {
            device: device.clone(),
            memory: instance.get_physical_device_memory_properties(physical),
            format,
            pool: vk::CommandPool::null(),
            command: vk::CommandBuffer::null(),
            fence: vk::Fence::null(),
            render_pass: vk::RenderPass::null(),
            layout: vk::PipelineLayout::null(),
            pipelines: vec![],
            descriptor_layout: vk::DescriptorSetLayout::null(),
            descriptor_pool: vk::DescriptorPool::null(),
            sampler: vk::Sampler::null(),
            vertices: None,
            indices: None,
            particles: None,
            particle_descriptors: vec![],
            particle_periods: vec![],
            clock: Default::default(),
            sound: None,
            textures: vec![],
            draws: vec![],
            targets: HashMap::new(),
            asset,
            frames: 0,
            suspended_frames: 0,
        };
        r.pool = device.create_command_pool(
            &vk::CommandPoolCreateInfo::default()
                .queue_family_index(family)
                .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER),
            None,
        )?;
        r.command = device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(r.pool)
                .command_buffer_count(1),
        )?[0];
        r.fence = device.create_fence(
            &vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED),
            None,
        )?;
        r.vertices =
            Some(r.upload_device(&vertices, vk::BufferUsageFlags::VERTEX_BUFFER, queue)?);
        if let Some(data) = r.asset.load_indices(directory, vertices.len())? {
            r.indices = Some(r.upload_device(&data, vk::BufferUsageFlags::INDEX_BUFFER, queue)?);
        }
        let particles = r.asset.particle_data(directory)?;
        r.particle_periods = r
            .asset
            .particles
            .iter()
            .map(|p| {
                let offset = p.first as usize * 52 + 36;
                f32::from_le_bytes(particles[offset..offset + 4].try_into().unwrap()).max(0.01)
            })
            .collect();
        if !particles.is_empty() {
            r.particles =
                Some(r.upload_device(&particles, vk::BufferUsageFlags::VERTEX_BUFFER, queue)?);
        }
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
                .max_sets(512)
                .pool_sizes(&[vk::DescriptorPoolSize {
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    descriptor_count: 512,
                }]),
            None,
        )?;
        r.sampler = device.create_sampler(
            &vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::LINEAR)
                .min_filter(vk::Filter::LINEAR)
                .mipmap_mode(vk::SamplerMipmapMode::LINEAR)
                .address_mode_u(vk::SamplerAddressMode::REPEAT)
                .address_mode_v(vk::SamplerAddressMode::REPEAT)
                .address_mode_w(vk::SamplerAddressMode::REPEAT),
            None,
        )?;
        let mut descriptors = HashMap::new();
        for index in 0..r.asset.draws.len() + r.asset.particles.len() {
            let name = if index < r.asset.draws.len() {
                r.asset.draws[index].texture.clone()
            } else {
                r.asset.particles[index - r.asset.draws.len()]
                    .texture
                    .clone()
            };
            if !descriptors.contains_key(&name) {
                let mut reader = image::ImageReader::open(directory.join(&name))?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(8192);
                limits.max_image_height = Some(8192);
                limits.max_alloc = Some(256 * 1024 * 1024);
                reader.limits(limits);
                let image = reader.decode()?.into_rgba8();
                let texture = r.texture(
                    image.width(),
                    image.height(),
                    vk::Format::R8G8B8A8_SRGB,
                    vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST,
                    vk::ImageAspectFlags::COLOR,
                )?;
                let staging = r.buffer(image.as_raw(), vk::BufferUsageFlags::TRANSFER_SRC)?;
                r.begin()?;
                let barrier = vk::ImageMemoryBarrier::default()
                    .image(texture.image)
                    .subresource_range(range(vk::ImageAspectFlags::COLOR))
                    .old_layout(vk::ImageLayout::UNDEFINED)
                    .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                    .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                    .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE);
                device.cmd_pipeline_barrier(
                    r.command,
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier],
                );
                device.cmd_copy_buffer_to_image(
                    r.command,
                    staging.handle,
                    texture.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &[vk::BufferImageCopy::default()
                        .image_subresource(
                            vk::ImageSubresourceLayers::default()
                                .aspect_mask(vk::ImageAspectFlags::COLOR)
                                .layer_count(1),
                        )
                        .image_extent(vk::Extent3D {
                            width: image.width(),
                            height: image.height(),
                            depth: 1,
                        })],
                );
                device.cmd_pipeline_barrier(
                    r.command,
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::FRAGMENT_SHADER,
                    vk::DependencyFlags::empty(),
                    &[],
                    &[],
                    &[barrier
                        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
                        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
                        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                        .dst_access_mask(vk::AccessFlags::SHADER_READ)],
                );
                r.submit(queue)?;
                let descriptor = device.allocate_descriptor_sets(
                    &vk::DescriptorSetAllocateInfo::default()
                        .descriptor_pool(r.descriptor_pool)
                        .set_layouts(&[r.descriptor_layout]),
                )?[0];
                device.update_descriptor_sets(
                    &[vk::WriteDescriptorSet::default()
                        .dst_set(descriptor)
                        .dst_binding(0)
                        .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                        .image_info(&[vk::DescriptorImageInfo {
                            sampler: r.sampler,
                            image_view: texture.view,
                            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
                        }])],
                    &[],
                );
                descriptors.insert(name.clone(), descriptor);
                r.textures.push(texture);
            }
            if index < r.asset.draws.len() {
                let d = &r.asset.draws[index];
                r.draws.push(Draw {
                    descriptor: descriptors[&name],
                    first: d.first,
                    count: d.count,
                    mode: d.mode,
                    cutoff: d.cutoff,
                    motions: d.motions.clone(),
                    uv_scroll: d.uv_scroll,
                });
            } else {
                r.particle_descriptors.push(descriptors[&name]);
            }
        }
        let attachments = [
            vk::AttachmentDescription::default()
                .format(format)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::STORE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
            vk::AttachmentDescription::default()
                .format(vk::Format::D32_SFLOAT)
                .samples(vk::SampleCountFlags::TYPE_1)
                .load_op(vk::AttachmentLoadOp::CLEAR)
                .store_op(vk::AttachmentStoreOp::DONT_CARE)
                .initial_layout(vk::ImageLayout::UNDEFINED)
                .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
        ];
        let color = [vk::AttachmentReference {
            attachment: 0,
            layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }];
        let depth = vk::AttachmentReference {
            attachment: 1,
            layout: vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        };
        let subpasses = [vk::SubpassDescription::default()
            .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
            .color_attachments(&color)
            .depth_stencil_attachment(&depth)];
        let dependencies = [vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            )
            .dst_stage_mask(
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
                    | vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS,
            )
            .dst_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )];
        r.render_pass = device.create_render_pass(
            &vk::RenderPassCreateInfo::default()
                .attachments(&attachments)
                .subpasses(&subpasses)
                .dependencies(&dependencies),
            None,
        )?;
        r.layout = device.create_pipeline_layout(
            &vk::PipelineLayoutCreateInfo::default()
                .set_layouts(&[r.descriptor_layout])
                .push_constant_ranges(&[vk::PushConstantRange {
                    stage_flags: vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    offset: 0,
                    size: 128,
                }]),
            None,
        )?;
        let vertex = ash::util::read_spv(&mut std::io::Cursor::new(include_bytes!(
            "shaders/backdrop.vert.spv"
        )))?;
        let fragment = ash::util::read_spv(&mut std::io::Cursor::new(include_bytes!(
            "shaders/backdrop.frag.spv"
        )))?;
        let vs = device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&vertex), None)?;
        let fs = match device
            .create_shader_module(&vk::ShaderModuleCreateInfo::default().code(&fragment), None)
        {
            Ok(fs) => fs,
            Err(e) => {
                device.destroy_shader_module(vs, None);
                return Err(e.into());
            }
        };
        let particle_code = ash::util::read_spv(&mut std::io::Cursor::new(include_bytes!(
            "shaders/particles.vert.spv"
        )))?;
        let particle_vs = match device.create_shader_module(
            &vk::ShaderModuleCreateInfo::default().code(&particle_code),
            None,
        ) {
            Ok(v) => v,
            Err(e) => {
                device.destroy_shader_module(vs, None);
                device.destroy_shader_module(fs, None);
                return Err(e.into());
            }
        };
        let result = (|| -> Result<()> {
            let main = CString::new("main")?;
            for kind in 0..2 {
                let stages = [
                    vk::PipelineShaderStageCreateInfo::default()
                        .stage(vk::ShaderStageFlags::VERTEX)
                        .module(if kind == 0 { vs } else { particle_vs })
                        .name(&main),
                    vk::PipelineShaderStageCreateInfo::default()
                        .stage(vk::ShaderStageFlags::FRAGMENT)
                        .module(fs)
                        .name(&main),
                ];
                let bindings = [vk::VertexInputBindingDescription {
                    binding: 0,
                    stride: if kind == 0 { r.asset.vertex_stride } else { 52 },
                    input_rate: if kind == 0 {
                        vk::VertexInputRate::VERTEX
                    } else {
                        vk::VertexInputRate::INSTANCE
                    },
                }];
                let attributes: Vec<_> = if kind == 1 {
                    vec![
                        (0, vk::Format::R32G32B32_SFLOAT),
                        (12, vk::Format::R32G32B32_SFLOAT),
                        (24, vk::Format::R32G32B32_SFLOAT),
                        (36, vk::Format::R32G32B32A32_SFLOAT),
                    ]
                } else if r.asset.version == 2 {
                    vec![
                        (0, vk::Format::R32G32B32_SFLOAT),
                        (12, vk::Format::R16G16_SFLOAT),
                        (16, vk::Format::R16G16B16_SFLOAT),
                        (24, vk::Format::R16G16B16_SFLOAT),
                        (22, vk::Format::R16_SFLOAT),
                    ]
                } else {
                    vec![
                        (0, vk::Format::R32G32B32_SFLOAT),
                        (12, vk::Format::R32G32_SFLOAT),
                        (20, vk::Format::R32G32B32_SFLOAT),
                        (32, vk::Format::R32G32B32_SFLOAT),
                        (44, vk::Format::R32_SFLOAT),
                    ]
                }
                .into_iter()
                .enumerate()
                .map(
                    |(location, (offset, format))| vk::VertexInputAttributeDescription {
                        location: location as u32,
                        binding: 0,
                        format,
                        offset,
                    },
                )
                .collect();
                let input = vk::PipelineVertexInputStateCreateInfo::default()
                    .vertex_binding_descriptions(&bindings)
                    .vertex_attribute_descriptions(&attributes);
                let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
                    .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
                let viewport = vk::PipelineViewportStateCreateInfo::default()
                    .viewport_count(1)
                    .scissor_count(1);
                let raster = vk::PipelineRasterizationStateCreateInfo::default()
                    .polygon_mode(vk::PolygonMode::FILL)
                    .cull_mode(vk::CullModeFlags::NONE)
                    .line_width(1.);
                let samples = vk::PipelineMultisampleStateCreateInfo::default()
                    .rasterization_samples(vk::SampleCountFlags::TYPE_1);
                let dynamic = vk::PipelineDynamicStateCreateInfo::default()
                    .dynamic_states(&[vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR]);
                for mode in 0..3 {
                    let depth = vk::PipelineDepthStencilStateCreateInfo::default()
                        .depth_test_enable(true)
                        .depth_write_enable(mode == 0)
                        .depth_compare_op(vk::CompareOp::LESS_OR_EQUAL);
                    let blends = [vk::PipelineColorBlendAttachmentState::default()
                        .color_write_mask(vk::ColorComponentFlags::RGBA)
                        .blend_enable(mode != 0)
                        .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
                        .dst_color_blend_factor(if mode == 2 {
                            vk::BlendFactor::ONE
                        } else {
                            vk::BlendFactor::ONE_MINUS_SRC_ALPHA
                        })
                        .color_blend_op(vk::BlendOp::ADD)
                        .src_alpha_blend_factor(if mode == 2 {
                            vk::BlendFactor::ZERO
                        } else {
                            vk::BlendFactor::ONE
                        })
                        .dst_alpha_blend_factor(if mode == 2 {
                            vk::BlendFactor::ONE
                        } else {
                            vk::BlendFactor::ONE_MINUS_SRC_ALPHA
                        })
                        .alpha_blend_op(vk::BlendOp::ADD)];
                    let blend =
                        vk::PipelineColorBlendStateCreateInfo::default().attachments(&blends);
                    let infos = [vk::GraphicsPipelineCreateInfo::default()
                        .stages(&stages)
                        .vertex_input_state(&input)
                        .input_assembly_state(&assembly)
                        .viewport_state(&viewport)
                        .rasterization_state(&raster)
                        .multisample_state(&samples)
                        .depth_stencil_state(&depth)
                        .color_blend_state(&blend)
                        .dynamic_state(&dynamic)
                        .layout(r.layout)
                        .render_pass(r.render_pass)];
                    match device.create_graphics_pipelines(vk::PipelineCache::null(), &infos, None)
                    {
                        Ok(p) => r.pipelines.extend(p),
                        Err((partial, e)) => {
                            r.pipelines.extend(partial);
                            return Err(e.into());
                        }
                    }
                }
            }
            Ok(())
        })();
        device.destroy_shader_module(vs, None);
        device.destroy_shader_module(particle_vs, None);
        device.destroy_shader_module(fs, None);
        result?;
        eprintln!(
            "XR backdrop loaded: {} draw batches, {} MB mesh, {} textures",
            r.draws.len(),
            vertices.len() / 1_048_576,
            r.textures.len()
        );
        if !r.asset.sounds.is_empty() {
            r.sound = Some(sound::Player::new(
                directory,
                &r.asset.sounds,
                &r.asset.motions,
            ));
        }
        Ok(r)
    }
    pub fn activity(&mut self, now: f64, active: bool, head: Pose, config: &Config, sound: bool) {
        let seconds = self.clock.advance(now, active);
        if !active {
            self.suspended_frames += 1;
        }
        if let Some(player) = &self.sound {
            let right = dome::xr_backdrop::anchor(config)
                .inverse()
                .transform_vector3(glam::Vec3::from_array(head.rotate([1., 0., 0.])));
            player.update(
                active && sound,
                seconds,
                dome::xr_backdrop::local_eye(head, config).to_array(),
                right.to_array(),
                config.xr_backdrop_volume,
            );
        }
    }
    pub fn suspend(&mut self, now: f64) {
        self.clock.advance(now, false);
        if let Some(player) = &self.sound {
            player.update(false, self.clock.seconds, [0.; 3], [1., 0., 0.], 0.);
        }
    }
    unsafe fn upload_device(
        &self,
        data: &[u8],
        usage: vk::BufferUsageFlags,
        queue: vk::Queue,
    ) -> Result<Buffer> {
        let target = self.device_buffer(data.len() as u64, usage)?;
        let staging = self.buffer(data, vk::BufferUsageFlags::TRANSFER_SRC)?;
        self.begin()?;
        self.device.cmd_copy_buffer(
            self.command,
            staging.handle,
            target.handle,
            &[vk::BufferCopy {
                src_offset: 0,
                dst_offset: 0,
                size: data.len() as u64,
            }],
        );
        self.device.cmd_pipeline_barrier(
            self.command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::VERTEX_INPUT,
            vk::DependencyFlags::empty(),
            &[vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(
                    vk::AccessFlags::VERTEX_ATTRIBUTE_READ | vk::AccessFlags::INDEX_READ,
                )],
            &[],
            &[],
        );
        self.submit(queue)?;
        Ok(target)
    }
    unsafe fn memory_type(&self, bits: u32, flags: vk::MemoryPropertyFlags) -> Result<u32> {
        (0..self.memory.memory_type_count)
            .find(|i| {
                bits & (1 << i) != 0
                    && self.memory.memory_types[*i as usize]
                        .property_flags
                        .contains(flags)
            })
            .ok_or("No backdrop GPU memory type".into())
    }
    unsafe fn device_buffer(&self, size: u64, usage: vk::BufferUsageFlags) -> Result<Buffer> {
        let mut b = Buffer {
            device: self.device.clone(),
            handle: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
        };
        b.handle = self.device.create_buffer(
            &vk::BufferCreateInfo::default()
                .size(size)
                .usage(usage | vk::BufferUsageFlags::TRANSFER_DST),
            None,
        )?;
        let req = self.device.get_buffer_memory_requirements(b.handle);
        b.memory = self.device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(
                    self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?,
                ),
            None,
        )?;
        self.device.bind_buffer_memory(b.handle, b.memory, 0)?;
        Ok(b)
    }
    unsafe fn buffer(&self, data: &[u8], usage: vk::BufferUsageFlags) -> Result<Buffer> {
        let mut b = Buffer {
            device: self.device.clone(),
            handle: vk::Buffer::null(),
            memory: vk::DeviceMemory::null(),
        };
        b.handle = self.device.create_buffer(
            &vk::BufferCreateInfo::default()
                .size(data.len() as u64)
                .usage(usage),
            None,
        )?;
        let req = self.device.get_buffer_memory_requirements(b.handle);
        b.memory = self.device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(self.memory_type(
                    req.memory_type_bits,
                    vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
                )?),
            None,
        )?;
        self.device.bind_buffer_memory(b.handle, b.memory, 0)?;
        let map =
            self.device
                .map_memory(b.memory, 0, data.len() as u64, vk::MemoryMapFlags::empty())?;
        std::ptr::copy_nonoverlapping(data.as_ptr(), map.cast(), data.len());
        self.device.unmap_memory(b.memory);
        Ok(b)
    }
    unsafe fn texture(
        &self,
        width: u32,
        height: u32,
        format: vk::Format,
        usage: vk::ImageUsageFlags,
        aspect: vk::ImageAspectFlags,
    ) -> Result<Texture> {
        let mut t = Texture {
            device: self.device.clone(),
            image: vk::Image::null(),
            memory: vk::DeviceMemory::null(),
            view: vk::ImageView::null(),
        };
        t.image = self.device.create_image(
            &vk::ImageCreateInfo::default()
                .image_type(vk::ImageType::TYPE_2D)
                .format(format)
                .extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })
                .mip_levels(1)
                .array_layers(1)
                .samples(vk::SampleCountFlags::TYPE_1)
                .usage(usage),
            None,
        )?;
        let req = self.device.get_image_memory_requirements(t.image);
        t.memory = self.device.allocate_memory(
            &vk::MemoryAllocateInfo::default()
                .allocation_size(req.size)
                .memory_type_index(
                    self.memory_type(req.memory_type_bits, vk::MemoryPropertyFlags::DEVICE_LOCAL)?,
                ),
            None,
        )?;
        self.device.bind_image_memory(t.image, t.memory, 0)?;
        t.view = self.device.create_image_view(
            &vk::ImageViewCreateInfo::default()
                .image(t.image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(format)
                .subresource_range(range(aspect)),
            None,
        )?;
        Ok(t)
    }
    unsafe fn begin(&self) -> Result<()> {
        self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        self.device
            .reset_command_buffer(self.command, vk::CommandBufferResetFlags::empty())?;
        self.device.begin_command_buffer(
            self.command,
            &vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
        )?;
        Ok(())
    }
    unsafe fn submit(&self, queue: vk::Queue) -> Result<()> {
        self.device.end_command_buffer(self.command)?;
        self.device.reset_fences(&[self.fence])?;
        self.device.queue_submit(
            queue,
            &[vk::SubmitInfo::default().command_buffers(&[self.command])],
            self.fence,
        )?;
        self.device.wait_for_fences(&[self.fence], true, u64::MAX)?;
        Ok(())
    }
    /// The caller acquires/waits both XR swapchain images, then releases them
    /// only after this fence completes. No GPU work is submitted while suspended.
    pub unsafe fn render(
        &mut self,
        queue: vk::Queue,
        targets: &[(vk::Image, u32, u32, Pose, [f32; 4])],
        config: &Config,
    ) -> Result<()> {
        for &(image, width, height, _, _) in targets {
            if self.targets.contains_key(&(image, width, height)) {
                continue;
            }
            let depth = self.texture(
                width,
                height,
                vk::Format::D32_SFLOAT,
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT,
                vk::ImageAspectFlags::DEPTH,
            )?;
            let view = self.device.create_image_view(
                &vk::ImageViewCreateInfo::default()
                    .image(image)
                    .view_type(vk::ImageViewType::TYPE_2D)
                    .format(self.format)
                    .subresource_range(range(vk::ImageAspectFlags::COLOR)),
                None,
            )?;
            let mut target = Target {
                device: self.device.clone(),
                view,
                framebuffer: vk::Framebuffer::null(),
                _depth: depth,
            };
            target.framebuffer = self.device.create_framebuffer(
                &vk::FramebufferCreateInfo::default()
                    .render_pass(self.render_pass)
                    .attachments(&[view, target._depth.view])
                    .width(width)
                    .height(height)
                    .layers(1),
                None,
            )?;
            self.targets.insert((image, width, height), target);
        }
        self.begin()?;
        for &(image, width, height, eye, fov) in targets {
            let area = vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: vk::Extent2D { width, height },
            };
            let sky = self.asset.sky;
            let clears = [
                vk::ClearValue {
                    color: vk::ClearColorValue {
                        float32: [sky[0], sky[1], sky[2], 1.],
                    },
                },
                vk::ClearValue {
                    depth_stencil: vk::ClearDepthStencilValue {
                        depth: 1.,
                        stencil: 0,
                    },
                },
            ];
            self.device.cmd_begin_render_pass(
                self.command,
                &vk::RenderPassBeginInfo::default()
                    .render_pass(self.render_pass)
                    .framebuffer(self.targets[&(image, width, height)].framebuffer)
                    .render_area(area)
                    .clear_values(&clears),
                vk::SubpassContents::INLINE,
            );
            self.device.cmd_set_viewport(
                self.command,
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
            self.device.cmd_set_scissor(self.command, 0, &[area]);
            self.device.cmd_bind_vertex_buffers(
                self.command,
                0,
                &[self.vertices.as_ref().unwrap().handle],
                &[0],
            );
            if let Some(indices) = &self.indices {
                self.device.cmd_bind_index_buffer(
                    self.command,
                    indices.handle,
                    0,
                    vk::IndexType::UINT32,
                );
            }
            let mut params = [0f32; 32];
            params[..16].copy_from_slice(
                &dome::xr_backdrop::view_projection(eye, fov, config).to_cols_array(),
            );
            params[16..19].copy_from_slice(&dome::xr_backdrop::local_eye(eye, config).to_array());
            params[19] = self.asset.fog[3];
            params[20..23].copy_from_slice(&self.asset.fog[..3]);
            params[26] = f32::from(self.format == vk::Format::R8G8B8A8_SRGB);
            for d in &self.draws {
                let motion = self.asset.motion(&d.motions, self.clock.seconds);
                params[..16].copy_from_slice(
                    &(dome::xr_backdrop::view_projection(eye, fov, config) * motion)
                        .to_cols_array(),
                );
                params[16..19].copy_from_slice(
                    &motion
                        .inverse()
                        .transform_point3(dome::xr_backdrop::local_eye(eye, config))
                        .to_array(),
                );
                params[23] = (d.uv_scroll[0] as f64 * self.clock.seconds).rem_euclid(1.) as f32;
                params[27] = (d.uv_scroll[1] as f64 * self.clock.seconds).rem_euclid(1.) as f32;
                params[24] = d.cutoff;
                params[25] = d.mode as f32;
                self.device.cmd_bind_pipeline(
                    self.command,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipelines[d.mode as usize],
                );
                self.device.cmd_bind_descriptor_sets(
                    self.command,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.layout,
                    0,
                    &[d.descriptor],
                    &[],
                );
                self.device.cmd_push_constants(
                    self.command,
                    self.layout,
                    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                    0,
                    std::slice::from_raw_parts(params.as_ptr().cast(), 128),
                );
                if self.indices.is_some() {
                    self.device
                        .cmd_draw_indexed(self.command, d.count, 1, d.first, 0, 0);
                } else {
                    self.device.cmd_draw(self.command, d.count, 1, d.first, 0);
                }
            }
            if let Some(particles) = &self.particles {
                self.device
                    .cmd_bind_vertex_buffers(self.command, 0, &[particles.handle], &[0]);
                for (index, p) in self.asset.particles.iter().enumerate() {
                    let motion = self.asset.motion(&p.motions, self.clock.seconds);
                    params[..16].copy_from_slice(
                        &(dome::xr_backdrop::view_projection(eye, fov, config) * motion)
                            .to_cols_array(),
                    );
                    params[16..19].copy_from_slice(
                        &motion
                            .inverse()
                            .transform_point3(dome::xr_backdrop::local_eye(eye, config))
                            .to_array(),
                    );
                    params[23] = if p.pulse.is_some() {
                        -1.
                    } else {
                        f32::from(p.steam)
                    };
                    params[24] = 0.;
                    params[25] = p.mode as f32;
                    params[27] = self
                        .clock
                        .seconds
                        .rem_euclid(self.particle_periods[index] as f64)
                        as f32;
                    params[28..32].copy_from_slice(&p.color);
                    if let Some([length, speed, min]) = p.pulse {
                        let ping = (self.clock.seconds as f32 * speed).rem_euclid(length * 2.);
                        params[31] *= (length - (ping - length).abs() + min) / (length + min);
                    }
                    self.device.cmd_bind_pipeline(
                        self.command,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.pipelines[3 + p.mode as usize],
                    );
                    self.device.cmd_bind_descriptor_sets(
                        self.command,
                        vk::PipelineBindPoint::GRAPHICS,
                        self.layout,
                        0,
                        &[self.particle_descriptors[index]],
                        &[],
                    );
                    self.device.cmd_push_constants(
                        self.command,
                        self.layout,
                        vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT,
                        0,
                        std::slice::from_raw_parts(params.as_ptr().cast(), 128),
                    );
                    self.device.cmd_draw(self.command, 6, p.count, 0, p.first);
                }
            }
            self.device.cmd_end_render_pass(self.command);
        }
        self.submit(queue)?;
        self.frames += 1;
        Ok(())
    }
    pub fn present(scene: &Scene, state: Presentation) -> bool {
        dome::xr_backdrop::visible(scene, state)
    }
}
fn range(aspect: vk::ImageAspectFlags) -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange::default()
        .aspect_mask(aspect)
        .level_count(1)
        .layer_count(1)
}
impl Drop for Renderer {
    fn drop(&mut self) {
        unsafe {
            // All render/upload submissions have completed their fences.
            self.targets.clear();
            self.sound.take();
            self.vertices.take();
            self.indices.take();
            self.particles.take();
            self.textures.clear();
            for p in self.pipelines.drain(..) {
                self.device.destroy_pipeline(p, None);
            }
            self.device.destroy_pipeline_layout(self.layout, None);
            self.device.destroy_render_pass(self.render_pass, None);
            self.device
                .destroy_descriptor_pool(self.descriptor_pool, None);
            self.device
                .destroy_descriptor_set_layout(self.descriptor_layout, None);
            self.device.destroy_sampler(self.sampler, None);
            self.device.destroy_fence(self.fence, None);
            self.device.destroy_command_pool(self.pool, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    unsafe fn readback(
        renderer: &Renderer,
        queue: vk::Queue,
        image: &Texture,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let device = &renderer.device;
        let buffer = renderer.buffer(
            &vec![0; (width * height * 4) as usize],
            vk::BufferUsageFlags::TRANSFER_DST,
        )?;
        renderer.begin()?;
        let barrier = vk::ImageMemoryBarrier::default()
            .image(image.image)
            .subresource_range(range(vk::ImageAspectFlags::COLOR))
            .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .new_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
        device.cmd_pipeline_barrier(
            renderer.command,
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &[barrier],
        );
        device.cmd_copy_image_to_buffer(
            renderer.command,
            image.image,
            vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
            buffer.handle,
            &[vk::BufferImageCopy::default()
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .layer_count(1),
                )
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                })],
        );
        renderer.submit(queue)?;
        let memory = device.map_memory(
            buffer.memory,
            0,
            u64::from(width * height * 4),
            vk::MemoryMapFlags::empty(),
        )?;
        let bytes =
            std::slice::from_raw_parts(memory.cast::<u8>(), (width * height * 4) as usize).to_vec();
        device.unmap_memory(buffer.memory);
        Ok(bytes)
    }
    /// Real Vulkan render/readback with imported assets; never opens an XR
    /// session or touches the active desktop. Run with --ignored --nocapture.
    #[test]
    #[ignore = "requires locally imported original Precipice assets and Vulkan"]
    fn imported_scene_stereo_vulkan() -> Result<()> {
        unsafe {
            let directory = dome::xr_backdrop::directory()
                .ok_or("Set DOME_XR_BACKDROP_PATH to imported assets")?;
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
                .find(|p| {
                    instance.get_physical_device_properties(*p).device_type
                        == vk::PhysicalDeviceType::DISCRETE_GPU
                })
                .or_else(|| instance.enumerate_physical_devices().ok()?.first().copied())
                .ok_or("No Vulkan GPU")?;
            let family = instance
                .get_physical_device_queue_family_properties(physical)
                .iter()
                .position(|p| p.queue_flags.contains(vk::QueueFlags::GRAPHICS))
                .ok_or("No graphics queue")? as u32;
            let queues = [vk::DeviceQueueCreateInfo::default()
                .queue_family_index(family)
                .queue_priorities(&[1.])];
            let device = instance.create_device(
                physical,
                &vk::DeviceCreateInfo::default().queue_create_infos(&queues),
                None,
            )?;
            let queue = device.get_device_queue(family, 0);
            let mut renderer = Renderer::new(
                &instance,
                &device,
                physical,
                family,
                queue,
                vk::Format::R8G8B8A8_SRGB,
                &directory,
            )?;
            let (width, height) = (1280u32, 720u32);
            let images = [
                renderer.texture(
                    width,
                    height,
                    vk::Format::R8G8B8A8_SRGB,
                    vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    vk::ImageAspectFlags::COLOR,
                )?,
                renderer.texture(
                    width,
                    height,
                    vk::Format::R8G8B8A8_SRGB,
                    vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC,
                    vk::ImageAspectFlags::COLOR,
                )?,
            ];
            let mut config = Config::default();
            config.yaw_deg = std::env::var("DOME_BACKDROP_VIEW_YAW")
                .unwrap_or_else(|_| "0".into())
                .parse()?;
            let fov = [-0.95, 0.95, -0.58, 0.58];
            let targets: Vec<_> = images
                .iter()
                .enumerate()
                .map(|(i, image)| {
                    (
                        image.image,
                        width,
                        height,
                        Pose {
                            position_m: [if i == 0 { -0.032 } else { 0.032 }, 1.4, 0.],
                            orientation_xyzw: [0., 0., 0., 1.],
                        },
                        fov,
                    )
                })
                .collect();
            renderer.render(queue, &targets, &config)?;
            let mut stereo = image::RgbaImage::new(width * 2, height);
            let mut pixels = Vec::new();
            for (i, image) in images.iter().enumerate() {
                let bytes = readback(&renderer, queue, image, width, height)?;
                let eye = image::RgbaImage::from_raw(width, height, bytes.clone()).unwrap();
                image::imageops::replace(&mut stereo, &eye, (i as u32 * width) as i64, 0);
                pixels.push(bytes);
            }
            let distinct = pixels[0]
                .chunks_exact(4)
                .map(|p| [p[0], p[1], p[2]])
                .collect::<std::collections::HashSet<_>>()
                .len();
            assert!(distinct > 1000, "Scene is blank or flat: {distinct} colors");
            let stereo_difference = pixels[0]
                .iter()
                .zip(&pixels[1])
                .filter(|(a, b)| a != b)
                .count();
            assert!(stereo_difference > 500, "Both eyes unexpectedly identical");
            assert!(
                pixels
                    .iter()
                    .all(|eye| eye.chunks_exact(4).all(|p| p[3] == 255)),
                "Backdrop must remain opaque behind desktop"
            );
            std::fs::create_dir_all("artifacts/precipice")?;
            stereo.save("artifacts/precipice/stereo.png")?;
            println!("Original geometry stereo render: {distinct} colors, {stereo_difference} differing channels");
            if renderer.asset.version >= 2 {
                renderer.clock.advance(0., true);
                renderer.clock.advance(10., false);
                renderer.render(queue, &targets, &config)?;
                assert_eq!(
                    readback(&renderer, queue, &images[0], width, height)?,
                    pixels[0],
                    "Paused scene must be pixel-stable"
                );
                renderer.clock.advance(15., true);
                renderer.clock.advance(15.2, true);
                renderer.render(queue, &targets, &config)?;
                let resumed = readback(&renderer, queue, &images[0], width, height)?;
                let changed = resumed
                    .iter()
                    .zip(&pixels[0])
                    .filter(|(a, b)| a != b)
                    .count();
                assert!(changed > 1000, "Native animation did not advance");
                image::RgbaImage::from_raw(width, height, resumed)
                    .unwrap()
                    .save("artifacts/precipice/animated-resumed.png")?;
                println!("Paused image unchanged; resumed animation changes {changed} samples");
            }
            let bench = std::env::var("DOME_BACKDROP_BENCH_FRAMES")
                .ok()
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0)
                .min(360);
            if bench > 0 {
                let start = std::time::Instant::now();
                for i in 0..bench {
                    renderer.clock.seconds = i as f64 / 90.;
                    renderer.render(queue, &targets, &config)?;
                }
                println!("Backdrop GPU submission+completion: {:.3} ms/frame across {bench} stereo frames",start.elapsed().as_secs_f64()*1000./bench as f64);
            }
            drop(renderer);
            drop(images);
            device.destroy_device(None);
            instance.destroy_instance(None);
            Ok(())
        }
    }
}
