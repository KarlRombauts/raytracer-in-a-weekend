use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::camera::CameraConfig;
use crate::color::Color;
use crate::geometry::{ObjData, Quad, RenderMesh, Rotate, Scale, Sphere, Translate, Triangle, make_box};
use crate::group::{IntersectGroup, Light};
use crate::interval::Interval;
use crate::material::{Dielectric, DiffuseLight, Glossy, Lambertian, Material, Metal};
use crate::ray::{HitRecord, Ray, AABB, BVH, Intersect};
use crate::texture::{
    CheckerTexture, ImageTexture, MappedTexture, NoiseTexture, Projection, SolidColor, Texture,
};
use crate::vec3::{Point3, Vec3};

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
    Noise {
        scale: f32,
        depth: u32,
    },
    Image {
        asset: Asset,
        mapping: Mapping,
    },
}

/// A checker cell. Deliberately omits `Checker`, so checker-in-checker
/// recursion is unrepresentable (one level of nesting only).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CellTexture {
    Solid { color: Color },
    Noise { scale: f32, depth: u32 },
    Image { asset: Asset },
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
            CellTexture::Noise { scale, depth } => Arc::new(NoiseTexture::new(*scale, *depth)),
            CellTexture::Image { asset } => build_image(asset),
        }
    }

    fn preview_color(&self) -> Color {
        match self {
            CellTexture::Solid { color } => *color,
            CellTexture::Noise { .. } => Color::new(0.5, 0.5, 0.5),
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
            TextureSpec::Noise { scale, depth } => Arc::new(NoiseTexture::new(*scale, *depth)),
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
            TextureSpec::Noise { .. } => Color::new(0.5, 0.5, 0.5),
            TextureSpec::Image { .. } => Color::new(0.5, 0.5, 0.5),
        }
    }
}

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
    fn build(&self) -> Arc<dyn Material> {
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

/// The portable description of a triangle mesh: positions + triangle indices.
/// Everything else (per-triangle geometry, BVH, preview mesh) is rebuilt from
/// these on load.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub verts: Vec<Vec3>,
    pub faces: Vec<[u32; 3]>,
}

impl MeshData {
    /// Build the runtime intersect handle (BVH) and the preview mesh from the
    /// stored arrays. The triangles bake a placeholder material; the object's
    /// real material is applied cheaply at world-build time by wrapping this
    /// BVH in a [`MaterialOverride`] (see `Shape::build`), so editing a mesh's
    /// material never rebuilds the (material-independent) BVH.
    pub fn build(&self) -> (Arc<dyn Intersect>, Arc<RenderMesh>) {
        let material = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
        }
        .build();
        let faces_usize: Vec<[usize; 3]> = self
            .faces
            .iter()
            .map(|[i, j, k]| [*i as usize, *j as usize, *k as usize])
            .collect();
        let triangles: Vec<Triangle> = faces_usize
            .iter()
            .map(|[i, j, k]| {
                Triangle::from_points(&self.verts[*i], &self.verts[*j], &self.verts[*k], material.clone())
            })
            .collect();
        let bvh = BVH::build(triangles);
        let render = Arc::new(RenderMesh::from_triangles_smooth(&self.verts, &faces_usize));
        (Arc::new(bvh), render)
    }
}

/// Wraps a prebuilt, material-agnostic intersect handle (a mesh BVH) and
/// overrides every hit's material with `material`. This lets a mesh's material
/// be changed by swapping one `Arc` at world-build time, with **no BVH rebuild**
/// — the spatial structure doesn't depend on the material.
struct MaterialOverride {
    inner: Arc<dyn Intersect>,
    material: Arc<dyn Material>,
}

impl Intersect for MaterialOverride {
    fn intersect(&self, ray: &Ray, ray_t: &Interval) -> Option<HitRecord<'_>> {
        let mut hit = self.inner.intersect(ray, ray_t)?;
        hit.material = &*self.material;
        Some(hit)
    }
    fn bounding_box(&self) -> &AABB {
        self.inner.bounding_box()
    }
    fn center(&self) -> Vec3 {
        self.inner.center()
    }
    fn sample_point(&self, u: f32, v: f32) -> Point3 {
        self.inner.sample_point(u, v)
    }
    fn area(&self) -> f32 {
        self.inner.area()
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
    fn build(&self, material: Arc<dyn Material>) -> Arc<dyn Intersect> {
        match self {
            Shape::Sphere { center, radius } => {
                Arc::new(Sphere::stationary(*center, *radius, material))
            }
            Shape::Quad { q, u, v } => Arc::new(Quad::new(*q, *u, *v, material)),
            Shape::Box { a, b } => Arc::new(make_box(*a, *b, material)),
            // Wrap the prebuilt (material-agnostic) BVH so the object's material
            // is applied at hit time — no per-edit BVH rebuild.
            Shape::Mesh { object, .. } => Arc::new(MaterialOverride {
                inner: object.clone(),
                material,
            }),
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
    Mesh { data: MeshData },
}

impl Serialize for Shape {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let repr = match self {
            Shape::Sphere { center, radius } => ShapeData::Sphere { center: *center, radius: *radius },
            Shape::Quad { q, u, v } => ShapeData::Quad { q: *q, u: *u, v: *v },
            Shape::Box { a, b } => ShapeData::Box { a: *a, b: *b },
            Shape::Mesh { data, .. } => ShapeData::Mesh { data: (**data).clone() },
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
                // Validate face indices before calling data.build(), which would
                // panic with an out-of-bounds index. A corrupt .scene file must
                // return Err rather than panic.
                let n = data.verts.len() as u32;
                for face in &data.faces {
                    for &idx in face.iter() {
                        if idx >= n {
                            return Err(<D::Error as serde::de::Error>::custom(
                                "mesh face index out of range",
                            ));
                        }
                    }
                }
                let data = Arc::new(data);
                let (object, render) = data.build();
                Shape::Mesh { data, object, render }
            }
        })
    }
}

/// Scale and Euler rotation (degrees) about the object's own centre, followed
/// by a world translation.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub rotate: Vec3,
    pub scale: Vec3,
    pub translate: Vec3,
}

impl Transform {
    pub fn identity() -> Self {
        Transform {
            rotate: Vec3::ZERO,
            scale: Vec3::new(1.0, 1.0, 1.0),
            translate: Vec3::ZERO,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ObjectSpec {
    pub name: String,
    pub shape: Shape,
    pub material: MaterialSpec,
    pub transform: Transform,
    /// When true the object is omitted from the rendered world and the GL
    /// preview (toggled by the outliner eye). Default false.
    pub hidden: bool,
}

impl ObjectSpec {
    /// Load a Wavefront OBJ as a BVH-backed mesh object, auto-fitting it to
    /// `target_center` and `target_size` (the largest mesh dimension is scaled
    /// to roughly `target_size`). This keeps imports visible regardless of the
    /// OBJ's native units/origin. The mesh keeps the baked default material and
    /// is positioned via Transform, never per-vertex. Returns `None` if the
    /// file isn't readable.
    ///
    /// Note: the underlying loader panics on malformed OBJ content; we only
    /// guard the readability of the path here.
    pub fn from_obj(path: &Path, target_center: Vec3, target_size: f32) -> Option<ObjectSpec> {
        let path_str = path.to_str()?;
        std::fs::metadata(path).ok()?; // bail early if unreadable

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mesh".to_string());

        let material = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
        };
        let obj = ObjData::load(path_str);
        let (verts, faces) = obj.mesh_data();
        let data = Arc::new(MeshData { verts, faces });
        let (object, render) = data.build();

        // Auto-fit: scale the mesh to the target size and recentre it.
        let bbox = object.bounding_box();
        let c = bbox.center();
        let e = bbox.extent();
        let e_max = e.x.max(e.y).max(e.z).max(1e-6);
        let s = (target_size / e_max).max(1e-4);

        let transform = Transform {
            rotate: Vec3::ZERO,
            scale: Vec3::new(s, s, s),
            translate: target_center - c,
        };

        Some(ObjectSpec {
            name,
            shape: Shape::Mesh { data, object, render },
            material,
            transform,
            hidden: false,
        })
    }

    /// World-space centre of the object's base geometry, ignoring its transform.
    /// This is the pivot `build` rotates and scales about, and the point the GL
    /// preview centres on — so it's where the transform gizmo should sit.
    pub(crate) fn pivot(&self) -> Vec3 {
        self.shape
            .build(self.material.build())
            .bounding_box()
            .center()
    }

    pub(crate) fn build(&self) -> Arc<dyn Intersect> {
        let t = &self.transform;
        let mut object = self.shape.build(self.material.build());

        // Apply scale and rotation about the object's own centre so editing
        // feels in-place, rather than swinging it around the world origin.
        let one = Vec3::new(1.0, 1.0, 1.0);
        if t.rotate != Vec3::ZERO || t.scale != one {
            let c = object.bounding_box().center();
            object = Arc::new(Translate::new(object, -c));
            if t.scale != one {
                object = Arc::new(Scale::new(object, t.scale));
            }
            if t.rotate != Vec3::ZERO {
                object = Arc::new(Rotate::new(object, t.rotate));
            }
            object = Arc::new(Translate::new(object, c));
        }
        if t.translate != Vec3::ZERO {
            object = Arc::new(Translate::new(object, t.translate));
        }
        object
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Scene {
    pub camera: CameraConfig,
    pub objects: Vec<ObjectSpec>,
}

/// Assemble the renderable world from the scene description. Cheap enough to
/// call on every edit (Mesh handles are shared, not rebuilt). Emissive objects
/// are also registered in `world.lights` for direct light sampling.
pub fn build_world(scene: &Scene) -> IntersectGroup {
    let mut world = IntersectGroup::new();
    for obj in &scene.objects {
        if obj.hidden {
            continue;
        }
        let geom = obj.build();
        world.add(geom.clone());
        if let MaterialSpec::DiffuseLight { emit } = &obj.material {
            // Only register emitters we can importance-sample (area() > 0).
            // Others (sphere/mesh/transformed) still glow when hit directly,
            // they're just not shadow-ray sampled.
            if geom.area() > 0.0 {
                world.lights.push(Light {
                    geom,
                    // Emission is Solid-only in Phase 1, so `preview_color()`
                    // equals the true emitted colour exactly. If emission ever
                    // becomes a non-Solid texture, this would feed a
                    // representative average here — revisit then.
                    emit: emit.preview_color(),
                });
            }
        }
    }
    world
}

/// Duplicate the object at `i`: insert a clone right after it with " copy"
/// appended to the name. Returns the new object's index, or None if `i` is out
/// of range. Cheap — meshes share their `Arc` BVH.
pub fn duplicate_object(objects: &mut Vec<ObjectSpec>, i: usize) -> Option<usize> {
    let mut clone = objects.get(i)?.clone();
    clone.name = format!("{} copy", clone.name);
    objects.insert(i + 1, clone);
    Some(i + 1)
}

/// Rough world-space bounds `(min, max)` of the placeable primitives, using
/// their base shape parameters (ignoring transforms). Meshes are skipped — they
/// can't be bounded without building them. Used to auto-fit imported meshes into
/// the existing scene. Returns `None` if there are no such primitives.
pub fn placeable_bounds(objects: &[ObjectSpec]) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(-f32::INFINITY, -f32::INFINITY, -f32::INFINITY);
    let mut any = false;

    for o in objects {
        let (lo, hi) = match &o.shape {
            Shape::Sphere { center, radius } => {
                let r = Vec3::new(*radius, *radius, *radius);
                (*center - r, *center + r)
            }
            Shape::Box { a, b } => (Vec3::min(a, b), Vec3::max(a, b)),
            Shape::Quad { q, u, v } => {
                let corners = [*q, *q + *u, *q + *v, *q + *u + *v];
                let mut lo = corners[0];
                let mut hi = corners[0];
                for c in &corners[1..] {
                    lo = Vec3::min(&lo, c);
                    hi = Vec3::max(&hi, c);
                }
                (lo, hi)
            }
            Shape::Mesh { .. } => continue,
        };
        min = Vec3::min(&min, &lo);
        max = Vec3::max(&max, &hi);
        any = true;
    }

    any.then_some((min, max))
}

#[cfg(test)]
mod visibility_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;

    fn emissive(name: &str) -> ObjectSpec {
        ObjectSpec {
            name: name.into(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 0.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 1.0, 0.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    #[test]
    fn hidden_object_is_excluded_from_world_and_lights() {
        let mut scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![emissive("a"), emissive("b")],
        };
        let full = build_world(&scene);
        scene.objects[1].hidden = true;
        let partial = build_world(&scene);
        // One fewer light registered when an emitter is hidden.
        assert_eq!(full.lights.len(), 2);
        assert_eq!(partial.lights.len(), 1);
    }

    #[test]
    fn duplicate_inserts_clone_after_with_suffixed_name() {
        let mut objs = vec![emissive("Light"), emissive("Box")];
        let new_i = super::duplicate_object(&mut objs, 0).unwrap();
        assert_eq!(new_i, 1);
        assert_eq!(objs.len(), 3);
        assert_eq!(objs[1].name, "Light copy");
        assert_eq!(objs[2].name, "Box"); // original order preserved after the insert
        assert!(super::duplicate_object(&mut objs, 99).is_none());
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;
    use crate::scenes::cornell_box;

    #[test]
    fn cornell_box_collects_one_light() {
        let scene = cornell_box();
        let world = build_world(&scene);
        assert_eq!(world.lights.len(), 1, "expected exactly one light");
        assert_eq!(world.lights[0].emit, Color::new(15.0, 15.0, 15.0));
    }
}

#[cfg(test)]
mod registration_tests {
    use super::*;
    use crate::camera::CameraConfig;

    #[test]
    fn quad_and_sphere_emitters_both_register() {
        let quad_light = ObjectSpec {
            name: "quad".to_string(),
            shape: Shape::Quad {
                q: Point3::new(0.0, 5.0, 0.0),
                u: Vec3::new(1.0, 0.0, 0.0),
                v: Vec3::new(0.0, 0.0, 1.0),
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let sphere_light = ObjectSpec {
            name: "sphere".to_string(),
            shape: Shape::Sphere {
                center: Point3::new(0.0, 0.0, 0.0),
                radius: 1.0,
            },
            material: MaterialSpec::DiffuseLight {
                emit: TextureSpec::solid(Color::new(5.0, 5.0, 5.0)),
            },
            transform: Transform::identity(),
            hidden: false,
        };
        let scene = Scene {
            camera: CameraConfig::builder().build(),
            objects: vec![quad_light, sphere_light],
        };
        let world = build_world(&scene);
        // Both the quad and the sphere have area()>0, so both register as
        // importance-sampled lights — the sphere via cone (solid-angle) sampling.
        assert_eq!(world.lights.len(), 2, "quad and sphere both register");
        // Both objects still live in the world geometry too.
        assert_eq!(world.objects.len(), 2, "both objects remain in the world");
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
    fn noise_previews_mid_gray() {
        let t = TextureSpec::Noise {
            scale: 4.0,
            depth: 7,
        };
        let _ = t.build();
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
mod mesh_serde_tests {
    use super::*;

    fn tiny_mesh_scene() -> Scene {
        // A single triangle mesh + a sphere, so we cover both Mesh and a primitive.
        let data = MeshData {
            verts: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            faces: vec![[0, 1, 2]],
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
}

#[cfg(test)]
mod serde_tests {
    use super::*;

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

    #[test]
    fn checker_texture_round_trips() {
        let t = TextureSpec::Checker {
            scale: 2.5,
            even: CellTexture::Solid { color: Color::new(0.1, 0.2, 0.3) },
            odd: CellTexture::Noise { scale: 4.0, depth: 7 },
        };
        let bytes = postcard::to_allocvec(&t).expect("encode");
        let back: TextureSpec = postcard::from_bytes(&bytes).expect("decode");
        assert_eq!(t, back);
    }
}
