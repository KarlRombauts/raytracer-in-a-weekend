use crate::vec3::Vec3;

pub type Color = Vec3;

impl Color {
    pub fn to_rgb_vec(&self) -> [u8; 3] {
        let mut r = linear_to_gamma(self.x);
        let mut g = linear_to_gamma(self.y);
        let mut b = linear_to_gamma(self.z);

        return [
            (r.clamp(0., 1.) * 255.0) as u8,
            (g.clamp(0., 1.) * 255.0) as u8,
            (b.clamp(0., 1.) * 255.0) as u8,
        ];
    }
}

fn linear_to_gamma(linear_component: f32) -> f32 {
    if linear_component > 0.0 {
        return linear_component.sqrt();
    }
    return 0.0;
}

pub fn write_color(out: &mut dyn std::io::Write, pixel_color: Vec3) -> () {
    let mut r = linear_to_gamma(pixel_color.x);
    let mut g = linear_to_gamma(pixel_color.y);
    let mut b = linear_to_gamma(pixel_color.z);

    r = r.clamp(0.0, 0.999) * 256.0;
    g = g.clamp(0.0, 0.999) * 256.0;
    b = b.clamp(0.0, 0.999) * 256.0;

    writeln!(out, "{} {} {}", r as u8, g as u8, b as u8).expect("Failed to write color to output");
}
