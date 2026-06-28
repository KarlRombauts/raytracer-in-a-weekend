use std::sync::Arc;

use crate::{
    interval::Interval,
    material::{material, Material},
    ray::{HitRecord, Intersect, Ray, AABB},
    vec3::{Point3, Vec3},
};
use rand::rngs::SmallRng;
use rand::Rng;

pub struct Triangle {
    centroid: Vec3,
    q: Point3,
    u: Vec3,
    v: Vec3,
    d: f32,
    w: Vec3,
    normal: Vec3,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Triangle {
    pub fn from_points(p1: &Vec3, p2: &Vec3, p3: &Vec3, material: Arc<dyn Material>) -> Self {
        let u = *p2 - *p1;
        let v = *p3 - *p1;
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(p1);
        let centroid = (*p1 + *p2 + *p3) / 3.;

        let mut triangle = Triangle {
            centroid,
            q: *p1,
            u,
            v,
            d,
            w: n / n.dot(&n),
            normal,
            material,
            bbox: AABB::EMPTY,
        };
        triangle.set_bounding_box();
        triangle
    }

    pub fn new(q: Point3, u: Vec3, v: Vec3, material: Arc<dyn Material>) -> Self {
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(&q);

        let centroid = (q + (q + u) + (q + v)) / 3.;

        let mut triangle = Triangle {
            centroid,
            q,
            u,
            v,
            d,
            w: n / n.dot(&n),
            normal,
            material,
            bbox: AABB::EMPTY,
        };
        triangle.set_bounding_box();
        triangle
    }

    fn set_bounding_box(&mut self) {
        let bbox_diagonal1 = AABB::from_points(self.q, self.q + self.u + self.v);
        let bbox_diagonal2 = AABB::from_points(self.q + self.u, self.q + self.v);
        self.bbox = AABB::from_boxes(&bbox_diagonal1, &bbox_diagonal2);
    }

    fn is_interior(a: f32, b: f32) -> bool {
        a > 0. && b > 0. && a + b < 1.
    }
}

impl Intersect for Triangle {
    fn center(&self) -> Vec3 {
        self.centroid
    }

    fn intersect(
        &self,
        ray: &Ray,
        ray_t: &crate::interval::Interval,
    ) -> Option<crate::ray::HitRecord<'_>> {
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

        // Check that the hit point lies inside the triangle region
        let p = ray.at(t);
        let planar_hit_point = p - self.q;
        let alpha = self.w.dot(&planar_hit_point.cross(&self.v));
        let beta = self.w.dot(&self.u.cross(&planar_hit_point));

        if !Self::is_interior(alpha, beta) {
            return None;
        }

        let mut hit_record = HitRecord::new(t, p, Vec3::ZERO, self.material.as_ref());
        hit_record.set_face_normal(ray, &self.normal);
        hit_record.u = alpha;
        hit_record.v = beta;
        Some(hit_record)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn sample_point(&self, rng: &mut SmallRng) -> crate::vec3::Point3 {
        let r1: f32 = rng.random();
        let r2: f32 = rng.random();
        let su = r1.sqrt();
        let p1 = self.q;
        let p2 = self.q + self.u;
        let p3 = self.q + self.v;
        (1.0 - su) * p1 + (su * (1.0 - r2)) * p2 + (su * r2) * p3
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_inside_triangle() {
        // Vertices q=(0,0,0), q+u=(1,0,0), q+v=(0,1,0): a sample (x,y,0) must
        // satisfy the barycentric bounds x>=0, y>=0, x+y<=1.
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        );
        let mut rng = SmallRng::seed_from_u64(2);
        for _ in 0..500 {
            let p = tri.sample_point(&mut rng);
            assert!(p.z.abs() < 1e-5, "off-plane: {:?}", p);
            assert!(p.x >= -1e-5 && p.y >= -1e-5, "negative bary: {:?}", p);
            assert!(p.x + p.y <= 1.0 + 1e-5, "outside tri: {:?}", p);
        }
    }
}
