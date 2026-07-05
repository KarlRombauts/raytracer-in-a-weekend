# Binary `.scene` file format

Scenes are serialized as a compact binary blob — a 4-byte magic + version header, then postcard-encoded data compressed with lz4 — rather than a human-readable format like JSON.

**Why:** file size. A scene embeds its baked assets (mesh vertices/faces, image textures, HDR environment maps), so a single scene can run to tens of megabytes. A text encoding would balloon that several-fold; postcard keeps the payload compact and lz4 shrinks it further. The version field lets the format evolve without silently mis-reading old files.

**Trade-off:** scenes are no longer diffable, greppable, or hand-editable. That cost is accepted in exchange for the size win.
