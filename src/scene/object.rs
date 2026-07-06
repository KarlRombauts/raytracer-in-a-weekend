#[cfg(not(target_arch = "wasm32"))]
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::Color;
use crate::geometry::transform::{apply, rotation_matrix};
use crate::geometry::{ObjData, Quad, Rotate, Scale, Translate};
use crate::material::Material;
use crate::ray::Intersect;
use crate::vec3::{Point3, Vec3};

use super::{MaterialSpec, MeshData, Shape, TextureSpec};

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
    /// Build a mesh object from already-parsed OBJ data, auto-fitting it to
    /// `target_center`/`target_size` (the largest dimension is scaled to roughly
    /// `target_size`) so it lands visible regardless of the OBJ's native units.
    /// The mesh keeps the baked default material and is placed via `Transform`,
    /// never per-vertex. Shared by the native (path) and web (bytes) loaders.
    fn from_obj_data(name: String, obj: &ObjData, target_center: Vec3, target_size: f32) -> ObjectSpec {
        let material = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.73, 0.73, 0.73)),
        };
        let (verts, faces, uvs) = obj.mesh_data();
        let data = Arc::new(MeshData { verts, faces, uvs });
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

        ObjectSpec {
            name,
            shape: Shape::Mesh { data, object, render },
            material,
            transform,
            hidden: false,
        }
    }

    /// Load a Wavefront OBJ from disk as a BVH-backed mesh object. Returns `None`
    /// if the file isn't readable. Native only (reads the filesystem).
    ///
    /// Note: the underlying loader panics on malformed OBJ content; we only
    /// guard the readability of the path here.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_obj(path: &Path, target_center: Vec3, target_size: f32) -> Option<ObjectSpec> {
        let path_str = path.to_str()?;
        std::fs::metadata(path).ok()?; // bail early if unreadable

        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "mesh".to_string());

        let obj = ObjData::load(path_str);
        Some(Self::from_obj_data(name, &obj, target_center, target_size))
    }

    /// Load a Wavefront OBJ from in-memory text (the web upload path) as a
    /// BVH-backed mesh object. `name` is the display name (no extension).
    pub fn from_obj_bytes(name: &str, raw: &str, target_center: Vec3, target_size: f32) -> ObjectSpec {
        let obj = ObjData::parse(raw);
        Self::from_obj_data(name.to_string(), &obj, target_center, target_size)
    }

    /// World-space centre of the object's base geometry, ignoring its transform.
    /// This is the pivot `build` rotates and scales about, and the point the GL
    /// preview centres on — so it's where the transform gizmo should sit.
    ///
    /// The bounding box is material-independent, so this builds with a throwaway
    /// solid material instead of the object's own. Building the real one decodes
    /// any image texture, and `pivot` runs *every frame* while the gizmo drags —
    /// re-decoding a texture per frame is what made dragging textured objects
    /// crawl.
    pub(crate) fn pivot(&self) -> Vec3 {
        let cheap = MaterialSpec::Lambertian {
            albedo: TextureSpec::solid(Color::new(0.5, 0.5, 0.5)),
        }
        .build();
        self.shape.build(cheap).bounding_box().center()
    }

    pub(crate) fn build(&self) -> Arc<dyn Intersect> {
        // A quad is transform-aware: bake the transform straight into a concrete
        // world-space quad, the same representation used for light sampling — one
        // surface, one affine definition (see `placed_quad`). Sphere/Box/Mesh
        // keep the decorator stack below: a baked `Sphere` would lose its
        // texture's rotation (it stores no orientation), and Box/Mesh can't
        // collapse to a single primitive.
        if let Shape::Quad { q, u, v } = &self.shape {
            return Arc::new(placed_quad(*q, *u, *v, &self.transform, self.material.build()));
        }

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

/// Object→world placement: rotate then scale about the base geometry's centre
/// `c`, then translate. This is the single definition of how a `Transform` maps
/// geometry into the world — shared by quad building and light baking so the
/// convention lives in one place. (The `Sphere`/`Box`/`Mesh` decorator path in
/// `ObjectSpec::build` is the other, necessarily-separate expression, since
/// those can't collapse to one baked primitive.)
pub(crate) struct Placement {
    r: [Vec3; 3],
    c: Vec3,
    s: Vec3,
    t: Vec3,
}

impl Placement {
    pub(crate) fn new(transform: &Transform, center: Vec3) -> Self {
        Placement {
            r: rotation_matrix(transform.rotate),
            c: center,
            s: transform.scale,
            t: transform.translate,
        }
    }

    /// Map a point on the base geometry into world space.
    pub(crate) fn point(&self, p: Point3) -> Point3 {
        apply(&self.r, (p - self.c) * self.s) + self.c + self.t
    }

    /// Map an edge/direction vector — the linear part only (no centre offset,
    /// no translation).
    pub(crate) fn vector(&self, e: Vec3) -> Vec3 {
        apply(&self.r, e * self.s)
    }
}

/// Bake a quad's defining data through `transform` into a concrete world-space
/// quad (base centre = its centroid). A transformed quad is still a quad, so
/// this is exact for both intersection and light sampling — including under
/// non-uniform scale (area = |u'×v'|) — and preserves UVs, since an affine map
/// keeps the fractional position along each edge.
pub(crate) fn placed_quad(q: Point3, u: Vec3, v: Vec3, transform: &Transform, material: Arc<dyn Material>) -> Quad {
    let pl = Placement::new(transform, q + (u + v) * 0.5);
    Quad::new(pl.point(q), pl.vector(u), pl.vector(v), material)
}

#[cfg(test)]
mod bake_equivalence_tests {
    use super::*;
    use crate::interval::Interval;
    use crate::material::Lambertian;
    use crate::ray::Ray;

    /// A baked quad must intersect *identically* to the old decorator stack —
    /// same hit/miss, hit point, normal, and UVs — under a rotate + non-uniform
    /// scale + translate. This is the safety net for routing `build()`'s quad
    /// path through `placed_quad`.
    #[test]
    fn baked_quad_matches_the_decorator_stack() {
        let mat = || -> Arc<dyn Material> {
            Arc::new(Lambertian::from_color(Color::new(0.2, 0.4, 0.6)))
        };
        let q = Point3::new(-1.0, 0.0, -1.0);
        let u = Vec3::new(2.0, 0.0, 0.0);
        let v = Vec3::new(0.0, 0.0, 2.0);
        let transform = Transform {
            rotate: Vec3::new(20.0, 35.0, -10.0),
            scale: Vec3::new(1.5, 1.0, 0.7),
            translate: Vec3::new(1.0, 2.0, -0.5),
        };

        // New path: baked concrete quad.
        let baked: Arc<dyn Intersect> = Arc::new(placed_quad(q, u, v, &transform, mat()));

        // Old path: the decorator stack, built by hand exactly as the previous
        // `build()` did (rotate/scale about the base centre, then translate).
        let c = q + (u + v) * 0.5;
        let base: Arc<dyn Intersect> = Arc::new(Quad::new(q, u, v, mat()));
        let mut deco = Arc::new(Translate::new(base, -c)) as Arc<dyn Intersect>;
        deco = Arc::new(Scale::new(deco, transform.scale));
        deco = Arc::new(Rotate::new(deco, transform.rotate));
        deco = Arc::new(Translate::new(deco, c));
        deco = Arc::new(Translate::new(deco, transform.translate));

        // Fire a ring of rays at the transformed centroid (c is the origin here,
        // so the world centroid is c + translate).
        let target = c + transform.translate;
        let ti = Interval::new(0.001, f32::INFINITY);
        let mut hits = 0;
        for k in 0..64 {
            let a = k as f32 * 0.19;
            let origin = Point3::new(3.0 * a.cos(), 4.0, 3.0 * a.sin());
            let ray = Ray::new(origin, target - origin);
            let hb = baked.intersect(&ray, &ti);
            let hd = deco.intersect(&ray, &ti);
            assert_eq!(hb.is_some(), hd.is_some(), "hit/miss disagree at k={k}");
            if let (Some(a), Some(b)) = (hb, hd) {
                hits += 1;
                assert!((a.p - b.p).length() < 1e-3, "point mismatch {:?} vs {:?}", a.p, b.p);
                assert!(a.normal.dot(&b.normal) > 0.999, "normal mismatch");
                assert!(
                    (a.u - b.u).abs() < 1e-3 && (a.v - b.v).abs() < 1e-3,
                    "uv mismatch ({},{}) vs ({},{})",
                    a.u, a.v, b.u, b.v
                );
            }
        }
        assert!(hits > 8, "expected several hits to compare, got {hits}");
    }
}
