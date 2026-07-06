use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::texture::{
    CheckerTexture, ImageTexture, MappedTexture, NoiseStyle, NoiseTexture, Projection, SolidColor,
    Texture,
};

/// (De)serialize `Arc<[u8]>` as a byte sequence without enabling serde's global
/// `rc` feature. Round-trips through a `Vec<u8>` (a postcard length-prefixed seq).
mod arc_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(bytes: &Arc<[u8]>, s: S) -> Result<S::Ok, S::Error> {
        bytes.as_ref().to_vec().serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<[u8]>, D::Error> {
        Ok(Arc::from(Vec::<u8>::deserialize(d)?))
    }
}

/// An embedded binary asset (image bytes now; meshes in Phase 2). Bytes are the
/// single source of truth, so a scene is self-contained and portable. `label`
/// is for display only (e.g. "earth.png").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    #[serde(with = "arc_bytes")]
    pub bytes: Arc<[u8]>,
    pub label: Option<String>,
}

impl Asset {
    /// An asset with no bytes yet — builds to the magenta placeholder until a
    /// file is chosen in the editor.
    pub fn empty() -> Self {
        Asset {
            bytes: Arc::from([] as [u8; 0]),
            label: None,
        }
    }
}

/// How an image texture's UV coordinates are projected and scaled.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub projection: Projection,
    pub scale: f32,
    pub offset: (f32, f32),
}

impl Default for Mapping {
    fn default() -> Self {
        Mapping {
            projection: Projection::MeshUv,
            scale: 1.0,
            offset: (0.0, 0.0),
        }
    }
}

impl Mapping {
    pub fn is_identity(&self) -> bool {
        self.projection == Projection::MeshUv && self.scale == 1.0 && self.offset == (0.0, 0.0)
    }
}

/// The magenta sentinel used when an image asset fails to decode.
fn magenta() -> Arc<dyn Texture> {
    Arc::new(SolidColor::from_color(Color::new(1.0, 0.0, 1.0)))
}

/// Plain-data description of a texture, mirroring the core `Texture` types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TextureSpec {
    Solid {
        color: Color,
    },
    Checker {
        scale: f32,
        even: CellTexture,
        odd: CellTexture,
    },
    /// Legacy grayscale noise (`white × turbulence`). Retained at its original
    /// position so pre-existing `.scene` files still decode (postcard keys an
    /// enum by declaration order); the editor upgrades it to [`TextureSpec::Noise`]
    /// — an identical-looking white/black turbulence — on first view. New
    /// variants must be appended after this, never inserted before it.
    NoiseLegacy {
        scale: f32,
        depth: u32,
    },
    Image {
        asset: Asset,
        mapping: Mapping,
    },
    /// Coloured procedural noise: a pattern [`style`](NoiseStyle) blended between
    /// `dark` and `light`. Appended last to keep older variant indices stable.
    Noise {
        scale: f32,
        depth: u32,
        style: NoiseStyle,
        light: Color,
        dark: Color,
    },
}

/// A checker cell. Deliberately omits `Checker`, so checker-in-checker
/// recursion is unrepresentable (one level of nesting only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellTexture {
    Solid { color: Color },
    /// Legacy grayscale noise — see [`TextureSpec::NoiseLegacy`]. Kept at its
    /// original index for `.scene` compatibility; new variants append after.
    NoiseLegacy { scale: f32, depth: u32 },
    Image { asset: Asset },
    Noise { scale: f32, depth: u32, style: NoiseStyle, light: Color, dark: Color },
}

fn build_image(asset: &Asset) -> Arc<dyn Texture> {
    match ImageTexture::from_bytes(&asset.bytes) {
        Ok(t) => Arc::new(t),
        Err(_) => magenta(),
    }
}

impl CellTexture {
    fn build(&self) -> Arc<dyn Texture> {
        match self {
            CellTexture::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            CellTexture::Noise { scale, depth, style, light, dark } => {
                Arc::new(NoiseTexture::new(*scale, *depth, *style, *light, *dark))
            }
            CellTexture::NoiseLegacy { scale, depth } => Arc::new(NoiseTexture::new(
                *scale,
                *depth,
                NoiseStyle::Turbulence,
                Color::new(1.0, 1.0, 1.0),
                Color::new(0.0, 0.0, 0.0),
            )),
            CellTexture::Image { asset } => build_image(asset),
        }
    }

    fn preview_color(&self) -> Color {
        match self {
            CellTexture::Solid { color } => *color,
            CellTexture::Noise { light, dark, .. } => (*light + *dark) * 0.5,
            CellTexture::NoiseLegacy { .. } => Color::new(0.5, 0.5, 0.5),
            CellTexture::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

impl TextureSpec {
    /// A bare flat color is just a solid texture.
    pub fn solid(color: Color) -> Self {
        TextureSpec::Solid { color }
    }

    pub fn build(&self) -> Arc<dyn Texture> {
        match self {
            TextureSpec::Solid { color } => Arc::new(SolidColor::from_color(*color)),
            TextureSpec::Checker { scale, even, odd } => Arc::new(CheckerTexture::from_textures(
                *scale,
                even.build(),
                odd.build(),
            )),
            TextureSpec::Noise { scale, depth, style, light, dark } => {
                Arc::new(NoiseTexture::new(*scale, *depth, *style, *light, *dark))
            }
            TextureSpec::NoiseLegacy { scale, depth } => Arc::new(NoiseTexture::new(
                *scale,
                *depth,
                NoiseStyle::Turbulence,
                Color::new(1.0, 1.0, 1.0),
                Color::new(0.0, 0.0, 0.0),
            )),
            TextureSpec::Image { asset, mapping } => {
                let inner = build_image(asset);
                if mapping.is_identity() {
                    inner
                } else {
                    Arc::new(MappedTexture::new(
                        inner,
                        mapping.projection,
                        mapping.scale,
                        mapping.offset,
                    ))
                }
            }
        }
    }

    /// A representative flat color for the rasterized preview and the editor's
    /// type-switch carry-over. Cheap and deterministic — never decodes an image
    /// (the preview runs every frame), so images report a neutral gray.
    pub fn preview_color(&self) -> Color {
        match self {
            TextureSpec::Solid { color } => *color,
            TextureSpec::Checker { even, odd, .. } => {
                (even.preview_color() + odd.preview_color()) * 0.5
            }
            TextureSpec::Noise { light, dark, .. } => (*light + *dark) * 0.5,
            TextureSpec::NoiseLegacy { .. } => Color::new(0.5, 0.5, 0.5),
            TextureSpec::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

#[cfg(test)]
mod texture_spec_tests {
    use super::*;
    use crate::color::Color;
    use crate::vec3::Point3;

    #[test]
    fn solid_builds_and_previews_its_color() {
        let t = TextureSpec::solid(Color::new(0.2, 0.4, 0.6));
        let built = t.build();
        let c = built.value(0.0, 0.0, &Point3::new(0.0, 0.0, 0.0));
        assert!((c.x - 0.2).abs() < 1e-6 && (c.y - 0.4).abs() < 1e-6 && (c.z - 0.6).abs() < 1e-6);
        assert_eq!(t.preview_color(), Color::new(0.2, 0.4, 0.6));
    }

    #[test]
    fn checker_previews_the_average_of_its_cells() {
        let t = TextureSpec::Checker {
            scale: 1.0,
            even: CellTexture::Solid {
                color: Color::new(0.0, 0.0, 0.0),
            },
            odd: CellTexture::Solid {
                color: Color::new(1.0, 1.0, 1.0),
            },
        };
        let _ = t.build(); // builds without panic
        let p = t.preview_color();
        assert!((p.x - 0.5).abs() < 1e-6 && (p.y - 0.5).abs() < 1e-6 && (p.z - 0.5).abs() < 1e-6);
    }

    #[test]
    fn noise_previews_the_average_of_its_colours() {
        let t = TextureSpec::Noise {
            scale: 4.0,
            depth: 7,
            style: NoiseStyle::Turbulence,
            light: Color::new(1.0, 1.0, 1.0),
            dark: Color::new(0.0, 0.0, 0.0),
        };
        let _ = t.build();
        // light=white, dark=black → mid-gray.
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }

    #[test]
    fn bad_image_builds_to_magenta_not_a_panic() {
        let t = TextureSpec::Image {
            asset: Asset {
                bytes: vec![1, 2, 3].into(),
                label: None,
            },
            mapping: Mapping::default(),
        };
        let built = t.build(); // must not panic
        let c = built.value(0.5, 0.5, &Point3::new(0.0, 0.0, 0.0));
        assert_eq!(c, Color::new(1.0, 0.0, 1.0));
        // Image preview is a constant neutral gray (no per-frame decode).
        assert_eq!(t.preview_color(), Color::new(0.5, 0.5, 0.5));
    }
}

#[cfg(test)]
mod mapping_tests {
    use super::*;

    #[test]
    fn default_mapping_is_identity() {
        let m = Mapping::default();
        assert!(m.is_identity());
        assert_eq!(m.projection, crate::texture::Projection::MeshUv);
        assert_eq!(m.scale, 1.0);
        assert_eq!(m.offset, (0.0, 0.0));
    }

    #[test]
    fn non_identity_when_changed() {
        let m = Mapping {
            projection: crate::texture::Projection::Planar,
            scale: 1.0,
            offset: (0.0, 0.0),
        };
        assert!(!m.is_identity());
        let m2 = Mapping {
            scale: 2.0,
            ..Mapping::default()
        };
        assert!(!m2.is_identity());
    }
}

#[cfg(test)]
mod serde_tests {
    use super::*;

    #[test]
    fn checker_texture_round_trips() {
        let t = TextureSpec::Checker {
            scale: 2.5,
            even: CellTexture::Solid { color: Color::new(0.1, 0.2, 0.3) },
            odd: CellTexture::Noise {
                scale: 4.0,
                depth: 7,
                style: NoiseStyle::Marble,
                light: Color::new(0.9, 0.9, 0.9),
                dark: Color::new(0.1, 0.1, 0.1),
            },
        };
        let bytes = postcard::to_allocvec(&t).expect("encode");
        let back: TextureSpec = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(t, back);
    }
}
