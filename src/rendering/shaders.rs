use ash::vk;
use anyhow::{Result, anyhow};

pub unsafe fn load_shader_module(device: &ash::Device, path: &str) -> Result<vk::ShaderModule> {
    let file = std::fs::File::open(path)
        .map_err(|e| anyhow!("Failed to open shader file {}: {}", path, e))?;
    let words = ash::util::read_spv(&mut std::io::BufReader::new(file))
        .map_err(|e| anyhow!("Failed to read shader SPV {}: {}", path, e))?;
    unsafe { create_shader_module(device, &words) }
}

pub unsafe fn create_shader_module(device: &ash::Device, code: &[u32]) -> Result<vk::ShaderModule> {
    let create_info = vk::ShaderModuleCreateInfo::default().code(code);
    unsafe { device.create_shader_module(&create_info, None) }
        .map_err(|e| anyhow!("Failed to create shader module: {}", e))
}
