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

//! Package PCBIO-14 (footprint library files): `ParseFootprintLib` / `FP_CACHE::Load`,
//! `FormatFootprintLibFile` and the `FootprintSave` normalisation.
//!
//! * D1 of `docs/analysis/pcb-io.md` 9.3 against `kicad-cli fp upgrade --force`: three byte
//!   goldens and the masked hash of the oracle output of every `.kicad_mod` of the corpus
//!   (`tests/oracle/pcbio14_golden.sh`).
//! * Ports of `qa/tests/pcbnew/test_fp_lib_load_save.cpp` (`FpLibLoadSave`) and of the
//!   footprint library cases of `qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`.

use std::path::{Path, PathBuf};

use kicad_geom::FlipDirection;
use kicad_pcb::board::Board;
use kicad_pcb::ids::{FootprintId, ItemId, NetCode};
use kicad_pcb::io::writer::CTL_FOR_LIBRARY;
use kicad_pcb::io::{
    FormatOptions, ParseOptions, format_footprint_lib_file, parse_board, parse_footprint_file,
};
use kicad_pcb::item::RecurseMode;

fn lib_options() -> FormatOptions {
    FormatOptions {
        ctl: CTL_FOR_LIBRARY,
        ..FormatOptions::default()
    }
}

/// `FP_CACHE::Load` of one file + `FP_CACHE::Save` with `PCB_IO_KICAD_SEXPR( CTL_FOR_LIBRARY )`,
/// as `kicad-cli fp upgrade --force` does: the footprint is renamed to the file base name.
fn upgrade(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap();
    let (mut holder, _) =
        parse_footprint_file(&text, &path.to_string_lossy(), &ParseOptions::default())
            .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.what()));
    let fp = first_fp(&holder);
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    holder.modify_footprint(fp, |f, _| {
        f.fpid = kicad_core::LibId::from_parts("", &stem);
    });
    format_footprint_lib_file(&holder, fp, &lib_options())
        .unwrap_or_else(|e| panic!("{}: {}", path.display(), e.what()))
        .0
}

fn first_fp(holder: &Board) -> FootprintId {
    holder.footprints().next().map(|(id, _)| id).unwrap()
}

/// Replace every UUID-shaped token (lower-case hex) of `out` that does not occur in `input` by
/// `UUID`: the oracle draws random UUIDs for items without one. Same scanner as
/// `tests/oracle/pcbio14_mask_hash.py` (leftmost non-overlapping matches).
fn mask_fresh_uuids(input: &str, out: &str) -> String {
    fn uuids(text: &str) -> Vec<(usize, &str)> {
        let b = text.as_bytes();
        let mut found = Vec::new();
        let mut i = 0;

        while i + 36 <= b.len() {
            let is_uuid = (0..36).all(|k| match k {
                8 | 13 | 18 | 23 => b[i + k] == b'-',
                _ => matches!(b[i + k], b'0'..=b'9' | b'a'..=b'f'),
            });

            if is_uuid {
                found.push((i, &text[i..i + 36]));
                i += 36;
            } else {
                i += 1;
            }
        }

        found
    }

    let known: std::collections::HashSet<&str> = uuids(input).into_iter().map(|(_, u)| u).collect();
    let mut masked = String::with_capacity(out.len());
    let mut last = 0;

    for (at, uuid) in uuids(out) {
        masked.push_str(&out[last..at]);
        masked.push_str(if known.contains(uuid) { uuid } else { "UUID" });
        last = at + 36;
    }

    masked.push_str(&out[last..]);
    masked
}

fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;

    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }

    h
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// D1, byte goldens: `kicad-cli fp upgrade --force` output of three small files (a 2024 library
/// file, a footprint zone with a net name, a footprint zone on `*.Cu`, which a null board does
/// not mask with the enabled layers).
#[test]
fn d1_byte_goldens() {
    let pcbnew = kicad_qa::pcbnew_data_dir();

    for rel in [
        "plugins/kicad_sexpr/fp.pretty/R_0201_0603Metric.kicad_mod",
        "plugins/kicad_sexpr/Issue25447_FpHasZoneWithNetName/zone_with_netname.kicad_mod",
        "plugins/altium/pcblib/Tracks.pretty/KEEP_OUT_LAYER.kicad_mod",
    ] {
        let path = pcbnew.join(rel);
        let name = path.file_name().unwrap();
        let golden = std::fs::read_to_string(golden_dir().join("pcbio14").join(name)).unwrap();
        assert!(upgrade(&path) == golden, "{rel}: differs from the oracle");
    }
}

/// D1, whole corpus: every `.kicad_mod` of `qa/data` and `demos` re-saved like
/// `kicad-cli fp upgrade --force`, compared with the masked hash of the oracle output.
#[test]
fn d1_corpus() {
    let root = kicad_qa::kicad_source_root();
    let golden = std::fs::read_to_string(golden_dir().join("pcbio14_d1_corpus.txt")).unwrap();
    let mut failures = Vec::new();
    let mut count = 0;

    for line in golden.lines().filter(|l| !l.starts_with('#')) {
        let (hash, rel) = line.split_once(' ').unwrap();
        let path = root.join(rel);
        let input = std::fs::read_to_string(&path).unwrap();
        let out = upgrade(&path);
        let got = format!(
            "{:016x}",
            fnv1a64(mask_fresh_uuids(&input, &out).as_bytes())
        );
        count += 1;

        if got != hash {
            failures.push(rel.to_string());
        }
    }

    assert_eq!(count, 426);
    assert!(failures.is_empty(), "differ from the oracle: {failures:#?}");
}

// ---- qa/tests/pcbnew/test_fp_lib_load_save.cpp ----------------------------------------------

/// `FpLibLoadSave`: `KI_TEST::LoadAndTestFootprintFile( lib, name, true, test, version )`:
/// load, test, check the version at load, save (`DumpFootprintToFile`, default control flags)
/// and test the reloaded footprint again.
#[test]
fn fp_lib_load_save() {
    // (library, footprint, version, pads, zones)
    let cases = [
        (
            "plugins/kicad_sexpr/fp.pretty",
            "R_0201_0603Metric",
            20240108,
            // 2 SMD pads, 2 paste pads
            4,
            0,
        ),
        (
            "plugins/kicad_sexpr/Issue25447_FpHasZoneWithNetName",
            "zone_with_netname",
            20260831,
            1,
            1,
        ),
    ];

    for (lib, name, version, pads, zones) in cases {
        let path = kicad_qa::pcbnew_data_dir()
            .join(lib)
            .join(format!("{name}.kicad_mod"));
        let text = std::fs::read_to_string(&path).unwrap();
        let (holder, _) =
            parse_footprint_file(&text, &path.to_string_lossy(), &ParseOptions::default()).unwrap();
        let fp = first_fp(&holder);

        let test = |holder: &Board| {
            let fp = first_fp(holder);
            assert_eq!(holder.footprint_pads(fp).count(), pads, "{name}: pads");
            assert_eq!(holder.footprint_zones(fp).count(), zones, "{name}: zones");
        };

        test(&holder);
        assert_eq!(holder[fp].file_format_version_at_load, version, "{name}");

        // Roundtrip (PCB_IO_KICAD_SEXPR io: CTL_FOR_BOARD)
        let (saved, _) = format_footprint_lib_file(&holder, fp, &FormatOptions::default()).unwrap();
        let (again, _) = parse_footprint_file(&saved, "roundtrip", &ParseOptions::default())
            .unwrap_or_else(|e| panic!("{name}: reload: {}", e.what()));
        test(&again);
    }
}

// ---- qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp ---------------------------------

/// `LegacyModuleFileImports`: a `(module ...)` file of the 5.x era loads.
#[test]
fn legacy_module_file_imports() {
    let path = kicad_qa::pcbnew_data_dir().join("legacy_module/Crystal_SMD_HC49-US.kicad_mod");
    let text = std::fs::read_to_string(&path).unwrap();
    let (holder, _) = parse_footprint_file(&text, "legacy", &ParseOptions::default()).unwrap();
    assert_eq!(holder.footprints().count(), 1);
}

/// `LibraryFootprintKeepsGroupsAndLoadsUnlocked`: a library load finishes the footprint the
/// way `Parse()` does (groups resolved) and unlocks it. The C++ builds the locked, grouped
/// footprint with the model and saves it with `FootprintSave`; here the same footprint is
/// written as text (`(locked yes)` and a group holding the first pad), loaded, saved for the
/// library and loaded again.
#[test]
fn library_footprint_keeps_groups_and_loads_unlocked() {
    let path =
        kicad_qa::qa_data_dir().join("libraries/Resistor_SMD.pretty/R_0402_1005Metric.kicad_mod");
    let text = std::fs::read_to_string(&path).unwrap();
    let (holder, _) = parse_footprint_file(&text, "source", &ParseOptions::default()).unwrap();
    let fp = first_fp(&holder);
    let pad_uuid = holder
        .footprint_pads(fp)
        .next()
        .unwrap()
        .1
        .base
        .item
        .uuid
        .as_string();

    // footprint->SetLocked( true ) and a group with the first pad, in the file
    let at = text.find("(layer \"F.Cu\")").unwrap();
    let mut grouped = text.clone();
    grouped.insert_str(at, "(locked yes) ");
    let close = grouped.rfind(')').unwrap();
    grouped.insert_str(
        close,
        &format!("(group \"\" (uuid \"00000000-0000-0000-0000-0000000000aa\") (members \"{pad_uuid}\"))\n"),
    );

    let (loaded, _) = parse_footprint_file(&grouped, "grouped", &ParseOptions::default()).unwrap();
    let saved = format_footprint_lib_file(&loaded, first_fp(&loaded), &lib_options())
        .unwrap()
        .0;
    let (loaded, _) = parse_footprint_file(&saved, "saved", &ParseOptions::default()).unwrap();
    let fp = first_fp(&loaded);

    assert!(!loaded[fp].base.is_locked);
    let groups: Vec<_> = loaded.footprint_groups(fp).map(|(id, _)| id).collect();
    assert_eq!(groups.len(), 1);
    assert_eq!(loaded.group_members(groups[0].into()).count(), 1);
}

/// `ParseWarningDoesNotFailTheLibrary`, the parser half: a zero size pad parses with a warning
/// and leaves the footprint usable; a file that does not parse fails. The library half
/// (`FootprintEnumerate`) is `run_fp_upgrade` in kicad-session.
#[test]
fn parse_warning_does_not_fail_the_library() {
    let path =
        kicad_qa::qa_data_dir().join("libraries/Resistor_SMD.pretty/R_0402_1005Metric.kicad_mod");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("(size 0.54 0.64)"));
    let zero_pad = text.replacen("(size 0.54 0.64)", "(size 0 0)", 1);

    let (holder, report) =
        parse_footprint_file(&zero_pad, "zero", &ParseOptions::default()).unwrap();
    assert_eq!(holder.footprints().count(), 1);
    assert!(!report.warnings.is_empty());

    assert!(parse_footprint_file("(footprint", "broken", &ParseOptions::default()).is_err());
}

/// `FootprintSave_OmitsNetsOnAllBoardConnectedItems`: a footprint whose pad, copper shape and
/// zone carry a net is saved for a library without any `(net ...)` token, and the
/// normalisation (`ClearAllNets`) orphans every connected descendant. The C++ builds the board
/// with the model; here it is a board file.
#[test]
fn footprint_save_omits_nets_on_all_board_connected_items() {
    let text = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (general (thickness 1.6) (legacy_teardrops no))
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (36 "B.SilkS" user) (37 "F.SilkS" user)
    (38 "B.Mask" user) (39 "F.Mask" user) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "TestNet")
  (footprint "scratch:test_fp_save_netinfo" (layer "F.Cu") (at 10 10)
    (pad "1" smd circle (at 0 0) (size 1 1) (layers "F.Cu") (net 1 "TestNet"))
    (fp_line (start 0 0) (end 5 0) (stroke (width 0.1) (type solid)) (layer "F.Cu") (net 1))
    (zone (net 1) (net_name "TestNet") (layer "F.Cu") (hatch edge 0.5)
      (connect_pads (clearance 0)) (min_thickness 0.25) (fill (thermal_gap 0.5) (thermal_bridge_width 0.5))
      (polygon (pts (xy 0 0) (xy 1 0) (xy 1 1) (xy 0 1))))))"#;

    let mut board = parse_board(text, "board", &ParseOptions::default()).unwrap();
    let fp = first_fp(&board);

    let children = |board: &Board| {
        let ctx = kicad_pcb::ctx::BoardCtx::bare(board);
        let mut ids: Vec<ItemId> = Vec::new();
        board[fp].run_on_children(&ctx, RecurseMode::Recurse, &mut |c| ids.extend(c.id()));
        ids.into_iter()
            .filter_map(|id| {
                board
                    .get(id)
                    .and_then(|i| i.as_connected().map(|c| c.net_code()))
            })
            .collect::<Vec<NetCode>>()
    };

    // BOOST_REQUIRE_EQUAL( pad->GetNet(), net ) etc.
    let nets = children(&board);
    assert_eq!(nets.len(), 3);
    assert!(nets.iter().all(|&n| n == NetCode(1)), "{nets:?}");

    board.footprint_save_normalise(fp, FlipDirection::TopBottom);
    let (contents, _) = format_footprint_lib_file(&board, fp, &lib_options()).unwrap();
    assert!(!contents.is_empty());
    assert!(
        !contents.contains("(net "),
        "Saved footprint library file must not contain (net ...) tokens:\n{contents}"
    );

    let nets = children(&board);
    assert!(nets.iter().all(|&n| n == NetCode::UNCONNECTED), "{nets:?}");
}
