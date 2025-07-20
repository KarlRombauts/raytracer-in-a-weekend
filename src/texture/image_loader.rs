use image::{ImageBuffer, Rgb};
use palette::{IntoColor, LinSrgb, Srgb};

pub fn load_image_linear_buffer(
    path: &str,
) -> Result<ImageBuffer<Rgb<f32>, Vec<f32>>, Box<dyn std::error::Error>> {
    let img = image::open(path)?.to_rgb8();
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

    Ok(ImageBuffer::from_raw(width, height, linear_data).unwrap())
}
