use crate::config::VALIDATION_ENABLED;
use crate::utils::vk_to_cstr;
use crate::{ cleanup::DeletionQueue, utils };
use core::fmt;

use anyhow::anyhow;
use anyhow::{ Ok, Result };
use ash::vk::ImageView;
use ash::{ Instance, khr::surface, vk::{ self, PhysicalDevice } };
use log::{ error, info, warn };
use raw_window_handle::{ HasDisplayHandle, HasWindowHandle };
use std::ffi::{ CStr, c_void };
use winit::window::Window;

//Required Device extensions
const DEVICE_EXTENSIONS: &[&std::ffi::CStr] = &[ash::khr::swapchain::NAME];
pub const MAX_FRAMES_IN_FLIGHT : usize = 2;

#[derive(Clone)]
pub(crate) struct VulkanContext {
    pub(crate) surface: vk::SurfaceKHR,
    pub(crate) surface_loader: ash::khr::surface::Instance,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) graphics_queue: vk::Queue,
    pub(crate) present_queue: vk::Queue,
    pub(crate) swapchain: Option<SafeSwapchain>,
    pub(crate) image_available_semaphores: Vec<vk::Semaphore>,
    pub(crate) rendering_finished_semaphores: Vec<vk::Semaphore>,
    pub(crate) frame_in_flight_fences: Vec<vk::Fence>,
    pub(crate) images_in_flight_fences: Vec<vk::Fence>,
    pub(crate) current_frame: usize,
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) command_buffers: Vec<vk::CommandBuffer>,
    pub(crate) render_pass: vk::RenderPass,
}

impl fmt::Debug for VulkanContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VulkanContext")
            .field("surface", &self.surface)
            .field("physical_device", &self.physical_device)
            .field("graphics_queue", &self.graphics_queue)
            .finish_non_exhaustive()
    }
}

impl VulkanContext {
    pub unsafe fn init(
        window: &winit::window::Window,
        entry: &ash::Entry,
        instance: &ash::Instance,
        deletion_queue: &mut DeletionQueue
    ) -> Result<(Self, ash::Device)> {
        //surface
        let surface_loader = surface::Instance::new(&entry, &instance);
        let surface = (unsafe {
            ash_window::create_surface(
                &entry,
                &instance,
                window.display_handle()?.as_raw(),
                window.window_handle()?.as_raw(),
                None
            )
        })?;

        //devices
        let (physical_device, q_family_indices) = (unsafe {
            pick_physical_device(&instance, surface, &surface_loader)
        })?;
        let (logical_device, graphics_queue, present_queue) = create_logical_device(
            &instance,
            physical_device,
            &q_family_indices
        )?; //logical device is cleaned from main.rs

        //debug
        if VALIDATION_ENABLED {
            let (debug_loader, debug_messenger) = (unsafe {
                setup_debug_messenger(entry, instance)
            })?;
            let deletion_queue_clone = debug_loader.clone();
            deletion_queue.push(move || {
                unsafe {
                    deletion_queue_clone.destroy_debug_utils_messenger(debug_messenger, None);
                }
            });
        }
        
        let swapchain_details = (unsafe {
            get_swapchain_details(physical_device, surface, &surface_loader)
        })?;

        let surface_format = get_swapchain_surface_format(&swapchain_details.formats);

        //render pass
        let render_pass = (unsafe {
            create_render_pass(surface_format.format, &logical_device, deletion_queue)
        })?;
        
        //swapchain
        let swapchain = (unsafe {SafeSwapchain::new(
                swapchain_details,
                window,
                instance,
                physical_device,
                &logical_device,
                surface,
                &surface_loader,
                window.inner_size(),
                render_pass
            )
        })?;

        //command pool/buffer
        let pool_info = vk::CommandPoolCreateInfo
            ::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(q_family_indices.graphics);
        let command_pool = (unsafe { logical_device.create_command_pool(&pool_info, None) })?;

        let buffer_info = vk::CommandBufferAllocateInfo
            ::default()
            .command_pool(command_pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(MAX_FRAMES_IN_FLIGHT.try_into().unwrap());
            
        let command_buffers = (unsafe { logical_device.allocate_command_buffers(&buffer_info) })?;

        let logical_device_clone = logical_device.clone();
        deletion_queue.push(move || unsafe {
            logical_device_clone.destroy_command_pool(command_pool, None);
        });

        //pipeline
        (unsafe { create_pipeline() })?;

        // gpu cpu synchronization
        //
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        let signaled_fence_info = vk::FenceCreateInfo
            ::default()
            .flags(vk::FenceCreateFlags::SIGNALED); // by default starts as unsignaled. We use flag to init it in Signaled state
        
        let mut image_available_semaphores = Vec::new();
        let mut rendering_finished_semaphores = Vec::new();
        let mut frame_in_flight_fences = Vec::new();
        let mut images_in_flight_fences = Vec::new();

        for _ in 0..swapchain.images.len() {
            images_in_flight_fences.push(vk::Fence::null());
            rendering_finished_semaphores.push((unsafe { logical_device.create_semaphore(&semaphore_info, None) })?);
        }

        for _ in 0..MAX_FRAMES_IN_FLIGHT {
            frame_in_flight_fences.push((unsafe { logical_device.create_fence(&signaled_fence_info, None) })?);
            image_available_semaphores.push((unsafe { logical_device.create_semaphore(&semaphore_info, None) })?);
        }
            
        let logical_device_clone = logical_device.clone();
        let frame_in_flight_fences_copy = frame_in_flight_fences.clone();
        let image_available_sems_copy = image_available_semaphores.clone();
        deletion_queue.push(move || unsafe {
            for fence in frame_in_flight_fences_copy {
                logical_device_clone.destroy_fence(fence, None);
            }
            for semaphore in image_available_sems_copy {
                logical_device_clone.destroy_semaphore(semaphore, None);
            }
        });
        

        //return
        let context = Self {
            surface: surface,
            surface_loader: surface_loader,
            physical_device: physical_device,
            graphics_queue: graphics_queue,
            present_queue: present_queue,
            swapchain: Some(swapchain),
            image_available_semaphores: image_available_semaphores,
            rendering_finished_semaphores: rendering_finished_semaphores,
            frame_in_flight_fences: frame_in_flight_fences,
            images_in_flight_fences: images_in_flight_fences,
            current_frame: 0,
            command_buffers,
            command_pool: command_pool,
            render_pass: render_pass,
        };

        Ok((context, logical_device))
    }

    pub unsafe fn recreate_swapchain(&mut self, window: &Window, size: winit::dpi::PhysicalSize<u32>, instance: &Instance, logical_device: &ash::Device) -> Result<()> {
        (unsafe { logical_device.device_wait_idle() })?;

        for sem in self.rendering_finished_semaphores.drain(..) {
            unsafe { logical_device.destroy_semaphore(sem, None) };
        }

        let swapchain_details = (unsafe {
            get_swapchain_details(self.physical_device, self.surface, &self.surface_loader)
        })?;
        self.swapchain = None; // Drop old swapchain and its resources
        let swapchain = unsafe { SafeSwapchain::new(
            swapchain_details,
            window,
            instance,
            self.physical_device,
            logical_device,
            self.surface,
            &self.surface_loader,
            size,
            self.render_pass
        )?};
        let new_image_count = swapchain.images.len();
        self.swapchain = Some(swapchain);

        // Re-create them for the new image count
        let semaphore_info = vk::SemaphoreCreateInfo::default();
        for _ in 0..new_image_count {
            let sem = unsafe { logical_device.create_semaphore(&semaphore_info, None)? };
            self.rendering_finished_semaphores.push(sem);
        }

        // Reset image-to-fence mapping (it's safe to just clear and fill with null)
        self.images_in_flight_fences.clear();
        self.images_in_flight_fences.resize(new_image_count, vk::Fence::null());

        Ok(())
    }
}

//logical device
fn create_logical_device(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    indices: &QueueFamilyIndices
) -> Result<(ash::Device, vk::Queue, vk::Queue)> {
    let mut unique_indices = std::collections::HashSet::new();
    unique_indices.insert(indices.graphics);
    unique_indices.insert(indices.present);

    let queue_priorities = [1.0_f32]; //https://docs.vulkan.org/spec/latest/chapters/devsandqueues.html#devsandqueues-priority 0-1. this is just smart 1
    let queue_create_infos: Vec<vk::DeviceQueueCreateInfo> = unique_indices
        .iter()
        .map(|&i| {
            vk::DeviceQueueCreateInfo
                ::default()
                .queue_family_index(i)
                .queue_priorities(&queue_priorities)
        })
        .collect();

    let mut extensions_names_ptr: Vec<*const std::ffi::c_char> = DEVICE_EXTENSIONS.iter()
        .map(|ext| ext.as_ptr())
        .collect();

    let available_extensions = (unsafe {
        instance.enumerate_device_extension_properties(physical_device)
    })?;
    let portability_supported = available_extensions.iter().any(|ext| {
        let name = vk_to_cstr(&ext.extension_name);
        name == vk::KHR_PORTABILITY_SUBSET_NAME
    });

    if portability_supported {
        info!("Adding Portability Subset extension.");
        extensions_names_ptr.push(vk::KHR_PORTABILITY_SUBSET_NAME.as_ptr());
    }

    let features = vk::PhysicalDeviceFeatures::default();
    let info = vk::DeviceCreateInfo
        ::default()
        .queue_create_infos(&queue_create_infos)
        .enabled_features(&features)
        .enabled_extension_names(&extensions_names_ptr);

    let device = (unsafe { instance.create_device(physical_device, &info, None) })?;

    let graphics_queue = unsafe { device.get_device_queue(indices.graphics, 0) };
    let present_queue = unsafe { device.get_device_queue(indices.present, 0) };

    Ok((device, graphics_queue, present_queue))
}

struct QueueFamilyIndices {
    graphics: u32,
    present: u32,
}

impl QueueFamilyIndices {
    unsafe fn get(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance
    ) -> Result<Self> {
        let properties = unsafe {
            instance.get_physical_device_queue_family_properties(physical_device)
        };

        let mut graphics = None;
        let mut present = None;

        for (index, info) in properties.iter().enumerate() {
            let i = index as u32;
            if info.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
                graphics = Some(i);
            }
            if unsafe {
                    surface_loader.get_physical_device_surface_support(physical_device, i, surface)?
                }  {
                present = Some(i);
            }
            if let (Some(g), Some(p)) = (graphics, present) {
                return Ok(Self {
                    graphics: g,
                    present: p,
                });
            }
        }
        return Err(anyhow!("Device does not support required queue families!"));
    }
}

//swapchain
struct SwapchainSupportDetails {
    capabilites: vk::SurfaceCapabilitiesKHR,
    formats: Vec<vk::SurfaceFormatKHR>,
    present_modes: Vec<vk::PresentModeKHR>,
}
impl SwapchainSupportDetails {
    unsafe fn get(
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance
    ) -> Result<Self> {
        Ok(Self {
            capabilites: unsafe {
                surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
            },
            formats: unsafe {
                surface_loader.get_physical_device_surface_formats(physical_device, surface)?
            },
            present_modes: unsafe {
                surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
            },
        })
    }
}

fn get_swapchain_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    formats
        .iter()
        .cloned()
        .find(|f| {
            f.format == vk::Format::B8G8R8A8_SRGB &&
                f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        })
        .unwrap_or_else(|| formats[0]) //TODO: Prioritize better alternatives
}
fn get_swapchain_present_mode(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .cloned()
        .find(|p| *p == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

#[derive(Clone)]
pub struct SafeSwapchain{
    pub swapchain: vk::SwapchainKHR,
    pub loader: ash::khr::swapchain::Device,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub extent: vk::Extent2D,
    device: ash::Device,
    pub format: vk::Format,
}
impl Drop for SafeSwapchain {
    fn drop(&mut self) {
        unsafe {
            for &view in &self.image_views {
                self.device.destroy_image_view(view, None);
            }
            for &fb in &self.framebuffers {
                self.device.destroy_framebuffer(fb, None);
            }
            self.loader.destroy_swapchain(self.swapchain, None);
        }
    }
}

fn get_swapchain_details(physical_device: vk::PhysicalDevice, surface: vk::SurfaceKHR, surface_loader: &ash::khr::surface::Instance) -> Result<SwapchainSupportDetails> {
    Ok(SwapchainSupportDetails {
        capabilites: unsafe {
            surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
        },
        formats: unsafe {
            surface_loader.get_physical_device_surface_formats(physical_device, surface)?
        },
        present_modes: unsafe {
            surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
        },
    })
}

impl SafeSwapchain {
    pub unsafe fn new(
        swapchain_details: SwapchainSupportDetails,
        window: &Window,
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        logical_device: &ash::Device,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
        size: winit::dpi::PhysicalSize<u32>,
        render_pass: vk::RenderPass
    ) -> Result<Self> {

        let indices = (unsafe {
            QueueFamilyIndices::get(instance, physical_device, surface, surface_loader)
        })?;
        let support = swapchain_details;

        let surface_format = get_swapchain_surface_format(&support.formats);
        let present_mode = get_swapchain_present_mode(&support.present_modes);
        let extent = vk::Extent2D {
                width: size.width,
                height: size.height,
            };
        let mut image_count = support.capabilites.min_image_count + 1;
        if support.capabilites.max_image_count != 0 {
            image_count = std::cmp::min(image_count, support.capabilites.max_image_count);
        }

        let mut queue_family_indices = vec![];
        let image_sharing_mode = if indices.graphics != indices.present {
            queue_family_indices.push(indices.graphics);
            queue_family_indices.push(indices.present);
            vk::SharingMode::CONCURRENT
        } else {
            vk::SharingMode::EXCLUSIVE
        };
        let info = vk::SwapchainCreateInfoKHR
            ::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1) 
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST) // we want to render directly to the swapchain images, so COLOR_ATTACHMENT. We also want to copy rendered
            .image_sharing_mode(image_sharing_mode)
            .queue_family_indices(&queue_family_indices)
            .pre_transform(support.capabilites.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(vk::SwapchainKHR::null());
        let swapchain_loader = ash::khr::swapchain::Device::new(instance, logical_device);
        let swapchain = (unsafe { swapchain_loader.create_swapchain(&info, None) })?;

        //initialization of images
        let swapchain_images = (unsafe { swapchain_loader.get_swapchain_images(swapchain) })?;
        let mut swapchain_image_views = Vec::with_capacity(swapchain_images.len());

        for image in &swapchain_images {
            let create_view_info = vk::ImageViewCreateInfo
                ::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D) // treating images as 2d/3d/1d textures or cube maps
                .format(surface_format.format)
                .components(
                    vk::ComponentMapping
                        ::default() //can switch to other color channels (ex. all shades of red). identity is its just what it is
                        .r(vk::ComponentSwizzle::IDENTITY)
                        .g(vk::ComponentSwizzle::IDENTITY)
                        .b(vk::ComponentSwizzle::IDENTITY)
                        .a(vk::ComponentSwizzle::IDENTITY)
                )
                .subresource_range(
                    vk::ImageSubresourceRange
                        ::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1)
                );
            let image_view = (unsafe { logical_device.create_image_view(&create_view_info, None) })?;

            swapchain_image_views.push(image_view);
        }
        let mut framebuffers = Vec::with_capacity(swapchain_images.len());
        for &image_view in &swapchain_image_views {
            let attachments = [image_view];
            let framebuffer_info = vk::FramebufferCreateInfo
                ::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(extent.width)
                .height(extent.height)
                .layers(1);
            let framebuffer = (unsafe { logical_device.create_framebuffer(&framebuffer_info, None) })?;
            framebuffers.push(framebuffer);
        }

        Ok(Self {
            swapchain,
            loader: swapchain_loader,
            images: swapchain_images,
            image_views: swapchain_image_views,
            device: logical_device.clone(),
            extent: extent,
            format: surface_format.format,
            framebuffers: framebuffers,
        })
    }
}


//resolution of swapchain images
fn get_swapchain_extent(window: &Window, capabilities: vk::SurfaceCapabilitiesKHR) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        vk::Extent2D {
            width: window
                .inner_size()
                .width.clamp(
                    capabilities.min_image_extent.width,
                    capabilities.max_image_extent.width
                ),
            height: window
                .inner_size()
                .width.clamp(
                    capabilities.min_image_extent.height,
                    capabilities.max_image_extent.height
                ),
        }
    }
}

//pipeline
unsafe fn create_pipeline() -> Result<()> {
    Ok(())
}

//phyiscal device
unsafe fn check_physical_device(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance
) -> Result<QueueFamilyIndices> {
    let indicies = unsafe {
        QueueFamilyIndices::get(instance, physical_device, surface, surface_loader)?
    };
    (unsafe { check_device_extension_support(instance, physical_device) })?;

    let details = unsafe {
        SwapchainSupportDetails::get(physical_device, surface, surface_loader)?
    };
    if details.formats.is_empty() || details.present_modes.is_empty() {
        return Err(
            anyhow!(
                "GPU supports Swapchwain extension, but has no compatible formats or present modes"
            )
        );
    }
    Ok(indicies)
}

unsafe fn check_device_extension_support(
    instance: &Instance,
    physical_device: vk::PhysicalDevice
) -> Result<()> {
    let available_extensions = (unsafe {
        instance.enumerate_device_extension_properties(physical_device)
    })?;
    let available_extensions_names: std::collections::HashSet<String> = available_extensions
        .iter()
        .map(|ext| { utils::vk_to_cstr(&ext.extension_name).to_string_lossy().into_owned() })
        .collect();

    for &extension in DEVICE_EXTENSIONS {
        let name = extension.to_string_lossy().into_owned();
        if !available_extensions_names.contains(&name) {
            return Err(anyhow!("Missing required extension: {}", name));
        }
    }

    Ok(())
}

unsafe fn pick_physical_device(
    instance: &Instance,
    surface: vk::SurfaceKHR,
    surface_loader: &ash::khr::surface::Instance
) -> Result<(PhysicalDevice, QueueFamilyIndices)> {
    let physical_devices = (unsafe { instance.enumerate_physical_devices() })?;
    for &physical_device in physical_devices.iter() {
        let properties = unsafe { instance.get_physical_device_properties(physical_device) };
        let name = utils::vk_to_cstr(&properties.device_name);
        //TODO: add scoring to pick best gpu first
        match unsafe {
                check_physical_device(instance, physical_device, surface, surface_loader)
            }  {
            Result::Ok(indices) => {
                info!("Selected GPU: {:?}", name);
                return Ok((physical_device, indices));
            }
            Result::Err(e) => {
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

unsafe fn create_render_pass(format: vk::Format, logical_device: &ash::Device, deletion_queue: &mut DeletionQueue) -> Result<vk::RenderPass> {
    let color_attachment_description = vk::AttachmentDescription
        ::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);
    let color_attachment_ref = vk::AttachmentReference
        ::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
    let subpass_description = vk::SubpassDescription
        ::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(std::slice::from_ref(&color_attachment_ref));

    let render_pass_info = vk::RenderPassCreateInfo
        ::default()
        .attachments(std::slice::from_ref(&color_attachment_description))
        .subpasses(std::slice::from_ref(&subpass_description));

    let render_pass = (unsafe { logical_device.create_render_pass(&render_pass_info, None) })?;
    let logical_device_clone = logical_device.clone();
    deletion_queue.push(move || unsafe {
        logical_device_clone.destroy_render_pass(render_pass, None);
    });

    Ok(render_pass)
}

