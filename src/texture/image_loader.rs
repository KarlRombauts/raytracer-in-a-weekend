use image::{ImageBuffer, Rgb, RgbImage};
use palette::{IntoColor, LinSrgb, Srgb};

/// Convert an 8-bit sRGB image into a linear-light f32 buffer.
fn rgb8_to_linear(img: RgbImage) -> ImageBuffer<Rgb<f32>, Vec<f32>> {
    let (width, height) = img.dimensions();
    let linear_data: Vec<f32> = img
        .pixels()
        .flat_map(|pixel| {
            let srgb = Srgb::new(
                pixel[0] as f32 / 255.0,
                pixel[1] as f32 / 255.0,
                pixel[2] as f32 / 255.0,
            );
            let linear: LinSrgb<f32> = srgb.into_color();
            [linear.red, linear.green, linear.blue]
        })
        .collect();
    ImageBuffer::from_raw(width, height, linear_data).unwrap()
}

pub fn load_image_linear_buffer(
    path: &str,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>> {
    Ok(rgb8_to_linear(image::open(path)?.to_rgb8()))
}

/// Decode an image from in-memory bytes (format inferred from content), into a
/// linear-light f32 buffer. Used for embedded, portable image assets.
pub fn load_image_linear_buffer_from_bytes(
    bytes: &[u8],
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>> {
    Ok(rgb8_to_linear(image::load_from_memory(bytes)?.to_rgb8()))
}
