use std::sync::Arc;

use crate::{
    interval::Interval,
    material::{material, Material},
    ray::{HitRecord, Intersect, Ray, AABB},
    vec3::{Point3, Vec3},
};

pub struct Quad {
    q: Point3,
    u: Vec3,
    v: Vec3,
    d: f32,
    w: Vec3,
    normal: Vec3,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Quad {
    pub fn new(q: Point3, u: Vec3, v: Vec3, material: Arc<dyn Material>) -> Self {
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(&q);

        let mut quad = Quad {
            q,
            u,
            v,
            d,
            w: n / n.dot(&n),
            normal,
            material,
            bbox: AABB::EMPTY,
        };
        quad.set_bounding_box();
        quad
    }

    fn set_bounding_box(&mut self) {
        let bbox_diagonal1 = AABB::from_points(self.q, self.q + self.u + self.v);
        let bbox_diagonal2 = AABB::from_points(self.q + self.u, self.q + self.v);
        self.bbox = AABB::from_boxes(&bbox_diagonal1, &bbox_diagonal2);
    }

    fn is_interior(a: f32, b: f32) -> bool {
        let unit = Interval::new(0., 1.);
        unit.contains(a) && unit.contains(b)
    }
}

impl Intersect for Quad {
    fn intersect(
        &self,
        ray: &Ray,
        ray_t: &crate::interval::Interval,
    ) -> Option<crate::ray::HitRecord> {
        let denom = self.normal.dot(&ray.direction);

        // No hit if the ray is parallel to the plane
        if denom.abs() < 1e-8 {
            return None;
        }

        let t = (self.d - self.normal.dot(&ray.origin)) / denom;
        // Return false if the hit point parameter t is outside the ray interval
        if !ray_t.contains(t) {
            return None;
        }

        // Check that the hit point lies inside the quad region
        let p = ray.at(t);
        let planar_hit_point = p - self.q;
        let alpha = self.w.dot(&planar_hit_point.cross(&self.v));
        let beta = self.w.dot(&self.u.cross(&planar_hit_point));

        if !Self::is_interior(alpha, beta) {
            return None;
        }

        let mut hit_record = HitRecord::new(t, p, Vec3::ZERO, self.material.clone());
        hit_record.set_face_normal(ray, &self.normal);
        hit_record.u = alpha;
        hit_record.v = beta;
        Some(hit_record)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }
}
