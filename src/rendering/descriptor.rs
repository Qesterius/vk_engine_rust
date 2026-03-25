use ash::vk;
use anyhow::{Result, anyhow};


//  Utility struct to keep memory behind Binding Info pointers in current scope
/// We store these instead of `vk::DescriptorSetLayoutBinding` because the 
/// Vulkan structs contain internal references (lifetimes) that make them 
/// difficult to store in a long-lived Vec. This struct "owns" its data.
pub struct DescriptorBindingInfo {
    binding: u32,
    descriptor_type: vk::DescriptorType,
    stage_flags: vk::ShaderStageFlags,
}

// Builder for creating descriptor set layouts
/// Descriptor acts like schema for shader resources. It defines which resources
/// (buffers, images, samplers) are used by the shader and how they are accessed
/// (read/write) at binding points. 
/// 
/// This builder pattern is used to construct descriptor set layouts, which define
/// the structure of resources that shaders will access.
pub struct DescriptorLayoutBuilder {
    bindings: Vec<DescriptorBindingInfo>,
}

impl DescriptorLayoutBuilder {
    pub fn new() -> Self {
        Self { bindings: Vec::new() }
    }


    /// Adds a binding to the layout.
    /// 
    /// This method adds a new binding to the descriptor set layout, specifying
    /// what kind of resource will be accessed at a particular binding point.
    /// 
    /// # Arguments
    /// * `binding` - The binding index used in the shader (e.g., `layout(binding = 0)`).
    /// * `descriptor_type` - The type of resource (e.g., `UNIFORM_BUFFER`, `COMBINED_IMAGE_SAMPLER`).
    /// * `stage_flags` - Which shader stages can access this resource (e.g., `VERTEX`, `FRAGMENT`).
    pub fn add_binding(mut self, 
        binding: u32,
        descriptor_type: vk::DescriptorType, 
        stage_flags: vk::ShaderStageFlags
    ) -> Self {

        self.bindings.push(DescriptorBindingInfo {
            binding,
            descriptor_type,
            stage_flags,
        });

        self
    }

    /// Builds the descriptor set layout from the accumulated bindings.
    /// 
    /// This method creates the actual Vulkan descriptor set layout using the
    /// accumulated bindings.
    pub fn build(self, device: &ash::Device) -> Result<vk::DescriptorSetLayout> {
        let layout_bindings: Vec<vk::DescriptorSetLayoutBinding> = self.bindings.iter().map(|b| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(b.binding)
                .descriptor_type(b.descriptor_type)
                .descriptor_count(1)
                .stage_flags(b.stage_flags)
        }).collect();

        let layout_info = vk::DescriptorSetLayoutCreateInfo::default()
            .bindings(&layout_bindings);

        unsafe {
            device.create_descriptor_set_layout(&layout_info, None)
                .map_err(|e| anyhow!("Failed to create descriptor set layout: {}", e))
        }
    }
}


/// Allocates descriptor sets from a descriptor pool using the specified layout and count.
/// 
/// This function creates a specified number of descriptor sets that follow the given layout.
/// All sets will use the same descriptor set layout.
/// 
/// # Arguments
/// * `device` - The Vulkan device to allocate the descriptor sets on
/// * `descriptor_pool` - The descriptor pool from which to allocate the sets
/// * `layout` - The descriptor set layout that defines the structure of the sets
/// * `count` - The number of descriptor sets to allocate
pub fn allocate_descriptor_sets(
    device: &ash::Device,
    descriptor_pool: vk::DescriptorPool, // heap of memory for descriptor sets
    layout: vk::DescriptorSetLayout, // layout defines the structure of the descriptor sets (bindings, types, stages)
    count: u32
) -> Result<Vec<vk::DescriptorSet>> {
    let layouts = vec![layout; count as usize];
    let alloc_info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&layouts);

    unsafe {
        device.allocate_descriptor_sets(&alloc_info)
            .map_err(|e| anyhow!("Failed to allocate descriptor sets: {}", e))
    }
} 

/// Updates a buffer descriptor set with a new buffer.
/// 
/// This function updates a descriptor set to point to a new buffer resource.
/// It's commonly used to update uniform buffers or other buffer-based resources
/// that may change between frames.
pub fn update_buffer_descriptor_set(
    device: &ash::Device,
    descriptor_set: vk::DescriptorSet,
    buffer : vk::Buffer,
    descriptor_type : vk::DescriptorType,
    binding: u32,
) {
    let buffer_info = vk::DescriptorBufferInfo::default()
        .buffer(buffer)
        .offset(0)
        .range(vk::WHOLE_SIZE);

    let descriptor_write = vk::WriteDescriptorSet::default()
        .dst_set(descriptor_set)
        .dst_binding(binding)
        .descriptor_type(descriptor_type)
        .buffer_info(std::slice::from_ref(&buffer_info));

    unsafe {
        device.update_descriptor_sets(&[descriptor_write], &[]);
    }
}

/// Pool of info about descriptors. It holds metadata, descriptors hold the data.
/// 
/// This function creates a descriptor pool which serves as a heap for allocating
/// descriptor sets. The pool must be large enough to accommodate all the descriptor
/// sets that will be needed during rendering.
pub fn create_descriptor_pool(logical_device: &ash::Device, max_sets: u32) -> Result<vk::DescriptorPool> {
    let pool_sizes =&[vk::DescriptorPoolSize {
            ty: vk::DescriptorType::UNIFORM_BUFFER,
            descriptor_count: max_sets
        }];

    let create_info = vk::DescriptorPoolCreateInfo::default()
        .max_sets(max_sets)
        .pool_sizes(pool_sizes);
    
    unsafe{
        Ok(logical_device.create_descriptor_pool(&create_info, None)?)
    }
}