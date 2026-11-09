// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2014 CERN
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
// @author Tomasz Wlostowski <tomasz.wlostowski@cern.ch>
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

//! Port of `include/core/typeinfo.h`: the `KICAD_T` enum and its classification helpers.
//!
//! The C++ RTTI helpers of that header (`IsA`, `dyn_cast`) have no Rust counterpart; item
//! hierarchies are enums or traits in the port.

/// Port of `enum KICAD_T`: the set of class identification values stored in
/// `EDA_ITEM::m_structType`.
///
/// The numeric values and their order are significant: they are used for sorting and type
/// scans, and they are exposed via IPC (new values must be appended at the end of the enum,
/// not inserted into existing blocks).  Use `as i32` to get the C++ value and
/// [`KicadT::from_i32`] for the reverse conversion.
///
/// The C++ enumerator `PCB_FOOTPRINT_T` is `KicadT::PcbFootprintT` and so on.  The free
/// functions of the header are methods: `BaseType( t )` is [`KicadT::base_type`],
/// `IsPcbnewType( t )` is [`KicadT::is_pcbnew_type`], etc.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(i32)]
pub enum KicadT {
    /// the 3d code uses this value
    NotUsed = -1,

    TypeNotInit = 0,
    PcbT,
    /// not really an item, used to identify a screen
    ScreenT,

    // Items in pcb
    /// class FOOTPRINT, a footprint
    PcbFootprintT,
    /// class PAD, a pad in a footprint
    PcbPadT,
    /// class PCB_SHAPE, a segment not on copper layers
    PcbShapeT,
    /// class PCB_REFERENCE_IMAGE, bitmap on a layer
    PcbReferenceImageT,
    /// class PCB_FIELD, text associated with a footprint property
    PcbFieldT,
    /// class PCB_GENERATOR, generator on a layer
    PcbGeneratorT,
    /// class PCB_TEXT, text on a layer
    PcbTextT,
    /// class PCB_TEXTBOX, wrapped text on a layer
    PcbTextboxT,
    /// class PCB_TABLE, table of PCB_TABLECELLs
    PcbTableT,
    /// class PCB_TABLECELL, PCB_TEXTBOX for use in tables
    PcbTablecellT,
    /// class PCB_TRACK, a track segment (segment on a copper layer)
    PcbTraceT,
    /// class PCB_VIA, a via (like a track segment on a copper layer)
    PcbViaT,
    /// class PCB_ARC, an arc track segment on a copper layer
    PcbArcT,
    /// class PCB_MARKER, a marker used to show something
    PcbMarkerT,
    /// class PCB_DIMENSION_BASE: abstract dimension meta-type
    PcbDimensionT,
    /// class PCB_BARCODE, a barcode (graphic item)
    PcbBarcodeT,
    /// class PCB_DIM_ALIGNED, a linear dimension (graphic item)
    PcbDimAlignedT,
    /// class PCB_DIM_LEADER, a leader dimension (graphic item)
    PcbDimLeaderT,
    /// class PCB_DIM_CENTER, a center point marking (graphic item)
    PcbDimCenterT,
    /// class PCB_DIM_RADIAL, a radius or diameter dimension
    PcbDimRadialT,
    /// class PCB_DIM_ORTHOGONAL, a linear dimension constrained to x/y
    PcbDimOrthogonalT,
    /// class PCB_TARGET, a target (graphic item)
    PcbTargetT,
    /// class ZONE, a copper pour area
    PcbZoneT,
    /// class BOARD_ITEM_LIST, a list of board items
    PcbItemListT,
    /// class NETINFO_ITEM, a description of a net
    PcbNetinfoT,
    /// class PCB_GROUP, a set of BOARD_ITEMs
    PcbGroupT,
    /// class PCB_BOARD_OUTLINE_T, a pcb board outline item
    PcbBoardOutlineT,
    /// class PCB_POINT, a 0-dimensional point
    PcbPointT,

    // Be prudent with these types:
    // they should be used only to locate a specific field type among PCB_FIELD_Ts
    // N.B. If you add a type here, be sure to add it below to the BaseType()
    PcbFieldLocateReferenceT,
    PcbFieldLocateValueT,
    PcbFieldLocateFootprintT,
    PcbFieldLocateDatasheetT,

    // Be prudent with these types:
    // they should be used only to locate specific item sub-types
    // N.B. If you add a type here, be sure to add it below to the BaseType()
    PcbLocateStdviaT,
    PcbLocateUviaT,
    PcbLocateBlindviaT,
    PcbLocateBuriedviaT,
    PcbLocateTextT,
    PcbLocateHoleT,
    PcbLocatePthT,
    PcbLocateNpthT,
    PcbLocateBoardEdgeT,

    // Same for locating shapes types from PCB_SHAPE_T items
    PcbShapeLocateSegmentT,
    PcbShapeLocateRectT,
    PcbShapeLocateCircleT,
    PcbShapeLocateArcT,
    PcbShapeLocatePolyT,
    PcbShapeLocateBezierT,

    // Draw items in library symbol.
    //
    // The order of these items effects the sort order for items inside the
    // "DRAW/ENDDRAW" section of the symbol definition in a library file.
    // If you add a new draw item, type, please make sure you add it so the
    // sort order is logical.
    LibSymbolT,
    SchShapeT,
    SchFieldT,
    SchTextT,
    SchTextboxT,
    SchPinT,

    // Schematic draw Items.  The order of these items effects the sort order.
    // It is currently ordered to mimic the old Eeschema locate behavior where
    // the smallest item is the selected item.
    SchMarkerT,
    SchJunctionT,
    SchNoConnectT,
    SchBusWireEntryT,
    SchBusBusEntryT,
    SchLineT,
    SchBitmapT,
    SchTableT,
    SchTablecellT,
    SchLabelT,
    SchGlobalLabelT,
    SchHierLabelT,
    SchRuleAreaT,
    SchDirectiveLabelT,
    SchSymbolT,
    SchGroupT,
    SchSheetPinT,
    SchSheetT,

    // Be prudent with these types:
    // they should be used only to locate a specific field type among SCH_FIELD_Ts
    // N.B. If you add a type here, be sure to add it below to the BaseType()
    SchFieldLocateReferenceT,
    SchFieldLocateValueT,
    SchFieldLocateFootprintT,
    SchFieldLocateDatasheetT,

    // Same for picking wires, buses and graphics from SCH_ITEM_T items
    SchItemLocateWireT,
    SchItemLocateBusT,
    SchItemLocateGraphicLineT,

    // Same for picking labels, or labels attached to wires and/or buses
    SchLabelLocateAnyT,
    SchLabelLocateWireT,
    SchLabelLocateBusT,

    // Same for picking symbols which are power symbols
    SchSymbolLocatePowerT,

    // matches any type
    SchLocateAnyT,

    // General
    SchScreenT,

    SchematicT,

    // For GerbView: item types:
    GerberLayoutT,
    GerberDrawItemT,
    GerberImageT,

    // For Pl_Editor: item types:
    WsgLineT,
    WsgRectT,
    WsgPolyT,
    WsgTextT,
    WsgBitmapT,
    WsgPageT,

    // serialized layout used in undo/redo commands
    /// serialized layout used in undo/redo commands
    WsProxyUndoItemT,
    /// serialized layout plus page and title block settings
    WsProxyUndoItemPlusT,

    // FOR PROJECT::_ELEMs
    SymbolLibTableT,
    FpLibTableT,
    DesignBlockLibTableT,
    SymbolLibsT,
    SearchStackT,
    S3dCacheT,

    // New types appended here to preserve historical KICAD_T ordinals
    // (IPC clients with stale protobuf-generated headers depend on stable values).
    PcbShapeLocateEllipseT,
    PcbShapeLocateEllipseArcT,
    /// a geometric constraint between board items
    PcbConstraintT,
    /// a subgrid placed on a board
    PcbGridItemT,
    /// class PCB_DRILL_CHART, a live drill chart derived from PCB_TABLE
    PcbDrillChartT,
    /// class PCB_DRILL_MAP, drill symbols drawn at the holes
    PcbDrillMapT,
    /// class PCB_3D_BODY, a 3D body (extruded or model) on a footprint
    Pcb3dBodyT,
    /// class PCB_STACKUP_TABLE, a live stackup table derived from PCB_TABLE
    PcbStackupTableT,

    // End value
    MaxStructTypeId,
}

// KICAD_T ordinals are exposed via IPC; new values must be appended at the end of the enum,
// not inserted into existing blocks.
const _: () = assert!(KicadT::S3dCacheT as i32 == 104);

impl KicadT {
    /// The numeric value of the type (the implicit `KICAD_T` to `int` conversion).
    #[inline]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }

    /// Convert a numeric `KICAD_T` value back into the enum; `None` when the value is not one
    /// of the enumerators.
    pub const fn from_i32(value: i32) -> Option<KicadT> {
        Some(match value {
            -1 => KicadT::NotUsed,
            0 => KicadT::TypeNotInit,
            1 => KicadT::PcbT,
            2 => KicadT::ScreenT,
            3 => KicadT::PcbFootprintT,
            4 => KicadT::PcbPadT,
            5 => KicadT::PcbShapeT,
            6 => KicadT::PcbReferenceImageT,
            7 => KicadT::PcbFieldT,
            8 => KicadT::PcbGeneratorT,
            9 => KicadT::PcbTextT,
            10 => KicadT::PcbTextboxT,
            11 => KicadT::PcbTableT,
            12 => KicadT::PcbTablecellT,
            13 => KicadT::PcbTraceT,
            14 => KicadT::PcbViaT,
            15 => KicadT::PcbArcT,
            16 => KicadT::PcbMarkerT,
            17 => KicadT::PcbDimensionT,
            18 => KicadT::PcbBarcodeT,
            19 => KicadT::PcbDimAlignedT,
            20 => KicadT::PcbDimLeaderT,
            21 => KicadT::PcbDimCenterT,
            22 => KicadT::PcbDimRadialT,
            23 => KicadT::PcbDimOrthogonalT,
            24 => KicadT::PcbTargetT,
            25 => KicadT::PcbZoneT,
            26 => KicadT::PcbItemListT,
            27 => KicadT::PcbNetinfoT,
            28 => KicadT::PcbGroupT,
            29 => KicadT::PcbBoardOutlineT,
            30 => KicadT::PcbPointT,
            31 => KicadT::PcbFieldLocateReferenceT,
            32 => KicadT::PcbFieldLocateValueT,
            33 => KicadT::PcbFieldLocateFootprintT,
            34 => KicadT::PcbFieldLocateDatasheetT,
            35 => KicadT::PcbLocateStdviaT,
            36 => KicadT::PcbLocateUviaT,
            37 => KicadT::PcbLocateBlindviaT,
            38 => KicadT::PcbLocateBuriedviaT,
            39 => KicadT::PcbLocateTextT,
            40 => KicadT::PcbLocateHoleT,
            41 => KicadT::PcbLocatePthT,
            42 => KicadT::PcbLocateNpthT,
            43 => KicadT::PcbLocateBoardEdgeT,
            44 => KicadT::PcbShapeLocateSegmentT,
            45 => KicadT::PcbShapeLocateRectT,
            46 => KicadT::PcbShapeLocateCircleT,
            47 => KicadT::PcbShapeLocateArcT,
            48 => KicadT::PcbShapeLocatePolyT,
            49 => KicadT::PcbShapeLocateBezierT,
            50 => KicadT::LibSymbolT,
            51 => KicadT::SchShapeT,
            52 => KicadT::SchFieldT,
            53 => KicadT::SchTextT,
            54 => KicadT::SchTextboxT,
            55 => KicadT::SchPinT,
            56 => KicadT::SchMarkerT,
            57 => KicadT::SchJunctionT,
            58 => KicadT::SchNoConnectT,
            59 => KicadT::SchBusWireEntryT,
            60 => KicadT::SchBusBusEntryT,
            61 => KicadT::SchLineT,
            62 => KicadT::SchBitmapT,
            63 => KicadT::SchTableT,
            64 => KicadT::SchTablecellT,
            65 => KicadT::SchLabelT,
            66 => KicadT::SchGlobalLabelT,
            67 => KicadT::SchHierLabelT,
            68 => KicadT::SchRuleAreaT,
            69 => KicadT::SchDirectiveLabelT,
            70 => KicadT::SchSymbolT,
            71 => KicadT::SchGroupT,
            72 => KicadT::SchSheetPinT,
            73 => KicadT::SchSheetT,
            74 => KicadT::SchFieldLocateReferenceT,
            75 => KicadT::SchFieldLocateValueT,
            76 => KicadT::SchFieldLocateFootprintT,
            77 => KicadT::SchFieldLocateDatasheetT,
            78 => KicadT::SchItemLocateWireT,
            79 => KicadT::SchItemLocateBusT,
            80 => KicadT::SchItemLocateGraphicLineT,
            81 => KicadT::SchLabelLocateAnyT,
            82 => KicadT::SchLabelLocateWireT,
            83 => KicadT::SchLabelLocateBusT,
            84 => KicadT::SchSymbolLocatePowerT,
            85 => KicadT::SchLocateAnyT,
            86 => KicadT::SchScreenT,
            87 => KicadT::SchematicT,
            88 => KicadT::GerberLayoutT,
            89 => KicadT::GerberDrawItemT,
            90 => KicadT::GerberImageT,
            91 => KicadT::WsgLineT,
            92 => KicadT::WsgRectT,
            93 => KicadT::WsgPolyT,
            94 => KicadT::WsgTextT,
            95 => KicadT::WsgBitmapT,
            96 => KicadT::WsgPageT,
            97 => KicadT::WsProxyUndoItemT,
            98 => KicadT::WsProxyUndoItemPlusT,
            99 => KicadT::SymbolLibTableT,
            100 => KicadT::FpLibTableT,
            101 => KicadT::DesignBlockLibTableT,
            102 => KicadT::SymbolLibsT,
            103 => KicadT::SearchStackT,
            104 => KicadT::S3dCacheT,
            105 => KicadT::PcbShapeLocateEllipseT,
            106 => KicadT::PcbShapeLocateEllipseArcT,
            107 => KicadT::PcbConstraintT,
            108 => KicadT::PcbGridItemT,
            109 => KicadT::PcbDrillChartT,
            110 => KicadT::PcbDrillMapT,
            111 => KicadT::Pcb3dBodyT,
            112 => KicadT::PcbStackupTableT,
            113 => KicadT::MaxStructTypeId,
            _ => return None,
        })
    }

    /// Port of `BaseType`: return the underlying type of the given type.
    ///
    /// This is useful for finding the element type given one of the "non-type" types such as
    /// `SCH_ITEM_LOCATE_WIRE_T`.
    pub const fn base_type(self) -> KicadT {
        match self {
            KicadT::SchFieldLocateReferenceT
            | KicadT::SchFieldLocateValueT
            | KicadT::SchFieldLocateFootprintT
            | KicadT::SchFieldLocateDatasheetT => KicadT::SchFieldT,

            KicadT::SchItemLocateWireT
            | KicadT::SchItemLocateBusT
            | KicadT::SchItemLocateGraphicLineT => KicadT::SchLineT,

            KicadT::SchLabelLocateAnyT
            | KicadT::SchLabelLocateWireT
            | KicadT::SchLabelLocateBusT => KicadT::SchLabelT,

            KicadT::SchSymbolLocatePowerT => KicadT::SchSymbolT,

            KicadT::PcbFieldLocateReferenceT
            | KicadT::PcbFieldLocateValueT
            | KicadT::PcbFieldLocateFootprintT
            | KicadT::PcbFieldLocateDatasheetT => KicadT::PcbFieldT,

            KicadT::PcbLocateHoleT | KicadT::PcbLocatePthT | KicadT::PcbLocateNpthT => {
                KicadT::PcbLocateHoleT
            }

            KicadT::PcbShapeLocateSegmentT
            | KicadT::PcbShapeLocateRectT
            | KicadT::PcbShapeLocateCircleT
            | KicadT::PcbShapeLocateArcT
            | KicadT::PcbShapeLocatePolyT
            | KicadT::PcbShapeLocateBezierT
            | KicadT::PcbShapeLocateEllipseT
            | KicadT::PcbShapeLocateEllipseArcT => KicadT::PcbShapeT,

            KicadT::PcbDimAlignedT
            | KicadT::PcbDimCenterT
            | KicadT::PcbDimRadialT
            | KicadT::PcbDimOrthogonalT
            | KicadT::PcbDimLeaderT => KicadT::PcbDimensionT,

            KicadT::PcbDrillChartT | KicadT::PcbStackupTableT => KicadT::PcbTableT,

            _ => self,
        }
    }

    /// Port of `IsNullType`.
    pub const fn is_null_type(self) -> bool {
        (self as i32) <= 0
    }

    /// Port of `IsGeneratedTableType`.
    ///
    /// A PCB_TABLE subclass whose cells are generated from the board rather than typed by the
    /// user.
    pub const fn is_generated_table_type(self) -> bool {
        matches!(self, KicadT::PcbDrillChartT | KicadT::PcbStackupTableT)
    }

    /// Port of `IsInstantiableType`.
    ///
    /// Not instantiable are the null types, the "locate" pseudo types, `PCB_DIMENSION_T`
    /// (base class is not instantiable), `SCH_TABLECELL_T` / `PCB_TABLECELL_T` (must be
    /// instantiated as part of a SCH_TABLE or PCB_TABLE), `SCH_SCREEN_T` and
    /// `PCB_ITEM_LIST_T`.
    pub const fn is_instantiable_type(self) -> bool {
        if self.is_null_type() {
            return false;
        }

        !matches!(
            self,
            KicadT::SchLocateAnyT
                | KicadT::SchFieldLocateReferenceT
                | KicadT::SchFieldLocateValueT
                | KicadT::SchFieldLocateFootprintT
                | KicadT::SchFieldLocateDatasheetT
                | KicadT::SchItemLocateWireT
                | KicadT::SchItemLocateBusT
                | KicadT::SchItemLocateGraphicLineT
                | KicadT::SchLabelLocateAnyT
                | KicadT::SchLabelLocateWireT
                | KicadT::SchLabelLocateBusT
                | KicadT::SchSymbolLocatePowerT
                | KicadT::PcbFieldLocateReferenceT
                | KicadT::PcbFieldLocateValueT
                | KicadT::PcbFieldLocateFootprintT
                | KicadT::PcbFieldLocateDatasheetT
                | KicadT::PcbLocateStdviaT
                | KicadT::PcbLocateUviaT
                | KicadT::PcbLocateBlindviaT
                | KicadT::PcbLocateBuriedviaT
                | KicadT::PcbLocateTextT
                | KicadT::PcbLocateHoleT
                | KicadT::PcbLocatePthT
                | KicadT::PcbLocateNpthT
                | KicadT::PcbLocateBoardEdgeT
                | KicadT::PcbShapeLocateSegmentT
                | KicadT::PcbShapeLocateRectT
                | KicadT::PcbShapeLocateCircleT
                | KicadT::PcbShapeLocateArcT
                | KicadT::PcbShapeLocatePolyT
                | KicadT::PcbShapeLocateBezierT
                | KicadT::PcbShapeLocateEllipseT
                | KicadT::PcbShapeLocateEllipseArcT
                | KicadT::PcbDimensionT
                | KicadT::SchTablecellT
                | KicadT::PcbTablecellT
                | KicadT::SchScreenT
                | KicadT::PcbItemListT
        )
    }

    /// Port of `IsEeschemaType`.
    pub const fn is_eeschema_type(self) -> bool {
        matches!(
            self,
            KicadT::SchMarkerT
                | KicadT::SchJunctionT
                | KicadT::SchNoConnectT
                | KicadT::SchBusWireEntryT
                | KicadT::SchBusBusEntryT
                | KicadT::SchLineT
                | KicadT::SchShapeT
                | KicadT::SchRuleAreaT
                | KicadT::SchBitmapT
                | KicadT::SchTextT
                | KicadT::SchTextboxT
                | KicadT::SchTableT
                | KicadT::SchTablecellT
                | KicadT::SchLabelT
                | KicadT::SchDirectiveLabelT
                | KicadT::SchGlobalLabelT
                | KicadT::SchHierLabelT
                | KicadT::SchFieldT
                | KicadT::SchSymbolT
                | KicadT::SchSheetPinT
                | KicadT::SchGroupT
                | KicadT::SchSheetT
                | KicadT::SchPinT
                | KicadT::SchFieldLocateReferenceT
                | KicadT::SchFieldLocateValueT
                | KicadT::SchFieldLocateFootprintT
                | KicadT::SchFieldLocateDatasheetT
                | KicadT::SchItemLocateWireT
                | KicadT::SchItemLocateBusT
                | KicadT::SchItemLocateGraphicLineT
                | KicadT::SchLabelLocateAnyT
                | KicadT::SchLabelLocateWireT
                | KicadT::SchLabelLocateBusT
                | KicadT::SchSymbolLocatePowerT
                | KicadT::SchLocateAnyT
                | KicadT::SchScreenT
                | KicadT::SchematicT
                | KicadT::LibSymbolT
        )
    }

    /// Port of `IsPcbnewType`.
    pub const fn is_pcbnew_type(self) -> bool {
        matches!(
            self,
            KicadT::PcbT
                | KicadT::PcbFootprintT
                | KicadT::PcbPadT
                | KicadT::PcbShapeT
                | KicadT::PcbReferenceImageT
                | KicadT::PcbFieldT
                | KicadT::PcbTextT
                | KicadT::PcbTextboxT
                | KicadT::PcbBarcodeT
                | KicadT::PcbTableT
                | KicadT::PcbTablecellT
                | KicadT::PcbTraceT
                | KicadT::PcbViaT
                | KicadT::PcbArcT
                | KicadT::PcbMarkerT
                | KicadT::PcbDimensionT
                | KicadT::PcbDimAlignedT
                | KicadT::PcbDimLeaderT
                | KicadT::PcbDimCenterT
                | KicadT::PcbDimRadialT
                | KicadT::PcbDimOrthogonalT
                | KicadT::PcbTargetT
                | KicadT::PcbDrillChartT
                | KicadT::PcbDrillMapT
                | KicadT::Pcb3dBodyT
                | KicadT::PcbStackupTableT
                | KicadT::PcbPointT
                | KicadT::PcbZoneT
                | KicadT::PcbItemListT
                | KicadT::PcbNetinfoT
                | KicadT::PcbGroupT
                | KicadT::PcbGeneratorT
                | KicadT::PcbGridItemT
                | KicadT::PcbFieldLocateReferenceT
                | KicadT::PcbFieldLocateValueT
                | KicadT::PcbFieldLocateFootprintT
                | KicadT::PcbFieldLocateDatasheetT
                | KicadT::PcbLocateStdviaT
                | KicadT::PcbLocateUviaT
                | KicadT::PcbLocateBlindviaT
                | KicadT::PcbLocateBuriedviaT
                | KicadT::PcbLocateTextT
                | KicadT::PcbLocateHoleT
                | KicadT::PcbLocatePthT
                | KicadT::PcbLocateNpthT
                | KicadT::PcbLocateBoardEdgeT
                | KicadT::PcbShapeLocateSegmentT
                | KicadT::PcbShapeLocateRectT
                | KicadT::PcbShapeLocateCircleT
                | KicadT::PcbShapeLocateArcT
                | KicadT::PcbShapeLocatePolyT
                | KicadT::PcbShapeLocateBezierT
                | KicadT::PcbShapeLocateEllipseT
                | KicadT::PcbShapeLocateEllipseArcT
                | KicadT::PcbBoardOutlineT
                | KicadT::PcbConstraintT
        )
    }

    /// Port of `IsSingleLayerType`.
    pub const fn is_single_layer_type(self) -> bool {
        matches!(
            self,
            KicadT::PcbShapeT
                | KicadT::PcbReferenceImageT
                | KicadT::PcbFieldT
                | KicadT::PcbTextT
                | KicadT::PcbTextboxT
                | KicadT::PcbBarcodeT
                | KicadT::PcbTableT
                | KicadT::PcbDrillChartT
                | KicadT::PcbDrillMapT
                | KicadT::PcbStackupTableT
                | KicadT::PcbTablecellT
                | KicadT::PcbTraceT
                | KicadT::PcbArcT
                | KicadT::PcbMarkerT
                | KicadT::PcbDimensionT
                | KicadT::PcbDimAlignedT
                | KicadT::PcbDimLeaderT
                | KicadT::PcbDimCenterT
                | KicadT::PcbDimRadialT
                | KicadT::PcbDimOrthogonalT
                | KicadT::PcbTargetT
                | KicadT::PcbPointT
                | KicadT::PcbGeneratorT
        )
    }

    /// Port of `IsGerbviewType`.
    pub const fn is_gerbview_type(self) -> bool {
        matches!(
            self,
            KicadT::GerberLayoutT | KicadT::GerberDrawItemT | KicadT::GerberImageT
        )
    }

    /// Port of `IsPageLayoutEditorType`.
    pub const fn is_page_layout_editor_type(self) -> bool {
        matches!(
            self,
            KicadT::WsgLineT
                | KicadT::WsgRectT
                | KicadT::WsgPolyT
                | KicadT::WsgTextT
                | KicadT::WsgBitmapT
                | KicadT::WsgPageT
                | KicadT::WsProxyUndoItemT
                | KicadT::WsProxyUndoItemPlusT
        )
    }

    /// Port of `IsMiscType`.
    pub const fn is_misc_type(self) -> bool {
        matches!(
            self,
            KicadT::ScreenT
                | KicadT::SymbolLibTableT
                | KicadT::FpLibTableT
                | KicadT::DesignBlockLibTableT
                | KicadT::SymbolLibsT
                | KicadT::SearchStackT
                | KicadT::S3dCacheT
        )
    }

    /// Port of `IsTypeCorrect`.
    pub const fn is_type_correct(self) -> bool {
        self.is_null_type()
            || self.is_eeschema_type()
            || self.is_pcbnew_type()
            || self.is_gerbview_type()
            || self.is_page_layout_editor_type()
            || self.is_misc_type()
    }
}

impl From<KicadT> for i32 {
    fn from(value: KicadT) -> i32 {
        value as i32
    }
}

impl TryFrom<i32> for KicadT {
    type Error = i32;

    /// See [`KicadT::from_i32`]; the error carries the rejected value.
    fn try_from(value: i32) -> Result<KicadT, i32> {
        KicadT::from_i32(value).ok_or(value)
    }
}
