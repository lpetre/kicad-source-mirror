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

//! Port of `common/plotters/common_plot_functions.cpp`: `GetDefaultPlotExtension` (L34-46,
//! with the `GetDefaultFileExtension()` statics of the plotter classes it calls) and
//! `PlotDrawingSheet` (L49-207).

use std::collections::BTreeMap;

use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::page_info::PageInfo;
use kicad_core::plot_enums::{FillT, PlotFormat};
use kicad_core::string_utils::wx_string_split;
use kicad_core::text_vars::{ResolutionContext, TextVarResolver};
use kicad_core::title_block::{TitleBlock, TitleBlockClock};
use kicad_core::base_units::UNITY_SCALE;
use kicad_font::{Metrics, get_font};
use kicad_geom::Vector2I;

use crate::drawing_sheet::{DsDataModel, DsDrawItemList, DsDrawItemShape};
use crate::plotter::Plotter;

/// Port of `GetDefaultPlotExtension`: the default plot extension for a format.
///
/// HPGL and `UNDEFINED` hit `wxFAIL` in C++ and return an empty string.
pub fn get_default_plot_extension(a_format: PlotFormat) -> &'static str {
    match a_format {
        PlotFormat::Dxf => "dxf",    // DXF_PLOTTER::GetDefaultFileExtension
        PlotFormat::Post => "ps",    // PS_PLOTTER::GetDefaultFileExtension
        PlotFormat::Pdf => "pdf",    // PDF_PLOTTER::GetDefaultFileExtension
        PlotFormat::Gerber => "gbr", // GERBER_PLOTTER::GetDefaultFileExtension
        PlotFormat::Svg => "svg",    // SVG_PLOTTER::GetDefaultFileExtension
        PlotFormat::Png => "png",    // PNG_PLOTTER::GetDefaultFileExtension
        PlotFormat::Hpgl | PlotFormat::Undefined => "",
    }
}

/// The arguments of `PlotDrawingSheet` besides the plotter, and what the C++ reads from
/// globals.
#[derive(Clone)]
pub struct PlotDrawingSheetArgs<'a> {
    /// `aProject`: the project's `TextVarResolver` (`None`: no project)
    pub project: Option<&'a TextVarResolver<'a>>,
    /// `aTitleBlock`
    pub title_block: &'a TitleBlock,
    /// `aPageInfo`
    pub page_info: &'a PageInfo,
    /// `aProperties`
    pub properties: Option<&'a BTreeMap<String, String>>,
    /// `aSheetNumber`
    pub sheet_number: &'a str,
    /// `aSheetCount`
    pub sheet_count: i32,
    /// `aSheetName`
    pub sheet_name: &'a str,
    /// `aSheetPath`
    pub sheet_path: &'a str,
    /// `aFilename`
    pub filename: &'a str,
    /// `aColor`
    pub color: Color4D,
    /// `aIsFirstPage`
    pub is_first_page: bool,
    /// `aVariantName`
    pub variant_name: &'a str,
    /// `aVariantDesc`
    pub variant_desc: &'a str,
    /// `plotter->RenderSettings()->GetLayerName()` (the port's `RenderSettings` has no layer
    /// name)
    pub layer_name: &'a str,
    /// `GetBaseVersion()` for `${KICAD_VERSION}` (`None`: no `PGM_BASE`, the variable stays
    /// unresolved)
    pub kicad_version: Option<&'a str>,
    /// The clock texts of `${CURRENT_DATE}` and the `CURRENT_TIME_*` variables
    pub clock: &'a TitleBlockClock,
}

/// Why [`plot_drawing_sheet`] could not plot the sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlotDrawingSheetError {
    /// A part of the sheet the port cannot plot yet (bitmaps in SVG, bitmaps of unknown
    /// size).
    NotPorted(String),
}

impl std::fmt::Display for PlotDrawingSheetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlotDrawingSheetError::NotPorted(what) => write!(f, "not ported yet: {what}"),
        }
    }
}

impl std::error::Error for PlotDrawingSheetError {}

/// Port of `PlotDrawingSheet( PLOTTER* plotter, const PROJECT* aProject, const TITLE_BLOCK&
/// aTitleBlock, const PAGE_INFO& aPageInfo, const std::map<wxString, wxString>* aProperties,
/// const wxString& aSheetNumber, int aSheetCount, const wxString& aSheetName, const wxString&
/// aSheetPath, const wxString& aFilename, COLOR4D aColor, bool aIsFirstPage, const wxString&
/// aVariantName, const wxString& aVariantDesc )`: plot the drawing sheet `model` (the C++
/// singleton `DS_DATA_MODEL::GetTheInstance()`) for one page.
///
/// Drawing sheet texts are plotted with the stroke font: a text asking for an outline font
/// (`(font (face ..))`) is recorded in `PlotterState::outline_font_fallbacks`. Bitmaps are
/// plotted as the base `PLOTTER::PlotImage` rectangle where the plotter does that (Gerber);
/// an SVG plot of a bitmap (base64 PNG re-encoded by wxImage) is not ported.
pub fn plot_drawing_sheet(
    plotter: &mut dyn Plotter,
    model: &mut DsDataModel,
    args: &PlotDrawingSheetArgs<'_>,
) -> Result<(), PlotDrawingSheetError> {
    // Note: Page sizes values are given in mils
    let ius_per_mil = plotter.ius_per_decimil() * 10.0;
    let mut plot_color = if plotter.color_mode() {
        args.color.clone()
    } else {
        Color4D::BLACK
    };
    let default_pen_width = plotter
        .render_settings()
        .map_or(0, |rs| rs.default_pen_width());

    if plot_color == Color4D::UNSPECIFIED {
        plot_color = Color4D::from_eda_color(EdaColorT::RED);
    }

    let mut draw_list = DsDrawItemList::new(UNITY_SCALE, ResolutionContext::ForCanvas);

    // Print only a short filename, if aFilename is the full filename
    // (wxFileName( aFilename ).GetFullPath())

    // Prepare plot parameters
    draw_list.set_default_pen_size(default_pen_width);
    draw_list.set_plotter_mils_to_iu_factor(ius_per_mil);
    draw_list.set_page_number(args.sheet_number);
    draw_list.set_sheet_count(args.sheet_count);
    draw_list.set_file_name(args.filename);
    draw_list.set_sheet_name(args.sheet_name);
    draw_list.set_sheet_path(args.sheet_path);
    draw_list.set_sheet_layer(args.layer_name);
    draw_list.set_project(args.project);
    draw_list.set_is_first_page(args.is_first_page);
    draw_list.set_properties(args.properties);
    draw_list.set_variant_name(args.variant_name);
    draw_list.set_variant_desc(args.variant_desc);
    draw_list.set_kicad_version(args.kicad_version);
    draw_list.set_clock(args.clock);

    draw_list.build_draw_items_list(model, args.page_info, args.title_block);

    // Draw bitmaps first
    for item in draw_list.items() {
        if let DsDrawItemShape::Bitmap(draw_item) = &item.shape {
            let Some(bitmap) = model.item(item.peer.peer as u32).and_then(|i| i.bitmap()) else {
                continue;
            };

            // BITMAP_BASE::PlotImage( plotter, pos, plotColor, defaultPenWidth )
            if !bitmap.has_image {
                continue;
            }

            if plotter.plotter_type() != PlotFormat::Gerber {
                return Err(PlotDrawingSheetError::NotPorted(
                    "drawing sheet bitmap in a plot that embeds images (SVG)".to_string(),
                ));
            }

            let Some(info) = bitmap.image_info else {
                return Err(PlotDrawingSheetError::NotPorted(
                    "drawing sheet bitmap of unknown image size (no image probe)".to_string(),
                ));
            };

            // These 2 lines are useful only for plotters that cannot plot a bitmap
            // and plot a rectangle instead of.
            plotter.set_color(&plot_color);
            plotter.set_current_line_width(default_pen_width, None);
            plotter.plot_image(
                info.width,
                info.height,
                draw_item.pos,
                bitmap.scaling_factor(),
                1.0,
            );
        }
    }

    // Draw other items
    for item in draw_list.items() {
        if matches!(item.shape, DsDrawItemShape::Bitmap(_)) {
            continue;
        }

        plotter.set_color(&plot_color);

        match &item.shape {
            DsDrawItemShape::Line(line) => {
                plotter.set_current_line_width(line.get_pen_width().max(default_pen_width), None);
                plotter.move_to(line.start);
                plotter.finish_to(line.end);
            }

            DsDrawItemShape::Rect(rect) => {
                plotter.set_current_line_width(rect.get_pen_width().max(default_pen_width), None);
                plotter.move_to(rect.start);
                plotter.line_to(Vector2I::new(rect.end.x, rect.start.y));
                plotter.line_to(Vector2I::new(rect.end.x, rect.end.y));
                plotter.line_to(Vector2I::new(rect.start.x, rect.end.y));
                plotter.finish_to(rect.start);
            }

            DsDrawItemShape::Text(text) => {
                // text->GetDrawFont( settings ): the default font of the render settings
                // (the plotter resolves it); an outline face falls back to it.
                if let Some(face) = &text.font
                    && get_font(face, text.bold, text.italic, None, false).is_outline_unavailable()
                {
                    plotter.state_mut().outline_font_fallbacks.push(face.clone());
                }

                let mut color = plot_color.clone();
                let shown_text = text.get_shown_text(ResolutionContext::ForCanvas);

                if plotter.color_mode() && text.color != Color4D::UNSPECIFIED {
                    color = text.color.clone();
                }

                let pen_width = text.effective_text_pen_width(0).max(default_pen_width);

                // Some plotters (PDF plotter) do not handle multiline very well. So handle
                // them here
                if text.multiline_allowed && shown_text.contains('\n') {
                    let strings_list = wx_string_split(&shown_text, '\n');
                    let positions = text.line_positions(strings_list.len() as i32);

                    for (ii, txt) in strings_list.iter().enumerate() {
                        plotter.text(
                            positions[ii],
                            &color,
                            txt,
                            text.angle,
                            text.size,
                            text.h_justify,
                            text.v_justify,
                            pen_width,
                            text.italic,
                            text.bold,
                            false,
                            None,
                            Metrics::default_metrics(),
                            None,
                        );
                    }
                } else {
                    plotter.text(
                        text.pos,
                        &color,
                        &shown_text,
                        text.angle,
                        text.size,
                        text.h_justify,
                        text.v_justify,
                        pen_width,
                        text.italic,
                        text.bold,
                        text.multiline_allowed,
                        None,
                        Metrics::default_metrics(),
                        None,
                    );
                }
            }

            DsDrawItemShape::PolyPolygons(poly) => {
                let pen_width = poly.get_pen_width().max(default_pen_width);
                let mut points: Vec<Vector2I> = Vec::new();

                for idx in 0..poly.polygons.outline_count() {
                    points.clear();
                    let outline = poly.polygons.c_outline(idx);

                    for ii in 0..outline.point_count() {
                        let p = outline.c_point(ii);
                        points.push(Vector2I::new(p.x, p.y));
                    }

                    plotter.plot_poly(&points, FillT::FilledShape, pen_width, None);
                }
            }

            DsDrawItemShape::Bitmap(_) => {}
        }
    }

    Ok(())
}
