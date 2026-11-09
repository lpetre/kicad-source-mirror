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

//! Tests of the port of `include/core/typeinfo.h` (there is no C++ unit test for it): every
//! enumerator value and every classification helper is compared with a dump made by a C++
//! program including the header (`tests/golden/typeinfo_values.txt`).
//!
//! The golden file has one line per value from -1 to `MAX_STRUCT_TYPE_ID`:
//! `value BaseType IsNullType IsGeneratedTableType IsInstantiableType IsEeschemaType
//! IsPcbnewType IsSingleLayerType IsGerbviewType IsPageLayoutEditorType IsMiscType
//! IsTypeCorrect`, followed by one `NAME value` line per enumerator.

use kicad_core::typeinfo::KicadT;

const NAMED: &[(&str, KicadT)] = &[
        ("NOT_USED", KicadT::NotUsed),
        ("TYPE_NOT_INIT", KicadT::TypeNotInit),
        ("PCB_T", KicadT::PcbT),
        ("SCREEN_T", KicadT::ScreenT),
        ("PCB_FOOTPRINT_T", KicadT::PcbFootprintT),
        ("PCB_PAD_T", KicadT::PcbPadT),
        ("PCB_SHAPE_T", KicadT::PcbShapeT),
        ("PCB_REFERENCE_IMAGE_T", KicadT::PcbReferenceImageT),
        ("PCB_FIELD_T", KicadT::PcbFieldT),
        ("PCB_GENERATOR_T", KicadT::PcbGeneratorT),
        ("PCB_TEXT_T", KicadT::PcbTextT),
        ("PCB_TEXTBOX_T", KicadT::PcbTextboxT),
        ("PCB_TABLE_T", KicadT::PcbTableT),
        ("PCB_TABLECELL_T", KicadT::PcbTablecellT),
        ("PCB_TRACE_T", KicadT::PcbTraceT),
        ("PCB_VIA_T", KicadT::PcbViaT),
        ("PCB_ARC_T", KicadT::PcbArcT),
        ("PCB_MARKER_T", KicadT::PcbMarkerT),
        ("PCB_DIMENSION_T", KicadT::PcbDimensionT),
        ("PCB_BARCODE_T", KicadT::PcbBarcodeT),
        ("PCB_DIM_ALIGNED_T", KicadT::PcbDimAlignedT),
        ("PCB_DIM_LEADER_T", KicadT::PcbDimLeaderT),
        ("PCB_DIM_CENTER_T", KicadT::PcbDimCenterT),
        ("PCB_DIM_RADIAL_T", KicadT::PcbDimRadialT),
        ("PCB_DIM_ORTHOGONAL_T", KicadT::PcbDimOrthogonalT),
        ("PCB_TARGET_T", KicadT::PcbTargetT),
        ("PCB_ZONE_T", KicadT::PcbZoneT),
        ("PCB_ITEM_LIST_T", KicadT::PcbItemListT),
        ("PCB_NETINFO_T", KicadT::PcbNetinfoT),
        ("PCB_GROUP_T", KicadT::PcbGroupT),
        ("PCB_BOARD_OUTLINE_T", KicadT::PcbBoardOutlineT),
        ("PCB_POINT_T", KicadT::PcbPointT),
        ("PCB_FIELD_LOCATE_REFERENCE_T", KicadT::PcbFieldLocateReferenceT),
        ("PCB_FIELD_LOCATE_VALUE_T", KicadT::PcbFieldLocateValueT),
        ("PCB_FIELD_LOCATE_FOOTPRINT_T", KicadT::PcbFieldLocateFootprintT),
        ("PCB_FIELD_LOCATE_DATASHEET_T", KicadT::PcbFieldLocateDatasheetT),
        ("PCB_LOCATE_STDVIA_T", KicadT::PcbLocateStdviaT),
        ("PCB_LOCATE_UVIA_T", KicadT::PcbLocateUviaT),
        ("PCB_LOCATE_BLINDVIA_T", KicadT::PcbLocateBlindviaT),
        ("PCB_LOCATE_BURIEDVIA_T", KicadT::PcbLocateBuriedviaT),
        ("PCB_LOCATE_TEXT_T", KicadT::PcbLocateTextT),
        ("PCB_LOCATE_HOLE_T", KicadT::PcbLocateHoleT),
        ("PCB_LOCATE_PTH_T", KicadT::PcbLocatePthT),
        ("PCB_LOCATE_NPTH_T", KicadT::PcbLocateNpthT),
        ("PCB_LOCATE_BOARD_EDGE_T", KicadT::PcbLocateBoardEdgeT),
        ("PCB_SHAPE_LOCATE_SEGMENT_T", KicadT::PcbShapeLocateSegmentT),
        ("PCB_SHAPE_LOCATE_RECT_T", KicadT::PcbShapeLocateRectT),
        ("PCB_SHAPE_LOCATE_CIRCLE_T", KicadT::PcbShapeLocateCircleT),
        ("PCB_SHAPE_LOCATE_ARC_T", KicadT::PcbShapeLocateArcT),
        ("PCB_SHAPE_LOCATE_POLY_T", KicadT::PcbShapeLocatePolyT),
        ("PCB_SHAPE_LOCATE_BEZIER_T", KicadT::PcbShapeLocateBezierT),
        ("LIB_SYMBOL_T", KicadT::LibSymbolT),
        ("SCH_SHAPE_T", KicadT::SchShapeT),
        ("SCH_FIELD_T", KicadT::SchFieldT),
        ("SCH_TEXT_T", KicadT::SchTextT),
        ("SCH_TEXTBOX_T", KicadT::SchTextboxT),
        ("SCH_PIN_T", KicadT::SchPinT),
        ("SCH_MARKER_T", KicadT::SchMarkerT),
        ("SCH_JUNCTION_T", KicadT::SchJunctionT),
        ("SCH_NO_CONNECT_T", KicadT::SchNoConnectT),
        ("SCH_BUS_WIRE_ENTRY_T", KicadT::SchBusWireEntryT),
        ("SCH_BUS_BUS_ENTRY_T", KicadT::SchBusBusEntryT),
        ("SCH_LINE_T", KicadT::SchLineT),
        ("SCH_BITMAP_T", KicadT::SchBitmapT),
        ("SCH_TABLE_T", KicadT::SchTableT),
        ("SCH_TABLECELL_T", KicadT::SchTablecellT),
        ("SCH_LABEL_T", KicadT::SchLabelT),
        ("SCH_GLOBAL_LABEL_T", KicadT::SchGlobalLabelT),
        ("SCH_HIER_LABEL_T", KicadT::SchHierLabelT),
        ("SCH_RULE_AREA_T", KicadT::SchRuleAreaT),
        ("SCH_DIRECTIVE_LABEL_T", KicadT::SchDirectiveLabelT),
        ("SCH_SYMBOL_T", KicadT::SchSymbolT),
        ("SCH_GROUP_T", KicadT::SchGroupT),
        ("SCH_SHEET_PIN_T", KicadT::SchSheetPinT),
        ("SCH_SHEET_T", KicadT::SchSheetT),
        ("SCH_FIELD_LOCATE_REFERENCE_T", KicadT::SchFieldLocateReferenceT),
        ("SCH_FIELD_LOCATE_VALUE_T", KicadT::SchFieldLocateValueT),
        ("SCH_FIELD_LOCATE_FOOTPRINT_T", KicadT::SchFieldLocateFootprintT),
        ("SCH_FIELD_LOCATE_DATASHEET_T", KicadT::SchFieldLocateDatasheetT),
        ("SCH_ITEM_LOCATE_WIRE_T", KicadT::SchItemLocateWireT),
        ("SCH_ITEM_LOCATE_BUS_T", KicadT::SchItemLocateBusT),
        ("SCH_ITEM_LOCATE_GRAPHIC_LINE_T", KicadT::SchItemLocateGraphicLineT),
        ("SCH_LABEL_LOCATE_ANY_T", KicadT::SchLabelLocateAnyT),
        ("SCH_LABEL_LOCATE_WIRE_T", KicadT::SchLabelLocateWireT),
        ("SCH_LABEL_LOCATE_BUS_T", KicadT::SchLabelLocateBusT),
        ("SCH_SYMBOL_LOCATE_POWER_T", KicadT::SchSymbolLocatePowerT),
        ("SCH_LOCATE_ANY_T", KicadT::SchLocateAnyT),
        ("SCH_SCREEN_T", KicadT::SchScreenT),
        ("SCHEMATIC_T", KicadT::SchematicT),
        ("GERBER_LAYOUT_T", KicadT::GerberLayoutT),
        ("GERBER_DRAW_ITEM_T", KicadT::GerberDrawItemT),
        ("GERBER_IMAGE_T", KicadT::GerberImageT),
        ("WSG_LINE_T", KicadT::WsgLineT),
        ("WSG_RECT_T", KicadT::WsgRectT),
        ("WSG_POLY_T", KicadT::WsgPolyT),
        ("WSG_TEXT_T", KicadT::WsgTextT),
        ("WSG_BITMAP_T", KicadT::WsgBitmapT),
        ("WSG_PAGE_T", KicadT::WsgPageT),
        ("WS_PROXY_UNDO_ITEM_T", KicadT::WsProxyUndoItemT),
        ("WS_PROXY_UNDO_ITEM_PLUS_T", KicadT::WsProxyUndoItemPlusT),
        ("SYMBOL_LIB_TABLE_T", KicadT::SymbolLibTableT),
        ("FP_LIB_TABLE_T", KicadT::FpLibTableT),
        ("DESIGN_BLOCK_LIB_TABLE_T", KicadT::DesignBlockLibTableT),
        ("SYMBOL_LIBS_T", KicadT::SymbolLibsT),
        ("SEARCH_STACK_T", KicadT::SearchStackT),
        ("S3D_CACHE_T", KicadT::S3dCacheT),
        ("PCB_SHAPE_LOCATE_ELLIPSE_T", KicadT::PcbShapeLocateEllipseT),
        ("PCB_SHAPE_LOCATE_ELLIPSE_ARC_T", KicadT::PcbShapeLocateEllipseArcT),
        ("PCB_CONSTRAINT_T", KicadT::PcbConstraintT),
        ("PCB_GRID_ITEM_T", KicadT::PcbGridItemT),
        ("PCB_DRILL_CHART_T", KicadT::PcbDrillChartT),
        ("PCB_DRILL_MAP_T", KicadT::PcbDrillMapT),
        ("PCB_3D_BODY_T", KicadT::Pcb3dBodyT),
        ("PCB_STACKUP_TABLE_T", KicadT::PcbStackupTableT),
        ("MAX_STRUCT_TYPE_ID", KicadT::MaxStructTypeId),
];

#[test]
fn enumerator_values_match_cpp() {
    let golden = include_str!("golden/typeinfo_values.txt");
    let mut checked = 0;

    for line in golden.lines() {
        let mut fields = line.split(' ');
        let first = fields.next().unwrap();

        if first.parse::<i32>().is_ok() {
            continue;
        }

        let value: i32 = fields.next().unwrap().parse().unwrap();
        let (name, kicad_t) = NAMED[checked];

        assert_eq!(name, first, "enumerator order");
        assert_eq!(kicad_t as i32, value, "{name}");
        assert_eq!(kicad_t.as_i32(), value);
        assert_eq!(i32::from(kicad_t), value);
        assert_eq!(KicadT::from_i32(value), Some(kicad_t), "{name}");
        assert_eq!(KicadT::try_from(value), Ok(kicad_t));
        checked += 1;
    }

    assert_eq!(checked, NAMED.len());
    assert_eq!(checked, 115);

    assert_eq!(KicadT::NotUsed as i32, -1);
    assert_eq!(KicadT::TypeNotInit as i32, 0);
    assert_eq!(KicadT::PcbT as i32, 1);
    assert_eq!(KicadT::S3dCacheT as i32, 104);
    assert_eq!(KicadT::from_i32(-2), None);
    assert_eq!(KicadT::from_i32(KicadT::MaxStructTypeId as i32 + 1), None);
    assert_eq!(KicadT::try_from(1000), Err(1000));

    // The declaration order is the sort order.
    assert!(KicadT::PcbFootprintT < KicadT::PcbPadT);
    assert!(KicadT::PcbTraceT < KicadT::PcbViaT);
    assert!(KicadT::NotUsed < KicadT::TypeNotInit);
}

#[test]
fn classification_matches_cpp() {
    let golden = include_str!("golden/typeinfo_values.txt");
    let mut checked = 0;

    for line in golden.lines() {
        let fields: Vec<&str> = line.split(' ').collect();

        let Ok(value) = fields[0].parse::<i32>() else {
            continue;
        };

        let numbers: Vec<i32> = fields.iter().map(|f| f.parse().unwrap()).collect();
        let t = KicadT::from_i32(value).expect("every value up to MAX_STRUCT_TYPE_ID is named");

        let actual = [
            t.base_type() as i32,
            i32::from(t.is_null_type()),
            i32::from(t.is_generated_table_type()),
            i32::from(t.is_instantiable_type()),
            i32::from(t.is_eeschema_type()),
            i32::from(t.is_pcbnew_type()),
            i32::from(t.is_single_layer_type()),
            i32::from(t.is_gerbview_type()),
            i32::from(t.is_page_layout_editor_type()),
            i32::from(t.is_misc_type()),
            i32::from(t.is_type_correct()),
        ];

        assert_eq!(&actual[..], &numbers[1..], "{t:?}");
        checked += 1;
    }

    assert_eq!(checked, 115);
}

#[test]
fn base_types() {
    assert_eq!(KicadT::PcbDimAlignedT.base_type(), KicadT::PcbDimensionT);
    assert_eq!(KicadT::PcbShapeLocateArcT.base_type(), KicadT::PcbShapeT);
    assert_eq!(KicadT::PcbFieldLocateValueT.base_type(), KicadT::PcbFieldT);
    assert_eq!(KicadT::PcbLocatePthT.base_type(), KicadT::PcbLocateHoleT);
    assert_eq!(KicadT::PcbDrillChartT.base_type(), KicadT::PcbTableT);
    assert_eq!(KicadT::SchItemLocateWireT.base_type(), KicadT::SchLineT);
    assert_eq!(KicadT::PcbViaT.base_type(), KicadT::PcbViaT);
    assert!(KicadT::PcbViaT.is_instantiable_type());
    assert!(!KicadT::PcbDimensionT.is_instantiable_type());
    assert!(!KicadT::TypeNotInit.is_instantiable_type());
    assert!(KicadT::PcbTraceT.is_single_layer_type());
    assert!(!KicadT::PcbViaT.is_single_layer_type());
}
