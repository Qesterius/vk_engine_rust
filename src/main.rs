mod utils;
mod cleanup;
mod vulkan_context;
mod config;

use crate::config::{VALIDATION_ENABLED, APPLICATION_NAME, APPLICATION_TITLE, ENGINE_NAME, ENGINE_VERSION};
use ash::{ Entry, Instance, vk };
use log::{error, info, warn};
use raw_window_handle::{HasDisplayHandle};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{WindowAttributes, WindowId};
use winit::window::Window;
use anyhow::{Ok, Result};
use ash_window::{enumerate_required_extensions};
use std::{collections::HashSet};
use anyhow::anyhow;

use crate::{cleanup::DeletionQueue};
use crate::vulkan_context::VulkanContext;



// Use the "name" method on the extension's functional struct
const PORTABILITY_ENUMERATION_EXTENSION_NAME: &std::ffi::CStr = ash::khr::portability_enumeration::NAME;
const GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION_NAME: &std::ffi::CStr = ash::khr::get_physical_device_properties2::NAME;
const VALIDATION_LAYER: &std::ffi::CStr = c"VK_LAYER_KHRONOS_validation";


fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("Logger initialized");
    let event_loop = EventLoop::new()?;
    let mut app = App{ state: None};
    event_loop.run_app(&mut app)?;
    Ok(())
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

struct App
{
    state: Option<RenderingState>
}

impl ApplicationHandler for App{
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none(){
            let window_attrs = WindowAttributes::default().with_title(APPLICATION_TITLE);
            let window = event_loop.create_window(window_attrs).expect("Failed to create window");

            match unsafe{ RenderingState::new(window)}{
                core::result::Result::Ok(state) => {
                    self.state = Some(state);
                }
                core::result::Result::Err(e) => {
                    error!("Failed to initialize Vulkan: {}", e);
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: winit::event::WindowEvent,
    ) {
        let _ = window_id;
        match event{
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting...");
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = &mut self.state{
                    unsafe{ state.render().expect("Failed to render frame");}
                    state.window.request_redraw();
                }
            }
            _ => (),
        }
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut state) = self.state.take(){
            
            unsafe{state.destroy();}
        }
    }
}


struct RenderingState
{
    entry: Entry,
    instance: Instance,
    window : Window,
    vulkan_context : VulkanContext,
    logical_device : ash::Device,
    deletion_queue : DeletionQueue
}

impl RenderingState
{
    pub unsafe fn new(window: Window) -> Result<Self>{

        let entry = unsafe {Entry::load()?};
        let instance = create_instance(&window, &entry)?;

        let mut deletion_queue = DeletionQueue::new();

        let (vulkan_context, logical_device) = unsafe { vulkan_context::VulkanContext::init(
            &window,
            &entry,
            &instance,
            &mut deletion_queue
        ) }?;

        Ok(Self {
            entry:entry, 
            instance:instance, 
            window:window, 
            vulkan_context:vulkan_context, 
            logical_device,
            deletion_queue:deletion_queue
        })
    }

    unsafe fn render(&mut self) -> Result<()> {
        /*
        1. CPU Synchronization: The CPU waits for the GPU to finish its previous work (in_flight_fence).
        2. Image Acquisition: We ask the Swapchain for an image index to draw on (acquire_next_image).
        3. Command Recording: We write a "script" (Command Buffer) for the GPU, telling it to clear the image to a color.
        4. Submission: We send that script to the GPU queue, linking it to semaphores so it knows when to start and finish.
        5. Presentation: We tell the OS to show the finished image on the screen.
        */

        let ctx = &mut self.vulkan_context;
        let device = &self.logical_device;
        let sync_frame_index = ctx.current_frame % vulkan_context::MAX_FRAMES_IN_FLIGHT;
        let curr_cmd_buf = ctx.command_buffers[sync_frame_index];

        //resize checks
        let current_size = self.window.inner_size();
        if current_size.width == 0 || current_size.height == 0 {
            return Ok(());
        }

        let needs_rebuild = match &ctx.swapchain {
                None => true,
                Some(sc) => sc.extent.width != current_size.width || sc.extent.height != current_size.height,
            };
        

       if needs_rebuild {
            (unsafe { device.device_wait_idle() })?;
            
            if ctx.swapchain.is_some() {
                ctx.swapchain = None;
                return Ok(());
            }

            // Pass the CURRENT size to the recreation function
            match unsafe { ctx.recreate_swapchain(&self.window, current_size, &self.instance, device) } {
                core::result::Result::Ok(_) => {
                    info!("Swapchain successfully rebuilt at {}x{}", current_size.width, current_size.height);
                    return Ok(());
                },
                core::result::Result::Err(e) => {
                    warn!("Swapchain recreation pending: {}", e);
                    return Ok(());
                }
            }
        }
        let sc = ctx.swapchain.as_mut().unwrap();

        // CPU wait for GPU to finish rendering the previous in_flight frame
        (unsafe { device.wait_for_fences(&[ctx.frame_in_flight_fences[sync_frame_index]], true, u64::MAX) })?;

        // Ask for the next image index to render to, and handle swapchain status
        let (image_index, _) = match unsafe {
            sc.loader.acquire_next_image(sc.swapchain, u64::MAX, ctx.image_available_semaphores[sync_frame_index], ash::vk::Fence::null())
        } {
            core::result::Result::Ok((image_index, is_suboptimal )) => {
                if is_suboptimal {
                    warn!("Swapchain is suboptimal, recreating...");
                    unsafe { ctx.recreate_swapchain(&self.window, self.window.inner_size(), &self.instance, device) }?;
                    return Ok(());
                }
                (image_index, is_suboptimal)
            },
            core::result::Result::Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => {
                warn!("Swapchain is out of date, needs recreation.");
                (unsafe { ctx.recreate_swapchain(&self.window, self.window.inner_size(), &self.instance, device) })?;
                return Ok(());
            }
            core::result::Result::Err(e) => {
                error!("Failed to acquire swapchain image: {}", e);
                return Err(anyhow!("Failed to acquire swapchain image: {}", e));
            }
        };

        // Wait for current image to be freed by GPU before we use it again and write command buffers
        if ctx.images_in_flight_fences[image_index as usize] != vk::Fence::null() {
            (unsafe { device.wait_for_fences(&[ctx.images_in_flight_fences[image_index as usize]], true, u64::MAX) })?;
        }
        // Map current image to the current in-flight fence
        ctx.images_in_flight_fences[image_index as usize] = ctx.frame_in_flight_fences[sync_frame_index];
        
        // Record command buffer
        unsafe { device.reset_command_buffer(curr_cmd_buf,ash::vk::CommandBufferResetFlags::empty())? };
        let begin_info = ash::vk::CommandBufferBeginInfo::default()
        .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { device.begin_command_buffer(curr_cmd_buf, &begin_info)? };

        let image = sc.images[image_index as usize];
        let range = vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        };

        // Clear image command
        let clear_color_arr = [vk::ClearValue{color: vk::ClearColorValue { float32: [0.0, 0.0, 0.0, 1.0] }}];

        let render_pass_info = vk::RenderPassBeginInfo::default()
            .render_pass(ctx.render_pass)
            .framebuffer(sc.framebuffers[image_index as usize])
            .render_area(vk::Rect2D {
                offset: vk::Offset2D { x: 0, y: 0 },
                extent: sc.extent,
            })
            .clear_values(&clear_color_arr);
        unsafe{
            device.cmd_begin_render_pass(curr_cmd_buf, &render_pass_info, vk::SubpassContents::INLINE);
            // rest of render commands would go here
            device.cmd_end_render_pass(curr_cmd_buf);
        }

        (unsafe { device.end_command_buffer(curr_cmd_buf) })?;
        (unsafe { device.reset_fences(&[ctx.frame_in_flight_fences[sync_frame_index]]) })?;


        //send command buffer to the queue
        let wait_semaphores = [ctx.image_available_semaphores[sync_frame_index]];
        let signal_semaphores = [ctx.rendering_finished_semaphores[image_index as usize]];
        let wait_stages = [vk::PipelineStageFlags::TRANSFER];
        let cmd_buff_array = [curr_cmd_buf];
        let submit_info = vk::SubmitInfo::default()
            .wait_semaphores(&wait_semaphores)
            .wait_dst_stage_mask(&wait_stages)
            .command_buffers(&cmd_buff_array)
            .signal_semaphores(&signal_semaphores);

        (unsafe { device.queue_submit(ctx.graphics_queue, &[submit_info], ctx.frame_in_flight_fences[sync_frame_index]) })?;

        let swapchains = [sc.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&signal_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);



        (unsafe { sc.loader.queue_present(ctx.present_queue, &present_info) })?;
        ctx.current_frame = ctx.current_frame + 1;
        Ok(())
    }

    unsafe fn destroy(&mut self){
        unsafe { self.logical_device.device_wait_idle().ok(); };
        
        for sem in self.vulkan_context.rendering_finished_semaphores.drain(..){
            unsafe { self.logical_device.destroy_semaphore(sem, None) };
        }
        self.vulkan_context.images_in_flight_fences.clear();
        drop(self.vulkan_context.swapchain.take());
        self.deletion_queue.flush();

        unsafe{
            self.logical_device.destroy_device(None);
            self.vulkan_context.surface_loader.destroy_surface(self.vulkan_context.surface, None);
            self.instance.destroy_instance(None);
        }
        info!("Cleanup:: All resources released!")
    }
}
