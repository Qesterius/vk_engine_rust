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
use super::render_target::render_target::Canvas;

use anyhow::anyhow;
use anyhow::{ Ok, Result };
use ash::vk::{DescriptorPool, DescriptorSet, DescriptorSetLayout};
use ash::vk::{ImageView, LogicOp};
use ash::{ Instance, khr::surface, vk::{ self, PhysicalDevice } };
use cgmath::num_traits::ToPrimitive;
use raw_window_handle::{ HasDisplayHandle, HasWindowHandle };
use std::ffi::{ CStr, c_void };
use winit::window::Window;

//Required Device extensions

#[derive(Clone)]
pub(crate) struct VulkanContext {
    pub(crate) surface: vk::SurfaceKHR,
//    pub(crate) debug_utils : Option<(ash::ext::debug_utils::Instance, vk::DebugUtilsMessengerEXT)>,
    pub(crate) surface_loader: ash::khr::surface::Instance,
    pub(crate) physical_device: vk::PhysicalDevice,
//    pub(crate) graphics_queue: vk::Queue,
//    pub(crate) present_queue: vk::Queue,
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



        //devices
        
        //render pass
        let swapchain_details = (unsafe {
            render_target::swapchain::get_swapchain_details(physical_device, surface, &surface_loader)
        })?; 
        let surface_format = render_target::swapchain::get_swapchain_surface_format(&swapchain_details.formats);

        let render_pass = (unsafe {
            create_render_pass(surface_format.format, &logical_device, deletion_queue)
        })?;
        
                 
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