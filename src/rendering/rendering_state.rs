use std::collections::HashSet;

use ash::ext::descriptor_buffer;
use ash_window::enumerate_required_extensions;
use winit::window::Window;
use anyhow::Result;
use ash::{vk, Entry, Instance};
use log::{error, info, warn};
use raw_window_handle::{ HasDisplayHandle, HasWindowHandle };

use super::vulkan_context::VulkanContext;
use super::cleanup::DeletionQueue;
use crate::component_system::simple_component_manager::ComponentManager;
use crate::config::{APPLICATION_NAME, ENGINE_NAME, ENGINE_VERSION, MAX_FRAMES_IN_FLIGHT, VALIDATION_ENABLED};
use crate::rendering::{buffer, descriptor, memory, render_target};
use crate::rendering::render_target::render_target::RenderTarget;
use crate::rendering::vertex::UniformBufferObject;
use crate::utils;


use anyhow::anyhow;

// Use the "name" method on the extension's functional struct
const PORTABILITY_ENUMERATION_EXTENSION_NAME: &std::ffi::CStr = ash::khr::portability_enumeration::NAME;
const GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION_NAME: &std::ffi::CStr = ash::khr::get_physical_device_properties2::NAME;
const VALIDATION_LAYER: &std::ffi::CStr = c"VK_LAYER_KHRONOS_validation";



pub struct RenderingState
{
    entry: ash::Entry,
    pub instance: ash::Instance,
    pub window : Window,
    pub vulkan_context : VulkanContext,
    pub logical_device : ash::Device,
    pub deletion_queue : DeletionQueue,
    start_time : std::time::Instant,
    pub render_target : RenderTarget,
    pub(crate) image_available_semaphores: Vec<vk::Semaphore>,
    pub(crate) rendering_finished_semaphores: Vec<vk::Semaphore>,
    pub(crate) frame_in_flight_fences: Vec<vk::Fence>,
    pub(crate) images_in_flight_fences: Vec<vk::Fence>,
    pub(crate) current_frame: usize,
    pub(crate) command_buffers: Vec<vk::CommandBuffer>,
    pub(crate) descriptor_sets: Vec<vk::DescriptorSet>, //one per frame in flight
    pub(crate) uniform_buffers: Vec<(vk::Buffer, vk::DeviceMemory)>, //one per frame in flight
}

impl RenderingState
{
    pub unsafe fn new(window: Window) -> Result<Self>{

        let entry = unsafe {ash::Entry::load()?};
        let instance = create_instance(&window, &entry)?;
        let mut deletion_queue = DeletionQueue::new();

        let (vulkan_context, logical_device) = unsafe { VulkanContext::init(
            &window,
            &entry,
            &instance,
            &mut deletion_queue
        ) }?;

        let render_target = unsafe { RenderTarget::new(
            &logical_device,
            &instance,
            vulkan_context.physical_device, 
            vulkan_context.surface, 
            &vulkan_context.surface_loader, 
            vulkan_context.render_pass, 
            window.inner_size(), 
            &vulkan_context.phys_memory_properties
        ) }?;
        let (i_a_sem, ren_f_sem, fif_fenc, imf_fenc) = setup_sync_objects(
            &logical_device, 
            render_target.swapchain.images.len())?;

        let command_buffers = setup_command_buffers(vulkan_context.command_pool, &logical_device)?;
        let (descriptor_sets, descriptor_buffers) = unsafe { allocate_descriptor_frames(
                &logical_device, 
                &vulkan_context.phys_memory_properties, 
                vulkan_context.descriptor_set_layout, 
                vulkan_context.descriptor_pool
            ) }?;

        Ok(Self {
            entry:entry, 
            instance:instance, 
            window:window, 
            vulkan_context:vulkan_context, 
            logical_device,
            deletion_queue:deletion_queue,
            start_time: std::time::Instant::now(),
            render_target,
            image_available_semaphores: i_a_sem,
            rendering_finished_semaphores: ren_f_sem,
            frame_in_flight_fences: fif_fenc,
            images_in_flight_fences: imf_fenc,
            current_frame: 0,
            command_buffers:command_buffers,
            descriptor_sets:descriptor_sets,
            uniform_buffers:descriptor_buffers,
        })
    }


    pub unsafe fn handle_resize(&mut self) -> Result<()> {
        (unsafe { self.logical_device.device_wait_idle() })?;
        
        // This 'take' removes the old target, triggering its Drop (cleanup)
        // then replaces it with a brand new one.
        self.render_target = unsafe { RenderTarget::new(
            &self.logical_device,
            &self.instance,
            self.vulkan_context.physical_device,
            self.vulkan_context.surface,
            &self.vulkan_context.surface_loader,
            self.vulkan_context.render_pass,
            self.window.inner_size(),
            &self.vulkan_context.phys_memory_properties
        ) }?;

        // only update the images_in_flight_fences size. 
        // NO need to recreate semaphores anymore.
        let image_count = self.render_target.swapchain.images.len();
        self.images_in_flight_fences.fill(vk::Fence::null());
        self.images_in_flight_fences.resize(image_count, vk::Fence::null()); 
        Ok(())
    }


    pub unsafe fn render(&mut self, component_manager: &ComponentManager) -> Result<()> {
        /*
        1. CPU Synchronization: The CPU waits for the GPU to finish its previous work (in_flight_fence).
        2. Image Acquisition: We ask the Swapchain for an image index to draw on (acquire_next_image).
        3. Command Recording: We write a "script" (Command Buffer) for the GPU, telling it to clear the image to a color.
        4. Submission: We send that script to the GPU queue, linking it to semaphores so it knows when to start and finish.
        5. Presentation: We tell the OS to show the finished image on the screen.
        */

        let ctx = &mut self.vulkan_context;
        let device = &self.logical_device;
        let sc = &self.render_target.swapchain;

        // 1. Setup frame-specific indices and handles
        let sync_frame_index = self.current_frame % MAX_FRAMES_IN_FLIGHT;
        let curr_cmd_buf = self.command_buffers[sync_frame_index];
        let current_ubo_memory = self.uniform_buffers[sync_frame_index].1;

        
        // 2. Calculate Camera Matrices (Static for this frame)
        let extent = sc.extent;
        let aspect = extent.width as f32 / extent.height as f32;
        let view_matrix = cgmath::Matrix4::look_at_rh(
            cgmath::Point3::new(2.0, 2.0, 2.0),
            cgmath::Point3::new(0.0, 0.0, 0.0),
            cgmath::Vector3::unit_z(),
        );
        
        let mut proj_matrix = cgmath::perspective(cgmath::Deg(45.0), aspect, 0.1, 10.0);
        proj_matrix[1][1] *= -1.0; // Vulkan Y-flip

        // CPU wait for GPU to finish rendering the previous in_flight frame
        (unsafe { device.wait_for_fences(&[self.frame_in_flight_fences[sync_frame_index]], true, u64::MAX) })?;

        // Ask for the next image index to render to, and handle swapchain status
        let (image_index, _) = match unsafe {
            sc.loader.acquire_next_image(
                sc.swapchain, 
                u64::MAX, 
                self.image_available_semaphores[sync_frame_index], 
                ash::vk::Fence::null()
            )
        } {
            core::result::Result::Ok((image_index, is_suboptimal )) => {
                if is_suboptimal {
                    warn!("Swapchain is suboptimal, recreating...");
                    unsafe { self.handle_resize()}?;
                    return Ok(());
                }
                (image_index, is_suboptimal)
            },
            core::result::Result::Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                warn!("Swapchain is out of date, needs recreation.");
                (unsafe { self.handle_resize()})?;
                return Ok(());
            }
            core::result::Result::Err(e) => {
                error!("Failed to acquire swapchain image: {}", e);
                return Err(anyhow!("Failed to acquire swapchain image: {}", e));
            }
        };

        // Wait for current image to be freed by GPU before we use it again and write command buffers
        if self.images_in_flight_fences[image_index as usize] != vk::Fence::null() {
            (unsafe { device.wait_for_fences(&[self.images_in_flight_fences[image_index as usize]], true, u64::MAX) })?;
        }
        // Map current image to the current in-flight fence
        self.images_in_flight_fences[image_index as usize] = self.frame_in_flight_fences[sync_frame_index];
        
        // Record command buffer
        unsafe { device.reset_command_buffer(curr_cmd_buf,ash::vk::CommandBufferResetFlags::empty())? };
        let begin_info = ash::vk::CommandBufferBeginInfo
                                                        ::default()
                                                        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { device.begin_command_buffer(curr_cmd_buf, &begin_info)? };


        // Clear image command
        let clear_values_arr = [
            vk::ClearValue{color: vk::ClearColorValue { float32: [0.2, 0.3, 0.3, 1.0] }},
            vk::ClearValue{depth_stencil: vk::ClearDepthStencilValue { depth: 1.0, stencil: 0}},
        ];

        //render pass
        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(ctx.render_pass)
            .framebuffer(self.render_target.framebuffers[image_index as usize])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: sc.extent,
            })
            .clear_values(&clear_values_arr);
        unsafe{
            device.cmd_begin_render_pass(
                curr_cmd_buf, 
                &render_pass_info, 
                vk::SubpassContents::INLINE
            );
            device.cmd_bind_pipeline(
                curr_cmd_buf, 
                vk::PipelineBindPoint::GRAPHICS, 
                ctx.graphics_pipeline
            );
            // Viewport & Scissor
            let viewport = vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: sc.extent.width as f32,
                height: sc.extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            };
            device.cmd_set_viewport(curr_cmd_buf, 0, &[viewport]);
            let scissor = vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: sc.extent,
            };
            device.cmd_set_scissor(curr_cmd_buf, 0, &[scissor]);
            
            //Global UBO
            device.cmd_bind_descriptor_sets(
                curr_cmd_buf, 
                vk::PipelineBindPoint::GRAPHICS, 
                ctx.pipeline_layout, 
                0,
                &[self.descriptor_sets[sync_frame_index]], 
                &[]
            );

            //draw components loop
            for (i, mesh_opt) in component_manager.meshes.iter().enumerate(){
                if let Some(mesh) = mesh_opt{
                    if let Some(transform) = component_manager.transforms.get(i).and_then(|t| t.as_ref()){

                        let model_matrix = transform.to_matrix();
                        let ubo_data = UniformBufferObject{
                            model:model_matrix,
                            view:view_matrix,
                            proj:proj_matrix
                        };
                        //each object overwrites one ubo buffer. it should change
                        memory::map_and_copy(&self.logical_device, current_ubo_memory, &[ubo_data])?;
                        mesh.bind(device, curr_cmd_buf);
                        mesh.draw(device, curr_cmd_buf);
                    }

                }
            }
            device.cmd_end_render_pass(curr_cmd_buf);
            device.end_command_buffer(curr_cmd_buf)?;
        }

        //Submit drawing
        (unsafe { device.reset_fences(&[self.frame_in_flight_fences[sync_frame_index]]) })?;
        //send command buffer to the queue
        let wait_semaphores = [self.image_available_semaphores[sync_frame_index]];
        let signal_semaphores = [self.rendering_finished_semaphores[image_index as usize]];
        let wait_stages = [vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT];
        let cmd_buff_array = [curr_cmd_buf];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&cmd_buff_array)
            .signal_semaphores(&signal_semaphores);
        unsafe { device.queue_submit(
            ctx.graphics_queue, 
            &[submit_info], 
            self.frame_in_flight_fences[sync_frame_index]
        )}?;

        //present
        let swapchains = [sc.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);

        match unsafe { sc.loader.queue_present(ctx.present_queue, &present_info) } {
            Ok(_) => (),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) | Err(vk::Result::SUBOPTIMAL_KHR) => {
                (unsafe { self.handle_resize() })?;
            }
            Err(e) => return Err(anyhow!("Failed to present: {}", e)),

        }       
        self.current_frame += 1;
        Ok(())
    }

}

impl Drop for RenderingState{
    fn drop(&mut self) {
        unsafe { self.logical_device.device_wait_idle().ok(); 
        
        // Clean up UBOs
        for (buf, mem) in self.uniform_buffers.drain(..) {
                self.logical_device.destroy_buffer(buf, None) ;
                self.logical_device.free_memory(mem, None);
        }

        for i in 0..MAX_FRAMES_IN_FLIGHT {
                self.logical_device.destroy_semaphore(self.image_available_semaphores[i], None);
                self.logical_device.destroy_fence(self.frame_in_flight_fences[i], None);
        }
        for &semaphore in &self.rendering_finished_semaphores {
                self.logical_device.destroy_semaphore(semaphore, None);
        }
        info!("Cleanup:: RenderingState resources released!")
    };
    }
}

fn create_instance(window: &Window, entry:&Entry) -> Result<Instance>{

    let mut extensions = enumerate_required_extensions(
        window.display_handle()?.as_raw())?.to_vec();

    //macos compability
    let mut flags = ash::vk::InstanceCreateFlags::empty();
    if cfg!(target_os = "macos"){
        extensions.push(PORTABILITY_ENUMERATION_EXTENSION_NAME.as_ptr());
        extensions.push(GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION_NAME.as_ptr());
        flags |= ash::vk::InstanceCreateFlags::ENUMERATE_PORTABILITY_KHR;
    }
    
    let available_layers =  unsafe {
        entry.enumerate_instance_layer_properties()?
            .iter()
            .map(|l| l.layer_name )
            .collect::<HashSet<_>>()
    };

    if VALIDATION_ENABLED {
        if !available_layers.iter().any(|l| utils::vk_to_cstr(l) == VALIDATION_LAYER) {
            return Err(anyhow!("Validation layer requested but not supported."));
        }
        extensions.push(ash::ext::debug_utils::NAME.as_ptr());
        //TODO: https://kylemayes.github.io/vulkanalia/setup/validation_layers.html#debugging-instance-creation-and-destruction
    }

    let layers = if VALIDATION_ENABLED{
        vec![VALIDATION_LAYER.as_ptr()]
    } else {
        Vec::new()
    };

    let app_info = ash::vk::ApplicationInfo::default()
        .api_version(ash::vk::API_VERSION_1_3)
        .application_name(APPLICATION_NAME)
        .engine_name(ENGINE_NAME)
        .engine_version(ENGINE_VERSION);

    let create_info= ash::vk::InstanceCreateInfo::default()
        .application_info(&app_info)
        .enabled_extension_names(&extensions)
        .flags(flags)
        .enabled_layer_names(&layers);

    return Ok(unsafe { entry.create_instance(&create_info, None)}?);
}


fn setup_sync_objects(logical_device: &ash::Device, swapchain_image_count: usize) -> Result<(Vec<vk::Semaphore>, Vec<vk::Semaphore>, Vec<vk::Fence>, Vec<vk::Fence>)> {
    let mut image_available_semaphores = Vec::new();
    let mut rendering_finished_semaphores = Vec::new();
    let mut frame_in_flight_fences = Vec::new();
    let images_in_flight_fences = vec![vk::Fence::null(); swapchain_image_count];

    let semaphore_info = vk::SemaphoreCreateInfo::default();
    let signaled_fence_info = vk::FenceCreateInfo
        ::default()
        .flags(vk::FenceCreateFlags::SIGNALED); // by default starts as unsignaled. We use flag to init it in Signaled state

    for _ in 0..swapchain_image_count {
        rendering_finished_semaphores.push(unsafe { logical_device.create_semaphore(&semaphore_info, None) }?);
    }

    for _ in 0..MAX_FRAMES_IN_FLIGHT {
        frame_in_flight_fences.push((unsafe { logical_device.create_fence(&signaled_fence_info, None) })?);
        image_available_semaphores.push((unsafe { logical_device.create_semaphore(&semaphore_info, None) })?);
    }
    
        
    Ok((image_available_semaphores, rendering_finished_semaphores, frame_in_flight_fences, images_in_flight_fences))
}


fn setup_command_buffers(command_pool : vk::CommandPool, logical_device: &ash::Device ) -> Result<Vec<vk::CommandBuffer>> {

    let buffer_info = vk::CommandBufferAllocateInfo
        ::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(MAX_FRAMES_IN_FLIGHT.try_into().unwrap());
        
    let command_buffers = (unsafe { logical_device.allocate_command_buffers(&buffer_info) })?;

    Ok(command_buffers)
}

unsafe fn allocate_descriptor_frames(
    logical_device: &ash::Device,
    phys_mem_props: &vk::PhysicalDeviceMemoryProperties,
    layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
) -> Result<(Vec<vk::DescriptorSet>, Vec<(vk::Buffer, vk::DeviceMemory)>)> {
    
    // 1. Allocate the sets from the pool using the layout
    let descriptor_sets = descriptor::allocate_descriptor_sets(
        logical_device, pool, layout, MAX_FRAMES_IN_FLIGHT as u32
    )?;

    let mut buffers = Vec::new();
    let ubo_size = std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize;

    for i in 0..MAX_FRAMES_IN_FLIGHT {
        // 2. Create the actual GPU memory
        let (buf, mem) = buffer::create_buffer(
            logical_device, 
            phys_mem_props,
            ubo_size, 
            vk::BufferUsageFlags::UNIFORM_BUFFER, 
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
        )?;
        
        buffers.push((buf, mem));

        // 3. Link this specific buffer to this specific set
        descriptor::update_buffer_descriptor_set(
            logical_device, 
            descriptor_sets[i], 
            buf, 
            vk::DescriptorType::UNIFORM_BUFFER,
            0
        );
    }

    Ok((descriptor_sets, buffers))
}