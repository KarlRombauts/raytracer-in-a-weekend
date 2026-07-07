# Procedural textures are evaluated in world space

Solid/procedural textures (Perlin noise, and anything else that reads the point
`p` in `Texture::value(u, v, &p)`) are evaluated at the **world-space** hit point,
not in the object's local frame. This is inherited from the "Ray Tracing in a
Weekend" lineage and reinforced by our design choice to bake each object's
transform directly into its geometry (`placed_quad` et al.), so the point handed
to `value`/`Material::emitted` is already in world space with no local frame
retained.

**Why:** simplicity. Baking transforms into geometry (rather than keeping ray/
point decorators) made intersection and the object-level material model cleaner,
and image textures — the common case — only use `(u, v)`, which is unaffected.
Solid procedural textures were never a priority, so the world-space `p` was never
revisited.

**Trade-off:** a procedural-textured object is *carved out of a fixed field in
world space* rather than having the texture bound to it. Move or rotate the object
(e.g. via the editor gizmo) and the pattern **swims** across the surface — the
marble veins stay put while the object slides through them — instead of following
the object. Correct solid texturing evaluates in object/local space, which would
require retaining a world→object transform per object and applying it in texture
eval — cutting against the transform-baking choice. Accepted for now because our
emitters/materials are Solid-color (which ignores `p` entirely) and scenes are
effectively static per render; revisit if procedural materials under live
transforms ever matter.
