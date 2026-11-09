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

// Dumps the values of the GUI layer enums of include/layer_ids.h (NETNAMES_LAYER_ID,
// GAL_LAYER_ID, SCH_LAYER_ID, GERBVIEW_LAYER_ID, LAYER_3D_ID) as "NAME value" lines:
// tests/golden/layer_ids_values.txt, checked by tests/test_layer_ids.rs.
//
// Built and run by tests/oracle/build_oracle.sh.
#include <cstdio>
#include <layer_ids.h>
int main() {
    printf("NETNAMES_LAYER_ID_START %d\n", (int) NETNAMES_LAYER_ID_START);
    printf("NETNAMES_LAYER_ID_RESERVED %d\n", (int) NETNAMES_LAYER_ID_RESERVED);
    printf("LAYER_PAD_FR_NETNAMES %d\n", (int) LAYER_PAD_FR_NETNAMES);
    printf("LAYER_PAD_BK_NETNAMES %d\n", (int) LAYER_PAD_BK_NETNAMES);
    printf("LAYER_PAD_NETNAMES %d\n", (int) LAYER_PAD_NETNAMES);
    printf("LAYER_VIA_NETNAMES %d\n", (int) LAYER_VIA_NETNAMES);
    printf("NETNAMES_LAYER_ID_END %d\n", (int) NETNAMES_LAYER_ID_END);
    printf("GAL_LAYER_ID_START %d\n", (int) GAL_LAYER_ID_START);
    printf("LAYER_VIAS %d\n", (int) LAYER_VIAS);
    printf("LAYER_VIA_MICROVIA %d\n", (int) LAYER_VIA_MICROVIA);
    printf("LAYER_VIA_BLIND %d\n", (int) LAYER_VIA_BLIND);
    printf("LAYER_VIA_BURIED %d\n", (int) LAYER_VIA_BURIED);
    printf("LAYER_VIA_THROUGH %d\n", (int) LAYER_VIA_THROUGH);
    printf("LAYER_NON_PLATEDHOLES %d\n", (int) LAYER_NON_PLATEDHOLES);
    printf("LAYER_FP_TEXT %d\n", (int) LAYER_FP_TEXT);
    printf("LAYER_ANCHOR %d\n", (int) LAYER_ANCHOR);
    printf("LAYER_RATSNEST %d\n", (int) LAYER_RATSNEST);
    printf("LAYER_GRID %d\n", (int) LAYER_GRID);
    printf("LAYER_GRID_AXES %d\n", (int) LAYER_GRID_AXES);
    printf("LAYER_FOOTPRINTS_FR %d\n", (int) LAYER_FOOTPRINTS_FR);
    printf("LAYER_FOOTPRINTS_BK %d\n", (int) LAYER_FOOTPRINTS_BK);
    printf("LAYER_FP_VALUES %d\n", (int) LAYER_FP_VALUES);
    printf("LAYER_FP_REFERENCES %d\n", (int) LAYER_FP_REFERENCES);
    printf("LAYER_TRACKS %d\n", (int) LAYER_TRACKS);
    printf("LAYER_PAD_PLATEDHOLES %d\n", (int) LAYER_PAD_PLATEDHOLES);
    printf("LAYER_VIA_HOLES %d\n", (int) LAYER_VIA_HOLES);
    printf("LAYER_DRC_ERROR %d\n", (int) LAYER_DRC_ERROR);
    printf("LAYER_DRAWINGSHEET %d\n", (int) LAYER_DRAWINGSHEET);
    printf("LAYER_GP_OVERLAY %d\n", (int) LAYER_GP_OVERLAY);
    printf("LAYER_SELECT_OVERLAY %d\n", (int) LAYER_SELECT_OVERLAY);
    printf("LAYER_PCB_BACKGROUND %d\n", (int) LAYER_PCB_BACKGROUND);
    printf("LAYER_CURSOR %d\n", (int) LAYER_CURSOR);
    printf("LAYER_AUX_ITEMS %d\n", (int) LAYER_AUX_ITEMS);
    printf("LAYER_DRAW_BITMAPS %d\n", (int) LAYER_DRAW_BITMAPS);
    printf("GAL_LAYER_ID_BITMASK_END %d\n", (int) GAL_LAYER_ID_BITMASK_END);
    printf("LAYER_PADS %d\n", (int) LAYER_PADS);
    printf("LAYER_ZONES %d\n", (int) LAYER_ZONES);
    printf("LAYER_PAD_HOLEWALLS %d\n", (int) LAYER_PAD_HOLEWALLS);
    printf("LAYER_VIA_HOLEWALLS %d\n", (int) LAYER_VIA_HOLEWALLS);
    printf("LAYER_DRC_WARNING %d\n", (int) LAYER_DRC_WARNING);
    printf("LAYER_DRC_EXCLUSION %d\n", (int) LAYER_DRC_EXCLUSION);
    printf("LAYER_MARKER_SHADOWS %d\n", (int) LAYER_MARKER_SHADOWS);
    printf("LAYER_LOCKED_ITEM_SHADOW %d\n", (int) LAYER_LOCKED_ITEM_SHADOW);
    printf("LAYER_CONFLICTS_SHADOW %d\n", (int) LAYER_CONFLICTS_SHADOW);
    printf("LAYER_FILLED_SHAPES %d\n", (int) LAYER_FILLED_SHAPES);
    printf("LAYER_BOARD_OUTLINE_AREA %d\n", (int) LAYER_BOARD_OUTLINE_AREA);
    printf("LAYER_POINTS %d\n", (int) LAYER_POINTS);
    printf("LAYER_CONSTRAINT_SHADOW %d\n", (int) LAYER_CONSTRAINT_SHADOW);
    printf("LAYER_SUBGRIDS %d\n", (int) LAYER_SUBGRIDS);
    printf("LAYER_VIA_STITCHING %d\n", (int) LAYER_VIA_STITCHING);
    printf("LAYER_DRAWINGSHEET_PAGE1 %d\n", (int) LAYER_DRAWINGSHEET_PAGE1);
    printf("LAYER_DRAWINGSHEET_PAGEn %d\n", (int) LAYER_DRAWINGSHEET_PAGEn);
    printf("LAYER_PAGE_LIMITS %d\n", (int) LAYER_PAGE_LIMITS);
    printf("LAYER_DRC_HIGHLIGHTED %d\n", (int) LAYER_DRC_HIGHLIGHTED);
    printf("LAYER_CONSTRAINT_UNDER %d\n", (int) LAYER_CONSTRAINT_UNDER);
    printf("LAYER_CONSTRAINT_WELL %d\n", (int) LAYER_CONSTRAINT_WELL);
    printf("LAYER_CONSTRAINT_OVER %d\n", (int) LAYER_CONSTRAINT_OVER);
    printf("LAYER_ROUTER_PREVIEW_SHADOW %d\n", (int) LAYER_ROUTER_PREVIEW_SHADOW);
    printf("LAYER_ROUTER_PREVIEW_COLLISION %d\n", (int) LAYER_ROUTER_PREVIEW_COLLISION);
    printf("LAYER_OUTSIDE_GROUP_OPACITY %d\n", (int) LAYER_OUTSIDE_GROUP_OPACITY);
    printf("LAYER_ZONE_START %d\n", (int) LAYER_ZONE_START);
    printf("LAYER_ZONE_END %d\n", (int) LAYER_ZONE_END);
    printf("LAYER_PAD_COPPER_START %d\n", (int) LAYER_PAD_COPPER_START);
    printf("LAYER_PAD_COPPER_END %d\n", (int) LAYER_PAD_COPPER_END);
    printf("LAYER_VIA_COPPER_START %d\n", (int) LAYER_VIA_COPPER_START);
    printf("LAYER_VIA_COPPER_END %d\n", (int) LAYER_VIA_COPPER_END);
    printf("LAYER_CLEARANCE_START %d\n", (int) LAYER_CLEARANCE_START);
    printf("LAYER_CLEARANCE_END %d\n", (int) LAYER_CLEARANCE_END);
    printf("LAYER_BITMAP_START %d\n", (int) LAYER_BITMAP_START);
    printf("LAYER_BITMAP_END %d\n", (int) LAYER_BITMAP_END);
    printf("LAYER_POINT_START %d\n", (int) LAYER_POINT_START);
    printf("LAYER_POINT_END %d\n", (int) LAYER_POINT_END);
    printf("LAYER_UI_START %d\n", (int) LAYER_UI_START);
    printf("LAYER_UI_END %d\n", (int) LAYER_UI_END);
    printf("LAYER_DRILL_SYMBOL_START %d\n", (int) LAYER_DRILL_SYMBOL_START);
    printf("LAYER_DRILL_SYMBOL_END %d\n", (int) LAYER_DRILL_SYMBOL_END);
    printf("GAL_LAYER_ID_END %d\n", (int) GAL_LAYER_ID_END);
    printf("SCH_LAYER_ID_START %d\n", (int) SCH_LAYER_ID_START);
    printf("LAYER_WIRE %d\n", (int) LAYER_WIRE);
    printf("LAYER_BUS %d\n", (int) LAYER_BUS);
    printf("LAYER_JUNCTION %d\n", (int) LAYER_JUNCTION);
    printf("LAYER_LOCLABEL %d\n", (int) LAYER_LOCLABEL);
    printf("LAYER_GLOBLABEL %d\n", (int) LAYER_GLOBLABEL);
    printf("LAYER_HIERLABEL %d\n", (int) LAYER_HIERLABEL);
    printf("LAYER_PINNUM %d\n", (int) LAYER_PINNUM);
    printf("LAYER_PINNAM %d\n", (int) LAYER_PINNAM);
    printf("LAYER_REFERENCEPART %d\n", (int) LAYER_REFERENCEPART);
    printf("LAYER_VALUEPART %d\n", (int) LAYER_VALUEPART);
    printf("LAYER_FIELDS %d\n", (int) LAYER_FIELDS);
    printf("LAYER_INTERSHEET_REFS %d\n", (int) LAYER_INTERSHEET_REFS);
    printf("LAYER_NETCLASS_REFS %d\n", (int) LAYER_NETCLASS_REFS);
    printf("LAYER_RULE_AREAS %d\n", (int) LAYER_RULE_AREAS);
    printf("LAYER_DEVICE %d\n", (int) LAYER_DEVICE);
    printf("LAYER_NOTES %d\n", (int) LAYER_NOTES);
    printf("LAYER_PRIVATE_NOTES %d\n", (int) LAYER_PRIVATE_NOTES);
    printf("LAYER_NOTES_BACKGROUND %d\n", (int) LAYER_NOTES_BACKGROUND);
    printf("LAYER_PIN %d\n", (int) LAYER_PIN);
    printf("LAYER_SHEET %d\n", (int) LAYER_SHEET);
    printf("LAYER_SHEETNAME %d\n", (int) LAYER_SHEETNAME);
    printf("LAYER_SHEETFILENAME %d\n", (int) LAYER_SHEETFILENAME);
    printf("LAYER_SHEETFIELDS %d\n", (int) LAYER_SHEETFIELDS);
    printf("LAYER_SHEETLABEL %d\n", (int) LAYER_SHEETLABEL);
    printf("LAYER_NOCONNECT %d\n", (int) LAYER_NOCONNECT);
    printf("LAYER_DANGLING %d\n", (int) LAYER_DANGLING);
    printf("LAYER_DNP_MARKER %d\n", (int) LAYER_DNP_MARKER);
    printf("LAYER_ERC_WARN %d\n", (int) LAYER_ERC_WARN);
    printf("LAYER_ERC_ERR %d\n", (int) LAYER_ERC_ERR);
    printf("LAYER_ERC_EXCLUSION %d\n", (int) LAYER_ERC_EXCLUSION);
    printf("LAYER_EXCLUDED_FROM_SIM %d\n", (int) LAYER_EXCLUDED_FROM_SIM);
    printf("LAYER_SHAPES_BACKGROUND %d\n", (int) LAYER_SHAPES_BACKGROUND);
    printf("LAYER_DEVICE_BACKGROUND %d\n", (int) LAYER_DEVICE_BACKGROUND);
    printf("LAYER_SHEET_BACKGROUND %d\n", (int) LAYER_SHEET_BACKGROUND);
    printf("LAYER_SCHEMATIC_GRID %d\n", (int) LAYER_SCHEMATIC_GRID);
    printf("LAYER_SCHEMATIC_GRID_AXES %d\n", (int) LAYER_SCHEMATIC_GRID_AXES);
    printf("LAYER_SCHEMATIC_BACKGROUND %d\n", (int) LAYER_SCHEMATIC_BACKGROUND);
    printf("LAYER_SCHEMATIC_CURSOR %d\n", (int) LAYER_SCHEMATIC_CURSOR);
    printf("LAYER_HOVERED %d\n", (int) LAYER_HOVERED);
    printf("LAYER_BRIGHTENED %d\n", (int) LAYER_BRIGHTENED);
    printf("LAYER_HIDDEN %d\n", (int) LAYER_HIDDEN);
    printf("LAYER_NET_COLOR_HIGHLIGHT %d\n", (int) LAYER_NET_COLOR_HIGHLIGHT);
    printf("LAYER_DRAG_NET_COLLISION %d\n", (int) LAYER_DRAG_NET_COLLISION);
    printf("LAYER_SELECTION_SHADOWS %d\n", (int) LAYER_SELECTION_SHADOWS);
    printf("LAYER_SCHEMATIC_DRAWINGSHEET %d\n", (int) LAYER_SCHEMATIC_DRAWINGSHEET);
    printf("LAYER_SCHEMATIC_PAGE_LIMITS %d\n", (int) LAYER_SCHEMATIC_PAGE_LIMITS);
    printf("LAYER_BUS_JUNCTION %d\n", (int) LAYER_BUS_JUNCTION);
    printf("LAYER_SCHEMATIC_AUX_ITEMS %d\n", (int) LAYER_SCHEMATIC_AUX_ITEMS);
    printf("LAYER_SCHEMATIC_ANCHOR %d\n", (int) LAYER_SCHEMATIC_ANCHOR);
    printf("LAYER_OP_VOLTAGES %d\n", (int) LAYER_OP_VOLTAGES);
    printf("LAYER_OP_CURRENTS %d\n", (int) LAYER_OP_CURRENTS);
    printf("LAYER_GROUP %d\n", (int) LAYER_GROUP);
    printf("SCH_LAYER_ID_END %d\n", (int) SCH_LAYER_ID_END);
    printf("GERBVIEW_LAYER_ID_START %d\n", (int) GERBVIEW_LAYER_ID_START);
    printf("GERBVIEW_LAYER_ID_RESERVED %d\n", (int) GERBVIEW_LAYER_ID_RESERVED);
    printf("LAYER_DCODES %d\n", (int) LAYER_DCODES);
    printf("LAYER_NEGATIVE_OBJECTS %d\n", (int) LAYER_NEGATIVE_OBJECTS);
    printf("LAYER_GERBVIEW_GRID %d\n", (int) LAYER_GERBVIEW_GRID);
    printf("LAYER_GERBVIEW_AXES %d\n", (int) LAYER_GERBVIEW_AXES);
    printf("LAYER_GERBVIEW_BACKGROUND %d\n", (int) LAYER_GERBVIEW_BACKGROUND);
    printf("LAYER_GERBVIEW_DRAWINGSHEET %d\n", (int) LAYER_GERBVIEW_DRAWINGSHEET);
    printf("LAYER_GERBVIEW_PAGE_LIMITS %d\n", (int) LAYER_GERBVIEW_PAGE_LIMITS);
    printf("GERBVIEW_LAYER_ID_END %d\n", (int) GERBVIEW_LAYER_ID_END);
    printf("LAYER_3D_START %d\n", (int) LAYER_3D_START);
    printf("LAYER_3D_BACKGROUND_BOTTOM %d\n", (int) LAYER_3D_BACKGROUND_BOTTOM);
    printf("LAYER_3D_BACKGROUND_TOP %d\n", (int) LAYER_3D_BACKGROUND_TOP);
    printf("LAYER_3D_BOARD %d\n", (int) LAYER_3D_BOARD);
    printf("LAYER_3D_COPPER_TOP %d\n", (int) LAYER_3D_COPPER_TOP);
    printf("LAYER_3D_COPPER_BOTTOM %d\n", (int) LAYER_3D_COPPER_BOTTOM);
    printf("LAYER_3D_SILKSCREEN_BOTTOM %d\n", (int) LAYER_3D_SILKSCREEN_BOTTOM);
    printf("LAYER_3D_SILKSCREEN_TOP %d\n", (int) LAYER_3D_SILKSCREEN_TOP);
    printf("LAYER_3D_SOLDERMASK_BOTTOM %d\n", (int) LAYER_3D_SOLDERMASK_BOTTOM);
    printf("LAYER_3D_SOLDERMASK_TOP %d\n", (int) LAYER_3D_SOLDERMASK_TOP);
    printf("LAYER_3D_SOLDERPASTE %d\n", (int) LAYER_3D_SOLDERPASTE);
    printf("LAYER_3D_ADHESIVE %d\n", (int) LAYER_3D_ADHESIVE);
    printf("LAYER_3D_USER_COMMENTS %d\n", (int) LAYER_3D_USER_COMMENTS);
    printf("LAYER_3D_USER_DRAWINGS %d\n", (int) LAYER_3D_USER_DRAWINGS);
    printf("LAYER_3D_USER_ECO1 %d\n", (int) LAYER_3D_USER_ECO1);
    printf("LAYER_3D_USER_ECO2 %d\n", (int) LAYER_3D_USER_ECO2);
    printf("LAYER_3D_F_FAB %d\n", (int) LAYER_3D_F_FAB);
    printf("LAYER_3D_B_FAB %d\n", (int) LAYER_3D_B_FAB);
    printf("LAYER_3D_F_COURTYARD %d\n", (int) LAYER_3D_F_COURTYARD);
    printf("LAYER_3D_B_COURTYARD %d\n", (int) LAYER_3D_B_COURTYARD);
    printf("LAYER_3D_USER_1 %d\n", (int) LAYER_3D_USER_1);
    printf("LAYER_3D_USER_2 %d\n", (int) LAYER_3D_USER_2);
    printf("LAYER_3D_USER_3 %d\n", (int) LAYER_3D_USER_3);
    printf("LAYER_3D_USER_4 %d\n", (int) LAYER_3D_USER_4);
    printf("LAYER_3D_USER_5 %d\n", (int) LAYER_3D_USER_5);
    printf("LAYER_3D_USER_6 %d\n", (int) LAYER_3D_USER_6);
    printf("LAYER_3D_USER_7 %d\n", (int) LAYER_3D_USER_7);
    printf("LAYER_3D_USER_8 %d\n", (int) LAYER_3D_USER_8);
    printf("LAYER_3D_USER_9 %d\n", (int) LAYER_3D_USER_9);
    printf("LAYER_3D_USER_10 %d\n", (int) LAYER_3D_USER_10);
    printf("LAYER_3D_USER_11 %d\n", (int) LAYER_3D_USER_11);
    printf("LAYER_3D_USER_12 %d\n", (int) LAYER_3D_USER_12);
    printf("LAYER_3D_USER_13 %d\n", (int) LAYER_3D_USER_13);
    printf("LAYER_3D_USER_14 %d\n", (int) LAYER_3D_USER_14);
    printf("LAYER_3D_USER_15 %d\n", (int) LAYER_3D_USER_15);
    printf("LAYER_3D_USER_16 %d\n", (int) LAYER_3D_USER_16);
    printf("LAYER_3D_USER_17 %d\n", (int) LAYER_3D_USER_17);
    printf("LAYER_3D_USER_18 %d\n", (int) LAYER_3D_USER_18);
    printf("LAYER_3D_USER_19 %d\n", (int) LAYER_3D_USER_19);
    printf("LAYER_3D_USER_20 %d\n", (int) LAYER_3D_USER_20);
    printf("LAYER_3D_USER_21 %d\n", (int) LAYER_3D_USER_21);
    printf("LAYER_3D_USER_22 %d\n", (int) LAYER_3D_USER_22);
    printf("LAYER_3D_USER_23 %d\n", (int) LAYER_3D_USER_23);
    printf("LAYER_3D_USER_24 %d\n", (int) LAYER_3D_USER_24);
    printf("LAYER_3D_USER_25 %d\n", (int) LAYER_3D_USER_25);
    printf("LAYER_3D_USER_26 %d\n", (int) LAYER_3D_USER_26);
    printf("LAYER_3D_USER_27 %d\n", (int) LAYER_3D_USER_27);
    printf("LAYER_3D_USER_28 %d\n", (int) LAYER_3D_USER_28);
    printf("LAYER_3D_USER_29 %d\n", (int) LAYER_3D_USER_29);
    printf("LAYER_3D_USER_30 %d\n", (int) LAYER_3D_USER_30);
    printf("LAYER_3D_USER_31 %d\n", (int) LAYER_3D_USER_31);
    printf("LAYER_3D_USER_32 %d\n", (int) LAYER_3D_USER_32);
    printf("LAYER_3D_USER_33 %d\n", (int) LAYER_3D_USER_33);
    printf("LAYER_3D_USER_34 %d\n", (int) LAYER_3D_USER_34);
    printf("LAYER_3D_USER_35 %d\n", (int) LAYER_3D_USER_35);
    printf("LAYER_3D_USER_36 %d\n", (int) LAYER_3D_USER_36);
    printf("LAYER_3D_USER_37 %d\n", (int) LAYER_3D_USER_37);
    printf("LAYER_3D_USER_38 %d\n", (int) LAYER_3D_USER_38);
    printf("LAYER_3D_USER_39 %d\n", (int) LAYER_3D_USER_39);
    printf("LAYER_3D_USER_40 %d\n", (int) LAYER_3D_USER_40);
    printf("LAYER_3D_USER_41 %d\n", (int) LAYER_3D_USER_41);
    printf("LAYER_3D_USER_42 %d\n", (int) LAYER_3D_USER_42);
    printf("LAYER_3D_USER_43 %d\n", (int) LAYER_3D_USER_43);
    printf("LAYER_3D_USER_44 %d\n", (int) LAYER_3D_USER_44);
    printf("LAYER_3D_USER_45 %d\n", (int) LAYER_3D_USER_45);
    printf("LAYER_3D_TH_MODELS %d\n", (int) LAYER_3D_TH_MODELS);
    printf("LAYER_3D_SMD_MODELS %d\n", (int) LAYER_3D_SMD_MODELS);
    printf("LAYER_3D_VIRTUAL_MODELS %d\n", (int) LAYER_3D_VIRTUAL_MODELS);
    printf("LAYER_3D_MODELS_NOT_IN_POS %d\n", (int) LAYER_3D_MODELS_NOT_IN_POS);
    printf("LAYER_3D_MODELS_MARKED_DNP %d\n", (int) LAYER_3D_MODELS_MARKED_DNP);
    printf("LAYER_3D_NAVIGATOR %d\n", (int) LAYER_3D_NAVIGATOR);
    printf("LAYER_3D_BOUNDING_BOXES %d\n", (int) LAYER_3D_BOUNDING_BOXES);
    printf("LAYER_3D_OFF_BOARD_SILK %d\n", (int) LAYER_3D_OFF_BOARD_SILK);
    printf("LAYER_3D_PLATED_BARRELS %d\n", (int) LAYER_3D_PLATED_BARRELS);
    printf("LAYER_3D_COPPER_INNER_1 %d\n", (int) LAYER_3D_COPPER_INNER_1);
    printf("LAYER_3D_COPPER_INNER_2 %d\n", (int) LAYER_3D_COPPER_INNER_2);
    printf("LAYER_3D_COPPER_INNER_3 %d\n", (int) LAYER_3D_COPPER_INNER_3);
    printf("LAYER_3D_COPPER_INNER_4 %d\n", (int) LAYER_3D_COPPER_INNER_4);
    printf("LAYER_3D_COPPER_INNER_5 %d\n", (int) LAYER_3D_COPPER_INNER_5);
    printf("LAYER_3D_COPPER_INNER_6 %d\n", (int) LAYER_3D_COPPER_INNER_6);
    printf("LAYER_3D_COPPER_INNER_7 %d\n", (int) LAYER_3D_COPPER_INNER_7);
    printf("LAYER_3D_COPPER_INNER_8 %d\n", (int) LAYER_3D_COPPER_INNER_8);
    printf("LAYER_3D_COPPER_INNER_9 %d\n", (int) LAYER_3D_COPPER_INNER_9);
    printf("LAYER_3D_COPPER_INNER_10 %d\n", (int) LAYER_3D_COPPER_INNER_10);
    printf("LAYER_3D_COPPER_INNER_11 %d\n", (int) LAYER_3D_COPPER_INNER_11);
    printf("LAYER_3D_COPPER_INNER_12 %d\n", (int) LAYER_3D_COPPER_INNER_12);
    printf("LAYER_3D_COPPER_INNER_13 %d\n", (int) LAYER_3D_COPPER_INNER_13);
    printf("LAYER_3D_COPPER_INNER_14 %d\n", (int) LAYER_3D_COPPER_INNER_14);
    printf("LAYER_3D_COPPER_INNER_15 %d\n", (int) LAYER_3D_COPPER_INNER_15);
    printf("LAYER_3D_COPPER_INNER_16 %d\n", (int) LAYER_3D_COPPER_INNER_16);
    printf("LAYER_3D_COPPER_INNER_17 %d\n", (int) LAYER_3D_COPPER_INNER_17);
    printf("LAYER_3D_COPPER_INNER_18 %d\n", (int) LAYER_3D_COPPER_INNER_18);
    printf("LAYER_3D_COPPER_INNER_19 %d\n", (int) LAYER_3D_COPPER_INNER_19);
    printf("LAYER_3D_COPPER_INNER_20 %d\n", (int) LAYER_3D_COPPER_INNER_20);
    printf("LAYER_3D_COPPER_INNER_21 %d\n", (int) LAYER_3D_COPPER_INNER_21);
    printf("LAYER_3D_COPPER_INNER_22 %d\n", (int) LAYER_3D_COPPER_INNER_22);
    printf("LAYER_3D_COPPER_INNER_23 %d\n", (int) LAYER_3D_COPPER_INNER_23);
    printf("LAYER_3D_COPPER_INNER_24 %d\n", (int) LAYER_3D_COPPER_INNER_24);
    printf("LAYER_3D_COPPER_INNER_25 %d\n", (int) LAYER_3D_COPPER_INNER_25);
    printf("LAYER_3D_COPPER_INNER_26 %d\n", (int) LAYER_3D_COPPER_INNER_26);
    printf("LAYER_3D_COPPER_INNER_27 %d\n", (int) LAYER_3D_COPPER_INNER_27);
    printf("LAYER_3D_COPPER_INNER_28 %d\n", (int) LAYER_3D_COPPER_INNER_28);
    printf("LAYER_3D_COPPER_INNER_29 %d\n", (int) LAYER_3D_COPPER_INNER_29);
    printf("LAYER_3D_COPPER_INNER_30 %d\n", (int) LAYER_3D_COPPER_INNER_30);
    printf("LAYER_3D_END %d\n", (int) LAYER_3D_END);
    printf("GAL_LAYER_ID_COUNT %d\n", (int) GAL_LAYER_ID_COUNT);
    printf("SCH_LAYER_ID_COUNT %d\n", (int) SCH_LAYER_ID_COUNT);
    printf("LAYER_ID_COUNT %d\n", (int) LAYER_ID_COUNT);
    printf("MIN_VISIBILITY_MASK %d\n", (int) MIN_VISIBILITY_MASK);
    return 0;
}
