# PRD: 32-byte BVH node compaction

Status: ready-for-agent

> First optimization off `.scratch/bvh-perf/RESEARCH.md`, chosen because it is the
> highest impact-per-effort **bit-identical** change (no image change, no re-pin).
> The benchmark harness (`.scratch/bvh-perf/PRD.md`) is the instrument that proves
> it: the traversal-work counter must stay **constant** (semantics unchanged)
> while `traversal/*` ns/ray drops.

## Problem Statement

`BVHFlatNode` is ~44 bytes: `left: u32`, `right: u32`, `aabb: AABB` (24 B),
`first_primitive: u32`, `primitive_count: u32`, `split_axis: u8`, `depth: u8`.
BVH traversal is memory-bound — it chases node indices through the flat array,
and each node touched is a potential cache miss. At 44 bytes, two nodes straddle
three 64-byte cache lines instead of fitting in one; and every node drags a
`depth` byte that exists only to feed the (dead, never-called) `get_stats`.
Traversal is the hot path (per ray, per bounce, per sample), and the inner
per-triangle mesh BVH is where it dominates.

## Solution

Pack `BVHFlatNode` to exactly **32 bytes**, so two nodes fit in one cache line,
by exploiting two facts:

1. **The left child is always `idx + 1`.** `flatten` reserves the parent slot,
   then immediately recurses left — so the left child is always the next node in
   the array. The `left` field is redundant; drop it and derive it.
2. **`depth` is dead weight.** Its only reader is `get_stats`, which nothing
   calls. Drop the field (and the dead `get_stats` / `BVHStats` / `get_split_axis`
   it fed).

The remaining data — the AABB, the right-child (or first-primitive) index, the
primitive count (leaf) or split axis (interior), and a leaf flag — packs into
`aabb (24) + offset: u32 (4) + meta: u32 (4) = 32 bytes`.

This changes **nothing** about which nodes or primitives a ray tests: the
traversal visits the same nodes in the same order, so the image is bit-identical
and the traversal-work counts are unchanged. It is purely a memory-layout win.

## User Stories

1. As a developer, I want each BVH node to fit two-per-cache-line, so traversal
   touches less memory and runs faster.
2. As a developer, I want the redundant `left` child index removed (it is always
   `idx + 1`), so the node is smaller with no loss of information.
3. As a maintainer, I want the dead `depth` field and the dead `get_stats` that
   read it removed, so nodes don't carry bytes nothing uses.
4. As a maintainer, I want the produced image to stay **bit-identical**, so I have
   direct evidence the layout change altered nothing about results.
5. As a developer, I want the **traversal-work counts unchanged**, so I have
   hardware-independent proof the traversal semantics are identical (only the
   per-node memory cost moved).
6. As a developer, I want the node's bit-packing hidden behind a small accessor
   API (`is_leaf`, right-child, split-axis, leaf primitive range), so the
   traversal reads cleanly and the encoding lives in one place.
7. As a maintainer, I want the empty-BVH case to keep working (a single empty leaf
   node), so a world with no primitives still misses cleanly.

## Implementation Decisions

**32-byte layout.**

```
struct BVHFlatNode {
    aabb:   AABB,   // 24 B  — the node bounds
    offset: u32,    //  4 B  — interior: right-child index; leaf: first-primitive index
    meta:   u32,    //  4 B  — bit 31 = leaf flag;
                    //         interior: low bits = split axis (0/1/2)
                    //         leaf:     low 31 bits = primitive count
}
```

- **Left child is implicit:** `idx + 1`. Interior nodes always have both children
  (binary SAH splits), and `flatten` guarantees the left subtree is emitted
  immediately after the parent.
- **Leaf flag** is the top bit of `meta` (`LEAF_BIT = 1 << 31`). `is_leaf()` tests
  it. Primitive counts and split axes are tiny, so the low bits never collide with
  it (a leaf could hold up to 2³¹ primitives — no `u16` overflow risk from a
  degenerate coplanar leaf, unlike a naive 16-bit count).
- **Empty BVH:** the single node is an empty leaf — `LEAF_BIT | 0` count, offset 0
  — so traversal tests its (empty) AABB and iterates an empty primitive range,
  missing cleanly, exactly as today.

**Encapsulate the encoding.** The bit-twiddling lives behind constructors
(`leaf(aabb, first, count)`, `interior(aabb, right, axis)`) and accessors
(`is_leaf`, `right_child`, `split_axis`, `leaf_range`), so `flatten` and
`closest_hit` never touch raw bits. `BVHFlatNode`'s fields become private.

**Traversal is structurally unchanged.** `closest_hit` reads the same information
through the new accessors: left child = `node_idx + 1`, right child =
`node.right_child()`, axis = `node.split_axis()`, leaf primitives =
`node.leaf_range()`. The push-farther-child-first ordering, the tightened-interval
AABB test, and the `#[cfg(feature = "bvh-stats")]` counter increments are all
preserved verbatim — so per-ray box/primitive test counts are identical.

**Drop the dead diagnostics.** `depth`'s only consumer is `get_stats`, which has
no callers; remove `get_stats`, `BVHStats`, its `Display`, and `get_split_axis`
(also unused) along with the field. (The separate, entirely-unused pointer BVH in
`bvh_node.rs` is out of scope — a different cleanup.)

**`flatten` emits the new encoding.** Leaf `BuildNode`s become `leaf(...)` nodes;
inner `BuildNode`s become `interior(aabb, right, axis)` where `right` is the
flattened right-child index (left is implicit). The reserve-then-recurse-left
structure that guarantees `left == idx + 1` is unchanged.

## Testing Decisions

**What makes a good test here.** This is a pure memory-layout change with a
mechanical, checkable size target and a strict behaviour-preservation contract.
The value is in (a) pinning the new size, and (b) proving nothing observable
moved — via the existing behavioural guards plus the traversal-work counter.

**New, drives the change (red → green).**
- **Node size:** `size_of::<BVHFlatNode>() == 32`. Red at ~44 today; green after
  the repack. This is the concrete target of the change.

**Behaviour-preservation guards (must stay green, unchanged).**
- **BVH-vs-linear equivalence** (`world.rs`) — closest hit + resolved material
  match a brute-force scan over hundreds of rays.
- **Winner-returning query** (`flat_bvh.rs`) — nearest-primitive identity.
- **Flat-face holes** (`flat_bvh.rs` `sample_tests`) — the degenerate all-coplanar
  leaf case; the load-bearing guard that the new leaf encoding still works when a
  node's AABB is flat and a leaf holds many primitives.
- **Render pin** (`tests/render_characterization.rs`) — `0x9436e82cbff110f1`
  must stay **bit-identical**.

**Counter invariance (the semantics-unchanged proof).** The `bvh-stats`
node-box-test and leaf-primitive-test counts for a fixed mesh + ray battery must
be **identical** before and after. Verified by re-running the `bvh_stats` example
and diffing against the pre-change numbers (e.g. `teapot/x` = 24242 box / 1395
prim). If they change, the "layout-only" claim is false — investigate.

**Perf (the point, not a pass/fail gate).** `cargo bench --baseline
pre-node-compaction` should show `traversal/*` ns/ray improve (and `build/*` at
worst unchanged). Measured, not asserted in a test.

## Out of Scope

- **Spatial splits / SBVH, wide (QBVH/MBVH), any-hit shadow rays** — later units
  in the research plan.
- **Removing the dead `bvh_node.rs` pointer BVH** — a separate cleanup, unrelated
  to `BVHFlatNode`'s layout.
- **Changing SAH build quality or traversal order** — this unit must be
  bit-identical; the tree built and the nodes visited are exactly as before.
- **Compressed/quantized nodes** (e.g. 16-bit AABBs) — a further, lossy step; this
  unit keeps full-precision `f32` bounds.

## Further Notes

- Expected win per the research: fewer cache lines touched per traversal. The
  counter staying flat while ns/ray drops is the signature of a pure cache win —
  the exact thing the harness was built to demonstrate.
- The 32-byte target assumes `AABB` stays 24 bytes (6 × `f32`) and `u32` indices.
  If a scene ever needed > 4 B of primitives/nodes the indices would have to grow,
  but that is far beyond current scene sizes.
