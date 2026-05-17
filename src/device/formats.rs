use ash::vk;

// Standard 8-bit Color (SRGB)
pub const COLOR_FORMAT_CANDIDATES: &[vk::Format] = &[
    vk::Format::R8G8B8A8_SRGB,
    vk::Format::B8G8R8A8_SRGB,
    vk::Format::A8B8G8R8_SRGB_PACK32,
];

// Linear Data (Normal Maps, Roughness, Metalness)
pub const DATA_FORMAT_CANDIDATES: &[vk::Format] =
    &[vk::Format::R8G8B8A8_UNORM, vk::Format::B8G8R8A8_UNORM];

// High Dynamic Range (HDR) / Skyboxes
#[allow(dead_code)]
pub const HDR_FORMAT_CANDIDATES: &[vk::Format] = &[
    vk::Format::R16G16B16A16_SFLOAT,
    vk::Format::R32G32B32A32_SFLOAT,
];

// Depth Buffers
pub const DEPTH_FORMAT_CANDIDATES: &[vk::Format] = &[
    vk::Format::D32_SFLOAT,
    vk::Format::D32_SFLOAT_S8_UINT,
    vk::Format::D24_UNORM_S8_UINT,
];
// --- Specialized & Compressed ---

/// Single-Channel Masks: Best for Font Atlases and Opacity masks.
/// Saves 75% VRAM compared to using a full RGBA texture.
#[allow(dead_code)]
pub const MASK_FORMAT_CANDIDATES: &[vk::Format] = &[
    vk::Format::R8_UNORM,
    vk::Format::R8G8_UNORM, // 2-channel fallback
];

/// Compressed Color (BC7): The "Pro" way to store world textures.
/// Reduces VRAM usage by ~4x with almost no visible quality loss.
#[allow(dead_code)]
pub const COMPRESSED_COLOR_CANDIDATES: &[vk::Format] = &[
    vk::Format::BC7_SRGB_BLOCK,
    vk::Format::BC3_SRGB_BLOCK, // DXT5 fallback for older hardware
];

/// Compressed Data (BC5): Highest quality for Normal maps.
/// Specifically optimized to store high-precision X and Y vector data.
#[allow(dead_code)]
pub const COMPRESSED_DATA_CANDIDATES: &[vk::Format] =
    &[vk::Format::BC5_UNORM_BLOCK, vk::Format::BC7_UNORM_BLOCK];

use anyhow::{Result, anyhow};

pub(crate) fn get_supported_format(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    candidates: &[vk::Format],
    tiling: vk::ImageTiling,
    features: vk::FormatFeatureFlags,
) -> Result<vk::Format> {
    candidates
        .iter()
        .cloned()
        .find(|&format| {
            let props =
                unsafe { instance.get_physical_device_format_properties(physical_device, format) };

            match tiling {
                vk::ImageTiling::LINEAR => props.linear_tiling_features.contains(features),
                vk::ImageTiling::OPTIMAL => props.optimal_tiling_features.contains(features),
                _ => false,
            }
        })
        .ok_or_else(|| {
            anyhow!(
                "Failed to find a supported format among candidates: {:?}",
                candidates
            )
        })
}
