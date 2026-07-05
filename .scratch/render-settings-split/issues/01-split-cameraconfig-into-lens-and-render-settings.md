# Split `CameraConfig` into lens params + `RenderSettings`

Status: ready-for-human

## Problem

`CameraConfig` (`src/camera/config.rs`) is two responsibilities wearing one hat:

- **Lens / orientation** (the actual camera): `look_from`, `look_at`, `v_up`, `fov`,
  `roll`, `dof_angle`, `focus_dist`, `aspect_ratio`.
- **Render settings** (how to render — nothing to do with the lens): `samples`,
  `max_depth`, `firefly_clamp`, `sky`, `integrator`.

The interface is a grab-bag: callers that only care about the lens still see the
render knobs, and vice-versa. The `sky` and `integrator` fields are already
`#[serde(skip)]` runtime-only settings riding in what is nominally scene/camera
data — a sign the render-settings cluster wants its own home.

## Proposed change

Extract a `RenderSettings` struct holding `{ samples, max_depth, firefly_clamp,
sky, integrator }`, leaving `CameraConfig` as the lens/orientation only. `Scene`
then carries both (e.g. `camera: CameraConfig` + `render: RenderSettings`), or a
`RenderConfig { camera, render }` wrapper.

The five render fields already move as a unit — the integrator extraction
(`docs/superpowers/plans/2026-07-05-integrator-extraction.md`) deliberately
grouped `integrator` next to `samples`/`sky` so this split is later a clean
"lift these five out together."

## Migration concerns (why this is `ready-for-human`, not `ready-for-agent`)

- **Wire format.** `CameraConfig` is the serialized `.scene` payload (postcard,
  append-only — see ADR-0001). Splitting the struct changes the layout, so it
  needs a version bump or the `SceneExt`-style trailing-blob treatment. This
  overlaps candidate 7 (the wire-format seam) in the architecture review.
- **Call sites.** Every `CameraConfig::builder()…build()` in `src/scenes/` and the
  tests, plus `Camera::from`, would re-split their field access.
- **Inspector.** The Camera and Output tabs (`src/viewer/panels/inspector/`)
  already roughly mirror this split (Camera tab = lens, Output tab = render
  settings) — good news, the UI seam is half-drawn.

## Origin

Surfaced during the integrator-extraction grilling (2026-07-05): deciding where
`IntegratorKind` lives exposed that `CameraConfig` had already crossed the
what/how line. Deferred so the integrator work stays in scope.
