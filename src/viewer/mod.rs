mod controls;
mod icons;
mod render_task;
mod view_transform;

use std::sync::{Arc, Mutex};

use eframe::egui;

use crate::camera::Camera;
use crate::scene::Scene;
use render_task::RenderTask;
use view_transform::ViewTransform;

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
    width: u32,
    height: u32,
    texture: Option<egui::TextureHandle>,
    shown_pass: u32,
    view: ViewTransform,
    /// Index of the object whose settings are shown (UI state, not scene data).
    selected: Option<usize>,
}

impl ViewerApp {
    fn new(cc: &eframe::CreationContext<'_>, scene: Scene, width: u32, height: u32) -> Self {
        icons::install(&cc.egui_ctx);
        let total = scene.camera.samples;
        let scene = Arc::new(Mutex::new(scene));
        let render = RenderTask::spawn(cc.egui_ctx.clone(), scene.clone(), width, height, total);

        ViewerApp {
            scene,
            render,
            width,
            height,
            texture: None,
            shown_pass: 0,
            view: ViewTransform::new(),
            selected: None,
        }
    }
}

impl eframe::App for ViewerApp {
    // eframe 0.35 hands the root `Ui` directly; we lay out panels inside it.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Pull the latest frame; rebuild the texture only when a new pass landed.
        let (passes, total, done, elapsed, new_image) = {
            let s = self.render.lock();
            let new_image = if s.passes != self.shown_pass {
                Some(egui::ColorImage::from_rgba_unmultiplied(
                    [self.width as usize, self.height as usize],
                    &s.rgba,
                ))
            } else {
                None
            };
            (s.passes, s.total, s.done, s.elapsed, new_image)
        };
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
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::CollapsingHeader::new(format!("{}  Camera", icons::CAMERA))
                            .default_open(true)
                            .show(ui, |ui| {
                                dirty |= controls::camera_controls(ui, &mut scene.camera);
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

        // --- Central panel: zoomable / pannable image ---
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(t) = &self.texture else {
                ui.centered_and_justified(|ui| ui.label("Rendering…"));
                return;
            };

            let vp = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(vp, egui::Sense::click_and_drag());
            let aspect = self.width as f32 / self.height as f32;

            // Drag to pan; double-click to reset the view.
            if response.dragged() {
                self.view.pan_by(response.drag_delta());
            }
            if response.double_clicked() {
                self.view.reset();
            }

            // Scroll to zoom, keeping the point under the cursor fixed.
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if response.hovered() && scroll != 0.0 {
                let cursor = ui
                    .input(|i| i.pointer.hover_pos())
                    .unwrap_or_else(|| vp.center());
                self.view.zoom_at(vp, aspect, cursor, scroll);
            }

            // Paint the image, clipped to the viewport so zoomed overflow hides.
            let rect = self.view.image_rect(vp, aspect);
            ui.painter_at(vp).image(
                t.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        });
    }
}
