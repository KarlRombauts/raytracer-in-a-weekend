# Session handoff — raytracer (Lumi viewer)

Snapshot of where things stand so a fresh session can pick up cleanly. Repo is
**public and live**; work is on `main`.

## Project state

- **GitHub:** https://github.com/KarlRombauts/raytracer (public, MIT, branch
  `main`). Origin is SSH; `gh` is authed as KarlRombauts.
- **Live demo:** https://raytracer-253.netlify.app — threaded WASM, verified
  serving COOP/COEP. Netlify site `raytracer-253` (the bare `raytracer` subdomain
  was globally taken); linked in the repo dir, deploy via `netlify deploy --prod
  --dir dist`. Netlify CLI is installed + logged in (Karl Rombauts's team).
- **Tests:** `cargo test` green (213 lib + render pin). `.git` ≈ 142 MB.
- **Internal crate name is still `raytracer-in-a-weekend`** (drives the wasm
  filename) — repo was renamed, crate wasn't. Rename is optional + touches many
  `raytracer_in_a_weekend::` refs.

## What this session did

1. Shipped **shadow-rays Stage 2** (per-light occlusion NEE) + measured the win
   (occlusion micro-bench, 1.2–1.6×/ray). See `.scratch/shadow-rays/`.
2. **Published the repo public**: history rewrite stripped all `Co-Authored-By:
   Claude` trailers (187 commits) + purged big binaries (dragon/monkey objs, old
   scene blobs, cruft, `.DS_Store`); renamed `master`→`main`; MIT license; secret
   scan clean. Mirror backup of pre-rewrite history is in the **scratchpad**
   (`raytracer-backup.git`) — deletable when you're confident.
3. **Slimmed the sample scenes**: they embedded full-res meshes (2.35M-tri
   dragon, etc.). Decimated all to ~150K via Blender → 47MB→6MB scenes. Pipeline
   committed: `examples/scene_inspect.rs`, `examples/scene_slim.rs`,
   `tools/decimate.py`. (`blender` is installed.)
4. **Fixed viewer bugs**: add-object row click-over-text; hover highlight (mesh
   rows); missing `assets/objs/monkey.obj` (added, decimated to ~5K); delete
   (icon) button's double-glyph hover overlay → idiomatic single-glyph, added
   `theme::` tokens (`SURFACE_HOVER`, `BORDER_PILL`, `DANGER*`).
5. **UI audit** (7-agent swarm + typography inventory) → `.scratch/ui-audit/README.md`.

## Immediate next task (the work order)

**Design-token unification** — the viewer has color tokens only; typography,
radius, and spacing are inline literals that drift (3 header sizes, 4 mono sizes,
pills at radius 6/7/8, rows disagreeing on radius/pad/font). `.scratch/ui-audit/
README.md` has the full spec: a 7-step `FONT_*` scale, `RADIUS_SM/MD/LG`,
`ROW_H/BUTTON_H`, `SPACE_*`/`ROW_INDENT`, plus a mapping table (literal → token,
from→to). Mechanical, low-risk, ~15 files. Start there. Also fold in the two
cheap audit MEDs: object-row hover (`outliner.rs:252`, same geometric
`pointer_hover_pos` fix already used for mesh rows) and routing gizmo axis colors
(`gizmo.rs:106`) through `theme::AXIS_*`.

## Other backlog (prioritized) — full detail in `.scratch/ui-audit/README.md`

- **HIGH (demo robustness):** silent async load failures (copy the scene toast to
  the OBJ/texture paths, `controls.rs:449,947`); web thread-pool init = white
  screen with no fallback (`lib.rs:145`, `index.html`); no loading/error UI in
  `index.html`; narrow-window layout breakage (`top_bar.rs:121`, `mod.rs:543`);
  unsaved-edit discard w/o confirm; HDR skies silently unavailable on web.
- **MED:** deprecated egui 0.34 APIs (build warnings); thumbnail cache key
  collision (`texture_library.rs:63`); name overflow (`.truncate()`); toast
  error/success indistinguishable; pointing-hand cursor missing on most clickables.
- **Optional:** enable `data-wasm-opt` (needs binaryen) to shrink the 8.8MB wasm;
  rename Netlify subdomain to a preferred available handle; rename the crate.

## Future feature (not started)

**PBR materials** (metallic-roughness, core 4 maps, loose image files) — fully
scoped + mapped to insertion points in `.scratch/pbr-materials/PLAN.md`. Pick up
with `/tdd` when ready.

## How things work (gotchas)

- **Build/run:** `just render` (native viewer). **Web:** `just web` (needs nightly
  + `-Z build-std`; toolchain installed) → `dist/`; then `netlify deploy --prod
  --dir dist` (do NOT let `netlify` run a build — the CI build command was removed
  from `netlify.toml` for exactly that reason; it once clobbered the local trunk
  binary). Redeploy after any Rust/asset change to update the live demo.
- **Scene format:** `.scene` = lz4(postcard(`SceneFile{version,name,scene,
  preview}`)) behind `RTSC` magic; `scene_file::{encode,decode}`. Meshes embed
  full `verts`/`faces` — re-import via the editor does NOT decimate, so heavy OBJs
  bloat scenes (that's what the `scene_slim` pipeline fixes).
- **Workflow (repo convention):** PRDs/plans in `.scratch/<feature>/`; use the
  Matt Pocock skills (`/tdd`, `/simplify`, `/grilling`, `/code-review`), NOT
  `superpowers:*`. Commits do NOT include Claude co-author trailers (stripped +
  don't reintroduce).
- **egui gotcha (recurring):** interact-before-paint puts the click/hover sense
  *below* later-drawn labels, which then steal it. Fix: interact with the row rect
  *after* content, or use a geometric `pointer_hover_pos()` test (see the fixed
  mesh rows / `buttons.rs`). `override_text_color = Some(TEXT)` (`theme.rs:104`)
  blocks per-state widget text colors, so custom-colored buttons paint their glyph
  manually.

## Recent commits (newest first)
`04b6f3e` ui-audit doc · `4e4c0de` idiomatic button hover + tokens · `cfee0d9`
monkey→5K + hover fix · `8263e5b` monkey.obj + row click fix · scene-slim +
publish commits before that.
