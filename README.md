# Rust Path Tracer

A multithreaded path tracer in Rust that grew out of the *Ray Tracing in a Weekend*
book into a full interactive renderer. It ships an egui-based viewer with orbit
camera and transform gizmo, a BVH-accelerated core, a range of materials and
geometry, OBJ mesh loading, and its own compact `.scene` file format. Builds
natively and to threaded WebAssembly for the browser.

**Stack:** Rust · rayon · glam · eframe/egui + glow (WebGL2) · image · serde + postcard/lz4 · trunk (wasm)

Features: Lambertian, metal, dielectric, glossy, microfacet, and emissive
materials; spheres, quads, boxes, triangles, and OBJ meshes; a flat BVH
acceleration structure; `.scene` files (lz4-compressed postcard blob with an
`RTSC` header) with a sample scene library and pre-baked thumbnails.

## Build & Run

Recipes live in the `justfile` (`just --list`). Native optimizations come from
`.cargo/config.toml`.

```sh
cargo run --release        # or: just render  — open the interactive viewer
just thumbnails            # render thumbnails for every sample scene
```

### Web (threaded WebAssembly)

Requires a nightly toolchain with `rust-src` and [Trunk](https://trunkrs.dev/).
Cross-origin isolation headers are set in `Trunk.toml` for `SharedArrayBuffer`.

```sh
just web                   # build the wasm bundle into ./dist
just serve                 # serve locally with COOP/COEP headers
```
