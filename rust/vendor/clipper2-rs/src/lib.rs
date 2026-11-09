//! Pure Rust port of [Clipper2](https://github.com/PolyClipping/Clipper2) by Angus Johnson
//! (2.0.1 plus later upstream fixes): polygon boolean operations, offsetting, rectangle
//! clipping, Minkowski sums and differences, and constrained Delaunay triangulation.
//!
//! Results are bit-for-bit identical to the C++ library except where the repository README
//! lists a deliberate difference (stable ordering of equal intersections, errors instead of
//! undefined behaviour on invalid input, and two offset corrections). The triangulation, which
//! upstream has withdrawn, is maintained here: its results are checked to cover the input
//! exactly, and differ from upstream's where those are wrong.
//!
//! ```
//! use clipper2_rs::{FillRule, intersect, make_path};
//!
//! let subject = vec![make_path(&[100, 50, 10, 79, 65, 2, 65, 98, 10, 21])?];
//! let clip = vec![make_path(&[98, 63, 4, 68, 77, 8, 52, 100, 19, 12])?];
//! let solution = intersect(&subject, &clip, FillRule::NonZero)?;
//! assert_eq!(solution.len(), 1);
//! # Ok::<(), clipper2_rs::ClipperError>(())
//! ```
//!
//! Integer paths (`Paths64`) are the native form; the `*_d` functions and [`ClipperD`] take
//! floating-point paths and scale them by a precision in decimal places. The `usingz` feature
//! adds a user `z` value to every point (it is not additive: see the README).
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod api;
mod clipper_d;
mod core;
mod engine;
mod gnu_sort;
mod minkowski;
mod offset;
mod polytree;
mod rectclip;
mod triangulation;
mod utils;

pub use crate::core::{
    ClipType, ClipperError, Coord, FillRule, MAX_COORD, MAX_DECIMAL_PRECISION, MIN_COORD, Path,
    Path64, PathD, Paths, Paths64, PathsD, Point, Point64, PointD, PointInPolygonResult, Rect,
    Rect64, RectD, area, area_paths, check_precision, get_bounds, get_bounds_paths, is_positive,
    mid_point, point_in_polygon, scale_path, scale_paths, scale_paths_64_to_d, scale_paths_d_to_64,
    strip_duplicates, strip_duplicates_paths, strip_near_equal, strip_near_equal_paths,
    transform_path, transform_paths,
};
pub use api::{
    boolean_op, boolean_op_d, boolean_op_tree, boolean_op_tree_d, difference, difference_d,
    intersect, intersect_d, union, union_d, union_self, union_self_d, xor, xor_d,
};
#[cfg(feature = "usingz")]
pub use clipper_d::ZCallbackD;
pub use clipper_d::{ClipperD, SolutionD};
#[cfg(feature = "usingz")]
pub use engine::ZCallback64;
pub use engine::{Clipper64, PathType, ReuseableDataContainer64, Solution};
pub use minkowski::{minkowski_diff, minkowski_diff_d, minkowski_sum, minkowski_sum_d};
pub use offset::{
    ClipperOffset, DeltaCallback64, EndType, JoinType, inflate_paths, inflate_paths_d,
};
pub use polytree::{PolyNode, PolyNode64, PolyNodeD, PolyTree, PolyTree64, PolyTreeD};
pub use rectclip::{
    RectClip64, RectClipLines64, rect_clip, rect_clip_d, rect_clip_lines, rect_clip_lines_d,
    rect_clip_lines_path, rect_clip_path,
};
pub use triangulation::{TriangulateError, triangulate, triangulate_d};
#[cfg(feature = "usingz")]
pub use utils::make_path_z;
pub use utils::{
    check_polytree_fully_contains_children, distance, ellipse, ellipse_rect, length, make_path,
    make_path_d, near_collinear, poly_tree_to_paths, ramer_douglas_peucker,
    ramer_douglas_peucker_paths, simplify_path, simplify_paths, translate_path, translate_paths,
    trim_collinear, trim_collinear_d,
};
