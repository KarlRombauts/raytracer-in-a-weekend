use std::sync::Arc;

use crate::{material::Material, ray::Ray, vec3::*};

pub struct HitRecord {
    pub t: f32,
    pub p: Point3,
    pub normal: Vec3,
    pub front_face: bool,
    pub material: Arc<dyn Material>,
    pub u: f32,
    pub v: f32,
}

impl HitRecord {
    pub fn new(
        t: f32,
        p: Point3,
        normal: Vec3,
        material: Arc<dyn Material>,
        // u: f32,
        // v: f32,
    ) -> Self {
        HitRecord {
            t,
            p,
            normal,
            front_face: true,
            material,
            u: 0.,
            v: 0.,
        }
    }

    pub fn set_face_normal(&mut self, ray: &Ray, outward_normal: &Vec3) {
        self.front_face = ray.direction.dot(outward_normal) < 0.0;
        self.normal = if self.front_face {
            *outward_normal
        } else {
            -*outward_normal
        };
    }
}
