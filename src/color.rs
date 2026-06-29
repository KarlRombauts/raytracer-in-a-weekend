use crate::vec3::Vec3;

pub type Color = Vec3;

/// Firefly suppression: if `c`'s luminance exceeds `max`, scale it down to that
/// luminance (preserving hue). Applied per sample before accumulation so a single
/// improbable high-energy path can't dominate the running average. `max` is
/// `f32::INFINITY` to disable. Introduces a small energy bias only above `max`.
/// Rec. 709 relative luminance of a linear-light colour. The weights sum to 1,
/// so a neutral grey `(v, v, v)` has luminance exactly `v`.
pub fn luminance(c: Color) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

pub fn clamp_luminance(c: Color, max: f32) -> Color {
    let lum = luminance(c);
    if lum > max && lum > 0.0 {
        c * (max / lum)
    } else {
        c
    }
}

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

#[cfg(test)]
mod clamp_tests {
    use super::*;

    #[test]
    fn below_threshold_is_unchanged() {
        let c = Color::new(1.0, 2.0, 3.0);
        let out = clamp_luminance(c, 100.0);
        assert_eq!(out, c);
    }

    #[test]
    fn above_threshold_is_scaled_to_max_luminance_preserving_hue() {
        let c = Color::new(100.0, 100.0, 100.0); // luminance 100
        let out = clamp_luminance(c, 10.0);
        let lum = 0.2126 * out.x + 0.7152 * out.y + 0.0722 * out.z;
        assert!((lum - 10.0).abs() < 1e-4, "lum={lum}");
        // hue preserved: all channels equal, scaled by 0.1
        assert!((out.x - 10.0).abs() < 1e-4 && (out.y - 10.0).abs() < 1e-4 && (out.z - 10.0).abs() < 1e-4);
    }

    #[test]
    fn infinity_max_disables_clamp() {
        let c = Color::new(500.0, 0.0, 0.0);
        assert_eq!(clamp_luminance(c, f32::INFINITY), c);
    }
}
