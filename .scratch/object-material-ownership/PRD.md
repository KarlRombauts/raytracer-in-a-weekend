# PRD: Object-level material ownership (candidate 4, thread 1)

Status: ready-for-agent

> Thread 1 of candidate 4 ("make the World a deep module"). Scope, mechanism, and
> staging were settled in a grilling session. See
> `.scratch/scene-worldbuilder-split/SEED.md` ("Candidate 4, sharpened") for the
> origin and the ordering of the follow-on threads.

## Problem Statement

In this renderer, material lives *on the primitive*: every `Sphere`, `Quad`, and
`Triangle` owns its own `Arc<dyn Material>` and stamps it onto each hit. That is
fine for a single primitive with one material, but it misfires for a mesh, where
thousands of triangles all share *one* material and there is no cheap way to swap
it. The workaround is `MaterialOverride` — a decorator that ignores what each
triangle stored and re-stamps a uniform material at hit time — plus a throwaway
placeholder material baked into every triangle of a mesh's BVH (a gray
`Lambertian` that is always overridden).

This is a leak of the runtime's material model into places it does not belong: a
mesh's spatial structure is material-independent, yet the material is fused into
its leaves. The result is a placeholder that means nothing, a per-hit re-stamp
that papers over the wrong answer stored below it, and a `World` type
(`IntersectGroup`) that is overloaded as *both* the top-level runtime container
*and* a generic list of hittables used inside objects.

## Solution

Adopt the model every production renderer uses (Blender, Cycles, PBRT): geometry
is material-agnostic, and material binds at the **Object** level, resolved at the
hit. A material-agnostic geometry hit carries only surface data; the runtime
**World** attaches the winning **Object**'s material at the closest hit, producing
the `HitRecord` the integrators already consume. `MaterialOverride` and the
placeholder material are deleted outright — the runtime finally mirrors the
document's clean `{shape, material, transform}` split.

Because the geometry-hit type must lose its material, the overloaded
`IntersectGroup` splits into two honest types: a **geometry group** (a list of
hittables, still material-agnostic) and a deep **World** (owns Objects, lights,
and sky). This split is the essence of "make the World a deep module."

No visible change: the same Scenes render the same images to the bit. This moves
*where* material is attached, not *what* any surface looks like.

## User Stories

1. As a developer, I want geometry primitives to carry no material, so that a
   primitive's spatial structure is independent of its appearance.
2. As a developer, I want a material-agnostic geometry hit type, so that geometry
   can report a surface hit without inventing a material it does not own.
3. As a developer, I want a runtime Object that pairs geometry with its material,
   so that appearance is bound at the object level, mirroring the Scene document.
4. As a developer, I want the World to attach the correct Object's material at the
   closest hit, so that the integrators receive a fully-resolved surface hit
   unchanged.
5. As a developer, I want `MaterialOverride` deleted, so that the mesh
   material-swap hack no longer exists.
6. As a developer, I want the placeholder material removed from mesh building, so
   that no meaningless gray material is baked into mesh leaves.
7. As a developer, I want a mesh's geometry to be shared across objects with
   different materials, so that reusing a mesh does not duplicate its acceleration
   structure.
8. As a developer, I want changing an object's material to rebuild no geometry, so
   that material edits stay cheap (the reason `MaterialOverride` existed, now
   structural).
9. As a developer, I want the overloaded `IntersectGroup` split into a geometry
   group and a `World`, so that the top-level runtime container is a distinct,
   deep type and not also a nested-geometry building block.
10. As a developer, I want lights and sky to live on the `World`, not on the
    geometry group, so that a generic list of hittables carries no scene-global
    state.
11. As a developer, I want the geometry group to remain a plain `Intersect` used
    by `make_box`, mesh building, and BVH construction, so that nested geometry
    keeps working unchanged apart from its hit type.
12. As a developer, I want the integrators to take a `World` and receive a
    material-bearing `HitRecord`, so that shading and next-event estimation are
    untouched by this change.
13. As a developer, I want emissive objects to still glow when hit directly, so
    that an object with an Emission material illuminates as before.
14. As a developer, I want emissive objects to still register as sampleable area
    lights, so that next-event estimation is unaffected (light *unification* is a
    later thread).
15. As a developer rendering the reference Cornell box, I want a bit-identical
    image before and after, so that I have direct evidence no shading changed.
16. As a maintainer, I want the `.scene` binary format untouched, so that existing
    saved scenes still load (this is a runtime change; the document types are not
    modified).
17. As a developer starting the multi-material thread next, I want the Object and
    geometry-hit shapes chosen so that per-face material slots drop in additively,
    so that multi-material meshes are an extension, not a rewrite.
18. As a developer, I want the change staged in green checkpoints, so that a
    failure at any point is localized rather than buried in one large sweep.

## Implementation Decisions

**Two hit types (the core mechanism).** Geometry reports a material-agnostic hit
(surface point, normal, front-face, texture coordinates, distance) with no
material and no borrow. The World combines the closest such hit with the winning
Object's material to produce the existing material-bearing `HitRecord` that
integrators consume. The `Intersect` trait's return type changes from the
material-bearing record to the material-agnostic one; integrators are unchanged
because they still receive the material-bearing record — now constructed one level
up, in the World, rather than in the primitive.

**Runtime Object.** A new runtime type pairs a material-agnostic geometry handle
with a single material. The transform is already baked into the geometry handle
(as today), so it is not a separate field. The Object is a distinct World-level
concept: it is **not** an `Intersect` (it yields a material-bearing hit, while
`Intersect` now yields a material-agnostic one).

**Type split of `IntersectGroup` (forced by the mechanism).** `IntersectGroup`
today serves two incompatible roles — the top-level World and a reusable geometry
group nested inside objects (`make_box`, mesh building, BVH-from-group). Since a
nested group must stay material-agnostic while the World must attach material,
they can no longer be one type:
- `IntersectGroup` remains the **geometry group**: a list of `Arc<dyn Intersect>`,
  still an `Intersect`, now returning the material-agnostic hit. Lights and sky are
  removed from it.
- A new **`World`** type owns the Objects, the lights, and the sky, returns the
  material-bearing `HitRecord`, and is not an `Intersect`. `build_world` returns a
  `World`; integrators take a `World`.

**Material resolution in the World.** The World's intersection finds the closest
material-agnostic hit across its Objects, remembers the winning Object, and
attaches that Object's material to produce the `HitRecord`. This is a single,
localized attach point — the one line that a later thread changes to index a
material-slot table.

**Primitives shed material.** `Sphere`, `Quad`, and `Triangle` lose their material
field and material constructor parameter. All `Intersect` implementations
(primitives, the transform decorators, the BVH variants, the geometry group)
return the material-agnostic hit. `make_box`, the mesh builder, quad-baking, and
`ObjectSpec::build` stop threading a material through geometry construction;
`build_world` pairs each built geometry with its material into an Object.

**Lights (behavior unchanged).** An emissive object becomes an Object with its
Emission material (so it glows on a direct hit) and is *also* registered as a
sampleable area light with its emission color (as today — the emission color is
already stored separately from geometry). The quad/sphere light-baking helper no
longer needs a material parameter. Light *unification* (not registering emissive
objects twice) is explicitly a later thread.

**Single-material now; slots later, additively.** Each Object binds exactly one
material. The Object is a named struct and material attachment is one localized
point, so the near-term multi-material thread can turn "the material" into "a slot
table + a per-face index" without unwinding anything here. No speculative material
index is added to the geometry hit in this unit.

**Staging — three green checkpoints.**
1. Split `World` out of `IntersectGroup`; move lights and sky onto `World`;
   integrators take a `World`. No material change; the geometry group still owns
   material. (compiles, tests + render pin green)
2. Introduce the material-agnostic hit type and the Object; flip the `Intersect`
   trait; shed material from primitives; the World attaches material; delete
   `MaterialOverride` and the placeholder. (compiles, tests + render pin green)
3. Cleanup. (green)

## Testing Decisions

**What makes a good test here.** The change is behavior-preserving for shading, so
most value is in proving that (a) the one new behavior — the World resolving the
correct Object's material at a hit — is correct, and (b) nothing else moved. Tests
assert externally observable facts (which material a hit resolves to, a rendered
image, a light count, an analytic pdf), never the internal arrangement of types.
Existing tests are not rewritten; they keep passing, adjusted only where a
constructor signature changed.

**New seam — material resolution at the World.** Build a World with two objects of
different materials; fire a ray at each and assert the returned hit carries that
object's material. Include the closest-hit-wins case: two overlapping objects, the
nearer object's material is the one resolved. This is the highest point at which
object-level material ownership is observable.

**New characterization seam — geometry is material-agnostic and shared.** Two
Objects built over the same mesh geometry handle with *different* materials both
resolve their own material, and the shared geometry is not duplicated (handle
identity preserved). This pins the win that made `MaterialOverride` and the
placeholder unnecessary.

**Existing seams that must keep passing (no new tests).** `build_world` assembly
(hidden-object exclusion, quad/sphere/transformed-emitter registration, ellipsoid
fallback, Cornell-box light collection, the analytic-pdf check); geometry
intersection equivalence (the baked-quad-vs-decorator test, now asserting on the
material-agnostic hit's fields); and the postcard serde round-trips (unchanged —
they prove the document types are untouched).

**End-to-end pin (existing).** The Cornell box render fingerprint must stay
bit-identical, demonstrating no shading changed.

**Prior art.** The material-resolution and shared-geometry tests follow the style
of the existing `registration_tests` and `bake_equivalence_tests` (construct
spec/geometry, assemble, assert on the result). The render pin already exists at
`tests/render_characterization.rs`.

## Out of Scope

Deferred to later threads of candidate 4, in order:
- **Thread 2 — one light source of truth:** stop registering an emissive object
  twice (as geometry and as a separate baked area light); derive lights from
  objects-that-emit. This unit keeps the current two-registration behavior.
- **Thread 3 — top-level BVH:** replace the World's flat linear object loop with a
  BVH over Objects. This unit keeps the flat loop.

Deferred to the near-term multi-material import unit:
- Per-face material slots: a material index on the geometry hit, a material-slot
  `Vec` on the Object, a slot id on mesh triangles.
- OBJ `usemtl`/`mtllib` parsing and the MTL→`MaterialSpec` translation layer.
- The document-model reshape for multiple materials per mesh object.

Also out of scope:
- Any change to the `.scene` binary format or the document spec types
  (`MaterialSpec`, `Shape`, `MeshData`, `Scene`). This is a runtime-only change.
- Multi-material meshes of any kind in this unit (single material per object).

## Further Notes

- **Why the type split is not scope creep:** it is forced by the mechanism. Once
  the geometry hit loses its material, `IntersectGroup` cannot be both a
  material-agnostic nested group and a material-attaching World. Splitting the two
  roles is the minimal correct response, and it is exactly the "deep World" goal.
- **Naming follows the codebase's spec→runtime convention:** `MaterialSpec`→
  `Material`, `TextureSpec`→`Texture`, and now `ObjectSpec`→`Object`. `World`
  matches the glossary headword for the built runtime structure.
- **The render pin is the strongest guard.** Because this change is defined as
  attachment-site-only (not color-changing), a bit-identical Cornell box is
  near-conclusive evidence of correctness across the whole pipeline.
- **This modifies the candidate-2 `build_world`/`ObjectSpec::build` code** — that
  is expected: candidate 2 concentrated the Scene→World assembly into one seam
  precisely so this thread has a single construction site to change.
