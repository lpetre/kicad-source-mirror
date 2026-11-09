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

//! Temporary stand-ins for foundation types that `kicad-geom` / `kicad-core` / `kicad-font` do
//! not provide yet (`board-model.md` 17). Each item is marked with the crate that will provide
//! it; `rg foundation_stub crates/kicad-pcb` must be empty before the crate is declared done.
//! Nothing else in kicad-pcb defines a foundation type. Checked at M0a against the
//! PORT_STATUS.md of kicad-geom (TransformTrs, Ellipse, Vector3D, ShapeNull, ErrorLoc,
//! Hash128 exist), kicad-core (UnitsProvider, plot enums, json::Value, KicadT exist) and
//! kicad-font (Font, Metrics exist).

use std::collections::BTreeMap;

use kicad_core::case_insensitive_map::CaseInsensitiveKey;
use kicad_geom::geometry::ellipse::Ellipse;

// REMOVE when kicad-font provides FontRef
/// Stand-in for a cheap shared reference to a `KIFONT::FONT` (`EDA_TEXT::m_attributes.m_Font`,
/// render-cache key): the font is identified by name and style, as `KIFONT::FONT::GetFont`.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct FontRef {
    pub name: String,
    pub bold: bool,
    pub italic: bool,
}

// REMOVE when kicad-geom provides ShapeEllipse
/// Stand-in for `SHAPE_ELLIPSE` (`libs/kimath/include/geometry/shape_ellipse.h`).
#[derive(Clone, Debug)]
pub struct ShapeEllipse {
    pub ellipse: Ellipse<i32>,
}

// REMOVE when kicad-core provides CaseInsensitiveMap
/// Stand-in for `CASE_INSENSITIVE_MAP<V>` (`std::map<wxString, V, CASE_INSENSITIVE_LESS>`).
pub type CaseInsensitiveMap<V> = BTreeMap<CaseInsensitiveKey, V>;
