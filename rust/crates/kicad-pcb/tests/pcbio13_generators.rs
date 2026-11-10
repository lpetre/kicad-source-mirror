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

//! Package PCBIO-13 (generators and geometric constraints): differential tests against the
//! reference `kicad-cli pcb upgrade --force` (the corpus summary
//! `tests/golden/pcbio13_corpus.txt` and the hand-made boards of `tests/golden/pcbio13/`, both
//! written by `tests/oracle/pcbio13_golden.sh`) and ports of
//! `qa/tests/pcbnew/test_generator_load_save.cpp`, `qa/tests/pcbnew/test_via_stack_saveload.cpp`
//! and `qa/tests/pcbnew/constraints/test_constraint_serialization.cpp`.
//!
//! Constructs of other, still pending packages are skipped by the staging switches
//! (`allow_pending`); `port_pending!` model stubs are reported as pending instead of failing.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::{Kiid, PcbLayerId};
use kicad_geom::Vector2I;
use kicad_pcb::board::Board;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::pcb_constraint::{ConstraintAnchor, PcbConstraintType};
use kicad_pcb::pcb_generator::{GeneratorProp, ViaStackStyle};

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

/// Parse and re-save with the staging switches; `Err` names a pending model stub.
fn resave(text: &str, name: &str) -> Result<String, String> {
    model(|| {
        let board = parse(text, name);
        format_board(&board, &staging()).unwrap_or_else(|e| panic!("{name}: {}", e.what()))
    })
}

/// The top-level keywords of this package.
const KINDS: [&str; 2] = ["generated", "constraint"];

/// The top-level (one tab) blocks of a prettified board file whose keyword is one of
/// [`KINDS`], in file order (a block is the lines from its keyword to its closing `)` line).
/// Mirrored by the Python of `tests/oracle/pcbio13_golden.sh`.
fn blocks(text: &str) -> Vec<String> {
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
            .split([' ', '\n'])
            .next()
            .unwrap_or("")
            .trim_end_matches(')');
        let keep = KINDS.contains(&word);
        let mut j = i;

        if !l.trim_end_matches('\n').ends_with(')')
            || l.matches('(').count() != l.matches(')').count()
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

/// FNV-1a 64 (as in the golden scripts).
fn fnv(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{h:016x}")
}

/// Removes the `(templates ...)` sub-block of a prettified `(generated ...)` block (the
/// template via is a staging skip until the track formatter takes a `&Track`, see
/// `PORT_STATUS.md`).
fn strip_templates(block: &str) -> String {
    let lines: Vec<&str> = block.split_inclusive('\n').collect();
    let mut out = String::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].starts_with("\t\t(templates") {
            while i < lines.len() && lines[i] != "\t\t)\n" {
                i += 1;
            }

            i += 1;
            continue;
        }

        out.push_str(lines[i]);
        i += 1;
    }

    out
}

/// The corpus boards with generators or constraints whose reference re-save is small enough
/// for the default test run (the large demos are covered by `pcbio13_golden.sh` only).
fn corpus_golden() -> Vec<(String, usize, String, String)> {
    let text = std::fs::read_to_string(crate_dir().join("tests/golden/pcbio13_corpus.txt"))
        .expect("pcbio13_corpus.txt");

    text.lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .map(|l| {
            let f: Vec<&str> = l.split('|').collect();
            (
                f[0].to_string(),
                f[1].parse().unwrap(),
                f[2].to_string(),
                f[3].to_string(),
            )
        })
        .collect()
}

/// Byte level, corpus: every `(generated ..)` / `(constraint ..)` block of the reference
/// re-save of each corpus board with generators or constraints (`pcbio13_corpus.txt`: count,
/// FNV of the blocks, FNV of the blocks without their `(templates ..)`) is reproduced by the
/// Rust re-save of the original board.
#[test]
fn oracle_corpus_generators() {
    let kicad = crate_dir().join("../../..");
    let mut checked = 0;
    let mut pending = Vec::new();

    for (path, count, all_fnv, untemplated_fnv) in corpus_golden() {
        let file = kicad.join(&path);
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };

        if text.len() > 4_000_000 {
            continue; // the large demos: golden script only
        }

        let out = match resave(&text, &path) {
            Ok(out) => out,
            Err(stub) => {
                pending.push(format!("{path}: {stub}"));
                continue;
            }
        };

        let got = blocks(&out);
        assert_eq!(got.len(), count, "{path}: block count");

        let joined = got.concat();

        if fnv(joined.as_bytes()) != all_fnv {
            let stripped: String = got.iter().map(|b| strip_templates(b)).collect();
            assert_eq!(
                fnv(stripped.as_bytes()),
                untemplated_fnv,
                "{path}: generator / constraint blocks differ from the reference re-save:\n{joined}"
            );
        }

        checked += 1;
    }

    eprintln!("pcbio13 corpus: {checked} boards checked, pending: {pending:?}");
}

/// The hand-made boards of `tests/data/pcbio13/` and their reference re-saves
/// (`tests/golden/pcbio13/<name>.kicad_pcb`): the Rust re-save reproduces every generator and
/// constraint block, of the input and of the reference output (fixed point).
#[test]
fn oracle_resave_handmade() {
    let data = crate_dir().join("tests/data/pcbio13");
    let golden = crate_dir().join("tests/golden/pcbio13");
    let mut names: Vec<_> = std::fs::read_dir(&data)
        .expect("tests/data/pcbio13")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "kicad_pcb"))
        .collect();
    names.sort();
    assert!(!names.is_empty());

    for input in names {
        let name = input.file_name().unwrap().to_string_lossy().to_string();
        let reference = std::fs::read_to_string(golden.join(&name)).expect("golden re-save");
        let expected = blocks(&reference);

        for (label, text) in [
            ("input", std::fs::read_to_string(&input).unwrap()),
            ("reference", reference.clone()),
        ] {
            let out = resave(&text, &name).unwrap_or_else(|stub| panic!("{name}: {stub}"));
            let got = blocks(&out);

            // The template via is a staging skip (see `strip_templates`).
            let strip = |v: &[String]| v.iter().map(|b| strip_templates(b)).collect::<String>();
            assert_eq!(
                strip(&got),
                strip(&expected),
                "{name} ({label}): generator / constraint blocks"
            );
        }
    }
}

/// Parse errors of the generator and constraint parsers carry the reference texts: each case
/// of `tests/data/pcbio13/parse_error_cases.txt` in a minimal board fails with the load error
/// of the reference build (`tests/golden/pcbio13/parse_errors.txt`).
#[test]
fn parse_errors() {
    let cases =
        std::fs::read_to_string(crate_dir().join("tests/data/pcbio13/parse_error_cases.txt"))
            .expect("cases");
    let expected =
        std::fs::read_to_string(crate_dir().join("tests/golden/pcbio13/parse_errors.txt"))
            .expect("golden");
    let expected: Vec<&str> = expected.lines().collect();
    let cases: Vec<&str> = cases.lines().collect();
    assert_eq!(cases.len(), expected.len());

    for (body, expected) in cases.iter().zip(expected) {
        let text = format!(
            "(kicad_pcb (version 20260206) (generator \"pcbnew\") (generator_version \"10.0\")\n\
             (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal))\n{body}\n)\n"
        );
        let got = match parse_board_report(&text, "t.kicad_pcb", &staging_parse()) {
            Ok(_) => String::new(),
            Err(e) => e.what().to_string(),
        };

        assert_eq!(got, expected, "{body}");
    }
}

/// The parsed model of a generator and a constraint: typed property values, ghost removal,
/// templates, member resolution (dangling members kept), value units.
#[test]
fn parsed_model() {
    let text = "(kicad_pcb (version 20260831) (generator \"pcbnew\") (generator_version \"10.99\")\n\
        (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (25 \"Edge.Cuts\" user))\n\
        (gr_line (start 0 0) (end 10 0) (stroke (width 0.1) (type solid)) (layer \"Edge.Cuts\") \
        (uuid \"00000000-0000-0000-0000-000000000001\"))\n\
        (generated (uuid \"00000000-0000-0000-0000-0000000000a1\") (type tuning_pattern) \
        (name \"x\") (layer \"F.Cu\"))\n\
        (constraint (type fixed_length) (uuid \"00000000-0000-0000-0000-0000000000c1\") \
        (members (member \"00000000-0000-0000-0000-000000000001\" whole) \
        (member \"00000000-0000-0000-0000-0000000000ff\" vertex 2) \
        (member \"00000000-0000-0000-0000-0000000000fe\" vertex)) (value 12.5) (driving no) \
        (custom_property \"k\" \"v\"))\n)\n";

    let board = parse(text, "t.kicad_pcb");

    // the member-less tuning pattern is a ghost
    assert_eq!(board.generators().count(), 0);

    let constraints: Vec<_> = board.constraints().collect();
    assert_eq!(constraints.len(), 1);
    let c = constraints[0].1;
    assert_eq!(c.constraint_type(), PcbConstraintType::FixedLength);
    assert_eq!(c.value(), Some(12.5 * 1_000_000.0));
    assert!(!c.is_driving());
    assert_eq!(
        c.base.custom_properties.get("k").map(String::as_str),
        Some("v")
    );
    let members = c.members();
    assert_eq!(members.len(), 3);
    assert_eq!(members[0].anchor, ConstraintAnchor::Whole);
    assert_eq!(members[0].index, -1);
    assert_eq!(members[1].anchor, ConstraintAnchor::Vertex);
    assert_eq!(members[1].index, 2);
    assert_eq!(
        members[1].item,
        Kiid::parse("00000000-0000-0000-0000-0000000000ff").unwrap()
    );
    assert_eq!(members[2].index, -1);

    // a via stitch with every property type
    let text = "(kicad_pcb (version 20260831) (generator \"pcbnew\") (generator_version \"10.99\")\n\
        (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal))\n\
        (generated (uuid \"00000000-0000-0000-0000-0000000000a2\") (type via_stitch) \
        (name \"s\") (layer \"B.Cu\") (locked yes) (zz_bool yes) (zz_num 1.25) (zz_num 7) \
        (zz_str \"q\") (zz_pt (xy 1 2)) (zz_cells (cells (ij 1 2) (ij -3 4))) \
        (zz_pts (pts (xy 0 0) (xy 1 0))) (members))\n)\n";

    let board = parse(text, "t.kicad_pcb");
    let gens: Vec<_> = board.generators().collect();
    assert_eq!(gens.len(), 1);
    let g = gens[0].1;
    assert_eq!(g.generator_type(), "via_stitch");
    assert_eq!(g.group.base.layer, PcbLayerId::BCu);
    assert!(g.group.base.is_locked);
    // normalize_properties keeps only the keys of the subclass
    let keys: BTreeMap<_, _> = g.properties.iter().collect();
    assert!(!keys.contains_key(&"zz_num".to_string()));
    assert!(matches!(
        g.properties.get("origin"),
        Some(GeneratorProp::Vector2I(_))
    ));
}

/// Port of `qa/tests/pcbnew/test_generator_load_save.cpp` `GeneratorLoading`: the tuning
/// pattern boards (current format and v20231212 with `(id ..)`) load the generator with the
/// expected lock, name and member count; a save / reload keeps the generator blocks.
#[test]
fn generator_load_save() {
    let kicad = crate_dir().join("../../..");

    for name in [
        "qa/data/pcbnew/tuning_generators_load_save.kicad_pcb",
        "qa/data/pcbnew/tuning_generators_load_save_v20231212.kicad_pcb",
    ] {
        let text = std::fs::read_to_string(kicad.join(name)).expect(name);
        let board = parse(&text, name);

        let uuid = Kiid::parse("4f22a815-3048-42b3-86fa-eb71720d35ae").unwrap();
        let (_, generator) = board
            .generators()
            .find(|(_, g)| g.group.base.uuid == uuid)
            .unwrap_or_else(|| panic!("{name}: generator"));

        assert!(!generator.group.base.is_locked, "{name}");
        assert_eq!(generator.group.name, "Tuning Pattern", "{name}");
        assert_eq!(generator.group.members.len(), 47, "{name}");

        let Ok(o1) = resave(&text, name) else {
            continue;
        };
        let o2 = resave(&o1, name).expect("second save");
        assert_eq!(blocks(&o1), blocks(&o2), "{name}");
    }
}

/// Ports of `qa/tests/pcbnew/test_via_stack_saveload.cpp`: `GoldenFileStillLoads`,
/// `PropertiesAndMembersRoundTrip` (on the golden board: the model cannot build a stack
/// without the editor setters) and `MemberlessStackIsNotSaved` (load side: the writer side is
/// in `oracle_resave_handmade`, whose board has a member-less stack).
#[test]
fn via_stack_saveload() {
    let kicad = crate_dir().join("../../..");
    let name = "qa/data/pcbnew/via_stacks.kicad_pcb";
    let text = std::fs::read_to_string(kicad.join(name)).expect(name);
    let board = parse(&text, name);

    // GoldenFileStillLoads
    let stacks: Vec<_> = board.generators().map(|(_, g)| g).collect();
    assert_eq!(stacks.len(), 2);

    let data: Vec<_> = stacks
        .iter()
        .map(|g| g.via_stack_data().expect("via stack"))
        .collect();
    let stacked = stacks
        .iter()
        .zip(&data)
        .find(|(_, d)| d.style == ViaStackStyle::Stacked)
        .unwrap();
    let staggered = stacks
        .iter()
        .zip(&data)
        .find(|(_, d)| d.style != ViaStackStyle::Stacked)
        .unwrap();

    let (g, d) = stacked;
    assert_eq!(d.start_layer, PcbLayerId::FCu);
    assert_eq!(d.end_layer, PcbLayerId::In2Cu);
    assert_eq!(g.group.base.layer, PcbLayerId::FCu);
    assert_eq!(d.via_size, 250_000);
    assert_eq!(d.via_drill, 100_000);
    assert!(d.filled);
    assert!(d.capped);
    assert!(!d.use_netclass);
    assert_eq!(d.preset_name, "HDI 1+N+1");
    assert_eq!(g.group.members.len(), 2);
    assert_eq!(d.origin, Vector2I::new(20_000_000, 20_000_000));

    let (g, d) = staggered;
    assert_eq!(d.start_layer, PcbLayerId::In1Cu);
    assert_eq!(d.end_layer, PcbLayerId::In3Cu);
    assert_eq!(g.group.base.layer, PcbLayerId::In1Cu);
    assert_eq!(d.pitch, 450_000);
    assert!(!d.filled);
    assert_eq!(d.preset_name, "Staggered 0.45");

    // PropertiesAndMembersRoundTrip
    if let Ok(o1) = resave(&text, name) {
        let reloaded = parse(&o1, name);
        let again: Vec<_> = reloaded.generators().map(|(_, g)| g).collect();
        assert_eq!(again.len(), 2);

        for (a, b) in stacks.iter().zip(again.iter()) {
            assert_eq!(a.group.base.uuid, b.group.base.uuid);
            assert_eq!(a.group.members.len(), b.group.members.len());
            assert_eq!(format!("{:?}", a.properties), format!("{:?}", b.properties));
        }
    }

    // MemberlessStackIsNotSaved (load): strip the members of the first stack
    let start = text.find("(generated").unwrap();
    let members = start + text[start..].find("(members").unwrap();
    let end = members + text[members..].find(')').unwrap();
    let ghost = format!("{}(members{}", &text[..members], &text[end..]);
    let board = parse(&ghost, name);
    assert_eq!(board.generators().count(), 1);
}

/// A board with two Edge.Cuts segments (`addSegment` of `constraint_test_utils.h`) and the
/// given constraint blocks.
fn constraint_board(constraints: &str) -> String {
    format!(
        "(kicad_pcb (version 20260831) (generator \"pcbnew\") (generator_version \"10.99\")\n\
         (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (25 \"Edge.Cuts\" user))\n\
         (gr_line (start 0 0) (end 10 0) (stroke (width 0.1) (type solid)) (layer \"Edge.Cuts\") \
         (uuid \"00000000-0000-0000-0000-00000000000a\"))\n\
         (gr_line (start 0 5) (end 8 6) (stroke (width 0.1) (type solid)) (layer \"Edge.Cuts\") \
         (uuid \"00000000-0000-0000-0000-00000000000b\"))\n{constraints}\n)\n"
    )
}

const A_ID: &str = "00000000-0000-0000-0000-00000000000a";
const B_ID: &str = "00000000-0000-0000-0000-00000000000b";

/// Save and reload (`DumpBoardToFile` + `ReadBoardFromFileOrStream`); `None` while a pending
/// model stub stops the save.
fn round_trip(text: &str) -> Option<(String, Board)> {
    let o1 = resave(text, "t.kicad_pcb").ok()?;
    let board = parse(&o1, "t.kicad_pcb");
    Some((o1, board))
}

fn find_constraint(
    board: &Board,
    type_: PcbConstraintType,
) -> Option<&kicad_pcb::pcb_constraint::PcbConstraint> {
    board
        .constraints()
        .map(|(_, c)| c)
        .find(|c| c.constraint_type() == type_)
}

/// Port of `test_constraint_serialization.cpp` `RoundTrip`.
#[test]
fn constraint_round_trip() {
    let text = constraint_board(&format!(
        "(constraint (type parallel) (uuid \"00000000-0000-0000-0000-0000000000c1\") \
         (members (member \"{A_ID}\" whole) (member \"{B_ID}\" whole)))\n\
         (constraint (type fixed_length) (uuid \"00000000-0000-0000-0000-0000000000c2\") \
         (members (member \"{A_ID}\" start) (member \"{A_ID}\" end)) (value 10) (driving no))"
    ));
    let Some((_, board2)) = round_trip(&text) else {
        return;
    };

    assert_eq!(board2.constraints().count(), 2);

    let par2 = find_constraint(&board2, PcbConstraintType::Parallel).unwrap();
    assert_eq!(par2.members().len(), 2);
    assert_eq!(par2.members()[0].item, Kiid::parse(A_ID).unwrap());
    assert_eq!(par2.members()[0].anchor, ConstraintAnchor::Whole);
    assert_eq!(par2.members()[1].item, Kiid::parse(B_ID).unwrap());
    assert!(par2.is_driving());
    assert!(!par2.has_value());

    let dim2 = find_constraint(&board2, PcbConstraintType::FixedLength).unwrap();
    assert_eq!(dim2.members().len(), 2);
    assert_eq!(dim2.members()[0].anchor, ConstraintAnchor::Start);
    assert_eq!(dim2.members()[1].anchor, ConstraintAnchor::End);
    assert!(!dim2.is_driving());
    let v = dim2.value().unwrap();
    assert!((v - 10.0 * 1_000_000.0).abs() <= 10.0 * 1_000_000.0 * 1e-8);
}

/// Port of `test_constraint_serialization.cpp` `VertexIndexRoundTrip`.
#[test]
fn constraint_vertex_index_round_trip() {
    let text = constraint_board(&format!(
        "(constraint (type coincident) (uuid \"00000000-0000-0000-0000-0000000000c1\") \
         (members (member \"{A_ID}\" vertex 1) (member \"{B_ID}\" vertex 7)))\n\
         (constraint (type horizontal) (uuid \"00000000-0000-0000-0000-0000000000c2\") \
         (members (member \"{A_ID}\" start)))"
    ));
    let Some((o1, board2)) = round_trip(&text) else {
        return;
    };

    // Written text carries the index only for VERTEX members a START member keeps the two
    // token form so the common format stays unchanged
    assert!(o1.contains(&format!("(member \"{A_ID}\" vertex 1)")));
    assert!(!o1.contains("start 1") && !o1.contains("start -"));

    assert_eq!(board2.constraints().count(), 2);
    let c2 = find_constraint(&board2, PcbConstraintType::Coincident).unwrap();
    assert_eq!(c2.members().len(), 2);
    assert_eq!(c2.members()[0].item, Kiid::parse(A_ID).unwrap());
    assert_eq!(c2.members()[0].anchor, ConstraintAnchor::Vertex);
    assert_eq!(c2.members()[0].index, 1);
    assert_eq!(c2.members()[1].item, Kiid::parse(B_ID).unwrap());
    assert_eq!(c2.members()[1].anchor, ConstraintAnchor::Vertex);
    assert_eq!(c2.members()[1].index, 7);

    let d2 = find_constraint(&board2, PcbConstraintType::Horizontal).unwrap();
    assert_eq!(d2.members().len(), 1);
    assert_eq!(d2.members()[0].anchor, ConstraintAnchor::Start);
    assert_eq!(d2.members()[0].index, -1); // absent index token maps back to -1
}

/// Port of `test_constraint_serialization.cpp` `VertexIndexValidation`.
#[test]
fn constraint_vertex_index_validation() {
    let variant = |member: &str| {
        constraint_board(&format!(
            "(constraint (type horizontal) (uuid \"00000000-0000-0000-0000-0000000000c1\") \
             (members (member \"{A_ID}\" {member})))"
        ))
    };

    // A bare vertex anchor still loads, mapping the absent index back to -1.
    let board2 = parse(&variant("vertex"), "t.kicad_pcb");
    let c: Vec<_> = board2.constraints().collect();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].1.members().len(), 1);
    assert_eq!(c[0].1.members()[0].anchor, ConstraintAnchor::Vertex);
    assert_eq!(c[0].1.members()[0].index, -1);

    // An index on a non-VERTEX anchor is rejected rather than loaded asymmetrically.
    assert!(parse_board_report(&variant("start 4"), "t", &staging_parse()).is_err());

    // A non-numeric index is rejected rather than silently becoming 0.
    assert!(parse_board_report(&variant("vertex garbage"), "t", &staging_parse()).is_err());
}

/// Port of `test_constraint_serialization.cpp` `MissingShapeMemberPreservedAsError`; also a
/// constraint without members is not written.
#[test]
fn constraint_missing_shape_member_preserved() {
    let dangling = "00000000-0000-0000-0000-0000000000ff";
    let text = constraint_board(&format!(
        "(constraint (type coincident) (uuid \"00000000-0000-0000-0000-0000000000c1\") \
         (members (member \"{A_ID}\" end) (member \"{dangling}\" start)))\n\
         (constraint (type horizontal) (uuid \"00000000-0000-0000-0000-0000000000c2\") (members))"
    ));
    let Some((_, board2)) = round_trip(&text) else {
        return;
    };

    assert_eq!(board2.constraints().count(), 1);
    let c2 = board2.constraints().next().unwrap().1;
    assert_eq!(c2.members().len(), 2);
    assert!(
        c2.members()
            .iter()
            .any(|m| m.item == Kiid::parse(A_ID).unwrap())
    );
    assert!(
        c2.members()
            .iter()
            .any(|m| m.item == Kiid::parse(dangling).unwrap())
    );
}

/// Manual byte-level check for the large boards the default run skips: `PCBIO13_BOARD` (the
/// original) and `PCBIO13_REF` (its reference `kicad-cli pcb upgrade --force` re-save, made
/// in a scratch directory); run with `--ignored`.
#[test]
#[ignore]
fn oracle_large_board() {
    let (Ok(board), Ok(reference)) = (std::env::var("PCBIO13_BOARD"), std::env::var("PCBIO13_REF"))
    else {
        return;
    };
    let text = std::fs::read_to_string(&board).expect("board");
    let expected = blocks(&std::fs::read_to_string(&reference).expect("reference"));
    let out = resave(&text, &board).unwrap_or_else(|stub| panic!("pending: {stub}"));
    let got = blocks(&out);
    let strip = |v: &[String]| v.iter().map(|b| strip_templates(b)).collect::<String>();

    assert_eq!(got.len(), expected.len());
    assert!(strip(&got) == strip(&expected), "{board}: blocks differ");
    eprintln!(
        "{board}: {} generator / constraint blocks identical",
        got.len()
    );
}
