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

//! FAB-06 verification (`PlotFootprintTextItems`, `PlotText`).
//!
//! * `crafted_text_matches_cli`: `tests/data/plot_text/text.kicad_pcb` (hand-written,
//!   re-saved with `kicad-cli pcb upgrade --force`) through the whole Gerber driver
//!   (`plot_gerber_layer_file`) for every layer and option set of
//!   `tests/golden/plot_text/generate.py`, whole files against the reference kicad-cli.
//! * Ports of `ScaledFootprintReferencePlotsScaled`, `ScaledFootprintGraphicTextPlotsScaled`
//!   (`qa/tests/pcbnew/test_plot_footprint_scaled_text.cpp`) and `EmptyKnockoutPlotsTheBox`
//!   (`qa/tests/pcbnew/test_pcb_textbox.cpp`).

mod plot_common;

use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_fab::pcb_plotter::build_plot_file_name;
use kicad_fab::pcbplot::gerber_protel_extension;
use kicad_fab::plot_board_layers::{plot_gerber_layer_file, plot_standard_layer};
use kicad_fab::wx_file_name::WxFileName;
use kicad_geom::Vector2I;
use kicad_pcb::board::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::{GerberPlotter, Plotter, RenderSettings};
use plot_common::*;

/// Layers of the crafted board that differ for a documented reason (in every option set).
/// `User.2` holds a board-level text in an outline font ("DejaVu Sans"): board text loses its
/// render cache at load (`EDA_TEXT::SetVisible`), the reference lays it out with FreeType,
/// the port (no outline font engine) with the stroke font. `User.3` holds a footprint text
/// in that font with a stored render cache, which the reference plots; the model's loader
/// drops that cache (change request), so the port falls back to the stroke font there too
/// (PORT_STATUS.md, FAB-06).
const KNOWN_DIFFERENCES: &[&str] = &["User.2", "User.3"];

/// The plot options of an option set of `generate.py` (`JobExportGerbers` mapping of the
/// `kicad-cli` flags).
fn set_opts(name: &str) -> PcbPlotParams {
    let mut opts = cli_gerber_opts(false, 6, false, false);

    match name {
        "default" => {}
        "erd" => opts.plot_reference = false,
        "ev" => opts.plot_value = false,
        "hdnp" => opts.hide_dnp_fps_on_fab_layers = true,
        "cdnp" => opts.crossout_dnp_fps_on_fab_layers = true,
        _ => panic!("unknown option set {name}"),
    }

    opts
}

/// `S <set>` / `<name> <header> <file>` of `generate.py`.
fn read_golden() -> Vec<(String, Vec<(String, String, String)>)> {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/plot_text/crafted.txt"),
    )
    .unwrap();
    let mut sets: Vec<(String, Vec<(String, String, String)>)> = Vec::new();

    for line in text.lines() {
        if let Some(name) = line.strip_prefix("S ") {
            sets.push((name.to_string(), Vec::new()));
        } else {
            let f: Vec<&str> = line.split(' ').collect();
            assert_eq!(f.len(), 3, "bad line {line:?}");
            sets.last_mut()
                .unwrap()
                .1
                .push((f[0].to_string(), f[1].to_string(), f[2].to_string()));
        }
    }

    sets
}

#[test]
fn crafted_text_matches_cli() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/plot_text/text.kicad_pcb");
    let mut board = load_board(&path).expect("text.kicad_pcb loads");
    let connectivity = build_connectivity(&mut board).expect("connectivity");
    let ctx = BoardCtx::bare(&board).with_connectivity(&connectivity);

    let names: Vec<(String, PcbLayerId)> = board
        .enabled_layers()
        .ui_order()
        .0
        .into_iter()
        .map(|layer| {
            let mut fn_ = WxFileName::new(&board.file_name);
            build_plot_file_name(
                &mut fn_,
                "",
                &board.layer_name(layer),
                &gerber_protel_extension(layer.0),
            );
            (fnv(&fn_.full_name())[..8].to_string(), layer)
        })
        .collect();

    let mut matched = 0;
    let mut known = 0;
    let mut failures = Vec::new();

    for (set, files) in read_golden() {
        let opts = set_opts(&set);

        for (name_hash, header_hash, file_hash) in &files {
            let &(_, layer) = names
                .iter()
                .find(|(h, _)| h == name_hash)
                .unwrap_or_else(|| panic!("{set}: no layer gives the file name {name_hash}"));
            let out = plot_gerber_layer_file(&ctx, &opts, layer, &[layer], "out", &env()).unwrap();
            let text = normalise(std::str::from_utf8(&out.contents).unwrap());
            let what = format!("{set} {}", board.layer_name(layer));

            assert_eq!(fnv(header(&text))[..8], *header_hash, "{what}: header");

            if fnv(&text)[..8] == *file_hash {
                matched += 1;
            } else if KNOWN_DIFFERENCES.contains(&board.layer_name(layer).as_str()) {
                known += 1;
            } else {
                if let Some(dir) = std::env::var_os("KICAD_FAB_CORPUS_DUMP") {
                    let name = format!("text-{set}-{}.gbr", board.layer_name(layer));
                    std::fs::write(std::path::Path::new(&dir).join(name), &text).unwrap();
                }
                failures.push(format!("{what}: file differs"));
            }
        }
    }

    println!(
        "plot text crafted: {matched} files matched, {known} known differences, {} failures",
        failures.len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(matched >= 60, "too few files compared: {matched}");
}

/// A Gerber plotter as the C++ tests set it up (`SetRenderSettings`, `SetViewport( (0,0),
/// IU_PER_MILS / 10, 1.0, false )`, `StartPlot( "1" )`).
/// `mm6`: millimetres with 6 decimals (1 unit = 1 IU), so that coordinates read back from
/// the file are the IU the C++ test records (the default is inches).
fn gerber_plotter(mm6: bool) -> GerberPlotter {
    let mut plotter = GerberPlotter::new();
    plotter.set_render_settings(Some(RenderSettings::default()));

    plotter.set_viewport(
        Vector2I::new(0, 0),
        PCB_IU_SCALE.iu_per_mils / 10.0,
        1.0,
        false,
    );

    // after SetViewport, which sets the IU scale the format is computed from
    if mm6 {
        plotter.set_gerber_coordinates_format(6, false);
    }

    assert!(plotter.start_plot("1"));
    plotter
}

/// `PlotStandardLayer( &board, &plotter, LSET( { F_SilkS } ), plotOpts )` with a default
/// `PCB_PLOT_PARAMS` set to Gerber; the Gerber text.
fn plot_f_silks(board: &Board, mm6: bool) -> String {
    let ctx = BoardCtx::bare(board);
    let mut plot_opts = PcbPlotParams::default();
    plot_opts.format = kicad_core::plot_enums::PlotFormat::Gerber;

    let mut plotter = gerber_plotter(mm6);
    plot_standard_layer(
        &ctx,
        &mut plotter,
        &Lset::from_layers(&[PcbLayerId::FSilkS]),
        &plot_opts,
        kicad_fab::color_settings::ColorSettings::default(),
        &kicad_fab::plot_board_layers::PadPlotState::new(),
    );
    assert!(plotter.end_plot());
    plotter.output().to_string()
}

/// The `X<x>Y<y>D0[12]*` coordinates of a Gerber text (as the C++ test's regex reads them).
fn coords(text: &str) -> Vec<(i64, i64)> {
    let mut out = Vec::new();

    for line in text.lines() {
        let Some(rest) = line.strip_prefix('X') else {
            continue;
        };
        let Some(y_at) = rest.find('Y') else {
            continue;
        };
        let Some(d_at) = rest.find('D') else {
            continue;
        };
        let tail = &rest[d_at..];

        if tail != "D01*" && tail != "D02*" {
            continue;
        }

        let x: i64 = rest[..y_at].parse().unwrap();
        let y: i64 = rest[y_at + 1..d_at].parse().unwrap();
        out.push((x, y));
    }

    out
}

/// `plotSilkBBox`: (width, height) of the bounding box of every `D01` / `D02` coordinate.
fn silk_bbox_size(board: &Board) -> (f64, f64) {
    let c = coords(&plot_f_silks(board, false));
    assert!(!c.is_empty(), "nothing plotted on F.SilkS");
    let min_x = c.iter().map(|p| p.0).min().unwrap();
    let max_x = c.iter().map(|p| p.0).max().unwrap();
    let min_y = c.iter().map(|p| p.1).min().unwrap();
    let max_y = c.iter().map(|p| p.1).max().unwrap();
    ((max_x - min_x) as f64, (max_y - min_y) as f64)
}

/// A board with one footprint at (50 mm, 50 mm) with the given transform scale and body.
fn scaled_board(scale: f64, body: &str) -> Board {
    let text = format!(
        "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")
  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (5 \"F.SilkS\" user \"F.Silkscreen\")
   (35 \"F.Fab\" user))
  (footprint \"test:scaled\" (layer \"F.Cu\")
    (transform (translate 50 50) (rotate 0) (scale {scale} {scale}))
    {body}))"
    );
    kicad_pcb::io::parse_board(
        &text,
        "scaled.kicad_pcb",
        &kicad_pcb::io::ParseOptions::default(),
    )
    .expect("scaled footprint board parses")
}

/// `BOOST_CHECK_CLOSE( a, b, pct )`: relative difference within `pct` percent of both.
fn check_close(a: f64, b: f64, pct: f64, what: &str) {
    let d = (a - b).abs();
    assert!(
        d <= a.abs() * pct / 100.0 && d <= b.abs() * pct / 100.0,
        "{what}: {a} vs {b} not within {pct} %"
    );
}

/// Port of `ScaledFootprintReferencePlotsScaled` (`test_plot_footprint_scaled_text.cpp`): a
/// scaled footprint's reference must plot at the scaled size, not lib size. The footprint is
/// built from board text (`(transform (scale 2 2))` is what `SetTransformScale( 2, 2 )`
/// stores).
#[test]
fn scaled_footprint_reference_plots_scaled() {
    let body = "(property \"Reference\" \"U1\" (at 0 0 0) (layer \"F.SilkS\")
        (effects (font (size 1 1) (thickness 0.15))))
      (property \"Value\" \"v\" (at 0 0 0) (layer \"F.Fab\") (hide yes)
        (effects (font (size 1 1) (thickness 0.15))))";

    let bbox1 = silk_bbox_size(&scaled_board(1.0, body));
    assert!(bbox1.0 > 0.0 && bbox1.1 > 0.0);

    let bbox2 = silk_bbox_size(&scaled_board(2.0, body));

    // Plotted text doubles. Slack covers stroke width and glyph metric rounding.
    check_close(bbox2.0, 2.0 * bbox1.0, 12.0, "width");
    check_close(bbox2.1, 2.0 * bbox1.1, 12.0, "height");
}

/// Port of `ScaledFootprintGraphicTextPlotsScaled`: the same for a plain graphic text item on
/// silk.
#[test]
fn scaled_footprint_graphic_text_plots_scaled() {
    let body = "(property \"Reference\" \"U1\" (at 0 0 0) (layer \"F.SilkS\") (hide yes)
        (effects (font (size 1 1) (thickness 0.15))))
      (property \"Value\" \"v\" (at 0 0 0) (layer \"F.Fab\") (hide yes)
        (effects (font (size 1 1) (thickness 0.15))))
      (fp_text user \"TEXT\" (at 0 0 0) (layer \"F.SilkS\")
        (effects (font (size 1 1) (thickness 0.15))))";

    let bbox1 = silk_bbox_size(&scaled_board(1.0, body));
    assert!(bbox1.0 > 0.0 && bbox1.1 > 0.0);

    let bbox2 = silk_bbox_size(&scaled_board(2.0, body));

    check_close(bbox2.0, 2.0 * bbox1.0, 12.0, "width");
    check_close(bbox2.1, 2.0 * bbox1.1, 12.0, "height");
}

/// Port of `EmptyKnockoutPlotsTheBox` (`test_pcb_textbox.cpp`): a knockout text box without
/// text still plots its box polygon on F.SilkS. The C++ test records `PlotPoly` calls; here
/// the Gerber regions (`G36` ... `G37`, Y axis flipped by the plotter) are read back.
#[test]
fn empty_knockout_plots_the_box() {
    let left = PCB_IU_SCALE.mm_to_iu(10.0) as i64;
    let top = PCB_IU_SCALE.mm_to_iu(10.0) as i64;
    let right = PCB_IU_SCALE.mm_to_iu(30.0) as i64;
    let bottom = PCB_IU_SCALE.mm_to_iu(15.0) as i64;

    let text = "(kicad_pcb (version 20261031) (generator \"pcbnew\") (generator_version \"10.99\")
  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (5 \"F.SilkS\" user \"F.Silkscreen\"))
  (gr_text_box \"\" (start 10 10) (end 30 15) (layer \"F.SilkS\")
    (effects (font (size 1 1))) (border no) (knockout yes)))";
    let board = kicad_pcb::io::parse_board(
        text,
        "ko.kicad_pcb",
        &kicad_pcb::io::ParseOptions::default(),
    )
    .expect("knockout box board parses");

    let gerber = plot_f_silks(&board, true);
    let mut regions: Vec<Vec<(i64, i64)>> = Vec::new();
    let mut current: Option<Vec<(i64, i64)>> = None;

    for line in gerber.lines() {
        if line == "G36*" {
            current = Some(Vec::new());
        } else if line == "G37*" {
            regions.push(current.take().expect("G37 after G36"));
        } else if let Some(region) = current.as_mut() {
            region.extend(coords(line));
        }
    }

    assert_eq!(
        regions.len(),
        1,
        "expected one plotted polygon, got {}",
        regions.len()
    );

    let r = &regions[0];
    let min_x = r.iter().map(|p| p.0).min().unwrap();
    let max_x = r.iter().map(|p| p.0).max().unwrap();
    // Gerber Y = -board Y
    let min_y = r.iter().map(|p| -p.1).min().unwrap();
    let max_y = r.iter().map(|p| -p.1).max().unwrap();

    assert_eq!(min_x, left);
    assert_eq!(min_y, top);
    assert_eq!(max_x, right);
    assert_eq!(max_y, bottom);
}
