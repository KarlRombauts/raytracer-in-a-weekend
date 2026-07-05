# Raytracer

A Monte-Carlo path tracer (grown from "Ray Tracing in a Weekend") with an interactive egui scene editor and progressive rendering. This glossary fixes the project's canonical vocabulary — when several words exist for one concept, the chosen headword wins and the rest are listed under _Avoid_.

## Scene & geometry

**Scene**:
The editable document — a camera plus a set of objects. What gets authored in the editor and serialized to a `.scene` file.
_Avoid_: world (for the document), level, project

**World**:
The built, acceleration-backed runtime structure the path tracer actually walks, produced from a Scene before rendering.
_Avoid_: scene (for the runtime)

**Object**:
A placed, named item in a scene, carrying a shape, a material, and a transform.
_Avoid_: primitive, element, entity, instance

**Shape**:
The *kind* of geometry an object has: Sphere, Quad, Box, or Mesh.
_Avoid_: form, geometry-type

**Primitive**:
A single leaf hittable inside the BVH acceleration structure. A layer below Object — one Object's mesh expands into many primitives.
_Avoid_: object (for a BVH leaf)

**Mesh**:
Triangle-based geometry, loaded from an OBJ file or built in code.

**Ray**:
A half-line (origin + direction) the tracer follows through the world.

**Hit**:
The record of where a ray meets an object — point, surface normal, material, and texture coordinates.
_Avoid_: intersection record, collision

**BVH**:
Bounding-volume hierarchy — the tree that accelerates ray/object intersection.

## Materials & shading

Headwords here are the **product** names shown in the editor (the "Blender-ish" UI labels). Each definition records the engine type or field name in parentheses so developers can bridge to the code, but the product word is canonical.

**Material**:
Defines how a surface scatters and emits light. Its type is chosen in the Surface picker.

**Surface**:
The material-type selector on an object — the choice between Diffuse, Glossy, Metal, Glass, and Emission.

**Diffuse**:
An ideal matte material. (Engine type: `Lambertian`.)
_Avoid_: Lambertian (as the product term), matte

**Glossy**:
A Fresnel clear-coat over a diffuse base.

**Metal**:
A reflective material; its Roughness blurs the reflection. (Field: `fuzz`.)

**Glass**:
A transparent, refracting material — it transmits light, not just reflects it. (Engine type: `Dielectric`.)
_Avoid_: Dielectric (as the product term)

**Emission**:
A material that radiates light; giving an object an Emission surface makes it a light source. (Engine type: `DiffuseLight`.)
_Avoid_: DiffuseLight, emitter, lamp (as the product term)

**Color**:
A material's base colour.
_Avoid_: albedo, tint (both are code fields, not the product term)

**Roughness**:
How blurred a surface's reflection or transmission is, from 0 (mirror-sharp) to 1. One term across every material; on Metal the underlying field is `fuzz`.
_Avoid_: fuzz (as the product term)

**IOR**:
Index of refraction for a Glass surface — how strongly it bends light.
_Avoid_: refraction index (spell out sparingly)

**Strength**:
The brightness multiplier applied to an Emission surface's colour.
_Avoid_: intensity

**BSDF**:
The surface-scattering function. Covers both reflection and transmission, so it is the umbrella term here (Glass transmits). BRDF is the reflection-only special case.
_Avoid_: brdf (as the umbrella)

**Scatter**:
What a material does to an incoming ray — reflect, refract, or absorb it.

## Lighting & sampling

**Light**:
An object whose material emits, registered so the tracer can sample toward it directly. Pairs a sampleable geometry (an **AreaLight**) with its emission.

**AreaLight**:
Geometry that can be sampled as a light surface — it hands back a direction toward itself and the solid-angle density of that choice. The trait a Sphere, Quad, or Triangle implements; the transform decorators deliberately don't, so a transformed light is baked into a concrete primitive rather than sampled through a decorator. Distinct from **Light** (the emissive object + its emission).

**Sky**:
The radiance a ray sees when it hits nothing. The umbrella concept, covering both a flat background colour and an environment map.
_Avoid_: background, environment (as the umbrella), HDR (as the concept)

**Environment map**:
An equirectangular HDR image used as the sky. Abbreviated EnvMap.
_Avoid_: skybox, IBL image

**Sample**:
One Monte-Carlo estimate of a pixel's colour. "Samples per pixel" is the per-pixel budget.
_Avoid_: sample (for a bundled scene — those are Presets), ray estimate

**PDF**:
Probability density of a chosen sampling direction, used to weight each Monte-Carlo estimate.

**MIS**:
Multiple importance sampling — combines light sampling and BSDF sampling, weighted by the power heuristic, to cut noise.
_Avoid_: mixture sampling

**Power heuristic**:
The weighting function MIS uses to blend the light-sampling and BSDF-sampling strategies.

**Direct light sampling**:
Sampling rays toward emitters explicitly (rather than hoping a bounce finds them) to reduce noise.

## Rendering & output

**Render**:
The offline, path-traced image — accumulated progressively over passes. Viewed in Render mode.
_Avoid_: preview (for the path-traced image)

**Preview**:
The interactive, OpenGL-rasterized editor view. Fast and approximate; not the path tracer.
_Avoid_: render (for the raster view), viewport render

**Pass**:
One progressive iteration that adds samples to the accumulating render.

**Integrator**:
The algorithm that estimates the radiance returned along a camera ray — path tracing. Selected per render (Naive or MIS) behind the `Integrator` trait, built from the camera config. Distinct from the **Render** (the image) and the progressive accumulator (`ProgressiveRenderer`) that drives it.
_Avoid_: renderer (that's the accumulator), ray_color

**Tone mapping**:
Mapping high-dynamic-range radiance down to a displayable colour (ACES filmic).

**Firefly**:
An outlier, excessively bright sample; clamped to keep noise down.

## Editor & app

**Preset scene**:
A bundled, ready-made scene in the sample library (Cornell Box, Glass Block, Teapot…).
_Avoid_: sample scene

**Preset mesh**:
A bundled mesh a user can drop into a scene (Monkey, Bunny, Teapot, Dragon).
_Avoid_: sample mesh

**Edit mode / Render mode**:
The two viewer modes — Edit mode shows the Preview for authoring; Render mode shows the path-traced Render.

**Outliner**:
The panel listing the objects in the current scene.

**Inspector**:
The panel for editing the selected object, the camera, or the output settings.

**Gizmo**:
The on-screen handle for translating, rotating, and scaling an object in the Preview.

**Transform**:
An object's rotate / scale / translate placement within a scene.
