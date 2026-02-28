use std::ffi::CStr;

use ash::vk;
use anyhow::{Result, anyhow};


pub struct PipelineBuilder{
    pub shader_stages_ingr: Vec<(vk::ShaderModule, vk::ShaderStageFlags)>,    
    pub binding_descriptions: Vec<vk::VertexInputBindingDescription>,
    pub attribute_descriptions: Vec<vk::VertexInputAttributeDescription>,

    pub topology: vk::PrimitiveTopology,
    pub polygon_mode: vk::PolygonMode,
    pub cull_mode: vk::CullModeFlags,
    pub front_face: vk::FrontFace,
    pub samples: vk::SampleCountFlags,
    pub color_blend_attachment: vk::PipelineColorBlendAttachmentState,
    pub pipeline_layout: vk::PipelineLayout,
}
impl PipelineBuilder{
    pub fn new (layout: vk::PipelineLayout) -> Self {
            Self {
                shader_stages_ingr: Vec::new(),
                binding_descriptions: Vec::new(),
                attribute_descriptions: Vec::new(),
                topology: vk::PrimitiveTopology::TRIANGLE_LIST,
                polygon_mode: vk::PolygonMode::FILL,
                cull_mode: vk::CullModeFlags::BACK,
                front_face: vk::FrontFace::CLOCKWISE,
                samples: vk::SampleCountFlags::TYPE_1,
                color_blend_attachment: vk::PipelineColorBlendAttachmentState::default()
                    .color_write_mask(vk::ColorComponentFlags::RGBA)
                    .blend_enable(false),
                pipeline_layout: layout,
            }
        }


    pub fn with_shader(mut self, module: vk::ShaderModule, stage: vk::ShaderStageFlags) -> Self {
        self.shader_stages_ingr.push((module, stage));
        self
    }

    pub fn with_vertex_input(mut self, bindings: Vec<vk::VertexInputBindingDescription>, attributes: Vec<vk::VertexInputAttributeDescription>) -> Self {
        self.binding_descriptions = bindings;
        self.attribute_descriptions = attributes;
        self
    }

    pub fn with_input_assembly(mut self, topology: vk::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    pub fn with_rasterization(mut self, polygon_mode: vk::PolygonMode, cull_mode: vk::CullModeFlags, front_face: vk::FrontFace) -> Self {
        self.polygon_mode = polygon_mode;
        self.cull_mode = cull_mode;
        self.front_face = front_face;
        self
    }

    pub fn with_multisampling(mut self, samples: vk::SampleCountFlags) -> Self {
        self.samples = samples;
        self
    }

    pub fn with_color_blending(mut self, color_blend_attachment: vk::PipelineColorBlendAttachmentState) -> Self {
        self.color_blend_attachment = color_blend_attachment;
        self
    }

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