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

#[cfg(target_arch = "wasm32")]
pub fn save_png(suggested_name: &str, bytes: &[u8]) {
    use wasm_bindgen::JsCast;

    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
        .expect("create blob");
    let url = web_sys::Url::create_object_url_with_blob(&blob).expect("object url");

    let document = web_sys::window().unwrap().document().unwrap();
    let anchor: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .unwrap()
        .dyn_into()
        .unwrap();
    anchor.set_href(&url);
    anchor.set_download(suggested_name);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
}
