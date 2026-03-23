use crate::config::{MAX_FRAMES_IN_FLIGHT, VALIDATION_ENABLED};
use crate::rendering::memory::find_memory_type;
use crate::rendering::render_target;
use crate::rendering::vertex::{MeshPushConstants, UniformBufferObject};
use super::buffer;
use super::memory;
use super::vertex::Vertex;
use crate::utils::vk_to_cstr;
use super::cleanup::DeletionQueue;
use crate::utils;
use core::fmt;
use super::pipeline::PipelineBuilder;
use super::descriptor;
use super::render_target::render_target::RenderTarget;

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
    pub(crate) descriptor_set_layout: vk::DescriptorSetLayout,
    pub(crate) command_pool: vk::CommandPool,
    pub(crate) render_pass: vk::RenderPass,
    pub(crate) graphics_pipeline: vk::Pipeline,
    pub(crate) pipeline_layout: vk::PipelineLayout,
    pub(crate) descriptor_pool: vk::DescriptorPool,
    pub(crate) phys_memory_properties : vk::PhysicalDeviceMemoryProperties
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
        
        //mem
        let physical_memory_properties =  unsafe { instance.get_physical_device_memory_properties(physical_device) };

        //render pass
        let swapchain_details = (unsafe {
            render_target::swapchain::get_swapchain_details(physical_device, surface, &surface_loader)
        })?; 
        let surface_format = render_target::swapchain::get_swapchain_surface_format(&swapchain_details.formats);

        let render_pass = (unsafe {
            create_render_pass(surface_format.format, &logical_device, deletion_queue)
        })?;
        
        //command pool/buffer
        let command_pool = setup_command_pool(&logical_device, q_family_indices, deletion_queue)?;
             
        let (descriptor_set_layout,
            descriptor_pool) = setup_descriptor_pool_and_layout(
                                                                        &logical_device,
                                                                        deletion_queue
                                                                    ) ?;
        //depth buffer
        let descriptor_set_layouts = vec![descriptor_set_layout];
        let (graphics_pipeline, pipeline_layout) = create_triangle_pipeline(&logical_device, render_pass, descriptor_set_layouts, deletion_queue)?;
        //return
        let context = Self {
            surface: surface,
            surface_loader: surface_loader,
            physical_device: physical_device,
            graphics_queue: graphics_queue,
            present_queue: present_queue,
            command_pool: command_pool,
            render_pass: render_pass,
            graphics_pipeline: graphics_pipeline,
            pipeline_layout: pipeline_layout,
            debug_utils: debug_utils,
            descriptor_pool: descriptor_pool,
            descriptor_set_layout : descriptor_set_layout,
            phys_memory_properties : physical_memory_properties
        };

        Ok((context, logical_device))
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
    let push_constant_ranges = vec![
            vk::PushConstantRange::default()
                .size(std::mem::size_of::<MeshPushConstants>() as u32)
                .stage_flags(vk::ShaderStageFlags::VERTEX)
                .offset(0)
        ];

    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&descriptor_set_layouts)
        .push_constant_ranges(&push_constant_ranges);
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
        render_target::swapchain::SwapchainSupportDetails::get(physical_device, surface, surface_loader)?
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


fn setup_command_pool(logical_device: &ash::Device, q_family_indices: QueueFamilyIndices, deletion_queue: &mut DeletionQueue) -> Result<vk::CommandPool> {
     let pool_info = vk::CommandPoolCreateInfo
            ::default()
            .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
            .queue_family_index(q_family_indices.graphics);
    let command_pool = (unsafe { logical_device.create_command_pool(&pool_info, None) })?;

    let logical_device_clone = logical_device.clone();
    deletion_queue.push(move || unsafe {
        logical_device_clone.destroy_command_pool(command_pool, None);
    });
    Ok(command_pool)
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
    //color attachment
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
    //depth attachment
    let depth_attachment_description = vk::AttachmentDescription
        ::default()
        .format(vk::Format::D32_SFLOAT) //match depth image
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR) //what means far? i assume its clearing image memory when its loaded?
        .store_op(vk::AttachmentStoreOp::DONT_CARE) // whats this then
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED) //why not depth?
        .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL); // whats stencil?? 
    let depth_attachment_ref = vk::AttachmentReference
        ::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    //subpass
    let subpass_description = vk::SubpassDescription
        ::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(std::slice::from_ref(&color_attachment_ref))
        .depth_stencil_attachment(&depth_attachment_ref);
    let attachments = [color_attachment_description, depth_attachment_description];
    //render_pass
    let render_pass_info = vk::RenderPassCreateInfo
        ::default()
        .attachments(&attachments)
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
pub unsafe fn create_vertex_buffer(phys_mem_props: &vk::PhysicalDeviceMemoryProperties, device: &ash::Device, vertices: &[Vertex], deletion_queue: &mut DeletionQueue) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let buffer_size = (std::mem::size_of::<Vertex>() * vertices.len()) as vk::DeviceSize;

    let (vertex_buffer, vertex_buffer_memory) = buffer::create_buffer(
        device,
        phys_mem_props,
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

//indices buffer
pub unsafe fn create_indices_buffer(phys_mem_props: &vk::PhysicalDeviceMemoryProperties, device: &ash::Device, indices: &[u32], deletion_queue: &mut DeletionQueue) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let buffer_size = (std::mem::size_of::<u32>() * indices.len()) as vk::DeviceSize;

    let (indice_buffer, indice_buffer_memory) = buffer::create_buffer(
        device,
        phys_mem_props,
        buffer_size,
        vk::BufferUsageFlags::INDEX_BUFFER,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT
    )?;

    unsafe {
        memory::map_and_copy(device, indice_buffer_memory, indices)?;
    }

    let device_clone = device.clone();
    deletion_queue.push(move || unsafe {
        device_clone.destroy_buffer(indice_buffer, None);
        device_clone.free_memory(indice_buffer_memory, None);
    });

    Ok((indice_buffer, indice_buffer_memory))
}

fn setup_descriptor_pool_and_layout(
    logical_device: &ash::Device, 
    deletion_queue: &mut DeletionQueue
) -> Result<(vk::DescriptorSetLayout, vk::DescriptorPool)> {
    // 1. Layout (The Blueprint)
    let descriptor_set_layout = descriptor::DescriptorLayoutBuilder::new()
        .add_binding(0, vk::DescriptorType::UNIFORM_BUFFER, vk::ShaderStageFlags::VERTEX)
        .build(logical_device)?;

    // 2. Pool (The Factory)
    let descriptor_pool = descriptor::create_descriptor_pool(
        logical_device, 
        MAX_FRAMES_IN_FLIGHT as u32
    )?;

    // Push to DeletionQueue because these are permanent
    let ld_clone = logical_device.clone();
    deletion_queue.push(move || unsafe {
        ld_clone.destroy_descriptor_pool(descriptor_pool, None);
        ld_clone.destroy_descriptor_set_layout(descriptor_set_layout, None);
    });

    Ok((descriptor_set_layout, descriptor_pool))
}