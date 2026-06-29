//! Phosphor icon font, vendored directly (the `egui-phosphor` crate is pinned
//! to egui 0.34 and won't interop with our 0.35 `FontDefinitions`).
//!
//! `assets/fonts/Phosphor.ttf` and these codepoints come from Phosphor Icons
//! (MIT). Each constant is a private-use-area glyph in that font. Install the
//! font once with [`install`], then render an icon by printing the constant,
//! e.g. `ui.label(icons::SPHERE)` or `format!("{}  Sphere", icons::PLUS)`.

use eframe::egui;

pub const SPHERE: &str = "\u{EE66}";
pub const CUBE: &str = "\u{E1DA}";
pub const RECTANGLE: &str = "\u{E3F0}";
pub const POLYGON: &str = "\u{E6D0}";
pub const PLUS: &str = "\u{E3D4}";
pub const TRASH: &str = "\u{E4A6}";
pub const CAMERA: &str = "\u{E10E}";
pub const STACK: &str = "\u{E466}";
pub const PALETTE: &str = "\u{E6C8}";
pub const SHAPES: &str = "\u{EC5E}";
pub const ARROWS_OUT_CARDINAL: &str = "\u{E0A4}";
pub const ARROWS_CLOCKWISE: &str = "\u{E094}";
pub const RESIZE: &str = "\u{ED6E}";
pub const CROSSHAIR: &str = "\u{E1D6}";
pub const APERTURE: &str = "\u{E00A}";
pub const IMAGE: &str = "\u{E2CA}";
pub const FLOPPY: &str = "\u{E2CC}";
pub const EYE: &str = "\u{E220}";
pub const EYE_SLASH: &str = "\u{E224}";
pub const COPY: &str = "\u{E1CA}";
pub const FOLDER: &str = "\u{E24A}";
pub const DOWNLOAD: &str = "\u{E20C}"; // DOWNLOAD_SIMPLE
pub const CARET_DOWN: &str = "\u{E136}";
pub const PLAY: &str = "\u{E3D0}";
pub const RESET: &str = "\u{E038}"; // ARROW_COUNTER_CLOCKWISE
pub const SLIDERS: &str = "\u{E434}"; // SLIDERS_HORIZONTAL

/// Register the Phosphor font as a fallback so the icon codepoints resolve in
/// any normal label/button. Call once at startup with the egui context.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "phosphor".to_owned(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/Phosphor.ttf")).into(),
    );

    // Append as a fallback in both families: ordinary text keeps the default
    // font, while the PUA icon glyphs fall through to Phosphor.
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push("phosphor".to_owned());
    }

    ctx.set_fonts(fonts);
}
