# 0001 — Env-map as a grid: the sampling mental model

**Date:** 2026-07-06
**Lesson:** `lessons/0001-texels-rows-and-the-envmap-grid.html`

## What was taught
The foundational mental model for environment-map importance sampling, grounded
in Karl's `src/texture/env_map.rs`:

- **Texel** = one HDR pixel (a radiance), stored in `EnvMap.data` row-major.
- **Equirectangular** layout: column = longitude, row = latitude; row 0 = up (+Y)
  in this tracer (tied to `sample()`'s `u`/`v` lines).
- **Importance sampling** = pick a texel with probability ∝ brightness, then aim
  a ray at it — vs the current BSDF-only "hope a bounce finds the sun" (fireflies).
- **Marginal + conditional**: pick a row by total brightness, then a column within
  it — this is PBRT's `Distribution2D`.
- **sin θ teaser**: pole texels cover less sky, so weight = luminance × sin θ.
  Full pdf/Jacobian derivation deliberately deferred to Lesson 2.

## Assessment / zone of proximal development
- Karl already has strong context: he built the MIS estimator for area lights and
  the `Sky`/`Integrator` seam, so he knows NEE + power heuristic conceptually. The
  *gap* is specifically the environment-as-samplable-distribution and the pdf
  bookkeeping — not MIS in general. Pitch lessons at "how does the sky become a
  light," not "what is MIS."
- He explicitly asked "what are texels, rows, etc." → he wanted the grid vocabulary
  first. Lesson 1 answers exactly that and stops before the math.

## Next
- **Lesson 2**: chosen (row, column) → world direction, and the solid-angle pdf
  `pdf_dir = pdf_image / (2π² sin θ)` — derive where sin θ comes from (the
  image↔sphere change of variables). This unlocks the MIS weight.
- Watch for: whether the marginal/conditional idea landed (quiz 2) before moving
  to CDF construction in code.
