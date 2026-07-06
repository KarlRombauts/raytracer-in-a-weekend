# PRD: Top-level BVH over the World's objects (candidate 4, thread 3)

Status: ready-for-agent

> Thread 3 of candidate 4. Threads 1 (object-level material) and 2 (one light
> source of truth) are done. Design settled in a grilling session; see
> `.scratch/scene-worldbuilder-split/SEED.md` for thread context.

## Problem Statement

`World::intersect` tests every ray against **every object** in a flat linear
loop. That is O(objects) per ray, per bounce — fine for a Cornell box with a
handful of walls, but it makes the path tracer scale poorly as scenes grow: a
scene with hundreds of objects pays for hundreds of bounding-box tests on every
one of millions of rays. The World is the one runtime structure with no spatial
acceleration; meshes already have their own BVHs, but the top level does not.

## Solution

Give the World a **top-level BVH** over its objects — the outer half of a
two-level (TLAS/BLAS) acceleration structure, where the inner half is each mesh's
existing per-triangle BVH, reused by reference. A ray now descends a spatial tree
of objects instead of scanning a list, and only visits objects whose bounds it
could plausibly hit. The mesh BVHs are untouched and shared, so rebuilding the
World on an edit rebuilds only the small, cheap top-level tree.

No visible change: the same rays find the same closest surfaces and the same
materials, so the rendered image is identical. This is a performance change,
guarded by an equivalence check against the brute-force scan and by the existing
render pin.

## User Stories

1. As a developer rendering a scene with many objects, I want ray/object
   intersection accelerated by a spatial tree, so that render time scales with
   the log of the object count, not linearly.
2. As a developer, I want the World to hold a top-level BVH over its objects, so
   that `World::intersect` traverses a tree instead of scanning every object.
3. As a developer, I want each mesh's existing inner BVH reused by reference, so
   that the top-level BVH is the only new structure and no per-triangle work is
   duplicated.
4. As a developer editing a scene, I want an edit to rebuild only the cheap
   top-level BVH (over ~dozens of objects), not the expensive mesh BVHs, so that
   interactive re-renders stay responsive.
5. As a developer, I want the World to still attach the hit object's material at
   one place, so that thread 1's material seam is preserved.
6. As a developer, I want the top-level BVH to report *which object* was hit, so
   that the World can resolve that object's material (the existing BVH returns a
   bare geometry hit with no identity).
7. As a developer, I want the objects kept in scene order as the source of truth
   for materials and lights, so that light sampling order is unchanged and the
   render stays bit-identical.
8. As a developer, I want the BVH to reorder only a lightweight object proxy, not
   the objects themselves, so that reordering for spatial locality never disturbs
   scene order.
9. As a developer, I want the existing SAH BVH machinery reused, so that the
   top-level tree is built with the same proven, parallel, cache-friendly
   construction as the mesh BVHs.
10. As a developer, I want the World built from a complete set of objects in one
    pass, so that the BVH — which cannot be cheaply appended to — is constructed
    once at build time.
11. As a developer, I want the integrators unchanged, so that shading and
    next-event estimation are unaffected by how intersection is accelerated.
12. As a maintainer, I want the produced image to be identical before and after,
    so that I have direct evidence acceleration changed nothing about the result.
13. As a maintainer, I want a direct check that the BVH finds the same closest
    hit as a brute-force scan, so that correctness is proven independently of the
    render pin.
14. As a developer starting the multi-material import work later, I want the
    top-level BVH to report object identity (not a baked material), so that a
    per-face material index can be added additively without touching the BVH.
15. As a maintainer, I want no change to the `.scene` binary format, so that
    saved scenes still load (this is a runtime-only change).

## Implementation Decisions

**Two-level acceleration (TLAS/BLAS).** The top-level BVH's leaves are the
World's objects; each mesh object's geometry remains its own inner triangle BVH,
referenced (not copied). Primitives (sphere/quad/box) sit as leaves directly.
This is the GPU-ray-tracing lineage (shared bottom-level geometry, identity
resolved at the top level), chosen deliberately over a single flat
all-primitives BVH because the latter would undo thread 1's object-level material
model and cheap shared geometry.

**Object identity from the BVH.** The existing generic SAH BVH returns only a
material-agnostic geometry hit, with no indication of which primitive produced
it. The top-level BVH must additionally return *which object* was hit, so the
World can attach that object's material. This is provided by a new BVH method
that returns the winning primitive alongside the hit, rather than the bare hit.

**A lightweight object proxy carrying a stable index.** The BVH reorders its
primitives in place during construction. To keep the objects in scene order
(needed so light-sampling order — and thus the render — is unchanged), the BVH is
built over a small proxy that pairs an object's geometry handle with that
object's stable index. The proxy is intersectable (delegating to the geometry).
The BVH reorders proxies freely; the object array never moves. On a hit, the
proxy's index resolves the object, whose material is attached.

**Material attachment stays a single seam.** `World::intersect` becomes: ask the
top-level BVH for the closest (hit, winning proxy); map that to a shading record
by attaching the resolved object's material. This is the same one-line
attachment point thread 1 established — the exact place a future per-face
material-slot lookup will change.

**Immutable World construction.** Because a BVH cannot be cheaply appended to,
the World is built from a complete object set in one pass: a constructor takes
the full object list plus the sky and builds the proxies, the BVH, the light
index set, and the bounds together. The incremental add-one-object API is
removed; the world builder collects its objects into a list and constructs the
World once. This also makes a built World immutable.

**Lights and sky unchanged.** Light objects are still tracked by scene-order
index (thread 2), and the environment light is still derived from the sky. Light
sampling order and count are preserved exactly, which is what keeps the render
bit-identical.

**Reuse, don't reinvent.** The top-level BVH uses the existing binned-SAH,
parallel, flattened BVH construction — the same machinery the mesh BVHs use. Only
the winner-returning query method is new. Per-BVH build quality (SAH) is
unchanged; only the two-level topology is added.

**Single-material now; multi-material additive later.** Each object still resolves
to one material. The design leaves the door open: object identity (top BVH) and a
future per-face material index (carried in the geometry hit from the mesh
triangle) are orthogonal, so multi-material slots drop in without changing the
top-level BVH. No speculative index is added in this unit.

## Testing Decisions

**What makes a good test here.** This is a behavior-preserving acceleration
change, so value lies in proving (a) the accelerated intersection returns the
*same closest hit* as the brute-force scan, and (b) nothing about the produced
image moved. Tests assert externally observable facts — which surface/material a
ray resolves to, a rendered image — never the tree's internal node layout.

**Primary new seam — BVH-vs-linear equivalence.** Build a World with several
objects at varied positions; fire a few hundred pseudo-random rays; for each,
assert that `World::intersect` (BVH-accelerated) agrees with a brute-force linear
scan over the World's objects on the closest hit — same distance and same
resolved material. The test reconstructs the linear scan itself from the public
object list, so no production linear path is retained. This proves the BVH's core
property independently of the render pin, and is the load-bearing correctness
guard.

**End-to-end pin (existing).** The Cornell-box render fingerprint must stay
bit-identical. Cornell has no coplanar ties (the light sits just below the
ceiling, walls meet only at edges, inner boxes don't overlap), so the BVH finds
the same closest object as the linear loop for every ray and the image is
expected to be unchanged. A break would signal an unexpected exact-distance tie
to investigate.

**Existing seams that must keep passing (no new tests).** The `build_world`
assembly and light-registration tests (hidden-object exclusion, emitter
registration, the analytic-pdf check, one-source-of-truth), geometry
intersection, and the postcard serde round-trips (unchanged — the document is
untouched).

**Prior art.** The equivalence test follows the style of the existing
`bake_equivalence` test (fire a battery of rays, compare two intersection paths).
The mesh BVH's own sample tests exercise the same `BVH<T>` machinery being reused.
The render pin already exists.

## Out of Scope

- **Multi-material import** (OBJ `usemtl` + per-face material slots): a later
  unit. This unit stays single-material; it only leaves the door open.
- **Any change to the mesh / inner BVH** — it is reused as-is.
- **Rebuild/refit optimizations** (incremental BVH updates on edit): the World is
  rebuilt wholesale by `build_world`, which is already how edits work; the
  top-level tree is cheap to rebuild.
- **Any change to the `.scene` binary format or document types.** Runtime-only.
- **Instancing** (multiple objects sharing one geometry with different transforms)
  as an authored feature — the structure supports it, but exposing it is separate.

## Further Notes

- **Why not a single flat BVH over all primitives (the PBRT default):** it would
  put material back on the leaf (undoing thread 1), lose cheap shared-geometry-
  with-different-material, and force a full rebuild of a giant tree on every edit.
  Two-level fits an interactive editor; the single-flat model fits batch
  rendering. This is a deliberate divergence toward the GPU-RT (TLAS/BLAS)
  lineage.
- **The render pin is expected to hold** precisely because light order is
  preserved and Cornell has no ties; the equivalence test is what makes
  correctness robust even if a future scene *does* have a benign tie.
- **This modifies thread-1/2 World code** (construction and `intersect`) — expected;
  the World is where this thread lives, and its material/light seams are preserved.
