use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::texture::{Perlin, Texture};
use crate::vec3::Point3;

/// The pattern a [`NoiseTexture`] draws from its underlying Perlin field.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum NoiseStyle {
    /// Summed |noise| octaves — soft, cloudy blotches. The classic RTOW look.
    Turbulence,
    /// Sine veins phase-shifted by turbulence — stone/marble streaks.
    Marble,
    /// Concentric rings about the Y axis, perturbed by turbulence — wood grain.
    Wood,
    /// A single smooth Perlin octave — gentle, rounded variation.
    Smooth,
}

impl NoiseStyle {
    pub const ALL: [NoiseStyle; 4] = [
        NoiseStyle::Turbulence,
        NoiseStyle::Marble,
        NoiseStyle::Wood,
        NoiseStyle::Smooth,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NoiseStyle::Turbulence => "Turbulence",
            NoiseStyle::Marble => "Marble",
            NoiseStyle::Wood => "Wood",
            NoiseStyle::Smooth => "Smooth",
        }
    }
}

pub struct NoiseTexture {
    noise: Perlin,
    scale: f32,
    depth: u32,
    style: NoiseStyle,
    /// Colour at the pattern's high end (intensity 1); `dark` is the low end (0).
    light: Color,
    dark: Color,
}

impl NoiseTexture {
    pub fn new(scale: f32, depth: u32, style: NoiseStyle, light: Color, dark: Color) -> Self {
        NoiseTexture {
            noise: Perlin::new(),
            scale,
            depth,
            style,
            light,
            dark,
        }
    }

    /// The pattern's intensity at `p`, in [0, 1]: 0 maps to `dark`, 1 to `light`.
    fn intensity(&self, p: &Point3) -> f32 {
        let depth = self.depth.max(1);
        let t = match self.style {
            NoiseStyle::Turbulence => self.noise.turb(&(self.scale * *p), depth),
            NoiseStyle::Marble => {
                // Veins running along Z, bent by turbulence (RTOW's marble).
                0.5 * (1.0 + (self.scale * p.z + 10.0 * self.noise.turb(p, depth)).sin())
            }
            NoiseStyle::Wood => {
                // Rings about the Y axis (radius in the XZ plane), perturbed so
                // the grain wobbles rather than forming perfect circles.
                let radius = (p.x * p.x + p.z * p.z).sqrt();
                0.5 * (1.0 + (self.scale * radius + 6.0 * self.noise.turb(p, depth)).sin())
            }
            // `noise` is ~[-1, 1]; remap to [0, 1].
            NoiseStyle::Smooth => 0.5 * (1.0 + self.noise.noise(&(self.scale * *p))),
        };
        t.clamp(0.0, 1.0)
    }
}

impl Texture for NoiseTexture {
    fn value(&self, _u: f32, _v: f32, p: &Point3) -> Color {
        let t = self.intensity(p);
        self.dark * (1.0 - t) + self.light * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vec3::Point3;

    fn tex(scale: f32, depth: u32) -> NoiseTexture {
        NoiseTexture::new(
            scale,
            depth,
            NoiseStyle::Turbulence,
            Color::new(1.0, 1.0, 1.0),
            Color::new(0.0, 0.0, 0.0),
        )
    }

    #[test]
    fn scale_actually_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = tex(1.0, 7).value(0.0, 0.0, &p);
        let b = tex(10.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "scale had no effect: a={a:?} b={b:?}");
    }

    #[test]
    fn depth_changes_the_sample() {
        let p = Point3::new(1.3, 0.7, 2.1);
        let a = tex(2.0, 1).value(0.0, 0.0, &p);
        let b = tex(2.0, 7).value(0.0, 0.0, &p);
        assert!((a.x - b.x).abs() > 1e-4, "depth had no effect: a={a:?} b={b:?}");
    }

    /// The intensity blends between `dark` and `light`, so a sample stays within
    /// the box those two colours span (per channel).
    #[test]
    fn value_lies_between_dark_and_light() {
        let dark = Color::new(0.1, 0.2, 0.3);
        let light = Color::new(0.9, 0.8, 0.7);
        let t = NoiseTexture::new(3.0, 5, NoiseStyle::Marble, light, dark);
        for p in [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.5, -2.0, 0.7),
            Point3::new(-3.1, 4.2, 1.1),
        ] {
            let c = t.value(0.0, 0.0, &p);
            for (ch, lo, hi) in [
                (c.x, dark.x, light.x),
                (c.y, dark.y, light.y),
                (c.z, dark.z, light.z),
            ] {
                assert!(ch >= lo - 1e-4 && ch <= hi + 1e-4, "{ch} outside [{lo},{hi}]");
            }
        }
    }

    /// Switching style actually changes the field at a representative point.
    #[test]
    fn styles_differ() {
        let p = Point3::new(0.6, 1.2, -0.4);
        let mk = |s| {
            NoiseTexture::new(4.0, 6, s, Color::new(1.0, 1.0, 1.0), Color::new(0.0, 0.0, 0.0))
                .value(0.0, 0.0, &p)
                .x
        };
        let vals = NoiseStyle::ALL.map(mk);
        let all_equal = vals.iter().all(|v| (v - vals[0]).abs() < 1e-6);
        assert!(!all_equal, "all styles produced the same value: {vals:?}");
    }
}
