use std::sync::Arc;
use ash::vk;

struct PipelineManager{
    device: Arc<RenderingDevice>,
    render_pass : vk::RenderPass,
    descriptor_set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    graphics_pipeline: vk::Pipeline,
}