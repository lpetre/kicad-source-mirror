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

//! Differential checks of the GLUE-2 wiring pass (second batch of change requests): the
//! barcode setters in the parser (`parsePCB_BARCODE`: `SetOrientation`, `SetTextSize`,
//! `SetKind`, `SetErrorCorrection`, `SetShowText`), the barcode writer's `GetTextSize()`, and
//! `PCB_POINT::SetParent` inside `FOOTPRINT::Add`, on a footprint with a non-uniform scale.
//!
//! Golden: `tests/golden/glue2/scaled_fp_items.kicad_pcb`, the reference
//! `kicad-cli pcb upgrade --force` of `tests/data/glue2/scaled_fp_items.kicad_pcb` (copied
//! alone into a scratch directory).

use std::path::{Path, PathBuf};

use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::item_ref::AnyItem;

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// The top-level `(footprint ..)` block of a pretty-printed board file.
fn footprint_block(text: &str) -> String {
    let start = text.find("\n\t(footprint").expect("footprint block") + 1;
    let end = start
        + text[start..]
            .find("\n\t)\n")
            .expect("end of footprint block")
        + 4;
    text[start..end].to_string()
}

/// Parse + format of the input reproduces the footprint block of the reference re-save
/// byte for byte (barcode `text_height` in the board frame of a footprint scaled
/// (1.5, 0.75), the hidden empty CODE_128, the point's library position).
#[test]
fn scaled_footprint_barcodes_and_point_resave() {
    let input =
        std::fs::read_to_string(crate_dir().join("tests/data/glue2/scaled_fp_items.kicad_pcb"))
            .unwrap();
    let golden =
        std::fs::read_to_string(crate_dir().join("tests/golden/glue2/scaled_fp_items.kicad_pcb"))
            .unwrap();
    let (board, report) = parse_board_report(&input, "", &ParseOptions::default()).unwrap();
    assert!(report.skipped.is_empty());

    let out = format_board(&board, &FormatOptions::default()).unwrap();
    assert_eq!(
        mask_new_uuids(&footprint_block(&out), &input),
        mask_new_uuids(&footprint_block(&golden), &input)
    );
}

/// `(uuid "..")` values that are not in `input` (the mandatory Datasheet / Description fields
/// the reference creates with random UUIDs) replaced by `*`.
fn mask_new_uuids(block: &str, input: &str) -> String {
    let mut out = String::new();
    let mut rest = block;

    while let Some(i) = rest.find("(uuid \"") {
        let start = i + 7;
        let len = rest[start..].find('"').unwrap();
        let uuid = &rest[start..start + len];
        out.push_str(&rest[..start]);
        out.push_str(if input.contains(uuid) { uuid } else { "*" });
        rest = &rest[start + len..];
    }

    out.push_str(rest);
    out
}

/// Item-level values of the same board against `pcb-dump board --no-init`: the barcodes'
/// `GetTextSize()` / `GetShowText()` / kind / ECC, and the point's board position
/// (`PCB_POINT::SetParent` recomputes it from the library position).
#[test]
fn scaled_footprint_item_values() {
    let input =
        std::fs::read_to_string(crate_dir().join("tests/data/glue2/scaled_fp_items.kicad_pcb"))
            .unwrap();
    let (board, _) = parse_board_report(&input, "", &ParseOptions::default()).unwrap();
    let ctx = BoardCtx::bare(&board);
    let mut barcodes = Vec::new();
    let mut points = Vec::new();

    for (fp, _) in board.footprints() {
        for (_, item) in board.footprint_drawings(fp) {
            if let AnyItem::Barcode(b) = item {
                barcodes.push((
                    b.text_size(&ctx),
                    b.show_text(),
                    format!("{:?}/{:?}", b.kind, b.error_correction),
                ));
            }
        }

        for (_, p) in board.footprint_points(fp) {
            let pos = p.position(&ctx);
            points.push((pos.x, pos.y));
        }
    }

    // pcb-dump: misc|..1114|barcode.text_size=1600000, show_text=true, QR_CODE / M;
    // misc|..1115|barcode.text_size=1000000, show_text=false, CODE_128 / L (the default ECC);
    // item|..1116|position=55397114,40348076.
    barcodes.sort();
    assert_eq!(
        barcodes,
        vec![
            (1_000_000, false, "Code128/L".to_string()),
            (1_600_000, true, "QrCode/M".to_string()),
        ]
    );
    assert_eq!(points, vec![(55_397_114, 40_348_076)]);
}

/// `BOARD_CONNECTED_ITEM::GetNetClassName()` (`GetEffectiveNetClass()->GetName()`) through
/// `${U1:NET_CLASS(n)}`: the comma-joined constituents of the composite netclass, as
/// `pcb-dump board` (QA load path, project `net_class_text.kicad_pro`) prints for the text's
/// `shown_text`.
#[cfg(feature = "dump")]
#[test]
fn net_class_name_is_composite_name() {
    use kicad_core::text_vars::ResolutionContext;

    let path = crate_dir().join("tests/data/glue2/net_class_text.kicad_pcb");
    let board = kicad_pcb::dump::load_board_for_dump(&path).unwrap();
    let ctx = BoardCtx::bare(&board);
    let shown: Vec<String> = board
        .drawings()
        .filter_map(|(_, item)| match item {
            AnyItem::Text(t) => Some(t.shown_text(&ctx, ResolutionContext::Resolved, 0)),
            _ => None,
        })
        .collect();

    assert_eq!(shown, vec!["L,M,Default|M,Default".to_string()]);
}
