use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::material::{Dielectric, DiffuseLight, Glossy, Lambertian, Material, Metal};

use super::TextureSpec;

/// Plain-data description of a material. Built into an `Arc<dyn Material>` only
/// when the world is (re)assembled, so the editor can mutate it freely.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum MaterialSpec {
    Lambertian {
        albedo: TextureSpec,
    },
    Glossy {
        albedo: TextureSpec,
        roughness: f32,
    },
    Metal {
        albedo: Color,
        fuzz: f32,
    },
    Dielectric {
        ior: f32,
        tint: Color,
        roughness: f32,
    },
    DiffuseLight {
        emit: TextureSpec,
    },
}

impl MaterialSpec {
    pub(crate) fn build(&self) -> Arc<dyn Material> {
        match self {
            MaterialSpec::Lambertian { albedo } => {
                Arc::new(Lambertian::from_texture(albedo.build()))
            }
            MaterialSpec::Glossy { albedo, roughness } => {
                Arc::new(Glossy::from_texture(albedo.build(), *roughness))
            }
            MaterialSpec::Metal { albedo, fuzz } => Arc::new(Metal::new(*albedo, *fuzz)),
            MaterialSpec::Dielectric {
                ior,
                tint,
                roughness,
            } => Arc::new(Dielectric::new_glass(*ior, *tint, *roughness)),
            MaterialSpec::DiffuseLight { emit } => {
                Arc::new(DiffuseLight::from_texture(emit.build()))
            }
        }
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;
    use crate::scene::{Asset, Mapping};

    #[test]
    fn material_spec_with_image_asset_round_trips_via_postcard() {
        let m = MaterialSpec::Glossy {
            albedo: TextureSpec::Image {
                asset: Asset {
                    bytes: Arc::from([1u8, 2, 3, 4, 5].as_slice()),
                    label: Some("tex.png".to_string()),
                },
                mapping: Mapping::default(),
            },
            roughness: 0.3,
        };
        let bytes = postcard::to_allocvec(&m).expect("encode");
        let back: MaterialSpec = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(m, back);
    }
}
