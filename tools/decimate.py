#!/usr/bin/env -S blender --background --python
"""Decimate the meshes listed in a scene_slim manifest, via Blender's Decimate
modifier (Collapse). Writes a `<name>.dec.obj` next to each input.

    blender --background --python tools/decimate.py -- <dir> <target_triangles>

`<dir>` is the directory `scene_slim extract` wrote (containing manifest.tsv).
Import and export use matching (default) axis settings, so the vertex coordinates
round-trip unchanged — the rebake step verifies this with a bounding-box check.
"""
import bpy
import os
import sys

argv = sys.argv[sys.argv.index("--") + 1:]
out_dir, target = argv[0], int(argv[1])

jobs = []
with open(os.path.join(out_dir, "manifest.tsv")) as f:
    for line in f:
        line = line.rstrip("\n")
        if not line:
            continue
        _scene, _idx, name, orig_faces, obj_path = line.split("\t")
        jobs.append((obj_path, int(orig_faces), name))

for obj_path, orig_faces, name in jobs:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.wm.obj_import(filepath=obj_path)

    obj = next(o for o in bpy.context.scene.objects if o.type == "MESH")
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj

    ratio = min(1.0, target / max(1, orig_faces))
    mod = obj.modifiers.new(name="dec", type="DECIMATE")
    mod.decimate_type = "COLLAPSE"
    mod.ratio = ratio
    bpy.ops.object.modifier_apply(modifier=mod.name)

    n_after = len(obj.data.polygons)
    dec_path = obj_path[:-4] + ".dec.obj"
    bpy.ops.wm.obj_export(
        filepath=dec_path,
        export_materials=False,
        export_uv=False,
        export_normals=False,
        export_triangulated_mesh=True,
    )
    print(f"[decimate] {name}: {orig_faces} -> {n_after} tris (ratio {ratio:.3f}) -> {dec_path}")

print("[decimate] done")
