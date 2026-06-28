use std::sync::Arc;

use crate::{
    color::Color,
    material::Material,
    ray::{HitRecord, Ray},
    texture::{self, SolidColor, Texture},
    vec3::Point3,
};

pub struct DiffuseLight {
    texture: Arc<dyn Texture>,
}

impl DiffuseLight {
    pub fn from_color(albedo: Color) -> Self {
        DiffuseLight {
            texture: Arc::new(SolidColor::from_color(albedo)),
        }
    }
    pub fn from_texture(texture: Arc<dyn Texture>) -> Self {
        DiffuseLight { texture }
    }
}

impl Material for DiffuseLight {
    fn emitted(&self, u: f32, v: f32, p: Point3) -> Color {
        return self.texture.value(u, v, &p);
    }

    fn scatter(
        &self,
        ray: &Ray,
        hit_record: &HitRecord,
        _rng: &mut rand::rngs::SmallRng,
    ) -> Option<(Ray, Color)> {
        None
    }
}
