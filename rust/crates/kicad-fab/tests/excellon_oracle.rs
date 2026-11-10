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

//! Differential replay of the FAB-02 C++ oracle (`tests/oracle/excellon_oracle.cpp`, golden
//! `tests/golden/excellon_oracle.txt`, regenerate with `tests/oracle/build_excellon_oracle.sh`).
//!
//! The oracle runs the real `EXCELLON_WRITER` (clock pinned to 2025-11-09T23:34:05+05:30) on
//! 4 synthetic boards, 7 boards built like `qa/tests/pcbnew/test_cam_backdrill.cpp` and the
//! 10 corpus boards of the drill goldens, with 12 option sets each (every zero format in both
//! units, mirror, minimal header, separate / merged PTH and NPTH, G85 / routed slots, aux
//! origin offset), plus `GenDrillReportFile` (merged and separate), and prints
//! `writeCoordinates`, `writePostMachiningComment`, `formatLinearValue` and
//! `writeEXCELLONHeader` for tables of values.
//!
//! The replay feeds the C++ enumeration of each board (the `raw` lists of the FAB-01 golden
//! `drill_oracle.txt`, or of this golden for the boards built here; `rawhash` proves they are
//! the oracle's lists) through the port's drill job (`job_export_drill_from`) and report and
//! compares every message, created-file list and file (FNV-1a 64 hash, and the full text for
//! the CLI default option set and the reports) exactly.

use std::collections::BTreeMap;

use kicad_core::Kiid;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::report_severity::Severity;
use kicad_fab::VecReporter;
use kicad_fab::drill::{DrillOpKind, DrillOperation, DrillPostMachining, DrillSpan, HoleAttribute};
use kicad_fab::exporters::gendrill_excellon_writer::ExcellonWriter;
use kicad_fab::exporters::gendrill_report::HoleListSource;
use kicad_fab::exporters::gendrill_writer_base::{GendrillWriterBase, TypeFile, ZerosFmt};
use kicad_fab::jobs::job_export_pcb_drill::{
    DrillExportEnv, DrillOrigin, DrillUnits, JobExportPcbDrill, ZerosFormat,
};
use kicad_fab::jobs_handler::{default_drill_report_path, job_export_drill_from};
use kicad_geom::{EdaAngle, Vector2I};
use kicad_pcb::padstack::PadDrillPostMachiningMode;
use kicad_pcb::{Board, BoardCtx};
use kicad_plot::gbr_metadata::GbrCreationDate;

const GOLDEN: &str = include_str!("golden/excellon_oracle.txt");
const FAB01_GOLDEN: &str = include_str!("golden/drill_oracle.txt");

/// The oracle's pinned clock (`PINNED_TIME` in `TZ=XST-5:30`).
fn pinned_date() -> GbrCreationDate {
    GbrCreationDate {
        year: 2025,
        month: 11,
        day: 9,
        hour: 23,
        minute: 34,
        second: 5,
        utc_offset_minutes: 330,
    }
}

fn fnv1a(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;

    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }

    format!("{h:016x}")
}

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

fn zeros_fmt(v: i32) -> ZerosFmt {
    [
        ZerosFmt::DecimalFormat,
        ZerosFmt::SuppressLeading,
        ZerosFmt::SuppressTrailing,
        ZerosFmt::KeepZeros,
    ][v as usize]
}

/// Parses the op fields of a `raw` line (format of `tests/drill_oracle.rs`).
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
        source_item: None,
        source_id: Kiid::default(),
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
        stub_length: if f[31] == "-" {
            None
        } else {
            Some(f[31].parse().unwrap())
        },
    }
}

/// The raw enumeration of one board: case (span index, merge, npth) -> op lines.
type RawLists = BTreeMap<(usize, bool, bool), Vec<String>>;

/// The `raw` lists of the FAB-01 golden, per board name.
fn fab01_raw_lists() -> BTreeMap<String, RawLists> {
    let mut boards: BTreeMap<String, RawLists> = BTreeMap::new();
    let mut board = String::new();
    let mut case = (0, false, false);

    for line in FAB01_GOLDEN.lines() {
        let f: Vec<&str> = line.split('|').collect();
        match f[0] {
            "board" => {
                board = f[1].to_string();
                boards.insert(board.clone(), RawLists::new());
            }
            "case" => {
                case = (f[1].parse().unwrap(), f[2] == "1", f[3] == "1");
                boards.get_mut(&board).unwrap().insert(case, Vec::new());
            }
            "raw" => boards
                .get_mut(&board)
                .unwrap()
                .get_mut(&case)
                .unwrap()
                .push(line[4..].to_string()),
            _ => {}
        }
    }

    boards
}

/// The hole lists recorded by the oracle, served like `EnumerateDrillSpans` /
/// `buildHolesList` would serve them for the board.
struct RecordedSource {
    /// All spans of the oracle, the NPTH pseudo span last.
    spans: Vec<DrillSpan>,
    raw: BTreeMap<(usize, bool, bool), Vec<DrillOperation>>,
}

impl HoleListSource for RecordedSource {
    fn drill_spans(&self, _writer: &GendrillWriterBase<'_>) -> Vec<DrillSpan> {
        self.spans[..self.spans.len() - 1].to_vec()
    }

    fn build_holes_list(
        &self,
        writer: &mut GendrillWriterBase<'_>,
        span: &DrillSpan,
        generate_npth_list: bool,
    ) {
        let idx = self
            .spans
            .iter()
            .position(|s| s == span)
            .unwrap_or_else(|| panic!("span {span:?} not recorded"));
        let holes = self.raw[&(idx, writer.merge_pth_npth, generate_npth_list)].clone();
        writer.build_tool_list(holes);
    }
}

#[derive(Default)]
struct OracleFile {
    size: usize,
    hash: String,
    lines: Vec<String>,
}

#[derive(Default)]
struct Combo {
    index: usize,
    flags: Vec<bool>,
    zeros: i32,
    result: bool,
    msgs: Vec<(i32, String)>,
    created: Vec<usize>,
    files: Vec<OracleFile>,
}

#[derive(Default)]
struct Report {
    merge: bool,
    path: String,
    file: OracleFile,
}

#[derive(Default)]
struct OracleBoard {
    name: String,
    copper: i32,
    file: String,
    own: bool,
    layers: Vec<(i32, String)>,
    aux: Vector2I,
    spans: Vec<DrillSpan>,
    raw: RawLists,
    rawhash: BTreeMap<(usize, bool, bool), String>,
    combos: Vec<Combo>,
    reports: Vec<Report>,
}

#[derive(Default)]
struct Golden {
    version: String,
    vals: BTreeMap<(bool, usize), (f64, f64)>,
    coords: Vec<(bool, i32, i32, i32, i32, usize, String)>,
    pms: Vec<(bool, i32, i32, i32, i32, String, String)>,
    lins: Vec<(bool, i32, String)>,
    headers: BTreeMap<(bool, i32, bool, i32), Vec<String>>,
    boards: Vec<OracleBoard>,
}

/// The file the last `file` / `report` line opened (where `line` lines go).
enum Target {
    None,
    ComboFile,
    Report,
}

fn parse_golden() -> Golden {
    let mut g = Golden::default();
    let mut case = (0, false, false);
    let mut target = Target::None;

    for line in GOLDEN.lines() {
        let f: Vec<&str> = line.splitn(2, '|').collect();
        let rest = f.get(1).copied().unwrap_or("");
        let p: Vec<&str> = rest.split('|').collect();
        let i = |k: usize| -> i32 { p[k].parse().unwrap() };

        match f[0] {
            "version" => g.version = rest.to_string(),
            "val" => {
                g.vals.insert(
                    (i(0) == 1, p[1].parse().unwrap()),
                    (p[2].parse().unwrap(), p[3].parse().unwrap()),
                );
            }
            "coord" => {
                let text = p[6..].join("|");
                g.coords.push((
                    i(0) == 1,
                    i(1),
                    i(2),
                    i(3),
                    i(4),
                    p[5].parse().unwrap(),
                    text,
                ))
            }
            "pm" => g.pms.push((
                i(0) == 1,
                i(1),
                i(2),
                i(3),
                i(4),
                p[5].to_string(),
                p[6..].join("|"),
            )),
            "lin" => g.lins.push((i(0) == 1, i(1), p[2..].join("|"))),
            "header" => g
                .headers
                .entry((i(0) == 1, i(1), i(2) == 1, i(3)))
                .or_default()
                .push(p[4..].join("|")),
            "board" => g.boards.push(OracleBoard {
                name: p[0].to_string(),
                copper: i(1),
                file: p[2].to_string(),
                own: p[3] == "1",
                ..OracleBoard::default()
            }),
            "layer" => {
                let b = g.boards.last_mut().unwrap();
                b.layers.push((i(0), p[1..].join("|")));
            }
            "aux" => g.boards.last_mut().unwrap().aux = Vector2I::new(i(0), i(1)),
            "span" => {
                let b = g.boards.last_mut().unwrap();
                assert_eq!(b.spans.len(), i(0) as usize);
                b.spans.push(DrillSpan::new(
                    PcbLayerId(i(1)),
                    PcbLayerId(i(2)),
                    i(3) != 0,
                    i(4) != 0,
                ));
            }
            "case" => {
                case = (p[0].parse().unwrap(), p[1] == "1", p[2] == "1");
                let b = g.boards.last_mut().unwrap();
                if b.own {
                    b.raw.insert(case, Vec::new());
                }
            }
            "raw" => g
                .boards
                .last_mut()
                .unwrap()
                .raw
                .get_mut(&case)
                .unwrap()
                .push(rest.to_string()),
            "rawhash" => {
                g.boards
                    .last_mut()
                    .unwrap()
                    .rawhash
                    .insert(case, rest.to_string());
            }
            "combo" => {
                g.boards.last_mut().unwrap().combos.push(Combo {
                    index: p[0].parse().unwrap(),
                    zeros: i(2),
                    flags: p[1..].iter().map(|v| *v == "1").collect(),
                    ..Combo::default()
                });
                target = Target::None;
            }
            "result" => g.boards.last_mut().unwrap().combos.last_mut().unwrap().result = i(0) == 1,
            "msg" => g
                .boards
                .last_mut()
                .unwrap()
                .combos
                .last_mut()
                .unwrap()
                .msgs
                .push((i(0), p[1..].join("|"))),
            "created" => {
                let c = g.boards.last_mut().unwrap().combos.last_mut().unwrap();
                c.created = rest
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse().unwrap())
                    .collect();
            }
            "file" => {
                let c = g.boards.last_mut().unwrap().combos.last_mut().unwrap();
                c.files.push(OracleFile {
                    size: p[0].parse().unwrap(),
                    hash: p[1].to_string(),
                    lines: Vec::new(),
                });
                target = Target::ComboFile;
            }
            "report" => {
                g.boards.last_mut().unwrap().reports.push(Report {
                    merge: p[0] == "1",
                    path: p[1].to_string(),
                    file: OracleFile {
                        size: p[3].parse().unwrap(),
                        hash: p[4].to_string(),
                        lines: Vec::new(),
                    },
                });
                assert_eq!(p[2], "1", "C++ report failed");
                target = Target::Report;
            }
            "line" => {
                let b = g.boards.last_mut().unwrap();
                let lines = match target {
                    Target::ComboFile => {
                        &mut b.combos.last_mut().unwrap().files.last_mut().unwrap().lines
                    }
                    Target::Report => &mut b.reports.last_mut().unwrap().file.lines,
                    Target::None => panic!("line outside a file"),
                };
                lines.push(rest.to_string());
            }
            "end" => assert_eq!(g.boards.last().unwrap().name, rest),
            other => panic!("unknown golden line kind {other:?}: {line}"),
        }
    }

    g
}

fn env(g: &Golden) -> DrillExportEnv {
    DrillExportEnv {
        build_version: g.version.clone(),
        creation_date: pinned_date(),
    }
}

fn fmt_board() -> Board {
    let mut b = Board::default();
    b.file_name = "/fmt/fmt.kicad_pcb".to_string();
    b
}

#[test]
fn coordinates_match_oracle() {
    let g = parse_golden();
    let b = fmt_board();
    let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
    assert!(g.coords.len() >= 2000, "only {} coordinates", g.coords.len());

    for (metric, zeros, lhs, rhs, mantissa, idx, expected) in &g.coords {
        w.set_format(*metric, zeros_fmt(*zeros), *lhs, *rhs);
        assert_eq!((w.base.precision.lhs, w.base.precision.rhs), (*lhs, *rhs));
        w.mantissa_lenght = *mantissa;
        let (x, y) = g.vals[&(*metric, *idx)];
        let got = w.write_coordinates(x, y);
        assert_eq!(
            got.strip_suffix('\n').unwrap(),
            expected,
            "metric {metric} zeros {zeros} {lhs}:{rhs} value {idx} ({x:e}, {y:e})"
        );
    }
}

#[test]
fn post_machining_and_linear_values_match_oracle() {
    let g = parse_golden();
    let b = fmt_board();
    let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
    assert!(g.pms.len() >= 96 && g.lins.len() >= 18);

    for (metric, mode, size, depth, angle, side, expected) in &g.pms {
        w.set_format(*metric, ZerosFmt::DecimalFormat, 0, 0);
        w.mantissa_lenght = if *metric { 3 } else { 4 };
        let got = w.post_machining_comment(pm_mode(*mode), *size, *depth, *angle, side);
        let got = got.strip_suffix('\n').unwrap_or(&got);
        assert_eq!(got, expected, "{metric} {mode} {size} {depth} {angle} {side}");
    }

    for (metric, value, expected) in &g.lins {
        w.set_format(*metric, ZerosFmt::DecimalFormat, 0, 0);
        w.mantissa_lenght = if *metric { 3 } else { 4 };
        assert_eq!(&w.format_linear_value(*value), expected, "{metric} {value}");
    }
}

#[test]
fn headers_match_oracle() {
    let g = parse_golden();
    let b = fmt_board();
    let mut w = ExcellonWriter::new(BoardCtx::bare(&b));
    w.set_build_version(&g.version);
    w.set_creation_date(pinned_date());
    let span = DrillSpan::new(PcbLayerId::FCu, PcbLayerId::BCu, false, false);
    assert_eq!(g.headers.len(), 48);

    for ((metric, zeros, minimal, type_), lines) in &g.headers {
        w.set_format(*metric, zeros_fmt(*zeros), 0, 0);
        w.set_options(false, *minimal, Vector2I::new(0, 0), false);
        let type_ = [TypeFile::PthFile, TypeFile::NpthFile, TypeFile::MixedFile][*type_ as usize];
        let expected = lines.join("\n") + "\n";
        assert_eq!(
            w.excellon_header_text(&span, type_),
            expected,
            "{metric} {zeros} {minimal} {type_:?}"
        );
    }
}

/// Builds the board and the recorded hole source of one oracle board.
fn board_and_source(ob: &OracleBoard, fab01: &BTreeMap<String, RawLists>) -> (Board, RecordedSource) {
    let mut board = Board::default();
    board.file_name = format!("/oracle/{}", ob.file);
    board.set_copper_layer_count(ob.copper);

    for (id, name) in &ob.layers {
        let layer = PcbLayerId(*id);
        if board.layer_name(layer) != *name {
            assert!(board.set_layer_name(layer, name), "{}: layer {id}", ob.name);
        }
    }

    board.design_settings_mut().set_aux_origin(ob.aux);

    let raw_text = if ob.own {
        &ob.raw
    } else {
        fab01
            .get(&ob.name)
            .unwrap_or_else(|| panic!("{} not in the FAB-01 golden", ob.name))
    };

    let mut raw = BTreeMap::new();

    for (case, hash) in &ob.rawhash {
        let lines = &raw_text[case];
        let mut all = String::new();
        for l in lines {
            all.push_str(l);
            all.push('\n');
        }
        assert_eq!(
            &fnv1a(all.as_bytes()),
            hash,
            "{}: raw list of case {case:?} differs from the oracle's",
            ob.name
        );
        let ops = lines
            .iter()
            .map(|l| parse_op(&l.split('|').collect::<Vec<_>>()))
            .collect();
        raw.insert(*case, ops);
    }

    (
        board,
        RecordedSource {
            spans: ob.spans.clone(),
            raw,
        },
    )
}

fn compare_file(what: &str, got: &[u8], expected: &OracleFile) {
    if !expected.lines.is_empty() {
        let text = String::from_utf8(got.to_vec()).unwrap();
        let exp = expected.lines.join("\n") + "\n";
        assert_eq!(text, exp, "{what}: text");
    }

    assert_eq!(
        (got.len(), fnv1a(got)),
        (expected.size, expected.hash.clone()),
        "{what}: size / hash\n{}",
        String::from_utf8_lossy(got)
    );
}

#[test]
fn drill_files_and_reports_match_oracle() {
    let g = parse_golden();
    let fab01 = fab01_raw_lists();
    assert_eq!(g.boards.len(), 21);

    let (mut files, mut reports) = (0, 0);

    for ob in &g.boards {
        let (board, source) = board_and_source(ob, &fab01);
        let ctx = BoardCtx::bare(&board);

        for combo in &ob.combos {
            // metric, zeros, mirror, minimal, combine, route, plot origin, full
            let fl = &combo.flags;
            let job = JobExportPcbDrill {
                drill_units: if fl[0] { DrillUnits::Mm } else { DrillUnits::Inch },
                zero_format: [
                    ZerosFormat::Decimal,
                    ZerosFormat::SuppressLeading,
                    ZerosFormat::SuppressTrailing,
                    ZerosFormat::KeepZeros,
                ][combo.zeros as usize],
                excellon_mirror_y: fl[2],
                excellon_minimal_header: fl[3],
                excellon_combine_pth_npth: fl[4],
                excellon_oval_drill_route: fl[5],
                drill_origin: if fl[6] {
                    DrillOrigin::Plot
                } else {
                    DrillOrigin::Abs
                },
                ..JobExportPcbDrill::default()
            };
            let what = format!("{} combo {}", ob.name, combo.index);

            let mut rep = VecReporter::default();
            let outcome = job_export_drill_from(&source, ctx, &job, "out", &env(&g), None, Some(&mut rep));
            assert_eq!(outcome.success, combo.result, "{what}: result");

            let msgs: Vec<(i32, String)> = rep
                .messages
                .iter()
                .map(|(t, s)| (s.as_i32(), t.clone()))
                .collect();
            assert_eq!(msgs, combo.msgs, "{what}: messages");

            assert_eq!(outcome.files.len(), combo.files.len(), "{what}: file count");
            let created: Vec<usize> = outcome
                .outputs
                .iter()
                .map(|o| outcome.files.iter().position(|f| &f.name == o).unwrap())
                .collect();
            assert_eq!(created, combo.created, "{what}: GetCreatedFiles order");

            for (k, (got, expected)) in outcome.files.iter().zip(&combo.files).enumerate() {
                assert_eq!(
                    format!("Created file '{}'", got.name),
                    combo.msgs[k].1,
                    "{what}: file {k} name"
                );
                compare_file(&format!("{what} {}", got.name), &got.contents, expected);
                files += 1;
            }
        }

        for report in &ob.reports {
            let path = default_drill_report_path("out", &board.file_name);
            assert_eq!(path, report.path, "{}: report path", ob.name);

            let mut w = ExcellonWriter::new(ctx);
            w.set_build_version(&g.version);
            w.set_creation_date(pinned_date());
            w.set_format(true, ZerosFmt::DecimalFormat, 3, 3);
            w.set_options(false, false, Vector2I::new(0, 0), report.merge);
            assert!(w.gen_drill_report_file_from(&source, &path, None));
            assert_eq!(w.base.outputs.len(), 1);
            assert_eq!(w.base.outputs[0].name, path);
            compare_file(
                &format!("{} report merge {}", ob.name, report.merge),
                &w.base.outputs[0].contents,
                &report.file,
            );
            reports += 1;
        }
    }

    assert!(files >= 400, "only {files} files compared");
    assert_eq!(reports, 42);
}

/// `qa/tests/pcbnew/test_cam_backdrill.cpp` expectations on the Excellon outputs and reports of
/// the boards built in code (`BackdrillCamOutputs`, `FrontAndBackBackdrillCamOutputs`,
/// `DualBackdrillSameViaCamOutputs`, `DrillFileLayerOrderInFilename`, `DrillReportWithTools`,
/// `DrillReportNullReporter`, and `DrillReportBackdrillHoleCount` on
/// `issue25021/backdrill.kicad_pcb`), checked on the port's output for the oracle's hole lists
/// (the C++ tests build the boards through the via model, which is not ported yet).
#[test]
fn cam_backdrill_expectations() {
    let g = parse_golden();
    let fab01 = fab01_raw_lists();

    let run = |name: &str| -> Vec<(String, String)> {
        let ob = g.boards.iter().find(|b| b.name == name).unwrap();
        let (board, source) = board_and_source(ob, &fab01);
        let mut w = ExcellonWriter::new(BoardCtx::bare(&board));
        // excellon.SetOptions( false, false, VECTOR2I( 0, 0 ), false ); SetFormat( true )
        w.set_options(false, false, Vector2I::new(0, 0), false);
        w.set_format(true, ZerosFmt::DecimalFormat, 0, 0);
        w.create_drill_and_map_files_set_from(&source, "tmp", true, false, None)
            .unwrap();
        assert!(w.gen_drill_report_file_from(&source, "tmp/report.rpt", None));
        w.base
            .outputs
            .iter()
            .map(|o| (o.name.clone(), String::from_utf8(o.contents.clone()).unwrap()))
            .collect()
    };
    let get = |files: &[(String, String)], name: &str| -> String {
        files
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("{name} not created"))
            .1
            .clone()
    };

    // BackdrillCamOutputs
    let files = run("backdrill_board");
    let bd = get(&files, "tmp/backdrill_board_Backdrills_Drill_1_4.drl");
    assert!(bd.contains("TF.FileFunction,NonPlated,1,4,Blind"));
    assert!(bd.contains("; Backdrill"));
    assert!(bd.contains("post-machining"));
    assert!(get(&files, "tmp/backdrill_board-front-in3-backdrill.drl").contains("; backdrill"));
    assert!(get(&files, "tmp/backdrill_board-PTH.drl").contains(
        "; Post-machining front countersink dia 0.600mm depth 0.150mm angle 90deg"
    ));

    // FrontAndBackBackdrillCamOutputs, DualBackdrillSameViaCamOutputs
    for name in ["backdrill_pair_board", "dual_backdrill_board"] {
        let files = run(name);
        get(&files, &format!("tmp/{name}_Backdrills_Drill_1_2.drl"));
        // Start=B_Cu (UI index 6) drilled toward In3_Cu (UI index 4) in a 6-layer board
        assert!(get(&files, &format!("tmp/{name}_Backdrills_Drill_6_4.drl")).contains("; Backdrill"));
    }

    // DrillFileLayerOrderInFilename
    let files = run("layer_order_board");
    get(&files, "tmp/layer_order_board-in2-back.drl");
    assert!(!files.iter().any(|(n, _)| n.contains("back-in2")));

    // DrillReportWithTools, DrillReportNullReporter
    let report = get(&run("test_board_with_drills"), "tmp/report.rpt");
    assert!(report.contains("T1") && report.contains("0.300mm"));
    get(&run("test_board"), "tmp/report.rpt");

    // DrillReportBackdrillHoleCount: one section per backdrill span with its hole count.
    let report = get(&run("backdrill"), "tmp/report.rpt");
    let mut sections: BTreeMap<String, i64> = BTreeMap::new();
    let mut current = String::new();
    for line in report.lines() {
        let line = line.trim();
        if let Some(r) = line.strip_prefix("Drill file '") {
            current = r.split('\'').next().unwrap().to_string();
        }
        if let Some(count) = line.strip_prefix("Total backdrilled holes count ") {
            sections.insert(current.clone(), count.parse().unwrap());
        }
    }
    let expected: BTreeMap<String, i64> = [
        ("backdrill_Backdrills_Drill_1_4.drl", 2),
        ("backdrill_Backdrills_Drill_10_4.drl", 1),
        ("backdrill_Backdrills_Drill_10_7.drl", 1),
    ]
    .into_iter()
    .map(|(n, c)| (n.to_string(), c))
    .collect();
    assert_eq!(sections, expected);
}

/// The job: report generation order, outputs and the not-ported paths.
#[test]
fn drill_job_outputs_and_pending_parts() {
    let g = parse_golden();
    let fab01 = fab01_raw_lists();
    let ob = g.boards.iter().find(|b| b.name == "basic_test").unwrap();
    let (board, source) = board_and_source(ob, &fab01);
    let ctx = BoardCtx::bare(&board);

    let job = JobExportPcbDrill {
        drill_units: DrillUnits::Mm,
        excellon_combine_pth_npth: false,
        generate_report: true,
        ..JobExportPcbDrill::default()
    };
    let resolve = |p: &str| format!("{p}.resolved");
    let mut rep = VecReporter::default();
    let out = job_export_drill_from(&source, ctx, &job, "o", &env(&g), Some(&resolve), Some(&mut rep));
    assert!(out.success && out.error.is_none());
    let names: Vec<&str> = out.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        ["o/basic_test-PTH.drl", "o/basic_test-NPTH.drl", "o/basic_test-drill.rpt.resolved"]
    );
    assert_eq!(out.outputs, names);
    assert_eq!(
        rep.messages,
        vec![
            ("Created file 'o/basic_test-PTH.drl'".to_string(), Severity::Action),
            ("Created file 'o/basic_test-NPTH.drl'".to_string(), Severity::Action),
            ("Done.".to_string(), Severity::Info),
        ]
    );

    // An explicit report path is used as given.
    let job2 = JobExportPcbDrill {
        report_path: "x/r.txt".to_string(),
        ..job.clone()
    };
    let out = job_export_drill_from(&source, ctx, &job2, "o", &env(&g), None, None);
    assert_eq!(out.outputs.last().unwrap(), "x/r.txt");

    // Drill maps (FAB-12): the drill files are kept, "Done." is reported, the job fails.
    let job3 = JobExportPcbDrill {
        generate_map: true,
        map_format: kicad_fab::jobs::job_export_pcb_drill::MapFormat::Svg,
        ..job.clone()
    };
    let mut rep = VecReporter::default();
    let out = job_export_drill_from(&source, ctx, &job3, "o", &env(&g), None, Some(&mut rep));
    assert!(!out.success);
    assert!(matches!(out.error, Some(kicad_fab::FabError::NotPorted(_))));
    assert_eq!(out.files.len(), 2);
    assert!(out.outputs.is_empty());
    assert_eq!(rep.messages.last().unwrap().0, "Done.");

    // Gerber drill files (FAB-08).
    let job4 = JobExportPcbDrill {
        format: kicad_fab::jobs::job_export_pcb_drill::DrillFormat::Gerber,
        ..job
    };
    let out = job_export_drill_from(&source, ctx, &job4, "o", &env(&g), None, None);
    assert!(!out.success && out.files.is_empty());
    assert!(matches!(out.error, Some(kicad_fab::FabError::NotPorted(_))));
}
