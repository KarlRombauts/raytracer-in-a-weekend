use std::sync::Arc;

use crate::color::Color;
use crate::texture::env_map::EnvMap;
use crate::vec3::Vec3;

/// The radiance a ray sees when it hits nothing: either a flat background colour
/// or an equirectangular HDR environment map. The umbrella "Sky" concept.
pub enum Sky {
    Flat(Color),
    Env(Arc<EnvMap>),
}

impl Sky {
    /// Radiance along `dir` for a ray that hit nothing.
    pub fn radiance(&self, dir: &Vec3) -> Color {
        match self {
            Sky::Flat(color) => *color,
            Sky::Env(env) => env.sample(dir),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_sky_returns_its_colour_for_any_direction() {
        let sky = Sky::Flat(Color::new(0.2, 0.4, 0.6));
        assert_eq!(sky.radiance(&Vec3::new(0.0, 1.0, 0.0)), Color::new(0.2, 0.4, 0.6));
        assert_eq!(sky.radiance(&Vec3::new(-1.0, 0.0, 0.0)), Color::new(0.2, 0.4, 0.6));
    }
}
