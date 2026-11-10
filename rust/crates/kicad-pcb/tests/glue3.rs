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

//! Package GLUE-3: the drill model generation (`BOARD::BumpDrillModelGeneration`,
//! `PCB_DRILL_CHART::IsStale` / `m_builtGeneration`) and the generated-table overrides reached
//! through the table cascade (`PCB_GENERATED_TABLE::Flip` from `PCB_TABLE::Flip` / `Mirror`,
//! `setAutosizedRowHeights` from `PCB_TABLE::Autosize`).
//!
//! `a_chart_follows_the_board_without_being_asked` is the full
//! `AChartFollowsTheBoardWithoutBeingAsked` of `qa/tests/pcbnew/test_drill_chart.cpp` (the
//! `GetBuiltGeneration` checks M26 left out). `flip_and_mirror_match_the_reference` replays
//! `tests/golden/glue3/flip_tables.txt` (`tests/oracle/glue3_flip_golden.sh`: the FIX-EDIT-1
//! mutator driver on `pcbio12/generated_tables.kicad_pcb`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::{FlipDirection, Vector2I};
use kicad_pcb::board::{AddMode, Board};
use kicad_pcb::ids::{ItemId, TableId};
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::pcb_table::PcbTable;
use kicad_pcb::pcb_track::{PcbVia, Track};

/// The centre of the FIX-EDIT-1 mutator driver.
const CENTRE: Vector2I = Vector2I::new(12345600, 7890000);

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn mm(v: f64) -> i32 {
    kicad_core::PCB_IU_SCALE.mm_to_iu(v)
}

fn parse(text: &str) -> Board {
    parse_board_report(text, "glue3.kicad_pcb", &ParseOptions::default())
        .unwrap_or_else(|e| panic!("{}", e.what()))
        .0
}

fn generated_tables() -> String {
    std::fs::read_to_string(crate_dir().join("tests/data/pcbio12/generated_tables.kicad_pcb"))
        .unwrap()
}

fn tables_of(board: &Board, kind: KicadT) -> Vec<TableId> {
    board
        .drawings()
        .filter(|(_, item)| item.as_dyn().type_() == kind)
        .filter_map(|(id, _)| id.typed::<PcbTable>())
        .collect()
}

/// `AChartFollowsTheBoardWithoutBeingAsked` (test_drill_chart.cpp), on the chart of
/// `generated_tables.kicad_pcb`: a refresh without a drill change keeps the built generation
/// and rebuilds nothing; a via added to the board bumps the generation and the next refresh
/// rebuilds the chart at the new generation.
#[test]
fn a_chart_follows_the_board_without_being_asked() {
    let mut board = parse(&generated_tables());
    let charts = tables_of(&board, KicadT::PcbDrillChartT);
    assert!(!charts.is_empty());

    // A loaded chart was built at no generation, so the first refresh rebuilds it
    for &chart in &charts {
        assert_eq!(board[chart].built_generation(), Some(0));
    }

    let first = kicad_pcb::board_tables::refresh_generated_tables(&mut board);
    assert!(first >= charts.len() as i32);

    let built_at = board[charts[0]].built_generation().unwrap();
    assert_eq!(built_at, board.drill_model_generation());
    let rows_before = board[charts[0]].row_count();

    // Nothing moved, so the refresh is one comparison and the cells stay as they are
    let second = kicad_pcb::board_tables::refresh_generated_tables(&mut board);
    assert_eq!(board[charts[0]].built_generation(), Some(built_at));
    assert_eq!(board[charts[0]].row_count(), rows_before);
    assert_eq!(second, 0);

    let mut via = PcbVia::new(&mut board.kiid_generator);
    via.track.set_start(Vector2I::new(mm(40.0), 0));
    via.track.set_end(Vector2I::new(mm(40.0), 0));
    via.set_drill(mm(0.45));
    via.set_width(PcbLayerId::FCu, mm(0.80));
    assert!(
        board
            .add_track(Track::Via(Box::new(via)), AddMode::Insert)
            .is_ok()
    );
    assert!(board.drill_model_generation() > built_at);

    kicad_pcb::board_tables::refresh_generated_tables(&mut board);

    // A new hole size is a new row, and nobody had to ask for it
    assert_eq!(board[charts[0]].row_count(), rows_before + 1);
    assert_eq!(
        board[charts[0]].built_generation(),
        Some(board.drill_model_generation())
    );
}

/// The layer setters of `BOARD` that bump the generation (`SetCopperLayerCount`,
/// `SetEnabledLayers`, a successful `SetLayerName`), and the ones that do not.
#[test]
fn layer_changes_bump_the_drill_model_generation() {
    let mut board = parse(&generated_tables());
    let g0 = board.drill_model_generation();

    board.set_copper_layer_count(board.copper_layer_count());
    assert_eq!(board.drill_model_generation(), g0 + 1);

    board.set_enabled_layers(board.enabled_layers());
    assert_eq!(board.drill_model_generation(), g0 + 2);

    assert!(board.set_layer_name(PcbLayerId::FCu, "Top"));
    assert_eq!(board.drill_model_generation(), g0 + 3);

    // Refused names and clearing a name do not
    assert!(!board.set_layer_name(PcbLayerId::FCu, "a\"b"));
    assert!(!board.set_layer_name(PcbLayerId::FCu, ""));
    assert_eq!(board.drill_model_generation(), g0 + 3);
}

/// Adding or removing a drill map, pad, via or footprint at board level bumps the generation;
/// other items do not (`affectsDrillModel`).
#[test]
fn only_drill_items_bump_the_drill_model_generation() {
    let mut board = parse(&generated_tables());
    let g0 = board.drill_model_generation();

    let shape = kicad_pcb::pcb_shape::PcbShape::default();
    let id: ItemId = board.add_shape(shape, AddMode::Insert).into();
    assert_eq!(board.drill_model_generation(), g0);
    assert!(board.remove(id));
    assert_eq!(board.drill_model_generation(), g0);

    let via = PcbVia::new(&mut board.kiid_generator);
    let id: ItemId = board
        .add_track(Track::Via(Box::new(via)), AddMode::Insert)
        .ok()
        .unwrap()
        .into();
    assert_eq!(board.drill_model_generation(), g0 + 1);
    assert!(board.remove(id));
    assert_eq!(board.drill_model_generation(), g0 + 2);
}

/// `normalize_save` of `tests/fix_edit1.rs`: UUIDs that are not in the input become
/// "generated".
fn normalize_save(saved: &str, known: &BTreeSet<String>) -> String {
    let b = saved.as_bytes();
    let is_uuid = |t: &[u8]| {
        t.len() == 36
            && t.iter().enumerate().all(|(i, &c)| {
                if matches!(i, 8 | 13 | 18 | 23) {
                    c == b'-'
                } else {
                    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
                }
            })
    };
    let mut out = String::with_capacity(saved.len());
    let mut i = 0;
    let mut last = 0;

    while i + 38 <= b.len() {
        if b[i] == b'"' && b[i + 37] == b'"' && is_uuid(&b[i + 1..i + 37]) {
            if !known.contains(&saved[i + 1..i + 37]) {
                out.push_str(&saved[last..i]);
                out.push_str("\"generated\"");
                last = i + 38;
            }

            i += 38;
        } else {
            i += 1;
        }
    }

    out.push_str(&saved[last..]);
    out
}

/// The quoted 36-character lower-case UUIDs of `text` (the golden script's `known`).
fn quoted_uuids(text: &str) -> BTreeSet<String> {
    let lower = text.to_ascii_lowercase();
    let b = lower.as_bytes();
    let mut out = BTreeSet::new();

    for i in 0..b.len().saturating_sub(37) {
        if b[i] == b'"' && b[i + 37] == b'"' {
            let t = &lower[i + 1..i + 37];
            let ok = t.bytes().enumerate().all(|(k, c)| {
                if matches!(k, 8 | 13 | 18 | 23) {
                    c == b'-'
                } else {
                    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
                }
            });

            if ok {
                out.insert(t.to_string());
            }
        }
    }

    out
}

fn fnv(data: &[u8]) -> String {
    let mut h: u64 = 0xCBF29CE484222325;

    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001B3);
    }

    format!("{h:016x}")
}

/// Flip and Mirror of every top-level item (the driver's walk) of a board holding a drill
/// chart and a stack-up table: the saved board equals the reference's (`Board::table_flip`
/// runs the `PCB_GENERATED_TABLE::Flip` override).
#[test]
fn flip_and_mirror_match_the_reference() {
    let input = generated_tables();
    let known = quoted_uuids(&input);
    let golden =
        std::fs::read_to_string(crate_dir().join("tests/golden/glue3/flip_tables.txt")).unwrap();
    let mut bad = Vec::new();

    for line in golden.lines().filter(|l| !l.is_empty()) {
        let (edit, want) = line.split_once('=').unwrap();
        let mut board = parse(&input);
        let mut items: Vec<ItemId> = board.footprints().map(|(id, _)| id.into()).collect();
        items.extend(board.drawings().map(|(id, _)| id));
        items.extend(board.tracks().map(|(id, _)| ItemId::from(id)));
        items.extend(board.zones().map(|(id, _)| ItemId::from(id)));
        items.extend(board.board_points().map(|(id, _)| ItemId::from(id)));
        items.extend(board.generators().map(|(id, _)| ItemId::from(id)));
        items.extend(board.groups().map(|(id, _)| ItemId::from(id)));

        for id in items {
            match edit {
                "none" => {}
                "flip_lr" => board.flip_item(id, CENTRE, FlipDirection::LeftRight),
                "flip_tb" => board.flip_item(id, CENTRE, FlipDirection::TopBottom),
                "mirror_lr" => board.mirror_item(id, CENTRE, FlipDirection::LeftRight),
                "mirror_tb" => board.mirror_item(id, CENTRE, FlipDirection::TopBottom),
                other => panic!("unknown edit {other}"),
            }
        }

        let saved = normalize_save(
            &format_board(&board, &FormatOptions::default()).unwrap(),
            &known,
        );
        let got = format!("{}:{}", fnv(saved.as_bytes()), saved.len());

        if let Ok(dir) = std::env::var("GLUE3_KEEP") {
            std::fs::write(format!("{dir}/{edit}.rs.kicad_pcb"), &saved).unwrap();
        }

        if got != want {
            bad.push(format!("{edit}: ref {want}, rust {got}"));
        }
    }

    assert!(bad.is_empty(), "{bad:#?}");
}

/// `VCSHASH` / `VCSSHORTHASH` (`PROJECT::TextVarResolver` with
/// `PROJECT_GIT_UTILS::GetCurrentHash`): the host-set hash, the short form its first 8
/// characters; "no hash" without one.
#[test]
fn vcs_hash_tokens_follow_the_host_hash() {
    let clock = kicad_core::title_block::TitleBlockClock::default();
    let mut project = kicad_pcb::board::BoardProject::default();
    let resolve = |project: &kicad_pcb::board::BoardProject, token: &str| {
        let mut t = token.to_string();
        assert!(project.text_var_resolver(&clock, &mut t));
        t
    };

    assert_eq!(resolve(&project, "VCSHASH"), "no hash");
    assert_eq!(resolve(&project, "VCSSHORTHASH"), "no hash");

    project.vcs_hash = Some("0123456789abcdef0123456789abcdef01234567".into());
    assert_eq!(
        resolve(&project, "VCSHASH"),
        "0123456789abcdef0123456789abcdef01234567"
    );
    assert_eq!(resolve(&project, "VCSSHORTHASH"), "01234567");
}

/// The `DRILL_SPAN` tests of FAB-01's `kicad-fab/src/drill/drill_span.rs`, kept when GLUE-3
/// replaced that copy by `kicad_pcb::pcb_drill_map::DrillSpan`.
mod drill_span {
    use std::collections::BTreeSet;

    use kicad_core::PcbLayerId;
    use kicad_pcb::pcb_drill_map::DrillSpan;

    const IN1: PcbLayerId = PcbLayerId::In1Cu;
    const IN2: PcbLayerId = PcbLayerId::In2Cu;
    const F: PcbLayerId = PcbLayerId::FCu;
    const B: PcbLayerId = PcbLayerId::BCu;

    #[test]
    fn top_bottom_follow_the_physical_stack() {
        let s = DrillSpan::new(B, IN1, true, false);
        assert_eq!(s.top_layer(), IN1);
        assert_eq!(s.bottom_layer(), B);
        assert_eq!(s.pair(), (IN1, B));
        assert_eq!(s.drill_start_layer(), B);
        let s = DrillSpan::new(F, IN2, false, false);
        assert_eq!(s.pair(), (F, IN2));
        assert_eq!(DrillSpan::default().pair(), (F, B));
    }

    #[test]
    fn reversed_span_is_distinct_but_same_pair() {
        let a = DrillSpan::new(F, IN2, true, false);
        let b = DrillSpan::new(IN2, F, true, false);
        assert_ne!(a, b);
        assert_eq!(a.pair(), b.pair());
        // Same top and bottom: start layer decides (F_Cu = 0 < In2_Cu = 6).
        assert!(a < b);
    }

    #[test]
    fn set_order_matches_operator_less() {
        let mut set = BTreeSet::new();
        set.insert(DrillSpan::new(IN1, IN2, false, false));
        set.insert(DrillSpan::new(F, IN1, false, false));
        set.insert(DrillSpan::new(IN2, B, false, false));
        set.insert(DrillSpan::new(F, IN1, true, false));
        set.insert(DrillSpan::new(B, IN2, true, false));
        set.insert(DrillSpan::new(F, IN1, false, false)); // duplicate
        let v: Vec<_> = set.into_iter().collect();
        assert_eq!(
            v,
            vec![
                // top F_Cu (0): bottom In1 (4); backdrill first
                DrillSpan::new(F, IN1, true, false),
                DrillSpan::new(F, IN1, false, false),
                // top In1 (4), bottom In2 (6)
                DrillSpan::new(IN1, IN2, false, false),
                // top In2 (6), bottom B_Cu (2); backdrill first
                DrillSpan::new(B, IN2, true, false),
                DrillSpan::new(IN2, B, false, false),
            ]
        );
    }
}
