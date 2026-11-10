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

//! Package ZF-06 (new code): the headless `ZONE_FILLER_TOOL::FillAllZones`
//! (`kicad_zone::fill_all_zones`) on a synthetic board, and the interim teardrop guard
//! (`TEARDROP_MANAGER::UpdateTeardrops` is package TD-01). The end-to-end comparison against
//! the reference `kicad-cli pcb drc --refill-zones --save-board` is
//! `kicad-session/tests/zf06_refill_save.rs`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_connectivity::data::ConnectivityData;
use kicad_core::PcbLayerId;
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_pcb::Board;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::teardrop_parameters::TargetTd;
use kicad_zone::{ZoneFiller, fill_all_zones};

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
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 1))
    (polygon (pts (xy 10 10) (xy 90 10) (xy 90 90) (xy 10 90))))
  (via (at 50 50) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "GND")
    (uuid "cccccccc-0000-0000-0000-000000000001"))
  (segment (start 30 50) (end 70 50) (width 0.25) (layer "F.Cu") (net "VCC")
    (uuid "dddddddd-0000-0000-0000-000000000001"))
)"#;

fn load() -> (Board, ConnectivityData, DrcEngine) {
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
    (board, connectivity, engine)
}

/// `FillAllZones` = teardrop update (nothing to do here) + `IncrementTimeStamp` +
/// `ZONE_FILLER::Fill( board->Zones() )` + `BuildConnectivity`: the same fill as the bare
/// filler.
#[test]
fn fill_all_zones_matches_the_filler() {
    let (mut board, mut connectivity, engine) = load();
    let outcome = fill_all_zones(&mut board, &mut connectivity, &engine);
    assert!(outcome.filled);
    assert!(outcome.warnings.is_empty());

    let (mut bare, mut bare_connectivity, bare_engine) = load();
    bare.increment_time_stamp();
    let zones: Vec<_> = bare.zones().map(|(id, _)| id).collect();
    ZoneFiller::new(&bare).fill(&mut bare, &mut bare_connectivity, &bare_engine, &zones);

    let fill = |b: &Board| {
        let (_, zone) = b.zones().next().unwrap();
        zone.get_fill(PcbLayerId::FCu).cloned().unwrap()
    };

    let filled = fill(&board);
    assert!(filled.outline_count() > 0);
    assert_eq!(format!("{:?}", filled), format!("{:?}", fill(&bare)));
}

/// A board that needs teardrop regeneration stops in the pending TD-01 hook instead of
/// silently losing its teardrops (connectivity-zones.md 10.2, interim guard).
#[test]
fn teardrop_boards_stop_in_the_pending_hook() {
    let (mut board, mut connectivity, engine) = load();
    board
        .design_settings_mut()
        .teardrop_params_list_mut()
        .parameters_mut(TargetTd::Track)
        .enabled = true;

    let result = catch_unwind(AssertUnwindSafe(|| {
        fill_all_zones(&mut board, &mut connectivity, &engine)
    }));
    let payload = result.expect_err("pending hook");
    let text = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_default();
    assert!(
        text.contains("port pending: TEARDROP_MANAGER::UpdateTeardrops"),
        "{text}"
    );

    // `if( m_board->LegacyTeardrops() ) return;`
    let (mut board, mut connectivity, engine) = load();
    board
        .design_settings_mut()
        .teardrop_params_list_mut()
        .parameters_mut(TargetTd::Track)
        .enabled = true;
    board.legacy_teardrops = true;
    assert!(fill_all_zones(&mut board, &mut connectivity, &engine).filled);
}
