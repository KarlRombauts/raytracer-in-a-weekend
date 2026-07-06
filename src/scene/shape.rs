use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::geometry::{Quad, RenderMesh, Sphere, Triangle, make_box};
use crate::ray::{Intersect, BVH};
use crate::vec3::{Point3, Vec3};

/// The portable description of a triangle mesh: positions, triangle indices, and
/// optional per-triangle UVs. Everything else (per-triangle geometry, BVH,
/// preview mesh) is rebuilt from these on load.
///
/// `uvs` is `#[serde(skip)]`: it would break the existing `.scene` postcard
/// layout if added to `MeshData`'s own wire format, so it travels alongside the
/// mesh in [`ShapeData::MeshUv`] instead (see `Shape`'s serde). It is either
/// empty (no texture coordinates) or aligned 1:1 with `faces`.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub verts: Vec<Vec3>,
    pub faces: Vec<[u32; 3]>,
    #[serde(skip)]
    pub uvs: Vec<[[f32; 2]; 3]>,
}

impl MeshData {
    /// Build the runtime intersect handle (BVH) and the preview mesh from the
    /// stored arrays. The geometry is material-agnostic — a [`World`]'s object
    /// binds the material at the hit — so editing a mesh's material never
    /// rebuilds the (material-independent) BVH, and the same handle can be shared
    /// across objects with different materials.
    ///
    /// [`World`]: crate::world::World
    pub fn build(&self) -> (Arc<dyn Intersect>, Arc<RenderMesh>) {
        // Compute the `usize` face indices once and share them between the BVH's
        // triangles and the preview mesh.
        let faces_usize = self.faces_usize();
        let bvh = BVH::build(self.triangles_from(&faces_usize));
        let render = Arc::new(RenderMesh::from_triangles_smooth(&self.verts, &faces_usize));
        (Arc::new(bvh), render)
    }

    /// The mesh's triangles with smooth vertex normals (and UVs when they line up
    /// with the faces) — the primitives the BVH is built over. Used internally by
    /// [`build`](Self::build); exposed so benchmarks can time `BVH::build` on the
    /// triangles in isolation from the rest of assembly.
    pub fn triangles(&self) -> Vec<Triangle> {
        self.triangles_from(&self.faces_usize())
    }

    /// The face vertex indices as `usize` — the form the triangle builder and the
    /// preview mesh both want.
    fn faces_usize(&self) -> Vec<[usize; 3]> {
        self.faces
            .iter()
            .map(|[i, j, k]| [*i as usize, *j as usize, *k as usize])
            .collect()
    }

    fn triangles_from(&self, faces_usize: &[[usize; 3]]) -> Vec<Triangle> {
        let vn = crate::geometry::vertex_normals(&self.verts, faces_usize);
        // Only honour UVs when they line up with the faces; a mismatch (or none)
        // falls back to the smooth-only triangle (barycentric UV).
        let has_uv = self.uvs.len() == faces_usize.len();
        faces_usize
            .iter()
            .enumerate()
            .map(|(t, [i, j, k])| {
                if has_uv {
                    Triangle::from_points_smooth_uv(
                        &self.verts[*i],
                        &self.verts[*j],
                        &self.verts[*k],
                        &vn[*i],
                        &vn[*j],
                        &vn[*k],
                        self.uvs[t],
                    )
                } else {
                    Triangle::from_points_smooth(
                        &self.verts[*i],
                        &self.verts[*j],
                        &self.verts[*k],
                        &vn[*i],
                        &vn[*j],
                        &vn[*k],
                    )
                }
            })
            .collect()
    }
}

/// Plain-data description of a shape. `Mesh` is an escape hatch for prebuilt,
/// non-editable geometry (e.g. a loaded OBJ wrapped in a BVH) — it's stored as
/// a shared handle and ignores the object's material.
#[derive(Clone)]
pub enum Shape {
    Sphere {
        center: Point3,
        radius: f32,
    },
    Quad {
        q: Point3,
        u: Vec3,
        v: Vec3,
    },
    Box {
        a: Point3,
        b: Point3,
    },
    Mesh {
        data: Arc<MeshData>,
        object: Arc<dyn Intersect>,
        render: Arc<crate::geometry::RenderMesh>,
    },
}

impl Shape {
    /// Build the material-agnostic runtime geometry for this shape. The object's
    /// material is bound separately, at the hit, by the [`World`](crate::world::World).
    pub(crate) fn build(&self) -> Arc<dyn Intersect> {
        match self {
            Shape::Sphere { center, radius } => Arc::new(Sphere::stationary(*center, *radius)),
            Shape::Quad { q, u, v } => Arc::new(Quad::new(*q, *u, *v)),
            Shape::Box { a, b } => Arc::new(make_box(*a, *b)),
            // The prebuilt, material-agnostic mesh BVH is shared directly — no
            // per-object rebuild, and reusable across objects with different
            // materials.
            Shape::Mesh { object, .. } => object.clone(),
        }
    }

    /// Triangle geometry for the rasterized preview, in the shape's own
    /// definition space (the object's transform is applied separately as a
    /// model matrix).
    pub fn render_mesh(&self) -> crate::geometry::RenderMesh {
        match self {
            Shape::Sphere { center, radius } => RenderMesh::sphere(*center, *radius, 16, 24),
            Shape::Quad { q, u, v } => RenderMesh::quad(*q, *u, *v),
            Shape::Box { a, b } => RenderMesh::unit_box(*a, *b),
            Shape::Mesh { render, .. } => RenderMesh {
                positions: render.positions.clone(),
                normals: render.normals.clone(),
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
enum ShapeData {
    Sphere { center: Point3, radius: f32 },
    Quad { q: Point3, u: Vec3, v: Vec3 },
    Box { a: Point3, b: Point3 },
    /// Legacy mesh with no texture coordinates. Kept at its original index so
    /// pre-existing `.scene` files still decode (postcard keys an enum by
    /// declaration order). New variants must be appended after it.
    Mesh { data: MeshData },
    /// Mesh carrying per-triangle UVs (`MeshData::uvs` is `serde(skip)`, so they
    /// ride alongside here). Appended last to keep older indices stable.
    MeshUv { data: MeshData, uvs: Vec<[[f32; 2]; 3]> },
}

/// Reject out-of-range face indices before `data.build()` (which would index a
/// vertex out of bounds and panic). A corrupt `.scene` must `Err`, not panic.
fn validate_faces<E: serde::de::Error>(data: &MeshData) -> Result<(), E> {
    let n = data.verts.len() as u32;
    for face in &data.faces {
        for &idx in face.iter() {
            if idx >= n {
                return Err(E::custom("mesh face index out of range"));
            }
        }
    }
    Ok(())
}

impl Serialize for Shape {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let repr = match self {
            Shape::Sphere { center, radius } => ShapeData::Sphere { center: *center, radius: *radius },
            Shape::Quad { q, u, v } => ShapeData::Quad { q: *q, u: *u, v: *v },
            Shape::Box { a, b } => ShapeData::Box { a: *a, b: *b },
            // Emit the UV-carrying variant only when the mesh actually has UVs, so
            // untextured meshes stay byte-compatible with the legacy `Mesh` form.
            Shape::Mesh { data, .. } if data.uvs.is_empty() => {
                ShapeData::Mesh { data: (**data).clone() }
            }
            Shape::Mesh { data, .. } => ShapeData::MeshUv {
                data: (**data).clone(),
                uvs: data.uvs.clone(),
            },
        };
        repr.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Shape {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match ShapeData::deserialize(d)? {
            ShapeData::Sphere { center, radius } => Shape::Sphere { center, radius },
            ShapeData::Quad { q, u, v } => Shape::Quad { q, u, v },
            ShapeData::Box { a, b } => Shape::Box { a, b },
            ShapeData::Mesh { data } => {
                validate_faces::<D::Error>(&data)?;
                let data = Arc::new(data);
                let (object, render) = data.build();
                Shape::Mesh { data, object, render }
            }
            ShapeData::MeshUv { mut data, uvs } => {
                validate_faces::<D::Error>(&data)?;
                // `build` only honours UVs that line up with the faces, so a
                // mismatch degrades gracefully to barycentric coordinates.
                data.uvs = uvs;
                let data = Arc::new(data);
                let (object, render) = data.build();
                Shape::Mesh { data, object, render }
            }
        })
    }
}

#[cfg(test)]
mod render_mesh_tests {
    use super::*;
    use crate::vec3::{Point3, Vec3};

    #[test]
    fn primitive_shapes_produce_nonempty_meshes() {
        let sphere = Shape::Sphere {
            center: Point3::new(0.0, 0.0, 0.0),
            radius: 1.0,
        };
        let quad = Shape::Quad {
            q: Point3::new(0.0, 0.0, 0.0),
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
        };
        let bx = Shape::Box {
            a: Point3::new(0.0, 0.0, 0.0),
            b: Point3::new(1.0, 1.0, 1.0),
        };
        assert!(!sphere.render_mesh().positions.is_empty());
        assert_eq!(quad.render_mesh().positions.len(), 6);
        assert_eq!(bx.render_mesh().positions.len(), 36);
    }
}

#[cfg(test)]
mod mesh_serde_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::scene::{build_world, MaterialSpec, ObjectSpec, Scene, TextureSpec, Transform};

    fn tiny_mesh_scene() -> Scene {
        // A single triangle mesh + a sphere, so we cover both Mesh and a primitive.
        let data = MeshData {
            verts: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
            uvs: vec![],
        };
        let (object, render) = data.build();
        let mesh = ObjectSpec {
            name: "tri".to_string(),
            shape: Shape::Mesh { data: Arc::new(data), object, render },
            material: MaterialSpec::Lambertian {
                albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let sphere = ObjectSpec {
            name: "ball".to_string(),
            shape: Shape::Sphere { center: Vec3::new(2.0, 0.0, 0.0), radius: 1.5 },
            material: MaterialSpec::Metal { albedo: Color::new(0.8, 0.8, 0.8), fuzz: 0.1 },
            transform: Transform::identity(),
            hidden: false,
        };
        Scene {
            camera: CameraConfig::builder().image_width(64).build(),
            objects: vec![mesh, sphere],
        }
    }

    #[test]
    fn scene_with_mesh_round_trips_via_postcard() {
        let scene = tiny_mesh_scene();
        let bytes = postcard::to_allocvec(&scene).expect("encode");
        let back: Scene = postcard::from_bytes(&bytes).expect("decode");

        assert_eq!(back.objects.len(), 2);
        assert_eq!(back.camera, scene.camera);
        assert_eq!(back.objects[0].name, "tri");
        assert_eq!(back.objects[1].name, "ball");
        assert_eq!(back.objects[0].material, scene.objects[0].material);

        // Mesh geometry survived as verts + faces.
        match &back.objects[0].shape {
            Shape::Mesh { data, .. } => {
                assert_eq!(data.verts.len(), 3);
                assert_eq!(data.faces, vec![[0u32, 1, 2]]);
            }
            _other => panic!("expected mesh"),
        }

        // The decoded mesh rebuilt a usable BVH: the world assembles and the
        // mesh's bounding box is finite.
        let world = build_world(&back);
        assert!(world.bounding_box().center().x.is_finite());
    }

    /// A mesh that carries per-triangle UVs round-trips through postcard (via the
    /// `MeshUv` shape variant) with its UVs intact.
    #[test]
    fn textured_mesh_uvs_round_trip() {
        let data = MeshData {
            verts: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
            uvs: vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]],
        };
        let (object, render) = data.build();
        let mesh = ObjectSpec {
            name: "uv-tri".to_string(),
            shape: Shape::Mesh { data: Arc::new(data), object, render },
            material: MaterialSpec::Lambertian {
                albedo: TextureSpec::solid(Color::new(0.7, 0.7, 0.7)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().image_width(16).build(),
            objects: vec![mesh],
        };

        let bytes = postcard::to_allocvec(&scene).expect("encode");
        let back: Scene = postcard::from_bytes(&bytes).expect("decode");
        match &back.objects[0].shape {
            Shape::Mesh { data, .. } => {
                assert_eq!(data.uvs, vec![[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]]);
            }
            _ => panic!("expected mesh"),
        }
    }
}
