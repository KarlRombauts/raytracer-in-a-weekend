use core::f32;
use rand::prelude::*;
use rayon::prelude::*;
use std::time::Instant;

use indicatif::{ProgressBar, ProgressStyle};

use image;

use crate::camera::config::CameraConfig;
use crate::color::Color;
use crate::group::*;
use crate::interval::Interval;
use crate::ray::{Intersect, Ray};
use crate::sampling::stratified_offset;
use crate::vec3::{Point3, Vec3};

pub struct Camera {
    // specified by config
    image_width: u32,
    samples: u32,
    max_depth: u32,
    dof_angle: f32,
    background: Color,

    // derived:
    pixel_samples_scale: f32,
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
        let pixel_samples_scale = 1. / config.samples as f32;
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
            samples: config.samples,
            max_depth: config.max_depth,
            dof_angle: config.dof_angle,
            background: config.background,

            pixel_samples_scale,
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
    pub fn render(&self, world: &IntersectGroup) {
        let start = Instant::now();
        let bar = ProgressBar::new(self.image_height as u64 * self.image_width as u64);
        bar.set_style(
            ProgressStyle::with_template(
                "[{elapsed_precise}] [{bar:40.cyan/blue}] {percent}% ({eta_precise})",
            )
            .unwrap()
            .progress_chars("#>-"),
        );
        let mut img_buf = image::ImageBuffer::new(self.image_width, self.image_height);
        img_buf
            .par_enumerate_pixels_mut()
            .for_each(|(i, j, pixel)| {
                // Per-pixel PRNG, seeded deterministically from the pixel coords so
                // renders are reproducible and threads never share RNG state.
                let mut rng = SmallRng::seed_from_u64(((j as u64) << 32) | i as u64);
                let mut pixel_color = Color::ZERO;
                for s in 0..self.samples {
                    let ray = self.get_ray(i, j, s, &mut rng);
                    pixel_color += self.ray_color(&ray, self.max_depth, world, &mut rng);
                }

                pixel_color *= self.pixel_samples_scale;

                *pixel = image::Rgb(pixel_color.to_rgb_vec());
                bar.inc(1);
            });

        bar.finish();
        let duration = start.elapsed();

        eprintln!(
            "Render complete: {:.3} secs ({}×{} pixels, {} samples)",
            duration.as_secs_f64(),
            self.image_width,
            self.image_height,
            self.samples
        );
        img_buf.save("test.png").unwrap();
    }

    pub fn image_width(&self) -> u32 {
        self.image_width
    }

    pub fn image_height(&self) -> u32 {
        self.image_height
    }

    pub fn samples(&self) -> u32 {
        self.samples
    }

    /// Trace a single sample for pixel (i, j). The progressive renderer calls
    /// this once per pass and averages the results itself.
    pub fn sample_pixel(
        &self,
        i: u32,
        j: u32,
        sample_index: u32,
        world: &IntersectGroup,
        rng: &mut SmallRng,
    ) -> Color {
        let ray = self.get_ray(i, j, sample_index, rng);
        self.ray_color_direct(&ray, world, rng)
    }

    fn dof_disk_sample(&self, rng: &mut SmallRng) -> Vec3 {
        let p = Vec3::random_in_unit_disk(rng);
        return self.center + (p.x * self.dof_disk_u) + (p.y * self.dof_disk_v);
    }

    fn get_ray(&self, i: u32, j: u32, sample_index: u32, rng: &mut SmallRng) -> Ray {
        let (dx, dy) = stratified_offset(i, j, sample_index);
        let pixel_sample = self.pixel00_loc
            + ((i as f32 + dx) * self.pixel_delta_u)
            + ((j as f32 + dy) * self.pixel_delta_v);

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

    /// Direct-lighting integrator: shade a hit by sampling each light with a
    /// shadow ray. No indirect bounce and no PDF/geometry weighting yet — the
    /// contribution is `albedo * emit * cos`, so the result is intentionally
    /// biased (a stepping stone toward full next-event estimation).
    fn ray_color_direct(
        &self,
        ray: &Ray,
        world: &IntersectGroup,
        rng: &mut SmallRng,
    ) -> Color {
        let interval = Interval::new(0.001, f32::INFINITY);
        let Some(hit) = world.intersect(ray, &interval) else {
            return self.background;
        };

        let emitted = hit.material.emitted(hit.u, hit.v, hit.p);

        // No scatter => the surface is a light (or pure absorber); show its emission.
        let Some((_, albedo)) = hit.material.scatter(ray, &hit, rng) else {
            return emitted;
        };

        let mut direct = Color::ZERO;
        for light in &world.lights {
            let lp = light.geom.sample_point(rng);
            let to = lp - hit.p;
            let dist = to.length();
            if dist <= 0.0 {
                continue;
            }
            let dir = to / dist;
            let cos = hit.normal.dot(&dir);
            if cos <= 0.0 {
                continue; // light is behind the surface
            }
            let shadow = Ray::new_t(hit.p, dir, ray.time);
            // Stop just short of the light so its own surface is not an occluder.
            // For a light almost touching the surface (dist < 0.002) this interval
            // inverts (max < min); that is safe — every `contains` is then false,
            // so the light simply counts as unoccluded. Do not "fix" it into a panic.
            let shadow_interval = Interval::new(0.001, dist - 0.001);
            if world.intersect(&shadow, &shadow_interval).is_none() {
                direct += albedo * light.emit * cos;
            }
        }

        emitted + direct
    }

    #[allow(dead_code)]
    fn ray_color(
        &self,
        ray: &Ray,
        depth: u32,
        world: &IntersectGroup,
        rng: &mut SmallRng,
    ) -> Color {
        // Iterative path tracing: accumulate emission weighted by the running
        // product of attenuations (throughput). Equivalent to the recursive
        // `emit + attenuation * ray_color(...)` but without per-bounce stack
        // frames, keeping the loop state in registers.
        let interval = Interval::new(0.001, f32::INFINITY);
        let mut color = Color::ZERO;
        let mut throughput = Color::ones();
        let mut current = ray.clone();

        for _ in 0..depth {
            let Some(hit) = world.intersect(&current, &interval) else {
                color += throughput * self.background;
                return color;
            };

            color += throughput * hit.material.emitted(hit.u, hit.v, hit.p);

            match hit.material.scatter(&current, &hit, rng) {
                Some((scattered, attenuation)) => {
                    throughput = throughput * attenuation;
                    current = scattered;
                }
                None => return color,
            }
        }

        // Depth exhausted: matches the recursive base case `ray_color(_, 0) == 0`,
        // contributing nothing further.
        color
    }
}

#[cfg(test)]
mod direct_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::geometry::Quad;
    use crate::group::{IntersectGroup, Light};
    use crate::material::{DiffuseLight, Lambertian};
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    fn test_camera() -> Camera {
        Camera::from(
            CameraConfig::builder()
                .image_width(1)
                .aspect_ratio(1.0)
                .background(Color::ZERO)
                .build(),
        )
    }

    fn floor() -> Arc<Quad> {
        let mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 0.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    fn light_quad() -> Arc<Quad> {
        let mat = Arc::new(DiffuseLight::from_color(Color::new(5.0, 5.0, 5.0)));
        Arc::new(Quad::new(
            Point3::new(-1.0, 2.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            mat,
        ))
    }

    #[test]
    fn occluded_point_is_darker_than_lit_point() {
        let cam = test_camera();
        // Camera ray straight down onto the floor centre at (0,0,0).
        let ray = Ray::new(Point3::new(0.0, 1.0, 0.0), Vec3::new(0.0, -1.0, 0.0));

        let mut lit = IntersectGroup::new();
        lit.add(floor());
        lit.add(light_quad());
        lit.lights.push(Light { geom: light_quad(), emit: Color::new(5.0, 5.0, 5.0) });

        let mut rng = SmallRng::seed_from_u64(1);
        let lit_color = cam.ray_color_direct(&ray, &lit, &mut rng);
        assert!(lit_color.x > 0.0, "expected lit floor, got {:?}", lit_color);

        // Same scene plus a blocker quad between the floor and the light.
        let mut blocked = IntersectGroup::new();
        blocked.add(floor());
        blocked.add(light_quad());
        let blocker_mat = Arc::new(Lambertian::from_color(Color::new(1.0, 1.0, 1.0)));
        blocked.add(Arc::new(Quad::new(
            Point3::new(-1.0, 1.0, -1.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 2.0),
            blocker_mat,
        )));
        blocked.lights.push(Light { geom: light_quad(), emit: Color::new(5.0, 5.0, 5.0) });
        let occ_color = cam.ray_color_direct(&ray, &blocked, &mut rng);
        assert!(
            occ_color.x < lit_color.x,
            "occluded should be darker: lit {:?} occ {:?}",
            lit_color,
            occ_color
        );
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
