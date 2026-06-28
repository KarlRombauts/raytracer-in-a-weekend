//! Pure camera-manipulation math for Edit mode. Each function mutates the live
//! `CameraConfig` in place, deriving its working state from `look_from`/
//! `look_at` each call — no stored orbit state to drift out of sync.

use eframe::egui;

use crate::camera::CameraConfig;
use crate::vec3::Vec3;

const ORBIT_SENS: f32 = 0.005; // radians per pixel
const PAN_SENS: f32 = 0.0015; // world units per pixel, per unit distance
const DOLLY_SENS: f32 = 0.001; // per scroll unit
const MIN_RADIUS: f32 = 0.05;
const MAX_ELEVATION: f32 = 89.0; // degrees

/// Orbit `look_from` around `look_at`. Horizontal drag changes azimuth,
/// vertical drag changes elevation (clamped to ±89°). `v_up` is untouched.
pub fn orbit(cam: &mut CameraConfig, delta: egui::Vec2) {
    let offset = cam.look_from - cam.look_at;
    let radius = offset.length();
    if radius < 1e-6 {
        return;
    }
    let mut azimuth = offset.z.atan2(offset.x);
    let mut elevation = (offset.y / radius).clamp(-1.0, 1.0).asin();

    azimuth += delta.x * ORBIT_SENS;
    elevation += delta.y * ORBIT_SENS;
    let max_el = MAX_ELEVATION.to_radians();
    elevation = elevation.clamp(-max_el, max_el);

    let new = Vec3::new(
        radius * elevation.cos() * azimuth.cos(),
        radius * elevation.sin(),
        radius * elevation.cos() * azimuth.sin(),
    );
    cam.look_from = cam.look_at + new;
}

/// Pan: slide both `look_from` and `look_at` along the camera's right/up axes,
/// scaled by distance so it feels roughly 1:1. View direction is preserved.
pub fn pan(cam: &mut CameraConfig, delta: egui::Vec2) {
    let forward = cam.look_at - cam.look_from;
    let dist = forward.length();
    if dist < 1e-6 {
        return;
    }
    let w = forward / dist;
    let right = w.cross(&cam.v_up).unit();
    let up = right.cross(&w);
    let scale = dist * PAN_SENS;
    let translate = right * (-delta.x * scale) + up * (delta.y * scale);
    cam.look_from = cam.look_from + translate;
    cam.look_at = cam.look_at + translate;
}

/// Dolly: move `look_from` toward/away from `look_at`. Positive scroll moves in.
pub fn dolly(cam: &mut CameraConfig, scroll: f32) {
    let offset = cam.look_from - cam.look_at;
    let radius = offset.length();
    if radius < 1e-6 {
        return;
    }
    let new_radius = (radius * (-scroll * DOLLY_SENS).exp()).max(MIN_RADIUS);
    cam.look_from = cam.look_at + offset * (new_radius / radius);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam() -> CameraConfig {
        CameraConfig::builder()
            .look_from(Vec3::new(0.0, 0.0, 5.0))
            .look_at(Vec3::new(0.0, 0.0, 0.0))
            .v_up(Vec3::new(0.0, 1.0, 0.0))
            .build()
    }

    fn radius(c: &CameraConfig) -> f32 {
        (c.look_from - c.look_at).length()
    }

    #[test]
    fn orbit_zero_delta_is_noop() {
        let mut c = cam();
        let before = c.look_from;
        orbit(&mut c, egui::Vec2::ZERO);
        assert!((c.look_from - before).length() < 1e-4);
    }

    #[test]
    fn orbit_preserves_radius() {
        let mut c = cam();
        orbit(&mut c, egui::vec2(120.0, 40.0));
        assert!((radius(&c) - 5.0).abs() < 1e-3, "radius={}", radius(&c));
    }

    #[test]
    fn orbit_clamps_elevation() {
        let mut c = cam();
        // Huge upward drag must not flip past the pole.
        orbit(&mut c, egui::vec2(0.0, 100_000.0));
        let offset = c.look_from - c.look_at;
        let elevation = (offset.y / radius(&c)).asin().to_degrees();
        assert!(elevation <= 89.0 + 1e-2 && elevation >= 88.9, "elev={}", elevation);
    }

    #[test]
    fn dolly_in_shrinks_radius_and_respects_min() {
        let mut c = cam();
        dolly(&mut c, 500.0); // strong zoom-in
        assert!(radius(&c) < 5.0);
        assert!(radius(&c) >= 0.05 - 1e-6, "radius={}", radius(&c));
    }

    #[test]
    fn pan_is_perpendicular_and_preserves_view_direction() {
        let mut c = cam();
        let dir_before = (c.look_at - c.look_from).unit();
        let from_before = c.look_from;
        pan(&mut c, egui::vec2(50.0, 0.0));
        let dir_after = (c.look_at - c.look_from).unit();
        // View direction unchanged.
        assert!((dir_after - dir_before).length() < 1e-4);
        // Movement perpendicular to the view direction.
        let moved = c.look_from - from_before;
        assert!(moved.dot(&dir_before).abs() < 1e-4, "dot={}", moved.dot(&dir_before));
        assert!(moved.length() > 1e-4);
    }
}
