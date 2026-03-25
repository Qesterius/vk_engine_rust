use anyhow::{Ok, Result};
use ash::vk;

use crate::rendering::{buffer, cleanup::DeletionQueue, vertex::Vertex};

pub struct Mesh {
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
        device: &ash::Device,
        deletion_queue: &mut DeletionQueue,
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
            unsafe { buffer::create_vertex_buffer(device, mem_props, &vertices, deletion_queue) }?;
        let (index_buffer, index_buffer_memory) =
            unsafe { buffer::create_index_buffer(device, mem_props, &indices, deletion_queue) }?;

        Ok(Self {
            vertex_buffer,
            vertex_buffer_memory,
            index_buffer,
            index_buffer_memory,
            index_count: indices.len() as u32,
        })
    }
}
