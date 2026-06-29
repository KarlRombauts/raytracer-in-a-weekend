mod controls;
mod icons;
mod orbit;
mod raster;
mod render_task;
mod view_transform;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;

use crate::camera::Camera;
use crate::scene::Scene;
use render_task::RenderTask;
use view_transform::ViewTransform;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Render,
    Edit,
}

/// Resolution divisor used while actively orbiting/panning, so the view tracks
/// the mouse; the render snaps back to full quality once motion stops.
const PREVIEW_SCALE: u32 = 4;
/// Seconds of stillness after the last camera motion before switching back to
/// full-resolution rendering.
const PREVIEW_DEBOUNCE: f32 = 0.15;

/// A rounded, slightly-darker card to group related controls.
fn card(ui: &egui::Ui) -> egui::Frame {
    egui::Frame::group(ui.style())
        .fill(egui::Color32::from_black_alpha(40))
        .corner_radius(egui::CornerRadius::same(6))
}

/// Open a window and progressively render `scene`, refining one sample-per-pixel
/// pass at a time. The side panel edits the scene live; each edit cancels the
/// in-flight render and restarts. Saves `test.png` when a render completes.
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

struct ViewerApp {
    scene: Arc<Mutex<Scene>>,
    render: RenderTask,
    texture: Option<egui::TextureHandle>,
    shown_pass: u32,
    view: ViewTransform,
    /// Index of the object whose settings are shown (UI state, not scene data).
    selected: Option<usize>,
    mode: Mode,
    initial_camera: crate::camera::CameraConfig,
    /// egui time (seconds) of the last camera motion, for the preview debounce.
    last_interact: f64,
    /// GL rasterizer for the Edit-mode preview.
    gl_renderer: Arc<Mutex<raster::renderer::SceneRenderer>>,
    /// Persistent transform-gizmo state (holds drag state between frames).
    gizmo: transform_gizmo_egui::Gizmo,
    /// Whether the gizmo manipulates in world (global) or object-local axes.
    gizmo_local: bool,
    /// Which handle groups (translate / rotate / scale) the gizmo shows.
    gizmo_modes: raster::gizmo::GizmoModes,
}

impl ViewerApp {
    fn new(cc: &eframe::CreationContext<'_>, scene: Scene, width: u32, height: u32) -> Self {
        icons::install(&cc.egui_ctx);
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
            selected: None,
            mode: Mode::Render,
            initial_camera,
            last_interact: -1.0,
            gl_renderer,
            gizmo: transform_gizmo_egui::Gizmo::default(),
            gizmo_local: false,
            gizmo_modes: raster::gizmo::GizmoModes {
                translate: true,
                rotate: true,
                scale: true,
            },
        }
    }
}

impl eframe::App for ViewerApp {
    // eframe 0.34 (same API as 0.35): hands the root `Ui` directly; we lay out panels inside it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // On wasm this advances the path trace one pass per frame (no render
        // thread in the browser); on native it is a no-op.
        self.render.pump();

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
            (s.width, s.height, s.passes, s.total, s.done, s.elapsed, new_image)
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

        // --- Side panel: status + editable scene ---
        // Capture the mode before the panel renders so we can detect a
        // Edit→Render transition and invalidate the path trace once.
        let mode_before = self.mode;
        let mut dirty = false;
        let mut selected = self.selected;
        {
            let scene_arc = self.scene.clone();
            let mut scene = scene_arc.lock().unwrap();
            egui::Panel::left("controls")
                .resizable(true)
                .default_size(270.0)
                .show(ui, |ui| {
                    let frac = if total > 0 {
                        passes as f32 / total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_height(14.0)
                            .text(format!("pass {} / {}   ·   {:.1}s", passes, total, elapsed)),
                    );
                    ui.horizontal(|ui| {
                        if done {
                            ui.label("done — saved test.png");
                        } else {
                            ui.spinner();
                            ui.label("rendering…");
                        }
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.mode, Mode::Render, "Render");
                        ui.selectable_value(&mut self.mode, Mode::Edit, "Edit");
                        if ui.button("Reset camera").clicked() {
                            scene.camera = self.initial_camera.clone();
                            self.render.invalidate();
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::CollapsingHeader::new(format!("{}  Camera", icons::CAMERA))
                            .default_open(true)
                            .show(ui, |ui| {
                                card(ui).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    dirty |= controls::camera_controls(ui, &mut scene.camera);
                                });
                            });
                        egui::CollapsingHeader::new(format!("{}  Objects", icons::STACK))
                            .default_open(true)
                            .show(ui, |ui| {
                                // Object list, in a card.
                                card(ui).show(ui, |ui| {
                                    ui.set_min_width(ui.available_width());
                                    dirty |= controls::object_list(
                                        ui,
                                        &mut scene.objects,
                                        &mut selected,
                                    );
                                });
                                ui.add_space(6.0);

                                // Selected-object settings, in their own card.
                                match selected {
                                    Some(i) if i < scene.objects.len() => {
                                        card(ui).show(ui, |ui| {
                                            ui.set_min_width(ui.available_width());
                                            dirty |= controls::object_settings(
                                                ui,
                                                &mut scene.objects[i],
                                            );
                                            ui.add_space(6.0);
                                            if ui
                                                .button(format!("{}  Delete", icons::TRASH))
                                                .clicked()
                                            {
                                                scene.objects.remove(i);
                                                selected = None;
                                                dirty = true;
                                            }
                                        });
                                    }
                                    Some(_) => selected = None,
                                    None => {
                                        ui.weak("Select an object to edit its settings.");
                                    }
                                }
                            });
                    });
                });
        }
        self.selected = selected;
        if dirty {
            self.render.invalidate();
        }
        // The path trace is wasted work in Edit mode (the GL preview is shown
        // instead), so pause it there and resume — restarting at the edited
        // scene — when returning to Render.
        if mode_before != self.mode {
            match self.mode {
                Mode::Edit => self.render.pause(),
                Mode::Render => self.render.resume(),
            }
        }

        // --- Top toolbar: gizmo handle-group toggles (Edit mode only) ---
        if self.mode == Mode::Edit {
            egui::Panel::top("gizmo_toolbar").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(2.0);
                    ui.strong("Gizmo");
                    ui.separator();
                    let mut tool = |ui: &mut egui::Ui, on: &mut bool, icon: &str, tip: &str| {
                        if ui
                            .selectable_label(*on, format!("{icon}  {tip}"))
                            .on_hover_text(format!("Toggle {tip} handles"))
                            .clicked()
                        {
                            *on ^= true;
                        }
                    };
                    tool(ui, &mut self.gizmo_modes.translate, icons::ARROWS_OUT_CARDINAL, "Move");
                    tool(ui, &mut self.gizmo_modes.rotate, icons::ARROWS_CLOCKWISE, "Rotate");
                    tool(ui, &mut self.gizmo_modes.scale, icons::RESIZE, "Scale");
                    ui.separator();
                    ui.checkbox(&mut self.gizmo_local, "Local axes");
                });
            });
        }

        // --- Central panel: path-traced image or GL preview ---
        egui::CentralPanel::default().show(ui, |ui| {
            let vp = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(vp, egui::Sense::click_and_drag());

            match self.mode {
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
                    let selected = self.selected;
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
                    if let Some(i) = self.selected {
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
                                self.gizmo_local,
                                self.gizmo_modes,
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
                    if !gizmo_active {
                        // Orbit/pan only for drags that began inside the preview.
                        let drag_in_view = down && origin.is_some_and(|o| rect.contains(o));
                        if drag_in_view && delta != egui::Vec2::ZERO {
                            let mut scene = self.scene.lock().unwrap();
                            if shift {
                                orbit::pan(&mut scene.camera, delta);
                            } else {
                                orbit::orbit(&mut scene.camera, delta);
                            }
                            moved = true;
                        }
                        if scroll != 0.0 && ptr.is_some_and(|p| rect.contains(p)) {
                            orbit::dolly(&mut self.scene.lock().unwrap().camera, scroll);
                            moved = true;
                        }
                        // A click in the preview selects the object under it; a
                        // click on empty space clears the selection. Same
                        // view/projection as the preview, so the pick matches
                        // what's drawn (near/far don't affect the ray).
                        if clicked && ptr.is_some_and(|p| vp.contains(p)) {
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
                                self.selected = raster::pick::pick(&s, &ray);
                            } else {
                                self.selected = None;
                            }
                            ui.ctx().request_repaint();
                        }
                    }
                    if moved {
                        self.last_interact = now;
                    }

                    // Reduced-resolution preview while actively interacting;
                    // snap back to full quality once motion has stopped. Inert in
                    // Edit (no invalidate), but kept for the Render handoff.
                    let interacting = (now - self.last_interact) < PREVIEW_DEBOUNCE as f64;
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
        });
    }
}
