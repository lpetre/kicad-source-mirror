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

//! FIX-FAB-3 (finding X-2 of `docs/design/review-fab.md`): `PCB_SHAPE::Normalize()` on
//! rectangles wider / taller than 2^31 nm.  The reference build wraps `end - start` and the
//! narrowing conversions of `BOX2I::Normalize()` / `GetEnd()`; the port must not panic and must
//! give the same corners.  Expected values: reference `pcb-dump board --sections shape` of the
//! board below (`shape|<uuid>|start=` / `end=`).

use kicad_core::Kiid;
use kicad_geom::Vector2I;
use kicad_pcb::io::{ParseOptions, parse_board_report};
use kicad_pcb::item_ref::AnyItem;

const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user) (37 "F.SilkS" user "F.Silkscreen"))
  (setup (pad_to_mask_clearance 0))
  (net 0 "")
  (gr_rect (start -510 -310) (end 2010 2010) (stroke (width 0.1) (type default)) (fill none) (layer "F.SilkS") (uuid "11111111-2222-3333-4444-555555555555"))
  (gr_rect (start 2010 5) (end -510 -310) (stroke (width 0.1) (type default)) (fill none) (layer "F.SilkS") (uuid "11111111-2222-3333-4444-555555555556"))
  (gr_rect (start 10 20) (end 5 1) (stroke (width 0.1) (type default)) (fill none) (layer "F.SilkS") (uuid "11111111-2222-3333-4444-555555555557"))
)
"#;

#[test]
fn normalize_rect_beyond_int_range() {
    let board = parse_board_report(
        BOARD,
        "big_rect",
        &ParseOptions {
            allow_pending: true,
        },
    )
    .unwrap_or_else(|e| panic!("big_rect: {}", e.what()))
    .0;

    // (uuid, start, end) as printed by the reference pcb-dump
    let expected = [
        (
            "11111111-2222-3333-4444-555555555555",
            Vector2I::new(2010000000, 2010000000),
            Vector2I::new(-510000000, -310000000),
        ),
        (
            "11111111-2222-3333-4444-555555555556",
            Vector2I::new(2010000000, -310000000),
            Vector2I::new(-510000000, 5000000),
        ),
        (
            "11111111-2222-3333-4444-555555555557",
            Vector2I::new(5000000, 1000000),
            Vector2I::new(10000000, 20000000),
        ),
    ];

    for (uuid, start, end) in expected {
        let id = Kiid::parse(uuid).unwrap();
        let shape = board
            .drawings()
            .find_map(|(_, item)| match item {
                AnyItem::Shape(s) if s.base.item.uuid == id => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("graphic {uuid} not found"));
        assert_eq!(shape.shape.start(), start, "{uuid} start");
        assert_eq!(shape.shape.end(), end, "{uuid} end");
    }
}
