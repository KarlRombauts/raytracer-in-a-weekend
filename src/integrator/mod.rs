pub mod common;
pub mod mis;
pub mod sky;

pub use mis::Mis;
pub use sky::Sky;

use rand::rngs::SmallRng;

use crate::color::Color;
use crate::group::IntersectGroup;
use crate::ray::Ray;
use crate::sampling::SampleId;

/// Estimates the radiance returned along a camera ray — the path tracer, selected
/// per render (see `IntegratorKind`). Returns raw, unclamped radiance; the
/// accumulator (`ProgressiveRenderer`) applies the firefly clamp.
pub trait Integrator: Send + Sync {
    fn radiance(&self, ray: &Ray, world: &IntersectGroup, sample: SampleId, rng: &mut SmallRng) -> Color;
}
