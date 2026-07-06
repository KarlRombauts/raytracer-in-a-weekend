# PRD: any-hit occlusion + light-point NEE (shadow rays)

Status: ready-for-agent

> The §6 optimization from `.scratch/bvh-perf/RESEARCH.md` — the highest-value
> shadow-ray win. Delivered in **two stages**: Stage 1 adds occlusion as a
> first-class query (bit-identical, safe); Stage 2 reformulates next-event
> estimation to point-sample a chosen light and occlusion-test to it (changes the
> Monte-Carlo estimator → deliberate render re-pin). Architecture rationale in the
> conversation that produced this; the guiding principle: *occlusion and
> closest-hit are different questions, and the current NEE conflates them.*

## Problem Statement

A shadow ray only needs a boolean — *is the segment from the shading point to the
light blocked?* Our NEE answers it with a **full closest-hit** query
(`world.intersect(&shadow, …)` in `mis.rs`), then reads emission from whatever
surface it strikes. That does strictly more work than occlusion needs, two ways:
it scans to the *nearest* surface instead of stopping at the *first* blocker, and
it builds a full shading record (point, normal, UV, material) it mostly discards.
Shadow rays are typically ≥50% of all rays in an NEE path tracer, so this is a
large, systematic cost. The compactness of the current design is *coupled to* its
slowness: it never learns the distance to the light, which is exactly why it
can't bound the ray or early-exit.

## Solution

Break that coupling with a principled, two-stage change:

**Stage 1 — occlusion as a first-class primitive (bit-identical).** Add an
early-exit any-hit query to the intersection interface and the World:
- `Intersect::occluded(&self, ray, t_max) -> bool`, with a default
  (`self.intersect(ray, (ε, t_max)).is_some()`) so existing primitives need no
  change, overridden by `BVH<T>` with a true early-exit traversal that returns on
  the first primitive hit in range, does no front-to-back ordering, and builds no
  hit record.
- `World::occluded(origin, dir, t_max) -> bool`, walking the top-level BVH's
  any-hit path; because `occluded` is recursive through the `Intersect` interface,
  a mesh object's inner triangle BVH early-exits too (two-level occlusion).

No integrator change in Stage 1, so the image is untouched — this stage only adds
a query and proves it agrees with the closest-hit one.

**Stage 2 — light-point NEE (re-pin).** Rework next-event estimation to the
standard formulation:
- A `LightSample { wi, dist, radiance, pdf }` value, produced by sampling a
  **chosen** light: area lights fill a finite `dist` + their emission; the env
  light fills `dist = ∞` + its directional radiance. This generalizes the current
  `LightRef`/`AreaLight` — the env light's "infinitely far" becomes a value, not a
  separate code path.
- Explicit, uniform **light selection** (`sample_one → (light, pmf)`) — a named
  seam that later admits light importance sampling (Stage 3, out of scope here).
- NEE becomes: select a light → `sample_li` → compute MIS pdfs → `occluded(…,
  dist − ε)` → if visible, add `radiance · bsdf · mis_weight`. The
  read-whatever-the-ray-hit block and its subtle marginal-pdf unbiasedness comment
  disappear; the marginal `light_pdf` stays, now as the explicit "pdf of the
  light-sampling strategy" for the power heuristic.

## User Stories

1. As a developer, I want an occlusion query that returns `true` on the first
   blocker within a distance bound, so shadow rays stop early and skip shading.
2. As a developer, I want occlusion to be part of the `Intersect` interface, so a
   mesh object's inner BVH early-exits as well (two-level occlusion), not just the
   top level.
3. As a maintainer, I want Stage 1 to leave the image **bit-identical**, so adding
   the primitive is provably safe (render pin unchanged).
4. As a developer, I want a light to hand back a full sample — direction,
   distance, radiance, pdf — so NEE can bound the shadow ray and take emission
   from the light it actually sampled.
5. As a developer, I want the environment light modelled under the same
   light-sample interface (distance ∞), so area and env lights share one NEE path.
6. As a developer, I want light selection to be an explicit, replaceable strategy
   (uniform now), so importance sampling can drop in later without touching NEE.
7. As a developer, I want NEE's shadow ray to be a distance-bounded occlusion
   test, so ≥half the rays get cheaper (early-exit + no shading).
8. As a maintainer, I want the reformulated NEE to remain **unbiased** — to still
   agree with the reference (Naive) integrator in the mean — so correctness is
   proven independently of the render pin, which is then re-pinned deliberately.

## Implementation Decisions

**Occlusion via a trait method with a default + a BVH override.** `Intersect`
gains `occluded(ray, t_max) -> bool`; the default routes through `intersect` (one
test, correct for single primitives), and `BVH<T>` overrides it with a real
any-hit traversal. `ObjRef` (the top-level proxy) overrides `occluded` to delegate
to its geometry, so occlusion recurses into a mesh's inner BVH. This is PBRT's
`IntersectP` on the primitive interface — the correct home for the query.

**Two focused traversals, not one generic one.** `closest_hit` and the new
any-hit walk are kept as separate methods. The any-hit loop is genuinely simpler
(no near/far ordering, no `curr_max` tightening — just "any hit in `(ε, t_max)`?
return true"). Folding both into a `walk<const ANY_HIT: bool>` would add branches
to the hot closest-hit path for the sake of DRY; PBRT keeps them separate and so
do we. This does *not* regress the earlier "one traversal" simplify — that removed
a *duplicate closest-hit*; occlusion is a different query.

**Per-geometry distance in `sample_li`.** The distance the shadow ray is bounded
to is the distance to the sampled light surface along `wi`: for a Quad, the
sampled point is known (`|point − origin|`); for a Sphere, the near cone-hit
distance (an intersection, as `surface_pdf_value` already computes for the pdf);
for the env light, ∞. The solid-angle `pdf` reuses the existing `pdf_value`
machinery. Radiance is the light's emission — currently a constant per emissive
object (emission is Solid-only), so it can be read from the object's material.

**CORRECTION (found during Stage-2 implementation): the marginal pdf CANNOT
stay — it must become per-light everywhere.** The original sketch above ("keep
the marginal `light_pdf`") is *wrong* and would be **biased**. Worked through:

- Today's estimator (A) samples a *direction* from the light mixture and reads
  the *closest surface's* emission `L_i(wi)`, weighted by the marginal pdf
  `P(wi) = Σ_j (1/n)·p_j(wi)`. Unbiased — but it needs `L_i(wi)`, which only a
  *closest-hit* can give. Occlusion (a boolean) can't feed it.
- To use occlusion you must switch to estimator (B): pick light `k`, evaluate
  *its* emission `L_k` times visibility `V_k`. That is unbiased **only** with the
  *per-light* pdf `(1/n)·p_k(wi)` — because `Σ_k L_k·V_k = L_i` (only the closest
  light is visible), the `p_k` cancels per term. Pairing per-light *radiance*
  `L_k·V_k` with the *marginal* pdf gives a weighted average of `L_k`, not `L_i`
  → **biased**.
- MIS then requires *partition of unity*: the light-branch weight and the
  BSDF-branch weight at a given direction must use the **same** light pdf. So the
  emitter-hit branch and the env-escape branch must *also* switch from the
  marginal to the per-light pdf of the *specific* light/env that was hit
  `(1/n)·p_hit(dir)`.

**Consequences (the true Stage-2 scope):**
1. `sample_li` returns per-light pdf `(1/n)·p_k(wi)`, `dist`, `radiance = L_k`.
2. **The emitter-hit MIS branch must know *which* light it hit**, to compute
   `(1/n)·p_that_light(dir)`. So `HitRecord` gains the hit object's area-light
   handle (`Option<&dyn AreaLight>`), populated by `World::intersect`; an
   unregistered/BSDF-only emitter reports `None` → light-pdf 0 → full BSDF weight
   (matches today).
3. A per-light env-pdf accessor `World::env_pdf(dir) = (1/n)·direction_pdf(dir)`
   for the env-escape branch.
4. **All three** MIS branches (NEE, emitter-hit, env-escape) are rewritten
   consistently on the per-light pdf. The marginal `light_pdf` becomes unused.

This is a genuine estimator reformulation with real bias risk, gated hard by the
MIS-vs-Naive unbiasedness tests — not the "keep the marginal, swap in occlusion"
change the sketch implied. Same converged image (both unbiased); the render is
deliberately re-pinned.

**Stage boundary is a real seam.** Stage 1 ships and is validated on its own
(bit-identical) before Stage 2 touches the integrator. The deliberate render
re-pin happens only in Stage 2, gated behind the unbiasedness tests passing.

## Testing Decisions

**Stage 1 (bit-identical).**
- **Occlusion-vs-closest-hit equivalence (new, load-bearing):** over a battery of
  pseudo-random rays against a multi-object World (and a `BVH<Triangle>` from a
  mesh), assert `occluded(ray, t_max)` equals `intersect(ray, (ε, t_max)).is_some()`
  — i.e. the fast boolean agrees with the truth the closest-hit path already
  provides. This is the direct correctness guard for the new query, independent of
  any integrator.
- **Two-level recursion:** a World with a mesh object — assert an occluder *inside*
  the mesh is detected (proves the inner BVH's any-hit path is reached).
- **Render pin unchanged** (`0x9436e82cbff110f1`): Stage 1 must not touch the
  image. Plus a micro-bench of `occluded` vs `closest_hit` to bank the win.

**Stage 2 (re-pin).**
- **Unbiasedness (the real correctness guard):** the existing MIS-vs-Naive
  statistical tests in `mis.rs` (`mixture_matches_pure_gi_mean`,
  `mis_agrees_with_naive_in_mean_on_a_broad_sky`, the variance-reduction tests)
  must stay green — they prove the reformulated NEE converges to the same image as
  the reference path tracer. Correctness is established here, *not* by the pin.
- **Light-sample analytic checks:** `sample_li` returns the correct solid-angle
  `pdf` (reuse the existing analytic-pdf tests), a finite `dist` for area lights
  and ∞ for env, and the sampled light's radiance.
- **Deliberate render re-pin:** once unbiasedness holds, update
  `BASELINE_FINGERPRINT` to the new value and note in the test doc that it was
  re-pinned for the NEE reformulation (same converged image, new per-sample
  estimator). This is expected and intended, not a regression.

## Out of Scope

- **Light importance sampling** (Stage 3): power/spatial light selection, a light
  BVH. The selection *seam* is built here (uniform); the smarter strategy is a
  later unit, and needs its own re-pin.
- **Textured / non-constant emission:** emission stays Solid-only (constant per
  emitter), as today; `LightSample.radiance` is that constant.
- **Spectral / participating media / other integrators.** Naive is unchanged.
- **Removing the marginal `light_pdf`:** it is still needed for MIS; not a target.

## Further Notes

- Expected win (research §6): shadow rays are ≥50% of rays; any-hit early-exit +
  no shading is a "meaningful double-digit-percent" reduction in shadowed scenes.
  The harness (`traversal` + a new occlusion micro-bench) and the render bench
  measure it; the `bvh-stats` counter can be extended to count occlusion tests if
  useful.
- Stage 1 is the low-risk place to start: it adds a reusable primitive with zero
  image change, so it can land and be measured before committing to the estimator
  rewrite and its re-pin.
- The render pin's own doc comment already explains the deterministic-render
  contract; re-pinning is a one-constant change plus a comment noting why.
