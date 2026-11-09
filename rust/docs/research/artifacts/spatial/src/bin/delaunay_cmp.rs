//! Research tool: generate point sets, run the Rust `delaunator` crate and the hand port of
//! KiCad's bundled delaunator-cpp, and compare against the output of the C++ driver.
//!
//!   delaunay_cmp gen <cases.txt>
//!   delaunay_cmp run-crate <cases.txt> <out.txt>
//!   delaunay_cmp run-port  <cases.txt> <out.txt>
//!   delaunay_cmp compare <cases.txt> <ref_out.txt> <cand_out.txt>

use spatial_research::delaunator_port;
use spatial_research::rng::Rng;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

type Pt = (i64, i64);

struct Case {
    label: &'static str,
    /// true when the set obeys KiCad's calling contract (unique points sorted by x then y,
    /// at least 3 points, not all collinear)
    contract: bool,
    pts: Vec<Pt>,
}

fn sort_unique(mut v: Vec<Pt>) -> Vec<Pt> {
    v.sort();
    v.dedup();
    v
}

fn all_collinear(v: &[Pt]) -> bool {
    if v.len() <= 2 {
        return true;
    }
    let p0 = v[0];
    let v0 = ((v[1].0 - p0.0) as i128, (v[1].1 - p0.1) as i128);
    v.iter().skip(2).all(|p| {
        let v1 = ((p.0 - p0.0) as i128, (p.1 - p0.1) as i128);
        v0.0 * v1.1 - v0.1 * v1.0 == 0
    })
}

fn push_contract(cases: &mut Vec<Case>, label: &'static str, pts: Vec<Pt>) {
    let pts = sort_unique(pts);
    if pts.len() >= 3 && !all_collinear(&pts) {
        cases.push(Case { label, contract: true, pts });
    }
}

fn generate(seed: u64) -> Vec<Case> {
    let mut rng = Rng::new(seed);
    let mut cases: Vec<Case> = Vec::new();

    // 1. tiny integer ranges: many cocircular / collinear subsets
    for _ in 0..400 {
        let n = rng.range(3, 14) as usize;
        let r = rng.range(2, 20);
        let pts = (0..n).map(|_| (rng.range(0, r), rng.range(0, r))).collect();
        push_contract(&mut cases, "tiny_int", pts);
    }

    // 2. random subsets of a PCB-like grid (2.54 mm / 1.27 mm / 0.1 mm / 0.5 mm pitch in nm)
    let pitches = [2_540_000i64, 1_270_000, 100_000, 500_000, 1_000_000, 650_000];
    for _ in 0..300 {
        let pitch = pitches[rng.range(0, pitches.len() as i64 - 1) as usize];
        let k = rng.range(3, 40);
        let n = rng.range(4, 250) as usize;
        let ox = rng.range(0, 200) * 1_000_000;
        let oy = rng.range(0, 200) * 1_000_000;
        let pts = (0..n)
            .map(|_| (ox + rng.range(0, k) * pitch, oy + rng.range(0, k) * pitch))
            .collect();
        push_contract(&mut cases, "grid_subset", pts);
    }

    // 3. uniform random integer coordinates at several scales
    let scales = [1_000i64, 1_000_000, 100_000_000, 300_000_000, 2_000_000_000];
    for _ in 0..400 {
        let s = scales[rng.range(0, scales.len() as i64 - 1) as usize];
        let n = rng.range(3, 400) as usize;
        let pts = (0..n).map(|_| (rng.range(0, s), rng.range(0, s))).collect();
        push_contract(&mut cases, "uniform", pts);
    }

    // 4. full grids (every 4 neighbours cocircular, many equal distances to the centre)
    for _ in 0..80 {
        let pitch = pitches[rng.range(0, pitches.len() as i64 - 1) as usize];
        let w = rng.range(2, 30);
        let h = rng.range(2, 30);
        let ox = rng.range(0, 250) * 1_000_000 + rng.range(0, 999_999);
        let oy = rng.range(0, 250) * 1_000_000 + rng.range(0, 999_999);
        let mut pts = Vec::new();
        for i in 0..w {
            for j in 0..h {
                pts.push((ox + i * pitch, oy + j * pitch));
            }
        }
        push_contract(&mut cases, "full_grid", pts);
    }

    // 5. grid with jitter on some points
    for _ in 0..120 {
        let pitch = pitches[rng.range(0, pitches.len() as i64 - 1) as usize];
        let w = rng.range(2, 20);
        let h = rng.range(2, 20);
        let jit = [1i64, 2, 10, 1000, 25_400][rng.range(0, 4) as usize];
        let mut pts = Vec::new();
        for i in 0..w {
            for j in 0..h {
                let (mut x, mut y) = (50_000_000 + i * pitch, 50_000_000 + j * pitch);
                if rng.chance(1, 4) {
                    x += rng.range(-jit, jit);
                    y += rng.range(-jit, jit);
                }
                pts.push((x, y));
            }
        }
        push_contract(&mut cases, "grid_jitter", pts);
    }

    // 6. points on circles (rounded to nm), optional centre, concentric rings
    for _ in 0..100 {
        let rings = rng.range(1, 3);
        let cx = rng.range(10, 200) * 1_000_000;
        let cy = rng.range(10, 200) * 1_000_000;
        let mut pts = Vec::new();
        if rng.chance(1, 2) {
            pts.push((cx, cy));
        }
        for ring in 1..=rings {
            let k = rng.range(3, 48);
            let r = (ring * rng.range(500_000, 20_000_000)) as f64;
            for i in 0..k {
                let a = (i as f64) * std::f64::consts::TAU / (k as f64);
                pts.push((cx + (r * a.cos()).round() as i64, cy + (r * a.sin()).round() as i64));
            }
        }
        push_contract(&mut cases, "circle", pts);
    }

    // 7. near-collinear sets: a line with a few points 1..3 nm off
    for _ in 0..150 {
        let n = rng.range(3, 60) as usize;
        let dx = rng.range(-5, 5) * rng.range(1, 1_000_000);
        let dy = rng.range(-5, 5) * rng.range(1, 1_000_000);
        if dx == 0 && dy == 0 {
            continue;
        }
        let ox = rng.range(0, 1_000_000_000);
        let oy = rng.range(0, 1_000_000_000);
        let off = rng.range(1, 3);
        let mut pts: Vec<Pt> = (0..n)
            .map(|_| {
                let t = rng.range(0, 400);
                (ox + t * dx, oy + t * dy)
            })
            .collect();
        let m = rng.range(1, 3);
        for _ in 0..m {
            let i = rng.range(0, n as i64 - 1) as usize;
            if rng.chance(1, 2) {
                pts[i].0 += off;
            } else {
                pts[i].1 -= off;
            }
        }
        push_contract(&mut cases, "near_collinear", pts);
    }

    // 8. QFP-like: pads on the four sides of a rectangle, plus a few random points
    for _ in 0..80 {
        let pitch = [400_000i64, 500_000, 650_000, 800_000, 1_270_000][rng.range(0, 4) as usize];
        let k = rng.range(4, 44);
        let half = (k * pitch) / 2 + 1_500_000;
        let cx = rng.range(20, 150) * 1_000_000;
        let cy = rng.range(20, 150) * 1_000_000;
        let mut pts = Vec::new();
        for i in 0..k {
            let o = -(k - 1) * pitch / 2 + i * pitch;
            pts.push((cx + o, cy - half));
            pts.push((cx + o, cy + half));
            pts.push((cx - half, cy + o));
            pts.push((cx + half, cy + o));
        }
        for _ in 0..rng.range(0, 10) {
            pts.push((cx + rng.range(-3 * half, 3 * half), cy + rng.range(-3 * half, 3 * half)));
        }
        push_contract(&mut cases, "qfp_like", pts);
    }

    // 9. DIP / connector like: two (or three) parallel rows
    for _ in 0..80 {
        let pitch = [2_540_000i64, 1_270_000, 2_000_000][rng.range(0, 2) as usize];
        let k = rng.range(2, 40);
        let rows = rng.range(2, 3);
        let row_pitch = [7_620_000i64, 2_540_000, 15_240_000, 1_270_000][rng.range(0, 3) as usize];
        let ox = rng.range(0, 200_000_000);
        let oy = rng.range(0, 200_000_000);
        let horiz = rng.chance(1, 2);
        let mut pts = Vec::new();
        for r in 0..rows {
            for i in 0..k {
                if rng.chance(1, 10) {
                    continue; // some pins unconnected to this net
                }
                let (a, b) = (i * pitch, r * row_pitch);
                pts.push(if horiz { (ox + a, oy + b) } else { (ox + b, oy + a) });
            }
        }
        push_contract(&mut cases, "two_rows", pts);
    }

    // 10. permutation-symmetric tiny sets: squares, rectangles, rhombi, hexagons
    for s in [1i64, 2, 1000, 2_540_000] {
        for (w, h) in [(1i64, 1i64), (1, 2), (3, 1), (2, 2), (5, 3)] {
            let (w, h) = (w * s, h * s);
            push_contract(&mut cases, "symmetric", vec![(0, 0), (w, 0), (0, h), (w, h)]);
            push_contract(
                &mut cases,
                "symmetric",
                vec![(0, 0), (w, 0), (0, h), (w, h), (2 * w, 0), (2 * w, h)],
            );
            push_contract(
                &mut cases,
                "symmetric",
                vec![(w, 0), (0, h), (-w, 0), (0, -h)],
            );
            push_contract(
                &mut cases,
                "symmetric",
                vec![(w, 0), (0, h), (-w, 0), (0, -h), (0, 0)],
            );
            push_contract(
                &mut cases,
                "symmetric",
                vec![(0, 0), (w, 0), (0, h), (w, h), (w / 2, h / 2)],
            );
            // octagon (cocircular 8 points on the integer lattice: (+-1,+-2),(+-2,+-1) scaled)
            let mut oct = Vec::new();
            for (a, b) in [(1i64, 2i64), (2, 1)] {
                for sx in [-1i64, 1] {
                    for sy in [-1i64, 1] {
                        oct.push((sx * a * w, sy * b * w));
                    }
                }
            }
            push_contract(&mut cases, "symmetric", oct.clone());
            oct.push((0, 0));
            push_contract(&mut cases, "symmetric", oct);
        }
    }

    // 11. big sets
    for _ in 0..8 {
        let n = rng.range(2000, 6000) as usize;
        let pts = (0..n)
            .map(|_| (rng.range(0, 300_000_000), rng.range(0, 300_000_000)))
            .collect();
        push_contract(&mut cases, "big_uniform", pts);
    }
    for (w, h) in [(50i64, 50i64), (64, 40), (100, 30)] {
        let mut pts = Vec::new();
        for i in 0..w {
            for j in 0..h {
                pts.push((10_000_000 + i * 1_270_000, 10_000_000 + j * 1_270_000));
            }
        }
        push_contract(&mut cases, "big_grid", pts);
    }

    // 12. outside KiCad's contract: unsorted input with duplicates
    for _ in 0..150 {
        let n = rng.range(3, 120) as usize;
        let r = [5i64, 50, 1_000_000][rng.range(0, 2) as usize];
        let mut pts: Vec<Pt> = (0..n).map(|_| (rng.range(0, r), rng.range(0, r))).collect();
        for _ in 0..rng.range(1, 10) {
            let i = rng.range(0, pts.len() as i64 - 1) as usize;
            pts.push(pts[i]);
        }
        rng.shuffle(&mut pts);
        cases.push(Case { label: "x_dups_unsorted", contract: false, pts });
    }

    // 13. outside KiCad's contract: unsorted unique points (index tie-breaking differs)
    for _ in 0..150 {
        let n = rng.range(3, 200) as usize;
        let k = rng.range(3, 30);
        let pts: Vec<Pt> =
            (0..n).map(|_| (rng.range(0, k) * 1_270_000, rng.range(0, k) * 1_270_000)).collect();
        let mut pts = sort_unique(pts);
        if pts.len() < 3 || all_collinear(&pts) {
            continue;
        }
        rng.shuffle(&mut pts);
        cases.push(Case { label: "x_unsorted_grid", contract: false, pts });
    }

    // 14. outside KiCad's contract: all collinear (KiCad never passes these)
    for _ in 0..30 {
        let n = rng.range(3, 30);
        let dx = rng.range(0, 3) * 1000;
        let dy = rng.range(0, 3) * 1000;
        if dx == 0 && dy == 0 {
            continue;
        }
        let pts = sort_unique((0..n).map(|i| (i * dx, i * dy)).collect());
        cases.push(Case { label: "x_all_collinear", contract: false, pts });
    }

    // 15. rotated footprints: rows / grids of pads rotated by an arbitrary angle and rounded to
    //     nm, like KiCad pad positions of a rotated footprint (collinear / cocircular up to
    //     +-1 nm rounding noise) -- the realistic near-degenerate input of the ratsnest.
    for _ in 0..600 {
        let pitch = [2_540_000i64, 1_270_000, 500_000, 800_000, 1_000_000][rng.range(0, 4) as usize] as f64;
        let cols = rng.range(2, 40);
        let kind = rng.range(0, 3);
        let rows = match kind {
            0 => 1,
            1 => 2,
            _ => rng.range(2, 12),
        };
        let row_pitch = if kind == 1 {
            [7_620_000i64, 2_540_000, 15_240_000][rng.range(0, 2) as usize] as f64
        } else {
            pitch
        };
        // angles in tenths of a degree like the GUI offers, sometimes arbitrary
        let theta = if rng.chance(1, 2) {
            (rng.range(1, 3599) as f64 / 10.0).to_radians()
        } else {
            [30.0f64, 45.0, 60.0, 15.0, 22.5, 135.0, 225.0, 315.0, 1.0][rng.range(0, 8) as usize].to_radians()
        };
        let (sn, cs) = theta.sin_cos();
        let ox = rng.range(20_000_000, 250_000_000) as f64;
        let oy = rng.range(20_000_000, 250_000_000) as f64;
        let mut pts = Vec::new();
        for r in 0..rows {
            for c in 0..cols {
                if kind != 0 && rng.chance(1, 8) {
                    continue;
                }
                let (lx, ly) = (c as f64 * pitch, r as f64 * row_pitch);
                let x = ox + lx * cs - ly * sn;
                let y = oy + lx * sn + ly * cs;
                pts.push((x.round() as i64, y.round() as i64));
            }
        }
        // a few unrelated anchors elsewhere on the board
        for _ in 0..rng.range(0, 4) {
            pts.push((rng.range(0, 300_000_000), rng.range(0, 300_000_000)));
        }
        let label = match kind {
            0 => "rot_row",
            1 => "rot_two_rows",
            _ => "rot_grid",
        };
        push_contract(&mut cases, label, pts);
    }

    // 16. near-collinear sets inside a realistic board area (<= ~300 mm, i32 nm coordinates):
    //     pads on a line with 1..3 of them 1..3 nm off the line
    for _ in 0..300 {
        let n = rng.range(3, 60) as usize;
        let dx = rng.range(-5, 5) * rng.range(1, 100_000);
        let dy = rng.range(-5, 5) * rng.range(1, 100_000);
        if dx == 0 && dy == 0 {
            continue;
        }
        let ox = 150_000_000;
        let oy = 150_000_000;
        let off = rng.range(1, 3);
        let mut pts: Vec<Pt> = (0..n)
            .map(|_| {
                let t = rng.range(-200, 200);
                (ox + t * dx, oy + t * dy)
            })
            .collect();
        let m = rng.range(1, 3);
        for _ in 0..m {
            let i = rng.range(0, n as i64 - 1) as usize;
            if rng.chance(1, 2) {
                pts[i].0 += off;
            } else {
                pts[i].1 -= off;
            }
        }
        push_contract(&mut cases, "near_coll_board", pts);
    }

    cases
}

fn write_cases(path: &str, cases: &[Case]) {
    let mut s = String::new();
    writeln!(s, "{}", cases.len()).unwrap();
    for c in cases {
        writeln!(s, "{}", c.pts.len()).unwrap();
        for p in &c.pts {
            writeln!(s, "{} {}", p.0, p.1).unwrap();
        }
    }
    std::fs::write(path, s).unwrap();
    let mut l = String::new();
    for c in cases {
        writeln!(l, "{} {}", c.label, if c.contract { 1 } else { 0 }).unwrap();
    }
    std::fs::write(format!("{path}.labels"), l).unwrap();
}

fn read_cases(path: &str) -> Vec<Vec<f64>> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut it = text.split_ascii_whitespace();
    let ncases: usize = it.next().unwrap().parse().unwrap();
    let mut out = Vec::with_capacity(ncases);
    for _ in 0..ncases {
        let n: usize = it.next().unwrap().parse().unwrap();
        let mut coords = Vec::with_capacity(2 * n);
        for _ in 0..2 * n {
            coords.push(it.next().unwrap().parse::<f64>().unwrap());
        }
        out.push(coords);
    }
    out
}

fn fmt_result(
    s: &mut String,
    idx: usize,
    res: Result<(&[usize], &[usize], Vec<usize>), String>,
) {
    writeln!(s, "case {idx}").unwrap();
    match res {
        Err(e) => writeln!(s, "EXC {e}").unwrap(),
        Ok((t, h, hull)) => {
            write!(s, "T {}", t.len()).unwrap();
            for v in t {
                write!(s, " {v}").unwrap();
            }
            write!(s, "\nH {}", h.len()).unwrap();
            for &v in h {
                if v == usize::MAX {
                    s.push_str(" -1");
                } else {
                    write!(s, " {v}").unwrap();
                }
            }
            write!(s, "\nHULL {}", hull.len()).unwrap();
            for v in hull {
                write!(s, " {v}").unwrap();
            }
            s.push('\n');
        }
    }
}

fn run_crate(cases: &[Vec<f64>], out: &str) {
    let mut s = String::new();
    for (idx, coords) in cases.iter().enumerate() {
        let pts: Vec<delaunator::Point> =
            coords.chunks(2).map(|c| delaunator::Point { x: c[0], y: c[1] }).collect();
        let t = delaunator::triangulate(&pts);
        if t.triangles.is_empty() {
            fmt_result(&mut s, idx, Err("empty (collinear) triangulation".to_string()));
        } else {
            fmt_result(&mut s, idx, Ok((&t.triangles, &t.halfedges, t.hull.clone())));
        }
    }
    std::fs::write(out, s).unwrap();
}

fn run_port(cases: &[Vec<f64>], out: &str) {
    let mut s = String::new();
    for (idx, coords) in cases.iter().enumerate() {
        match delaunator_port::Delaunator::new(coords) {
            Err(delaunator_port::DelaunatorError::NotTriangulation) => {
                fmt_result(&mut s, idx, Err("not triangulation".to_string()))
            }
            Err(e) => fmt_result(&mut s, idx, Err(format!("{e:?}"))),
            Ok(d) => {
                let n = coords.len() / 2;
                let mut hull = Vec::new();
                let mut e = d.hull_start;
                loop {
                    hull.push(e);
                    e = d.hull_next[e];
                    if e == d.hull_start || hull.len() > n {
                        break;
                    }
                }
                fmt_result(&mut s, idx, Ok((&d.triangles, &d.halfedges, hull)));
            }
        }
    }
    std::fs::write(out, s).unwrap();
}

#[derive(Debug, Clone, PartialEq)]
enum Parsed {
    Exc(String),
    Ok { t: Vec<i64>, h: Vec<i64>, hull: Vec<i64> },
}

fn parse_out(path: &str) -> Vec<Parsed> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        assert!(line.starts_with("case "), "bad line {line}");
        let l = lines.next().unwrap();
        if let Some(rest) = l.strip_prefix("EXC ") {
            out.push(Parsed::Exc(rest.to_string()));
            continue;
        }
        let nums = |l: &str, tag: &str| -> Vec<i64> {
            let mut it = l.split_ascii_whitespace();
            assert_eq!(it.next().unwrap(), tag);
            let n: usize = it.next().unwrap().parse().unwrap();
            let v: Vec<i64> = it.map(|x| x.parse().unwrap()).collect();
            assert_eq!(v.len(), n);
            v
        };
        let t = nums(l, "T");
        let h = nums(lines.next().unwrap(), "H");
        let hull = nums(lines.next().unwrap(), "HULL");
        out.push(Parsed::Ok { t, h, hull });
    }
    out
}

fn tri_set(t: &[i64]) -> BTreeSet<[i64; 3]> {
    t.chunks(3)
        .map(|c| {
            let mut a = [c[0], c[1], c[2]];
            a.sort();
            a
        })
        .collect()
}

/// The undirected edges exactly as `RN_NET::TRIANGULATOR_STATE::Triangulate` extracts them.
fn edge_set(t: &[i64], h: &[i64]) -> BTreeSet<(i64, i64)> {
    let mut s = BTreeSet::new();
    for e in 0..t.len() {
        let opposite = h[e];
        if opposite != -1 && (opposite as usize) < e {
            continue;
        }
        let next = if e % 3 == 2 { e - 2 } else { e + 1 };
        let (a, b) = (t[e], t[next]);
        s.insert((a.min(b), a.max(b)));
    }
    s
}

fn cyclic_eq(a: &[i64], b: &[i64]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    if a.is_empty() {
        return true;
    }
    let n = a.len();
    (0..n).any(|s| (0..n).all(|i| a[i] == b[(i + s) % n]))
        || (0..n).any(|s| (0..n).all(|i| a[i] == b[(s + n - i) % n]))
}

#[derive(Default)]
struct Stat {
    total: usize,
    status_mismatch: usize,
    both_exc: usize,
    exact: usize,
    tri_set_eq: usize,
    edge_set_eq: usize,
    hull_eq: usize,
    first_bad: Option<usize>,
    edge_diff_cases: usize,
    edge_diff_edges: usize,
}

fn compare(cases_path: &str, ref_path: &str, cand_path: &str) {
    let labels: Vec<(String, bool)> = std::fs::read_to_string(format!("{cases_path}.labels"))
        .unwrap()
        .lines()
        .map(|l| {
            let mut it = l.split(' ');
            (it.next().unwrap().to_string(), it.next().unwrap() == "1")
        })
        .collect();
    let r = parse_out(ref_path);
    let c = parse_out(cand_path);
    assert_eq!(r.len(), labels.len());
    assert_eq!(c.len(), labels.len());

    let mut stats: BTreeMap<String, Stat> = BTreeMap::new();
    for (i, ((label, _), (rr, cc))) in labels.iter().zip(r.iter().zip(c.iter())).enumerate() {
        for key in [label.clone(), if labels[i].1 { "ALL(contract)".into() } else { "ALL(x_)".into() }] {
            let st = stats.entry(key).or_default();
            st.total += 1;
            match (rr, cc) {
                (Parsed::Exc(_), Parsed::Exc(_)) => st.both_exc += 1,
                (Parsed::Exc(_), _) | (_, Parsed::Exc(_)) => {
                    st.status_mismatch += 1;
                    st.first_bad.get_or_insert(i);
                }
                (Parsed::Ok { t: t1, h: h1, hull: hu1 }, Parsed::Ok { t: t2, h: h2, hull: hu2 }) => {
                    let exact = t1 == t2 && h1 == h2;
                    if exact {
                        st.exact += 1;
                    }
                    if tri_set(t1) == tri_set(t2) {
                        st.tri_set_eq += 1;
                    }
                    let (e1, e2) = (edge_set(t1, h1), edge_set(t2, h2));
                    if e1 == e2 {
                        st.edge_set_eq += 1;
                    } else {
                        st.first_bad.get_or_insert(i);
                        st.edge_diff_cases += 1;
                        st.edge_diff_edges += e1.symmetric_difference(&e2).count();
                    }
                    if cyclic_eq(hu1, hu2) {
                        st.hull_eq += 1;
                    }
                }
            }
        }
    }
    println!(
        "{:<18} {:>6} {:>8} {:>8} {:>8} {:>9} {:>9} {:>8} {:>10}",
        "category", "cases", "bothExc", "statDiff", "exact", "triSetEq", "edgeSetEq", "hullEq", "firstDiff"
    );
    for (k, s) in &stats {
        println!(
            "{:<18} {:>6} {:>8} {:>8} {:>8} {:>9} {:>9} {:>8} {:>10}",
            k,
            s.total,
            s.both_exc,
            s.status_mismatch,
            s.exact,
            s.tri_set_eq,
            s.edge_set_eq,
            s.hull_eq,
            s.first_bad.map(|v| v.to_string()).unwrap_or_else(|| "-".into())
        );
    }
    let tot_diff: usize = stats
        .iter()
        .filter(|(k, _)| k.starts_with("ALL"))
        .map(|(_, s)| s.edge_diff_edges)
        .sum();
    println!("total differing edges (symmetric difference, all cases): {tot_diff}");
}

/// Minimal union-find for the MST comparison.
struct Uf(Vec<usize>);
impl Uf {
    fn find(&mut self, mut a: usize) -> usize {
        while self.0[a] != a {
            self.0[a] = self.0[self.0[a]];
            a = self.0[a];
        }
        a
    }
    fn unite(&mut self, a: usize, b: usize) -> bool {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return false;
        }
        self.0[ra] = rb;
        true
    }
}

/// What KiCad's `RN_NET::kruskalMST` would select from the triangulation edges: Kruskal with the
/// strict total order (weight, lower endpoint (x, y, tag), upper endpoint (x, y, tag)) after
/// uniting the `pre` pairs (zero-weight board edges).  Weight is the rounded euclidean distance.
fn kicad_mst(
    coords: &[f64],
    t: &[i64],
    h: &[i64],
    pre: &[(usize, usize)],
) -> BTreeSet<(i64, i64)> {
    let n = coords.len() / 2;
    let mut uf = Uf((0..n).collect());
    for &(a, b) in pre {
        uf.unite(a, b);
    }
    let key = |i: i64| (coords[2 * i as usize] as i64, coords[2 * i as usize + 1] as i64, i);
    let mut edges: Vec<(i64, (i64, i64, i64), (i64, i64, i64))> = edge_set(t, h)
        .into_iter()
        .map(|(a, b)| {
            let (ka, kb) = (key(a), key(b));
            let dx = (ka.0 - kb.0) as f64;
            let dy = (ka.1 - kb.1) as f64;
            let w = (dx * dx + dy * dy).sqrt().round() as i64;
            let (lo, hi) = if kb < ka { (kb, ka) } else { (ka, kb) };
            (w, lo, hi)
        })
        .collect();
    edges.sort();
    let mut out = BTreeSet::new();
    for (_, lo, hi) in edges {
        if uf.unite(lo.2 as usize, hi.2 as usize) {
            out.insert((lo.2, hi.2));
        }
    }
    out
}

fn mst_compare(cases_path: &str, ref_path: &str, cand_path: &str) {
    let labels: Vec<(String, bool)> = std::fs::read_to_string(format!("{cases_path}.labels"))
        .unwrap()
        .lines()
        .map(|l| {
            let mut it = l.split(' ');
            (it.next().unwrap().to_string(), it.next().unwrap() == "1")
        })
        .collect();
    let cases = read_cases(cases_path);
    let r = parse_out(ref_path);
    let c = parse_out(cand_path);
    let mut rng = Rng::new(99);
    // per label: cases, ref drops points, cand drops points, mst differs (no pre), mst differs (random pre)
    let mut stats: BTreeMap<String, [usize; 6]> = BTreeMap::new();
    for i in 0..cases.len() {
        if !labels[i].1 {
            continue;
        }
        let (Parsed::Ok { t: t1, h: h1, .. }, Parsed::Ok { t: t2, h: h2, .. }) = (&r[i], &c[i]) else {
            continue;
        };
        let n = cases[i].len() / 2;
        let used = |t: &[i64]| t.iter().copied().collect::<BTreeSet<i64>>().len();
        for key in [labels[i].0.clone(), "ALL(contract)".to_string()] {
            let st = stats.entry(key).or_default();
            st[0] += 1;
            if used(t1) != n {
                st[1] += 1;
            }
            if used(t2) != n {
                st[2] += 1;
            }
        }
        let m0 = kicad_mst(&cases[i], t1, h1, &[]) != kicad_mst(&cases[i], t2, h2, &[]);
        // three random "already routed" scenarios
        let mut m1 = false;
        let mut diff_trials = 0;
        for _ in 0..5 {
            let k = rng.range(1, (n as i64 / 2).max(1));
            let pre: Vec<(usize, usize)> = (0..k)
                .map(|_| (rng.range(0, n as i64 - 1) as usize, rng.range(0, n as i64 - 1) as usize))
                .collect();
            if kicad_mst(&cases[i], t1, h1, &pre) != kicad_mst(&cases[i], t2, h2, &pre) {
                m1 = true;
                diff_trials += 1;
            }
        }
        for key in [labels[i].0.clone(), "ALL(contract)".to_string()] {
            let st = stats.get_mut(&key).unwrap();
            if m0 {
                st[3] += 1;
            }
            if m1 {
                st[4] += 1;
            }
            st[5] += diff_trials;
        }
    }
    println!(
        "{:<16} {:>6} {:>12} {:>13} {:>12} {:>16} {:>12}",
        "category", "cases", "refDropsPts", "candDropsPts", "mstDiff", "mstDiff(routed)", "trialsDiff/5"
    );
    for (k, s) in &stats {
        println!(
            "{:<16} {:>6} {:>12} {:>13} {:>12} {:>16} {:>12}",
            k, s[0], s[1], s[2], s[3], s[4], s[5]
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(|s| s.as_str()) {
        Some("gen") => {
            let seed = args.get(3).map(|s| s.parse::<u64>().unwrap()).unwrap_or(0x4b69_4361_6452_5354);
            let cases = generate(seed);
            write_cases(&args[2], &cases);
            let mut by: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
            for c in &cases {
                let e = by.entry(c.label).or_default();
                e.0 += 1;
                e.1 += c.pts.len();
            }
            println!("{} cases", cases.len());
            for (k, (n, p)) in by {
                println!("  {k:<18} {n:>5} cases {p:>8} points");
            }
        }
        Some("run-crate") => run_crate(&read_cases(&args[2]), &args[3]),
        Some("run-port") => run_port(&read_cases(&args[2]), &args[3]),
        Some("compare") => compare(&args[2], &args[3], &args[4]),
        Some("mst-compare") => mst_compare(&args[2], &args[3], &args[4]),
        _ => eprintln!("usage: delaunay_cmp gen|run-crate|run-port|compare ..."),
    }
}
