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
use crate::vec3::{Point3, Vec3};

pub struct Camera {
    // specified by config
    image_width: u32,
    samples: u32,
    max_depth: u32,
    dof_angle: f32,

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
        let u = config.v_up.cross(&w).unit();
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
                let mut pixel_color = Color::ZERO;
                for _ in 0..self.samples {
                    let ray = self.get_ray(i, j);
                    pixel_color += self.ray_color(&ray, self.max_depth, world);
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

    fn dof_disk_sample(&self) -> Vec3 {
        let p = Vec3::random_in_unit_disk();
        return self.center + (p.x * self.dof_disk_u) + (p.y * self.dof_disk_v);
    }

    fn get_ray(&self, i: u32, j: u32) -> Ray {
        let offset = self.sample_square();
        let pixel_sample = self.pixel00_loc
            + ((i as f32 + offset.x) * self.pixel_delta_u)
            + ((j as f32 + offset.y) * self.pixel_delta_v);

        let ray_origin = if self.dof_angle <= 0. {
            self.center
        } else {
            self.dof_disk_sample()
        };
        let ray_direction = pixel_sample - ray_origin;

        let mut rng = rand::rng();
        let ray_time = rng.random::<f32>();
        Ray::new_t(ray_origin, ray_direction, ray_time)
    }

    fn sample_square(&self) -> Vec3 {
        let mut rng = rand::rng();
        Vec3::new(rng.random::<f32>() - 0.5, rng.random::<f32>() - 0.5, 0.0)
    }

    fn ray_color(&self, ray: &Ray, depth: u32, world: &IntersectGroup) -> Color {
        if depth <= 0 {
            return Color::ZERO;
        }
        // if let Some(hit_record) = world.intersect(ray, &Interval::new(0.0, f32::INFINITY)) {
        //     return 0.5 * (hit_record.normal + Color::new(1.0, 1.0, 1.0));
        // }
        if let Some(hit_record) = world.intersect(ray, &Interval::new(0.001, f32::INFINITY)) {
            if let Some((scattered, attenuation)) = hit_record.material.scatter(ray, &hit_record) {
                return attenuation * self.ray_color(&scattered, depth - 1, world);
            }
            return Color::ZERO;
        }

        let unit_direction = ray.direction.unit();
        let t = 0.5 * (unit_direction.y + 1.0);
        (1.0 - t) * Color::new(1.0, 1.0, 1.0) + t * Color::new(0.5, 0.7, 1.0)
    }
}
