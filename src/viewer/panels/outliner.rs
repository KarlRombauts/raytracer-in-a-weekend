use eframe::egui::{self, Ui};

use crate::scene::Scene;

use super::super::{
    controls, icons,
    state::{Tab, UiState},
    theme,
};

/// Returns `true` if the scene was dirtied (needs a render restart).
pub fn show_outliner(ui: &mut Ui, ui_state: &mut UiState, scene: &mut Scene) -> bool {
    let mut dirty = false;

    // Header: SCENE label + object count.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(icons::STACK).color(theme::TEXT_MUTED));
        ui.label(
            egui::RichText::new("SCENE")
                .family(theme::semibold())
                .color(theme::TEXT_MUTED)
                .size(11.0),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{} objects", scene.objects.len()))
                    .monospace()
                    .color(theme::TEXT_DIM),
            );
        });
    });
    ui.add_space(4.0);

    // "Add object" button — opens a popup menu via Popup::menu (egui 0.34).
    let add = ui.add_sized(
        [ui.available_width(), 33.0],
        egui::Button::new(
            egui::RichText::new(format!("{}  Add object", icons::PLUS)).color(theme::TEXT_STRONG),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, theme::BORDER_FIELD)),
    );
    // Style with accent look when the popup is open (check after button is created,
    // then repaint the button rect with accent fill + border overlay).
    let popup_id = egui::Popup::default_response_id(&add);
    let menu_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if menu_open {
        ui.painter().rect_filled(add.rect, egui::CornerRadius::same(7), theme::accent_soft());
        ui.painter().rect_stroke(
            add.rect,
            egui::CornerRadius::same(7),
            egui::Stroke::new(1.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }

    egui::Popup::menu(&add).show(|ui| {
        // Opaque popup: match mockup bg #1c1e23 with border #34393f, ~10px radius.
        // We override the panel fill for this scope so the popup is fully opaque.
        ui.visuals_mut().panel_fill = egui::Color32::from_rgb(0x1c, 0x1e, 0x23);
        ui.visuals_mut().window_fill = egui::Color32::from_rgb(0x1c, 0x1e, 0x23);
        // Softer shadow: large blur, low alpha.
        ui.visuals_mut().popup_shadow = egui::Shadow {
            offset: [0, 4],
            blur: 24,
            spread: 2,
            color: egui::Color32::from_black_alpha(60),
        };

        // Constrain width to roughly the panel content width (~262px).
        ui.set_min_width(262.0);
        ui.set_max_width(262.0);

        // Tighten row spacing; add left padding to item rows.
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.spacing_mut().button_padding.x = 10.0;

        // Indent the category label to line up with the option icons below
        // (primitive buttons use button_padding.x = 10; mesh rows add_space(10)).
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("PRIMITIVES")
                .size(10.0)
                .color(theme::TEXT_DIM),
        );
        if ui.button(format!("{}  Plane", icons::RECTANGLE)).clicked() {
            let obj = controls::default_plane(scene.objects.len());
            scene.objects.push(obj);
            ui_state.selected = Some(scene.objects.len() - 1);
            ui_state.tab = Tab::Object;
            dirty = true;
        }
        if ui.button(format!("{}  Box", icons::CUBE)).clicked() {
            let obj = controls::default_box(scene.objects.len());
            scene.objects.push(obj);
            ui_state.selected = Some(scene.objects.len() - 1);
            ui_state.tab = Tab::Object;
            dirty = true;
        }
        if ui.button(format!("{}  Sphere", icons::SPHERE)).clicked() {
            let obj = controls::default_sphere(scene.objects.len());
            scene.objects.push(obj);
            ui_state.selected = Some(scene.objects.len() - 1);
            ui_state.tab = Tab::Object;
            dirty = true;
        }

        ui.separator();
        // Indent the category label to line up with the option icons below
        // (primitive buttons use button_padding.x = 10; mesh rows add_space(10)).
        ui.add_space(10.0);
        ui.label(
            egui::RichText::new("SAMPLE MESHES")
                .size(10.0)
                .color(theme::TEXT_DIM),
        );
        for m in [
            "Suzanne",
            "Stanford Bunny",
            "Utah Teapot",
            "Stanford Dragon",
        ] {
            // Custom row: icon + name + ".obj" mono suffix (dim), fully disabled.
            // egui Button doesn't support mixed-style text, so we allocate a
            // fixed row rect and draw the content manually.
            let row_h = 32.0;
            let (row_rect, row_resp) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_h),
                egui::Sense::hover(),
            );
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row_rect));
            child.horizontal(|ui| {
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(icons::POLYGON)
                        .color(theme::TEXT_DIM)
                        .size(13.0),
                );
                ui.add_space(2.0);
                ui.label(
                    egui::RichText::new(m)
                        .color(theme::TEXT_MUTED)
                        .size(13.0),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(".obj")
                            .monospace()
                            .color(theme::TEXT_DIM)
                            .size(10.0),
                    );
                });
            });
            row_resp.on_hover_text("Bundled sample meshes are coming soon");
        }

        ui.separator();
        if controls::import_obj(ui, &mut scene.objects, &mut ui_state.selected) {
            ui_state.tab = Tab::Object;
            dirty = true;
        }
    });

    ui.add_space(6.0);

    // Scrollable object rows.
    egui::ScrollArea::vertical().show(ui, |ui| {
        // Tighten inter-row gap to ~2px (mockup: compact 32px rows).
        ui.spacing_mut().item_spacing.y = 2.0;

        let mut toggle_hidden: Option<usize> = None;
        let mut new_selection: Option<usize> = None;

        for (i, obj) in scene.objects.iter().enumerate() {
            let selected = ui_state.selected == Some(i);

            // Pre-allocate the full row rect so we can paint highlights
            // BEHIND the row content (painter goes below widgets).
            let row_h = 32.0; // compact row height matching mockup
            let (row_rect, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), row_h),
                egui::Sense::hover(),
            );

            // Paint selection or hover highlight behind text/buttons.
            if selected {
                ui.painter().rect_filled(
                    row_rect,
                    egui::CornerRadius::same(7),
                    theme::selection_soft(),
                );
                // Inset 1px SELECTION border for selected row.
                ui.painter().rect_stroke(
                    row_rect,
                    egui::CornerRadius::same(7),
                    egui::Stroke::new(1.0, theme::SELECTION),
                    egui::StrokeKind::Inside,
                );
            } else {
                // Subtle hover background when not selected.
                let row_response =
                    ui.interact(row_rect, ui.id().with(("row_hover", i)), egui::Sense::hover());
                if row_response.hovered() {
                    ui.painter().rect_filled(
                        row_rect,
                        egui::CornerRadius::same(7),
                        egui::Color32::from_rgb(0x22, 0x25, 0x2a),
                    );
                }
            }

            // Now draw row content inside the allocated rect, with left padding.
            let padded_rect = egui::Rect::from_min_max(
                row_rect.min + egui::vec2(8.0, 0.0),
                row_rect.max,
            );
            let mut eye_clicked = false;
            let mut eye_rect: Option<egui::Rect> = None;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(padded_rect));
            child.horizontal(|ui| {
                let icon_col = if selected {
                    theme::SELECTION
                } else {
                    theme::TEXT_MUTED
                };
                ui.label(egui::RichText::new(controls::shape_icon(&obj.shape)).color(icon_col));
                let name_col = if selected {
                    theme::SELECTION
                } else {
                    theme::TEXT
                };
                ui.label(egui::RichText::new(&obj.name).color(name_col));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let eye = if obj.hidden {
                        icons::EYE_SLASH
                    } else {
                        icons::EYE
                    };
                    let col = if obj.hidden {
                        theme::TEXT_DIM
                    } else if selected {
                        theme::SELECTION
                    } else {
                        theme::TEXT_DIM
                    };
                    let eye_resp = ui.add(
                        egui::Button::new(egui::RichText::new(eye).color(col))
                            .fill(egui::Color32::TRANSPARENT)
                            .stroke(egui::Stroke::NONE),
                    );
                    if eye_resp.clicked() {
                        toggle_hidden = Some(i);
                        eye_clicked = true;
                    }
                    eye_rect = Some(eye_resp.rect);
                });
            });

            // Click on the row selects it. The interact rect must stop before the
            // eye button: a full-row rect overlapping the eye ties at distance 0
            // and, being registered last, wins egui's "topmost" tie-break — which
            // would swallow the eye's click and break hide/unhide.
            if !eye_clicked {
                let mut click_rect = row_rect;
                if let Some(er) = eye_rect {
                    click_rect.max.x = er.min.x;
                }
                let row_response =
                    ui.interact(click_rect, ui.id().with(("row", i)), egui::Sense::click());
                if row_response.clicked() {
                    new_selection = Some(i);
                }
            }
        }

        if let Some(i) = toggle_hidden {
            scene.objects[i].hidden = !scene.objects[i].hidden;
            dirty = true;
        }
        if let Some(i) = new_selection {
            ui_state.selected = Some(i);
            ui_state.tab = Tab::Object;
        }
    });

    dirty
}
