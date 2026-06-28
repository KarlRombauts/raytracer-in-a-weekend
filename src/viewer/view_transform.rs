use eframe::egui;

/// View transform for the rendered image. `zoom` multiplies the fit-to-window
/// size (1.0 = fit); `pan` offsets the image centre from the viewport centre,
/// in screen px.
pub struct ViewTransform {
    zoom: f32,
    pan: egui::Vec2,
}

impl ViewTransform {
    pub fn new() -> Self {
        ViewTransform {
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }

    /// Reset to the default fit-to-window view.
    pub fn reset(&mut self) {
        self.zoom = 1.0;
        self.pan = egui::Vec2::ZERO;
    }

    /// Pan by a screen-space delta (e.g. a drag).
    pub fn pan_by(&mut self, delta: egui::Vec2) {
        self.pan += delta;
    }

    /// Base "fit to window" size for `aspect`, preserving aspect ratio.
    fn fit(vp: egui::Rect, aspect: f32) -> egui::Vec2 {
        let mut fit = egui::vec2(vp.width(), vp.width() / aspect);
        if fit.y > vp.height() {
            fit = egui::vec2(vp.height() * aspect, vp.height());
        }
        fit
    }

    /// Zoom by scroll `delta`, keeping the point under `cursor` fixed on screen.
    pub fn zoom_at(&mut self, vp: egui::Rect, aspect: f32, cursor: egui::Pos2, scroll: f32) {
        let fit = Self::fit(vp, aspect);
        let old_disp = fit * self.zoom;
        let old_top_left = vp.center() + self.pan - old_disp * 0.5;

        let new_zoom = (self.zoom * (scroll * 0.002).exp()).clamp(0.1, 40.0);
        let factor = new_zoom / self.zoom;
        // Anchor the cursor: screen point under it must not move.
        let new_top_left = cursor + (old_top_left - cursor) * factor;
        let new_disp = fit * new_zoom;
        self.pan = (new_top_left + new_disp * 0.5) - vp.center();
        self.zoom = new_zoom;
    }

    /// Rectangle to paint the image into, for the given viewport and aspect.
    pub fn image_rect(&self, vp: egui::Rect, aspect: f32) -> egui::Rect {
        let disp = Self::fit(vp, aspect) * self.zoom;
        let top_left = vp.center() + self.pan - disp * 0.5;
        egui::Rect::from_min_size(top_left, disp)
    }
}
