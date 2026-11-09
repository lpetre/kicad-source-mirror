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

//! Port of `qa/tests/common/test_layer_ids.cpp` (Boost test suite `LayerIds`), followed by
//! additional tests of `include/layer_ids.h` / `common/layer_id.cpp`.

use std::collections::BTreeMap;

use kicad_core::layer_ids::*;
use kicad_core::lseq::Lseq;
use kicad_core::lset::Lset;

// ---- qa/tests/common/test_layer_ids.cpp ----

#[test]
fn lseq_test_layers() {
    let all_layers = Lset::all_layers_mask().clone();
    let seq1: Lseq = all_layers.seq_stackup_top2bottom(PcbLayerId::UndefinedLayer);

    assert_eq!(seq1.test_layers(PcbLayerId::FCu, PcbLayerId::FCu), 0);
    assert!(seq1.test_layers(PcbLayerId::FCu, PcbLayerId::In1Cu) > 0);
    assert!(seq1.test_layers(PcbLayerId::In1Cu, PcbLayerId::FCu) < 0);

    // Pretend like inner copper layer one is the currently selected layer.
    let seq2: Lseq = all_layers.seq_stackup_top2bottom(PcbLayerId::In1Cu);
    assert!(seq2.test_layers(PcbLayerId::FCu, PcbLayerId::In1Cu) < 0);
    assert!(seq2.test_layers(PcbLayerId::In1Cu, PcbLayerId::FCu) > 0);
}

#[test]
fn copper_layers() {
    assert!(is_copper_layer(PcbLayerId::FCu));
    assert!(is_copper_layer(PcbLayerId::BCu));
    assert!(is_copper_layer(PcbLayerId::In1Cu));
    assert!(!is_copper_layer(PcbLayerId::UnselectedLayer));
    assert!(!is_copper_layer(PcbLayerId::UndefinedLayer));
    assert!(!is_copper_layer(PcbLayerId::FSilkS));

    assert!(!is_copper_layer_lower_than(PcbLayerId::FCu, PcbLayerId::BCu));
    assert!(is_copper_layer_lower_than(PcbLayerId::In1Cu, PcbLayerId::FCu));
    assert!(is_copper_layer_lower_than(PcbLayerId::In2Cu, PcbLayerId::FCu));
    assert!(is_copper_layer_lower_than(PcbLayerId::BCu, PcbLayerId::FCu));
    assert!(!is_copper_layer_lower_than(PcbLayerId::In1Cu, PcbLayerId::BCu));
    assert!(!is_copper_layer_lower_than(PcbLayerId::BCu, PcbLayerId::BCu));
}

#[test]
fn flip_lset() {
    let mut front = Lset::from([PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::In2Cu]);
    let mut back = Lset::from([PcbLayerId::BCu, PcbLayerId::In3Cu, PcbLayerId::In4Cu]);

    front.flip_standard_layers(6);
    back.flip_standard_layers(6);

    assert!(
        front.compare(&Lset::from([
            PcbLayerId::BCu,
            PcbLayerId::In3Cu,
            PcbLayerId::In4Cu
        ])) == 0
    );
    assert!(
        back.compare(&Lset::from([
            PcbLayerId::FCu,
            PcbLayerId::In1Cu,
            PcbLayerId::In2Cu
        ])) == 0
    );
}

// ---- additional tests (not in the C++ suite) ----

/// The numeric values of `PCB_LAYER_ID` are part of the file formats.
#[test]
fn pcb_layer_id_values() {
    use PcbLayerId as L;

    assert_eq!(L::UndefinedLayer.0, -1);
    assert_eq!(L::UnselectedLayer.0, -2);
    assert_eq!(UNDEFINED_LAYER, L::UndefinedLayer);
    assert_eq!(UNSELECTED_LAYER, L::UnselectedLayer);
    assert_eq!(PCBNEW_LAYER_ID_START, L::FCu);

    let named: [(PcbLayerId, i32); 23] = [
        (L::FCu, 0),
        (L::BCu, 2),
        (L::In1Cu, 4),
        (L::In30Cu, 62),
        (L::FMask, 1),
        (L::BMask, 3),
        (L::FSilkS, 5),
        (L::BSilkS, 7),
        (L::FAdhes, 9),
        (L::BAdhes, 11),
        (L::FPaste, 13),
        (L::BPaste, 15),
        (L::DwgsUser, 17),
        (L::CmtsUser, 19),
        (L::Eco1User, 21),
        (L::Eco2User, 23),
        (L::EdgeCuts, 25),
        (L::Margin, 27),
        (L::BCrtYd, 29),
        (L::FCrtYd, 31),
        (L::BFab, 33),
        (L::FFab, 35),
        (L::Rescue, 37),
    ];

    for (layer, value) in named {
        assert_eq!(layer.0, value, "{layer:?}");
        assert_eq!(layer.as_i32(), value);
        assert_eq!(i32::from(layer), value);
        assert_eq!(PcbLayerId::from(value), layer);
    }

    let inner = [
        L::In1Cu, L::In2Cu, L::In3Cu, L::In4Cu, L::In5Cu, L::In6Cu, L::In7Cu, L::In8Cu, L::In9Cu,
        L::In10Cu, L::In11Cu, L::In12Cu, L::In13Cu, L::In14Cu, L::In15Cu, L::In16Cu, L::In17Cu,
        L::In18Cu, L::In19Cu, L::In20Cu, L::In21Cu, L::In22Cu, L::In23Cu, L::In24Cu, L::In25Cu,
        L::In26Cu, L::In27Cu, L::In28Cu, L::In29Cu, L::In30Cu,
    ];

    for (i, layer) in inner.iter().enumerate() {
        assert_eq!(layer.0, 4 + 2 * i as i32);
        assert_eq!(PcbLayerId::inner_cu(i as i32 + 1), *layer);
        assert_eq!(format!("{layer:?}"), format!("In{}Cu", i + 1));
    }

    let user = [
        L::User1, L::User2, L::User3, L::User4, L::User5, L::User6, L::User7, L::User8, L::User9,
        L::User10, L::User11, L::User12, L::User13, L::User14, L::User15, L::User16, L::User17,
        L::User18, L::User19, L::User20, L::User21, L::User22, L::User23, L::User24, L::User25,
        L::User26, L::User27, L::User28, L::User29, L::User30, L::User31, L::User32, L::User33,
        L::User34, L::User35, L::User36, L::User37, L::User38, L::User39, L::User40, L::User41,
        L::User42, L::User43, L::User44, L::User45,
    ];

    for (i, layer) in user.iter().enumerate() {
        assert_eq!(layer.0, 39 + 2 * i as i32);
        assert_eq!(PcbLayerId::user(i as i32 + 1), *layer);
        assert_eq!(format!("{layer:?}"), format!("User{}", i + 1));
    }

    assert_eq!(PCB_LAYER_ID_COUNT, 128);
    assert_eq!(PcbLayerId::PcbLayerIdCount.0, 128);
    assert_eq!(MAX_CU_LAYERS, 32);
    assert_eq!(MAX_USER_DEFINED_LAYERS, 45);
    assert_eq!(user.len() as i32, MAX_USER_DEFINED_LAYERS);
    assert_eq!(inner.len() as i32 + 2, MAX_CU_LAYERS);

    assert_eq!(format!("{:?}", L::FCu), "FCu");
    assert_eq!(format!("{:?}", L::EdgeCuts), "EdgeCuts");
    assert_eq!(format!("{:?}", L::UndefinedLayer), "UndefinedLayer");
    assert_eq!(format!("{:?}", PcbLayerId(64)), "PcbLayerId(64)");
    assert_eq!(format!("{:?}", PcbLayerId(-7)), "PcbLayerId(-7)");

    // The constants can be used as patterns.
    let classify = |layer: PcbLayerId| match layer {
        PcbLayerId::FCu | PcbLayerId::BCu => "outer",
        PcbLayerId::EdgeCuts => "edge",
        _ => "other",
    };

    assert_eq!(classify(PcbLayerId(2)), "outer");
    assert_eq!(classify(PcbLayerId(25)), "edge");
    assert_eq!(classify(PcbLayerId(4)), "other");

    // Ordering is the numeric one.
    assert!(L::UnselectedLayer < L::UndefinedLayer);
    assert!(L::UndefinedLayer < L::FCu);
    assert!(L::FMask < L::BCu);
}

/// The values of the GUI layer enums (`NETNAMES_LAYER_ID`, `GAL_LAYER_ID`, `SCH_LAYER_ID`,
/// `GERBVIEW_LAYER_ID`, `LAYER_3D_ID`) against a dump made by a C++ program including
/// `include/layer_ids.h` (`tests/golden/layer_ids_values.txt`, lines of `NAME value`).
#[test]
fn gui_layer_id_values_match_cpp() {
    #[allow(non_upper_case_globals)]
    let table: &[(&str, i32)] = &[
        ("NETNAMES_LAYER_ID_START", NETNAMES_LAYER_ID_START),
        ("NETNAMES_LAYER_ID_RESERVED", NETNAMES_LAYER_ID_RESERVED),
        ("LAYER_PAD_FR_NETNAMES", LAYER_PAD_FR_NETNAMES),
        ("LAYER_PAD_BK_NETNAMES", LAYER_PAD_BK_NETNAMES),
        ("LAYER_PAD_NETNAMES", LAYER_PAD_NETNAMES),
        ("LAYER_VIA_NETNAMES", LAYER_VIA_NETNAMES),
        ("NETNAMES_LAYER_ID_END", NETNAMES_LAYER_ID_END),
        ("GAL_LAYER_ID_START", GAL_LAYER_ID_START),
        ("LAYER_VIAS", LAYER_VIAS),
        ("LAYER_VIA_MICROVIA", LAYER_VIA_MICROVIA),
        ("LAYER_VIA_BLIND", LAYER_VIA_BLIND),
        ("LAYER_VIA_BURIED", LAYER_VIA_BURIED),
        ("LAYER_VIA_THROUGH", LAYER_VIA_THROUGH),
        ("LAYER_NON_PLATEDHOLES", LAYER_NON_PLATEDHOLES),
        ("LAYER_FP_TEXT", LAYER_FP_TEXT),
        ("LAYER_ANCHOR", LAYER_ANCHOR),
        ("LAYER_RATSNEST", LAYER_RATSNEST),
        ("LAYER_GRID", LAYER_GRID),
        ("LAYER_GRID_AXES", LAYER_GRID_AXES),
        ("LAYER_FOOTPRINTS_FR", LAYER_FOOTPRINTS_FR),
        ("LAYER_FOOTPRINTS_BK", LAYER_FOOTPRINTS_BK),
        ("LAYER_FP_VALUES", LAYER_FP_VALUES),
        ("LAYER_FP_REFERENCES", LAYER_FP_REFERENCES),
        ("LAYER_TRACKS", LAYER_TRACKS),
        ("LAYER_PAD_PLATEDHOLES", LAYER_PAD_PLATEDHOLES),
        ("LAYER_VIA_HOLES", LAYER_VIA_HOLES),
        ("LAYER_DRC_ERROR", LAYER_DRC_ERROR),
        ("LAYER_DRAWINGSHEET", LAYER_DRAWINGSHEET),
        ("LAYER_GP_OVERLAY", LAYER_GP_OVERLAY),
        ("LAYER_SELECT_OVERLAY", LAYER_SELECT_OVERLAY),
        ("LAYER_PCB_BACKGROUND", LAYER_PCB_BACKGROUND),
        ("LAYER_CURSOR", LAYER_CURSOR),
        ("LAYER_AUX_ITEMS", LAYER_AUX_ITEMS),
        ("LAYER_DRAW_BITMAPS", LAYER_DRAW_BITMAPS),
        ("GAL_LAYER_ID_BITMASK_END", GAL_LAYER_ID_BITMASK_END),
        ("LAYER_PADS", LAYER_PADS),
        ("LAYER_ZONES", LAYER_ZONES),
        ("LAYER_PAD_HOLEWALLS", LAYER_PAD_HOLEWALLS),
        ("LAYER_VIA_HOLEWALLS", LAYER_VIA_HOLEWALLS),
        ("LAYER_DRC_WARNING", LAYER_DRC_WARNING),
        ("LAYER_DRC_EXCLUSION", LAYER_DRC_EXCLUSION),
        ("LAYER_MARKER_SHADOWS", LAYER_MARKER_SHADOWS),
        ("LAYER_LOCKED_ITEM_SHADOW", LAYER_LOCKED_ITEM_SHADOW),
        ("LAYER_CONFLICTS_SHADOW", LAYER_CONFLICTS_SHADOW),
        ("LAYER_FILLED_SHAPES", LAYER_FILLED_SHAPES),
        ("LAYER_BOARD_OUTLINE_AREA", LAYER_BOARD_OUTLINE_AREA),
        ("LAYER_POINTS", LAYER_POINTS),
        ("LAYER_CONSTRAINT_SHADOW", LAYER_CONSTRAINT_SHADOW),
        ("LAYER_SUBGRIDS", LAYER_SUBGRIDS),
        ("LAYER_VIA_STITCHING", LAYER_VIA_STITCHING),
        ("LAYER_DRAWINGSHEET_PAGE1", LAYER_DRAWINGSHEET_PAGE1),
        ("LAYER_DRAWINGSHEET_PAGEn", LAYER_DRAWINGSHEET_PAGEn),
        ("LAYER_PAGE_LIMITS", LAYER_PAGE_LIMITS),
        ("LAYER_DRC_HIGHLIGHTED", LAYER_DRC_HIGHLIGHTED),
        ("LAYER_CONSTRAINT_UNDER", LAYER_CONSTRAINT_UNDER),
        ("LAYER_CONSTRAINT_WELL", LAYER_CONSTRAINT_WELL),
        ("LAYER_CONSTRAINT_OVER", LAYER_CONSTRAINT_OVER),
        ("LAYER_ROUTER_PREVIEW_SHADOW", LAYER_ROUTER_PREVIEW_SHADOW),
        ("LAYER_ROUTER_PREVIEW_COLLISION", LAYER_ROUTER_PREVIEW_COLLISION),
        ("LAYER_OUTSIDE_GROUP_OPACITY", LAYER_OUTSIDE_GROUP_OPACITY),
        ("LAYER_ZONE_START", LAYER_ZONE_START),
        ("LAYER_ZONE_END", LAYER_ZONE_END),
        ("LAYER_PAD_COPPER_START", LAYER_PAD_COPPER_START),
        ("LAYER_PAD_COPPER_END", LAYER_PAD_COPPER_END),
        ("LAYER_VIA_COPPER_START", LAYER_VIA_COPPER_START),
        ("LAYER_VIA_COPPER_END", LAYER_VIA_COPPER_END),
        ("LAYER_CLEARANCE_START", LAYER_CLEARANCE_START),
        ("LAYER_CLEARANCE_END", LAYER_CLEARANCE_END),
        ("LAYER_BITMAP_START", LAYER_BITMAP_START),
        ("LAYER_BITMAP_END", LAYER_BITMAP_END),
        ("LAYER_POINT_START", LAYER_POINT_START),
        ("LAYER_POINT_END", LAYER_POINT_END),
        ("LAYER_UI_START", LAYER_UI_START),
        ("LAYER_UI_END", LAYER_UI_END),
        ("LAYER_DRILL_SYMBOL_START", LAYER_DRILL_SYMBOL_START),
        ("LAYER_DRILL_SYMBOL_END", LAYER_DRILL_SYMBOL_END),
        ("GAL_LAYER_ID_END", GAL_LAYER_ID_END),
        ("SCH_LAYER_ID_START", SCH_LAYER_ID_START),
        ("LAYER_WIRE", LAYER_WIRE),
        ("LAYER_BUS", LAYER_BUS),
        ("LAYER_JUNCTION", LAYER_JUNCTION),
        ("LAYER_LOCLABEL", LAYER_LOCLABEL),
        ("LAYER_GLOBLABEL", LAYER_GLOBLABEL),
        ("LAYER_HIERLABEL", LAYER_HIERLABEL),
        ("LAYER_PINNUM", LAYER_PINNUM),
        ("LAYER_PINNAM", LAYER_PINNAM),
        ("LAYER_REFERENCEPART", LAYER_REFERENCEPART),
        ("LAYER_VALUEPART", LAYER_VALUEPART),
        ("LAYER_FIELDS", LAYER_FIELDS),
        ("LAYER_INTERSHEET_REFS", LAYER_INTERSHEET_REFS),
        ("LAYER_NETCLASS_REFS", LAYER_NETCLASS_REFS),
        ("LAYER_RULE_AREAS", LAYER_RULE_AREAS),
        ("LAYER_DEVICE", LAYER_DEVICE),
        ("LAYER_NOTES", LAYER_NOTES),
        ("LAYER_PRIVATE_NOTES", LAYER_PRIVATE_NOTES),
        ("LAYER_NOTES_BACKGROUND", LAYER_NOTES_BACKGROUND),
        ("LAYER_PIN", LAYER_PIN),
        ("LAYER_SHEET", LAYER_SHEET),
        ("LAYER_SHEETNAME", LAYER_SHEETNAME),
        ("LAYER_SHEETFILENAME", LAYER_SHEETFILENAME),
        ("LAYER_SHEETFIELDS", LAYER_SHEETFIELDS),
        ("LAYER_SHEETLABEL", LAYER_SHEETLABEL),
        ("LAYER_NOCONNECT", LAYER_NOCONNECT),
        ("LAYER_DANGLING", LAYER_DANGLING),
        ("LAYER_DNP_MARKER", LAYER_DNP_MARKER),
        ("LAYER_ERC_WARN", LAYER_ERC_WARN),
        ("LAYER_ERC_ERR", LAYER_ERC_ERR),
        ("LAYER_ERC_EXCLUSION", LAYER_ERC_EXCLUSION),
        ("LAYER_EXCLUDED_FROM_SIM", LAYER_EXCLUDED_FROM_SIM),
        ("LAYER_SHAPES_BACKGROUND", LAYER_SHAPES_BACKGROUND),
        ("LAYER_DEVICE_BACKGROUND", LAYER_DEVICE_BACKGROUND),
        ("LAYER_SHEET_BACKGROUND", LAYER_SHEET_BACKGROUND),
        ("LAYER_SCHEMATIC_GRID", LAYER_SCHEMATIC_GRID),
        ("LAYER_SCHEMATIC_GRID_AXES", LAYER_SCHEMATIC_GRID_AXES),
        ("LAYER_SCHEMATIC_BACKGROUND", LAYER_SCHEMATIC_BACKGROUND),
        ("LAYER_SCHEMATIC_CURSOR", LAYER_SCHEMATIC_CURSOR),
        ("LAYER_HOVERED", LAYER_HOVERED),
        ("LAYER_BRIGHTENED", LAYER_BRIGHTENED),
        ("LAYER_HIDDEN", LAYER_HIDDEN),
        ("LAYER_NET_COLOR_HIGHLIGHT", LAYER_NET_COLOR_HIGHLIGHT),
        ("LAYER_DRAG_NET_COLLISION", LAYER_DRAG_NET_COLLISION),
        ("LAYER_SELECTION_SHADOWS", LAYER_SELECTION_SHADOWS),
        ("LAYER_SCHEMATIC_DRAWINGSHEET", LAYER_SCHEMATIC_DRAWINGSHEET),
        ("LAYER_SCHEMATIC_PAGE_LIMITS", LAYER_SCHEMATIC_PAGE_LIMITS),
        ("LAYER_BUS_JUNCTION", LAYER_BUS_JUNCTION),
        ("LAYER_SCHEMATIC_AUX_ITEMS", LAYER_SCHEMATIC_AUX_ITEMS),
        ("LAYER_SCHEMATIC_ANCHOR", LAYER_SCHEMATIC_ANCHOR),
        ("LAYER_OP_VOLTAGES", LAYER_OP_VOLTAGES),
        ("LAYER_OP_CURRENTS", LAYER_OP_CURRENTS),
        ("LAYER_GROUP", LAYER_GROUP),
        ("SCH_LAYER_ID_END", SCH_LAYER_ID_END),
        ("GERBVIEW_LAYER_ID_START", GERBVIEW_LAYER_ID_START),
        ("GERBVIEW_LAYER_ID_RESERVED", GERBVIEW_LAYER_ID_RESERVED),
        ("LAYER_DCODES", LAYER_DCODES),
        ("LAYER_NEGATIVE_OBJECTS", LAYER_NEGATIVE_OBJECTS),
        ("LAYER_GERBVIEW_GRID", LAYER_GERBVIEW_GRID),
        ("LAYER_GERBVIEW_AXES", LAYER_GERBVIEW_AXES),
        ("LAYER_GERBVIEW_BACKGROUND", LAYER_GERBVIEW_BACKGROUND),
        ("LAYER_GERBVIEW_DRAWINGSHEET", LAYER_GERBVIEW_DRAWINGSHEET),
        ("LAYER_GERBVIEW_PAGE_LIMITS", LAYER_GERBVIEW_PAGE_LIMITS),
        ("GERBVIEW_LAYER_ID_END", GERBVIEW_LAYER_ID_END),
        ("LAYER_3D_START", LAYER_3D_START),
        ("LAYER_3D_BACKGROUND_BOTTOM", LAYER_3D_BACKGROUND_BOTTOM),
        ("LAYER_3D_BACKGROUND_TOP", LAYER_3D_BACKGROUND_TOP),
        ("LAYER_3D_BOARD", LAYER_3D_BOARD),
        ("LAYER_3D_COPPER_TOP", LAYER_3D_COPPER_TOP),
        ("LAYER_3D_COPPER_BOTTOM", LAYER_3D_COPPER_BOTTOM),
        ("LAYER_3D_SILKSCREEN_BOTTOM", LAYER_3D_SILKSCREEN_BOTTOM),
        ("LAYER_3D_SILKSCREEN_TOP", LAYER_3D_SILKSCREEN_TOP),
        ("LAYER_3D_SOLDERMASK_BOTTOM", LAYER_3D_SOLDERMASK_BOTTOM),
        ("LAYER_3D_SOLDERMASK_TOP", LAYER_3D_SOLDERMASK_TOP),
        ("LAYER_3D_SOLDERPASTE", LAYER_3D_SOLDERPASTE),
        ("LAYER_3D_ADHESIVE", LAYER_3D_ADHESIVE),
        ("LAYER_3D_USER_COMMENTS", LAYER_3D_USER_COMMENTS),
        ("LAYER_3D_USER_DRAWINGS", LAYER_3D_USER_DRAWINGS),
        ("LAYER_3D_USER_ECO1", LAYER_3D_USER_ECO1),
        ("LAYER_3D_USER_ECO2", LAYER_3D_USER_ECO2),
        ("LAYER_3D_F_FAB", LAYER_3D_F_FAB),
        ("LAYER_3D_B_FAB", LAYER_3D_B_FAB),
        ("LAYER_3D_F_COURTYARD", LAYER_3D_F_COURTYARD),
        ("LAYER_3D_B_COURTYARD", LAYER_3D_B_COURTYARD),
        ("LAYER_3D_USER_1", LAYER_3D_USER_1),
        ("LAYER_3D_USER_2", LAYER_3D_USER_2),
        ("LAYER_3D_USER_3", LAYER_3D_USER_3),
        ("LAYER_3D_USER_4", LAYER_3D_USER_4),
        ("LAYER_3D_USER_5", LAYER_3D_USER_5),
        ("LAYER_3D_USER_6", LAYER_3D_USER_6),
        ("LAYER_3D_USER_7", LAYER_3D_USER_7),
        ("LAYER_3D_USER_8", LAYER_3D_USER_8),
        ("LAYER_3D_USER_9", LAYER_3D_USER_9),
        ("LAYER_3D_USER_10", LAYER_3D_USER_10),
        ("LAYER_3D_USER_11", LAYER_3D_USER_11),
        ("LAYER_3D_USER_12", LAYER_3D_USER_12),
        ("LAYER_3D_USER_13", LAYER_3D_USER_13),
        ("LAYER_3D_USER_14", LAYER_3D_USER_14),
        ("LAYER_3D_USER_15", LAYER_3D_USER_15),
        ("LAYER_3D_USER_16", LAYER_3D_USER_16),
        ("LAYER_3D_USER_17", LAYER_3D_USER_17),
        ("LAYER_3D_USER_18", LAYER_3D_USER_18),
        ("LAYER_3D_USER_19", LAYER_3D_USER_19),
        ("LAYER_3D_USER_20", LAYER_3D_USER_20),
        ("LAYER_3D_USER_21", LAYER_3D_USER_21),
        ("LAYER_3D_USER_22", LAYER_3D_USER_22),
        ("LAYER_3D_USER_23", LAYER_3D_USER_23),
        ("LAYER_3D_USER_24", LAYER_3D_USER_24),
        ("LAYER_3D_USER_25", LAYER_3D_USER_25),
        ("LAYER_3D_USER_26", LAYER_3D_USER_26),
        ("LAYER_3D_USER_27", LAYER_3D_USER_27),
        ("LAYER_3D_USER_28", LAYER_3D_USER_28),
        ("LAYER_3D_USER_29", LAYER_3D_USER_29),
        ("LAYER_3D_USER_30", LAYER_3D_USER_30),
        ("LAYER_3D_USER_31", LAYER_3D_USER_31),
        ("LAYER_3D_USER_32", LAYER_3D_USER_32),
        ("LAYER_3D_USER_33", LAYER_3D_USER_33),
        ("LAYER_3D_USER_34", LAYER_3D_USER_34),
        ("LAYER_3D_USER_35", LAYER_3D_USER_35),
        ("LAYER_3D_USER_36", LAYER_3D_USER_36),
        ("LAYER_3D_USER_37", LAYER_3D_USER_37),
        ("LAYER_3D_USER_38", LAYER_3D_USER_38),
        ("LAYER_3D_USER_39", LAYER_3D_USER_39),
        ("LAYER_3D_USER_40", LAYER_3D_USER_40),
        ("LAYER_3D_USER_41", LAYER_3D_USER_41),
        ("LAYER_3D_USER_42", LAYER_3D_USER_42),
        ("LAYER_3D_USER_43", LAYER_3D_USER_43),
        ("LAYER_3D_USER_44", LAYER_3D_USER_44),
        ("LAYER_3D_USER_45", LAYER_3D_USER_45),
        ("LAYER_3D_TH_MODELS", LAYER_3D_TH_MODELS),
        ("LAYER_3D_SMD_MODELS", LAYER_3D_SMD_MODELS),
        ("LAYER_3D_VIRTUAL_MODELS", LAYER_3D_VIRTUAL_MODELS),
        ("LAYER_3D_MODELS_NOT_IN_POS", LAYER_3D_MODELS_NOT_IN_POS),
        ("LAYER_3D_MODELS_MARKED_DNP", LAYER_3D_MODELS_MARKED_DNP),
        ("LAYER_3D_NAVIGATOR", LAYER_3D_NAVIGATOR),
        ("LAYER_3D_BOUNDING_BOXES", LAYER_3D_BOUNDING_BOXES),
        ("LAYER_3D_OFF_BOARD_SILK", LAYER_3D_OFF_BOARD_SILK),
        ("LAYER_3D_PLATED_BARRELS", LAYER_3D_PLATED_BARRELS),
        ("LAYER_3D_COPPER_INNER_1", LAYER_3D_COPPER_INNER_1),
        ("LAYER_3D_COPPER_INNER_2", LAYER_3D_COPPER_INNER_2),
        ("LAYER_3D_COPPER_INNER_3", LAYER_3D_COPPER_INNER_3),
        ("LAYER_3D_COPPER_INNER_4", LAYER_3D_COPPER_INNER_4),
        ("LAYER_3D_COPPER_INNER_5", LAYER_3D_COPPER_INNER_5),
        ("LAYER_3D_COPPER_INNER_6", LAYER_3D_COPPER_INNER_6),
        ("LAYER_3D_COPPER_INNER_7", LAYER_3D_COPPER_INNER_7),
        ("LAYER_3D_COPPER_INNER_8", LAYER_3D_COPPER_INNER_8),
        ("LAYER_3D_COPPER_INNER_9", LAYER_3D_COPPER_INNER_9),
        ("LAYER_3D_COPPER_INNER_10", LAYER_3D_COPPER_INNER_10),
        ("LAYER_3D_COPPER_INNER_11", LAYER_3D_COPPER_INNER_11),
        ("LAYER_3D_COPPER_INNER_12", LAYER_3D_COPPER_INNER_12),
        ("LAYER_3D_COPPER_INNER_13", LAYER_3D_COPPER_INNER_13),
        ("LAYER_3D_COPPER_INNER_14", LAYER_3D_COPPER_INNER_14),
        ("LAYER_3D_COPPER_INNER_15", LAYER_3D_COPPER_INNER_15),
        ("LAYER_3D_COPPER_INNER_16", LAYER_3D_COPPER_INNER_16),
        ("LAYER_3D_COPPER_INNER_17", LAYER_3D_COPPER_INNER_17),
        ("LAYER_3D_COPPER_INNER_18", LAYER_3D_COPPER_INNER_18),
        ("LAYER_3D_COPPER_INNER_19", LAYER_3D_COPPER_INNER_19),
        ("LAYER_3D_COPPER_INNER_20", LAYER_3D_COPPER_INNER_20),
        ("LAYER_3D_COPPER_INNER_21", LAYER_3D_COPPER_INNER_21),
        ("LAYER_3D_COPPER_INNER_22", LAYER_3D_COPPER_INNER_22),
        ("LAYER_3D_COPPER_INNER_23", LAYER_3D_COPPER_INNER_23),
        ("LAYER_3D_COPPER_INNER_24", LAYER_3D_COPPER_INNER_24),
        ("LAYER_3D_COPPER_INNER_25", LAYER_3D_COPPER_INNER_25),
        ("LAYER_3D_COPPER_INNER_26", LAYER_3D_COPPER_INNER_26),
        ("LAYER_3D_COPPER_INNER_27", LAYER_3D_COPPER_INNER_27),
        ("LAYER_3D_COPPER_INNER_28", LAYER_3D_COPPER_INNER_28),
        ("LAYER_3D_COPPER_INNER_29", LAYER_3D_COPPER_INNER_29),
        ("LAYER_3D_COPPER_INNER_30", LAYER_3D_COPPER_INNER_30),
        ("LAYER_3D_END", LAYER_3D_END),
        ("GAL_LAYER_ID_COUNT", GAL_LAYER_ID_COUNT),
        ("SCH_LAYER_ID_COUNT", SCH_LAYER_ID_COUNT),
        ("LAYER_ID_COUNT", LAYER_ID_COUNT),
        ("MIN_VISIBILITY_MASK", MIN_VISIBILITY_MASK),
    ];

    let rust: BTreeMap<&str, i32> = table.iter().copied().collect();
    assert_eq!(rust.len(), table.len(), "duplicate names in the table");

    let golden = include_str!("golden/layer_ids_values.txt");
    let mut checked = 0;

    for line in golden.lines() {
        let (name, value) = line.split_once(' ').expect("NAME value");
        let value: i32 = value.parse().expect("integer value");

        assert_eq!(rust.get(name).copied(), Some(value), "{name}");
        checked += 1;
    }

    assert_eq!(checked, table.len());
}

#[test]
fn gui_layer_helpers() {
    assert_eq!(GAL_UI_LAYER_COUNT, 10);
    assert_eq!(GERBER_DRAWLAYERS_COUNT, PCB_LAYER_ID_COUNT);
    assert_eq!(netnames_layer_index(PcbLayerId::FCu), NETNAMES_LAYER_ID_START);
    assert_eq!(netnames_layer_index(PcbLayerId::BCu), NETNAMES_LAYER_ID_START + 2);
    assert_eq!(gal_layer_index(LAYER_VIAS), 0);
    assert_eq!(gal_layer_index(LAYER_RATSNEST), 11);
    assert_eq!(sch_layer_index(LAYER_BUS), 1);
    assert_eq!(bitmap_layer_for(PcbLayerId::BCu), LAYER_BITMAP_START + 2);
    assert_eq!(zone_layer_for(PcbLayerId::In1Cu), LAYER_ZONE_START + 4);
    assert_eq!(pad_copper_layer_for(PcbLayerId::FCu), LAYER_PAD_COPPER_START);
    assert_eq!(via_copper_layer_for(PcbLayerId::FCu), LAYER_VIA_COPPER_START);
    assert_eq!(clearance_layer_for(PcbLayerId::FCu), LAYER_CLEARANCE_START);
    assert_eq!(point_layer_for(PcbLayerId::FCu), LAYER_POINT_START);
    assert_eq!(drill_symbol_layer_for(PcbLayerId::BCu), LAYER_DRILL_SYMBOL_START + 2);
    assert_eq!(
        board_layer_for_drill_symbol(LAYER_DRILL_SYMBOL_START + 2),
        PcbLayerId::BCu
    );
    assert_eq!(gerber_draw_layer(3), GERBVIEW_LAYER_ID_START + 3);
    assert_eq!(gerber_dcode_layer(3), GERBER_DRAWLAYERS_COUNT + 3);
    assert_eq!(gerber_draw_layer_index(GERBVIEW_LAYER_ID_START + 3), 3);

    assert!(is_via_pad_layer(LAYER_VIA_THROUGH));
    assert!(is_via_pad_layer(LAYER_VIA_MICROVIA));
    assert!(!is_via_pad_layer(LAYER_VIAS));
    assert!(is_hole_layer(LAYER_PAD_PLATEDHOLES));
    assert!(is_hole_layer(LAYER_NON_PLATEDHOLES));
    assert!(!is_hole_layer(LAYER_PADS));
    assert!(is_solder_mask_layer(PcbLayerId::FMask));
    assert!(is_solder_mask_layer(PcbLayerId::BMask));
    assert!(!is_solder_mask_layer(PcbLayerId::FPaste));

    assert_eq!(get_netname_layer(PcbLayerId::FCu), NETNAMES_LAYER_ID_START);
    assert_eq!(
        get_netname_layer(LAYER_VIA_THROUGH),
        NETNAMES_LAYER_ID_START + LAYER_VIA_THROUGH
    );
    assert_eq!(get_netname_layer(PcbLayerId::FSilkS), PcbLayerId::CmtsUser.0);
    assert!(is_netname_layer(NETNAMES_LAYER_ID_START));
    assert!(is_netname_layer(LAYER_VIA_NETNAMES));
    assert!(!is_netname_layer(NETNAMES_LAYER_ID_END));
    assert!(!is_netname_layer(PcbLayerId::FCu));

    assert!(is_zone_fill_layer(LAYER_ZONE_START));
    assert!(is_zone_fill_layer(LAYER_ZONE_END));
    assert!(!is_zone_fill_layer(LAYER_ZONE_END + 1));
    assert!(is_pad_copper_layer(LAYER_PAD_COPPER_START));
    assert!(!is_pad_copper_layer(LAYER_PAD_COPPER_START - 1));
    assert!(is_via_copper_layer(LAYER_VIA_COPPER_END));
    assert!(is_clearance_layer(LAYER_CLEARANCE_START));
    assert!(is_points_layer(LAYER_POINT_END));
    assert!(is_drill_symbol_layer(LAYER_DRILL_SYMBOL_START));
    assert!(!is_drill_symbol_layer(LAYER_DRILL_SYMBOL_END));
    assert!(is_dcode_layer(GERBVIEW_LAYER_ID_START + GERBER_DRAWLAYERS_COUNT));
    assert!(!is_dcode_layer(GERBVIEW_LAYER_ID_START));
    assert!(!is_dcode_layer(GERBVIEW_LAYER_ID_START + 2 * GERBER_DRAWLAYERS_COUNT));

    // Synthetic copper layers: anything that is not an (odd) non-copper board layer.
    assert!(is_copper_layer_incl_synthetic(LAYER_VIA_THROUGH, true));
    assert!(!is_copper_layer_incl_synthetic(LAYER_VIA_THROUGH, false));
    assert!(is_copper_layer_incl_synthetic(PcbLayerId::FCu, false));
    assert!(!is_copper_layer_incl_synthetic(PcbLayerId::FMask, true));

    assert_eq!(to_layer_id(i32::MAX), PcbLayerId::BCu);
    assert_eq!(to_layer_id(5), PcbLayerId::FSilkS);
    assert_eq!(Flashing::default(), Flashing::Default);
    assert_ne!(Flashing::AlwaysFlashed, Flashing::NeverFlashed);
}

#[test]
fn layer_classification() {
    use PcbLayerId as L;

    assert!(is_valid_layer(0));
    assert!(is_valid_layer(127));
    assert!(!is_valid_layer(128));
    assert!(!is_valid_layer(-1));
    assert!(is_pcb_layer(L::User45));
    assert!(!is_pcb_layer(L::UndefinedLayer));
    assert!(!is_pcb_layer(128));

    assert!(is_external_copper_layer(L::FCu));
    assert!(is_external_copper_layer(L::BCu));
    assert!(!is_external_copper_layer(L::In1Cu));
    assert!(is_inner_copper_layer(L::In1Cu));
    assert!(is_inner_copper_layer(L::In30Cu));
    assert!(is_inner_copper_layer(126)); // any even id below PCB_LAYER_ID_COUNT
    assert!(!is_inner_copper_layer(128));
    assert!(!is_inner_copper_layer(L::FCu));
    assert!(!is_inner_copper_layer(L::FMask));

    assert!(is_non_copper_layer(L::FMask));
    assert!(is_non_copper_layer(L::User45));
    assert!(!is_non_copper_layer(L::FCu));
    // Parity test only: UNDEFINED_LAYER (-1) is "non copper", UNSELECTED_LAYER (-2) is not.
    assert!(is_non_copper_layer(L::UndefinedLayer));
    assert!(!is_non_copper_layer(L::UnselectedLayer));
    assert!(!is_non_copper_layer(129));

    assert!(is_user_layer(L::DwgsUser));
    assert!(is_user_layer(L::CmtsUser));
    assert!(is_user_layer(L::Eco1User));
    assert!(is_user_layer(L::Eco2User));
    assert!(is_user_layer(L::User1));
    assert!(is_user_layer(L::User45));
    assert!(!is_user_layer(L::EdgeCuts));
    assert!(!is_user_layer(L::Margin));
    assert!(!is_user_layer(L::Rescue));
    assert!(!is_user_layer(L::In20Cu));
    assert!(!is_user_layer(L::FFab));

    let front = [L::FCu, L::FAdhes, L::FPaste, L::FSilkS, L::FMask, L::FCrtYd, L::FFab];
    let back = [L::BCu, L::BAdhes, L::BPaste, L::BSilkS, L::BMask, L::BCrtYd, L::BFab];

    for id in -2..PCB_LAYER_ID_COUNT {
        let layer = PcbLayerId(id);
        assert_eq!(is_front_layer(layer), front.contains(&layer), "{layer:?}");
        assert_eq!(is_back_layer(layer), back.contains(&layer), "{layer:?}");
    }

    for (f, b) in front.iter().zip(&back) {
        assert_eq!(flip_layer(*f, 0), *b);
        assert_eq!(flip_layer(*b, 0), *f);
        assert_eq!(flip_layer(*f, 6), *b);
    }

    // Inner layers only flip when the layer count is known (>= 4).
    assert_eq!(flip_layer(L::In1Cu, 0), L::In1Cu);
    assert_eq!(flip_layer(L::In1Cu, 2), L::In1Cu);
    assert_eq!(flip_layer(L::In1Cu, 4), L::In2Cu);
    assert_eq!(flip_layer(L::In2Cu, 4), L::In1Cu);
    assert_eq!(flip_layer(L::In1Cu, 6), L::In4Cu);
    assert_eq!(flip_layer(L::In2Cu, 6), L::In3Cu);
    assert_eq!(flip_layer(L::In4Cu, 6), L::In1Cu);
    // An inner layer outside the stack is clamped into it.
    assert_eq!(flip_layer(L::In10Cu, 4), L::In1Cu);
    assert_eq!(flip_layer(L::EdgeCuts, 6), L::EdgeCuts);
    assert_eq!(flip_layer(L::User3, 6), L::User3);
    assert_eq!(flip_layer(L::UndefinedLayer, 6), L::UndefinedLayer);

    assert_eq!(copper_layer_to_ordinal(L::FCu), 0);
    assert_eq!(copper_layer_to_ordinal(L::In1Cu), 1);
    assert_eq!(copper_layer_to_ordinal(L::In30Cu), 30);
    assert_eq!(copper_layer_to_ordinal(L::BCu), 31);
    assert_eq!(copper_layer_to_ordinal(L::FMask), 0);
    assert_eq!(copper_layer_to_ordinal(L::UndefinedLayer), 0);
}

#[test]
fn layer_names() {
    use PcbLayerId as L;

    // Default display names (LayerName()); the canonical file names are LSET::Name().
    assert_eq!(layer_name(L::FCu), "F.Cu");
    assert_eq!(layer_name(L::BCu), "B.Cu");
    assert_eq!(layer_name(L::In1Cu), "In1.Cu");
    assert_eq!(layer_name(L::In30Cu), "In30.Cu");
    assert_eq!(layer_name(L::FSilkS), "F.Silkscreen");
    assert_eq!(layer_name(L::BAdhes), "B.Adhesive");
    assert_eq!(layer_name(L::FCrtYd), "F.Courtyard");
    assert_eq!(layer_name(L::DwgsUser), "User.Drawings");
    assert_eq!(layer_name(L::Eco2User), "User.Eco2");
    assert_eq!(layer_name(L::EdgeCuts), "Edge.Cuts");
    assert_eq!(layer_name(L::Rescue), "Rescue");
    assert_eq!(layer_name(L::User1), "User.1");
    assert_eq!(layer_name(L::User45), "User.45");
    assert_eq!(layer_name(L::UndefinedLayer), "undefined");
    assert_eq!(layer_name(L::UnselectedLayer), "undefined");
    assert_eq!(layer_name(LAYER_WIRE), "Wires");
    assert_eq!(layer_name(LAYER_NOTES), "Schematic text && graphics");
    assert_eq!(layer_name(LAYER_VIA_THROUGH), "Through vias");
    assert_eq!(layer_name(LAYER_RATSNEST), "Ratsnest");
    assert_eq!(layer_name(NETNAMES_LAYER_ID_START), "Track net names");
    assert_eq!(layer_name(LAYER_VIA_NETNAMES), "Via net names");
}

#[test]
fn board_layer_from_legacy_id_mapping() {
    use PcbLayerId as L;

    assert_eq!(board_layer_from_legacy_id(0), L::FCu);
    assert_eq!(board_layer_from_legacy_id(1), L::In1Cu);
    assert_eq!(board_layer_from_legacy_id(30), L::In30Cu);
    assert_eq!(board_layer_from_legacy_id(31), L::BCu);
    assert_eq!(board_layer_from_legacy_id(32), L::BAdhes);
    assert_eq!(board_layer_from_legacy_id(33), L::FAdhes);
    assert_eq!(board_layer_from_legacy_id(34), L::BPaste);
    assert_eq!(board_layer_from_legacy_id(35), L::FPaste);
    assert_eq!(board_layer_from_legacy_id(36), L::BSilkS);
    assert_eq!(board_layer_from_legacy_id(37), L::FSilkS);
    assert_eq!(board_layer_from_legacy_id(38), L::BMask);
    assert_eq!(board_layer_from_legacy_id(39), L::FMask);
    assert_eq!(board_layer_from_legacy_id(40), L::DwgsUser);
    assert_eq!(board_layer_from_legacy_id(41), L::CmtsUser);
    assert_eq!(board_layer_from_legacy_id(42), L::Eco1User);
    assert_eq!(board_layer_from_legacy_id(43), L::Eco2User);
    assert_eq!(board_layer_from_legacy_id(44), L::EdgeCuts);
    assert_eq!(board_layer_from_legacy_id(45), L::Margin);
    assert_eq!(board_layer_from_legacy_id(46), L::BCrtYd);
    assert_eq!(board_layer_from_legacy_id(47), L::FCrtYd);
    assert_eq!(board_layer_from_legacy_id(48), L::BFab);
    assert_eq!(board_layer_from_legacy_id(49), L::FFab);
    assert_eq!(board_layer_from_legacy_id(50), L::User1);
    assert_eq!(board_layer_from_legacy_id(58), L::User9);
    assert_eq!(board_layer_from_legacy_id(59), L::Rescue);
    assert_eq!(board_layer_from_legacy_id(60), L::UndefinedLayer);
    assert_eq!(board_layer_from_legacy_id(1000), L::UndefinedLayer);
    assert_eq!(board_layer_from_legacy_id(-1), L::UndefinedLayer);
    assert_eq!(board_layer_from_legacy_id(-2), L::UnselectedLayer);
    assert_eq!(board_layer_from_legacy_id(-3), L::UndefinedLayer);
}
