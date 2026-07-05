use std::sync::Arc;

use crate::{
    interval::Interval,
    material::{material, Material},
    ray::{surface_pdf_value, AreaLight, HitRecord, Intersect, Ray, AABB},
    vec3::{Point3, Vec3},
};

pub struct Triangle {
    centroid: Vec3,
    q: Point3,
    u: Vec3,
    v: Vec3,
    d: f32,
    w: Vec3,
    normal: Vec3,
    /// Optional per-vertex (shading) normals for `(p1, p2, p3)`. When present,
    /// the hit's shading normal is their barycentric interpolation, so a faceted
    /// mesh shades as a smooth surface — essential for specular meshes (glass,
    /// metal), where flat per-facet normals shatter the reflection/refraction.
    vnormals: Option<[Vec3; 3]>,
    /// Optional per-vertex texture coordinates `(uv1, uv2, uv3)` from the source
    /// mesh (e.g. an OBJ's `vt`). When present, the hit's `(u, v)` is their
    /// barycentric interpolation, so an image texture maps across the mesh's own
    /// UV layout. Absent → the hit reports raw barycentrics (the prior behaviour).
    uvs: Option<[[f32; 2]; 3]>,
    material: Arc<dyn Material>,
    bbox: AABB,
}

impl Triangle {
    pub fn from_points(p1: &Vec3, p2: &Vec3, p3: &Vec3, material: Arc<dyn Material>) -> Self {
        Self::build(p1, p2, p3, None, None, material)
    }

    /// Like [`from_points`](Self::from_points) but with per-vertex shading normals
    /// `(n1, n2, n3)` for smooth shading. The geometric (flat) normal still drives
    /// the front-face test and ray geometry; the interpolated normal is used only
    /// for shading, oriented to the geometric side to avoid light leaks.
    pub fn from_points_smooth(
        p1: &Vec3,
        p2: &Vec3,
        p3: &Vec3,
        n1: &Vec3,
        n2: &Vec3,
        n3: &Vec3,
        material: Arc<dyn Material>,
    ) -> Self {
        Self::build(p1, p2, p3, Some([*n1, *n2, *n3]), None, material)
    }

    /// Like [`from_points_smooth`](Self::from_points_smooth) but also carrying
    /// per-vertex texture coordinates `(uv1, uv2, uv3)`, so the hit reports the
    /// mesh's interpolated UV instead of raw barycentrics.
    #[allow(clippy::too_many_arguments)]
    pub fn from_points_smooth_uv(
        p1: &Vec3,
        p2: &Vec3,
        p3: &Vec3,
        n1: &Vec3,
        n2: &Vec3,
        n3: &Vec3,
        uvs: [[f32; 2]; 3],
        material: Arc<dyn Material>,
    ) -> Self {
        Self::build(p1, p2, p3, Some([*n1, *n2, *n3]), Some(uvs), material)
    }

    fn build(
        p1: &Vec3,
        p2: &Vec3,
        p3: &Vec3,
        vnormals: Option<[Vec3; 3]>,
        uvs: Option<[[f32; 2]; 3]>,
        material: Arc<dyn Material>,
    ) -> Self {
        let u = *p2 - *p1;
        let v = *p3 - *p1;
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(p1);
        let centroid = (*p1 + *p2 + *p3) / 3.;

        let mut triangle = Triangle {
            centroid,
            q: *p1,
            u,
            v,
            d,
            w: Self::barycentric_w(n),
            normal,
            vnormals,
            uvs,
            material,
            bbox: AABB::EMPTY,
        };
        triangle.set_bounding_box();
        triangle
    }

    pub fn new(q: Point3, u: Vec3, v: Vec3, material: Arc<dyn Material>) -> Self {
        let n = u.cross(&v);
        let normal = n.unit();
        let d = normal.dot(&q);

        let centroid = (q + (q + u) + (q + v)) / 3.;

        let mut triangle = Triangle {
            centroid,
            q,
            u,
            v,
            d,
            w: Self::barycentric_w(n),
            normal,
            vnormals: None,
            uvs: None,
            material,
            bbox: AABB::EMPTY,
        };
        triangle.set_bounding_box();
        triangle
    }

    fn set_bounding_box(&mut self) {
        // Enclose the triangle's three actual vertices: q, q+u, q+v. (Using
        // q+u+v here would add the parallelogram's 4th corner — correct for a
        // quad, but it lies outside a triangle and inflates the box, shifting the
        // BVH centre off the true vertex centre.)
        let p2 = self.q + self.u;
        let p3 = self.q + self.v;
        let bb1 = AABB::from_points(self.q, p2);
        let bb2 = AABB::from_points(p2, p3);
        self.bbox = AABB::from_boxes(&bb1, &bb2);
    }

    fn is_interior(a: f32, b: f32) -> bool {
        a > 0. && b > 0. && a + b < 1.
    }

    /// Precomputed `n / (n·n)` used to recover barycentric coordinates at hit
    /// time. A degenerate (zero-area) triangle has `n = 0` and `n·n = 0`; real
    /// meshes contain these, so we return zero rather than dividing by zero. Its
    /// `normal` is likewise zero, so `intersect` rejects every ray (the parallel
    /// check) — the triangle simply never registers a hit.
    fn barycentric_w(n: Vec3) -> Vec3 {
        let nn = n.dot(&n);
        if nn > 0.0 { n / nn } else { Vec3::ZERO }
    }
}

impl Intersect for Triangle {
    fn center(&self) -> Vec3 {
        self.centroid
    }

    fn intersect(
        &self,
        ray: &Ray,
        ray_t: &crate::interval::Interval,
    ) -> Option<crate::ray::HitRecord<'_>> {
        let denom = self.normal.dot(&ray.direction);

        // No hit if the ray is parallel to the plane
        if denom.abs() < 1e-8 {
            return None;
        }

        let t = (self.d - self.normal.dot(&ray.origin)) / denom;
        // Return false if the hit point parameter t is outside the ray interval
        if !ray_t.contains(t) {
            return None;
        }

        // Check that the hit point lies inside the triangle region
        let p = ray.at(t);
        let planar_hit_point = p - self.q;
        let alpha = self.w.dot(&planar_hit_point.cross(&self.v));
        let beta = self.w.dot(&self.u.cross(&planar_hit_point));

        if !Self::is_interior(alpha, beta) {
            return None;
        }

        let mut hit_record = HitRecord::new(t, p, Vec3::ZERO, self.material.as_ref());
        // Front-face and orientation come from the geometric (flat) normal.
        hit_record.set_face_normal(ray, &self.normal);
        // Smooth shading: replace the shading normal with the barycentric blend of
        // the vertex normals (weights 1-α-β, α, β for p1, p2, p3), oriented to the
        // geometric side. Degenerate blends (opposing normals cancel) fall back to
        // the flat normal already set above.
        if let Some([n1, n2, n3]) = self.vnormals {
            let interp = (1.0 - alpha - beta) * n1 + alpha * n2 + beta * n3;
            if interp.length_squared() > 1e-12 {
                let shading = interp.unit();
                hit_record.normal = if shading.dot(&hit_record.normal) < 0.0 {
                    -shading
                } else {
                    shading
                };
            }
        }
        // Texture coordinates: barycentric blend of the per-vertex UVs when the
        // mesh carries them, else the raw barycentrics (α, β) as before.
        match self.uvs {
            Some([uv0, uv1, uv2]) => {
                let w0 = 1.0 - alpha - beta;
                hit_record.u = w0 * uv0[0] + alpha * uv1[0] + beta * uv2[0];
                hit_record.v = w0 * uv0[1] + alpha * uv1[1] + beta * uv2[1];
            }
            None => {
                hit_record.u = alpha;
                hit_record.v = beta;
            }
        }
        Some(hit_record)
    }

    fn bounding_box(&self) -> &AABB {
        &self.bbox
    }
}

impl Triangle {
    fn sample_point(&self, u: f32, v: f32) -> Point3 {
        let su = u.sqrt();
        let p1 = self.q;
        let p2 = self.q + self.u;
        let p3 = self.q + self.v;
        (1.0 - su) * p1 + (su * (1.0 - v)) * p2 + (su * v) * p3
    }

    fn area(&self) -> f32 {
        0.5 * self.u.cross(&self.v).length()
    }
}

impl AreaLight for Triangle {
    fn sample_dir(&self, origin: Point3, u: f32, v: f32) -> Vec3 {
        self.sample_point(u, v) - origin
    }

    fn pdf_value(&self, origin: Point3, dir: Vec3) -> f32 {
        surface_pdf_value(self, self.area(), origin, dir)
    }
}

#[cfg(test)]
mod smooth_normal_tests {
    use super::*;
    use crate::color::Color;
    use crate::interval::Interval;
    use crate::material::Lambertian;
    use crate::ray::Ray;
    use crate::vec3::Point3;
    use std::sync::Arc;

    // Triangle in the z=0 plane (flat normal +z), but with per-vertex normals that
    // tilt outward — so the shading normal must vary across the face.
    fn tri() -> Triangle {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        Triangle::from_points_smooth(
            &Point3::new(0.0, 0.0, 0.0),
            &Point3::new(1.0, 0.0, 0.0),
            &Point3::new(0.0, 1.0, 0.0),
            &Vec3::new(-1.0, -1.0, 1.0).unit(), // n at p1
            &Vec3::new(1.0, 0.0, 1.0).unit(),   // n at p2
            &Vec3::new(0.0, 1.0, 1.0).unit(),   // n at p3
            mat,
        )
    }

    fn shade_at(x: f32, y: f32) -> Vec3 {
        let t = tri();
        // Fire straight down -z at (x, y); barycentric (α,β) = (x, y).
        let ray = Ray::new(Point3::new(x, y, 1.0), Vec3::new(0.0, 0.0, -1.0));
        t.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap().normal
    }

    #[test]
    fn shading_normal_interpolates_the_vertex_normals() {
        // Near p2 (α≈1): shading normal ≈ n2.
        let near_p2 = shade_at(0.96, 0.02);
        let n2 = Vec3::new(1.0, 0.0, 1.0).unit();
        assert!(near_p2.dot(&n2) > 0.99, "near p2 got {near_p2:?}");
        // It is NOT the flat normal (+z) — smoothing actually happened.
        assert!(near_p2.dot(&Vec3::new(0.0, 0.0, 1.0)) < 0.95, "still flat: {near_p2:?}");
        // The result is unit length.
        assert!((near_p2.length() - 1.0).abs() < 1e-5);
    }

    /// With per-vertex UVs, the hit reports their barycentric interpolation —
    /// here uv(p1)=(0,0), uv(p2)=(1,0), uv(p3)=(0,1), so a hit at barycentric
    /// (α,β)=(0.25,0.5) maps to (u,v)=(0.25,0.5).
    #[test]
    fn uv_interpolates_across_the_face() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let n = Vec3::new(0.0, 0.0, 1.0);
        let tri = Triangle::from_points_smooth_uv(
            &Point3::new(0.0, 0.0, 0.0),
            &Point3::new(1.0, 0.0, 0.0),
            &Point3::new(0.0, 1.0, 0.0),
            &n,
            &n,
            &n,
            [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
            mat,
        );
        let ray = Ray::new(Point3::new(0.25, 0.5, 1.0), Vec3::new(0.0, 0.0, -1.0));
        let hit = tri.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!((hit.u - 0.25).abs() < 1e-5, "u={}", hit.u);
        assert!((hit.v - 0.5).abs() < 1e-5, "v={}", hit.v);
    }

    /// Without UVs the hit still reports raw barycentrics (the prior behaviour).
    #[test]
    fn no_uv_reports_barycentrics() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        );
        let ray = Ray::new(Point3::new(0.25, 0.5, 1.0), Vec3::new(0.0, 0.0, -1.0));
        let hit = tri.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!((hit.u - 0.25).abs() < 1e-5 && (hit.v - 0.5).abs() < 1e-5);
    }

    #[test]
    fn shading_normal_faces_the_ray() {
        // From below (+z ray going up), front_face flips; the shading normal must
        // still point toward the incoming ray (negative z component here).
        let t = tri();
        let ray = Ray::new(Point3::new(0.3, 0.3, -1.0), Vec3::new(0.0, 0.0, 1.0));
        let hit = t.intersect(&ray, &Interval::new(0.001, f32::INFINITY)).unwrap();
        assert!(hit.normal.dot(&ray.direction) < 0.0, "normal not facing ray: {:?}", hit.normal);
    }
}

#[cfg(test)]
mod area_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use std::sync::Arc;

    #[test]
    fn area_is_half_cross_product() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 3.0, 0.0),
            mat,
        );
        // |u x v| = |(2,0,0) x (0,3,0)| = 6; triangle area = 3.
        assert!((tri.area() - 3.0).abs() < 1e-5);
    }

    // The bounding box must enclose exactly the three vertices — not the 4th
    // corner of the parallelogram spanned by u and v (q+u+v = p2+p3-p1), which
    // lies outside the triangle and would inflate the box. An inflated box shifts
    // the BVH centre away from the true vertex centre, desyncing the path-traced
    // mesh from the GL preview and the transform gizmo.
    #[test]
    fn bounding_box_encloses_only_the_three_vertices() {
        use crate::ray::Intersect;
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        // Vertices (0,0,0), (1,0,0), (1,1,0). The phantom 4th corner is
        // p2+p3-p1 = (2,1,0), whose x=2 lies outside the true bbox [0,1].
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(1.0, 1.0, 0.0),
            mat,
        );
        let bb = tri.bounding_box();
        let (mn, mx) = (bb.min_vec(), bb.max_vec());
        // Padding on the flat z axis is fine (~1e-4); x/y must be tight to [0,1].
        assert!(mn.x >= -1e-3 && mx.x <= 1.0 + 1e-3, "x not tight: [{}, {}]", mn.x, mx.x);
        assert!(mn.y >= -1e-3 && mx.y <= 1.0 + 1e-3, "y not tight: [{}, {}]", mn.y, mx.y);
    }

    // A degenerate (zero-area) triangle has collinear vertices, so n = u x v is
    // the zero vector and n·n = 0. Real meshes contain these (e.g. imported OBJs),
    // so construction must not divide by zero — it must produce a harmless,
    // non-intersecting triangle instead of panicking.
    #[test]
    fn degenerate_triangle_does_not_panic() {
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        // Three collinear points along x (same y, same z).
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(2.0, 0.0, 0.0),
            mat,
        );
        assert!(tri.area() < 1e-6, "degenerate triangle should have ~zero area");
        // A ray that would hit the line must simply miss (no panic, no hit).
        let ray = crate::ray::Ray::new(
            Point3::new(0.5, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
        );
        assert!(
            tri.intersect(&ray, &crate::interval::Interval::new(0.001, f32::INFINITY)).is_none(),
            "degenerate triangle must not register intersections",
        );
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;
    use crate::color::Color;
    use crate::material::Lambertian;
    use crate::vec3::{Point3, Vec3};
    use rand::rngs::SmallRng;
    use rand::{Rng, SeedableRng};
    use std::sync::Arc;

    #[test]
    fn sampled_point_is_inside_triangle() {
        // Vertices q=(0,0,0), q+u=(1,0,0), q+v=(0,1,0): a sample (x,y,0) must
        // satisfy the barycentric bounds x>=0, y>=0, x+y<=1.
        let mat = Arc::new(Lambertian::from_color(Color::new(0.0, 0.0, 0.0)));
        let tri = Triangle::new(
            Point3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            mat,
        );
        let mut rng = SmallRng::seed_from_u64(2);
        let mut xs: Vec<f32> = Vec::with_capacity(500);
        let mut ys: Vec<f32> = Vec::with_capacity(500);
        for _ in 0..500 {
            let p = tri.sample_point(rng.random::<f32>(), rng.random::<f32>());
            assert!(p.z.abs() < 1e-5, "off-plane: {:?}", p);
            assert!(p.x >= -1e-5 && p.y >= -1e-5, "negative bary: {:?}", p);
            assert!(p.x + p.y <= 1.0 + 1e-5, "outside tri: {:?}", p);
            xs.push(p.x);
            ys.push(p.y);
        }
        // Spread check: the centroid is (1/3, 1/3, 0). If sample_point were replaced
        // by the default center() all 500 x-values would equal ~0.333 and y-values
        // would equal ~0.333.  A real uniform sample must spread across the triangle.
        let x_range = xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let y_range = ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max)
            - ys.iter().cloned().fold(f32::INFINITY, f32::min);
        assert!(x_range > 0.5, "x spread too small ({}); samples may not vary", x_range);
        assert!(y_range > 0.5, "y spread too small ({}); samples may not vary", y_range);
    }
}
