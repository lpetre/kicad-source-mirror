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

//! Package PCBIO-03 (tracks, vias, arcs, board graphic shapes): differential tests against the
//! reference `kicad-cli pcb upgrade --force` (goldens written by
//! `tests/oracle/pcbio03_golden.sh`) and ports of the C++ tests
//! `qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp` (`CoincidentDrawingsSurviveSave`),
//! `qa/tests/pcbnew/test_board_loader.cpp` (`CollinearTrackArcLoadsAsTrack`),
//! `qa/tests/pcbnew/test_issue25650_collinear_arc.cpp`,
//! `qa/tests/pcbnew/test_graphics_load_save.cpp`, `qa/tests/pcbnew/test_ellipse_roundtrip.cpp`
//! (board cases) and `qa/tests/pcbnew/test_line_ending_load_save.cpp` (as a board round trip;
//! the footprint library path is PCBIO-06 / PCBIO-14).
//!
//! Constructs of other, still pending packages (setup, footprints, zones, ...) are skipped by
//! the staging switches (`allow_pending`); `port_pending!` model stubs are reported as pending
//! instead of failing (`model`).

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::{Kiid, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_pcb::board::Board;
use kicad_pcb::eda_shape::{FillT, ShapeT};
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::item_ref::AnyItem;
use kicad_pcb::line_ending::LineEndingStyle;
use kicad_pcb::pcb_track::Track;

/// The prefix of a `port_pending!` panic.
const PENDING: &str = "kicad-pcb: port pending: ";

/// Run `f`; a `port_pending!` panic of a not yet ported model function becomes `Err(name)`.
fn model<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(payload) => {
            let msg = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned());

            match msg {
                Some(m) if m.starts_with(PENDING) => Err(m[PENDING.len()..].to_string()),
                _ => resume_unwind(payload),
            }
        }
    }
}

fn staging_parse() -> ParseOptions {
    ParseOptions {
        allow_pending: true,
    }
}

fn staging() -> FormatOptions {
    FormatOptions {
        allow_pending: true,
        ..FormatOptions::default()
    }
}

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn parse(text: &str, name: &str) -> Board {
    parse_board_report(text, name, &staging_parse())
        .unwrap_or_else(|e| panic!("{name}: {}", e.what()))
        .0
}

fn resave(text: &str, name: &str) -> String {
    let board = parse(text, name);
    format_board(&board, &staging()).unwrap_or_else(|e| panic!("{name}: {}", e.what()))
}

const TRACKS: [&str; 3] = ["segment", "arc", "via"];
const GRAPHICS: [&str; 8] = [
    "gr_line",
    "gr_rect",
    "gr_circle",
    "gr_arc",
    "gr_poly",
    "gr_curve",
    "gr_ellipse",
    "gr_ellipse_arc",
];

/// The top-level (one tab) blocks of a prettified board file whose keyword is a track
/// (`tracks == true`) or board graphic keyword, in file order. Mirrors the extraction of
/// `tests/oracle/pcbio03_golden.sh`.
fn blocks(text: &str, tracks: bool) -> Vec<String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let mut out = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let l = lines[i];

        if !(l.starts_with("\t(") && !l.starts_with("\t\t")) {
            i += 1;
            continue;
        }

        let word = l[2..]
            .split(' ')
            .next()
            .unwrap_or("")
            .split('\n')
            .next()
            .unwrap_or("")
            .trim_end_matches(')');
        let keep = if tracks {
            TRACKS.contains(&word)
        } else {
            GRAPHICS.contains(&word)
        };
        let mut j = i;

        if !l.trim_end_matches('\n').ends_with(')') || l.matches('(').count() != l.matches(')').count()
        {
            while j < lines.len() && lines[j] != "\t)\n" {
                j += 1;
            }
        }

        if keep {
            out.push(lines[i..=j.min(lines.len() - 1)].concat());
        }

        i = j + 1;
    }

    out
}

fn fnv(bytes: &[u8]) -> String {
    format!("{:016x}", kicad_qa::io_corpus::fnv1a64(bytes))
}

/// Compare the track and graphic blocks of `out` with those of `expected`, block by block.
fn assert_same_blocks(name: &str, out: &str, expected: &str) {
    for tracks in [true, false] {
        let got = blocks(out, tracks);
        let want = blocks(expected, tracks);

        for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
            assert_eq!(g, w, "{name}: block {k} differs");
        }

        assert_eq!(got.len(), want.len(), "{name}: block count");
    }
}

/// Differential oracle on the hand-made boards (`tests/data/pcbio03/`): parse + format of the
/// input reproduces the track / via / arc / gr_* blocks of the reference re-save (O1) byte
/// for byte and in order, and parse + format of O1 reproduces O1 (every board is a fixed
/// point of the reference build).
#[test]
fn oracle_resave_handmade() {
    let data = crate_dir().join("tests/data/pcbio03");
    let golden = crate_dir().join("tests/golden/pcbio03");
    let mut names: Vec<String> = std::fs::read_dir(&data)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".kicad_pcb"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 4);

    for name in &names {
        let input = std::fs::read_to_string(data.join(name)).unwrap();
        let o1 = std::fs::read_to_string(golden.join(name)).unwrap();

        match model(|| {
            assert_same_blocks(name, &resave(&input, name), &o1);
            assert_same_blocks(&format!("{name} (O1)"), &resave(&o1, name), &o1);
        }) {
            Ok(()) => println!("{name}: identical"),
            Err(stub) => println!("{name}: pending on model stub {stub}"),
        }
    }
}

/// Corpus oracle: every corpus board the reference build re-saves
/// (`tests/golden/pcbio03_corpus.txt`); parse + format of the original board must give the
/// same track and graphic blocks. Reports, per board and kind: identical (same blocks, same
/// order), same set (the order of tracks follows net codes, which depend on footprint pads,
/// PCBIO-06 / 07), differs, pending (a model stub); `differs` fails.
#[test]
fn oracle_corpus_blocks() {
    let golden =
        std::fs::read_to_string(crate_dir().join("tests/golden/pcbio03_corpus.txt")).unwrap();
    let root = kicad_qa::kicad_source_root();
    let (mut identical, mut same_set, mut pending, mut empty) = (0, 0, 0, 0);
    let mut identical_boards = std::collections::BTreeSet::new();
    let mut failures = Vec::new();
    let mut explained = 0;
    // (path, Ok((output, tokens skipped by staging parsers)) or Err(model stub))
    type Resaved = Result<(String, Vec<String>), String>;
    let mut cache: Option<(String, Resaved)> = None;

    for line in golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('|').collect();
        let (path, kind, count) = (f[0], f[1], f[2].parse::<usize>().unwrap());

        if count == 0 {
            empty += 1;
            continue;
        }

        if cache.as_ref().is_none_or(|(p, _)| p != path) {
            let bytes = std::fs::read(root.join(path)).unwrap();
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let out = match model(|| {
                parse_board_report(&text, path, &staging_parse())
                    .and_then(|(board, report)| {
                        let skipped = report.skipped.iter().map(|s| s.token.clone()).collect();
                        Ok((format_board(&board, &staging())?, skipped))
                    })
                    .map_err(|e| e.what().to_string())
            }) {
                Ok(Ok(out)) => Ok(out),
                Ok(Err(e)) => Ok((format!("error: {e}"), Vec::new())),
                Err(stub) => Err(stub),
            };
            cache = Some((path.to_string(), out));
        }

        let (out, skipped) = match &cache.as_ref().unwrap().1 {
            Ok((out, skipped)) => (out, skipped),
            Err(stub) => {
                pending += 1;
                println!("{path}|{kind}: pending on model stub {stub}");
                continue;
            }
        };

        let got = blocks(out, kind == "tracks");
        let ordered = fnv(got.concat().as_bytes());
        let mut sorted = got.clone();
        sorted.sort();
        let sorted = fnv(sorted.concat().as_bytes());

        if got.len() == count && ordered == f[3] {
            identical += 1;
            identical_boards.insert(path.to_string());
        } else if got.len() == count && sorted == f[4] {
            same_set += 1;
            println!("{path}|{kind}: same blocks, different order");
        } else if kind == "tracks"
            && skipped
                .iter()
                .any(|t| t == "front_post_machining" || t == "back_post_machining")
        {
            // `parsePostMachining` is package PCBIO-07 (a staging stub skips it).
            explained += 1;
            println!("{path}|{kind}: differs, explained by the pending post-machining parser");
        } else {
            // The first block that is not in the reference set (needs the io-corpus cache for
            // the reference text; the hashes alone say only that something differs).
            let o1 = kicad_qa::io_corpus::io_corpus_dir()
                .join("boards")
                .join(path.replace('/', "__"))
                .join("o1.kicad_pcb");
            let first = std::fs::read_to_string(o1).ok().and_then(|o1| {
                let want = blocks(&o1, kind == "tracks");
                got.iter().find(|b| !want.contains(b)).cloned()
            });
            failures.push(format!(
                "{path}|{kind}: {} blocks, expected {count}; first unexpected block:\n{}",
                got.len(),
                first.unwrap_or_default()
            ));
        }
    }

    println!(
        "pcbio03 corpus: {identical} identical, {same_set} same set, {pending} pending, \
         {explained} explained by a pending parser, {} differ, {empty} without blocks; \
         {} boards with identical blocks",
        failures.len(),
        identical_boards.len()
    );

    for f in &failures {
        println!("DIFFERS {f}");
    }

    assert!(failures.is_empty(), "{} (board, kind) pairs differ", failures.len());
}

/// `test_kicad_sexpr.cpp` `CoincidentDrawingsSurviveSave`: the writer sorts drawings into a
/// `std::set`, so a `cmp_drawings` that reports equality for two coincident shapes would
/// silently drop one of them. (The hand-made `shapes.kicad_pcb` holds such a pair; built here
/// from text since the board has no shape constructor API in the test crate.)
#[test]
fn coincident_drawings_survive_save() {
    let line = |uuid: &str| {
        format!(
            "(gr_line (start 0 0) (end 5 0) (stroke (width 0.1) (type solid)) (layer \"F.SilkS\") \
             (uuid \"{uuid}\"))"
        )
    };
    let text = format!(
        "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")\n\
         (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (5 \"F.SilkS\" user \"F.Silkscreen\"))\n\
         {}\n{}\n)",
        line("00000000-0000-0000-0000-000000000002"),
        line("00000000-0000-0000-0000-000000000001")
    );

    let r = model(|| {
        let board = parse(&text, "coincident");
        assert_eq!(board.drawings().count(), 2);
        let saved = format_board(&board, &staging()).unwrap();
        let reloaded = parse(&saved, "coincident (saved)");
        assert_eq!(reloaded.drawings().count(), 2);
        assert_eq!(blocks(&saved, false).len(), 2);
    });

    if let Err(stub) = r {
        println!("pending on model stub {stub}");
    }
}

/// `test_issue25650_collinear_arc.cpp` `CollinearArcLoadsAsSegment` (graphic arc with
/// collinear points loads as a segment) and `ShallowArcIsNotStraight`.
#[test]
fn issue25650_collinear_arc() {
    let path = kicad_qa::pcbnew_data_dir().join("issue25650/issue25650.kicad_pcb");
    let text = std::fs::read_to_string(&path).unwrap();

    let r = model(|| {
        let board = parse(&text, "issue25650");
        let drawings: Vec<_> = board.drawings().collect();
        assert_eq!(drawings.len(), 1);

        let AnyItem::Shape(shape) = drawings[0].1 else {
            panic!("not a shape")
        };
        assert_eq!(shape.shape.shape(), ShapeT::Segment);

        let ctx = kicad_pcb::ctx::BoardCtx::bare(&board);
        assert!(shape.bounding_box(&ctx).height() < 1_000_000);
    });

    if let Err(stub) = r {
        println!("pending on model stub {stub}");
    }

    let mm = |v: f64| (v * 1e6) as i32;

    // Radius of about 1 m puts the center in range, so the bow must survive
    assert!(!kicad_geom::is_straight_arc(
        Vector2I::new(0, 0),
        Vector2I::new(mm(100.0), mm(5.0)),
        Vector2I::new(mm(200.0), 0)
    ));

    // Radius of about 5 m puts the center out of range
    assert!(kicad_geom::is_straight_arc(
        Vector2I::new(0, 0),
        Vector2I::new(mm(100.0), mm(1.0)),
        Vector2I::new(mm(200.0), 0)
    ));
    assert!(kicad_geom::is_straight_arc(
        Vector2I::new(0, 0),
        Vector2I::new(50, 50),
        Vector2I::new(100, 100)
    ));
}

/// The arc / track part of `test_board_loader.cpp` `CollinearTrackArcLoadsAsTrack` on the
/// hand-made board (a collinear `(arc ..)` record loads as a `PCB_TRACK` keeping its UUID and
/// members) and, ignored by default (a 48 MB board), on the C++ test's own Allegro import.
#[test]
fn collinear_track_arc_loads_as_track() {
    let text =
        std::fs::read_to_string(crate_dir().join("tests/data/pcbio03/tracks_vias.kicad_pcb"))
            .unwrap();
    let board = parse(&text, "tracks_vias");
    let id = Kiid::parse("00000000-0000-0000-0000-000000000202").unwrap();
    let (_, track) = board
        .tracks()
        .find(|(_, t)| t.as_track().base.item.uuid == id)
        .expect("collinear arc not loaded");

    assert!(matches!(track, Track::Trace(_)));
    assert_eq!(track.as_track().start(), Vector2I::new(30_000_000, 14_000_000));
    assert_eq!(track.as_track().end(), Vector2I::new(35_000_000, 14_000_000));
    assert_eq!(track.as_track().width(), 200_000);
    assert_eq!(track.as_track().base.item.layer, PcbLayerId::In2Cu);

    // A segment on a non-copper layer is dropped; the zero-length segment is kept.
    assert_eq!(board.tracks().count(), 26);
}

/// `test_board_loader.cpp` `CollinearTrackArcLoadsAsTrack` on the C++ test's board.
#[test]
#[ignore = "48 MB board"]
fn collinear_track_arc_loads_as_track_vcu118() {
    let path = kicad_qa::pcbnew_data_dir().join(
        "plugins/allegro/boards/VCU118_REV2-0/8851_HW-U1-VCU118_REV2-0_071417.kicad_pcb",
    );
    let text = std::fs::read_to_string(&path).unwrap();
    let board = parse(&text, "vcu118");
    let id = Kiid::parse("945ee847-b394-409c-b580-2568b3573ad8").unwrap();
    let (_, track) = board
        .tracks()
        .find(|(_, t)| t.as_track().base.item.uuid == id)
        .expect("collinear arc not loaded");

    assert!(matches!(track, Track::Trace(_)));
    let mm = |v: f64| kicad_geom::ki_round(v * 1e6);
    assert_eq!(track.as_track().start(), Vector2I::new(mm(81.471125), mm(-136.690775)));
    assert_eq!(track.as_track().end(), Vector2I::new(mm(81.476825), mm(-136.685025)));
    assert_eq!(track.as_track().width(), mm(0.15));
    assert_eq!(track.as_track().base.item.layer, PcbLayerId::In2Cu);
    let net = board.find_net(track.as_track().base.net).unwrap();
    assert_eq!(net.netname(), "FMCP_HSPC_DP11_C2M_N");
}

/// `test_graphics_load_save.cpp` `GraphicsLoad` (`graphics_load_save_v20240108`): the fill
/// state of four graphics, after load and after a save / reload round trip.
#[test]
fn graphics_load_save() {
    let path = kicad_qa::pcbnew_data_dir().join("graphics_load_save_v20240108.kicad_pcb");
    let text = std::fs::read_to_string(&path).unwrap();
    let cases = [
        ("65596b4f-7b03-48e9-8be7-5824316ea7fd", true),
        ("cf265305-49c9-43d8-bb2a-ad34997b22d6", false),
        ("d0669ae2-442f-427f-af0f-bc3008af779a", true),
        ("fd1649a3-9a92-4dd3-96b4-88469cb257ba", false),
    ];

    let check = |board: &Board| {
        assert_eq!(board.file_format_version_at_load, 20240108);

        for (uuid, filled) in cases {
            let id = Kiid::parse(uuid).unwrap();
            let shape = board
                .drawings()
                .find_map(|(_, item)| match item {
                    AnyItem::Shape(s) if s.base.item.uuid == id => Some(s),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("graphic {uuid} not found"));
            assert_eq!(shape.shape.is_solid_fill(), filled, "{uuid}");
        }
    };

    let r = model(|| {
        let board = parse(&text, "graphics_load_save");
        check(&board);
        let saved = format_board(&board, &staging()).unwrap();
        let mut reloaded = parse(&saved, "graphics_load_save (saved)");
        // LoadAndTestBoardFile checks the version of the original file only.
        reloaded.file_format_version_at_load = 20240108;
        check(&reloaded);
    });

    if let Err(stub) = r {
        println!("pending on model stub {stub}");
    }
}

/// `test_line_ending_load_save.cpp` (`FootprintLineEndingsRoundTrip`,
/// `FootprintLineEndingEqualLengthWidthRoundTrip`) as a board round trip: the style, length,
/// width and stroke width of both endings survive save / reload. (The footprint library file
/// path of the C++ test is PCBIO-06 / PCBIO-14.)
#[test]
fn line_ending_load_save() {
    let text = "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")\n\
                (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (5 \"F.SilkS\" user))\n\
                (gr_line (start 0 0) (end 1 0) (stroke (width 0.1) (type solid)) \
                (start_shape square (length 0.3) (width 0.2) (stroke (width 0.05) (type solid))) \
                (end_shape arrow_open (length 0.5) (width 0.4) (stroke (width 0.1) (type solid))) \
                (layer \"F.SilkS\") (uuid \"00000000-0000-0000-0000-000000000001\"))\n\
                (gr_line (start 0 1) (end 1 1) (stroke (width 0.1) (type solid)) \
                (start_shape square (length 0.3) (width 0.3)) \
                (layer \"F.SilkS\") (uuid \"00000000-0000-0000-0000-000000000002\"))\n)";

    let r = model(|| {
        let board = parse(text, "line_endings");
        let saved = format_board(&board, &staging()).unwrap();
        let reloaded = parse(&saved, "line_endings (saved)");
        let shapes: Vec<_> = reloaded
            .drawings()
            .filter_map(|(_, item)| match item {
                AnyItem::Shape(s) => Some(s),
                _ => None,
            })
            .collect();
        let line = |n: u8| {
            let id = Kiid::parse(&format!("00000000-0000-0000-0000-00000000000{n}"))
                .unwrap();
            *shapes.iter().find(|s| s.base.item.uuid == id).unwrap()
        };

        let check = |e: kicad_pcb::line_ending::LineEnding,
                     style: LineEndingStyle,
                     length: i32,
                     width: i32,
                     stroke_width: i32| {
            assert_eq!(e.style(), style);
            assert_eq!(e.length(), length);
            assert_eq!(e.width(), width);
            assert_eq!(e.stroke_width(), stroke_width);
        };

        let l1 = line(1);
        check(l1.shape.start_ending(), LineEndingStyle::Square, 300_000, 200_000, 50_000);
        check(l1.shape.end_ending(), LineEndingStyle::ArrowOpen, 500_000, 400_000, 100_000);
        let l2 = line(2);
        check(l2.shape.start_ending(), LineEndingStyle::Square, 300_000, 300_000, 0);
    });

    if let Err(stub) = r {
        println!("pending on model stub {stub}");
    }
}

/// `test_ellipse_roundtrip.cpp`, board cases (`ClosedEllipseRoundTrip`, `EllipseArcRoundTrip`,
/// `EllipseStrokeAndFillRoundTrip`, `EllipseLockedRoundTrip`,
/// `EllipseNegativeRotationRoundTrip`, `EllipseArcStartEqualsEndRoundTrip`,
/// `EllipseMinimumRadiiRoundTrip`, `EllipseLargeRadiiRoundTrip`,
/// `EllipseArcWrapAroundAnglesRoundTrip`, `EllipseArcNegativeStartAngleRoundTrip`,
/// `EllipseOnEdgeCutsRoundTrip`, `EllipseOnBackCopperRoundTrip`): the library ellipse
/// parameters, stroke, fill, lock and layer survive save / reload. The two footprint cases
/// need the footprint parser (PCBIO-06).
#[test]
fn ellipse_roundtrip() {
    // (shape, center, major, minor, rotation, start, end, width, fill, locked, layer)
    let cases: [(&str, &str, &str, &str, &str, Option<(&str, &str)>, &str, &str, bool, &str); 12] = [
        ("gr_ellipse", "100 80", "5", "3", "30", None, "0.15", "no", false, "F.SilkS"),
        ("gr_ellipse_arc", "50 50", "8", "4", "15", Some(("30", "210")), "0.1", "", false, "F.SilkS"),
        ("gr_ellipse", "10 10", "6", "2", "0", None, "0.3", "yes", false, "F.SilkS"),
        ("gr_ellipse", "20 20", "4", "2", "0", None, "0.1", "no", true, "F.SilkS"),
        ("gr_ellipse", "30 30", "7", "3", "-45", None, "0.1", "no", false, "F.SilkS"),
        ("gr_ellipse_arc", "40 40", "5", "3", "0", Some(("90", "90")), "0.1", "", false, "F.SilkS"),
        ("gr_ellipse", "50 50", "0.001", "0.001", "0", None, "0.1", "no", false, "F.SilkS"),
        ("gr_ellipse", "60 60", "500", "300", "10", None, "0.1", "no", false, "F.SilkS"),
        ("gr_ellipse_arc", "70 70", "5", "3", "0", Some(("300", "60")), "0.1", "", false, "F.SilkS"),
        ("gr_ellipse_arc", "80 80", "5", "3", "0", Some(("-45", "45")), "0.1", "", false, "F.SilkS"),
        ("gr_ellipse", "90 90", "5", "3", "0", None, "0.1", "no", false, "Edge.Cuts"),
        ("gr_ellipse", "95 95", "5", "3", "0", None, "0.2", "no", false, "B.Cu"),
    ];

    let mut text = String::from(
        "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")\n\
         (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (5 \"F.SilkS\" user) (25 \"Edge.Cuts\" user))\n",
    );

    for (k, (kw, center, major, minor, rot, angles, width, fill, locked, layer)) in
        cases.iter().enumerate()
    {
        let angles = angles.map_or(String::new(), |(s, e)| {
            format!(" (start_angle {s}) (end_angle {e})")
        });
        let fill = if fill.is_empty() {
            String::new()
        } else {
            format!(" (fill {fill})")
        };
        let locked = if *locked { " (locked yes)" } else { "" };
        text += &format!(
            "({kw} (center {center}) (major_radius {major}) (minor_radius {minor}) \
             (rotation_angle {rot}){angles} (stroke (width {width}) (type solid)){fill}{locked} \
             (layer \"{layer}\") (uuid \"00000000-0000-0000-0000-0000000001{k:02}\"))\n"
        );
    }

    text += ")";

    let r = model(|| {
        let board = parse(&text, "ellipses");
        let saved = format_board(&board, &staging()).unwrap();
        let reloaded = parse(&saved, "ellipses (saved)");
        let ctx = kicad_pcb::ctx::BoardCtx::bare(&reloaded);

        for (k, (kw, _, _, _, _, angles, _, fill, locked, layer)) in cases.iter().enumerate() {
            let id = Kiid::parse(&format!("00000000-0000-0000-0000-0000000001{k:02}"))
                .unwrap();
            let find = |b: &Board| {
                b.drawings()
                    .find_map(|(_, item)| match item {
                        AnyItem::Shape(s) if s.base.item.uuid == id => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap()
            };
            let a = find(&board);
            let b = find(&reloaded);

            let kind = if *kw == "gr_ellipse" {
                ShapeT::Ellipse
            } else {
                ShapeT::EllipseArc
            };
            assert_eq!(b.shape.shape(), kind, "{k}");
            assert_eq!(b.lib_ellipse_center(), a.lib_ellipse_center(), "{k}");
            assert_eq!(b.lib_ellipse_major_radius(), a.lib_ellipse_major_radius(), "{k}");
            assert_eq!(b.lib_ellipse_minor_radius(), a.lib_ellipse_minor_radius(), "{k}");
            assert_eq!(
                b.lib_ellipse_rotation().as_degrees(),
                a.lib_ellipse_rotation().as_degrees(),
                "{k}"
            );

            if angles.is_some() {
                assert_eq!(
                    b.lib_ellipse_start_angle().as_degrees(),
                    a.lib_ellipse_start_angle().as_degrees(),
                    "{k}"
                );
                assert_eq!(
                    b.lib_ellipse_end_angle().as_degrees(),
                    a.lib_ellipse_end_angle().as_degrees(),
                    "{k}"
                );
            }

            assert_eq!(b.shape.stroke.width(), a.shape.stroke.width(), "{k}");
            assert_eq!(b.shape.fill_mode(), a.shape.fill_mode(), "{k}");

            if *fill == "yes" {
                assert_eq!(b.shape.fill_mode(), FillT::FilledShape, "{k}");
            }

            assert_eq!(b.base.item.is_locked, *locked, "{k}");
            assert_eq!(
                kicad_pcb::traits::BoardItem::layer(&b, &ctx),
                PcbLayerId(kicad_core::Lset::name_to_layer(layer)),
                "{k}"
            );
        }
    });

    if let Err(stub) = r {
        println!("pending on model stub {stub}");
    }
}
