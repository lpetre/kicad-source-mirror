//! Path utilities from `clipper.h`.

use std::f64::consts::PI;

use crate::core::{
    ClipperError, Coord, Path, Path64, PathD, Paths, Point, PointInPolygonResult, Rect,
    check_precision, distance_sqr, is_collinear, perpendic_dist_from_line_sqrd, point_in_polygon,
    scale_path, sqr,
};
use crate::polytree::{PolyTree, PolyTree64};

/// An ellipse approximated by `steps` points (`steps <= 2` picks a count from the radii).
/// `radius_y <= 0` means a circle.
/// Returns an empty path for non-finite radii, non-positive `radius_x`, or a point
/// count that cannot be allocated.
pub fn ellipse<T: Coord>(
    center: Point<T>,
    radius_x: f64,
    mut radius_y: f64,
    mut steps: usize,
) -> Path<T> {
    if !radius_x.is_finite() || !radius_y.is_finite() || radius_x <= 0.0 {
        return Vec::new();
    }
    if radius_y <= 0.0 {
        radius_y = radius_x;
    }
    if steps <= 2 {
        steps = (PI * ((radius_x + radius_y) / 2.0).sqrt()) as usize;
    }
    let mut result = Vec::new();
    if result.try_reserve_exact(steps.max(1)).is_err() {
        return result;
    }
    let si = (2.0 * PI / steps as f64).sin();
    let co = (2.0 * PI / steps as f64).cos();
    let (mut dx, mut dy) = (co, si);
    let cx = center.x.to_f64();
    let cy = center.y.to_f64();
    result.push(Point::from_f64(cx + radius_x, cy));
    for _ in 1..steps {
        result.push(Point::from_f64(cx + radius_x * dx, cy + radius_y * dy));
        let x = dx * co - dy * si;
        dy = dy * co + dx * si;
        dx = x;
    }
    result
}

/// [`ellipse`] inscribed in a rectangle.
pub fn ellipse_rect<T: Coord>(rect: &Rect<T>, steps: usize) -> Path<T> {
    ellipse(
        rect.mid_point(),
        rect.width().to_f64() * 0.5,
        rect.height().to_f64() * 0.5,
        steps,
    )
}

fn get_next(mut current: usize, high: usize, flags: &[bool]) -> usize {
    current += 1;
    while current <= high && flags[current] {
        current += 1;
    }
    if current <= high {
        return current;
    }
    current = 0;
    while flags[current] {
        current += 1;
    }
    current
}

fn get_prior(mut current: usize, high: usize, flags: &[bool]) -> usize {
    if current == 0 {
        current = high;
    } else {
        current -= 1;
    }
    while current > 0 && flags[current] {
        current -= 1;
    }
    if !flags[current] {
        return current;
    }
    current = high;
    while flags[current] {
        current -= 1;
    }
    current
}

/// Removes vertices closer than `epsilon` to the line through their neighbours (`SimplifyPath`).
pub fn simplify_path<T: Coord>(path: &[Point<T>], epsilon: f64, is_closed_path: bool) -> Path<T> {
    let len = path.len();
    if len < 4 {
        return path.to_vec();
    }
    let high = len - 1;
    let eps_sqr = sqr(epsilon);
    let mut flags = vec![false; len];
    let mut dist_sqr = vec![0.0; len];
    let mut curr = 0usize;
    if is_closed_path {
        dist_sqr[0] = perpendic_dist_from_line_sqrd(path[0], path[high], path[1]);
        dist_sqr[high] = perpendic_dist_from_line_sqrd(path[high], path[0], path[high - 1]);
    } else {
        dist_sqr[0] = f64::MAX;
        dist_sqr[high] = f64::MAX;
    }
    for i in 1..high {
        dist_sqr[i] = perpendic_dist_from_line_sqrd(path[i], path[i - 1], path[i + 1]);
    }
    loop {
        if dist_sqr[curr] > eps_sqr {
            let start = curr;
            loop {
                curr = get_next(curr, high, &flags);
                if curr == start || dist_sqr[curr] <= eps_sqr {
                    break;
                }
            }
            if curr == start {
                break;
            }
        }
        let mut prior = get_prior(curr, high, &flags);
        let mut next = get_next(curr, high, &flags);
        if next == prior {
            break;
        }
        let prior2;
        if dist_sqr[next] < dist_sqr[curr] {
            prior2 = prior;
            prior = curr;
            curr = next;
            next = get_next(next, high, &flags);
        } else {
            prior2 = get_prior(prior, high, &flags);
        }
        flags[curr] = true;
        curr = next;
        next = get_next(next, high, &flags);
        if is_closed_path || (curr != high && curr != 0) {
            dist_sqr[curr] = perpendic_dist_from_line_sqrd(path[curr], path[prior], path[next]);
        }
        if is_closed_path || (prior != 0 && prior != high) {
            dist_sqr[prior] = perpendic_dist_from_line_sqrd(path[prior], path[prior2], path[curr]);
        }
    }
    let mut result = Vec::with_capacity(len);
    result.extend(
        path.iter()
            .zip(&flags)
            .filter(|(_, f)| !**f)
            .map(|(p, _)| *p),
    );
    result
}

/// [`simplify_path`] for several paths.
pub fn simplify_paths<T: Coord>(paths: &[Path<T>], epsilon: f64, is_closed_path: bool) -> Paths<T> {
    paths
        .iter()
        .map(|p| simplify_path(p, epsilon, is_closed_path))
        .collect()
}

fn rdp<T: Coord>(
    path: &[Point<T>],
    begin: usize,
    mut end: usize,
    eps_sqrd: f64,
    flags: &mut [bool],
) {
    let mut idx = 0;
    let mut max_d = 0.0;
    while end > begin && path[begin] == path[end] {
        flags[end] = false;
        end -= 1;
    }
    for i in begin + 1..end {
        let d = perpendic_dist_from_line_sqrd(path[i], path[begin], path[end]);
        if d <= max_d {
            continue;
        }
        max_d = d;
        idx = i;
    }
    if max_d <= eps_sqrd {
        return;
    }
    flags[idx] = true;
    if idx > begin + 1 {
        rdp(path, begin, idx, eps_sqrd, flags);
    }
    if idx < end - 1 {
        rdp(path, idx, end, eps_sqrd, flags);
    }
}

/// Ramer-Douglas-Peucker simplification.
pub fn ramer_douglas_peucker<T: Coord>(path: &[Point<T>], epsilon: f64) -> Path<T> {
    let len = path.len();
    if len < 5 {
        return path.to_vec();
    }
    let mut flags = vec![false; len];
    flags[0] = true;
    flags[len - 1] = true;
    rdp(path, 0, len - 1, sqr(epsilon), &mut flags);
    path.iter()
        .zip(&flags)
        .filter(|(_, f)| **f)
        .map(|(p, _)| *p)
        .collect()
}

/// [`ramer_douglas_peucker`] for several paths.
pub fn ramer_douglas_peucker_paths<T: Coord>(paths: &[Path<T>], epsilon: f64) -> Paths<T> {
    paths
        .iter()
        .map(|p| ramer_douglas_peucker(p, epsilon))
        .collect()
}

/// Removes collinear (and, for closed paths, wrap-around collinear) vertices exactly.
pub fn trim_collinear(p: &Path64, is_open_path: bool) -> Path64 {
    let len = p.len();
    if len < 3 {
        return if !is_open_path || len < 2 || p[0] == p[1] {
            Vec::new()
        } else {
            p.clone()
        };
    }
    let mut dst = Vec::with_capacity(len);
    let mut src = 0;
    let mut stop = len - 1;
    if !is_open_path {
        while src != stop && is_collinear(p[stop], p[src], p[src + 1]) {
            src += 1;
        }
        while src != stop && is_collinear(p[stop - 1], p[stop], p[src]) {
            stop -= 1;
        }
        if src == stop {
            return Vec::new();
        }
    }
    let mut prev = src;
    src += 1;
    dst.push(p[prev]);
    while src != stop {
        if !is_collinear(p[prev], p[src], p[src + 1]) {
            prev = src;
            dst.push(p[prev]);
        }
        src += 1;
    }
    if is_open_path {
        dst.push(p[src]);
    } else if !is_collinear(p[prev], p[stop], dst[0]) {
        dst.push(p[stop]);
    } else {
        while dst.len() > 2 && is_collinear(dst[dst.len() - 1], dst[dst.len() - 2], dst[0]) {
            dst.pop();
        }
        if dst.len() < 3 {
            return Vec::new();
        }
    }
    dst
}

/// Floating-point [`trim_collinear`]; coordinates are scaled by `10^precision`.
pub fn trim_collinear_d(
    path: &PathD,
    precision: i32,
    is_open_path: bool,
) -> Result<PathD, ClipperError> {
    check_precision(precision)?;
    let scale = 10f64.powi(precision);
    let p: Path64 = scale_path(path, scale, scale)?;
    scale_path(&trim_collinear(&p, is_open_path), 1.0 / scale, 1.0 / scale)
}

/// Every polygon of the tree in depth-first pre-order.
pub fn poly_tree_to_paths<T: Coord>(tree: &PolyTree<T>) -> Paths<T> {
    tree.dfs()
        .into_iter()
        .map(|i| tree.node(i).polygon.clone())
        .collect()
}

fn poly_path_contains_children(tree: &PolyTree64, id: usize) -> bool {
    let node = tree.node(id);
    for &child in &node.children {
        let mut outside_cnt = 0;
        for &pt in &tree.node(child).polygon {
            match point_in_polygon(pt, &node.polygon) {
                PointInPolygonResult::IsInside => outside_cnt -= 1,
                PointInPolygonResult::IsOutside => outside_cnt += 1,
                PointInPolygonResult::IsOn => {}
            }
            if outside_cnt > 1 {
                return false;
            } else if outside_cnt < -1 {
                break;
            }
        }
        if !tree.node(child).children.is_empty() && !poly_path_contains_children(tree, child) {
            return false;
        }
    }
    true
}

/// True if every child polygon lies inside its parent.
pub fn check_polytree_fully_contains_children(tree: &PolyTree64) -> bool {
    tree.roots()
        .iter()
        .all(|&r| tree.node(r).children.is_empty() || poly_path_contains_children(tree, r))
}

/// A path from `[x0, y0, x1, y1, ...]`.
pub fn make_path(coords: &[i64]) -> Result<Path64, ClipperError> {
    if coords.len() % 2 != 0 {
        return Err(ClipperError::OddCoordinateCount);
    }
    Ok(coords
        .chunks_exact(2)
        .map(|c| Point::new(c[0], c[1]))
        .collect())
}

/// A floating-point path from `[x0, y0, x1, y1, ...]`.
pub fn make_path_d(coords: &[f64]) -> Result<PathD, ClipperError> {
    if coords.len() % 2 != 0 {
        return Err(ClipperError::OddCoordinateCount);
    }
    Ok(coords
        .chunks_exact(2)
        .map(|c| Point::new(c[0], c[1]))
        .collect())
}

/// A path from `[x0, y0, z0, x1, y1, z1, ...]`.
#[cfg(feature = "usingz")]
pub fn make_path_z(coords: &[i64]) -> Result<Path64, ClipperError> {
    if coords.len() % 3 != 0 {
        return Err(ClipperError::OddCoordinateCount);
    }
    Ok(coords
        .chunks_exact(3)
        .map(|c| Point::new_z(c[0], c[1], c[2]))
        .collect())
}

/// Moves a path by `(dx, dy)`. Like upstream, the result's `z` values are 0.
pub fn translate_path<T: Coord>(path: &[Point<T>], dx: T, dy: T) -> Path<T> {
    path.iter()
        .map(|p| Point::new(p.x.add(dx), p.y.add(dy)))
        .collect()
}

/// [`translate_path`] for several paths.
pub fn translate_paths<T: Coord>(paths: &[Path<T>], dx: T, dy: T) -> Paths<T> {
    paths.iter().map(|p| translate_path(p, dx, dy)).collect()
}

/// Euclidean distance.
pub fn distance<T: Coord>(pt1: Point<T>, pt2: Point<T>) -> f64 {
    distance_sqr(pt1, pt2).sqrt()
}

/// Length of a path (closing segment included when `is_closed_path`).
pub fn length<T: Coord>(path: &[Point<T>], is_closed_path: bool) -> f64 {
    let mut result = 0.0;
    if path.len() < 2 {
        return result;
    }
    for w in path.windows(2) {
        result += distance(w[0], w[1]);
    }
    if is_closed_path {
        result += distance(path[path.len() - 1], path[0]);
    }
    result
}

/// True when the angle at `pt2` is within `asin(sqrt(sin_sqrd_min_angle_rads))` of straight.
pub fn near_collinear<T: Coord>(
    pt1: Point<T>,
    pt2: Point<T>,
    pt3: Point<T>,
    sin_sqrd_min_angle_rads: f64,
) -> bool {
    let cp = crate::core::cross_product(pt1, pt2, pt3).abs();
    (cp * cp) / (distance_sqr(pt1, pt2) * distance_sqr(pt2, pt3)) < sin_sqrd_min_angle_rads
}
