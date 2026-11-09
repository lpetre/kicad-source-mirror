//! Does rstar work with KiCad's integer nanometre coordinates?  (overflow checks are on)
use rstar::primitives::{GeomWithData, Rectangle};
use rstar::{AABB, RTree};
use std::num::Wrapping;

fn try_it<F: FnOnce() -> usize + std::panic::UnwindSafe>(name: &str, f: F) {
    match std::panic::catch_unwind(f) {
        Ok(n) => println!("{name:<58} ok ({n} hits)"),
        Err(_) => println!("{name:<58} PANIC"),
    }
}

fn main() {
    std::panic::set_hook(Box::new(|info| {
        let loc = info.location().map(|l| format!("{}:{}", l.file().rsplit('/').next().unwrap(), l.line()));
        let msg = info.payload().downcast_ref::<&str>().copied().unwrap_or("?");
        eprintln!("    panic: {msg} at {}", loc.unwrap_or_default());
    }));

    // 200 pads of 1.5 mm on a 100 mm board, coordinates in nm
    let boxes: Vec<[i64; 4]> = (0..200)
        .map(|i| {
            let x = (i % 20) * 5_000_000;
            let y = (i / 20) * 10_000_000;
            [x, y, x + 1_500_000, y + 1_500_000]
        })
        .collect();

    try_it("rstar AABB<[i32; 2]>, nm coordinates, bulk_load", || {
        let v: Vec<_> = boxes
            .iter()
            .enumerate()
            .map(|(i, b)| GeomWithData::new(Rectangle::from_corners([b[0] as i32, b[1] as i32], [b[2] as i32, b[3] as i32]), i))
            .collect();
        let t = RTree::bulk_load(v);
        t.locate_in_envelope_intersecting(AABB::from_corners([0, 0], [20_000_000, 20_000_000])).count()
    });
    try_it("rstar AABB<[i32; 2]>, nm coordinates, insert", || {
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([b[0] as i32, b[1] as i32], [b[2] as i32, b[3] as i32]), i));
        }
        t.locate_in_envelope_intersecting(AABB::from_corners([0, 0], [20_000_000, 20_000_000])).count()
    });
    try_it("rstar AABB<[i64; 2]>, nm coordinates, insert", || {
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([b[0], b[1]], [b[2], b[3]]), i));
        }
        t.locate_in_envelope_intersecting(AABB::from_corners([0, 0], [20_000_000, 20_000_000])).count()
    });
    try_it("rstar AABB<[i64; 2]>, + one item spanning the i32 range", || {
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([b[0], b[1]], [b[2], b[3]]), i));
        }
        let (lo, hi) = (i32::MIN as i64, i32::MAX as i64);
        t.insert(GeomWithData::new(Rectangle::from_corners([lo, lo], [hi, hi]), 999));
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([b[0] + 7, b[1] + 7], [b[2], b[3]]), 1000 + i));
        }
        t.locate_in_envelope_intersecting(AABB::from_corners([0, 0], [20_000_000, 20_000_000])).count()
    });
    try_it("rstar AABB<[i64; 3]> (layer, x, y) like CN_RTREE, insert", || {
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            let l = (i % 4) as i64;
            t.insert(GeomWithData::new(Rectangle::from_corners([l, b[0], b[1]], [l + 28, b[2], b[3]]), i));
        }
        let (lo, hi) = (i32::MIN as i64, i32::MAX as i64);
        t.insert(GeomWithData::new(Rectangle::from_corners([0, lo, lo], [31, hi, hi]), 999));
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([0, b[0] + 7, b[1] + 7], [31, b[2], b[3]]), 1000 + i));
        }
        t.locate_in_envelope_intersecting(AABB::from_corners([0, 0, 0], [0, 20_000_000, 20_000_000])).count()
    });
    try_it("rstar AABB<[Wrapping<i64>; 2]>, + item spanning i32 range", || {
        let w = Wrapping::<i64>;
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(Rectangle::from_corners([w(b[0]), w(b[1])], [w(b[2]), w(b[3])]), i));
        }
        let (lo, hi) = (w(i32::MIN as i64), w(i32::MAX as i64));
        t.insert(GeomWithData::new(Rectangle::from_corners([lo, lo], [hi, hi]), 999));
        t.locate_in_envelope_intersecting(AABB::from_corners([w(0), w(0)], [w(20_000_000), w(20_000_000)])).count()
    });
    try_it("rstar AABB<[f64; 2]>, + item spanning i32 range", || {
        let mut t = RTree::new();
        for (i, b) in boxes.iter().enumerate() {
            t.insert(GeomWithData::new(
                Rectangle::from_corners([b[0] as f64, b[1] as f64], [b[2] as f64, b[3] as f64]),
                i,
            ));
        }
        let (lo, hi) = (i32::MIN as f64, i32::MAX as f64);
        t.insert(GeomWithData::new(Rectangle::from_corners([lo, lo], [hi, hi]), 999));
        t.locate_in_envelope_intersecting(AABB::from_corners([0.0, 0.0], [20_000_000.0, 20_000_000.0])).count()
    });
}
