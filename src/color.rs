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
    // Drop non-finite contributions outright. A single NaN/Inf sample folded into
    // a pixel's running mean poisons it permanently (NaN is sticky, and the
    // adaptive convergence test never fires on it) — it shows up as a frozen
    // black speck. This per-sample point is where such paths get sanitized.
    if !c.x.is_finite() || !c.y.is_finite() || !c.z.is_finite() {
        return Color::ZERO;
    }
    let lum = luminance(c);
    if lum > max && lum > 0.0 {
        c * (max / lum)
    } else {
        c
    }
}

/// Exposure multiplier applied before tone mapping (stops of light; 1.0 = none).
pub const EXPOSURE: f32 = 1.0;

/// ACES filmic tone-map (Narkowicz fit). Maps open-domain linear radiance
/// `[0, ∞)` into `[0, 1]` with a gentle toe and a smooth highlight shoulder, so
/// bright speculars and emitters roll off like film instead of clipping flat to
/// white. The S-curve also adds a little contrast/punch through the mid-tones.
fn aces(x: f32) -> f32 {
    let (a, b, c, d, e) = (2.51, 0.03, 2.43, 0.59, 0.14);
    ((x * (a * x + b)) / (x * (c * x + d) + e)).clamp(0.0, 1.0)
}

/// Linear HDR radiance → display-ready value in `[0, 1]`: exposure, then ACES
/// tone mapping, then the gamma (OETF) encode. Non-finite / negative input → 0.
fn tonemap_channel(linear: f32) -> f32 {
    if !(linear > 0.0) {
        return 0.0; // NaN (NaN > 0 is false) and non-positive → black
    }
    if linear.is_infinite() {
        return 1.0; // an infinite highlight saturates to white, not NaN→black
    }
    aces(linear * EXPOSURE).sqrt()
}

impl Color {
    pub fn to_rgb_vec(&self) -> [u8; 3] {
        [
            (tonemap_channel(self.x) * 255.0) as u8,
            (tonemap_channel(self.y) * 255.0) as u8,
            (tonemap_channel(self.z) * 255.0) as u8,
        ]
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

    #[test]
    fn tonemap_rolls_off_highlights_without_hard_clipping() {
        // Black stays black.
        assert_eq!(Color::ZERO.to_rgb_vec(), [0, 0, 0]);
        // A bright (>1) highlight rolls off near — but below — pure white, and
        // brighter still gets closer to white: a smooth shoulder, not a hard clip.
        let bright = Color::new(4.0, 4.0, 4.0).to_rgb_vec()[0];
        let brighter = Color::new(40.0, 40.0, 40.0).to_rgb_vec()[0];
        assert!(bright > 230 && bright < 255, "4.0 -> {bright}");
        assert!(brighter > bright, "40.0 ({brighter}) should exceed 4.0 ({bright})");
        assert!(brighter >= 250, "very bright should approach white: {brighter}");
        // Monotonic across the mid-tones.
        let a = Color::new(0.2, 0.2, 0.2).to_rgb_vec()[0];
        let b = Color::new(0.5, 0.5, 0.5).to_rgb_vec()[0];
        assert!(b > a, "mid-tones must be monotonic: {a} !< {b}");
    }

    #[test]
    fn tonemap_handles_non_finite_as_black_not_garbage() {
        // The mean is sanitized upstream, but the encoder must never emit garbage.
        assert_eq!(Color::new(f32::NAN, 0.5, 0.5).to_rgb_vec()[0], 0);
        // Inf is an extreme highlight -> white, not 0.
        assert_eq!(Color::new(f32::INFINITY, 0.0, 0.0).to_rgb_vec()[0], 255);
    }

    #[test]
    fn non_finite_samples_are_sanitized_to_zero() {
        // A NaN or Inf path contribution must not survive: folded into a pixel's
        // running mean it would poison it permanently and freeze it black. This
        // is the per-sample sanitization point, so it's where they get dropped.
        let nan = Color::new(f32::NAN, 0.5, 0.5);
        let inf = Color::new(f32::INFINITY, 0.0, 0.0);
        for max in [10.0_f32, f32::INFINITY] {
            assert_eq!(clamp_luminance(nan, max), Color::ZERO, "max={max}");
            assert_eq!(clamp_luminance(inf, max), Color::ZERO, "max={max}");
        }
        // The old failure mode: Inf luminance * (finite max / Inf) = Inf*0 = NaN.
        let out = clamp_luminance(inf, 10.0);
        assert!(out.x.is_finite() && out.y.is_finite() && out.z.is_finite());
    }
}
