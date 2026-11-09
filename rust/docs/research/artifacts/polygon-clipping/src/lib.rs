// Scratch research crate: differential drivers for Clipper2 candidates.
pub mod common;
pub mod libmtrace;

/// clipper2-rs 0.3.0 from crates.io (feature `usingz`).
pub mod d_clipper2_rs {
    use clipper2_rs as cl;
    include!("body_clipper2_rs.rs");
}

/// clipper2-rs 0.3.0 + libstdc++ std::sort patch (vendor/clipper2-rs-gnusort).
pub mod d_gnusort {
    use clipper2_rs_gnusort as cl;
    include!("body_clipper2_rs.rs");
}

/// clipper2-rust 1.2.0 from crates.io (feature `using_z`).
pub mod d_clipper2_rust;

pub fn smoke_clipper2_rs() -> usize {
    use clipper2_rs::*;
    let subj = vec![vec![Point64::new_z(0, 0, 1), Point64::new_z(10, 0, 2), Point64::new_z(10, 10, 3), Point64::new_z(0, 10, 4)]];
    let clip = vec![vec![Point64::new_z(5, 5, 5), Point64::new_z(15, 5, 6), Point64::new_z(15, 15, 7), Point64::new_z(5, 15, 8)]];
    let mut c = Clipper64::new();
    c.add_subject(&subj);
    c.add_clip(&clip);
    c.set_z_callback(Some(Box::new(|_a, _b, _c, _d, pt: &mut Point64| pt.z = 99)));
    let (tree, _) = c.execute_tree(ClipType::Difference, FillRule::NonZero).unwrap();
    let mut co = ClipperOffset::default();
    co.add_paths(&subj, JoinType::Round, EndType::Polygon);
    co.set_arc_tolerance(0.25);
    tree.len() + co.execute(3.0).unwrap().len()
}

pub fn smoke_clipper2_rust() -> usize {
    use clipper2_rust::*;
    let subj = vec![vec![Point64::new_z(0, 0, 1), Point64::new_z(10, 0, 2), Point64::new_z(10, 10, 3), Point64::new_z(0, 10, 4)]];
    let clip = vec![vec![Point64::new_z(5, 5, 5), Point64::new_z(15, 5, 6), Point64::new_z(15, 15, 7), Point64::new_z(5, 15, 8)]];
    let mut c = Clipper64::new();
    c.add_subject(&subj);
    c.add_clip(&clip);
    c.set_z_callback(|_a: &Point64, _b: &Point64, _c: &Point64, _d: &Point64, pt: &mut Point64| pt.z = 99);
    let mut tree = PolyTree64::new();
    let mut open = Paths64::new();
    c.execute_tree(ClipType::Difference, FillRule::NonZero, &mut tree, &mut open);
    let mut co = ClipperOffset::new_default();
    co.add_paths(&subj, JoinType::Round, EndType::Polygon);
    co.set_arc_tolerance(0.25);
    let mut paths = Paths64::new();
    co.execute(3.0, &mut paths);
    tree.nodes.len() + paths.len()
}

#[cfg(test)]
mod tests {
    #[test]
    fn smoke() {
        assert!(super::smoke_clipper2_rs() >= 2);
        assert!(super::smoke_clipper2_rust() >= 2);
    }
}
