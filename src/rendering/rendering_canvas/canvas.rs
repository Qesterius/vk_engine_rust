use std::mem::ManuallyDrop;
use std::sync::Arc;

use ash::vk;
use anyhow::{Result, anyhow};
use log::info;

use crate::device::device::Device;
use super::{depth_buffer::DepthBuffer, swapchain::SwapchainState};

pub struct Canvas {
    device: Arc<Device>,
    instance: ash::Instance,
    pub surface_loader: ash::khr::surface::Instance,
    pub surface: vk::SurfaceKHR,
    // ManuallyDrop so we control destruction order in Drop:
    // swapchain and depth_buffer must die before surface.
    pub swapchain: ManuallyDrop<SwapchainState>,
    pub depth_buffer: ManuallyDrop<DepthBuffer>,
    pub framebuffers: Vec<vk::Framebuffer>,
    pub render_pass: vk::RenderPass,
}

impl Canvas {
    pub unsafe fn new(
        device: Arc<Device>,
        instance: ash::Instance,
        surface: vk::SurfaceKHR,
        surface_loader: ash::khr::surface::Instance,
        window_size: winit::dpi::PhysicalSize<u32>,
    ) -> Result<Self> {
        let swapchain = unsafe {
            SwapchainState::new(&device, &instance, surface, &surface_loader, window_size)
        }?;

        let render_pass = unsafe { create_render_pass(swapchain.format, &device.logical_device) }?;

        let depth_buffer = DepthBuffer::new(Arc::clone(&device), swapchain.extent)?;

        let framebuffers = unsafe {
            create_framebuffers(&device.logical_device, render_pass, &swapchain, depth_buffer.depth_view)
        }?;

        Ok(Self {
            device,
            instance,
            surface_loader,
            surface,
            swapchain: ManuallyDrop::new(swapchain),
            depth_buffer: ManuallyDrop::new(depth_buffer),
            framebuffers,
            render_pass,
        })
    }

    pub unsafe fn handle_resize(&mut self, window_size: winit::dpi::PhysicalSize<u32>) -> Result<()> {
        // Skip if dimensions haven't changed (Wayland fires Resized on initial window creation)
        // Also skip minimized windows (size 0) which would crash swapchain creation
        if window_size.width == 0 || window_size.height == 0 {
            return Ok(());
        }
        if self.swapchain.extent.width == window_size.width
            && self.swapchain.extent.height == window_size.height
        {
            return Ok(());
        }

        unsafe { self.device.logical_device.device_wait_idle() }?;

        for &fb in &self.framebuffers {
            unsafe { self.device.logical_device.destroy_framebuffer(fb, None) };
        }
        self.framebuffers.clear();

        // Drop old swapchain/depth before creating new ones
        unsafe { ManuallyDrop::drop(&mut self.depth_buffer) };
        unsafe { ManuallyDrop::drop(&mut self.swapchain) };

        let new_swapchain = unsafe {
            SwapchainState::new(&self.device, &self.instance, self.surface, &self.surface_loader, window_size)
        }?;
        let new_depth = DepthBuffer::new(Arc::clone(&self.device), new_swapchain.extent)?;
        let new_framebuffers = unsafe {
            create_framebuffers(
                &self.device.logical_device,
                self.render_pass,
                &new_swapchain,
                new_depth.depth_view,
            )
        }?;

        self.swapchain = ManuallyDrop::new(new_swapchain);
        self.depth_buffer = ManuallyDrop::new(new_depth);
        self.framebuffers = new_framebuffers;

        Ok(())
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            for &fb in &self.framebuffers {
                self.device.logical_device.destroy_framebuffer(fb, None);
            }
            self.device.logical_device.destroy_render_pass(self.render_pass, None);

            // Explicit order: depth and swapchain must die before surface
            ManuallyDrop::drop(&mut self.depth_buffer);
            ManuallyDrop::drop(&mut self.swapchain);

            self.surface_loader.destroy_surface(self.surface, None);
            info!("Canvas destroyed.");
        }
    }
}

unsafe fn create_render_pass(format: vk::Format, device: &ash::Device) -> Result<vk::RenderPass> {
    let color_attachment = vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::PRESENT_SRC_KHR);

    let color_attachment_ref = vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);

    let depth_attachment = vk::AttachmentDescription::default()
        .format(vk::Format::D32_SFLOAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::DONT_CARE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let depth_attachment_ref = vk::AttachmentReference::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);

    let subpass = vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(std::slice::from_ref(&color_attachment_ref))
        .depth_stencil_attachment(&depth_attachment_ref);

    let attachments = [color_attachment, depth_attachment];
    let render_pass_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(std::slice::from_ref(&subpass));

    unsafe {
        device
            .create_render_pass(&render_pass_info, None)
            .map_err(|e| anyhow!("Failed to create render pass: {}", e))
    }
}

unsafe fn create_framebuffers(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    swapchain: &SwapchainState,
    depth_view: vk::ImageView,
) -> Result<Vec<vk::Framebuffer>> {
    let mut framebuffers = Vec::with_capacity(swapchain.image_views.len());
    for &image_view in &swapchain.image_views {
        let attachments = [image_view, depth_view];
        let fb_info = vk::FramebufferCreateInfo::default()
            .render_pass(render_pass)
            .attachments(&attachments)
            .width(swapchain.extent.width)
            .height(swapchain.extent.height)
            .layers(1);
        let fb = unsafe { device.create_framebuffer(&fb_info, None) }?;
        framebuffers.push(fb);
    }
    Ok(framebuffers)
}
