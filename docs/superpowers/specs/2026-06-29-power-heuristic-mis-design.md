# Power-Heuristic Multiple Importance Sampling — Design

**Date:** 2026-06-29
**Status:** Approved (autonomous — design decisions made by the implementer)

## Context

The current integrator (`Camera::ray_color`) is a one-sample mixture path tracer:
each diffuse bounce draws one ray from a 50/50 mixture of cosine and light
directions. That is already the *balance heuristic* in the one-sample model. To
reduce fireflies further (beyond the per-sample luminance clamp just added), we
move to the **multi-sample model with the power heuristic** — the classic Veach
NEE + BRDF MIS path tracer. It evaluates *two* estimators per diffuse bounce
(an explicit shadow ray and the BRDF bounce) and combines them with weights that
suppress the high-variance technique per direction.

This reuses all existing machinery: `Material::scattering_pdf`/`is_specular`,
`IntersectGroup::light_pdf`/`sample_light_dir`, `Intersect::pdf_value`/`random_dir`,
and `Material::emitted`.

## The math

Power heuristic (β = 2): for a technique with PDF `a` competing with PDF `b`,
`w(a, b) = a² / (a² + b²)` (0 if both are 0).

At a Lambertian hit, BRDF·cos for Lambertian equals `albedo · scattering_pdf`
(since `scattering_pdf = cos/π` and the BRDF is `albedo/π`). The two estimators:

- **Light sample (NEE):** sample direction `ld` toward a light (`sample_light_dir`),
  `p_light = light_pdf(hit.p, ld)`, `p_brdf = scattering_pdf(hit, ld)`. Trace the
  shadow ray; let `Le` be the emitted radiance of the nearest hit (0 if an
  occluder or non-emitter is hit — this is the visibility test). Contribution:
  `throughput · w(p_light, p_brdf) · albedo · (p_brdf / p_light) · Le`.
- **BRDF sample (indirect bounce):** cosine direction `dir`,
  `p_brdf = scattering_pdf(hit, dir)`, throughput update `throughput *= albedo`
  (`= albedo · p_brdf / p_brdf`). When a later bounce ray lands on an emitter,
  add its emission weighted by `w(prev_p_brdf, p_light_for_that_dir)`.

A non-registered emitter (mesh/sphere) has `p_light = 0` along any direction, so
`w_brdf = 1` and its emission is counted in full via the BRDF path — meshes still
illuminate via GI, unbiased. With no lights at all, `p_light = 0` everywhere, so
the integrator collapses to the plain cosine path tracer.

## The integrator (rewrite of `Camera::ray_color`)

Iterative. Track `specular_bounce` (true on the camera ray and after specular
bounces — emission gets full weight then, since NEE can't sample those) and
`prev_brdf_pdf` (the cosine PDF of the bounce that produced the current ray).

```
color = 0; throughput = 1; current = ray;
specular_bounce = true; prev_brdf_pdf = 0;
for _ in 0..max_depth {
    let Some(hit) = world.intersect(current, (0.001, INF)) else {
        color += throughput * background; break;
    };

    let emitted = hit.material.emitted(hit.u, hit.v, hit.p);
    if emitted != ZERO {
        if specular_bounce {
            color += throughput * emitted;            // camera ray / post-specular: full weight
        } else {
            let p_light = world.light_pdf(current.origin, current.direction);
            color += throughput * emitted * power_heuristic(prev_brdf_pdf, p_light);
        }
    }

    let Some((scattered, atten)) = hit.material.scatter(current, hit, rng) else { break; };

    if hit.material.is_specular() {
        throughput *= atten; current = scattered;
        specular_bounce = true; continue;
    }

    // Lambertian.
    let albedo = atten;

    // (1) NEE light sample.
    if let Some(ld) = world.sample_light_dir(hit.p, rng) {
        let p_light = world.light_pdf(hit.p, ld);
        let p_brdf = hit.material.scattering_pdf(&hit, &ld);
        if p_light > 0.0 && p_brdf > 0.0 {
            let shadow = Ray::new_t(hit.p, ld, current.time);
            if let Some(lh) = world.intersect(&shadow, (0.001, INF)) {
                let le = lh.material.emitted(lh.u, lh.v, lh.p);
                if le != ZERO {
                    let w = power_heuristic(p_light, p_brdf);
                    color += throughput * w * albedo * (p_brdf / p_light) * le;
                }
            }
        }
    }

    // (2) BRDF bounce (cosine).
    let dir = cosine_direction(&hit.normal, rng);
    let p_brdf = hit.material.scattering_pdf(&hit, &dir);
    if p_brdf <= 0.0 { break; }
    throughput *= albedo;
    prev_brdf_pdf = p_brdf;
    specular_bounce = false;
    current = Ray::new_t(hit.p, dir, current.time);
}
color
```

Notes:
- `current.origin`/`current.direction` give the previous diffuse hit point and the
  bounce direction, so `prev_hit_p` need not be tracked separately.
- The NEE shadow ray traces to infinity and reads `emitted()` of the nearest hit:
  occluder → 0 (shadowed), light → its radiance. One `intersect` handles
  visibility, which-light, and emission. `Light::emit` is not consulted (it
  remains registration metadata).
- `ld` is unnormalized (`random_dir`); `scattering_pdf`/`light_pdf`/intersection
  are magnitude-invariant — do not normalize it.
- The per-sample `firefly_clamp` (just added) is orthogonal and stays in place.

## New code

- `fn power_heuristic(a: f32, b: f32) -> f32` — private free function in
  `camera.rs` (next to `cosine_direction`): `a²/(a²+b²)`, 0 when both are 0.
- The `ray_color` rewrite above. `cosine_direction` is reused. The specular branch
  is unchanged in spirit (`throughput *= atten`, continue).

## Scope / out of scope (YAGNI)

**In:** power-heuristic NEE + BRDF MIS for Lambertian; specular materials
BRDF-only with full-weight emission; meshes/spheres via BRDF path (unbiased);
collapse to plain path tracer with no lights.

**Out:** Russian roulette; sphere/composite *light* sampling (still `area()=0`);
multiple light samples per bounce / light-tree selection; removing the now-unused
`Light::emit` field (leave it).

## Testing

- `power_heuristic`: `(3,4) → 9/25 = 0.36`; `(x,0) → 1` for `x>0`; `(0,x) → 0` for
  `x>0`; `(0,0) → 0` (no NaN).
- Unbiasedness: on a Cornell-like diffuse scene, the MIS integrator's mean (light
  registered) equals the pure-GI mean (light not registered) within tolerance —
  both estimators are unbiased, so MIS must not shift the mean. (Mirrors the
  existing `mixture_matches_pure_gi_mean` test; that test should continue to pass
  after the rewrite, confirming correctness is preserved.)
- Variance reduction: with a *small, bright* light, the per-sample variance of the
  MIS integrator (light registered) is markedly lower than the pure-GI integrator
  (light unregistered) over the same sample count — the whole point of NEE+MIS.
- Direct emission: a camera ray hitting an emitter returns its emission
  (`specular_bounce` full weight on the first hit) — the existing
  `camera_sees_emitter_emission` test.
