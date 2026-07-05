use rand::prelude::*;

use crate::camera::config::CameraConfig;
use crate::ray::Ray;
use crate::sampling::{stratified_offset, SampleId};
use crate::vec3::{Point3, Vec3};

/// The lens: turns a [`SampleId`] into a world-space [`Ray`]. Ray generation only
/// — depth-of-field, the viewport basis, and sub-pixel AA (stratification dim 0).
/// Radiance estimation lives in the [`Integrator`](crate::integrator::Integrator).
pub struct Camera {
    // specified by config
    image_width: u32,
    dof_angle: f32,

    // derived:
    image_height: u32,
    center: Point3,
    pixel00_loc: Point3,
    pixel_delta_u: Vec3,
    pixel_delta_v: Vec3,
    u: Vec3,
    v: Vec3,
    w: Vec3,

    dof_disk_u: Vec3,
    dof_disk_v: Vec3,
    #[allow(dead_code)]
    basis_u: Vec3,
}

impl From<CameraConfig> for Camera {
    fn from(config: CameraConfig) -> Self {
        // derived
        let image_height = ((config.image_width as f64 / config.aspect_ratio) as u32).max(1);
        let center = config.look_from;

        let theta = config.fov.to_radians();
        let h = (theta / 2.0).tan();
        let viewport_height = 2.0 * h * config.focus_dist;
        let viewport_width = viewport_height * (config.image_width as f32) / (image_height as f32);

        let w = (config.look_from - config.look_at).unit();
        // Roll spins the up reference about the view axis before deriving right.
        let up = config.v_up.rotate_about_axis(&w, config.roll.to_radians());
        let u = up.cross(&w).unit();
        let v = w.cross(&u);

        let viewport_u = viewport_width * u;
        let viewport_v = viewport_height * -v;

        let pixel_delta_u = viewport_u / config.image_width as f32;
        let pixel_delta_v = viewport_v / image_height as f32;

        let viewport_upper_left =
            center - (config.focus_dist * w) - (viewport_u / 2.0) - (viewport_v / 2.0);

        let dof_radius = config.focus_dist * (config.dof_angle / 2.).to_radians().tan();
        let pixel00_loc = viewport_upper_left + 0.5 * (pixel_delta_v + pixel_delta_u);
        let dof_disk_u = u * dof_radius;
        let dof_disk_v = v * dof_radius;

        Camera {
            image_width: config.image_width,
            dof_angle: config.dof_angle,

            image_height,
            center,
            pixel00_loc,
            pixel_delta_u,
            pixel_delta_v,
            u,
            v,
            w,
            dof_disk_u,
            dof_disk_v,
            basis_u: u,
        }
    }
}

impl Camera {
    pub fn image_width(&self) -> u32 {
        self.image_width
    }

    pub fn image_height(&self) -> u32 {
        self.image_height
    }

    fn dof_disk_sample(&self, rng: &mut SmallRng) -> Vec3 {
        let p = Vec3::random_in_unit_disk(rng);
        return self.center + (p.x * self.dof_disk_u) + (p.y * self.dof_disk_v);
    }

    /// The camera ray for one sample: a jittered ray through the pixel, with the
    /// sub-pixel offset drawn from stratification dim 0 and (when enabled) a
    /// depth-of-field origin on the lens disk.
    pub fn get_ray(&self, sample: SampleId, rng: &mut SmallRng) -> Ray {
        let (dx, dy) = stratified_offset(sample.i, sample.j, sample.index);
        let pixel_sample = self.pixel00_loc
            + ((sample.i as f32 + dx) * self.pixel_delta_u)
            + ((sample.j as f32 + dy) * self.pixel_delta_v);

        let ray_origin = if self.dof_angle <= 0. {
            self.center
        } else {
            self.dof_disk_sample(rng)
        };
        let ray_direction = pixel_sample - ray_origin;

        let ray_time = rng.random::<f32>();
        Ray::new_t(ray_origin, ray_direction, ray_time)
    }

    #[cfg(test)]
    pub(crate) fn basis_u(&self) -> crate::vec3::Vec3 {
        self.basis_u
    }
}

#[cfg(test)]
mod roll_tests {
    use super::Camera;
    use crate::camera::CameraConfig;
    use crate::vec3::Vec3;

    fn cfg(roll: f32) -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 0.0))
            .look_at(Vec3::new(0.0, 0.0, -1.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .roll(roll)
            .build()
    }

    #[test]
    fn zero_roll_keeps_upright_basis() {
        // With no roll, the right axis u should be world +x (within sign tol).
        let cam = Camera::from(cfg(0.0));
        assert!((cam.basis_u().x.abs() - 1.0).abs() < 1e-5, "u={:?}", cam.basis_u());
        assert!(cam.basis_u().y.abs() < 1e-5);
    }

    #[test]
    fn ninety_roll_tilts_right_axis_to_vertical() {
        // Rolling 90° should swing the right axis onto the world vertical.
        let cam = Camera::from(cfg(90.0));
        assert!(cam.basis_u().y.abs() > 0.99, "u={:?}", cam.basis_u());
    }
}
