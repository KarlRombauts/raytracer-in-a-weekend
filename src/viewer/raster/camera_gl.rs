//! View / projection / model matrices for the rasterized preview, built to
//! match the path tracer's framing so Edit and Render line up.

use glam::{Mat4, Vec3};

use crate::camera::CameraConfig;
use crate::scene::Transform;

fn g(v: crate::vec3::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Right-handed view matrix; `v_up` rolled about the forward axis by `roll`
/// (degrees), matching `Camera::from`.
pub fn view_matrix(cam: &CameraConfig) -> Mat4 {
    let eye = g(cam.look_from);
    let target = g(cam.look_at);
    let forward = (target - eye).normalize();
    let up = g(cam.v_up);
    let rolled_up = glam::Quat::from_axis_angle(forward, cam.roll.to_radians()) * up;
    Mat4::look_at_rh(eye, target, rolled_up)
}

/// GL-NDC perspective (z ∈ [−1, 1]) using the config's vertical fov.
pub fn projection_matrix(cam: &CameraConfig, near: f32, far: f32) -> Mat4 {
    let aspect = cam.image_width as f32
        / ((cam.image_width as f64 / cam.aspect_ratio) as f32).max(1.0);
    Mat4::perspective_rh_gl(cam.fov.to_radians(), aspect, near, far)
}

/// Object model matrix: scale + Euler rotation about `center`, then translate —
/// the same composition as `ObjectSpec::build`.
pub fn model_matrix(t: &Transform, center: Vec3) -> Mat4 {
    let translate = Mat4::from_translation(g(t.translate));
    let to_center = Mat4::from_translation(center);
    let from_center = Mat4::from_translation(-center);
    let rot = Mat4::from_euler(
        glam::EulerRot::XYZ,
        t.rotate.x.to_radians(),
        t.rotate.y.to_radians(),
        t.rotate.z.to_radians(),
    );
    let scale = Mat4::from_scale(g(t.scale));
    translate * to_center * rot * scale * from_center
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CameraConfig {
        CameraConfig::builder()
            .look_from(crate::vec3::Vec3::new(0.0, 0.0, 5.0))
            .look_at(crate::vec3::Vec3::new(0.0, 0.0, 0.0))
            .v_up(crate::vec3::Vec3::new(0.0, 1.0, 0.0))
            .build()
    }

    #[test]
    fn view_places_look_from_at_origin_looking_down_neg_z() {
        let v = view_matrix(&cfg());
        // The camera position maps to the origin in view space.
        let eye = v.transform_point3(Vec3::new(0.0, 0.0, 5.0));
        assert!(eye.length() < 1e-4, "eye={eye:?}");
        // A point in front of the camera (toward look_at) has negative view z.
        let front = v.transform_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!(front.z < 0.0, "front={front:?}");
    }

    #[test]
    fn projection_keeps_a_centered_point_centered() {
        let p = projection_matrix(&cfg(), 0.1, 100.0);
        let clip = p.project_point3(Vec3::new(0.0, 0.0, -5.0));
        assert!(clip.x.abs() < 1e-4 && clip.y.abs() < 1e-4, "clip={clip:?}");
    }

    #[test]
    fn model_identity_transform_is_identity() {
        let m = model_matrix(&Transform::identity(), Vec3::ZERO);
        assert!((m - Mat4::IDENTITY).abs_diff_eq(Mat4::ZERO, 1e-5));
    }

    #[test]
    fn model_translate_moves_point() {
        let mut t = Transform::identity();
        t.translate = crate::vec3::Vec3::new(2.0, 0.0, 0.0);
        let m = model_matrix(&t, Vec3::ZERO);
        let p = m.transform_point3(Vec3::new(0.0, 0.0, 0.0));
        assert!((p - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5, "p={p:?}");
    }
}
