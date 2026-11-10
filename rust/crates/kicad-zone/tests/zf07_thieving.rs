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

//! Copper thieving tests of package ZF-07 (`ZONE_FILLER::addCopperThievingPattern`): port of
//! the `CopperThievingZone_*` cases of `qa/tests/pcbnew/test_zone_filler.cpp`.
//! (`qa/tests/pcbnew/drc/test_drc_copper_thieving.cpp` is ported in kicad-drc,
//! tests/drc07_connectivity.rs: it checks the stored fill, no refill.) The vertex-exact
//! comparison against the reference refill is tests/zone_fill_corpus.rs on the crafted boards of
//! tests/oracle/gen_zf07_thieving.py.

use std::f64::consts::PI;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::{PCB_IU_SCALE, PcbLayerId};
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_geom::math::vector2::Vector2I;
use kicad_geom::{Shape, ShapePolySet};
use kicad_pcb::Board;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{
    IslandRemovalMode, ThievingPattern, ThievingSettings, ZoneFillMode,
};
use kicad_pcb::{AddMode, ParentRef};
use kicad_zone::ZoneFiller;

fn mm(v: f64) -> i32 {
    PCB_IU_SCALE.mm_to_iu(v)
}

/// `KI_TEST::FillZones( board )`: fill every zone of `board`.
fn fill_zones(board: &mut Board) {
    let mut engine = DrcEngine::new();
    engine
        .init_engine(
            board,
            None,
            default_layer_choices(),
            &TuningProfiles::default(),
            None,
        )
        .expect("implicit rules");

    let mut connectivity = ConnectivityData::new();
    connectivity.build_connectivity(board);

    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();
    let mut filler = ZoneFiller::new(board);
    filler.fill(board, &mut connectivity, &engine, &zones);
}

/// `new ZONE( board )` on `layer` over the square (0, 0) - (size_mm, size_mm), copper
/// thieving with `thieving`, island removal NEVER, added to `board`.
fn add_square_thieving_zone(
    board: &mut Board,
    layer: PcbLayerId,
    size_mm: f64,
    thieving: &ThievingSettings,
) -> ZoneId {
    let settings = board.design_settings().default_zone_settings().clone();
    let mut zone = Zone::new_with_settings(&mut board.kiid_generator, &settings, false);
    let env = board.child_env(ParentRef::Board);
    zone.set_layer(&env, layer);
    zone.append_corner(Vector2I::new(0, 0), -1, false);
    zone.append_corner(Vector2I::new(mm(size_mm), 0), -1, false);
    zone.append_corner(Vector2I::new(mm(size_mm), mm(size_mm)), -1, false);
    zone.append_corner(Vector2I::new(0, mm(size_mm)), -1, false);
    zone.set_fill_mode(ZoneFillMode::CopperThieving);
    zone.set_thieving_settings(thieving);
    zone.island_removal_mode = IslandRemovalMode::Never;
    board.add_zone(zone, AddMode::Insert)
}

/// `m_board = std::make_unique<BOARD>(); m_board->SetCopperLayerCount( 2 );`
fn two_layer_board() -> Board {
    let mut board = Board::new();
    board.set_copper_layer_count(2);
    board
}

fn fill_of(board: &Board, zone: ZoneId, layer: PcbLayerId) -> std::sync::Arc<ShapePolySet> {
    board.zone(zone).unwrap().filled_polys_list(layer)
}

/// `CheckAllOutlineAreasAtLeast( aFill, aMinArea, aLabel )`.
fn check_all_outline_areas_at_least(fill: &ShapePolySet, min_area: f64, label: &str) {
    for ii in 0..fill.outline_count() {
        let area = fill.c_outline(ii).area(true).abs();
        assert!(
            area >= min_area,
            "{label} {ii} area {area:.0} IU^2 below {min_area:.0} IU^2; partial stamps should \
             not survive."
        );
    }
}

fn thieving(pattern: ThievingPattern, element_size: f64, gap: f64) -> ThievingSettings {
    ThievingSettings {
        pattern,
        element_size: mm(element_size),
        gap: mm(gap),
        ..ThievingSettings::default()
    }
}

/// Port of `CopperThievingZone_HatchSurvivesTrackBisection`: a track bisecting a thieving
/// hatch zone must not destroy the fill on the far side of the track.
#[test]
fn copper_thieving_zone_hatch_survives_track_bisection() {
    let path = kicad_qa::pcbnew_data_dir().join("zone_thieving_track_bisection.kicad_pcb");
    let text = std::fs::read_to_string(&path).expect("board file");
    let mut board =
        parse_board(&text, path.to_str().unwrap(), &ParseOptions::default()).expect("parses");
    fill_zones(&mut board);

    let (zone_id, zone) = board
        .zones()
        .find(|(_, z)| z.fill_mode() == ZoneFillMode::CopperThieving)
        .expect("thieving zone");

    let fill = zone.filled_polys_list(PcbLayerId::FCu);

    // The track splits the fill area in two; before the fix the connectivity
    // pass classified the narrow side as an isolated island and deleted it.
    // Expect at least two outlines covering both halves of the original zone.
    assert!(fill.outline_count() >= 2, "{}", fill.outline_count());

    // The fill must span the full zone width (left edge through right edge).
    let fill_box = fill.bbox(0);
    let zone_box = board.zone(zone_id).unwrap().outline().bbox(0);

    assert!(fill_box.left() < zone_box.left() + mm(2.0));
    assert!(fill_box.right() > zone_box.right() - mm(2.0));

    // The mesh must have real structure on both sides.
    assert!(fill.total_vertices() > 200, "{}", fill.total_vertices());
}

/// Port of `CopperThievingZone_NonCopperLayerStampsNotSolid`: a copper-thieving zone on a
/// non-copper layer must still produce stamps rather than degenerating to a solid fill.
#[test]
fn copper_thieving_zone_non_copper_layer_stamps_not_solid() {
    let mut board = Board::new();
    let t = thieving(ThievingPattern::Dots, 0.5, 1.5);
    let zone = add_square_thieving_zone(&mut board, PcbLayerId::FSilkS, 10.0, &t);

    fill_zones(&mut board);

    let fill = fill_of(&board, zone, PcbLayerId::FSilkS);

    // A solid fill would have one outline (the zone polygon); a dots grid
    // produces dozens.  Lower bound is conservative to avoid edge-clipping flakiness.
    assert!(fill.outline_count() > 5, "{}", fill.outline_count());
}

/// Port of `CopperThievingZone_DotsGrid`: a grid of dot outlines whose count matches the
/// geometric expectation; only full dots survive.
#[test]
fn copper_thieving_zone_dots_grid() {
    let mut board = two_layer_board();
    let t = ThievingSettings {
        line_width: mm(0.3),
        stagger: false,
        ..thieving(ThievingPattern::Dots, 0.5, 2.0)
    };
    let zone = add_square_thieving_zone(&mut board, PcbLayerId::FCu, 10.0, &t);

    fill_zones(&mut board);

    let fill = fill_of(&board, zone, PcbLayerId::FCu);
    assert!(fill.outline_count() > 0);

    // 2.5 mm pitch, 0.5 mm dot.  The four positions whose disc touches the
    // zone edge (x or y at 0 or 10 mm) are dropped, leaving a 3 x 3 grid.
    assert!(fill.outline_count() >= 6, "{}", fill.outline_count());
    assert!(fill.outline_count() <= 12, "{}", fill.outline_count());

    // 10% slack covers the polygonal circle approximation plus post-fill corner rounding.
    let full_dot_area = PI * (mm(0.25) as f64).powi(2);
    check_all_outline_areas_at_least(&fill, 0.9 * full_dot_area, "Dot");
}

/// Port of `CopperThievingZone_StaggerProducesDifferentLayout`.
#[test]
fn copper_thieving_zone_stagger_produces_different_layout() {
    let count_dots = |stagger: bool| -> i32 {
        let mut board = two_layer_board();
        let t = ThievingSettings {
            stagger,
            ..thieving(ThievingPattern::Dots, 0.5, 2.0)
        };
        let zone = add_square_thieving_zone(&mut board, PcbLayerId::FCu, 20.0, &t);
        fill_zones(&mut board);
        fill_of(&board, zone, PcbLayerId::FCu).outline_count()
    };

    let plain = count_dots(false);
    let staggered = count_dots(true);

    // Within a factor of two -- catches the offset walking dots off the board
    // (returning ~0) without being brittle about edge-clipping rounding.
    assert_ne!(plain, staggered);
    assert!(staggered >= plain / 2);
    assert!(staggered <= plain * 2);
}

/// Port of `CopperThievingZone_SquaresGrid`.
#[test]
fn copper_thieving_zone_squares_grid() {
    let mut board = two_layer_board();
    let t = thieving(ThievingPattern::Squares, 0.6, 2.0);
    let zone = add_square_thieving_zone(&mut board, PcbLayerId::FCu, 10.0, &t);

    fill_zones(&mut board);

    let fill = fill_of(&board, zone, PcbLayerId::FCu);
    assert!(fill.outline_count() > 0);

    // 2.6 mm pitch, 0.6 mm square.  Same edge-drop behavior as the dots test:
    // strict-containment leaves a 3 x 3 grid of full squares.
    assert!(fill.outline_count() >= 6, "{}", fill.outline_count());
    assert!(fill.outline_count() <= 12, "{}", fill.outline_count());

    let full_square_area = (mm(0.6) as f64).powi(2);
    check_all_outline_areas_at_least(&fill, 0.9 * full_square_area, "Square");
}

/// Port of `CopperThievingZone_HighDensityPerformance` (the stamp count; the 30 s time bound
/// is not checked: no wall clock in tests that must be deterministic, and debug builds are
/// slower than the C++ release build).
#[test]
fn copper_thieving_zone_high_density_performance() {
    let mut board = two_layer_board();
    let t = thieving(ThievingPattern::Dots, 0.3, 1.0);
    let zone = add_square_thieving_zone(&mut board, PcbLayerId::FCu, 100.0, &t);

    fill_zones(&mut board);

    let fill = fill_of(&board, zone, PcbLayerId::FCu);
    assert!(fill.outline_count() > 4000, "{}", fill.outline_count());
}

/// Port of `CopperThievingZone_HatchPattern`: a hatch carves voids out of the zone outline,
/// leaving one connected polygon with a perimeter border.
#[test]
fn copper_thieving_zone_hatch_pattern() {
    let mut board = two_layer_board();
    let t = ThievingSettings {
        pattern: ThievingPattern::Hatch,
        gap: mm(2.0),
        line_width: mm(0.3),
        ..ThievingSettings::default()
    };
    let zone = add_square_thieving_zone(&mut board, PcbLayerId::FCu, 10.0, &t);

    fill_zones(&mut board);

    let fill = fill_of(&board, zone, PcbLayerId::FCu);
    assert!(fill.total_vertices() > 0);

    // Subtractive hatch produces a single connected outline after fracturing
    // (perimeter border + interior mesh linked through bridges).
    assert_eq!(fill.outline_count(), 1);

    let fill_box = fill.bbox(0);
    assert!(fill_box.left() < mm(0.5));
    assert!(fill_box.right() > mm(9.5));
    assert!(fill_box.top() < mm(0.5));
    assert!(fill_box.bottom() > mm(9.5));

    // A solid 10x10 mm rectangle would have ~4 vertices.  A hatched mesh has
    // many vertices because each void cut adds outline segments.
    assert!(fill.total_vertices() > 30, "{}", fill.total_vertices());
}
