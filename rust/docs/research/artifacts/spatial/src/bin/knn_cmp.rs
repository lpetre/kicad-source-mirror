//! Research tool: KiCad's nanoflann usage vs Rust candidates.
//!   knn_cmp gen <cases.txt> [seed]
//!   knn_cmp compare <cases.txt> <cpp.out>
use rstar::primitives::GeomWithData;
use spatial_research::nanoflann_port::KdTree;
use spatial_research::rng::Rng;
use spatial_research::stdsort::std_sort;
use std::collections::BTreeMap;
use std::fmt::Write as _;

struct Case {
    label: &'static str,
    pts: Vec<(i32, i32)>,
    queries: Vec<(i32, i32, i64)>,
}

fn generate(seed: u64) -> Vec<Case> {
    let mut rng = Rng::new(seed);
    let mut cases = Vec::new();
    let eps_set = [1i64, 2, 5, 100, 1000, 20_000, 1_000_000];

    let add_queries = |rng: &mut Rng, pts: &[(i32, i32)]| -> Vec<(i32, i32, i64)> {
        let mut q = Vec::new();
        if pts.is_empty() {
            return q;
        }
        for _ in 0..40 {
            let p = pts[rng.range(0, pts.len() as i64 - 1) as usize];
            let eps = eps_set[rng.range(0, eps_set.len() as i64 - 1) as usize];
            q.push((p.0, p.1, eps));
        }
        q
    };

    // 1. board-outline like: closed polygons made of segments (2 endpoints per shape), with
    //    gaps, nm-scale near misses, dangling segments and T junctions
    for _ in 0..400 {
        let mut pts: Vec<(i32, i32)> = Vec::new();
        let polys = rng.range(1, 6);
        let grid = [1_000_000i64, 500_000, 100_000, 1][rng.range(0, 3) as usize];
        for _ in 0..polys {
            let k = rng.range(3, 40);
            let cx = rng.range(10, 200) * 1_000_000;
            let cy = rng.range(10, 200) * 1_000_000;
            let r = rng.range(2, 40) * 1_000_000;
            // vertices roughly around a centre, snapped to a grid
            let verts: Vec<(i64, i64)> = (0..k)
                .map(|i| {
                    // octagon-ish walk without trigonometry
                    let side = (i * 8 / k) as usize;
                    let t = rng.range(0, r);
                    let (x, y) = match side {
                        0 => (cx - r + t, cy - r),
                        1 => (cx + t, cy - r),
                        2 => (cx + r, cy - r + t),
                        3 => (cx + r, cy + t),
                        4 => (cx + r - t, cy + r),
                        5 => (cx - t, cy + r),
                        6 => (cx - r, cy + r - t),
                        _ => (cx - r, cy - t),
                    };
                    ((x / grid) * grid, (y / grid) * grid)
                })
                .collect();
            let skip = if rng.chance(1, 2) { rng.range(0, k - 1) } else { -1 };
            for i in 0..k {
                if i == skip {
                    continue; // a gap in the outline
                }
                let a = verts[i as usize];
                let mut b = verts[((i + 1) % k) as usize];
                if rng.chance(1, 10) {
                    // near miss: the end point is a few nm away from the next start point
                    let d = [1i64, 2, 3, 50, 999][rng.range(0, 4) as usize];
                    b = (b.0 + rng.range(-d, d), b.1 + rng.range(-d, d));
                }
                pts.push((a.0 as i32, a.1 as i32));
                pts.push((b.0 as i32, b.1 as i32));
                if rng.chance(1, 12) {
                    // dangling segment starting exactly on a vertex (T junction)
                    pts.push((a.0 as i32, a.1 as i32));
                    pts.push(((a.0 + rng.range(-3, 3) * 1_000_000) as i32, (a.1 + rng.range(-3, 3) * 1_000_000) as i32));
                }
            }
        }
        let queries = add_queries(&mut rng, &pts);
        cases.push(Case { label: "outline_like", pts, queries });
    }

    // 2. random end points on a coarse grid: many exact duplicates and equal distances
    for _ in 0..300 {
        let n = rng.range(1, 400) as usize * 2;
        let k = rng.range(1, 30);
        let step = [1i64, 1000, 1_000_000][rng.range(0, 2) as usize];
        let pts: Vec<(i32, i32)> =
            (0..n).map(|_| ((rng.range(0, k) * step) as i32, (rng.range(0, k) * step) as i32)).collect();
        let mut queries = add_queries(&mut rng, &pts);
        // exact boundary: radius equal to an existing distance
        for _ in 0..10 {
            let p = pts[rng.range(0, pts.len() as i64 - 1) as usize];
            queries.push((p.0, p.1, step * rng.range(1, 5)));
            queries.push((p.0, p.1, 5 * step)); // 3-4-5 triangles
        }
        cases.push(Case { label: "grid_dups", pts, queries });
    }

    // 3. degenerate: all points share x (or y); > 32 equal coordinates on one axis
    for _ in 0..100 {
        let n = rng.range(1, 300) as usize * 2;
        let vertical = rng.chance(1, 2);
        let pts: Vec<(i32, i32)> = (0..n)
            .map(|_| {
                let t = (rng.range(0, 200) * 250_000) as i32;
                if vertical { (5_000_000, t) } else { (t, -7_000_000) }
            })
            .collect();
        let queries = add_queries(&mut rng, &pts);
        cases.push(Case { label: "one_axis", pts, queries });
    }

    // 4. uniform random
    for _ in 0..200 {
        let n = rng.range(1, 1500) as usize * 2;
        let pts: Vec<(i32, i32)> =
            (0..n).map(|_| (rng.range(-300_000_000, 300_000_000) as i32, rng.range(-300_000_000, 300_000_000) as i32)).collect();
        let mut queries = add_queries(&mut rng, &pts);
        for _ in 0..10 {
            queries.push((rng.range(-300_000_000, 300_000_000) as i32, rng.range(-300_000_000, 300_000_000) as i32, 50_000_000));
        }
        cases.push(Case { label: "uniform", pts, queries });
    }
    cases
}

fn write_cases(path: &str, cases: &[Case]) {
    let mut s = String::new();
    writeln!(s, "{}", cases.len()).unwrap();
    for c in cases {
        writeln!(s, "{} {}", c.pts.len(), c.queries.len()).unwrap();
        for p in &c.pts {
            writeln!(s, "{} {}", p.0, p.1).unwrap();
        }
        for q in &c.queries {
            writeln!(s, "{} {} {}", q.0, q.1, q.2).unwrap();
        }
    }
    std::fs::write(path, s).unwrap();
}

#[derive(Default)]
struct CppQuery {
    r: Vec<u32>,
    u: Vec<u32>,
    k: Vec<u32>,
}

fn parse_cpp(path: &str) -> Vec<Vec<CppQuery>> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out: Vec<Vec<CppQuery>> = Vec::new();
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        let tag = it.next().unwrap();
        if tag == "case" {
            out.push(Vec::new());
            continue;
        }
        let n: usize = it.next().unwrap().parse().unwrap();
        let v: Vec<u32> = it.map(|x| x.parse().unwrap()).collect();
        assert_eq!(v.len(), n);
        let c = out.last_mut().unwrap();
        match tag {
            "R" => c.push(CppQuery { r: v, ..Default::default() }),
            "U" => c.last_mut().unwrap().u = v,
            "K" => c.last_mut().unwrap().k = v,
            _ => panic!(),
        }
    }
    out
}

/// KiCad's `closerEndpoint` order.
fn canonical(mut v: Vec<(u32, f64)>) -> Vec<u32> {
    v.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap().then(a.0.cmp(&b.0)));
    v.into_iter().map(|p| p.0).collect()
}

/// What `report_gap` in ConvertOutlineToPolygon derives from the knn result:
/// shapeA = shape of indices[0], shapeB = first later index of a different shape (else shapeA).
fn gap_pair(k: &[u32]) -> Option<(u32, u32)> {
    let a = *k.first()? / 2;
    let b = k.iter().skip(1).map(|i| i / 2).find(|&s| s != a).unwrap_or(a);
    Some((a, b))
}

fn d2(p: (i32, i32), q: (i32, i32)) -> f64 {
    // same expression as L2_Simple_Adaptor::evalMetric
    let dx = q.0 as f64 - p.0 as f64;
    let dy = q.1 as f64 - p.1 as f64;
    let mut r = 0.0;
    r += dx * dx;
    r += dy * dy;
    r
}

fn compare(cases: &[Case], cpp: &[Vec<CppQuery>]) {
    // name -> [total, equal]
    let mut t: BTreeMap<String, [usize; 2]> = BTreeMap::new();
    let mut add = |name: &str, label: &str, ok: bool| {
        for key in [format!("{name:<52} {label}"), format!("{name:<52} ALL")] {
            let e = t.entry(key).or_default();
            e[0] += 1;
            if ok {
                e[1] += 1;
            }
        }
    };
    let mut kiddo_panics = 0usize;

    for (case, cq) in cases.iter().zip(cpp.iter()) {
        let ptsf: Vec<[f64; 2]> = case.pts.iter().map(|p| [p.0 as f64, p.1 as f64]).collect();
        let port = KdTree::new(ptsf.clone());

        // kd-tree crate
        let kd_items: Vec<([f64; 2], u32)> = ptsf.iter().copied().zip(0u32..).collect();
        let kdt = kd_tree::KdTree::build_by_ordered_float(
            kd_items.iter().map(|(p, i)| KdItem { p: *p, id: *i }).collect::<Vec<_>>(),
        );
        // rstar
        let rst = rstar::RTree::bulk_load(
            ptsf.iter().copied().zip(0u32..).map(|(p, i)| GeomWithData::new(p, i)).collect::<Vec<_>>(),
        );
        // kiddo
        let kid = std::panic::catch_unwind(|| kiddo::ImmutableKdTree::<f64, 2>::new_from_slice(&ptsf));
        if kid.is_err() {
            kiddo_panics += 1;
        }

        for (q, c) in case.queries.iter().zip(cq.iter()) {
            let qp = (q.0, q.1);
            let qf = [q.0 as f64, q.1 as f64];
            let eps = q.2 as f64;
            let r2 = eps * eps;

            // --- hand port
            let raw = port.radius_search_unsorted(&qf, r2);
            add("PORT radius (KiCad canonical order)", case.label, canonical(raw.clone()) == c.r);
            let mut sorted_like_nanoflann = raw.clone();
            std_sort(&mut sorted_like_nanoflann, |a, b| a.1 < b.1);
            add(
                "PORT radius (nanoflann's own order, std::sort port)",
                case.label,
                sorted_like_nanoflann.iter().map(|p| p.0).collect::<Vec<_>>() == c.u,
            );
            let k: Vec<u32> = port.knn_search(&qf, 8).into_iter().map(|p| p.0).collect();
            add("PORT knn(8) order", case.label, k == c.k);

            // --- brute force
            let all: Vec<(u32, f64)> = case.pts.iter().zip(0u32..).map(|(p, i)| (i, d2(qp, *p))).collect();
            let within: Vec<(u32, f64)> = all.iter().copied().filter(|p| p.1 < r2).collect();
            add("BRUTE radius (KiCad canonical order)", case.label, canonical(within) == c.r);
            let bk: Vec<u32> = canonical(all.clone()).into_iter().take(8).collect();
            add("BRUTE knn(8), ties by index: same order", case.label, bk == c.k);
            add("BRUTE knn(8), ties by index: same (shapeA, shapeB)", case.label, gap_pair(&bk) == gap_pair(&c.k));

            // --- kd-tree crate
            let w: Vec<(u32, f64)> =
                kdt.within_radius(&KdItem { p: qf, id: 0 }, eps).into_iter().map(|it| (it.id, d2(qp, case.pts[it.id as usize]))).collect();
            add("kd-tree 0.6.2 within_radius (canonicalised)", case.label, canonical(w) == c.r);
            let kk: Vec<u32> = kdt.nearests(&KdItem { p: qf, id: 0 }, 8).into_iter().map(|n| n.item.id).collect();
            add("kd-tree 0.6.2 nearests(8): same order", case.label, kk == c.k);
            add("kd-tree 0.6.2 nearests(8): same (shapeA, shapeB)", case.label, gap_pair(&kk) == gap_pair(&c.k));

            // --- rstar
            let w: Vec<(u32, f64)> =
                rst.locate_within_distance(qf, r2).map(|o| (o.data, d2(qp, case.pts[o.data as usize]))).collect();
            add("rstar locate_within_distance (<=, canonicalised)", case.label, canonical(w.clone()) == c.r);
            let w_strict: Vec<(u32, f64)> = w.into_iter().filter(|p| p.1 < r2).collect();
            add("rstar locate_within_distance + strict filter", case.label, canonical(w_strict) == c.r);
            let rk: Vec<u32> = rst.nearest_neighbor_iter(qf).take(8).map(|o| o.data).collect();
            add("rstar nearest_neighbor_iter(8): same order", case.label, rk == c.k);
            add("rstar nearest_neighbor_iter(8): same (shapeA, shapeB)", case.label, gap_pair(&rk) == gap_pair(&c.k));

            // --- kiddo
            if let Ok(Ok(tree)) = &kid {
                let res = tree
                    .query(&qf)
                    .within::<kiddo::SquaredEuclidean<f64>>(r2)
                    .exclusive_boundaries()
                    .execute();
                let w: Vec<(u32, f64)> = res.iter().map(|n| (n.item as u32, d2(qp, case.pts[n.item as usize]))).collect();
                add("kiddo 6.3.0 within (exclusive, canonicalised)", case.label, canonical(w) == c.r);
                let kn = tree
                    .query(&qf)
                    .nearest_n::<kiddo::SquaredEuclidean<f64>>(std::num::NonZero::new(8usize).unwrap())
                    .execute();
                let kk: Vec<u32> = kn.iter().map(|n| n.item as u32).collect();
                add("kiddo 6.3.0 nearest_n(8): same order", case.label, kk == c.k);
                add("kiddo 6.3.0 nearest_n(8): same (shapeA, shapeB)", case.label, gap_pair(&kk) == gap_pair(&c.k));
            }
        }
    }
    println!("kiddo ImmutableKdTree construction panics: {kiddo_panics} of {} cases", cases.len());
    println!("{:<52} {:<14} {:>8} {:>8}", "candidate / check", "category", "queries", "equal");
    for (k, v) in &t {
        println!("{k:<67} {:>8} {:>8}", v[0], v[1]);
    }
}

#[derive(Clone, Copy)]
struct KdItem {
    p: [f64; 2],
    id: u32,
}

impl kd_tree::KdPoint for KdItem {
    type Scalar = f64;
    type Dim = typenum::U2;
    fn at(&self, k: usize) -> f64 {
        self.p[k]
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let seed_arg = if args.get(1).map(|s| s.as_str()) == Some("gen") { 3 } else { 4 };
    let seed = args.get(seed_arg).map(|s| s.parse::<u64>().unwrap()).unwrap_or(0x4b4e_4e00);
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => {
            let cases = generate(seed);
            write_cases(&args[2], &cases);
            println!(
                "{} cases, {} points, {} queries",
                cases.len(),
                cases.iter().map(|c| c.pts.len()).sum::<usize>(),
                cases.iter().map(|c| c.queries.len()).sum::<usize>()
            );
        }
        Some("compare") => compare(&generate(seed), &parse_cpp(&args[3])),
        _ => eprintln!("usage"),
    }
}
