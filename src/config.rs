use std::ffi::CStr;
use ash::vk::make_api_version;

pub const VALIDATION_ENABLED: bool = cfg!(debug_assertions);
pub const APPLICATION_TITLE : &str = "Rust Vulkan engine";
pub const APPLICATION_NAME: &CStr = c"My Rust Engine";
pub const ENGINE_NAME: &CStr = c"No Engine";
pub const ENGINE_VERSION: u32 = make_api_version(0, 1, 0, 0);
pub const MAX_FRAMES_IN_FLIGHT : usize = 2;
