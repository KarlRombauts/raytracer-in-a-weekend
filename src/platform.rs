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

use std::sync::{Arc, Mutex};

/// A file the user picked: its raw bytes and original file name.
pub struct PickedFile {
    pub bytes: Vec<u8>,
    pub name: String,
}

/// Outcome of an async file pick, polled by the UI each frame.
pub enum PickStatus {
    Pending,
    Done(PickedFile),
    Cancelled,
    Failed(String),
}

/// Handle to an in-flight (web) or already-resolved (native) file pick. Cheap to
/// clone — clones share the result slot — so it can be stashed in egui's
/// per-frame data and polled by the widget that started it.
#[derive(Clone)]
pub struct FilePicker {
    slot: Arc<Mutex<Option<PickStatus>>>,
}

impl FilePicker {
    /// Returns the outcome once, then `Pending` thereafter. Callers should drop
    /// the picker once they get a non-`Pending` status.
    pub fn poll(&self) -> PickStatus {
        self.slot.lock().unwrap().take().unwrap_or(PickStatus::Pending)
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn save_scene(suggested_name: &str, bytes: &[u8]) {
    if let Some(path) = rfd::FileDialog::new()
        .add_filter("Scene", &["scene"])
        .set_file_name(suggested_name)
        .save_file()
    {
        if let Err(e) = std::fs::write(&path, bytes) {
            eprintln!("failed to save {}: {e}", path.display());
        }
    }
}

/// Open a file picker filtered to `exts`, reading the chosen file's bytes.
///
/// Native: a blocking OS dialog, so the returned handle resolves immediately.
/// Web: an async `<input type=file>`; the handle stays `Pending` until the user
/// chooses, then the spawned task fills the slot. Either way the caller polls.
#[cfg(not(target_arch = "wasm32"))]
pub fn pick_file(filter: &str, exts: &[&str]) -> FilePicker {
    let status = match rfd::FileDialog::new().add_filter(filter, exts).pick_file() {
        Some(path) => match std::fs::read(&path) {
            Ok(bytes) => PickStatus::Done(PickedFile {
                bytes,
                name: path
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }),
            Err(e) => PickStatus::Failed(e.to_string()),
        },
        None => PickStatus::Cancelled,
    };
    FilePicker { slot: Arc::new(Mutex::new(Some(status))) }
}

/// Scene-file (`.scene`) picker — a thin wrapper over [`pick_file`].
pub fn pick_scene() -> FilePicker {
    pick_file("Scene", &["scene"])
}

/// Outcome of an in-flight scene decode, polled each frame.
pub enum DecodeStatus {
    Pending,
    Done(crate::scene_file::LoadedScene),
    Failed(String),
}

/// Handle to a scene decode running on a worker thread.
pub struct SceneDecoder {
    slot: Arc<Mutex<Option<Result<crate::scene_file::LoadedScene, String>>>>,
}

impl SceneDecoder {
    pub fn poll(&self) -> DecodeStatus {
        match self.slot.lock().unwrap().take() {
            None => DecodeStatus::Pending,
            Some(Ok(loaded)) => DecodeStatus::Done(loaded),
            Some(Err(e)) => DecodeStatus::Failed(e),
        }
    }
}

/// Decode a `.scene` blob on a rayon worker, never on the calling (UI) thread.
///
/// This is mandatory in the browser: decoding rebuilds each mesh's BVH, which
/// forks via `rayon::join`, and a `join` on the main thread blocks on
/// `Atomics.wait` — forbidden there, so it hangs. Running the decode on a worker
/// (where blocking is allowed) sidesteps that, and as a bonus keeps the UI
/// responsive (the loading bar keeps animating) on native too.
pub fn decode_scene(bytes: Vec<u8>) -> SceneDecoder {
    let slot: Arc<Mutex<Option<Result<crate::scene_file::LoadedScene, String>>>> =
        Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    rayon::spawn(move || {
        let result = crate::scene_file::decode(&bytes).map_err(|e| e.to_string());
        *slot2.lock().unwrap() = Some(result);
    });
    SceneDecoder { slot }
}

/// Handle to an OBJ parse + mesh build running on a worker thread. Cheap to
/// clone (shares the slot) so it can live in egui's per-frame data.
#[derive(Clone)]
pub struct ObjBuilder {
    slot: Arc<Mutex<Option<crate::scene::ObjectSpec>>>,
}

impl ObjBuilder {
    /// The finished object once it's ready, else `None`.
    pub fn poll(&self) -> Option<crate::scene::ObjectSpec> {
        self.slot.lock().unwrap().take()
    }
}

/// Parse OBJ `bytes` and build the mesh object on a rayon worker, auto-fitting it
/// to `center`/`size`. Off-thread for the same reason as [`decode_scene`]: the
/// mesh BVH forks via `rayon::join`, which can't run on the browser's main
/// thread. `center`/`size` are computed by the caller (cheaply) beforehand.
pub fn build_obj(
    name: String,
    bytes: Vec<u8>,
    center: crate::vec3::Vec3,
    size: f32,
) -> ObjBuilder {
    let slot: Arc<Mutex<Option<crate::scene::ObjectSpec>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    rayon::spawn(move || {
        let raw = String::from_utf8_lossy(&bytes);
        let obj = crate::scene::ObjectSpec::from_obj_bytes(&name, &raw, center, size);
        *slot2.lock().unwrap() = Some(obj);
    });
    ObjBuilder { slot }
}

/// Fetch a bundled asset's bytes by path. Native: a filesystem read (resolves
/// immediately). Web: an async `fetch()` of the same path relative to the page
/// (Trunk copies the assets into the bundle). Polled via the returned handle —
/// used for the sample-scene cards, whose `.scene` files are too large to embed.
#[cfg(not(target_arch = "wasm32"))]
pub fn fetch_file(path: &str) -> FilePicker {
    let status = match std::fs::read(path) {
        Ok(bytes) => PickStatus::Done(PickedFile { bytes, name: path.to_string() }),
        Err(e) => PickStatus::Failed(e.to_string()),
    };
    FilePicker { slot: Arc::new(Mutex::new(Some(status))) }
}

#[cfg(target_arch = "wasm32")]
pub fn fetch_file(path: &str) -> FilePicker {
    let slot: Arc<Mutex<Option<PickStatus>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    let path = path.to_string();
    wasm_bindgen_futures::spawn_local(async move {
        let status = match fetch_bytes(&path).await {
            Ok(bytes) => PickStatus::Done(PickedFile { bytes, name: path }),
            Err(e) => PickStatus::Failed(e),
        };
        *slot2.lock().unwrap() = Some(status);
    });
    FilePicker { slot }
}

#[cfg(target_arch = "wasm32")]
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let window = web_sys::window().ok_or("no window")?;
    let resp_val = JsFuture::from(window.fetch_with_str(url))
        .await
        .map_err(|_| "network error".to_string())?;
    let resp: web_sys::Response = resp_val.dyn_into().map_err(|_| "bad response".to_string())?;
    if !resp.ok() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let buf = JsFuture::from(resp.array_buffer().map_err(|_| "no body".to_string())?)
        .await
        .map_err(|_| "read error".to_string())?;
    Ok(js_sys::Uint8Array::new(&buf).to_vec())
}

#[cfg(target_arch = "wasm32")]
pub fn save_scene(suggested_name: &str, bytes: &[u8]) {
    use wasm_bindgen::JsCast;

    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type("application/octet-stream");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts)
        .expect("create blob");
    let url = web_sys::Url::create_object_url_with_blob(&blob).expect("object url");

    let document = web_sys::window().unwrap().document().unwrap();
    let anchor: web_sys::HtmlAnchorElement =
        document.create_element("a").unwrap().dyn_into().unwrap();
    anchor.set_href(&url);
    anchor.set_download(suggested_name);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).ok();
}

#[cfg(target_arch = "wasm32")]
pub fn pick_file(filter: &str, exts: &[&str]) -> FilePicker {
    let slot: Arc<Mutex<Option<PickStatus>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    let filter = filter.to_string();
    let exts: Vec<String> = exts.iter().map(|s| s.to_string()).collect();
    wasm_bindgen_futures::spawn_local(async move {
        let exts_ref: Vec<&str> = exts.iter().map(String::as_str).collect();
        let status = match rfd::AsyncFileDialog::new()
            .add_filter(&filter, &exts_ref)
            .pick_file()
            .await
        {
            Some(handle) => PickStatus::Done(PickedFile {
                name: handle.file_name(),
                bytes: handle.read().await,
            }),
            None => PickStatus::Cancelled,
        };
        *slot2.lock().unwrap() = Some(status);
    });
    FilePicker { slot }
}

// Native tests for the worker-offload mechanism (cargo test runs natively). They
// verify the rayon::spawn + poll round-trip returns the right result; the
// browser-specific benefit (not blocking the main thread) can't be unit-tested.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod worker_tests {
    use super::*;
    use crate::camera::CameraConfig;
    use crate::scene::Scene;
    use crate::vec3::Vec3;

    #[test]
    fn decode_scene_returns_from_worker() {
        let scene = Scene {
            camera: CameraConfig::builder().image_width(16).build(),
            objects: vec![],
        };
        let bytes = crate::scene_file::encode(&scene, Some("worker"), &[]);
        let dec = decode_scene(bytes);
        for _ in 0..1_000_000 {
            match dec.poll() {
                DecodeStatus::Done(loaded) => {
                    assert_eq!(loaded.name.as_deref(), Some("worker"));
                    return;
                }
                DecodeStatus::Failed(e) => panic!("decode failed: {e}"),
                DecodeStatus::Pending => std::thread::yield_now(),
            }
        }
        panic!("decode never completed");
    }

    #[test]
    fn build_obj_returns_from_worker() {
        let obj = "v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n";
        let builder = build_obj("tri".to_string(), obj.as_bytes().to_vec(), Vec3::ZERO, 2.0);
        for _ in 0..1_000_000 {
            if let Some(o) = builder.poll() {
                assert_eq!(o.name, "tri");
                return;
            }
            std::thread::yield_now();
        }
        panic!("build never completed");
    }
}
