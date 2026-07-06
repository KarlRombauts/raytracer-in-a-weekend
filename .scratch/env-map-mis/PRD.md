# PRD: Multiple Importance Sampling for the Sky (environment-map light)

Status: ready-for-agent

## Problem Statement

When a scene is lit by an HDR **Sky** (an **Environment map**), diffuse surfaces
are lit *noisily*. The tracer currently finds the sky only by BSDF sampling — it
bounces a ray in a random direction and hopes it lands on a bright part of the
sky. For a smooth sky that's fine, but the moment the HDR has a small bright
feature (a sun, a bright window), a diffuse surface catches that light only on
the rare bounce that happens to point at it. The result is **fireflies** and a
grainy image that needs thousands of **Samples** to clean up.

Every other light in the scene already avoids this: an emissive **Object** is
registered as a **Light** and sampled directly via **Direct light sampling**,
combined with BSDF sampling through **MIS**. The sky is the one emitter that is
*not* sampled directly — so it is the one emitter that produces fireflies.

## Solution

Make the sky a first-class **Light** that the tracer can sample directly, so its
bright directions are found reliably every sample and combined with BSDF sampling
by the **Power heuristic** — exactly the treatment area lights already get.

From the user's point of view:

- An HDR-lit scene converges to a clean image in far fewer samples; the sun lights
  the floor without salt-and-pepper speckle.
- Switching the **Integrator** picker from Naive to MIS visibly removes the noise
  on an environment-lit scene — the comparison the picker exists for.
- The image is unchanged in the limit: MIS only cuts noise, it does not change the
  picture (same mean as a brute-force render).
- Flat-colour skies and non-HDR scenes render exactly as before — no regressions.

Under the hood, the **World** becomes the single authority for light sampling: the
environment joins the World's set of lights alongside area lights, so MIS "just
works" through the existing sampling machinery rather than a parallel path.

## User Stories

1. As someone rendering a scene lit by an HDR sky, I want diffuse surfaces lit
   cleanly, so that I get a usable image without pushing samples into the thousands.
2. As someone using a "sunny" HDR with a small bright sun, I want that sun's
   contribution found on every sample, so that lit areas and soft shadows converge fast.
3. As someone comparing integrators, I want MIS to visibly beat Naive on an
   environment-lit scene, so that the Output-inspector picker demonstrates its value.
4. As someone rendering, I want the MIS result to match the brute-force result in
   the limit (same mean), so that importance sampling never biases my image.
5. As someone who rotates the sky with the yaw control, I want sampling to follow
   the rotation, so that the sampled directions track the sky's bright features.
6. As someone with a flat-colour sky, I want byte-for-byte-equivalent output to
   today, so that adding this feature cannot regress simple scenes.
7. As someone rendering a scene with *both* area lights and a bright sky, I want
   both sampled and correctly combined, so that neither is double-counted and
   neither is missed.
8. As someone rendering headless from the CLI, I want the same env-MIS benefit as
   the interactive renderer, so that batch renders converge just as fast.
9. As someone editing in the viewer, I want switching to a different HDR sky to
   transparently rebuild the sampling data, so that the render stays correct after
   a sky change without a manual step.
10. As someone rendering a very high-contrast sky (tiny, extremely bright sun), I
    want the estimator to stay stable (no NaNs, no frozen-black pixels), so that
    extreme HDRs don't break the image.
11. As someone rendering a scene where the sky is partly occluded (an object
    between the surface and the bright sky direction), I want the shadow correctly
    resolved, so that occluders cast proper shadows from the sky.
12. As a developer, I want the environment registered as one of the World's lights,
    so that MIS flows through the existing `light_pdf` / `sample_light_dir` seam
    rather than a second, parallel light-sampling path.
13. As a developer, I want the sky's importance-sampling data built once when the
    sky is loaded, so that there is no per-pass cost.
14. As a developer, I want the sky's miss-radiance and its light-sampling to come
    from one source of truth, so that the shading and the sampling pdf cannot drift
    apart (the failure mode that causes double-counting or dark bias).
15. As a developer, I want the environment light's pdf to integrate to 1 and to
    match its own sampler, so that MIS weights are provably correct.
16. As a developer, I want the Naive integrator unchanged, so that it remains the
    honest BSDF-only baseline the MIS result is compared against.
17. As a developer, I want no change to the `.scene` wire format, so that existing
    saved scenes keep loading (respecting ADR-0001).
18. As a developer extending lights later, I want the environment light and area
    lights to share one light abstraction, so that a future light type slots into
    the same seam.
19. As someone rendering, I want the pole directions (straight up / straight down)
    sampled correctly-weighted, so that the top/bottom of an HDR is neither over-
    nor under-represented.
20. As someone verifying the change, I want a scene and a test that make the noise
    reduction measurable, so that "it looks less noisy" becomes a real assertion.

## Implementation Decisions

- **Generalize the World's light set to host an infinite environment light.**
  Introduce a single **Light** abstraction that both surface **AreaLight**s and a
  new environment light satisfy: it answers "give me a direction from this shading
  point toward the light" and "what is the solid-angle pdf of that direction."
  `IntersectGroup.lights` becomes a collection of this abstraction; the current
  `Light { geom, emit }` struct is subsumed. `light_pdf` and `sample_light_dir`
  iterate the unified set unchanged in spirit (average pdf over lights; pick one
  light then sample it). The environment light's pdf is **position-independent**
  (an infinite light), so the abstraction must permit a light that ignores the
  shading `origin` — surface lights continue to use it.

- **The environment light owns an importance-sampling distribution over the
  EnvMap.** A 2D piecewise-constant distribution built as a marginal-over-rows plus
  a conditional-per-row (PBRT's `Distribution2D`), with each texel weighted by
  **luminance × sin θ** (the sin θ corrects the equirectangular pole stretch).
  Sampling draws an image position, maps it to a world direction, and reports a
  **solid-angle** pdf — the image-space density divided by the Jacobian
  `2π² · sin θ`. The two sin θ factors cancel, so the light samples directions in
  proportion to sky radiance. The distribution is built **once when the sky is
  loaded** (alongside the existing sky cache), never per pass.

- **The World owns the Sky (candidate-4 alignment).** Sky ownership moves from the
  integrator into the World: `build_world` registers an environment light when the
  scene's sky is an Environment map, and the World becomes the source for both the
  sky's miss-radiance and the env light's pdf. `Sky` leaves the `Mis` integrator
  and `build_integrator`; the integrator queries the World instead. This makes the
  sky's shading and sampling share one source of truth.

- **The Mis integrator's sky-on-miss becomes MIS-weighted.** Today a BSDF bounce
  that escapes to the sky adds `sky.radiance(dir)` at full weight. Once the sky is
  sampled directly, that would double-count, so this branch is weighted by the
  power heuristic against the environment light's pdf — mirroring the existing
  emitter-emission MIS branch. Camera rays and post-specular bounces still take the
  sky at full weight (they cannot be light-sampled).

- **Next-event estimation gains the escaping-shadow-ray case.** A shadow ray
  sampled toward the environment light contributes `sky.radiance(dir)` only if it
  **escapes** (unoccluded); if it hits any geometry it is occluded and contributes
  zero — a natural extension of the existing "radiance from whatever the shadow ray
  reaches" logic.

- **Naive is unchanged.** It performs no NEE, so it keeps taking the sky at full
  weight on a miss — the correct behaviour for a BSDF-only estimator and the
  baseline MIS is measured against.

- **Only the Environment sky is importance-sampled.** A flat/solid sky
  (`Sky::Flat`) is already low-variance under BSDF sampling, so it registers no
  environment light and the miss branch keeps full weight for it. (A uniform-sphere
  environment light for flat skies is a possible later addition, not part of this.)

- **No `.scene` wire-format change.** The sky is already a `serde(skip)` runtime
  selection (a bundled HDR name); the environment light and its distribution are
  built at world-build time from that name. ADR-0001 is unaffected.

## Testing Decisions

A good test exercises **external behavior at a seam**, never internals: it builds
a World / integrator / sampler through public constructors and asserts on outputs
(radiance means and variances, pdfs, sampled directions), so it survives
refactors. All randomness is seeded (`SmallRng`) and tolerances are set tight
enough to catch a wrong Jacobian or a missing MIS weight — not just gross errors.

Three seams, most to least existing:

- **Seam 1 — `Integrator::radiance` on `Mis` (existing, highest, behavioral).**
  The env-MIS payoff is asserted here, the same seam and shape as the current MIS
  tests. A scene with a bright `Sky::Env` hot-spot and a diffuse floor: `Mis`
  per-sample variance is markedly below `Naive` variance, and `Mis` agrees in mean
  with the brute-force / `Naive` estimate (unbiased). Prior art:
  `mixture_matches_pure_gi_mean`, `mis_cuts_variance_versus_pure_gi`,
  `naive_and_mis_agree_in_mean` in the `integrator::mis` tests.

- **Seam 2 — World light sampling, `light_pdf` / `sample_light_dir` (existing,
  mid).** With an environment light registered, `sample_light_dir` sometimes
  returns directions toward bright sky, and `light_pdf` includes the env light's
  directional pdf while remaining an average over all registered lights. Prior art:
  `light_mixture_tests` in `group.rs` (extend, don't replace).

- **Seam 3 — the environment distribution (new, lowest).** The one genuinely new
  seam. Its sampler must satisfy the invariants any importance sampler must: its
  pdf integrates to ≈1 over the sphere; sampled directions land with frequency
  proportional to texel brightness (a bright patch is hit proportionally often);
  and the pdf reported by `sample` equals `pdf(dir)` for the same direction. Prior
  art: the sphere/quad `pdf_value` tests and the distribution invariants in
  `sampling.rs`.

## Out of Scope

- The rest of architecture-review candidate 4 beyond what env-MIS needs: a
  top-level BVH over the World, unifying the two BVH implementations, and the
  `Light.emit` source-of-truth cleanup. Only the light-abstraction generalization
  required to host the environment light is in scope.
- Importance-sampling a flat/solid sky.
- Advanced environment sampling: portal sampling, bright-region guiding, adaptive
  or hierarchical refinement, product sampling with the BSDF.
- Any integrator other than `Mis`; `Naive` stays strictly BSDF-only.
- wasm environment-map support (skies are already unavailable on wasm — the sky
  loads to `None` there, so no environment light is registered).
- Persisting the sky or integrator choice in the `.scene` wire format (that's the
  separate versioned-format work, candidate 7).

## Further Notes

- **This delivers the core of architecture-review candidate 4** — the World as the
  single light-sampling authority, with the sky living in the World. It was chosen
  (option B) over an integrator-local sky-NEE path (option A) precisely because it
  routes env-MIS through the existing light-sampling seam instead of a second,
  parallel one — fewer seams, one source of truth.
- **The sin θ appears twice and cancels**: as a weight (× sin θ) when building the
  distribution, and in the pdf conversion (÷ 2π² sin θ). The net effect is sampling
  directions in proportion to sky radiance. Worth an explicit sanity assertion.
- **Position independence**: the environment light's pdf does not depend on the
  shading point (it is infinitely far away), unlike surface area lights whose pdf
  depends on `origin`. The unified light abstraction must accommodate both.
- **Numerical care**: extreme HDRs produce large pdfs at the sun; the existing
  `power_heuristic` already guards against overflow/NaN, and the accumulator's
  firefly clamp and NaN defence remain the backstop. The env light's pdf should
  return 0 (not NaN) for degenerate directions (e.g. exactly at a pole).
- Background reading captured during design: PBRT §12.6 (Infinite Area Lights,
  the sin θ weighting and the 2π² sin θ pdf conversion), §13.3 (the inversion
  method / `Distribution1D`), §13.5 (change of variables / the Jacobian). See the
  `teach/` workspace lessons 01–03 for the worked intuition.
