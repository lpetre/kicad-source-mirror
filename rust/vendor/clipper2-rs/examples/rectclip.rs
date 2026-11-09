//! Clipping polygons and polylines to a rectangle (much faster than a general boolean).
use clipper2_rs::{Paths64, Point64, Rect64, ellipse, make_path, rect_clip, rect_clip_lines};

fn main() -> Result<(), clipper2_rs::ClipperError> {
    let rect = Rect64::new(0, 0, 100, 100);
    let circles: Paths64 = vec![
        ellipse(Point64::new(0, 0), 60.0, 60.0, 64),
        ellipse(Point64::new(100, 50), 30.0, 30.0, 64),
    ];
    for path in rect_clip(&rect, &circles) {
        println!("clipped polygon: {} points", path.len());
    }

    let line: Paths64 = vec![make_path(&[-50, 50, 150, 50, 150, 80, 50, 80])?];
    println!("clipped polyline: {:?}", rect_clip_lines(&rect, &line));
    Ok(())
}
