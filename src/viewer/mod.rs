mod controls;
mod icons;
mod orbit;
mod panels;
mod raster;
mod render_task;
mod state;
pub mod theme;
mod view_transform;
mod texture_library;
mod widgets;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::camera::Camera;
use crate::scene::Scene;
use render_task::RenderTask;
use view_transform::ViewTransform;

use state::Mode;

/// Resolution divisor used while actively orbiting/panning, so the view tracks
/// the mouse; the render snaps back to full quality once motion stops.
const PREVIEW_SCALE: u32 = 4;
/// Seconds of stillness after the last camera motion before switching back to
/// full-resolution rendering.
const PREVIEW_DEBOUNCE: f32 = 0.15;

/// Encode an already-gamma-corrected RGBA buffer to PNG bytes (RGB, opaque).
fn encode_rgba_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut rgb = image::RgbImage::new(width, height);
    for (i, px) in rgba.chunks_exact(4).enumerate() {
        let x = i as u32 % width.max(1);
        let y = i as u32 / width.max(1);
        rgb.put_pixel(x, y, image::Rgb([px[0], px[1], px[2]]));
    }
    let mut bytes = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::Png,
    )
    .expect("PNG encode");
    bytes
}

/// A small PNG preview (≤256px on the long edge, aspect preserved) of the given
/// RGBA frame, for embedding in a `.scene` file. Empty if the buffer is empty.
fn scene_thumbnail(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    if width == 0 || height == 0 || rgba.len() != (width * height * 4) as usize {
        return Vec::new();
    }
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .expect("rgba buffer matches dimensions");
    // `thumbnail` preserves aspect within the 256x256 box.
    let thumb = image::DynamicImage::ImageRgba8(img).thumbnail(256, 256);
    let mut bytes = Vec::new();
    thumb
        .to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("PNG encode");
    bytes
}

/// Open a window and progressively render `scene`, refining one sample-per-pixel
/// pass at a time. The side panel edits the scene live; each edit cancels the
/// in-flight render and restarts. A Save image button lets the user explicitly
/// save the current render.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(scene: Scene) {
    let camera = Camera::from(scene.camera.clone());
    let width = camera.image_width();
    let height = camera.image_height();

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        // The Edit-mode GL preview needs a depth buffer; egui's surface has
        // none by default, so DEPTH_TEST would otherwise be a silent no-op.
        depth_buffer: 24,
        // Multisample the window framebuffer so the rasterized preview's
        // geometry edges are antialiased (the paint callback draws into it).
        multisampling: 4,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([width as f32 + 290.0, height as f32 + 48.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Raytracer",
        options,
        Box::new(move |cc| Ok(Box::new(ViewerApp::new(cc, scene, width, height)))),
    )
    .unwrap();
}

/// Build the viewer app for the web runner (mirrors `run` minus the native
/// window setup). Public so `lib::web` can construct it.
#[cfg(target_arch = "wasm32")]
pub fn web_app(cc: &eframe::CreationContext<'_>, scene: Scene) -> ViewerApp {
    let camera = Camera::from(scene.camera.clone());
    let width = camera.image_width();
    let height = camera.image_height();
    ViewerApp::new(cc, scene, width, height)
}

pub struct ViewerApp {
    scene: Arc<Mutex<Scene>>,
    render: RenderTask,
    texture: Option<egui::TextureHandle>,
    shown_pass: u32,
    view: ViewTransform,
    initial_camera: crate::camera::CameraConfig,
    /// GL rasterizer for the Edit-mode preview.
    gl_renderer: Arc<Mutex<raster::renderer::SceneRenderer>>,
    /// Persistent transform-gizmo state (holds drag state between frames).
    gizmo: transform_gizmo_egui::Gizmo,
    /// All editor UI state (mode, selection, inspector tab, gizmo options,
    /// preview debounce clock) lives here.
    ui_state: state::UiState,
    /// In-flight scene-file load (None when idle).
    scene_picker: Option<crate::platform::ScenePicker>,
    /// Transient status line for scene save/load (message + egui-time when set).
    /// Auto-dismissed by the toast a few seconds after it appears.
    scene_status: Option<(String, f64)>,
}

impl ViewerApp {
    fn new(cc: &eframe::CreationContext<'_>, scene: Scene, width: u32, height: u32) -> Self {
        theme::install(&cc.egui_ctx);
        let total = scene.camera.samples;
        let initial_camera = scene.camera.clone();
        let scene = Arc::new(Mutex::new(scene));
        let render = RenderTask::spawn(cc.egui_ctx.clone(), scene.clone(), width, height, total);

        let gl = cc.gl.as_ref().expect("eframe glow context");
        let gl_renderer = Arc::new(Mutex::new(raster::renderer::SceneRenderer::new(gl)));

        ViewerApp {
            scene,
            render,
            texture: None,
            shown_pass: 0,
            view: ViewTransform::new(),
            initial_camera,
            gl_renderer,
            gizmo: transform_gizmo_egui::Gizmo::default(),
            ui_state: state::UiState::default(),
            scene_picker: None,
            scene_status: None,
        }
    }
}

#[cfg(test)]
mod thumb_tests {
    use super::*;

    #[test]
    fn thumbnail_shrinks_to_max_edge_and_keeps_aspect() {
        // 800x400 opaque buffer → thumbnail capped at 256 on the long edge.
        let (w, h) = (800u32, 400u32);
        let rgba = vec![128u8; (w * h * 4) as usize];
        let png = scene_thumbnail(&rgba, w, h);
        let img = image::load_from_memory(&png).expect("valid PNG");
        assert!(img.width() <= 256 && img.height() <= 256, "{}x{}", img.width(), img.height());
        assert_eq!(img.width(), 256); // long edge maps to 256
        assert_eq!(img.height(), 128); // aspect preserved
    }

    #[test]
    fn thumbnail_is_empty_for_empty_buffer() {
        assert!(scene_thumbnail(&[], 0, 0).is_empty());
    }
}

impl eframe::App for ViewerApp {
    // eframe 0.34 (same API as 0.35): hands the root `Ui` directly; we lay out panels inside it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // On wasm this advances the path trace one pass per frame (no render
        // thread in the browser); on native it is a no-op.
        self.render.pump();

        // egui time (seconds), used to time-stamp + auto-dismiss the status toast.
        let now = ui.input(|i| i.time);

        // Apply a completed scene load (the picker resolves asynchronously on web).
        if let Some(status) = self.scene_picker.as_ref().map(|p| p.poll()) {
            match status {
                crate::platform::PickStatus::Pending => {
                    ui.ctx().request_repaint(); // keep polling until it resolves
                }
                crate::platform::PickStatus::Done(bytes) => {
                    self.scene_picker = None;
                    match crate::scene_file::decode(&bytes) {
                        Ok(loaded) => {
                            let loaded_camera = loaded.scene.camera.clone();
                            *self.scene.lock().unwrap() = loaded.scene;
                            self.initial_camera = loaded_camera;
                            self.gl_renderer.lock().unwrap().mark_dirty();
                            self.ui_state.selected = None;
                            self.render.invalidate();
                            self.scene_status = Some(("Loaded scene".to_string(), now));
                        }
                        Err(e) => self.scene_status = Some((format!("Load failed: {e}"), now)),
                    }
                }
                crate::platform::PickStatus::Cancelled => self.scene_picker = None,
                crate::platform::PickStatus::Failed(e) => {
                    self.scene_picker = None;
                    self.scene_status = Some((format!("Load failed: {e}"), now));
                }
            }
        }

        // Pull the latest frame; rebuild the texture only when a new pass landed.
        // Dims come from the frame so a resolution change resizes the texture.
        let (img_w, img_h, passes, total, done, elapsed, new_image) = {
            let s = self.render.lock();
            let new_image = if s.passes != self.shown_pass {
                Some(egui::ColorImage::from_rgba_unmultiplied(
                    [s.width as usize, s.height as usize],
                    &s.rgba,
                ))
            } else {
                None
            };
            (
                s.width, s.height, s.passes, s.total, s.done, s.elapsed, new_image,
            )
        };
        let aspect = img_w as f32 / img_h as f32;
        if let Some(image) = new_image {
            // LINEAR filtering so the image stays smooth when scaled up or down.
            match &mut self.texture {
                Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.texture =
                        Some(ctx.load_texture("render", image, egui::TextureOptions::LINEAR));
                }
            }
            self.shown_pass = passes;
        }

        // Capture the mode before the panels render so we can detect a
        // Render↔Edit transition and pause/resume the path trace once.
        let mode_before = self.ui_state.mode;
        let mut dirty = false;
        let mut actions: Vec<panels::Action> = Vec::new();

        // --- Top bar: logo, scene chip, mode toggle, save buttons ---
        egui::Panel::top("top_bar")
            .exact_size(54.0)
            .frame(egui::Frame::NONE.fill(theme::BG_TOPBAR).inner_margin(egui::Margin::symmetric(14, 0)))
            .show_inside(ui, |ui| {
                let scene = self.scene.lock().unwrap();
                actions.push(panels::show_top_bar(ui, &mut self.ui_state, &scene));
            });

        // --- Left outliner: scene object rows + Add menu ---
        egui::Panel::left("outliner")
            .default_width(286.0)
            .width_range(220.0..=460.0)
            .resizable(true)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show_inside(ui, |ui| {
                let mut scene = self.scene.lock().unwrap();
                dirty |= panels::show_outliner(ui, &mut self.ui_state, &mut scene);
            });

        // --- Right inspector: Object / Camera / Output tabs ---
        egui::Panel::right("inspector")
            .default_width(342.0)
            .width_range(280.0..=520.0)
            .resizable(true)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_PANEL)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show_inside(ui, |ui| {
                let mut scene = self.scene.lock().unwrap();
                dirty |= panels::show_inspector(ui, &mut self.ui_state, &mut scene);
            });

        // --- Status dock: progress line + status + Samples/Bounces + restart ---
        egui::Panel::bottom("status_dock")
            .exact_size(63.0)
            .frame(egui::Frame::NONE.fill(theme::BG_TOPBAR).inner_margin(egui::Margin::symmetric(14, 0)))
            .show_inside(ui, |ui| {
                let mut scene = self.scene.lock().unwrap();
                let out = panels::status_dock(
                    ui,
                    self.ui_state.mode,
                    done,
                    passes,
                    total,
                    elapsed as f32,
                    &mut scene.camera,
                );
                if out.restart {
                    actions.push(panels::Action::Restart);
                }
                dirty |= out.dirty;
            });

        // --- Central viewport: path-traced image or GL preview + overlays ---
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG_VIEWPORT))
            .show_inside(ui, |ui| {
            let vp = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(vp, egui::Sense::click_and_drag());

            // The rect the path-traced image / GL preview occupies, captured so
            // the overlays can pin to it after painting.
            let mut image_rect = vp;

            match self.ui_state.mode {
                Mode::Render => {
                    // Leaving Edit mid-preview: restore full-resolution rendering.
                    if self.render.preview_scale() != 1 {
                        self.render.set_preview_scale(1);
                        self.render.invalidate();
                    }
                    // Drag to pan; double-click to reset the 2D view.
                    if response.dragged() {
                        self.view.pan_by(response.drag_delta());
                    }
                    if response.double_clicked() {
                        self.view.reset();
                    }
                    let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                    if response.hovered() && scroll != 0.0 {
                        let cursor = ui
                            .input(|i| i.pointer.hover_pos())
                            .unwrap_or_else(|| vp.center());
                        self.view.zoom_at(vp, aspect, cursor, scroll);
                    }

                    // Paint the path-traced image, clipped to the viewport so
                    // zoomed overflow hides.
                    match &self.texture {
                        Some(t) => {
                            let rect = self.view.image_rect(vp, aspect);
                            image_rect = rect.intersect(vp);
                            ui.painter_at(vp).image(
                                t.id(),
                                rect,
                                egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                ),
                                egui::Color32::WHITE,
                            );
                        }
                        None => {
                            ui.centered_and_justified(|ui| ui.label("Rendering…"));
                        }
                    }
                }
                Mode::Edit => {
                    let now = ui.input(|i| i.time);

                    // Letterbox to the output aspect (centred, unit zoom) so the
                    // preview lines up with the path-traced image in Render mode.
                    let mut fit = egui::vec2(vp.width(), vp.width() / aspect);
                    if fit.y > vp.height() {
                        fit = egui::vec2(vp.height() * aspect, vp.height());
                    }
                    let rect = egui::Rect::from_center_size(vp.center(), fit);
                    image_rect = rect;
                    let cam_proj = |s: &Scene| {
                        raster::camera_gl::projection_matrix(
                            &s.camera,
                            rect.width() / rect.height(),
                            0.05,
                            1000.0,
                        )
                    };

                    // 1) Paint the GL rasterised preview (drawn first, so the
                    //    gizmo overlays it). The outline tracks `selected`.
                    let scene_arc = self.scene.clone();
                    let renderer = self.gl_renderer.clone();
                    let selected = self.ui_state.selected;
                    let cb = eframe::egui_glow::CallbackFn::new(move |info, painter| {
                        let scene = scene_arc.lock().unwrap();
                        renderer
                            .lock()
                            .unwrap()
                            .paint(painter.gl(), &scene, &info, selected);
                    });
                    ui.painter().add(egui::PaintCallback {
                        rect,
                        callback: std::sync::Arc::new(cb),
                    });

                    // 2) Transform gizmo on the selected object. It takes pointer
                    //    precedence, so dragging a handle won't also orbit the
                    //    camera or reselect.
                    let mut gizmo_active = false;
                    let mut moved = false;
                    if let Some(i) = self.ui_state.selected {
                        let mut scene = self.scene.lock().unwrap();
                        if i < scene.objects.len() {
                            let view = raster::camera_gl::view_matrix(&scene.camera);
                            let proj = cam_proj(&scene);
                            let pivot = scene.objects[i].pivot();
                            let changed = raster::gizmo::interact(
                                ui,
                                &mut self.gizmo,
                                view,
                                proj,
                                rect,
                                self.ui_state.gizmo_local,
                                self.ui_state.gizmo_modes,
                                &mut scene.objects[i].transform,
                                pivot,
                            );
                            gizmo_active = changed || self.gizmo.is_focused();
                            drop(scene);
                            if changed {
                                // Show the new pose immediately; the path trace
                                // is paused in Edit and picks it up on resume.
                                moved = true;
                            }
                        }
                    }

                    // 3) Camera orbit/pan/dolly + click-to-select, suppressed
                    //    while the gizmo has the pointer. Driven from raw pointer
                    //    state rather than the panel `response`: the gizmo's own
                    //    interaction widget sits on top of the viewport and would
                    //    otherwise swallow every drag/click while it's shown.
                    let (down, clicked, scroll, ptr, origin, delta, shift) = ui.input(|i| {
                        (
                            i.pointer.primary_down(),
                            i.pointer.primary_clicked(),
                            i.smooth_scroll_delta.y,
                            i.pointer.interact_pos(),
                            i.pointer.press_origin(),
                            i.pointer.delta(),
                            i.modifiers.shift,
                        )
                    });
                    // Suppress viewport input when the pointer is over a floating
                    // widget on top of the preview (gizmo toolbar, reset chip,
                    // colour-picker popup, …). Those live in non-Background layers;
                    // the bare viewport is the Background layer. Without this, the
                    // raw pointer reads steal clicks/drags from those widgets —
                    // clicking a gizmo toggle would deselect the object, and
                    // dragging on the colour picker would orbit the camera.
                    let ctx = ui.ctx().clone();
                    let over_overlay = |p: egui::Pos2| {
                        ctx.layer_id_at(p)
                            .is_some_and(|l| l.order != egui::Order::Background)
                    };
                    if !gizmo_active {
                        // Orbit/pan only for drags that began inside the preview
                        // (and not on an overlay/popup floating above it).
                        let drag_in_view =
                            down && origin.is_some_and(|o| rect.contains(o) && !over_overlay(o));
                        if drag_in_view && delta != egui::Vec2::ZERO {
                            let mut scene = self.scene.lock().unwrap();
                            if shift {
                                orbit::pan(&mut scene.camera, delta);
                            } else {
                                orbit::orbit(&mut scene.camera, delta);
                            }
                            moved = true;
                        }
                        if scroll != 0.0 && ptr.is_some_and(|p| rect.contains(p) && !over_overlay(p)) {
                            orbit::dolly(&mut self.scene.lock().unwrap().camera, scroll);
                            moved = true;
                        }
                        // A click in the preview selects the object under it; a
                        // click on empty space clears the selection. Same
                        // view/projection as the preview, so the pick matches
                        // what's drawn (near/far don't affect the ray).
                        if clicked && ptr.is_some_and(|p| vp.contains(p) && !over_overlay(p)) {
                            let pos = ptr.unwrap();
                            if rect.contains(pos) {
                                let s = self.scene.lock().unwrap();
                                let view = raster::camera_gl::view_matrix(&s.camera);
                                let proj = cam_proj(&s);
                                let ndc = glam::Vec2::new(
                                    2.0 * (pos.x - rect.left()) / rect.width() - 1.0,
                                    1.0 - 2.0 * (pos.y - rect.top()) / rect.height(),
                                );
                                let ray = raster::pick::screen_ray(view, proj, ndc);
                                self.ui_state.selected = raster::pick::pick(&s, &ray);
                            } else {
                                self.ui_state.selected = None;
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                    if moved {
                        self.ui_state.last_interact = now;
                    }

                    // Reduced-resolution preview while actively interacting;
                    // snap back to full quality once motion has stopped. Inert in
                    // Edit (no invalidate), but kept for the Render handoff.
                    let interacting =
                        (now - self.ui_state.last_interact) < PREVIEW_DEBOUNCE as f64;
                    let want_scale = if interacting { PREVIEW_SCALE } else { 1 };
                    let scale_changed = self.render.preview_scale() != want_scale;
                    if scale_changed {
                        self.render.set_preview_scale(want_scale);
                    }
                    if moved || scale_changed {
                        // The GL view is instant; just request a repaint rather
                        // than restarting the path trace.
                        ui.ctx().request_repaint();
                    }
                    if interacting {
                        ui.ctx()
                            .request_repaint_after(Duration::from_secs_f32(PREVIEW_DEBOUNCE));
                    }
                }
            }

            // Floating overlays (resolution badge, Reset-camera chip, Edit
            // toolbar) on top of whatever was just painted.
            // R2+R7: overlays anchor to the full viewport rect (`vp`), not
            // the letterboxed image rect. `image_rect` stays on gizmo/GL.
            let mut gm = self.ui_state.gizmo_modes;
            let mut gl = self.ui_state.gizmo_local;
            if panels::overlays(
                ui,
                vp,
                self.ui_state.mode,
                &mut gm,
                &mut gl,
                (img_w, img_h),
            ) {
                actions.push(panels::Action::ResetCamera);
            }
            self.ui_state.gizmo_modes = gm;
            self.ui_state.gizmo_local = gl;
        });

        // --- Apply panel actions + dirty centrally (all scene locks released) ---
        for a in actions {
            match a {
                panels::Action::SaveImage => {
                    let bytes = {
                        // Re-encode the current shown frame from the shared RGBA
                        // buffer (already gamma-corrected).
                        let s = self.render.lock();
                        encode_rgba_png(&s.rgba, s.width, s.height)
                    };
                    crate::platform::save_png("render.png", &bytes);
                }
                panels::Action::ResetCamera => {
                    let mut scene = self.scene.lock().unwrap();
                    scene.camera = self.initial_camera.clone();
                    self.render.invalidate();
                }
                panels::Action::Restart => self.render.invalidate(),
                panels::Action::SaveScene => {
                    // Thumbnail from the currently displayed frame (gamma-corrected RGBA).
                    let (rgba, w, h) = {
                        let s = self.render.lock();
                        (s.rgba.clone(), s.width, s.height)
                    };
                    let preview = scene_thumbnail(&rgba, w, h);
                    let bytes = {
                        let scene = self.scene.lock().unwrap();
                        crate::scene_file::encode(&scene, None, &preview)
                    };
                    crate::platform::save_scene("scene.scene", &bytes);
                    self.scene_status = Some(("Saved scene".to_string(), now));
                }
                panels::Action::LoadScene => {
                    self.scene_picker = Some(crate::platform::pick_scene());
                    self.scene_status = None;
                }
                panels::Action::None => {}
            }
        }
        if dirty {
            self.render.invalidate();
        }

        // The path trace is wasted work in Edit mode (the GL preview is shown
        // instead), so pause it there and resume — restarting at the edited
        // scene — when returning to Render.
        if mode_before != self.ui_state.mode {
            match self.ui_state.mode {
                Mode::Edit => self.render.pause(),
                Mode::Render => self.render.resume(),
            }
        }

        // Scene save/load status toast: appears for a couple of seconds above the
        // status dock, then fades out and clears itself.
        if let Some((msg, set_at)) = self.scene_status.clone() {
            const TOAST_SECS: f64 = 2.4;
            const FADE_SECS: f64 = 0.6;
            let age = now - set_at;
            if age >= TOAST_SECS {
                self.scene_status = None;
            } else {
                // Hold at full opacity, then fade over the final FADE_SECS.
                let alpha = (((TOAST_SECS - age) / FADE_SECS).min(1.0)) as f32;
                egui::Area::new(egui::Id::new("scene_status"))
                    // Lift clear of the 63px status dock so it doesn't overlap controls.
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -80.0))
                    .interactable(false)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::NONE
                            .fill(theme::BG_TOPBAR.gamma_multiply(alpha))
                            .corner_radius(egui::CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(14, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(msg)
                                        .color(theme::TEXT_STRONG.gamma_multiply(alpha))
                                        .size(13.0),
                                );
                            });
                    });
                // Keep repainting so the timer advances + fade animates without
                // requiring other input events.
                ui.ctx().request_repaint();
            }
        }
    }
}
