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

//! Ports of the FAB-04 cases of `qa/tests/pcbnew/test_plot_edge_cuts_drill_marks.cpp`
//! (`NoDrillFlashesOnEdgeCuts`, `NoDrillMarksOnGerberFromStoredOptions`) and
//! `qa/tests/pcbnew/test_plot_pad_mask.cpp` (`RoundRectGerberMaskApertureHasCorrectRadius`).
//!
//! The C++ tests call `PlotBoardLayers` / `PlotStandardLayer`, which first run the footprint
//! text and graphic loops (FAB-05 / FAB-06, still pending stubs that panic for every
//! footprint). Each test therefore runs twice: the C++ call itself, reported as pending while
//! it reaches a pending stub; and the same plot through the FAB-04 part
//! (`plot_standard_layer_pads_vias_tracks_zones`), which is what the assertions are about
//! (the footprints of these boards have no text or graphics on the plotted layer).

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_core::plot_enums::{DrillMarks, PlotFormat};
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_fab::plot_board_layers::{
    StandardLayerFlags, plot_board_layers, plot_standard_layer,
    plot_standard_layer_pads_vias_tracks_zones,
};
use kicad_fab::plot_brditems_plotter::BrditemsPlotter;
use kicad_geom::Vector2I;
use kicad_pcb::board::Board;
use kicad_pcb::board::containers::AddMode;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::item_ref::OwnedItem;
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{PadAttrib, PadDrillShape, PadShape, Padstack, PadstackMode};
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::{GerberPlotter, Plotter, RenderSettings};

/// `new FOOTPRINT( &board )` at `fp_pos` with one pad built by `make` (its padstack and
/// attributes), placed at `pad_pos` (`pad->SetPosition`), added to the board.
fn board_with_pad(fp_pos: Vector2I, pad_pos: Vector2I, make: impl FnOnce(&mut Pad)) -> Board {
    let mut board = Board::new();
    let fp = board.create_footprint();
    board.footprint_set_position(fp, fp_pos);

    let mut pad = Pad::default();
    make(&mut pad);
    let id = board
        .footprint_add(fp, OwnedItem::Pad(Box::new(pad)), AddMode::Insert)
        .unwrap();
    board.modify_pad(id.typed::<Pad>().unwrap(), |pad, env| {
        pad.set_position(env, pad_pos)
    });
    board.attach_footprint(fp, AddMode::Insert);
    board
}

/// A padstack of `pad` with the given mode, shape, size, drill and layers.
fn set_padstack(
    pad: &mut Pad,
    shape: PadShape,
    size: Vector2I,
    drill: Option<Vector2I>,
    layers: Lset,
) {
    let mut ps = pad.lib_padstack().clone();
    ps.set_mode(PadstackMode::Normal);
    ps.set_shape(shape, Padstack::ALL_LAYERS);
    ps.set_size(size, Padstack::ALL_LAYERS);

    if let Some(drill) = drill {
        ps.drill_mut().shape = PadDrillShape::Circle;
        ps.drill_mut().size = drill;
    }

    ps.layer_set = layers;
    pad.set_padstack(&ps);
}

/// A Gerber plotter as the C++ tests set it up (`SetRenderSettings`, `SetViewport( (0,0),
/// IU_PER_MILS / 10, 1.0, false )`, `StartPlot( "1" )`).
fn gerber_plotter(x2: bool, no_macros: bool) -> GerberPlotter {
    let mut plotter = GerberPlotter::new();
    plotter.set_render_settings(Some(RenderSettings::default()));

    if x2 {
        plotter.use_x2format(true);
    }

    if no_macros {
        plotter.disable_apert_macros(true);
    }

    plotter.set_viewport(
        Vector2I::new(0, 0),
        PCB_IU_SCALE.iu_per_mils / 10.0,
        1.0,
        false,
    );
    assert!(plotter.start_plot("1"));
    plotter
}

/// The FAB-04 part of `PlotStandardLayer` for `layers` (see the module comment).
fn plot_items(ctx: &BoardCtx<'_>, plotter: &mut dyn Plotter, layers: &Lset, opts: &PcbPlotParams) {
    let mut itemplotter = BrditemsPlotter::new(plotter, ctx, opts);
    itemplotter.set_layer_set(layers);
    let flags = StandardLayerFlags::new(ctx, layers, opts);
    plot_standard_layer_pads_vias_tracks_zones(&mut itemplotter, &flags);
}

/// Runs the C++ call `f`; `None` while it reaches a pending stub.
fn unless_pending(f: impl FnOnce() -> String) -> Option<String> {
    std::panic::set_hook(Box::new(|_| {}));
    let result = catch_unwind(AssertUnwindSafe(f));
    let _ = std::panic::take_hook();

    match result {
        Ok(text) => Some(text),
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            assert!(msg.contains("port pending"), "unexpected panic: {msg}");
            println!("pending: {msg}");
            None
        }
    }
}

/// Occurrences of `D0*3\*` (a flash).
fn flashes(text: &str) -> usize {
    text.match_indices("D03*").count() + text.match_indices("D003*").count()
}

/// The THT pad of `NoDrillFlashesOnEdgeCuts`.
fn tht_board() -> Board {
    let pos = Vector2I::new(PCB_IU_SCALE.mm_to_iu(50.0), PCB_IU_SCALE.mm_to_iu(50.0));
    let pad_dia = PCB_IU_SCALE.mm_to_iu(1.4);
    let drill = PCB_IU_SCALE.mm_to_iu(0.8);

    board_with_pad(pos, pos, |pad| {
        pad.attribute = PadAttrib::Pth;
        // THT pad: NOT on Edge_Cuts
        let layers = Lset::all_cu_mask().clone()
            | Lset::from_layers(&[PcbLayerId::FMask, PcbLayerId::BMask]);
        set_padstack(
            pad,
            PadShape::Circle,
            Vector2I::new(pad_dia, pad_dia),
            Some(Vector2I::new(drill, drill)),
            layers,
        );
    })
}

/// `NoDrillFlashesOnEdgeCuts` (issue 24416): no drill-mark flash on an Edge_Cuts Gerber even
/// with drill marks requested.
#[test]
fn no_drill_flashes_on_edge_cuts() {
    let board = tht_board();
    let ctx = BoardCtx::bare(&board);
    let mut plot_opts = PcbPlotParams::default();
    plot_opts.format = PlotFormat::Gerber;
    plot_opts.drill_marks = DrillMarks::FullDrillShape; // triggers PlotDrillMarks()

    let full = unless_pending(|| {
        let mut plotter = gerber_plotter(false, false);
        plot_board_layers(&ctx, &mut plotter, &[PcbLayerId::EdgeCuts], &plot_opts);
        assert!(plotter.end_plot());
        plotter.output().to_string()
    });

    if let Some(text) = full {
        assert_eq!(flashes(&text), 0, "Edge_Cuts gerber flashes (#24416)");
    }

    // PlotBoardLayers forces the drill marks off for Gerber
    let mut opts = plot_opts.clone();
    opts.drill_marks = DrillMarks::NoDrillShape;
    let mut plotter = gerber_plotter(false, false);
    plot_items(
        &ctx,
        &mut plotter,
        &Lset::from_layers(&[PcbLayerId::EdgeCuts]),
        &opts,
    );
    assert!(plotter.end_plot());
    assert_eq!(
        flashes(plotter.output()),
        0,
        "Edge_Cuts gerber flashes (#24416)"
    );
}

/// `NoDrillMarksOnGerberFromStoredOptions` (issue 25532): an NPTH mounting hole on F_Cu
/// plots nothing, even with the stored (small) drill marks.
#[test]
fn no_drill_marks_on_gerber_from_stored_options() {
    let pos = Vector2I::new(PCB_IU_SCALE.mm_to_iu(50.0), PCB_IU_SCALE.mm_to_iu(50.0));
    let drill = PCB_IU_SCALE.mm_to_iu(3.2);

    // An NPTH mounting hole plots no copper of its own, so any flash in the file is the drill
    // mark
    let board = board_with_pad(pos, pos, |pad| {
        pad.attribute = PadAttrib::Npth;
        set_padstack(
            pad,
            PadShape::Circle,
            Vector2I::new(drill, drill),
            Some(Vector2I::new(drill, drill)),
            Pad::unplated_hole_mask(),
        );
    });
    let ctx = BoardCtx::bare(&board);
    let mut plot_opts = PcbPlotParams::default();
    plot_opts.format = PlotFormat::Gerber;
    plot_opts.drill_marks = DrillMarks::SmallDrillShape; // what boards store by default

    let full = unless_pending(|| {
        let mut plotter = gerber_plotter(false, false);
        plot_board_layers(&ctx, &mut plotter, &[PcbLayerId::FCu], &plot_opts);
        assert!(plotter.end_plot());
        plotter.output().to_string()
    });

    if let Some(text) = full {
        assert_eq!(flashes(&text), 0, "F_Cu gerber flashes (#25532)");
    }

    // PlotBoardLayers: drill marks off for Gerber; PlotOneBoardLayer: skip NPTH pads on copper
    let mut opts = plot_opts.clone();
    opts.drill_marks = DrillMarks::NoDrillShape;
    opts.skip_npth_pads = true;
    let mut plotter = gerber_plotter(false, false);
    plot_items(
        &ctx,
        &mut plotter,
        &Lset::from_layers(&[PcbLayerId::FCu]),
        &opts,
    );
    assert!(plotter.end_plot());
    assert_eq!(flashes(plotter.output()), 0, "F_Cu gerber flashes (#25532)");
}

/// `RoundRectGerberMaskApertureHasCorrectRadius` (issue 24327): with macros disabled the
/// F_Mask region of a round-rect pad with a 1 mm mask margin has 4 arcs whose radius /
/// bounding box ratio is 0.3125.
#[test]
fn round_rect_gerber_mask_aperture_has_correct_radius() {
    let pad_w = PCB_IU_SCALE.mm_to_iu(2.0);
    let pad_h = PCB_IU_SCALE.mm_to_iu(2.0);
    let mask_clearance = PCB_IU_SCALE.mm_to_iu(1.0);
    let radius_ratio = 0.125;
    let original_radius = kicad_geom::ki_round(f64::from(pad_w.min(pad_h)) * radius_ratio);
    let pos = Vector2I::new(PCB_IU_SCALE.mm_to_iu(50.0), PCB_IU_SCALE.mm_to_iu(50.0));

    let mut board = board_with_pad(pos, pos, |pad| {
        pad.attribute = PadAttrib::Smd;
        set_padstack(
            pad,
            PadShape::Roundrect,
            Vector2I::new(pad_w, pad_h),
            None,
            Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::FMask]),
        );
        pad.set_round_rect_radius_ratio(Padstack::ALL_LAYERS, radius_ratio);
        pad.set_local_solder_mask_margin(Some(mask_clearance));
    });
    board.design_settings_mut().solder_mask_expansion = 0;
    let ctx = BoardCtx::bare(&board);

    let mut plot_opts = PcbPlotParams::default();
    plot_opts.format = PlotFormat::Gerber;
    plot_opts.use_gerber_x2_format = true;
    plot_opts.gerber_disable_apert_macros = true;
    let layers = Lset::from_layers(&[PcbLayerId::FMask]);

    let check = |buffer: &str| {
        // Sample every D02/D01 coordinate to get the plotted region's bounding box, and
        // every arc command (X..Y..I..J..D01) to read the corner radius.
        let mut min = (i64::MAX, i64::MAX);
        let mut max = (i64::MIN, i64::MIN);
        let mut coord_count = 0;
        let mut arcs = Vec::new();

        for line in buffer.lines() {
            let Some(rest) = line.strip_prefix('X') else {
                continue;
            };
            let num = |s: &str| -> (i64, usize) {
                let end = s
                    .char_indices()
                    .find(|&(i, c)| !(c.is_ascii_digit() || (i == 0 && c == '-')))
                    .map_or(s.len(), |(i, _)| i);
                (s[..end].parse().unwrap(), end)
            };
            let (x, n) = num(rest);
            let rest = &rest[n..];
            let Some(rest) = rest.strip_prefix('Y') else {
                continue;
            };
            let (y, n) = num(rest);
            let rest = &rest[n..];

            if rest == "D01*" || rest == "D02*" {
                min = (min.0.min(x), min.1.min(y));
                max = (max.0.max(x), max.1.max(y));
                coord_count += 1;
            } else if let Some(r) = rest.strip_prefix('I') {
                let (i, n) = num(r);
                let r = &r[n..];
                let r = r.strip_prefix('J').unwrap();
                let (j, n) = num(r);

                if &r[n..] == "D01*" {
                    arcs.push((i as f64).hypot(j as f64));
                }
            }
        }

        assert!(coord_count > 0);
        assert_eq!(max.0 - min.0, max.1 - min.1);
        let bbox_extent = max.0 - min.0;
        assert!(bbox_extent > 0);

        // The expected corner radius is (originalRadius + maskClearance), and the expected
        // bounding-box extent is (padW + 2 * maskClearance).
        let expected_radius_ratio =
            f64::from(original_radius + mask_clearance) / f64::from(pad_w + 2 * mask_clearance);

        for r in &arcs {
            let ratio = r / bbox_extent as f64;
            assert!(
                (ratio - expected_radius_ratio).abs() < 1e-3,
                "arc radius / bbox ratio {ratio}, expected {expected_radius_ratio}"
            );
        }

        assert_eq!(arcs.len(), 4);
    };

    let full = unless_pending(|| {
        let mut plotter = gerber_plotter(true, true);
        plot_standard_layer(&ctx, &mut plotter, &layers, &plot_opts);
        assert!(plotter.end_plot());
        plotter.output().to_string()
    });

    if let Some(text) = full {
        check(&text);
    }

    let mut plotter = gerber_plotter(true, true);
    plot_items(&ctx, &mut plotter, &layers, &plot_opts);
    assert!(plotter.end_plot());
    check(plotter.output());
}
