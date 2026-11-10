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

//! Package PCBIO-12 (reference images, barcodes, targets, points, grid items, tables, generated
//! tables, drill maps): differential tests against the reference `kicad-cli pcb upgrade
//! --force` (goldens written by `tests/oracle/pcbio12_golden.sh`), the corpus re-saves of the
//! io-corpus harness, and ports of the I/O parts of `qa/tests/pcbnew/test_reference_image_load.cpp`
//! (`ReferenceImageLoading`, `ReferenceImagePpiScaleMigration`),
//! `qa/tests/pcbnew/test_custom_property_roundtrip.cpp` (`EveryBoardItemKeepsItsCustomProperty`,
//! the item kinds of this package) and `qa/tests/pcbnew/test_drill_chart.cpp`
//! (`MapSymbolSizeSurvivesBoardFileRoundTrip`).
//!
//! Constructs of other, still pending packages are skipped by the staging switches
//! (`allow_pending`); `port_pending!` model stubs are reported as pending instead of failing.

use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::path::{Path, PathBuf};

use kicad_core::PcbLayerId;
use kicad_pcb::board::Board;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::item_ref::AnyItem;

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

/// The top-level keywords of this package.
const KINDS: [&str; 9] = [
    "image",
    "barcode",
    "target",
    "point",
    "grid_item",
    "table",
    "drill_chart",
    "stackup_table",
    "drill_map",
];

/// The top-level (one tab) blocks of a prettified board file whose keyword is one of
/// [`KINDS`], in file order (a block is the lines from its keyword to its closing `)` line).
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

/// `(uuid "..")` values replaced by `*`.
fn mask_uuids(block: &str) -> String {
    let mut out = String::new();
    let mut rest = block;

    while let Some(i) = rest.find("(uuid ") {
        out.push_str(&rest[..i]);
        out.push_str("(uuid *)");
        rest = &rest[i..];
        rest = &rest[rest.find(')').map_or(rest.len(), |j| j + 1)..];
    }

    out.push_str(rest);
    out
}

/// The `(uuid "..")` value of a block.
fn uuid_of(block: &str) -> Option<&str> {
    let i = block.find("(uuid \"")? + 7;
    Some(&block[i..i + block[i..].find('"')?])
}

/// Compare the blocks of this package in `out` with those of `expected`, block by block. An
/// item without a uuid in `input` gets a random one in the reference build (R1 of
/// `pcb-io.md` 11.1): such a block is compared with its uuid masked. Returns the number of
/// blocks compared.
fn assert_same_blocks(name: &str, out: &str, expected: &str, input: &str) -> usize {
    let got = blocks(out);
    let want = blocks(expected);

    for (k, (g, w)) in got.iter().zip(want.iter()).enumerate() {
        if g != w && uuid_of(w).is_some_and(|u| !input.contains(u)) {
            assert_eq!(mask_uuids(g), mask_uuids(w), "{name}: block {k} differs");
            continue;
        }

        assert_eq!(g, w, "{name}: block {k} differs");
    }

    assert_eq!(got.len(), want.len(), "{name}: block count");
    got.len()
}

/// The goldens of `tests/oracle/pcbio12_golden.sh` and their inputs.
fn golden_inputs() -> Vec<(String, PathBuf)> {
    let root = kicad_qa::kicad_source_root();
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(crate_dir().join("tests/data/pcbio12"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), p))
        .collect();
    v.sort();

    for path in [
        "qa/data/pcbnew/reference_images_load_save.kicad_pcb",
        "qa/data/pcbnew/prettifier/group_and_image.kicad_pcb",
        "qa/data/pcbnew/issue7567.kicad_pcb",
    ] {
        let p = root.join(path);
        v.push((p.file_stem().unwrap().to_string_lossy().into_owned(), p));
    }

    v
}

/// Differential oracle on the hand-made boards (`tests/data/pcbio12/`) and the small corpus
/// boards of the golden script: parse + format of the input reproduces the blocks of this
/// package in the reference re-save (O1) byte for byte and in order, and parse + format of O1
/// reproduces the reference re-save of O1 (O2, or O1 for a fixed point).
#[test]
fn oracle_resave_blocks() {
    let golden = crate_dir().join("tests/golden/pcbio12");
    let mut compared = 0;
    let mut pending = Vec::new();

    for (name, input) in golden_inputs() {
        let input = String::from_utf8_lossy(&std::fs::read(&input).unwrap()).into_owned();

        // kicad-cli's loader rebuilds the cells of generated tables from the board
        // (RefreshGeneratedTables, tier 2): those boards are compared with the pure parse + write
        // driver only (oracle_parse_write_blocks).
        if input.contains("(drill_chart") || input.contains("(stackup_table") {
            continue;
        }

        let o1 = std::fs::read_to_string(golden.join(format!("{name}.kicad_pcb"))).unwrap();
        let o2 = std::fs::read_to_string(golden.join(format!("{name}_o2.kicad_pcb")))
            .unwrap_or_else(|_| o1.clone());

        match model(|| {
            assert_same_blocks(&name, &resave(&input, &name), &o1, &input)
                + assert_same_blocks(&format!("{name} (O1)"), &resave(&o1, &name), &o2, &o1)
        }) {
            Ok(n) => {
                println!("{name}: {n} blocks identical");
                compared += n;
            }
            Err(stub) => {
                println!("{name}: pending on model stub {stub}");
                pending.push(name);
            }
        }
    }

    println!("compared {compared} blocks; pending: {pending:?}");
    assert!(compared >= 30, "too few blocks compared: {compared}");
}

/// Pure parse + write oracle (`tests/oracle/pcbio12_resave.cpp`: the reference parser and
/// writer without kicad-cli's loader, which rebuilds the cells of generated tables): parse +
/// format of every hand-made board reproduces the blocks of this package in the driver's output
/// (`<name>_pw.kicad_pcb`) byte for byte and in order.
#[test]
fn oracle_parse_write_blocks() {
    let golden = crate_dir().join("tests/golden/pcbio12");
    let mut compared = 0;
    let mut pending = Vec::new();

    for (name, input) in golden_inputs() {
        let Ok(pw) = std::fs::read_to_string(golden.join(format!("{name}_pw.kicad_pcb"))) else {
            continue;
        };
        let input = std::fs::read_to_string(&input).unwrap();

        match model(|| assert_same_blocks(&name, &resave(&input, &name), &pw, &input)) {
            Ok(n) => {
                println!("{name}: {n} blocks identical");
                compared += n;
            }
            Err(stub) => {
                println!("{name}: pending on model stub {stub}");
                pending.push(name);
            }
        }
    }

    println!("compared {compared} blocks; pending: {pending:?}");
    assert!(compared >= 15, "too few blocks compared: {compared}");
}

/// Corpus oracle (needs the io-corpus cache of `rust/tools/io-corpus/run.sh`; skips silently
/// without it): every corpus board with items of this package; parse + format of the original
/// board must reproduce the blocks of these kinds in the reference re-save.
#[test]
fn oracle_corpus_blocks() {
    let cache = kicad_qa::io_corpus::io_corpus_dir().join("boards");
    let root = kicad_qa::kicad_source_root();
    let boards = [
        "qa/data/pcbnew/reference_images_load_save.kicad_pcb",
        "qa/data/pcbnew/prettifier/group_and_image.kicad_pcb",
        "qa/data/pcbnew/prettifier/group_and_image_formatted.kicad_pcb",
        "qa/data/pcbnew/issue7567.kicad_pcb",
        "qa/data/pcbnew/issue24525/issue24525.kicad_pcb",
        "qa/data/pcbnew/stonehenge.kicad_pcb",
        "qa/data/pcbnew/api_kitchen_sink.kicad_pcb",
    ];

    for path in boards {
        let Ok(o1) =
            std::fs::read_to_string(cache.join(path.replace('/', "__")).join("o1.kicad_pcb"))
        else {
            continue;
        };
        let input = String::from_utf8_lossy(&std::fs::read(root.join(path)).unwrap()).into_owned();

        match model(|| {
            parse_board_report(&input, path, &staging_parse())
                .and_then(|(board, _)| format_board(&board, &staging()))
                .map_err(|e| e.what().to_string())
        }) {
            Ok(Ok(out)) => {
                // kicad-cli's loader rebuilds the cells of generated tables from the board
                // (board_loader.cpp RefreshGeneratedTables, M26): api_kitchen_sink's stored
                // drill chart cells "0.30" are re-saved as "0.30 mm". Those blocks are compared
                // by the pure parse + write driver only (oracle_parse_write_blocks).
                let (out, o1) = (without_generated_tables(&out), without_generated_tables(&o1));
                let n = assert_same_blocks(path, &out, &o1, &input);
                println!("{path}: {n} blocks identical");
            }
            Ok(Err(e)) => println!("{path}: error {e}"),
            Err(stub) => println!("{path}: pending on model stub {stub}"),
        }
    }
}

/// `text` without its top-level `(drill_chart ..)` and `(stackup_table ..)` blocks (generated
/// tables, refreshed by the reference loader).
fn without_generated_tables(text: &str) -> String {
    let mut out = String::new();
    let mut skipping = false;

    for line in text.split_inclusive('\n') {
        if !skipping
            && (line.starts_with("\t(drill_chart") || line.starts_with("\t(stackup_table"))
        {
            skipping = line.matches('(').count() != line.matches(')').count();
            continue;
        }

        if skipping {
            skipping = line != "\t)\n";
            continue;
        }

        out.push_str(line);
    }

    out
}

/// The items of `board`'s drawing list of one kind.
fn drawings<'a>(board: &'a Board) -> Vec<AnyItem<'a>> {
    board.drawings().map(|(_, item)| item).collect()
}

/// Port of `test_reference_image_load.cpp` `ReferenceImageLoading` (load part): the reference
/// images of `reference_images_load_save.kicad_pcb` load with their position, layer, lock,
/// scale; and of `ReferenceImagePpiScaleMigration` (file part): a pre-20260623 file whose
/// image PPI differs from the legacy PPI is re-scaled at load.
#[test]
fn reference_image_loading() {
    let path = kicad_qa::pcbnew_data_dir().join("reference_images_load_save.kicad_pcb");
    let text = std::fs::read_to_string(path).unwrap();
    let board = parse(&text, "reference_images_load_save");
    let images: Vec<_> = drawings(&board)
        .into_iter()
        .filter_map(|i| match i {
            AnyItem::Image(img) => Some(img),
            _ => None,
        })
        .collect();

    // C++ data table: (position mm, layer, locked, scale)
    let expected = [
        ((100.0, 46.0), PcbLayerId::FCu, false, 1.0),
        ((100.0, 65.0), PcbLayerId::FCu, true, 1.0),
        ((100.0, 90.0), PcbLayerId::BCu, false, 2.0),
    ];
    assert!(images.len() >= expected.len());

    for ((x, y), layer, locked, scale) in expected {
        let pos = kicad_geom::Vector2I::new((x * 1e6) as i32, (y * 1e6) as i32);
        let img = images
            .iter()
            .find(|i| i.pos == pos)
            .unwrap_or_else(|| panic!("no image at {pos:?}"));
        assert_eq!(img.base.layer, layer);
        assert_eq!(img.base.is_locked, locked);
        assert_eq!(img.image_scale(), scale);
        assert!(img.has_image());
    }
}

/// Port of `test_custom_property_roundtrip.cpp` `EveryBoardItemKeepsItsCustomProperty` for the
/// item kinds of this package that load today (image, target, point, grid item, drill map):
/// a custom property survives save and reload.
#[test]
fn custom_property_roundtrip() {
    let text = std::fs::read_to_string(crate_dir().join("tests/data/pcbio12/misc_items.kicad_pcb"))
        .unwrap();
    let saved = resave(&text, "misc_items");
    let board = parse(&saved, "misc_items (saved)");
    let kv = |k: &str, v: &str| std::collections::BTreeMap::from([(k.to_string(), v.to_string())]);
    let mut found = std::collections::BTreeMap::new();

    for item in drawings(&board) {
        let (kind, props) = match item {
            AnyItem::Image(i) => ("image", &i.base.custom_properties),
            AnyItem::Target(i) => ("target", &i.base.custom_properties),
            AnyItem::GridItem(i) => ("grid_item", &i.base.custom_properties),
            AnyItem::DrillMap(i) => ("drill_map", &i.base.custom_properties),
            _ => continue,
        };

        if !props.is_empty() {
            found.insert(kind, props.clone());
        }
    }

    // Points are not in the drawing list (BOARD::Points() is not public in the port): check the
    // second save.
    let saved_again = format_board(&board, &staging()).unwrap();
    if saved_again.contains("(custom_property \"role\" \"anchor\")") {
        found.insert("point", kv("role", "anchor"));
    }

    assert_eq!(found.get("image"), Some(&kv("k", "v")));
    assert_eq!(found.get("target"), Some(&kv("name", "fid")));
    assert_eq!(found.get("point"), Some(&kv("role", "anchor")));
    assert_eq!(found.get("grid_item"), Some(&kv("a", "b")));
    assert_eq!(found.get("drill_map"), Some(&kv("x", "y")));
}

/// Port of `test_drill_chart.cpp` `MapSymbolSizeSurvivesBoardFileRoundTrip` (file part): a drill
/// map's symbol size, offset, span and flags survive save and reload.
#[test]
fn drill_map_round_trip() {
    let text = std::fs::read_to_string(crate_dir().join("tests/data/pcbio12/misc_items.kicad_pcb"))
        .unwrap();
    let board = parse(&resave(&text, "misc_items"), "misc_items (saved)");
    let maps: Vec<_> = drawings(&board)
        .into_iter()
        .filter_map(|i| match i {
            AnyItem::DrillMap(m) => Some(m),
            _ => None,
        })
        .collect();
    assert_eq!(maps.len(), 3);

    let m = maps
        .iter()
        .find(|m| m.base.layer == PcbLayerId::FFab)
        .unwrap();
    assert_eq!(m.symbol_size(), 1_250_000);
    assert_eq!(m.offset(), kicad_geom::Vector2I::new(1_500_000, -2_000_000));
    assert!(!m.all_spans());
    assert_eq!(
        (m.span().drill_start_layer(), m.span().drill_end_layer()),
        (PcbLayerId::FCu, PcbLayerId::In1Cu)
    );
    assert!(m.span().is_backdrill && !m.span().is_non_plated_file);
    assert!(!m.outline_slots() && m.guide_cross() && m.base.is_locked);
}

/// The error texts of the strict places.
#[test]
fn strict_errors() {
    let text = std::fs::read_to_string(crate_dir().join("tests/data/pcbio12/misc_items.kicad_pcb"))
        .unwrap();
    let head = text.split("  (image").next().unwrap();
    let err = |item: &str| {
        let board = format!("{head}  {item}\n)\n");
        match parse_board_report(&board, "t", &staging_parse()) {
            Ok(_) => String::from("ok"),
            Err(e) => e.what().to_string(),
        }
    };

    assert!(
        err(r#"(drill_map (layer "F.Cu") (size 1) (span all))"#)
            .contains("Invalid drill map: not on a documentation layer")
    );
    assert!(err(r#"(image (at 1 2) (data "AAAA"))"#).contains("Failed to read image data."));
    assert!(
        err(r#"(image (at 1 2) (foo 1))"#)
            .contains("at, layer, rotate, opacity, scale, data, locked, uuid, or custom_property")
    );
    assert!(
        err(r#"(target plus (at 1 2) (foo 1))"#)
            .contains("x, plus, at, size, width, layer, uuid, or tstamp")
    );
    assert!(err(r#"(point (at 1 2) (foo 1))"#).contains("at, size, layer or uuid"));
    assert!(err(r#"(grid_item xy (affects (foo yes)))"#).contains("cursor, routing or placement"));
    assert_eq!(err(r#"(target x (at 1 2) (size 1) (layer "F.Fab"))"#), "ok");
}

/// Port of `test_reference_image_load.cpp` `ReferenceImagePpiScaleMigration` (file part): a
/// board older than 20260623 whose image PPI (96, from 3780 pixels / m) differs from the legacy
/// truncated PPI (94) is re-scaled by 96 / 94 at load; the current version loads verbatim.
#[test]
fn reference_image_ppi_scale_migration() {
    let legacy = std::fs::read_to_string(
        crate_dir().join("tests/data/pcbio12/images_ppi_migration.kicad_pcb"),
    )
    .unwrap();
    let current = legacy.replace("(version 20260512)", "(version 20261031)");
    let first_scale = |text: &str| {
        let board = parse(text, "ppi");
        drawings(&board)
            .into_iter()
            .find_map(|i| match i {
                AnyItem::Image(img) => Some(img.image_scale()),
                _ => None,
            })
            .unwrap()
    };

    assert!((first_scale(&current) - 2.0).abs() < 2.0 * 1e-8);
    let migrated = first_scale(&legacy);
    assert!((migrated - 2.0 * 96.0 / 94.0).abs() < 2.0 * 96.0 / 94.0 * 1e-5);
}
