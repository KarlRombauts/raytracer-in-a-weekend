//! Platform-specific helpers that differ between native and the browser.

/// Save PNG `bytes` to a user-chosen location.
///
/// Native: opens a save dialog, writes the file. Wasm: triggers a browser
/// download (implemented in the wasm cfg block).
#[cfg(not(target_arch = "wasm32"))]
pub fn save_png(suggested_name: &str, bytes: &[u8]) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name(suggested_name)
        .save_file()
    {
        if let Err(e) = std::fs::write(&path, bytes) {
            eprintln!("failed to save {}: {e}", path.display());
        }
    }
}
