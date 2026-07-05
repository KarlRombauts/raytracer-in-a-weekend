pub mod common;
pub mod mis;
pub mod sky;

pub use mis::Mis;
pub use sky::Sky;

use rand::rngs::SmallRng;

use crate::camera::config::{CameraConfig, IntegratorKind};
use crate::color::Color;
use crate::group::IntersectGroup;
use crate::ray::Ray;
use crate::sampling::SampleId;
use crate::texture::env_map::load_cached;

/// Estimates the radiance returned along a camera ray — the path tracer, selected
/// per render (see `IntegratorKind`). Returns raw, unclamped radiance; the
/// accumulator (`ProgressiveRenderer`) applies the firefly clamp.
pub trait Integrator: Send + Sync {
    fn radiance(&self, ray: &Ray, world: &IntersectGroup, sample: SampleId, rng: &mut SmallRng) -> Color;
}

/// The sky an integrator sees on a ray miss, from the camera config: the HDR
/// environment map if one is named and loads, else the flat background colour.
fn sky_from(cfg: &CameraConfig) -> Sky {
    match cfg.sky.as_deref().and_then(load_cached) {
        Some(env) => Sky::Env(env),
        None => Sky::Flat(cfg.background),
    }
}

/// Construct the selected integrator from the camera config. Both integrators
/// read the same config, so a Naive-vs-MIS comparison can't disagree on depth or
/// sky.
pub fn build_integrator(cfg: &CameraConfig) -> Box<dyn Integrator> {
    match cfg.integrator {
        IntegratorKind::Mis => Box::new(Mis { max_depth: cfg.max_depth, sky: sky_from(cfg) }),
        // TEMP: the Naive integrator lands in the next slice; until then this arm
        // builds Mis so the factory stays total and compiling.
        IntegratorKind::Naive => Box::new(Mis { max_depth: cfg.max_depth, sky: sky_from(cfg) }),
    }
}
