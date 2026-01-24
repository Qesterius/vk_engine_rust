use crate::{cleanup::DeletionQueue, utils};
use crate::utils::vk_to_cstr;
use crate::config::VALIDATION_ENABLED;
use core::fmt;

use ash::vk::ImageView;
use ash::{Instance, khr::surface, vk::{self, PhysicalDevice} };
use log::{error, info, warn};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use anyhow::{Ok, Result};
use std::ffi::{CStr, c_void};
use anyhow::anyhow;
use winit::window::Window;

//Required Device extensions
const DEVICE_EXTENSIONS: &[&std::ffi::CStr] = &[
    ash::khr::swapchain::NAME,
];

#[derive(Clone)]
pub(crate) struct VulkanContext{
    pub(crate) surface : vk::SurfaceKHR, 
    pub(crate) surface_loader : ash::khr::surface::Instance,
    physical_device: vk::PhysicalDevice,
    graphics_queue : vk::Queue,
    present_queue : vk::Queue,
    swapchain: vk::SwapchainKHR,
    swapchain_loader : ash::khr::swapchain::Device,
    swapchain_images: Vec<vk::Image>,
    swapchain_image_views: Vec<ImageView>
}


impl fmt::Debug for VulkanContext{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VulkanContext")
            .field("surface", &self.surface)
            .field("physical_device", &self.physical_device)
            .field("graphics_queue", &self.graphics_queue)
            .finish_non_exhaustive() 
    }
}

impl VulkanContext{
   pub unsafe fn init(
        window: &winit::window::Window,
        entry: &ash::Entry,
        instance: &ash::Instance,
        deletion_queue: &mut DeletionQueue,
    ) -> Result<(Self, ash::Device)> {
        //surface
        let surface_loader = surface::Instance::new(&entry, &instance);
        let surface = unsafe{
            ash_window::create_surface(
                &entry, 
                &instance, 
                window.display_handle()?.as_raw(), 
                window.window_handle()?.as_raw(), 
                None)
        }?;

        //devices
        let (physical_device, q_family_indices) = unsafe { pick_physical_device(&instance, surface, &surface_loader) }?;
        let (logical_device, graphics_queue, present_queue) = create_logical_device( 
            &instance,
            physical_device,
            &q_family_indices
        )?; //logical device is cleaned from main.rs

        //debug
        if VALIDATION_ENABLED { 
            let (debug_loader, debug_messenger) = unsafe { setup_debug_messenger(entry, instance) }?;
            let deletion_queue_clone = debug_loader.clone();
            deletion_queue.push(move || {
                unsafe { deletion_queue_clone.destroy_debug_utils_messenger(debug_messenger, None); };
            });
        }

        //swapchain
        let (swapchain_loader, swapchain, swapchain_images, swapchain_image_views ) = unsafe { create_swapchain(window, instance, physical_device, &logical_device, surface, &surface_loader, deletion_queue) }?;
        let swapchain_loader_clone = swapchain_loader.clone();
        deletion_queue.push(move || {
            unsafe { swapchain_loader_clone.destroy_swapchain(swapchain, None); };
        });

        //return
        let context = Self{
            surface:surface,
            surface_loader:surface_loader,
            physical_device:physical_device,
            graphics_queue:graphics_queue,
            present_queue:present_queue,
            swapchain:swapchain,
            swapchain_loader:swapchain_loader,
            swapchain_images:swapchain_images,
            swapchain_image_views:swapchain_image_views
        };

        Ok((context,logical_device))
    }
}


//logical device
fn create_logical_device(
    instance :&Instance, 
    physical_device : vk::PhysicalDevice, 
    indices: &QueueFamilyIndices
) -> Result<(ash::Device, vk::Queue, vk::Queue)>{
    let mut unique_indices = std::collections::HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = [1.0_f32]; //https://docs.vulkan.org/spec/latest/chapters/devsandqueues.html#devsandqueues-priority 0-1. this is just smart 1
    let queue_create_infos : Vec<vk::DeviceQueueCreateInfo> = unique_indices
        .iter()
        .map(|&i|{
            vk::DeviceQueueCreateInfo::default()
            .queue_family_index(i)
            .queue_priorities(&queue_priorities)
        })
        .collect();

    let mut extensions_names_ptr : Vec<*const std::ffi::c_char> = DEVICE_EXTENSIONS
        .iter()
        .map(| ext | ext.as_ptr())
        .collect();
    
    let available_extensions = unsafe { instance.enumerate_device_extension_properties(physical_device) }?;
    let portability_supported = available_extensions.iter().any(|ext| {
        let name = vk_to_cstr(&ext.extension_name);
        name == vk::KHR_PORTABILITY_SUBSET_NAME
    });

    if portability_supported{
        info!("Adding Portability Subset extension.");
        extensions_names_ptr.push(vk::KHR_PORTABILITY_SUBSET_NAME.as_ptr());
    }

    let features = vk::PhysicalDeviceFeatures::default();
    let info = vk::DeviceCreateInfo::default()
    .queue_create_infos(&queue_create_infos)
    .enabled_features(&features)
    .enabled_extension_names(&extensions_names_ptr);

    let device = unsafe { instance.create_device(
        physical_device,
        &info,
        None) }?;
    
    let graphics_queue = unsafe { device.get_device_queue(indices.graphics, 0) };
    let present_queue = unsafe { device.get_device_queue(indices.present, 0) };

    Ok((device, graphics_queue, present_queue))    
}

struct QueueFamilyIndices{
    graphics: u32,
    present: u32
}

impl QueueFamilyIndices{
    unsafe fn get(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        surface : vk::SurfaceKHR,
        surface_loader : &ash::khr::surface::Instance
    ) -> Result<Self>{
        let properties = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
        
        let mut graphics = None;
        let mut present = None;

        for (index, info) in properties.iter().enumerate(){
            let i = index as u32;
            if info.queue_flags.contains(vk::QueueFlags::GRAPHICS){
                graphics = Some(i);
            }
            if unsafe{ surface_loader.get_physical_device_surface_support(physical_device, i, surface)?}{
                present = Some(i);
            }
            if let(Some(g), Some(p)) =  (graphics, present){
                return Ok( Self{graphics:g, present:p});
            }
        }
        return Err(anyhow!("Device does not support required queue families!"));
    }
}

//swapchain
struct SwapchainSupportDetails{
    capabilites : vk::SurfaceCapabilitiesKHR,
    formats : Vec<vk::SurfaceFormatKHR>,
    present_modes : Vec<vk::PresentModeKHR>
}
impl SwapchainSupportDetails{
    unsafe fn get(
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader : &ash::khr::surface::Instance
    ) -> Result<Self> {
        Ok(Self {
            capabilites: unsafe { surface_loader.get_physical_device_surface_capabilities(physical_device, surface)? },
            formats : unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface)? },
            present_modes : unsafe { surface_loader.get_physical_device_surface_present_modes(physical_device, surface)? }
        })
    }

}

fn get_swapchain_surface_format( formats: &[vk::SurfaceFormatKHR])-> vk::SurfaceFormatKHR {
    formats
        .iter()
        .cloned()
        .find(|f| 
            { f.format == vk::Format::B8G8R8A8_SRGB 
                && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR})
        .unwrap_or_else(|| formats[0]) //TODO: Prioritize better alternatives 
}
fn get_swapchain_present_mode(present_modes : &[vk::PresentModeKHR])-> vk::PresentModeKHR{
    present_modes
        .iter()
        .cloned()
        .find(|p| {
            *p == vk::PresentModeKHR::MAILBOX
        })
        .unwrap_or(vk::PresentModeKHR::FIFO)
}
//resolution of swapchain images
fn get_swapchain_extent( window: &Window, capabilities: vk::SurfaceCapabilitiesKHR) -> vk::Extent2D{
    if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        vk::Extent2D{ 
            width : window
                    .inner_size()
                    .width
                    .clamp(
                            capabilities.min_image_extent.width,
                            capabilities.max_image_extent.width 
                        ),
            height : window
                    .inner_size()
                    .width
                    .clamp(
                            capabilities.min_image_extent.height,
                            capabilities.max_image_extent.height 
                        )
            }
    }
}

unsafe fn create_swapchain(
    window: &Window, 
    instance :&Instance, 
    physical_device: vk::PhysicalDevice,
    logical_device: &ash::Device,
    surface: vk::SurfaceKHR,
    surface_loader : &ash::khr::surface::Instance,
    deletion_queue : &mut DeletionQueue,
)-> Result<(
    ash::khr::swapchain::Device,
    vk::SwapchainKHR,
    Vec<vk::Image>,
    Vec<ImageView>
    )>
{
    let indices = unsafe { QueueFamilyIndices::get(instance, physical_device, surface, surface_loader) }?;
    let support = unsafe { SwapchainSupportDetails::get(physical_device, surface, surface_loader) }?;

    let surface_format = get_swapchain_surface_format(&support.formats);
    let present_mode  = get_swapchain_present_mode(&support.present_modes);
    let extent = get_swapchain_extent(window, support.capabilites);

    let mut image_count = support.capabilites.min_image_count + 1;
    if support.capabilites.max_image_count != 0 {
        image_count = std::cmp::min( image_count, support.capabilites.max_image_count);
    }

    let mut queue_family_indices = vec![];
    let image_sharing_mode = if indices.graphics != indices.present{
        queue_family_indices.push(indices.graphics);
        queue_family_indices.push(indices.present);
        vk::SharingMode::CONCURRENT
    } else{
        vk::SharingMode::EXCLUSIVE
    };
    let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
            .image_sharing_mode(image_sharing_mode)
            .queue_family_indices(&queue_family_indices)
            .pre_transform(support.capabilites.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(vk::SwapchainKHR::null());
    let swapchain_loader = ash::khr::swapchain::Device::new(instance, logical_device);
    let swapchain = unsafe{ swapchain_loader.create_swapchain( &info, None)}?;
    
    //initialization of images
    let swapchain_images =  unsafe { swapchain_loader.get_swapchain_images(swapchain) }?;
    let mut swapchain_image_views = Vec::with_capacity(swapchain_images.len());

    for image in &swapchain_images{
        let create_view_info = vk::ImageViewCreateInfo::default()
                                                        .image(*image)
                                                        .view_type(vk::ImageViewType::TYPE_2D)// treating images as 2d/3d/1d textures or cube maps
                                                        .format(surface_format.format)
                                                        .components(vk::ComponentMapping::default()//can switch to other color channels (ex. all shades of red). identity is its just what it is
                                                                                        .r(vk::ComponentSwizzle::IDENTITY)
                                                                                        .g(vk::ComponentSwizzle::IDENTITY)
                                                                                        .b(vk::ComponentSwizzle::IDENTITY)
                                                                                        .a(vk::ComponentSwizzle::IDENTITY))
                                                        .subresource_range(vk::ImageSubresourceRange::default()
                                                                                                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                                                                                                    .base_mip_level(0)
                                                                                                    .level_count(1)
                                                                                                    .base_array_layer(0)
                                                                                                    .layer_count(1));
        let image_view = unsafe { logical_device.create_image_view(&create_view_info, None) }?;
        let logical_device_clone = logical_device.clone();
        deletion_queue.push(move|| { unsafe { logical_device_clone.destroy_image_view(image_view, None) };});
        swapchain_image_views.push(image_view);
    }
    
    Ok((swapchain_loader, swapchain, swapchain_images, swapchain_image_views))
}

//phyiscal device
unsafe fn check_physical_device(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance
) -> Result<QueueFamilyIndices>{

    let indicies = unsafe { QueueFamilyIndices::get(instance, physical_device, surface, surface_loader)? };
    (unsafe { check_device_extension_support(instance, physical_device) })?;

    let details = unsafe { SwapchainSupportDetails::get(physical_device, surface, surface_loader)? };
    if details.formats.is_empty() || details.present_modes.is_empty(){
        return Err(anyhow!("GPU supports Swapchwain extension, but has no compatible formats or present modes"))
    }
    Ok(indicies)
}

unsafe fn check_device_extension_support( instance: &Instance, physical_device: vk::PhysicalDevice) -> Result<()> {
    let available_extensions = unsafe { instance.enumerate_device_extension_properties(physical_device) }?;
    let available_extensions_names : std::collections::HashSet<String> =  available_extensions
    .iter()
    .map(
        |ext|
            utils::vk_to_cstr(&ext.extension_name )
            .to_string_lossy()
            .into_owned())
    .collect();
    
    for &extension in DEVICE_EXTENSIONS{
        let name = extension.to_string_lossy().into_owned();
        if !available_extensions_names.contains(&name){
            return Err(anyhow!("Missing required extension: {}", name));
        }
    }

    Ok(())
}

unsafe fn pick_physical_device(
    instance: &Instance, 
    surface : vk::SurfaceKHR, 
    surface_loader: &ash::khr::surface::Instance 
) -> Result<(PhysicalDevice, QueueFamilyIndices)>{
    let physical_devices = unsafe { instance.enumerate_physical_devices() }?;
    for &physical_device in physical_devices.iter(){
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let name = utils::vk_to_cstr(&properties.device_name);
        //TODO: add scoring to pick best gpu first
        match unsafe { check_physical_device(instance, physical_device, surface, surface_loader) } {
            Result::Ok(indices) =>{ 
                info!("Selected GPU: {:?}", name);
                return Ok((physical_device, indices));
            }
            Result::Err(e) =>{
                warn!("Skipping GPU {:?}: {}", name, e);
            }
        }
    }
    Err(anyhow!("Failed to pick any suitable device!"))
}


//debug
extern "system" fn debug_callback(
    message_severity: vk::DebugUtilsMessageSeverityFlagsEXT,
    message_type: vk::DebugUtilsMessageTypeFlagsEXT,
    p_callback_data: *const vk::DebugUtilsMessengerCallbackDataEXT,
    _: *mut c_void,
) -> vk::Bool32 {

    let data = unsafe{ *p_callback_data};
    let message = unsafe{ CStr::from_ptr(data.p_message)};

    match message_severity{
        vk::DebugUtilsMessageSeverityFlagsEXT::ERROR => {error!("{:?} - {:?}", message_type, message)}
        vk::DebugUtilsMessageSeverityFlagsEXT::WARNING => {warn!("{:?} - {:?}", message_type, message)}
        _ => info!("{:?} - {:?}", message_type, message)
    }

    vk::FALSE
}

unsafe fn setup_debug_messenger (
    entry: &ash::Entry,
    instance: &ash::Instance
) -> Result<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>{
    let debug_utils_loader = ash::ext::debug_utils::Instance::new(entry, instance);
    let create_info = vk::DebugUtilsMessengerCreateInfoEXT::default()
    .message_severity(vk::DebugUtilsMessageSeverityFlagsEXT::ERROR 
                        | vk::DebugUtilsMessageSeverityFlagsEXT::INFO
                        | vk::DebugUtilsMessageSeverityFlagsEXT::WARNING)
    .message_type(vk::DebugUtilsMessageTypeFlagsEXT::GENERAL
                    | vk::DebugUtilsMessageTypeFlagsEXT::VALIDATION
                    | vk::DebugUtilsMessageTypeFlagsEXT::PERFORMANCE
                )
    .pfn_user_callback(Some(debug_callback));

    let messenger = unsafe { debug_utils_loader.create_debug_utils_messenger(&create_info, None) }?;

    Ok((debug_utils_loader, messenger))
}