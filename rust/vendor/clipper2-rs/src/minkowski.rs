//! Minkowski sum and difference (`clipper.minkowski.h`).

use crate::core::{
    ClipType, ClipperError, FillRule, Path64, PathD, Paths64, PathsD, check_precision, check_range,
    is_positive, scale_path, scale_paths,
};
use crate::engine::Clipper64;

fn minkowski(
    pattern: &Path64,
    path: &Path64,
    is_sum: bool,
    is_closed: bool,
) -> Result<Paths64, ClipperError> {
    check_range(std::slice::from_ref(pattern))?;
    check_range(std::slice::from_ref(path))?;
    let delta = if is_closed { 0 } else { 1 };
    let (pat_len, path_len) = (pattern.len(), path.len());
    if pat_len == 0 || path_len == 0 {
        return Ok(Vec::new());
    }
    let tmp: Vec<Path64> = path
        .iter()
        .map(|&p| {
            pattern
                .iter()
                .map(|&pt2| if is_sum { p + pt2 } else { p - pt2 })
                .collect()
        })
        .collect();
    let mut result = Vec::with_capacity(path_len.saturating_sub(delta) * pat_len);
    let mut g = if is_closed { path_len - 1 } else { 0 };
    let mut h = pat_len - 1;
    for i in delta..path_len {
        for j in 0..pat_len {
            let mut quad = vec![tmp[g][h], tmp[i][h], tmp[i][j], tmp[g][j]];
            if !is_positive(&quad) {
                quad.reverse();
            }
            result.push(quad);
            h = j;
        }
        g = i;
    }
    Ok(result)
}

fn union(subjects: &Paths64) -> Result<Paths64, ClipperError> {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    Ok(c.execute(ClipType::Union, FillRule::NonZero)?.closed)
}

/// The Minkowski sum of `pattern` swept along `path`.
pub fn minkowski_sum(
    pattern: &Path64,
    path: &Path64,
    is_closed: bool,
) -> Result<Paths64, ClipperError> {
    union(&minkowski(pattern, path, true, is_closed)?)
}

/// The Minkowski difference of `pattern` swept along `path`.
pub fn minkowski_diff(
    pattern: &Path64,
    path: &Path64,
    is_closed: bool,
) -> Result<Paths64, ClipperError> {
    union(&minkowski(pattern, path, false, is_closed)?)
}

fn minkowski_d(
    pattern: &PathD,
    path: &PathD,
    is_sum: bool,
    is_closed: bool,
    decimal_places: i32,
) -> Result<PathsD, ClipperError> {
    let scale = check_precision(decimal_places)?;
    let pat64: Path64 = scale_path(pattern, scale, scale)?;
    let path64: Path64 = scale_path(path, scale, scale)?;
    let tmp = union(&minkowski(&pat64, &path64, is_sum, is_closed)?)?;
    scale_paths(&tmp, 1.0 / scale, 1.0 / scale)
}

/// Floating-point [`minkowski_sum`]; coordinates are scaled by `10^decimal_places`.
pub fn minkowski_sum_d(
    pattern: &PathD,
    path: &PathD,
    is_closed: bool,
    decimal_places: i32,
) -> Result<PathsD, ClipperError> {
    minkowski_d(pattern, path, true, is_closed, decimal_places)
}

/// Floating-point [`minkowski_diff`].
pub fn minkowski_diff_d(
    pattern: &PathD,
    path: &PathD,
    is_closed: bool,
    decimal_places: i32,
) -> Result<PathsD, ClipperError> {
    minkowski_d(pattern, path, false, is_closed, decimal_places)
}
