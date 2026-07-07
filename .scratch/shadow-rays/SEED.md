# Resume breadcrumb: shadow-rays (any-hit occlusion + light-point NEE)

Status: in-progress — Stage 1 done; Stage 2 slice 1 done; slices 2–5 remain.

Handoff mid-Stage-2. The design is **locked** (grilled hard — do not re-litigate);
this is mostly execution against a settled plan. Read `PRD.md` in this dir first —
especially the **"Locked design (post-grill)"** section — it is the durable spec.

## The one thing to do

Continue **`/tdd`** on Stage 2, slices 2 → 5, then `/simplify`. Suggested open line:

> Read `.scratch/shadow-rays/PRD.md` (esp. "Locked design") and `SEED.md`, and
> continue Stage 2 — next slice is `World::sample_light` → `LightSample`. Keep the
> render pin bit-identical until the deliberate re-pin in the estimator slice.

## Where things stand (commits, newest first)

- `d89a363` Stage 2 **slice 1**: `AreaLight::sample_toward(origin,u,v) -> AreaLightSample{wi,t_light,pdf}` on quad/triangle/sphere. Added *alongside* `sample_dir` (not yet retired), so integrator unchanged, **render pin still bit-identical** (`0x9436e82cbff110f1`).
- `2ae59fa` locked Stage-2 design after the self-grill.
- `d99a576` ADR 0002: procedural textures are world-space (a tangent found in the grill; orthogonal to this work).
- `8cbbb38` **design correction**: per-light pdf, NOT marginal (the original sketch was biased — see below).
- `a7fbffd` Stage **1** done: `Intersect::occluded` (default = closest-hit) + `BVH::any_hit` override + `ObjRef`/`World::occluded`. Bit-identical. Tested (occluded == closest-hit-within-t_max; mesh recursion).
- `d3adc67` initial PRD.

Baseline: `cargo test` all green (199 lib + render pin). `cargo test --lib --features bvh-stats` also green.

## Locked design (do NOT re-litigate — grill happened)

1. **Estimator (B), per-light pdf everywhere.** Verified against PBRT `SampleLd`.
   Light-sampling pdf is `(1/n)·p_k(wi)` (per-light), NOT the marginal — for both
   the estimator `1/pdf` and the MIS weight, in **all three** MIS branches (NEE,
   emitter-hit, env-escape). Pairing per-light radiance with the marginal pdf is
   *biased*.
2. **Identity on the hit, pdf math in the World.** `HitRecord` gains
   `light: Option<&dyn AreaLight>` — a dumb token the integrator holds but never
   dereferences. The World owns selection: `world.light_pdf(hit.light, origin,
   dir)`, `world.env_pdf(dir)`, and `sample_light`'s pdf all return fully-formed
   `(1/n)·p` (integrator never touches `n` or `pdf_value`).
3. **Two geometry methods.** `sample_toward` (done, slice 1) is the polymorphic
   per-shape kernel; `pdf_value` is KEPT for the emitter-hit branch (arbitrary
   BSDF direction). `sample_dir` is RETIRED in the NEE-rewrite slice (its only
   caller is the marginal `sample_light_dir` being deleted).
4. **`World::sample_light`** = policy layer: pick light `(1/n)`, call the kernel,
   add `radiance = material.emitted(point)`, handle env (`t_light = ∞`, radiance =
   sky). Returns `LightSample { wi, t_light, radiance, pdf }`.
5. **NEE:** sample_light → `world.occluded(shadow, Interval(ε, t_max))` where
   t_max = `t_light` (∞ for env) → if visible, `color += throughput · w · albedo ·
   (p_brdf/pdf) · radiance`.
6. **Re-pin gate:** a NEW tight (~1–2%) high-sample MIS-vs-Naive agreement test is
   the gate. Correctness proven by the Naive oracle BEFORE the fingerprint moves.
   Existing 5–8% tests (`mixture_matches_pure_gi_mean` rel<0.05,
   `mis_agrees_with_naive_in_mean_on_a_broad_sky` rel<0.08) stay as fast guards
   but are TOO LOOSE to authorize a re-pin — hence the new tight one.

## Remaining slices

- **Slice 2 — `World::sample_light` → `LightSample`.** Additive, low-risk.
  `LightSample { wi, t_light, radiance, pdf }` in `world.rs`. Pick `i` in
  `0..light_count()`; area light → `obj.light.sample_toward(...)` + `radiance =
  obj.material.emitted(0,0, origin+wi*t_light)`; env slot → `sky.radiance(wi)`,
  `t_light=∞`, `env.direction_pdf(wi)`. `pdf = p_k / n`. Test: radiance = emission,
  `pdf*n ≈ light.pdf_value(origin, wi)`, point lands on the light.
- **Slice 3 — `HitRecord.light` + World pdf accessors.** Add `light: Option<&'a
  dyn AreaLight>` to `HitRecord`; populate in `World::intersect`
  (`obj.light.as_deref()`); update `HitRecord::from_geo` (+ test-only
  `HitRecord::new` → `None`). Add `World::light_pdf(Option<&dyn AreaLight>, origin,
  dir) -> f32` = `l.map_or(0.0, |l| l.pdf_value(o,d)) / n`, and
  `World::env_pdf(dir) -> f32` = `if Sky::Env {direction_pdf/n} else 0`.
- **Slice 4 — the estimator rewrite (RISKY, the re-pin).** Rewrite all 3 MIS
  branches in `mis.rs` to per-light pdf; NEE uses `sample_light` + `occluded`.
  Retire `sample_dir`, `World::sample_light_dir`, marginal `World::light_pdf`
  (the old one), and likely `LightRef`/`light_refs` if now unused. GATE: the new
  tight MIS-vs-Naive test must be green. THEN re-pin `BASELINE_FINGERPRINT` in
  `tests/render_characterization.rs` + note why in its doc comment.
- **Slice 5 — `/simplify`.** Expected finds: sphere `sample_toward` does 2
  intersects (t_light + pdf_value) — fuse to 1 (grill Q4 wanted ≤1; quad already
  0). Also dead-code sweep after retiring the marginal path.

## Gotchas / verify during implementation

- **Shadow-ray bounds:** occlusion interval `(ε, t_max)` where `t_max = t_light *
  (1 - 1e-3)` for area lights (stop just short of the light so it isn't its own
  occluder), `∞` for env. Ray dir = `wi` (unnormalized), so `t_light` is in `wi`
  units — the occlusion `Interval` is in the same units. No world-distance convert.
- **Division by n:** fold `/n` inside the `Option::map` so a `None` light gives
  0.0 (never `0.0 / 0`). n≥1 whenever a light handle is `Some`.
- **BSDF-only emitters** (mesh/box lights, `light: None`): `hit.light` is `None` →
  light-pdf 0 → full BSDF weight. Matches today's behaviour.
- **Textured emission is out of scope** (emission is Solid-only) so
  `emitted(0,0,point)` is fine; if textured emitters ever land, `sample_toward`
  must also return the sampled point's `(u,v)`.
- Naming is deliberate: `sample_toward` (geometry, no radiance — it's
  material-agnostic), `sample_light` (World, adds radiance). See ADR/grill.

## Why re-pin is safe/correct

Both the old (marginal, read-closest) and new (per-light, occlusion) estimators
are unbiased → same *converged* image, different per-sample values. The
fingerprint is a reproducibility characterization, not a correctness proof;
correctness = the tight Naive-oracle agreement, which must pass first.
