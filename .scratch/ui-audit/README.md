# UI audit — Lumi viewer (`src/viewer/`)

From a 7-dimension swarm audit + a focused typography/element inventory. The
delete-button hover glitch and the two add-object bugs are already **fixed**; the
rest is captured here as the actionable backlog.

## Root cause of the inconsistency

`theme.rs` has **color tokens only** (+ `FIELD_H`). There is **no typography,
radius, or spacing scale**, so every size/gap/radius is an inline literal. egui
0.34 defaults fill the gaps (Body/Button 12.5px, Mono 12.0, Heading 18). Result:
equivalent things drift.

## Typography drift (equivalent role → different size)

- **Uppercase headers use 3 sizes:** 10.0 (`outliner.rs:107,127`) vs 11.0
  (`outliner.rs:43`) vs 11.5 (`prop_row.rs:41`).
- **Mono values use 4 sizes:** 12.5 (`axis_field.rs:84`) vs 12.0 (chip
  `top_bar.rs:45`, passes `viewport.rs:250`, res-badge `viewport.rs:31`, count
  `outliner.rs:48`) vs 11.0 (U/V) vs 10.0 (`.obj`).
- **Viewport control labels:** 12.5 (`viewport.rs:63,169`) vs 11.0 (`:264,:272`).
- **Body labels:** manual 12.0 (`outliner.rs:210`, `object.rs:20`) vs 12.5 default.
- **Card-title cluster:** 13.5 (`home.rs:235,287`) vs 13.0 (`mod.rs:983`,
  `outliner.rs:173`).

### Proposed type scale → add to `theme.rs`

| token | px | family | role |
|---|---|---|---|
| `FONT_DISPLAY` | 30 | semibold | hero |
| `FONT_TITLE` | 15 | semibold | wordmark, lede |
| `FONT_STRONG` | 13.5 | proportional | card titles, toast |
| `FONT_BODY` | 12.5 | proportional | default body/labels/buttons |
| `FONT_LABEL` | 11.5 | semibold | section & category headers |
| `FONT_CAPTION` | 11 | proportional | sub-labels, mini control labels |
| `FONT_MONO` | 12.5 | monospace | ALL field values + mono chips |

Key remaps: uppercase headers 10/11 → 11.5; mono 12.0 → 12.5; viewport labels
12.5 → 11.0; body 12.0 → 12.5; toast/mesh-row 13.0 → 13.5. (Icon glyph sizes are
a separate `ICON_LG/MD/SM` set, out of scope for text.)

## Element drift (same concept → different dimensions)

- **Pills** ("action chips") use radius **6/7/8** and height **30/31/32/33/34**:
  `pill_button` 32/8, `pill_tabs` 31/7, tool pill /7, add-object btn **33**/7,
  scene chip 30/6, reset chip ~34/8. `pill_button` used **literal** fills; tabs
  use tokens.
- **Rows** (all nominally 32px): mesh vs object row disagree on radius (6 vs 7),
  left pad (10 vs 8), and font (13 vs 12.5).
- **Containers** use 6 radii: swatch 5, thumb/mesh-hover 6, chip/toast 8,
  overlay/segment/card 9, texture frame 10, home cards 12.

### Proposed tokens → add to `theme.rs`

```
ROW_H = 32; BUTTON_H = 32; CHIP_H = 30;
RADIUS_SM = 6; RADIUS_MD = 8; RADIUS_LG = 12;   // pills→MD, rows/swatch/thumb→SM, cards→LG
SPACE_XS/SM/MD/LG/XL = 2/4/6/8/12; ROW_INDENT = 8;   // retire the 8-vs-10 / 3-mechanism row pad
```
Colors already added (this pass): `SURFACE_HOVER` (#22252a — was repeated at
`outliner.rs:158,258` + mesh/object rows + `buttons.rs`), `BORDER_PILL` (#33373d),
`DANGER`/`DANGER_BORDER`/`danger_soft()`. Still missing: `SUCCESS` (green at
`viewport.rs:237`), and route **gizmo axis colors** (`gizmo.rs:106-110`, which
diverge in RGB) through `theme::AXIS_*`/`SELECTION`.

## Spacing drift
Row left-indent uses 3 mechanisms (`button_padding.x=10`, `add_space(10)`, `+8`
rect pad); panel inner margins 12 vs 14; bare `add_space` 4/6/8 used
interchangeably. → the `SPACE_*` + `ROW_INDENT` tokens above.

## Other audit findings (severity-ranked) — the non-typography backlog

**HIGH:** silent async OBJ/texture load failures (`controls.rs:449,947`; scene
path already toasts — copy it); web thread-pool init failure = white screen, no
fallback (`lib.rs:145`, `index.html`); no loading/error UI in `index.html`;
responsive breakage on narrow windows (top-bar toggle overlaps side groups
`top_bar.rs:121`; 500px side-panel min squeezes viewport `mod.rs:543-570`);
unsaved edits discarded with no confirm (`mod.rs:208,911`); HDR skies silently
unavailable on web (`env_map.rs:171`, hdrs not copied to dist).

**MED:** object-row hover highlight dead — same interact-steal bug as the fixed
mesh rows (`outliner.rs:252`, use the geometric `pointer_hover_pos` fix); no
pointing-hand cursor on most clickables; deprecated egui 0.34 APIs
(`Panel::default_width/width_range` `mod.rs:528-560`, `wants_keyboard_input`
`:444,473,933`, `allocate_new_ui` `home.rs:138`, `style/set_style`
`theme.rs:97,133`); thumbnail cache key collision `"__current__"` shows wrong
texture + never evicts (`texture_library.rs:63`); unbounded name overflow — no
`.truncate()` (`outliner.rs:283`, `top_bar.rs:42`); save feedback inconsistent
(native failures eprintln-only; Save-image no toast); error/success toasts look
identical (`mod.rs:961`); no web fetch timeout → stuck on Loading forever.

**LOW:** no Escape-to-deselect / arrow nav; eye toggle no tooltip
(`outliner.rs:297`); viewport pick doesn't switch to Object tab (`mod.rs:792`);
"Local axes" label not clickable (`viewport.rs:135`); gizmo tools can all toggle
off (`viewport.rs:108`); editor shortcuts (F/X/Delete) act in Render mode
(`mod.rs:473`); index-keyed row ids (latent); per-frame `format!` churn; subpath
deploy fragility (no Trunk `public-url`); `data-wasm-opt="0"`.

**Verified good (don't touch):** selection bounds-guarded, undo/redo gated +
tooltipped, delete undoable, panic hook set, decode/BVH off the main thread,
repaints all conditional, home/texture grids responsive.
