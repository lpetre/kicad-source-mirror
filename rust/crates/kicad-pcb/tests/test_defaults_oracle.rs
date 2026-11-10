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

//! Constructor defaults of the board model against the reference build: every expected value
//! is copied from `pcb-dump defaults` (`rust/docs/PCB_DUMP_FORMAT.md`; the line is quoted as
//! `KEY|field=value`). Spot checks of the wave-0b integration review, five per type package;
//! the full comparison is the dump replay of the `dump` module once it exists.

use kicad_core::eda_units::EdaUnits;
use kicad_core::plot_enums::{DrillMarks, PlotFormat};
use kicad_core::{KicadT, Lset, PcbLayerId};
use kicad_geom::{ANGLE_45, Vector2I};
use kicad_pcb::board_design_settings::BoardDesignSettings;
use kicad_pcb::board_stackup::{BoardStackup, BsEdgeConnectorConstraints};
use kicad_pcb::footprint::{Footprint, FootprintStackup};
use kicad_pcb::pad::Pad;
use kicad_pcb::padstack::{CustomShapeZoneMode, PadAttrib, Padstack, UnconnectedLayerMode};
use kicad_pcb::pcb_barcode::{BarcodeEccT, BarcodeT, PcbBarcode};
use kicad_pcb::pcb_dimension::{
    DimPrecision, DimTextPosition, DimUnitsFormat, DimUnitsMode, DimensionKind, PcbDimension,
};
use kicad_pcb::pcb_generator::PcbGenerator;
use kicad_pcb::pcb_group::PcbGroup;
use kicad_pcb::pcb_plot_params::PcbPlotParams;
use kicad_pcb::pcb_point::PcbPoint;
use kicad_pcb::pcb_reference_image::PcbReferenceImage;
use kicad_pcb::pcb_table::PcbTable;
use kicad_pcb::pcb_tablecell::PcbTablecell;
use kicad_pcb::pcb_target::PcbTarget;
use kicad_pcb::pcb_text::PcbText;
use kicad_pcb::pcb_textbox::PcbTextbox;
use kicad_pcb::teardrop_parameters::TeardropParameters;
use kicad_pcb::zone::Zone;
use kicad_pcb::zone_settings::{IslandRemovalMode, ZoneConnection, ZoneSettings};
use kicad_pcb::{Board, NetCode};

/// Package M0b: pad / padstack, footprint, zone, zone settings, teardrop parameters.
#[test]
fn m0b_defaults() {
    // PAD|pad.attribute=PTH, PAD|pad.drill_size=762000,762000,
    // PAD|padstack.layerset=0000000000000000555555555555555f (all copper + both masks),
    // PAD|pad.thermal_spoke_angle=45, PAD|pad.unconnected_layer_mode=KEEP_ALL,
    // PAD|pad.custom_shape_in_zone=OUTLINE
    let pad = Pad::default();
    assert_eq!(pad.attribute, PadAttrib::Pth);
    let padstack = pad.lib_padstack();
    assert_eq!(padstack.drill.size, Vector2I::new(762_000, 762_000));
    let mut pth_mask = Lset::all_cu_mask().clone();
    pth_mask.set(PcbLayerId::FMask);
    pth_mask.set(PcbLayerId::BMask);
    assert_eq!(*padstack.layer_set(), pth_mask);
    assert_eq!(padstack.layer_set().count(), 34);
    let all = padstack.copper_props(Padstack::ALL_LAYERS);
    assert_eq!(all.thermal_spoke_angle.map(|a| a.angle()), Some(ANGLE_45));
    assert_eq!(
        padstack.unconnected_layer_mode,
        UnconnectedLayerMode::KeepAll
    );
    assert_eq!(
        padstack.custom_shape_in_zone_mode,
        CustomShapeZoneMode::Outline
    );

    // FOOTPRINT|footprint.layer=F.Cu, footprint.attributes=0,
    // footprint.stackup_mode=EXPAND_INNER_LAYERS,
    // footprint.stackup_layers=00000000000000000000000000000015 (F.Cu, B.Cu, In1.Cu),
    // footprint.local_zone_connection=INHERITED
    let fp = Footprint::default();
    assert_eq!(fp.base.layer, PcbLayerId::FCu);
    assert_eq!(fp.attributes, 0);
    assert_eq!(fp.stackup_mode, FootprintStackup::ExpandInnerLayers);
    assert_eq!(
        fp.stackup_layers,
        Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu, PcbLayerId::In1Cu])
    );
    assert_eq!(fp.local_zone_connection(), ZoneConnection::Inherited);

    // ZONE|item.layerset=000...0, zone.local_clearance=500000, zone.min_thickness=250000,
    // zone.thermal_relief_gap=500000, zone.thermal_relief_spoke_width=500000,
    // zone.hatch_thickness=1000000, zone.hatch_gap=1500000, zone.pad_connection=THERMAL,
    // zone.island_removal_mode=ALWAYS, zone.min_island_area=10000000000000,
    // zone.border_hatch_pitch=500000, zone.keepout.vias=true, zone.keepout.footprints=false
    let zone = Zone::default();
    assert!(zone.layer_set.none());
    assert_eq!(zone.zone_clearance, 500_000);
    assert_eq!(zone.zone_min_thickness, 250_000);
    assert_eq!(
        (zone.thermal_relief_gap, zone.thermal_relief_spoke_width),
        (500_000, 500_000)
    );
    assert_eq!(
        (zone.hatch_thickness, zone.hatch_gap),
        (1_000_000, 1_500_000)
    );
    assert_eq!(zone.pad_connection, ZoneConnection::Thermal);
    assert_eq!(zone.island_removal_mode, IslandRemovalMode::Always);
    assert_eq!(zone.min_island_area, 10_000_000_000_000);
    assert_eq!(zone.border_hatch_pitch, 500_000);
    assert!(zone.do_not_allow_vias && zone.do_not_allow_tracks && zone.do_not_allow_pads);
    assert!(!zone.do_not_allow_zone_fills && !zone.do_not_allow_footprints);

    // ZONE_SETTINGS|layers=00000000000000000000000000000001, clearance=500000,
    // thieving.element_size=500000, thieving.gap=1000000, thieving.line_width=300000,
    // hatch_smoothing_value=0.10000000000000001, hatch_hole_min_area=0.14999999999999999
    let zs = ZoneSettings::default();
    assert_eq!(zs.layers, Lset::from_layers(&[PcbLayerId::FCu]));
    assert_eq!(zs.zone_clearance, 500_000);
    assert_eq!(
        (
            zs.thieving_settings.element_size,
            zs.thieving_settings.gap,
            zs.thieving_settings.line_width
        ),
        (500_000, 1_000_000, 300_000)
    );
    assert_eq!(
        (zs.hatch_smoothing_value, zs.hatch_hole_min_area),
        (0.1, 0.15)
    );

    // TEARDROP_PARAMETERS|max_len=1000000, max_width=2000000, best_length_ratio=0.5,
    // best_width_ratio=1, width_to_size_filter_ratio=0.90000000000000002,
    // allow_use_two_tracks=true, enabled=false
    let td = TeardropParameters::default();
    assert_eq!((td.td_max_len, td.td_max_width), (1_000_000, 2_000_000));
    assert_eq!((td.best_length_ratio, td.best_width_ratio), (0.5, 1.0));
    assert_eq!(td.width_to_size_filter_ratio, 0.9);
    assert!(td.allow_use_two_tracks && !td.enabled && !td.curved_edges);
}

/// Package M0c: text, textbox, table cell, table, dimension, barcode.
#[test]
fn m0c_defaults() {
    // PCB_TEXT|text.text_size=1270000,1270000, text.text_thickness=0, text.multiline=true,
    // text.visible=true
    let text = PcbText::default();
    assert_eq!(text.text.attributes.size, (1_270_000, 1_270_000));
    assert_eq!(text.text.attributes.stroke_width, 0);
    assert!(text.text.attributes.multiline && text.text.visible);

    // PCB_TEXTBOX|text.textbox.margins=1002500,1002500,1002500,1002500,
    // text.textbox.border_enabled=true, text.textbox.border_width=100000
    let tb = PcbTextbox::default();
    assert_eq!(
        (
            tb.margin_left,
            tb.margin_top,
            tb.margin_right,
            tb.margin_bottom
        ),
        (1_002_500, 1_002_500, 1_002_500, 1_002_500)
    );
    assert!(tb.border_enabled);
    assert_eq!(tb.shape.shape.stroke.width(), 100_000);

    // PCB_TABLECELL|shape.end=1073741823,1073741823, text.cell.span=1,1,
    // text.textbox.margins=1002500,...
    let cell = PcbTablecell::default();
    assert_eq!(
        cell.textbox.shape.shape.end(),
        Vector2I::new(1_073_741_823, 1_073_741_823)
    );
    assert_eq!((cell.col_span, cell.row_span), (1, 1));
    assert_eq!(cell.textbox.margin_left, 1_002_500);

    // PCB_TABLE|misc.table.border_width=100000, misc.table.separators_width=100000,
    // misc.table.stroke_external=true, stroke_header=true, stroke_columns=true,
    // stroke_rows=true, misc.table.cols=0
    let table = PcbTable::default();
    assert_eq!(table.border_stroke.width(), 100_000);
    assert_eq!(table.separators_stroke.width(), 100_000);
    assert!(table.stroke_external && table.stroke_header_separator);
    assert!(table.stroke_columns && table.stroke_rows);
    assert_eq!(table.col_count, 0);

    // PCB_DIM_ALIGNED|dim.units=INCH, dim.units_format=BARE_SUFFIX, dim.precision=X_XXXX,
    // dim.suppress_zeroes=false, dim.keep_text_aligned=true, dim.text_position_mode=OUTSIDE,
    // dim.arrow_length=1270000, dim.extension_offset=0, dim.line_thickness=200000,
    // dim.extension_height=586420; PCB_DIM_LEADER|dim.units_format=NO_SUFFIX
    let dim = PcbDimension::of_type(KicadT::PcbDimAlignedT).unwrap();
    assert_eq!(dim.units, EdaUnits::Inch);
    assert!(!dim.auto_units);
    assert_eq!(dim.units_format, DimUnitsFormat::BareSuffix);
    assert_eq!(dim.precision, DimPrecision::X_XXXX);
    assert!(!dim.suppress_zeroes && dim.keep_text_aligned);
    assert_eq!(dim.text_position, DimTextPosition::Outside);
    assert_eq!((dim.arrow_length, dim.extension_offset), (1_270_000, 0));
    assert_eq!(dim.line_thickness, 200_000);
    let DimensionKind::Aligned(aligned) = &dim.kind else {
        panic!("PCB_DIM_ALIGNED_T is DimensionKind::Aligned");
    };
    assert_eq!(aligned.extension_height, 586_420);
    let leader = PcbDimension::of_type(KicadT::PcbDimLeaderT).unwrap();
    assert_eq!(leader.units_format, DimUnitsFormat::NoSuffix);

    // PCB_BARCODE|misc.barcode.kind=QR_CODE, misc.barcode.ecc=L,
    // misc.barcode.size=40000000,40000000, misc.barcode.text_size=1270000,
    // misc.barcode.margin=0,0
    let barcode = PcbBarcode::default();
    assert_eq!(barcode.kind, BarcodeT::QrCode);
    assert_eq!(barcode.error_correction, BarcodeEccT::L);
    assert_eq!((barcode.width, barcode.height), (40_000_000, 40_000_000));
    assert_eq!(barcode.text.text.attributes.size.1, 1_270_000);
    assert_eq!(barcode.margin, Vector2I::new(0, 0));
}

/// Package M0d: board, design settings, nets and net classes, plot parameters, stackup.
#[test]
fn m0d_defaults() {
    // BOARD|copper_layer_count=2, BOARD|enabled_layers=000000000000000000002aaaaaaaaaaf
    // (F.Cu, B.Cu and the 21 non-copper layers up to Margin). The layer types
    // (BOARD|layer.F.Cu.type=LT_SIGNAL, ...) are checked by `board::layers::tests`.
    let board = Board::new();
    assert_eq!(board.copper_layer_count(), 2);
    let enabled = board.enabled_layers();
    let enabled_ids: Vec<i32> = (0..128).filter(|&l| enabled.test(PcbLayerId(l))).collect();
    // 0x2aaaaaaaaaaf: bits 0..=3 and the odd bits 5..=45.
    let expected_ids: Vec<i32> = (0..4).chain((5..=45).step_by(2)).collect();
    assert_eq!(enabled_ids, expected_ids);

    // BOARD|net.0.name="", BOARD|net.0.netclass="Default",
    // BOARD|ds.netclass.Default.priority=-1,
    // BOARD|ds.netclass.Default.description="This is the default net class.",
    // BOARD|ds.netclass.Default.clearance=200000, ...via_diameter=600000,
    // ...diff_pair_gap=250000, ...wire_width=1524
    let net0 = board.find_net(NetCode::UNCONNECTED).unwrap();
    assert_eq!(net0.netname, "");
    assert_eq!(net0.netclass.as_ref().unwrap().name, "Default");
    let nc = board.design_settings().net_settings.default_netclass();
    assert_eq!(nc.priority, -1);
    assert_eq!(nc.description, "This is the default net class.");
    assert_eq!((nc.clearance, nc.via_dia), (Some(200_000), Some(600_000)));
    assert_eq!(
        (nc.diff_pair_gap, nc.wire_width),
        (Some(250_000), Some(1524))
    );

    // BOARD_DESIGN_SETTINGS|max_error=5000, board_thickness=1600000, hole_clearance=250000,
    // min_through_drill=300000, track_min_width=200000, dimension.units_mode=AUTOMATIC,
    // dimension.suppress_zeroes=true, dimension.arrow_length=1270000,
    // dimension.extension_offset=500000, use_height_for_length_calcs=true
    let bds = BoardDesignSettings::default();
    assert_eq!(bds.max_error(), 5000);
    assert_eq!(bds.board_thickness(), 1_600_000);
    assert_eq!(
        (bds.hole_clearance, bds.min_through_drill),
        (250_000, 300_000)
    );
    assert_eq!(bds.track_min_width, 200_000);
    assert_eq!(bds.dimension_units_mode, DimUnitsMode::Automatic);
    assert!(bds.dimension_suppress_zeroes && bds.use_height_for_length_calcs);
    assert_eq!(
        (bds.dimension_arrow_length, bds.dimension_extension_offset),
        (1_270_000, 500_000)
    );

    // PCB_PLOT_PARAMS|format=GERBER, drill_marks=SMALL_DRILL_SHAPE,
    // sketch_pad_line_width=100000, gerber_precision=6, svg_precision=4,
    // dashed_line_dash_ratio=12, dashed_line_gap_ratio=3, scale_selection=1, layer=F.Cu
    let pp = PcbPlotParams::default();
    assert_eq!(pp.format, PlotFormat::Gerber);
    assert_eq!(pp.drill_marks, DrillMarks::SmallDrillShape);
    assert_eq!(pp.sketch_pad_line_width, 100_000);
    assert_eq!((pp.gerber_precision, pp.svg_precision), (6, 4));
    assert_eq!(
        (pp.dashed_line_dash_ratio, pp.dashed_line_gap_ratio),
        (12.0, 3.0)
    );
    assert_eq!(pp.scale_selection, 1);
    assert_eq!(pp.layer, PcbLayerId::FCu);

    // BOARD_STACKUP|finish_type="None", count=0,
    // edge_connector_constraints=BS_EDGE_CONNECTOR_NONE, has_dielectric_constraints=false
    let stackup = BoardStackup::default();
    assert_eq!(stackup.finish_type, "None");
    assert!(stackup.list.is_empty() && !stackup.has_dielectric_constrains);
    assert_eq!(
        stackup.edge_connector_constraints,
        BsEdgeConnectorConstraints::None
    );
}

/// Package M0e: target, point, group, generator, reference image.
#[test]
fn m0e_defaults() {
    // PCB_TARGET|item.layer=Edge.Cuts, misc.target.shape=0, misc.target.size=5000000,
    // misc.target.width=200000
    let target = PcbTarget::default();
    assert_eq!(target.base.layer, PcbLayerId::EdgeCuts);
    assert_eq!(
        (target.shape, target.size, target.line_width),
        (0, 5_000_000, 200_000)
    );

    // PCB_POINT|item.layer=F.Cu, misc.point.size=1000000
    let point = PcbPoint::default();
    assert_eq!(point.base.layer, PcbLayerId::FCu);
    assert_eq!(point.size, 1_000_000);

    // PCB_GROUP|misc.group.name="", misc.group.members=[]
    let group = PcbGroup::default();
    assert!(group.name.is_empty() && group.members.is_empty());

    // PCB_GENERATOR.PCB_TUNING_PATTERN|item.layer=F.Cu (the default layer argument of every
    // registered subclass constructor)
    let generator = PcbGenerator::default();
    assert_eq!(generator.group.base.layer, PcbLayerId::FCu);
    assert!(!generator.is_dirty);

    // PCB_REFERENCE_IMAGE|item.layer=F.Cu, misc.image.opacity=1, misc.image.scale=1,
    // misc.image.position=0,0
    let image = PcbReferenceImage::default();
    assert_eq!(image.base.layer, PcbLayerId::FCu);
    assert_eq!((image.opacity, image.scale), (1.0, 1.0));
    assert_eq!(image.pos, Vector2I::new(0, 0));
}
