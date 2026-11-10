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

//! Port of `pcbnew/plot_brditems_plotter.cpp`: `BRDITEMS_PLOTTER` struct, `getColor`, pads, zones,
//! drill marks.
//!
//! FAB-03 defines the struct (`pcbplot.h` L61-162) with its options and the inline members;
//! FAB-04 ports `getColor`, `PlotPadNumber`, `PlotPad` (L72-398) and `PlotZone` (L902-957).
//! FAB-05 ports the graphic items in `plot_brditems_shapes.rs` (an `impl` block of this
//! struct). The other per-item entry points are pending functions that name the work package
//! porting them (FAB-11 drill marks, FAB-13 drill symbols); text (FAB-06) is in
//! `plot_brditems_text.rs`. The pending ones panic with a "port pending" message until
//! that package lands.

use kicad_core::color4d::{Color4D, EdaColorT};
use kicad_core::plot_enums::{DrillMarks, DxfOutlineMode, FillT, PlotFormat};
use kicad_core::string_utils::{printable_char_count, unescape_string};
use kicad_core::printf::format_f;
use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_core::{Lset, PCB_IU_SCALE, PcbLayerId};
use kicad_font::Metrics;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::trigo::rotate_point;
use kicad_geom::{ANGLE_0, ANGLE_90, EdaAngle, ShapePolySet, Vector2I, ki_round};
use kicad_pcb::AnyItem;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drill::{
    DrillMarkMode, DrillQuery, DrillSymbolProfile, drill_symbol_cache, enumerate_drill_operations,
    enumerate_drill_spans,
};
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::item_ref::ItemRef;
use kicad_pcb::padstack::{PadAttrib, PadDrillShape, PadProp, PadShape, Padstack};
use kicad_pcb::pcb_drill_map::PcbDrillMap;
use kicad_pcb::pcb_generated_table::GeneratedTableKind;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::traits::EdaShapeItem;
use kicad_plot::drill_markers::curated_shape;
use kicad_plot::{PlotData, Plotter, USE_DEFAULT_LINE_WIDTH};
use kicad_plot::gbr_metadata::{GbrApertureAttrib, GbrMetadata};
use kicad_plot::gbr_netlist_metadata::gbr_netinfo;

use crate::color_settings::ColorSettings;
use crate::plot_board_layers::PadPlotState;

/// Panic with a "port pending" message naming the C++ function and the work package that
/// ports it (the plotting counterpart of `kicad_pcb::port_pending!`).
macro_rules! fab_port_pending {
    ($cxx:literal $(, $arg:expr)* $(,)?) => {{
        $( let _ = &$arg; )*
        panic!(concat!("kicad-fab: port pending: ", $cxx))
    }};
}

pub(crate) use fab_port_pending;

/// Port of `BRDITEMS_PLOTTER` (`pcbplot.h`): a helper class to plot board items.
///
/// The C++ class *is-a* `PCB_PLOT_PARAMS` (a copy of the options, `opts` here) and holds the
/// plotter, the board and the layers being plotted. One instance is created per call of
/// `PlotStandardLayer`, `PlotPolySet` and `PlotBoardLayers` (drill marks); it has no caches.
/// The C++ member functions are `const` but draw through the plotter pointer; the Rust
/// methods take `&mut self` for the plotter borrow.
pub struct BrditemsPlotter<'a, 'p> {
    /// The `PCB_PLOT_PARAMS` base class.
    pub opts: PcbPlotParams,
    /// `m_plotter`
    pub plotter: &'p mut dyn Plotter,
    /// `m_board` (the board and its hooks: connectivity for pad flashing, rules for margins).
    pub ctx: &'a BoardCtx<'a>,
    /// `m_layerMask`: the layers being plotted (`SetLayerSet`).
    pub layer_mask: Lset,
    /// `ColorSettings()` of the `PCB_PLOT_PARAMS` base (the port's `PcbPlotParams` has no
    /// colour settings; the caller supplies them).
    pub colors: ColorSettings,
    /// Not a C++ member: the board's pads as the `plotPadLayer` calls of the running plot job
    /// left them (C++ modifies the board's pads in place, see [`PadPlotState`]). `None` (the
    /// default of [`BrditemsPlotter::new`]) gives every `PlotStandardLayer` call its own
    /// state; [`crate::plot_board_layers::plot_standard_layer`] installs the job's.
    pub pad_state: Option<&'a PadPlotState>,
}

impl<'a, 'p> BrditemsPlotter<'a, 'p> {
    /// `BRDITEMS_PLOTTER( PLOTTER* aPlotter, const BOARD* aBoard, const PCB_PLOT_PARAMS& )`;
    /// `a_colors` is the `ColorSettings()` of the options.
    pub fn new(
        a_plotter: &'p mut dyn Plotter,
        a_board: &'a BoardCtx<'a>,
        a_plot_opts: &PcbPlotParams,
        a_colors: ColorSettings,
    ) -> Self {
        BrditemsPlotter {
            opts: a_plot_opts.clone(),
            plotter: a_plotter,
            ctx: a_board,
            layer_mask: Lset::new(),
            colors: a_colors,
            pad_state: None,
        }
    }

    /// Port of `BRDITEMS_PLOTTER::getFineWidthAdj`: the PostScript width adjustment, 0 for
    /// every other format.
    pub fn fine_width_adj(&self) -> i32 {
        if self.opts.format == PlotFormat::Post {
            self.opts.width_adjust
        } else {
            0
        }
    }

    /// Port of `BRDITEMS_PLOTTER::SetLayerSet`.
    pub fn set_layer_set(&mut self, a_layer_mask: &Lset) {
        self.layer_mask = a_layer_mask.clone();
    }

    /// Port of `BRDITEMS_PLOTTER::hideDNPItems` (private in C++).
    pub fn hide_dnp_items(&self, a_layer: PcbLayerId) -> bool {
        self.opts.hide_dnp_fps_on_fab_layers
            && (a_layer == PcbLayerId::FFab || a_layer == PcbLayerId::BFab)
    }

    /// Port of `BRDITEMS_PLOTTER::crossoutDNPItems` (private in C++).
    pub fn crossout_dnp_items(&self, a_layer: PcbLayerId) -> bool {
        self.opts.crossout_dnp_fps_on_fab_layers
            && (a_layer == PcbLayerId::FFab || a_layer == PcbLayerId::BFab)
    }

    /// Port of `BRDITEMS_PLOTTER::PlotZone`: plot the outlines of `a_polys_list` (the fill of
    /// `a_zone` on `a_layer`, or a part of it) as filled polygons; Gerber regions carry the
    /// zone's net attributes. `a_zone` may be a detached copy (`ZONE dummy`).
    pub fn plot_zone(
        &mut self,
        a_zone: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_polys_list: &ShapePolySet,
    ) {
        if a_polys_list.is_empty() {
            return;
        }

        let mut gbr_metadata = GbrMetadata::new();
        let zone = a_zone.as_zone().expect("PlotZone: a zone");

        if zone.is_on_copper_layer(a_zone.ctx) {
            gbr_metadata.set_net_name(a_zone.netname());
            gbr_metadata.set_copper(true);

            // Zones with no net name can exist.
            // they are not used to connect items, so the aperture attribute cannot
            // be set as conductor
            if a_zone.netname().is_empty() {
                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Nonconductor);
            } else {
                gbr_metadata.set_aperture_attrib(GbrApertureAttrib::Conductor);
                gbr_metadata.set_net_attrib_type(gbr_netinfo::NET);
            }
        }

        let color = self.get_color(a_layer.0);
        self.plotter.set_color(&color);

        self.plotter.start_block(None); // Clean current object attributes

        /*
         * In non filled mode the outline is plotted, but not the filling items
         */

        for idx in 0..a_polys_list.outline_count() {
            let outline = a_polys_list.c_outline(idx);

            // Plot the current filled area (as region for Gerber plotter to manage attributes)
            if self.plotter.plotter_type() == PlotFormat::Gerber {
                self.plotter
                    .as_gerber_mut()
                    .expect("a Gerber plotter")
                    .plot_gerber_region_chain(outline, Some(&gbr_metadata));
            } else if self.plotter.plotter_type() == PlotFormat::Dxf {
                fab_port_pending!("BRDITEMS_PLOTTER::PlotZone: DXF (out of scope)")
            } else {
                // getMetadata(): nullptr for every engine but Gerber and DXF
                self.plotter
                    .plot_poly_chain(outline, FillT::FilledShape, 0, None);
            }
        }

        self.plotter.end_block(None); // Clear object attributes
    }

    /// Port of `BRDITEMS_PLOTTER::PlotPad`: plot a pad (flashed, or outlined in sketch mode)
    /// on `a_layer` with its Gerber attributes. `a_pad` may be a detached copy (`PAD dummy`,
    /// or the pad with the sizes `plotPadLayer` set).
    pub fn plot_pad(
        &mut self,
        a_pad: ItemRef<'_>,
        a_layer: PcbLayerId,
        a_color: &Color4D,
        a_sketch_mode: bool,
    ) {
        let ctx = a_pad.ctx;
        let pad = a_pad.as_pad().expect("PlotPad: a pad");
        let shape_pos = pad.shape_pos(ctx, a_layer);
        let mut metadata = GbrMetadata::new();

        // m_plotter->MarkFootprintPlotted( aPad->GetParentFootprint() ): PDF only (out of
        // scope).

        let plot_on_copper_layer = (Lset::all_cu_mask() & &self.layer_mask).any();
        let plot_on_external_copper_layer = (Lset::external_cu_mask() & &self.layer_mask).any();

        // Pad not on the solder mask layer cannot be soldered.
        // therefore it can have a specific aperture attribute.
        // Not yet in use.
        // bool isPadOnBoardTechLayers = ( aPad->GetLayerSet() & LSET::AllBoardTechMask() ).any();

        let reference = pad
            .parent_footprint(ctx)
            .expect("PlotPad: a footprint pad")
            .reference(ctx);
        metadata.set_cmp_reference(&reference);

        if plot_on_copper_layer {
            metadata.set_net_attrib_type(gbr_netinfo::ALL);
            metadata.set_copper(true);

            // Gives a default attribute, for instance for pads used as tracks in net ties:
            // Connector pads and SMD pads are on external layers
            // if on internal layers, they are certainly used as net tie
            // and are similar to tracks: just conductor items
            metadata.set_aperture_attrib(GbrApertureAttrib::Conductor);

            let use_utf8 = false;
            let use_quoting = false;
            metadata.set_pad_name(pad.number(), use_utf8, use_quoting);

            if !pad.number().is_empty() {
                metadata.set_pad_pin_function(pad.pin_function(), use_utf8, use_quoting);
            }

            metadata.set_net_name(a_pad.netname());

            // Some pads are mechanical pads ( through hole or smd )
            // when this is the case, they have no pad name and/or are not plated.
            // In this case gerber files have slightly different attributes.
            if pad.attribute() == PadAttrib::Npth || pad.number().is_empty() {
                metadata.netlist_metadata.not_in_net = true;
            }

            if !plot_on_external_copper_layer {
                // the .P object attribute (GBR_NETLIST_METADATA::GBR_NETINFO_PAD)
                // is used on outer layers, unless the component is embedded
                // or a "etched" component (fp only drawn, not a physical component)
                // Currently, Pcbnew does not handle embedded component, so we disable the .P
                // attribute on internal layers
                // Note the Gerber doc is not really clear about through holes pads about the .P
                metadata.set_net_attrib_type(gbr_netinfo::NET | gbr_netinfo::CMP);
            }

            // Some attributes are reserved to the external copper layers:
            // GBR_APERTURE_ATTRIB_CONNECTORPAD and GBR_APERTURE_ATTRIB_SMDPAD_CUDEF
            // for instance.
            // Pad with type PAD_ATTRIB::CONN or PAD_ATTRIB::SMD that is not on outer layer
            // has its aperture attribute set to GBR_APERTURE_ATTRIB_CONDUCTOR
            match pad.attribute() {
                // Mechanical pad through hole
                PadAttrib::Npth => metadata.set_aperture_attrib(GbrApertureAttrib::Washerpad),

                // Pad through hole, a hole is also expected
                PadAttrib::Pth => metadata.set_aperture_attrib(GbrApertureAttrib::Componentpad),

                // Connector pads, no solder paste but with solder mask.
                PadAttrib::Conn => {
                    if plot_on_external_copper_layer {
                        metadata.set_aperture_attrib(GbrApertureAttrib::Connectorpad);
                    }
                }

                // Non-component only pads on any layer
                // For now, keep the existing "Conductor" logic above, but this can be changed
                // later if needed.
                PadAttrib::NonComponent => {}

                // SMD pads (on external copper layer only) with solder paste and mask
                PadAttrib::Smd => {
                    if plot_on_external_copper_layer {
                        metadata.set_aperture_attrib(GbrApertureAttrib::SmdpadCudef);
                    }
                }
            }

            // Fabrication properties can have specific GBR_APERTURE_METADATA options
            // that replace previous aperture attribute:
            match pad.property() {
                // Only applicable to outer layers
                PadProp::Bga => {
                    if plot_on_external_copper_layer {
                        metadata.set_aperture_attrib(GbrApertureAttrib::BgapadCudef);
                    }
                }

                PadProp::FiducialGlbl => {
                    metadata.set_aperture_attrib(GbrApertureAttrib::FiducialGlbl);
                }

                PadProp::FiducialLocal => {
                    metadata.set_aperture_attrib(GbrApertureAttrib::FiducialLocal);
                }

                // Only applicable to outer layers
                PadProp::Testpoint => {
                    if plot_on_external_copper_layer {
                        metadata.set_aperture_attrib(GbrApertureAttrib::Testpoint);
                    }
                }

                PadProp::Heatsink => metadata.set_aperture_attrib(GbrApertureAttrib::Heatsinkpad),

                PadProp::Castellated => {
                    metadata.set_aperture_attrib(GbrApertureAttrib::Castellatedpad);
                }

                // used only in drill files
                PadProp::Pressfit | PadProp::None | PadProp::Mechanical => {}
            }

            // Ensure NPTH pads have *always* the GBR_APERTURE_ATTRIB_WASHERPAD attribute
            if pad.attribute() == PadAttrib::Npth {
                metadata.set_aperture_attrib(GbrApertureAttrib::Washerpad);
            }
        } else {
            metadata.set_net_attrib_type(gbr_netinfo::CMP);
        }

        // Set plot color (change WHITE to LIGHTGRAY because
        // the white items are not seen on a white paper or screen
        let color = if *a_color != Color4D::WHITE {
            a_color.clone()
        } else {
            Color4D::from_eda_color(EdaColorT::LIGHTGRAY)
        };
        self.plotter.set_color(&color);

        let orientation = pad.orientation(ctx);

        if a_sketch_mode {
            let sketch_width = self.opts.sketch_pad_line_width;

            match pad.shape(a_layer) {
                PadShape::Circle => {
                    self.plotter.thick_circle(
                        shape_pos,
                        pad.size(ctx, a_layer).x,
                        sketch_width,
                        None,
                    );
                }

                PadShape::Oval => {
                    self.plotter.thick_oval(
                        shape_pos,
                        pad.size(ctx, a_layer),
                        orientation,
                        sketch_width,
                        None,
                    );
                }

                PadShape::Rectangle
                | PadShape::Roundrect
                | PadShape::Trapezoid
                | PadShape::ChamferedRect
                | PadShape::Custom => {
                    let mut outline = ShapePolySet::new();
                    pad.transform_shape_to_polygon(
                        ctx,
                        &mut outline,
                        a_layer,
                        0,
                        self.plotter.plotter_arc_high_def(),
                        ErrorLoc::Inside,
                        false,
                    );

                    self.plotter.thick_poly(&outline, sketch_width, None);
                }
            }

            return;
        }

        match pad.shape(a_layer) {
            PadShape::Circle => {
                self.plotter
                    .flash_pad_circle(shape_pos, pad.size(ctx, a_layer).x, Some(&metadata));
            }

            PadShape::Oval => {
                self.plotter.flash_pad_oval(
                    shape_pos,
                    pad.size(ctx, a_layer),
                    orientation,
                    Some(&metadata),
                );
            }

            PadShape::Rectangle => {
                self.plotter.flash_pad_rect(
                    shape_pos,
                    pad.size(ctx, a_layer),
                    orientation,
                    Some(&metadata),
                );
            }

            PadShape::Roundrect => {
                self.plotter.flash_pad_round_rect(
                    shape_pos,
                    pad.size(ctx, a_layer),
                    pad.round_rect_corner_radius(a_layer),
                    orientation,
                    Some(&metadata),
                );
            }

            PadShape::Trapezoid => {
                // Build the pad polygon in coordinates relative to the pad
                // (i.e. for a pad at pos 0,0, rot 0.0). Needed to use aperture macros,
                // to be able to create a pattern common to all trapezoid pads having the same
                // shape

                // Order is lower left, lower right, upper right, upper left.
                let half_size = pad.size(ctx, a_layer) / 2;
                let trap_delta = pad.delta(a_layer) / 2;

                let coord = [
                    Vector2I::new(-half_size.x - trap_delta.y, half_size.y + trap_delta.x),
                    Vector2I::new(half_size.x + trap_delta.y, half_size.y - trap_delta.x),
                    Vector2I::new(half_size.x - trap_delta.y, -half_size.y + trap_delta.x),
                    Vector2I::new(-half_size.x + trap_delta.y, -half_size.y - trap_delta.x),
                ];

                self.plotter
                    .flash_pad_trapez(shape_pos, &coord, orientation, Some(&metadata));
            }

            shape @ (PadShape::ChamferedRect | PadShape::Custom) => {
                if shape == PadShape::ChamferedRect
                    && self.plotter.plotter_type() == PlotFormat::Gerber
                {
                    let gerber_plotter = self.plotter.as_gerber_mut().expect("a Gerber plotter");

                    gerber_plotter.flash_pad_chamfer_round_rect_default(
                        shape_pos,
                        pad.size(ctx, a_layer),
                        pad.round_rect_corner_radius(a_layer),
                        pad.chamfer_rect_ratio(a_layer),
                        pad.chamfer_positions(a_layer),
                        orientation,
                        Some(&metadata),
                    );
                    return;
                }

                // KI_FALLTHROUGH to CUSTOM
                let polygons = pad.effective_polygon(ctx, a_layer, ErrorLoc::Inside);

                if polygons.outline_count() > 0 {
                    self.plotter.flash_pad_custom(
                        shape_pos,
                        pad.size(ctx, a_layer),
                        orientation,
                        &polygons,
                        Some(&metadata),
                    );
                }
            }
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotPadNumber`: the pad number as stroke text sized to the
    /// pad (sketched pads on fabrication layers).
    pub fn plot_pad_number(&mut self, a_pad: ItemRef<'_>, a_color: &Color4D) {
        let ctx = a_pad.ctx;
        let pad = a_pad.as_pad().expect("PlotPadNumber: a pad");
        let pad_number = unescape_string(pad.number());

        if pad_number.is_empty() {
            return;
        }

        let pad_bbox = pad.bounding_box(ctx);
        let mut position = pad_bbox.centre();
        let size = pad_bbox.size();
        // VECTOR2I padsize = padBBox.GetSize(): the int64 size clamped to int
        let mut padsize = Vector2I::new(clamp_i64_to_i32(size.x), clamp_i64_to_i32(size.y));

        // TODO(JE) padstacks
        if pad.shape(Padstack::TEMP_ALL_LAYERS) == PadShape::Custom {
            // See if we have a number box
            for primitive in pad.lib_padstack().primitives(Padstack::TEMP_ALL_LAYERS) {
                if primitive.is_proxy_item() && primitive.shape.shape() == ShapeT::Rectangle {
                    position = primitive.center(ctx);
                    rotate_point(&mut position, pad.orientation(ctx));
                    position += pad.shape_pos(ctx, Padstack::TEMP_ALL_LAYERS);

                    padsize.x = (primitive.bot_right(ctx).x - primitive.top_left(ctx).x).abs();
                    padsize.y = (primitive.bot_right(ctx).y - primitive.top_left(ctx).y).abs();

                    break;
                }
            }
        }

        if pad.shape(Padstack::TEMP_ALL_LAYERS) != PadShape::Custom {
            // Don't allow a 45 degree rotation to bloat a pad's bounding box unnecessarily
            let pad_size = pad.size(ctx, Padstack::TEMP_ALL_LAYERS);
            let limit = ki_round(f64::from(pad_size.x.min(pad_size.y)) * 1.1);

            if padsize.x > limit && padsize.y > limit {
                padsize.x = limit;
                padsize.y = limit;
            }
        }

        let mut text_attrs = TextAttributes {
            mirrored: self.plotter.plot_mirrored(),
            ..TextAttributes::default()
        };

        if f64::from(padsize.x) < (f64::from(padsize.y) * 0.95) {
            text_attrs.angle = ANGLE_90.as_degrees();
            std::mem::swap(&mut padsize.x, &mut padsize.y);
        }

        // approximate the size of the pad number text:
        // We use a size for at least 3 chars, to give a good look even for short numbers
        // KiROUND( int / int ): the integer quotient
        let mut tsize = padsize.x / printable_char_count(&pad_number).max(3);
        tsize = tsize.min(padsize.y);

        // enforce a max size
        tsize = tsize.min(PCB_IU_SCALE.mm_to_iu(5.0));

        text_attrs.size = (tsize, tsize);

        // use a somewhat spindly font to go with the outlined pads
        text_attrs.stroke_width = ki_round(f64::from(tsize) / 12.0);

        self.plotter.plot_text(
            position,
            a_color,
            &pad_number,
            &text_attrs,
            None,
            Metrics::default_metrics(),
            None,
        );
    }

    /// Port of `BRDITEMS_PLOTTER::plotOneDrillMark` (private in C++): flash one hole of
    /// `a_drill_shape` at `a_drill_pos`, shrunk to `a_small_drill` for round holes when
    /// non-zero and kept inside the pad.
    fn plot_one_drill_mark(
        &mut self,
        a_drill_shape: PadDrillShape,
        a_drill_pos: Vector2I,
        a_drill_size: Vector2I,
        a_pad_size: Vector2I,
        a_orientation: EdaAngle,
        a_small_drill: i32,
    ) {
        let mut drill_size = a_drill_size;

        // Small drill marks have no significance when applied to slots
        if a_small_drill != 0 && a_drill_shape == PadDrillShape::Circle {
            drill_size.x = a_small_drill.min(drill_size.x);
        }

        // Round holes only have x diameter, slots have both
        drill_size.x -= self.fine_width_adj();
        drill_size.x = std_clamp(drill_size.x, 1, a_pad_size.x - 1);

        if a_drill_shape == PadDrillShape::Oblong {
            drill_size.y -= self.fine_width_adj();
            drill_size.y = std_clamp(drill_size.y, 1, a_pad_size.y - 1);

            self.plotter
                .flash_pad_oval(a_drill_pos, drill_size, a_orientation, None);
        } else {
            self.plotter
                .flash_pad_circle(a_drill_pos, drill_size.x, None);
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotDrillMarks`: the via and pad holes of the board as
    /// filled marks (`GetDrillMarksType()`: small or full size), in white where they knock out
    /// a plotted pad.
    pub fn plot_drill_marks(&mut self) {
        let ctx = self.ctx;
        let board = ctx.board;
        let mut small_drill = 0;

        if self.opts.drill_marks == DrillMarks::SmallDrillShape {
            small_drill = PCB_IU_SCALE.mm_to_iu(SMALL_DRILL_MARK_SIZE);
        }

        // Drill marks are drawn white-on-black to knock-out the underlying pad.  This works
        // only for drivers supporting color change, obviously... it means that:
        //   - PS, SVG and PDF output is correct (i.e. you have a 'donut' pad)
        //   - In gerbers you can't see them. This is arguably the right thing to do since
        //     having drill marks and high speed drill stations is a sure recipe for broken
        //     tools and angry manufacturers. If you *really* want them you could start a layer
        //     with negative polarity to knock-out the film.
        //   - In DXF they go into the 'WHITE' layer. This could be useful.
        let dxf_filled = self.plotter.plotter_type() != PlotFormat::Dxf
            || self.opts.dxf_plot_mode == DxfOutlineMode::Filled;

        if dxf_filled {
            self.plotter.set_color(&Color4D::WHITE);
        }

        for (_, track) in board.tracks() {
            if let Track::Via(via) = track {
                // Via are not always on all layers
                if !(&via.layer_set(ctx) & &self.layer_mask).any() {
                    continue;
                }

                self.plot_one_drill_mark(
                    PadDrillShape::Circle,
                    via.track.start(),
                    Vector2I::new(via.drill_value(ctx), 0),
                    Vector2I::new(via.width(Padstack::TEMP_ALL_LAYERS), 0),
                    ANGLE_0,
                    small_drill,
                );
            }
        }

        for (footprint, _) in board.footprints() {
            for (_, pad) in board.footprint_pads(footprint) {
                let drill_size = pad.drill_size(ctx);

                if drill_size.x == 0 {
                    continue;
                }

                // Skip marks on layers the pad isn't on for Gerber only (24416). Other
                // formats keep them as a drill map, e.g. Edge.Cuts (24867).
                if self.plotter.plotter_type() == PlotFormat::Gerber
                    && !(&pad.layer_set(ctx) & &self.layer_mask).any()
                {
                    continue;
                }

                if dxf_filled {
                    // Drill mark is in black unless we can find something to knock it out of
                    self.plotter.set_color(&Color4D::BLACK);

                    for layer in self.layer_mask.iter() {
                        if !pad.is_on_layer(ctx, layer) {
                            continue;
                        }

                        let pad_size = pad.size(ctx, layer);

                        if pad_size.x > drill_size.x || pad_size.y > drill_size.y {
                            self.plotter.set_color(&Color4D::WHITE);
                            break;
                        }
                    }
                }

                self.plot_one_drill_mark(
                    pad.drill_shape(),
                    pad.position(ctx),
                    drill_size,
                    pad.size(ctx, Padstack::TEMP_ALL_LAYERS),
                    pad.orientation(ctx),
                    small_drill,
                );
            }
        }

        if dxf_filled {
            self.plotter.set_color(&Color4D::BLACK);
        }
    }

    /// Port of `BRDITEMS_PLOTTER::PlotDrillSymbols`: the drill map items (`PCB_DRILL_MAP`) on
    /// `a_layer`: for each map the board outline (moved by the map's offset), then a mark per
    /// hole of the map's spans (a `PLOTTER::Marker` shape, a letter or the drill size as text),
    /// the optional guide cross and the slot outline.
    pub fn plot_drill_symbols(&mut self, a_layer: PcbLayerId) {
        let ctx = self.ctx;
        let maps = drill_maps_on_layer(ctx, a_layer);

        if maps.is_empty() {
            return;
        }

        let profile = DrillSymbolProfile::from_board(ctx.board.design_settings());
        let symbol_width = profile.symbol_width();

        // Resolved rather than read straight from the profile, so a map drawn without any chart
        // ever having been placed still shows marks
        let cache = drill_symbol_cache(ctx);

        // DXF reads its sketch/filled mode back out of this. Without it every stroked mark is
        // emitted filled no matter what the user asked for
        let gbr_metadata = GbrMetadata::new();
        let plot_format = self.plotter.plotter_type();

        let color = self.get_color(a_layer.0);
        self.plotter.set_color(&color);
        self.plotter.set_current_line_width(symbol_width, None);

        let mut query = DrillQuery {
            merge_pth_npth: true,
            ..DrillQuery::default()
        };

        // Several maps can share a layer. The UI prevents it but a parsed, pasted or
        // API-created board need not
        for map in maps {
            let outlines = map.board_outlines(ctx);

            for ii in 0..outlines.outline_count() {
                self.plotter.plot_poly_chain(
                    outlines.c_outline(ii),
                    FillT::NoFill,
                    symbol_width,
                    get_metadata(plot_format, &gbr_metadata),
                );

                for jj in 0..outlines.hole_count(ii) {
                    self.plotter.plot_poly_chain(
                        outlines.c_hole(ii, jj),
                        FillT::NoFill,
                        symbol_width,
                        get_metadata(plot_format, &gbr_metadata),
                    );
                }
            }

            for span in enumerate_drill_spans(ctx, false) {
                // The map's own span filter, so a backdrill map does not print every other hole
                // too
                if !map.all_spans() && span != *map.span() {
                    continue;
                }

                query.span = span;

                for op in enumerate_drill_operations(ctx, &query) {
                    // The map is a set of marks on the holes, displaced as a whole. The holes
                    // themselves never move
                    let pos = op.position + map.offset();

                    let Some(assignment) = cache.by_group.get(&profile.group_key_string(&op))
                    else {
                        continue;
                    };

                    if assignment.mark_mode == DrillMarkMode::Shape {
                        self.plotter.marker(
                            pos,
                            map.symbol_size(),
                            curated_shape(assignment.shape_index),
                        );
                    } else {
                        // wxString::Format( wxT( "%.2f" ), pcbIUScale.IUTomm( op.m_Diameter ) )
                        let text = if assignment.mark_mode == DrillMarkMode::Letter {
                            assignment.letter.clone()
                        } else {
                            format_f(PCB_IU_SCALE.iu_to_mm(op.diameter), 2)
                        };

                        let attrs = TextAttributes {
                            size: (map.symbol_size(), map.symbol_size()),
                            stroke_width: symbol_width,
                            halign: GrTextHAlignT::Center,
                            valign: GrTextVAlignT::Center,
                            ..TextAttributes::default()
                        };

                        self.plotter.plot_text(
                            pos,
                            &color,
                            &text,
                            &attrs,
                            None,
                            Metrics::default_metrics(),
                            None,
                        );
                    }

                    if map.guide_cross() {
                        let reach = map.symbol_size();

                        self.plotter.thick_segment(
                            pos - Vector2I::new(reach, 0),
                            pos + Vector2I::new(reach, 0),
                            symbol_width,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                        self.plotter.thick_segment(
                            pos - Vector2I::new(0, reach),
                            pos + Vector2I::new(0, reach),
                            symbol_width,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    }

                    // A slot's true extent is the cost driver, so it is drawn as well as marked
                    if map.outline_slots() && op.is_slot {
                        self.plotter.thick_oval(
                            pos,
                            op.size_xy,
                            op.orientation,
                            symbol_width,
                            get_metadata(plot_format, &gbr_metadata),
                        );
                    }
                }
            }
        }

        self.plotter
            .set_current_line_width(USE_DEFAULT_LINE_WIDTH, None);
    }

    /// Port of `BRDITEMS_PLOTTER::PlotChartSymbols`: the curated marker of every drill chart
    /// row that has one, centred in the row's cell of the symbol column.
    pub fn plot_chart_symbols(&mut self, a_chart: ItemRef<'_>) {
        let ctx = a_chart.ctx;
        let table = a_chart.as_table().expect("PCB_DRILL_CHART");

        let Some(GeneratedTableKind::DrillChart {
            row_shapes,
            symbol_column,
            ..
        }) = table.generated.as_deref().map(|g| &g.kind)
        else {
            return;
        };

        if *symbol_column < 0 || row_shapes.is_empty() {
            return;
        }

        let profile = DrillSymbolProfile::from_board(ctx.board.design_settings());

        let color = self.get_color(a_chart.layer().0);
        self.plotter.set_color(&color);
        self.plotter
            .set_current_line_width(profile.symbol_width(), None);

        for (&row, &shape_index) in row_shapes {
            let Some(cell) = table
                .cell(row, *symbol_column)
                .and_then(|id| ctx.item(id.into()))
            else {
                continue;
            };

            let bbox = cell.bounding_box();

            // Same fit as the painter, so the plotted chart matches what was on screen
            // std::min<int>( size, std::min( w, h ) * 2 / 3 ): the int64 fit narrowed to int
            let size = profile
                .symbol_size()
                .min((bbox.width().min(bbox.height()) * 2 / 3) as i32);

            if size > 0 {
                self.plotter
                    .marker(bbox.centre(), size, curated_shape(shape_index));
            }
        }

        self.plotter
            .set_current_line_width(USE_DEFAULT_LINE_WIDTH, None);
    }

    /// Port of `BRDITEMS_PLOTTER::getColor`.
    pub fn get_color(&self, a_layer: i32) -> Color4D {
        let mut color = self.color_settings_color(a_layer);

        // A hack to avoid plotting a white item in white color on white paper
        if color == Color4D::WHITE {
            color = Color4D::from_eda_color(EdaColorT::LIGHTGRAY);
        }

        color
    }

    /// `ColorSettings()->GetColor( aLayer )` of the plot options (`PCB_PLOT_PARAMS::
    /// ColorSettings`, [`ColorSettings::get_color`]).
    pub fn color_settings_color(&self, a_layer: i32) -> Color4D {
        self.colors.get_color(a_layer)
    }
}

/// `ADVANCED_CFG::m_SmallDrillMarkSize` (mm; default of `common/advanced_config.cpp`, the
/// port reads no `kicad_advanced` file).
pub const SMALL_DRILL_MARK_SIZE: f64 = 0.35;

/// libstdc++ `std::clamp( v, lo, hi )` (`std::min( std::max( v, lo ), hi )` without the debug
/// assertion `lo <= hi`): a pad narrower than 2 nm yields `hi`.
fn std_clamp(v: i32, lo: i32, hi: i32) -> i32 {
    v.max(lo).min(hi)
}

/// `VECTOR2I( BOX2I::GetSize() )`: the int64 size clamped to `int`.
fn clamp_i64_to_i32(v: i64) -> i32 {
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Port of `BOARD::DrillMapsOnLayer( aLayer )`: the drill map items of `m_drawings` on
/// `a_layer`, in drawing order.
fn drill_maps_on_layer<'b>(ctx: &'b BoardCtx<'_>, a_layer: PcbLayerId) -> Vec<&'b PcbDrillMap> {
    let mut maps = Vec::new();

    for (_, item) in ctx.board.drawings() {
        if let AnyItem::DrillMap(map) = item
            && map.base.layer == a_layer
        {
            maps.push(map);
        }
    }

    maps
}

/// `plotMetadata( m_plotter, gbr_metadata, this )` (the `getMetadata()` macro): the Gerber
/// metadata for a Gerber plotter, nothing for SVG.
fn get_metadata<'m>(plot_format: PlotFormat, gbr_metadata: &'m GbrMetadata) -> PlotData<'m> {
    if plot_format == PlotFormat::Gerber {
        Some(gbr_metadata)
    } else {
        None
    }
}
