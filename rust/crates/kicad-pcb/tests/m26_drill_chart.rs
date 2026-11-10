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

//! Port of `qa/tests/pcbnew/test_drill_chart.cpp` (package M26), the cases of the drill chart
//! model, its rebuild and the drill map that apply to the board model.
//!
//! The C++ builds its boards item by item (`buildHoles`, `buildOutline`); here they are the
//! same items written as a board file and parsed (6 copper layers from
//! `qa/data/pcbnew/via_stacks.kicad_pcb`'s header). A chart is part of that board file (no
//! cells) instead of a free-standing `PCB_DRILL_CHART`, and a hole added or removed between two
//! rebuilds is added to the written board and re-parsed (the cells, their formatting and the
//! row keys travel through the file, which the C++ `RowKeysSurviveABoardFileRoundTrip` /
//! `HandFormattedCellsSurviveABoardFileRoundTrip` cover) or removed with `Board::remove`.
//!
//! Not ported: the `GetBuiltGeneration` checks of `AChartFollowsTheBoardWithoutBeingAsked` (the
//! port keeps no drill model generation, see PORT_STATUS.md M26), the view / layer / property
//! / protobuf / clone cases (`HolesReportTheDrillSymbolLayerOfAMap`,
//! `MapOwnsItsSymbolSizeAndOnlyExposesOffsets`, `MapSymbolSizeSurvivesCloneSwapAndProtobuf`,
//! `HoleViewBoundsIncludeDisplacedMapSymbols`, `ChartCellTextIsNotUserEditableButItsFormattingIs`,
//! `OnlyHolesCarryTheDrillSymbolMargin`, `MovingAMapUpdatesTheHoleViewBounds`,
//! `ChartIsVisitedOnceWhenBothTypesAreScanned`, `CloneAndSwapPreserveChartSettings`), and the
//! file round trips that belong to the I/O packages (`MapSymbolSizeSurvivesBoardFileRoundTrip`
//! is in `pcbio12_misc_items.rs`).

use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::{Box2I, Shape, Vector2I};
use kicad_pcb::board::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drill::{
    DrillChartModel, DrillSymbolProfile, enumerate_drill_spans, resolve_drill_symbols,
};
use kicad_pcb::ids::{DrillMapId, ItemId, TableId};
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::pcb_drill_map::PcbDrillMap;
use kicad_pcb::pcb_table::PcbTable;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::traits::EdaTextItem;

fn mm(v: f64) -> i32 {
    kicad_core::PCB_IU_SCALE.mm_to_iu(v)
}

/// `buildHoles`: two through vias of 0.30 mm, one of 0.60 mm and an NPTH mounting hole.
const HOLES: &str = r#"	(via (at 0 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "") (uuid "00000000-0000-4000-8000-000000000101"))
	(via (at 5 0) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net "") (uuid "00000000-0000-4000-8000-000000000102"))
	(via (at 20 0) (size 1) (drill 0.6) (layers "F.Cu" "B.Cu") (net "") (uuid "00000000-0000-4000-8000-000000000103"))
	(footprint "MountingHole"
		(layer "F.Cu")
		(uuid "00000000-0000-4000-8000-000000000104")
		(at 30 0)
		(pad "" np_thru_hole circle
			(at 0 0)
			(size 3.2 3.2)
			(drill 3.2)
			(layers "*.Cu" "*.Mask")
			(uuid "00000000-0000-4000-8000-000000000105")
		)
	)
"#;

/// The 0.15 mm via the C++ adds between two rebuilds.
const TINY: &str = r#"	(via (at 40 0) (size 0.4) (drill 0.15) (layers "F.Cu" "B.Cu") (net "") (uuid "00000000-0000-4000-8000-000000000106"))
"#;

/// A drill chart without cells (`new PCB_DRILL_CHART( &board )`), on `layer`.
fn chart(layer: &str) -> String {
    format!(
        "\t(drill_chart (uuid \"00000000-0000-4000-8000-000000000201\") (layer \"{layer}\") (column_count 6))\n"
    )
}

/// `buildOutline( board, box )`: four Edge.Cuts segments.
fn outline(b: &Box2I) -> String {
    let c = [
        (b.left(), b.top()),
        (b.right(), b.top()),
        (b.right(), b.bottom()),
        (b.left(), b.bottom()),
    ];
    let mut s = String::new();

    for ii in 0..4 {
        let (a, e) = (c[ii], c[(ii + 1) % 4]);
        s += &format!(
            "\t(gr_line (start {} {}) (end {} {}) (stroke (width 0.05) (type default)) (layer \"Edge.Cuts\") (uuid \"00000000-0000-4000-8000-00000000030{ii}\"))\n",
            a.0 as f64 / 1e6,
            a.1 as f64 / 1e6,
            e.0 as f64 / 1e6,
            e.1 as f64 / 1e6
        );
    }

    s
}

/// A 6-copper-layer board holding `items`.
fn board(items: &str) -> Board {
    let base = std::fs::read_to_string(kicad_qa::pcbnew_data_dir().join("via_stacks.kicad_pcb"))
        .unwrap();
    let header = &base[..base.find("\n\t(via").unwrap() + 1];
    parse(&format!("{header}{items})\n"))
}

fn parse(text: &str) -> Board {
    parse_board_report(text, "m26.kicad_pcb", &ParseOptions::default())
        .unwrap_or_else(|e| panic!("{}", e.what()))
        .0
}

/// The board written and parsed again with `extra` items appended.
fn reparse_with(board: &Board, extra: &str) -> Board {
    let text = format_board(board, &FormatOptions::default()).unwrap();
    let text = text.trim_end();
    parse(&format!("{}{extra})\n", &text[..text.len() - 1]))
}

fn the_chart(board: &Board) -> TableId {
    board
        .drawings()
        .find_map(|(id, item)| (item.as_dyn().type_() == KicadT::PcbDrillChartT).then_some(id))
        .and_then(|id| id.typed::<PcbTable>())
        .expect("a drill chart")
}

fn the_map(board: &Board) -> DrillMapId {
    board
        .drawings()
        .find_map(|(id, item)| (item.as_dyn().type_() == KicadT::PcbDrillMapT).then_some(id))
        .and_then(|id| id.typed::<PcbDrillMap>())
        .expect("a drill map")
}

/// `PCB_DRILL_CHART::RebuildCells( board )` (a preview: no refresh).
fn rebuild(board: &mut Board, chart: TableId) {
    board.generated_rebuild_cells(chart, None);
}

/// `cellCount( chart, text )`.
fn cell_count(board: &Board, chart: TableId, text: &str) -> usize {
    board
        .table_cells(chart)
        .filter(|(_, c)| c.textbox.text.text() == text)
        .count()
}

/// `rowShowingDiameter( chart, text )`: the row reporting a drill group, or -1.
fn row_showing(board: &Board, chart: TableId, text: &str) -> i32 {
    let ctx = BoardCtx::bare(board);

    board
        .table_cells(chart)
        .find(|(_, c)| c.textbox.text.text() == text)
        .map_or(-1, |(_, c)| c.row(&ctx))
}

fn is_bold(board: &Board, chart: TableId, row: i32, col: i32) -> bool {
    board[board[chart].cell(row, col).unwrap()]
        .textbox
        .text
        .is_bold()
}

fn set_row_bold(board: &mut Board, chart: TableId, row: i32, size: Option<i32>) {
    for col in 0..board[chart].col_count() {
        let cell = board[chart].cell(row, col).unwrap();
        board.modify_tablecell(cell, |c, env| {
            c.textbox.text.set_bold(true);

            if let Some(size) = size {
                c.set_text_size(env, Vector2I::new(size, size), true);
            }
        });
    }
}

#[test]
fn groups_holes_by_size_and_plating() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    // Heading, three groups (0.3 PTH, 0.6 PTH, 3.2 NPTH) and totals. No caption row
    assert_eq!(board[chart].row_count(), 5);

    // Two vias share one size, so they collapse into a single row reporting two operations
    assert_eq!(cell_count(&board, chart, "2"), 1);

    assert_eq!(cell_count(&board, chart, "Yes"), 2);
    assert_eq!(cell_count(&board, chart, "No"), 1);
}

#[test]
fn rebuild_keeps_cell_identity() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let uuids = |board: &Board| -> Vec<_> {
        board
            .table_cells(chart)
            .map(|(_, c)| c.textbox.shape.base.item.uuid)
            .collect()
    };
    let before = uuids(&board);

    rebuild(&mut board, chart);

    // Clearing and repopulating would mint new UUIDs every rebuild and break selection restore
    // and file diffs
    assert_eq!(before, uuids(&board));
}

#[test]
fn a_chart_follows_the_board_without_being_asked() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);
    let rows_before = board[chart].row_count();

    // Nothing moved: the cells stay as they are (the port rebuilds, see the module comment)
    kicad_pcb::board_tables::refresh_generated_tables(&mut board);
    assert_eq!(board[chart].row_count(), rows_before);

    let mut board = reparse_with(&board, TINY);
    let chart = the_chart(&board);
    kicad_pcb::board_tables::refresh_generated_tables(&mut board);

    // A new hole size is a new row, and nobody had to ask for it
    assert_eq!(board[chart].row_count(), rows_before + 1);
}

#[test]
fn map_is_hit_at_its_marks_and_nowhere_else() {
    let mut board = board(&format!(
        "{HOLES}\t(drill_map (uuid \"00000000-0000-4000-8000-000000000202\") (layer \"User.1\"))\n"
    ));
    let map = the_map(&board);

    let hole = Vector2I::new(0, 0);
    let empty = Vector2I::new(mm(12.0), mm(12.0));

    {
        let ctx = BoardCtx::bare(&board);
        assert!(board[map].hit_test(&ctx, hole, 0));
        assert!(!board[map].hit_test(&ctx, empty, 0));
    }

    // The marks travel with the offset. The holes they mark stay where they are
    let offset = Vector2I::new(mm(50.0), mm(40.0));
    board.modify_drill_map(map, |m, _| m.set_offset(offset));

    let ctx = BoardCtx::bare(&board);
    assert!(!board[map].hit_test(&ctx, hole, 0));
    assert!(board[map].hit_test(&ctx, hole + offset, 0));
}

#[test]
fn map_is_hit_on_its_outline_when_the_board_has_no_holes() {
    let outline_box = Box2I::new(
        Vector2I::new(mm(10.0), mm(20.0)),
        Vector2I::new(mm(30.0), mm(40.0)),
    );
    let mut board = board(&format!(
        "{}\t(drill_map (uuid \"00000000-0000-4000-8000-000000000202\") (layer \"User.1\"))\n",
        outline(&outline_box)
    ));
    let map = the_map(&board);
    let offset = Vector2I::new(mm(100.0), mm(80.0));
    board.modify_drill_map(map, |m, _| m.set_offset(offset));

    let ctx = BoardCtx::bare(&board);
    let m = &board[map];

    // The outline is drawn and plotted, so it has to be selectable. Without holes there are no
    // marks to hit instead
    assert!(m.hit_test(&ctx, outline_box.origin() + offset, 0));
    assert!(m.hit_test(
        &ctx,
        Vector2I::new(outline_box.left(), outline_box.center().y) + offset,
        0
    ));

    // Inside the outline is not on it, and neither is the undisplaced board
    assert!(!m.hit_test(&ctx, outline_box.center() + offset, 0));
    assert!(!m.hit_test(&ctx, outline_box.origin(), 0));
}

#[test]
fn map_board_outline_follows_its_offset() {
    let outline_box = Box2I::new(
        Vector2I::new(mm(10.0), mm(20.0)),
        Vector2I::new(mm(30.0), mm(40.0)),
    );
    let mut board = board(&format!(
        "{HOLES}{}\t(drill_map (uuid \"00000000-0000-4000-8000-000000000202\") (layer \"User.1\"))\n",
        outline(&outline_box)
    ));
    let map = the_map(&board);
    let offset = Vector2I::new(mm(100.0), mm(80.0));
    board.modify_drill_map(map, |m, _| m.set_offset(offset));

    let ctx = BoardCtx::bare(&board);
    let outlines = board[map].board_outlines(&ctx);

    assert_eq!(outlines.outline_count(), 1);
    assert_eq!(
        outlines.bbox(0),
        Box2I::new(outline_box.origin() + offset, outline_box.size())
    );
}

#[test]
fn span_choice_round_trips_through_the_board_spans() {
    let mut board = board(&format!(
        "{HOLES}\t(drill_map (uuid \"00000000-0000-4000-8000-000000000202\") (layer \"User.1\"))\n"
    ));
    let map = the_map(&board);

    let spans = {
        let ctx = BoardCtx::bare(&board);
        assert_eq!(board[map].span_choice(&ctx), -1);
        enumerate_drill_spans(&ctx, false)
    };

    assert!(!spans.is_empty());

    let set = |board: &mut Board, index: i32| {
        let mut m = board[map].clone();
        m.set_span_choice(&BoardCtx::bare(board), index);
        board.modify_drill_map(map, |x, _| *x = m);
    };

    set(&mut board, 0);

    {
        let ctx = BoardCtx::bare(&board);
        assert!(!board[map].all_spans());
        assert_eq!(*board[map].span(), spans[0]);
        assert_eq!(board[map].span_choice(&ctx), 0);
    }

    // A choice the board cannot honour means every span, not a map with nothing on it
    set(&mut board, spans.len() as i32);

    let ctx = BoardCtx::bare(&board);
    assert!(board[map].all_spans());
    assert_eq!(board[map].span_choice(&ctx), -1);
}

#[test]
fn cell_formatting_survives_a_rebuild() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    // The last data row, past the heading
    let row = board[chart].row_count() - 2;
    assert!(row > 1);

    set_row_bold(&mut board, chart, row, Some(mm(2.5)));
    rebuild(&mut board, chart);

    // Cells are reused rather than recreated, which is what lets the ordinary table formatting
    // be the chart's formatting
    assert!(is_bold(&board, chart, row, 0));
    let cell = &board[board[chart].cell(row, 0).unwrap()];
    assert_eq!(
        EdaTextItem::text_size(cell, &BoardCtx::bare(&board)).y,
        mm(2.5)
    );
}

#[test]
fn cell_formatting_follows_its_group_when_a_group_is_added() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    // The 3.2 mm mounting hole, which is the largest and so the last data row
    let before = row_showing(&board, chart, "3.200 mm");
    assert!(before > 1);
    assert!(board[chart].is_data_row(before));

    set_row_bold(&mut board, chart, before, None);

    // A smaller hole than any already present, so it takes the first data row and pushes every
    // existing group down one
    let mut board = reparse_with(&board, TINY);
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let after = row_showing(&board, chart, "3.200 mm");
    assert!(after > 1);
    assert_ne!(after, before);

    // The formatting belongs to the group, not to the row number it happened to occupy
    assert!(is_bold(&board, chart, after, 0));
    assert!(!is_bold(&board, chart, before, 0));
}

#[test]
fn cell_formatting_follows_its_group_when_a_group_is_removed() {
    let mut board = board(&format!("{HOLES}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let before = row_showing(&board, chart, "3.200 mm");
    assert!(before > 1);

    set_row_bold(&mut board, chart, before, None);

    // Drop the smallest group entirely, so the rows below it move up. Both 0.30 vias have to
    // go. One of them left behind keeps the group and the table the same shape.
    let doomed: Vec<ItemId> = {
        let ctx = BoardCtx::bare(&board);
        board
            .tracks()
            .filter(|(_, t)| matches!(t, Track::Via(v) if v.drill_value(&ctx) == mm(0.30)))
            .map(|(id, _)| id.into())
            .collect()
    };
    assert_eq!(doomed.len(), 2);
    board.remove_many(&doomed);

    let rows_before = board[chart].row_count();
    rebuild(&mut board, chart);

    // The group really did go, or the rest of this proves nothing
    assert_eq!(board[chart].row_count(), rows_before - 1);

    let after = row_showing(&board, chart, "3.200 mm");
    assert!(after > 1);
    assert_ne!(after, before);
    assert!(is_bold(&board, chart, after, 0));
}

#[test]
fn rows_keep_separate_cells_when_a_rebuild_reorders_them() {
    // A buried via drilled like the through ones. The profile groups by span, so it reports
    // its own row rather than joining theirs
    let buried = r#"	(via buried (at 10 0) (size 0.6) (drill 0.3) (layers "In1.Cu" "In2.Cu") (net "") (uuid "00000000-0000-4000-8000-000000000107"))
"#;
    let mut board = board(&format!("{HOLES}{buried}{}", chart("User.1")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let keys = board[chart].generated.as_ref().unwrap().row_keys.clone();
    let distinct: std::collections::BTreeSet<_> = keys.values().collect();

    // The through 0.30s, the buried 0.30, the 0.60 via and the 3.2 mm mounting hole
    assert_eq!(keys.len(), 4);

    // Identity has to tell apart rows the chart is showing separately
    assert_eq!(distinct.len(), keys.len());

    // A rebuild that reorders the rows is what makes the migration run
    let mut board = reparse_with(&board, TINY);
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    // Two rows holding one cell would be freed twice when the table goes
    let cells = &board[chart].cells;
    let seen: std::collections::BTreeSet<_> = cells.iter().collect();
    assert_eq!(seen.len(), cells.len());
}

#[test]
fn fresh_chart_is_built_at_the_origin() {
    let mut board = board(&format!("{HOLES}{}", chart("Cmts.User")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let ctx = BoardCtx::bare(&board);
    let bbox = board[chart].bounding_box(&ctx);

    // A new cell carries a half-INT_MAX rectangle and Normalize() anchors on cell 0's centre,
    // so without correction the chart lands half a metre off the board
    assert!(
        bbox.left().abs() < mm(1.0) && bbox.top().abs() < mm(1.0),
        "chart origin is ({},{}), expected near 0",
        bbox.left(),
        bbox.top()
    );

    // Cells keep their wide default rectangle so the text does not wrap. A chart this size
    // must be wider than it is tall
    assert!(bbox.width() > bbox.height());
}

#[test]
fn rebuild_keeps_chart_position() {
    let mut board = board(&format!("{HOLES}{}", chart("Cmts.User")));
    let chart = the_chart(&board);
    rebuild(&mut board, chart);

    let placed = Vector2I::new(mm(60.0), mm(40.0));
    board.move_item(chart.into(), placed);

    let position = |board: &Board| board[chart].position(&BoardCtx::bare(board));
    let before = position(&board);
    rebuild(&mut board, chart);

    // A rebuild must not walk the chart across the board
    assert_eq!(position(&board), before);
}

#[test]
fn every_symbol_resolves_without_a_chart() {
    let board = board(HOLES);
    let ctx = BoardCtx::bare(&board);

    // A drill map must show marks on a board where no chart has ever been placed, so the
    // resolver has to cover every group rather than reading assignments the profile lacks
    let profile = DrillSymbolProfile::from_board(board.design_settings());
    assert!(profile.assignments().is_empty());

    let resolved = resolve_drill_symbols(&ctx);

    let mut model = DrillChartModel::new(&profile);
    model.build(&ctx, &enumerate_drill_spans(&ctx, false));

    assert!(!model.groups().is_empty());
    assert_eq!(resolved.len(), model.groups().len());

    for group in model.groups() {
        assert!(resolved.contains_key(&group.key), "no symbol for {}", group.key);
    }

    // Resolving must not have written anything back to the board
    assert!(board.design_settings().drill_symbol_profile.is_none());
}

/// The layer of a new cell follows the chart's, and a back-layer chart mirrors its new cells
/// (`PCB_TABLECELL::PCB_TABLECELL`, `PCB_TABLE::AddCell`).
#[test]
fn new_cells_take_the_chart_layer_and_side() {
    for (layer, id, mirrored) in [("B.Fab", PcbLayerId::BFab, true), ("User.1", PcbLayerId::User1, false)] {
        let mut board = board(&format!("{HOLES}{}", chart(layer)));
        let chart = the_chart(&board);
        rebuild(&mut board, chart);

        for (_, cell) in board.table_cells(chart) {
            assert_eq!(cell.textbox.shape.base.item.layer, id);
            assert_eq!(cell.textbox.text.is_mirrored(), mirrored);
        }
    }
}
