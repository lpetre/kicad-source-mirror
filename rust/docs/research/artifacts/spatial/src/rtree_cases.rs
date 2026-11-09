//! Deterministic generator of R-tree test cases (integer arithmetic only, so that it produces
//! the same cases natively and on wasm32).

use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub struct B {
    pub min: [i32; 3],
    pub max: [i32; 3],
}

pub struct Case {
    pub label: &'static str,
    pub dim: usize,
    pub items: Vec<B>,
    pub queries: Vec<B>,
}

fn mk(dim: usize, x0: i64, y0: i64, x1: i64, y1: i64, l0: i64, l1: i64) -> B {
    let c = |v: i64| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    if dim == 2 {
        B { min: [c(x0.min(x1)), c(y0.min(y1)), 0], max: [c(x0.max(x1)), c(y0.max(y1)), 0] }
    } else {
        // CN_RTREE layout: { layer, x, y }
        B {
            min: [c(l0.min(l1)), c(x0.min(x1)), c(y0.min(y1))],
            max: [c(l0.max(l1)), c(x0.max(x1)), c(y0.max(y1))],
        }
    }
}

fn gen_queries(rng: &mut Rng, dim: usize, items: &[B], lo: i64, hi: i64, nq: usize) -> Vec<B> {
    let mut q = Vec::new();
    for k in 0..nq {
        match k % 5 {
            // an item's own box (CN_LIST::FindNearby, DRC item queries)
            0 if !items.is_empty() => {
                q.push(items[rng.range(0, items.len() as i64 - 1) as usize]);
            }
            // an item's box inflated by a clearance
            1 if !items.is_empty() => {
                let mut b = items[rng.range(0, items.len() as i64 - 1) as usize];
                let infl = rng.range(0, 2_000_000) as i32;
                let first = if dim == 3 { 1 } else { 0 };
                for d in first..dim {
                    b.min[d] = b.min[d].saturating_sub(infl);
                    b.max[d] = b.max[d].saturating_add(infl);
                }
                q.push(b);
            }
            // point query
            2 => {
                let (x, y) = (rng.range(lo, hi), rng.range(lo, hi));
                let l = rng.range(0, 31);
                q.push(mk(dim, x, y, x, y, l, l));
            }
            // everything
            3 if k == 3 => {
                q.push(mk(dim, i32::MIN as i64, i32::MIN as i64, i32::MAX as i64, i32::MAX as i64, 0, 63));
            }
            // random window
            _ => {
                let span = (hi - lo).max(1);
                let dw = [1, 2, 10, 100][rng.range(0, 3) as usize];
                let dh = [1, 2, 10, 100][rng.range(0, 3) as usize];
                let w = rng.range(0, span / dw);
                let h = rng.range(0, span / dh);
                let (x, y) = (rng.range(lo, hi), rng.range(lo, hi));
                let l0 = rng.range(0, 31);
                let l1 = l0 + rng.range(0, 31);
                q.push(mk(dim, x, y, x + w, y + h, l0, l1));
            }
        }
    }
    q
}

pub fn generate(seed: u64) -> Vec<Case> {
    let mut rng = Rng::new(seed);
    let mut cases = Vec::new();
    let nq = 25;

    for dim in [2usize, 3] {
        // tiny trees incl. the node-boundary sizes
        for n in [0usize, 1, 2, 3, 15, 16, 17, 31, 32, 33, 255, 256, 257, 272, 273, 4096, 4097] {
            let items: Vec<B> = (0..n)
                .map(|_| {
                    let (x, y) = (rng.range(0, 100_000_000), rng.range(0, 100_000_000));
                    let l = rng.range(0, 31);
                    mk(dim, x, y, x + rng.range(0, 3_000_000), y + rng.range(0, 3_000_000), l, l + rng.range(0, 3))
                })
                .collect();
            let queries = gen_queries(&mut rng, dim, &items, 0, 100_000_000, nq);
            cases.push(Case { label: "sizes", dim, items, queries });
        }

        // board-like: small boxes (pads, vias, track segments)
        for _ in 0..120 {
            let n = rng.range(1, 3000) as usize;
            let ext = [20_000_000i64, 100_000_000, 300_000_000][rng.range(0, 2) as usize];
            let items: Vec<B> = (0..n)
                .map(|_| {
                    let (x, y) = (rng.range(0, ext), rng.range(0, ext));
                    let (w, h) = if rng.chance(1, 2) {
                        (rng.range(100_000, 2_000_000), rng.range(100_000, 2_000_000))
                    } else if rng.chance(1, 2) {
                        (rng.range(100_000, 40_000_000), rng.range(100_000, 400_000))
                    } else {
                        (rng.range(100_000, 400_000), rng.range(100_000, 40_000_000))
                    };
                    let l = if rng.chance(1, 5) { 0 } else { rng.range(0, 5) };
                    let l1 = if rng.chance(1, 5) { 31 } else { l };
                    mk(dim, x, y, x + w, y + h, l, l1)
                })
                .collect();
            let queries = gen_queries(&mut rng, dim, &items, 0, ext, nq);
            cases.push(Case { label: "board_like", dim, items, queries });
        }

        // heavy Hilbert ties: many items share exactly the same bbox centre
        // (pad + its hole, via on pad, concentric shapes, the two triangles of a rectangle)
        for _ in 0..120 {
            let n = rng.range(2, 1500) as usize;
            let ncent = rng.range(1, (n as i64 / 2).max(1));
            let centres: Vec<(i64, i64)> = (0..ncent)
                .map(|_| (rng.range(0, 2000) * 100_000, rng.range(0, 2000) * 100_000))
                .collect();
            let items: Vec<B> = (0..n)
                .map(|_| {
                    let (cx, cy) = centres[rng.range(0, ncent - 1) as usize];
                    let (hw, hh) = (rng.range(0, 30) * 50_000, rng.range(0, 30) * 50_000);
                    let l = rng.range(0, 3);
                    mk(dim, cx - hw, cy - hh, cx + hw, cy + hh, l, l + rng.range(0, 2))
                })
                .collect();
            let queries = gen_queries(&mut rng, dim, &items, 0, 200_000_000, nq);
            cases.push(Case { label: "centre_ties", dim, items, queries });
        }

        // identical boxes / a handful of distinct boxes repeated
        for _ in 0..40 {
            let n = rng.range(1, 600) as usize;
            let kinds = rng.range(1, 4);
            let protos: Vec<B> = (0..kinds)
                .map(|_| {
                    let (x, y) = (rng.range(0, 1_000_000), rng.range(0, 1_000_000));
                    mk(dim, x, y, x + rng.range(0, 500_000), y + rng.range(0, 500_000), 0, rng.range(0, 31))
                })
                .collect();
            let items: Vec<B> = (0..n).map(|_| protos[rng.range(0, kinds - 1) as usize]).collect();
            let queries = gen_queries(&mut rng, dim, &items, 0, 1_500_000, nq);
            cases.push(Case { label: "identical", dim, items, queries });
        }

        // triangulated zone like: a grid of cells, two "triangles" (same bbox) per cell
        for _ in 0..40 {
            let (w, h) = (rng.range(1, 40), rng.range(1, 40));
            let cell = rng.range(1, 20) * 100_000;
            let mut items = Vec::new();
            for i in 0..w {
                for j in 0..h {
                    let b = mk(dim, i * cell, j * cell, (i + 1) * cell, (j + 1) * cell, 0, 0);
                    items.push(b);
                    items.push(b);
                }
            }
            let queries = gen_queries(&mut rng, dim, &items, 0, 40 * cell, nq);
            cases.push(Case { label: "tri_pairs", dim, items, queries });
        }

        // negative / mixed-sign coordinates and zero-size boxes (points)
        for _ in 0..60 {
            let n = rng.range(1, 800) as usize;
            let r = [1000i64, 1_000_000, 1_000_000_000][rng.range(0, 2) as usize];
            let items: Vec<B> = (0..n)
                .map(|_| {
                    let (x, y) = (rng.range(-r, r), rng.range(-r, r));
                    if rng.chance(1, 3) {
                        mk(dim, x, y, x, y, rng.range(0, 31), rng.range(0, 31))
                    } else {
                        mk(dim, x, y, x + rng.range(0, r / 10), y + rng.range(0, r / 10), 0, rng.range(0, 31))
                    }
                })
                .collect();
            let queries = gen_queries(&mut rng, dim, &items, -r, r, nq);
            cases.push(Case { label: "neg_points", dim, items, queries });
        }
    }

    // full int range (2D packed tree only matters here; exercises the centre / normalisation code)
    for _ in 0..40 {
        let n = rng.range(1, 500) as usize;
        let items: Vec<B> = (0..n)
            .map(|_| {
                let x0 = rng.range(i32::MIN as i64, i32::MAX as i64);
                let y0 = rng.range(i32::MIN as i64, i32::MAX as i64);
                let x1 = rng.range(i32::MIN as i64, i32::MAX as i64);
                let y1 = rng.range(i32::MIN as i64, i32::MAX as i64);
                mk(2, x0, y0, x1, y1, 0, 0)
            })
            .collect();
        let queries = gen_queries(&mut rng, 2, &items, i32::MIN as i64, i32::MAX as i64, nq);
        cases.push(Case { label: "full_int_range", dim: 2, items, queries });
    }

    cases
}

