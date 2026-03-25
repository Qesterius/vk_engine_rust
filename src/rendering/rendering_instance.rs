use std::{collections::HashSet, ffi::{CStr, c_void}};
use crate::{config::{
    APPLICATION_NAME, ENGINE_NAME, ENGINE_VERSION, VALIDATION_ENABLED
}, utils};

//Holds GPU API instance. Currently it encapsulates Vulkan
use ash::{Entry,Instance, vk};
use anyhow::{Result, Ok, anyhow};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use winit::window::Window;
use ash::ext::debug_utils;
use log::{ error, info, warn };

// Use the "name" method on the extension's functional struct
const PORTABILITY_ENUMERATION_EXTENSION_NAME: &std::ffi::CStr = ash::khr::portability_enumeration::NAME;
const GET_PHYSICAL_DEVICE_PROPERTIES2_EXTENSION_NAME: &std::ffi::CStr = ash::khr::get_physical_device_properties2::NAME;
const VALIDATION_LAYER: &std::ffi::CStr = c"VK_LAYER_KHRONOS_validation";



pub struct RenderingInstance{
    pub(crate) debug_utils : Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub entry: ash::Entry,
    pub instance: ash::Instance,
}


impl RenderingInstance{
    pub fn new(window: &Window) -> Result<Self>{

        let entry = unsafe {ash::Entry::load()?};
        let instance = create_instance(&window, &entry)?;
        
        //debug
        let mut debug_utils = Some((debug_utils::Instance::new(&entry, &instance), vk::DebugUtilsMessengerEXT::null()));
        if VALIDATION_ENABLED {
           debug_utils = unsafe { setup_debug_messenger(&entry, &instance).ok() };
        }
        
        Ok(Self { debug_utils: debug_utils, entry: entry, instance: instance })
    }

    pub fn create_surface(&self, window: &Window) -> Result<(vk::SurfaceKHR, ash::khr::surface::Instance)>{
        let surface_loader = ash::khr::surface::Instance::new(&self.entry, &self.instance);
        let surface = (unsafe {
            ash_window::create_surface(
                &self.entry,
                &self.instance,
                window.display_handle()?.as_raw(),
                window.window_handle()?.as_raw(),
                None
            )
        })?;
        Ok((surface,surface_loader))
    }
}
impl Drop for RenderingInstance {
    fn drop(&mut self) {
        unsafe {
            if let Some((loader, messenger)) = self.debug_utils.take() {
                loader.destroy_debug_utils_messenger(messenger, None);
            }
            self.instance.destroy_instance(None);
        }
    }
}



//debug
extern "system" fn debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void
) -> vk::Bool32 {
    let data = unsafe { *p_callback_data };
    let message = unsafe { CStr::from_ptr(data.p_message) };

    match message_severity {
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {
            error!("{:?} - {:?}", message_type, message);
        }
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {
            warn!("{:?} - {:?}", message_type, message);
        }
        _ => info!("{:?} - {:?}", message_type, message),
    }

    vk::FALSE
}

unsafe fn setup_debug_messenger(
    entry: &ash::Entry,
    instance: &ash::Instance
) -> Result<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)> {
    let debug_utils_loader = ash::ext::debug_utils::Instance::new(entry, instance);
    let create_info = vk::DebugUtilsMessengerCreateInfoEXT
        ::default()
        .message_severity(
            vk::DebugUtilsMessageSeverityFlagsEXT::ERROR |
                vk::DebugUtilsMessageSeverityFlagsEXT::INFO |
                vk::DebugUtilsMessageSeverityFlagsEXT::WARNING
        )
        .message_type(
            vk::DebugUtilsMessageTypeFlagsEXT::GENERAL |
                vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION |
                vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
        )
        .pfn_user_callback(Some(debug_callback));

    let messenger = (unsafe {
        debug_utils_loader.create_debug_utils_messenger(&create_info, None)
    })?;

    Ok((debug_utils_loader, messenger))
}


fn create_instance(window: &Window, entry:&Entry) -> Result<Instance>{

    let mut extensions = ash_window::enumerate_required_extensions(
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