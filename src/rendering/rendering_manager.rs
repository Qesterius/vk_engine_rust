use std::sync::Arc;

use anyhow::{Result, anyhow};
use ash::vk;
use bevy_ecs::resource::Resource;
use bevy_ecs::world::World;
use log::{info, warn};

use std::sync::atomic::Ordering;

use crate::assets::texture::TextureHandle;
use crate::component_system::transform::Transform;
use crate::config::MAX_FRAMES_IN_FLIGHT;
use crate::device::device::Device;
use crate::rendering::components::mesh::Mesh;
use crate::rendering::rendering_instance::RenderingInstance;
use crate::rendering::vertex::Vertex;
use crate::rendering::{
    buffer, descriptor, memory,
    pipeline::pipeline_builder::PipelineBuilder,
    rendering_canvas::canvas::Canvas,
    shaders::load_shader_module,
    vertex::{MeshPushConstants, UniformBufferObject},
};
use crate::time::RenderedFrameCount;
use crate::utils;

struct FrameData {
    command_buffer: vk::CommandBuffer,
    global_descriptor_set: vk::DescriptorSet,
    uniform_buffer: vk::Buffer,
    uniform_buffer_memory: vk::DeviceMemory,
}

#[derive(Resource)]
pub struct RenderingManager {
    device: Arc<Device>,
    pub canvas: Canvas,

    // Pipeline resources
    graphics_pipeline: vk::Pipeline,
    pipeline_layout: vk::PipelineLayout,
    global_descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,

    // Per-frame data
    frames: Vec<FrameData>,

    // Synchronization
    // image_available: one per frame-in-flight (CPU submits, waits for GPU to release image)
    image_available_semaphores: Vec<vk::Semaphore>,
    // render_finished: one per swapchain image (GPU signals when rendering to that image is done)
    render_finished_semaphores: Vec<vk::Semaphore>,
    // in_flight_fences: one per frame-in-flight (CPU waits before reusing frame resources)
    in_flight_fences: Vec<vk::Fence>,
    // Tracks which frame fence is currently rendering to each swapchain image
    images_in_flight_fences: Vec<vk::Fence>,
}

impl RenderingManager {
    pub fn new(
        device: Arc<Device>,
        rendering_instance: Arc<RenderingInstance>,
        surface: vk::SurfaceKHR,
        surface_loader: ash::khr::surface::Instance,
        window_size: winit::dpi::PhysicalSize<u32>,
        texture_descriptor_set_layout: vk::DescriptorSetLayout,
    ) -> Result<Self> {
        let canvas = unsafe {
            Canvas::new(
                Arc::clone(&device),
                rendering_instance,
                surface,
                surface_loader,
                window_size,
            )
        }?;

        let (ubo_descriptor_set_layout, ubo_descriptor_pool) =
            create_descriptor_resources(&device.logical_device)?;

        let (graphics_pipeline, pipeline_layout) = create_pipeline(
            &device.logical_device,
            canvas.render_pass,
            ubo_descriptor_set_layout,
            texture_descriptor_set_layout
        )?;

        let frames = allocate_frames(
            &device.logical_device,
            &device.memory_properties,
            device.command_pool,
            ubo_descriptor_set_layout,
            ubo_descriptor_pool,
        )?;

        let swapchain_image_count = canvas.swapchain.images.len();
        let (
            image_available_semaphores,
            render_finished_semaphores,
            in_flight_fences,
            images_in_flight_fences,
        ) = create_sync_objects(&device.logical_device, swapchain_image_count)?;

        Ok(Self {
            device,
            canvas,
            graphics_pipeline,
            pipeline_layout,
            global_descriptor_set_layout: ubo_descriptor_set_layout,
            descriptor_pool: ubo_descriptor_pool,
            frames,
            image_available_semaphores,
            render_finished_semaphores,
            in_flight_fences,
            images_in_flight_fences,
        })
    }

    pub fn render(&mut self, world: &mut World) -> Result<()> {
        let current_frame = world.resource::<RenderedFrameCount>().0 as usize;
        let sync_idx = current_frame % MAX_FRAMES_IN_FLIGHT;
        let device = &self.device.logical_device;

        // Wait for this frame slot to be free
        unsafe { device.wait_for_fences(&[self.in_flight_fences[sync_idx]], true, u64::MAX) }?;
        // Fence is signaled: GPU finished all work submitted in the previous use of this slot.

        // Update the shared sync index so dynamic entities drops are  pushed
        // to the correct dynamic queue,
        self.device
            .current_sync_idx
            .store(sync_idx, Ordering::Relaxed);
        // ... flush anything queued for destruction from that previous use.
        self.device.flush_dynamic_deletion_queue(sync_idx);

        // Acquire swapchain image
        let image_index = match unsafe {
            self.canvas.swapchain.loader.acquire_next_image(
                self.canvas.swapchain.swapchain,
                u64::MAX,
                self.image_available_semaphores[sync_idx],
                vk::Fence::null(),
            )
        } {
            Ok((idx, false)) => idx,
            Ok((idx, true)) => {
                warn!("Swapchain suboptimal, recreating...");
                // Still use this frame but recreate next frame
                idx
            }
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                warn!("Swapchain out of date, recreating...");
                // We can't recreate here without the window size — handled in the event loop
                return Ok(());
            }
            Err(e) => return Err(anyhow!("Failed to acquire swapchain image: {}", e)),
        };

        // Wait if this image is still being rendered by another frame
        let img_idx = image_index as usize;
        if self.images_in_flight_fences[img_idx] != vk::Fence::null() {
            unsafe {
                device.wait_for_fences(&[self.images_in_flight_fences[img_idx]], true, u64::MAX)
            }?;
        }
        self.images_in_flight_fences[img_idx] = self.in_flight_fences[sync_idx];

        // Build camera matrices
        let extent = self.canvas.swapchain.extent;
        let aspect = extent.width as f32 / extent.height as f32;
        let view = cgmath::Matrix4::look_at_rh(
            cgmath::Point3::new(0.0, 2.0, 10.0),
            cgmath::Point3::new(0.0, 0.0, 0.0),
            cgmath::Vector3::new(0.0, 1.0, 0.0),
        );
        let mut proj = cgmath::perspective(cgmath::Deg(90.0f32), aspect, 0.1, 1000.0);
        proj[1][1] *= -1.0; // Vulkan Y-flip

        // Update UBO for this frame
        let frame = &self.frames[sync_idx];
        unsafe {
            memory::map_and_copy(
                device,
                frame.uniform_buffer_memory,
                &[UniformBufferObject { view, proj }],
            )
        }?;

        // Record commands
        let cmd = frame.command_buffer;
        unsafe {
            device.reset_command_buffer(cmd, vk::CommandBufferResetFlags::empty())?;
            device.begin_command_buffer(
                cmd,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )?;
        }

        let clear_values = [
            vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.2, 0.3, 0.3, 1.0],
                },
            },
            vk::ClearValue {
                depth_stencil: vk::ClearDepthStencilValue {
                    depth: 1.0,
                    stencil: 0,
                },
            },
        ];
        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(self.canvas.render_pass)
            .framebuffer(self.canvas.framebuffers[img_idx])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent,
            })
            .clear_values(&clear_values);

        unsafe {
            device.cmd_begin_render_pass(cmd, &render_pass_info, vk::SubpassContents::INLINE);
            device.cmd_bind_pipeline(cmd, vk::PipelineBindPoint::GRAPHICS, self.graphics_pipeline);

            device.cmd_set_viewport(
                cmd,
                0,
                &[vk::Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: extent.width as f32,
                    height: extent.height as f32,
                    min_depth: 0.0,
                    max_depth: 1.0,
                }],
            );
            device.cmd_set_scissor(
                cmd,
                0,
                &[vk::Rect2D {
                    offset: vk::Offset2D { x: 0, y: 0 },
                    extent,
                }],
            );

            device.cmd_bind_descriptor_sets(
                cmd,
                vk::PipelineBindPoint::GRAPHICS,
                self.pipeline_layout,
                0,
                &[frame.global_descriptor_set],
                &[],
            );

            let mut query = world.query::<(&Mesh, &Transform, &TextureHandle)>();

            for (mesh, transform, texture) in query.iter(world) {
                let push = MeshPushConstants::new(transform.to_matrix());
                device.cmd_push_constants(
                    cmd,
                    self.pipeline_layout,
                    vk::ShaderStageFlags::VERTEX,
                    0,
                    utils::any_as_u8_slice(&push),
                );
                mesh.bind(device, cmd);
                device.cmd_bind_descriptor_sets(
                    cmd,
                    vk::PipelineBindPoint::GRAPHICS,
                    self.pipeline_layout,
                    1, 
                    &[texture.descriptor_set],
                    &[],
                );
                mesh.draw(device, cmd);
            }

            device.cmd_end_render_pass(cmd);
            device.end_command_buffer(cmd)?;
        }

        // Submit
        unsafe { device.reset_fences(&[self.in_flight_fences[sync_idx]]) }?;
        let wait_semaphores = [self.image_available_semaphores[sync_idx]];
        let signal_semaphores = [self.render_finished_semaphores[img_idx]];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let cmd_bufs = [cmd];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&cmd_bufs)
            .signal_semaphores(&signal_semaphores);
        unsafe {
            device.queue_submit(
                self.device.graphics_queue,
                &[submit_info],
                self.in_flight_fences[sync_idx],
            )
        }?;

        // Present
        let swapchains = [self.canvas.swapchain.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        match unsafe {
            self.canvas
                .swapchain
                .loader
                .queue_present(self.device.present_queue, &present_info)
        } {
            Ok(_) => {}
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                // handled externally via handle_resize
            }
            Err(e) => return Err(anyhow!("Failed to present: {}", e)),
        }

        world.resource_mut::<RenderedFrameCount>().0 += 1;
        Ok(())
    }

    pub fn handle_resize(&mut self, window_size: winit::dpi::PhysicalSize<u32>) -> Result<()> {
        unsafe { self.canvas.handle_resize(window_size) }?;

        // Resize images_in_flight_fences to match new swapchain image count
        let new_image_count = self.canvas.swapchain.images.len();
        self.images_in_flight_fences.clear();
        self.images_in_flight_fences
            .resize(new_image_count, vk::Fence::null());

        // Recreate render_finished semaphores to match new image count
        let device = &self.device.logical_device;
        for &sem in &self.render_finished_semaphores {
            unsafe { device.destroy_semaphore(sem, None) };
        }
        self.render_finished_semaphores.clear();
        let sem_info = vk::SemaphoreCreateInfo::default();
        for _ in 0..new_image_count {
            self.render_finished_semaphores
                .push(unsafe { device.create_semaphore(&sem_info, None) }?);
        }

        Ok(())
    }
}

impl Drop for RenderingManager {
    fn drop(&mut self) {
        let device = &self.device.logical_device;
        unsafe {
            let _ = device.device_wait_idle();

            // Sync objects
            for &sem in &self.image_available_semaphores {
                device.destroy_semaphore(sem, None);
            }
            for &sem in &self.render_finished_semaphores {
                device.destroy_semaphore(sem, None);
            }
            for &fence in &self.in_flight_fences {
                device.destroy_fence(fence, None);
            }

            // Per-frame GPU resources
            for frame in &self.frames {
                device.destroy_buffer(frame.uniform_buffer, None);
                device.free_memory(frame.uniform_buffer_memory, None);
            }

            // Pipeline
            device.destroy_pipeline(self.graphics_pipeline, None);
            device.destroy_pipeline_layout(self.pipeline_layout, None);

            // Descriptors
            device.destroy_descriptor_pool(self.descriptor_pool, None);
            device.destroy_descriptor_set_layout(self.global_descriptor_set_layout, None);

            info!("RenderingManager destroyed.");
        }
        // canvas, device drop via field order (canvas first, then Arc<Device>)
    }
}

// --- Private helpers ---

fn create_descriptor_resources(
    device: &ash::Device,
) -> Result<(vk::DescriptorSetLayout, vk::DescriptorPool)> {
    let layout = descriptor::DescriptorLayoutBuilder::new()
        .add_binding(
            0, 
            vk::DescriptorType::UNIFORM_BUFFER, 
            vk::ShaderStageFlags::VERTEX
        )
        .build(device)?;
    let pool_sizes = [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::UNIFORM_BUFFER)
            .descriptor_count(MAX_FRAMES_IN_FLIGHT as u32),
    ];
    let pool = descriptor::create_descriptor_pool(
        device, 
        MAX_FRAMES_IN_FLIGHT as u32, 
        &pool_sizes
    )?;
    Ok((layout, pool))
}

fn create_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    ubo_descriptor_set_layout: vk::DescriptorSetLayout,
    texture_descriptor_set_layout: vk::DescriptorSetLayout,
) -> Result<(vk::Pipeline, vk::PipelineLayout)> {
    let push_constant_range = vk::PushConstantRange::default()
        .size(std::mem::size_of::<MeshPushConstants>() as u32)
        .stage_flags(vk::ShaderStageFlags::VERTEX)
        .offset(0);

    let descriptor_set_layouts = [ubo_descriptor_set_layout, texture_descriptor_set_layout];
    let layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&descriptor_set_layouts)
        .push_constant_ranges(std::slice::from_ref(&push_constant_range));
    let pipeline_layout = unsafe { device.create_pipeline_layout(&layout_info, None) }?;

    let vert = unsafe { load_shader_module(device, "src/rendering/shaders/vert.spv") }?;
    let frag = unsafe { load_shader_module(device, "src/rendering/shaders/frag.spv") }?;

    let bindings = Vertex::get_binding_description();
    let attributes = Vertex::get_attribute_descriptions();

    let pipeline = PipelineBuilder::new(pipeline_layout)
        .with_shader(vert, vk::ShaderStageFlags::VERTEX)
        .with_shader(frag, vk::ShaderStageFlags::FRAGMENT)
        .with_binding_descriptions(vec![bindings])
        .with_attribute_descriptions(attributes.to_vec())
        .build(device, render_pass)?;

    unsafe { device.destroy_shader_module(vert, None) };
    unsafe { device.destroy_shader_module(frag, None) };

    Ok((pipeline, pipeline_layout))
}

fn allocate_frames(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    command_pool: vk::CommandPool,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
) -> Result<Vec<FrameData>> {
    let cmd_bufs = {
        let alloc_info = vk::CommandBufferAllocateInfo::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(MAX_FRAMES_IN_FLIGHT as u32);
        unsafe { device.allocate_command_buffers(&alloc_info) }?
    };

    let descriptor_sets = descriptor::allocate_descriptor_sets(
        device,
        descriptor_pool,
        descriptor_set_layout,
        MAX_FRAMES_IN_FLIGHT as u32,
    )?;

    let ubo_size = std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize;
    let mut frames = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        let (uniform_buffer, uniform_buffer_memory) = buffer::create_buffer(
            device,
            mem_props,
            ubo_size,
            vk::BufferUsageFlags::UNIFORM_BUFFER,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
        )?;

        descriptor::update_buffer_descriptor_set(
            device,
            descriptor_sets[i],
            uniform_buffer,
            vk::DescriptorType::UNIFORM_BUFFER,
            0,
        );

        frames.push(FrameData {
            command_buffer: cmd_bufs[i],
            global_descriptor_set: descriptor_sets[i],
            uniform_buffer,
            uniform_buffer_memory,
        });
    }

    Ok(frames)
}

fn create_sync_objects(
    device: &ash::Device,
    swapchain_image_count: usize,
) -> Result<(
    Vec<vk::Semaphore>,
    Vec<vk::Semaphore>,
    Vec<vk::Fence>,
    Vec<vk::Fence>,
)> {
    let sem_info = vk::SemaphoreCreateInfo::default();
    let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);

    let mut image_available = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    let mut in_flight = Vec::with_capacity(MAX_FRAMES_IN_FLIGHT);
    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        image_available.push(unsafe { device.create_semaphore(&sem_info, None) }?);
        in_flight.push(unsafe { device.create_fence(&fence_info, None) }?);
    }

    let mut render_finished = Vec::with_capacity(swapchain_image_count);
    for _ in 0..swapchain_image_count {
        render_finished.push(unsafe { device.create_semaphore(&sem_info, None) }?);
    }

    let images_in_flight = vec![vk::Fence::null(); swapchain_image_count];

    Ok((
        image_available,
        render_finished,
        in_flight,
        images_in_flight,
    ))
}
