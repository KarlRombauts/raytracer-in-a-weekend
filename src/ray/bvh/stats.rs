//! Deterministic BVH traversal-work counters — **node box tests** (AABB
//! intersection tests) and **leaf primitive tests** (`prim.intersect` attempts).
//!
//! Compiled only under the `bvh-stats` feature; the increments in
//! [`closest_hit`](super::flat_bvh) are `#[cfg]`-gated, so a normal build (and
//! the timing benches) carry zero cost and identical codegen. Counts are a
//! hardware-independent measure of BVH quality: a bit-identical layout change
//! leaves them unchanged while ns/ray drops, whereas a tree-quality change moves
//! them.
//!
//! Counters are thread-local, so the intended use is a single-threaded
//! diagnostic run (fire a ray battery on one thread, then [`snapshot`]).

use std::cell::Cell;

thread_local! {
    static BOX_TESTS: Cell<u64> = const { Cell::new(0) };
    static PRIM_TESTS: Cell<u64> = const { Cell::new(0) };
}

/// Record one node bounding-box (AABB) intersection test.
#[inline(always)]
pub fn count_box() {
    BOX_TESTS.with(|c| c.set(c.get() + 1));
}

/// Record one leaf primitive intersection test.
#[inline(always)]
pub fn count_primitive() {
    PRIM_TESTS.with(|c| c.set(c.get() + 1));
}

/// Zero both counters for the current thread.
pub fn reset() {
    BOX_TESTS.with(|c| c.set(0));
    PRIM_TESTS.with(|c| c.set(0));
}

/// `(node box tests, leaf primitive tests)` on this thread since the last
/// [`reset`].
pub fn snapshot() -> (u64, u64) {
    (BOX_TESTS.with(Cell::get), PRIM_TESTS.with(Cell::get))
}
