# BVH & CPU Path-Tracer Performance Optimizations — Research Findings

Research target: a Rust CPU path tracer with a two-level flat BVH (binned-SAH build, 12 bins,
rayon-parallel, flattened `Vec<BVHFlatNode>`, stack-based front-to-back traversal with precomputed
inverse ray direction, ~44-byte nodes; TLAS over scene-object proxies + per-mesh inner triangle BLAS;
MIS + NEE integrator whose shadow rays currently do a **full closest-hit** query).

Every non-obvious claim is cited inline. Primary sources are PBRT 4th ed (pbr-book.org/4ed), Jacco
Bikker's "How to build a BVH" series (recognized authority), the original acceleration-structure papers
(Stich, Dammertz, Wald, Karras/Aila, Baldwin/Weber), and NVIDIA's CWBVH work. Full list under **Sources**.

A note on "does it change image output": a pure-perf change that only reorders traversal or repacks
memory is **bit-identical** — the same closest hit is found, so no render fingerprint needs re-pinning.
Anything that changes *which* primitive/emitter is sampled, or changes floating-point rounding order in
the intersection math, is **not** bit-identical and needs a re-pin. Each item is flagged explicitly.

---

## 1. Node layout / memory compaction

**What it is.** Shrink the BVH node so two nodes fit in one 64-byte cache line. The canonical layout is
a **32-byte node**: `float3 aabbMin, aabbMax` (24 B) + two `uint`s. The two uints are overloaded: one field
(`leftFirst`) means *left-child index* for an interior node and *first-primitive index* for a leaf, with
`triCount`/`primCount` distinguishing the two (interior nodes have count 0, right child implicit at
`leftFirst+1` when children are stored contiguously) — this removes redundant pointers and lets "every node
always use exactly half of a cache line" (Bikker, *How to build a BVH — Part 1: Basics*,
https://jacco.ompf2.com/2022/04/13/how-to-build-a-bvh-part-1-basics/). PBRT's flattened `LinearBVHNode` is
likewise deliberately padded to **32 bytes** so "multiple nodes fit in a cache line"
(PBRT 4e, *Bounding Volume Hierarchies* §, https://pbr-book.org/4ed/Primitives_and_Intersection_Acceleration/Bounding_Volume_Hierarchies).

Beyond 32 B: **quantized / compressed nodes** store child AABBs relative to the parent, quantized to 8-bit
per axis. NVIDIA's CWBVH (compressed wide BVH8) reaches **80 bytes per 8-wide node** and total memory of
"35–60% of a typical uncompressed BVH", buying **1.9–2.1× faster incoherent traversal** largely from reduced
memory traffic (Ylitie, Karras, Laine, *Efficient Incoherent Ray Traversal on GPUs through Compressed Wide
BVHs*, HPG 2017, https://research.nvidia.com/publication/2017-07_efficient-incoherent-ray-traversal-gpus-through-compressed-wide-bvhs).

**Expected speedup.** The current node is **~44 bytes** → two nodes straddle three cache lines instead of
one. Dropping to 32 B is the single highest-leverage layout change; the cost model is memory-bound, so gains
track the reduction in cache-line touches. Bikker frames the 32-B node as *the* enabling representation for
the fast traversal in his series (no isolated % given for the shrink alone, because it is baked into the
baseline). Quantized nodes are a further step with the 1.9–2.1× figure above, but that number is for GPU
incoherent workloads and 8-wide nodes, not a 2-wide CPU tree.

**Implementation complexity.** Low–medium. Getting to 32 B: drop `split_axis` (recompute or store 2 bits in
spare bits) and `depth` (only needed at build time, not traversal), and union the left/first + right/count
fields. The current node carries `u32 left, u32 right, u32 first_primitive, u32 primitive_count, AABB(24),
u8 split_axis, u8 depth`. Interior nodes don't need `first_primitive`/`primitive_count`; leaves don't need
two child pointers. Overload one pair and store children contiguously so `right = left + 1` is implicit.
Quantized nodes are high complexity (dequantize in the hot loop, conservative rounding).

**Does it change image output?** 32-B compaction: **bit-identical** (pure memory layout; same hits). Quantized
nodes: **changes output** — dequantized bounds are conservative approximations, so the set of nodes/prims
visited differs and floating-point results can shift; needs a re-pin.

**Fit.** Excellent for the 32-B compaction — directly applicable to the flat `Vec<BVHFlatNode>`. `split_axis`
is used by ordered traversal (see §2) but only needs 2 bits; `depth` is almost certainly build-only telemetry
and can leave the traversal node entirely. Quantized nodes are overkill for a CPU 2-wide tree unless you also
go wide (§3).

---

## 2. Traversal improvements

**What it is.** Four related tricks on the stack-based traversal:
1. **Ordered (front-to-back) traversal** — at an interior node, compute the ray's entry distance to *both*
   child boxes and descend the nearer child first, pushing the farther child. A hit in the near subtree
   shortens the ray (`t_max`) so the far subtree is often skipped entirely.
2. **Distance culling on pop** — when popping a deferred node, re-test its entry distance against the current
   `t_max` and drop it if the ray has since shortened past it.
3. **"Test child boxes before push"** — only push a child if its box is actually hit and nearer than `t_max`,
   rather than pushing unconditionally and testing on pop.
4. **Slab test with precomputed inverse direction** — replace per-box divisions with multiplies by cached
   `1/dir`.

**Expected speedup.** Ordered traversal is large: Bikker measures a backside view going **300 ms → 91 ms
(3.3×)** purely from traversing the nearer child first, and the fully-tuned traversal reaching **86 ms
(4.5 MRays/s)** backside / **103 ms** frontside (Bikker, *Part 2: Faster Rays*,
https://jacco.ompf2.com/2022/04/18/how-to-build-a-bvh-part-2-faster-rays/). The slab reciprocal trick had
"limited effect" in his measurements (the loop was already memory/branch bound), and distance culling on pop
"may only prevent visiting two nodes versus one leaf" — a small win. PBRT does the same ordering, choosing
child order from the ray's sign per split axis (`dirIsNeg[node->axis]`), PBRT 4e BVH §.

**Your codebase already does #1, #4, and pushes the farther child first** — so the big 3.3× is already
banked. The remaining deltas are #2 and #3: make sure you (a) re-check `t_min < t_max` when popping, and
(b) don't push a child whose near-hit distance already exceeds the best hit found so far.

**Stackless / short-stack (further option).** A *short stack* keeps a small fixed stack (e.g. 4 entries) and
falls back to restart/parent-pointer traversal on overflow, bounding stack memory and improving locality; a
fully *stackless* traversal uses parent pointers or a state bitmask. These matter most on GPUs (register/stack
pressure). On a CPU with a `Vec` stack they are largely neutral-to-slightly-negative and not worth it here.

**Implementation complexity.** Low. #2/#3 are a few lines in the existing traversal loop. Short-stack is
medium and not recommended for CPU.

**Does it change image output?** **Bit-identical.** Ordering and culling change *the order* work is done and
*how much* is skipped, never which primitive is the closest hit. Safe, no re-pin.

**Fit.** Directly applicable. Audit the pop path for the `t_max` re-check (#2) and the pre-push cull (#3);
these are the only untapped wins in this bucket since ordered traversal + inverse-dir slab are done.

---

## 3. Wide BVHs — QBVH / MBVH (4-wide, 8-wide) + SIMD box tests

**What it is.** Replace the binary tree with an *N-ary* tree (N = 4 or 8) built by collapsing every
`log2(N)` levels of the binary BVH into one wide node. Each wide node stores N child boxes in
**structure-of-arrays** layout (all `xmin` together, etc.) so one SIMD instruction tests the ray against all
N boxes at once. Fewer, shallower nodes → fewer pointer chases and better SIMD utilization. MBVH/QBVH is due
to Ernst & Greiner and Dammertz et al.; Wald's *Getting Rid of Packets* pushes the idea to branching factor
16 for single-ray SIMD traversal (Dammertz, Hanika, Keller, *Shallow Bounding Volume Hierarchies for Fast
SIMD Ray Tracing of Incoherent Rays*, EGSR 2008; Wald, Benthin, Boulos, *Getting Rid of Packets*, IRT 2008,
https://www.cs.cmu.edu/afs/cs/academic/class/15869-f11/www/readings/wald08_widebvh.pdf).

**Expected speedup.** The whole point of the shallow-BVH line of work is that a 4-wide SIMD tree is
*competitive with or faster than* packet tracing **for incoherent rays** — which is exactly the path-tracer
case — where packets collapse (Wald 2008; Dammertz 2008). Embree, the production embodiment, uses 4-/8-wide
BVHs (BVH4/BVH8) as its default CPU structure precisely because single-ray SIMD-wide traversal wins on
incoherent workloads (Embree overview, https://www.embree.org/). Typical reported wins over a scalar binary
BVH are ~1.5–2× on CPU, dominated by the SIMD box test and reduced node count, but the exact figure is very
implementation- and ISA-dependent.

**Implementation complexity.** High. Touches node layout (SoA child boxes, up to N children + N child
indices), the builder (a collapse pass from the binary BVH, or a native wide builder), and the traversal
inner loop (SIMD intersect of N boxes, then a sort/priority of hit children before pushing). In Rust this
means `std::simd` or `wide`/`glam` SIMD types and careful alignment. It is the biggest single engineering
item on this list.

**Does it change image output?** **Bit-identical** *if* the SIMD box test uses the same rounding as the
scalar test and the closest-hit reduction is order-independent (it is — you keep the min `t`). In practice
SIMD min/max and FMA can change rounding; if you use FMA in the slab test where the scalar path did not,
expect tiny differences → treat as **needs re-pin** unless you match the math exactly.

**Fit.** Applies to the **inner triangle BLAS** (many triangles, deep trees → most to gain from SIMD box
tests). The TLAS is tiny (one box per scene object) and gains almost nothing from going wide. So: consider a
wide BLAS, keep the TLAS binary. This is the highest-ceiling, highest-effort optimization here.

---

## 4. Packet / stream / wavefront traversal for coherent rays

**What it is.** Trace many rays together. **Packets** (4/8/16 rays in lockstep through one binary tree) amortize
node fetch across a SIMD-width of rays. **Streams** (Barringer & Akenine-Möller; Embree ray streams) keep a
larger, dynamically-compacted set of active rays per node to keep SIMD lanes full even as rays diverge.
**Wavefront** (GPU-style) sorts rays into queues by state.

**Expected speedup.** Big for **coherent** rays (camera/primary, tightly-clustered shadow rays): packets can
be several× faster. But "as ray distribution becomes incoherent, SIMD packet tracing quickly suffers from low
SIMD utilization, and incoherent rays can lead to performing more computations than single-ray code would
have" (paraphrase of Wald/Embree guidance; Embree recommends **single-ray** kernels for incoherent workloads
and packet/hybrid only for coherent ones, https://www.embree.org/). Ray streams recover some efficiency for
mixed coherence.

**Implementation complexity.** High, and it fights the current architecture. Packets need the integrator to
generate and shade rays in groups; a path tracer's bounce rays are incoherent by construction, so most of the
SIMD width is wasted after the first bounce.

**Does it change image output?** Traversal itself is bit-identical; but restructuring the integrator to
shade in packets can reorder RNG consumption / sample assignment → likely **needs re-pin**.

**Fit.** **Low priority / poor fit.** A unidirectional path tracer with MIS+NEE produces incoherent
secondary and shadow rays. This is precisely the regime where the wide single-ray BVH (§3) beats packets.
Only the primary rays and (loosely) NEE shadow rays toward a small light are coherent enough to benefit, and
even there the integration-side rewrite is expensive. Prefer §3 over §4 for this codebase.

---

## 5. SAH build quality vs speed

**Binned vs full-sweep SAH.** The quality gap is *small* and the build-time difference is *enormous*.
Bikker measures a full centroid-sweep SAH build at "multiple seconds" (7.4 s in one test) for a **43%** render
speedup over midpoint (183 ms → 128 ms), then shows **binned** SAH recovers essentially all of that quality at
a fraction of the build cost: 8 bins renders **111 ms vs 109 ms** for the full sweep (a ~2 ms / <2% quality
gap), while build time drops from seconds to **~11 ms** — a **30–90× faster build** for ~2% render cost
(Bikker, *Part 2* and *Part 3: Quick Builds*, https://jacco.ompf2.com/2022/04/21/how-to-build-a-bvh-part-3-quick-builds/).
PBRT independently settles on **12 buckets** ("we have found that 12 buckets usually work well in practice",
PBRT 4e BVH §). **Takeaway: your 12-bin binned SAH is already the right call** — going to full-sweep SAH would
cost seconds of build for a sub-2% render gain. Not worth it.

**Spatial splits (SBVH).** Standard SAH can only *partition* primitives; a large triangle straddling a split
inflates both children's boxes. SBVH adds *spatial* splits (à la kd-trees): a straddling triangle's reference
is duplicated into both children, with **reference unsplitting** (put it in only one child if that lowers SAH
cost) to limit duplication, and an `alpha` parameter bounding memory blow-up (Stich, Friedrich, Dietrich,
*Spatial Splits in Bounding Volume Hierarchies*, HPG 2009, https://www.nvidia.in/docs/IO/77714/sbvh.pdf).
Measured impact is large: on Sponza, SBVH gives **14.3% lower SAH cost and 46.0% higher ray-tracing
performance** over a full-sweep baseline (vs binned SAH's 10.2%); on Bistro, SBVH is **+48.9% CPU / +77.7%
GPU** over full-sweep (Bikker, *BVH Quality: Beyond SBVH*,
https://jacco.ompf2.com/2025/05/20/bvh-quality-beyond-sbvh/). Cost: extra memory for duplicated references
(bounded by `alpha`; commonly a small % to ~1.3× primitive references) and a more complex, slower build.

**Triangle pre-splitting.** A cheaper cousin of SBVH: split large triangles into sub-references *before*
building an ordinary SAH BVH. Less effective than in-build SBVH but far simpler and composes with any builder.

**Treelet reordering (post-build optimization).** Take a fast/mediocre tree and locally re-optimize small
subtrees (treelets) by trying all topologies. Karras & Aila's TRBVH reaches "**over 90% of the ray-tracing
performance of the best offline construction method (SBVH)**", where prior fast GPU builds reached only ~50%,
at ~40M tris/s on a Titan (Karras & Aila, *Fast Parallel Construction of High-Quality BVHs*, HPG 2013,
https://research.nvidia.com/publication/2013-07_fast-parallel-construction-high-quality-bounding-volume-hierarchies).
Bikker's reinsertion optimization adds a further **+17% CPU / +12% GPU** on top of SBVH on Bistro (*Beyond SBVH*).

**Expected speedup summary.** SBVH: ~15–50% faster *tracing* on real scenes, most on scenes with large/thin
triangles. Treelet/reinsertion: another ~10–17% on top, or ~90% of SBVH quality starting from a cheap build.
Binned→full-sweep: <2%, not worth it.

**Implementation complexity.** SBVH: high (reference duplication, spatial bins, unsplitting, memory
management). Pre-splitting: medium. Treelet reordering: medium–high. All touch only the **builder**, not
traversal.

**Does it change image output?** **Changes output.** A different tree visits primitives in a different order;
with exact `t` comparisons the closest hit is the same *value*, but ties and NEE emitter selection can shift,
and duplicated references (SBVH/pre-split) change the primitive-hit bookkeeping. Treat all build-quality
changes as **needs re-pin**.

**Fit.** SBVH applies to the **triangle BLAS** and pays off most when meshes have large or high-aspect-ratio
triangles. It is the single biggest *quality* lever after you've exhausted the free traversal wins. If your
meshes are already finely tessellated, SBVH's benefit shrinks (little straddling), so measure first. The
TLAS (boxes around whole objects) rarely benefits from spatial splits.

---

## 6. Shadow / occlusion rays: any-hit early-exit vs closest-hit — **highest-value fix for this codebase**

**What it is.** A shadow/occlusion ray only needs to know *whether anything blocks* the segment between the
shading point and the light — a boolean. The standard formulation is: sample a point on a light, form a ray
toward it with `t_max` = distance to the light (minus epsilon), and run an **any-hit / early-exit** query
that returns `true` the instant it finds *any* intersection inside `(0, t_max)`. PBRT implements this as a
separate predicate method: "`IntersectP()` implementations in shapes return early when they find any
intersection" and "do not return the geometric information associated with an intersection" — no normal, no UV,
no material eval, and traversal aborts on first hit rather than scanning to the true closest surface
(PBRT 4e, *Primitive Interface and Geometric Primitives* §,
https://pbr-book.org/4ed/Primitives_and_Intersection_Acceleration/Primitive_Interface_and_Geometric_Primitives).

**Why it's cheaper than closest-hit.** A closest-hit query must keep going after the first hit to find the
*nearest* one and then compute shading data; an any-hit query stops at the first blocker and computes nothing.
Two savings compound: (1) **early termination** — traversal aborts mid-tree, often after a fraction of the
nodes; (2) **no shading-data computation** at the hit. In an occluded scene (the common case for interior
lighting) the first blocker is found quickly, so any-hit shadow rays are frequently a large fraction cheaper
than the equivalent closest-hit.

**Your current setup vs the standard one.** Your NEE "samples a direction and reads the closest surface's
emission" via a **full closest-hit** query. That is doing strictly more work than necessary for occlusion,
*and* it couples NEE to a direction-sampling formulation rather than the light-point-sampling +
distance-bounded-occlusion formulation that MIS/NEE is normally written against. The standard NEE:
1. Sample a point on a chosen emitter (area-measure sample) → direction + distance to that point.
2. Evaluate BSDF and light PDFs for MIS weight.
3. Fire a **distance-bounded any-hit** shadow ray to that point; if unoccluded, add the (weighted)
   contribution. No closest-hit, no reading "whatever surface the direction happened to hit."

This both speeds up the shadow ray (early-exit, no shading) *and* gives the cleaner MIS pairing (the light
sample's contribution is the specific emitter you sampled, not whatever the ray hit).

**Expected speedup.** No single universal number (it depends on occlusion rate and light size), but this is a
textbook, always-positive change: shadow/occlusion rays are typically a *large* share of total rays in a
path tracer with NEE (often ≥50%), and converting each from closest-hit-with-shading to any-hit-early-exit
removes both the "scan to closest" work and the shading-data work on that entire ray class. Expect a
meaningful double-digit-percent reduction in total trace time in shadowed scenes.

**Implementation complexity.** Medium. Add an `intersect_predicate(ray, t_max) -> bool` traversal path that
mirrors the existing traversal but returns on first hit and skips the hit-record fill. Then rework the NEE
sampler to sample a point on the emitter and call the predicate with the light distance as `t_max`. Touches
the BVH traversal (new any-hit path — can share the node-visit code) and the integrator's NEE routine.
Note: with any-hit you generally **don't** want front-to-back ordering effort — just take the first blocker,
so the any-hit loop can be even simpler than the ordered closest-hit loop.

**Does it change image output?** Two parts. The **any-hit occlusion query alone** (keeping direction
sampling) is *close to* bit-identical for the occlusion decision but can differ at the epsilon boundary
(early-exit vs closest can disagree on grazing self-intersections). **Switching to light-point-sampling NEE**
definitely **changes output** — it changes which emitter/sample contributes and the MIS weights, i.e. the
Monte Carlo estimator itself (same *expectation*, different per-sample values). **Needs re-pin.** This is the
one place where the perf win and a correctness/architecture improvement come together, so re-pinning is
justified.

**Fit.** Best impact-per-effort item for *this* integrator. It's squarely aimed at the current NEE weakness.

---

## 7. Two-level TLAS / BLAS best practices

**What it is.** Exactly the GPU-RT / DXR / Embree model your codebase already uses: a **BLAS** (bottom-level)
BVH per mesh built in object space, and a **TLAS** (top-level) BVH over instances, each instance carrying a
transform. Rays are transformed by the instance's inverse matrix into object space to test the shared BLAS,
so many instances share one BLAS ("a pair of BVHs can be combined into a single BVH by adding one new node…
we can intersect its BVH in world space by applying the inverse transform to the rays", Bikker, *Part 5:
TLAS & BLAS*, https://jacco.ompf2.com/2022/05/07/how-to-build-a-bvh-part-5-tlas-blas/).

**Best practices from the sources.**
- **Instancing:** never duplicate mesh geometry; share one BLAS across instances and vary the transform. Your
  proxy-carrying-a-stable-index TLAS is the right shape for this — the missing piece is *sharing* a BLAS
  across multiple transformed instances if you don't already.
- **Rebuild vs refit:** a **refit** updates node bounds bottom-up *without changing topology* — cheap, for
  small deformation/animation; a **rebuild** reconstructs topology — needed after large motion. "BLAS can be
  refitted for deforming meshes; the TLAS can be rebuilt each frame since it's minimal overhead" (Bikker,
  *Part 5*).
- **Why the top level is cheap to rebuild:** the TLAS has only one leaf per scene *object* (dozens–thousands
  of boxes), versus the BLAS's per-*triangle* leaves (millions). Rebuilding N object-boxes is orders of
  magnitude cheaper than rebuilding a mesh, so a full TLAS rebuild per frame is affordable; rigid instance
  motion (matrix change only) is "essentially free" — no BLAS work at all (Bikker, *Part 5*).

**Expected speedup.** This is an *architecture* property, not a per-frame % — it's what makes dynamic scenes
and instancing tractable. For a mostly-static offline path tracer the win is build-time and memory (shared
BLAS), not trace time.

**Implementation complexity.** Already largely built. Adding true multi-instance BLAS sharing is low–medium;
adding refit is low (a bottom-up bounds pass).

**Does it change image output?** BLAS sharing / refit vs rebuild are **bit-identical** as long as the final
bounds are conservative and correct. Refit produces slightly looser (still-correct) boxes than a rebuild → a
few more node visits but the same hits → bit-identical result, possibly slightly slower trace. Safe.

**Fit.** You already have the two-level structure. Low-hanging: (a) confirm BLAS reuse across instances of the
same mesh (you note meshes keep their own BVH "reused by reference" — good); (b) if scenes are static, none
of the refit/rebuild machinery is needed. This bucket is mostly "you're already doing it right"; the only new
work is instance BLAS sharing if a mesh appears multiple times.

---

## 8. Leaf / primitive level

**Triangle intersection: Möller-Trumbore vs precomputed (Woop / Baldwin-Weber).**
- **Möller-Trumbore (MT):** no per-triangle storage, computes intersection from the three vertices directly.
  Baseline; robust-ish but not watertight.
- **Woop watertight (PBRT 4e):** transforms the ray so its origin is at `(0,0,0)` and direction along `+z`,
  then does a 2D edge-function test. This "makes it possible to have a watertight ray–triangle intersection
  algorithm, such that intersections with tricky rays like those that hit the triangle right on the edge are
  never incorrectly reported as misses" (PBRT 4e, *Triangle Meshes* §,
  https://pbr-book.org/4ed/Shapes/Triangle_Meshes). Watertightness matters for path tracers: it removes the
  black-pixel "light leaks" at shared triangle edges.
- **Baldwin-Weber precomputed transform:** stores a per-triangle affine transform (to the unit triangle) and
  only transforms the *ray* at runtime. "Always faster than the standard Möller-Trumbore algorithm, and faster
  than a highly tuned modern version of it except at very high ray-triangle hit rates", at a cost of **37–48
  bytes per triangle** of precomputed data (Baldwin & Weber, *Fast Ray-Triangle Intersections by Coordinate
  Transformation*, JCGT 5(3) 2016, https://www.jcgt.org/published/0005/03/03/paper-lowres.pdf).

**Expected speedup.** Switching MT → a precomputed transform is a modest but consistent per-triangle win
(single-digit to low-double-digit %, workload-dependent), traded against extra memory (37–48 B/tri) and a
larger cache footprint per leaf. Because a BVH leaf test is *hit-rate-limited* (most box-passing triangles
still miss), the "faster except at very high hit rates" property is favorable for BVH leaves. Watertight Woop
is roughly MT-cost but buys correctness, not speed.

**Primitives per leaf (optimal per SAH).** The SAH build decides leaf size by cost, but the *cap* matters.
PBRT defaults `maxPrimsInNode = 1` and caps it at 255 ("`maxPrimsInNode(std::min(255, maxPrimsInNode))`",
PBRT 4e BVH §). Bikker's builder stops subdividing at **≤2 primitives** per leaf (*Part 1*). The SAH cost
model that drives this uses an intersection-to-traversal cost ratio; PBRT/Bikker "set the estimated
intersection cost to 1 and the traversal cost to 1/2" (Bikker *Part 1*), i.e. a leaf is worth making when
intersecting its prims is cheaper than one more traversal step. Practically, **2–4 triangles per leaf** is a
good CPU range: it lets several triangles be tested straight-line (good for SIMD / no pointer chase) while
keeping boxes tight. If leaves are currently forced to 1 primitive, allowing 2–4 usually *reduces* node count
and traversal steps with negligible extra triangle tests.

**Implementation complexity.** MT→precomputed: medium (per-triangle precompute + storage + new leaf test).
Tuning `maxPrimsInNode`: trivial (one constant + measure). Watertight Woop: medium (careful edge math).

**Does it change image output?** Any change to the triangle intersection math (MT↔Woop↔Baldwin-Weber)
changes floating-point rounding and edge behavior → **changes output, needs re-pin** (and watertight Woop
*should* change output — that's the point, it fixes edge misses). Changing `maxPrimsInNode` changes tree
topology → **needs re-pin** (same reasoning as §5).

**Fit.** If you currently use MT and see edge artifacts, watertight Woop is the correctness-motivated pick.
Baldwin-Weber is a pure-speed option if triangle memory is not a concern. Bumping leaf size from 1→2-4 is a
one-line experiment worth running.

---

## 9. Other high-value items the sources emphasize

- **Parallel build threshold.** PBRT only parallelizes subtree builds above a size threshold ("if
  `bvhPrimitives.size() > 128 * 1024`", PBRT 4e BVH §) to avoid task-spawn overhead on small subtrees. If your
  rayon build forks unconditionally, adding a serial cutoff for small subtrees reduces scheduling overhead.
  Bit-identical.
- **Allocator/arena for build.** PBRT notes that using a monotonic/arena allocator instead of general
  allocation is "approximately 10% less memory and 10% faster for complex scenes" during build (PBRT 4e BVH §).
  Rust equivalent: build into a preallocated `Vec` / bump arena rather than per-node allocation. Bit-identical,
  build-time only.
- **Leaf primitive reordering for locality.** Flattening already helps; also store each leaf's primitives
  contiguously (indexed by `first_primitive`, which you do) so a leaf test is a straight array scan. Confirm
  the triangle data (not just indices) is stored in BVH-leaf order for cache locality — an indirection through
  a scattered index array costs a cache miss per triangle.
- **RRS over SAH as the quality metric.** Bikker's recent work finds that a *ray-traversal-step* proxy ("RRS")
  predicts real GPU/CPU performance better than SAH cost alone (*Beyond SBVH*). If you add build-quality
  benchmarking, count actual traversal steps + triangle tests, don't just trust SAH cost.
- **Don't over-invest in the slab reciprocal / micro-opts.** Bikker explicitly found the precomputed-inverse
  slab test had "limited effect" once the loop is memory-bound (*Part 2*) — you already do it; further
  micro-tuning of the box test is low-yield vs the structural items (§3, §6).

---

## Prioritized for this codebase

Ranked by (impact × applicability ÷ effort). "Safe" = bit-identical, no render re-pin. "Re-pin" = changes
image output, needs a new render fingerprint.

| # | Optimization | Impact | Effort | Bit-identical? | Notes |
|---|---|---|---|---|---|
| 1 | **Any-hit early-exit shadow rays + light-point NEE (§6)** | High | Med | **Re-pin** (NEE reformulation) | Directly fixes the current full-closest-hit NEE; shadow rays are a huge share of rays. Best impact/effort. |
| 2 | **32-byte node compaction (§1)** | Med-High | Low | **Safe** | Drop `depth` from traversal node, 2-bit `split_axis`, union child/leaf fields. Two nodes per cache line. Free win. |
| 3 | **Finish traversal culling: t_max recheck on pop + pre-push cull (§2)** | Low-Med | Low | **Safe** | Ordered traversal + inv-dir already done; these are the last free traversal wins. |
| 4 | **Leaf size 1→2-4 primitives + confirm triangles stored in leaf order (§8, §9)** | Low-Med | Low | **Re-pin** (topology) / Safe (data reorder) | One-constant experiment; reorder is safe, topology change re-pins. |
| 5 | **Serial build cutoff + arena allocation (§9)** | Low (build time) | Low | **Safe** | Reduces rayon overhead + build memory; no trace/image effect. |
| 6 | **Watertight Woop triangle intersection (§8)** | Correctness + neutral speed | Med | **Re-pin** (intended) | Do this if you see edge/black-pixel artifacts. |
| 7 | **SBVH / spatial splits in the BLAS (§5)** | High (15–50% trace) | High | **Re-pin** | Biggest *quality* lever; most benefit on large/thin triangles. Measure straddling first. |
| 8 | **Wide BLAS (BVH4/BVH8 + SIMD box test) (§3)** | High | High | Re-pin (unless math matched) | Highest ceiling for incoherent path-tracer rays; only worth it after the cheap wins. TLAS stays binary. |
| 9 | **Treelet reordering / reinsertion (§5)** | Med (+10-17%) | Med-High | **Re-pin** | Post-build polish; ~90% of SBVH quality from a cheap base build. |
| 10 | **Baldwin-Weber precomputed triangle transform (§8)** | Low-Med | Med | **Re-pin** | Pure speed, +37-48 B/tri memory. Optional. |
| — | Packet / stream traversal (§4) | Low for this workload | High | Re-pin | **Skip.** Incoherent path-tracer rays defeat packets; wide single-ray (§8) is the right answer instead. |
| — | Full-sweep SAH over binned (§5) | <2% | Low | Re-pin | **Skip.** Your 12-bin binned SAH is already near-optimal quality; full sweep costs seconds of build. |

**Suggested order:** do #1–#5 first (all low-effort; #2/#3/#5 are bit-identical and can land without touching
the render test). Then decide between #7 (SBVH, quality) and #8 (wide BVH, SIMD) based on profiling — if
triangle *box tests* dominate, go wide (#8); if traversal *steps* dominate on scenes with large triangles, go
SBVH (#7).

---

## Sources

Primary / authoritative:
- PBRT 4th ed — *Bounding Volume Hierarchies*: https://pbr-book.org/4ed/Primitives_and_Intersection_Acceleration/Bounding_Volume_Hierarchies (32-B LinearBVHNode, 12 buckets, `maxPrimsInNode` cap 255, cost ratio, 128K parallel threshold, arena +10%/+10%).
- PBRT 4th ed — *Primitive Interface and Geometric Primitives*: https://pbr-book.org/4ed/Primitives_and_Intersection_Acceleration/Primitive_Interface_and_Geometric_Primitives (`IntersectP` any-hit early-exit vs `Intersect` closest-hit).
- PBRT 4th ed — *Triangle Meshes*: https://pbr-book.org/4ed/Shapes/Triangle_Meshes (watertight Woop-style ray-triangle intersection).
- Jacco Bikker, *How to build a BVH — Part 1: Basics*: https://jacco.ompf2.com/2022/04/13/how-to-build-a-bvh-part-1-basics/ (32-B node, overloaded leftFirst, half-cache-line, ≤2 prims/leaf, cost 1 : 1/2, 20× vs brute force).
- Jacco Bikker, *Part 2: Faster Rays*: https://jacco.ompf2.com/2022/04/18/how-to-build-a-bvh-part-2-faster-rays/ (SAH 183→128 ms/43%; ordered traversal 300→91 ms/3.3×; inverse-dir "limited effect"; distance culling).
- Jacco Bikker, *Part 3: Quick Builds*: https://jacco.ompf2.com/2022/04/21/how-to-build-a-bvh-part-3-quick-builds/ (binned 8 bins 111 ms vs full 109 ms; build ~11 ms; 30–90× faster build).
- Jacco Bikker, *Part 5: TLAS & BLAS*: https://jacco.ompf2.com/2022/05/07/how-to-build-a-bvh-part-5-tlas-blas/ (two-level, ray inverse-transform instancing, refit vs rebuild, cheap TLAS rebuild).
- Jacco Bikker, *BVH Quality: Beyond SBVH* (2025): https://jacco.ompf2.com/2025/05/20/bvh-quality-beyond-sbvh/ (SBVH +46% RRS Sponza, +48.9% CPU Bistro; reinsertion +17% CPU; RRS vs SAH metric).
- Stich, Friedrich, Dietrich, *Spatial Splits in Bounding Volume Hierarchies*, HPG 2009: https://www.nvidia.in/docs/IO/77714/sbvh.pdf (SBVH, reference unsplitting, alpha).
- Dammertz, Hanika, Keller, *Shallow Bounding Volume Hierarchies for Fast SIMD Ray Tracing of Incoherent Rays*, EGSR 2008 (MBVH/BVH4 collapse + SIMD box test).
- Wald, Benthin, Boulos, *Getting Rid of Packets — Efficient SIMD Single-Ray Traversal using Multi-branching BVHs*, IRT 2008: https://www.cs.cmu.edu/afs/cs/academic/class/15869-f11/www/readings/wald08_widebvh.pdf (wide single-ray SIMD beats packets for incoherent rays).
- Ylitie, Karras, Laine, *Efficient Incoherent Ray Traversal on GPUs through Compressed Wide BVHs* (CWBVH), HPG 2017: https://research.nvidia.com/publication/2017-07_efficient-incoherent-ray-traversal-gpus-through-compressed-wide-bvhs (80 B/node, 35–60% memory, 1.9–2.1× incoherent).
- Karras & Aila, *Fast Parallel Construction of High-Quality Bounding Volume Hierarchies* (TRBVH), HPG 2013: https://research.nvidia.com/publication/2013-07_fast-parallel-construction-high-quality-bounding-volume-hierarchies (treelet reordering, >90% of SBVH vs ~50% prior, 40M tris/s).
- Baldwin & Weber, *Fast Ray-Triangle Intersections by Coordinate Transformation*, JCGT 5(3) 2016: https://www.jcgt.org/published/0005/03/03/paper-lowres.pdf (precomputed transform, 37–48 B/tri, faster than MT except at very high hit rates).
- Intel Embree: https://www.embree.org/ (BVH4/BVH8 default CPU; single-ray for incoherent, packet/hybrid for coherent; ray streams).

Supporting / aggregation:
- Meister et al., *A Survey on Bounding Volume Hierarchies for Ray Tracing*, CGF 2021: https://onlinelibrary.wiley.com/doi/abs/10.1111/cgf.142662 (survey of the above).
- *Performance Comparison of Bounding Volume Hierarchies for GPU Ray Tracing*, JCGT 11(4) 2022: https://jcgt.org/published/0011/04/01/paper.pdf (build-method quality/perf comparison).
