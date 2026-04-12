use std::{ collections::HashMap, sync::Arc };
use ash::vk;
use anyhow::{ Result, Ok };
use bevy_ecs::resource::Resource;

use crate::{
    assets::{ loaders, texture::Texture },
    device::device::Device,
    rendering::{ buffer, descriptor::{ self, DescriptorLayoutBuilder }, memory, image },
};
#[derive(Hash, Eq, PartialEq)]
pub enum SamplerType {
    SAMPLER_LINEAR_REPEAT,
    //SAMPLER_NEAREST_REPEAT,
    //SAMPLER_LINEAR_CLAMP,
    //SAMPLER_NEAREST_CLAMP
}
#[derive(Resource)]
pub struct AssetManager {
    device: Arc<Device>,
    pub texture_descriptor_set_layout: vk::DescriptorSetLayout,
    pub texture_descriptor_pool: vk::DescriptorPool,
    pub samplers: HashMap<SamplerType, vk::Sampler>,
    pub loaded_textures: HashMap<String, Arc<Texture>>,
}

impl Drop for AssetManager {
    fn drop(&mut self) {
        for texture in self.loaded_textures.values() {
            texture.destroy(&self.device);
        }
        unsafe {
            for sampler in self.samplers.values() {
                self.device.logical_device.destroy_sampler(*sampler, None);
            }
            self.device.logical_device.destroy_descriptor_pool(self.texture_descriptor_pool, None);
            self.device.logical_device.destroy_descriptor_set_layout(self.texture_descriptor_set_layout, None);
        }
    }
}

impl AssetManager {
    pub fn new(device: Arc<Device>) -> Result<Self> {
        let texture_descriptor_layout = DescriptorLayoutBuilder::new()
            .add_binding(
                0,
                vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                vk::ShaderStageFlags::FRAGMENT
            )
            .build(&device.logical_device)?;

        let pool_sizes = [
            vk::DescriptorPoolSize
                ::default()
                .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
                .descriptor_count(100),
        ];

        let descriptor_pool = descriptor::create_descriptor_pool(
            &device.logical_device,
            100,
            &pool_sizes
        )?;

        let linear_sampler = crate::rendering::image::create_texture_sampler(&device)?;
        let mut samplers = HashMap::new();
        samplers.insert(SamplerType::SAMPLER_LINEAR_REPEAT, linear_sampler);

        Ok(Self {
            device: device,
            texture_descriptor_set_layout: texture_descriptor_layout,
            texture_descriptor_pool: descriptor_pool,
            samplers: samplers,
            loaded_textures: HashMap::new(),
        })
    }

    pub fn load_texture(&mut self, path: &str) -> Result<Arc<Texture>> {
        if let Some(texture) = self.loaded_textures.get(path) {
            return Ok(Arc::clone(texture));
        }

        let img = loaders::image_loader::load(path)?;
        let img_data = img.as_raw();
        let sampler = self.samplers.get(&SamplerType::SAMPLER_LINEAR_REPEAT).unwrap();

        //loading image into cpu ram
        let (staging_buffer, staging_memory) = buffer::create_buffer(
            &self.device.logical_device,
            &self.device.memory_properties,
            img_data.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE
        )?;

        // put image pixels into staging buffer
        (unsafe { memory::map_and_copy(&self.device.logical_device, staging_memory, img_data) })?;
        let extent = vk::Extent3D {
            width: img.width(),
            height: img.height(),
            depth: 1,
        };
        // create empty image on gpu
        let (img_vk, mem_img_vk) = image::create_image(&self.device, extent, self.device.format_color)?;

        // transition: using pipeline barrier to change image layout to be optimal for receiving data from buffer
        image::transition_image_layout(
            &self.device,
            img_vk,
            self.device.format_color,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL
        )?;

        //move staging buffer data to image
        image::copy_buffer_to_image(&self.device, staging_buffer, img_vk, extent)?;

        //transition image layout to be sampled in shader after its loaded
        image::transition_image_layout(
            &self.device,
            img_vk,
            self.device.format_color,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
        )?;
        
        // create img view
        let img_vk_view = image::create_image_view(&self.device, img_vk, self.device.format_color)?;

        // allocate descriptor set for this texture
        let descriptor_set = descriptor::allocate_descriptor_sets(
            &self.device.logical_device,
            self.texture_descriptor_pool,
            self.texture_descriptor_set_layout,
            1
        )?[0];

        // put image and sampler into descriptor set
        descriptor::update_image_descriptor_set(
            &self.device.logical_device,
            descriptor_set,
            img_vk_view,
            *sampler,
            0
        )?;

        unsafe {
            self.device.logical_device.destroy_buffer(staging_buffer, None);
        }
        unsafe {
            self.device.logical_device.free_memory(staging_memory, None);
        }

        let texture = Texture::new(
            &self.device,
            img_vk,
            img_vk_view,
            mem_img_vk,
            descriptor_set,
            path.to_string()
        );

        let texture_arc = Arc::new(texture);
        self.loaded_textures.insert(path.to_string(), Arc::clone(&texture_arc));
        Ok(texture_arc)
    }
}
