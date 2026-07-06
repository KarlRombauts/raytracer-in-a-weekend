# 0003 — The Jacobian as a stretch factor

**Date:** 2026-07-06
**Lesson:** `lessons/0003-the-jacobian-stretch-factor.html`

## Trigger
Mid-way into building the (planned) texel→direction lesson, Karl interrupted:
"I don't know what a Jacobian is." Prerequisite gap surfaced exactly when it
mattered. Reordered: the Jacobian became its own Lesson 03; texel→direction +
solid-angle pdf slid to Lesson 04.

## What was taught
- A Jacobian = **local area stretch factor** of a mapping (output area ÷ input
  area). Taught with NO matrix machinery.
- 1D warmup: derivative = stretch factor.
- **The rule that matters**: density divides by the stretch factor (rubber-sheet
  dots analogy). Stretch → density falls; squish → density concentrates.
- Grounded in his problem via the **flat-map ↔ globe** analogy (Greenland):
  image→sphere shrinks the poles; a whole top row of the image ≈ one direction.
- Landed the concrete fact: image→direction stretch = `2π²·sinθ`, → 0 at poles;
  so to go from an image-pdf to a direction-pdf you **divide by 2π²·sinθ**.
- Polar coords (`dA = r dr dθ`, Jacobian = r) as a familiar sanity anchor.
- Explicitly deferred: the derivation, and the sinθ *cancellation* (→ Lesson 4).

## Assessment
- Karl is comfortable saying "I don't know X" and asking to back up — excellent
  for a learner; means I can trust his signals and shouldn't over-assume prior math.
- Likely calculus comfort is light. Keep new math intuition-first, formulas second,
  one idea per lesson. Avoid determinants/matrices unless he asks (the lesson's
  "ask me" prompt invites that if he's curious).

## Next
- **Lesson 04**: assemble it — sampled (u,v) → world direction (run his `sample()`
  convention backwards), then `p_ω = p_img / (2π²·sinθ)`, and show the two sinθ
  cancel → sampling ∝ luminance. This is the last conceptual piece before code.
- **Lesson 05**: `Distribution2D` in Rust against `EnvMap.data`.
