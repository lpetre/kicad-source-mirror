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

//! Differential replay of the FAB-01 C++ oracle (`tests/oracle/drill_oracle.cpp`, golden
//! `tests/golden/drill_oracle.txt`, regenerate with `tests/oracle/build_oracle.sh`).
//!
//! For every board / drill span / (merge, npth) case the oracle printed the unsorted
//! `EnumerateDrillOperations` result (`raw`), the hole list after `buildHolesList` with tool
//! references (`sorted`), the tool list (`tool`), `BuildFileFunctionAttributeString` for every
//! file type (`ffs`), `getDrillFileName` (`name`) and per span `getProtectionFileName`
//! (`prot`). This test feeds the raw list through `GendrillWriterBase::build_tool_list` and
//! compares everything else exactly (sorting with libstdc++ tie order included).
//!
//! The enumeration itself (board model -> raw list) is compared by the ignored test
//! `enumeration_matches_oracle`, which needs the board parser and the pad / via model.
//!
//! Line formats (fields separated by `|`):
//! * `board|<name>|<copper layer count>|<file name>`, `end|<name>`
//! * `span|<index>|<start layer>|<end layer>|<backdrill>|<npth>`
//! * `prot|<feature>|<ext>|<name>`
//! * `case|<span index>|<merge>|<npth>`
//! * `raw|<op>` and `sorted|<tool ref>|<op>` with `<op>` = kind, source (`V<n>` = index in
//!   `Tracks()`, `P<n>` = running pad index over `Footprints()` x `Pads()`, `-` = none),
//!   x, y, size x, size y, orientation (degrees, `%.17g`), diameter, slot, top, bottom,
//!   drill start, drill end, not plated, attribute, filled, capped, top / bottom covered,
//!   top / bottom plugged, top / bottom tented, front post-machining mode, size, depth,
//!   angle, the same for the back, stub length (`-` = none)
//! * `tool|<diameter>|<total>|<oval>|<not plated>|<attribute>|<backdrill>|<post-machining>|<min stub>|<max stub>`
//! * `ffs|<type>|<compat>|<string>`, `name|<ext>|<name>`

use kicad_core::Kiid;
use kicad_core::layer_ids::PcbLayerId;
use kicad_fab::drill::{DrillOpKind, DrillOperation, DrillPostMachining, DrillSpan, HoleAttribute};
use kicad_fab::exporters::gendrill_writer_base::{
    DrillTool, GendrillWriterBase, Ipc4761Features, TypeFile,
};
use kicad_geom::{EdaAngle, Vector2I};
use kicad_pcb::padstack::PadDrillPostMachiningMode;
use kicad_pcb::{Board, BoardCtx};

const GOLDEN: &str = include_str!("golden/drill_oracle.txt");

fn hole_attribute(v: i32) -> HoleAttribute {
    use HoleAttribute::*;
    [
        HoleUnknown,
        HoleViaThrough,
        HoleViaBuried,
        HoleViaBackdrill,
        HolePad,
        HolePadCastellated,
        HolePadPressfit,
        HoleMechanical,
        HolePadBackdrill,
    ][v as usize]
}

fn pm_mode(v: i32) -> PadDrillPostMachiningMode {
    use PadDrillPostMachiningMode::*;
    [Unknown, NotPostMachined, Counterbore, Countersink][v as usize]
}

fn feature(v: i32) -> Ipc4761Features {
    use Ipc4761Features::*;
    [
        Filled,
        Capped,
        PluggedFront,
        PluggedBack,
        CoveredFront,
        CoveredBack,
        TentedFront,
        TentedBack,
    ][v as usize]
}

fn type_file(v: i32) -> TypeFile {
    [TypeFile::PthFile, TypeFile::NpthFile, TypeFile::MixedFile][v as usize]
}

fn opt(s: &str) -> Option<i32> {
    if s == "-" {
        None
    } else {
        Some(s.parse().unwrap())
    }
}

/// The source ordinal is carried in the source UUID (the port compares holes by value; the
/// ordinal is what identifies the item in the oracle output).
fn source_kiid(src: &str) -> Kiid {
    let mut bytes = [0u8; 16];
    if src != "-" {
        bytes[0] = src.as_bytes()[0];
        let n: u32 = src[1..].parse().unwrap();
        bytes[1..5].copy_from_slice(&n.to_le_bytes());
    }
    Kiid::from_bytes(bytes)
}

/// Parses the `<op>` fields of a `raw` / `sorted` line.
fn parse_op(f: &[&str]) -> DrillOperation {
    let i = |k: usize| -> i32 { f[k].parse().unwrap() };
    let b = |k: usize| -> bool { i(k) != 0 };
    let l = |k: usize| -> PcbLayerId { PcbLayerId(i(k)) };
    assert_eq!(f.len(), 32, "op field count: {f:?}");
    DrillOperation {
        kind: [
            DrillOpKind::PrimaryDrill,
            DrillOpKind::SecondaryDrill,
            DrillOpKind::TertiaryDrill,
        ][i(0) as usize],
        // The handle is not reproducible without a board; holes are identified by `source_id`.
        source_item: None,
        source_id: source_kiid(f[1]),
        position: Vector2I::new(i(2), i(3)),
        size_xy: Vector2I::new(i(4), i(5)),
        orientation: EdaAngle::from_degrees(f[6].parse().unwrap()),
        diameter: i(7),
        is_slot: b(8),
        top_layer: l(9),
        bottom_layer: l(10),
        drill_start: l(11),
        drill_end: l(12),
        not_plated: b(13),
        attribute: hole_attribute(i(14)),
        filled: b(15),
        capped: b(16),
        top_covered: b(17),
        bottom_covered: b(18),
        top_plugged: b(19),
        bottom_plugged: b(20),
        top_tented: b(21),
        bottom_tented: b(22),
        front_post_machining: DrillPostMachining {
            mode: pm_mode(i(23)),
            size: i(24),
            depth: i(25),
            angle: i(26),
        },
        back_post_machining: DrillPostMachining {
            mode: pm_mode(i(27)),
            size: i(28),
            depth: i(29),
            angle: i(30),
        },
        stub_length: opt(f[31]),
    }
}

fn parse_tool(f: &[&str]) -> DrillTool {
    let i = |k: usize| -> i32 { f[k].parse().unwrap() };
    DrillTool {
        diameter: i(0),
        total_count: i(1),
        oval_count: i(2),
        hole_not_plated: i(3) != 0,
        hole_attribute: hole_attribute(i(4)),
        is_backdrill: i(5) != 0,
        has_post_machining: i(6) != 0,
        min_stub_length: opt(f[7]),
        max_stub_length: opt(f[8]),
    }
}

/// One `case` block of the golden.
#[derive(Default)]
struct Case {
    span: usize,
    merge: bool,
    npth: bool,
    raw: Vec<DrillOperation>,
    sorted: Vec<(i32, DrillOperation)>,
    tools: Vec<DrillTool>,
    ffs: Vec<(TypeFile, bool, String)>,
    names: Vec<(String, String)>,
}

/// One `board` block of the golden.
#[derive(Default)]
struct OracleBoard {
    name: String,
    copper: i32,
    file: String,
    spans: Vec<DrillSpan>,
    prots: Vec<(usize, Ipc4761Features, String, String)>,
    cases: Vec<Case>,
}

fn parse_golden() -> Vec<OracleBoard> {
    let mut boards: Vec<OracleBoard> = Vec::new();

    for line in GOLDEN.lines() {
        let f: Vec<&str> = line.split('|').collect();
        match f[0] {
            "board" => boards.push(OracleBoard {
                name: f[1].to_string(),
                copper: f[2].parse().unwrap(),
                file: f[3].to_string(),
                ..OracleBoard::default()
            }),
            "end" => assert_eq!(boards.last().unwrap().name, f[1]),
            "error" => panic!("oracle error line: {line}"),
            "span" => {
                let i = |k: usize| -> i32 { f[k].parse().unwrap() };
                let b = boards.last_mut().unwrap();
                assert_eq!(b.spans.len(), i(1) as usize);
                b.spans.push(DrillSpan::new(
                    PcbLayerId(i(2)),
                    PcbLayerId(i(3)),
                    i(4) != 0,
                    i(5) != 0,
                ));
            }
            "prot" => {
                let b = boards.last_mut().unwrap();
                let span = b.spans.len() - 1;
                b.prots.push((
                    span,
                    feature(f[1].parse().unwrap()),
                    f[2].to_string(),
                    f[3].to_string(),
                ));
            }
            "case" => boards.last_mut().unwrap().cases.push(Case {
                span: f[1].parse().unwrap(),
                merge: f[2] == "1",
                npth: f[3] == "1",
                ..Case::default()
            }),
            "raw" => {
                let c = boards.last_mut().unwrap().cases.last_mut().unwrap();
                c.raw.push(parse_op(&f[1..]));
            }
            "sorted" => {
                let c = boards.last_mut().unwrap().cases.last_mut().unwrap();
                c.sorted.push((f[1].parse().unwrap(), parse_op(&f[2..])));
            }
            "tool" => {
                let c = boards.last_mut().unwrap().cases.last_mut().unwrap();
                c.tools.push(parse_tool(&f[1..]));
            }
            "ffs" => {
                let c = boards.last_mut().unwrap().cases.last_mut().unwrap();
                // The NC form starts with "; #@! "; nothing in the strings contains '|'.
                c.ffs.push((
                    type_file(f[1].parse().unwrap()),
                    f[2] == "1",
                    f[3..].join("|"),
                ));
            }
            "name" => {
                let c = boards.last_mut().unwrap().cases.last_mut().unwrap();
                c.names.push((f[1].to_string(), f[2].to_string()));
            }
            other => panic!("unknown golden line kind {other:?}"),
        }
    }

    boards
}

fn make_board(ob: &OracleBoard) -> Board {
    let mut board = Board::default();
    board.file_name = format!("/oracle/{}", ob.file);
    board.set_copper_layer_count(ob.copper);
    board
}

#[test]
fn tool_list_names_and_file_functions_match_oracle() {
    let boards = parse_golden();
    assert!(boards.len() >= 14, "golden lists {} boards", boards.len());

    let (mut cases, mut holes, mut tie_groups) = (0usize, 0usize, 0usize);

    for ob in &boards {
        let board = make_board(ob);
        let ctx = BoardCtx::bare(&board);
        let mut w = GendrillWriterBase::new(ctx);

        for (span_idx, feat, ext, expected) in &ob.prots {
            w.drill_file_extension = ext.clone();
            let got = w.protection_file_name(&ob.spans[*span_idx], *feat);
            assert_eq!(
                &got, expected,
                "{}: protection name {feat:?} span {span_idx}",
                ob.name
            );
        }

        for case in &ob.cases {
            let span = ob.spans[case.span];
            let what = format!(
                "{} span {} ({span:?}) merge {} npth {}",
                ob.name, case.span, case.merge, case.npth
            );

            w.set_merge_option(case.merge);
            w.build_tool_list(case.raw.clone());

            assert_eq!(
                w.hole_list_buffer.len(),
                case.sorted.len(),
                "{what}: hole count"
            );
            for (k, (tool_ref, op)) in case.sorted.iter().enumerate() {
                assert_eq!(&w.hole_list_buffer[k], op, "{what}: sorted hole {k}");
                assert_eq!(w.hole_tool_references[k], *tool_ref, "{what}: tool ref {k}");
            }
            assert_eq!(w.tool_list_buffer, case.tools, "{what}: tools");

            for (type_, compat, expected) in &case.ffs {
                let got = w.build_file_function_attribute_string(&span, *type_, *compat);
                assert_eq!(
                    &got, expected,
                    "{what}: file function {type_:?} compat {compat}"
                );
            }

            for (ext, expected) in &case.names {
                w.drill_file_extension = ext.clone();
                let got = w.drill_file_name(&span, case.npth, case.merge);
                assert_eq!(&got, expected, "{what}: drill file name ({ext})");
            }

            // Count tie groups (distinct holes that cmpHoleSorting cannot order), so that the
            // test demonstrably covers the libstdc++ tie order.
            for pair in case.sorted.windows(2) {
                let (a, b) = (&pair[0].1, &pair[1].1);
                if a.not_plated == b.not_plated
                    && a.diameter == b.diameter
                    && a.attribute == b.attribute
                    && a.position == b.position
                {
                    tie_groups += 1;
                }
            }

            cases += 1;
            holes += case.sorted.len();
        }
    }

    assert!(cases >= 200, "only {cases} cases");
    assert!(holes >= 1000, "only {holes} holes");
    assert!(tie_groups >= 100, "only {tie_groups} tied neighbours");
}

/// The sort must not be a stable sort: the oracle contains tie groups whose order differs
/// from the enumeration order (guards against replacing `std_sort` by `slice::sort_by`).
#[test]
fn oracle_contains_unstable_tie_order() {
    let boards = parse_golden();
    let mut reordered = 0;

    for ob in &boards {
        for case in &ob.cases {
            let mut stable = case.raw.clone();
            stable.sort_by(|a, b| {
                use kicad_fab::exporters::gendrill_writer_base::cmp_hole_sorting;
                if cmp_hole_sorting(a, b) {
                    std::cmp::Ordering::Less
                } else if cmp_hole_sorting(b, a) {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Equal
                }
            });
            let oracle: Vec<_> = case.sorted.iter().map(|(_, op)| op.clone()).collect();
            if stable != oracle {
                reordered += 1;
            }
        }
    }

    assert!(
        reordered > 0,
        "no case distinguishes std::sort from a stable sort"
    );
}

/// Enumeration from a loaded board (`EnumerateDrillOperations` / `EnumerateDrillSpans`)
/// against the oracle's raw lists, for the corpus boards of the golden.
///
/// Needs `kicad_pcb::io::parse_board` (PCBIO packages) and the pad (M14) and via (M17)
/// accessors; enable with `--features e2e` once they exist (see `PORT_STATUS.md`).
#[test]
#[ignore = "needs kicad_pcb::io::parse_board and the pad / via model (M14, M17)"]
fn enumeration_matches_oracle() {
    #[cfg(feature = "e2e")]
    {
        use kicad_fab::drill::enumerate_drill_spans;
        use kicad_fab::drill::{DrillQuery, DrillSource, enumerate_drill_operations};

        let boards = parse_golden();
        // The corpus list of tests/golden/drill/generate.sh.
        let script = include_str!("golden/drill/generate.sh");

        for ob in boards.iter().filter(|b| !b.name.starts_with("synthetic")) {
            let rel = script
                .lines()
                .map(str::trim)
                .find(|l| l.ends_with(&format!("/{}.kicad_pcb", ob.name)))
                .expect("board listed in generate.sh");
            let path = kicad_qa::kicad_source_root().join(rel);
            let text = std::fs::read_to_string(&path).unwrap();
            let (mut board, _warnings) =
                kicad_pcb::io::parse_board(&text, &Default::default()).expect("parse");
            board.file_name = path.to_string_lossy().into_owned();
            board.project = Some(Default::default());
            let ctx = BoardCtx::bare(&board);

            // Ordinals as the oracle prints them.
            let mut ordinal = std::collections::HashMap::new();
            for (n, (id, _)) in board.tracks().enumerate() {
                ordinal.insert(DrillSource::Via(id), format!("V{n}"));
            }
            let mut n = 0;
            for (fp, _) in board.footprints() {
                for (pad, _) in board.footprint_pads(fp) {
                    ordinal.insert(DrillSource::Pad(pad), format!("P{n}"));
                    n += 1;
                }
            }

            let mut spans = enumerate_drill_spans(&ctx, false);
            spans.push(DrillSpan::new(
                PcbLayerId::FCu,
                PcbLayerId::BCu,
                false,
                true,
            ));
            assert_eq!(spans, ob.spans, "{}: spans", ob.name);

            for case in &ob.cases {
                let query = DrillQuery {
                    span: ob.spans[case.span],
                    non_plated_only: case.npth,
                    merge_pth_npth: case.merge,
                    ..DrillQuery::default()
                };
                let got: Vec<DrillOperation> = enumerate_drill_operations(&ctx, &query)
                    .into_iter()
                    .map(|mut op| {
                        op.source_id = source_kiid(&ordinal[&op.source_item.unwrap()]);
                        op.source_item = None;
                        op
                    })
                    .collect();
                assert_eq!(
                    got, case.raw,
                    "{}: case span {} merge {} npth {}",
                    ob.name, case.span, case.merge, case.npth
                );
            }
        }
    }
    #[cfg(not(feature = "e2e"))]
    panic!("enable with --features e2e once kicad_pcb::io::parse_board exists");
}
