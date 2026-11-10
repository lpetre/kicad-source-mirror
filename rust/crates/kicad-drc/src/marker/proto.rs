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

//! The IPC API messages a DRC exclusion is made of, without a protobuf dependency (work
//! package DRC-11): `kiapi.board.DrcMarker`, `kiapi.board.DrcExclusion`,
//! `kiapi.common.types.Vector2` and `kiapi.common.types.KIID`
//! (`api/proto/board/board_rules.proto`, `api/proto/common/types/base_types.proto`), the enum
//! conversions of `pcbnew/api/api_pcb_enums.cpp` / `common/api/api_enums.cpp` that
//! `PCB_MARKER::ToProto` / `FromProto` use, and the three encodings KiCad applies to these
//! messages:
//!
//! * the binary wire format (`SerializeAsString`), which is the identity and the sort key of a
//!   `DRC_EXCLUSION` (`DRC_EXCLUSION::GetSortKey`, `DRC_EXCLUSION_COMPARE`);
//! * ProtoJSON as printed by protobuf 3.21 `MessageToJsonString` with
//!   `preserve_proto_field_names` (`to_json( json&, const DRC_EXCLUSION& )`);
//! * ProtoJSON as parsed by protobuf 3.21 `JsonStringToMessage` with default options
//!   (`from_json( const json&, DRC_EXCLUSION& )`).
//!
//! The reference build links the Ubuntu 24.04 libprotobuf 3.21.12, whose JSON parser converts
//! to binary first and leaves the message unchanged when the input is rejected.

use kicad_core::json::{Map, Number, Value};
use kicad_core::{PcbLayerId, Severity};
use kicad_pcb::drc_api::*;
use kicad_pcb::pcb_marker::MarkerT;

// ---- messages ----------------------------------------------------------------------------------

/// `kiapi.common.types.Vector2`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProtoVector2 {
    /// `int64 x_nm = 1`.
    pub x_nm: i64,
    /// `int64 y_nm = 2`.
    pub y_nm: i64,
}

/// `kiapi.board.DrcMarker` (proto3: zero / empty values are "not set").
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrcMarkerMsg {
    /// `DrcErrorType error_type = 1` (enum value; open enum, any `int32`).
    pub error_type: i32,
    /// `kiapi.common.types.Vector2 position = 2` (`None` = not present).
    pub position: Option<ProtoVector2>,
    /// `repeated kiapi.common.types.KIID items = 3` (the `value` strings).
    pub items: Vec<String>,
    /// `kiapi.board.types.BoardLayer layer = 4`.
    pub layer: i32,
    /// `string violating_rule = 5` (report-only).
    pub violating_rule: String,
    /// `string description = 6` (report-only).
    pub description: String,
    /// `kiapi.common.types.RuleSeverity severity = 7` (report-only).
    pub severity: i32,
    /// `bool excluded = 8` (report-only).
    pub excluded: bool,
    /// `kiapi.common.types.MarkerType marker_type = 9`.
    pub marker_type: i32,
}

/// `kiapi.board.DrcExclusion`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DrcExclusionMsg {
    /// `DrcMarker marker = 1` (`None` = `!has_marker()`).
    pub marker: Option<DrcMarkerMsg>,
    /// `string comment = 2`.
    pub comment: String,
}

// ---- enum tables -------------------------------------------------------------------------------

/// `kiapi.board.DrcErrorType` values with their names and the `PCB_DRC_CODE` of
/// `FromProtoEnum<PCB_DRC_CODE, DrcErrorType>` (0 for `DRCET_UNKNOWN`).
const DRC_ERROR_TYPES: &[(i32, &str, i32)] = &[
    (0, "DRCET_UNKNOWN", 0),
    (1, "DRCET_UNCONNECTED_ITEMS", DRCE_UNCONNECTED_ITEMS),
    (2, "DRCET_SHORTING_ITEMS", DRCE_SHORTING_ITEMS),
    (3, "DRCET_ALLOWED_ITEMS", DRCE_ALLOWED_ITEMS),
    (4, "DRCET_TEXT_ON_EDGECUTS", DRCE_TEXT_ON_EDGECUTS),
    (5, "DRCET_CLEARANCE", DRCE_CLEARANCE),
    (6, "DRCET_CREEPAGE", DRCE_CREEPAGE),
    (7, "DRCET_TRACKS_CROSSING", DRCE_TRACKS_CROSSING),
    (8, "DRCET_EDGE_CLEARANCE", DRCE_EDGE_CLEARANCE),
    (9, "DRCET_ZONES_INTERSECT", DRCE_ZONES_INTERSECT),
    (10, "DRCET_ISOLATED_COPPER", DRCE_ISOLATED_COPPER),
    (11, "DRCET_STARVED_THERMAL", DRCE_STARVED_THERMAL),
    (12, "DRCET_DANGLING_VIA", DRCE_DANGLING_VIA),
    (13, "DRCET_DANGLING_TRACK", DRCE_DANGLING_TRACK),
    (
        14,
        "DRCET_DRILLED_HOLES_TOO_CLOSE",
        DRCE_DRILLED_HOLES_TOO_CLOSE,
    ),
    (
        15,
        "DRCET_DRILLED_HOLES_COLOCATED",
        DRCE_DRILLED_HOLES_COLOCATED,
    ),
    (16, "DRCET_HOLE_CLEARANCE", DRCE_HOLE_CLEARANCE),
    (17, "DRCET_CONNECTION_WIDTH", DRCE_CONNECTION_WIDTH),
    (18, "DRCET_TRACK_WIDTH", DRCE_TRACK_WIDTH),
    (19, "DRCET_TRACK_ANGLE", DRCE_TRACK_ANGLE),
    (20, "DRCET_TRACK_SEGMENT_LENGTH", DRCE_TRACK_SEGMENT_LENGTH),
    (21, "DRCET_ANNULAR_WIDTH", DRCE_ANNULAR_WIDTH),
    (22, "DRCET_DRILL_OUT_OF_RANGE", DRCE_DRILL_OUT_OF_RANGE),
    (23, "DRCET_VIA_DIAMETER", DRCE_VIA_DIAMETER),
    (24, "DRCET_PADSTACK", DRCE_PADSTACK),
    (25, "DRCET_PADSTACK_INVALID", DRCE_PADSTACK_INVALID),
    (
        26,
        "DRCET_MICROVIA_DRILL_OUT_OF_RANGE",
        DRCE_MICROVIA_DRILL_OUT_OF_RANGE,
    ),
    (
        27,
        "DRCET_OVERLAPPING_FOOTPRINTS",
        DRCE_OVERLAPPING_FOOTPRINTS,
    ),
    (28, "DRCET_MISSING_COURTYARD", DRCE_MISSING_COURTYARD),
    (29, "DRCET_MALFORMED_COURTYARD", DRCE_MALFORMED_COURTYARD),
    (30, "DRCET_PTH_IN_COURTYARD", DRCE_PTH_IN_COURTYARD),
    (31, "DRCET_NPTH_IN_COURTYARD", DRCE_NPTH_IN_COURTYARD),
    (32, "DRCET_DISABLED_LAYER_ITEM", DRCE_DISABLED_LAYER_ITEM),
    (33, "DRCET_INVALID_OUTLINE", DRCE_INVALID_OUTLINE),
    (34, "DRCET_MISSING_FOOTPRINT", DRCE_MISSING_FOOTPRINT),
    (35, "DRCET_DUPLICATE_FOOTPRINT", DRCE_DUPLICATE_FOOTPRINT),
    (36, "DRCET_NET_CONFLICT", DRCE_NET_CONFLICT),
    (37, "DRCET_EXTRA_FOOTPRINT", DRCE_EXTRA_FOOTPRINT),
    (38, "DRCET_SCHEMATIC_PARITY", DRCE_SCHEMATIC_PARITY),
    (
        39,
        "DRCET_SCHEMATIC_FIELDS_PARITY",
        DRCE_SCHEMATIC_FIELDS_PARITY,
    ),
    (40, "DRCET_FOOTPRINT_FILTERS", DRCE_FOOTPRINT_FILTERS),
    (41, "DRCET_LIB_FOOTPRINT_ISSUES", DRCE_LIB_FOOTPRINT_ISSUES),
    (
        42,
        "DRCET_LIB_FOOTPRINT_MISMATCH",
        DRCE_LIB_FOOTPRINT_MISMATCH,
    ),
    (43, "DRCET_UNRESOLVED_VARIABLE", DRCE_UNRESOLVED_VARIABLE),
    (44, "DRCET_ASSERTION_FAILURE", DRCE_ASSERTION_FAILURE),
    (45, "DRCET_GENERIC_WARNING", DRCE_GENERIC_WARNING),
    (46, "DRCET_GENERIC_ERROR", DRCE_GENERIC_ERROR),
    (47, "DRCET_COPPER_SLIVER", DRCE_COPPER_SLIVER),
    (48, "DRCET_SILK_CLEARANCE", DRCE_SILK_CLEARANCE),
    (49, "DRCET_SILK_MASK_CLEARANCE", DRCE_SILK_MASK_CLEARANCE),
    (50, "DRCET_SILK_EDGE_CLEARANCE", DRCE_SILK_EDGE_CLEARANCE),
    (51, "DRCET_SOLDERMASK_BRIDGE", DRCE_SOLDERMASK_BRIDGE),
    (52, "DRCET_TEXT_HEIGHT", DRCE_TEXT_HEIGHT),
    (53, "DRCET_TEXT_THICKNESS", DRCE_TEXT_THICKNESS),
    (54, "DRCET_LENGTH_OUT_OF_RANGE", DRCE_LENGTH_OUT_OF_RANGE),
    (55, "DRCET_SKEW_OUT_OF_RANGE", DRCE_SKEW_OUT_OF_RANGE),
    (
        56,
        "DRCET_VIA_COUNT_OUT_OF_RANGE",
        DRCE_VIA_COUNT_OUT_OF_RANGE,
    ),
    (
        57,
        "DRCET_DIFF_PAIR_GAP_OUT_OF_RANGE",
        DRCE_DP_GAP_OUT_OF_RANGE,
    ),
    (
        58,
        "DRCET_DIFF_PAIR_UNCOUPLED_LENGTH_TOO_LONG",
        DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG,
    ),
    (59, "DRCET_FOOTPRINT", DRCE_FOOTPRINT),
    (
        60,
        "DRCET_FOOTPRINT_TYPE_MISMATCH",
        DRCE_FOOTPRINT_TYPE_MISMATCH,
    ),
    (61, "DRCET_PAD_TH_WITH_NO_HOLE", DRCE_PAD_TH_WITH_NO_HOLE),
    (
        62,
        "DRCET_MIRRORED_TEXT_ON_FRONT_LAYER",
        DRCE_MIRRORED_TEXT_ON_FRONT_LAYER,
    ),
    (
        63,
        "DRCET_NONMIRRORED_TEXT_ON_BACK_LAYER",
        DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER,
    ),
    (
        64,
        "DRCET_MISSING_TUNING_PROFILE",
        DRCE_MISSING_TUNING_PROFILE,
    ),
    (
        65,
        "DRCET_TUNING_PROFILE_IMPLICIT_RULES",
        DRCE_TUNING_PROFILE_IMPLICIT_RULES,
    ),
    (
        66,
        "DRCET_TRACK_ON_POST_MACHINED_LAYER",
        DRCE_TRACK_ON_POST_MACHINED_LAYER,
    ),
    (
        67,
        "DRCET_TRACK_NOT_CENTERED_ON_VIA",
        DRCE_TRACK_NOT_CENTERED_ON_VIA,
    ),
    (
        68,
        "DRCET_FOOTPRINT_SCALED_WITH_PADS",
        DRCE_FOOTPRINT_SCALED_WITH_PADS,
    ),
    (
        69,
        "DRCET_NET_CHAIN_STUB_TOO_LONG",
        DRCE_NET_CHAIN_STUB_TOO_LONG,
    ),
    (
        70,
        "DRCET_NET_CHAIN_RETURN_PATH_BREAK",
        DRCE_NET_CHAIN_RETURN_PATH_BREAK,
    ),
    (
        71,
        "DRCET_NET_CHAIN_TUNING_PROFILES",
        DRCE_NET_CHAIN_TUNING_PROFILES,
    ),
    (
        72,
        "DRCET_MALFORMED_MICROVIA_STACK_SPAN",
        DRCE_MALFORMED_MICROVIA_STACK_SPAN,
    ),
    (
        73,
        "DRCET_MICROVIA_STACK_NOT_FILLED",
        DRCE_MICROVIA_STACK_NOT_FILLED,
    ),
    (74, "DRCET_MICROVIA_STACK_DEPTH", DRCE_MICROVIA_STACK_DEPTH),
    (75, "DRCET_HOLE_ASPECT_RATIO", DRCE_HOLE_ASPECT_RATIO),
    (
        76,
        "DRCET_MICROVIA_CROSSES_CORE",
        DRCE_MICROVIA_CROSSES_CORE,
    ),
    (77, "DRCET_COPPER_OUTSIDE_BOARD", DRCE_COPPER_OUTSIDE_BOARD),
];

/// Port of `ToProtoEnum<PCB_DRC_CODE, DrcErrorType>`: `DRCET_UNKNOWN` (0) for codes without a
/// counterpart (`DRCE_VIA_STITCH_OVERLAP` and anything out of range).
pub fn drc_error_type_to_proto(code: i32) -> i32 {
    if code == 0 {
        return 0;
    }

    DRC_ERROR_TYPES
        .iter()
        .find(|(_, _, c)| *c == code)
        .map_or(0, |(v, _, _)| *v)
}

/// Port of `FromProtoEnum<PCB_DRC_CODE, DrcErrorType>`: 0 for `DRCET_UNKNOWN` and unknown
/// values.
pub fn drc_error_type_from_proto(value: i32) -> i32 {
    DRC_ERROR_TYPES
        .iter()
        .find(|(v, _, _)| *v == value)
        .map_or(0, |(_, _, c)| *c)
}

/// `kiapi.common.types.MarkerType` names.
const MARKER_TYPES: &[(i32, &str)] = &[
    (0, "MT_UNKNOWN"),
    (1, "MT_ERC"),
    (2, "MT_DRC"),
    (3, "MT_DRAWING_SHEET"),
    (4, "MT_RATSNEST"),
    (5, "MT_PARITY"),
    (6, "MT_SIMULATION"),
];

/// Port of `ToProtoEnum<MARKER_BASE::MARKER_T, types::MarkerType>`.
pub fn marker_type_to_proto(marker_type: MarkerT) -> i32 {
    match marker_type {
        MarkerT::Unspec => 0,
        MarkerT::Erc => 1,
        MarkerT::Drc => 2,
        MarkerT::DrawingSheet => 3,
        MarkerT::Ratsnest => 4,
        MarkerT::Parity => 5,
        MarkerT::Simulation => 6,
    }
}

/// Port of `FromProtoEnum<MARKER_BASE::MARKER_T, types::MarkerType>` (unknown values:
/// `MARKER_UNSPEC`).
pub fn marker_type_from_proto(value: i32) -> MarkerT {
    match value {
        1 => MarkerT::Erc,
        2 => MarkerT::Drc,
        3 => MarkerT::DrawingSheet,
        4 => MarkerT::Ratsnest,
        5 => MarkerT::Parity,
        6 => MarkerT::Simulation,
        _ => MarkerT::Unspec,
    }
}

/// `kiapi.common.types.RuleSeverity` names.
const RULE_SEVERITIES: &[(i32, &str)] = &[
    (0, "RS_UNKNOWN"),
    (1, "RS_WARNING"),
    (2, "RS_ERROR"),
    (3, "RS_EXCLUSION"),
    (4, "RS_IGNORE"),
    (5, "RS_INFO"),
    (6, "RS_ACTION"),
    (7, "RS_DEBUG"),
    (8, "RS_UNDEFINED"),
];

/// Port of `ToProtoEnum<SEVERITY, types::RuleSeverity>`.
pub fn rule_severity_to_proto(severity: Severity) -> i32 {
    match severity {
        Severity::Warning => 1,
        Severity::Error => 2,
        Severity::Exclusion => 3,
        Severity::Ignore => 4,
        Severity::Info => 5,
        Severity::Action => 6,
        Severity::Debug => 7,
        Severity::Undefined => 8,
    }
}

/// The `PCB_LAYER_ID` of a `kiapi.board.types.BoardLayer` value 3 ..= 98 (`BL_F_Cu` ..
/// `BL_User_45`), in enum order.
fn board_layer_table() -> impl Iterator<Item = (i32, PcbLayerId)> {
    let mut layers: Vec<PcbLayerId> = vec![PcbLayerId::FCu];
    layers.extend((1..=30).map(|n| PcbLayerId(2 * n + 2))); // In1_Cu .. In30_Cu
    layers.extend([
        PcbLayerId::BCu,
        PcbLayerId::BAdhes,
        PcbLayerId::FAdhes,
        PcbLayerId::BPaste,
        PcbLayerId::FPaste,
        PcbLayerId::BSilkS,
        PcbLayerId::FSilkS,
        PcbLayerId::BMask,
        PcbLayerId::FMask,
        PcbLayerId::DwgsUser,
        PcbLayerId::CmtsUser,
        PcbLayerId::Eco1User,
        PcbLayerId::Eco2User,
        PcbLayerId::EdgeCuts,
        PcbLayerId::Margin,
        PcbLayerId::BCrtYd,
        PcbLayerId::FCrtYd,
        PcbLayerId::BFab,
        PcbLayerId::FFab,
    ]);
    layers.extend((1..=9).map(|n| PcbLayerId(37 + 2 * n))); // User_1 .. User_9
    layers.push(PcbLayerId::Rescue);
    layers.extend((10..=45).map(|n| PcbLayerId(37 + 2 * n))); // User_10 .. User_45
    layers
        .into_iter()
        .zip(3..)
        .map(|(layer, value)| (value, layer))
}

/// Port of `ToProtoEnum<PCB_LAYER_ID, board::types::BoardLayer>` (`BL_UNKNOWN` for ids
/// without a value; C++ also asserts).
pub fn board_layer_to_proto(layer: PcbLayerId) -> i32 {
    match layer {
        PcbLayerId::UndefinedLayer => 1,
        PcbLayerId::UnselectedLayer => 2,
        _ => board_layer_table()
            .find(|(_, l)| *l == layer)
            .map_or(0, |(v, _)| v),
    }
}

/// Port of `FromProtoEnum<PCB_LAYER_ID, board::types::BoardLayer>`: `BL_UNKNOWN`,
/// `BL_UNDEFINED` and values outside the enum give `UNDEFINED_LAYER` (the C++ asserts for the
/// latter).
pub fn board_layer_from_proto(value: i32) -> PcbLayerId {
    match value {
        2 => PcbLayerId::UnselectedLayer,
        _ => board_layer_table()
            .find(|(v, _)| *v == value)
            .map_or(PcbLayerId::UndefinedLayer, |(_, l)| l),
    }
}

/// The name of a `BoardLayer` value (`BL_` + the C++ enumerator name).
fn board_layer_name(value: i32) -> Option<String> {
    match value {
        0 => Some("BL_UNKNOWN".to_string()),
        1 => Some("BL_UNDEFINED".to_string()),
        2 => Some("BL_UNSELECTED".to_string()),
        _ => board_layer_table()
            .find(|(v, _)| *v == value)
            .map(|(_, layer)| {
                format!(
                    "BL_{}",
                    kicad_core::lset::Lset::name(layer).replace('.', "_")
                )
            }),
    }
}

/// The value of a `BoardLayer` name.
fn board_layer_value(name: &str) -> Option<i32> {
    (0..=98).find(|&v| board_layer_name(v).as_deref() == Some(name))
}

fn enum_name(table: &[(i32, &'static str)], value: i32) -> Option<&'static str> {
    table.iter().find(|(v, _)| *v == value).map(|(_, n)| *n)
}

fn enum_value(table: &[(i32, &'static str)], name: &str) -> Option<i32> {
    table.iter().find(|(_, n)| *n == name).map(|(v, _)| *v)
}

/// Which enum a field holds (for JSON names).
#[derive(Clone, Copy)]
enum EnumKind {
    DrcErrorType,
    BoardLayer,
    RuleSeverity,
    MarkerType,
}

impl EnumKind {
    fn name(self, value: i32) -> Option<String> {
        match self {
            EnumKind::DrcErrorType => DRC_ERROR_TYPES
                .iter()
                .find(|(v, _, _)| *v == value)
                .map(|(_, n, _)| n.to_string()),
            EnumKind::BoardLayer => board_layer_name(value),
            EnumKind::RuleSeverity => enum_name(RULE_SEVERITIES, value).map(str::to_string),
            EnumKind::MarkerType => enum_name(MARKER_TYPES, value).map(str::to_string),
        }
    }

    fn value(self, name: &str) -> Option<i32> {
        match self {
            EnumKind::DrcErrorType => DRC_ERROR_TYPES
                .iter()
                .find(|(_, n, _)| *n == name)
                .map(|(v, _, _)| *v),
            EnumKind::BoardLayer => board_layer_value(name),
            EnumKind::RuleSeverity => enum_value(RULE_SEVERITIES, name),
            EnumKind::MarkerType => enum_value(MARKER_TYPES, name),
        }
    }
}

// ---- wire format -------------------------------------------------------------------------------

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }

    out.push(value as u8);
}

fn put_tag(out: &mut Vec<u8>, field: u32, wire_type: u32) {
    put_varint(out, u64::from((field << 3) | wire_type));
}

/// An `int32` / enum field: negative values are sign-extended to ten bytes.
fn put_int32(out: &mut Vec<u8>, field: u32, value: i32) {
    if value != 0 {
        put_tag(out, field, 0);
        put_varint(out, i64::from(value) as u64);
    }
}

fn put_int64(out: &mut Vec<u8>, field: u32, value: i64) {
    if value != 0 {
        put_tag(out, field, 0);
        put_varint(out, value as u64);
    }
}

fn put_bytes(out: &mut Vec<u8>, field: u32, bytes: &[u8]) {
    put_tag(out, field, 2);
    put_varint(out, bytes.len() as u64);
    out.extend_from_slice(bytes);
}

fn put_string(out: &mut Vec<u8>, field: u32, value: &str) {
    if !value.is_empty() {
        put_bytes(out, field, value.as_bytes());
    }
}

impl ProtoVector2 {
    /// `SerializeAsString()`.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_int64(&mut out, 1, self.x_nm);
        put_int64(&mut out, 2, self.y_nm);
        out
    }
}

impl DrcMarkerMsg {
    /// `SerializeAsString()`: the proto3 wire format, fields in number order, default values
    /// omitted (the generated C++ serializer).
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_int32(&mut out, 1, self.error_type);

        if let Some(position) = &self.position {
            put_bytes(&mut out, 2, &position.serialize());
        }

        for item in &self.items {
            let mut kiid = Vec::new();
            put_string(&mut kiid, 1, item);
            put_bytes(&mut out, 3, &kiid);
        }

        put_int32(&mut out, 4, self.layer);
        put_string(&mut out, 5, &self.violating_rule);
        put_string(&mut out, 6, &self.description);
        put_int32(&mut out, 7, self.severity);

        if self.excluded {
            put_tag(&mut out, 8, 0);
            put_varint(&mut out, 1);
        }

        put_int32(&mut out, 9, self.marker_type);
        out
    }
}

impl DrcExclusionMsg {
    /// `SerializeAsString()`.
    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();

        if let Some(marker) = &self.marker {
            put_bytes(&mut out, 1, &marker.serialize());
        }

        put_string(&mut out, 2, &self.comment);
        out
    }
}

// ---- ProtoJSON printing (MessageToJsonString, preserve_proto_field_names) ---------------------

fn enum_json(kind: EnumKind, value: i32) -> Value {
    // An enum value without a name is printed as its number.
    match kind.name(value) {
        Some(name) => Value::String(name),
        None => Value::Number(Number::from(value)),
    }
}

fn put_json_enum(map: &mut Map<String, Value>, key: &str, kind: EnumKind, value: i32) {
    if value != 0 {
        map.insert(key.to_string(), enum_json(kind, value));
    }
}

fn put_json_string(map: &mut Map<String, Value>, key: &str, value: &str) {
    if !value.is_empty() {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}

impl ProtoVector2 {
    /// ProtoJSON (int64 values are strings).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();

        if self.x_nm != 0 {
            map.insert("x_nm".to_string(), Value::String(self.x_nm.to_string()));
        }

        if self.y_nm != 0 {
            map.insert("y_nm".to_string(), Value::String(self.y_nm.to_string()));
        }

        Value::Object(map)
    }
}

impl DrcMarkerMsg {
    /// ProtoJSON with the proto field names, fields in number order.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        put_json_enum(
            &mut map,
            "error_type",
            EnumKind::DrcErrorType,
            self.error_type,
        );

        if let Some(position) = &self.position {
            map.insert("position".to_string(), position.to_json());
        }

        if !self.items.is_empty() {
            let items = self
                .items
                .iter()
                .map(|value| {
                    let mut kiid = Map::new();
                    put_json_string(&mut kiid, "value", value);
                    Value::Object(kiid)
                })
                .collect();
            map.insert("items".to_string(), Value::Array(items));
        }

        put_json_enum(&mut map, "layer", EnumKind::BoardLayer, self.layer);
        put_json_string(&mut map, "violating_rule", &self.violating_rule);
        put_json_string(&mut map, "description", &self.description);
        put_json_enum(&mut map, "severity", EnumKind::RuleSeverity, self.severity);

        if self.excluded {
            map.insert("excluded".to_string(), Value::Bool(true));
        }

        put_json_enum(
            &mut map,
            "marker_type",
            EnumKind::MarkerType,
            self.marker_type,
        );
        Value::Object(map)
    }
}

impl DrcExclusionMsg {
    /// Port of `to_json( json&, const DRC_EXCLUSION& )`: `MessageToJsonString` with
    /// `preserve_proto_field_names`, re-parsed by nlohmann (the order of the keys is that of
    /// the fields; `nlohmann::json` sorts them when the project file is written).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();

        if let Some(marker) = &self.marker {
            map.insert("marker".to_string(), marker.to_json());
        }

        put_json_string(&mut map, "comment", &self.comment);
        Value::Object(map)
    }
}

// ---- ProtoJSON parsing (JsonStringToMessage, protobuf 3.21 default options) --------------------

/// Why `JsonStringToMessage` rejected the input (the message is then left unchanged).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProtoJsonError(pub String);

type JsonResult<T> = Result<T, ProtoJsonError>;

fn err<T>(msg: impl Into<String>) -> JsonResult<T> {
    Err(ProtoJsonError(msg.into()))
}

/// `DataPiece::ToInt64` / `ToInt32` of a JSON value: an integer number, a double that converts
/// exactly, or a string `safe_strto64` accepts (no surrounding spaces).
fn json_integer(value: &Value, min: i64, max: i64) -> JsonResult<i64> {
    let parsed = match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i)
            } else if n.as_u64().is_some() {
                None
            } else {
                // FloatingPointToIntConvertAndCheck: the double must convert exactly.
                let d = n.as_f64().unwrap_or(f64::NAN);

                if d.is_finite()
                    && d.trunc() == d
                    && d >= -9.223_372_036_854_776e18
                    && d < 9.223_372_036_854_776e18
                {
                    Some(d as i64)
                } else {
                    None
                }
            }
        }
        Value::String(s) => parse_strto64(s),
        _ => None,
    };

    match parsed {
        Some(i) if i >= min && i <= max => Ok(i),
        _ => err(format!("invalid integer {value}")),
    }
}

/// `safe_strto64` behind `DataPiece::StringToNumber`: an optional sign and decimal digits;
/// strings starting or ending with a space are rejected first.
fn parse_strto64(s: &str) -> Option<i64> {
    if s.starts_with(' ') || s.ends_with(' ') {
        return None;
    }

    let s = s.trim_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };

    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }

    let mut value: i64 = 0;

    for b in digits.bytes() {
        let d = i64::from(b - b'0');
        value = value.checked_mul(10)?;
        value = if negative {
            value.checked_sub(d)?
        } else {
            value.checked_add(d)?
        };
    }

    Some(value)
}

fn json_enum(kind: EnumKind, value: &Value) -> JsonResult<i32> {
    match value {
        Value::String(name) => {
            // First try the given value as a name, then an int sent as a string (only if it is
            // a declared value).
            if let Some(v) = kind.value(name) {
                return Ok(v);
            }

            if let Some(v) = parse_strto64(name).and_then(|v| i32::try_from(v).ok()) {
                if kind.name(v).is_some() {
                    return Ok(v);
                }
            }

            err(format!("invalid enum value {name}"))
        }
        // Unknown enum values are preserved.
        _ => json_integer(value, i64::from(i32::MIN), i64::from(i32::MAX)).map(|v| v as i32),
    }
}

fn json_string(value: &Value) -> JsonResult<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        _ => err("invalid string"),
    }
}

/// `DataPiece::ToBool`: a JSON boolean or a string `safe_strtob` accepts.
fn json_bool(value: &Value) -> JsonResult<bool> {
    match value {
        Value::Bool(b) => Ok(*b),
        Value::String(s) => match s.to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" => Ok(true),
            "false" | "f" | "no" | "n" | "0" => Ok(false),
            _ => err("invalid bool"),
        },
        _ => err("invalid bool"),
    }
}

fn json_object(value: &Value) -> JsonResult<&Map<String, Value>> {
    match value {
        Value::Object(map) => Ok(map),
        _ => err("expected an object"),
    }
}

/// The field a JSON key names: the proto name or the lowerCamelCase json name.
fn field_of<'f>(key: &str, fields: &[(&'f str, &'f str)]) -> JsonResult<&'f str> {
    fields
        .iter()
        .find(|(name, json_name)| key == *name || key == *json_name)
        .map(|(name, _)| *name)
        .ok_or_else(|| ProtoJsonError(format!("Cannot find field: {key}")))
}

fn parse_vector2(value: &Value, into: &mut ProtoVector2) -> JsonResult<()> {
    const FIELDS: &[(&str, &str)] = &[("x_nm", "xNm"), ("y_nm", "yNm")];

    for (key, v) in json_object(value)? {
        let field = field_of(key, FIELDS)?;

        if v.is_null() {
            continue;
        }

        let parsed = json_integer(v, i64::MIN, i64::MAX)?;

        match field {
            "x_nm" => into.x_nm = parsed,
            _ => into.y_nm = parsed,
        }
    }

    Ok(())
}

fn parse_kiid(value: &Value) -> JsonResult<String> {
    const FIELDS: &[(&str, &str)] = &[("value", "value")];
    let mut result = String::new();

    for (key, v) in json_object(value)? {
        field_of(key, FIELDS)?;

        if !v.is_null() {
            result = json_string(v)?;
        }
    }

    Ok(result)
}

fn parse_marker(value: &Value, into: &mut DrcMarkerMsg) -> JsonResult<()> {
    const FIELDS: &[(&str, &str)] = &[
        ("error_type", "errorType"),
        ("position", "position"),
        ("items", "items"),
        ("layer", "layer"),
        ("violating_rule", "violatingRule"),
        ("description", "description"),
        ("severity", "severity"),
        ("excluded", "excluded"),
        ("marker_type", "markerType"),
    ];

    for (key, v) in json_object(value)? {
        let field = field_of(key, FIELDS)?;

        // A null value leaves the field unset.
        if v.is_null() {
            continue;
        }

        match field {
            "error_type" => into.error_type = json_enum(EnumKind::DrcErrorType, v)?,
            "position" => {
                // A message field given twice is merged.
                let mut position = into.position.unwrap_or_default();
                parse_vector2(v, &mut position)?;
                into.position = Some(position);
            }
            "items" => match v {
                Value::Array(list) => {
                    for element in list {
                        into.items.push(parse_kiid(element)?);
                    }
                }
                // The 3.21 converter accepts a single element without the list.
                _ => into.items.push(parse_kiid(v)?),
            },
            "layer" => into.layer = json_enum(EnumKind::BoardLayer, v)?,
            "violating_rule" => into.violating_rule = json_string(v)?,
            "description" => into.description = json_string(v)?,
            "severity" => into.severity = json_enum(EnumKind::RuleSeverity, v)?,
            "excluded" => into.excluded = json_bool(v)?,
            _ => into.marker_type = json_enum(EnumKind::MarkerType, v)?,
        }
    }

    Ok(())
}

/// Port of `JsonStringToMessage( aJson.dump(), &message )` for a `DrcExclusion` (protobuf 3.21,
/// default options: unknown fields are errors, enum names must be exact, nulls are skipped).
/// On error the caller keeps its (empty) message, as the 3.21 implementation does.
pub fn parse_exclusion_json(value: &Value) -> JsonResult<DrcExclusionMsg> {
    const FIELDS: &[(&str, &str)] = &[("marker", "marker"), ("comment", "comment")];
    let mut message = DrcExclusionMsg::default();

    for (key, v) in json_object(value)? {
        let field = field_of(key, FIELDS)?;

        if v.is_null() {
            continue;
        }

        match field {
            "marker" => {
                let mut marker = message.marker.take().unwrap_or_default();
                parse_marker(v, &mut marker)?;
                message.marker = Some(marker);
            }
            _ => message.comment = json_string(v)?,
        }
    }

    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enum_tables() {
        assert_eq!(drc_error_type_to_proto(DRCE_CLEARANCE), 5);
        assert_eq!(drc_error_type_to_proto(DRCE_DP_GAP_OUT_OF_RANGE), 57);
        assert_eq!(drc_error_type_to_proto(DRCE_VIA_STITCH_OVERLAP), 0);
        assert_eq!(drc_error_type_from_proto(77), DRCE_COPPER_OUTSIDE_BOARD);
        assert_eq!(drc_error_type_from_proto(99), 0);

        for code in DRCE_FIRST..=DRCE_LAST {
            let v = drc_error_type_to_proto(code);

            if code != DRCE_VIA_STITCH_OVERLAP {
                assert_eq!(drc_error_type_from_proto(v), code);
            }
        }

        assert_eq!(board_layer_to_proto(PcbLayerId::FCu), 3);
        assert_eq!(board_layer_to_proto(PcbLayerId::In1Cu), 4);
        assert_eq!(board_layer_to_proto(PcbLayerId::In30Cu), 33);
        assert_eq!(board_layer_to_proto(PcbLayerId::BCu), 34);
        assert_eq!(board_layer_to_proto(PcbLayerId::EdgeCuts), 47);
        assert_eq!(board_layer_to_proto(PcbLayerId::User9), 61);
        assert_eq!(board_layer_to_proto(PcbLayerId::Rescue), 62);
        assert_eq!(board_layer_to_proto(PcbLayerId::User45), 98);
        assert_eq!(board_layer_to_proto(PcbLayerId::UndefinedLayer), 1);
        assert_eq!(board_layer_from_proto(98), PcbLayerId::User45);
        assert_eq!(board_layer_from_proto(0), PcbLayerId::UndefinedLayer);
        assert_eq!(board_layer_name(3).unwrap(), "BL_F_Cu");
        assert_eq!(board_layer_name(46).unwrap(), "BL_Eco2_User");
        assert_eq!(board_layer_name(63).unwrap(), "BL_User_10");
        assert_eq!(board_layer_value("BL_Edge_Cuts"), Some(47));
        assert_eq!(board_layer_value("BL_In12_Cu"), Some(15));
    }

    #[test]
    fn wire_format() {
        let msg = DrcMarkerMsg {
            error_type: 5,
            position: Some(ProtoVector2 {
                x_nm: 150,
                y_nm: -1,
            }),
            items: vec!["ab".into(), String::new()],
            layer: 3,
            marker_type: 2,
            ..DrcMarkerMsg::default()
        };
        let mut want = vec![0x08, 5, 0x12, 14, 0x08, 0x96, 0x01, 0x10];
        want.extend([0xff; 9]);
        want.push(0x01);
        want.extend([0x1a, 4, 0x0a, 2, b'a', b'b', 0x1a, 0, 0x20, 3, 0x48, 2]);
        assert_eq!(msg.serialize(), want);

        let msg = DrcMarkerMsg {
            position: Some(ProtoVector2::default()),
            ..DrcMarkerMsg::default()
        };
        assert_eq!(msg.serialize(), vec![0x12, 0]);
    }

    #[test]
    fn json_round_trip() {
        let msg = DrcExclusionMsg {
            marker: Some(DrcMarkerMsg {
                error_type: 5,
                position: Some(ProtoVector2 {
                    x_nm: 123,
                    y_nm: -456,
                }),
                items: vec!["x".into()],
                layer: 3,
                marker_type: 2,
                ..DrcMarkerMsg::default()
            }),
            comment: "ok".into(),
        };
        let json = msg.to_json();
        assert_eq!(
            kicad_core::json::dump(&json, None, kicad_core::json::ObjectOrder::Sorted),
            "{\"comment\":\"ok\",\"marker\":{\"error_type\":\"DRCET_CLEARANCE\",\"items\":\
             [{\"value\":\"x\"}],\"layer\":\"BL_F_Cu\",\"marker_type\":\"MT_DRC\",\"position\":\
             {\"x_nm\":\"123\",\"y_nm\":\"-456\"}}}"
        );
        assert_eq!(parse_exclusion_json(&json).unwrap(), msg);

        let alt: Value = serde_json_like(
            r#"{"marker":{"errorType":5,"position":{"xNm":123.0,"yNm":"-456"},"items":[{"value":"x"}],"layer":"BL_F_Cu","markerType":"2","severity":null}, "comment":"ok"}"#,
        );
        assert_eq!(parse_exclusion_json(&alt).unwrap(), msg);

        for bad in [
            r#"{"marker":{"foo":1}}"#,
            r#"{"marker":{"error_type":"DRCET_NOPE"}}"#,
            r#"{"marker":{"position":{"x_nm":1.5}}}"#,
            r#"{"marker":{"position":{"x_nm":" 1"}}}"#,
            r#"{"comment":5}"#,
            r#"[1]"#,
        ] {
            assert!(
                parse_exclusion_json(&serde_json_like(bad)).is_err(),
                "{bad}"
            );
        }

        // Unknown enum numbers are kept and printed as numbers.
        let v = parse_exclusion_json(&serde_json_like(r#"{"marker":{"error_type":500}}"#)).unwrap();
        assert_eq!(v.marker.as_ref().unwrap().error_type, 500);
        assert_eq!(
            kicad_core::json::dump(&v.to_json(), None, kicad_core::json::ObjectOrder::Sorted),
            "{\"marker\":{\"error_type\":500}}"
        );
    }

    fn serde_json_like(text: &str) -> Value {
        kicad_core::json::parse(text, kicad_core::json::ObjectOrder::Sorted).unwrap()
    }
}
