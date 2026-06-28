# R2 Stratified Sub-Pixel Sampling — Design

**Date:** 2026-06-28
**Status:** Approved (pending spec review)

## Problem

The camera currently jitters each sub-pixel sample with a uniform random offset
(`Camera::sample_square`):

```rust
fn sample_square(&self, rng: &mut SmallRng) -> Vec3 {
    Vec3::new(rng.random::<f32>() - 0.5, rng.random::<f32>() - 0.5, 0.0)
}
```

Pure random offsets clump and leave gaps within the pixel, which shows up as
anti-aliasing noise (especially on edges) that only averages out slowly
(~1/√N). Replacing this with a low-discrepancy sequence spreads samples evenly
at every prefix length and converges faster (~closer to 1/N), reducing noise for
the same number of samples.

The renderer is **progressive**: `ProgressiveRenderer::add_pass` adds exactly one
sample per pixel per pass and accumulates over time. A sequence that is
well-distributed at *every* length (not just at a fixed N²) is the natural fit —
no fixed sample budget required.

## Approach

Use the **R2 low-discrepancy sequence** (Martin Roberts, 2018; based on the
plastic constant) for the sub-pixel offset, decorrelated per pixel with a
Cranley-Patterson rotation.

### Components

1. **R2 sequence** — pure function returning a well-distributed 2D point for a
   given integer sample index:

   ```
   r2(index) = ( frac(index * 0.7548776662),
                 frac(index * 0.5698402909) )
   ```

   `index` is the sample number for that pixel (0, 1, 2, …).

2. **Per-pixel decorrelation (Cranley-Patterson rotation)** — without this, every
   pixel walks the identical R2 path and produces visible structured patterns.
   Add a per-pixel offset derived from a **deterministic hash** of `(i, j)`, then
   wrap mod 1:

   ```
   offset01 = frac( r2(index) + hash01(i, j) )
   ```

   The hash is deterministic (not drawn from the rng) so the rotation is stable
   across passes and the render stays reproducible. The same pixel always gets
   the same rotation; neighbouring pixels get different ones.

3. **Camera wiring** — `get_ray` and `sample_pixel` gain a `sample_index: u32`
   parameter. `sample_square` becomes:

   ```
   let (sx, sy) = stratified_offset(i, j, sample_index);
   Vec3::new(sx - 0.5, sy - 0.5, 0.0)
   ```

   - **Progressive path:** `ProgressiveRenderer::add_pass` passes `self.passes`
     as the sample index (pass 0, 1, 2, … = sequence position 0, 1, 2, …).
   - **Offline path:** `Camera::render`'s sample loop passes its `0..samples`
     counter as the sample index.

### Location

A new `src/sampling.rs` module holds `r2`, `hash01`, and `stratified_offset`,
keeping the sampling logic isolated and unit-testable independent of the camera.

## Out of scope (YAGNI)

- DOF disk sampling and `ray.time` keep using the rng (white noise). Only the
  pixel anti-aliasing offset is stratified, as requested. Stratifying those is a
  separate later win.
- No change to bounce/path sampling — that is the book-3 importance-sampling
  work, tracked separately.

## Testing

- Unit-test `r2` against the known first few values and that outputs stay in
  [0, 1).
- Unit-test that `stratified_offset` stays within the pixel bounds (±0.5) and
  that two different pixels receive different rotations.
- Visual confirmation in the interactive viewer (lower noise at equal pass
  counts; no structured patterns).
