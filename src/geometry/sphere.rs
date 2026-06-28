use std::f32;
use std::sync::Arc;

use crate::interval::Interval;
use crate::material::Material;
use crate::ray::*;
use crate::vec3::{Point3, Vec3};
use rand::rngs::SmallRng;

pub struct Sphere {
    pub center: Ray,
    pub radius: f32,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Sphere {
    pub fn stationary(center: Point3, radius: f32, material: Arc<dyn Material>) -> Self {
        let rvec = Vec3::new(radius, radius, radius);
        Sphere {
            center: Ray::new(center, Vec3::ZERO), //: Ray::new(center, Vec3::ZERO),
            radius,
            material,
            bbox: AABB::from_points(center - rvec, center + rvec),
        }
    }

    pub fn moving(
        center1: Point3,
        center2: Point3,
        radius: f32,
        material: Arc<dyn Material>,
    ) -> Self {
        let rvec = Vec3::new(radius, radius, radius);
        let box1 = AABB::from_points(center1 - rvec, center1 + rvec);
        let box2 = AABB::from_points(center2 - rvec, center2 + rvec);
        let bbox = AABB::from_boxes(&box1, &box2);

        Sphere {
            center: Ray::new(center1, center2 - center1), //: Ray::new(center, Vec3::ZERO),
            radius,
            material,
            bbox,
        }
    }

    pub fn get_spherical_uv(&self, p: &Point3) -> (f32, f32) {
        let theta = (-p.y).acos();
        let phi = f32::atan2(-p.z, p.x) + f32::consts::PI;

        let u = phi / (2. * f32::consts::PI);
        let v = theta / f32::consts::PI;
        (u, v)
    }
}

impl Intersect for Sphere {
    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn center(&self) -> Vec3 {
        self.bbox.center()
    }

    fn sample_point(&self, rng: &mut SmallRng) -> crate::vec3::Point3 {
        let center = self.center.at(0.0);
        center + self.radius * Vec3::random_unit(rng)
    }

    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let current_center = self.center.at(ray.time);
        let oc = ray.origin - current_center;
        let a = ray.direction.length_squared();
        let half_b = oc.dot(&ray.direction);
        let c = oc.length_squared() - self.radius * self.radius;
        let discriminant = half_b * half_b - a * c;

        if discriminant < 0.0 {
            return None;
        }

        let sqrt_d = discriminant.sqrt();
        let mut root = (-half_b - sqrt_d) / a;

        if !ray_t.surrounds(root) {
            root = (-half_b + sqrt_d) / a;
            if !ray_t.surrounds(root) {
                return None;
            }
        }

        let p = ray.at(root);
        let outward_normal = (p - current_center).unit();
        let mut hit_record = HitRecord::new(root, p, outward_normal, self.material.as_ref());
        (hit_record.u, hit_record.v) = self.get_spherical_uv(&outward_normal);
        hit_record.set_face_normal(ray, &outward_normal);
        Some(hit_record)
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::Point3;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_on_sphere_surface() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let center = Point3::new(1.0, 2.0, 3.0);
        let radius = 5.0;
        let s = Sphere::stationary(center, radius, mat);
        let mut rng = SmallRng::seed_from_u64(3);
        for _ in 0..500 {
            let p = s.sample_point(&mut rng);
            let r = (p - center).length();
            assert!((r - radius).abs() < 1e-3, "off-surface: r={}", r);
        }
    }
}
