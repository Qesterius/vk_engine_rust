use std::sync::Arc;

use ash::vk;
use anyhow::{Result, Ok};

use crate::device::device::Device;
use crate::rendering::memory::find_memory_type;

pub struct DepthBuffer {
    pub depth_image: vk::Image,
    pub depth_memory: vk::DeviceMemory,
    pub depth_view: vk::ImageView,
    device: Arc<Device>,
}

impl DepthBuffer {
    pub fn new(
        device: Arc<Device>,
        extent: vk::Extent2D,
    ) -> Result<Self> {
        let depth_format = vk::Format::D32_SFLOAT;

        // image
        let image_info = vk::ImageCreateInfo::default()
            .extent(extent.into())
            .image_type(vk::ImageType::TYPE_2D)
            .mip_levels(1) // compressed lower res versions of image. We dont need it here, so 1 is enough
            .array_layers(1) // more would be here for example for skybox with 6 faces. this could be stored as just 6 arrays
            .format(depth_format)
            .tiling(vk::ImageTiling::OPTIMAL) // memory arrangement. LINEAR - good for CPU to read, sequential. OPTIMAL - good for GPU to read.
            .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
            .samples(vk::SampleCountFlags::TYPE_1); // MSAA - every pixel with info abt neighbors

        let image = unsafe { device.logical_device.create_image(&image_info, None)? };

        // memory
        let mem_reqs = unsafe { device.logical_device.get_image_memory_requirements(image) };
        let memory_type = find_memory_type(
            &device.memory_properties,
            mem_reqs.memory_type_bits,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        )?;
        let alloc_info = vk::MemoryAllocateInfo::default()
            .allocation_size(mem_reqs.size)
            .memory_type_index(memory_type);
        let memory = unsafe { device.logical_device.allocate_memory(&alloc_info, None) }?;
        unsafe { device.logical_device.bind_image_memory(image, memory, 0) }?;

        // image view
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(depth_format)
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::DEPTH,
                level_count: 1,
                layer_count: 1,
                ..Default::default()
            });
        let view = unsafe { device.logical_device.create_image_view(&view_info, None)? };

        Ok(Self {
            depth_image: image,
            depth_memory: memory,
            depth_view: view,
            device,
        })
    }
}

impl Drop for DepthBuffer {
    fn drop(&mut self) {
        unsafe {
            self.device.logical_device.destroy_image_view(self.depth_view, None);
            self.device.logical_device.destroy_image(self.depth_image, None);
            self.device.logical_device.free_memory(self.depth_memory, None);
        }
    }
}
