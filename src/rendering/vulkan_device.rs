use ash::vk;

pub struct VulkanDevice {
    pub instance: ash::Instance,
    pub logical_device: ash::Device,
    pub physical_device: vk::PhysicalDevice,
    pub memory_properties: vk::PhysicalDeviceMemoryProperties,
    pub graphics_queue: vk::Queue,
}

impl Drop for VulkanDevice {
    fn drop(&mut self) {
        unsafe {
            self.logical_device.device_wait_idle().ok();
            self.logical_device.destroy_device(None);
            // Instance destruction handled here too
        }
    }
}