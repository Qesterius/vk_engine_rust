use std::sync::Arc;
use ash::vk;
use anyhow::Result;

use crate::device::device::Device;


#[derive(Clone)]
pub struct SwapchainState {
    pub swapchain: vk::SwapchainKHR,
    pub loader: ash::khr::swapchain::Device,
    pub images: Vec<vk::Image>,
    pub image_views: Vec<vk::ImageView>,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    device: Arc<Device>,
}

impl Drop for SwapchainState {
    fn drop(&mut self) {
        unsafe {
            for &view in &self.image_views {
                self.device.logical_device.destroy_image_view(view, None);
            }
            self.loader.destroy_swapchain(self.swapchain, None);
        }
    }
}

impl SwapchainState {
    pub unsafe fn new(
        device: &Arc<Device>,
        instance: &ash::Instance,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
        size: winit::dpi::PhysicalSize<u32>,
    ) -> Result<Self> {
        let support = unsafe { SwapchainSupportDetails::get(device.physical_device, surface, surface_loader) }?;

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

        let indices = device.queue_family_indices;
        let mut queue_family_indices_arr = vec![];
        let image_sharing_mode = if indices.graphics != indices.present {
            queue_family_indices_arr.push(indices.graphics);
            queue_family_indices_arr.push(indices.present);
            vk::SharingMode::CONCURRENT
        } else {
            vk::SharingMode::EXCLUSIVE
        };

        let info = vk::SwapchainCreateInfoKHR::default()
            .surface(surface)
            .min_image_count(image_count)
            .image_format(surface_format.format)
            .image_color_space(surface_format.color_space)
            .image_extent(extent)
            .image_array_layers(1)
            .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST)
            .image_sharing_mode(image_sharing_mode)
            .queue_family_indices(&queue_family_indices_arr)
            .pre_transform(support.capabilites.current_transform)
            .composite_alpha(vk::CompositeAlphaFlagsKHR::OPAQUE)
            .present_mode(present_mode)
            .clipped(true)
            .old_swapchain(vk::SwapchainKHR::null());

        let swapchain_loader = ash::khr::swapchain::Device::new(instance, &device.logical_device);
        let swapchain = unsafe { swapchain_loader.create_swapchain(&info, None) }?;

        let swapchain_images = unsafe { swapchain_loader.get_swapchain_images(swapchain) }?;
        let mut swapchain_image_views = Vec::with_capacity(swapchain_images.len());

        for image in &swapchain_images {
            let create_view_info = vk::ImageViewCreateInfo::default()
                .image(*image)
                .view_type(vk::ImageViewType::TYPE_2D)
                .format(surface_format.format)
                .components(
                    vk::ComponentMapping::default()
                        .r(vk::ComponentSwizzle::IDENTITY)
                        .g(vk::ComponentSwizzle::IDENTITY)
                        .b(vk::ComponentSwizzle::IDENTITY)
                        .a(vk::ComponentSwizzle::IDENTITY),
                )
                .subresource_range(
                    vk::ImageSubresourceRange::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .base_mip_level(0)
                        .level_count(1)
                        .base_array_layer(0)
                        .layer_count(1),
                );
            let image_view = unsafe { device.logical_device.create_image_view(&create_view_info, None) }?;
            swapchain_image_views.push(image_view);
        }

        Ok(Self {
            swapchain,
            loader: swapchain_loader,
            images: swapchain_images,
            image_views: swapchain_image_views,
            device: device.clone(),
            extent,
            format: surface_format.format,
        })
    }
}

pub(crate) fn get_swapchain_surface_format(formats: &[vk::SurfaceFormatKHR]) -> vk::SurfaceFormatKHR {
    formats
        .iter()
        .cloned()
        .find(|f| f.format == vk::Format::B8G8R8A8_SRGB && f.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR)
        .unwrap_or_else(|| formats[0])
}

fn get_swapchain_present_mode(present_modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    present_modes
        .iter()
        .cloned()
        .find(|p| *p == vk::PresentModeKHR::MAILBOX)
        .unwrap_or(vk::PresentModeKHR::FIFO)
}

pub(crate) struct SwapchainSupportDetails {
    pub(crate) capabilites: vk::SurfaceCapabilitiesKHR,
    pub(crate) formats: Vec<vk::SurfaceFormatKHR>,
    pub(crate) present_modes: Vec<vk::PresentModeKHR>,
}

impl SwapchainSupportDetails {
    pub unsafe fn get(
        physical_device: vk::PhysicalDevice,
        surface: vk::SurfaceKHR,
        surface_loader: &ash::khr::surface::Instance,
    ) -> Result<Self> {
        Ok(Self {
            capabilites: unsafe {
                surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
            },
            formats: unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface)? },
            present_modes: unsafe {
                surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
            },
        })
    }
}
