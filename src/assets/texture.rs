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
    pub index: u32,
    pub sampler_index: u32,
    #[allow(dead_code)]
    pub path: String,
}

impl Texture {
    pub fn new(
        image: vk::Image,
        image_view: vk::ImageView,
        memory: vk::DeviceMemory,
        index: u32,
        sampler_index: u32,
        path: String,
    ) -> Self {
        Self {
            image,
            image_view,
            memory,
            index,
            sampler_index,
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
