# 0002 — Sampling from a pdf: the inversion method

**Date:** 2026-07-06
**Lesson:** `lessons/0002-sampling-from-a-pdf-inversion.html`

## Trigger
Karl asked, unprompted: "how does a computer sample from a PDF? I get the PRNG
gives a uniform sample, but how does it weight that by the PDF?" — a precise,
foundational question that revealed his true ZPD. This is the prerequisite for
the `Distribution2D` build, so it was slotted in as Lesson 02 (the planned
texel→direction lesson moved to 03).

## What was taught
- **Inverse transform sampling**: the pdf never multiplies the random draw; it
  carves [0,1) into segments whose widths are the probabilities (the CDF's
  cut-points). Uniform draw + uneven carving = weighted samples.
- **Discrete recipe** (the one he'll code): normalise weights → cumulative array
  (CDF) → draw u → binary-search for the first cut-point > u. O(log N).
- Worked example `w=[1,6,1,2]`; the sampler must also report `pᵢ = wᵢ/Σw` for MIS.
- **Two flavours**: analytic inversion (he already has one —
  `cosine_direction_from_uv`, Malley) vs tabulated (the envmap → cumulative array
  + search). The row/column picks from Lesson 1 are two tabulated inversions.

## Assessment
- Strong sign: he's asking the *mechanism* question, not the *what* question — he's
  moving from "what is this" to "how would I build it." Good momentum toward
  implementation.
- Gave a paper exercise (`[3,1,4,2]`, u=0.55) with answers, to check the recipe
  landed. Watch for whether he does it / reports back.

## Next
- **Lesson 03**: chosen texel → world direction + solid-angle pdf; derive sin θ as
  the image↔sphere Jacobian. (This is where his Lesson-1 "sin θ teaser" pays off,
  and it connects the pdf from Lesson 2 to a *directional* pdf MIS can use.)
- Then Lesson 04: `Distribution2D` in Rust (marginal + conditionals) against his
  actual `EnvMap.data`.
