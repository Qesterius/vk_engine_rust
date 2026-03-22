use ash::{khr::{surface, swapchain}, vk::{self, PhysicalDevice}};
use anyhow::{Result, Ok};
use crate::rendering::render_target::{depth_buffer, swapchain::{SwapchainState, SwapchainSupportDetails}};

use super::depth_buffer::DepthBuffer;

#[derive(Clone)]
pub struct RenderTarget{
    pub swapchain : SwapchainState,
    pub depth_buffer: DepthBuffer,
    pub framebuffers: Vec<vk::Framebuffer>, // Links swapchain views + depth view
    pub device : ash::Device
}

impl RenderTarget{
    pub unsafe fn new(
        logical_device: &ash::Device,
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
        render_pass: vk::RenderPass,
        window_size: winit::dpi::PhysicalSize<u32>,
        phys_dev_mem_prop : &vk::PhysicalDeviceMemoryProperties
     ) -> Result<Self>{
        //swapchain
        let swapchain_support_details = unsafe { SwapchainSupportDetails::get(physical_device, surface, surface_loader) }?;
        let swapchain = unsafe { SwapchainState::new(swapchain_support_details, instance, physical_device, logical_device, surface, surface_loader, window_size) }?;
        
        //depth buffer
        let extent = swapchain.extent;
        let depth_buffer = DepthBuffer::new(logical_device, phys_dev_mem_prop, extent)?;

        //frame buffer
        let frame_buffers = unsafe { create_framebuffers(
            logical_device,
            render_pass,
            &swapchain,
            depth_buffer.depth_view) }?;
                
        Ok(Self{
            depth_buffer: depth_buffer,
            swapchain:swapchain,
            framebuffers: frame_buffers,
            device:logical_device.clone()
            })
    }
    pub unsafe fn handle_resize(&mut self) -> Result<()>{Ok(())}
}
impl Drop for RenderTarget{
    fn drop(&mut self){
        unsafe{
            //framebuffers
            for &fb in &self.framebuffers{
                self.device.destroy_framebuffer(fb, None);
            }
        }
    }
}

pub unsafe fn create_framebuffers(
    logical_device : &ash::Device, 
    render_pass : vk::RenderPass,
    swapchain: &SwapchainState,
    depth_view: vk::ImageView
) -> Result<Vec<vk::Framebuffer>>{
     let mut framebuffers = Vec::with_capacity(swapchain.image_views.len());
        for &image_view in &swapchain.image_views {
            let attachments = [image_view, depth_view]; //match renderpass order
            let framebuffer_info = vk::FramebufferCreateInfo
                ::default()
                .render_pass(render_pass)
                .attachments(&attachments)
                .width(swapchain.extent.width)
                .height(swapchain.extent.height)
                .layers(1);
            let framebuffer = (unsafe { logical_device.create_framebuffer(&framebuffer_info, None) })?;
            framebuffers.push(framebuffer);
        }
    Ok(framebuffers)
}

