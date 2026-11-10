//! An index over the ring of an owner `OutRec`, answering `PointInOpPolygon` without walking it.
//!
//! Not part of Clipper2: `RecursiveCheckOwners` asks whether each ring lies inside its owner, and
//! every question walks the owner's whole ring. A copper pour is one ring of many thousands of
//! points owning thousands of holes, which makes that step quadratic. The rings are final by
//! then (`CheckBounds` has cleaned them), so the edges can be bucketed by y once.
//!
//! The answer is the walk's answer. `PointInOpPolygon` returns `IsOn` when any vertex or edge
//! meets the point, and otherwise the parity of the edges that take the ring from one side of
//! the scanline to the other, neither of which depends on where the walk starts. Debug builds
//! compare every answer with the walk.

use super::types::*;
use crate::core::{Point64, PointInPolygonResult, cross_product_sign_t};

/// Rings smaller than this are walked.
const MIN_RING_POINTS: usize = 64;

/// Bucket entries allowed per edge; rings of long edges get fewer, taller buckets.
const MAX_ENTRIES_PER_EDGE: usize = 8;

#[derive(Clone, Copy)]
struct Edge {
    /// `op.prev`'s point.
    p: Point64,
    /// `op`'s point.
    c: Point64,
    op: Idx,
    y_lo: i64,
    y_hi: i64,
}

/// What is known about an owner so far. An owner asked about once is not worth an index.
pub(crate) enum OwnerRing {
    Seen,
    Walk,
    Indexed(Box<RingIndex>),
}

pub(crate) struct RingIndex {
    y_min: i64,
    y_max: i64,
    bucket_height: u64,
    /// `entries[offsets[b]..offsets[b + 1]]` are the edges touching bucket `b`.
    offsets: Vec<u32>,
    entries: Vec<u32>,
    edges: Vec<Edge>,
}

impl RingIndex {
    /// The index of the ring through `start`, or `None` when the walk should answer: a small
    /// ring, or one the walk treats specially (fewer than three points, all on one scanline).
    pub(crate) fn build(outpts: &[OutPt], start: Idx) -> Option<RingIndex> {
        let first = &outpts[start as usize];
        if start == first.next || first.prev == first.next {
            return None;
        }

        let mut edges = Vec::new();
        let (mut y_min, mut y_max) = (i64::MAX, i64::MIN);
        let mut op = start;
        loop {
            let o = &outpts[op as usize];
            let p = outpts[o.prev as usize].pt;
            edges.push(Edge {
                p,
                c: o.pt,
                op,
                y_lo: p.y.min(o.pt.y),
                y_hi: p.y.max(o.pt.y),
            });
            y_min = y_min.min(o.pt.y);
            y_max = y_max.max(o.pt.y);
            op = o.next;
            if op == start {
                break;
            }
        }
        if edges.len() < MIN_RING_POINTS || y_min == y_max {
            return None;
        }

        let range = y_max.wrapping_sub(y_min) as u64;
        let bucket_of = |y: i64, h: u64| (y.wrapping_sub(y_min) as u64 / h) as usize;
        let mut buckets = edges.len();
        let (bucket_height, total) = loop {
            let h = range / buckets as u64 + 1;
            let total: usize = edges
                .iter()
                .map(|e| bucket_of(e.y_hi, h) - bucket_of(e.y_lo, h) + 1)
                .sum();
            if buckets == 1 || total <= edges.len() * MAX_ENTRIES_PER_EDGE {
                break (h, total);
            }
            buckets = (buckets / 4).max(1);
        };

        let mut offsets = vec![0u32; buckets + 1];
        for e in &edges {
            for b in bucket_of(e.y_lo, bucket_height)..=bucket_of(e.y_hi, bucket_height) {
                offsets[b + 1] += 1;
            }
        }
        for b in 0..buckets {
            offsets[b + 1] += offsets[b];
        }
        let mut next = offsets.clone();
        let mut entries = vec![0u32; total];
        for (i, e) in edges.iter().enumerate() {
            for b in bucket_of(e.y_lo, bucket_height)..=bucket_of(e.y_hi, bucket_height) {
                entries[next[b] as usize] = i as u32;
                next[b] += 1;
            }
        }

        Some(RingIndex {
            y_min,
            y_max,
            bucket_height,
            offsets,
            entries,
            edges,
        })
    }

    /// `PointInOpPolygon` of `pt` against the indexed ring.
    pub(crate) fn locate(&self, outpts: &[OutPt], pt: Point64) -> PointInPolygonResult {
        if pt.y < self.y_min || pt.y > self.y_max {
            return PointInPolygonResult::IsOutside;
        }

        let b = (pt.y.wrapping_sub(self.y_min) as u64 / self.bucket_height) as usize;
        let range = self.offsets[b] as usize..self.offsets[b + 1] as usize;
        let mut inside = false;

        for &i in &self.entries[range] {
            let e = &self.edges[i as usize];
            if pt.y < e.y_lo || pt.y > e.y_hi {
                continue;
            }
            let (p, c) = (e.p, e.c);

            if c.y == pt.y {
                if c.x == pt.x || (c.y == p.y && ((pt.x < p.x) != (pt.x < c.x))) {
                    return PointInPolygonResult::IsOn;
                }
                continue;
            }

            // The side the ring was on before arriving at c: for an edge leaving the scanline,
            // the side it was on before it reached the scanline.
            let was_above = if p.y != pt.y {
                p.y < pt.y
            } else {
                let mut q = outpts[e.op as usize].prev;
                while outpts[q as usize].pt.y == pt.y {
                    q = outpts[q as usize].prev;
                }
                outpts[q as usize].pt.y < pt.y
            };
            if was_above == (c.y < pt.y) {
                continue;
            }

            if pt.x < c.x && pt.x < p.x {
                // no crossing
            } else if pt.x > p.x && pt.x > c.x {
                inside = !inside;
            } else {
                let i = cross_product_sign_t(p, c, pt);
                if i == 0 {
                    return PointInPolygonResult::IsOn;
                }
                if (i < 0) == was_above {
                    inside = !inside;
                }
            }
        }

        if inside {
            PointInPolygonResult::IsInside
        } else {
            PointInPolygonResult::IsOutside
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ClipperBase;
    use super::*;

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n
        }
    }

    /// The ring as `OutPt`s, in the engine that owns them.
    fn engine_with_ring(points: &[(i64, i64)]) -> ClipperBase {
        let mut engine = ClipperBase::default();
        let n = points.len() as Idx;
        for (i, &(x, y)) in points.iter().enumerate() {
            let i = i as Idx;
            engine.outpts.push(OutPt {
                pt: Point64::new(x, y),
                next: (i + 1) % n,
                prev: (i + n - 1) % n,
                outrec: 0,
                horz: NONE,
            });
        }
        engine
    }

    /// Small coordinate ranges so that vertices and edges land on the queried points and
    /// scanlines all the time: self-touching, horizontal runs and repeated points included.
    #[test]
    fn locate_answers_as_the_walk_does() {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        let (mut indexed, mut on, mut inside) = (0, 0, 0);

        for round in 0..3000 {
            let span = [4, 8, 16, 64, 1 << 40][round % 5];
            let len = MIN_RING_POINTS + rng.below(200) as usize;
            let flat = rng.below(4) == 0;
            let mut last = (0, 0);
            let points: Vec<(i64, i64)> = (0..len)
                .map(|_| {
                    let x = rng.below(span) as i64 - span as i64 / 2;
                    let y = rng.below(span) as i64 - span as i64 / 2;
                    last = if flat && rng.below(2) == 0 {
                        (x, last.1)
                    } else {
                        (x, y)
                    };
                    last
                })
                .collect();

            let engine = engine_with_ring(&points);
            let start = rng.below(len as u64) as Idx;
            let Some(index) = RingIndex::build(&engine.outpts, start) else {
                continue;
            };
            indexed += 1;

            for q in 0..600 {
                let pt = if q % 3 == 0 {
                    let (x, y) = points[rng.below(len as u64) as usize];
                    let dx = rng.below(3) as i64 - 1;
                    Point64::new(x + dx, y)
                } else {
                    Point64::new(
                        rng.below(span + 4) as i64 - span as i64 / 2 - 2,
                        rng.below(span + 4) as i64 - span as i64 / 2 - 2,
                    )
                };
                let walk = engine.point_in_op_polygon(pt, start);
                assert_eq!(
                    index.locate(&engine.outpts, pt),
                    walk,
                    "round {round} {pt:?}"
                );
                match walk {
                    PointInPolygonResult::IsOn => on += 1,
                    PointInPolygonResult::IsInside => inside += 1,
                    PointInPolygonResult::IsOutside => {}
                }
            }
        }

        assert!(
            indexed > 2000 && on > 10_000 && inside > 10_000,
            "{indexed} {on} {inside}"
        );
    }

    #[test]
    fn small_and_flat_rings_are_left_to_the_walk() {
        let small: Vec<_> = (0..MIN_RING_POINTS as i64 - 1)
            .map(|i| (i, i * i))
            .collect();
        assert!(RingIndex::build(&engine_with_ring(&small).outpts, 0).is_none());

        let flat: Vec<_> = (0..MIN_RING_POINTS as i64 * 2).map(|i| (i, 7)).collect();
        assert!(RingIndex::build(&engine_with_ring(&flat).outpts, 0).is_none());
    }
}
