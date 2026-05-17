use anyhow::{Ok, Result};
use ash::vk;
use bevy_ecs::resource::Resource;
use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
};

use crate::{
    assets::{loaders, texture::Texture},
    device::device::Device,
    rendering::{
        buffer,
        descriptor::{self, DescriptorLayoutBuilder},
        image, memory,
    },
};

/// Maximum number of textures loaded at once; one descriptor set each.
const MAX_TEXTURES: u32 = 100;

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
#[allow(dead_code)]
pub enum SamplerType {
    LinearRepeat,
    LinearClamp,
    NearestClamp,
}

impl SamplerType {
    pub fn index(self) -> usize {
        match self {
            SamplerType::LinearRepeat => 0,
            SamplerType::LinearClamp => 1,
            SamplerType::NearestClamp => 2,
        }
    }
}

#[derive(Resource)]
pub struct AssetManager {
    device: Arc<Device>,
    pub texture_descriptor_set_layout: vk::DescriptorSetLayout,
    pub texture_descriptor_pool: vk::DescriptorPool,
    samplers: Vec<vk::Sampler>,
    pub loaded_textures: HashMap<String, Arc<Texture>>,
}

impl Drop for AssetManager {
    fn drop(&mut self) {
        for texture in self.loaded_textures.values() {
            texture.destroy(&self.device);
        }
        unsafe {
            for &sampler in &self.samplers {
                self.device.logical_device.destroy_sampler(sampler, None);
            }
            // Destroying the pool implicitly frees every texture's descriptor set
            self.device
                .logical_device
                .destroy_descriptor_pool(self.texture_descriptor_pool, None);
            self.device
                .logical_device
                .destroy_descriptor_set_layout(self.texture_descriptor_set_layout, None);
        }
    }
}

impl AssetManager {
    pub fn new(device: Arc<Device>) -> Result<Self> {
        // One COMBINED_IMAGE_SAMPLER per texture set, bound per draw at set 1
        let layout = DescriptorLayoutBuilder::new()
            .add_binding(
                0,
                vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                vk::ShaderStageFlags::FRAGMENT,
            )
            .build(&device.logical_device)?;

        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(MAX_TEXTURES)];
        // FREE_DESCRIPTOR_SET: texture sets live until their texture is unloaded
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .flags(vk::DescriptorPoolCreateFlags::FREE_DESCRIPTOR_SET)
            .max_sets(MAX_TEXTURES)
            .pool_sizes(&pool_sizes);
        let pool = unsafe { device.logical_device.create_descriptor_pool(&pool_info, None) }?;

        // Built in SamplerType::index() order
        let samplers = vec![
            image::create_texture_sampler(&device)?,       // 0: LinearRepeat
            image::create_sampler_linear_clamp(&device)?,  // 1: LinearClamp
            image::create_sampler_nearest_clamp(&device)?, // 2: NearestClamp
        ];

        Ok(Self {
            device,
            texture_descriptor_set_layout: layout,
            texture_descriptor_pool: pool,
            samplers,
            loaded_textures: HashMap::new(),
        })
    }

    pub fn load_texture(&mut self, path: &str, sampler_type: SamplerType) -> Result<Arc<Texture>> {
        if let Some(texture) = self.loaded_textures.get(path) {
            return Ok(Arc::clone(texture));
        }

        let img = loaders::image_loader::load(path)?;
        let img_data = img.as_raw();

        let (staging_buffer, staging_memory) = buffer::create_buffer(
            &self.device.logical_device,
            &self.device.memory_properties,
            img_data.len() as vk::DeviceSize,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
        )?;
        unsafe { memory::map_and_copy(&self.device.logical_device, staging_memory, img_data) }?;

        let extent = vk::Extent3D {
            width: img.width(),
            height: img.height(),
            depth: 1,
        };
        let (img_vk, mem_img_vk) =
            image::create_image(&self.device, extent, self.device.format_color)?;

        image::transition_image_layout(
            &self.device,
            img_vk,
            self.device.format_color,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        )?;
        image::copy_buffer_to_image(&self.device, staging_buffer, img_vk, extent)?;
        image::transition_image_layout(
            &self.device,
            img_vk,
            self.device.format_color,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )?;

        let img_vk_view =
            image::create_image_view(&self.device, img_vk, self.device.format_color)?;

        // The texture's own set, written once here; no frame uses a new set yet
        let descriptor_set = descriptor::allocate_descriptor_sets(
            &self.device.logical_device,
            self.texture_descriptor_pool,
            self.texture_descriptor_set_layout,
            1,
        )?[0];
        descriptor::update_image_descriptor_set(
            &self.device.logical_device,
            descriptor_set,
            img_vk_view,
            self.samplers[sampler_type.index()],
            0,
        );

        unsafe {
            self.device
                .logical_device
                .destroy_buffer(staging_buffer, None);
            self.device.logical_device.free_memory(staging_memory, None);
        }

        let texture = Texture::new(
            img_vk,
            img_vk_view,
            mem_img_vk,
            descriptor_set,
            path.to_string(),
        );

        let texture_arc = Arc::new(texture);
        self.loaded_textures
            .insert(path.to_string(), Arc::clone(&texture_arc));
        Ok(texture_arc)
    }

    /// Unloads a texture. Frames still in flight may have its descriptor set bound, so the
    /// set and the GPU resources (view, image, memory) are freed only once the newest
    /// submitted frame, the last one that can use them, has finished.
    #[allow(dead_code)]
    pub fn unload_texture(&mut self, path: &str) {
        let Some(texture) = self.loaded_textures.remove(path) else {
            return;
        };

        let device = Arc::clone(&self.device);
        let pool = self.texture_descriptor_pool;
        let descriptor_set = texture.descriptor_set;
        let image = texture.image;
        let image_view = texture.image_view;
        let memory = texture.memory;
        // The newest submitted frame's queue is flushed only after its fence is waited on
        let queue_idx = self.device.current_sync_idx.load(Ordering::Relaxed);
        self.device.dynamic_deletion_queues[queue_idx]
            .lock()
            .unwrap()
            .push(move || unsafe {
                // The pool has FREE_DESCRIPTOR_SET, so freeing cannot fail
                let _ = device
                    .logical_device
                    .free_descriptor_sets(pool, &[descriptor_set]);
                device.logical_device.destroy_image_view(image_view, None);
                device.logical_device.destroy_image(image, None);
                device.logical_device.free_memory(memory, None);
            });
    }
}
