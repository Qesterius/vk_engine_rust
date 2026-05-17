use crate::device::device::Device;
use anyhow::{Ok, Result};
use ash::vk;

impl Device {
    pub fn begin_single_time_commands(&self) -> Result<vk::CommandBuffer> {
        let info = vk::CommandBufferAllocateInfo::default()
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_pool(self.command_pool)
            .command_buffer_count(1);
        let command_buffer = unsafe { self.logical_device.allocate_command_buffers(&info) }?[0];

        let info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe {
            self.logical_device
                .begin_command_buffer(command_buffer, &info)
        }?;
        Ok(command_buffer)
    }
    pub fn end_single_time_commands(&self, command_buffer: vk::CommandBuffer) -> Result<()> {
        unsafe {
            self.logical_device.end_command_buffer(command_buffer)?;
            let command_buffers = &[command_buffer];
            let info = vk::SubmitInfo::default().command_buffers(command_buffers);
            self.logical_device
                .queue_submit(self.graphics_queue, &[info], vk::Fence::null())?;
            self.logical_device.queue_wait_idle(self.graphics_queue)?;
            self.logical_device
                .free_command_buffers(self.command_pool, command_buffers);
        }
        Ok(())
    }
}
