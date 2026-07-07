//! End-to-end characterization pin for the Scene → World → render pipeline.
//!
//! This is the golden safety net for the `scene.rs` → `scene/` reorganization
//! (see `.scratch/scene-worldbuilder-split/PRD.md`). It renders a fixed Cornell
//! box deterministically and pins the exact pixels via a stable fingerprint. It
//! asserts no *new* behavior — only that reshuffling the Scene document and the
//! world build changes the produced image by not one bit.
//!
//! The render is reproducible: each pixel is seeded from `(sample_index, pixel)`
//! with no entropy, and pixels accumulate independently (parallelism is across
//! pixels, with no cross-pixel float reduction), so the image is bit-identical
//! across runs and machines.
//!
//! Note (shadow-rays Stage 2): the NEE reformulation to per-light occlusion
//! sampling (estimator B) was expected to re-pin this fingerprint, but did not —
//! and correctly so. This Cornell box has exactly one registered light and a
//! black flat sky, and at `n == 1` the per-light pdf `(1/n)·p_k` equals the old
//! marginal pdf while `sample_light` consumes rng identically to the retired
//! `sample_light_dir`, so estimators (A) and (B) coincide bit-for-bit here. The
//! reformulation's correctness at `n ≥ 2` (where they genuinely diverge) is
//! proven by the multi-light MIS-vs-Naive gate in `integrator::mis`, not by this
//! pin. So the baseline below is deliberately *unchanged*.

use raytracer_in_a_weekend::camera::Camera;
use raytracer_in_a_weekend::integrator::build_integrator;
use raytracer_in_a_weekend::render::ProgressiveRenderer;
use raytracer_in_a_weekend::scene::build_world;
use raytracer_in_a_weekend::scenes::cornell_box;

/// FNV-1a over the byte buffer — a small, dependency-free, forever-stable
/// fingerprint. (Unlike `std`'s `DefaultHasher`, whose algorithm carries no
/// cross-version stability guarantee.)
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[test]
fn cornell_box_render_is_bit_identical_to_the_pinned_baseline() {
    let mut scene = cornell_box();
    // Small and shallow: enough light transport to exercise the whole pipeline,
    // cheap enough for the unit suite. These values define the pinned image.
    scene.camera.image_width = 48;
    scene.camera.max_depth = 8;

    let world = build_world(&scene);
    let camera = Camera::from(scene.camera.clone());
    let integ = build_integrator(&scene.camera);

    let mut r = ProgressiveRenderer::new(
        camera.image_width(),
        camera.image_height(),
        scene.camera.firefly_clamp,
    );
    // A fixed pass count (not convergence-based early-stop), so the pin is a
    // pure function of the assembled World, not of the convergence heuristic.
    for _ in 0..32 {
        r.add_pass(&camera, integ.as_ref(), &world);
    }
    let rgba = r.to_rgba();

    // Dimensions derive from the camera's aspect; assert them so a fingerprint
    // mismatch is easy to localize.
    let expected_len = (camera.image_width() * camera.image_height() * 4) as usize;
    assert_eq!(rgba.len(), expected_len, "unexpected pixel buffer size");

    // Baseline captured from the pre-reorganization code. Any drift in how a
    // Scene builds into a World turns this red.
    const BASELINE_FINGERPRINT: u64 = 0x9436_e82c_bff1_10f1;
    let actual = fnv1a(&rgba);
    assert_eq!(
        actual, BASELINE_FINGERPRINT,
        "cornell box render drifted from the pinned baseline (actual {actual:#018x})",
    );
}
