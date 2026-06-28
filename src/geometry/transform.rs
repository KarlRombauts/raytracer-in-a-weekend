use std::sync::Arc;

use crate::interval::Interval;
use crate::ray::{HitRecord, Intersect, Ray, AABB};
use crate::vec3::Vec3;

/// Translates a wrapped object by `offset`. Implemented by moving the ray in
/// the opposite direction, intersecting, then shifting the hit point back.
pub struct Translate {
    object: Arc<dyn Intersect>,
    offset: Vec3,
    bbox: AABB,
}

impl Translate {
    pub fn new(object: Arc<dyn Intersect>, offset: Vec3) -> Self {
        let b = object.bounding_box();
        let bbox = AABB::from_points(b.min_vec() + offset, b.max_vec() + offset);
        Translate {
            object,
            offset,
            bbox,
        }
    }
}

impl Intersect for Translate {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let moved = Ray::new_t(ray.origin - self.offset, ray.direction, ray.time);
        let mut hit = self.object.intersect(&moved, ray_t)?;
        hit.p += self.offset;
        Some(hit)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn center(&self) -> Vec3 {
        self.bbox.center()
    }
}

/// Non-uniform scale (about the origin) by per-axis factors. Because the ray's
/// origin and direction are scaled by the same map, the hit `t` is preserved,
/// so no interval rescaling is needed. Normals use the inverse-transpose.
pub struct Scale {
    object: Arc<dyn Intersect>,
    scale: Vec3,
    inv: Vec3,
    bbox: AABB,
}

impl Scale {
    pub fn new(object: Arc<dyn Intersect>, scale: Vec3) -> Self {
        // Keep factors positive and away from zero to avoid singular maps.
        let scale = Vec3::new(scale.x.max(1e-3), scale.y.max(1e-3), scale.z.max(1e-3));
        let inv = Vec3::new(1.0 / scale.x, 1.0 / scale.y, 1.0 / scale.z);
        let b = object.bounding_box();
        let bbox = AABB::from_points(b.min_vec() * scale, b.max_vec() * scale);
        Scale {
            object,
            scale,
            inv,
            bbox,
        }
    }
}

impl Intersect for Scale {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let scaled = Ray::new_t(ray.origin * self.inv, ray.direction * self.inv, ray.time);
        let mut hit = self.object.intersect(&scaled, ray_t)?;
        hit.p = hit.p * self.scale;
        // Inverse-transpose of diag(scale) is diag(inv); renormalise afterwards.
        hit.normal = (hit.normal * self.inv).unit();
        Some(hit)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn center(&self) -> Vec3 {
        self.bbox.center()
    }
}

/// Euler rotation (degrees) about the X, then Y, then Z axes, through the
/// origin. Stores the forward (object->world) matrix and its inverse (the
/// transpose, since rotations are orthonormal).
pub struct Rotate {
    object: Arc<dyn Intersect>,
    fwd: [Vec3; 3],
    inv: [Vec3; 3],
    bbox: AABB,
}

impl Rotate {
    pub fn new(object: Arc<dyn Intersect>, degrees: Vec3) -> Self {
        let fwd = rotation_matrix(degrees);
        let inv = transpose(&fwd);

        // Bound the rotated object by rotating its 8 bbox corners.
        let min = object.bounding_box().min_vec();
        let max = object.bounding_box().max_vec();
        let mut new_min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
        let mut new_max = Vec3::new(-f32::INFINITY, -f32::INFINITY, -f32::INFINITY);
        for i in 0..2 {
            for j in 0..2 {
                for k in 0..2 {
                    let (fi, fj, fk) = (i as f32, j as f32, k as f32);
                    let corner = Vec3::new(
                        fi * max.x + (1.0 - fi) * min.x,
                        fj * max.y + (1.0 - fj) * min.y,
                        fk * max.z + (1.0 - fk) * min.z,
                    );
                    let r = apply(&fwd, corner);
                    new_min = Vec3::new(new_min.x.min(r.x), new_min.y.min(r.y), new_min.z.min(r.z));
                    new_max = Vec3::new(new_max.x.max(r.x), new_max.y.max(r.y), new_max.z.max(r.z));
                }
            }
        }

        Rotate {
            object,
            fwd,
            inv,
            bbox: AABB::from_points(new_min, new_max),
        }
    }
}

impl Intersect for Rotate {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        // World -> object using the inverse rotation.
        let rotated = Ray::new_t(
            apply(&self.inv, ray.origin),
            apply(&self.inv, ray.direction),
            ray.time,
        );
        let mut hit = self.object.intersect(&rotated, ray_t)?;
        // Object -> world. For orthonormal rotations the normal uses the same
        // forward matrix (inverse-transpose == forward).
        hit.p = apply(&self.fwd, hit.p);
        hit.normal = apply(&self.fwd, hit.normal);
        Some(hit)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }

    fn center(&self) -> Vec3 {
        self.bbox.center()
    }
}

/// Multiply a row-major 3x3 matrix by a vector.
fn apply(m: &[Vec3; 3], v: Vec3) -> Vec3 {
    Vec3::new(m[0].dot(&v), m[1].dot(&v), m[2].dot(&v))
}

fn transpose(m: &[Vec3; 3]) -> [Vec3; 3] {
    [
        Vec3::new(m[0].x, m[1].x, m[2].x),
        Vec3::new(m[0].y, m[1].y, m[2].y),
        Vec3::new(m[0].z, m[1].z, m[2].z),
    ]
}

fn mat_mul(a: &[Vec3; 3], b: &[Vec3; 3]) -> [Vec3; 3] {
    let bt = transpose(b); // rows of bt are columns of b
    [
        Vec3::new(a[0].dot(&bt[0]), a[0].dot(&bt[1]), a[0].dot(&bt[2])),
        Vec3::new(a[1].dot(&bt[0]), a[1].dot(&bt[1]), a[1].dot(&bt[2])),
        Vec3::new(a[2].dot(&bt[0]), a[2].dot(&bt[1]), a[2].dot(&bt[2])),
    ]
}

/// Forward (object->world) rotation matrix for Euler angles `degrees` (x,y,z),
/// applied in X-then-Y-then-Z order (R = Rz * Ry * Rx).
fn rotation_matrix(degrees: Vec3) -> [Vec3; 3] {
    let (sx, cx) = degrees.x.to_radians().sin_cos();
    let (sy, cy) = degrees.y.to_radians().sin_cos();
    let (sz, cz) = degrees.z.to_radians().sin_cos();

    let rx = [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, cx, -sx),
        Vec3::new(0.0, sx, cx),
    ];
    let ry = [
        Vec3::new(cy, 0.0, sy),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-sy, 0.0, cy),
    ];
    let rz = [
        Vec3::new(cz, -sz, 0.0),
        Vec3::new(sz, cz, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ];

    mat_mul(&rz, &mat_mul(&ry, &rx))
}
