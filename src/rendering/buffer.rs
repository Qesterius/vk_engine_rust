use ash::vk;
use anyhow::Result;

use super::cleanup::DeletionQueue;
use super::memory::{find_memory_type, map_and_copy};


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

pub unsafe fn create_vertex_buffer<T: Copy>(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    data: &[T],
    deletion_queue: &mut DeletionQueue,
) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let size = (std::mem::size_of::<T>() * data.len()) as vk::DeviceSize;
    let (buf, mem) = create_buffer(
        device,
        mem_props,
        size,
        vk::BufferUsageFlags::VERTEX_BUFFER,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    unsafe { map_and_copy(device, mem, data) }?;
    let d = device.clone();
    deletion_queue.push(move || unsafe {
        d.destroy_buffer(buf, None);
        d.free_memory(mem, None);
    });
    Ok((buf, mem))
}

pub unsafe fn create_index_buffer(
    device: &ash::Device,
    mem_props: &vk::PhysicalDeviceMemoryProperties,
    indices: &[u32],
    deletion_queue: &mut DeletionQueue,
) -> Result<(vk::Buffer, vk::DeviceMemory)> {
    let size = (std::mem::size_of::<u32>() * indices.len()) as vk::DeviceSize;
    let (buf, mem) = create_buffer(
        device,
        mem_props,
        size,
        vk::BufferUsageFlags::INDEX_BUFFER,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    unsafe { map_and_copy(device, mem, indices) }?;
    let d = device.clone();
    deletion_queue.push(move || unsafe {
        d.destroy_buffer(buf, None);
        d.free_memory(mem, None);
    });
    Ok((buf, mem))
}
