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

//! Port of `include/eda_text.h` and `common/eda_text.cpp`.
//!
//! Placeholder written by M0a; the type package fills in the definitions and `Default`
//! (`board-model.md` 16).

use kicad_geom::{Box2I, EdaAngle, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::traits::EdaTextItem;

/// `EDA_TEXT (data members)`.
///
/// PLACEHOLDER (M0a): M0c replaces the body with all fields of `board-model-types.md` and
/// `Default` with the C++ constructor values.
#[derive(Clone, Debug, Default)]
pub struct EdaText {}

// ---- EDA_TEXT base-class bodies, generic over the host (board-model.md 6.2a) ----

/// Port of `EDA_TEXT::GetTextPos` (the base-class body the `EdaTextItem` default forwards to).
pub fn text_pos<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vector2I {
    crate::port_pending!("EDA_TEXT::GetTextPos", host, ctx)
}

/// Port of `EDA_TEXT::GetTextSize` (the base-class body the `EdaTextItem` default forwards to).
pub fn text_size<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> Vector2I {
    crate::port_pending!("EDA_TEXT::GetTextSize", host, ctx)
}

/// Port of `EDA_TEXT::GetTextThickness` (the base-class body the `EdaTextItem` default forwards to).
pub fn text_thickness<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> i32 {
    crate::port_pending!("EDA_TEXT::GetTextThickness", host, ctx)
}

/// Port of `EDA_TEXT::GetTextAngle` (the base-class body the `EdaTextItem` default forwards to).
pub fn text_angle<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>) -> EdaAngle {
    crate::port_pending!("EDA_TEXT::GetTextAngle", host, ctx)
}

/// Port of `EDA_TEXT::GetShownText` (the base-class body the `EdaTextItem` default forwards to).
pub fn shown_text<H: EdaTextItem + ?Sized>(
    host: &H,
    ctx: &BoardCtx<'_>,
    allow_extra_text: bool,
    depth: i32,
) -> String {
    crate::port_pending!("EDA_TEXT::GetShownText", host, ctx, allow_extra_text, depth)
}

/// Port of `EDA_TEXT::GetTextBox` (the base-class body the `EdaTextItem` default forwards to).
pub fn text_box<H: EdaTextItem + ?Sized>(host: &H, ctx: &BoardCtx<'_>, line: Option<i32>) -> Box2I {
    crate::port_pending!("EDA_TEXT::GetTextBox", host, ctx, line)
}

/// Port of `EDA_TEXT::SetTextPos` (the base-class body the `EdaTextItem` default forwards to).
pub fn set_text_pos<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, pos: Vector2I) {
    crate::port_pending!("EDA_TEXT::SetTextPos", host, env, pos)
}

/// Port of `EDA_TEXT::SetTextSize` (the base-class body the `EdaTextItem` default forwards to).
pub fn set_text_size<H: EdaTextItem + ?Sized>(
    host: &mut H,
    env: &MutEnv,
    size: Vector2I,
    enforce_min: bool,
) {
    crate::port_pending!("EDA_TEXT::SetTextSize", host, env, size, enforce_min)
}

/// Port of `EDA_TEXT::SetTextThickness` (the base-class body the `EdaTextItem` default forwards to).
pub fn set_text_thickness<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, width: i32) {
    crate::port_pending!("EDA_TEXT::SetTextThickness", host, env, width)
}

/// Port of `EDA_TEXT::SetTextAngle` (the base-class body the `EdaTextItem` default forwards to).
pub fn set_text_angle<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, angle: EdaAngle) {
    crate::port_pending!("EDA_TEXT::SetTextAngle", host, env, angle)
}

/// Port of `EDA_TEXT::Offset` (the base-class body the `EdaTextItem` default forwards to).
pub fn offset<H: EdaTextItem + ?Sized>(host: &mut H, env: &MutEnv, delta: Vector2I) {
    crate::port_pending!("EDA_TEXT::Offset", host, env, delta)
}
