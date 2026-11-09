//! Growing and shrinking polygons, and turning an open polyline into a stroke outline.
use clipper2_rs::{
    ClipperOffset, EndType, JoinType, Paths64, area_paths, inflate_paths, make_path,
};

fn main() -> Result<(), clipper2_rs::ClipperError> {
    let square: Paths64 = vec![make_path(&[0, 0, 100, 0, 100, 100, 0, 100])?];

    for join in [
        JoinType::Miter,
        JoinType::Round,
        JoinType::Square,
        JoinType::Bevel,
    ] {
        let grown = inflate_paths(&square, 10.0, join, EndType::Polygon, 2.0, 0.0)?;
        println!("{join:?}: area {}", area_paths(&grown));
    }
    let shrunk = inflate_paths(&square, -10.0, JoinType::Miter, EndType::Polygon, 2.0, 0.0)?;
    println!("shrunk: area {}", area_paths(&shrunk));

    // An open path with round caps, 5 units either side.
    let mut co = ClipperOffset::default();
    co.add_paths(
        &vec![make_path(&[0, 0, 100, 0, 100, 60])?],
        JoinType::Round,
        EndType::Round,
    );
    let stroke = co.execute(5.0)?;
    println!("stroke outline: {} points", stroke[0].len());
    Ok(())
}
