use std::ffi::CStr;

use ash::vk::{self, CompareOp};
use anyhow::{Result, anyhow};

/// A builder pattern for creating Vulkan graphics pipelines.
pub struct PipelineBuilder{
    shader_stages_ingr: Vec<(vk::ShaderModule, vk::ShaderStageFlags)>,    
    binding_descriptions: Vec<vk::VertexInputBindingDescription>,
    attribute_descriptions: Vec<vk::VertexInputAttributeDescription>,
    topology: vk::PrimitiveTopology,
    polygon_mode: vk::PolygonMode,
    cull_mode: vk::CullModeFlags,
    front_face: vk::FrontFace,
    samples: vk::SampleCountFlags,
    color_blend_attachment: vk::PipelineColorBlendAttachmentState,
    pipeline_layout: vk::PipelineLayout,
    depth_test_enabled : bool, 
    depth_write_enabled : bool, 
    depth_compare_op : CompareOp,
}
impl PipelineBuilder{
    /// Creates a new PipelineBuilder with the specified pipeline layout.
    /// 
    /// # Arguments
    /// * `layout` - The pipeline layout that defines the interface between shaders and the pipeline.
    pub fn new (layout: vk::PipelineLayout) -> Self {
            Self {
                shader_stages_ingr: Vec::new(),
                binding_descriptions: Vec::new(),
                attribute_descriptions: Vec::new(),
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                polygon_mode: vk::PolygonMode::FILL,
                cull_mode: vk::CullModeFlags::NONE,
                front_face: vk::FrontFace::CLOCKWISE,
                samples: vk::SampleCountFlags::TYPE_1,
                color_blend_attachment: vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA)
                    .blend_enable(false),
                pipeline_layout: layout,
                depth_test_enabled: true,
                depth_write_enabled: true,
                depth_compare_op: vk::CompareOp::LESS,
            }
        }

    /// Configures the depth testing and writing behavior for the pipeline.
    /// 
    /// # Arguments
    /// * `test_enabled` - If true, the GPU will compare the depth of each new pixel 
    ///   against the value already in the depth buffer. This is what allows 
    ///   closer objects to "occlude" (hide) objects further away.
    /// 
    /// * `write_enabled` - If true, the depth value of the new pixel will be 
    ///   saved to the depth buffer if it passes the comparison test. 
    ///   (Note: Usually false for transparent objects to prevent them from 
    ///   "blocking" objects behind them).
    /// 
    /// * `compare_op` - The mathematical rule used for the test. 
    ///   - `LESS`: A pixel is drawn only if it is closer to the camera than the existing pixel.
    ///   - `LESS_OR_EQUAL`: Common for skyboxes or overlapping geometry.
    ///   - `GREATER`: Used if you are using an "Inverted Z" buffer for better precision.
    pub fn with_depth(mut self, test_enabled: bool, write_enabled: bool, compare_op: vk::CompareOp) -> Self {
        self.depth_test_enabled = test_enabled;
        self.depth_write_enabled = write_enabled;
        self.depth_compare_op = compare_op;
        self
    }

    /// Adds a shader stage to the pipeline.
    /// 
    /// # Arguments
    /// * `module` - The shader module containing the compiled shader code.
    /// * `stage` - The shader stage (vertex, fragment, etc.) this module represents.
    pub fn with_shader(mut self, module: vk::ShaderModule, stage: vk::ShaderStageFlags) -> Self {
        self.shader_stages_ingr.push((module, stage));
        self
    }

    /// Sets the vertex input configuration for the pipeline.
    /// 
    /// # Arguments
    /// * `bindings` - Descriptions of how vertex data is laid out in memory.
    /// * `attributes` - Descriptions of how individual attributes (position, color, etc.) are mapped to vertex data.
    pub fn with_vertex_input(mut self, bindings: Vec<vk::VertexInputBindingDescription>, attributes: Vec<vk::VertexInputAttributeDescription>) -> Self {
        self.binding_descriptions = bindings;
        self.attribute_descriptions = attributes;
        self
    }

    /// Sets the input assembly state for the pipeline.
    /// 
    /// # Arguments
    /// * `topology` - How the vertex data should be interpreted (points, lines, triangles, etc.).
    pub fn with_input_assembly(mut self, topology: vk::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    /// Sets the rasterization state for the pipeline.
    /// 
    /// # Arguments
    /// * `polygon_mode` - How polygons should be rendered (filled, wireframe, etc.).
    /// * `cull_mode` - Which faces should be culled (none, front, back, both).
    /// * `front_face` - The winding order that defines front-facing polygons.
    pub fn with_rasterization(mut self, polygon_mode: vk::PolygonMode, cull_mode: vk::CullModeFlags, front_face: vk::FrontFace) -> Self {
        self.polygon_mode = polygon_mode;
        self.cull_mode = cull_mode;
        self.front_face = front_face;
        self
    }

    /// Sets the multisampling state for the pipeline.
    /// 
    /// # Arguments
    /// * `samples` - The number of samples to use for multisampling.
    pub fn with_multisampling(mut self, samples: vk::SampleCountFlags) -> Self {
        self.samples = samples;
        self
    }

    /// Sets the color blending state for the pipeline.
    /// 
    /// # Arguments
    /// * `color_blend_attachment` - Configuration for how colors should be blended.
    pub fn with_color_blending(mut self, color_blend_attachment: vk::PipelineColorBlendAttachmentState) -> Self {
        self.color_blend_attachment = color_blend_attachment;
        self
    }

    /// Sets the vertex binding descriptions for the pipeline.
    /// 
    /// # Arguments
    /// * `bindings` - Descriptions of how vertex data is laid out in memory.
    pub fn with_binding_descriptions(mut self, bindings: Vec<vk::VertexInputBindingDescription>) -> Self {
        self.binding_descriptions = bindings;
        self
    }
    
    /// Sets the vertex attribute descriptions for the pipeline.
    /// 
    /// # Arguments
    /// * `attributes` - Descriptions of how individual attributes (position, color, etc.) are mapped to vertex data.
    pub fn with_attribute_descriptions(mut self, attributes: Vec<vk::VertexInputAttributeDescription>) -> Self {
        self.attribute_descriptions = attributes;
        self
    }

    /// Builds the final graphics pipeline.
    /// 
    /// # Arguments
    /// * `device` - The Vulkan device to create the pipeline on.
    /// * `render_pass` - The render pass that this pipeline will be used with.
    pub fn build(self, device: &ash::Device, render_pass: vk::RenderPass) -> Result<vk::Pipeline> {
        //vectors to Vertex Input State
        let vertex_input_info = vk::PipelineVertexInputStateCreateInfo::default()
            .vertex_binding_descriptions(&self.binding_descriptions)
            .vertex_attribute_descriptions(&self.attribute_descriptions);

        let viewport_state = vk::PipelineViewportStateCreateInfo::default()
            .viewport_count(1)
            .scissor_count(1);

        let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
        let dynamic_state_info = vk::PipelineDynamicStateCreateInfo::default()
            .dynamic_states(&dynamic_states);

        //shader stages to Shader Stage Create Infos
        let shader_stage_infos: Vec<vk::PipelineShaderStageCreateInfo> = self.shader_stages_ingr.iter().map(|(module, stage)| {
            vk::PipelineShaderStageCreateInfo
                ::default()
                .stage(*stage)
                .module(*module)
                .name(CStr::from_bytes_with_nul(b"main\0").unwrap())
        }).collect();

        let input_asembly_info = vk::PipelineInputAssemblyStateCreateInfo::default()
            .topology(self.topology)
            .primitive_restart_enable(false);
        let rasterizer_info = vk::PipelineRasterizationStateCreateInfo::default()
            .depth_clamp_enable(false)
            .rasterizer_discard_enable(false)
            .polygon_mode(self.polygon_mode)
            .line_width(1.0)
            .cull_mode(self.cull_mode)
            .front_face(self.front_face)
            .depth_bias_enable(false);

        let multisampling = vk::PipelineMultisampleStateCreateInfo::default()
            .sample_shading_enable(false)
            .rasterization_samples(self.samples);
        let color_blending = vk::PipelineColorBlendStateCreateInfo::default()
            .logic_op_enable(false)
            .attachments(std::slice::from_ref(&self.color_blend_attachment));
        
        let depth_stencil_info = vk::PipelineDepthStencilStateCreateInfo::default()
            .depth_test_enable(self.depth_test_enabled) // Assuming you stored bools in the struct
            .depth_write_enable(self.depth_write_enabled)
            .depth_compare_op(self.depth_compare_op)
            .depth_bounds_test_enable(false)
            .stencil_test_enable(false);


        let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
            .stages(&shader_stage_infos)
            .vertex_input_state(&vertex_input_info)
            .input_assembly_state(&input_asembly_info)
            .rasterization_state(&rasterizer_info)
            .viewport_state(&viewport_state)
            .rasterization_state(&rasterizer_info)
            .multisample_state(&multisampling)
            .color_blend_state(&color_blending)
            .dynamic_state(&dynamic_state_info)
            .depth_stencil_state(&depth_stencil_info)
            .layout(self.pipeline_layout)
            .render_pass(render_pass)
            .subpass(0);

        let pipeline = (unsafe {
            device.create_graphics_pipelines(
                vk::PipelineCache::null(),
                std::slice::from_ref(&pipeline_info),
                None
            ).map_err(|e| anyhow!("Failed to create graphics pipeline: {:?}", e))?
        })[0];
        Ok(pipeline)
    }
}
