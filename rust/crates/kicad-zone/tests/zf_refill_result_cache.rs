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

//! The refill result cache (`ZONE_FILLER::m_refillResultCache`, seeded by the port from the
//! knockout-free initial fills) on a board whose iterative refill hits it both ways:
//!
//! * wave 1 refills the higher-priority VCC zone, whose island was removed and which has no
//!   knockout at all: the seed from its initial fill is reused;
//! * wave 2 refills the lower GND zone again with the knockouts of wave 1 (the VCC fill did
//!   not change): the wave-1 result is reused.
//!
//! In the dev profile every hit is also recomputed and compared (a `debug_assert!` in the
//! filler), so running this test proves both hits equal the recomputation. The test itself
//! checks the visible results: the refill reclaimed the freed area, the fills are the same
//! with a reused and a fresh filler (R-11), and no cache reference to a board fill survives
//! `fill()`.

use std::sync::Arc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_geom::{ShapePolySet, Vector2I};
use kicad_pcb::Board;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_zone::ZoneFiller;

/// GND zone (priority 0) over most of the board, connected by a pad at (20, 20); VCC zone
/// (priority 1) in its middle, cut in two by a GND track at y = 50 mm, of which only the upper
/// half holds a VCC pad. The lower VCC half is an island.
const BOARD: &str = r#"(kicad_pcb (version 20260206) (generator "pcbnew") (generator_version "10.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "GND") (net 2 "VCC")
  (gr_rect (start 0 0) (end 100 100) (layer "Edge.Cuts")
    (uuid "aaaaaaaa-0000-0000-0000-000000000001") (stroke (width 0.05) (type default)))
  (zone (net "GND") (layers "F.Cu") (uuid "bbbbbbbb-0000-0000-0000-000000000001")
    (hatch edge 0.5) (priority 0) (connect_pads (clearance 0.2)) (min_thickness 0.25)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 0))
    (polygon (pts (xy 10 10) (xy 90 10) (xy 90 90) (xy 10 90))))
  (zone (net "VCC") (layers "F.Cu") (uuid "bbbbbbbb-0000-0000-0000-000000000002")
    (hatch edge 0.5) (priority 1) (connect_pads (clearance 0.2)) (min_thickness 0.25)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 0))
    (polygon (pts (xy 40 40) (xy 60 40) (xy 60 60) (xy 40 60))))
  (footprint "G" (layer "F.Cu") (at 20 20) (uuid "eeeeeeee-0000-0000-0000-000000000001")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net "GND")
      (uuid "ffffffff-0000-0000-0000-000000000001")))
  (footprint "V" (layer "F.Cu") (at 50 45) (uuid "eeeeeeee-0000-0000-0000-000000000002")
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net "VCC")
      (uuid "ffffffff-0000-0000-0000-000000000002")))
  (segment (start 35 50) (end 65 50) (width 0.25) (layer "F.Cu") (net "GND")
    (uuid "dddddddd-0000-0000-0000-000000000001"))
)"#;

struct Fixture {
    board: Board,
    connectivity: ConnectivityData,
    engine: DrcEngine,
}

impl Fixture {
    fn new() -> Fixture {
        let mut board = parse_board(BOARD, "test.kicad_pcb", &ParseOptions::default()).unwrap();
        let mut engine = DrcEngine::new();
        engine
            .init_engine(
                &mut board,
                None,
                default_layer_choices(),
                &TuningProfiles::default(),
                None,
            )
            .expect("implicit rules");

        let mut connectivity = ConnectivityData::new();
        connectivity.build_connectivity(&mut board);

        Fixture {
            board,
            connectivity,
            engine,
        }
    }

    fn zones(&self) -> Vec<ZoneId> {
        self.board.zones().map(|(id, _)| id).collect()
    }

    fn fill_with(&mut self, filler: &mut ZoneFiller) {
        let zones = self.zones();
        assert!(filler.fill(
            &mut self.board,
            &mut self.connectivity,
            &self.engine,
            &zones
        ));
        assert!(filler.warnings().is_empty(), "{:?}", filler.warnings());
    }

    fn fill(&self, zone: ZoneId) -> &Arc<ShapePolySet> {
        self.board
            .zone(zone)
            .unwrap()
            .fill()
            .filled_polys_list
            .get(&PcbLayerId::FCu)
            .expect("F.Cu fill")
    }
}

fn mm(x: f64, y: f64) -> Vector2I {
    Vector2I::new(PCB_IU_SCALE.mm_to_iu(x), PCB_IU_SCALE.mm_to_iu(y))
}

#[test]
fn refill_result_cache_hits_keep_the_fills() {
    let mut f = Fixture::new();
    let mut filler = ZoneFiller::new(&f.board);
    f.fill_with(&mut filler);

    let zones = f.zones();
    let (gnd, vcc) = (zones[0], zones[1]);

    // The VCC island (lower half) is removed, the upper half stays; the GND zone reclaimed the
    // freed area in the refill.
    assert_eq!(f.fill(vcc).outline_count(), 1);
    assert!(f.fill(vcc).contains(mm(50.0, 42.0), -1, 0, false));
    assert!(!f.fill(vcc).contains(mm(50.0, 57.0), -1, 0, false));
    assert!(f.fill(gnd).contains(mm(50.0, 57.0), -1, 0, false));
    assert!(!f.fill(gnd).contains(mm(50.0, 42.0), -1, 0, false));

    // No cache entry still shares a board fill once fill() has returned.
    for &zone in &zones {
        assert_eq!(Arc::strong_count(f.fill(zone)), 1);
    }

    let first: Vec<ShapePolySet> = zones.iter().map(|&z| (**f.fill(z)).clone()).collect();

    // A reused filler (R-11) starts each fill() with an empty result cache: refilling the same
    // board gives the same fills as the first run and as a fresh filler on a fresh board.
    f.fill_with(&mut filler);

    let mut fresh = Fixture::new();
    let mut fresh_filler = ZoneFiller::new(&fresh.board);
    fresh.fill_with(&mut fresh_filler);

    for (i, &zone) in zones.iter().enumerate() {
        let again = f.fill(zone);
        let other = fresh.fill(fresh.zones()[i]);

        assert_eq!(again.c_polygons(), first[i].c_polygons());
        assert_eq!(other.c_polygons(), first[i].c_polygons());
        assert_eq!(Arc::strong_count(again), 1);
    }
}
