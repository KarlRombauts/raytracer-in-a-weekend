# Resume breadcrumb: thread 3 — top-level BVH over the World

Status: done

**Done and committed** (`8492219` feat + `b084187` /simplify). Two-level TLAS/BLAS
BVH over objects: `BVH::closest_hit` winner-returning query, `ObjRef` proxy
(stable scene-order index), immutable `World::new(objects, sky)`, BVH-backed
`World::intersect`. Guarded by a new BVH-vs-linear equivalence test and the
unchanged render pin `0x9436e82cbff110f1`. The /simplify pass folded the
duplicated traversal so `closest_hit` is the BVH's single descent.

All three threads of candidate 4 (object-level material, one light source of
truth, top-level BVH) are now done. Next up (per
`.scratch/scene-worldbuilder-split/SEED.md`): the multi-material import
candidate, then the camera-as-object / render-settings-split editor track.

---

_Original handoff (kept for reference):_

Handoff from a context-limited session. Threads 1 & 2 of candidate 4 are **done
and committed**; thread 3 is **designed (PRD written) but not implemented**.

## The one thing to do

Run **`/tdd`** against `.scratch/world-top-bvh/PRD.md`. That PRD is the durable
spec — read it first; everything below is orientation, not a substitute.

Suggested open line for the fresh thread:

> Read `.scratch/world-top-bvh/PRD.md` and let's implement thread 3 — the
> top-level BVH — test-first, keeping the render pin bit-identical.

## Where things stand

- Branch: `renderer-realism`. Tree clean, all committed.
- **192–193 tests + the render pin all green.** Baseline before starting:
  `cargo test`. The render pin lives at `tests/render_characterization.rs`
  (`cornell_box` fingerprint **`0x9436e82cbff110f1`**) — it is the behaviour
  guard for every thread.
- Recent commits (newest first): thread-2 seed update, `ace7284` thread 2 (one
  light source of truth), `7fe6214` thread-1 simplify, `4aa018c` thread 1,
  `f65f4b1` thread-1 checkpoint, `b7428fb` thread-1 PRD.

## Design decisions already locked (do not re-litigate — grill happened)

1. **Two-level (TLAS/BLAS).** Top-level BVH over objects; mesh objects keep their
   existing inner `BVH<Triangle>`, reused by reference. NOT a single flat
   all-primitives BVH (that would undo thread 1's object-level material model).
2. **Stable objects + proxy.** `World.objects: Vec<Object>` stays in **scene
   order** (source of truth for materials AND lights → light-sampling order
   preserved → render stays bit-identical). Build `BVH<ObjRef>` where
   `ObjRef { geometry: Arc<dyn Intersect>, object: usize }` impls `Intersect`
   (delegates to `geometry`) and carries the stable object index. The BVH reorders
   `ObjRef`s; `objects` never moves.
3. **BVH returns the winner.** Add a method to the existing SAH `BVH<T>`
   (`src/ray/bvh/flat_bvh.rs`) returning `Option<(GeoHit, &T)>` (the winning
   primitive), not just the bare `GeoHit`. Keep the existing bare-`GeoHit`
   `Intersect` impl for the mesh use.
4. **`World::intersect`** = `bvh.closest_hit(ray, rt).map(|(geo, r)|
   HitRecord::from_geo(geo, self.objects[r.object].material.as_ref()))` — same
   one-line material-attach seam as thread 1.
5. **Immutable construction.** `World::new(objects: Vec<Object>, sky: Sky)` builds
   proxies + BVH + light indices + bbox in one pass. **Drop incremental `add()`.**
   `build_world` collects a `Vec<Object>` then calls it. Churn: every test world
   (`mis.rs`, `naive.rs`, `world.rs`, `render.rs`) switches from `new()+add()...`
   to `World::new(vec![...], sky)` — mechanical, yields cleaner tests.
6. **Lights/sky unchanged** (thread 2): light objects by scene-order index; env
   light derived from `sky`.
7. **Single-material now.** Do NOT add `GeoHit.material_index` (YAGNI; additive
   later for multi-material). Object identity (top BVH) + future per-face index
   (inner mesh) are orthogonal.

## Verification

- **Primary guard (new test):** BVH-vs-linear equivalence — build a multi-object
  World, fire a few hundred pseudo-random rays, assert `World::intersect` (BVH)
  agrees with a hand-rolled linear scan over `world.objects` on the closest hit
  (same `t`, same resolved material). Reconstruct the linear scan in the test from
  the public `objects` — keep NO production linear path. Prior art:
  `bake_equivalence` test in `src/scene/object.rs`.
- **Render pin:** expected to stay bit-identical (Cornell has no coplanar ties —
  light at y=3.99, ceiling at y=4.0). If it breaks, investigate an exact-distance
  tie; re-pin only if genuinely benign.

## Gotchas / verify during implementation

- Confirm nothing outside `build_world` mutates a World incrementally (viewer
  rebuilds via `build_world`; check `render_task.rs`). Ray-picking (`pick.rs`) —
  see if it goes through `World::intersect` (would get BVH-accelerated for free).
- `flat_bvh.rs` `BVH<T>` reorders primitives in place and returns `Option<GeoHit>`
  from `intersectBVH` (leaf loop already knows the winning `prim` ref — the new
  method just tracks it alongside `closest_hit`).
- Keep `Object`'s material-attach seam localized to the one `World::intersect`
  line (altitude reviewers flagged this as the future multi-material slot site).

## Rhythm

`/tdd` now. After green + committed, a `/simplify` pass is worth running (threads
1 & 2 each had one; thread 2's is still pending too — noted in the candidate-4
seed at `.scratch/scene-worldbuilder-split/SEED.md`).
