pub mod blank;
pub mod dielectric;
pub mod diffuse_light;
pub mod glossy;
pub mod lambertian;
pub mod material;
pub mod metal;

pub use blank::Blank;
pub use dielectric::Dielectric;
pub use diffuse_light::*;
pub use glossy::Glossy;
pub use lambertian::Lambertian;
pub use material::Material;
pub use metal::Metal;

/// Returns a throwaway `Arc<dyn Material>` suitable for geometry-only queries
/// (e.g. computing a bounding box) where the actual material is irrelevant.
pub fn blank_material() -> std::sync::Arc<dyn Material> {
    std::sync::Arc::new(Blank::new())
}
