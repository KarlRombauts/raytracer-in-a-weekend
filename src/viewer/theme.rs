//! The Lumi visual language: palette, fonts, and egui style. Installed once at
//! startup. Colours and spacing are taken verbatim from `ui-design-mockup.html`.

use eframe::egui::{self, Color32, CornerRadius, Stroke};

// --- Palette (mockup hexes) ---
pub const BG_APP: Color32 = Color32::from_rgb(0x0d, 0x0e, 0x11);
pub const BG_PANEL: Color32 = Color32::from_rgb(0x16, 0x17, 0x1b);
pub const BG_TOPBAR: Color32 = Color32::from_rgb(0x15, 0x16, 0x1a);
pub const BG_VIEWPORT: Color32 = Color32::from_rgb(0x08, 0x09, 0x0b);
pub const FIELD_BG: Color32 = Color32::from_rgb(0x10, 0x11, 0x16);

pub const BORDER: Color32 = Color32::from_rgb(0x23, 0x26, 0x2b);
pub const BORDER_FIELD: Color32 = Color32::from_rgb(0x2c, 0x2f, 0x36);
pub const BORDER_HOVER: Color32 = Color32::from_rgb(0x41, 0x47, 0x4e);

pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xda, 0xdf);
pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xe7, 0xe9, 0xec);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x8a, 0x8f, 0x97);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x6b, 0x70, 0x79);

pub const ACCENT: Color32 = Color32::from_rgb(0x4d, 0x84, 0xe6);
pub const SELECTION: Color32 = Color32::from_rgb(0xef, 0x8a, 0x3c);

/// Destructive-action red: glyph/text on hover, and its darker border.
pub const DANGER: Color32 = Color32::from_rgb(0xd9, 0x70, 0x70);
pub const DANGER_BORDER: Color32 = Color32::from_rgb(0x7a, 0x3a, 0x3a);

/// The dark "pill" surface — hover fill for icon buttons and the dark pill
/// button fill (was the repeated `0x22,0x25,0x2a` literal), plus its border.
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x22, 0x25, 0x2a);
pub const BORDER_PILL: Color32 = Color32::from_rgb(0x33, 0x37, 0x3d);

/// A soft red-tinted fill for a hovered destructive control.
pub fn danger_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(0xd9, 0x70, 0x70, 28)
}

pub const AXIS_X: Color32 = Color32::from_rgb(0xc0, 0x59, 0x4f);
pub const AXIS_Y: Color32 = Color32::from_rgb(0x5a, 0x9e, 0x5a);
pub const AXIS_Z: Color32 = Color32::from_rgb(0x4f, 0x7f, 0xc0);

/// accent at alpha (0..=255) over the panel background — for soft fills/strokes.
pub fn accent_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(0x4d, 0x84, 0xe6, 36)
}
pub fn selection_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(0xef, 0x8a, 0x3c, 40)
}

/// Field height / single-line interact size used across the inspector (mockup
/// fields are 30px tall).
pub const FIELD_H: f32 = 30.0;

/// Install fonts + style. Call once with the egui context at startup.
pub fn install(ctx: &egui::Context) {
    install_fonts(ctx);
    apply_style(ctx);
}

fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions, FontFamily};
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(
        "plex_sans".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "plex_sans_semibold".to_owned(),
        FontData::from_static(include_bytes!(
            "../../assets/fonts/IBMPlexSans-SemiBold.ttf"
        ))
        .into(),
    );
    fonts.font_data.insert(
        "plex_mono".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/IBMPlexMono-Medium.ttf")).into(),
    );
    fonts.font_data.insert(
        "phosphor".to_owned(),
        FontData::from_static(include_bytes!("../../assets/fonts/Phosphor.ttf")).into(),
    );

    // Proportional = Plex Sans, Monospace = Plex Mono, Phosphor as fallback in
    // both so the icon PUA glyphs still resolve.
    fonts.families.insert(
        FontFamily::Proportional,
        vec!["plex_sans".into(), "phosphor".into()],
    );
    fonts.families.insert(
        FontFamily::Monospace,
        vec!["plex_mono".into(), "phosphor".into()],
    );
    // SemiBold available as a named family for headings/wordmark.
    fonts.families.insert(
        FontFamily::Name("semibold".into()),
        vec!["plex_sans_semibold".into(), "phosphor".into()],
    );

    ctx.set_fonts(fonts);
}

/// SemiBold proportional family, for the few bold bits (wordmark, section
/// headers). Use as `egui::RichText::new(t).family(theme::semibold())`.
pub fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}

fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    let v = &mut style.visuals;
    v.dark_mode = true;
    v.panel_fill = BG_PANEL;
    v.window_fill = BG_PANEL;
    v.extreme_bg_color = FIELD_BG; // text edit / DragValue background
    v.faint_bg_color = FIELD_BG;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = accent_soft();
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;

    let r = CornerRadius::same(6);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
        w.bg_fill = FIELD_BG;
        w.weak_bg_fill = FIELD_BG;
        w.bg_stroke = Stroke::new(1.0, BORDER_FIELD);
        w.fg_stroke = Stroke::new(1.0, TEXT);
    }
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, BORDER_HOVER);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);

    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(8.0, 8.0);
    s.button_padding = egui::vec2(8.0, 6.0);
    s.interact_size.y = FIELD_H;
    s.window_margin = egui::Margin::same(0);

    ctx.set_style(style);
}
