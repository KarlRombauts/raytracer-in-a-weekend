# Resources

Trusted sources for environment-map importance sampling and MIS. **Primary
source is PBRT** — the reference implementation everyone cites.

## Primary (high-trust)

- **PBRT, 12.6 Infinite Area Lights (3rd ed.)** — *the* canonical treatment:
  building the `Distribution2D` over the envmap (weighted by luminance × sinθ),
  sampling it, and the image→direction pdf conversion.
  https://pbr-book.org/3ed-2018/Light_Sources/Infinite_Area_Lights
- **PBRT, 13.10 2D Sampling with Multidimensional Transformations (3rd ed.)** —
  how `Distribution2D` works: the marginal-over-rows + conditional-per-row
  construction that lets you sample a 2D grid by weight.
  https://pbr-book.org/3ed-2018/Monte_Carlo_Integration/2D_Sampling_with_Multidimensional_Transformations
- **PBRT, 13.3 Sampling Random Variables (3rd ed.)** — the inversion method +
  `Distribution1D` (cumulative array + binary search). Lesson 2's source.
  https://pbr-book.org/3ed-2018/Monte_Carlo_Integration/Sampling_Random_Variables
- **PBRT, 13.5 Transforming between Distributions (3rd ed.)** — the change-of-
  variables rule `p_y = p_x / |J|` (the Jacobian). Lesson 3's source.
  https://pbr-book.org/3ed-2018/Monte_Carlo_Integration/Transforming_between_Distributions

## Supplementary (gentler / visual)

- **"Keeping Probabilities Honest: The Jacobian Adjustment"** (Towards Data
  Science) — the Jacobian from a probability angle. Medium-trust, good intuition.
  https://towardsdatascience.com/keeping-probabilities-honest-the-jacobian-adjustment/
- **PBRT, 14.2 Sampling Light Sources / MIS (3rd ed.)** — the power heuristic and
  how two strategies (light-sample + BSDF-sample) combine. (You already
  implement this for area lights: `src/integrator/common.rs::power_heuristic`.)
  https://pbr-book.org/3ed-2018/Light_Transport_I_Surface_Reflection/Sampling_Light_Sources
- **PBRT, 12.5 Image Infinite Lights (4th ed.)** — the newer edition's version,
  with `PiecewiseConstant2D`. Same ideas, updated names.
  https://www.pbr-book.org/4ed/Light_Sources/Infinite_Area_Lights

## Your own code (grounding)

- `src/texture/env_map.rs` — the `EnvMap`: `data` (row-major texels), `texel(x,y)`,
  and `sample(dir)` (the equirectangular direction→(u,v) lookup).
- `src/integrator/mis.rs` — where a sky-sampling branch would slot into the NEE block.

## Communities (for wisdom, when ready)

- **Graphics Programming Discord** (`#raytracing`) — very high-signal for path
  tracing / importance sampling questions. https://discord.gg/graphicsprogramming
- **r/GraphicsProgramming** — good for "is my pdf right?" sanity checks.
  https://www.reddit.com/r/GraphicsProgramming/
