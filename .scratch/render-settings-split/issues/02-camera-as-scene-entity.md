# Model the camera as a scene entity, not a privileged field

Status: needs-triage

> Bigger, exploratory framing of the same problem area as
> [01-split-cameraconfig-into-lens-and-render-settings.md](01-split-cameraconfig-into-lens-and-render-settings.md).
> If this is pursued, 01 is largely subsumed by it. Needs its own design pass
> (grilling) before planning — recorded here so it isn't lost.

## Idea

Today `Scene { camera: CameraConfig, objects: Vec<ObjectSpec> }` — the camera is a
privileged top-level field. Instead, make the camera **a scene entity like any
other**, so it appears in the Outliner, is selectable, and is driven by the
Gizmo in the viewport (the Blender-ish model the product framing already implies).

This is a **scene-entity generalization**: `Object` stops meaning "a hittable"
and becomes one *kind* of entity, alongside `Camera` (and, eventually, `Light`
as a first-class entity rather than a material side-effect).

## Payoff

- Camera placement (`look_from`/`look_at`/`v_up`/`roll`) *is* a `Transform` —
  reuse the existing transform + gizmo + duplicate machinery for free.
- Camera becomes selectable/nameable/hideable in the Outliner.
- Multiple cameras + an active-camera selection fall out of a list.

## Tension (why this needs a real design pass)

- A camera has **no geometry and no material**, so it doesn't fit `ObjectSpec`
  (`shape + material + transform`). Either add a non-hittable variant (muddies
  "Shape = Sphere/Quad/Box/Mesh") or widen the model to
  `Entity { Geometry | Camera | Light }`.
- Every consumer of `objects` (`build_world`, `placeable_bounds`, BVH build)
  must then **skip non-hittables** — complexity spreading to N call sites unless
  the entity split is done carefully (e.g. keep a typed `cameras()` / `geometry()`
  view rather than one untyped `Vec`).
- The camera still needs a **param bag** (fov, dof, focus) beyond `Transform`,
  and a decision on whether **render settings** (samples, sky, integrator) are
  scene-global or per-camera.
- Reworks the `.scene` wire format (overlaps ADR-0001 / candidate 7).

## Relationship to other work

- Subsumes / redirects issue 01 (the `CameraConfig` split).
- Interacts with architecture-review candidate 2 (Scene→World builder) and
  candidate 4 (a real `World`): "which entities are hittable" is exactly what
  `build_world` would consume.
- Does **not** block the integrator extraction — `integrator` lives with the
  render settings today and migrates with them whenever the camera model changes.

## Origin

Raised during the integrator-extraction grilling (2026-07-05), one step past the
`CameraConfig` split: "the camera should probably just be a scene object like
everything else."
