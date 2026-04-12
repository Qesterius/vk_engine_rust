use anyhow::Result;
use image::{ImageReader, RgbaImage};

pub fn load(file_path: &str) -> Result<RgbaImage> {
    let img = ImageReader::open(file_path)?.decode()?;
    Ok(img.to_rgba8())
}
