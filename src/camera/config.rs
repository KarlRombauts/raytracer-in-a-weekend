use crate::{color::Color, vec3::Vec3};
use serde::{Deserialize, Serialize};
use typed_builder::TypedBuilder;

/// Which integration algorithm renders the scene. A render *method*, not scene
/// content — kept out of the `.scene` wire format via `#[serde(skip)]` on the
/// field below (runtime only, defaults to `Mis` on load), mirroring `sky`. When
/// `CameraConfig` is later split into lens + render settings (see
/// `.scratch/render-settings-split/`), this moves with the render settings.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum IntegratorKind {
    #[default]
    Mis,
    Naive,
}

impl IntegratorKind {
    /// Every variant, for populating the editor's integrator picker.
    pub const ALL: [IntegratorKind; 2] = [IntegratorKind::Mis, IntegratorKind::Naive];

    /// Human-readable label for the picker.
    pub fn label(self) -> &'static str {
        match self {
            IntegratorKind::Mis => "MIS (low noise)",
            IntegratorKind::Naive => "Naive",
        }
    }
}

#[derive(TypedBuilder, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraConfig {
    #[builder(default = 16.0 / 9.0)]
    pub aspect_ratio: f64,

    #[builder(default = 400)]
    pub image_width: u32,

    #[builder(default = 100)]
    pub samples: u32,

    #[builder(default = 10)]
    pub max_depth: u32,

    #[builder(default = 60.0)]
    pub fov: f32,

    #[builder(default = 0.0)]
    pub roll: f32,

    #[builder(default = 0.)]
    pub dof_angle: f32,

    #[builder(default = 10.)]
    pub focus_dist: f32,

    #[builder(default = Vec3::new(0., 0., 0.))]
    pub look_from: Vec3,

    #[builder(default = Vec3::new(0., 0., -1.))]
    pub look_at: Vec3,

    #[builder(default = Vec3::new(0., 1., 0.))]
    pub v_up: Vec3,

    #[builder(default = Color::new(0.7, 0.8, 1.))]
    pub background: Color,

    /// Name of a bundled HDR sky (`assets/hdrs/<name>.hdr`) used in place of the
    /// flat `background`; `None` keeps the solid colour. `serde(skip)` for now so
    /// adding it doesn't change the `.scene` wire format (old scenes still load) —
    /// it's a runtime selection until a versioned scene format persists it.
    #[builder(default)]
    #[serde(skip)]
    pub sky: Option<String>,

    /// Render method (Naive vs MIS). Runtime-only; see [`IntegratorKind`]. Skipped
    /// in the wire format so adding it doesn't change the `.scene` bytes.
    #[builder(default)]
    #[serde(skip)]
    pub integrator: IntegratorKind,

    /// Firefly suppression: per-sample luminance cap. `f32::INFINITY` disables it.
    #[builder(default = f32::INFINITY)]
    pub firefly_clamp: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_config_postcard_round_trip() {
        let original = CameraConfig::builder()
            .aspect_ratio(4.0 / 3.0)
            .image_width(800)
            .samples(50)
            .max_depth(20)
            .fov(45.0)
            .roll(0.1)
            .dof_angle(0.5)
            .focus_dist(5.0)
            .look_from(Vec3::new(1.0, 2.0, 3.0))
            .look_at(Vec3::new(0.0, 0.5, -1.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .background(Color::new(0.1, 0.2, 0.3))
            .firefly_clamp(10.0)
            .build();

        let bytes = postcard::to_allocvec(&original).expect("postcard serialise");
        let decoded: CameraConfig = postcard::from_bytes(&bytes).expect("postcard deserialise");
        assert_eq!(original, decoded);
    }

    #[test]
    fn integrator_is_skipped_in_the_wire_format_and_defaults_to_mis() {
        let mis = CameraConfig::builder().build();
        let naive = CameraConfig::builder().integrator(IntegratorKind::Naive).build();
        assert_eq!(mis.integrator, IntegratorKind::Mis, "default is Mis");
        assert_eq!(naive.integrator, IntegratorKind::Naive);

        // #[serde(skip)]: the field never reaches the bytes, so a Mis and a Naive
        // config encode identically — the wire format is untouched — and a decode
        // fills it from Default (Mis). This is what keeps old .scene files loading.
        let mis_bytes = postcard::to_allocvec(&mis).unwrap();
        let naive_bytes = postcard::to_allocvec(&naive).unwrap();
        assert_eq!(mis_bytes, naive_bytes, "integrator must not affect the wire format");
        let decoded: CameraConfig = postcard::from_bytes(&naive_bytes).unwrap();
        assert_eq!(decoded.integrator, IntegratorKind::Mis, "skipped field decodes to default");
    }
}
