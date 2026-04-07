use std::sync::Arc;

use anyhow::{Ok, Result};
use ash::vk;
use bevy_ecs::component::Component;

use std::sync::atomic::Ordering;

use crate::{
    device::device::Device,
    rendering::{buffer, vertex::Vertex},
};
#[derive(Component)]
pub struct Mesh {
    device: Arc<Device>,
    pub vertex_buffer: vk::Buffer,
    pub vertex_buffer_memory: vk::DeviceMemory,
    pub index_buffer: vk::Buffer,
    pub index_buffer_memory: vk::DeviceMemory,
    pub index_count: u32,
}

impl Mesh {
    pub fn bind(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        unsafe {
            device.cmd_bind_vertex_buffers(cmd, 0, &[self.vertex_buffer], &[0]);
            device.cmd_bind_index_buffer(cmd, self.index_buffer, 0, vk::IndexType::UINT32);
        }
    }

    pub fn draw(&self, device: &ash::Device, cmd: vk::CommandBuffer) {
        unsafe { device.cmd_draw_indexed(cmd, self.index_count, 1, 0, 0, 0) };
    }

    pub fn new_cube(
        mem_props: &vk::PhysicalDeviceMemoryProperties,
        device: Arc<Device>,
    ) -> Result<Self> {
        let vertices = [
            // Front face
            Vertex::new([-0.5, -0.5, 0.5], [1.0, 0.0, 0.0]),
            Vertex::new([0.5, -0.5, 0.5], [0.0, 1.0, 0.0]),
            Vertex::new([0.5, 0.5, 0.5], [0.0, 0.0, 1.0]),
            Vertex::new([-0.5, 0.5, 0.5], [1.0, 1.0, 1.0]),
            // Back face
            Vertex::new([-0.5, -0.5, -0.5], [1.0, 0.0, 1.0]),
            Vertex::new([0.5, -0.5, -0.5], [0.0, 1.0, 1.0]),
            Vertex::new([0.5, 0.5, -0.5], [1.0, 1.0, 0.0]),
            Vertex::new([-0.5, 0.5, -0.5], [0.0, 0.0, 0.0]),
        ];
        let indices: [u32; 36] = [
            0, 1, 2, 2, 3, 0, // Front
            1, 5, 6, 6, 2, 1, // Right
            7, 6, 5, 5, 4, 7, // Back
            4, 0, 3, 3, 7, 4, // Left
            4, 5, 1, 1, 0, 4, // Bottom
            3, 2, 6, 6, 7, 3, // Top
        ];

        let (vertex_buffer, vertex_buffer_memory) =
            unsafe { buffer::create_vertex_buffer(&device.logical_device, mem_props, &vertices) }?;
        let (index_buffer, index_buffer_memory) =
            unsafe { buffer::create_index_buffer(&device.logical_device, mem_props, &indices) }?;

        Ok(Self {
            device: Arc::clone(&device),
            vertex_buffer,
            vertex_buffer_memory,
            index_buffer,
            index_buffer_memory,
            index_count: indices.len() as u32,
        })
    }
}

impl Drop for Mesh {
    fn drop(&mut self) {
        // Read the sync slot that was current when this Drop fired.
        // At runtime this is the slot whose fence was most recently waited on, so pushing
        // here defers destruction until that slot comes around again (fence-guarded).
        // At shutdown, engine.shutdown() called device_wait_idle before world drop, so
        // any slot is safe — all dynamic queues are flushed in Device::drop.
        let slot = self.device.current_sync_idx.load(Ordering::Relaxed);

        let d = self.device.logical_device.clone();
        let vb = self.vertex_buffer;
        let vbm = self.vertex_buffer_memory;
        let ib = self.index_buffer;
        let ibm = self.index_buffer_memory;

        self.device.dynamic_deletion_queues[slot]
            .lock()
            .unwrap()
            .push(move || unsafe {
                d.destroy_buffer(vb, None);
                d.free_memory(vbm, None);
                d.destroy_buffer(ib, None);
                d.free_memory(ibm, None);
            });
    }
}
