# Mission

**Learner:** Karl — building a Monte-Carlo path tracer (grown from *Ray Tracing in a Weekend*) in Rust, with an egui editor.

**Goal:** Understand environment-map importance sampling well enough to
**implement MIS on the sky map** in the tracer — so an HDR sky with a bright,
small feature (a sun, a bright window) lights diffuse surfaces *cleanly* instead
of producing fireflies.

**Why it matters now:** The tracer just grew an `Integrator` seam and a `Sky`
type (`src/integrator/`). The sky is currently sampled BSDF-only (a ray that
misses returns `sky.radiance(dir)`), so bright, concentrated sky features are
found only by luck — the exact high-variance case NEE + MIS fixes. Extending MIS
to the sky is the natural next feature, and it's a forcing function for the
"fold Sky into the World" refactor (architecture-review candidate 4).

**Definition of done for the learning:** Karl can explain, and then implement,
how to (1) turn the HDR into a 2D distribution he can sample by brightness,
(2) convert an image-space sample to a world direction with a correct
solid-angle pdf, and (3) wire both sampling strategies into the MIS estimator.

**Grounding code:** `src/texture/env_map.rs` (the `EnvMap`), `src/integrator/mis.rs`
(the MIS loop), `src/integrator/sky.rs` (the `Sky` type).

## Current focus

Foundations: the mental model of the environment map as a *grid of texels in
rows*, and what it means to "pick a bright direction." (Lesson 0001.)
