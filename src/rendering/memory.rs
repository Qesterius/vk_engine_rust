use ash::vk;
use anyhow::Result;
use anyhow::Ok;

/// Finds a memory type index that supports both the required bits (from the resource)
/// and the desired properties (e.g. Host Visible, Device Local).
/// 
/// This function searches through the available memory types on the physical device
/// and returns the first one that satisfies both the type filter (which specifies
/// what kind of memory is required for the resource) and the property requirements
/// (such as HOST_VISIBLE for CPU access or DEVICE_LOCAL for GPU-only memory).
/// 
/// # Arguments
/// * `instance` - The Vulkan instance used to query memory properties
/// * `physical_device` - The physical device whose memory properties are queried
/// * `type_filter` - A bitmask of memory type bits that are acceptable for the resource
/// * `properties` - The required memory properties (e.g., HOST_VISIBLE, DEVICE_LOCAL)
pub fn find_memory_type(
    mem_properties: &vk::PhysicalDeviceMemoryProperties,
    type_filter: u32,
    properties: vk::MemoryPropertyFlags,
) -> Result<u32> {
    for (i, memory_type) in mem_properties.memory_types.iter().enumerate() {
        if (type_filter & (1 << i)) != 0 && (memory_type.property_flags & properties) == properties
        {
            return Ok(i as u32);
        }
    }
    Err(anyhow::anyhow!("Failed to find suitable memory type!"))
}

/// Helper function to send memory to GPU VRAM.
/// 
/// This function maps device memory, copies data to it, and unmaps it.
/// It only works if the memory was allocated with the HOST_VISIBLE flag.
/// If the memory is DEVICE_LOCAL (GPU-only), you cannot map it directly.
/// In that case, you would need a "Staging Buffer" to act as a middleman.
/// 
/// # Arguments
/// * `device` - The Vulkan device used to map and unmap the memory
/// * `memory` - The device memory handle to map and copy to
/// * `data` - The data to copy to the memory
pub unsafe fn map_and_copy<T: Copy>(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    data: &[T],
) -> Result<()> {
    let size = (std::mem::size_of::<T>() * data.len()) as vk::DeviceSize;
    let ptr = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }?;
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut T, data.len()) };
    unsafe { device.unmap_memory(memory) };
    Ok(())
}


