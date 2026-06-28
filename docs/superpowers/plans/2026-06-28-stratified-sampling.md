# R2 Stratified Sub-Pixel Sampling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the camera's white-noise sub-pixel jitter with an R2 low-discrepancy sequence (decorrelated per pixel) so the image converges with less anti-aliasing noise in both the progressive viewer and the offline render.

**Architecture:** A new `src/sampling.rs` module provides the pure sampling math (`r2`, `hash01`, `stratified_offset`). The camera's `get_ray`/`sample_pixel` gain a `sample_index: u32` and call `stratified_offset` instead of drawing a random offset. The progressive renderer feeds its pass counter as the index; the offline render loop feeds its sample counter.

**Tech Stack:** Rust, existing `Vec3`, `rand`/`SmallRng` (still used for DOF and ray time).

## Global Constraints

- The sub-pixel offset must stay in the range [-0.5, 0.5) per axis (one pixel wide), matching the current `sample_square` contract.
- The per-pixel rotation must be deterministic from `(i, j)` only — not drawn from the rng — so renders stay reproducible and the rotation is stable across passes.
- Do NOT change DOF disk sampling or `ray.time`; they keep using the rng.
- R2 constants (verbatim): x multiplier `0.7548776662`, y multiplier `0.5698402909`.

---

### Task 1: Sampling module (`r2`, `hash01`, `stratified_offset`)

**Files:**
- Create: `src/sampling.rs`
- Modify: `src/main.rs:3-15` (add `mod sampling;`)
- Test: inline `#[cfg(test)]` module in `src/sampling.rs`

**Interfaces:**
- Consumes: nothing.
- Produces:
  - `pub fn r2(index: u32) -> (f32, f32)` — R2 low-discrepancy point in [0,1)².
  - `pub fn hash01(i: u32, j: u32) -> (f32, f32)` — deterministic per-pixel offset in [0,1)².
  - `pub fn stratified_offset(i: u32, j: u32, sample_index: u32) -> (f32, f32)` — sub-pixel offset in [-0.5, 0.5) per axis.

- [ ] **Step 1: Register the module**

In `src/main.rs`, add `mod sampling;` alongside the other `mod` declarations (keep alphabetical order — between `mod ray;`/`mod render;` and `mod scene;`):

```rust
mod render;
mod sampling;
mod scene;
```

- [ ] **Step 2: Write the failing tests**

Create `src/sampling.rs` with only the test module first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r2_stays_in_unit_square() {
        for index in 0..1000 {
            let (x, y) = r2(index);
            assert!((0.0..1.0).contains(&x), "x out of range at {index}: {x}");
            assert!((0.0..1.0).contains(&y), "y out of range at {index}: {y}");
        }
    }

    #[test]
    fn r2_first_values_match_known_sequence() {
        // index 0 -> (0,0); index 1 -> the two multipliers themselves.
        let (x0, y0) = r2(0);
        assert!(x0.abs() < 1e-6 && y0.abs() < 1e-6);
        let (x1, y1) = r2(1);
        assert!((x1 - 0.7548776662).abs() < 1e-5, "x1={x1}");
        assert!((y1 - 0.5698402909).abs() < 1e-5, "y1={y1}");
    }

    #[test]
    fn stratified_offset_stays_within_pixel() {
        for index in 0..256 {
            let (dx, dy) = stratified_offset(3, 7, index);
            assert!((-0.5..0.5).contains(&dx), "dx out of range: {dx}");
            assert!((-0.5..0.5).contains(&dy), "dy out of range: {dy}");
        }
    }

    #[test]
    fn different_pixels_get_different_rotations() {
        // Same sample index, different pixels => different offsets (decorrelated).
        let a = stratified_offset(0, 0, 5);
        let b = stratified_offset(1, 0, 5);
        let c = stratified_offset(0, 1, 5);
        assert!(a != b, "pixels (0,0) and (1,0) collide: {a:?}");
        assert!(a != c, "pixels (0,0) and (0,1) collide: {a:?}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test sampling 2>&1 | tail -20`
Expected: compile error — `r2`, `stratified_offset` not found.

- [ ] **Step 4: Write the implementation**

Add above the test module in `src/sampling.rs`:

```rust
//! Low-discrepancy sub-pixel sampling.
//!
//! Replaces white-noise pixel jitter with the R2 sequence (Martin Roberts,
//! 2018), decorrelated per pixel via a Cranley-Patterson rotation so the same
//! offset pattern is not reused across pixels.

/// R2 low-discrepancy point for `index`, each component in [0, 1).
pub fn r2(index: u32) -> (f32, f32) {
    // Plastic-constant multipliers (the 2D golden-ratio analogue).
    let x = (index as f32 * 0.7548776662).fract();
    let y = (index as f32 * 0.5698402909).fract();
    (x, y)
}

/// Deterministic per-pixel offset in [0, 1), used to rotate the R2 sequence so
/// neighbouring pixels do not share the same sample positions. Stable across
/// passes (depends only on the pixel coordinates).
pub fn hash01(i: u32, j: u32) -> (f32, f32) {
    let hx = wang_hash(i.wrapping_mul(0x9E3779B1) ^ j);
    let hy = wang_hash(j.wrapping_mul(0x85EBCA77) ^ i.wrapping_add(0x165667B1));
    (
        hx as f32 / u32::MAX as f32,
        hy as f32 / u32::MAX as f32,
    )
}

fn wang_hash(mut x: u32) -> u32 {
    x = (x ^ 61) ^ (x >> 16);
    x = x.wrapping_mul(9);
    x = x ^ (x >> 4);
    x = x.wrapping_mul(0x27D4EB2D);
    x = x ^ (x >> 15);
    x
}

/// Sub-pixel offset for sample `sample_index` of pixel `(i, j)`, each component
/// in [-0.5, 0.5). Drop-in replacement for the old random `sample_square`.
pub fn stratified_offset(i: u32, j: u32, sample_index: u32) -> (f32, f32) {
    let (rx, ry) = r2(sample_index);
    let (ox, oy) = hash01(i, j);
    let dx = (rx + ox).fract() - 0.5;
    let dy = (ry + oy).fract() - 0.5;
    (dx, dy)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test sampling 2>&1 | tail -20`
Expected: all four tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/sampling.rs src/main.rs
git commit -m "feat: add R2 stratified sub-pixel sampling module

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Wire stratified sampling into the camera and both render paths

**Files:**
- Modify: `src/camera/camera.rs` (`get_ray`, `sample_pixel`, `render`; remove `sample_square`)
- Modify: `src/render.rs:43` (pass the pass counter)
- Test: build + existing `roll_tests` in `src/camera/camera.rs`

**Interfaces:**
- Consumes: `crate::sampling::stratified_offset(i, j, sample_index)` from Task 1.
- Produces:
  - `get_ray(&self, i: u32, j: u32, sample_index: u32, rng: &mut SmallRng) -> Ray`
  - `sample_pixel(&self, i: u32, j: u32, sample_index: u32, world: &IntersectGroup, rng: &mut SmallRng) -> Color`

This task compiles only once every caller is updated together (signature change), so all edits below land in one task.

- [ ] **Step 1: Add the import**

In `src/camera/camera.rs`, add to the `use crate::...` block near the top:

```rust
use crate::sampling::stratified_offset;
```

- [ ] **Step 2: Replace `sample_square` with the stratified offset in `get_ray`**

Change `get_ray` (currently `src/camera/camera.rs:170-185`) to take `sample_index` and use it:

```rust
fn get_ray(&self, i: u32, j: u32, sample_index: u32, rng: &mut SmallRng) -> Ray {
    let (dx, dy) = stratified_offset(i, j, sample_index);
    let pixel_sample = self.pixel00_loc
        + ((i as f32 + dx) * self.pixel_delta_u)
        + ((j as f32 + dy) * self.pixel_delta_v);

    let ray_origin = if self.dof_angle <= 0. {
        self.center
    } else {
        self.dof_disk_sample(rng)
    };
    let ray_direction = pixel_sample - ray_origin;

    let ray_time = rng.random::<f32>();
    Ray::new_t(ray_origin, ray_direction, ray_time)
}
```

Then delete the now-unused `sample_square` method (currently `src/camera/camera.rs:187-189`).

- [ ] **Step 3: Thread `sample_index` through `sample_pixel`**

Change `sample_pixel` (currently `src/camera/camera.rs:154-163`):

```rust
pub fn sample_pixel(
    &self,
    i: u32,
    j: u32,
    sample_index: u32,
    world: &IntersectGroup,
    rng: &mut SmallRng,
) -> Color {
    let ray = self.get_ray(i, j, sample_index, rng);
    self.ray_color(&ray, self.max_depth, world, rng)
}
```

- [ ] **Step 4: Pass the sample counter in the offline `render` loop**

In `render` (currently `src/camera/camera.rs:116-119`), give the loop a counter and pass it:

```rust
for s in 0..self.samples {
    let ray = self.get_ray(i, j, s, &mut rng);
    pixel_color += self.ray_color(&ray, self.max_depth, world, &mut rng);
}
```

- [ ] **Step 5: Pass the pass counter in the progressive renderer**

In `src/render.rs`, update the `sample_pixel` call (currently line 43). Note `passes` is captured just above as `let passes = self.passes;`:

```rust
*c += camera.sample_pixel(i, j, passes, world, &mut rng);
```

- [ ] **Step 6: Build and run the existing tests**

Run: `cargo test 2>&1 | tail -25`
Expected: compiles with no errors; `roll_tests` and `sampling::tests` all PASS. No remaining reference to `sample_square`.

- [ ] **Step 7: Commit**

```bash
git add src/camera/camera.rs src/render.rs
git commit -m "feat: use R2 stratified sampling for sub-pixel jitter

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review

- **Spec coverage:** R2 sequence (Task 1 `r2`), Cranley-Patterson per-pixel rotation (Task 1 `hash01`/`stratified_offset`), `src/sampling.rs` location (Task 1), camera wiring with `sample_index` (Task 2 steps 2-3), progressive path = pass counter (Task 2 step 5), offline path = sample counter (Task 2 step 4), DOF/ray-time untouched (preserved in step 2), tests for r2 range/known values and offset bounds/decorrelation (Task 1 step 2). All spec points covered.
- **Placeholder scan:** none — every code/command step is concrete.
- **Type consistency:** `stratified_offset(i: u32, j: u32, sample_index: u32) -> (f32, f32)` defined in Task 1 and called identically in Task 2; `sample_index: u32` consistent across `get_ray`/`sample_pixel` and both callers.
