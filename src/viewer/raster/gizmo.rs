//! Transform gizmo overlay for the Edit-mode preview. Bridges the gizmo crate's
//! TRS (scale + quaternion + translation, in world space) to our `Transform`
//! (Euler degrees + per-axis scale + translate, applied about the object's own
//! pivot). The conversion is the only fiddly part, so it lives in pure,
//! unit-tested functions separate from the egui glue.

use eframe::egui;
use transform_gizmo_egui::math::Transform as GizmoTransform;
use transform_gizmo_egui::prelude::*;

use crate::scene::Transform;
use crate::vec3::Vec3;

fn to_dvec3(v: Vec3) -> glam::DVec3 {
    glam::DVec3::new(v.x as f64, v.y as f64, v.z as f64)
}

fn from_dvec3(v: glam::DVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, v.z as f32)
}

/// Quaternion for our Euler degrees, composed `Rz · Ry · Rx` — the exact order
/// `camera_gl::model_matrix` uses, so the gizmo's orientation matches the
/// rendered object.
fn euler_deg_to_quat(rot_deg: Vec3) -> glam::DQuat {
    glam::DQuat::from_rotation_z((rot_deg.z as f64).to_radians())
        * glam::DQuat::from_rotation_y((rot_deg.y as f64).to_radians())
        * glam::DQuat::from_rotation_x((rot_deg.x as f64).to_radians())
}

/// Inverse of [`euler_deg_to_quat`]: Euler degrees `(x, y, z)` from a quaternion,
/// using the matching ZYX intrinsic convention.
fn quat_to_euler_deg(q: glam::DQuat) -> Vec3 {
    let (z, y, x) = q.to_euler(glam::EulerRot::ZYX);
    Vec3::new(
        (x.to_degrees()) as f32,
        (y.to_degrees()) as f32,
        (z.to_degrees()) as f32,
    )
}

/// Build the gizmo's world-space TRS for an object. The gizmo sits at the
/// object's pivot (its geometry centre), so its translation is `pivot +
/// transform.translate`; rotation and scale map directly.
pub fn to_gizmo_transform(t: &Transform, pivot: Vec3) -> GizmoTransform {
    GizmoTransform::from_scale_rotation_translation(
        to_dvec3(t.scale),
        euler_deg_to_quat(t.rotate),
        to_dvec3(pivot + t.translate),
    )
}

/// Read an edited gizmo TRS back into our `Transform`, undoing the pivot offset
/// so `translate` is again relative to the pivot.
pub fn from_gizmo_transform(g: &GizmoTransform, pivot: Vec3) -> Transform {
    let translation = from_dvec3(glam::DVec3::from(g.translation));
    Transform {
        scale: from_dvec3(glam::DVec3::from(g.scale)),
        rotate: quat_to_euler_deg(glam::DQuat::from(g.rotation)),
        translate: translation - pivot,
    }
}

/// Show and drive the gizmo for `transform` within `viewport`, using the same
/// `view`/`proj` the preview renders with. Returns whether the transform was
/// edited this frame. `pivot` is the object's geometry centre (where the gizmo
/// sits). Call [`Gizmo::is_focused`] afterwards to know if the gizmo captured
/// the pointer (so the caller can suppress camera orbit / click-select).
/// Which handle groups the gizmo shows. Translate arrows+planes, rotate rings,
/// scale handles — each toggled independently. Deliberately never the Arcball
/// trackball (it overlaps the rotate rings and makes them hard to grab).
#[derive(Clone, Copy)]
pub struct GizmoModes {
    pub translate: bool,
    pub rotate: bool,
    pub scale: bool,
}

fn mode_set(m: GizmoModes) -> EnumSet<GizmoMode> {
    let mut set = EnumSet::empty();
    if m.translate {
        set |= GizmoMode::TranslateX
            | GizmoMode::TranslateY
            | GizmoMode::TranslateZ
            | GizmoMode::TranslateXY
            | GizmoMode::TranslateXZ
            | GizmoMode::TranslateYZ;
    }
    if m.rotate {
        // RotateView is the white, screen-facing ring.
        set |= GizmoMode::RotateX
            | GizmoMode::RotateY
            | GizmoMode::RotateZ
            | GizmoMode::RotateView;
    }
    if m.scale {
        set |= GizmoMode::ScaleX | GizmoMode::ScaleY | GizmoMode::ScaleZ | GizmoMode::ScaleUniform;
    }
    set
}

/// Blender-style axis colours: X red, Y green, Z blue, the view ring white, and
/// an orange highlight for the hovered/active handle.
fn visuals() -> GizmoVisuals {
    GizmoVisuals {
        x_color: Color32::from_rgb(235, 60, 50),
        y_color: Color32::from_rgb(130, 200, 60),
        z_color: Color32::from_rgb(70, 140, 230),
        s_color: Color32::from_rgb(230, 230, 230),
        highlight_color: Some(Color32::from_rgb(245, 160, 40)),
        stroke_width: 3.0,
        ..Default::default()
    }
}

pub fn interact(
    ui: &egui::Ui,
    gizmo: &mut Gizmo,
    view: glam::Mat4,
    proj: glam::Mat4,
    viewport: egui::Rect,
    local: bool,
    modes: GizmoModes,
    transform: &mut Transform,
    pivot: Vec3,
) -> bool {
    let orientation = if local {
        GizmoOrientation::Local
    } else {
        GizmoOrientation::Global
    };
    gizmo.update_config(GizmoConfig {
        view_matrix: view.as_dmat4().into(),
        projection_matrix: proj.as_dmat4().into(),
        viewport,
        modes: mode_set(modes),
        orientation,
        visuals: visuals(),
        // Match the egui pixel ratio so handle positions line up with the cursor
        // on high-DPI / web canvases.
        pixels_per_point: ui.ctx().pixels_per_point(),
        ..Default::default()
    });
    let current = to_gizmo_transform(transform, pivot);

    // Drive the gizmo ourselves instead of `Gizmo::interact`. The crate's egui
    // wrapper derives `hovered` from a 1px interaction rect placed at the cursor;
    // the instant a press carries any motion — as the browser does when it
    // coalesces mousedown with the first mousemove — the pointer leaves that 1px
    // rect, `hovered` reads false on the drag-start frame, the handle is never
    // grabbed, and the drag falls through to camera orbit. Using the gizmo's own
    // geometric pick for `hovered` is motion-independent, so a handle press grabs
    // reliably. `pick_preview` is the same hit-test that drives the hover
    // highlight, so "if it highlights, it grabs".
    let cursor = ui.input(|i| i.pointer.hover_pos()).unwrap_or_default();
    let hovered = gizmo.pick_preview((cursor.x, cursor.y));
    let drag_started = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    let dragging = ui.input(|i| i.pointer.button_down(egui::PointerButton::Primary));

    let result = gizmo.update(
        transform_gizmo_egui::GizmoInteraction {
            cursor_pos: (cursor.x, cursor.y),
            hovered,
            drag_started,
            dragging,
        },
        &[current],
    );

    // Paint the gizmo (same mesh build as the crate's egui wrapper).
    let draw_data = gizmo.draw();
    egui::Painter::new(ui.ctx().clone(), ui.layer_id(), viewport).add(egui::Mesh {
        indices: draw_data.indices,
        vertices: draw_data
            .vertices
            .into_iter()
            .zip(draw_data.colors)
            .map(|(pos, [r, g, b, a])| egui::epaint::Vertex {
                pos: pos.into(),
                uv: egui::Pos2::default(),
                color: egui::Rgba::from_rgba_premultiplied(r, g, b, a).into(),
            })
            .collect(),
        ..Default::default()
    });

    if let Some((_result, updated)) = result {
        if let Some(new) = updated.first() {
            *transform = from_gizmo_transform(new, pivot);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Vec3, b: Vec3, eps: f32) -> bool {
        (a.x - b.x).abs() < eps && (a.y - b.y).abs() < eps && (a.z - b.z).abs() < eps
    }

    #[test]
    fn euler_quat_round_trips() {
        // Representative non-degenerate rotations (avoid the ±90° Y gimbal lock).
        for &e in &[
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(30.0, 0.0, 0.0),
            Vec3::new(0.0, 45.0, 0.0),
            Vec3::new(0.0, 0.0, 60.0),
            Vec3::new(20.0, 35.0, -50.0),
        ] {
            let back = quat_to_euler_deg(euler_deg_to_quat(e));
            assert!(approx(back, e, 1e-3), "euler round-trip: {e:?} -> {back:?}");
        }
    }

    /// The quaternion convention must match `model_matrix`'s `Rz·Ry·Rx`, so a
    /// vector rotated by our quat equals one rotated by the explicit product.
    #[test]
    fn quat_matches_model_matrix_rotation_order() {
        let e = Vec3::new(20.0, 35.0, -50.0);
        let q = euler_deg_to_quat(e);
        let explicit = glam::DMat4::from_rotation_z((e.z as f64).to_radians())
            * glam::DMat4::from_rotation_y((e.y as f64).to_radians())
            * glam::DMat4::from_rotation_x((e.x as f64).to_radians());
        let p = glam::DVec3::new(1.0, 2.0, 3.0);
        let got = q * p;
        let want = explicit.transform_point3(p);
        assert!((got - want).length() < 1e-9, "got={got:?} want={want:?}");
    }

    #[test]
    fn transform_round_trips_through_gizmo() {
        let pivot = Vec3::new(2.0, -1.0, 0.5);
        let t = Transform {
            rotate: Vec3::new(15.0, -25.0, 40.0),
            scale: Vec3::new(1.5, 2.0, 0.75),
            translate: Vec3::new(3.0, 4.0, -2.0),
        };
        let back = from_gizmo_transform(&to_gizmo_transform(&t, pivot), pivot);
        assert!(approx(back.rotate, t.rotate, 1e-3), "rotate {:?} {:?}", back.rotate, t.rotate);
        assert!(approx(back.scale, t.scale, 1e-5), "scale");
        assert!(approx(back.translate, t.translate, 1e-5), "translate");
    }

    /// Moving the gizmo by a world delta moves `translate` by the same delta,
    /// independent of the pivot.
    #[test]
    fn gizmo_translation_maps_to_transform_translate() {
        let pivot = Vec3::new(5.0, 5.0, 5.0);
        let t = Transform::identity();
        let mut g = to_gizmo_transform(&t, pivot);
        // Push the gizmo +1 on X in world space.
        let mut tr = glam::DVec3::from(g.translation);
        tr.x += 1.0;
        g.translation = tr.into();
        let back = from_gizmo_transform(&g, pivot);
        assert!(approx(back.translate, Vec3::new(1.0, 0.0, 0.0), 1e-5), "{:?}", back.translate);
    }
}
