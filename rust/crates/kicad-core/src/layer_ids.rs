// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2014 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
// Copyright (C) 2010 Jean-Pierre Charras, jp.charras at wanadoo.fr
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

//! Port of `include/layer_ids.h` and `common/layer_id.cpp` (plus `ToLAYER_ID` from
//! `common/lset.cpp`).
//!
//! A quick note on layer IDs (from the C++ header):
//!
//! The layers are stored in separate enums so that certain functions can take in the enums as
//! data types and don't have to know about layers from other applications.  Layers that are
//! shared between applications should be in the `GAL_LAYER_ID` enum.  The `PCB_LAYER_ID`
//! values must start at zero for compatibility with legacy board files.  Some functions accept
//! any layer ID, so they start at zero (i.e. `F_Cu`) and go up to `LAYER_ID_COUNT`.
//!
//! # Representation
//!
//! `PCB_LAYER_ID` is an unscoped C++ enum with underlying type `int`, and the C++ code freely
//! converts between it and `int` (`PCB_LAYER_ID( In1_Cu + 2 * i )`, `ToLAYER_ID( int )`, loops
//! over all ids, ids beyond `PCB_LAYER_ID_COUNT` produced by `LAYER_RANGE`, ...).  It is
//! therefore ported as the transparent newtype [`PcbLayerId`] over `i32` with one associated
//! constant per enumerator (`PCB_LAYER_ID::F_Cu` is [`PcbLayerId::FCu`]), not as a Rust `enum`.
//! The constants can be used in patterns: `match layer { PcbLayerId::FCu => .., _ => .. }`.
//!
//! The copper layers are the even ids (`F_Cu` = 0, `B_Cu` = 2, `In1_Cu` = 4, `In2_Cu` = 6, ...)
//! and the non-copper layers are the odd ids.
//!
//! The GUI-only enums (`NETNAMES_LAYER_ID`, `GAL_LAYER_ID`, `SCH_LAYER_ID`,
//! `GERBVIEW_LAYER_ID`, `LAYER_3D_ID`) are ported as plain `i32` constants carrying their C++
//! names, in the second half of this file.

use std::fmt;

use crate::base_set::BitPos;

/// Port of `enum PCB_LAYER_ID : int`: the definition of all layers used in Pcbnew.
///
/// The PCB layer types are fixed at value 0 through `LAYER_ID_COUNT` to ensure compatibility
/// with legacy board files.
///
/// See the [module documentation](self) for why this is a newtype over `i32` and not an enum.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PcbLayerId(pub i32);

#[allow(non_upper_case_globals)]
impl PcbLayerId {
    pub const UndefinedLayer: PcbLayerId = PcbLayerId(-1);
    pub const UnselectedLayer: PcbLayerId = PcbLayerId(-2);

    pub const FCu: PcbLayerId = PcbLayerId(0);
    pub const BCu: PcbLayerId = PcbLayerId(2);
    pub const In1Cu: PcbLayerId = PcbLayerId(4);
    pub const In2Cu: PcbLayerId = PcbLayerId(6);
    pub const In3Cu: PcbLayerId = PcbLayerId(8);
    pub const In4Cu: PcbLayerId = PcbLayerId(10);
    pub const In5Cu: PcbLayerId = PcbLayerId(12);
    pub const In6Cu: PcbLayerId = PcbLayerId(14);
    pub const In7Cu: PcbLayerId = PcbLayerId(16);
    pub const In8Cu: PcbLayerId = PcbLayerId(18);
    pub const In9Cu: PcbLayerId = PcbLayerId(20);
    pub const In10Cu: PcbLayerId = PcbLayerId(22);
    pub const In11Cu: PcbLayerId = PcbLayerId(24);
    pub const In12Cu: PcbLayerId = PcbLayerId(26);
    pub const In13Cu: PcbLayerId = PcbLayerId(28);
    pub const In14Cu: PcbLayerId = PcbLayerId(30);
    pub const In15Cu: PcbLayerId = PcbLayerId(32);
    pub const In16Cu: PcbLayerId = PcbLayerId(34);
    pub const In17Cu: PcbLayerId = PcbLayerId(36);
    pub const In18Cu: PcbLayerId = PcbLayerId(38);
    pub const In19Cu: PcbLayerId = PcbLayerId(40);
    pub const In20Cu: PcbLayerId = PcbLayerId(42);
    pub const In21Cu: PcbLayerId = PcbLayerId(44);
    pub const In22Cu: PcbLayerId = PcbLayerId(46);
    pub const In23Cu: PcbLayerId = PcbLayerId(48);
    pub const In24Cu: PcbLayerId = PcbLayerId(50);
    pub const In25Cu: PcbLayerId = PcbLayerId(52);
    pub const In26Cu: PcbLayerId = PcbLayerId(54);
    pub const In27Cu: PcbLayerId = PcbLayerId(56);
    pub const In28Cu: PcbLayerId = PcbLayerId(58);
    pub const In29Cu: PcbLayerId = PcbLayerId(60);
    pub const In30Cu: PcbLayerId = PcbLayerId(62);

    pub const FMask: PcbLayerId = PcbLayerId(1);
    pub const BMask: PcbLayerId = PcbLayerId(3);

    pub const FSilkS: PcbLayerId = PcbLayerId(5);
    pub const BSilkS: PcbLayerId = PcbLayerId(7);
    pub const FAdhes: PcbLayerId = PcbLayerId(9);
    pub const BAdhes: PcbLayerId = PcbLayerId(11);
    pub const FPaste: PcbLayerId = PcbLayerId(13);
    pub const BPaste: PcbLayerId = PcbLayerId(15);

    pub const DwgsUser: PcbLayerId = PcbLayerId(17);
    pub const CmtsUser: PcbLayerId = PcbLayerId(19);
    pub const Eco1User: PcbLayerId = PcbLayerId(21);
    pub const Eco2User: PcbLayerId = PcbLayerId(23);

    pub const EdgeCuts: PcbLayerId = PcbLayerId(25);
    pub const Margin: PcbLayerId = PcbLayerId(27);

    pub const BCrtYd: PcbLayerId = PcbLayerId(29);
    pub const FCrtYd: PcbLayerId = PcbLayerId(31);

    pub const BFab: PcbLayerId = PcbLayerId(33);
    pub const FFab: PcbLayerId = PcbLayerId(35);

    pub const Rescue: PcbLayerId = PcbLayerId(37);

    // User definable layers.
    pub const User1: PcbLayerId = PcbLayerId(39);
    pub const User2: PcbLayerId = PcbLayerId(41);
    pub const User3: PcbLayerId = PcbLayerId(43);
    pub const User4: PcbLayerId = PcbLayerId(45);
    pub const User5: PcbLayerId = PcbLayerId(47);
    pub const User6: PcbLayerId = PcbLayerId(49);
    pub const User7: PcbLayerId = PcbLayerId(51);
    pub const User8: PcbLayerId = PcbLayerId(53);
    pub const User9: PcbLayerId = PcbLayerId(55);
    pub const User10: PcbLayerId = PcbLayerId(57);
    pub const User11: PcbLayerId = PcbLayerId(59);
    pub const User12: PcbLayerId = PcbLayerId(61);
    pub const User13: PcbLayerId = PcbLayerId(63);
    pub const User14: PcbLayerId = PcbLayerId(65);
    pub const User15: PcbLayerId = PcbLayerId(67);
    pub const User16: PcbLayerId = PcbLayerId(69);
    pub const User17: PcbLayerId = PcbLayerId(71);
    pub const User18: PcbLayerId = PcbLayerId(73);
    pub const User19: PcbLayerId = PcbLayerId(75);
    pub const User20: PcbLayerId = PcbLayerId(77);
    pub const User21: PcbLayerId = PcbLayerId(79);
    pub const User22: PcbLayerId = PcbLayerId(81);
    pub const User23: PcbLayerId = PcbLayerId(83);
    pub const User24: PcbLayerId = PcbLayerId(85);
    pub const User25: PcbLayerId = PcbLayerId(87);
    pub const User26: PcbLayerId = PcbLayerId(89);
    pub const User27: PcbLayerId = PcbLayerId(91);
    pub const User28: PcbLayerId = PcbLayerId(93);
    pub const User29: PcbLayerId = PcbLayerId(95);
    pub const User30: PcbLayerId = PcbLayerId(97);
    pub const User31: PcbLayerId = PcbLayerId(99);
    pub const User32: PcbLayerId = PcbLayerId(101);
    pub const User33: PcbLayerId = PcbLayerId(103);
    pub const User34: PcbLayerId = PcbLayerId(105);
    pub const User35: PcbLayerId = PcbLayerId(107);
    pub const User36: PcbLayerId = PcbLayerId(109);
    pub const User37: PcbLayerId = PcbLayerId(111);
    pub const User38: PcbLayerId = PcbLayerId(113);
    pub const User39: PcbLayerId = PcbLayerId(115);
    pub const User40: PcbLayerId = PcbLayerId(117);
    pub const User41: PcbLayerId = PcbLayerId(119);
    pub const User42: PcbLayerId = PcbLayerId(121);
    pub const User43: PcbLayerId = PcbLayerId(123);
    pub const User44: PcbLayerId = PcbLayerId(125);
    pub const User45: PcbLayerId = PcbLayerId(127);

    /// The enumerator `PCB_LAYER_ID_COUNT` (= 128) as a layer id; see also the plain integer
    /// constant [`PCB_LAYER_ID_COUNT`].
    pub const PcbLayerIdCount: PcbLayerId = PcbLayerId(PCB_LAYER_ID_COUNT);

    /// The numeric value of the layer id (the implicit `PCB_LAYER_ID` to `int` conversion).
    #[inline]
    pub const fn as_i32(self) -> i32 {
        self.0
    }

    /// The `n`-th inner copper layer, `In<n>_Cu`, i.e. `PCB_LAYER_ID( In1_Cu + ( n - 1 ) * 2 )`.
    #[inline]
    pub const fn inner_cu(n: i32) -> PcbLayerId {
        PcbLayerId(Self::In1Cu.0 + (n - 1) * 2)
    }

    /// The `n`-th user defined layer, `User_<n>`, i.e. `PCB_LAYER_ID( User_1 + ( n - 1 ) * 2 )`.
    #[inline]
    pub const fn user(n: i32) -> PcbLayerId {
        PcbLayerId(Self::User1.0 + (n - 1) * 2)
    }

    /// Name of the C++ enumerator in the Rust spelling (`"FCu"`, `"In3Cu"`, `"User12"`, ...),
    /// or `None` when the value is not one of the named enumerators.
    fn enumerator_name(self) -> Option<String> {
        let fixed = match self {
            Self::UndefinedLayer => "UndefinedLayer",
            Self::UnselectedLayer => "UnselectedLayer",
            Self::FCu => "FCu",
            Self::BCu => "BCu",
            Self::FMask => "FMask",
            Self::BMask => "BMask",
            Self::FSilkS => "FSilkS",
            Self::BSilkS => "BSilkS",
            Self::FAdhes => "FAdhes",
            Self::BAdhes => "BAdhes",
            Self::FPaste => "FPaste",
            Self::BPaste => "BPaste",
            Self::DwgsUser => "DwgsUser",
            Self::CmtsUser => "CmtsUser",
            Self::Eco1User => "Eco1User",
            Self::Eco2User => "Eco2User",
            Self::EdgeCuts => "EdgeCuts",
            Self::Margin => "Margin",
            Self::BCrtYd => "BCrtYd",
            Self::FCrtYd => "FCrtYd",
            Self::BFab => "BFab",
            Self::FFab => "FFab",
            Self::Rescue => "Rescue",
            _ => "",
        };

        if !fixed.is_empty() {
            return Some(fixed.to_string());
        }

        let id = self.0;

        if id >= Self::In1Cu.0 && id <= Self::In30Cu.0 && id % 2 == 0 {
            Some(format!("In{}Cu", (id - Self::In1Cu.0) / 2 + 1))
        } else if id >= Self::User1.0 && id <= Self::User45.0 && id % 2 == 1 {
            Some(format!("User{}", (id - Self::User1.0) / 2 + 1))
        } else {
            None
        }
    }
}

impl fmt::Debug for PcbLayerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.enumerator_name() {
            Some(name) => f.write_str(&name),
            None => write!(f, "PcbLayerId({})", self.0),
        }
    }
}

impl From<PcbLayerId> for i32 {
    #[inline]
    fn from(layer: PcbLayerId) -> i32 {
        layer.0
    }
}

impl From<i32> for PcbLayerId {
    /// The unchecked `PCB_LAYER_ID( int )` cast.
    #[inline]
    fn from(layer: i32) -> PcbLayerId {
        PcbLayerId(layer)
    }
}

impl BitPos for PcbLayerId {
    /// At the moment, LSET cannot store negative layers, but PCB_LAYER_ID can contain them.
    #[inline]
    fn bit_pos(self) -> Option<usize> {
        usize::try_from(self.0).ok()
    }
}

/// `UNDEFINED_LAYER` (-1).
pub const UNDEFINED_LAYER: PcbLayerId = PcbLayerId::UndefinedLayer;

/// `UNSELECTED_LAYER` (-2).
pub const UNSELECTED_LAYER: PcbLayerId = PcbLayerId::UnselectedLayer;

/// The enumerator `PCB_LAYER_ID_COUNT`: the number of board layer ids (and the size of an
/// `LSET`).
pub const PCB_LAYER_ID_COUNT: i32 = 128;

/// `PCBNEW_LAYER_ID_START`.
pub const PCBNEW_LAYER_ID_START: PcbLayerId = PcbLayerId::FCu;

/// `MAX_CU_LAYERS`: the maximum number of copper layers of a board.
pub const MAX_CU_LAYERS: i32 = 32;

/// `MAX_USER_DEFINED_LAYERS`: the number of user definable layers (`User_1` .. `User_45`).
pub const MAX_USER_DEFINED_LAYERS: i32 = 45;

/// Port of `enum class FLASHING`.
///
/// Enum used during connectivity building to ensure we do not query connectivity while
/// building the database.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Flashing {
    /// Flashing follows connectivity.
    #[default]
    Default,
    /// Always flashed for connectivity.
    AlwaysFlashed,
    /// Never flashed for connectivity.
    NeverFlashed,
}

/// Port of the macro `NETNAMES_LAYER_INDEX`: obtain the netname layer for a given PCB layer.
#[inline]
pub fn netnames_layer_index(layer: impl Into<i32>) -> i32 {
    NETNAMES_LAYER_ID_START + layer.into()
}

/// `GAL_UI_LAYER_COUNT`.
pub const GAL_UI_LAYER_COUNT: i32 = 10;

/// Port of the macro `GAL_LAYER_INDEX`: convert a GAL layer to a 0-indexed offset from
/// `LAYER_VIAS`.
#[inline]
pub const fn gal_layer_index(x: i32) -> i32 {
    x - GAL_LAYER_ID_START
}

/// Port of the macro `BITMAP_LAYER_FOR`.
#[inline]
pub fn bitmap_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_BITMAP_START + board_layer.into()
}

/// Port of the macro `ZONE_LAYER_FOR`.
#[inline]
pub fn zone_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_ZONE_START + board_layer.into()
}

/// Port of the macro `PAD_COPPER_LAYER_FOR`.
#[inline]
pub fn pad_copper_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_PAD_COPPER_START + board_layer.into()
}

/// Port of the macro `VIA_COPPER_LAYER_FOR`.
#[inline]
pub fn via_copper_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_VIA_COPPER_START + board_layer.into()
}

/// Port of the macro `CLEARANCE_LAYER_FOR`.
#[inline]
pub fn clearance_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_CLEARANCE_START + board_layer.into()
}

/// Port of the macro `DRILL_SYMBOL_LAYER_FOR`.
#[inline]
pub fn drill_symbol_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_DRILL_SYMBOL_START + board_layer.into()
}

/// Port of the macro `BOARD_LAYER_FOR_DRILL_SYMBOL`.
#[inline]
pub const fn board_layer_for_drill_symbol(gal_layer: i32) -> PcbLayerId {
    PcbLayerId(gal_layer - LAYER_DRILL_SYMBOL_START)
}

/// Port of the macro `POINT_LAYER_FOR`.
#[inline]
pub fn point_layer_for(board_layer: impl Into<i32>) -> i32 {
    LAYER_POINT_START + board_layer.into()
}

/// `GAL_LAYER_ID_COUNT`.
pub const GAL_LAYER_ID_COUNT: i32 = GAL_LAYER_ID_END - GAL_LAYER_ID_START;

/// `SCH_LAYER_ID_COUNT`.
pub const SCH_LAYER_ID_COUNT: i32 = SCH_LAYER_ID_END - SCH_LAYER_ID_START;

/// Port of the macro `SCH_LAYER_INDEX`.
#[inline]
pub const fn sch_layer_index(x: i32) -> i32 {
    x - SCH_LAYER_ID_START
}

/// `GERBER_DRAWLAYERS_COUNT`: number of draw layers in Gerbview.
pub const GERBER_DRAWLAYERS_COUNT: i32 = PCB_LAYER_ID_COUNT;

/// Port of the macro `GERBER_DRAW_LAYER`.
#[inline]
pub const fn gerber_draw_layer(x: i32) -> i32 {
    GERBVIEW_LAYER_ID_START + x
}

/// Port of the macro `GERBER_DCODE_LAYER`.
#[inline]
pub const fn gerber_dcode_layer(x: i32) -> i32 {
    GERBER_DRAWLAYERS_COUNT + x
}

/// Port of the macro `GERBER_DRAW_LAYER_INDEX`.
#[inline]
pub const fn gerber_draw_layer_index(x: i32) -> i32 {
    x - GERBVIEW_LAYER_ID_START
}

/// `LAYER_ID_COUNT`.  Must update this if you add any enums after Gerbview!
pub const LAYER_ID_COUNT: i32 = LAYER_3D_END;

/// Port of the macro `MIN_VISIBILITY_MASK`.
///
/// Some elements do not have yet a visibility control from a dialog, but have a visibility
/// control flag.  Here is a mask to set them visible, to be sure they are displayed after
/// loading a board for instance.
pub const MIN_VISIBILITY_MASK: i32 = (1 << gal_layer_index(LAYER_PAD_PLATEDHOLES))
    + (1 << gal_layer_index(LAYER_VIA_HOLES))
    + (1 << gal_layer_index(LAYER_SELECT_OVERLAY))
    + (1 << gal_layer_index(LAYER_GP_OVERLAY))
    + (1 << gal_layer_index(LAYER_RATSNEST));

/// Port of `LayerName( int aLayer )`: return the string equivalent of a given layer.
///
/// Returns the default display name for a given layer.  These are not the same as the
/// canonical name in `LSET::Name()` ([`crate::lset::Lset::name`]), which is used in board
/// files and cannot be translated or changed.
///
/// WARNING: do not translate board physical layers names (F.Cu to User.9): because canonical
/// names are used in files (boards and fab files), using translated names in UI create
/// mistakes for users.  Board physical layers names must be seen as proper nouns.
///
/// The strings wrapped in `_()` in C++ are returned untranslated (English).
pub fn layer_name(layer: impl Into<i32>) -> String {
    let layer: i32 = layer.into();

    // PCB_LAYER_ID
    let pcb_name = match PcbLayerId(layer) {
        PcbLayerId::UnselectedLayer | PcbLayerId::UndefinedLayer => "undefined",

        // Copper
        PcbLayerId::FCu => "F.Cu",
        PcbLayerId::BCu => "B.Cu",

        // Technicals
        PcbLayerId::BAdhes => "B.Adhesive",
        PcbLayerId::FAdhes => "F.Adhesive",
        PcbLayerId::BPaste => "B.Paste",
        PcbLayerId::FPaste => "F.Paste",
        PcbLayerId::BSilkS => "B.Silkscreen",
        PcbLayerId::FSilkS => "F.Silkscreen",
        PcbLayerId::BMask => "B.Mask",
        PcbLayerId::FMask => "F.Mask",

        // Users
        PcbLayerId::DwgsUser => "User.Drawings",
        PcbLayerId::CmtsUser => "User.Comments",
        PcbLayerId::Eco1User => "User.Eco1",
        PcbLayerId::Eco2User => "User.Eco2",
        PcbLayerId::EdgeCuts => "Edge.Cuts",
        PcbLayerId::Margin => "Margin",

        // Footprint
        PcbLayerId::FCrtYd => "F.Courtyard",
        PcbLayerId::BCrtYd => "B.Courtyard",
        PcbLayerId::FFab => "F.Fab",
        PcbLayerId::BFab => "B.Fab",

        // Rescue
        PcbLayerId::Rescue => "Rescue",

        _ => "",
    };

    if !pcb_name.is_empty() {
        return pcb_name.to_string();
    }

    let name = match layer {
        // SCH_LAYER_ID
        LAYER_WIRE => "Wires",
        LAYER_BUS => "Buses",
        LAYER_BUS_JUNCTION => "Bus junctions",
        LAYER_JUNCTION => "Junctions",
        LAYER_LOCLABEL => "Labels",
        LAYER_GLOBLABEL => "Global labels",
        LAYER_HIERLABEL => "Hierarchical labels",
        LAYER_PINNUM => "Pin numbers",
        LAYER_PINNAM => "Pin names",
        LAYER_REFERENCEPART => "Symbol references",
        LAYER_VALUEPART => "Symbol values",
        LAYER_FIELDS => "Symbol fields",
        LAYER_INTERSHEET_REFS => "Sheet references",
        LAYER_NETCLASS_REFS => "Net class references",
        LAYER_RULE_AREAS => "Rule areas",
        LAYER_DEVICE => "Symbol body outlines",
        LAYER_DEVICE_BACKGROUND => "Symbol body fills",
        LAYER_SHAPES_BACKGROUND => "Shape fills",
        LAYER_NOTES => "Schematic text && graphics",
        LAYER_PRIVATE_NOTES => "Symbol private text && graphics",
        LAYER_NOTES_BACKGROUND => "Schematic text && graphics backgrounds",
        LAYER_PIN => "Pins",
        LAYER_SHEET => "Sheet borders",
        LAYER_SHEET_BACKGROUND => "Sheet backgrounds",
        LAYER_SHEETNAME => "Sheet names",
        LAYER_SHEETFIELDS => "Sheet fields",
        LAYER_SHEETFILENAME => "Sheet file names",
        LAYER_SHEETLABEL => "Sheet pins",
        LAYER_NOCONNECT => "No-connect symbols",
        LAYER_DNP_MARKER => "DNP markers",
        LAYER_EXCLUDED_FROM_SIM => "Excluded-from-simulation markers",
        LAYER_ERC_WARN => "ERC warnings",
        LAYER_ERC_ERR => "ERC errors",
        LAYER_ERC_EXCLUSION => "ERC exclusions",
        LAYER_SCHEMATIC_ANCHOR => "Anchors",
        LAYER_SCHEMATIC_AUX_ITEMS => "Helper items",
        LAYER_SCHEMATIC_GRID => "Grid",
        LAYER_SCHEMATIC_GRID_AXES => "Axes",
        LAYER_SCHEMATIC_BACKGROUND => "Background",
        LAYER_SCHEMATIC_CURSOR => "Cursor",
        LAYER_HOVERED => "Hovered items",
        LAYER_BRIGHTENED => "Highlighted items",
        LAYER_HIDDEN => "Hidden items",
        LAYER_NET_COLOR_HIGHLIGHT => "Net color highlight",
        LAYER_DRAG_NET_COLLISION => "Drag net collisions",
        LAYER_SELECTION_SHADOWS => "Selection highlight",
        LAYER_SCHEMATIC_DRAWINGSHEET => "Drawing sheet",
        LAYER_SCHEMATIC_PAGE_LIMITS => "Page limits",
        LAYER_OP_VOLTAGES => "Operating point voltages",
        LAYER_OP_CURRENTS => "Operating point currents",

        // GAL_LAYER_ID
        LAYER_FOOTPRINTS_FR => "Footprints front",
        LAYER_FOOTPRINTS_BK => "Footprints back",
        LAYER_FP_VALUES => "Values",
        LAYER_FP_REFERENCES => "Reference designators",
        LAYER_FP_TEXT => "Footprint text",
        LAYER_TRACKS => "Tracks",
        LAYER_VIA_THROUGH => "Through vias",
        LAYER_VIA_BLIND => "Blind vias",
        LAYER_VIA_BURIED => "Buried vias",
        LAYER_VIA_MICROVIA => "Micro-vias",
        LAYER_VIA_HOLES => "Via holes",
        LAYER_VIA_HOLEWALLS => "Via hole walls",
        LAYER_PAD_PLATEDHOLES => "Plated holes",
        LAYER_PAD_HOLEWALLS => "Plated hole walls",
        LAYER_NON_PLATEDHOLES => "Non-plated holes",
        LAYER_RATSNEST => "Ratsnest",
        LAYER_DRC_WARNING => "DRC warnings",
        LAYER_DRC_ERROR => "DRC errors",
        LAYER_DRC_EXCLUSION => "DRC exclusions",
        LAYER_MARKER_SHADOWS => "DRC marker shadows",
        LAYER_DRC_HIGHLIGHTED => "DRC highlighted",
        LAYER_ANCHOR => "Anchors",
        LAYER_POINTS => "Points",
        LAYER_SUBGRIDS => "Grids",
        LAYER_VIA_STITCHING => "Via stitching",
        LAYER_DRAWINGSHEET => "Drawing sheet",
        LAYER_PAGE_LIMITS => "Page limits",
        LAYER_CURSOR => "Cursor",
        LAYER_AUX_ITEMS => "Helper items",
        LAYER_GRID => "Grid",
        LAYER_GRID_AXES => "Grid axes",
        LAYER_PCB_BACKGROUND => "Background",
        LAYER_SELECT_OVERLAY => "Selection highlight",
        LAYER_LOCKED_ITEM_SHADOW => "Locked item shadow",
        LAYER_CONFLICTS_SHADOW => "Courtyard collision shadow",
        LAYER_CONSTRAINT_SHADOW => "Constrained item shadow",
        LAYER_CONSTRAINT_UNDER => "Under-constrained tint",
        LAYER_CONSTRAINT_WELL => "Fully constrained tint",
        LAYER_CONSTRAINT_OVER => "Over-constrained tint",
        LAYER_ROUTER_PREVIEW_SHADOW => "Router clearance shadow",
        LAYER_ROUTER_PREVIEW_COLLISION => "Router collision marker",
        LAYER_OUTSIDE_GROUP_OPACITY => "Items outside group opacity",
        LAYER_BOARD_OUTLINE_AREA => "Board outline area",
        NETNAMES_LAYER_ID_START => "Track net names",
        LAYER_PAD_NETNAMES => "Pad net names",
        LAYER_VIA_NETNAMES => "Via net names",

        _ => {
            // Catch the general board layers that have numerically increasing names
            if layer > 0 && layer < PCB_LAYER_ID_COUNT && (layer & 1) != 0 {
                return format!("User.{}", (layer - PcbLayerId::User1.0) / 2 + 1);
            }

            // The C++ arithmetic is plain `int`; wrap instead of overflowing for absurd ids.
            return format!(
                "In{}.Cu",
                layer.wrapping_sub(PcbLayerId::In1Cu.0) / 2 + 1
            );
        }
    };

    name.to_string()
}

/// Port of `IsValidLayer`: test whether a given integer is a valid layer index, i.e. can be
/// safely put in a `PCB_LAYER_ID`.
///
/// `layer_id` can be an int, so it is useful during I/O.
#[inline]
pub fn is_valid_layer(layer_id: impl Into<i32>) -> bool {
    (layer_id.into() as u32) < PCB_LAYER_ID_COUNT as u32
}

/// Port of `IsPcbLayer`: test whether a layer is a valid layer for Pcbnew.
#[inline]
pub fn is_pcb_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= PcbLayerId::FCu.0 && layer < PCB_LAYER_ID_COUNT
}

/// Port of `IsCopperLayer( int aLayerId )`: test whether a layer is a (valid) copper layer.
#[inline]
pub fn is_copper_layer(layer_id: impl Into<i32>) -> bool {
    let layer_id: i32 = layer_id.into();
    (layer_id & 1) == 0 && layer_id < PCB_LAYER_ID_COUNT && layer_id >= 0
}

/// Port of `IsExternalCopperLayer`: test whether a layer is an external (`F_Cu` or `B_Cu`)
/// copper layer.
#[inline]
pub fn is_external_copper_layer(layer_id: impl Into<i32>) -> bool {
    let layer_id: i32 = layer_id.into();
    layer_id == PcbLayerId::FCu.0 || layer_id == PcbLayerId::BCu.0
}

/// Port of `IsInnerCopperLayer`: test whether a layer is an inner (`In1_Cu` to `In30_Cu`)
/// copper layer.
#[inline]
pub fn is_inner_copper_layer(layer_id: impl Into<i32>) -> bool {
    let layer_id: i32 = layer_id.into();
    is_copper_layer(layer_id) && !is_external_copper_layer(layer_id)
}

/// Port of `IsNonCopperLayer`: test whether a layer is a non copper layer.
///
/// As in C++ this is purely a test on the parity of the id (and an upper bound), so it is
/// also true for `UNDEFINED_LAYER` (-1).
#[inline]
pub fn is_non_copper_layer(layer_id: impl Into<i32>) -> bool {
    let layer_id: i32 = layer_id.into();
    (layer_id & 1) != 0 && layer_id <= PCB_LAYER_ID_COUNT
}

/// Port of `IsCopperLayer( int aLayerId, bool aIncludeSyntheticCopperLayers )`: test whether
/// a layer is a copper layer, optionally including synthetic copper layers such as
/// `LAYER_VIA_THROUGH`, `LAYER_PADS`, etc.
#[inline]
pub fn is_copper_layer_incl_synthetic(
    layer_id: impl Into<i32>,
    include_synthetic_copper_layers: bool,
) -> bool {
    let layer_id: i32 = layer_id.into();

    if include_synthetic_copper_layers {
        !is_non_copper_layer(layer_id)
    } else {
        is_copper_layer(layer_id)
    }
}

/// Port of `IsViaPadLayer`.
#[inline]
pub fn is_via_pad_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();

    layer == LAYER_VIA_THROUGH
        || layer == LAYER_VIA_MICROVIA
        || layer == LAYER_VIA_BLIND
        || layer == LAYER_VIA_BURIED
}

/// Port of `IsHoleLayer`.
#[inline]
pub fn is_hole_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();

    layer == LAYER_VIA_HOLES
        || layer == LAYER_VIA_HOLEWALLS
        || layer == LAYER_PAD_PLATEDHOLES
        || layer == LAYER_PAD_HOLEWALLS
        || layer == LAYER_NON_PLATEDHOLES
}

/// Port of `IsSolderMaskLayer`.
#[inline]
pub fn is_solder_mask_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer == PcbLayerId::FMask.0 || layer == PcbLayerId::BMask.0
}

/// Port of `IsUserLayer`: test whether a layer is a non copper and a non tech layer.
#[inline]
pub fn is_user_layer(layer_id: PcbLayerId) -> bool {
    layer_id == PcbLayerId::DwgsUser
        || layer_id == PcbLayerId::CmtsUser
        || layer_id == PcbLayerId::Eco1User
        || layer_id == PcbLayerId::Eco2User
        || (layer_id >= PcbLayerId::User1 && !is_copper_layer(layer_id))
}

// IMPORTANT: If a layer is not a front layer that doesn't necessarily mean it's a back layer.
//
// So a layer can be:
//   - Front
//   - Back
//   - Neither (internal or auxiliary)
//
// The check most frequent is for back layers, since it involves flips.

/// Port of `IsFrontLayer`: layer classification: check if it's a front layer.
#[inline]
pub fn is_front_layer(layer_id: PcbLayerId) -> bool {
    matches!(
        layer_id,
        PcbLayerId::FCu
            | PcbLayerId::FAdhes
            | PcbLayerId::FPaste
            | PcbLayerId::FSilkS
            | PcbLayerId::FMask
            | PcbLayerId::FCrtYd
            | PcbLayerId::FFab
    )
}

/// Port of `IsBackLayer`: layer classification: check if it's a back layer.
#[inline]
pub fn is_back_layer(layer_id: PcbLayerId) -> bool {
    matches!(
        layer_id,
        PcbLayerId::BCu
            | PcbLayerId::BAdhes
            | PcbLayerId::BPaste
            | PcbLayerId::BSilkS
            | PcbLayerId::BMask
            | PcbLayerId::BCrtYd
            | PcbLayerId::BFab
    )
}

/// Port of `IsCopperLayerLowerThan`: return true if copper `layer_a` is placed lower than
/// `layer_b`, false otherwise.
#[inline]
pub fn is_copper_layer_lower_than(layer_a: PcbLayerId, layer_b: PcbLayerId) -> bool {
    if layer_a == layer_b {
        return false;
    }

    if layer_a == PcbLayerId::BCu {
        return true;
    }

    if layer_b == PcbLayerId::BCu {
        return false;
    }

    layer_a > layer_b
}

/// Port of `FlipLayer`.
///
/// Returns the layer number after flipping an item.  Some (not all) layers: external copper,
/// and paired layers (Mask, Paste, solder ...) are swapped between front and back sides.
/// Internal layers are flipped only if the copper layers count is known.
///
/// `copper_layers_count` is the number of copper layers; if 0 (in fact if < 4, the C++ default
/// argument is 0) internal layers will be not flipped because the layer count is not known.
pub fn flip_layer(layer_id: PcbLayerId, copper_layers_count: i32) -> PcbLayerId {
    match layer_id {
        PcbLayerId::BCu => PcbLayerId::FCu,
        PcbLayerId::FCu => PcbLayerId::BCu,

        PcbLayerId::BSilkS => PcbLayerId::FSilkS,
        PcbLayerId::FSilkS => PcbLayerId::BSilkS,

        PcbLayerId::BAdhes => PcbLayerId::FAdhes,
        PcbLayerId::FAdhes => PcbLayerId::BAdhes,

        PcbLayerId::BMask => PcbLayerId::FMask,
        PcbLayerId::FMask => PcbLayerId::BMask,

        PcbLayerId::BPaste => PcbLayerId::FPaste,
        PcbLayerId::FPaste => PcbLayerId::BPaste,

        PcbLayerId::BCrtYd => PcbLayerId::FCrtYd,
        PcbLayerId::FCrtYd => PcbLayerId::BCrtYd,

        PcbLayerId::BFab => PcbLayerId::FFab,
        PcbLayerId::FFab => PcbLayerId::BFab,

        _ => {
            // change internal layer if aCopperLayersCount is >= 4
            if is_copper_layer(layer_id) && copper_layers_count >= 4 {
                let inner_index = (layer_id.0 - PcbLayerId::In1Cu.0) / 2;
                let mut flipped_index = copper_layers_count - 3 - inner_index;

                if flipped_index < 0 {
                    flipped_index = 0;
                }

                let max_index = copper_layers_count - 3;

                if flipped_index > max_index {
                    flipped_index = max_index;
                }

                // Wrapping only matters for absurd layer counts (C++ would overflow `int`).
                return PcbLayerId(
                    PcbLayerId::In1Cu
                        .0
                        .wrapping_add(flipped_index.wrapping_mul(2)),
                );
            }

            // No change for the other layers
            layer_id
        }
    }
}

/// Port of `GetNetnameLayer`: return a netname layer corresponding to the given layer.
#[inline]
pub fn get_netname_layer(layer: impl Into<i32>) -> i32 {
    let layer: i32 = layer.into();

    if is_copper_layer(layer) || is_via_pad_layer(layer) {
        return netnames_layer_index(layer);
    }

    // Fallback
    PcbLayerId::CmtsUser.0
}

/// Port of `IsNetnameLayer`: test whether a layer is a netname layer.
#[inline]
pub fn is_netname_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= netnames_layer_index(PcbLayerId::FCu) && layer < NETNAMES_LAYER_ID_END
}

/// Port of `IsZoneFillLayer`.
#[inline]
pub fn is_zone_fill_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_ZONE_START && layer <= LAYER_ZONE_END
}

/// Port of `IsPadCopperLayer`.
#[inline]
pub fn is_pad_copper_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_PAD_COPPER_START && layer <= LAYER_PAD_COPPER_END
}

/// Port of `IsViaCopperLayer`.
#[inline]
pub fn is_via_copper_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_VIA_COPPER_START && layer <= LAYER_VIA_COPPER_END
}

/// Port of `IsClearanceLayer`.
#[inline]
pub fn is_clearance_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_CLEARANCE_START && layer <= LAYER_CLEARANCE_END
}

/// Port of `IsDrillSymbolLayer`.
#[inline]
pub fn is_drill_symbol_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_DRILL_SYMBOL_START && layer < LAYER_DRILL_SYMBOL_END
}

/// Port of `IsPointsLayer`.
#[inline]
pub fn is_points_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();
    layer >= LAYER_POINT_START && layer <= LAYER_POINT_END
}

/// Port of `IsDCodeLayer`.
#[inline]
pub fn is_dcode_layer(layer: impl Into<i32>) -> bool {
    let layer: i32 = layer.into();

    layer >= (GERBVIEW_LAYER_ID_START + GERBER_DRAWLAYERS_COUNT)
        && layer < (GERBVIEW_LAYER_ID_START + (2 * GERBER_DRAWLAYERS_COUNT))
}

/// Port of `CopperLayerToOrdinal`: converts KiCad copper layer enum to an ordinal between the
/// front and back layers.
///
/// Returns 0 for a non-copper layer (`wxCHECK( IsCopperLayer( aLayer ), 0 )`).
#[inline]
pub fn copper_layer_to_ordinal(layer: PcbLayerId) -> usize {
    if !is_copper_layer(layer) {
        return 0;
    }

    match layer {
        PcbLayerId::FCu => 0,
        PcbLayerId::BCu => (MAX_CU_LAYERS - 1) as usize,
        _ => ((layer.0 - PcbLayerId::BCu.0) / 2) as usize,
    }
}

/// Port of `BoardLayerFromLegacyId`: retrieve a layer ID from an integer converted from a
/// legacy (pre-V9) enum value.
///
/// Before V9 the copper layers were numbered `F_Cu` = 0, `In1_Cu` = 1 .. `In30_Cu` = 30,
/// `B_Cu` = 31, followed by the technical and user layers 32 .. 59.
pub fn board_layer_from_legacy_id(legacy_id: i32) -> PcbLayerId {
    match legacy_id {
        0 => PcbLayerId::FCu,
        31 => PcbLayerId::BCu,

        _ => {
            if legacy_id < 0 {
                // magic_enum::enum_cast<PCB_LAYER_ID>( aLegacyId ).value_or( UNDEFINED_LAYER ):
                // the only negative enumerators are UNDEFINED_LAYER and UNSELECTED_LAYER.
                return if legacy_id == PcbLayerId::UnselectedLayer.0 {
                    PcbLayerId::UnselectedLayer
                } else {
                    PcbLayerId::UndefinedLayer
                };
            }

            if legacy_id < 31 {
                return PcbLayerId(PcbLayerId::In1Cu.0 + (legacy_id - 1) * 2);
            }

            match legacy_id {
                32 => PcbLayerId::BAdhes,
                33 => PcbLayerId::FAdhes,
                34 => PcbLayerId::BPaste,
                35 => PcbLayerId::FPaste,
                36 => PcbLayerId::BSilkS,
                37 => PcbLayerId::FSilkS,
                38 => PcbLayerId::BMask,
                39 => PcbLayerId::FMask,
                40 => PcbLayerId::DwgsUser,
                41 => PcbLayerId::CmtsUser,
                42 => PcbLayerId::Eco1User,
                43 => PcbLayerId::Eco2User,
                44 => PcbLayerId::EdgeCuts,
                45 => PcbLayerId::Margin,
                46 => PcbLayerId::BCrtYd,
                47 => PcbLayerId::FCrtYd,
                48 => PcbLayerId::BFab,
                49 => PcbLayerId::FFab,
                50 => PcbLayerId::User1,
                51 => PcbLayerId::User2,
                52 => PcbLayerId::User3,
                53 => PcbLayerId::User4,
                54 => PcbLayerId::User5,
                55 => PcbLayerId::User6,
                56 => PcbLayerId::User7,
                57 => PcbLayerId::User8,
                58 => PcbLayerId::User9,
                59 => PcbLayerId::Rescue,
                _ => PcbLayerId::UndefinedLayer,
            }
        }
    }
}

/// Port of `ToLAYER_ID` (defined in `common/lset.cpp`).
pub fn to_layer_id(layer: i32) -> PcbLayerId {
    // We use std::numeric_limits<int>::max() to represent B_Cu for the connectivity_rtree
    if layer == i32::MAX {
        return PcbLayerId::BCu;
    }

    debug_assert!(layer < GAL_LAYER_ID_END);
    PcbLayerId(layer)
}

// =================================================================================================
// GUI layer ids
//
// The enumerators of NETNAMES_LAYER_ID, GAL_LAYER_ID, SCH_LAYER_ID, GERBVIEW_LAYER_ID and
// LAYER_3D_ID, as plain `i32` constants with their C++ names.  Enumerators without an explicit
// value in C++ are written as "previous + 1".  The values are checked against a dump made from
// the C++ header in `tests/test_layer_ids.rs`.
//
// GAL layers are "virtual" layers, i.e. not tied into design data.  Some layers here are shared
// between applications.
//
// NOTE: Be very careful where you add new layers here.  Layers up to GAL_LAYER_ID_BITMASK_END
// must never be re-ordered and new layers must always be added after this value, because the
// layers before this value are mapped to bit locations in legacy board files.
//
// The values in this enum that are used to store visibility state are explicitly encoded with an
// offset from GAL_LAYER_ID_START, which is explicitly encoded itself.  The exact value of
// GAL_LAYER_ID_START is not that sensitive, but the offsets should never be changed or else any
// existing visibility settings will be disrupted.
// =================================================================================================

// ---- enum NETNAMES_LAYER_ID: dedicated layers for net names used in Pcbnew ----
pub const NETNAMES_LAYER_ID_START: i32 = PCB_LAYER_ID_COUNT;
/// Reserved space for board layer netnames.
pub const NETNAMES_LAYER_ID_RESERVED: i32 = NETNAMES_LAYER_ID_START + PCB_LAYER_ID_COUNT;
/// Additional netnames layers (not associated with a PCB layer).
pub const LAYER_PAD_FR_NETNAMES: i32 = NETNAMES_LAYER_ID_RESERVED + 1;
pub const LAYER_PAD_BK_NETNAMES: i32 = LAYER_PAD_FR_NETNAMES + 1;
pub const LAYER_PAD_NETNAMES: i32 = LAYER_PAD_BK_NETNAMES + 1;
pub const LAYER_VIA_NETNAMES: i32 = LAYER_PAD_NETNAMES + 1;
pub const NETNAMES_LAYER_ID_END: i32 = LAYER_VIA_NETNAMES + 1;

// ---- enum GAL_LAYER_ID ----
pub const GAL_LAYER_ID_START: i32 = NETNAMES_LAYER_ID_END;
/// Meta control for all vias opacity/visibility.
pub const LAYER_VIAS: i32 = GAL_LAYER_ID_START + 0;
/// Draw micro vias.
pub const LAYER_VIA_MICROVIA: i32 = GAL_LAYER_ID_START + 1;
/// Draw blind vias.
pub const LAYER_VIA_BLIND: i32 = GAL_LAYER_ID_START + 2;
/// Draw buried vias.
pub const LAYER_VIA_BURIED: i32 = GAL_LAYER_ID_START + 3;
/// Draw usual through hole vias.
pub const LAYER_VIA_THROUGH: i32 = GAL_LAYER_ID_START + 4;
/// Handle color for not plated holes (holes, not pads).
pub const LAYER_NON_PLATEDHOLES: i32 = GAL_LAYER_ID_START + 5;
pub const LAYER_FP_TEXT: i32 = GAL_LAYER_ID_START + 6;
//  LAYER_MOD_TEXT_BK deprecated                  +  6,
//  DEPRECATED, UNUSED SINCE 9.0. text marked as invisible.
//  LAYER_HIDDEN_TEXT        = GAL_LAYER_ID_START +  7,
/// Anchor of items having an anchor point (texts, footprints).
pub const LAYER_ANCHOR: i32 = GAL_LAYER_ID_START + 8;
//  LAYER_PADS_SMD_FR        = GAL_LAYER_ID_START +  9, // Deprecated since 9.0
//  LAYER_PADS_SMD_BK        = GAL_LAYER_ID_START + 10, // Deprecated since 9.0
pub const LAYER_RATSNEST: i32 = GAL_LAYER_ID_START + 11;
pub const LAYER_GRID: i32 = GAL_LAYER_ID_START + 12;
pub const LAYER_GRID_AXES: i32 = GAL_LAYER_ID_START + 13;
//  LAYER_NO_CONNECTS deprecated                  + 14, // show a marker on pads with no nets
/// Show footprints on front.
pub const LAYER_FOOTPRINTS_FR: i32 = GAL_LAYER_ID_START + 15;
/// Show footprints on back.
pub const LAYER_FOOTPRINTS_BK: i32 = GAL_LAYER_ID_START + 16;
/// Show footprints values (when texts are visible).
pub const LAYER_FP_VALUES: i32 = GAL_LAYER_ID_START + 17;
/// Show footprints references (when texts are visible).
pub const LAYER_FP_REFERENCES: i32 = GAL_LAYER_ID_START + 18;
pub const LAYER_TRACKS: i32 = GAL_LAYER_ID_START + 19;
//  LAYER_PADS_TH            = GAL_LAYER_ID_START + 20, ///< Deprecated since 9.0
/// to draw pad holes (plated)
pub const LAYER_PAD_PLATEDHOLES: i32 = GAL_LAYER_ID_START + 21;
/// Draw via holes (pad holes do not use this layer).
pub const LAYER_VIA_HOLES: i32 = GAL_LAYER_ID_START + 22;
/// Layer for DRC markers with #RPT_SEVERITY_ERROR.
pub const LAYER_DRC_ERROR: i32 = GAL_LAYER_ID_START + 23;
/// Sheet frame and title block.
pub const LAYER_DRAWINGSHEET: i32 = GAL_LAYER_ID_START + 24;
/// General purpose overlay.
pub const LAYER_GP_OVERLAY: i32 = GAL_LAYER_ID_START + 25;
/// Selected items overlay.
pub const LAYER_SELECT_OVERLAY: i32 = GAL_LAYER_ID_START + 26;
/// PCB background color.
pub const LAYER_PCB_BACKGROUND: i32 = GAL_LAYER_ID_START + 27;
/// PCB cursor.
pub const LAYER_CURSOR: i32 = GAL_LAYER_ID_START + 28;
/// Auxiliary items (guides, rule, etc).
pub const LAYER_AUX_ITEMS: i32 = GAL_LAYER_ID_START + 29;
/// Draw images.
pub const LAYER_DRAW_BITMAPS: i32 = GAL_LAYER_ID_START + 30;
/// This is the end of the layers used for visibility bit masks in legacy board files
pub const GAL_LAYER_ID_BITMASK_END: i32 = GAL_LAYER_ID_START + 31;
// Layers in this section have visibility controls but were not present in legacy board files.
/// Meta control for all pads opacity/visibility (color ignored).
pub const LAYER_PADS: i32 = GAL_LAYER_ID_START + 32;
/// Control for copper zone opacity/visibility (color ignored).
pub const LAYER_ZONES: i32 = GAL_LAYER_ID_START + 33;
pub const LAYER_PAD_HOLEWALLS: i32 = GAL_LAYER_ID_START + 34;
pub const LAYER_VIA_HOLEWALLS: i32 = GAL_LAYER_ID_START + 35;
/// Layer for DRC markers with #RPT_SEVERITY_WARNING.
pub const LAYER_DRC_WARNING: i32 = GAL_LAYER_ID_START + 36;
/// Layer for DRC markers which have been individually excluded.
pub const LAYER_DRC_EXCLUSION: i32 = GAL_LAYER_ID_START + 37;
/// Shadows for DRC markers.
pub const LAYER_MARKER_SHADOWS: i32 = GAL_LAYER_ID_START + 38;
/// Shadow layer for locked items.
pub const LAYER_LOCKED_ITEM_SHADOW: i32 = GAL_LAYER_ID_START + 39;
/// Shadow layer for items flagged conflicting.
pub const LAYER_CONFLICTS_SHADOW: i32 = GAL_LAYER_ID_START + 40;
/// Copper graphic shape opacity/visibility (color ignored).
pub const LAYER_FILLED_SHAPES: i32 = GAL_LAYER_ID_START + 41;
//  LAYER_DRC_SHAPES         = GAL_LAYER_ID_START + 42,  ///< Deprecated since 11.0
//  LAYER_DRC_SHAPE2         = GAL_LAYER_ID_START + 43,  ///< Deprecated since 10.0
/// PCB board outline
pub const LAYER_BOARD_OUTLINE_AREA: i32 = GAL_LAYER_ID_START + 44;
/// PCB reference/manual snap points visibility
pub const LAYER_POINTS: i32 = GAL_LAYER_ID_START + 45;
/// Shadow layer for items bound to a constraint
pub const LAYER_CONSTRAINT_SHADOW: i32 = GAL_LAYER_ID_START + 46;
/// Routing/placement subgrids (PCB_GRID_ITEM) visibility and color
pub const LAYER_SUBGRIDS: i32 = GAL_LAYER_ID_START + 47;
/// Outline of via stitching generators
pub const LAYER_VIA_STITCHING: i32 = GAL_LAYER_ID_START + 48;
// Add layers below this point that do not have visibility controls, so don't need explicit
// enum values
/// Sheet Editor previewing first page.
pub const LAYER_DRAWINGSHEET_PAGE1: i32 = LAYER_VIA_STITCHING + 1;
/// Sheet Editor previewing pages after first page.
#[allow(non_upper_case_globals)]
pub const LAYER_DRAWINGSHEET_PAGEn: i32 = LAYER_DRAWINGSHEET_PAGE1 + 1;
/// Color for drawing the page extents (visibility stored in
/// PCBNEW_SETTINGS::m_ShowPageLimits)
pub const LAYER_PAGE_LIMITS: i32 = LAYER_DRAWINGSHEET_PAGEn + 1;
/// Color for highlighted DRC markers.
pub const LAYER_DRC_HIGHLIGHTED: i32 = LAYER_PAGE_LIMITS + 1;
// Constraint tints themed and drawn by overlay with no visibility control of their own
/// Under-constrained (free DOF remain).
pub const LAYER_CONSTRAINT_UNDER: i32 = LAYER_DRC_HIGHLIGHTED + 1;
/// Fully constrained.
pub const LAYER_CONSTRAINT_WELL: i32 = LAYER_CONSTRAINT_UNDER + 1;
/// Over-constrained / conflicting.
pub const LAYER_CONSTRAINT_OVER: i32 = LAYER_CONSTRAINT_WELL + 1;
/// Router preview clearance shadow, drawn behind board copper
pub const LAYER_ROUTER_PREVIEW_SHADOW: i32 = LAYER_CONSTRAINT_OVER + 1;
/// Router preview collision marker, drawn above all board items
pub const LAYER_ROUTER_PREVIEW_COLLISION: i32 = LAYER_ROUTER_PREVIEW_SHADOW + 1;
/// Opacity slider for items outside the entered group
pub const LAYER_OUTSIDE_GROUP_OPACITY: i32 = LAYER_ROUTER_PREVIEW_COLLISION + 1;
/// Virtual layers for stacking zones and tracks on a given copper layer.
pub const LAYER_ZONE_START: i32 = LAYER_OUTSIDE_GROUP_OPACITY + 1;
pub const LAYER_ZONE_END: i32 = LAYER_ZONE_START + PCB_LAYER_ID_COUNT;
/// Virtual layers for pad copper on a given copper layer.
pub const LAYER_PAD_COPPER_START: i32 = LAYER_ZONE_END + 1;
pub const LAYER_PAD_COPPER_END: i32 = LAYER_PAD_COPPER_START + PCB_LAYER_ID_COUNT;
/// Virtual layers for via copper on a given copper layer.
pub const LAYER_VIA_COPPER_START: i32 = LAYER_PAD_COPPER_END + 1;
pub const LAYER_VIA_COPPER_END: i32 = LAYER_VIA_COPPER_START + PCB_LAYER_ID_COUNT;
/// Virtual layers for pad/via/track clearance outlines for a given copper layer.
pub const LAYER_CLEARANCE_START: i32 = LAYER_VIA_COPPER_END + 1;
pub const LAYER_CLEARANCE_END: i32 = LAYER_CLEARANCE_START + PCB_LAYER_ID_COUNT;
/// Virtual layers for background images per board layer.
pub const LAYER_BITMAP_START: i32 = LAYER_CLEARANCE_END + 1;
pub const LAYER_BITMAP_END: i32 = LAYER_BITMAP_START + PCB_LAYER_ID_COUNT;
/// Virtual layers for points per board layer.
pub const LAYER_POINT_START: i32 = LAYER_BITMAP_END + 1;
pub const LAYER_POINT_END: i32 = LAYER_POINT_START + PCB_LAYER_ID_COUNT;
// Layers for drawing on-canvas UI
pub const LAYER_UI_START: i32 = LAYER_POINT_END + 1;
pub const LAYER_UI_END: i32 = LAYER_UI_START + GAL_UI_LAYER_COUNT;
/// Drill symbols, one channel per board layer. Appended at the end because the offsets
/// from GAL_LAYER_ID_START are stored in visibility settings.
pub const LAYER_DRILL_SYMBOL_START: i32 = LAYER_UI_END + 1;
pub const LAYER_DRILL_SYMBOL_END: i32 = LAYER_DRILL_SYMBOL_START + PCB_LAYER_ID_COUNT;
pub const GAL_LAYER_ID_END: i32 = LAYER_DRILL_SYMBOL_END + 1;

// ---- enum SCH_LAYER_ID: Eeschema drawing layers ----
pub const SCH_LAYER_ID_START: i32 = GAL_LAYER_ID_END;
pub const LAYER_WIRE: i32 = SCH_LAYER_ID_START;
pub const LAYER_BUS: i32 = LAYER_WIRE + 1;
pub const LAYER_JUNCTION: i32 = LAYER_BUS + 1;
pub const LAYER_LOCLABEL: i32 = LAYER_JUNCTION + 1;
pub const LAYER_GLOBLABEL: i32 = LAYER_LOCLABEL + 1;
pub const LAYER_HIERLABEL: i32 = LAYER_GLOBLABEL + 1;
pub const LAYER_PINNUM: i32 = LAYER_HIERLABEL + 1;
pub const LAYER_PINNAM: i32 = LAYER_PINNUM + 1;
pub const LAYER_REFERENCEPART: i32 = LAYER_PINNAM + 1;
pub const LAYER_VALUEPART: i32 = LAYER_REFERENCEPART + 1;
pub const LAYER_FIELDS: i32 = LAYER_VALUEPART + 1;
pub const LAYER_INTERSHEET_REFS: i32 = LAYER_FIELDS + 1;
pub const LAYER_NETCLASS_REFS: i32 = LAYER_INTERSHEET_REFS + 1;
pub const LAYER_RULE_AREAS: i32 = LAYER_NETCLASS_REFS + 1;
pub const LAYER_DEVICE: i32 = LAYER_RULE_AREAS + 1;
pub const LAYER_NOTES: i32 = LAYER_DEVICE + 1;
pub const LAYER_PRIVATE_NOTES: i32 = LAYER_NOTES + 1;
pub const LAYER_NOTES_BACKGROUND: i32 = LAYER_PRIVATE_NOTES + 1;
pub const LAYER_PIN: i32 = LAYER_NOTES_BACKGROUND + 1;
pub const LAYER_SHEET: i32 = LAYER_PIN + 1;
pub const LAYER_SHEETNAME: i32 = LAYER_SHEET + 1;
pub const LAYER_SHEETFILENAME: i32 = LAYER_SHEETNAME + 1;
pub const LAYER_SHEETFIELDS: i32 = LAYER_SHEETFILENAME + 1;
pub const LAYER_SHEETLABEL: i32 = LAYER_SHEETFIELDS + 1;
pub const LAYER_NOCONNECT: i32 = LAYER_SHEETLABEL + 1;
pub const LAYER_DANGLING: i32 = LAYER_NOCONNECT + 1;
pub const LAYER_DNP_MARKER: i32 = LAYER_DANGLING + 1;
pub const LAYER_ERC_WARN: i32 = LAYER_DNP_MARKER + 1;
pub const LAYER_ERC_ERR: i32 = LAYER_ERC_WARN + 1;
pub const LAYER_ERC_EXCLUSION: i32 = LAYER_ERC_ERR + 1;
pub const LAYER_EXCLUDED_FROM_SIM: i32 = LAYER_ERC_EXCLUSION + 1;
pub const LAYER_SHAPES_BACKGROUND: i32 = LAYER_EXCLUDED_FROM_SIM + 1;
pub const LAYER_DEVICE_BACKGROUND: i32 = LAYER_SHAPES_BACKGROUND + 1;
pub const LAYER_SHEET_BACKGROUND: i32 = LAYER_DEVICE_BACKGROUND + 1;
pub const LAYER_SCHEMATIC_GRID: i32 = LAYER_SHEET_BACKGROUND + 1;
pub const LAYER_SCHEMATIC_GRID_AXES: i32 = LAYER_SCHEMATIC_GRID + 1;
pub const LAYER_SCHEMATIC_BACKGROUND: i32 = LAYER_SCHEMATIC_GRID_AXES + 1;
pub const LAYER_SCHEMATIC_CURSOR: i32 = LAYER_SCHEMATIC_BACKGROUND + 1;
pub const LAYER_HOVERED: i32 = LAYER_SCHEMATIC_CURSOR + 1;
pub const LAYER_BRIGHTENED: i32 = LAYER_HOVERED + 1;
pub const LAYER_HIDDEN: i32 = LAYER_BRIGHTENED + 1;
pub const LAYER_NET_COLOR_HIGHLIGHT: i32 = LAYER_HIDDEN + 1;
pub const LAYER_DRAG_NET_COLLISION: i32 = LAYER_NET_COLOR_HIGHLIGHT + 1;
pub const LAYER_SELECTION_SHADOWS: i32 = LAYER_DRAG_NET_COLLISION + 1;
pub const LAYER_SCHEMATIC_DRAWINGSHEET: i32 = LAYER_SELECTION_SHADOWS + 1;
pub const LAYER_SCHEMATIC_PAGE_LIMITS: i32 = LAYER_SCHEMATIC_DRAWINGSHEET + 1;
pub const LAYER_BUS_JUNCTION: i32 = LAYER_SCHEMATIC_PAGE_LIMITS + 1;
pub const LAYER_SCHEMATIC_AUX_ITEMS: i32 = LAYER_BUS_JUNCTION + 1;
pub const LAYER_SCHEMATIC_ANCHOR: i32 = LAYER_SCHEMATIC_AUX_ITEMS + 1;
pub const LAYER_OP_VOLTAGES: i32 = LAYER_SCHEMATIC_ANCHOR + 1;
pub const LAYER_OP_CURRENTS: i32 = LAYER_OP_VOLTAGES + 1;
pub const LAYER_GROUP: i32 = LAYER_OP_CURRENTS + 1;
pub const SCH_LAYER_ID_END: i32 = LAYER_GROUP + 1;

// ---- enum GERBVIEW_LAYER_ID: Gerbview draw layers ----
pub const GERBVIEW_LAYER_ID_START: i32 = SCH_LAYER_ID_END;
/// Gerbview draw layers and d-code layers
pub const GERBVIEW_LAYER_ID_RESERVED: i32 = GERBVIEW_LAYER_ID_START + (2 * GERBER_DRAWLAYERS_COUNT);
pub const LAYER_DCODES: i32 = GERBVIEW_LAYER_ID_RESERVED + 1;
pub const LAYER_NEGATIVE_OBJECTS: i32 = LAYER_DCODES + 1;
pub const LAYER_GERBVIEW_GRID: i32 = LAYER_NEGATIVE_OBJECTS + 1;
pub const LAYER_GERBVIEW_AXES: i32 = LAYER_GERBVIEW_GRID + 1;
pub const LAYER_GERBVIEW_BACKGROUND: i32 = LAYER_GERBVIEW_AXES + 1;
pub const LAYER_GERBVIEW_DRAWINGSHEET: i32 = LAYER_GERBVIEW_BACKGROUND + 1;
pub const LAYER_GERBVIEW_PAGE_LIMITS: i32 = LAYER_GERBVIEW_DRAWINGSHEET + 1;
pub const GERBVIEW_LAYER_ID_END: i32 = LAYER_GERBVIEW_PAGE_LIMITS + 1;

// ---- enum LAYER_3D_ID: 3D Viewer virtual layers for color settings ----
pub const LAYER_3D_START: i32 = GERBVIEW_LAYER_ID_END;
pub const LAYER_3D_BACKGROUND_BOTTOM: i32 = LAYER_3D_START + 1;
pub const LAYER_3D_BACKGROUND_TOP: i32 = LAYER_3D_BACKGROUND_BOTTOM + 1;
pub const LAYER_3D_BOARD: i32 = LAYER_3D_BACKGROUND_TOP + 1;
pub const LAYER_3D_COPPER_TOP: i32 = LAYER_3D_BOARD + 1;
pub const LAYER_3D_COPPER_BOTTOM: i32 = LAYER_3D_COPPER_TOP + 1;
pub const LAYER_3D_SILKSCREEN_BOTTOM: i32 = LAYER_3D_COPPER_BOTTOM + 1;
pub const LAYER_3D_SILKSCREEN_TOP: i32 = LAYER_3D_SILKSCREEN_BOTTOM + 1;
pub const LAYER_3D_SOLDERMASK_BOTTOM: i32 = LAYER_3D_SILKSCREEN_TOP + 1;
pub const LAYER_3D_SOLDERMASK_TOP: i32 = LAYER_3D_SOLDERMASK_BOTTOM + 1;
pub const LAYER_3D_SOLDERPASTE: i32 = LAYER_3D_SOLDERMASK_TOP + 1;
pub const LAYER_3D_ADHESIVE: i32 = LAYER_3D_SOLDERPASTE + 1;
pub const LAYER_3D_USER_COMMENTS: i32 = LAYER_3D_ADHESIVE + 1;
pub const LAYER_3D_USER_DRAWINGS: i32 = LAYER_3D_USER_COMMENTS + 1;
pub const LAYER_3D_USER_ECO1: i32 = LAYER_3D_USER_DRAWINGS + 1;
pub const LAYER_3D_USER_ECO2: i32 = LAYER_3D_USER_ECO1 + 1;
pub const LAYER_3D_F_FAB: i32 = LAYER_3D_USER_ECO2 + 1;
pub const LAYER_3D_B_FAB: i32 = LAYER_3D_F_FAB + 1;
pub const LAYER_3D_F_COURTYARD: i32 = LAYER_3D_B_FAB + 1;
pub const LAYER_3D_B_COURTYARD: i32 = LAYER_3D_F_COURTYARD + 1;
pub const LAYER_3D_USER_1: i32 = LAYER_3D_B_COURTYARD + 1;
pub const LAYER_3D_USER_2: i32 = LAYER_3D_USER_1 + 1;
pub const LAYER_3D_USER_3: i32 = LAYER_3D_USER_2 + 1;
pub const LAYER_3D_USER_4: i32 = LAYER_3D_USER_3 + 1;
pub const LAYER_3D_USER_5: i32 = LAYER_3D_USER_4 + 1;
pub const LAYER_3D_USER_6: i32 = LAYER_3D_USER_5 + 1;
pub const LAYER_3D_USER_7: i32 = LAYER_3D_USER_6 + 1;
pub const LAYER_3D_USER_8: i32 = LAYER_3D_USER_7 + 1;
pub const LAYER_3D_USER_9: i32 = LAYER_3D_USER_8 + 1;
pub const LAYER_3D_USER_10: i32 = LAYER_3D_USER_9 + 1;
pub const LAYER_3D_USER_11: i32 = LAYER_3D_USER_10 + 1;
pub const LAYER_3D_USER_12: i32 = LAYER_3D_USER_11 + 1;
pub const LAYER_3D_USER_13: i32 = LAYER_3D_USER_12 + 1;
pub const LAYER_3D_USER_14: i32 = LAYER_3D_USER_13 + 1;
pub const LAYER_3D_USER_15: i32 = LAYER_3D_USER_14 + 1;
pub const LAYER_3D_USER_16: i32 = LAYER_3D_USER_15 + 1;
pub const LAYER_3D_USER_17: i32 = LAYER_3D_USER_16 + 1;
pub const LAYER_3D_USER_18: i32 = LAYER_3D_USER_17 + 1;
pub const LAYER_3D_USER_19: i32 = LAYER_3D_USER_18 + 1;
pub const LAYER_3D_USER_20: i32 = LAYER_3D_USER_19 + 1;
pub const LAYER_3D_USER_21: i32 = LAYER_3D_USER_20 + 1;
pub const LAYER_3D_USER_22: i32 = LAYER_3D_USER_21 + 1;
pub const LAYER_3D_USER_23: i32 = LAYER_3D_USER_22 + 1;
pub const LAYER_3D_USER_24: i32 = LAYER_3D_USER_23 + 1;
pub const LAYER_3D_USER_25: i32 = LAYER_3D_USER_24 + 1;
pub const LAYER_3D_USER_26: i32 = LAYER_3D_USER_25 + 1;
pub const LAYER_3D_USER_27: i32 = LAYER_3D_USER_26 + 1;
pub const LAYER_3D_USER_28: i32 = LAYER_3D_USER_27 + 1;
pub const LAYER_3D_USER_29: i32 = LAYER_3D_USER_28 + 1;
pub const LAYER_3D_USER_30: i32 = LAYER_3D_USER_29 + 1;
pub const LAYER_3D_USER_31: i32 = LAYER_3D_USER_30 + 1;
pub const LAYER_3D_USER_32: i32 = LAYER_3D_USER_31 + 1;
pub const LAYER_3D_USER_33: i32 = LAYER_3D_USER_32 + 1;
pub const LAYER_3D_USER_34: i32 = LAYER_3D_USER_33 + 1;
pub const LAYER_3D_USER_35: i32 = LAYER_3D_USER_34 + 1;
pub const LAYER_3D_USER_36: i32 = LAYER_3D_USER_35 + 1;
pub const LAYER_3D_USER_37: i32 = LAYER_3D_USER_36 + 1;
pub const LAYER_3D_USER_38: i32 = LAYER_3D_USER_37 + 1;
pub const LAYER_3D_USER_39: i32 = LAYER_3D_USER_38 + 1;
pub const LAYER_3D_USER_40: i32 = LAYER_3D_USER_39 + 1;
pub const LAYER_3D_USER_41: i32 = LAYER_3D_USER_40 + 1;
pub const LAYER_3D_USER_42: i32 = LAYER_3D_USER_41 + 1;
pub const LAYER_3D_USER_43: i32 = LAYER_3D_USER_42 + 1;
pub const LAYER_3D_USER_44: i32 = LAYER_3D_USER_43 + 1;
pub const LAYER_3D_USER_45: i32 = LAYER_3D_USER_44 + 1;
pub const LAYER_3D_TH_MODELS: i32 = LAYER_3D_USER_45 + 1;
pub const LAYER_3D_SMD_MODELS: i32 = LAYER_3D_TH_MODELS + 1;
pub const LAYER_3D_VIRTUAL_MODELS: i32 = LAYER_3D_SMD_MODELS + 1;
pub const LAYER_3D_MODELS_NOT_IN_POS: i32 = LAYER_3D_VIRTUAL_MODELS + 1;
pub const LAYER_3D_MODELS_MARKED_DNP: i32 = LAYER_3D_MODELS_NOT_IN_POS + 1;
pub const LAYER_3D_NAVIGATOR: i32 = LAYER_3D_MODELS_MARKED_DNP + 1;
pub const LAYER_3D_BOUNDING_BOXES: i32 = LAYER_3D_NAVIGATOR + 1;
pub const LAYER_3D_OFF_BOARD_SILK: i32 = LAYER_3D_BOUNDING_BOXES + 1;
pub const LAYER_3D_PLATED_BARRELS: i32 = LAYER_3D_OFF_BOARD_SILK + 1;
// Inner copper layers are appended here to keep existing enum values stable.
pub const LAYER_3D_COPPER_INNER_1: i32 = LAYER_3D_PLATED_BARRELS + 1;
pub const LAYER_3D_COPPER_INNER_2: i32 = LAYER_3D_COPPER_INNER_1 + 1;
pub const LAYER_3D_COPPER_INNER_3: i32 = LAYER_3D_COPPER_INNER_2 + 1;
pub const LAYER_3D_COPPER_INNER_4: i32 = LAYER_3D_COPPER_INNER_3 + 1;
pub const LAYER_3D_COPPER_INNER_5: i32 = LAYER_3D_COPPER_INNER_4 + 1;
pub const LAYER_3D_COPPER_INNER_6: i32 = LAYER_3D_COPPER_INNER_5 + 1;
pub const LAYER_3D_COPPER_INNER_7: i32 = LAYER_3D_COPPER_INNER_6 + 1;
pub const LAYER_3D_COPPER_INNER_8: i32 = LAYER_3D_COPPER_INNER_7 + 1;
pub const LAYER_3D_COPPER_INNER_9: i32 = LAYER_3D_COPPER_INNER_8 + 1;
pub const LAYER_3D_COPPER_INNER_10: i32 = LAYER_3D_COPPER_INNER_9 + 1;
pub const LAYER_3D_COPPER_INNER_11: i32 = LAYER_3D_COPPER_INNER_10 + 1;
pub const LAYER_3D_COPPER_INNER_12: i32 = LAYER_3D_COPPER_INNER_11 + 1;
pub const LAYER_3D_COPPER_INNER_13: i32 = LAYER_3D_COPPER_INNER_12 + 1;
pub const LAYER_3D_COPPER_INNER_14: i32 = LAYER_3D_COPPER_INNER_13 + 1;
pub const LAYER_3D_COPPER_INNER_15: i32 = LAYER_3D_COPPER_INNER_14 + 1;
pub const LAYER_3D_COPPER_INNER_16: i32 = LAYER_3D_COPPER_INNER_15 + 1;
pub const LAYER_3D_COPPER_INNER_17: i32 = LAYER_3D_COPPER_INNER_16 + 1;
pub const LAYER_3D_COPPER_INNER_18: i32 = LAYER_3D_COPPER_INNER_17 + 1;
pub const LAYER_3D_COPPER_INNER_19: i32 = LAYER_3D_COPPER_INNER_18 + 1;
pub const LAYER_3D_COPPER_INNER_20: i32 = LAYER_3D_COPPER_INNER_19 + 1;
pub const LAYER_3D_COPPER_INNER_21: i32 = LAYER_3D_COPPER_INNER_20 + 1;
pub const LAYER_3D_COPPER_INNER_22: i32 = LAYER_3D_COPPER_INNER_21 + 1;
pub const LAYER_3D_COPPER_INNER_23: i32 = LAYER_3D_COPPER_INNER_22 + 1;
pub const LAYER_3D_COPPER_INNER_24: i32 = LAYER_3D_COPPER_INNER_23 + 1;
pub const LAYER_3D_COPPER_INNER_25: i32 = LAYER_3D_COPPER_INNER_24 + 1;
pub const LAYER_3D_COPPER_INNER_26: i32 = LAYER_3D_COPPER_INNER_25 + 1;
pub const LAYER_3D_COPPER_INNER_27: i32 = LAYER_3D_COPPER_INNER_26 + 1;
pub const LAYER_3D_COPPER_INNER_28: i32 = LAYER_3D_COPPER_INNER_27 + 1;
pub const LAYER_3D_COPPER_INNER_29: i32 = LAYER_3D_COPPER_INNER_28 + 1;
pub const LAYER_3D_COPPER_INNER_30: i32 = LAYER_3D_COPPER_INNER_29 + 1;
pub const LAYER_3D_END: i32 = LAYER_3D_COPPER_INNER_30 + 1;
