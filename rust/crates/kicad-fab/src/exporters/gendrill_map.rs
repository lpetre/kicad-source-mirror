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

//! Port of `pcbnew/exporters/gendrill_writer_base.cpp` L506-847 (`genDrillMapFile`) and
//! L996-1019 (`plotDrillMarks`): drill map drawings (work package FAB-12).
//!
//! A drill map is the board outline (the `Edge.Cuts` shapes of the board and of the
//! footprints, plotted as `Dwgs.User` shapes), one drill symbol (`PLOTTER::Marker`) per hole of
//! the current hole list (plus the outline of slots) and a legend listing every tool with its
//! symbol, diameter in mm and inch and hole / slot counts.
//!
//! The map formats whose plot engines exist in `kicad-plot` are ported: Gerber X2 and SVG.
//! PDF, PostScript and DXF (and HPGL, which C++ maps to PDF) return
//! [`FabError::Unsupported`] naming the format. The build version and the creation date the
//! C++ plotters read from the environment come in through [`PlotEnv`]. The C++ board edge
//! bounding box is computed with `Edge.Cuts` made visible temporarily; the port does not
//! touch the board: `GetBoardEdgesBoundingBox` adds `Edge.Cuts` to the visible layers anyway.

use kicad_core::color4d::Color4D;
use kicad_core::page_info::{CustomPageSize, PageInfo, PageSizeType};
use kicad_core::plot_enums::PlotFormat;
use kicad_core::printf::format_f;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_core::{KicadT, Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_font::Metrics;
use kicad_geom::cmath::f64_to_i32_x86;
use kicad_geom::math::vector2::ki_round_xy;
use kicad_geom::{Vector2I, ki_round};
use kicad_pcb::{AnyItem, MutEnv};
use kicad_pcb::item::DetachedCopy;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_plot::{GerberPlotter, Plotter, RenderSettings, SvgPlotter, USE_DEFAULT_LINE_WIDTH};

use super::gendrill_report::fmt_fixed_width;
use super::gendrill_writer_base::{
    GendrillWriterBase, diameter_in_inches, diameter_in_mm, get_default_pen_size,
    get_marker_best_pen_size, get_sketch_oval_best_pen_size,
};
use crate::FabError;
use crate::color_settings::ColorSettings;
use crate::drill::HoleAttribute;
use crate::pcbplot::{PlotEnv, add_gerber_x2_header};
use crate::plot_brditems_plotter::BrditemsPlotter;

/// The `FabError::Unsupported` of a map format without a plot engine in the port.
fn unsupported_map_format(name: &str) -> FabError {
    FabError::Unsupported(format!(
        "drill map format {name} is not part of the port (Gerber X2 and SVG only)"
    ))
}

/// Port of `GENDRILL_WRITER_BASE::genDrillMapFile( aFullFileName, aFormat )`: plot a map of
/// drill marks for the current hole list (built by `build_holes_list`) and return the file
/// contents.
///
/// `env` supplies the build version and the creation date of the Gerber header and of the
/// SVG title. C++ returns `false` when the file cannot be opened (cannot happen here) or
/// `EndPlot` fails; the port returns `Err(FabError::Unsupported)` for the formats it has no
/// plot engine for (PDF, PostScript, DXF, HPGL).
pub fn gen_drill_map_file(
    writer: &GendrillWriterBase<'_>,
    full_file_name: &str,
    format: PlotFormat,
    env: &PlotEnv,
) -> Result<Vec<u8>, FabError> {
    // Remark:
    // Hole list must be created before calling this function, by buildHolesList(),
    // for the right holes set (PTH, NPTH, buried/blind vias ...)

    let mut scale: f64 = 1.0;
    let mut offset: Vector2I = writer.offset();
    let dummy = PageInfo::new(PageSizeType::A4, false, &CustomPageSize::default());
    let mut bottom_limit: i32 = 0; // Y coord limit of page. 0 mean do not use

    let plot_opts = PcbPlotParams::default(); // starts plotting with default options

    let page_info: &PageInfo = writer.page_info.as_ref().unwrap_or(&dummy);

    let ctx = &writer.pcb;
    let board = ctx.board;

    // Calculate dimensions and center of PCB. The Edge_Cuts layer must be visible
    // to calculate the board edges bounding box
    // (C++ adds Edge_Cuts to the visible layers around the call; `ComputeBoundingBox(
    // true, true )` sets it in its own copy of the visible layers anyway.)
    let bbbox = board.board_edges_bounding_box(ctx);

    let mut format = format;

    // Some formats cannot be used to generate a document like the map files
    // Currently HPGL (old format not very used)
    if format == PlotFormat::Hpgl {
        format = PlotFormat::Pdf;
    }

    // Calculate the scale for the format type, scale 1 in HPGL, drawing on
    // an A4 sheet in PS, + text description of symbols
    let mut plotter: Box<dyn Plotter> = match format {
        PlotFormat::Gerber => {
            let mut gbr = GerberPlotter::new();
            gbr.set_build_version(&env.build_version);
            gbr.set_creation_date(&env.gerber_comment_date());
            gbr.set_viewport(offset, PCB_IU_SCALE.iu_per_mils / 10.0, scale, false);
            gbr.set_gerber_coordinates_format(5, false); // format x.5 unit = mm
            Box::new(gbr)
        }

        PlotFormat::Dxf => return Err(unsupported_map_format("DXF")),

        // PDF, POST, SVG; default: wxASSERT( false ); KI_FALLTHROUGH; (to PDF)
        _ => {
            let page_size_iu = page_info.size_iu(PCB_IU_SCALE.iu_per_mils);
            // VECTOR2I pageSizeIU (the values are whole numbers)
            let page_size_iu = Vector2I::new(page_size_iu.0 as i32, page_size_iu.1 as i32);

            // Reserve a 10 mm margin around the page.
            let margin = PCB_IU_SCALE.mm_to_iu(10.0);

            // Calculate a scaling factor to print the board on the sheet
            let xscale = f64::from(page_size_iu.x - (2 * margin)) / bbbox.width() as f64;

            // We should print the list of drill sizes, so reserve room for it
            // 60% height for board 40% height for list
            let ypagesize_for_board: i32 = ki_round(f64::from(page_size_iu.y) * 0.6);
            let yscale = f64::from(ypagesize_for_board - margin) / bbbox.height() as f64;

            scale = xscale.min(yscale);

            // Experience shows the scale should not to large, because texts
            // create problem (can be to big or too small).
            // So the scale is clipped at 3.0;
            scale = scale.min(3.0);

            offset.x = ki_round(
                f64::from(bbbox.centre().x) - (f64::from(page_size_iu.x) / 2.0) / scale,
            );
            offset.y = ki_round(
                f64::from(bbbox.centre().y) - (f64::from(ypagesize_for_board) / 2.0) / scale,
            );

            // bottom_limit is used to plot the legend (drill diameters)
            // texts are scaled differently for scale > 1.0 and <= 1.0
            // so the limit is scaled differently.
            bottom_limit = f64_to_i32_x86(f64::from(page_size_iu.y - margin) / scale.min(1.0));

            let mut plotter: Box<dyn Plotter> = match format {
                PlotFormat::Svg => {
                    let mut svg = SvgPlotter::new();
                    svg.set_creation_date(&env.iso8601_date());
                    Box::new(svg)
                }
                PlotFormat::Post => return Err(unsupported_map_format("PostScript")),
                _ => return Err(unsupported_map_format("PDF")),
            };

            plotter.set_page_settings(page_info);
            plotter.set_viewport(offset, PCB_IU_SCALE.iu_per_mils / 10.0, scale, false);
            plotter
        }
    };

    plotter.set_creator("PCBNEW");
    plotter.set_color_mode(false);

    // KIGFX::PCB_RENDER_SETTINGS renderSettings;
    let mut render_settings = RenderSettings::default();
    render_settings.set_default_pen_width(get_default_pen_size());

    plotter.set_render_settings(Some(render_settings));

    plotter.open_file(full_file_name);

    plotter.clear_header_lines_list();

    // For the Gerber X2 format we need to set the  "FileFunction" to Drillmap
    // and set a few other options.
    if plotter.plotter_type() == PlotFormat::Gerber {
        let gbrplotter = plotter.as_gerber_mut().expect("a GERBER_PLOTTER");
        gbrplotter.disable_apert_macros(false);
        gbrplotter.use_x2format(true); // Mandatory
        gbrplotter.use_x2_net_attributes(false); // net attributes have no meaning here

        // Attributes are added using X2 format
        add_gerber_x2_header(gbrplotter, board, env, false);

        // Add the TF.FileFunction
        gbrplotter.add_line_to_header("%TF.FileFunction,Drillmap*%");

        // Add the TF.FilePolarity
        gbrplotter.add_line_to_header("%TF.FilePolarity,Positive*%");
    }

    plotter.start_plot("1");

    {
        // Draw items on edge layer.
        // Not all, only items useful for drill map, i.e. board outlines.
        let mut itemplotter = BrditemsPlotter::new(
            plotter.as_mut(),
            ctx,
            &plot_opts,
            ColorSettings::default(),
        );

        // Use attributes of a drawing layer (we are not really draw the Edge.Cuts layer)
        itemplotter.set_layer_set(&Lset::from_layers(&[PcbLayerId::DwgsUser]));

        let env_mut = MutEnv::detached();

        // Plots `PCB_SHAPE dummy_shape( *item )` on Dwgs_User, without its group.
        let plot_edge_shape = |itemplotter: &mut BrditemsPlotter<'_, '_>, item: AnyItem<'_>| {
            let item = ctx.view(item);

            if item.layer() != PcbLayerId::EdgeCuts {
                return;
            }

            if item.type_() == KicadT::PcbShapeT {
                let mut dummy_shape = item.as_shape().expect("PCB_SHAPE").detached_copy();
                dummy_shape.set_layer(&env_mut, PcbLayerId::DwgsUser);
                // Remove group association, not needed for plotting
                dummy_shape.base.item.group = None;
                itemplotter.plot_shape(ctx.view(AnyItem::Shape(&dummy_shape)));
            }
        };

        for (_, item) in board.drawings() {
            plot_edge_shape(&mut itemplotter, item);
        }

        // Plot edge cuts in footprints
        for (fp, _) in board.footprints() {
            for (_, item) in board.footprint_drawings(fp) {
                plot_edge_shape(&mut itemplotter, item);
            }
        }
    }

    let mut plot_x: i32;
    let mut plot_y: i32;
    let text_width: i32;
    let mut intervalle: i32;
    let textmarginaftersymbol = PCB_IU_SCALE.mm_to_iu(2.0);

    // Set Drill Symbols width
    plotter.set_current_line_width(-1, None);

    // Plot board outlines and drill map
    plot_drill_marks(writer, plotter.as_mut());

    // Print a list of symbols used.
    let char_size = PCB_IU_SCALE.mm_to_iu(2.0); // text size in IUs

    // real char scale will be 1/scale, because the global plot scale is scale
    // for scale < 1.0 ( plot bigger actual size)
    // Therefore charScale = 1.0 / scale keep the initial charSize
    // (for scale < 1 we use the global scaling factor: the board must be plotted
    // smaller than the actual size)
    let char_scale = 1.0_f64.min(1.0 / scale);
    let char_size_d = f64::from(char_size) * char_scale;

    text_width = ki_round(char_size_d / 10.0); // Set text width (thickness)
    intervalle = ki_round(char_size_d) + text_width;

    // Trace information.
    plot_x = ki_round(f64::from(bbbox.x()) + f64::from(textmarginaftersymbol) * char_scale);
    plot_y = bbbox.bottom() + intervalle;

    // Plot title  "Info"
    let text = "Drill Map:";

    let attrs = TextAttributes {
        stroke_width: text_width,
        angle: 0.0, // ANGLE_HORIZONTAL
        size: {
            let s = ki_round_xy(char_size_d, char_size_d);
            (s.x, s.y)
        },
        halign: GrTextHAlignT::Left,
        valign: GrTextVAlignT::Center,
        multiline: false,
        ..TextAttributes::default()
    };

    let metrics = Metrics::default_metrics();

    plotter.plot_text(
        Vector2I::new(plot_x, plot_y),
        &Color4D::UNSPECIFIED,
        text,
        &attrs,
        None, // stroke font
        metrics,
        None,
    );

    // For some formats (PS, PDF SVG) we plot the drill size list on more than one column
    // because the list must be contained inside the printed page
    // (others formats do not have a defined page size)
    let mut max_line_len: i32 = 0; // The max line len in iu of the currently plotted column

    for (ii, tool) in writer.tool_list_buffer.iter().enumerate() {
        if tool.total_count == 0 {
            continue;
        }

        plot_y += intervalle;

        // Ensure there are room to plot the line
        if bottom_limit != 0 && (plot_y + intervalle > bottom_limit) {
            plot_y = bbbox.bottom() + intervalle;
            plot_x += max_line_len + PCB_IU_SCALE.mm_to_iu(10.0); //column_width;
            max_line_len = 0;
        }

        let mut plot_diam: i32 = tool.diameter;

        // For markers plotted with the comment, keep marker size <= text height
        plot_diam = plot_diam.min(ki_round(char_size_d));
        let x: i32 = ki_round(
            f64::from(plot_x)
                - f64::from(textmarginaftersymbol) * char_scale
                - f64::from(plot_diam) / 2.0,
        );
        let y: i32 = ki_round(f64::from(plot_y) + char_size_d);

        plotter.set_current_line_width(get_marker_best_pen_size(plot_diam), None);
        plotter.marker(Vector2I::new(x, y), plot_diam, ii as u32);
        plotter.set_current_line_width(-1, None);

        // List the diameter of each drill in mm and inches.
        let mut msg = format!(
            "{}mm / {}\" ",
            fmt_fixed_width(diameter_in_mm(f64::from(tool.diameter)), 3, 3),
            fmt_fixed_width(diameter_in_inches(f64::from(tool.diameter)), 2, 4)
        );

        let mut extra_info = String::new();

        if tool.hole_attribute == HoleAttribute::HolePadCastellated {
            extra_info.push_str(", castellated");
        } else if tool.hole_attribute == HoleAttribute::HolePadPressfit {
            extra_info.push_str(", press-fit");
        }

        if tool.is_backdrill {
            if let Some(min_stub_length) = tool.min_stub_length {
                let min_stub = PCB_IU_SCALE.iu_to_mm(min_stub_length);

                match tool.max_stub_length {
                    Some(max_stub_length) if tool.max_stub_length != tool.min_stub_length => {
                        let max_stub = PCB_IU_SCALE.iu_to_mm(max_stub_length);
                        extra_info.push_str(&format!(
                            ", backdrill stub {}-{}mm",
                            format_f(min_stub, 3),
                            format_f(max_stub, 3)
                        ));
                    }
                    _ => {
                        extra_info.push_str(&format!(", backdrill stub {}mm", format_f(min_stub, 3)));
                    }
                }
            } else {
                extra_info.push_str(", backdrill");
            }
        }

        if tool.has_post_machining {
            extra_info.push_str(", post-machined");
        }

        let counts = if tool.total_count == 1 && tool.oval_count == 0 {
            format!("(1 hole{extra_info})")
        } else if tool.total_count == 1 {
            format!("(1 slot{extra_info})")
        } else if tool.oval_count == 0 {
            format!("({} holes{extra_info})", tool.total_count)
        } else if tool.oval_count == 1 {
            format!("({} holes + 1 slot{extra_info})", tool.total_count - 1)
        } else {
            format!(
                "({} holes + {} slots{extra_info})",
                tool.total_count - tool.oval_count,
                tool.oval_count
            )
        };

        msg.push_str(&counts);

        // Backdrills carry the non-plated flag but are already called out as backdrills above
        if tool.hole_not_plated && !tool.is_backdrill {
            msg.push_str(" (not plated)");
        }

        plotter.plot_text(
            Vector2I::new(plot_x, y),
            &Color4D::UNSPECIFIED,
            &msg,
            &attrs,
            None, // stroke font
            metrics,
            None,
        );

        intervalle = ki_round((char_size_d + f64::from(text_width)) * 1.2);

        let min_intervalle = f64::from(plot_diam)
            + (1.0 * PCB_IU_SCALE.iu_per_mm / scale)
            + f64::from(text_width);

        if f64::from(intervalle) < min_intervalle {
            intervalle = f64_to_i32_x86(min_intervalle);
        }

        // Evaluate the text horizontal size, to know the maximal column size
        // This is a rough value, but ok to create a new column to plot next texts
        let text_len =
            f64_to_i32_x86(msg.chars().count() as f64 * (char_size_d + f64::from(text_width)));
        max_line_len = max_line_len.max(text_len + plot_diam);
    }

    plotter.end_plot();

    Ok(plotter.output().as_bytes().to_vec())
}

/// Port of `GENDRILL_WRITER_BASE::plotDrillMarks( aPlotter )`: one drill symbol per hole of
/// the current hole list (the tool's marker shape), plus the outline of slots.
pub fn plot_drill_marks(writer: &GendrillWriterBase<'_>, a_plotter: &mut dyn Plotter) -> bool {
    // Plot the drill map:
    for (ii, hole) in writer.hole_list_buffer.iter().enumerate() {
        // Gives a good line thickness to have a good marker shape:
        a_plotter.set_current_line_width(get_marker_best_pen_size(hole.diameter), None);

        // Always plot the drill symbol (for slots identifies the needed cutter!
        // (C++ passes the int `m_holeToolReferences[ii] - 1` to an `unsigned` parameter.)
        a_plotter.marker(
            hole.position,
            hole.diameter,
            (writer.hole_tool_references[ii] - 1) as u32,
        );

        if hole.is_slot {
            a_plotter.thick_oval(
                hole.position,
                hole.size_xy,
                hole.orientation,
                get_sketch_oval_best_pen_size(),
                None,
            );
        }
    }

    a_plotter.set_current_line_width(USE_DEFAULT_LINE_WIDTH, None);

    true
}
