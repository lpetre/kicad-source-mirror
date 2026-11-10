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

//! `Board::set_zone_layer_override` / `Board::clear_zone_layer_overrides` on a stored via
//! (`PCB_VIA::SetZoneLayerOverride` / `ClearZoneLayerOverrides`, written by the zone filler).

use kicad_core::PcbLayerId;
use kicad_pcb::ids::ItemId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::pcb_track::{Track, ZoneLayerOverride};

#[test]
fn via_zone_layer_overrides_through_board() {
    let text = r#"(kicad_pcb (version 20260206) (generator "pcbnew") (generator_version "10.0")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "")
  (via (at 10 10) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "")
    (uuid "cccccccc-0000-0000-0000-000000000001")))"#;
    let mut board = parse_board(text, "t.kicad_pcb", &ParseOptions::default()).unwrap();
    let (track_id, _) = board.tracks().next().unwrap();
    let id: ItemId = track_id.into();

    let get = |board: &kicad_pcb::Board, layer| match board.track(track_id) {
        Some(Track::Via(via)) => via.zone_layer_override(layer),
        _ => unreachable!(),
    };

    let stamp = board.time_stamp();
    board.set_zone_layer_override(id, PcbLayerId::BCu, ZoneLayerOverride::ForceFlashed);
    // a non-copper layer has no slot (PCB_VIA ignores it)
    board.set_zone_layer_override(id, PcbLayerId::EdgeCuts, ZoneLayerOverride::ForceFlashed);

    assert_eq!(get(&board, PcbLayerId::BCu), ZoneLayerOverride::ForceFlashed);
    assert_eq!(get(&board, PcbLayerId::FCu), ZoneLayerOverride::None);
    assert_eq!(get(&board, PcbLayerId::EdgeCuts), ZoneLayerOverride::None);

    board.clear_zone_layer_overrides(id);
    assert_eq!(get(&board, PcbLayerId::BCu), ZoneLayerOverride::None);

    // derived state: no time stamp bump
    assert_eq!(board.time_stamp(), stamp);
}
