use ash::vk;
use bevy_ecs::component::Component;
use std::sync::Arc;

use crate::device::device::Device;

#[derive(Component, Clone)]
pub struct TextureHandle(pub Arc<Texture>);

impl std::ops::Deref for TextureHandle {
    type Target = Texture;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Texture {
    pub image: vk::Image,
    pub image_view: vk::ImageView,
    pub memory: vk::DeviceMemory,
    /// This texture's own descriptor set: its image view and sampler, bound per draw at set 1.
    pub descriptor_set: vk::DescriptorSet,
    #[allow(dead_code)]
    pub path: String,
}

impl Texture {
    pub fn new(
        image: vk::Image,
        image_view: vk::ImageView,
        memory: vk::DeviceMemory,
        descriptor_set: vk::DescriptorSet,
        path: String,
    ) -> Self {
        Self {
            image,
            image_view,
            memory,
            descriptor_set,
            path,
        }
    }

    pub fn destroy(&self, device: &Device) {
        unsafe {
            device
                .logical_device
                .destroy_image_view(self.image_view, None);
            device.logical_device.destroy_image(self.image, None);
            device.logical_device.free_memory(self.memory, None);
        }
    }
}
