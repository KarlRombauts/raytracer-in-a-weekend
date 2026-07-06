# Split the Scene document from the world build (architecture candidate 2)

Status: needs-triage

> Seed for a fresh thread. This was **candidate 2** in the architecture review
> (the `/improve-codebase-architecture` run). Recorded here because that review
> lived in a temp HTML file and won't survive into a new session.

## The friction

`src/scene.rs` is a god-module (~1366 lines, ~820 non-test) fusing at least three
cohesive responsibilities into one file:

1. **The Scene document / spec types** — `Asset`, `Mapping`, `TextureSpec`,
   `CellTexture`, `MaterialSpec`, `MeshData`, `Shape`, `Transform`, `ObjectSpec`,
   `Scene` — plain serializable data + their `.build()` methods that turn specs
   into runtime `Arc<dyn Texture/Material/Intersect>`.
2. **The world builder** — `build_world(&Scene) -> IntersectGroup`, plus
   `Placement`, `placed_quad`, `bake_area_light`, `BakedLight`, and the
   decorator-stack placement in `ObjectSpec::build`.
3. **Editor document ops** — `duplicate_object`, `placeable_bounds`, `pivot`.

Plus a layering violation: `MaterialOverride` (a runtime `Intersect`) and
`MeshData::build` (constructs a BVH) — render-engine machinery living in the
"plain data" document module.

## The deepening (proposed)

Extract a **`WorldBuilder`** whose entire interface is `build(&Scene) -> World`,
hiding placement / baking / decorators / light-collection behind it. Alongside:
a `scene::spec` module (the mirror types + `build()`), and `scene::edit` (the
editor ops). Move `MaterialOverride` next to the other `Intersect` decorators in
`ray/`, and relocate mesh runtime construction into the world builder — leaving
`MeshData` as pure `verts/faces/uvs`.

Deletion test: `build_world` is deep as a function but its implementation leaks
across five helpers in the same file; a real module seam concentrates that.

## Relationship to recent work

- The env-map MIS work (just completed) already moved **Sky into the World** and
  registered an environment `Light` — a down payment on **candidate 4** ("make the
  World a deep module"). Candidate 2 (this) and candidate 4 are adjacent: 2 is
  about the *builder* seam (Scene→World), 4 about the *World* runtime itself
  (top-level BVH, one light source of truth). **Decision: sequence, 2 then 4** —
  candidate 2 concentrates all Scene→World assembly into one construction site,
  which candidate 4's runtime changes then have a single place to work through.

### Candidate 4, sharpened: object-level material ownership

Candidate 4 is upgraded from "make the World deep" to a concrete goal:
**make the runtime World hold Blender-style objects that own their material.**

> **Progress (thread ordering: 1 material → 2 lights → 3 top-BVH):**
> - ✅ **Thread 1 — object-level material ownership.** Done, committed
>   (`GeoHit`/`HitRecord` split, runtime `Object`, `IntersectGroup`→`World` split,
>   `MaterialOverride`/placeholder deleted). Reviewed via `/simplify`.
> - ✅ **Thread 2 — one light source of truth.** Done, committed. An emissive
>   object owns its `light: Option<Arc<dyn AreaLight>>` handle (single
>   registration); the env light derives from `sky`; the `Light` enum is deleted.
>   Render pin stayed bit-identical. **Not yet `/simplify`-reviewed** (was on a
>   run) — worth a review pass.
> - ⏭️ **Thread 3 — top-level BVH** over the World's objects (currently a flat
>   linear loop in `World::intersect`). Independent perf change. Then the
>   near-term **multi-material import** candidate.

- Today material lives *on the primitive* (`Triangle`/`Sphere`/`Quad` each store
  `Arc<dyn Material>` — the RTOW convention). For meshes this misfires: N
  triangles all store the *same* material with no cheap way to swap it, so
  `MaterialOverride` (a decorator) + a throwaway placeholder material paper over
  it (`MeshData::build` bakes a gray Lambertian into every triangle, always
  overridden).
- The principled fix mirrors Blender/Cycles/PBRT: geometry (the BVH) is
  **material-agnostic**; material is bound at the **object** level (or a per-face
  material index) and resolved at the hit. A runtime `Object = geometry-handle +
  material (+ transform)` — which is exactly the document's `ObjectSpec { shape,
  material, transform }` mirrored into the runtime.
- This **deletes `MaterialOverride` and the placeholder** for real. It touches
  `Triangle` (constructible without a material), `HitRecord` (material resolved at
  the object boundary), the BVH, and the World. That's why it's candidate-4 work,
  not candidate 2.

**Constraint this places on candidate 2:** design the `WorldBuilder`'s output type
(`World`) anticipating that objects will soon own their material, so candidate 4
is a *deepening* of that type, not a reshape. In candidate 2, `MaterialOverride`
is only *relocated* (to `geometry/`, next to the other decorators), never
redesigned.

### Downstream candidate: camera as a placeable scene-graph object

Blender models the camera as an *object* — a transform + a camera data-block in
the scene graph — so the gizmo moves it, the outliner lists it, and you can have
several. Worth adopting, but at the **document/editor** layer, not the runtime
one: the camera is never intersected or shaded, so it does **not** belong in
candidate 4's runtime `Object` (geometry + material). This is a separate axis.

- **Gated on `render-settings-split`** (`.scratch/render-settings-split/`).
  `CameraConfig` currently fuses *lens/view* (position, look-at, fov, aperture —
  the placeable part) with *render settings* (image width, samples, background,
  sky — global config). Blender keeps render settings on the Scene and only
  lens/sensor on the Camera object. So: split those first, then the lens/view
  half can be promoted to a placeable entity alongside `objects`.
- Payoff: gizmo-movable camera, multiple cameras, an outliner row. All
  editor/document features — orthogonal to candidates 2 and 4.

### Downstream candidate: multi-material mesh import (OBJ `usemtl`/`mtllib`)

Follows the object-level material work (candidate 4, thread 1). Import an OBJ
with several materials on different face groups. Three layers:

- **Runtime** — already handled by thread 1's additive bias: `GeoHit` gains a
  `material_index`, the runtime `Object`'s single material becomes a slot `Vec`,
  `Triangle` gains a slot id. No teardown of thread-1 work.
- **Import/parse** — teach the OBJ loader `usemtl` (per-face material name) and
  `mtllib` (the sidecar `.mtl` file). Then a **translation layer**: MTL is a
  lossy Phong-ish lingua franca (`Kd`/`Ks`/`Ns`/`d`/`Ni`/`Ke`/`map_Kd`), so
  importing means heuristically mapping MTL fields onto our `MaterialSpec`
  (`Kd`→Diffuse, `Ke`→Emission, `d<1`/`Ni>1`→Glass, `Ns`+`Ks`→Glossy/Metal,
  `map_Kd`→Image). Approximate by nature — every engine's material spec differs.
  (glTF metallic-roughness is the modern PBR alternative if higher fidelity is
  ever wanted — different format.)
- **Document** — today `ObjectSpec` has one `material: MaterialSpec`. Multi-material
  needs several: either `ObjectSpec` goes plural or the material list moves onto
  `MeshData` with per-face slot indices. A document-model decision for this unit.
- Also adjacent: `.scratch/render-settings-split/` (splitting `CameraConfig`).

## How to start the new thread

Run `/improve-codebase-architecture` (it re-explores and re-grills), or go
straight to `/grilling` on "extract a WorldBuilder from scene.rs" using this seed
as the brief. Then `/to-prd` → `/tdd`, same as the integrator extraction and
env-map MIS.
