//! The library ("Welcome back") home screen: bundled sample scenes as cards,
//! plus New scene / Open .scene file actions. Selecting a card enters the editor.

use eframe::egui::{self, Ui};

use super::super::{icons, samples, theme, widgets};

pub enum HomeAction {
    None,
    NewScene,
    OpenSample(usize),
    OpenSceneFile,
}

/// Full-screen "loading scene" view: the wordmark, the scene name, and an
/// indeterminate animated bar. Shown the instant a sample card is clicked while
/// its `.scene` is still fetching (async on the web), so the click feels
/// immediate instead of leaving the welcome page looking frozen. The caller
/// repaints every frame to drive the animation.
pub fn show_loading(ui: &mut Ui, name: &str) {
    let area = ui.available_rect_before_wrap();
    let t = ui.input(|i| i.time) as f32;
    let p = ui.painter();
    let (cx, cy) = (area.center().x, area.center().y);

    p.text(
        egui::pos2(cx, cy - 52.0),
        egui::Align2::CENTER_CENTER,
        icons::APERTURE,
        egui::FontId::proportional(30.0),
        theme::ACCENT,
    );
    p.text(
        egui::pos2(cx, cy - 8.0),
        egui::Align2::CENTER_CENTER,
        format!("Loading {name}\u{2026}"),
        egui::FontId::proportional(15.0),
        theme::TEXT,
    );

    // Indeterminate bar: a track with a highlight segment sliding across it.
    let (bar_w, bar_h) = (320.0_f32, 6.0_f32);
    let track = egui::Rect::from_center_size(egui::pos2(cx, cy + 26.0), egui::vec2(bar_w, bar_h));
    let radius = egui::CornerRadius::same(3);
    p.rect_filled(track, radius, theme::FIELD_BG);

    let seg = 0.34_f32;
    let phase = (t * 0.7).rem_euclid(1.0); // one sweep every ~1.4s
    let x0 = (phase * (1.0 + seg) - seg).clamp(0.0, 1.0);
    let x1 = (x0 + seg).clamp(0.0, 1.0);
    if x1 > x0 {
        let hi = egui::Rect::from_min_max(
            egui::pos2(track.left() + x0 * bar_w, track.top()),
            egui::pos2(track.left() + x1 * bar_w, track.bottom()),
        );
        p.rect_filled(hi, radius, theme::ACCENT);
    }
}

/// Per-sample display data, computed once and cached. The thumbnail PNG (the
/// saved render that ships with each `.scene`) is the only thing decoded here;
/// the scene itself is loaded lazily on click, so its resolution is read from
/// the thumbnail's own dimensions rather than by decoding the (large) scene.
struct Card {
    name: String,
    subtitle: String,
    texture: Option<egui::TextureHandle>,
}

#[derive(Default)]
pub struct HomeState {
    cards: Vec<Card>,
}

impl HomeState {
    /// Build (once) the decoded thumbnail textures + derived metadata.
    fn ensure(&mut self, ctx: &egui::Context) {
        if !self.cards.is_empty() {
            return;
        }
        for sample in samples::SAMPLES {
            self.cards.push(Card {
                name: sample.name.to_string(),
                subtitle: format!("{}×{}", sample.res.0, sample.res.1),
                texture: decode_texture(ctx, sample.file, samples::sample_thumbnail(sample.file)),
            });
        }
    }
}

/// Decode a thumbnail PNG into an egui texture.
fn decode_texture(ctx: &egui::Context, key: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    if png.is_empty() {
        return None;
    }
    let img = image::load_from_memory(png).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw());
    Some(ctx.load_texture(format!("thumb-{key}"), color, egui::TextureOptions::LINEAR))
}

const CARD_MIN_W: f32 = 278.0;
const CARD_GAP: f32 = 18.0;
const TEXT_BLOCK_H: f32 = 62.0;
const CONTENT_MAX_W: f32 = 1120.0;

pub fn show_home(ui: &mut Ui, state: &mut HomeState) -> HomeAction {
    state.ensure(ui.ctx());
    let mut action = HomeAction::None;

    // Header: logo + wordmark + version badge.
    ui.add_space(20.0);
    ui.horizontal(|ui| {
        ui.add_space(30.0);
        ui.label(
            egui::RichText::new(icons::APERTURE)
                .color(theme::ACCENT)
                .size(20.0),
        );
        ui.label(
            egui::RichText::new("Lumi")
                .family(theme::semibold())
                .color(theme::TEXT_STRONG)
                .size(15.0),
        );
    });

    egui::ScrollArea::vertical().show(ui, |ui| {
        // Centered content column, capped at CONTENT_MAX_W.
        let full = ui.available_rect_before_wrap();
        let content_w = (full.width() - 60.0).min(CONTENT_MAX_W);
        let left = full.left() + (full.width() - content_w) * 0.5;
        let content = egui::Rect::from_min_size(
            egui::pos2(left, full.top() + 14.0),
            egui::vec2(content_w, full.height()),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(content), |ui| {
            ui.add_space(20.0);
            ui.label(
                egui::RichText::new("Welcome back")
                    .family(theme::semibold())
                    .color(theme::TEXT_STRONG)
                    .size(30.0),
            );
            ui.add_space(7.0);
            ui.label(
                egui::RichText::new(
                    "Open a sample scene, start fresh, or load a .scene file from disk.",
                )
                .color(theme::TEXT_MUTED)
                .size(15.0),
            );

            ui.add_space(22.0);
            ui.horizontal(|ui| {
                if widgets::pill_button(ui, &format!("{}  New scene", icons::PLUS), true, true)
                    .clicked()
                {
                    action = HomeAction::NewScene;
                }
                if widgets::pill_button(
                    ui,
                    &format!("{}  Open .scene file…", icons::FOLDER),
                    false,
                    true,
                )
                .clicked()
                {
                    action = HomeAction::OpenSceneFile;
                }
            });

            ui.add_space(34.0);
            widgets::section_header(ui, icons::CUBE, "Sample scenes");
            ui.add_space(14.0);

            // Responsive grid laid out as horizontal rows of `cols` cards.
            let avail = ui.available_width();
            let cols = (((avail + CARD_GAP) / (CARD_MIN_W + CARD_GAP)).floor() as usize).max(1);
            let card_w = (avail - (cols as f32 - 1.0) * CARD_GAP) / cols as f32;

            // Item 0 is the "New scene" card; items 1.. are samples.
            let total = state.cards.len() + 1;
            let mut idx = 0;
            while idx < total {
                ui.horizontal(|ui| {
                    for _ in 0..cols {
                        if idx >= total {
                            break;
                        }
                        if idx == 0 {
                            if new_scene_card(ui, card_w).clicked() {
                                action = HomeAction::NewScene;
                            }
                        } else {
                            let card = &state.cards[idx - 1];
                            if sample_card(ui, card, card_w).clicked() {
                                action = HomeAction::OpenSample(idx - 1);
                            }
                        }
                        idx += 1;
                    }
                });
                ui.add_space(CARD_GAP);
            }
        });
    });

    action
}

fn card_height(card_w: f32) -> f32 {
    // 16:10 thumbnail + a fixed text block.
    card_w * 10.0 / 16.0 + TEXT_BLOCK_H
}

fn new_scene_card(ui: &mut Ui, w: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, card_height(w)), egui::Sense::click());
    let stroke = if resp.hovered() {
        theme::ACCENT
    } else {
        theme::BORDER
    };
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(12),
        egui::Stroke::new(1.5, stroke),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("{}   New scene", icons::PLUS),
        egui::FontId::proportional(13.5),
        if resp.hovered() {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        },
    );
    resp
}

fn sample_card(ui: &mut Ui, card: &Card, w: f32) -> egui::Response {
    let h = card_height(w);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let border = if resp.hovered() {
        theme::BORDER_HOVER
    } else {
        theme::BORDER
    };
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(12), theme::BG_PANEL);

    // Thumbnail (top), cover-fit so the scene fills the 16:10 box without distortion.
    // Round the top corners to match the card; leave the bottom edge square since
    // the text block sits flush below it.
    let thumb_h = w * 10.0 / 16.0;
    let thumb_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, thumb_h));
    let thumb_radius = egui::CornerRadius {
        nw: 12,
        ne: 12,
        sw: 0,
        se: 0,
    };
    match &card.texture {
        Some(tex) => {
            let [iw, ih] = tex.size();
            let uv = cover_uv(iw as f32 / ih as f32, w / thumb_h);
            ui.painter().add(
                egui::epaint::RectShape::filled(thumb_rect, thumb_radius, egui::Color32::WHITE)
                    .with_texture(tex.id(), uv),
            );
        }
        None => {
            ui.painter()
                .rect_filled(thumb_rect, thumb_radius, theme::BG_VIEWPORT);
        }
    }

    // Name + metadata below the thumbnail.
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, thumb_rect.bottom() + 12.0),
        egui::Align2::LEFT_TOP,
        &card.name,
        egui::FontId::proportional(13.5),
        theme::TEXT_STRONG,
    );
    ui.painter().text(
        egui::pos2(rect.left() + 14.0, thumb_rect.bottom() + 36.0),
        egui::Align2::LEFT_TOP,
        &card.subtitle,
        egui::FontId::proportional(11.0),
        theme::TEXT_DIM,
    );

    // Border on top so it frames the thumbnail edge.
    ui.painter().rect_stroke(
        rect,
        egui::CornerRadius::same(12),
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    resp
}

/// UV sub-rect that cover-fits an image of aspect `img` into a box of aspect
/// `box_`, cropping the overflow and centering.
fn cover_uv(img: f32, box_: f32) -> egui::Rect {
    if img > box_ {
        // Image is wider than the box: crop the sides.
        let vis = box_ / img;
        egui::Rect::from_min_max(egui::pos2((1.0 - vis) * 0.5, 0.0), egui::pos2((1.0 + vis) * 0.5, 1.0))
    } else {
        // Image is taller than the box: crop top/bottom.
        let vis = img / box_;
        egui::Rect::from_min_max(egui::pos2(0.0, (1.0 - vis) * 0.5), egui::pos2(1.0, (1.0 + vis) * 0.5))
    }
}
