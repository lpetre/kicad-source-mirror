//! Research tool: compare KiCad's own KIRTREE (C++ driver output) with
//!  * the hand port of PACKED_RTREE (exact visit order),
//!  * rstar, static_aabb2d_index and geo-index (result sets, and how often the order matches).
//!
//!   rtree_cmp gen <cases.txt> [seed]
//!   rtree_cmp compare <cases.txt> <cpp.out>

use geo_index::rtree::sort::HilbertSort;
use geo_index::rtree::{RTreeBuilder, RTreeIndex};
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};
use spatial_research::packed_rtree;
use static_aabb2d_index::StaticAABB2DIndexBuilder;
use std::collections::BTreeMap;
use std::fmt::Write as _;

use spatial_research::rtree_cases::{B, Case, generate};

fn write_cases(path: &str, cases: &[Case]) {
    let mut s = String::new();
    writeln!(s, "{}", cases.len()).unwrap();
    let mut l = String::new();
    for c in cases {
        writeln!(l, "{}", c.label).unwrap();
        writeln!(s, "{} {} {}", c.dim, c.items.len(), c.queries.len()).unwrap();
        for b in c.items.iter().chain(c.queries.iter()) {
            for d in 0..c.dim {
                write!(s, "{} ", b.min[d]).unwrap();
            }
            for d in 0..c.dim {
                write!(s, "{} ", b.max[d]).unwrap();
            }
            s.push('\n');
        }
    }
    std::fs::write(path, s).unwrap();
    std::fs::write(format!("{path}.labels"), l).unwrap();
}

fn read_cases(path: &str) -> Vec<Case> {
    let text = std::fs::read_to_string(path).unwrap();
    let labels = std::fs::read_to_string(format!("{path}.labels")).unwrap();
    let mut it = text.split_ascii_whitespace().map(|t| t.parse::<i64>().unwrap());
    let ncases = it.next().unwrap() as usize;
    let mut out = Vec::new();
    for label in labels.lines().take(ncases) {
        let dim = it.next().unwrap() as usize;
        let n = it.next().unwrap() as usize;
        let nq = it.next().unwrap() as usize;
        let mut rd = |k: usize| -> Vec<B> {
            (0..k)
                .map(|_| {
                    let mut b = B { min: [0; 3], max: [0; 3] };
                    for d in 0..dim {
                        b.min[d] = it.next().unwrap() as i32;
                    }
                    for d in 0..dim {
                        b.max[d] = it.next().unwrap() as i32;
                    }
                    b
                })
                .collect()
        };
        let items = rd(n);
        let queries = rd(nq);
        let label: &'static str = Box::leak(label.to_string().into_boxed_str());
        out.push(Case { label, dim, items, queries });
    }
    out
}

/// Output of the C++ driver for one case.
#[derive(Default)]
struct CppCase {
    packed_iter: Vec<i64>,
    pq: Vec<Vec<i64>>,
    bq: Vec<Vec<i64>>,
    iq: Vec<Vec<i64>>,
    rq: Vec<Vec<i64>>,
    remove_failed: usize,
}

fn parse_cpp(path: &str) -> Vec<CppCase> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out: Vec<CppCase> = Vec::new();
    for line in text.lines() {
        let mut it = line.split_ascii_whitespace();
        let tag = it.next().unwrap();
        match tag {
            "case" => out.push(CppCase::default()),
            "PI" => out.last_mut().unwrap().packed_iter = it.map(|x| x.parse().unwrap()).collect(),
            "REMOVE-FAILED" => out.last_mut().unwrap().remove_failed += 1,
            "PQ" | "BQ" | "IQ" | "RQ" => {
                let k: usize = it.next().unwrap().parse().unwrap();
                let v: Vec<i64> = it.map(|x| x.parse().unwrap()).collect();
                assert_eq!(v.len(), k);
                let c = out.last_mut().unwrap();
                match tag {
                    "PQ" => c.pq.push(v),
                    "BQ" => c.bq.push(v),
                    "IQ" => c.iq.push(v),
                    _ => c.rq.push(v),
                }
            }
            _ => panic!("bad tag {tag}"),
        }
    }
    out
}

fn overlaps(dim: usize, a: &B, q: &B) -> bool {
    (0..dim).all(|d| a.min[d] <= q.max[d] && a.max[d] >= q.min[d])
}

fn sorted(v: &[i64]) -> Vec<i64> {
    let mut s = v.to_vec();
    s.sort();
    s
}

/// Hand port of DYNAMIC_RTREE: visit orders after BulkLoad, after n x Insert, and after
/// removing every odd id (same protocol as the C++ driver).
fn run_dynamic<const D: usize>(case: &Case) -> [Vec<Vec<i64>>; 3] {
    use spatial_research::dynamic_rtree::{BulkEntry, DynamicRTree};
    let arr = |v: &[i32; 3]| -> [i32; D] { std::array::from_fn(|d| v[d]) };
    let run_queries = |tree: &DynamicRTree<i64, D>| -> Vec<Vec<i64>> {
        case.queries
            .iter()
            .map(|q| {
                let mut r = Vec::new();
                tree.search(&arr(&q.min), &arr(&q.max), |&id| {
                    r.push(id);
                    true
                });
                r
            })
            .collect()
    };

    let mut bulk = DynamicRTree::<i64, D>::default();
    let entries: Vec<BulkEntry<i64, D>> = case
        .items
        .iter()
        .enumerate()
        .map(|(i, b)| BulkEntry { min: arr(&b.min), max: arr(&b.max), data: i as i64 })
        .collect();
    bulk.bulk_load(&entries);
    let bq = run_queries(&bulk);

    let mut inc = DynamicRTree::<i64, D>::default();
    for (i, b) in case.items.iter().enumerate() {
        inc.insert(&arr(&b.min), &arr(&b.max), i as i64);
    }
    let iq = run_queries(&inc);

    for (i, b) in case.items.iter().enumerate() {
        if i % 2 == 1 {
            assert!(inc.remove(&arr(&b.min), &arr(&b.max), i as i64));
        }
    }
    let rq = run_queries(&inc);
    [bq, iq, rq]
}

#[derive(Default, Clone)]
struct Tally {
    queries: usize,
    set_eq: usize,
    order_eq: usize,
    /// queries with at least two hits
    multi: usize,
    multi_order_eq: usize,
    multi_first_eq: usize,
}

impl Tally {
    fn add(&mut self, reference: &[i64], cand: &[i64]) {
        self.queries += 1;
        if sorted(reference) == sorted(cand) {
            self.set_eq += 1;
        }
        if reference == cand {
            self.order_eq += 1;
        }
        if reference.len() >= 2 {
            self.multi += 1;
            if reference == cand {
                self.multi_order_eq += 1;
            }
            if reference.first() == cand.first() {
                self.multi_first_eq += 1;
            }
        }
    }
}

fn compare(cases_path: &str, cpp_path: &str) {
    let cases = read_cases(cases_path);
    let cpp = parse_cpp(cpp_path);
    assert_eq!(cases.len(), cpp.len());

    // name -> label -> tally
    let mut t: BTreeMap<&'static str, BTreeMap<String, Tally>> = BTreeMap::new();
    let mut add = |name: &'static str, label: &str, r: &[i64], c: &[i64]| {
        let m = t.entry(name).or_default();
        m.entry(label.to_string()).or_default().add(r, c);
        m.entry("ALL".to_string()).or_default().add(r, c);
    };

    let mut panics: BTreeMap<(&'static str, String), usize> = BTreeMap::new();
    let mut packed_iter_eq = 0usize;
    let mut packed_iter_total = 0usize;
    let mut tie_cases = 0usize;
    let mut remove_failed = 0usize;

    for (case, cc) in cases.iter().zip(cpp.iter()) {
        let dim = case.dim;
        let label = format!("{}{}", case.label, if dim == 3 { "_3d" } else { "" });
        remove_failed += cc.remove_failed;

        // brute-force truth, in item order
        let truth: Vec<Vec<i64>> = case
            .queries
            .iter()
            .map(|q| {
                case.items
                    .iter()
                    .enumerate()
                    .filter(|(_, b)| overlaps(dim, b, q))
                    .map(|(i, _)| i as i64)
                    .collect()
            })
            .collect();
        let truth_odd_removed: Vec<Vec<i64>> =
            truth.iter().map(|v| v.iter().copied().filter(|i| i % 2 == 0).collect()).collect();

        // C++ dynamic tree vs brute force (sanity of the reference itself)
        for (qi, tr) in truth.iter().enumerate() {
            add("cpp DYNAMIC BulkLoad vs brute force", &label, tr, &cc.bq[qi]);
            add("cpp DYNAMIC Insert   vs brute force", &label, tr, &cc.iq[qi]);
            add("cpp DYNAMIC Remove   vs brute force", &label, &truth_odd_removed[qi], &cc.rq[qi]);
        }

        // hand port of DYNAMIC_RTREE: exact visit order expected
        let [pbq, piq, prq] = if dim == 2 { run_dynamic::<2>(case) } else { run_dynamic::<3>(case) };
        for qi in 0..case.queries.len() {
            add("HAND PORT DynamicRTree BulkLoad vs cpp DYNAMIC", &label, &cc.bq[qi], &pbq[qi]);
            add("HAND PORT DynamicRTree Insert   vs cpp DYNAMIC", &label, &cc.iq[qi], &piq[qi]);
            add("HAND PORT DynamicRTree Remove   vs cpp DYNAMIC", &label, &cc.rq[qi], &prq[qi]);
        }

        // rstar (f64 coordinates), any dimension: bulk load, insert, remove
        if dim == 2 {
            let objs: Vec<GeomWithData<Rectangle<[f64; 2]>, i64>> = case
                .items
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    GeomWithData::new(
                        Rectangle::from_corners(
                            [b.min[0] as f64, b.min[1] as f64],
                            [b.max[0] as f64, b.max[1] as f64],
                        ),
                        i as i64,
                    )
                })
                .collect();
            let bulk = RTree::bulk_load(objs.clone());
            let mut inc = RTree::new();
            for o in &objs {
                inc.insert(*o);
            }
            let q_env = |q: &B| {
                AABB::from_corners([q.min[0] as f64, q.min[1] as f64], [q.max[0] as f64, q.max[1] as f64])
            };
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = bulk.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar bulk_load vs cpp PACKED", &label, &cc.pq[qi], &r);
                add("rstar bulk_load vs cpp DYNAMIC BulkLoad", &label, &cc.bq[qi], &r);
                let r: Vec<i64> = inc.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar insert    vs cpp DYNAMIC Insert", &label, &cc.iq[qi], &r);
            }
            for (i, o) in objs.iter().enumerate() {
                if i % 2 == 1 {
                    assert!(inc.remove(o).is_some());
                }
            }
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = inc.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar remove    vs cpp DYNAMIC Remove", &label, &cc.rq[qi], &r);
            }
        } else {
            let objs: Vec<GeomWithData<Rectangle<[f64; 3]>, i64>> = case
                .items
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    GeomWithData::new(
                        Rectangle::from_corners(
                            [b.min[0] as f64, b.min[1] as f64, b.min[2] as f64],
                            [b.max[0] as f64, b.max[1] as f64, b.max[2] as f64],
                        ),
                        i as i64,
                    )
                })
                .collect();
            let bulk = RTree::bulk_load(objs.clone());
            let mut inc = RTree::new();
            for o in &objs {
                inc.insert(*o);
            }
            let q_env = |q: &B| {
                AABB::from_corners(
                    [q.min[0] as f64, q.min[1] as f64, q.min[2] as f64],
                    [q.max[0] as f64, q.max[1] as f64, q.max[2] as f64],
                )
            };
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = bulk.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar bulk_load vs cpp DYNAMIC BulkLoad", &label, &cc.bq[qi], &r);
                let r: Vec<i64> = inc.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar insert    vs cpp DYNAMIC Insert", &label, &cc.iq[qi], &r);
            }
            for (i, o) in objs.iter().enumerate() {
                if i % 2 == 1 {
                    assert!(inc.remove(o).is_some());
                }
            }
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = inc.locate_in_envelope_intersecting(q_env(q)).map(|o| o.data).collect();
                add("rstar remove    vs cpp DYNAMIC Remove", &label, &cc.rq[qi], &r);
            }
        }

        if dim != 2 {
            continue;
        }

        // hand port of PACKED_RTREE: must reproduce iteration order and visit order exactly
        let mut b = packed_rtree::Builder::<i64, 2>::default();
        for (i, it) in case.items.iter().enumerate() {
            b.add([it.min[0], it.min[1]], [it.max[0], it.max[1]], i as i64);
        }
        let tree = b.build();
        let iter_order: Vec<i64> = tree.iter().copied().collect();
        packed_iter_total += 1;
        if iter_order == cc.packed_iter {
            packed_iter_eq += 1;
        }
        // does this case have Hilbert-key ties at all? (same centre => same key)
        {
            let mut centres: Vec<(i32, i32)> = case
                .items
                .iter()
                .map(|it| (it.min[0] / 2 + it.max[0] / 2, it.min[1] / 2 + it.max[1] / 2))
                .collect();
            centres.sort();
            let before = centres.len();
            centres.dedup();
            if centres.len() != before {
                tie_cases += 1;
            }
        }
        for (qi, q) in case.queries.iter().enumerate() {
            let mut r = Vec::new();
            tree.search(&[q.min[0], q.min[1]], &[q.max[0], q.max[1]], |&id| {
                r.push(id);
                true
            });
            add("HAND PORT PackedRTree vs cpp PACKED", &label, &cc.pq[qi], &r);
            add("cpp PACKED vs brute force", &label, &truth[qi], &cc.pq[qi]);
        }

        // static_aabb2d_index (i32) -- arithmetic on the coordinate type may overflow
        if !case.items.is_empty() {
            let res = std::panic::catch_unwind(|| {
                let mut sb = StaticAABB2DIndexBuilder::<i32>::new(case.items.len());
                for it in &case.items {
                    sb.add(it.min[0], it.min[1], it.max[0], it.max[1]);
                }
                let idx = sb.build().unwrap();
                case.queries
                    .iter()
                    .map(|q| {
                        idx.query(q.min[0], q.min[1], q.max[0], q.max[1])
                            .into_iter()
                            .map(|v| v as i64)
                            .collect::<Vec<i64>>()
                    })
                    .collect::<Vec<_>>()
            });
            match res {
                Ok(rs) => {
                    for (qi, r) in rs.iter().enumerate() {
                        add("static_aabb2d_index<i32> vs cpp PACKED", &label, &cc.pq[qi], r);
                    }
                }
                Err(_) => *panics.entry(("static_aabb2d_index<i32>", label.clone())).or_default() += 1,
            }

            // same crate with f64 coordinates
            let mut sb = StaticAABB2DIndexBuilder::<f64>::new(case.items.len());
            for it in &case.items {
                sb.add(it.min[0] as f64, it.min[1] as f64, it.max[0] as f64, it.max[1] as f64);
            }
            let idx = sb.build().unwrap();
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = idx
                    .query(q.min[0] as f64, q.min[1] as f64, q.max[0] as f64, q.max[1] as f64)
                    .into_iter()
                    .map(|v| v as i64)
                    .collect();
                add("static_aabb2d_index<f64> vs cpp PACKED", &label, &cc.pq[qi], &r);
            }

            // geo-index (flatbush port, i32)
            let res = std::panic::catch_unwind(|| {
                let mut gb = RTreeBuilder::<i32>::new(case.items.len() as u32);
                for it in &case.items {
                    gb.add(it.min[0], it.min[1], it.max[0], it.max[1]);
                }
                let gt = gb.finish::<HilbertSort>();
                case.queries
                    .iter()
                    .map(|q| {
                        gt.search(q.min[0], q.min[1], q.max[0], q.max[1])
                            .into_iter()
                            .map(|v| v as i64)
                            .collect::<Vec<i64>>()
                    })
                    .collect::<Vec<_>>()
            });
            match res {
                Ok(rs) => {
                    for (qi, r) in rs.iter().enumerate() {
                        add("geo-index RTree<i32> vs cpp PACKED", &label, &cc.pq[qi], r);
                    }
                }
                Err(_) => *panics.entry(("geo-index RTree<i32>", label.clone())).or_default() += 1,
            }

            let mut gb = RTreeBuilder::<f64>::new(case.items.len() as u32);
            for it in &case.items {
                gb.add(it.min[0] as f64, it.min[1] as f64, it.max[0] as f64, it.max[1] as f64);
            }
            let gt = gb.finish::<HilbertSort>();
            for (qi, q) in case.queries.iter().enumerate() {
                let r: Vec<i64> = gt
                    .search(q.min[0] as f64, q.min[1] as f64, q.max[0] as f64, q.max[1] as f64)
                    .into_iter()
                    .map(|v| v as i64)
                    .collect();
                add("geo-index RTree<f64> vs cpp PACKED", &label, &cc.pq[qi], &r);
            }
        }
    }

    for ((name, label), n) in &panics {
        println!("PANIC (arithmetic overflow) in {name}: {n} cases of category {label}");
    }
    println!(
        "packed iteration order (begin()..end()) identical: {packed_iter_eq}/{packed_iter_total} 2D cases \
         ({tie_cases} of them contain Hilbert-key ties); C++ Remove() failures: {remove_failed}"
    );
    for (name, m) in &t {
        println!("\n== {name}");
        println!(
            "   {:<22} {:>8} {:>10} {:>10} | {:>10} {:>12} {:>12}",
            "category", "queries", "setEqual", "orderEqual", ">=2 hits", "orderEqual", "firstHitEq"
        );
        for (label, tal) in m {
            println!(
                "   {:<22} {:>8} {:>10} {:>10} | {:>10} {:>12} {:>12}",
                label, tal.queries, tal.set_eq, tal.order_eq, tal.multi, tal.multi_order_eq, tal.multi_first_eq
            );
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => {
            let seed = args.get(3).map(|s| s.parse::<u64>().unwrap()).unwrap_or(0x5254_5245_4500);
            let cases = generate(seed);
            write_cases(&args[2], &cases);
            let items: usize = cases.iter().map(|c| c.items.len()).sum();
            let queries: usize = cases.iter().map(|c| c.queries.len()).sum();
            println!("{} cases, {} items, {} queries", cases.len(), items, queries);
        }
        Some("compare") => compare(&args[2], &args[3]),
        _ => eprintln!("usage: rtree_cmp gen|compare ..."),
    }
}
