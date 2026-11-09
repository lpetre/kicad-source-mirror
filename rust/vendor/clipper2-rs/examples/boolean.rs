//! Boolean operations on integer and floating-point paths, and a PolyTree of the result.
use clipper2_rs::{
    ClipType, Clipper64, FillRule, Paths64, PathsD, PointD, area_paths, intersect, make_path,
    union_d,
};

fn main() -> Result<(), clipper2_rs::ClipperError> {
    let subject: Paths64 = vec![make_path(&[100, 50, 10, 79, 65, 2, 65, 98, 10, 21])?];
    let clip: Paths64 = vec![make_path(&[98, 63, 4, 68, 77, 8, 52, 100, 19, 12])?];

    let solution = intersect(&subject, &clip, FillRule::NonZero)?;
    println!(
        "intersection: {} paths, area {}",
        solution.len(),
        area_paths(&solution)
    );

    // The same operation with full control, into a PolyTree that keeps holes under their outers.
    let mut clipper = Clipper64::new();
    clipper.add_subject(&subject);
    clipper.add_clip(&clip);
    let (tree, _open) = clipper.execute_tree(ClipType::Xor, FillRule::EvenOdd)?;
    let holes = tree
        .dfs()
        .into_iter()
        .filter(|&id| tree.is_hole(id))
        .count();
    println!(
        "xor: {} polygons, {holes} of them holes, area {}",
        tree.len(),
        tree.area()
    );

    // Floating-point paths are scaled to integers internally (2 decimal places here).
    let square = |x: f64, y: f64| -> Vec<PointD> {
        [(0.0, 0.0), (1.5, 0.0), (1.5, 1.5), (0.0, 1.5)]
            .iter()
            .map(|&(dx, dy)| PointD::new(x + dx, y + dy))
            .collect()
    };
    let squares: PathsD = vec![square(0.0, 0.0), square(1.0, 1.0)];
    let merged = union_d(&squares, &PathsD::new(), FillRule::NonZero, 2)?;
    println!("union of two squares: {:?}", merged[0]);
    Ok(())
}
