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
