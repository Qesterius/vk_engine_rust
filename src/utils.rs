use std::ffi::CStr;

/// Safely converts a fixed-size Vulkan char array (like [i8; 256])
/// into a Rust &CStr for easy comparison or printing.
pub fn vk_to_cstr(raw_char_array: &[i8]) -> &CStr {
    unsafe {
        // Vulkan guarantees these strings are null-terminated
        CStr::from_ptr(raw_char_array.as_ptr())
    }
}

/// Safely casts any Sized Rust type into a raw byte slice for Vulkan.
///
/// This is required for functions like `cmd_push_constants` or `update_buffer`,
/// which expect a `&[u8]` regardless of whether the data is a Matrix, a struct,
/// or a single float.
///
/// # Safety
/// The type `T` must be `#[repr(C)]` to ensure the byte layout matches
/// what the GPU (SPIR-V) expects.
pub unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    unsafe { std::slice::from_raw_parts((p as *const T) as *const u8, std::mem::size_of::<T>()) }
}
