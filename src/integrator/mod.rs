pub mod common;
pub mod mis;
pub mod naive;
pub mod sky;

pub use mis::Mis;
pub use naive::Naive;
pub use sky::Sky;

use rand::rngs::SmallRng;

use crate::camera::config::{CameraConfig, IntegratorKind};
use crate::color::Color;
use crate::world::World;
use crate::ray::Ray;
use crate::sampling::SampleId;

/// Estimates the radiance returned along a camera ray — the path tracer, selected
/// per render (see `IntegratorKind`). Returns raw, unclamped radiance; the
/// accumulator (`ProgressiveRenderer`) applies the firefly clamp. The sky (both
/// its miss-radiance and its importance-sampled light) is owned by the World, not
/// the integrator — so both integrators see one source of truth.
pub trait Integrator: Send + Sync {
    fn radiance(&self, ray: &Ray, world: &World, sample: SampleId, rng: &mut SmallRng) -> Color;
}

/// Construct the selected integrator from the camera config.
pub fn build_integrator(cfg: &CameraConfig) -> Box<dyn Integrator> {
    match cfg.integrator {
        IntegratorKind::Mis => Box::new(Mis { max_depth: cfg.max_depth }),
        IntegratorKind::Naive => Box::new(Naive { max_depth: cfg.max_depth }),
    }
}
