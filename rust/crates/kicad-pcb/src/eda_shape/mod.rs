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
