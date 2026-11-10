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

//! Port of `ENUM_MAP` (`include/properties/property.h`) and of every `ENUM_MAP<T>` the board
//! item properties use, as registered by the `*_DESC` constructors (`common/eda_item.cpp`,
//! `common/eda_shape.cpp`, `common/eda_text.cpp`, `pcbnew/board_item.cpp`, `pcbnew/pad.cpp`,
//! `pcbnew/pcb_track.cpp`, `pcbnew/zone.cpp`, `pcbnew/footprint.cpp`, `pcbnew/pcb_dimension.cpp`,
//! `pcbnew/pcb_barcode.cpp`, `pcbnew/pcb_generated_table.cpp`, `pcbnew/pcb_grid_item.cpp`,
//! `pcbnew/generators/*.cpp`). Values are the C++ enumerator values (`static_cast<int>`); the
//! labels are the untranslated (`_HKI`) source strings. Maps whose C++ registration is guarded
//! by `Choices().GetCount() == 0` are registered identically everywhere, so the static
//! initialisation order across translation units does not matter. A map without `Undefined()`
//! has the zero-initialised `m_undefined` of a static object: 0.

use std::sync::OnceLock;

use kicad_core::Lset;

/// The C++ enum type of an `ENUM_MAP<T>`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum EnumId {
    /// `KICAD_T`.
    KicadT,
    /// `PCB_LAYER_ID`.
    PcbLayerId,
    /// `SHAPE_T`.
    ShapeT,
    /// `LINE_STYLE`.
    LineStyle,
    /// `UI_FILL_MODE`.
    UiFillMode,
    /// `LINE_ENDING_STYLE`.
    LineEndingStyle,
    /// `GR_TEXT_H_ALIGN_T`.
    GrTextHAlignT,
    /// `GR_TEXT_V_ALIGN_T`.
    GrTextVAlignT,
    /// `PAD_ATTRIB`.
    PadAttrib,
    /// `PAD_SHAPE`.
    PadShape,
    /// `PAD_PROP`.
    PadProp,
    /// `PAD_DRILL_SHAPE`.
    PadDrillShape,
    /// `PAD_SIM_ELECTRICAL_TYPE`.
    PadSimElectricalType,
    /// `PAD_DRILL_POST_MACHINING_MODE`.
    PadDrillPostMachiningMode,
    /// `BACKDRILL_MODE`.
    BackdrillMode,
    /// `ZONE_CONNECTION`.
    ZoneConnection,
    /// `UNCONNECTED_LAYER_MODE`.
    UnconnectedLayerMode,
    /// `VIATYPE`.
    Viatype,
    /// `TENTING_MODE`.
    TentingMode,
    /// `COVERING_MODE`.
    CoveringMode,
    /// `PLUGGING_MODE`.
    PluggingMode,
    /// `CAPPING_MODE`.
    CappingMode,
    /// `FILLING_MODE`.
    FillingMode,
    /// `ZONE_FILL_MODE`.
    ZoneFillMode,
    /// `THIEVING_PATTERN`.
    ThievingPattern,
    /// `ISLAND_REMOVAL_MODE`.
    IslandRemovalMode,
    /// `PLACEMENT_SOURCE_T`.
    PlacementSourceT,
    /// `FOOTPRINT_TYPE`.
    FootprintType,
    /// `DIM_PRECISION`.
    DimPrecision,
    /// `DIM_UNITS_FORMAT`.
    DimUnitsFormat,
    /// `DIM_UNITS_MODE`.
    DimUnitsMode,
    /// `DIM_ARROW_DIRECTION`.
    DimArrowDirection,
    /// `DIM_VALUE_MODE`.
    DimValueMode,
    /// `DIM_TEXT_POSITION`.
    DimTextPosition,
    /// `DIM_TEXT_BORDER`.
    DimTextBorder,
    /// `BARCODE_T`.
    BarcodeT,
    /// `BARCODE_ECC_T`.
    BarcodeEccT,
    /// `GENERATED_TABLE_UNITS`.
    GeneratedTableUnits,
    /// `PCB_GRID_TYPE`.
    PcbGridType,
    /// `LENGTH_TUNING_MODE`.
    LengthTuningMode,
    /// `PNS::MEANDER_SIDE`.
    MeanderSide,
    /// `VIA_STACK_STYLE`.
    ViaStackStyle,
    /// `PCB_VIA_STITCH_LAYOUT`.
    PcbViaStitchLayout,
    /// `PCB_VIA_STITCH_MODE`.
    PcbViaStitchMode,
}

impl EnumId {
    /// Every map, in the order the oracle table lists them.
    pub const ALL: &'static [EnumId] = &[EnumId::KicadT, EnumId::PcbLayerId, EnumId::ShapeT, EnumId::LineStyle, EnumId::UiFillMode, EnumId::LineEndingStyle, EnumId::GrTextHAlignT, EnumId::GrTextVAlignT, EnumId::PadAttrib, EnumId::PadShape, EnumId::PadProp, EnumId::PadDrillShape, EnumId::PadSimElectricalType, EnumId::PadDrillPostMachiningMode, EnumId::BackdrillMode, EnumId::ZoneConnection, EnumId::UnconnectedLayerMode, EnumId::Viatype, EnumId::TentingMode, EnumId::CoveringMode, EnumId::PluggingMode, EnumId::CappingMode, EnumId::FillingMode, EnumId::ZoneFillMode, EnumId::ThievingPattern, EnumId::IslandRemovalMode, EnumId::PlacementSourceT, EnumId::FootprintType, EnumId::DimPrecision, EnumId::DimUnitsFormat, EnumId::DimUnitsMode, EnumId::DimArrowDirection, EnumId::DimValueMode, EnumId::DimTextPosition, EnumId::DimTextBorder, EnumId::BarcodeT, EnumId::BarcodeEccT, EnumId::GeneratedTableUnits, EnumId::PcbGridType, EnumId::LengthTuningMode, EnumId::MeanderSide, EnumId::ViaStackStyle, EnumId::PcbViaStitchLayout, EnumId::PcbViaStitchMode];

    /// The C++ type name.
    pub fn cxx_name(self) -> &'static str {
        match self {
            EnumId::KicadT => "KICAD_T",
            EnumId::PcbLayerId => "PCB_LAYER_ID",
            EnumId::ShapeT => "SHAPE_T",
            EnumId::LineStyle => "LINE_STYLE",
            EnumId::UiFillMode => "UI_FILL_MODE",
            EnumId::LineEndingStyle => "LINE_ENDING_STYLE",
            EnumId::GrTextHAlignT => "GR_TEXT_H_ALIGN_T",
            EnumId::GrTextVAlignT => "GR_TEXT_V_ALIGN_T",
            EnumId::PadAttrib => "PAD_ATTRIB",
            EnumId::PadShape => "PAD_SHAPE",
            EnumId::PadProp => "PAD_PROP",
            EnumId::PadDrillShape => "PAD_DRILL_SHAPE",
            EnumId::PadSimElectricalType => "PAD_SIM_ELECTRICAL_TYPE",
            EnumId::PadDrillPostMachiningMode => "PAD_DRILL_POST_MACHINING_MODE",
            EnumId::BackdrillMode => "BACKDRILL_MODE",
            EnumId::ZoneConnection => "ZONE_CONNECTION",
            EnumId::UnconnectedLayerMode => "UNCONNECTED_LAYER_MODE",
            EnumId::Viatype => "VIATYPE",
            EnumId::TentingMode => "TENTING_MODE",
            EnumId::CoveringMode => "COVERING_MODE",
            EnumId::PluggingMode => "PLUGGING_MODE",
            EnumId::CappingMode => "CAPPING_MODE",
            EnumId::FillingMode => "FILLING_MODE",
            EnumId::ZoneFillMode => "ZONE_FILL_MODE",
            EnumId::ThievingPattern => "THIEVING_PATTERN",
            EnumId::IslandRemovalMode => "ISLAND_REMOVAL_MODE",
            EnumId::PlacementSourceT => "PLACEMENT_SOURCE_T",
            EnumId::FootprintType => "FOOTPRINT_TYPE",
            EnumId::DimPrecision => "DIM_PRECISION",
            EnumId::DimUnitsFormat => "DIM_UNITS_FORMAT",
            EnumId::DimUnitsMode => "DIM_UNITS_MODE",
            EnumId::DimArrowDirection => "DIM_ARROW_DIRECTION",
            EnumId::DimValueMode => "DIM_VALUE_MODE",
            EnumId::DimTextPosition => "DIM_TEXT_POSITION",
            EnumId::DimTextBorder => "DIM_TEXT_BORDER",
            EnumId::BarcodeT => "BARCODE_T",
            EnumId::BarcodeEccT => "BARCODE_ECC_T",
            EnumId::GeneratedTableUnits => "GENERATED_TABLE_UNITS",
            EnumId::PcbGridType => "PCB_GRID_TYPE",
            EnumId::LengthTuningMode => "LENGTH_TUNING_MODE",
            EnumId::MeanderSide => "PNS::MEANDER_SIDE",
            EnumId::ViaStackStyle => "VIA_STACK_STYLE",
            EnumId::PcbViaStitchLayout => "PCB_VIA_STITCH_LAYOUT",
            EnumId::PcbViaStitchMode => "PCB_VIA_STITCH_MODE",
        }
    }

    /// `ENUM_MAP<T>::Instance()`.
    pub fn map(self) -> &'static EnumMap {
        maps()
            .iter()
            .find(|m| m.id == self)
            .expect("every EnumId has a map")
    }
}

/// `ENUM_MAP<T>`: the choices (`wxPGChoices`: value, label in `Map` order) and the value
/// returned for an unknown string (`Undefined`).
#[derive(Clone, Debug)]
pub struct EnumMap {
    pub id: EnumId,
    /// m_undefined.
    pub undefined: i32,
    /// m_choices.
    pub choices: Vec<(i32, &'static str)>,
}

impl EnumMap {
    /// `wxPGChoices::Index( value )`: the first choice with that value.
    fn index(&self, value: i32) -> Option<usize> {
        self.choices.iter().position(|(v, _)| *v == value)
    }

    /// `ToString( value )`: the label, `"UNDEFINED"` for an unmapped value.
    pub fn to_string(&self, value: i32) -> &'static str {
        match self.index(value) {
            Some(i) => self.choices[i].1,
            None => "UNDEFINED",
        }
    }

    /// `IsValueDefined( value )`.
    pub fn is_value_defined(&self, value: i32) -> bool {
        self.index(value).is_some()
    }

    /// `ToEnum( value )`: the value of a label (the last `Map` of a label wins, as in the
    /// C++ `m_reverseMap`), else the undefined value.
    pub fn to_enum(&self, label: &str) -> i32 {
        self.choices
            .iter()
            .rev()
            .find(|(_, l)| *l == label)
            .map_or(self.undefined, |(v, _)| *v)
    }

    /// The `DECLARE_ENUM_TO_WXANY` conversion of a value to `wxString`: fails (`None`) for a
    /// value that is neither mapped nor the undefined value.
    pub fn any_to_string(&self, value: i32) -> Option<&'static str> {
        if !self.is_value_defined(value) && value != self.undefined {
            return None;
        }

        Some(self.to_string(value))
    }

    /// The choices as owned (value, label) pairs.
    pub fn choices_owned(&self) -> Vec<(i32, String)> {
        self.choices
            .iter()
            .map(|(v, l)| (*v, (*l).to_string()))
            .collect()
    }
}

/// All maps.
fn maps() -> &'static [EnumMap] {
    static MAPS: OnceLock<Vec<EnumMap>> = OnceLock::new();

    MAPS.get_or_init(|| {
        let mut v = vec![
        // KICAD_T
        EnumMap { id: EnumId::KicadT, undefined: 0, choices: vec![
            (-1, "<not used>"),
            (2, "Screen"),
            (87, "Schematic"),
            (3, "Footprint"),
            (4, "Pad"),
            (5, "Graphic"),
            (6, "Reference Image"),
            (8, "Generator"),
            (7, "Text"),
            (9, "Text"),
            (10, "Text Box"),
            (11, "Table"),
            (109, "Drill Chart"),
            (110, "Drill Map"),
            (112, "Stackup Table"),
            (12, "Table Cell"),
            (13, "Track"),
            (15, "Track"),
            (14, "Via"),
            (16, "Marker"),
            (19, "Dimension"),
            (23, "Dimension"),
            (21, "Dimension"),
            (22, "Dimension"),
            (20, "Leader"),
            (24, "Target"),
            (30, "Point"),
            (25, "Zone"),
            (26, "ItemList"),
            (27, "NetInfo"),
            (28, "Group"),
            (18, "Barcode"),
            (108, "Grid"),
            (111, "3D Body"),
            (56, "Marker"),
            (57, "Junction"),
            (58, "No-Connect Flag"),
            (59, "Wire Entry"),
            (60, "Bus Entry"),
            (61, "Line"),
            (62, "Bitmap"),
            (51, "Graphic"),
            (68, "Rule Area"),
            (53, "Text"),
            (54, "Text Box"),
            (63, "Table"),
            (64, "Table Cell"),
            (65, "Net Label"),
            (69, "Directive Label"),
            (66, "Global Label"),
            (67, "Hierarchical Label"),
            (52, "Field"),
            (70, "Symbol"),
            (55, "Pin"),
            (72, "Sheet Pin"),
            (73, "Sheet"),
            (71, "Group"),
            (86, "SCH Screen"),
            (50, "Symbol"),
            (88, "Gerber Layout"),
            (89, "Draw Item"),
            (90, "Image"),
        ] },
        // SHAPE_T
        EnumMap { id: EnumId::ShapeT, undefined: 0, choices: vec![
            (0, "Segment"),
            (1, "Rectangle"),
            (2, "Arc"),
            (3, "Circle"),
            (4, "Polygon"),
            (5, "Bezier"),
            (6, "Ellipse"),
            (7, "Elliptical Arc"),
        ] },
        // LINE_STYLE
        EnumMap { id: EnumId::LineStyle, undefined: 0, choices: vec![
            (0, "Solid"),
            (1, "Dashed"),
            (2, "Dotted"),
            (3, "Dash-Dot"),
            (4, "Dash-Dot-Dot"),
        ] },
        // UI_FILL_MODE
        EnumMap { id: EnumId::UiFillMode, undefined: 0, choices: vec![
            (0, "None"),
            (1, "Solid"),
            (2, "Hatch"),
            (3, "Reverse Hatch"),
            (4, "Cross-hatch"),
        ] },
        // LINE_ENDING_STYLE
        EnumMap { id: EnumId::LineEndingStyle, undefined: 0, choices: vec![
            (0, "None"),
            (1, "Arrow"),
            (2, "Circle"),
            (3, "Square"),
            (4, "Open Arrow"),
        ] },
        // GR_TEXT_H_ALIGN_T
        EnumMap { id: EnumId::GrTextHAlignT, undefined: 0, choices: vec![
            (-1, "Left"),
            (0, "Center"),
            (1, "Right"),
        ] },
        // GR_TEXT_V_ALIGN_T
        EnumMap { id: EnumId::GrTextVAlignT, undefined: 0, choices: vec![
            (-1, "Top"),
            (0, "Center"),
            (1, "Bottom"),
        ] },
        // PAD_ATTRIB
        EnumMap { id: EnumId::PadAttrib, undefined: 0, choices: vec![
            (0, "Through-hole"),
            (1, "SMD"),
            (2, "Edge connector"),
            (3, "NPTH, mechanical"),
            (4, "Non-component"),
        ] },
        // PAD_SHAPE
        EnumMap { id: EnumId::PadShape, undefined: 0, choices: vec![
            (0, "Circle"),
            (1, "Rectangle"),
            (2, "Oval"),
            (3, "Trapezoid"),
            (4, "Rounded rectangle"),
            (5, "Chamfered rectangle"),
            (6, "Custom"),
        ] },
        // PAD_PROP
        EnumMap { id: EnumId::PadProp, undefined: 0, choices: vec![
            (0, "None"),
            (1, "BGA pad"),
            (2, "Fiducial, global to board"),
            (3, "Fiducial, local to footprint"),
            (4, "Test point pad"),
            (5, "Heatsink pad"),
            (6, "Castellated pad"),
            (7, "Mechanical pad"),
            (8, "Press-fit pad"),
        ] },
        // PAD_DRILL_SHAPE
        EnumMap { id: EnumId::PadDrillShape, undefined: 0, choices: vec![
            (0, "Undefined"),
            (1, "Round"),
            (2, "Oblong"),
        ] },
        // PAD_SIM_ELECTRICAL_TYPE
        EnumMap { id: EnumId::PadSimElectricalType, undefined: 0, choices: vec![
            (0, "None"),
            (1, "Source"),
            (2, "Sink"),
        ] },
        // PAD_DRILL_POST_MACHINING_MODE
        EnumMap { id: EnumId::PadDrillPostMachiningMode, undefined: 0, choices: vec![
            (1, "Not post-machined"),
            (2, "Counterbore"),
            (3, "Countersink"),
        ] },
        // BACKDRILL_MODE
        EnumMap { id: EnumId::BackdrillMode, undefined: 0, choices: vec![
            (0, "No backdrill"),
            (1, "Backdrill bottom"),
            (2, "Backdrill top"),
            (3, "Backdrill both"),
        ] },
        // ZONE_CONNECTION
        EnumMap { id: EnumId::ZoneConnection, undefined: -1, choices: vec![
            (-1, "Inherited"),
            (0, "None"),
            (1, "Thermal reliefs"),
            (2, "Solid"),
            (3, "Thermal reliefs for PTH"),
        ] },
        // UNCONNECTED_LAYER_MODE
        EnumMap { id: EnumId::UnconnectedLayerMode, undefined: 0, choices: vec![
            (0, "All copper layers"),
            (2, "Connected layers only"),
            (3, "Front, back and connected layers"),
            (1, "Start and end layers only"),
        ] },
        // VIATYPE
        EnumMap { id: EnumId::Viatype, undefined: 0, choices: vec![
            (4, "Through"),
            (2, "Blind"),
            (3, "Buried"),
            (1, "Micro"),
        ] },
        // TENTING_MODE
        EnumMap { id: EnumId::TentingMode, undefined: 0, choices: vec![
            (0, "From board stackup"),
            (1, "Tented"),
            (2, "Not tented"),
        ] },
        // COVERING_MODE
        EnumMap { id: EnumId::CoveringMode, undefined: 0, choices: vec![
            (0, "From board stackup"),
            (1, "Covered"),
            (2, "Not covered"),
        ] },
        // PLUGGING_MODE
        EnumMap { id: EnumId::PluggingMode, undefined: 0, choices: vec![
            (0, "From board stackup"),
            (1, "Plugged"),
            (2, "Not plugged"),
        ] },
        // CAPPING_MODE
        EnumMap { id: EnumId::CappingMode, undefined: 0, choices: vec![
            (0, "From board stackup"),
            (1, "Capped"),
            (2, "Not capped"),
        ] },
        // FILLING_MODE
        EnumMap { id: EnumId::FillingMode, undefined: 0, choices: vec![
            (0, "From board stackup"),
            (1, "Filled"),
            (2, "Not filled"),
        ] },
        // ZONE_FILL_MODE
        EnumMap { id: EnumId::ZoneFillMode, undefined: 0, choices: vec![
            (0, "Solid fill"),
            (1, "Hatch pattern"),
            (2, "Copper thieving"),
        ] },
        // THIEVING_PATTERN
        EnumMap { id: EnumId::ThievingPattern, undefined: 0, choices: vec![
            (0, "Dots"),
            (1, "Squares"),
            (2, "Hatch"),
        ] },
        // ISLAND_REMOVAL_MODE
        EnumMap { id: EnumId::IslandRemovalMode, undefined: 0, choices: vec![
            (0, "Always"),
            (1, "Never"),
            (2, "Below area limit"),
        ] },
        // PLACEMENT_SOURCE_T
        EnumMap { id: EnumId::PlacementSourceT, undefined: 0, choices: vec![
            (0, "Sheet Name"),
            (1, "Component Class"),
            (2, "Group"),
            (3, "Design Block"),
        ] },
        // FOOTPRINT_TYPE
        EnumMap { id: EnumId::FootprintType, undefined: 0, choices: vec![
            (1, "Through hole"),
            (2, "SMD"),
            (0, "Unspecified"),
        ] },
        // DIM_PRECISION
        EnumMap { id: EnumId::DimPrecision, undefined: 0, choices: vec![
            (0, "0"),
            (1, "0.0"),
            (2, "0.00"),
            (3, "0.000"),
            (4, "0.0000"),
            (5, "0.00000"),
            (6, "0.00 in / 0 mils / 0.0 mm"),
            (7, "0.000 / 0 / 0.00"),
            (8, "0.0000 / 0.0 / 0.000"),
            (9, "0.00000 / 0.00 / 0.0000"),
        ] },
        // DIM_UNITS_FORMAT
        EnumMap { id: EnumId::DimUnitsFormat, undefined: 0, choices: vec![
            (0, "1234.0"),
            (1, "1234.0 mm"),
            (2, "1234.0 (mm)"),
        ] },
        // DIM_UNITS_MODE
        EnumMap { id: EnumId::DimUnitsMode, undefined: 0, choices: vec![
            (0, "Inches"),
            (1, "Mils"),
            (2, "Millimeters"),
            (3, "Automatic"),
        ] },
        // DIM_ARROW_DIRECTION
        EnumMap { id: EnumId::DimArrowDirection, undefined: 0, choices: vec![
            (0, "Inward"),
            (1, "Outward"),
        ] },
        // DIM_VALUE_MODE
        EnumMap { id: EnumId::DimValueMode, undefined: 0, choices: vec![
            (0, "Driven"),
            (1, "Driving"),
            (2, "Arbitrary"),
        ] },
        // DIM_TEXT_POSITION
        EnumMap { id: EnumId::DimTextPosition, undefined: 0, choices: vec![
            (0, "Outside"),
            (1, "Inline"),
            (2, "Manual"),
        ] },
        // DIM_TEXT_BORDER
        EnumMap { id: EnumId::DimTextBorder, undefined: 0, choices: vec![
            (0, "None"),
            (1, "Rectangle"),
            (2, "Circle"),
        ] },
        // BARCODE_T
        EnumMap { id: EnumId::BarcodeT, undefined: 3, choices: vec![
            (0, "CODE_39"),
            (1, "CODE_128"),
            (2, "DATA_MATRIX"),
            (3, "QR_CODE"),
            (4, "MICRO_QR_CODE"),
        ] },
        // BARCODE_ECC_T
        EnumMap { id: EnumId::BarcodeEccT, undefined: 1, choices: vec![
            (1, "L (Low)"),
            (2, "M (Medium)"),
            (3, "Q (Quartile)"),
            (4, "H (High)"),
        ] },
        // GENERATED_TABLE_UNITS
        EnumMap { id: EnumId::GeneratedTableUnits, undefined: 0, choices: vec![
            (0, "Millimeters"),
            (2, "Mils"),
            (1, "Inches"),
        ] },
        // PCB_GRID_TYPE
        EnumMap { id: EnumId::PcbGridType, undefined: 0, choices: vec![
            (0, "Cartesian"),
            (1, "Polar"),
        ] },
        // LENGTH_TUNING_MODE
        EnumMap { id: EnumId::LengthTuningMode, undefined: 0, choices: vec![
            (0, "Single track"),
            (1, "Differential pair"),
            (2, "Diff pair skew"),
        ] },
        // PNS::MEANDER_SIDE
        EnumMap { id: EnumId::MeanderSide, undefined: 0, choices: vec![
            (-1, "Left"),
            (1, "Right"),
            (0, "Default"),
        ] },
        // VIA_STACK_STYLE
        EnumMap { id: EnumId::ViaStackStyle, undefined: 0, choices: vec![
            (0, "Stacked"),
            (1, "Staggered"),
        ] },
        // PCB_VIA_STITCH_LAYOUT
        EnumMap { id: EnumId::PcbViaStitchLayout, undefined: 0, choices: vec![
            (0, "Plain grid"),
            (1, "Staggered grid"),
            (2, "Poisson disk"),
        ] },
        // PCB_VIA_STITCH_MODE
        EnumMap { id: EnumId::PcbViaStitchMode, undefined: 0, choices: vec![
            (0, "Stitch"),
            (1, "Guard"),
        ] },
        ];

        // BOARD_ITEM_DESC (and the others, guarded): layerEnum.Undefined( UNDEFINED_LAYER );
        // for( PCB_LAYER_ID layer : LSET::AllLayersMask() )
        //     layerEnum.Map( layer, LSET::Name( layer ) );
        let layers = Lset::all_layers_mask()
            .iter()
            .map(|l| (l as i32, &*Box::leak(Lset::name(l).into_boxed_str())))
            .collect();
        v.push(EnumMap {
            id: EnumId::PcbLayerId,
            undefined: -1,
            choices: layers,
        });

        v
    })
}
