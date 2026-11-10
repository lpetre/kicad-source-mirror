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

//! Driver tests of package ZF-01 (`ZONE_FILLER::Fill` without the per-zone copper computation,
//! which is package ZF-03): the fill DAG order, the flashing passes, island handling and the
//! iterative refill, on synthetic boards filled with the test stand-in
//! (`set_trivial_copper_fill`: fill = smoothed outline). Port of the driver part of
//! `qa/tests/pcbnew/test_zone_filler.cpp` `ZoneFillDependencyKnockoutMargin`; the cases that
//! compare real fills run the real copper fill (ZF-03).

use std::sync::Arc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_geom::{Shape, ShapePolySet};
use kicad_pcb::Board;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::pcb_track::{Track, ZoneLayerOverride};
use kicad_zone::ZoneFiller;

/// A two-layer board with nets GND (1) and VCC (2), a 100 x 100 mm Edge.Cuts rectangle and
/// `items`.
fn board_text(items: &str) -> String {
    format!(
        r#"(kicad_pcb (version 20260206) (generator "pcbnew") (generator_version "10.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (paper "A4")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (setup (pad_to_mask_clearance 0))
  (net 0 "") (net 1 "GND") (net 2 "VCC")
  (gr_rect (start 0 0) (end 100 100) (layer "Edge.Cuts")
    (uuid "aaaaaaaa-0000-0000-0000-000000000001") (stroke (width 0.05) (type default)))
  {items}
)"#
    )
}

/// A zone on `layers` (e.g. `"F.Cu"` or `"F.Cu" "B.Cu"`) over the rectangle (x0, y0) - (x1,
/// y1) in mm.
fn zone_text(uuid_n: u32, net: &str, layers: &str, priority: u32, rect: [f64; 4]) -> String {
    let [x0, y0, x1, y1] = rect;
    format!(
        r#"(zone (net "{net}") (layers {layers}) (uuid "bbbbbbbb-0000-0000-0000-{uuid_n:012}")
    (hatch edge 0.5) (priority {priority}) (connect_pads (clearance 0.2)) (min_thickness 0.25)
    (fill yes (thermal_gap 0.5) (thermal_bridge_width 0.5) (island_removal_mode 0))
    (polygon (pts (xy {x0} {y0}) (xy {x1} {y0}) (xy {x1} {y1}) (xy {x0} {y1}))))"#
    )
}

/// A through via at (x, y) mm that removes its unconnected layers.
fn via_text(uuid_n: u32, net: &str, x: f64, y: f64) -> String {
    format!(
        r#"(via (at {x} {y}) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu")
    (remove_unused_layers yes) (keep_end_layers no) (net "{net}")
    (uuid "cccccccc-0000-0000-0000-{uuid_n:012}"))"#
    )
}

struct Fixture {
    board: Board,
    connectivity: ConnectivityData,
    engine: DrcEngine,
}

impl Fixture {
    fn new(items: &str) -> Fixture {
        Fixture::with_board(
            parse_board(
                &board_text(items),
                "test.kicad_pcb",
                &ParseOptions::default(),
            )
            .expect("board parses"),
        )
    }

    fn with_board(mut board: Board) -> Fixture {
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

    /// `KI_TEST::FillZones`-like: fill every board zone with the trivial copper fill.
    fn fill_trivial(&mut self) -> ZoneFiller {
        let zones = self.zones();
        let mut filler = ZoneFiller::new(&self.board);
        filler.set_trivial_copper_fill(true);
        assert!(filler.fill(
            &mut self.board,
            &mut self.connectivity,
            &self.engine,
            &zones
        ));
        filler
    }

    fn via_override(&self, n: usize, layer: PcbLayerId) -> ZoneLayerOverride {
        let vias: Vec<_> = self
            .board
            .tracks()
            .filter_map(|(_, t)| match t {
                Track::Via(v) => Some(v),
                _ => None,
            })
            .collect();
        vias[n].zone_layer_override(layer)
    }
}

/// Port of the scheduling part of `ZoneFillDependencyKnockoutMargin`
/// (test_zone_filler.cpp L4150): a lower-priority zone whose separation from a higher-priority
/// different-net zone lies inside the knockout reach (clearance + extra clearance + max
/// error / 2) must be filled after it. The C++ asserts it through the resulting fills (needs
/// ZF-03, see `zone_fill_dependency_knockout_margin_fills`); the sequential port checks the
/// dependency edge through the fill order directly.
#[test]
fn zone_fill_dependency_knockout_margin_order() {
    let mut board = Board::new();

    {
        let bds = board.design_settings_mut();
        let mut netclass = (**bds.net_settings.default_netclass()).clone();
        netclass.clearance = Some(PCB_IU_SCALE.mm_to_iu(2.0));
        bds.net_settings.set_default_netclass(Arc::new(netclass));
        bds.net_settings.clear_all_caches();
    }

    let clearance = PCB_IU_SCALE.mm_to_iu(2.0);
    let extra_margin = PCB_IU_SCALE.mm_to_iu(0.0005);
    let max_error = board.design_settings().max_error();
    let sep = clearance + extra_margin + max_error / 2;
    let ax = (PCB_IU_SCALE.mm_to_iu(100.0) + sep) as f64 / 1e6;

    // The sawtooth of the C++ test only slows zone B down (thread race); it does not change
    // the dependency, so the port uses the plain rectangle.
    let text = board_text(&format!(
        "{}\n{}",
        zone_text(1, "GND", r#""F.Cu""#, 0, [ax, 0.0, ax + 2.0, 10.0]),
        zone_text(2, "VCC", r#""F.Cu""#, 1, [1.0, 0.0, 100.0, 100.0]),
    ));
    let parsed = parse_board(&text, "test.kicad_pcb", &ParseOptions::default()).unwrap();

    // Keep the netclass of `board`, take the items of `parsed`.
    let mut parsed = parsed;
    *parsed.design_settings_mut() = board.design_settings().clone();

    let mut f = Fixture::with_board(parsed);
    let filler = f.fill_trivial();

    let zones = f.zones();
    let order: Vec<ZoneId> = filler.fill_order().iter().map(|(z, _)| *z).collect();

    assert_eq!(filler.worst_clearance(), clearance);
    assert_eq!(
        order,
        vec![zones[1], zones[0]],
        "zone B (priority 1) fills first"
    );
}

/// The fill assertion of `ZoneFillDependencyKnockoutMargin` with the real copper fill
/// (package ZF-03): both zones fill.
#[test]
fn zone_fill_dependency_knockout_margin_fills() {
    let mut f = Fixture::new(&format!(
        "{}\n{}",
        zone_text(1, "GND", r#""F.Cu""#, 0, [60.0, 0.0, 62.0, 10.0]),
        zone_text(2, "VCC", r#""F.Cu""#, 1, [1.0, 0.0, 59.0, 100.0]),
    ));
    let zones = f.zones();
    let mut filler = ZoneFiller::new(&f.board);
    filler.fill(&mut f.board, &mut f.connectivity, &f.engine, &zones);

    let fill_a = f
        .board
        .zone(zones[0])
        .unwrap()
        .filled_polys_list(PcbLayerId::FCu);
    let fill_b = f
        .board
        .zone(zones[1])
        .unwrap()
        .filled_polys_list(PcbLayerId::FCu);
    assert!(fill_a.outline_count() > 0 && fill_b.outline_count() > 0);
}

/// Conditional via flashing: the pre-pass forces flashing inside a same-net zone and removes
/// the connection elsewhere; the second pass keeps it where the fill reaches the via.
#[test]
fn via_flashing_passes() {
    let mut f = Fixture::new(&format!(
        "{}\n{}\n{}\n{}\n{}",
        zone_text(1, "GND", r#""F.Cu" "B.Cu""#, 0, [10.0, 10.0, 40.0, 40.0]),
        zone_text(2, "VCC", r#""B.Cu""#, 1, [20.0, 20.0, 40.0, 40.0]),
        via_text(1, "GND", 15.0, 15.0), // inside GND on both layers
        via_text(2, "GND", 30.0, 30.0), // GND on F.Cu; B.Cu: GND under higher VCC (same net preferred)
        via_text(3, "GND", 60.0, 60.0), // outside every zone
    ));

    f.fill_trivial();

    use ZoneLayerOverride::{ForceFlashed, ForceNoZoneConnection};

    assert_eq!(f.via_override(0, PcbLayerId::FCu), ForceFlashed);
    assert_eq!(f.via_override(0, PcbLayerId::BCu), ForceFlashed);
    assert_eq!(f.via_override(1, PcbLayerId::FCu), ForceFlashed);
    // Prefer any same-net zone over a higher-priority different-net zone (issue 24175); the
    // trivial fill of the GND zone does not knock the VCC zone out, so the fill reaches it.
    assert_eq!(f.via_override(1, PcbLayerId::BCu), ForceFlashed);
    assert_eq!(f.via_override(2, PcbLayerId::FCu), ForceNoZoneConnection);
    assert_eq!(f.via_override(2, PcbLayerId::BCu), ForceNoZoneConnection);
}

/// Island handling without pads: every outline of every zone is isolated, so each zone layer
/// is "initially fully isolated" and kept as is (unconnected pours are preserved), and nothing
/// is flagged; a net-less zone reports all its outlines as islands.
#[test]
fn unconnected_pours_are_preserved() {
    let mut f = Fixture::new(&format!(
        "{}\n{}",
        zone_text(1, "GND", r#""F.Cu""#, 0, [10.0, 10.0, 40.0, 40.0]),
        zone_text(2, "", r#""B.Cu""#, 0, [10.0, 10.0, 40.0, 40.0]),
    ));

    f.fill_trivial();

    let zones = f.zones();
    let gnd = f.board.zone(zones[0]).unwrap();
    let nonet = f.board.zone(zones[1]).unwrap();

    assert!(gnd.is_filled() && nonet.is_filled());
    assert_eq!(gnd.filled_polys_list(PcbLayerId::FCu).outline_count(), 1);
    assert!(!gnd.is_island(PcbLayerId::FCu, 0));
    assert!(gnd.fill().fill_flag(PcbLayerId::FCu));
    assert_eq!(nonet.filled_polys_list(PcbLayerId::BCu).outline_count(), 1);
    assert!(nonet.is_island(PcbLayerId::BCu, 0));
}

/// The same-net overlap seeds start the iterative refill: the lower-priority zone is refilled
/// from its cached pre-knockout fill minus the higher-priority same-net zone's fill
/// (refillZoneFromCache), so the two fills no longer overlap.
#[test]
fn same_net_overlap_refill() {
    let mut f = Fixture::new(&format!(
        "{}\n{}",
        zone_text(1, "GND", r#""F.Cu""#, 0, [10.0, 10.0, 40.0, 40.0]),
        zone_text(2, "GND", r#""F.Cu""#, 1, [30.0, 10.0, 50.0, 40.0]),
    ));

    let filler = f.fill_trivial();
    assert!(filler.warnings().is_empty());

    let zones = f.zones();
    let low = f
        .board
        .zone(zones[0])
        .unwrap()
        .filled_polys_list(PcbLayerId::FCu);
    let high = f
        .board
        .zone(zones[1])
        .unwrap()
        .filled_polys_list(PcbLayerId::FCu);

    // The trivial fill is BuildSmoothedPoly's apron output: each zone reaches min_thickness
    // (0.25 mm) into the abutting same-net zone. The refilled lower zone stops where the
    // higher zone's fill starts (x = 29.75 mm).
    let mm2 = 1e12;
    assert!(high.area() / mm2 > 600.0 && high.area() / mm2 < 610.0);
    assert!(
        low.area() / mm2 > 580.0 && low.area() / mm2 < 600.0,
        "{}",
        low.area() / mm2
    );

    let mut overlap = ShapePolySet::new();
    overlap.boolean_intersection_sets(&low, &high);
    assert!(overlap.area() < 1.0);
    assert!(low.bbox(0).right() <= PCB_IU_SCALE.mm_to_iu(30.0));
}
