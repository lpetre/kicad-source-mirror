//! Constrained Delaunay triangulation of a polygon with a hole.
use clipper2_rs::{Paths64, area, make_path, triangulate};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let polygon: Paths64 = vec![
        make_path(&[0, 0, 300, 0, 300, 300, 0, 300])?,
        make_path(&[100, 100, 100, 200, 200, 200, 200, 100])?,
    ];
    let triangles = triangulate(&polygon, true)?;
    let total: f64 = triangles.iter().map(|t| area(t).abs()).sum();
    println!("{} triangles covering an area of {total}", triangles.len());
    Ok(())
}
