use anyhow::{Ok, Result, anyhow};
use ash::vk;
use bevy_ecs::resource::Resource;
use std::{collections::HashMap, sync::Arc};

use crate::{
    assets::{loaders, slot_allocator::SlotAllocator, texture::Texture},
    config::MAX_FRAMES_IN_FLIGHT,
    device::device::Device,
    rendering::{
        buffer,
        descriptor::{self, DescriptorLayoutBuilder},
        image, memory,
    },
};

// Binding indices within set 1
const TEXTURES_BINDING: u32 = 0;
const SAMPLERS_BINDING: u32 = 1;

#[derive(Hash, Eq, PartialEq, Clone, Copy)]
#[allow(dead_code)]
pub enum SamplerType {
    LinearRepeat,
    LinearClamp,
    NearestClamp,
}

impl SamplerType {
    pub fn index(self) -> u32 {
        match self {
            SamplerType::LinearRepeat => 0,
            SamplerType::LinearClamp => 1,
            SamplerType::NearestClamp => 2,
        }
    }
}

const N_SAMPLERS: u32 = 3;

#[derive(Resource)]
pub struct AssetManager {
    device: Arc<Device>,
    pub bindless_descriptor_set_layout: vk::DescriptorSetLayout,
    pub bindless_descriptor_pool: vk::DescriptorPool,
    pub bindless_descriptor_set: vk::DescriptorSet,
    samplers: Vec<vk::Sampler>,
    pub loaded_textures: HashMap<String, Arc<Texture>>,
    slots: SlotAllocator,
    // Placeholder GPU resources — kept alive until AssetManager drops
    placeholder_image: vk::Image,
    placeholder_image_view: vk::ImageView,
    placeholder_memory: vk::DeviceMemory,
}

impl Drop for AssetManager {
    fn drop(&mut self) {
        for texture in self.loaded_textures.values() {
            texture.destroy(&self.device);
        }
        unsafe {
            self.device
                .logical_device
                .destroy_image_view(self.placeholder_image_view, None);
            self.device
                .logical_device
                .destroy_image(self.placeholder_image, None);
            self.device
                .logical_device
                .free_memory(self.placeholder_memory, None);

            for &sampler in &self.samplers {
                self.device.logical_device.destroy_sampler(sampler, None);
            }
            // Destroying the pool implicitly frees bindless_descriptor_set
            self.device
                .logical_device
                .destroy_descriptor_pool(self.bindless_descriptor_pool, None);
            self.device
                .logical_device
                .destroy_descriptor_set_layout(self.bindless_descriptor_set_layout, None);
        }
    }
}

impl AssetManager {
    pub fn new(device: Arc<Device>) -> Result<Self> {
        let max_textures = device.max_texture_slots;

        // Layout: binding 0 = SAMPLED_IMAGE array, binding 1 = SAMPLER array
        let layout = DescriptorLayoutBuilder::new()
            .add_binding_array(
                TEXTURES_BINDING,
                vk::DescriptorType::SAMPLED_IMAGE,
                vk::ShaderStageFlags::FRAGMENT,
                max_textures,
            )
            .add_binding_array(
                SAMPLERS_BINDING,
                vk::DescriptorType::SAMPLER,
                vk::ShaderStageFlags::FRAGMENT,
                N_SAMPLERS,
            )
            .build(&device.logical_device)?;

        let pool_sizes = [
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLED_IMAGE)
                .descriptor_count(max_textures),
            vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::SAMPLER)
                .descriptor_count(N_SAMPLERS),
        ];
        let pool = descriptor::create_descriptor_pool(&device.logical_device, 1, &pool_sizes)?;

        let bindless_set =
            descriptor::allocate_descriptor_sets(&device.logical_device, pool, layout, 1)?[0];

        // Build sampler pool in SamplerType::index() order
        let samplers = vec![
            image::create_texture_sampler(&device)?, // 0: LinearRepeat
            image::create_sampler_linear_clamp(&device)?, // 1: LinearClamp
            image::create_sampler_nearest_clamp(&device)?, // 2: NearestClamp
        ];

        // Write all samplers into binding 1
        for (i, &sampler) in samplers.iter().enumerate() {
            descriptor::update_sampler_slot(
                &device.logical_device,
                bindless_set,
                sampler,
                i as u32,
                SAMPLERS_BINDING,
            );
        }

        // Create a 1×1 white placeholder texture to fill all image slots
        let white_pixel: [u8; 4] = [255, 255, 255, 255];
        let placeholder_extent = vk::Extent3D {
            width: 1,
            height: 1,
            depth: 1,
        };

        let (staging_buf, staging_mem) = buffer::create_buffer(
            &device.logical_device,
            &device.memory_properties,
            4,
            vk::BufferUsageFlags::TRANSFER_SRC,
            vk::MemoryPropertyFlags::HOST_COHERENT | vk::MemoryPropertyFlags::HOST_VISIBLE,
        )?;
        unsafe { memory::map_and_copy(&device.logical_device, staging_mem, &white_pixel) }?;

        let (placeholder_image, placeholder_memory) =
            image::create_image(&device, placeholder_extent, device.format_color)?;

        image::transition_image_layout(
            &device,
            placeholder_image,
            device.format_color,
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        )?;
        image::copy_buffer_to_image(&device, staging_buf, placeholder_image, placeholder_extent)?;
        image::transition_image_layout(
            &device,
            placeholder_image,
            device.format_color,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )?;

        let placeholder_image_view =
            image::create_image_view(&device, placeholder_image, device.format_color)?;

        unsafe {
            device.logical_device.destroy_buffer(staging_buf, None);
            device.logical_device.free_memory(staging_mem, None);
        }

        // Pre-fill all image slots with the placeholder
        for i in 0..max_textures {
            descriptor::update_sampled_image_slot(
                &device.logical_device,
                bindless_set,
                placeholder_image_view,
                i,
                TEXTURES_BINDING,
            );
        }

        Ok(Self {
            device,
            bindless_descriptor_set_layout: layout,
            bindless_descriptor_pool: pool,
            bindless_descriptor_set: bindless_set,
            samplers,
            loaded_textures: HashMap::new(),
            slots: SlotAllocator::new(max_textures),
            placeholder_image,
            placeholder_image_view,
            placeholder_memory,
        })
    }

    /// Moves reclaimed slots that are safe at `current_frame` back into the free pool.
    /// Call once per frame before any load/unload operations.
    pub fn flush_pending_reclaims(&mut self, current_frame: usize) {
        self.slots.flush(current_frame);
    }

    pub fn load_texture(&mut self, path: &str, sampler_type: SamplerType) -> Result<Arc<Texture>> {
        if let Some(texture) = self.loaded_textures.get(path) {
            return Ok(Arc::clone(texture));
        }

        let slot = self
            .slots
            .alloc()
            .ok_or_else(|| anyhow!("Bindless texture array is full ({} slots)", self.device.max_texture_slots))?;

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

        // Write this image into its slot in the bindless array
        descriptor::update_sampled_image_slot(
            &self.device.logical_device,
            self.bindless_descriptor_set,
            img_vk_view,
            slot,
            TEXTURES_BINDING,
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
            slot,
            sampler_type.index(),
            path.to_string(),
        );

        let texture_arc = Arc::new(texture);
        self.loaded_textures
            .insert(path.to_string(), Arc::clone(&texture_arc));
        Ok(texture_arc)
    }

    /// Unloads a texture. The descriptor slot is restored to the placeholder immediately;
    /// the slot index is returned to `free_slots` only after `MAX_FRAMES_IN_FLIGHT` frames
    /// have elapsed to ensure no in-flight frame still references the old image.
    /// GPU resources (image, memory) are queued for deferred destruction via the device deletion queue.
    #[allow(dead_code)]
    pub fn unload_texture(&mut self, path: &str, current_frame: usize) {
        let Some(texture) = self.loaded_textures.remove(path) else {
            return;
        };

        let slot = texture.index;

        // Restore placeholder immediately so the slot is never uninitialized
        descriptor::update_sampled_image_slot(
            &self.device.logical_device,
            self.bindless_descriptor_set,
            self.placeholder_image_view,
            slot,
            TEXTURES_BINDING,
        );

        // Defer slot reuse until all in-flight frames have passed
        self.slots.free(slot, current_frame);

        // Defer GPU resource destruction via the per-frame deletion queue
        let device = Arc::clone(&self.device);
        let image = texture.image;
        let image_view = texture.image_view;
        let memory = texture.memory;
        let queue_idx = current_frame % MAX_FRAMES_IN_FLIGHT;
        self.device.dynamic_deletion_queues[queue_idx]
            .lock()
            .unwrap()
            .push(move || unsafe {
                device.logical_device.destroy_image_view(image_view, None);
                device.logical_device.destroy_image(image, None);
                device.logical_device.free_memory(memory, None);
            });
    }
}
