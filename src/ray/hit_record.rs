use crate::{material::Material, ray::Ray, vec3::*};

pub struct HitRecord<'a> {
    pub t: f32,
    pub p: Point3,
    pub normal: Vec3,
    pub front_face: bool,
    pub material: &'a dyn Material,
    pub u: f32,
    pub v: f32,
}

impl<'a> HitRecord<'a> {
    pub fn new(
        t: f32,
        p: Point3,
        normal: Vec3,
        material: &'a dyn Material,
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
