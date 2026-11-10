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

//! Board access and editing API for external routers (package WASM-API-1).
//!
//! New code without one C++ counterpart: a host-independent, serde-serialisable view of the
//! board model for clients that route by themselves (a browser autorouter) and need KiCad
//! for everything around the routing: reading the board, the resolved design rules, editing
//! the copper, teardrops, zone refill, DRC and save. Every value comes from the ported model
//! getters (the same calls the reference dump `pcb-dump` / `pcb-dump-rs` prints), every edit
//! goes through the model's mutation API (`Board::add_track`, `Board::modify_track`,
//! `Board::remove`, ...) followed by the connectivity bookkeeping of `BOARD_COMMIT::Push`
//! (`CONNECTIVITY_DATA::Add / Remove / Update`, then `RecalculateRatsnest`).
//!
//! Conventions (see `rust/docs/WASM_API.md`):
//! * coordinates and lengths are KiCad internal units (integer nanometres), points are
//!   `[x, y]`, boxes `[x, y, w, h]` (`BOX2I` position and size as stored);
//! * angles are degrees (`EDA_ANGLE::AsDegrees`);
//! * layers are canonical names (`LSET::Name`: `F.Cu`, `In1.Cu`, `B.Cu`, `Edge.Cuts`);
//! * items are identified by their UUID string and a numeric handle `id`
//!   (`ItemId::to_bits`, exact as a JavaScript number) that is valid for the session until
//!   the item is removed;
//! * polygons are lists of outlines, each `{ outline, holes }`, the vertex order KiCad's.
//!
//! | module | content |
//! |---|---|
//! | [`read`] | board, nets, net classes, pads, footprints, tracks, zones, obstacles, connectivity |
//! | [`rules`] | resolved constraints through `DRC_ENGINE::EvalRules`, router default sizes |
//! | [`edit`] | add / move / modify / remove items, batches, teardrops, refill |
//! | [`check`] | structured DRC (full job and quick provider selection) |
//! | [`commit`] | transactions (`BOARD_COMMIT::Push` with incremental connectivity), checkpoints, finalize |
//! | [`bulk`] | flat integer record forms of the route items for batch transfer |
//! | [`refill`] | change damage of commits, affected-only zone refill |

pub mod bulk;
pub mod check;
pub mod commit;
pub mod edit;
pub mod read;
pub mod refill;
pub mod rules;

use kicad_core::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_geom::{ShapeLineChain, ShapePolySet, Vector2I};
use kicad_pcb::ItemId;
use serde::{Deserialize, Serialize};

use crate::error::SessionError;

/// A point `[x, y]` in nm.
pub type Pt = [i32; 2];

/// One polygon of a `SHAPE_POLY_SET`: the outline and its holes (closed chains, KiCad's
/// vertex order, no repeated closing point).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Polygon {
    /// `COutline( i ).CPoints()`.
    pub outline: Vec<Pt>,
    /// `CHole( i, h ).CPoints()` for every hole.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<Pt>>,
    /// `COutline( i ).Width()`: the stroke width a chain carries (courtyards keep the width
    /// of their graphic lines), 0 for plain areas.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub width: i32,
}

fn is_zero(v: &i32) -> bool {
    *v == 0
}

/// A `SHAPE_POLY_SET` as a list of [`Polygon`]s.
pub type Polygons = Vec<Polygon>;

pub(crate) fn pt(v: Vector2I) -> Pt {
    [v.x, v.y]
}

pub(crate) fn vec2(p: Pt) -> Vector2I {
    Vector2I::new(p[0], p[1])
}

fn chain_points(c: &ShapeLineChain) -> Vec<Pt> {
    c.c_points().iter().map(|p| pt(*p)).collect()
}

/// The [`Polygons`] of a `SHAPE_POLY_SET` (outline order, then hole order).
pub fn polygons(set: &ShapePolySet) -> Polygons {
    (0..set.outline_count())
        .map(|i| Polygon {
            width: set.c_outline(i).width(),
            outline: chain_points(set.c_outline(i)),
            holes: (0..set.hole_count(i))
                .map(|h| chain_points(set.c_hole(i, h)))
                .collect(),
        })
        .collect()
}

/// `LSET::Name( aLayer )`.
pub fn layer_name(layer: PcbLayerId) -> String {
    Lset::name(layer)
}

/// `LSET::NameToLayer` restricted to real layers: the layer of a canonical name.
pub fn parse_layer(name: &str) -> Result<PcbLayerId, SessionError> {
    let id = Lset::name_to_layer(name);

    if id < 0 {
        return Err(SessionError::invalid_argument(format!(
            "unknown layer '{name}'"
        )));
    }

    Ok(PcbLayerId::from(id))
}

/// The layer names of an `LSET` in `Seq()` order.
pub fn layer_names(set: &Lset) -> Vec<String> {
    set.seq().iter().map(|l| layer_name(*l)).collect()
}

/// The numeric handle of an item (`ItemId::to_bits`).
pub fn handle(id: impl Into<ItemId>) -> u64 {
    id.into().to_bits()
}

/// The item of a handle; `StaleId` for a malformed handle.
pub(crate) fn item_id(id: u64) -> Result<ItemId, SessionError> {
    ItemId::from_bits(id).ok_or(SessionError::StaleId { id })
}

/// The C++ enumerator name of a ported enum (the Rust variant name in UPPER_SNAKE, as the
/// reference dump prints it: `Roundrect` -> `ROUNDRECT`, `ChamferedRect` -> `CHAMFERED_RECT`).
pub fn enum_name<E: std::fmt::Debug>(e: E) -> String {
    let dbg = format!("{e:?}");
    let name: Vec<char> = dbg
        .split(['(', ' ', '{'])
        .next()
        .unwrap_or("")
        .chars()
        .collect();
    let mut r = String::with_capacity(name.len() + 4);

    for (i, &c) in name.iter().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            let prev = name[i - 1];
            let next_lower = name.get(i + 1).is_some_and(|n| n.is_ascii_lowercase());

            if prev.is_ascii_lowercase()
                || prev.is_ascii_digit()
                || (prev.is_ascii_uppercase() && next_lower)
            {
                r.push('_');
            }
        }

        r.push(c.to_ascii_uppercase());
    }

    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_names_follow_the_dump() {
        assert_eq!(
            enum_name(kicad_pcb::padstack::PadShape::ChamferedRect),
            "CHAMFERED_RECT"
        );
        assert_eq!(enum_name(kicad_pcb::padstack::PadAttrib::Npth), "NPTH");
        assert_eq!(enum_name(PcbLayerId::In1Cu), "IN1_CU");
    }

    #[test]
    fn polygon_conversion_keeps_holes() {
        let mut set = ShapePolySet::new();
        let mut outer = ShapeLineChain::new();
        for p in [[0, 0], [10, 0], [10, 10], [0, 10]] {
            outer.append_point(vec2(p), false);
        }
        outer.set_closed(true);
        set.add_outline(&outer);
        let mut hole = ShapeLineChain::new();
        for p in [[2, 2], [4, 2], [4, 4]] {
            hole.append_point(vec2(p), false);
        }
        hole.set_closed(true);
        set.add_hole(&hole, -1);

        let p = polygons(&set);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].outline.len(), 4);
        assert_eq!(p[0].holes, vec![vec![[2, 2], [4, 2], [4, 4]]]);
        assert_eq!(p[0].width, 0);
    }
}
