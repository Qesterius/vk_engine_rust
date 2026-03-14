use ash::vk;
use cgmath;

/// A vertex structure containing position and color data.
/// 
/// This struct represents a single vertex with 3D position and RGB color components.
/// It's used for rendering 3D objects with colored vertices.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Vertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

impl Vertex {
    /// Creates a new vertex with the specified position and color.
    /// 
    /// # Arguments
    /// * `position` - The 3D position coordinates [x, y, z]
    /// * `color` - The RGB color components [r, g, b]
    pub fn new(position: [f32; 3], color: [f32; 3]) -> Self {
        Self { position, color }
    }
    /// Returns the vertex input binding description for this vertex structure.
    /// 
    /// This function provides the binding description that tells Vulkan how to
    /// bind the vertex data to the shader inputs.
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Vertex>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }
    /// Returns the vertex input attribute descriptions for this vertex structure.
    /// 
    /// This function provides the attribute descriptions that tell Vulkan how to
    /// interpret the vertex data for each shader input attribute.
    pub fn get_attribute_descriptions() -> [vk::VertexInputAttributeDescription; 2] {
        [
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32G32B32_SFLOAT,
                offset: std::mem::size_of::<[f32; 3]>() as u32,
            },
        ]
    }
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
/// Uniform buffer object for passing transformation matrices to the shader.
/// 
/// This struct holds transformation matrices (model, view, and projection) that
/// are passed to the shader as a uniform buffer. These matrices define how the
/// 3D object should be transformed in the scene. The uniform buffer is consistent
/// across all vertices in a draw call but can be updated between draw calls.
pub struct UniformBufferObject {
    pub model: cgmath::Matrix4<f32>,
    pub view: cgmath::Matrix4<f32>,
    pub proj: cgmath::Matrix4<f32>,
}