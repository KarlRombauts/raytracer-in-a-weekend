//! Click-to-select picking for the Edit-mode preview: turn a click in the
//! letterboxed viewport into a world-space ray and find the nearest object it
//! hits. Reuses the path tracer's exact geometry (`ObjectSpec::build`), so a
//! pick corresponds to the true surface, not the rasterized approximation.

use glam::{Mat4, Vec2, Vec3 as GVec3, Vec4};

use crate::interval::Interval;
use crate::ray::Ray;
use crate::scene::Scene;
use crate::vec3::Vec3;

/// Build a world-space pick ray from a normalized-device-coordinate point.
/// `ndc` is in [-1, 1] with +y up (GL convention).
///
/// Independent of the near/far planes baked into `proj`: under a perspective
/// transform an eye ray projects to a vertical line in NDC, so unprojecting the
/// near (z = -1) and far (z = +1) points yields two distinct points on that
/// same ray regardless of where the planes sit.
pub fn screen_ray(view: Mat4, proj: Mat4, ndc: Vec2) -> Ray {
    let inv = (proj * view).inverse();
    let unproject = |z: f32| {
        let p = inv * Vec4::new(ndc.x, ndc.y, z, 1.0);
        p.truncate() / p.w
    };
    let near = unproject(-1.0);
    let far = unproject(1.0);
    let dir = (far - near).normalize();
    Ray::new(g2c(near), g2c(dir))
}

/// Index of the nearest object hit by `ray`, or `None` if it misses everything.
pub fn pick(scene: &Scene, ray: &Ray) -> Option<usize> {
    let range = Interval::new(1e-3, f32::INFINITY);
    let mut best: Option<(f32, usize)> = None;
    for (i, obj) in scene.objects.iter().enumerate() {
        if let Some(hit) = obj.build().intersect(ray, &range) {
            if best.is_none_or(|(t, _)| hit.t < t) {
                best = Some((hit.t, i));
            }
        }
    }
    best.map(|(_, i)| i)
}

fn g2c(v: GVec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

#[cfg(test)]
mod tests {
    use super::super::camera_gl;
    use super::*;
    use crate::camera::CameraConfig;
    use crate::color::Color;
    use crate::scene::{MaterialSpec, ObjectSpec, Shape, TextureSpec, Transform};

    fn looking_down_neg_z() -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 5.0))
            .look_at(Vec3::new(0.0, 0.0, 0.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .build()
    }

    fn matrices(cam: &CameraConfig) -> (Mat4, Mat4) {
        let view = camera_gl::view_matrix(cam);
        let proj = camera_gl::projection_matrix(cam, 1.0, 0.05, 1000.0);
        (view, proj)
    }

    fn sphere(name: &str, center: Vec3, radius: f32) -> ObjectSpec {
        ObjectSpec {
            name: name.to_string(),
            shape: Shape::Sphere { center, radius },
            material: MaterialSpec::Lambertian {
                albedo: TextureSpec::solid(Color::new(0.5, 0.5, 0.5)),
            },
            transform: Transform::identity(),
            hidden: false,
        }
    }

    #[test]
    fn center_click_casts_ray_toward_look_at() {
        let cam = looking_down_neg_z();
        let (view, proj) = matrices(&cam);
        let ray = screen_ray(view, proj, Vec2::ZERO);
        // Camera sits at +z looking at the origin, so the centre ray points -z.
        let d = ray.direction.unit();
        assert!(d.z < -0.99, "direction not toward look_at: {d:?}");
        assert!(
            d.x.abs() < 1e-3 && d.y.abs() < 1e-3,
            "centre ray off-axis: {d:?}"
        );
        // Ray starts in front of the eye (between eye and scene).
        assert!(ray.origin.z < 5.0, "origin behind eye: {:?}", ray.origin);
    }

    #[test]
    fn picks_nearer_of_two_objects_on_axis() {
        let cam = looking_down_neg_z();
        let (view, proj) = matrices(&cam);
        let scene = Scene {
            camera: cam,
            // index 0 is farther (z=-3), index 1 is nearer the camera (z=0).
            objects: vec![
                sphere("far", Vec3::new(0.0, 0.0, -3.0), 1.0),
                sphere("near", Vec3::new(0.0, 0.0, 0.0), 1.0),
            ],
        };
        let ray = screen_ray(view, proj, Vec2::ZERO);
        assert_eq!(pick(&scene, &ray), Some(1));
    }

    #[test]
    fn returns_none_when_ray_misses_everything() {
        let cam = looking_down_neg_z();
        let (view, proj) = matrices(&cam);
        let scene = Scene {
            camera: cam,
            objects: vec![sphere("offscreen", Vec3::new(20.0, 0.0, 0.0), 1.0)],
        };
        let ray = screen_ray(view, proj, Vec2::ZERO);
        assert_eq!(pick(&scene, &ray), None);
    }
}
