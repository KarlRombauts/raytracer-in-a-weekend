# PRD: Split the Scene document from the world build

Status: ready-for-agent

> Candidate 2 from the architecture review. Scope, structure, and deferrals were
> settled in a grilling session; see `SEED.md` in this directory for the origin
> story and the relationship to candidates 4 (deep World / object-level material)
> and the camera-as-object downstream idea.

## Problem Statement

`src/scene.rs` is a 1385-line god-module. Everything about a Scene lives in it,
all mixed together: the plain-data *descriptions* of textures, materials, shapes,
and objects; the code that *assembles* those descriptions into a runtime World;
and the *editor helpers* that mutate the document. Three unrelated
responsibilities share one file, so navigating it, reasoning about it, and
changing any one concern means wading through the other two.

Worse, the runtime layer has leaked *into* the document types in two places — a
runtime `Intersect` decorator (`MaterialOverride`) and a BVH-constructing method
(`MeshData::build`) sit among types that are supposed to be pure serializable
data. The boundary between "the Scene document" and "the runtime World" has been
erased inside this file.

This blocks the next piece of work. Candidate 4 (making the World a deep runtime
module with object-level material ownership) needs a single, clean place where a
Scene becomes a World. Today that assembly is smeared across `build_world` plus
five loose helpers, so any runtime change has to fight through the god-module.

## Solution

Replace the one big file with a focused `scene/` folder, split along the three
responsibilities, with the document types further separated by kind. Concentrate
the Scene→World assembly behind a single seam: one public `build_world` function
whose helpers become private to its module. Evict the two runtime leaks from the
document — relocate them beside their runtime siblings — **without changing their
logic**.

This is a pure reorganization. No runtime behavior changes: the same Scenes build
the same Worlds and render the same images. Every existing call site and every
existing test keeps working, because the module's public paths are preserved by
re-export.

The deeper fixes those two leaks are begging for (the object-level material model)
are deliberately **not** done here — they are runtime redesigns that belong to
candidate 4. This PRD only moves code so that candidate 4 has a clean seam to work
against.

## User Stories

1. As a developer, I want the Scene document types split from the world-build
   code, so that I can read and change one without wading through the other.
2. As a developer, I want texture descriptions in their own file, so that I can
   find and edit `TextureSpec`/`CellTexture` without scrolling past materials and
   meshes.
3. As a developer, I want material descriptions in their own file, so that the
   `MaterialSpec` surface is self-contained.
4. As a developer, I want shape and mesh descriptions in their own file, so that
   the `Shape`/`MeshData` types and their locked serde layout live together.
5. As a developer, I want object and transform descriptions in their own file, so
   that `ObjectSpec`/`Transform` — the thing the editor manipulates — is easy to
   locate.
6. As a developer, I want the world-build logic (`build_world` and its placement,
   baking, and light-collection helpers) in one file, so that the Scene→World
   assembly is a single unit I can reason about.
7. As a developer, I want the world-build helpers to be private to the build
   module, so that the assembly presents exactly one entry point ("give me a
   Scene, get a World") and nothing outside can reach into its internals.
8. As a developer, I want the editor document operations (`duplicate_object`,
   `placeable_bounds`) in their own file, so that document mutation is separated
   from document description.
9. As a developer, I want the runtime `MaterialOverride` decorator to live beside
   the other runtime `Intersect` decorators, so that the document module no longer
   holds a runtime object.
10. As a developer, I want the `MeshData` BVH-build to move with the shape types
    untouched, so that mesh construction still happens exactly when it does today
    and the scene file format is unaffected.
11. As a developer consuming the module, I want `crate::scene::Scene`,
    `crate::scene::ObjectSpec`, `crate::scene::build_world`, and the other current
    paths to keep resolving, so that none of the ~15 files that import from
    `scene` need to change.
12. As a developer, I want the world-build tests to move with the build code, so
    that the assembly's behavior is verified right next to it.
13. As a developer, I want the texture, material, mesh, and serde tests to move
    with their respective types, so that each file carries its own focused test
    module instead of a shared test soup.
14. As a maintainer, I want a Scene's serialized `.scene` bytes to be identical
    before and after this change, so that ADR-0001's locked wire format is
    preserved and existing saved scenes still load.
15. As a maintainer, I want a known Scene (Cornell box) to render to an identical
    image before and after this change, so that I have direct evidence the
    reorganization altered no runtime behavior.
16. As a developer about to start candidate 4, I want a single Scene→World
    construction site, so that introducing a deep World and object-level material
    ownership has one place to change instead of five scattered helpers.
17. As a developer, I want focused placement- and light-baking tests to be low
    friction to add, so that the build module's math can grow finer-grained
    coverage over time.

## Implementation Decisions

**Module reorganization.** `src/scene.rs` becomes a `src/scene/` directory
module. The single file is split into per-concern files. Document types are
separated by kind (per-type granularity), not lumped into one `spec` file:

- A **texture** module holds the texture descriptions (`Asset`, `Mapping`,
  `TextureSpec`, `CellTexture`) and their build/preview helpers.
- A **material** module holds `MaterialSpec`.
- A **shape** module holds `MeshData` (including its BVH-building method,
  relocated verbatim), `Shape`, the hand-written `Shape` serde, the intermediate
  serialization enum, and face-index validation.
- An **object** module holds `Transform` and `ObjectSpec` (its OBJ loaders,
  pivot, and build).
- A **build** module holds `build_world` plus the placement, baking, and
  light-collection helpers (`Placement`, `placed_quad`, `bake`,
  `bake_area_light`, and the baked-light pairing type).
- An **edit** module holds the editor document operations (`duplicate_object`,
  `placeable_bounds`).
- The module root holds the `Scene` type and the re-exports.

**The world build is a plain function, not a struct.** The "WorldBuilder" from
the seed is realized as the build *module*, not a stateless type. The public
surface is a single function taking a `Scene` and returning a World. The
improvement is encapsulation: the placement/baking/light-collection helpers,
currently loose at the top level of `scene.rs`, become private to the build
module. No empty builder struct is introduced.

**Dependency direction is preserved.** The build code depends on both the Scene
document types and the runtime World type; it therefore lives on the document
side (`scene/`). The runtime World module (`group`) must not gain any dependency
on the Scene document types. This keeps the World module clean for candidate 4.

**Runtime leaks are relocated, never redesigned.**
- `MaterialOverride` (a runtime `Intersect` decorator) moves out of the document
  entirely, into its own file under `geometry/`, beside the other `Intersect`
  decorators (`Translate`/`Rotate`/`Scale`), and is re-exported through the
  geometry module. Its logic is unchanged.
- `MeshData`'s BVH-building method moves with the shape types into the shape
  module, verbatim. It still runs at deserialize time and is still eagerly cached
  on the mesh shape, exactly as today. The placeholder material it bakes is left
  in place.

**Public API is preserved by re-export.** The module root re-exports the document
types (`Scene`, `ObjectSpec`, `Shape`, `MaterialSpec`, `TextureSpec`, `Transform`,
`Mapping`, `MeshData`, and siblings), `build_world`, and the editor operations, so
every existing `crate::scene::…` import continues to resolve unchanged. Consumer
files are not touched.

**Vocabulary.** The code and comments use the canonical glossary terms: **Scene**
for the editable document, **World** for the built runtime structure, **Object**,
**Shape**, **Primitive**, **Material**. The build function's signature —
Scene in, World out — already reads in canonical terms.

## Testing Decisions

**What makes a good test here.** This is a reorganization with no behavior change,
so the goal is to prove behavior is *unchanged*, not to specify new behavior. A
good test asserts an externally observable fact about the Scene→World pipeline (a
World's light count, an emitter's analytic pdf, a serialized round-trip, a
rendered image) rather than the internal arrangement of files. Tests are not
rewritten; they move with the code they exercise and must stay green.

**Seams — zero new ones.** All tests sit at existing boundaries:

- **Primary seam — `build_world(&Scene) → World`.** The highest seam, exercising
  the whole assembly. Existing coverage that moves to the build module: hidden
  objects are excluded from World and lights; the Cornell box collects exactly one
  light with the expected emission; quad and sphere emitters both register; a
  transformed quad emitter still registers with the correct analytic pdf; a
  non-uniformly-scaled sphere falls back to BSDF-only; a baked quad intersects
  identically to the old decorator stack (the safety-net equivalence test).
- **Supporting lower seams (existing).** The spec `build()`/`preview_color`
  methods (texture and mapping tests move to the texture module) and the postcard
  serde round-trips for `Scene`, `MaterialSpec`, `TextureSpec`, and meshes (move
  to the shape/material modules). The serde tests double as the guard for
  ADR-0001's locked wire format.
- **Editor operations.** The `duplicate_object` test moves to the edit module.

**Prior art.** The existing `#[cfg(test)]` modules in `scene.rs`
(`visibility_tests`, `light_tests`, `registration_tests`, `bake_equivalence_tests`,
`mesh_serde_tests`, `texture_spec_tests`, `mapping_tests`, `serde_tests`) are the
template — each moves next to the code it covers.

**One net-new verification.** A golden/snapshot check: render the Cornell box
before and after the change and confirm the produced image is identical. This is
a technique the repo does not currently use; it directly demonstrates that the
reorganization changed no runtime behavior, and can remain as a regression guard.

## Out of Scope

Deferred to **candidate 4** (deep World / object-level material ownership):

- Adopting the Blender/Cycles/PBRT model where geometry (the BVH) is
  material-agnostic and material is bound at the Object level (or via a per-face
  material index) and resolved at the hit.
- **Deleting** `MaterialOverride` and the throwaway placeholder material that
  `MeshData`'s build bakes into every triangle.
- **Purifying** `MeshData` to bare `verts`/`faces`/`uvs` — this requires reworking
  when and where meshes are built (moving construction off the deserialize path),
  which is a behavior change entangled with the material-model fix.

Deferred further downstream:

- Making the camera a placeable scene-graph Object (gizmo-movable, multiple
  cameras). This is a document/editor concern, gated on the render-settings split
  (separating lens/view from render settings in the camera config).

Also out of scope now:

- Any change to the `.scene` binary format or the `Shape`/`MeshData` serde.
- Making the build helpers testable from outside their module — they stay private;
  in-module tests are their home.
- Introducing a `WorldBuilder` struct or any stateful builder type.

## Further Notes

- **Design constraint carried forward:** the build function's output type (the
  World) should be shaped anticipating that Objects will soon own their material,
  so candidate 4 is a *deepening* of that type rather than a reshape.
- **Testability, honestly stated:** the split does not unlock fundamentally new
  external-behavior tests — the `build_world` seam was always testable and the
  build helpers were always reachable by in-module tests. What it buys is *lower
  friction and focus*: placement and light-baking gain their own small test module
  next to the code, so finer-grained tests become the natural thing to write.
  Genuinely new runtime seams (e.g. an Object resolving its material at a hit
  without assembling a Scene) arrive with candidate 4; candidate 2 is what makes
  that seam clean to introduce.
- **Suggested commit shape:** the reorganization is safest as a sequence of small
  moves — extract each module, keep `cargo test` green after each — rather than
  one large cut. The re-export root means consumers never break mid-sequence.
