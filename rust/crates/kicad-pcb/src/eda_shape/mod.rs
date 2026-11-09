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

//! Port of `include/eda_shape.h` and `common/eda_shape.cpp`.
//!
//! Placeholder written by M0a; the type package fills in the definitions and `Default`
//! (`board-model.md` 16).

pub mod geometry;
pub mod shapes;

use kicad_geom::{EdaAngle, Vector2I};

use crate::traits::EdaShapeItem;

/// `EDA_SHAPE (data members)`.
///
/// PLACEHOLDER (M0a): M0c replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct EdaShape {}

// ---- EDA_SHAPE base-class bodies, generic over the host (board-model.md 6.2a) ----

/// Port of `EDA_SHAPE::GetWidth` (the base-class body the `EdaShapeItem` default forwards to).
pub fn width<H: EdaShapeItem + ?Sized>(host: &H) -> i32 {
    crate::port_pending!("EDA_SHAPE::GetWidth", host)
}

/// Port of `EDA_SHAPE::IsProxyItem` (the base-class body the `EdaShapeItem` default forwards to).
pub fn is_proxy_item<H: EdaShapeItem + ?Sized>(host: &H) -> bool {
    crate::port_pending!("EDA_SHAPE::IsProxyItem", host)
}

/// Port of `EDA_SHAPE::IsFilledForHitTesting` (the base-class body the `EdaShapeItem` default forwards to).
pub fn is_filled_for_hit_testing<H: EdaShapeItem + ?Sized>(host: &H) -> bool {
    crate::port_pending!("EDA_SHAPE::IsFilledForHitTesting", host)
}

/// Port of `EDA_SHAPE::GetTopLeft` (the base-class body the `EdaShapeItem` default forwards to).
pub fn top_left<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::GetTopLeft", host)
}

/// Port of `EDA_SHAPE::GetBotRight` (the base-class body the `EdaShapeItem` default forwards to).
pub fn bot_right<H: EdaShapeItem + ?Sized>(host: &H) -> Vector2I {
    crate::port_pending!("EDA_SHAPE::GetBotRight", host)
}

/// Port of `EDA_SHAPE::GetCornersInSequence` (the base-class body the `EdaShapeItem` default forwards to).
pub fn corners_in_sequence<H: EdaShapeItem + ?Sized>(host: &H, angle: EdaAngle) -> Vec<Vector2I> {
    crate::port_pending!("EDA_SHAPE::GetCornersInSequence", host, angle)
}

// ---- non-virtual EDA_SHAPE members that call the virtuals (board-model.md 6.2a) ----
//
// Each is a generic function over the host (here, in `geometry.rs` or in `shapes.rs`, by the
// package that ports it) plus a forwarder in `EdaShapeExt`, which gives it method syntax on
// every host and on `&dyn EdaShapeItem`. M0c adds the signatures of board-model.md 6.2a
// (make_effective_shapes, hatching, shape_bounding_box, shape_hit_test,
// shape_transform_to_polygon, ...) with `port_pending!` bodies in the file of the owning
// package (M7: mod.rs / geometry.rs, M8: shapes.rs). `traits.rs` is not edited for them.

/// Method syntax for the non-virtual `EDA_SHAPE` members that call the virtuals. Implemented
/// for every `EdaShapeItem` (also `dyn EdaShapeItem`) by the blanket impl below and nowhere
/// else, so these methods cannot be overridden - like the non-virtual C++ members they port.
pub trait EdaShapeExt: EdaShapeItem {}

impl<T: EdaShapeItem + ?Sized> EdaShapeExt for T {}
