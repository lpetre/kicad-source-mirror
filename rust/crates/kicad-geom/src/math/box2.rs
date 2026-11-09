// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2012 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
// Copyright (C) 2013 CERN
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>

//! Port of `libs/kimath/include/math/box2.h`.
//!
//! `BOX2<Vec>` is a class template over the vector type.  The port is the struct
//! [`Box2<T>`] over the coordinate type, with the three instantiations KiCad uses:
//! [`Box2I`] (`BOX2<VECTOR2I>`: `int` coordinates, `int64_t` sizes), [`Box2D`]
//! (`BOX2<VECTOR2D>`) and [`Box2L`] (`BOX2<VECTOR2L>`).  The methods are generated per
//! instantiation by a macro so that the mixed coordinate/size arithmetic and the implicit
//! conversions of the C++ are reproduced exactly for each of them.
//!
//! Overload mapping (the C++ name is `Contains`, `Intersects`, ... in each group):
//!
//! | C++ | Rust |
//! |---|---|
//! | `BOX2( aPos, aSize )` / `BOX2( aPos )` / `BOX2()` | `new` / `from_pos` / `default` |
//! | `Contains( Vec )` / `Contains( x, y )` / `Contains( BOX2 )` | `contains` / `contains_xy` / `contains_box` |
//! | `Intersects( BOX2 )` / `Intersects( Vec, Vec )` / `Intersects( BOX2, EDA_ANGLE )` | `intersects` / `intersects_segment` / `intersects_rotated` |
//! | `Inflate( d )` / `Inflate( dx, dy )` | `inflate` / `inflate_xy` |
//! | `GetInflated( d )` / `GetInflated( dx, dy )` | `inflated` / `inflated_xy` |
//! | `Merge( BOX2 )` / `Merge( Vec )` | `merge` / `merge_point` |
//! | `SquaredDistance( Vec )` / `SquaredDistance( BOX2 )` | `squared_distance_point` / `squared_distance_box` |
//! | `Distance( Vec )` / `Distance( BOX2 )` | `distance_point` / `distance_box` |
//! | `SetOrigin( Vec )` / `SetOrigin( x, y )` | `set_origin` / `set_origin_xy` |
//! | `SetSize( SizeVec )` / `SetSize( w, h )` | `set_size` / `set_size_wh` |
//! | `SetEnd( Vec )` / `SetEnd( x, y )` | `set_end` / `set_end_xy` |
//! | `Offset( Vec )` / `Offset( dx, dy )` | `offset` / `offset_xy` |
//! | `Move( Vec )` | `move_` |
//! | `BOX2ISafe( BOX2D )` / `BOX2ISafe( VECTOR2D, VECTOR2D )` / `BOX2ISafe( VECTOR2I, VECTOR2<S> )` | [`box2i_safe`] / [`box2i_safe_from_pos_size`] / [`box2i_safe_from_int`] |
//! | `IsBOX2Safe( box )` | `is_box2_safe` (method) |
//! | `KiROUND( BOX2D )` | [`crate::math::util::ki_round`] |

use crate::geometry::eda_angle::{ANGLE_0, ANGLE_90, ANGLE_180, ANGLE_270, ANGLE_360, DEGREES_T, EdaAngle};
use crate::math::util::{KiRound, ki_checked_cast, ki_round, ki_round_to, std_clamp, std_max, std_min};
use crate::math::vector2::{Coord, Vector2, Vector2D, Vector2I, Vector2L};
use crate::trigo::{rotate_point, rotate_point_around, segment_intersects_segment};

/// Port of `BOX2<Vec>`: a 2D bounding box built on top of an origin point and size vector.
///
/// The origin has the coordinate type `T`, the size the extended type (`int64_t` for
/// `BOX2I`).  A default-constructed box is *not initialized* (`is_valid()` is false) and is
/// replaced, rather than extended, by the first [`merge`](Box2::merge).
///
/// Equality (`==`) compares the normalized positions and sizes and ignores the
/// initialization flag, as `BOX2::operator==` does.
#[derive(Clone, Copy, Debug)]
pub struct Box2<T: Coord> {
    /// Rectangle Origin
    pos: Vector2<T>,
    /// Rectangle Size
    size: Vector2<T::Extended>,
    /// Is the rectangle initialized
    init: bool,
}

/// Port of `BOX2I` (`BOX2<VECTOR2I>`).
pub type Box2I = Box2<i32>;

/// Port of `BOX2D` (`BOX2<VECTOR2D>`).
pub type Box2D = Box2<f64>;

/// Port of `BOX2L` (`BOX2<VECTOR2L>`).
pub type Box2L = Box2<i64>;

/// Port of `OPT_BOX2I`.
pub type OptBox2I = Option<Box2I>;

impl<T: Coord> Default for Box2<T> {
    /// Port of `BOX2::BOX2()`: position and size 0, not initialized.
    #[inline]
    fn default() -> Self {
        Box2 {
            pos: Vector2::new(T::ZERO, T::ZERO),
            size: Vector2::new(<T::Extended as Coord>::ZERO, <T::Extended as Coord>::ZERO),
            init: false,
        }
    }
}

/// Generates the methods of `BOX2<Vec>` for one instantiation: `$c` is `coord_type`, `$e` is
/// `size_type` / `ecoord_type`.
macro_rules! impl_box2 {
    ($c:ty, $e:ty) => {
        #[allow(clippy::unnecessary_cast)]
        impl Box2<$c> {
            /// Port of `BOX2::BOX2( const Vec& aPos, const SizeVec& aSize )`.
            ///
            /// The box is normalized (the size made non-negative).  The C++ constructor
            /// additionally runs a debug-only range check on `pos + size`, which has no
            /// effect in release builds and is not reproduced.
            ///
            /// As in C++, the size may be given as any vector that converts implicitly to
            /// `SizeVec` (e.g. a `Vector2I` for a `Box2I`).
            pub fn new(a_pos: Vector2<$c>, a_size: impl Into<Vector2<$e>>) -> Self {
                let mut b = Box2 { pos: a_pos, size: a_size.into(), init: true };

                b.normalize();
                b
            }

            /// Port of `BOX2::BOX2( const Vec& aPos )` (the size defaults to `( 0, 0 )`).
            pub fn from_pos(a_pos: Vector2<$c>) -> Self {
                Self::new(a_pos, Vector2::<$e>::new(0 as $e, 0 as $e))
            }

            /// Port of `BOX2::ByCorners()`.
            pub fn by_corners(a_corner1: Vector2<$c>, a_corner2: Vector2<$c>) -> Self {
                Self::new(a_corner1, (a_corner2 - a_corner1).cast::<$e>())
            }

            /// Port of `BOX2::ByCenter()`.
            pub fn by_center(a_center: Vector2<$c>, a_size: impl Into<Vector2<$e>>) -> Self {
                let a_size: Vector2<$e> = a_size.into();
                let half: Vector2<$e> = a_size / 2;
                let pos =
                    Vector2::<$e>::new(a_center.x as $e - half.x, a_center.y as $e - half.y).cast::<$c>();

                Self::new(pos, a_size)
            }

            /// Port of `BOX2::Centre()`.
            pub fn centre(&self) -> Vector2<$c> {
                Vector2::new(
                    ki_checked_cast::<$e, $c>(self.pos.x as $e + self.size.x / (2 as $e)),
                    ki_checked_cast::<$e, $c>(self.pos.y as $e + self.size.y / (2 as $e)),
                )
            }

            /// Port of `BOX2::Compute()`.
            ///
            /// Compute the bounding box from a given list of points.  An empty list leaves
            /// the box unchanged.
            pub fn compute(&mut self, a_point_list: &[Vector2<$c>]) {
                if a_point_list.is_empty() {
                    return;
                }

                let mut vmin: Vector2<$c> = a_point_list[0];
                let mut vmax: Vector2<$c> = a_point_list[0];

                for p in a_point_list {
                    vmin.x = std_min(vmin.x, p.x);
                    vmin.y = std_min(vmin.y, p.y);
                    vmax.x = std_max(vmax.x, p.x);
                    vmax.y = std_max(vmax.y, p.y);
                }

                self.set_origin(vmin);
                self.set_size((vmax - vmin).cast::<$e>());
            }

            /// Port of `BOX2::Move()`: move the rectangle by the `a_move_vector`.
            pub fn move_(&mut self, a_move_vector: Vector2<$c>) {
                self.pos += a_move_vector;
            }

            /// Port of `BOX2::Normalize()`: ensure that the height and width are positive.
            pub fn normalize(&mut self) -> &mut Self {
                if self.size.y < (0 as $e) {
                    self.size.y = -self.size.y;
                    self.pos.y = ki_checked_cast::<$e, $c>(self.pos.y as $e - self.size.y);
                }

                if self.size.x < (0 as $e) {
                    self.size.x = -self.size.x;
                    self.pos.x = ki_checked_cast::<$e, $c>(self.pos.x as $e - self.size.x);
                }

                self
            }

            /// Port of `BOX2::Contains( const Vec& aPoint )`.
            ///
            /// Returns true if `a_point` is inside the boundary box. A point on a edge is
            /// seen as inside.
            pub fn contains(&self, a_point: Vector2<$c>) -> bool {
                let mut rel_pos = Vector2::<$e>::new(
                    a_point.x as $e - self.pos.x as $e,
                    a_point.y as $e - self.pos.y as $e,
                );
                let mut size: Vector2<$e> = self.size;

                if size.x < (0 as $e) {
                    size.x = -size.x;
                    rel_pos.x += size.x;
                }

                if size.y < (0 as $e) {
                    size.y = -size.y;
                    rel_pos.y += size.y;
                }

                (rel_pos.x >= (0 as $e)) && (rel_pos.y >= (0 as $e)) && (rel_pos.y <= size.y) && (rel_pos.x <= size.x)
            }

            /// Port of `BOX2::Contains( coord_type x, coord_type y )`.
            pub fn contains_xy(&self, x: $c, y: $c) -> bool {
                self.contains(Vector2::new(x, y))
            }

            /// Port of `BOX2::Contains( const BOX2<Vec>& aRect )`.
            ///
            /// Returns true if `a_rect` is contained. A common edge is seen as contained.
            pub fn contains_box(&self, a_rect: &Self) -> bool {
                self.contains(a_rect.origin()) && self.contains(a_rect.end())
            }

            /// Port of `BOX2::GetSize()`.
            #[inline]
            pub fn size(&self) -> Vector2<$e> {
                self.size
            }

            /// Port of `BOX2::GetX()`.
            #[inline]
            pub fn x(&self) -> $c {
                self.pos.x
            }

            /// Port of `BOX2::GetY()`.
            #[inline]
            pub fn y(&self) -> $c {
                self.pos.y
            }

            /// Port of `BOX2::GetOrigin()`.
            #[inline]
            pub fn origin(&self) -> Vector2<$c> {
                self.pos
            }

            /// Port of `BOX2::GetPosition()`.
            #[inline]
            pub fn position(&self) -> Vector2<$c> {
                self.pos
            }

            /// Port of `BOX2::GetEnd()`.
            #[inline]
            pub fn end(&self) -> Vector2<$c> {
                Vector2::new(self.right(), self.bottom())
            }

            /// Port of `BOX2::GetWidth()`.
            #[inline]
            pub fn width(&self) -> $e {
                self.size.x
            }

            /// Port of `BOX2::GetHeight()`.
            #[inline]
            pub fn height(&self) -> $e {
                self.size.y
            }

            /// Port of `BOX2::GetRight()`.
            #[inline]
            pub fn right(&self) -> $c {
                ki_checked_cast::<$e, $c>(self.pos.x as $e + self.size.x)
            }

            /// Port of `BOX2::GetBottom()`.
            #[inline]
            pub fn bottom(&self) -> $c {
                ki_checked_cast::<$e, $c>(self.pos.y as $e + self.size.y)
            }

            /// Port of `BOX2::GetLeft()` (compatibility alias of `GetX()`).
            #[inline]
            pub fn left(&self) -> $c {
                self.x()
            }

            /// Port of `BOX2::GetTop()` (compatibility alias of `GetY()`).
            #[inline]
            pub fn top(&self) -> $c {
                self.y()
            }

            /// Port of `BOX2::GetCenter()` (compatibility alias of `Centre()`).
            #[inline]
            pub fn center(&self) -> Vector2<$c> {
                self.centre()
            }

            /// Port of `BOX2::GetSizeMax()`: the width or height, whichever is greater,
            /// converted to `int` as the C++ return type does.
            #[inline]
            pub fn size_max(&self) -> i32 {
                (if self.size.x > self.size.y { self.size.x } else { self.size.y }) as i32
            }

            /// Port of `BOX2::SetOrigin( const Vec& pos )`.
            #[inline]
            pub fn set_origin(&mut self, pos: Vector2<$c>) {
                self.pos = pos;
                self.init = true;
            }

            /// Port of `BOX2::SetOrigin( coord_type x, coord_type y )`.
            #[inline]
            pub fn set_origin_xy(&mut self, x: $c, y: $c) {
                self.set_origin(Vector2::new(x, y));
            }

            /// Port of `BOX2::SetSize( const SizeVec& size )`.
            #[inline]
            pub fn set_size(&mut self, size: impl Into<Vector2<$e>>) {
                self.size = size.into();
                self.init = true;
            }

            /// Port of `BOX2::SetSize( size_type w, size_type h )`.
            #[inline]
            pub fn set_size_wh(&mut self, w: $e, h: $e) {
                self.set_size(Vector2::<$e>::new(w, h));
            }

            /// Port of `BOX2::Offset( coord_type dx, coord_type dy )`.
            #[inline]
            pub fn offset_xy(&mut self, dx: $c, dy: $c) {
                self.pos.x += dx;
                self.pos.y += dy;
            }

            /// Port of `BOX2::Offset( const Vec& offset )`.
            #[inline]
            pub fn offset(&mut self, offset: Vector2<$c>) {
                self.offset_xy(offset.x, offset.y);
            }

            /// Port of `BOX2::GetWithOffset()`.
            #[inline]
            pub fn with_offset(&self, a_move_vector: Vector2<$c>) -> Self {
                let mut ret: Self = *self;

                ret.move_(a_move_vector);
                ret
            }

            /// Port of `BOX2::SetX()`.
            #[inline]
            pub fn set_x(&mut self, val: $c) {
                self.set_origin_xy(val, self.pos.y);
            }

            /// Port of `BOX2::SetY()`.
            #[inline]
            pub fn set_y(&mut self, val: $c) {
                self.set_origin_xy(self.pos.x, val);
            }

            /// Port of `BOX2::SetWidth()`.
            #[inline]
            pub fn set_width(&mut self, val: $e) {
                self.set_size_wh(val, self.size.y);
            }

            /// Port of `BOX2::SetHeight()`.
            #[inline]
            pub fn set_height(&mut self, val: $e) {
                self.set_size_wh(self.size.x, val);
            }

            /// Port of `BOX2::SetEnd( coord_type x, coord_type y )`.
            #[inline]
            pub fn set_end_xy(&mut self, x: $c, y: $c) {
                self.set_end(Vector2::new(x, y));
            }

            /// Port of `BOX2::SetEnd( const Vec& pos )`.
            #[inline]
            pub fn set_end(&mut self, pos: Vector2<$c>) {
                self.set_size(Vector2::<$e>::new(pos.x as $e - self.pos.x as $e, pos.y as $e - self.pos.y as $e));
            }

            /// Port of `BOX2::Intersects( const BOX2<Vec>& aRect )`.
            ///
            /// Returns true if the argument rectangle intersects this rectangle.
            /// (i.e. if the 2 rectangles have at least a common point)
            pub fn intersects(&self, a_rect: &Self) -> bool {
                // this logic taken from wxWidgets' geometry.cpp file:
                let mut me: Self = *self;
                let mut rect: Self = *a_rect;
                me.normalize(); // ensure size is >= 0
                rect.normalize(); // ensure size is >= 0

                // calculate the left common area coordinate:
                let left: $e = std_max(me.pos.x, rect.pos.x) as $e;

                // calculate the right common area coordinate:
                let right: $e = std_min(me.pos.x as $e + me.size.x, rect.pos.x as $e + rect.size.x);

                // calculate the upper common area coordinate:
                let top: $e = std_max(me.pos.y, rect.pos.y) as $e;

                // calculate the lower common area coordinate:
                let bottom: $e = std_min(me.pos.y as $e + me.size.y, rect.pos.y as $e + rect.size.y);

                // if a common area exists, it must have a positive (null accepted) size
                left <= right && top <= bottom
            }

            /// Port of `BOX2::Intersect()`: the common area of this rectangle and `a_rect`
            /// (an initialized, empty box at the origin if there is none).
            pub fn intersect(&self, a_rect: &Self) -> Self {
                let mut me: Self = *self;
                let mut rect: Self = *a_rect;
                me.normalize(); // ensure size is >= 0
                rect.normalize(); // ensure size is >= 0

                let top_left_x: $c = std_max(me.pos.x, rect.pos.x);
                let bottom_right_x: $c = std_min(me.right(), rect.right());
                let top_left_y: $c = std_max(me.pos.y, rect.pos.y);
                let bottom_right_y: $c = std_min(me.bottom(), rect.bottom());

                if top_left_x < bottom_right_x && top_left_y < bottom_right_y {
                    Self::new(
                        Vector2::new(top_left_x, top_left_y),
                        Vector2::<$e>::new(
                            bottom_right_x as $e - top_left_x as $e,
                            bottom_right_y as $e - top_left_y as $e,
                        ),
                    )
                } else {
                    Self::new(Vector2::new(0 as $c, 0 as $c), Vector2::<$e>::new(0 as $e, 0 as $e))
                }
            }

            /// Port of `BOX2::Intersects( const Vec& aPoint1, const Vec& aPoint2 )`.
            ///
            /// Returns true if this rectangle intersects a line from `a_point1` to
            /// `a_point2`.
            pub fn intersects_segment(&self, a_point1: Vector2<$c>, a_point2: Vector2<$c>) -> bool {
                if self.contains(a_point1) || self.contains(a_point2) {
                    return true;
                }

                let point2: Vector2<$c> = Vector2::new(self.end().x, self.origin().y);
                let point4: Vector2<$c> = Vector2::new(self.origin().x, self.end().y);

                let p1: Vector2I = a_point1.cast();
                let p2: Vector2I = a_point2.cast();

                //Only need to test 3 sides since a straight line can't enter and exit on same side
                if segment_intersects_segment(p1, p2, self.origin().cast(), point2.cast()) {
                    return true;
                }

                if segment_intersects_segment(p1, p2, point2.cast(), self.end().cast()) {
                    return true;
                }

                if segment_intersects_segment(p1, p2, self.end().cast(), point4.cast()) {
                    return true;
                }

                false
            }

            /// Port of `BOX2::Intersects( const BOX2<Vec>& aRect, const EDA_ANGLE& aRotation )`.
            ///
            /// Returns true if this rectangle intersects a rotated rect given by `a_rect` and
            /// `a_rotation`.
            pub fn intersects_rotated(&self, a_rect: &Self, a_rotation: EdaAngle) -> bool {
                if !self.init {
                    return false;
                }

                let mut rotation: EdaAngle = a_rotation;
                rotation.normalize();

                /*
                 * Most rectangles will be axis aligned.  It is quicker to check for this case and pass
                 * the rect to the simpler intersection test.
                 */

                // Prevent floating point comparison errors
                let rot_epsilon = EdaAngle::new(0.000000001, DEGREES_T);

                // Test for non-rotated rectangle
                for ii in [ANGLE_0, ANGLE_180, ANGLE_360] {
                    if (rotation - ii).abs() < rot_epsilon {
                        return self.intersects(a_rect);
                    }
                }

                // Test for rectangle rotated by multiple of 90 degrees
                for jj in [ANGLE_90, ANGLE_270] {
                    if (rotation - jj).abs() < rot_epsilon {
                        let mut rot_rect = Self::default();

                        // Rotate the supplied rect by 90 degrees
                        rot_rect.set_origin(a_rect.centre());
                        rot_rect.inflate_xy(
                            ((a_rect.height() / (2 as $e)) + (1 as $e)) as $c,
                            ((a_rect.width() / (2 as $e)) + (1 as $e)) as $c,
                        );

                        return self.intersects(&rot_rect);
                    }
                }

                /* There is some non-orthogonal rotation.
                 * There are three cases to test:
                 * A) One point of this rect is inside the rotated rect
                 * B) One point of the rotated rect is inside this rect
                 * C) One of the sides of the rotated rect intersect this
                 */

                /* Test A : Any corners exist in rotated rect? */
                let mut corners: [Vector2I; 4] = [
                    Vector2I::new(self.left() as i32, self.top() as i32),
                    Vector2I::new(self.right() as i32, self.top() as i32),
                    Vector2I::new(self.right() as i32, self.bottom() as i32),
                    Vector2I::new(self.left() as i32, self.bottom() as i32),
                ];

                let r_centre: Vector2I = a_rect.centre().cast();

                for i in 0..4 {
                    let mut delta: Vector2I = corners[i] - r_centre;
                    rotate_point(&mut delta, -rotation);
                    delta += r_centre;

                    if a_rect.contains(delta.cast()) {
                        return true;
                    }
                }

                /* Test B : Any corners of rotated rect exist in this one? */
                let w: i32 = ki_checked_cast::<$e, $c>(a_rect.width() / (2 as $e)) as i32;
                let h: i32 = ki_checked_cast::<$e, $c>(a_rect.height() / (2 as $e)) as i32;

                // Construct corners around center of shape
                corners[0] = Vector2I::new(-w, -h);
                corners[1] = Vector2I::new(w, -h);
                corners[2] = Vector2I::new(w, h);
                corners[3] = Vector2I::new(-w, h);

                // Rotate and test each corner
                for j in 0..4 {
                    rotate_point(&mut corners[j], rotation);
                    corners[j] += r_centre;

                    if self.contains(corners[j].cast()) {
                        return true;
                    }
                }

                /* Test C : Any sides of rotated rect intersect this */
                if self.intersects_segment(corners[0].cast(), corners[1].cast())
                    || self.intersects_segment(corners[1].cast(), corners[2].cast())
                    || self.intersects_segment(corners[2].cast(), corners[3].cast())
                    || self.intersects_segment(corners[3].cast(), corners[0].cast())
                {
                    return true;
                }

                false
            }

            /// Port of `BOX2::IntersectsCircle()`.
            ///
            /// Returns true if this rectangle intersects the circle defined by `a_center` and
            /// `a_radius`.
            pub fn intersects_circle(&self, a_center: Vector2<$c>, a_radius: i32) -> bool {
                if !self.init {
                    return false;
                }

                let closest: Vector2<$c> = self.nearest_point(a_center);

                let dx: f64 = a_center.x as f64 - closest.x as f64;
                let dy: f64 = a_center.y as f64 - closest.y as f64;

                let r: f64 = a_radius as f64;

                (dx * dx + dy * dy) <= (r * r)
            }

            /// Port of `BOX2::IntersectsCircleEdge()`.
            ///
            /// Returns true if this rectangle intersects the edge of a circle defined by
            /// `a_center` and `a_radius`.
            pub fn intersects_circle_edge(&self, a_center: Vector2<$c>, a_radius: i32, a_width: i32) -> bool {
                if !self.init {
                    return false;
                }

                // Test if the circle intersects at all
                if !self.intersects_circle(a_center, a_radius + (a_width / 2) + 1) {
                    return false;
                }

                let farpt: Vector2<$c> = self.farthest_point_to(a_center);
                // Farthest point must be further than the inside of the line
                let fx: f64 = farpt.x as f64 - a_center.x as f64;
                let fy: f64 = farpt.y as f64 - a_center.y as f64;

                let r: f64 = a_radius as f64 - a_width as f64 / 2.0;

                (fx * fx + fy * fy) > (r * r)
            }

            /// Port of `BOX2::Format()`: `( box corner ( xy <x> <y> ) w <w> h <h> )`.
            pub fn format(&self) -> String {
                format!(
                    "( box corner {} w {} h {} )",
                    self.pos.format(),
                    self.size.x.coord_format(),
                    self.size.y.coord_format()
                )
            }

            /// Port of `BOX2::Inflate( coord_type dx, coord_type dy )`.
            ///
            /// Inflates the rectangle horizontally by `dx` and vertically by `dy`. If `dx`
            /// and/or `dy` is negative the rectangle is deflated.
            pub fn inflate_xy(&mut self, dx: $c, dy: $c) -> &mut Self {
                Self::inflate_axis(&mut self.pos.x, &mut self.size.x, dx);
                Self::inflate_axis(&mut self.pos.y, &mut self.size.y, dy);

                self
            }

            /// Port of `BOX2::Inflate( coord_type aDelta )`.
            ///
            /// Inflate the rectangle horizontally and vertically by `a_delta`. If `a_delta`
            /// is negative the rectangle is deflated.
            pub fn inflate(&mut self, a_delta: $c) -> &mut Self {
                self.inflate_xy(a_delta, a_delta);
                self
            }

            /// Port of `BOX2::GetInflated( coord_type aDx, coord_type aDy )`.
            ///
            /// Get a new rectangle that is this one, inflated by `a_dx` and `a_dy`.
            pub fn inflated_xy(&self, a_dx: $c, a_dy: $c) -> Self {
                let mut ret: Self = *self;

                ret.inflate_xy(a_dx, a_dy);
                ret
            }

            /// Port of `BOX2::GetInflated( coord_type aDelta )`.
            ///
            /// Get a new rectangle that is this one, inflated by `a_delta`.
            pub fn inflated(&self, a_delta: $c) -> Self {
                self.inflated_xy(a_delta, a_delta)
            }

            /// Port of `BOX2::Merge( const BOX2<Vec>& aRect )`.
            ///
            /// Modify the position and size of the rectangle in order to contain `a_rect`.
            pub fn merge(&mut self, a_rect: &Self) -> &mut Self {
                if !self.init {
                    if a_rect.init {
                        self.pos = a_rect.position();
                        self.size = a_rect.size();
                        self.init = true;
                    }

                    return self;
                }

                self.normalize(); // ensure width and height >= 0
                let mut rect: Self = *a_rect;
                rect.normalize(); // ensure width and height >= 0
                let mut end: Vector2<$c> = self.end();
                let rect_end: Vector2<$c> = rect.end();

                // Change origin and size in order to contain the given rect
                self.pos.x = std_min(self.pos.x, rect.pos.x);
                self.pos.y = std_min(self.pos.y, rect.pos.y);
                end.x = std_max(end.x, rect_end.x);
                end.y = std_max(end.y, rect_end.y);
                self.set_end(end);
                self
            }

            /// Port of `BOX2::Merge( const Vec& aPoint )`.
            ///
            /// Modify the position and size of the rectangle in order to contain the given
            /// point.
            pub fn merge_point(&mut self, a_point: Vector2<$c>) -> &mut Self {
                if !self.init {
                    self.pos = a_point;
                    self.size = Vector2::<$e>::new(0 as $e, 0 as $e);
                    self.init = true;
                    return self;
                }

                self.normalize(); // ensure width and height >= 0

                let mut end: Vector2<$c> = self.end();

                // Change origin and size in order to contain the given rectangle.
                self.pos.x = std_min(self.pos.x, a_point.x);
                self.pos.y = std_min(self.pos.y, a_point.y);
                end.x = std_max(end.x, a_point.x);
                end.y = std_max(end.y, a_point.y);
                self.set_end(end);
                self
            }

            /// Port of `BOX2::GetBoundingBoxRotated()`.
            ///
            /// Useful to calculate bounding box of rotated items, when rotation is not
            /// cardinal.
            ///
            /// Returns the bounding box of this, after rotation.
            pub fn bounding_box_rotated(&self, a_rot_center: Vector2I, a_angle: EdaAngle) -> Self {
                let mut corners: [Vector2I; 4] = [Vector2I::new(0, 0); 4];

                // Build the corners list
                corners[0] = self.origin().cast();
                corners[2] = self.end().cast();
                corners[1].x = corners[0].x;
                corners[1].y = corners[2].y;
                corners[3].x = corners[2].x;
                corners[3].y = corners[0].y;

                // Rotate all corners, to find the bounding box
                for ii in 0..4 {
                    rotate_point_around(&mut corners[ii], a_rot_center, a_angle);
                }

                // Find the corners bounding box
                let mut start: Vector2I = corners[0];
                let mut end: Vector2I = corners[0];

                for ii in 1..4 {
                    start.x = start.x.min(corners[ii].x);
                    start.y = start.y.min(corners[ii].y);
                    end.x = end.x.max(corners[ii].x);
                    end.y = end.y.max(corners[ii].y);
                }

                let mut bbox = Self::default();
                bbox.set_origin(start.cast());
                bbox.set_end(end.cast());

                bbox
            }

            /// Port of `BOX2::GetArea()`: return the area of the rectangle.
            #[inline]
            pub fn area(&self) -> $e {
                self.width() * self.height()
            }

            /// Port of `BOX2::Diagonal()`: return the length of the diagonal of the
            /// rectangle.
            #[inline]
            pub fn diagonal(&self) -> $e {
                self.size.euclidean_norm()
            }

            /// Port of `BOX2::SquaredDiagonal()`.
            ///
            /// Return the square of the length of the diagonal of the rectangle.
            ///
            /// When all you need is a comparison, this is faster than Diagonal().
            #[inline]
            pub fn squared_diagonal(&self) -> $e {
                self.size.squared_euclidean_norm()
            }

            /// Port of `BOX2::SquaredDistance( const Vec& aP )`.
            pub fn squared_distance_point(&self, a_p: Vector2<$c>) -> $e {
                let x1: $e = std_min(self.pos.x as $e, self.pos.x as $e + self.size.x);
                let x2: $e = std_max(self.pos.x as $e, self.pos.x as $e + self.size.x);
                let y1: $e = std_min(self.pos.y as $e, self.pos.y as $e + self.size.y);
                let y2: $e = std_max(self.pos.y as $e, self.pos.y as $e + self.size.y);

                let xdiff: $e =
                    std_max(if (a_p.x as $e) < x1 { x1 - a_p.x as $e } else { a_p.x as $e - x2 }, 0 as $e);
                let ydiff: $e =
                    std_max(if (a_p.y as $e) < y1 { y1 - a_p.y as $e } else { a_p.y as $e - y2 }, 0 as $e);

                xdiff * xdiff + ydiff * ydiff
            }

            /// Port of `BOX2::SquaredDistance( const BOX2<Vec>& aBox )`.
            ///
            /// Return the square of the minimum distance between self and box `a_box`.
            pub fn squared_distance_box(&self, a_box: &Self) -> $e {
                let mut s: $e = 0 as $e;

                if a_box.pos.x as $e + a_box.size.x < self.pos.x as $e {
                    let d: $e = a_box.pos.x as $e + a_box.size.x - self.pos.x as $e;
                    s += d * d;
                } else if a_box.pos.x as $e > self.pos.x as $e + self.size.x {
                    let d: $e = a_box.pos.x as $e - self.size.x - self.pos.x as $e;
                    s += d * d;
                }

                if a_box.pos.y as $e + a_box.size.y < self.pos.y as $e {
                    let d: $e = a_box.pos.y as $e + a_box.size.y - self.pos.y as $e;
                    s += d * d;
                } else if a_box.pos.y as $e > self.pos.y as $e + self.size.y {
                    let d: $e = a_box.pos.y as $e - self.size.y - self.pos.y as $e;
                    s += d * d;
                }

                s
            }

            /// Port of `BOX2::NearestPoint()`.
            ///
            /// Return the point in this rect that is closest to the provided point.
            pub fn nearest_point(&self, a_point: Vector2<$c>) -> Vector2<$c> {
                let mut me: Self = *self;

                me.normalize(); // ensure size is >= 0

                // Determine closest point to the circle centre within this rect
                let nx: $c = std_clamp(a_point.x, me.left(), me.right());
                let ny: $c = std_clamp(a_point.y, me.top(), me.bottom());

                Vector2::new(nx, ny)
            }

            /// Port of `BOX2::FarthestPointTo()`.
            ///
            /// Return the point in this rect that is farthest from the provided point.
            pub fn farthest_point_to(&self, a_point: Vector2<$c>) -> Vector2<$c> {
                let mut me: Self = *self;

                me.normalize(); // ensure size is >= 0

                let center: Vector2<$c> = me.center();

                let fx: $c = if a_point.x < center.x { me.right() } else { me.left() };
                let fy: $c = if a_point.y < center.y { me.bottom() } else { me.top() };

                Vector2::new(fx, fy)
            }

            /// Port of `BOX2::IsValid()`: whether the rectangle is initialized.
            #[inline]
            pub fn is_valid(&self) -> bool {
                self.init
            }

            /// Port of the free function template `IsBOX2Safe`.
            ///
            /// Check if a BOX2 is safe for use with BOX2D
            /// (probably BOX2D or BOX2L)
            pub fn is_box2_safe(&self) -> bool {
                const HIGH: f64 = i32::MAX as f64;
                const LOW: f64 = -(i32::MAX as f64);

                self.left() as f64 >= LOW
                    && self.top() as f64 >= LOW
                    && self.right() as f64 <= HIGH
                    && self.bottom() as f64 <= HIGH
            }

            /// Port of `BOX2::inflateAxis`.
            ///
            /// Widened so that sentinel deltas cannot wrap, results saturate outward to keep
            /// the original region covered.
            fn inflate_axis(a_pos: &mut $c, a_size: &mut $e, a_delta: $c) {
                let pos: $e = *a_pos as $e;
                let delta: $e = a_delta as $e;
                let low_limit: $e = <$c>::MIN as $e;
                let high_limit: $e = <$c>::MAX as $e;

                if *a_size >= (0 as $e) {
                    if *a_size < (-2 as $e) * delta {
                        // Don't allow deflate to eat more width than we have,
                        *a_pos = ki_checked_cast::<$e, $c>(pos + *a_size / (2 as $e));
                        *a_size = 0 as $e;
                    } else {
                        let lo: $e = std_clamp(pos - delta, low_limit, high_limit);
                        let hi: $e = std_clamp(pos + *a_size + delta, low_limit, high_limit);

                        *a_pos = lo as $c;
                        *a_size = hi - lo;
                    }
                } else {
                    if *a_size > (2 as $e) * delta {
                        *a_pos = ki_checked_cast::<$e, $c>(pos - *a_size / (2 as $e));
                        *a_size = 0 as $e;
                    } else {
                        // Negative size, pos is the high edge so inflating moves it up and the low edge down
                        let hi: $e = std_clamp(pos + delta, low_limit, high_limit);
                        let lo: $e = std_clamp(pos + *a_size - delta, low_limit, high_limit);

                        *a_pos = hi as $c;
                        *a_size = lo - hi;
                    }
                }
            }
        }

        impl PartialEq for Box2<$c> {
            /// Port of `BOX2::operator==` (and `operator!=`): compares the normalized boxes.
            fn eq(&self, a_other: &Self) -> bool {
                let mut t1: Self = *self;
                let mut t2: Self = *a_other;
                t1.normalize();
                t2.normalize();

                t1.pos == t2.pos && t1.size == t2.size
            }
        }

        impl core::fmt::Display for Box2<$c> {
            /// Same text as `format()` (`BOX2::Format()`).
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(&self.format())
            }
        }
    };
}

impl_box2!(i32, i64);
impl_box2!(i64, i64);
impl_box2!(f64, f64);

impl Box2<i32> {
    /// Port of `BOX2::SetMaximum()` for `BOX2I`.
    pub fn set_maximum(&mut self) {
        // We want to be able to invert the box, so don't use lowest()
        self.pos.x = -i32::MAX;
        self.pos.y = -i32::MAX;
        self.size.x = i32::MAX as i64 + i32::MAX as i64;
        self.size.y = self.size.x;

        self.init = true;
    }

    /// Port of `BOX2::Distance( const Vec& aP )` for `BOX2I`.
    pub fn distance_point(&self, a_p: Vector2I) -> i64 {
        let dist: f64 = (self.squared_distance_point(a_p) as f64).sqrt();

        ki_round_to::<i64, f64>(dist)
    }

    /// Port of `BOX2::Distance( const BOX2<Vec>& aBox )` for `BOX2I`.
    ///
    /// Return the minimum distance between self and `a_box`.
    pub fn distance_box(&self, a_box: &Self) -> i64 {
        let dist: f64 = (self.squared_distance_box(a_box) as f64).sqrt();

        ki_round_to::<i64, f64>(dist)
    }
}

impl Box2<i64> {
    /// Port of `BOX2::SetMaximum()` for `BOX2L`.
    pub fn set_maximum(&mut self) {
        // BOX2L uses 64-bit ints for both coords and sizes
        // (std::numeric_limits<int64_t>::epsilon() is 0)
        self.pos.x = -(i64::MAX / 2);
        self.pos.y = -(i64::MAX / 2);
        self.size.x = i64::MAX;
        self.size.y = i64::MAX;

        self.init = true;
    }

    /// Port of `BOX2::Distance( const Vec& aP )` for `BOX2L`.
    pub fn distance_point(&self, a_p: Vector2L) -> i64 {
        let dist: f64 = (self.squared_distance_point(a_p) as f64).sqrt();

        ki_round_to::<i64, f64>(dist)
    }

    /// Port of `BOX2::Distance( const BOX2<Vec>& aBox )` for `BOX2L`.
    ///
    /// Return the minimum distance between self and `a_box`.
    pub fn distance_box(&self, a_box: &Self) -> i64 {
        let dist: f64 = (self.squared_distance_box(a_box) as f64).sqrt();

        ki_round_to::<i64, f64>(dist)
    }
}

impl Box2<f64> {
    /// Port of `BOX2::SetMaximum()` for `BOX2D`.
    pub fn set_maximum(&mut self) {
        self.pos.x = f64::MIN / 2.0;
        self.pos.y = f64::MIN / 2.0;
        self.size.x = f64::MAX;
        self.size.y = f64::MAX;

        self.init = true;
    }

    /// Port of `BOX2::Distance( const Vec& aP )` for `BOX2D`.
    pub fn distance_point(&self, a_p: Vector2D) -> f64 {
        self.squared_distance_point(a_p).sqrt()
    }

    /// Port of `BOX2::Distance( const BOX2<Vec>& aBox )` for `BOX2D`.
    ///
    /// Return the minimum distance between self and `a_box`.
    pub fn distance_box(&self, a_box: &Self) -> f64 {
        self.squared_distance_box(a_box).sqrt()
    }
}

/// Port of `BOX2ISafe( const BOX2D& aInput )`.
pub fn box2i_safe(a_input: &Box2D) -> Box2I {
    const HIGH: f64 = i32::MAX as f64;
    const LOW: f64 = -(i32::MAX as f64);

    let left: i32 = std_clamp(a_input.left(), LOW, HIGH) as i32;
    let top: i32 = std_clamp(a_input.top(), LOW, HIGH) as i32;

    let right: i64 = std_clamp(a_input.right(), LOW, HIGH) as i64;
    let bottom: i64 = std_clamp(a_input.bottom(), LOW, HIGH) as i64;

    Box2I::new(Vector2I::new(left, top), Vector2L::new(right - left as i64, bottom - top as i64))
}

/// Port of `BOX2ISafe( const VECTOR2D& aPos, const VECTOR2D& aSize )`.
pub fn box2i_safe_from_pos_size(a_pos: Vector2D, a_size: Vector2D) -> Box2I {
    const HIGH: f64 = i32::MAX as f64;
    const LOW: f64 = -(i32::MAX as f64);

    let left: i32 = std_clamp(a_pos.x, LOW, HIGH) as i32;
    let top: i32 = std_clamp(a_pos.y, LOW, HIGH) as i32;

    let right: i64 = std_clamp(a_pos.x + a_size.x, LOW, HIGH) as i64;
    let bottom: i64 = std_clamp(a_pos.y + a_size.y, LOW, HIGH) as i64;

    Box2I::new(Vector2I::new(left, top), Vector2L::new(right - left as i64, bottom - top as i64))
}

/// Port of `BOX2ISafe( const VECTOR2I& aPos, const VECTOR2<S>& aSize )` for integral `S`.
pub fn box2i_safe_from_int(a_pos: Vector2I, a_size: impl Into<Vector2L>) -> Box2I {
    const HIGH: i64 = i32::MAX as i64;
    const LOW: i64 = -(i32::MAX as i64);

    let a_size: Vector2L = a_size.into();

    let ext_right: i64 = a_pos.x as i64 + a_size.x;
    let ext_bottom: i64 = a_pos.y as i64 + a_size.y;

    let right: i64 = std_clamp(ext_right, LOW, HIGH);
    let bottom: i64 = std_clamp(ext_bottom, LOW, HIGH);

    Box2I::new(a_pos, Vector2L::new(right - a_pos.x as i64, bottom - a_pos.y as i64))
}

impl KiRound for Box2<f64> {
    type Output = Box2<i32>;

    /// Port of `KiROUND( const BOX2D& aBoxD )`: KiROUND specialization for double -> int
    /// boxes.
    fn ki_round(self) -> Box2<i32> {
        Box2I::new(ki_round(self.origin()), ki_round(self.size()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_box_is_invalid_and_merges_by_replacement() {
        let mut b = Box2I::default();
        assert!(!b.is_valid());

        let other = Box2I::new(Vector2I::new(10, 20), Vector2I::new(5, 5));
        b.merge(&other);
        assert!(b.is_valid());
        assert_eq!(b.origin(), Vector2I::new(10, 20));
        assert_eq!(b.size(), Vector2L::new(5, 5));

        let mut c = Box2I::default();
        c.merge(&Box2I::default());
        assert!(!c.is_valid());

        let mut d = Box2I::default();
        d.merge_point(Vector2I::new(3, 4));
        assert!(d.is_valid());
        assert_eq!(d.origin(), Vector2I::new(3, 4));
        d.merge_point(Vector2I::new(-3, 14));
        assert_eq!(d.origin(), Vector2I::new(-3, 4));
        assert_eq!(d.end(), Vector2I::new(3, 14));
    }

    #[test]
    fn constructor_normalizes() {
        let b = Box2I::new(Vector2I::new(100, 100), Vector2L::new(-100, -50));
        assert_eq!(b.origin(), Vector2I::new(0, 50));
        assert_eq!(b.size(), Vector2L::new(100, 50));
        assert_eq!(b.end(), Vector2I::new(100, 100));
        assert_eq!(b, Box2I::new(Vector2I::new(0, 50), Vector2I::new(100, 50)));
        assert_eq!(Box2I::from_pos(Vector2I::new(1, 2)).size(), Vector2L::new(0, 0));
    }

    #[test]
    fn accessors_and_setters() {
        let mut b = Box2I::new(Vector2I::new(1, 2), Vector2I::new(30, 40));

        assert_eq!((b.x(), b.y(), b.left(), b.top(), b.right(), b.bottom()), (1, 2, 1, 2, 31, 42));
        assert_eq!((b.width(), b.height()), (30, 40));
        assert_eq!(b.centre(), Vector2I::new(16, 22));
        assert_eq!(b.center(), b.centre());
        assert_eq!(b.size_max(), 40);
        assert_eq!(b.area(), 1200);
        assert_eq!(b.diagonal(), 50);
        assert_eq!(b.squared_diagonal(), 2500);
        assert_eq!(b.format(), "( box corner ( xy 1 2 ) w 30 h 40 )");
        assert_eq!(b.to_string(), "( box corner ( xy 1 2 ) w 30 h 40 )");

        b.set_x(10);
        b.set_y(20);
        b.set_width(5);
        b.set_height(6);
        assert_eq!(b, Box2I::new(Vector2I::new(10, 20), Vector2I::new(5, 6)));
        b.set_end_xy(110, 120);
        assert_eq!(b.size(), Vector2L::new(100, 100));
        b.offset_xy(-10, -20);
        b.offset(Vector2I::new(1, 1));
        b.move_(Vector2I::new(1, 1));
        assert_eq!(b.origin(), Vector2I::new(2, 2));
        assert_eq!(b.with_offset(Vector2I::new(8, 8)).origin(), Vector2I::new(10, 10));
        b.set_origin_xy(0, 0);
        b.set_size_wh(7, 8);
        assert_eq!(b.end(), Vector2I::new(7, 8));
    }

    #[test]
    fn contains_and_intersects() {
        let b = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 100));

        assert!(b.contains(Vector2I::new(0, 0)));
        assert!(b.contains(Vector2I::new(100, 100)));
        assert!(!b.contains(Vector2I::new(101, 100)));
        assert!(b.contains_xy(50, 50));
        assert!(b.contains_box(&Box2I::new(Vector2I::new(10, 10), Vector2I::new(90, 90))));
        assert!(!b.contains_box(&Box2I::new(Vector2I::new(10, 10), Vector2I::new(91, 90))));

        assert!(b.intersects(&Box2I::new(Vector2I::new(100, 100), Vector2I::new(10, 10))));
        assert!(!b.intersects(&Box2I::new(Vector2I::new(101, 100), Vector2I::new(10, 10))));

        let i = b.intersect(&Box2I::new(Vector2I::new(50, 60), Vector2I::new(100, 100)));
        assert_eq!(i, Box2I::new(Vector2I::new(50, 60), Vector2I::new(50, 40)));
        // Touching boxes have no common area
        let i = b.intersect(&Box2I::new(Vector2I::new(100, 100), Vector2I::new(10, 10)));
        assert_eq!(i, Box2I::new(Vector2I::new(0, 0), Vector2I::new(0, 0)));
        assert!(i.is_valid());

        assert!(b.intersects_segment(Vector2I::new(-10, 50), Vector2I::new(110, 50)));
        assert!(b.intersects_segment(Vector2I::new(-10, -10), Vector2I::new(10, 10)));
        assert!(!b.intersects_segment(Vector2I::new(-10, -10), Vector2I::new(-10, 110)));
        assert!(b.intersects_segment(Vector2I::new(-10, 10), Vector2I::new(10, -10)));
    }

    #[test]
    fn intersects_rotated_rect() {
        let b = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 100));
        let far = Box2I::new(Vector2I::new(200, 0), Vector2I::new(400, 10));

        assert!(!b.intersects_rotated(&far, ANGLE_0));
        assert!(!b.intersects_rotated(&far, ANGLE_90));
        assert!(!Box2I::default().intersects_rotated(&far, ANGLE_0));

        // A long thin rect through the middle of the box, at any angle
        let thin = Box2I::new(Vector2I::new(-150, 45), Vector2I::new(400, 10));
        for deg in [0.0, 30.0, 45.0, 90.0, 135.0, 180.0, 270.0, 300.0] {
            assert!(b.intersects_rotated(&thin, EdaAngle::new(deg, DEGREES_T)), "{deg}");
        }

        // Centre (400, 5): rotating by 45 degrees keeps it away from the box
        assert!(!b.intersects_rotated(&far, EdaAngle::new(45.0, DEGREES_T)));
    }

    #[test]
    fn inflate_and_deflate() {
        let b = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 50));

        assert_eq!(b.inflated(10), Box2I::new(Vector2I::new(-10, -10), Vector2I::new(120, 70)));
        assert_eq!(b.inflated_xy(10, -10), Box2I::new(Vector2I::new(-10, 10), Vector2I::new(120, 30)));
        // Deflating by more than half the size collapses the axis onto its centre
        assert_eq!(b.inflated(-30), Box2I::new(Vector2I::new(30, 25), Vector2I::new(40, 0)));
        assert_eq!(b.inflated(-60), Box2I::new(Vector2I::new(50, 25), Vector2I::new(0, 0)));

        let d = Box2D::new(Vector2D::new(1.0, 2.0), Vector2D::new(3.0, 4.0));
        assert_eq!(d.inflated(1.5), Box2D::new(Vector2D::new(-0.5, 0.5), Vector2D::new(6.0, 7.0)));

        let mut l = Box2L::new(Vector2L::new(0, 0), Vector2L::new(10, 10));
        l.inflate(5_000_000_000);
        assert_eq!(l.origin(), Vector2L::new(-5_000_000_000, -5_000_000_000));
        assert_eq!(l.size(), Vector2L::new(10_000_000_010, 10_000_000_010));
    }

    #[test]
    fn set_maximum_per_type() {
        let mut bi = Box2I::default();
        bi.set_maximum();
        assert!(bi.is_valid());
        assert_eq!(bi.origin(), Vector2I::new(-i32::MAX, -i32::MAX));
        assert_eq!(bi.end(), Vector2I::new(i32::MAX, i32::MAX));
        assert!(bi.contains(Vector2I::new(123456789, -987654321)));

        let mut bl = Box2L::default();
        bl.set_maximum();
        assert_eq!(bl.origin(), Vector2L::new(-(i64::MAX / 2), -(i64::MAX / 2)));
        assert_eq!(bl.size(), Vector2L::new(i64::MAX, i64::MAX));

        let mut bd = Box2D::default();
        bd.set_maximum();
        assert_eq!(bd.origin(), Vector2D::new(f64::MIN / 2.0, f64::MIN / 2.0));
        assert_eq!(bd.size(), Vector2D::new(f64::MAX, f64::MAX));
    }

    #[test]
    fn distances() {
        let b = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 100));

        assert_eq!(b.distance_point(Vector2I::new(103, 104)), 5);
        assert_eq!(b.squared_distance_point(Vector2I::new(103, 104)), 25);
        assert_eq!(b.distance_point(Vector2I::new(50, 50)), 0);

        let other = Box2I::new(Vector2I::new(103, 104), Vector2I::new(10, 10));
        assert_eq!(b.squared_distance_box(&other), 25);
        assert_eq!(b.distance_box(&other), 5);
        assert_eq!(other.distance_box(&b), 5);
        assert_eq!(b.distance_box(&Box2I::new(Vector2I::new(50, 50), Vector2I::new(10, 10))), 0);

        let d = Box2D::new(Vector2D::new(0.0, 0.0), Vector2D::new(1.0, 1.0));
        assert_eq!(d.distance_point(Vector2D::new(4.0, 5.0)), 5.0);
    }

    #[test]
    fn compute_and_rotated_bounding_box() {
        let mut b = Box2I::default();
        b.compute(&[]);
        assert!(!b.is_valid());
        b.compute(&[Vector2I::new(5, -5), Vector2I::new(-10, 20), Vector2I::new(0, 0)]);
        assert_eq!(b, Box2I::new(Vector2I::new(-10, -5), Vector2I::new(15, 25)));

        let r = Box2I::new(Vector2I::new(0, 0), Vector2I::new(100, 50));
        let bb = r.bounding_box_rotated(Vector2I::new(0, 0), ANGLE_90);
        assert_eq!(bb, Box2I::new(Vector2I::new(0, -100), Vector2I::new(50, 100)));
    }

    #[test]
    fn safe_boxes_and_rounding() {
        let huge = Box2D::new(Vector2D::new(-1e12, 10.5), Vector2D::new(2e12, 20.0));
        let safe = box2i_safe(&huge);
        assert_eq!(safe.origin(), Vector2I::new(-i32::MAX, 10));
        assert_eq!(safe.end(), Vector2I::new(i32::MAX, 30));
        assert!(!huge.is_box2_safe());
        assert!(safe.is_box2_safe());

        let s2 = box2i_safe_from_pos_size(Vector2D::new(1.9, 2.9), Vector2D::new(1e12, 5.0));
        assert_eq!(s2.origin(), Vector2I::new(1, 2));
        assert_eq!(s2.end(), Vector2I::new(i32::MAX, 7));

        let s3 = box2i_safe_from_int(Vector2I::new(10, 20), Vector2L::new(1 << 40, 5));
        assert_eq!(s3.origin(), Vector2I::new(10, 20));
        assert_eq!(s3.end(), Vector2I::new(i32::MAX, 25));

        let rounded = ki_round(Box2D::new(Vector2D::new(0.5, -0.5), Vector2D::new(2.5, 3.4)));
        assert_eq!(rounded, Box2I::new(Vector2I::new(1, -1), Vector2I::new(3, 3)));
    }
}
