use std::ffi::CStr;

/// Safely converts a fixed-size Vulkan char array (like [i8; 256]) 
/// into a Rust &CStr for easy comparison or printing.
pub fn vk_to_cstr(raw_char_array: &[i8]) -> &CStr {
    unsafe { 
        // Vulkan guarantees these strings are null-terminated
        CStr::from_ptr(raw_char_array.as_ptr()) 
    }
}

