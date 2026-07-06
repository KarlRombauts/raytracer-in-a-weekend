# Notes

## Workspace location
The teaching workspace lives in `teach/` (a subfolder), not the repo root, to
avoid cluttering the raytracer project. If Karl wants it versioned separately or
gitignored, adjust here. Lessons open with `open teach/lessons/<file>.html`.

## Teaching preferences
- Ground every lesson in Karl's actual code (`src/texture/env_map.rs`,
  `src/integrator/*`). He learns the abstraction faster when it's pinned to a
  line he can open. Keep doing this.
- Lessons stay short — one tangible win. Defer heavy math (pdf Jacobians) to its
  own lesson rather than front-loading.

## Course order (reordered twice as Karl's questions surfaced his ZPD)
- 01 — env map as a grid (texels, rows). ✓
- 02 — inverse transform sampling (how you sample from a pdf at all). ✓
- 03 — the Jacobian as a stretch factor; densities ÷ stretch. ✓
  ↳ Added because Karl said "I don't know what a Jacobian is" mid-lesson.
- 04 — chosen texel → world direction + solid-angle pdf; the sinθ cancellation. NEXT
- 05 — Distribution2D in Rust against his EnvMap.data (marginal + conditionals).
- 06+ — MIS wiring, and the design fork (sky-local NEE vs fold Sky into the
  World / architecture-review candidate 4).

Two teaching signals confirmed:
- He learns from the *mechanism* ("how would I build it") — lean into codeable recipes.
- Calculus is light; keep math intuition-first, no matrices unless he asks. He'll
  say when he doesn't know something — trust that and back up without hesitation.
