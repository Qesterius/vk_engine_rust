use std::sync::Arc;
use ash::vk;
use crate::device::device::Device;
use super::render_target

pub struct RenderingManger{
    device: Arc<Device>,
    canvas: Canvas,
    command_pool: vk::CommandPool,
    command_buffers: Vec<vk::CommandBuffer>,
    in_flight_fences: Vec<vk::Fence>,
    render_finished_semaphores: Vec<vk::Semaphore>,
    image_available_semaphores: Vec<vk::Semaphore>,
}