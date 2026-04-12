use std::sync::Arc;
use ash::vk;
use bevy_ecs::component::Component;

use crate::device::device::Device;

#[derive(Component, Clone)]
pub struct TextureHandle(pub Arc<Texture>);

impl std::ops::Deref for TextureHandle {
    type Target = Texture;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

pub struct Texture{
    pub image : vk::Image,
    pub image_view : vk::ImageView,
    pub memory : vk::DeviceMemory,
    pub descriptor_set : vk::DescriptorSet,
    pub path: String,
}
impl Texture{
    pub fn destroy(&self, device: &Device){
        unsafe{
            device.logical_device.destroy_image(self.image, None);
            device.logical_device.destroy_image_view(self.image_view, None);
            device.logical_device.free_memory(self.memory, None);
        }
    }
    pub fn new(device: &Device, image: vk::Image, image_view: vk::ImageView, memory: vk::DeviceMemory, descriptor_set: vk::DescriptorSet, path: String) -> Self{
        Self{
            image,
            image_view,
            memory,
            descriptor_set,
            path
        }
    }
}