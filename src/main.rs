mod utils;
mod cleanup;
mod vulkan_context;
mod config;

use crate::config::{VALIDATION_ENABLED, APPLICATION_NAME, APPLICATION_TITLE, ENGINE_NAME, ENGINE_VERSION};
use ash::{ Entry, Instance };
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
        Ok(())
    }

    unsafe fn destroy(&mut self){
        self.deletion_queue.flush();

        unsafe{
            self.logical_device.destroy_device(None);
            self.vulkan_context.surface_loader.destroy_surface(self.vulkan_context.surface, None);
            self.instance.destroy_instance(None);
        }
        info!("Cleanup:: All resources released!")
    }
}
