//! Core types and geometry helpers (`clipper.core.h`).
//!
//! Every function keeps the C++ evaluation order: the results are compared bit for bit
//! against the upstream library.

use std::fmt;
use std::ops::{Add, Sub};

/// Largest coordinate accepted by the integer engine (`INT64_MAX >> 2`).
pub const MAX_COORD: i64 = i64::MAX >> 2;
/// Smallest coordinate accepted by the integer engine.
pub const MIN_COORD: i64 = -MAX_COORD;
/// Largest number of decimal places accepted by the floating-point APIs.
pub const MAX_DECIMAL_PRECISION: i32 = 8;

/// Errors reported by Clipper2 operations (the C++ library throws `Clipper2Exception`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClipperError {
    /// A coordinate (after scaling, for the floating-point APIs) is outside `MIN_COORD..=MAX_COORD`.
    CoordinateOutOfRange,
    /// A decimal precision is outside `-8..=8`.
    PrecisionOutOfRange,
    /// A scale is zero, NaN or infinite.
    InvalidScale,
    /// The clipping engine could not complete the operation.
    ExecuteFailed,
    /// A flat coordinate list does not hold whole points.
    OddCoordinateCount,
    /// An offset delta is NaN or infinite (upstream would loop or allocate without bound).
    InvalidDelta,
}

impl fmt::Display for ClipperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CoordinateOutOfRange => "Values exceed permitted range",
            Self::PrecisionOutOfRange => "Precision exceeds the permitted range",
            Self::InvalidScale => "Scale must be finite and non-zero",
            Self::ExecuteFailed => "Clipping operation failed",
            Self::OddCoordinateCount => "There must be 2 values for each coordinate",
            Self::InvalidDelta => "Offset delta is not finite",
        })
    }
}

impl std::error::Error for ClipperError {}

/// Polygon filling rules.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FillRule {
    /// Only odd-numbered sub-regions are filled (also called Alternate).
    #[default]
    EvenOdd = 0,
    /// Only non-zero sub-regions are filled (also called Winding).
    NonZero = 1,
    /// Only sub-regions with winding counts `> 0` are filled.
    Positive = 2,
    /// Only sub-regions with winding counts `< 0` are filled.
    Negative = 3,
}

/// Boolean operations.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ClipType {
    /// No clipping operation.
    #[default]
    NoClip = 0,
    /// AND: regions covered by both subject and clip polygons.
    Intersection = 1,
    /// OR: regions covered by subject or clip polygons, or both.
    Union = 2,
    /// NOT: regions covered by subject but not clip polygons.
    Difference = 3,
    /// Exclusive or: regions covered by subject or clip polygons, but not both.
    Xor = 4,
}

/// Result of [`point_in_polygon`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PointInPolygonResult {
    /// The point lies on the polygon's boundary.
    IsOn,
    /// The point is inside the polygon.
    IsInside,
    /// The point is outside the polygon.
    IsOutside,
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for i64 {}
    impl Sealed for f64 {}
}

/// Coordinate type of a [`Point`]: `i64` or `f64`. Sealed: only these two implement it.
pub trait Coord:
    sealed::Sealed + Copy + PartialOrd + PartialEq + fmt::Debug + Default + Send + Sync + 'static
{
    /// True for `i64`.
    const INTEGRAL: bool;
    /// Largest value (`std::numeric_limits<T>::max()`).
    const MAX: Self;
    /// Smallest value (`std::numeric_limits<T>::lowest()`).
    const LOWEST: Self;
    /// The value as `f64`.
    fn to_f64(self) -> f64;
    /// `static_cast<T>(v)` for integers (truncation), identity for `f64`.
    fn from_f64_cast(v: f64) -> Self;
    /// `Point<T>` construction from a `double`: `std::round` for integers.
    fn from_f64_round(v: f64) -> Self;
    /// `static_cast<T>(v)` from `i64`.
    fn from_i64(v: i64) -> Self;
    /// The value itself for `i64`; `static_cast<int64_t>` (truncation) for `f64`.
    fn as_i64(self) -> i64;
    /// `static_cast<__int128_t>(v)`.
    fn as_i128(self) -> i128;
    /// `self + o` (wrapping for `i64`).
    fn add(self, o: Self) -> Self;
    /// `self - o` (wrapping for `i64`).
    fn sub(self, o: Self) -> Self;
    /// `self * o` (wrapping for `i64`).
    fn mul(self, o: Self) -> Self;
    /// `self / 2` (truncating for `i64`).
    fn half(self) -> Self;
    /// True if the value is zero.
    fn is_zero(self) -> bool;
}

impl Coord for i64 {
    const INTEGRAL: bool = true;
    const MAX: Self = i64::MAX;
    const LOWEST: Self = i64::MIN;
    #[inline]
    fn to_f64(self) -> f64 {
        self as f64
    }
    #[inline]
    fn from_f64_cast(v: f64) -> Self {
        v as i64
    }
    #[inline]
    fn from_f64_round(v: f64) -> Self {
        v.round() as i64
    }
    #[inline]
    fn from_i64(v: i64) -> Self {
        v
    }
    #[inline]
    fn as_i64(self) -> i64 {
        self
    }
    #[inline]
    fn as_i128(self) -> i128 {
        self as i128
    }
    #[inline]
    fn add(self, o: Self) -> Self {
        self.wrapping_add(o)
    }
    #[inline]
    fn sub(self, o: Self) -> Self {
        self.wrapping_sub(o)
    }
    #[inline]
    fn mul(self, o: Self) -> Self {
        self.wrapping_mul(o)
    }
    #[inline]
    fn half(self) -> Self {
        self / 2
    }
    #[inline]
    fn is_zero(self) -> bool {
        self == 0
    }
}

impl Coord for f64 {
    const INTEGRAL: bool = false;
    const MAX: Self = f64::MAX;
    const LOWEST: Self = f64::MIN;
    #[inline]
    fn to_f64(self) -> f64 {
        self
    }
    #[inline]
    fn from_f64_cast(v: f64) -> Self {
        v
    }
    #[inline]
    fn from_f64_round(v: f64) -> Self {
        v
    }
    #[inline]
    fn from_i64(v: i64) -> Self {
        v as f64
    }
    #[inline]
    fn as_i64(self) -> i64 {
        self as i64
    }
    #[inline]
    fn as_i128(self) -> i128 {
        self as i128
    }
    #[inline]
    fn add(self, o: Self) -> Self {
        self + o
    }
    #[inline]
    fn sub(self, o: Self) -> Self {
        self - o
    }
    #[inline]
    fn mul(self, o: Self) -> Self {
        self * o
    }
    #[inline]
    fn half(self) -> Self {
        self / 2.0
    }
    #[inline]
    fn is_zero(self) -> bool {
        self == 0.0
    }
}

/// A 2D point; with the `usingz` feature it also carries a user `z` value that does not take
/// part in equality.
#[derive(Clone, Copy, Debug, Default)]
pub struct Point<T> {
    /// X coordinate.
    pub x: T,
    /// Y coordinate.
    pub y: T,
    /// User data carried through clipping (`usingz` feature).
    #[cfg(feature = "usingz")]
    pub z: i64,
}

/// Integer point used by the clipping engine.
pub type Point64 = Point<i64>;
/// Floating-point point.
pub type PointD = Point<f64>;
/// A path (polygon or polyline).
pub type Path<T> = Vec<Point<T>>;
/// A list of paths.
pub type Paths<T> = Vec<Path<T>>;
/// An integer path.
pub type Path64 = Path<i64>;
/// A list of integer paths.
pub type Paths64 = Paths<i64>;
/// A floating-point path.
pub type PathD = Path<f64>;
/// A list of floating-point paths.
pub type PathsD = Paths<f64>;

impl<T: Coord> Point<T> {
    /// A point with `z = 0`.
    #[inline]
    pub fn new(x: T, y: T) -> Self {
        Point {
            x,
            y,
            #[cfg(feature = "usingz")]
            z: 0,
        }
    }

    /// A point with a `z` value.
    #[cfg(feature = "usingz")]
    #[inline]
    pub fn new_z(x: T, y: T, z: i64) -> Self {
        Point { x, y, z }
    }

    /// `Point<T>(double x, double y)`: rounds for integer points.
    #[inline]
    pub fn from_f64(x: f64, y: f64) -> Self {
        Self::new(T::from_f64_round(x), T::from_f64_round(y))
    }

    /// Converts between coordinate types (`explicit Point<T>(const Point<T2>&)`), keeping `z`.
    #[inline]
    pub fn convert<U: Coord>(self) -> Point<U> {
        self.with_xy(convert_coord::<T, U>(self.x), convert_coord::<T, U>(self.y))
    }

    /// A point at `(x, y)` carrying this point's `z`.
    #[inline]
    pub(crate) fn with_xy<U: Coord>(self, x: U, y: U) -> Point<U> {
        Point {
            x,
            y,
            #[cfg(feature = "usingz")]
            z: self.z,
        }
    }

    /// `pt * scale` (rounding for integer points), keeping `z`.
    #[inline]
    pub fn scale(self, scale: f64) -> Self {
        self.with_xy(
            T::from_f64_round(self.x.to_f64() * scale),
            T::from_f64_round(self.y.to_f64() * scale),
        )
    }

    #[inline]
    pub(crate) fn z_value(self) -> i64 {
        #[cfg(feature = "usingz")]
        {
            self.z
        }
        #[cfg(not(feature = "usingz"))]
        {
            0
        }
    }

    #[inline]
    pub(crate) fn set_z(&mut self, _z: i64) {
        #[cfg(feature = "usingz")]
        {
            self.z = _z;
        }
    }
}

impl<T: Coord> PartialEq for Point<T> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.x == other.x && self.y == other.y
    }
}

impl Eq for Point<i64> {}

impl std::hash::Hash for Point<i64> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
    }
}

impl<T: Coord> Add for Point<T> {
    type Output = Self;
    #[inline]
    fn add(self, b: Self) -> Self {
        Self::new(self.x.add(b.x), self.y.add(b.y))
    }
}

impl<T: Coord> Sub for Point<T> {
    type Output = Self;
    #[inline]
    fn sub(self, b: Self) -> Self {
        Self::new(self.x.sub(b.x), self.y.sub(b.y))
    }
}

/// `static_cast` between coordinate types, rounding `f64` -> `i64` like `Point::Init`.
#[inline]
pub(crate) fn convert_coord<T: Coord, U: Coord>(v: T) -> U {
    match (T::INTEGRAL, U::INTEGRAL) {
        (true, true) => U::from_i64(v.as_i64()),
        (false, true) => U::from_f64_round(v.to_f64()),
        _ => U::from_f64_cast(v.to_f64()),
    }
}

/// The midpoint of two points (integer division for `i64`).
#[inline]
pub fn mid_point<T: Coord>(p1: Point<T>, p2: Point<T>) -> Point<T> {
    Point::new(p1.x.add(p2.x).half(), p1.y.add(p2.y).half())
}

/// An axis-aligned rectangle; `top < bottom` in Clipper2's convention.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect<T> {
    /// Smallest x.
    pub left: T,
    /// Smallest y.
    pub top: T,
    /// Largest x.
    pub right: T,
    /// Largest y.
    pub bottom: T,
}

/// An integer rectangle.
pub type Rect64 = Rect<i64>;
/// A floating-point rectangle.
pub type RectD = Rect<f64>;

impl<T: Coord> Rect<T> {
    /// A rectangle from its edges.
    pub fn new(left: T, top: T, right: T, bottom: T) -> Self {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    /// The empty "invalid" rectangle used as a bounds accumulator.
    pub fn invalid() -> Self {
        Rect {
            left: T::MAX,
            top: T::MAX,
            right: T::LOWEST,
            bottom: T::LOWEST,
        }
    }

    /// False for the [`Rect::invalid`] accumulator.
    pub fn is_valid(&self) -> bool {
        self.left != T::MAX
    }

    /// `right - left`.
    pub fn width(&self) -> T {
        self.right.sub(self.left)
    }

    /// `bottom - top`.
    pub fn height(&self) -> T {
        self.bottom.sub(self.top)
    }

    /// The centre point (integer division for `i64`).
    pub fn mid_point(&self) -> Point<T> {
        Point::new(
            self.left.add(self.right).half(),
            self.top.add(self.bottom).half(),
        )
    }

    /// The rectangle as a 4-point path (`left,top`, `right,top`, `right,bottom`, `left,bottom`).
    pub fn as_path(&self) -> Path<T> {
        vec![
            Point::new(self.left, self.top),
            Point::new(self.right, self.top),
            Point::new(self.right, self.bottom),
            Point::new(self.left, self.bottom),
        ]
    }

    /// True if `pt` is strictly inside.
    pub fn contains_point(&self, pt: Point<T>) -> bool {
        pt.x > self.left && pt.x < self.right && pt.y > self.top && pt.y < self.bottom
    }

    /// True if `rec` lies inside or on the edges.
    pub fn contains_rect(&self, rec: &Rect<T>) -> bool {
        rec.left >= self.left
            && rec.right <= self.right
            && rec.top >= self.top
            && rec.bottom <= self.bottom
    }

    /// True if the rectangle has no area (`bottom <= top` or `right <= left`).
    pub fn is_empty(&self) -> bool {
        self.bottom <= self.top || self.right <= self.left
    }

    /// True if the rectangles overlap or touch.
    pub fn intersects(&self, rec: &Rect<T>) -> bool {
        max(self.left, rec.left) <= min(self.right, rec.right)
            && max(self.top, rec.top) <= min(self.bottom, rec.bottom)
    }

    /// The bounding union of two rectangles (`operator+`).
    pub fn union(&self, o: &Rect<T>) -> Rect<T> {
        Rect {
            left: min(self.left, o.left),
            top: min(self.top, o.top),
            right: max(self.right, o.right),
            bottom: max(self.bottom, o.bottom),
        }
    }

    /// Converts and scales a rectangle (`ScaleRect`), rounding when converting to integers.
    pub fn scale_to<U: Coord>(&self, scale: f64) -> Rect<U> {
        let f = |v: T| {
            let s = v.to_f64() * scale;
            if U::INTEGRAL && !T::INTEGRAL {
                U::from_f64_round(s)
            } else {
                U::from_f64_cast(s)
            }
        };
        Rect {
            left: f(self.left),
            top: f(self.top),
            right: f(self.right),
            bottom: f(self.bottom),
        }
    }
}

/// `(std::min)`: returns `a` unless `b < a`.
#[inline]
pub(crate) fn min<T: PartialOrd>(a: T, b: T) -> T {
    if b < a { b } else { a }
}

/// `(std::max)`: returns `a` unless `a < b`.
#[inline]
pub(crate) fn max<T: PartialOrd>(a: T, b: T) -> T {
    if a < b { b } else { a }
}

/// Bounds of one path; [`Rect::invalid`] when empty.
pub fn get_bounds<T: Coord>(path: &[Point<T>]) -> Rect<T> {
    let mut r = Rect::invalid();
    for p in path {
        if p.x < r.left {
            r.left = p.x;
        }
        if p.x > r.right {
            r.right = p.x;
        }
        if p.y < r.top {
            r.top = p.y;
        }
        if p.y > r.bottom {
            r.bottom = p.y;
        }
    }
    r
}

/// Bounds of several paths; [`Rect::invalid`] when empty.
pub fn get_bounds_paths<T: Coord>(paths: &[Path<T>]) -> Rect<T> {
    let mut r = Rect::invalid();
    for p in paths.iter().flatten() {
        if p.x < r.left {
            r.left = p.x;
        }
        if p.x > r.right {
            r.right = p.x;
        }
        if p.y < r.top {
            r.top = p.y;
        }
        if p.y > r.bottom {
            r.bottom = p.y;
        }
    }
    r
}

/// Fails if any coordinate is outside `MIN_COORD..=MAX_COORD`.
pub(crate) fn check_range(paths: &[Path64]) -> Result<(), ClipperError> {
    let ok = paths
        .iter()
        .flatten()
        .all(|p| (MIN_COORD..=MAX_COORD).contains(&p.x) && (MIN_COORD..=MAX_COORD).contains(&p.y));
    if ok {
        Ok(())
    } else {
        Err(ClipperError::CoordinateOutOfRange)
    }
}

/// The scale `10^precision` for a decimal precision in `-8..=8`.
pub fn check_precision(precision: i32) -> Result<f64, ClipperError> {
    if (-MAX_DECIMAL_PRECISION..=MAX_DECIMAL_PRECISION).contains(&precision) {
        Ok(10f64.powi(precision))
    } else {
        Err(ClipperError::PrecisionOutOfRange)
    }
}

/// `ScalePath<T1, T2>(path, scale_x, scale_y)`.
/// Scales must be finite and non-zero. Results must be finite, and integer targets
/// must lie in `MIN_COORD..=MAX_COORD` after rounding.
pub fn scale_path<T: Coord, U: Coord>(
    path: &[Point<T>],
    scale_x: f64,
    scale_y: f64,
) -> Result<Path<U>, ClipperError> {
    check_scale(scale_x, scale_y)?;
    path.iter()
        .map(|p| {
            Ok(p.with_xy(
                scale_coord::<T, U>(p.x, scale_x)?,
                scale_coord::<T, U>(p.y, scale_y)?,
            ))
        })
        .collect()
}

#[inline]
fn check_scale(scale_x: f64, scale_y: f64) -> Result<(), ClipperError> {
    if !scale_x.is_finite() || !scale_y.is_finite() || scale_x == 0.0 || scale_y == 0.0 {
        Err(ClipperError::InvalidScale)
    } else {
        Ok(())
    }
}

#[inline]
fn scale_coord<T: Coord, U: Coord>(v: T, scale: f64) -> Result<U, ClipperError> {
    let scaled = v.to_f64() * scale;
    if !scaled.is_finite() {
        return Err(ClipperError::CoordinateOutOfRange);
    }
    let result = U::from_f64_round(scaled);
    if U::INTEGRAL && !(MIN_COORD..=MAX_COORD).contains(&result.as_i64()) {
        return Err(ClipperError::CoordinateOutOfRange);
    }
    Ok(result)
}

/// [`scale_path`] for several paths, with the same scale and coordinate checks.
pub fn scale_paths<T: Coord, U: Coord>(
    paths: &[Path<T>],
    scale_x: f64,
    scale_y: f64,
) -> Result<Paths<U>, ClipperError> {
    check_scale(scale_x, scale_y)?;
    paths
        .iter()
        .map(|p| scale_path(p, scale_x, scale_y))
        .collect()
}

/// Scales floating-point paths into the integer domain.
pub fn scale_paths_d_to_64(paths: &PathsD, scale: f64) -> Result<Paths64, ClipperError> {
    scale_paths(paths, scale, scale)
}

/// Scales integer paths into floating-point paths.
pub fn scale_paths_64_to_d(paths: &Paths64, scale: f64) -> Result<PathsD, ClipperError> {
    scale_paths(paths, scale, scale)
}

/// `TransformPath<T1, T2>`: converts coordinates without scaling.
pub fn transform_path<T: Coord, U: Coord>(path: &[Point<T>]) -> Path<U> {
    path.iter().map(|p| p.convert()).collect()
}

/// `TransformPaths<T1, T2>`.
pub fn transform_paths<T: Coord, U: Coord>(paths: &[Path<T>]) -> Paths<U> {
    paths.iter().map(|p| transform_path(p)).collect()
}

/// `Sqr`: the square as `f64`.
#[inline]
pub(crate) fn sqr<T: Coord>(v: T) -> f64 {
    v.to_f64() * v.to_f64()
}

#[inline]
pub(crate) fn near_equal<T: Coord>(p1: Point<T>, p2: Point<T>, max_dist_sqrd: f64) -> bool {
    sqr(p1.x.sub(p2.x)) + sqr(p1.y.sub(p2.y)) < max_dist_sqrd
}

/// Removes consecutive points closer than `sqrt(max_dist_sqrd)`.
pub fn strip_near_equal<T: Coord>(
    path: &[Point<T>],
    max_dist_sqrd: f64,
    is_closed_path: bool,
) -> Path<T> {
    let Some((&first, rest)) = path.split_first() else {
        return Vec::new();
    };
    let mut result = Vec::with_capacity(path.len());
    let mut last = first;
    result.push(first);
    for &p in rest {
        if !near_equal(p, last, max_dist_sqrd) {
            last = p;
            result.push(last);
        }
    }
    if !is_closed_path {
        return result;
    }
    while result.len() > 1 && near_equal(*result.last().unwrap(), first, max_dist_sqrd) {
        result.pop();
    }
    result
}

/// `StripNearEqual` for several paths.
pub fn strip_near_equal_paths<T: Coord>(
    paths: &[Path<T>],
    max_dist_sqrd: f64,
    is_closed_path: bool,
) -> Paths<T> {
    paths
        .iter()
        .map(|p| strip_near_equal(p, max_dist_sqrd, is_closed_path))
        .collect()
}

/// Removes consecutive duplicates (and, for closed paths, trailing copies of the first point).
pub fn strip_duplicates<T: Coord>(path: &mut Path<T>, is_closed_path: bool) {
    path.dedup();
    if is_closed_path {
        while path.len() > 1 && path.last() == path.first() {
            path.pop();
        }
    }
}

/// `StripDuplicates` for several paths.
pub fn strip_duplicates_paths<T: Coord>(paths: &mut Paths<T>, is_closed_path: bool) {
    for p in paths {
        strip_duplicates(p, is_closed_path);
    }
}

/// `a * b == c * d` without overflow.
#[inline]
pub(crate) fn products_are_equal(a: i64, b: i64, c: i64, d: i64) -> bool {
    (a as i128) * (b as i128) == (c as i128) * (d as i128)
}

/// Exact collinearity of `pt1`, `shared`, `pt2`.
#[inline]
pub(crate) fn is_collinear(pt1: Point64, shared: Point64, pt2: Point64) -> bool {
    let a = shared.x.wrapping_sub(pt1.x);
    let b = pt2.y.wrapping_sub(shared.y);
    let c = shared.y.wrapping_sub(pt1.y);
    let d = pt2.x.wrapping_sub(shared.x);
    products_are_equal(a, b, c, d)
}

/// Cross product of `pt1->pt2` and `pt2->pt3`.
#[inline]
pub(crate) fn cross_product<T: Coord>(pt1: Point<T>, pt2: Point<T>, pt3: Point<T>) -> f64 {
    pt2.x.sub(pt1.x).to_f64() * pt3.y.sub(pt2.y).to_f64()
        - pt2.y.sub(pt1.y).to_f64() * pt3.x.sub(pt2.x).to_f64()
}

/// Cross product of two vectors (`vec1.y * vec2.x - vec2.y * vec1.x`, multiplied in `T`).
#[inline]
pub(crate) fn cross_product_vec<T: Coord>(vec1: Point<T>, vec2: Point<T>) -> f64 {
    vec1.y.mul(vec2.x).to_f64() - vec2.y.mul(vec1.x).to_f64()
}

/// Dot product of `pt1->pt2` and `pt2->pt3`.
#[inline]
pub(crate) fn dot_product<T: Coord>(pt1: Point<T>, pt2: Point<T>, pt3: Point<T>) -> f64 {
    pt2.x.sub(pt1.x).to_f64() * pt3.x.sub(pt2.x).to_f64()
        + pt2.y.sub(pt1.y).to_f64() * pt3.y.sub(pt2.y).to_f64()
}

/// Dot product of two vectors (multiplied in `T`).
#[inline]
pub(crate) fn dot_product_vec<T: Coord>(vec1: Point<T>, vec2: Point<T>) -> f64 {
    vec1.x.mul(vec2.x).to_f64() + vec1.y.mul(vec2.y).to_f64()
}

#[inline]
pub(crate) fn distance_sqr<T: Coord>(pt1: Point<T>, pt2: Point<T>) -> f64 {
    sqr(pt1.x.sub(pt2.x)) + sqr(pt1.y.sub(pt2.y))
}

/// Squared perpendicular distance of `pt` from the line through `line1` and `line2`.
#[inline]
pub(crate) fn perpendic_dist_from_line_sqrd<T: Coord>(
    pt: Point<T>,
    line1: Point<T>,
    line2: Point<T>,
) -> f64 {
    let a = pt.x.sub(line1.x).to_f64();
    let b = pt.y.sub(line1.y).to_f64();
    let c = line2.x.sub(line1.x).to_f64();
    let d = line2.y.sub(line1.y).to_f64();
    if c == 0.0 && d == 0.0 {
        return 0.0;
    }
    let v = a * d - c * b;
    v * v / (c * c + d * d)
}

/// Signed area; positive for counter-clockwise paths when the Y axis points up.
pub fn area<T: Coord>(path: &[Point<T>]) -> f64 {
    let cnt = path.len();
    if cnt < 3 {
        return 0.0;
    }
    let mut a = 0.0;
    let mut it2 = cnt - 1;
    let stop = if cnt & 1 == 0 { cnt } else { cnt - 1 };
    let mut it1 = 0;
    while it1 != stop {
        a += path[it2].y.add(path[it1].y).to_f64() * path[it2].x.sub(path[it1].x).to_f64();
        it2 = it1 + 1;
        a += path[it1].y.add(path[it2].y).to_f64() * path[it1].x.sub(path[it2].x).to_f64();
        it1 += 2;
    }
    if cnt & 1 == 1 {
        a += path[it2].y.add(path[it1].y).to_f64() * path[it2].x.sub(path[it1].x).to_f64();
    }
    a * 0.5
}

/// Sum of the signed areas of `paths`.
pub fn area_paths<T: Coord>(paths: &[Path<T>]) -> f64 {
    let mut a = 0.0;
    for p in paths {
        a += area(p);
    }
    a
}

/// True when `area(poly) >= 0`.
pub fn is_positive<T: Coord>(poly: &[Point<T>]) -> bool {
    area(poly) >= 0.0
}

/// Intersection of the lines through two segments, clamped to the first segment
/// (`GetLineIntersectPt`, default precision). `None` for parallel lines. The result's `z` is 0.
pub(crate) fn get_line_intersect_pt<T: Coord>(
    ln1a: Point<T>,
    ln1b: Point<T>,
    ln2a: Point<T>,
    ln2b: Point<T>,
) -> Option<Point<T>> {
    let dx1 = ln1b.x.sub(ln1a.x).to_f64();
    let dy1 = ln1b.y.sub(ln1a.y).to_f64();
    let dx2 = ln2b.x.sub(ln2a.x).to_f64();
    let dy2 = ln2b.y.sub(ln2a.y).to_f64();
    let det = dy1 * dx2 - dy2 * dx1;
    if det == 0.0 {
        return None;
    }
    let t = (ln1a.x.sub(ln2a.x).to_f64() * dy2 - ln1a.y.sub(ln2a.y).to_f64() * dx2) / det;
    Some(if t <= 0.0 {
        ln1a
    } else if t >= 1.0 {
        ln1b
    } else {
        Point::new(
            T::from_f64_cast(ln1a.x.to_f64() + t * dx1),
            T::from_f64_cast(ln1a.y.to_f64() + t * dy1),
        )
    })
}

/// `TranslatePoint`: `pt + (dx, dy)`, rounding for integers, keeping `z`.
#[inline]
pub(crate) fn translate_point<T: Coord>(pt: Point<T>, dx: f64, dy: f64) -> Point<T> {
    pt.with_xy(
        T::from_f64_round(pt.x.to_f64() + dx),
        T::from_f64_round(pt.y.to_f64() + dy),
    )
}

/// `ReflectPoint`: `pt` reflected through `pivot`, keeping `pt.z`.
#[inline]
pub(crate) fn reflect_point<T: Coord>(pt: Point<T>, pivot: Point<T>) -> Point<T> {
    pt.with_xy(
        pivot.x.add(pivot.x.sub(pt.x)),
        pivot.y.add(pivot.y.sub(pt.y)),
    )
}

/// True if two segments cross; touching counts only when `inclusive`.
pub(crate) fn segments_intersect(
    seg1a: Point64,
    seg1b: Point64,
    seg2a: Point64,
    seg2b: Point64,
    inclusive: bool,
) -> bool {
    let dy1 = seg1b.y.wrapping_sub(seg1a.y) as f64;
    let dx1 = seg1b.x.wrapping_sub(seg1a.x) as f64;
    let dy2 = seg2b.y.wrapping_sub(seg2a.y) as f64;
    let dx2 = seg2b.x.wrapping_sub(seg2a.x) as f64;
    let cp = dy1 * dx2 - dy2 * dx1;
    if cp == 0.0 {
        return false;
    }
    let ax = seg1a.x.wrapping_sub(seg2a.x) as f64;
    let ay = seg1a.y.wrapping_sub(seg2a.y) as f64;
    if inclusive {
        let t = ax * dy2 - ay * dx2;
        if t == 0.0 {
            return true;
        }
        if t > 0.0 {
            if cp < 0.0 || t > cp {
                return false;
            }
        } else if cp > 0.0 || t < cp {
            return false;
        }
        let t = ax * dy1 - ay * dx1;
        if t == 0.0 {
            return true;
        }
        if t > 0.0 {
            cp > 0.0 && t <= cp
        } else {
            cp < 0.0 && t >= cp
        }
    } else {
        let t = ax * dy2 - ay * dx2;
        if t == 0.0 {
            return false;
        }
        if t > 0.0 {
            if cp < 0.0 || t >= cp {
                return false;
            }
        } else if cp > 0.0 || t <= cp {
            return false;
        }
        let t = ax * dy1 - ay * dx1;
        if t == 0.0 {
            return false;
        }
        if t > 0.0 {
            cp > 0.0 && t < cp
        } else {
            cp < 0.0 && t > cp
        }
    }
}

/// The point of segment `seg1-seg2` closest to `off_pt` (`nearbyint` for integers).
pub(crate) fn get_closest_point_on_segment<T: Coord>(
    off_pt: Point<T>,
    seg1: Point<T>,
    seg2: Point<T>,
) -> Point<T> {
    if seg1.x == seg2.x && seg1.y == seg2.y {
        return seg1;
    }
    let dx = seg2.x.sub(seg1.x).to_f64();
    let dy = seg2.y.sub(seg1.y).to_f64();
    let mut q = (off_pt.x.sub(seg1.x).to_f64() * dx + off_pt.y.sub(seg1.y).to_f64() * dy)
        / (dx * dx + dy * dy);
    q = q.clamp(0.0, 1.0);
    if T::INTEGRAL {
        Point::new(
            seg1.x.add(T::from_f64_cast((q * dx).round_ties_even())),
            seg1.y.add(T::from_f64_cast((q * dy).round_ties_even())),
        )
    } else {
        Point::new(
            seg1.x.add(T::from_f64_cast(q * dx)),
            seg1.y.add(T::from_f64_cast(q * dy)),
        )
    }
}

/// Winding test of `pt` against `polygon` (`PointInPolygon`).
pub fn point_in_polygon<T: Coord>(pt: Point<T>, polygon: &[Point<T>]) -> PointInPolygonResult {
    use PointInPolygonResult::*;
    let len = polygon.len();
    if len < 3 {
        return IsOutside;
    }
    let mut val = 0;
    let cbegin = 0;
    let mut first = 0;
    let mut cend = len;
    while first != cend && polygon[first].y == pt.y {
        first += 1;
    }
    if first == cend {
        return IsOutside;
    }
    let mut is_above = polygon[first].y < pt.y;
    let starting_above = is_above;
    let mut curr = first + 1;
    let mut prev;
    loop {
        if curr == cend {
            if cend == first || first == cbegin {
                break;
            }
            cend = first;
            curr = cbegin;
        }
        if is_above {
            while curr != cend && polygon[curr].y < pt.y {
                curr += 1;
            }
            if curr == cend {
                continue;
            }
        } else {
            while curr != cend && polygon[curr].y > pt.y {
                curr += 1;
            }
            if curr == cend {
                continue;
            }
        }
        prev = if curr == cbegin { len - 1 } else { curr - 1 };
        let (c, p) = (polygon[curr], polygon[prev]);
        if c.y == pt.y {
            if c.x == pt.x || (c.y == p.y && ((pt.x < p.x) != (pt.x < c.x))) {
                return IsOn;
            }
            curr += 1;
            if curr == first {
                break;
            }
            continue;
        }
        if pt.x < c.x && pt.x < p.x {
            // both edge ends are right of pt: no crossing
        } else if pt.x > p.x && pt.x > c.x {
            val = 1 - val;
        } else {
            let d = cross_product_sign_t(p, c, pt);
            if d == 0 {
                return IsOn;
            }
            if (d < 0) == is_above {
                val = 1 - val;
            }
        }
        is_above = !is_above;
        curr += 1;
    }
    if is_above != starting_above {
        cend = len;
        if curr == cend {
            curr = cbegin;
        }
        prev = if curr == cbegin { cend - 1 } else { curr - 1 };
        let d = cross_product_sign_t(polygon[prev], polygon[curr], pt);
        if d == 0 {
            return IsOn;
        }
        if (d < 0) == is_above {
            val = 1 - val;
        }
    }
    if val == 0 { IsOutside } else { IsInside }
}

/// `CrossProductSign<T>`: the operand differences are cast to 128-bit integers (truncating
/// `f64`, as the C++ template does) and the products compared.
#[inline]
pub(crate) fn cross_product_sign_t<T: Coord>(pt1: Point<T>, pt2: Point<T>, pt3: Point<T>) -> i32 {
    let a = pt2.x.sub(pt1.x).as_i128();
    let b = pt3.y.sub(pt2.y).as_i128();
    let c = pt2.y.sub(pt1.y).as_i128();
    let d = pt3.x.sub(pt2.x).as_i128();
    let (ab, cd) = (a.wrapping_mul(b), c.wrapping_mul(d));
    if ab > cd {
        1
    } else if ab < cd {
        -1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(pts: &[(i64, i64)]) -> Path64 {
        pts.iter().map(|&(x, y)| Point64::new(x, y)).collect()
    }

    #[test]
    fn area_of_ccw_unit_square_is_positive_one() {
        let p = path(&[(0, 0), (1, 0), (1, 1), (0, 1)]);
        assert_eq!(area(&p), 1.0);
        assert!(is_positive(&p));
    }

    #[test]
    fn area_of_cw_square_is_negative() {
        assert_eq!(area(&path(&[(0, 0), (0, 1), (1, 1), (1, 0)])), -1.0);
    }

    #[test]
    fn products_are_equal_detects_collinearity_at_max_coord() {
        let a = Point64::new(0, 0);
        let b = Point64::new(MAX_COORD, MAX_COORD);
        let c = Point64::new(MAX_COORD / 2, MAX_COORD / 2);
        assert!(is_collinear(a, c, b));
        assert!(!is_collinear(a, c, Point64::new(MAX_COORD, MAX_COORD - 1)));
    }

    #[test]
    fn point_in_polygon_on_inside_outside() {
        let sq = path(&[(0, 0), (10, 0), (10, 10), (0, 10)]);
        assert_eq!(
            point_in_polygon(Point64::new(5, 5), &sq),
            PointInPolygonResult::IsInside
        );
        assert_eq!(
            point_in_polygon(Point64::new(0, 5), &sq),
            PointInPolygonResult::IsOn
        );
        assert_eq!(
            point_in_polygon(Point64::new(11, 5), &sq),
            PointInPolygonResult::IsOutside
        );
    }

    #[test]
    fn coordinates_beyond_max_coord_are_rejected() {
        assert!(check_range(&[path(&[(MAX_COORD, MIN_COORD)])]).is_ok());
        assert_eq!(
            check_range(&[path(&[(MAX_COORD + 1, 0)])]),
            Err(ClipperError::CoordinateOutOfRange)
        );
        assert_eq!(
            check_range(&[path(&[(0, MIN_COORD - 1)])]),
            Err(ClipperError::CoordinateOutOfRange)
        );
    }

    #[test]
    fn precision_9_is_rejected() {
        assert_eq!(check_precision(8), Ok(1e8));
        assert_eq!(check_precision(-8), Ok(1e-8));
        assert_eq!(check_precision(9), Err(ClipperError::PrecisionOutOfRange));
        assert_eq!(check_precision(-9), Err(ClipperError::PrecisionOutOfRange));
    }

    #[test]
    fn scaling_d_to_64_rounds_half_away_from_zero_and_checks_range() {
        let p = vec![vec![PointD::new(0.125, -0.125)]];
        assert_eq!(
            scale_paths_d_to_64(&p, 100.0).unwrap()[0][0],
            Point64::new(13, -13)
        );
        let far = vec![vec![PointD::new(1e17, 0.0)]];
        assert_eq!(
            scale_paths_d_to_64(&far, 100.0),
            Err(ClipperError::CoordinateOutOfRange)
        );
    }

    #[test]
    fn segment_intersection_truncates_like_cpp() {
        let ip = get_line_intersect_pt(
            Point64::new(0, 0),
            Point64::new(10, 3),
            Point64::new(0, 3),
            Point64::new(10, 0),
        )
        .unwrap();
        assert_eq!(ip, Point64::new(5, 1));
    }
}
