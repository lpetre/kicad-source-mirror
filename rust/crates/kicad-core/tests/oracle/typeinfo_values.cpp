/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * Copyright The KiCad Developers, see AUTHORS.txt for contributors.
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the
 * Free Software Foundation, either version 3 of the License, or (at your
 * option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

// Dumps the KICAD_T enum of include/core/typeinfo.h: tests/golden/typeinfo_values.txt, checked
// by tests/test_typeinfo.rs.  One line per value from -1 to MAX_STRUCT_TYPE_ID:
//
//   value BaseType IsNullType IsGeneratedTableType IsInstantiableType IsEeschemaType
//   IsPcbnewType IsSingleLayerType IsGerbviewType IsPageLayoutEditorType IsMiscType IsTypeCorrect
//
// followed by one "NAME value" line per enumerator.
//
// Built and run by tests/oracle/build_oracle.sh.
#include <cstdio>
#include <core/typeinfo.h>
int main() {
    for( int i = -1; i <= (int) MAX_STRUCT_TYPE_ID; ++i ) {
        KICAD_T t = (KICAD_T) i;
        printf( "%d %d %d %d %d %d %d %d %d %d %d %d\n", i, (int) BaseType( t ), (int) IsNullType( t ),
                (int) IsGeneratedTableType( t ), (int) IsInstantiableType( t ), (int) IsEeschemaType( t ),
                (int) IsPcbnewType( t ), (int) IsSingleLayerType( t ), (int) IsGerbviewType( t ),
                (int) IsPageLayoutEditorType( t ), (int) IsMiscType( t ), (int) IsTypeCorrect( t ) );
    }
    printf("NOT_USED %d\n", (int) NOT_USED);
    printf("TYPE_NOT_INIT %d\n", (int) TYPE_NOT_INIT);
    printf("PCB_T %d\n", (int) PCB_T);
    printf("SCREEN_T %d\n", (int) SCREEN_T);
    printf("PCB_FOOTPRINT_T %d\n", (int) PCB_FOOTPRINT_T);
    printf("PCB_PAD_T %d\n", (int) PCB_PAD_T);
    printf("PCB_SHAPE_T %d\n", (int) PCB_SHAPE_T);
    printf("PCB_REFERENCE_IMAGE_T %d\n", (int) PCB_REFERENCE_IMAGE_T);
    printf("PCB_FIELD_T %d\n", (int) PCB_FIELD_T);
    printf("PCB_GENERATOR_T %d\n", (int) PCB_GENERATOR_T);
    printf("PCB_TEXT_T %d\n", (int) PCB_TEXT_T);
    printf("PCB_TEXTBOX_T %d\n", (int) PCB_TEXTBOX_T);
    printf("PCB_TABLE_T %d\n", (int) PCB_TABLE_T);
    printf("PCB_TABLECELL_T %d\n", (int) PCB_TABLECELL_T);
    printf("PCB_TRACE_T %d\n", (int) PCB_TRACE_T);
    printf("PCB_VIA_T %d\n", (int) PCB_VIA_T);
    printf("PCB_ARC_T %d\n", (int) PCB_ARC_T);
    printf("PCB_MARKER_T %d\n", (int) PCB_MARKER_T);
    printf("PCB_DIMENSION_T %d\n", (int) PCB_DIMENSION_T);
    printf("PCB_BARCODE_T %d\n", (int) PCB_BARCODE_T);
    printf("PCB_DIM_ALIGNED_T %d\n", (int) PCB_DIM_ALIGNED_T);
    printf("PCB_DIM_LEADER_T %d\n", (int) PCB_DIM_LEADER_T);
    printf("PCB_DIM_CENTER_T %d\n", (int) PCB_DIM_CENTER_T);
    printf("PCB_DIM_RADIAL_T %d\n", (int) PCB_DIM_RADIAL_T);
    printf("PCB_DIM_ORTHOGONAL_T %d\n", (int) PCB_DIM_ORTHOGONAL_T);
    printf("PCB_TARGET_T %d\n", (int) PCB_TARGET_T);
    printf("PCB_ZONE_T %d\n", (int) PCB_ZONE_T);
    printf("PCB_ITEM_LIST_T %d\n", (int) PCB_ITEM_LIST_T);
    printf("PCB_NETINFO_T %d\n", (int) PCB_NETINFO_T);
    printf("PCB_GROUP_T %d\n", (int) PCB_GROUP_T);
    printf("PCB_BOARD_OUTLINE_T %d\n", (int) PCB_BOARD_OUTLINE_T);
    printf("PCB_POINT_T %d\n", (int) PCB_POINT_T);
    printf("PCB_FIELD_LOCATE_REFERENCE_T %d\n", (int) PCB_FIELD_LOCATE_REFERENCE_T);
    printf("PCB_FIELD_LOCATE_VALUE_T %d\n", (int) PCB_FIELD_LOCATE_VALUE_T);
    printf("PCB_FIELD_LOCATE_FOOTPRINT_T %d\n", (int) PCB_FIELD_LOCATE_FOOTPRINT_T);
    printf("PCB_FIELD_LOCATE_DATASHEET_T %d\n", (int) PCB_FIELD_LOCATE_DATASHEET_T);
    printf("PCB_LOCATE_STDVIA_T %d\n", (int) PCB_LOCATE_STDVIA_T);
    printf("PCB_LOCATE_UVIA_T %d\n", (int) PCB_LOCATE_UVIA_T);
    printf("PCB_LOCATE_BLINDVIA_T %d\n", (int) PCB_LOCATE_BLINDVIA_T);
    printf("PCB_LOCATE_BURIEDVIA_T %d\n", (int) PCB_LOCATE_BURIEDVIA_T);
    printf("PCB_LOCATE_TEXT_T %d\n", (int) PCB_LOCATE_TEXT_T);
    printf("PCB_LOCATE_HOLE_T %d\n", (int) PCB_LOCATE_HOLE_T);
    printf("PCB_LOCATE_PTH_T %d\n", (int) PCB_LOCATE_PTH_T);
    printf("PCB_LOCATE_NPTH_T %d\n", (int) PCB_LOCATE_NPTH_T);
    printf("PCB_LOCATE_BOARD_EDGE_T %d\n", (int) PCB_LOCATE_BOARD_EDGE_T);
    printf("PCB_SHAPE_LOCATE_SEGMENT_T %d\n", (int) PCB_SHAPE_LOCATE_SEGMENT_T);
    printf("PCB_SHAPE_LOCATE_RECT_T %d\n", (int) PCB_SHAPE_LOCATE_RECT_T);
    printf("PCB_SHAPE_LOCATE_CIRCLE_T %d\n", (int) PCB_SHAPE_LOCATE_CIRCLE_T);
    printf("PCB_SHAPE_LOCATE_ARC_T %d\n", (int) PCB_SHAPE_LOCATE_ARC_T);
    printf("PCB_SHAPE_LOCATE_POLY_T %d\n", (int) PCB_SHAPE_LOCATE_POLY_T);
    printf("PCB_SHAPE_LOCATE_BEZIER_T %d\n", (int) PCB_SHAPE_LOCATE_BEZIER_T);
    printf("LIB_SYMBOL_T %d\n", (int) LIB_SYMBOL_T);
    printf("SCH_SHAPE_T %d\n", (int) SCH_SHAPE_T);
    printf("SCH_FIELD_T %d\n", (int) SCH_FIELD_T);
    printf("SCH_TEXT_T %d\n", (int) SCH_TEXT_T);
    printf("SCH_TEXTBOX_T %d\n", (int) SCH_TEXTBOX_T);
    printf("SCH_PIN_T %d\n", (int) SCH_PIN_T);
    printf("SCH_MARKER_T %d\n", (int) SCH_MARKER_T);
    printf("SCH_JUNCTION_T %d\n", (int) SCH_JUNCTION_T);
    printf("SCH_NO_CONNECT_T %d\n", (int) SCH_NO_CONNECT_T);
    printf("SCH_BUS_WIRE_ENTRY_T %d\n", (int) SCH_BUS_WIRE_ENTRY_T);
    printf("SCH_BUS_BUS_ENTRY_T %d\n", (int) SCH_BUS_BUS_ENTRY_T);
    printf("SCH_LINE_T %d\n", (int) SCH_LINE_T);
    printf("SCH_BITMAP_T %d\n", (int) SCH_BITMAP_T);
    printf("SCH_TABLE_T %d\n", (int) SCH_TABLE_T);
    printf("SCH_TABLECELL_T %d\n", (int) SCH_TABLECELL_T);
    printf("SCH_LABEL_T %d\n", (int) SCH_LABEL_T);
    printf("SCH_GLOBAL_LABEL_T %d\n", (int) SCH_GLOBAL_LABEL_T);
    printf("SCH_HIER_LABEL_T %d\n", (int) SCH_HIER_LABEL_T);
    printf("SCH_RULE_AREA_T %d\n", (int) SCH_RULE_AREA_T);
    printf("SCH_DIRECTIVE_LABEL_T %d\n", (int) SCH_DIRECTIVE_LABEL_T);
    printf("SCH_SYMBOL_T %d\n", (int) SCH_SYMBOL_T);
    printf("SCH_GROUP_T %d\n", (int) SCH_GROUP_T);
    printf("SCH_SHEET_PIN_T %d\n", (int) SCH_SHEET_PIN_T);
    printf("SCH_SHEET_T %d\n", (int) SCH_SHEET_T);
    printf("SCH_FIELD_LOCATE_REFERENCE_T %d\n", (int) SCH_FIELD_LOCATE_REFERENCE_T);
    printf("SCH_FIELD_LOCATE_VALUE_T %d\n", (int) SCH_FIELD_LOCATE_VALUE_T);
    printf("SCH_FIELD_LOCATE_FOOTPRINT_T %d\n", (int) SCH_FIELD_LOCATE_FOOTPRINT_T);
    printf("SCH_FIELD_LOCATE_DATASHEET_T %d\n", (int) SCH_FIELD_LOCATE_DATASHEET_T);
    printf("SCH_ITEM_LOCATE_WIRE_T %d\n", (int) SCH_ITEM_LOCATE_WIRE_T);
    printf("SCH_ITEM_LOCATE_BUS_T %d\n", (int) SCH_ITEM_LOCATE_BUS_T);
    printf("SCH_ITEM_LOCATE_GRAPHIC_LINE_T %d\n", (int) SCH_ITEM_LOCATE_GRAPHIC_LINE_T);
    printf("SCH_LABEL_LOCATE_ANY_T %d\n", (int) SCH_LABEL_LOCATE_ANY_T);
    printf("SCH_LABEL_LOCATE_WIRE_T %d\n", (int) SCH_LABEL_LOCATE_WIRE_T);
    printf("SCH_LABEL_LOCATE_BUS_T %d\n", (int) SCH_LABEL_LOCATE_BUS_T);
    printf("SCH_SYMBOL_LOCATE_POWER_T %d\n", (int) SCH_SYMBOL_LOCATE_POWER_T);
    printf("SCH_LOCATE_ANY_T %d\n", (int) SCH_LOCATE_ANY_T);
    printf("SCH_SCREEN_T %d\n", (int) SCH_SCREEN_T);
    printf("SCHEMATIC_T %d\n", (int) SCHEMATIC_T);
    printf("GERBER_LAYOUT_T %d\n", (int) GERBER_LAYOUT_T);
    printf("GERBER_DRAW_ITEM_T %d\n", (int) GERBER_DRAW_ITEM_T);
    printf("GERBER_IMAGE_T %d\n", (int) GERBER_IMAGE_T);
    printf("WSG_LINE_T %d\n", (int) WSG_LINE_T);
    printf("WSG_RECT_T %d\n", (int) WSG_RECT_T);
    printf("WSG_POLY_T %d\n", (int) WSG_POLY_T);
    printf("WSG_TEXT_T %d\n", (int) WSG_TEXT_T);
    printf("WSG_BITMAP_T %d\n", (int) WSG_BITMAP_T);
    printf("WSG_PAGE_T %d\n", (int) WSG_PAGE_T);
    printf("WS_PROXY_UNDO_ITEM_T %d\n", (int) WS_PROXY_UNDO_ITEM_T);
    printf("WS_PROXY_UNDO_ITEM_PLUS_T %d\n", (int) WS_PROXY_UNDO_ITEM_PLUS_T);
    printf("SYMBOL_LIB_TABLE_T %d\n", (int) SYMBOL_LIB_TABLE_T);
    printf("FP_LIB_TABLE_T %d\n", (int) FP_LIB_TABLE_T);
    printf("DESIGN_BLOCK_LIB_TABLE_T %d\n", (int) DESIGN_BLOCK_LIB_TABLE_T);
    printf("SYMBOL_LIBS_T %d\n", (int) SYMBOL_LIBS_T);
    printf("SEARCH_STACK_T %d\n", (int) SEARCH_STACK_T);
    printf("S3D_CACHE_T %d\n", (int) S3D_CACHE_T);
    printf("PCB_SHAPE_LOCATE_ELLIPSE_T %d\n", (int) PCB_SHAPE_LOCATE_ELLIPSE_T);
    printf("PCB_SHAPE_LOCATE_ELLIPSE_ARC_T %d\n", (int) PCB_SHAPE_LOCATE_ELLIPSE_ARC_T);
    printf("PCB_CONSTRAINT_T %d\n", (int) PCB_CONSTRAINT_T);
    printf("PCB_GRID_ITEM_T %d\n", (int) PCB_GRID_ITEM_T);
    printf("PCB_DRILL_CHART_T %d\n", (int) PCB_DRILL_CHART_T);
    printf("PCB_DRILL_MAP_T %d\n", (int) PCB_DRILL_MAP_T);
    printf("PCB_3D_BODY_T %d\n", (int) PCB_3D_BODY_T);
    printf("PCB_STACKUP_TABLE_T %d\n", (int) PCB_STACKUP_TABLE_T);
    printf("MAX_STRUCT_TYPE_ID %d\n", (int) MAX_STRUCT_TYPE_ID);
    return 0;
}
