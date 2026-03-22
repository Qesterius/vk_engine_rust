use ash::vk;
use anyhow::{Result, anyhow};

use super::memory::find_memory_type;


/// Creates a Vulkan Buffer and allocates its backing Device Memory.
///
/// This function performs three key steps:
/// 1. Creates the Buffer handle with a specific size and usage (Vertex, Index, etc.)
/// 2. Queries the GPU for memory requirements (alignment, size, and compatible memory types)
/// 3. Allocates and binds the memory to the Buffer
///
/// # Arguments
/// * `instance` - The Vulkan instance used to query memory properties
/// * `device` - The logical device used to create the buffer and allocate memory
/// * `physical_device` - The physical device whose memory properties are queried
/// * `size` - The size in bytes of the buffer to create
/// * `usage` - How the buffer will be used (e.g., `vk::BufferUsageFlags::VERTEX_BUFFER`)
/// * `properties` - Memory flags (e.g., `vk::MemoryPropertyFlags::HOST_VISIBLE` for CPU access).
pub fn create_buffer(
    device: &ash::Device,
    physical_device_memory_props: &vk::PhysicalDeviceMemoryProperties,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    properties: vk::MemoryPropertyFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory)> {

    let buffer_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);

    let buffer = unsafe { device.create_buffer(&buffer_info, None) }?;

    let mem_requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let mem_type_index = find_memory_type(
        physical_device_memory_props,
        mem_requirements.memory_type_bits,
        properties,
    )?;

    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(mem_requirements.size)
        .memory_type_index(mem_type_index);

    let buffer_memory = unsafe { device.allocate_memory(&alloc_info, None) }?;
    unsafe { device.bind_buffer_memory(buffer, buffer_memory, 0) }?;

    Ok((buffer, buffer_memory))
}

