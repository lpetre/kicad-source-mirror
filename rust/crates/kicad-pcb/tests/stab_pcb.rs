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

//! Package STAB-PCB: regression cases of the model defects found by the corpus scoreboard
//! (`tools/pcb-dump/scoreboard.sh`) and reported by other packages, replayed against goldens
//! of the reference build (`tests/oracle/stabpcb_golden.sh`):
//!
//! * `PCB_SHAPE::Flip` / `syncLibCoords` of the primitives of a custom pad in a scaled
//!   footprint read the pad's `GetPrimitiveLibScale()` (was `(1, 1)` in the port);
//! * `(zone_connect N)` of pads and footprints keeps the raw integer (was read as INHERITED
//!   and dropped by the writer for values outside the enum);
//! * the dump's `lib.poly` is `GetLibraryPolyShape()` (the board polygon mapped back).

#![cfg(feature = "dump")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kicad_geom::FlipDirection;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::dump::format::{v_enum, v_f64, v_layer, v_points, v_polyset, v_vec};
use kicad_pcb::dump::{DumpOptions, dump_board, scan_file_uuids};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board_report};
use kicad_pcb::zone_settings::ZoneConnection;

fn crate_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(crate_dir().join(rel)).unwrap()
}

/// `SECTION|KEY|FIELD` -> value of a dump text.
fn as_map(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("file|"))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// The `prim|` lines of `stabpcb_oracle.cpp`.
fn dump_primitives(ctx: &BoardCtx<'_>) -> String {
    let board = ctx.board;
    let mut out = String::new();
    let mut p = |k: &str, f: &str, v: String| out.push_str(&format!("prim|{k}|{f}={v}\n"));

    for (fp, _) in board.footprints() {
        for (_, pad) in board.footprint_pads(fp) {
            let uuid = pad.base.item.uuid.as_string();
            let (sx, sy) = pad.primitive_lib_scale(ctx);
            p(
                &uuid,
                "primitive_lib_scale",
                format!("{},{}", v_f64(sx), v_f64(sy)),
            );

            for layer in pad.lib_padstack().unique_layers() {
                for (i, prim) in pad.lib_padstack().primitives(layer).iter().enumerate() {
                    let k = format!("{uuid}.{}.{i}", v_layer(layer));
                    let es = &prim.shape;
                    p(&k, "shape", v_enum(es.shape()));
                    p(&k, "start", v_vec(es.start()));
                    p(&k, "end", v_vec(es.end()));

                    if es.shape() == ShapeT::Arc {
                        p(&k, "mid", v_vec(es.arc_mid()));
                    }

                    if es.shape() == ShapeT::Bezier {
                        p(&k, "bezier", v_points(&[es.bezier_c1(), es.bezier_c2()]));
                    }

                    if es.shape() == ShapeT::Poly {
                        p(&k, "poly", v_polyset(es.poly_shape()));
                    }

                    let ls = prim.lib_shape();
                    p(&k, "lib.shape", v_enum(ls));
                    p(&k, "lib.start", v_vec(prim.lib_start()));
                    p(&k, "lib.end", v_vec(prim.lib_end()));

                    if ls == ShapeT::Arc {
                        p(&k, "lib.arc_mid", v_vec(prim.library_arc_mid(ctx)));
                    }

                    if ls == ShapeT::Bezier {
                        p(
                            &k,
                            "lib.bezier",
                            v_points(&[prim.library_bezier_c1(ctx), prim.library_bezier_c2(ctx)]),
                        );
                    }

                    if ls == ShapeT::Poly {
                        p(&k, "lib.poly", v_polyset(&prim.library_poly_shape(ctx)));
                    }
                }
            }
        }
    }

    out
}

/// Every footprint flipped left / right about its position: the pad primitives (board and
/// library frame) and the pad / shape sections equal the reference (`FOOTPRINT::Flip` ->
/// `PAD::FlipPrimitives` -> `PCB_SHAPE::Flip` with `GetPrimitiveLibScale()`).
#[test]
fn flipped_scaled_custom_pads_match_oracle() {
    let input = read("tests/data/stabpcb/scaled_custom_pads.kicad_pcb");
    let golden = as_map(&read("tests/golden/stabpcb/flip_scaled_custom_pads.txt"));
    let (mut board, _) = parse_board_report(&input, "", &ParseOptions::default()).unwrap();
    let fps: Vec<_> = board.footprints().map(|(id, _)| id).collect();

    for fp in fps {
        let pos = board[fp].position(&BoardCtx::bare(&board));
        board.footprint_flip(fp, pos, FlipDirection::LeftRight);
    }

    let ctx = BoardCtx::bare(&board);
    let mut text = dump_primitives(&ctx);
    let opts = DumpOptions {
        sections: ["pad", "shape"].iter().map(|s| s.to_string()).collect(),
        hash: false,
        file_uuids: Some(scan_file_uuids(input.as_bytes())),
    };
    text.push_str(&dump_board(&board, &opts));
    let rust = as_map(&text);

    let mut bad = Vec::new();

    for (k, v) in &golden {
        match rust.get(k) {
            Some(r) if r == v => {}
            other => bad.push(format!("{k}\n  ref:  {v}\n  rust: {other:?}")),
        }
    }

    assert!(golden.len() > 500, "golden too small: {}", golden.len());
    assert!(
        bad.is_empty(),
        "{} of {} values differ:\n{}",
        bad.len(),
        golden.len(),
        bad[..bad.len().min(20)].join("\n")
    );
}

/// `(zone_connect N)` outside the enum: the reference re-save (`kicad-cli pcb upgrade`)
/// writes 7 / 9 / -4 back; the readers see FULL (`ZoneConnection::from_raw`).
#[test]
fn raw_zone_connect_values_survive_a_resave() {
    let input = read("tests/data/stabpcb/scaled_custom_pads.kicad_pcb");
    let golden = read("tests/golden/stabpcb/scaled_custom_pads.upgraded.kicad_pcb");
    let (board, _) = parse_board_report(&input, "", &ParseOptions::default()).unwrap();
    let out = format_board(&board, &FormatOptions::default()).unwrap();

    let lines = |t: &str| -> Vec<String> {
        t.lines()
            .filter(|l| l.contains("(zone_connect"))
            .map(|l| l.trim().to_string())
            .collect()
    };
    assert_eq!(lines(&out), lines(&golden));
    assert_eq!(
        lines(&out),
        ["(zone_connect 7)", "(zone_connect 9)", "(zone_connect -4)"]
    );

    let ctx = BoardCtx::bare(&board);
    let (fp, footprint) = board.footprints().next().unwrap();
    assert_eq!(footprint.local_zone_connection_raw(), 7);
    assert_eq!(footprint.local_zone_connection(), ZoneConnection::Full);

    let pads: Vec<_> = board.footprint_pads(fp).map(|(_, p)| p).collect();
    assert_eq!(pads[0].local_zone_connection_raw(), 9);
    assert_eq!(pads[0].local_zone_connection(), ZoneConnection::Full);
    assert_eq!(pads[1].local_zone_connection_raw(), -4);
    // PAD::GetZoneConnectionOverrides: -4 != INHERITED, so the pad's value wins.
    assert_eq!(
        pads[1].zone_connection_overrides(&ctx, false).0,
        ZoneConnection::Full
    );
}
