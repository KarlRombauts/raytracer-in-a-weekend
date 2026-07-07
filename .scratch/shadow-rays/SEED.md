# Resume breadcrumb: shadow-rays (any-hit occlusion + light-point NEE)

Status: **DONE** — Stage 1 + Stage 2 (all slices 1–5) complete, incl. the
`/simplify` pass. `cargo test` all green (215 lib + render pin; 217 with
`--features bvh-stats`). The design was **locked** (grilled — see `PRD.md`
"Locked design"); this now records how it finished. History below is retained.

## How it finished (Stage 2 slices 2–5, commits newest first)

- `5bfa499` `/simplify`: `LightSample::shadow_interval()` owns the shadow-ray
  bound (∞·(1−ε)=∞ collapses the finite-vs-env branch out of the integrator);
  env branch keeps `direction_pdf` (differs from the sampler pdf by a few %, must
  match `env_pdf`). Bit-identical.
- `55ce599` slice 5: retired the marginal path (`sample_light_dir`, `LightRef`,
  `light_refs`, trait `sample_dir`); fused sphere `sample_toward` to 1 intersect
  (was 2) via inherent `cone_pdf`. Bit-identical.
- `5e66432` slice 4: per-light occlusion NEE — all three MIS branches on the
  chosen light's `(1/n)·p_k`; `World::light_pdf` → per-light identity.
- `d726ee1` slice 3: `HitRecord.light` identity token + `World::env_pdf`.
- `def037e` slice 2: `World::sample_light` → `LightSample`.

## The one thing that surprised us: the pin did NOT re-pin (correctly)

The PRD anticipated a deliberate re-pin; it wasn't needed. The pinned Cornell box
is **single-light + black-sky**, where at `n = 1` the per-light pdf equals the old
marginal pdf and `sample_light` consumes rng identically to the retired
`sample_light_dir` — so estimators (A) and (B) coincide bit-for-bit. Fingerprint
stays `0x9436e82cbff110f1` (doc-noted in `render_characterization.rs`).

Because every *pre-existing* test was also `n = 1` (reformulation a no-op there),
the `n ≥ 2` correctness — the whole point of the per-light correction — is proven
by a NEW **two-area-light** MIS-vs-Naive gate in
`integrator::mis::repin_gate_tests`. A mutation dropping the `1/n` factor is
invisible to every single-light test but fails that gate at rel=0.41 — so it is
necessary and load-bearing. Keep it.

## If resuming: Stage 3 (out of scope here)

Light importance sampling — power/spatial selection, a light BVH. The selection
*seam* (`sample_light` picks uniformly `1/n`) is built; swap the policy there.
Needs its own re-pin. See PRD "Out of Scope".

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
