//! The `.scene` container: an lz4-compressed postcard blob behind a 4-byte
//! magic header. Pure-Rust codec so it builds for both native and wasm.

use serde::{Deserialize, Serialize};

use crate::scene::Scene;

const MAGIC: &[u8; 4] = b"RTSC";
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct SceneFile {
    version: u32,
    name: Option<String>,
    scene: Scene,
    preview: Vec<u8>,
}

/// A decoded scene plus its metadata.
pub struct LoadedScene {
    pub scene: Scene,
    pub name: Option<String>,
    pub preview: Vec<u8>,
}

#[derive(Debug)]
pub enum SceneFileError {
    BadMagic,
    UnsupportedVersion(u32),
    Decompress,
    Decode,
}

impl std::fmt::Display for SceneFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SceneFileError::BadMagic => write!(f, "not a .scene file"),
            SceneFileError::UnsupportedVersion(v) => write!(f, "unsupported .scene version {v}"),
            SceneFileError::Decompress => write!(f, "could not decompress"),
            SceneFileError::Decode => write!(f, "could not decode scene"),
        }
    }
}

/// Encode a scene (with optional name + preview PNG bytes) into `.scene` bytes.
pub fn encode(scene: &Scene, name: Option<&str>, preview: &[u8]) -> Vec<u8> {
    let file = SceneFile {
        version: VERSION,
        name: name.map(str::to_string),
        scene: scene.clone(),
        preview: preview.to_vec(),
    };
    let raw = postcard::to_allocvec(&file).expect("postcard encode");
    let compressed = lz4_flex::compress_prepend_size(&raw);
    let mut out = Vec::with_capacity(MAGIC.len() + compressed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&compressed);
    out
}

/// Decode `.scene` bytes. Never panics — malformed input returns `Err`.
pub fn decode(bytes: &[u8]) -> Result<LoadedScene, SceneFileError> {
    if bytes.len() < MAGIC.len() || &bytes[..MAGIC.len()] != MAGIC {
        return Err(SceneFileError::BadMagic);
    }
    let raw = lz4_flex::decompress_size_prepended(&bytes[MAGIC.len()..])
        .map_err(|_| SceneFileError::Decompress)?;
    let file: SceneFile = postcard::from_bytes(&raw).map_err(|_| SceneFileError::Decode)?;
    if file.version != VERSION {
        return Err(SceneFileError::UnsupportedVersion(file.version));
    }
    Ok(LoadedScene { scene: file.scene, name: file.name, preview: file.preview })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::scene::Scene;

    fn empty_scene() -> Scene {
        Scene { camera: CameraConfig::builder().image_width(32).build(), objects: vec![] }
    }

    #[test]
    fn round_trips_scene_name_and_preview() {
        let scene = empty_scene();
        let preview = vec![1u8, 2, 3, 4];
        let bytes = encode(&scene, Some("My Scene"), &preview);
        let loaded = decode(&bytes).expect("decode");
        assert_eq!(loaded.name.as_deref(), Some("My Scene"));
        assert_eq!(loaded.preview, preview);
        assert_eq!(loaded.scene.camera.image_width, 32);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(&empty_scene(), None, &[]);
        bytes[0] = b'X';
        assert!(matches!(decode(&bytes), Err(SceneFileError::BadMagic)));
    }

    #[test]
    fn rejects_truncated_garbage() {
        assert!(decode(&[1, 2, 3]).is_err());
        assert!(decode(b"RTSCgarbage").is_err());
    }

    /// A corrupt mesh with an out-of-range face index must return `Err` on
    /// deserialize — not panic — because `MeshData::build` would index
    /// `verts[99]` on a 3-vertex mesh. The guard in `Shape`'s `Deserialize`
    /// catches this before `build()` is called.
    #[test]
    fn decode_rejects_mesh_with_out_of_range_face_index() {
        use crate::scene::{MeshData, Shape};
        use crate::vec3::Vec3;

        let bad_data = MeshData {
            verts: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 99]], // index 99 is out of range for 3 verts
        };

        // ShapeData is private, so we mirror its serde layout to produce raw
        // postcard bytes that deserialize into Shape with the bad index.
        #[derive(serde::Serialize)]
        enum ShapeDataMirror {
            Sphere { center: Vec3, radius: f32 },
            Quad { q: Vec3, u: Vec3, v: Vec3 },
            Box { a: Vec3, b: Vec3 },
            Mesh { data: MeshData },
        }

        let bytes = postcard::to_allocvec(&ShapeDataMirror::Mesh { data: bad_data })
            .expect("encode shape");
        let result: Result<Shape, _> = postcard::from_bytes(&bytes);
        assert!(result.is_err(), "expected Err for out-of-range face index, got Ok");
    }
}
