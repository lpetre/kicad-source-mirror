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

//! Package DRC-34: the net chain tuning profile and via-stitch overlap providers and
//! `DRC_ITEM::GetViolationLayers`.
//!
//! Ported C++ tests:
//! * `qa/tests/pcbnew/drc/test_drc_net_chain_tuning_profiles.cpp` (`DRCNetChainTuningProfiles`)
//! * `qa/tests/pcbnew/drc/test_drc_violation_layers_issue24941.cpp`
//!   (`EdgeClearanceFocusesEdgeCuts`, `MarkerLayerTakesPriority`, `FallbackToItemLayers`)
//!
//! Reference comparisons: the corpus scoreboard of the two error codes (golden of the shared
//! harness) and the report order of the synthetic chain boards (`tests/data/drc34`, golden
//! `tests/golden/drc34/chains.txt`, `tests/oracle/gen_drc34_boards.py`).

mod drc_harness;

use std::path::PathBuf;

use drc_harness::{cli_report_entries_flow, compare_corpus_flow, load_board};
use kicad_core::{KiidGenerator, Lset, PcbLayerId};
use kicad_drc::item;
use kicad_drc::violation_layers::violation_layers;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{DRCE_CLEARANCE, DRCE_CONNECTION_WIDTH, DRCE_EDGE_CLEARANCE};
use kicad_pcb::io::{ParseOptions, parse_board_report};
use kicad_pcb::pcb_marker::PcbMarker;
use kicad_pcb::{Board, BoardCtx};

/// The error types of the two providers.
const TYPES: [&str; 2] = ["net_chain_tuning_profiles", "via_stitch_overlap"];

/// Their registry slots (`GetName()`).
const PROVIDERS: &[&str] = &["net chain tuning profiles", "via_stitch_overlap"];

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Port of `DRCNetChainTuningProfiles`: one `net_chain_tuning_profiles` error on the QA
/// board (the C++ test counts every error-severity violation of a full run with the library,
/// dangling via and outline checks ignored; here only the provider under test runs, and the
/// reference report of the corpus confirms that it is the board's only error of its type).
#[test]
fn drc_net_chain_tuning_profiles() {
    let path = kicad_qa::pcbnew_data_dir()
        .join("drc_net_chain_tuning_profiles/test_drc_net_chain_tuning_profiles.kicad_pcb");
    let mut loaded = load_board(&path).expect("load");
    let entries = cli_report_entries_flow(&mut loaded, true, PROVIDERS);
    let found: Vec<_> = entries
        .iter()
        .filter(|e| e.type_ == "net_chain_tuning_profiles" && e.severity == "error")
        .collect();

    assert_eq!(found.len(), 1, "{entries:#?}");
    assert_eq!(
        found[0].description,
        "Net chain CHAIN_2 nets have multiple tuning profiles assigned"
    );
}

/// The synthetic chain boards: every chain reported, in the reference order (the libstdc++
/// `std::unordered_map` iteration order of the C++).
#[test]
fn net_chain_report_order_matches_reference() {
    let golden = std::fs::read_to_string(manifest().join("tests/golden/drc34/chains.txt")).unwrap();
    let mut cases: Vec<(String, Vec<String>)> = Vec::new();

    for line in golden.lines().filter(|l| !l.is_empty()) {
        let (case, desc) = line.split_once('\t').unwrap();

        match cases.last_mut() {
            Some((c, v)) if c == case => v.push(desc.to_string()),
            _ => cases.push((case.to_string(), vec![desc.to_string()])),
        }
    }

    assert_eq!(cases.len(), 3);

    for (case, reference) in cases {
        let path = manifest().join(format!("tests/data/drc34/{case}.kicad_pcb"));
        let mut loaded = load_board(&path).expect("load");
        let mine: Vec<String> = cli_report_entries_flow(&mut loaded, false, PROVIDERS)
            .into_iter()
            .filter(|e| e.type_ == "net_chain_tuning_profiles")
            .map(|e| e.description)
            .collect();

        assert_eq!(mine, reference, "{case}");
    }
}

/// The synthetic via-stitch board: three overlapping same-net pairs out of five stitches, in
/// the reference order, with the reference items and item positions.
#[test]
fn via_stitch_overlap_matches_reference() {
    let golden = std::fs::read_to_string(manifest().join("tests/golden/drc34/stitch.txt")).unwrap();
    let path = manifest().join("tests/data/drc34/stitch_overlap.kicad_pcb");
    let mut loaded = load_board(&path).expect("load");
    let mine: Vec<_> = cli_report_entries_flow(&mut loaded, false, PROVIDERS)
        .into_iter()
        .filter(|e| e.type_ == "via_stitch_overlap")
        .collect();
    let reference: Vec<&str> = golden.lines().filter(|l| !l.is_empty()).collect();

    assert_eq!(mine.len(), reference.len(), "{mine:#?}");

    for (e, line) in mine.iter().zip(&reference) {
        let parts: Vec<&str> = line.split('\t').collect();
        assert_eq!(e.description, parts[0]);
        let uuids: Vec<&str> = e.items.iter().map(|i| i.0.as_str()).collect();
        assert_eq!(uuids.join(","), parts[1]);

        for (item, pos) in e.items.iter().zip(parts[2].split(';')) {
            let (x, y) = pos.split_once(',').unwrap();
            assert_eq!((item.2, item.3), (x.parse().unwrap(), y.parse().unwrap()));
        }
    }
}

/// Every corpus board the port loads completely, the two error codes only, against the
/// reference reports. `DRC34_FILTER=<path substring>` limits the boards.
#[test]
fn net_chain_and_via_stitch_match_reference_corpus() {
    let filter = std::env::var("DRC34_FILTER").unwrap_or_default();
    let score = compare_corpus_flow(&TYPES, &filter, PROVIDERS);
    score.print("DRC-34 net chain tuning profiles / via-stitch overlap");
    score.assert_no_mismatch();
}

// ---- test_drc_violation_layers_issue24941.cpp ----------------------------------------------

const BOARD_HEAD: &str = r#"(kicad_pcb (version 20260206) (generator "pcbnew")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user)
    (31 "F.CrtYd" user "F.Courtyard") (29 "B.CrtYd" user "B.Courtyard"))
  (net 0 "")
"#;

fn board(items: &str) -> Board {
    let text = format!("{BOARD_HEAD}{items})\n");
    let options = ParseOptions {
        allow_pending: true,
    };
    parse_board_report(&text, "issue24941.kicad_pcb", &options)
        .expect("parse")
        .0
}

fn uuid(s: &str) -> kicad_core::Kiid {
    kicad_core::Kiid::parse(s).expect("uuid")
}

const EDGE: &str = "00000000-0000-0000-0034-000000000001";
const TRACK1: &str = "00000000-0000-0000-0034-000000000002";
const TRACK2: &str = "00000000-0000-0000-0034-000000000003";
const ZONE: &str = "00000000-0000-0000-0034-000000000004";

/// Issue 24941: clicking a board edge clearance violation must focus Edge.Cuts, not the
/// copper layer the marker sits on.
#[test]
fn edge_clearance_focuses_edge_cuts() {
    let board = board(&format!(
        "(gr_line (start 0 0) (end 1 0) (layer \"Edge.Cuts\") (uuid \"{EDGE}\"))
         (segment (start 0 0.1) (end 1 0.1) (width 0.2) (layer \"F.Cu\") (net 0) (uuid \"{TRACK1}\"))"
    ));
    let ctx = BoardCtx::bare(&board);

    let mut drc_item = item::create(DRCE_EDGE_CLEARANCE).unwrap();
    drc_item.set_items(
        uuid(EDGE),
        uuid(TRACK1),
        kicad_core::NILUUID,
        kicad_core::NILUUID,
    );
    let drc_item = std::sync::Arc::new(drc_item);

    // The edge clearance provider stamps the marker with the copper item's layer
    let mut generator = KiidGenerator::seeded(34);
    let marker = PcbMarker::new(
        &mut generator,
        drc_item.clone(),
        Vector2I::new(0, 100_000),
        PcbLayerId::FCu.0,
    );

    let r = violation_layers(&ctx, &drc_item, Some(&marker));
    assert_eq!(r.principal_layer, PcbLayerId::EdgeCuts);
}

/// Guard for the original fix: for a violation on a multi-layer item the marker layer must win
/// over the first layer of the item's layer set.
#[test]
fn marker_layer_takes_priority() {
    let board = board(&format!(
        "(zone (net 0) (net_name \"\") (layers \"F.Cu\" \"B.Cu\") (uuid \"{ZONE}\")
           (hatch edge 0.5) (connect_pads (clearance 0)) (min_thickness 0.25)
           (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
           (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1))))"
    ));
    let ctx = BoardCtx::bare(&board);

    let mut drc_item = item::create(DRCE_CONNECTION_WIDTH).unwrap();
    drc_item.set_item_ids(vec![uuid(ZONE)]);
    let drc_item = std::sync::Arc::new(drc_item);

    // The narrow neck is on B.Cu, the provider marks it there
    let mut generator = KiidGenerator::seeded(34);
    let marker = PcbMarker::new(
        &mut generator,
        drc_item.clone(),
        Vector2I::new(0, 0),
        PcbLayerId::BCu.0,
    );

    let r = violation_layers(&ctx, &drc_item, Some(&marker));
    assert_eq!(r.principal_layer, PcbLayerId::BCu);
}

/// Without a marker the layer comes from intersecting the items' layer sets.
#[test]
fn fallback_to_item_layers() {
    let board = board(&format!(
        "(segment (start 0 0) (end 1 0) (width 0.2) (layer \"B.Cu\") (net 0) (uuid \"{TRACK1}\"))
         (segment (start 0 0.1) (end 1 0.1) (width 0.2) (layer \"B.Cu\") (net 0) (uuid \"{TRACK2}\"))"
    ));
    let ctx = BoardCtx::bare(&board);

    let mut drc_item = item::create(DRCE_CLEARANCE).unwrap();
    drc_item.set_item_ids(vec![uuid(TRACK1), uuid(TRACK2)]);

    let r = violation_layers(&ctx, &drc_item, None);
    assert_eq!(r.principal_layer, PcbLayerId::BCu);
    assert!(r.violation_layers.contains(PcbLayerId::BCu));

    let mut only_b = Lset::new();
    only_b.set(PcbLayerId::BCu);
    assert_eq!(r.violation_layers, only_b);
}
