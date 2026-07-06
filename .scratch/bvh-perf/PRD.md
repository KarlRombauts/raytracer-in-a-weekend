# PRD: BVH benchmark harness

Status: done

> **Done and committed** (`16ac39b` counter core, `0d1240e` benches+example,
> `767d8e9` /simplify). Test-first `stats` counter behind the zero-cost
> `bvh-stats` feature; `benches/bvh.rs` (traversal per mesh×orientation, build,
> top-level, macro render) + `examples/bvh_stats.rs` diagnostic, sharing
> `src/bench_support.rs`. Render pin bit-identical. Ready to measure the first
> optimization — 32-byte node compaction (bit-identical per RESEARCH.md).

> A measurement harness that must exist **before** we touch BVH performance, so
> every optimization (node compaction, traversal culling, wide BVH, any-hit
> shadow rays) can be proven to help — or caught regressing. Companion to
> `.scratch/bvh-perf/RESEARCH.md` (the optimization survey from PBRT / Bikker /
> the RT literature).

## Problem Statement

We want to improve BVH performance, but we have no way to measure whether a
change actually helps, hurts, or does nothing. Wall-clock render time alone is
noisy (rayon threads) and dilutes a BVH win under shading cost, and it can't
distinguish "the traversal got cheaper" from "the tree got better." Without a
harness, every optimization is a guess.

## Solution

A **criterion**-based benchmark harness with two layers:

1. **Micro-benchmarks** that isolate the BVH — traversal throughput (ns/ray) and
   build time — over real meshes at several sizes, plus a synthetic top-level
   scene. These are the precision instrument: they attribute a change to the BVH
   directly.
2. **One macro benchmark** — a fixed, single-threaded, low-sample render of a
   mesh-heavy scene — as a reality check that micro wins translate to real time.

Plus a **deterministic traversal-work counter** (node box tests + leaf primitive
tests) behind a zero-cost `bvh-stats` cargo feature, surfaced through a separate
diagnostic so it never pollutes timing. The counter is the hardware-independent
quality signal that *explains* a timing change: a bit-identical layout change
keeps the counts constant while ns/ray drops; a tree-quality change drops the
counts.

## User Stories

1. As a developer optimizing the BVH, I want ns/ray traversal throughput over a
   real mesh, so I can measure whether a traversal or node-layout change helped.
2. As a developer, I want traversal measured **per camera orientation** around an
   anisotropic mesh (the long-skinny dragon face-on vs edge-on), so a front-to-
   back ordering or split-axis regression shows up as one orientation blowing up
   rather than hiding in the mean.
3. As a developer, I want traversal measured at **several mesh sizes** (teapot /
   bunny / dragon), so I see how a change scales with triangle count.
4. As a developer, I want BVH **build time** measured per mesh, so SAH-quality and
   parallel-build changes are visible.
5. As a developer, I want the **top-level `BVH<ObjRef>`** exercised by a synthetic
   many-object scene, since real scenes have too few top-level objects to time.
6. As a maintainer, I want **one end-to-end render** number on a mesh-heavy scene,
   so I can confirm micro wins move real render time.
7. As a developer, I want deterministic **counts of node box tests and leaf
   primitive tests**, so I can prove a bit-identical change (counts constant, time
   down) versus a tree-quality change (counts down).
8. As a maintainer, I want the counter to be **strictly zero-cost when off**, so
   normal builds, the render, and the timing benches are byte-identical to today.
9. As a developer, I want a **before/after comparison workflow**, so I can save a
   baseline, make a change, and read the delta.
10. As a maintainer, I want the harness inputs to be **deterministic** (fixed
    seeds, fixed ray sets, single-threaded traversal), so runs are comparable.

## Implementation Decisions

**criterion, benches under `benches/`.** The standard Rust statistical bench
crate (warmup, outlier rejection, `--save-baseline` / `--baseline`). Added as a
dev-dependency; no effect on the shipping binary.

**Robust asset paths.** Bench/example setup resolves `assets/objs/*.obj` via
`CARGO_MANIFEST_DIR`, not a working-dir-relative path (the existing
`ObjData::load("./objs/…")` convention breaks when run from the crate root).

**Micro — traversal throughput.** Build a `BVH<Triangle>` from teapot / bunny /
dragon. Fire a fixed, seeded ray battery from a **ring of camera orientations**
(face-on down the long axis, edge-on, top-down, two obliques). Report **ns/ray
per (mesh, orientation)**, single-threaded for a clean signal.

**Micro — build time.** Time `BVH::build(triangles)` per mesh size.

**Micro — top-level traversal.** A synthetic scene of a few hundred spheres →
`World` → seeded rays, so the `BVH<ObjRef>` path is actually exercised.

**Macro — one render.** A fixed, low-sample, single-threaded, fixed-seed render
of the dragon scene (`new_bvh`). Correctness stays guarded by the existing
cornell render pin; this is a timing reality-check only.

**Traversal-work counter behind `bvh-stats` (off by default).** Two thread-local
counters — **node box tests** (each AABB test) and **leaf primitive tests** (each
`prim.intersect` attempt) — incremented inside the single `closest_hit` via
`#[cfg(feature = "bvh-stats")]` lines that compile to nothing when the feature is
off. Both BVH levels funnel through `closest_hit`, so counts aggregate across the
whole two-level traversal automatically (the pure-triangle micro-bench has no top
level, so there it is exactly triangle + box tests). Small API:
`bvh_stats::reset()` / `bvh_stats::snapshot() -> (boxes, prims)`. Surfaced via
`examples/bvh_stats.rs` (run with `--features bvh-stats`, single-threaded),
printing counts per (mesh, orientation) — kept entirely out of the timed loop.

**Workflow.** `cargo bench` for timing (feature off);
`cargo bench -- --save-baseline before` → change → `--baseline before` for the
delta. `cargo run --release --example bvh_stats --features bvh-stats` for counts.

## Testing Decisions

**What makes a good test here.** The harness is measurement code, so the value is
in the counter being *trustworthy*, not in unit-testing criterion. The counter is
the only piece with logic worth pinning.

**Counter tests (under `--features bvh-stats`).**
- **Determinism:** the same ray battery over the same BVH yields **identical
  counts** across two runs.
- **Monotonic sanity:** a denser mesh (dragon) records **more** primitive tests
  than a sparser one (teapot) for a comparable ray set.
- **Non-interference:** enabling the feature does not change traversal *results* —
  `closest_hit` returns the same hits with the counter on as off (guards that the
  `#[cfg]` lines only count, never alter control flow).

**Existing guards that must keep passing.** The cornell render pin
(`0x9436e82cbff110f1`) and the BVH-vs-linear equivalence test — the harness adds
benches/examples and a feature flag, and must not perturb production traversal.

## Out of Scope

- **CI perf-gating** — no automated regression failure in CI for now; the harness
  is developer-run.
- **Hardware counters / flamegraphs** — cache-miss counters, perf integration.
  Just criterion timing + the two logical counters.
- **The optimizations themselves** — node compaction, any-hit shadow rays, wide
  BVH, etc. are separate units (see RESEARCH.md); this unit only builds the ruler.
- **New mesh assets** — reuse the existing teapot / bunny / dragon under
  `assets/objs/`.

## Further Notes

- The research (`RESEARCH.md`) flags the first likely optimization — 32-byte node
  compaction — as *bit-identical*. The counter is exactly what proves that:
  counts constant, ns/ray down. So the harness earns its keep on the very first
  change.
- Per-orientation reporting is the deliberate answer to Sebastian Lague's
  demonstration that traversal cost is view-dependent for anisotropic meshes.
- Keep the counter increments confined to the one `closest_hit` function — the
  traversal was just unified in the thread-3 simplify pass; don't re-split it.
