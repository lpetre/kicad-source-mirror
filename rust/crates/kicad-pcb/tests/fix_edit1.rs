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

//! Package FIX-EDIT-1: the editing paths of the board model (mutators that no load path runs)
//! against the reference build. `tests/oracle/editmut_oracle.cpp` applies every scripted
//! edit (Move, Rotate 90 / 180 / 37.5 degrees around a point, Flip and Mirror in both
//! directions, SetLayer, SetPosition, SetWidth / SetSize, SetNetCode, the footprint
//! transforms, and the same on the children of every footprint) to every item of the boards
//! of `tests/oracle/editmut_boards.txt`, then dumps the board in pcb-dump format and saves it;
//! `editmut_golden.sh` condenses both into `tests/golden/editmut/editmut.txt` (one hash per
//! edit, board and dump section, one per saved file). This replay applies the same edits
//! through the model API (`Board::move_item` etc.), dumps with `pcb-dump-rs`'s code and saves
//! with `format_board`, and compares every hash.

#![cfg(feature = "dump")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kicad_core::PcbLayerId;
use kicad_geom::{EdaAngle, FlipDirection, Vector2I};
use kicad_pcb::board::Board;
use kicad_pcb::dump::{DumpOptions, dump_board, scan_file_uuids};
use kicad_pcb::ids::{ItemId, ItemKind, NetCode};
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::padstack::Padstack;
use kicad_pcb::pcb_track::Track;

const DELTA: Vector2I = Vector2I::new(1234567, -2500000);
const CENTRE: Vector2I = Vector2I::new(12345600, 7890000);
const NEWPOS: Vector2I = Vector2I::new(3300000, 4400000);
const WIDTH: i32 = 333000;

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// FNV-1a 64 (`editmut_hash.py`).
fn fnv(data: &[u8]) -> String {
    let mut h: u64 = 0xCBF29CE484222325;

    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001B3);
    }

    format!("{h:016x}")
}

/// `normalize_save()` of `editmut_hash.py`: every quoted UUID that is not in the input file
/// becomes "generated" (the port draws a different UUID sequence during the load).
fn normalize_save(saved: &str, known: &std::collections::BTreeSet<String>) -> String {
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

/// `hasMirror()` of the driver: the kinds without a `Mirror` override.
fn has_mirror(id: ItemId) -> bool {
    !matches!(
        id.kind(),
        ItemKind::Footprint | ItemKind::Barcode | ItemKind::Target | ItemKind::Marker
    )
}

fn is_via(board: &Board, id: ItemId) -> bool {
    id.kind() == ItemKind::Track
        && id
            .typed()
            .and_then(|t| board.track(t))
            .is_some_and(|t| matches!(t, Track::Via(_)))
}

/// `setWidth()` of the driver.
fn set_width(board: &mut Board, id: ItemId) {
    match id.kind() {
        ItemKind::Track => {
            if let Some(t) = id.typed() {
                board.modify_track(t, |t, _| match t {
                    Track::Trace(t) => t.set_width(WIDTH),
                    Track::Arc(a) => a.track.set_width(WIDTH),
                    Track::Via(v) => v.set_width(PcbLayerId::FCu, WIDTH),
                });
            }
        }
        ItemKind::Shape => {
            if let Some(s) = id.typed() {
                board.modify_shape(s, |s, env| s.set_width(env, WIDTH));
            }
        }
        ItemKind::Pad => {
            if let Some(p) = id.typed() {
                board.modify_pad(p, |p, env| {
                    p.set_size(env, Padstack::ALL_LAYERS, Vector2I::new(1100000, 700000))
                });
            }
        }
        _ => {}
    }
}

/// Applies one scripted edit (the names of `edits()` in the driver) to one item.
fn apply(board: &mut Board, edit: &str, id: ItemId) {
    let op = edit.strip_prefix("c_").unwrap_or(edit);
    let fp = || id.typed::<kicad_pcb::footprint::Footprint>();

    match op {
        "none" => {}
        "move" => board.move_item(id, DELTA),
        "rot90" => board.rotate_item(id, CENTRE, EdaAngle::from_degrees(90.0)),
        "rot180" => board.rotate_item(id, CENTRE, EdaAngle::from_degrees(180.0)),
        "rot37_5" => board.rotate_item(id, CENTRE, EdaAngle::from_degrees(37.5)),
        "flip_lr" => board.flip_item(id, CENTRE, FlipDirection::LeftRight),
        "flip_tb" => board.flip_item(id, CENTRE, FlipDirection::TopBottom),
        "mirror_lr" | "mirror_tb" => {
            if has_mirror(id) {
                let dir = if op == "mirror_lr" {
                    FlipDirection::LeftRight
                } else {
                    FlipDirection::TopBottom
                };
                board.mirror_item(id, CENTRE, dir);
            }
        }
        "layer_bcu" => board.set_item_layer(id, PcbLayerId::BCu),
        "layer_bsilk" => {
            if !is_via(board, id) {
                board.set_item_layer(id, PcbLayerId::BSilkS);
            }
        }
        "setpos" => board.set_item_position(id, NEWPOS),
        "width" => set_width(board, id),
        "net1" => {
            board.set_item_net(id, NetCode(1), true);
        }
        "fp_orient" => {
            if let Some(fp) = fp() {
                board.footprint_set_orientation(fp, EdaAngle::from_degrees(123.4));
            }
        }
        "fp_pos" => {
            if let Some(fp) = fp() {
                board.footprint_set_position(fp, NEWPOS);
            }
        }
        "fp_layerflip" => {
            if let Some(fp) = fp() {
                let layer = if board[fp].base.layer == PcbLayerId::FCu {
                    PcbLayerId::BCu
                } else {
                    PcbLayerId::FCu
                };
                board.footprint_set_layer_and_flip(fp, layer);
            }
        }
        "fp_scale" => {
            if let Some(fp) = fp() {
                board.footprint_set_transform_scale(fp, 1.25, 0.8);
            }
        }
        other => panic!("unknown edit {other}"),
    }
}

/// `walk()` of the driver.
fn walk(board: &Board, children: bool) -> Vec<ItemId> {
    let mut items: Vec<ItemId> = Vec::new();

    if children {
        for (fp, _) in board.footprints() {
            items.extend(board.footprint_fields(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(board.footprint_pads(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(board.footprint_drawings(fp).map(|(id, _)| id));
            items.extend(board.footprint_zones(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(board.footprint_groups(fp).map(|(id, _)| ItemId::from(id)));
            items.extend(board.footprint_points(fp).map(|(id, _)| ItemId::from(id)));
        }

        return items;
    }

    items.extend(board.footprints().map(|(id, _)| ItemId::from(id)));
    items.extend(board.drawings().map(|(id, _)| id));
    items.extend(board.tracks().map(|(id, _)| ItemId::from(id)));
    items.extend(board.zones().map(|(id, _)| ItemId::from(id)));
    items.extend(board.board_points().map(|(id, _)| ItemId::from(id)));
    items.extend(board.generators().map(|(id, _)| ItemId::from(id)));
    items.extend(board.groups().map(|(id, _)| ItemId::from(id)));
    items
}

/// `tests/oracle/editmut_exclude.txt`: (board or "*", key substring).
fn exclusions() -> Vec<(String, String)> {
    std::fs::read_to_string(crate_dir().join("tests/oracle/editmut_exclude.txt"))
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let (b, s) = l.split_once('\t').unwrap();
            (b.to_string(), s.to_string())
        })
        .collect()
}

/// The golden lines of one edit and board, as `editmut_hash.py` writes them (without the
/// assert count, which has no Rust counterpart). `pending|` lines (fields pcb-dump-rs cannot
/// print yet) and the fields of `editmut_exclude.txt` are left out, as there.
fn replay(
    edit: &str,
    name: &str,
    input: &str,
    rules: &[(String, String)],
) -> (BTreeMap<String, String>, String, String) {
    let (mut board, _) = parse_board_report(input, "", &ParseOptions::default()).unwrap();

    for id in walk(&board, edit.starts_with("c_")) {
        apply(&mut board, edit, id);
    }

    let opts = DumpOptions {
        sections: Default::default(),
        hash: false,
        file_uuids: Some(scan_file_uuids(input.as_bytes())),
    };
    let dump = dump_board(&board, &opts);
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut kept = String::new();

    for line in dump.lines() {
        if line.is_empty()
            || line.starts_with("file|")
            || line.starts_with('#')
            || line.starts_with("pending|")
        {
            continue;
        }

        let key = line.split_once('=').map_or(line, |(k, _)| k);

        if rules
            .iter()
            .any(|(b, sub)| (b == "*" || b == name) && key.contains(sub.as_str()))
        {
            continue;
        }

        let section = line.split('|').next().unwrap();
        groups.entry(section.to_string()).or_default().push(line);
        kept.push_str(line);
        kept.push('\n');
    }

    let mut out = BTreeMap::new();

    for (section, mut lines) in groups {
        lines.sort_unstable();
        let data = lines.join("\n");
        out.insert(
            format!("{edit}|{name}|{section}"),
            format!("{}:{}", fnv(data.as_bytes()), lines.len()),
        );
    }

    // The 36-character UUIDs of the file, as editmut_hash.py collects them.
    let lower = input.to_ascii_lowercase();
    let known = opts
        .file_uuids
        .as_ref()
        .unwrap()
        .iter()
        .filter(|u| lower.contains(&format!("\"{u}\"")))
        .cloned()
        .collect();
    let saved = normalize_save(
        &format_board(&board, &FormatOptions::default()).unwrap(),
        &known,
    );
    out.insert(
        format!("{edit}|{name}|save"),
        format!("{}:{}", fnv(saved.as_bytes()), saved.len()),
    );

    (out, kept, saved)
}

/// Every edit on every board: each dump section and the saved file equal the reference.
#[test]
fn edits_match_the_reference() {
    let golden_text =
        std::fs::read_to_string(crate_dir().join("tests/golden/editmut/editmut.txt")).unwrap();
    let mut golden: BTreeMap<String, String> = BTreeMap::new();

    for line in golden_text.lines() {
        if let Some((k, v)) = line.split_once('=')
            && !k.ends_with("|asserts")
        {
            golden.insert(k.to_string(), v.to_string());
        }
    }

    let boards = std::fs::read_to_string(crate_dir().join("tests/oracle/editmut_boards.txt"))
        .unwrap();
    let edits: Vec<String> = {
        let mut seen = Vec::new();

        for k in golden.keys() {
            let e = k.split('|').next().unwrap().to_string();

            if !seen.contains(&e) {
                seen.push(e);
            }
        }

        seen
    };
    let keep = std::env::var("FIX_EDIT1_KEEP").ok();
    let rules = exclusions();
    let mut compared = 0usize;
    let mut values = 0usize;
    let mut bad: Vec<String> = Vec::new();

    for rel in boards.lines().filter(|l| !l.is_empty()) {
        let name = Path::new(rel).file_name().unwrap().to_str().unwrap();
        let input = std::fs::read_to_string(crate_dir().join("tests/data").join(rel)).unwrap();

        for edit in &edits {
            let (rust, dump, saved) = replay(edit, name, &input, &rules);

            if let Some(dir) = &keep {
                std::fs::write(format!("{dir}/{edit}__{name}.rs.dump"), &dump).unwrap();
                std::fs::write(format!("{dir}/{edit}__{name}.rs"), &saved).unwrap();
            }

            let prefix = format!("{edit}|{name}|");

            for (k, v) in golden.range(prefix.clone()..) {
                if !k.starts_with(&prefix) {
                    break;
                }

                compared += 1;

                if !k.ends_with("|save") {
                    values += v
                        .rsplit(':')
                        .next()
                        .and_then(|n| n.parse::<usize>().ok())
                        .unwrap_or(0);
                }

                if rust.get(k) != Some(v) {
                    bad.push(format!("{k}: ref {v}, rust {:?}", rust.get(k)));
                }
            }
        }
    }

    eprintln!(
        "FIX-EDIT-1: {compared} hashes ({values} dump values) compared, {} differ",
        bad.len()
    );
    assert!(compared > 2900, "golden too small: {compared}");
    assert!(
        bad.is_empty(),
        "{} of {compared} hashes differ:\n{}",
        bad.len(),
        bad.join("\n")
    );
}
