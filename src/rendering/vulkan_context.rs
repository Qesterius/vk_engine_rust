use crate::config::{MAX_FRAMES_IN_FLIGHT, VALIDATION_ENABLED};
use crate::rendering::vertex::UniformBufferObject;
use super::buffer;
use super::memory;
use super::swapchain;
use super::vertex::Vertex;
use crate::utils::vk_to_cstr;
use super::cleanup::DeletionQueue;
use crate::utils;
use core::fmt;
use super::pipeline::PipelineBuilder;
use super::descriptor;

use anyhow::anyhow;
use anyhow::{ Ok, Result };
use ash::vk::{DescriptorPool, DescriptorSet, DescriptorSetLayout};
use ash::vk::{ImageView, LogicOp};
use ash::{ Instance, khr::surface, vk::{ self, PhysicalDevice } };
use cgmath::num_traits::ToPrimitive;
use log::{ error, info, warn };
use raw_window_handle::{ HasDisplayHandle, HasWindowHandle };
use std::ffi::{ CStr, c_void };
use winit::window::Window;

//Required Device extensions
const DEVICE_EXTENSIONS: &[&std::ffi::CStr] = &[ash::khr::swapchain::NAME];

#[derive(Clone)]
pub(crate) struct VulkanContext {
    pub(crate) surface: vk::SurfaceKHR,
    pub(crate) debug_utils : Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub(crate) surface_loader: ash::khr::surface::Instance,
    pub(crate) physical_device: vk::PhysicalDevice,
    pub(crate) graphics_queue: vk::Queue,
    pub(crate) present_queue: vk::Queue,
    pub(crate) swapchain: Option<swapchain::SafeSwapchain>,
    pub(crate) image_available_semaphores: Vec<vk::Semaphore>,
    pub(crate) rendering_finished_semaphores: Vec<vk::Semaphore>,
    pub(crate) frame_in_flight_fences: Vec<vk::Fence>,
    pub(crate) images_in_flight_fences: Vec<vk::Fence>,
    pub(crate) current_frame: usize,
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) command_buffers: Vec<vk::CommandBuffer>,
    pub(crate) render_pass: vk::RenderPass,
    pub(crate) graphics_pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) vertex_buffer: Option<(vk::Buffer, vk::DeviceMemory)>,
    pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) descriptor_sets: Vec<vk::DescriptorSet>, //one per frame in flight
    pub(crate) uniform_buffers: Vec<(vk::Buffer, vk::DeviceMemory)> //one per frame in flight
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
        let mut debug_utils = Some((ash::ext::debug_utils::Instance::new(entry, instance), vk::DebugUtilsMessengerEXT::null()));
        if VALIDATION_ENABLED {
           debug_utils = unsafe { setup_debugging(entry, instance, deletion_queue).ok() };
        }
        
        //swapchain
        let swapchain_details = (unsafe {
            swapchain::get_swapchain_details(physical_device, surface, &surface_loader)
        })?;

        let surface_format = swapchain::get_swapchain_surface_format(&swapchain_details.formats);

        //render pass
        let render_pass = (unsafe {
            create_render_pass(surface_format.format, &logical_device, deletion_queue)
        })?;
        
        //swapchain
        let swapchain = (unsafe {swapchain::SafeSwapchain::new(
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
        let (command_pool, command_buffers) = setup_command_buffers(&logical_device, q_family_indices, deletion_queue)?;
        

        let vertices = [
            // Triangle 1
            Vertex::new([-0.5, -0.5, 0.0], [1.0, 0.0, 0.0]), // Top Left (inverted Y)
            Vertex::new([ 0.5, -0.5, 0.0], [0.0, 1.0, 0.0]), // Top Right
            Vertex::new([ 0.5,  0.5, 0.0], [0.0, 0.0, 1.0]), // Bottom Right
            // Triangle 2
            Vertex::new([ 0.5,  0.5, 0.0], [0.0, 0.0, 1.0]), // Bottom Right
            Vertex::new([-0.5,  0.5, 0.0], [1.0, 1.0, 1.0]), // Bottom Left
            Vertex::new([-0.5, -0.5, 0.0], [1.0, 0.0, 0.0]), // Top Left
        ];
        let vertex_buffer = unsafe { create_vertex_buffer(instance,  &logical_device, physical_device, &vertices, deletion_queue) };
        
        let (descriptor_set_layout,
            descriptor_pool, 
            descriptor_sets, 
            uniform_buffers) = unsafe { create_descriptor_resources(&logical_device, instance, physical_device, deletion_queue) }?;
        
        // gpu cpu synchronization
        let (image_available_semaphores, 
            rendering_finished_semaphores,
            frame_in_flight_fences,
            images_in_flight_fences) = setup_sync_objects(&logical_device, swapchain.images.len(), deletion_queue)?;        
        
        let descriptor_set_layouts = vec![descriptor_set_layout];
        let (graphics_pipeline, pipeline_layout) = create_triangle_pipeline(&logical_device, render_pass, descriptor_set_layouts, deletion_queue)?;
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
            graphics_pipeline: graphics_pipeline,
            pipeline_layout: pipeline_layout,
            debug_utils: debug_utils,
            vertex_buffer: Some(vertex_buffer?),
            descriptor_pool: descriptor_pool,
            descriptor_set_layout : descriptor_set_layout,
            descriptor_sets : descriptor_sets,
            uniform_buffers: uniform_buffers
        };

        Ok((context, logical_device))
    }

    pub unsafe fn recreate_swapchain(&mut self, window: &Window, size: winit::dpi::PhysicalSize<u32>, instance: &Instance, logical_device: &ash::Device) -> Result<()> {
        (unsafe { logical_device.device_wait_idle() })?;

        for sem in self.rendering_finished_semaphores.drain(..) {
            unsafe { logical_device.destroy_semaphore(sem, None) };
        }

        let swapchain_details = (unsafe {
            swapchain::get_swapchain_details(self.physical_device, self.surface, &self.surface_loader)
        })?;
        self.swapchain = None; // Drop old swapchain and its resources
        let swapchain = unsafe { swapchain::SafeSwapchain::new(
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

// pipeline
/// Creates a graphics pipeline for rendering triangles.
///
/// This function creates a complete graphics pipeline with vertex and fragment shaders,
/// vertex input bindings and attributes, and the specified descriptor set layouts.
/// It also handles proper cleanup of shader modules and pipeline resources.
///
/// # Arguments
/// * `logical_device` - The Vulkan logical device used to create the pipeline
/// * `render_pass` - The render pass that the pipeline will be compatible with
/// * `descriptor_set_layouts` - Array of descriptor set layouts for shader resources
/// * `deletion_queue` - Queue for managing cleanup of pipeline resources
///
/// # Returns
/// A tuple containing the created graphics pipeline and its layout
pub fn create_triangle_pipeline(logical_device: &ash::Device, render_pass: vk::RenderPass, descriptor_set_layouts : Vec<DescriptorSetLayout>, deletion_queue: &mut DeletionQueue) -> Result<(vk::Pipeline, vk::PipelineLayout)> {
    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&descriptor_set_layouts);
    let pipeline_layout = (unsafe { logical_device.create_pipeline_layout(&pipeline_layout_info, None) })?;

    let vert_module = (unsafe { load_shader_module(&logical_device, "src/rendering/shaders/vert.spv") })?;
    let frag_module = (unsafe { load_shader_module(&logical_device, "src/rendering/shaders/frag.spv") })?;
    let bindings = Vertex::get_binding_description();
    let attributes = Vertex::get_attribute_descriptions();

    let pipeline_builder = PipelineBuilder::new(pipeline_layout)
        .with_shader(vert_module, vk::ShaderStageFlags::VERTEX)
        .with_shader(frag_module, vk::ShaderStageFlags::FRAGMENT)
        .with_binding_descriptions(vec![bindings])
        .with_attribute_descriptions(attributes.to_vec());
    //rest is default


    let pipeline = pipeline_builder.build(logical_device, render_pass)?; 

    unsafe { logical_device.destroy_shader_module(vert_module, None) };
    unsafe { logical_device.destroy_shader_module(frag_module, None) };

    let logical_device_clone = logical_device.clone();
    deletion_queue.push(move || unsafe {
        logical_device_clone.destroy_pipeline(pipeline, None);
        logical_device_clone.destroy_pipeline_layout(pipeline_layout, None);
     });
    Ok((pipeline, pipeline_layout))
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

pub(crate) struct QueueFamilyIndices {
    pub graphics: u32,
    pub present: u32,
}

impl QueueFamilyIndices {
    pub unsafe fn get(
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
        swapchain::SwapchainSupportDetails::get(physical_device, surface, surface_loader)?
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

fn setup_sync_objects(logical_device: &ash::Device, swapchain_image_count: usize, deletion_queue: &mut DeletionQueue) -> Result<(Vec<vk::Semaphore>, Vec<vk::Semaphore>, Vec<vk::Fence>, Vec<vk::Fence>)> {
    let mut image_available_semaphores = Vec::new();
    let mut rendering_finished_semaphores = Vec::new();
    let mut frame_in_flight_fences = Vec::new();
    let mut images_in_flight_fences = Vec::new();


    let semaphore_info = vk::SemaphoreCreateInfo::default();
    let signaled_fence_info = vk::FenceCreateInfo
        ::default()
        .flags(vk::FenceCreateFlags::SIGNALED); // by default starts as unsignaled. We use flag to init it in Signaled state


    for _ in 0..swapchain_image_count {
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

    Ok((image_available_semaphores, rendering_finished_semaphores, frame_in_flight_fences, images_in_flight_fences))
}



fn setup_command_buffers(logical_device: &ash::Device, q_family_indices: QueueFamilyIndices, deletion_queue: &mut DeletionQueue) -> Result<(vk::CommandPool,Vec<vk::CommandBuffer>)> {
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
    Ok((command_pool, command_buffers))
}

//debug
fn setup_debugging(entry: &ash::Entry, instance: &ash::Instance, deletion_queue: &mut DeletionQueue) -> Result<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)> {
    let (debug_loader, debug_messenger) = (unsafe {
        setup_debug_messenger(entry, instance)
    })?;
    let deletion_queue_clone = debug_loader.clone();
    deletion_queue.push(move || {
        unsafe {
            deletion_queue_clone.destroy_debug_utils_messenger(debug_messenger, None);
        }
    });
    Ok((debug_loader, debug_messenger))
}

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

pub unsafe fn create_shader_module(logical_device: &ash::Device, code: &[u32]) -> Result<vk::ShaderModule> {
    let create_info = vk::ShaderModuleCreateInfo
        ::default()
        .code(code);

    let shader_module = (unsafe { logical_device.create_shader_module(&create_info, None) })?;
    Ok(shader_module)
}

pub unsafe fn load_shader_module(device: &ash::Device, path: &str) -> Result<vk::ShaderModule> {
    let file = std::fs::File::open(path).map_err(|e| anyhow!("Failed to open shader file {}:{}", path, e))?;
    let words = ash::util::read_spv(&mut std::io::BufReader::new(file)).map_err(|e| anyhow!("Failed to read shader file {}:{}", path, e))?;
    Ok(unsafe { create_shader_module(device, &words) }?)
}

//vertex buffer
unsafe fn create_vertex_buffer(instance: &ash::Instance, device: &ash::Device, physical_device: vk::PhysicalDevice, vertices: &[Vertex], deletion_queue: &mut DeletionQueue) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let buffer_size = (std::mem::size_of::<Vertex>() * vertices.len()) as vk::DeviceSize;

    let (vertex_buffer, vertex_buffer_memory) = buffer::create_buffer(
        instance,
        device,
        physical_device,
        buffer_size,
        vk::BufferUsageFlags::VERTEX_BUFFER,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
    )?;

    unsafe {
        memory::map_and_copy(device, vertex_buffer_memory, vertices)?;
    }

    let device_clone = device.clone();
    deletion_queue.push(move || unsafe {
        device_clone.destroy_buffer(vertex_buffer, None);
        device_clone.free_memory(vertex_buffer_memory, None);
    });

    Ok((vertex_buffer, vertex_buffer_memory))
}

unsafe fn create_descriptor_resources(logical_device: &ash::Device, instance: &ash::Instance, physical_device: vk::PhysicalDevice, deletion_queue: &mut DeletionQueue)
-> Result<(DescriptorSetLayout, DescriptorPool, Vec<DescriptorSet>,Vec<(vk::Buffer, vk::DeviceMemory)>)>{
    let descriptor_layout_builder = descriptor::DescriptorLayoutBuilder::new()
        .add_binding(
             0,
             vk::DescriptorType::UNIFORM_BUFFER,
             vk::ShaderStageFlags::VERTEX);
    let descriptor_set_layout = descriptor_layout_builder.build(&logical_device)?;
    let descriptor_pool = descriptor::create_descriptor_pool(&logical_device, MAX_FRAMES_IN_FLIGHT as u32)?;
    let descriptor_sets = descriptor::allocate_descriptor_sets(&logical_device, descriptor_pool, descriptor_set_layout, MAX_FRAMES_IN_FLIGHT as u32)?;
    
    //bufers
    let mut buffers = Vec::new();
    let ubo_size = std::mem::size_of::<UniformBufferObject>() as vk::DeviceSize;
    for i in 0..MAX_FRAMES_IN_FLIGHT{
        let (buf, mem) = buffer::create_buffer(
            instance,
            logical_device, 
            physical_device, 
            ubo_size, 
            vk::BufferUsageFlags::UNIFORM_BUFFER, 
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT)?;
        buffers.push((buf, mem));

        descriptor::update_buffer_descriptor_set(
            logical_device, 
            descriptor_sets[i], 
            buffers[i].0, 
            vk::DescriptorType::UNIFORM_BUFFER,
            0
            );
        let logical_device_clone = logical_device.clone();
        deletion_queue.push(move || unsafe {
            logical_device_clone.destroy_buffer(buf, None);
            logical_device_clone.free_memory(mem, None);
        });
    }
    let logical_device_clone = logical_device.clone();
    deletion_queue.push(move || unsafe{
        logical_device_clone.destroy_descriptor_pool(descriptor_pool, None);
        logical_device_clone.destroy_descriptor_set_layout(descriptor_set_layout, None);
    }); 


    return Ok((descriptor_set_layout, descriptor_pool, descriptor_sets, buffers));
}