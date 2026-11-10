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

//! Port of the `NESTED_SETTINGS` side of `pcbnew/board_design_settings.cpp`
//! (`BOARD_DESIGN_SETTINGS`): the `m_params` registrations of the constructor (every `PARAM`,
//! `PARAM_SCALED`, `PARAM_ENUM`, `PARAM_LIST` and `PARAM_LAMBDA` with its key, default, range,
//! scale and lambda bodies), the schema migrations 0 -> 1 -> 2 -> 3 (`migrateSchema0to1`, the
//! inline 1 -> 2 lambda, `migrateSchema2to3`), `LoadFromFile` with its legacy post-step, and
//! the store side (`Store`, `NESTED_SETTINGS::SaveToFile`). Also ports the parts of
//! `common/settings/bom_settings.cpp` that the `FIELDS_TABLE_BOM_SETTINGS` base registers
//! (`BOM_FIELD`, `BOM_PRESET`, `BOM_FMT_PRESET` with their `to_json` / `from_json`,
//! `BOM_PRESET::DefaultEditing`, `BOM_FMT_PRESET::CSV`), work package SET-4.
//!
//! The generic machinery is `kicad_core::json_settings` / `kicad_core::settings_parameters`.
//! The parameters operate on a private [`Target`] (the design settings plus the BOM members the
//! Rust [`BoardDesignSettings`] does not carry); the public functions move the caller's
//! settings into it for the duration of a call.
//!
//! `m_resetParamsIfMissing` is false for this object: a key missing from the project keeps the
//! constructor or board-file value; a parameter default only replaces out-of-range values;
//! `PARAM_LAMBDA` setters always run (with their default when the key is missing).

use std::cell::RefCell;
use std::rc::Rc;

use kicad_core::json::{self, FromJson, JsonError, ObjectOrder, ToJson, Value};
use kicad_core::json_settings::{JsonSettingsInternals, Migrator, NestedSettings};
use kicad_core::layer_ids::{is_copper_layer, to_layer_id};
use kicad_core::lset::Lset;
use kicad_core::report_severity::{severity_from_string, severity_to_string};
use kicad_core::settings_field;
use kicad_core::settings_parameters::{
    JsonValue, Param, ParamEnum, ParamLambda, ParamList, ParamScaled, ParamT, ParamValue,
};
use kicad_core::{PCB_IU_SCALE, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::cmath::{f64_to_i32_x86, f64_to_i64_x86};
use kicad_geom::geometry::eda_angle::{EdaAngle, EdaAngleT};
use kicad_geom::geometry::geometry_utils::ARC_HIGH_DEF;

use crate::board_design_settings::*;
use crate::drc_api::*;
use crate::pcb_dimension::{DimPrecision, DimTextPosition, DimUnitsFormat, DimUnitsMode};
use crate::teardrop_parameters::{
    TargetTd, teardrop_target_canonical_name, teardrop_target_type_from_canonical_name,
};
use crate::zone_settings::{
    CornerSmoothing, IslandRemovalMode, ZoneBorderDisplayStyle, ZoneConnection, ZoneFillMode,
};

/// `bdsSchemaVersion`.
pub const BDS_SCHEMA_VERSION: i32 = 3;

/// The dotted path of the design settings in the project file (`board.design_settings`).
pub const BDS_JSON_PATH: &str = "board.design_settings";

/// `MAX_MICROVIA_STACK_PITCH_MM` (`include/board_design_settings_defaults.h`).
pub const MAX_MICROVIA_STACK_PITCH_MM: f64 = 25.0;

/// `TEXT_MIN_SIZE_MM` / `TEXT_MAX_SIZE_MM` (`include/eda_text.h`).
const TEXT_MIN_SIZE_MM: f64 = 0.001;
const TEXT_MAX_SIZE_MM: f64 = 250.0;

/// Zone defaults of `zone_settings.h` used as parameter fallbacks.
const ZONE_CLEARANCE_MM: f64 = 0.5;
const ZONE_THICKNESS_MM: f64 = 0.25;
const ZONE_THICKNESS_MIN_VALUE_MM: f64 = 0.025;
const ZONE_BORDER_HATCH_DIST_MM: f64 = 0.5;
const ZONE_BORDER_HATCH_MINDIST_MM: f64 = 0.1;
const ZONE_BORDER_HATCH_MAXDIST_MM: f64 = 2.0;
const ZONE_THERMAL_RELIEF_GAP_MM: f64 = 0.5;
const ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM: f64 = 0.5;

/// `pcbIUScale.MM_PER_IU` (`1 / IU_PER_MM`).
const MM_PER_IU: f64 = PCB_IU_SCALE.mm_per_iu;

// ------------------------------------------------------------------------------------------
// DRC exclusions
// ------------------------------------------------------------------------------------------

/// The `DRC_EXCLUSION` conversions the parameters need (`from_json`, `FromLegacyStrings`).
/// Their port lives in `kicad-drc` (`exclusion::from_json`, `exclusion::from_legacy_strings`),
/// above this crate, so the caller passes them in. `to_json` is
/// [`DrcExclusion::json`].
pub trait DrcExclusionCodec {
    /// `entry.get<DRC_EXCLUSION>()`.
    fn from_json(&self, json: &Value) -> DrcExclusion;
    /// `DRC_EXCLUSION::FromLegacyStrings( aMarkerData, aComment )`.
    fn from_legacy_strings(&self, marker_data: &str, comment: &str) -> DrcExclusion;
}

/// The codec used when no DRC is linked (deviation SET-4-1 in `PORT_STATUS.md`): an exclusion
/// object is kept verbatim with its compact sorted dump as sort key (no protobuf
/// canonicalisation), and legacy marker strings cannot be decoded (empty sort key: the entry
/// is dropped, as C++ drops strings it cannot parse).
#[derive(Clone, Copy, Debug, Default)]
pub struct RawDrcExclusionCodec;

impl DrcExclusionCodec for RawDrcExclusionCodec {
    fn from_json(&self, json: &Value) -> DrcExclusion {
        let comment = match json.get("comment") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };

        DrcExclusion {
            sort_key: json::dump(json, None, ObjectOrder::Sorted),
            comment,
            json: json.clone(),
        }
    }

    fn from_legacy_strings(&self, _marker_data: &str, comment: &str) -> DrcExclusion {
        DrcExclusion {
            sort_key: String::new(),
            comment: comment.to_string(),
            json: Value::Null,
        }
    }
}

// ------------------------------------------------------------------------------------------
// BOM settings (common/settings/bom_settings.cpp)
// ------------------------------------------------------------------------------------------

/// `BOM_FIELD`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BomField {
    pub name: String,   // name
    pub label: String,  // label
    pub show: bool,     // show
    pub group_by: bool, // groupBy
}

/// `BOM_FILTER_SCOPE` (`NLOHMANN_JSON_SERIALIZE_ENUM`: unknown values read as the first
/// entry).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum BomFilterScope {
    #[default]
    Reference,
    Visible,
    All,
}

impl BomFilterScope {
    fn to_json_value(self) -> Value {
        Value::from(match self {
            BomFilterScope::Reference => "reference",
            BomFilterScope::Visible => "visible",
            BomFilterScope::All => "all",
        })
    }

    fn from_json_value(v: &Value) -> Self {
        match v {
            Value::String(s) if s == "visible" => BomFilterScope::Visible,
            Value::String(s) if s == "all" => BomFilterScope::All,
            _ => BomFilterScope::Reference,
        }
    }
}

/// `BOM_PRESET`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BomPreset {
    pub name: String,                    // name
    pub read_only: bool,                 // readOnly
    pub fields_ordered: Vec<BomField>,   // fieldsOrdered
    pub sort_field: String,              // sortField
    pub sort_asc: bool,                  // sortAsc
    pub filter_string: String,           // filterString
    pub filter_scope: BomFilterScope,    // filterScope
    pub group_symbols: bool,             // groupSymbols
    pub exclude_dnp: bool,               // excludeDNP
    pub include_excluded_from_bom: bool, // includeExcludedFromBOM
}

/// `BOM_FMT_PRESET`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct BomFmtPreset {
    pub name: String,                  // name
    pub read_only: bool,               // readOnly
    pub field_delimiter: String,       // fieldDelimiter
    pub string_delimiter: String,      // stringDelimiter
    pub ref_delimiter: String,         // refDelimiter
    pub ref_range_delimiter: String,   // refRangeDelimiter
    pub keep_tabs: bool,               // keepTabs
    pub keep_line_breaks: bool,        // keepLineBreaks
    pub include_byte_order_mark: bool, // includeByteOrderMark
}

/// `FIELDS_TABLE_BOM_SETTINGS` (the second base class of `BOARD_DESIGN_SETTINGS`). Only the
/// project file reads and writes these; the board model has no use for them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldsTableBomSettings {
    pub bom_export_file_name: String,       // m_BomExportFileName
    pub bom_settings: BomPreset,            // m_BomSettings
    pub bom_presets: Vec<BomPreset>,        // m_BomPresets
    pub bom_fmt_settings: BomFmtPreset,     // m_BomFmtSettings
    pub bom_fmt_presets: Vec<BomFmtPreset>, // m_BomFmtPresets
}

impl Default for FieldsTableBomSettings {
    /// The values the `BOARD_DESIGN_SETTINGS` constructor assigns (`board_design_settings.cpp`
    /// 224-226); the preset lists stay empty.
    fn default() -> Self {
        Self {
            bom_export_file_name: "${PROJECTNAME}.csv".to_string(),
            bom_settings: BomPreset::default_editing(),
            bom_presets: Vec::new(),
            bom_fmt_settings: BomFmtPreset::csv(),
            bom_fmt_presets: Vec::new(),
        }
    }
}

impl BomPreset {
    /// `BOM_PRESET{}` (member initialisers: `sortAsc = true`, everything else empty/false).
    fn default_struct() -> Self {
        Self {
            sort_asc: true,
            ..Default::default()
        }
    }

    /// Port of `BOM_PRESET::DefaultEditing`.
    pub fn default_editing() -> Self {
        let f = |name: &str, label: &str, show: bool, group_by: bool| BomField {
            name: name.to_string(),
            label: label.to_string(),
            show,
            group_by,
        };

        BomPreset {
            name: "Default Editing".to_string(),
            read_only: true,
            fields_ordered: vec![
                f("Reference", "Reference", true, false),
                f("${QUANTITY}", "Qty", true, false),
                f("Value", "Value", true, true),
                f("${DNP}", "DNP", true, true),
                f("${EXCLUDE_FROM_BOM}", "Exclude from BOM", true, true),
                f("${EXCLUDE_FROM_BOARD}", "Exclude from Board", true, true),
                f("${EXCLUDE_FROM_SIM}", "Exclude from Simulation", true, true),
                f(
                    "${EXCLUDE_FROM_POS_FILES}",
                    "Exclude from Position Files",
                    true,
                    true,
                ),
                f("Footprint", "Footprint", true, true),
                f("Datasheet", "Datasheet", true, false),
            ],
            sort_field: "Reference".to_string(),
            sort_asc: true,
            filter_string: String::new(),
            filter_scope: BomFilterScope::Reference,
            group_symbols: true,
            exclude_dnp: false,
            include_excluded_from_bom: true,
        }
    }
}

impl BomFmtPreset {
    /// Port of `BOM_FMT_PRESET::CSV`.
    pub fn csv() -> Self {
        BomFmtPreset {
            name: "CSV".to_string(),
            read_only: true,
            field_delimiter: ",".to_string(),
            string_delimiter: "\"".to_string(),
            ref_delimiter: ",".to_string(),
            ref_range_delimiter: String::new(),
            keep_tabs: false,
            keep_line_breaks: false,
            include_byte_order_mark: false,
        }
    }
}

/// `j.at( key )` (throws `out_of_range` when missing, `type_error` when `j` is no object).
fn at<'v>(j: &'v Value, key: &str) -> Result<&'v Value, JsonError> {
    match j {
        Value::Object(m) => m.get(key).ok_or_else(|| JsonError {
            kind: json::JsonErrorKind::OutOfRange,
            id: 403,
            byte: 0,
            message: format!("key '{key}' not found"),
        }),
        other => Err(JsonError::type_must_be("object", other)),
    }
}

/// `wxString` `from_json` (`common/json_conversions.cpp`): `wxString::FromUTF8(
/// j.get<std::string>().c_str() )`, i.e. cut at the first NUL.
fn wx_string_from_json(j: &Value) -> Result<String, JsonError> {
    let s = String::from_json(j)?;

    Ok(match s.find('\0') {
        Some(p) => s[..p].to_string(),
        None => s,
    })
}

/// `j.value( key, default )` on an object: the converted member when present (throwing when
/// it does not convert), else the default.
fn value_or<T: FromJson>(j: &Value, key: &str, default: T) -> Result<T, JsonError> {
    match j.get(key) {
        Some(v) => T::from_json(v),
        None => Ok(default),
    }
}

impl FromJson for BomField {
    /// Port of `from_json( const nlohmann::json&, BOM_FIELD& )` on a default `BOM_FIELD`.
    fn from_json(j: &Value) -> Result<Self, JsonError> {
        Ok(BomField {
            name: wx_string_from_json(at(j, "name")?)?,
            label: wx_string_from_json(at(j, "label")?)?,
            show: bool::from_json(at(j, "show")?)?,
            group_by: bool::from_json(at(j, "group_by")?)?,
        })
    }
}

impl ToJson for BomField {
    /// Port of `to_json( nlohmann::json&, const BOM_FIELD& )`.
    fn to_json(&self) -> Value {
        let mut m = json::Map::new();
        m.insert("name".into(), Value::from(self.name.as_str()));
        m.insert("label".into(), Value::from(self.label.as_str()));
        m.insert("show".into(), Value::from(self.show));
        m.insert("group_by".into(), Value::from(self.group_by));
        Value::Object(m)
    }
}

impl FromJson for BomPreset {
    /// Port of `from_json( const nlohmann::json&, BOM_PRESET& )` on a default `BOM_PRESET`
    /// (`get<BOM_PRESET>()` default-constructs the target).
    fn from_json(j: &Value) -> Result<Self, JsonError> {
        let mut f = BomPreset::default_struct();
        f.name = wx_string_from_json(at(j, "name")?)?;
        f.fields_ordered = match j.get("fields_ordered") {
            Some(v) => Vec::<BomField>::from_json(v)?,
            None => Vec::new(),
        };
        f.sort_field = wx_string_from_json(at(j, "sort_field")?)?;
        f.sort_asc = bool::from_json(at(j, "sort_asc")?)?;
        f.filter_string = wx_string_from_json(at(j, "filter_string")?)?;
        f.filter_scope = j
            .get("filter_scope")
            .map_or(BomFilterScope::Reference, BomFilterScope::from_json_value);
        f.group_symbols = bool::from_json(at(j, "group_symbols")?)?;
        f.exclude_dnp = bool::from_json(at(j, "exclude_dnp")?)?;

        // Was not present in initial BOM settings in 8.0, so default to false if not found
        f.include_excluded_from_bom = value_or(j, "include_excluded_from_bom", false)?;
        Ok(f)
    }
}

impl ToJson for BomPreset {
    /// Port of `to_json( nlohmann::json&, const BOM_PRESET& )`.
    fn to_json(&self) -> Value {
        let mut m = json::Map::new();
        m.insert("name".into(), Value::from(self.name.as_str()));
        m.insert(
            "fields_ordered".into(),
            Value::Array(self.fields_ordered.iter().map(ToJson::to_json).collect()),
        );
        m.insert("sort_field".into(), Value::from(self.sort_field.as_str()));
        m.insert("sort_asc".into(), Value::from(self.sort_asc));
        m.insert(
            "filter_string".into(),
            Value::from(self.filter_string.as_str()),
        );
        m.insert("filter_scope".into(), self.filter_scope.to_json_value());
        m.insert("group_symbols".into(), Value::from(self.group_symbols));
        m.insert("exclude_dnp".into(), Value::from(self.exclude_dnp));
        m.insert(
            "include_excluded_from_bom".into(),
            Value::from(self.include_excluded_from_bom),
        );
        Value::Object(m)
    }
}

impl FromJson for BomFmtPreset {
    /// Port of `from_json( const nlohmann::json&, BOM_FMT_PRESET& )` on a default
    /// `BOM_FMT_PRESET`.
    fn from_json(j: &Value) -> Result<Self, JsonError> {
        Ok(BomFmtPreset {
            name: wx_string_from_json(at(j, "name")?)?,
            read_only: false,
            field_delimiter: wx_string_from_json(at(j, "field_delimiter")?)?,
            string_delimiter: wx_string_from_json(at(j, "string_delimiter")?)?,
            ref_delimiter: wx_string_from_json(at(j, "ref_delimiter")?)?,
            ref_range_delimiter: wx_string_from_json(at(j, "ref_range_delimiter")?)?,
            keep_tabs: bool::from_json(at(j, "keep_tabs")?)?,
            keep_line_breaks: bool::from_json(at(j, "keep_line_breaks")?)?,
            // Added after the other options, so may not be present in old settings
            include_byte_order_mark: value_or(j, "include_byte_order_mark", false)?,
        })
    }
}

impl ToJson for BomFmtPreset {
    /// Port of `to_json( nlohmann::json&, const BOM_FMT_PRESET& )`.
    fn to_json(&self) -> Value {
        let mut m = json::Map::new();
        m.insert("name".into(), Value::from(self.name.as_str()));
        m.insert(
            "field_delimiter".into(),
            Value::from(self.field_delimiter.as_str()),
        );
        m.insert(
            "string_delimiter".into(),
            Value::from(self.string_delimiter.as_str()),
        );
        m.insert(
            "ref_delimiter".into(),
            Value::from(self.ref_delimiter.as_str()),
        );
        m.insert(
            "ref_range_delimiter".into(),
            Value::from(self.ref_range_delimiter.as_str()),
        );
        m.insert("keep_tabs".into(), Value::from(self.keep_tabs));
        m.insert(
            "keep_line_breaks".into(),
            Value::from(self.keep_line_breaks),
        );
        m.insert(
            "include_byte_order_mark".into(),
            Value::from(self.include_byte_order_mark),
        );
        Value::Object(m)
    }
}

/// `PARAM<BOM_PRESET>` / `PARAM<BOM_FMT_PRESET>`: `Get<T>` is `get<T>()` (a conversion error
/// reads as missing), `==` is the struct's `operator==`.
macro_rules! struct_param_value {
    ($($t:ty),*) => {$(
        impl PartialOrd for $t {
            fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
                None
            }
        }

        impl ParamValue for $t {
            fn get_setting(
                settings: &JsonSettingsInternals,
                path: &str,
            ) -> Result<Option<Self>, JsonError> {
                settings.get::<$t>(path)
            }

            fn same(&self, other: &Self) -> bool {
                self == other
            }
        }
    )*};
}

struct_param_value!(BomPreset, BomFmtPreset);

// ------------------------------------------------------------------------------------------
// PNS::MEANDER_SETTINGS (the JSON side of pcbnew/router/pns_meander.cpp)
// ------------------------------------------------------------------------------------------

/// The members of `PNS::MEANDER_SETTINGS` that `tuning_pattern_settings` reads and writes.
/// [`BoardDesignSettings::meander_settings`] keeps them as the objects `make_settings` writes.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MeanderSettings {
    min_amplitude: i32,            // m_minAmplitude
    max_amplitude: i32,            // m_maxAmplitude
    spacing: i32,                  // m_spacing
    corner_style_chamfer: bool,    // m_cornerStyle == MEANDER_STYLE_CHAMFER
    corner_radius_percentage: i32, // m_cornerRadiusPercentage
    single_sided: bool,            // m_singleSided
}

impl Default for MeanderSettings {
    /// `PNS::MEANDER_SETTINGS()` (`pcbnew/router/pns_meander.cpp`).
    fn default() -> Self {
        Self {
            min_amplitude: PCB_IU_SCALE.mm_to_iu(0.2),
            max_amplitude: PCB_IU_SCALE.mm_to_iu(1.0),
            spacing: PCB_IU_SCALE.mm_to_iu(0.6),
            corner_style_chamfer: false,
            corner_radius_percentage: 80,
            single_sided: false,
        }
    }
}

impl MeanderSettings {
    /// The `make_settings` lambda of the `tuning_pattern_settings` getter.
    fn to_json(self) -> Value {
        let mut entry = json::Map::new();
        entry.insert("min_amplitude".into(), iu_to_mm_json(self.min_amplitude));
        entry.insert("max_amplitude".into(), iu_to_mm_json(self.max_amplitude));
        entry.insert("spacing".into(), iu_to_mm_json(self.spacing));
        entry.insert(
            "corner_style".into(),
            Value::from(if self.corner_style_chamfer { 0 } else { 1 }),
        );
        entry.insert(
            "corner_radius_percentage".into(),
            Value::from(self.corner_radius_percentage),
        );
        entry.insert("single_sided".into(), Value::from(self.single_sided));
        Value::Object(entry)
    }

    /// The `read_settings` lambda of the `tuning_pattern_settings` setter: a fresh
    /// `MEANDER_SETTINGS()` overlaid with the members `entry` contains. `Err` where a `get<T>`
    /// throws.
    fn read(entry: &Value) -> Result<Self, JsonError> {
        let mut settings = MeanderSettings::default();

        if let Some(v) = entry.get("min_amplitude") {
            settings.min_amplitude = mm_to_iu(f64::from_json(v)?);
        }

        if let Some(v) = entry.get("max_amplitude") {
            settings.max_amplitude = mm_to_iu(f64::from_json(v)?);
        }

        if let Some(v) = entry.get("spacing") {
            settings.spacing = mm_to_iu(f64::from_json(v)?);
        }

        if let Some(v) = entry.get("corner_style") {
            // entry["corner_style"] == 0: true for any number equal to zero
            settings.corner_style_chamfer = json::json_eq(v, &Value::from(0));
        }

        if let Some(v) = entry.get("corner_radius_percentage") {
            settings.corner_radius_percentage = i32::from_json(v)?;
        }

        if let Some(v) = entry.get("single_sided") {
            settings.single_sided = bool::from_json(v)?;
        }

        Ok(settings)
    }
}

// ------------------------------------------------------------------------------------------
// Helpers
// ------------------------------------------------------------------------------------------

/// `pcbIUScale.mmToIU( double )` as compiled for the reference platform: the C cast of an
/// out-of-range value yields `INT_MIN` (`cvttsd2si`), PORTING.md section 9.
fn mm_to_iu(mm: f64) -> i32 {
    f64_to_i32_x86(if mm < 0.0 {
        mm * PCB_IU_SCALE.iu_per_mm - 0.5
    } else {
        mm * PCB_IU_SCALE.iu_per_mm + 0.5
    })
}

/// `pcbIUScale.IUTomm( iu )` as a JSON double.
fn iu_to_mm_json(iu: i32) -> Value {
    json::float_value(PCB_IU_SCALE.iu_to_mm(iu))
}

/// `basic_json::empty()`.
fn json_empty(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// `towlower` simple case mapping (as `kicad_core::wxstr::to_lower`, which is crate-private).
fn wx_to_lower(c: char) -> char {
    if c.is_ascii() {
        return c.to_ascii_lowercase();
    }

    let mut it = c.to_lowercase();

    match (it.next(), it.next()) {
        (Some(l), None) => l,
        _ if c == '\u{130}' => 'i',
        _ => c,
    }
}

/// `wxString::CmpNoCase( other ) == 0`.
fn same_no_case(a: &str, b: &str) -> bool {
    let mut ia = a.chars();
    let mut ib = b.chars();

    loop {
        let c1 = ia.next().map_or(0, |c| wx_to_lower(c) as i32);
        let c2 = ib.next().map_or(0, |c| wx_to_lower(c) as i32);

        if c1 == 0 || c1 != c2 {
            return c1 == c2;
        }
    }
}

/// The elements a C++ range-for over a `nlohmann::json` visits: array elements, object
/// values, nothing for `null`, the value itself for a primitive.
fn json_elements(v: &Value) -> Vec<&Value> {
    match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(m) => m.values().collect(),
        Value::Null => Vec::new(),
        other => vec![other],
    }
}

/// `DRC_ITEM::GetItemsWithSeverities()`: every entry of `DRC_ITEM::allItemTypes` before
/// `heading_internal`, in that order, as (settings key, error code). The headings have code 0
/// and an empty key.
const ITEMS_WITH_SEVERITIES: &[(&str, i32)] = &[
    ("", 0),
    ("shorting_items", DRCE_SHORTING_ITEMS),
    ("tracks_crossing", DRCE_TRACKS_CROSSING),
    ("clearance", DRCE_CLEARANCE),
    ("creepage", DRCE_CREEPAGE),
    ("via_dangling", DRCE_DANGLING_VIA),
    ("track_dangling", DRCE_DANGLING_TRACK),
    ("starved_thermal", DRCE_STARVED_THERMAL),
    ("", 0),
    ("copper_edge_clearance", DRCE_EDGE_CLEARANCE),
    ("copper_outside_board", DRCE_COPPER_OUTSIDE_BOARD),
    ("hole_clearance", DRCE_HOLE_CLEARANCE),
    ("hole_to_hole", DRCE_DRILLED_HOLES_TOO_CLOSE),
    ("holes_co_located", DRCE_DRILLED_HOLES_COLOCATED),
    ("track_width", DRCE_TRACK_WIDTH),
    ("track_angle", DRCE_TRACK_ANGLE),
    ("track_segment_length", DRCE_TRACK_SEGMENT_LENGTH),
    ("annular_width", DRCE_ANNULAR_WIDTH),
    ("drill_out_of_range", DRCE_DRILL_OUT_OF_RANGE),
    (
        "microvia_drill_out_of_range",
        DRCE_MICROVIA_DRILL_OUT_OF_RANGE,
    ),
    (
        "malformed_microvia_stack_span",
        DRCE_MALFORMED_MICROVIA_STACK_SPAN,
    ),
    ("microvia_stack_not_filled", DRCE_MICROVIA_STACK_NOT_FILLED),
    ("microvia_stack_depth", DRCE_MICROVIA_STACK_DEPTH),
    ("hole_aspect_ratio", DRCE_HOLE_ASPECT_RATIO),
    ("microvia_crosses_core", DRCE_MICROVIA_CROSSES_CORE),
    ("via_diameter", DRCE_VIA_DIAMETER),
    ("courtyards_overlap", DRCE_OVERLAPPING_FOOTPRINTS),
    ("missing_courtyard", DRCE_MISSING_COURTYARD),
    ("malformed_courtyard", DRCE_MALFORMED_COURTYARD),
    ("invalid_outline", DRCE_INVALID_OUTLINE),
    ("copper_sliver", DRCE_COPPER_SLIVER),
    ("solder_mask_bridge", DRCE_SOLDERMASK_BRIDGE),
    ("connection_width", DRCE_CONNECTION_WIDTH),
    (
        "track_on_post_machined_layer",
        DRCE_TRACK_ON_POST_MACHINED_LAYER,
    ),
    ("track_not_centered_on_via", DRCE_TRACK_NOT_CENTERED_ON_VIA),
    ("via_stitch_overlap", DRCE_VIA_STITCH_OVERLAP),
    (
        "tuning_profile_track_geometries",
        DRCE_TUNING_PROFILE_IMPLICIT_RULES,
    ),
    ("", 0),
    ("duplicate_footprints", DRCE_DUPLICATE_FOOTPRINT),
    ("missing_footprint", DRCE_MISSING_FOOTPRINT),
    ("extra_footprint", DRCE_EXTRA_FOOTPRINT),
    ("footprint_symbol_mismatch", DRCE_SCHEMATIC_PARITY),
    (
        "footprint_symbol_field_mismatch",
        DRCE_SCHEMATIC_FIELDS_PARITY,
    ),
    ("footprint_filters_mismatch", DRCE_FOOTPRINT_FILTERS),
    ("net_conflict", DRCE_NET_CONFLICT),
    ("unconnected_items", DRCE_UNCONNECTED_ITEMS),
    ("", 0),
    ("length_out_of_range", DRCE_LENGTH_OUT_OF_RANGE),
    ("net_chain_stub_length", DRCE_NET_CHAIN_STUB_TOO_LONG),
    ("net_chain_return_path", DRCE_NET_CHAIN_RETURN_PATH_BREAK),
    ("net_chain_tuning_profiles", DRCE_NET_CHAIN_TUNING_PROFILES),
    ("skew_out_of_range", DRCE_SKEW_OUT_OF_RANGE),
    ("too_many_vias", DRCE_VIA_COUNT_OUT_OF_RANGE),
    ("diff_pair_gap_out_of_range", DRCE_DP_GAP_OUT_OF_RANGE),
    (
        "diff_pair_uncoupled_length_too_long",
        DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG,
    ),
    ("", 0),
    ("silk_overlap", DRCE_SILK_CLEARANCE),
    ("silk_over_copper", DRCE_SILK_MASK_CLEARANCE),
    ("silk_edge_clearance", DRCE_SILK_EDGE_CLEARANCE),
    ("text_height", DRCE_TEXT_HEIGHT),
    ("text_thickness", DRCE_TEXT_THICKNESS),
    (
        "mirrored_text_on_front_layer",
        DRCE_MIRRORED_TEXT_ON_FRONT_LAYER,
    ),
    (
        "nonmirrored_text_on_back_layer",
        DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER,
    ),
    ("", 0),
    ("items_not_allowed", DRCE_ALLOWED_ITEMS),
    ("text_on_edge_cuts", DRCE_TEXT_ON_EDGECUTS),
    ("isolated_copper", DRCE_ISOLATED_COPPER),
    ("footprint", DRCE_FOOTPRINT),
    ("padstack", DRCE_PADSTACK),
    ("pth_inside_courtyard", DRCE_PTH_IN_COURTYARD),
    ("npth_inside_courtyard", DRCE_NPTH_IN_COURTYARD),
    ("item_on_disabled_layer", DRCE_DISABLED_LAYER_ITEM),
    ("unresolved_variable", DRCE_UNRESOLVED_VARIABLE),
    ("assertion_failure", DRCE_ASSERTION_FAILURE),
    ("footprint_type_mismatch", DRCE_FOOTPRINT_TYPE_MISMATCH),
    ("lib_footprint_issues", DRCE_LIB_FOOTPRINT_ISSUES),
    ("lib_footprint_mismatch", DRCE_LIB_FOOTPRINT_MISMATCH),
    ("through_hole_pad_without_hole", DRCE_PAD_TH_WITH_NO_HOLE),
    (
        "footprint_scaled_with_pads",
        DRCE_FOOTPRINT_SCALED_WITH_PADS,
    ),
    ("missing_tuning_profile", DRCE_MISSING_TUNING_PROFILE),
];

// ------------------------------------------------------------------------------------------
// The parameter target
// ------------------------------------------------------------------------------------------

/// What the parameters read and write: `this` of the C++ lambdas.
struct Target {
    bds: BoardDesignSettings,
    bom: FieldsTableBomSettings,
}

/// The member assignments of migration 1 -> 2 (the migrator runs on the document; the port
/// records them and applies them right after the load).
#[derive(Clone, Copy, Debug, Default)]
struct MaskMigration {
    solder_mask_expansion: Option<i32>,
    solder_mask_min_width: Option<i32>,
    solder_paste_margin: Option<i32>,
    solder_paste_margin_ratio: Option<f64>,
}

/// Port of the `NESTED_SETTINGS` part of `BOARD_DESIGN_SETTINGS`: the parameter list,
/// migrations, document and the BOM members. One instance corresponds to one
/// `BOARD_DESIGN_SETTINGS` object's JSON state; the design-rule members themselves live in the
/// [`BoardDesignSettings`] passed to each call.
pub struct BoardDesignSettingsJson {
    /// The nested settings engine (`m_internals`, `m_params`, migrations, `m_path`).
    pub nested: NestedSettings<BdsTarget>,
    /// The `FIELDS_TABLE_BOM_SETTINGS` members.
    pub bom: FieldsTableBomSettings,
    mask_migration: Rc<RefCell<MaskMigration>>,
}

/// Opaque parameter target type of [`BoardDesignSettingsJson::nested`].
pub struct BdsTarget(Target);

impl BoardDesignSettingsJson {
    /// Port of the `BOARD_DESIGN_SETTINGS( aParent, aPath )` constructor's JSON part: registers
    /// every parameter and migration. `codec` converts DRC exclusions (see
    /// [`DrcExclusionCodec`]).
    pub fn new(path: &str, codec: Rc<dyn DrcExclusionCodec>) -> Self {
        let mut nested = NestedSettings::new(BDS_SCHEMA_VERSION, path);

        // We want to leave alone parameters that aren't found in the project JSON as they may
        // be initialized by the board file parser before NESTED_SETTINGS::LoadFromFile is
        // called.
        nested.settings.reset_params_if_missing = false;

        let mask_migration = Rc::new(RefCell::new(MaskMigration::default()));

        for p in build_params(codec.clone()) {
            nested.settings.add_param(p);
        }

        register_migrations(&mut nested, mask_migration.clone(), codec);

        Self {
            nested,
            bom: FieldsTableBomSettings::default(),
            mask_migration,
        }
    }

    /// Runs `f` with the caller's settings moved into the parameter target.
    fn with_target<R>(
        &mut self,
        bds: &mut BoardDesignSettings,
        f: impl FnOnce(&mut NestedSettings<BdsTarget>, &mut BdsTarget) -> R,
    ) -> R {
        let mut target = BdsTarget(Target {
            bds: std::mem::take(bds),
            bom: std::mem::take(&mut self.bom),
        });

        let r = f(&mut self.nested, &mut target);

        *bds = target.0.bds;
        self.bom = target.0.bom;
        r
    }

    /// The document (`m_internals`).
    pub fn internals(&self) -> &JsonSettingsInternals {
        &self.nested.settings.internals
    }

    /// The document, for `Set` / `At` style edits.
    pub fn internals_mut(&mut self) -> &mut JsonSettingsInternals {
        &mut self.nested.settings.internals
    }

    /// Port of `JSON_SETTINGS::Load()`: every parameter, in registration order.
    pub fn load(&mut self, bds: &mut BoardDesignSettings) {
        self.with_target(bds, |n, t| n.settings.load(t));
    }

    /// Port of `JSON_SETTINGS::Store()`: writes every parameter into the document; returns
    /// `m_modified`.
    pub fn store(&mut self, bds: &mut BoardDesignSettings) -> bool {
        self.with_target(bds, |n, t| n.settings.store(t))
    }

    /// Port of `BOARD_DESIGN_SETTINGS::LoadFromFile`: the nested load from `parent` (the
    /// project document; `None` = no parent), the migrations, then - only when the parent is
    /// a `PROJECT_FILE` (`parent_is_project_file`) - the legacy post-step. Returns the nested
    /// load's result (object found and at, or migrated to, the current schema version), or
    /// the nlohmann exception that escapes the C++ function (the post-step's
    /// `project->At( "legacy" ).erase( "pcbnew" )` when the project has no `legacy` object);
    /// the members keep the state reached at that point.
    pub fn load_from_file(
        &mut self,
        mut parent: Option<&mut JsonSettingsInternals>,
        parent_is_project_file: bool,
        bds: &mut BoardDesignSettings,
    ) -> Result<bool, JsonError> {
        *self.mask_migration.borrow_mut() = MaskMigration::default();

        let ret = self.with_target(bds, |n, t| n.load_from_parent(parent.as_deref(), t));

        // Schema 1 to 2 assigned these members while migrating (before the parameters were
        // loaded, which do not touch them).
        let mm = *self.mask_migration.borrow();

        if let Some(v) = mm.solder_mask_expansion {
            bds.solder_mask_expansion = v;
        }

        if let Some(v) = mm.solder_mask_min_width {
            bds.solder_mask_min_width = v;
        }

        if let Some(v) = mm.solder_paste_margin {
            bds.solder_paste_margin = v;
        }

        if let Some(v) = mm.solder_paste_margin_ratio {
            bds.solder_paste_margin_ratio = v;
        }

        // A number of things won't have been translated by the PROJECT_FILE migration because
        // of descoped objects required to decode this data.  So, it will be in the
        // legacy.pcbnew section and needs to be pulled out here

        let Some(project) = parent.as_deref_mut() else {
            return Ok(ret);
        };

        if !parent_is_project_file {
            return Ok(ret);
        }

        let mut migrated = false;

        let rs = "rule_severities.";
        let no_courtyard_key = "legacy_no_courtyard_defined";
        let courtyard_overlap_key = "legacy_courtyards_overlap";

        // try { ... } catch( ... ) {}
        let _ = (|| -> Result<(), JsonError> {
            let severities = project.at_mut("board.design_settings.rule_severities")?;

            if contains(severities, no_courtyard_key) {
                let flag = bool::from_json(&severities[no_courtyard_key])?;
                let key = format!(
                    "{rs}{}",
                    drc_settings_key(DRCE_MISSING_COURTYARD).unwrap_or("")
                );
                self.nested
                    .settings
                    .internals
                    .set(&key, if flag { "error" } else { "ignore" })?;

                erase(severities, no_courtyard_key)?;
                migrated = true;
            }

            if contains(severities, courtyard_overlap_key) {
                let flag = bool::from_json(&severities[courtyard_overlap_key])?;
                let key = format!(
                    "{rs}{}",
                    drc_settings_key(DRCE_OVERLAPPING_FOOTPRINTS).unwrap_or("")
                );
                self.nested
                    .settings
                    .internals
                    .set(&key, if flag { "error" } else { "ignore" })?;

                erase(severities, courtyard_overlap_key)?;
                migrated = true;
            }

            Ok(())
        })();

        if self
            .nested
            .settings
            .internals
            .contains("legacy")
            .unwrap_or(false)
        {
            // This defaults to false for new boards, but version 5.1.x and prior kept the
            // fillets so we do the same for legacy boards.
            bds.zone_keep_external_fillets = true;

            // project->At( "legacy" ).erase( "pcbnew" ): the exception this throws when the
            // project has no "legacy" object escapes LoadFromFile (skipping the reload below).
            project
                .at_mut("legacy")
                .and_then(|legacy| erase(legacy, "pcbnew"))?;
        }

        // Now that we have everything, we need to load again
        if migrated {
            self.load(bds);
        }

        Ok(ret)
    }

    /// Port of `NESTED_SETTINGS::SaveToFile`: stores the parameters and, when something
    /// changed (or with `force`), merges the document into `parent` at the nested path.
    /// Returns whether something changed.
    pub fn save_to_file(
        &mut self,
        parent: &mut JsonSettingsInternals,
        bds: &mut BoardDesignSettings,
        force: bool,
    ) -> bool {
        self.with_target(bds, |n, t| n.save_to_parent(parent, t, force))
    }
}

/// `j.contains( key )` (false for anything but an object).
fn contains(j: &Value, key: &str) -> bool {
    matches!(j, Value::Object(m) if m.contains_key(key))
}

/// `j.erase( key )` (`type_error` 307 for anything but an object).
fn erase(j: &mut Value, key: &str) -> Result<(), JsonError> {
    match j {
        Value::Object(m) => {
            m.shift_remove(key);
            Ok(())
        }
        other => Err(JsonError {
            kind: json::JsonErrorKind::TypeError,
            id: 307,
            byte: 0,
            message: format!("cannot use erase() with {}", json::type_name(other)),
        }),
    }
}

/// Port of `BOARD_DESIGN_SETTINGS::LoadFromFile` for a board bound to a project document, as
/// `BOARD::SetProject` does it (`bds.SetParent( &projectFile, !m_LegacyDesignSettingsLoaded )`).
/// Returns the settings object for a later [`BoardDesignSettingsJson::save_to_file`].
pub fn load_board_design_settings(
    project: &mut JsonSettingsInternals,
    bds: &mut BoardDesignSettings,
    codec: Rc<dyn DrcExclusionCodec>,
) -> (BoardDesignSettingsJson, Result<bool, JsonError>) {
    let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, codec);
    let ok = settings.load_from_file(Some(project), true, bds);
    (settings, ok)
}

// ------------------------------------------------------------------------------------------
// Migrations (board_design_settings.cpp:1148-1181, 1431-1521)
// ------------------------------------------------------------------------------------------

fn register_migrations(
    nested: &mut NestedSettings<BdsTarget>,
    mask_migration: Rc<RefCell<MaskMigration>>,
    codec: Rc<dyn DrcExclusionCodec>,
) {
    let m01: Migrator = Box::new(migrate_schema_0_to_1);

    let m12: Migrator = Box::new(move |internals: &mut JsonSettingsInternals| {
        // Schema 1 to 2: move mask and paste margin settings back to board.
        // The parameters are removed, so we just have to manually load them here and
        // they will get saved with the board
        let mut mm = mask_migration.borrow_mut();
        let iu = PCB_IU_SCALE.iu_per_mm;

        if let Some(v) = internals.get::<f64>("rules.solder_mask_clearance")? {
            mm.solder_mask_expansion = Some(f64_to_i32_x86(v * iu));
        }

        if let Some(v) = internals.get::<f64>("rules.solder_mask_min_width")? {
            mm.solder_mask_min_width = Some(f64_to_i32_x86(v * iu));
        }

        if let Some(v) = internals.get::<f64>("rules.solder_paste_clearance")? {
            mm.solder_paste_margin = Some(f64_to_i32_x86(v * iu));
        }

        if let Some(v) = internals.get::<f64>("rules.solder_paste_margin_ratio")? {
            mm.solder_paste_margin_ratio = Some(v);
        }

        // try { At( "rules" ).erase( ... ) x4 } catch( ... ) {}
        let _ = internals.at_mut("rules").and_then(|rules| {
            erase(rules, "solder_mask_clearance")?;
            erase(rules, "solder_mask_min_width")?;
            erase(rules, "solder_paste_clearance")?;
            erase(rules, "solder_paste_margin_ratio")
        });

        Ok(true)
    });

    let m23: Migrator = Box::new(move |internals: &mut JsonSettingsInternals| {
        migrate_schema_2_to_3(internals, &*codec)
    });

    nested.settings.register_migration(0, 1, m01);
    nested.settings.register_migration(1, 2, m12);
    nested.settings.register_migration(2, 3, m23);
}

/// Port of `BOARD_DESIGN_SETTINGS::migrateSchema0to1`.
///
/// Schema 0 to 1: default dimension precision changed in meaning.
/// Previously it was an enum with the following meaning:
///
/// 0: 0.01mm / 1 mil / 0.001 in
/// 1: 0.001mm / 0.1 mil / 0.0001 in
/// 2: 0.0001mm / 0.01 mil / 0.00001 in
///
/// Now it is independent of display units and is an integer meaning the number of digits
/// displayed after the decimal point, so we have to migrate based on the default units.
///
/// The units is an integer with the following mapping:
///
/// 0: Inches
/// 1: Mils
/// 2: Millimeters
fn migrate_schema_0_to_1(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    let units_ptr = "defaults.dimension_units";
    let precision_ptr = "defaults.dimension_precision";

    if !(internals.contains(units_ptr)?
        && internals.contains(precision_ptr)?
        && json::is_number_integer(internals.at_mut(units_ptr)?)
        && json::is_number_integer(internals.at_mut(precision_ptr)?))
    {
        // if either is missing or invalid, migration doesn't make sense
        return Ok(true);
    }

    let units = internals.get::<i32>(units_ptr)?.unwrap_or(0);
    let mut precision = internals.get::<i32>(precision_ptr)?.unwrap_or(0);

    // The enum maps directly to precision if the units is mils
    let extra_digits = match units {
        0 => 3,
        2 => 2,
        _ => 0,
    };

    precision = precision.wrapping_add(extra_digits);

    internals.set(precision_ptr, &precision)?;

    Ok(true)
}

/// Port of `BOARD_DESIGN_SETTINGS::migrateSchema2to3`.
///
/// Schema 2 to 3: convert DRC exclusions from legacy pipe-delimited strings to ProtoJSON
fn migrate_schema_2_to_3(
    internals: &mut JsonSettingsInternals,
    codec: &dyn DrcExclusionCodec,
) -> Result<bool, JsonError> {
    let Some(opt) = internals.get_json("drc_exclusions")? else {
        return Ok(true);
    };

    let mut migrated: Vec<Value> = Vec::new();

    for entry in json_elements(&opt) {
        match entry {
            Value::Object(_) => migrated.push(entry.clone()),
            Value::Array(a) if !a.is_empty() => {
                let marker_data = wx_string_from_json(&a[0])?;
                let comment = if a.len() > 1 {
                    wx_string_from_json(&a[1])?
                } else {
                    String::new()
                };

                let ex = codec.from_legacy_strings(&marker_data, &comment);

                if !ex.sort_key.is_empty() {
                    migrated.push(ex.json.clone());
                }
            }
            Value::String(_) => {
                let ex = codec.from_legacy_strings(&wx_string_from_json(entry)?, "");

                if !ex.sort_key.is_empty() {
                    migrated.push(ex.json.clone());
                }
            }
            _ => {}
        }
    }

    internals.set("drc_exclusions", &Value::Array(migrated))?;

    Ok(true)
}

// ------------------------------------------------------------------------------------------
// Parameters (board_design_settings.cpp:276-1145, in registration order)
// ------------------------------------------------------------------------------------------

type P = Box<dyn Param<BdsTarget>>;

macro_rules! fld {
    ($($m:tt)+) => {
        settings_field!(BdsTarget, 0 . $($m)+)
    };
}

/// `PARAM_SCALED<int>( path, member, mmToIU( default_mm ), min_iu, max_iu, MM_PER_IU )`.
fn scaled(
    path: &str,
    field: kicad_core::settings_parameters::Field<BdsTarget, i32>,
    default: i32,
    min: i32,
    max: i32,
) -> P {
    Box::new(ParamScaled::with_range(
        path, field, default, min, max, MM_PER_IU,
    ))
}

/// `PARAM_SCALED<long>( ... )`.
fn scaled_long(
    path: &str,
    field: kicad_core::settings_parameters::Field<BdsTarget, i64>,
    default: i32,
    min: i32,
    max: i32,
) -> P {
    Box::new(ParamScaled::with_range(
        path,
        field,
        i64::from(default),
        i64::from(min),
        i64::from(max),
        MM_PER_IU,
    ))
}

/// A `PARAM_LAMBDA<nlohmann::json>` with a fallible setter body: an `Err` is a nlohmann
/// exception that leaves the setter (assignments made so far stay).
fn json_lambda(
    path: &str,
    getter: impl Fn(&Target) -> Value + 'static,
    setter: impl Fn(&mut Target, &Value) -> Result<(), JsonError> + 'static,
) -> P {
    Box::new(ParamLambda::new(
        path,
        move |t: &BdsTarget| JsonValue(getter(&t.0)),
        move |t: &mut BdsTarget, v: JsonValue| {
            let _ = setter(&mut t.0, &v.0);
        },
        JsonValue(Value::Null),
    ))
}

fn build_params(codec: Rc<dyn DrcExclusionCodec>) -> Vec<P> {
    let mm = |v: f64| PCB_IU_SCALE.mm_to_iu(v);
    let mut p: Vec<P> = Vec::new();

    // NOTE: Previously, BOARD_DESIGN_SETTINGS stored the basic board layer information (layer
    // names and enable/disable state) in the project file even though this information is
    // also stored in the board file.  This was implemented for importing these settings from
    // another project.  Going forward, the import feature will just import from other board
    // files (since we could have multi-board projects in the future anyway) so this
    // functionality is dropped.

    p.push(Box::new(ParamT::new(
        "bom_export_filename",
        fld!(bom.bom_export_file_name),
        "${PROJECTNAME}.csv".to_string(),
    )));
    p.push(Box::new(ParamT::new(
        "bom_settings",
        fld!(bom.bom_settings),
        BomPreset::default_editing(),
    )));
    p.push(Box::new(ParamList::new(
        "bom_presets",
        fld!(bom.bom_presets),
        Vec::new(),
        false,
    )));
    p.push(Box::new(ParamT::new(
        "bom_fmt_settings",
        fld!(bom.bom_fmt_settings),
        BomFmtPreset::csv(),
    )));
    p.push(Box::new(ParamList::new(
        "bom_fmt_presets",
        fld!(bom.bom_fmt_presets),
        Vec::new(),
        false,
    )));

    p.push(Box::new(ParamT::new(
        "rules.use_height_for_length_calcs",
        fld!(bds.use_height_for_length_calcs),
        true,
    )));

    p.push(scaled(
        "rules.min_clearance",
        fld!(bds.min_clearance),
        mm(DEFAULT_MINCLEARANCE),
        mm(0.0),
        mm(25.0),
    ));
    p.push(scaled(
        "rules.min_connection",
        fld!(bds.min_conn),
        mm(DEFAULT_MINCONNECTION),
        mm(0.0),
        mm(100.0),
    ));
    p.push(scaled(
        "rules.min_track_width",
        fld!(bds.track_min_width),
        mm(DEFAULT_TRACKMINWIDTH),
        mm(0.0),
        mm(25.0),
    ));
    p.push(scaled(
        "rules.min_via_annular_width",
        fld!(bds.vias_min_annular_width),
        mm(DEFAULT_VIASMINSIZE),
        mm(0.0),
        mm(25.0),
    ));
    p.push(scaled(
        "rules.min_via_diameter",
        fld!(bds.vias_min_size),
        mm(DEFAULT_VIASMINSIZE),
        mm(0.0),
        mm(25.0),
    ));
    p.push(scaled(
        "rules.min_through_hole_diameter",
        fld!(bds.min_through_drill),
        mm(DEFAULT_MINTHROUGHDRILL),
        mm(0.0),
        mm(25.0),
    ));
    p.push(scaled(
        "rules.min_microvia_diameter",
        fld!(bds.micro_vias_min_size),
        mm(DEFAULT_MICROVIASMINSIZE),
        mm(0.0),
        mm(10.0),
    ));
    p.push(scaled(
        "rules.min_microvia_drill",
        fld!(bds.micro_vias_min_drill),
        mm(DEFAULT_MICROVIASMINDRILL),
        mm(0.0),
        mm(10.0),
    ));
    p.push(scaled(
        "rules.min_hole_to_hole",
        fld!(bds.hole_to_hole_min),
        mm(DEFAULT_HOLETOHOLEMIN),
        mm(0.0),
        mm(10.0),
    ));
    p.push(scaled(
        "rules.min_hole_clearance",
        fld!(bds.hole_clearance),
        mm(DEFAULT_HOLECLEARANCE),
        mm(0.0),
        mm(100.0),
    ));
    p.push(scaled(
        "rules.min_silk_clearance",
        fld!(bds.silk_clearance),
        mm(DEFAULT_SILKCLEARANCE),
        mm(-10.0),
        mm(100.0),
    ));
    p.push(scaled(
        "rules.min_groove_width",
        fld!(bds.min_groove_width),
        mm(DEFAULT_MINGROOVEWIDTH),
        mm(0.0),
        mm(25.0),
    ));

    // While the maximum *effective* value is 4, we've had users interpret this as the count
    // on all layers, and enter something like 10.  They'll figure it out soon enough *unless*
    // we enforce a max of 4 (and therefore reset it back to the default of 2), at which point
    // it just looks buggy.
    p.push(Box::new(ParamT::with_range(
        "rules.min_resolved_spokes",
        fld!(bds.min_resolved_spokes),
        DEFAULT_MINRESOLVEDSPOKES,
        0,
        99,
    )));

    p.push(scaled(
        "rules.min_text_height",
        fld!(bds.min_silk_text_height),
        mm(DEFAULT_SILK_TEXT_SIZE * 0.8),
        mm(0.0),
        mm(100.0),
    ));
    p.push(scaled(
        "rules.min_text_thickness",
        fld!(bds.min_silk_text_thickness),
        mm(DEFAULT_SILK_TEXT_WIDTH * 0.8),
        mm(0.0),
        mm(25.0),
    ));

    // Note: a clearance of -0.01 is a flag indicating we should use the legacy (pre-6.0)
    // method based on the edge cut thicknesses.
    p.push(scaled(
        "rules.min_copper_edge_clearance",
        fld!(bds.copper_edge_clearance),
        mm(DEFAULT_COPPEREDGECLEARANCE),
        mm(-0.01),
        mm(25.0),
    ));

    p.push(json_lambda(
        "rule_severities",
        |t| {
            let mut ret = json::Map::new();

            for &(name, code) in ITEMS_WITH_SEVERITIES {
                let Some(sev) = t.bds.drc_severities.get(&code) else {
                    continue;
                };

                if name.is_empty() {
                    continue;
                }

                ret.insert(name.to_string(), Value::from(severity_to_string(*sev)));
            }

            Value::Object(ret)
        },
        |t, a_json| {
            if !a_json.is_object() {
                return Ok(());
            }

            // Load V8 'hole_near_hole' token first (if present).  Any current 'hole_to_hole'
            // token found will then overwrite it.
            // We can't use the migration architecture because we forgot to bump the version
            // number when the change was made.  But this is a one-off as any future
            // deprecations should bump the version number and use registerMigration().
            if let Some(v) = a_json.get("hole_near_hole") {
                t.bds.drc_severities.insert(
                    DRCE_DRILLED_HOLES_TOO_CLOSE,
                    severity_from_string(&wx_string_from_json(v)?),
                );
            }

            for &(key, code) in ITEMS_WITH_SEVERITIES {
                if let Some(v) = a_json.get(key) {
                    t.bds
                        .drc_severities
                        .insert(code, severity_from_string(&wx_string_from_json(v)?));
                }
            }

            Ok(())
        },
    ));

    let codec_ex = codec.clone();

    p.push(json_lambda(
        "drc_exclusions",
        |t| {
            Value::Array(
                t.bds
                    .drc_exclusions
                    .iter()
                    .map(|e| e.json.clone())
                    .collect(),
            )
        },
        move |t, a_obj| {
            t.bds.drc_exclusions.clear();

            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            for entry in entries {
                if entry.is_object() {
                    let exclusion = codec_ex.from_json(entry);

                    if !exclusion.sort_key.is_empty() {
                        t.bds.drc_exclusions.insert(exclusion);
                    }
                }
            }

            Ok(())
        },
    ));

    p.push(json_lambda(
        "track_widths",
        |t| {
            Value::Array(
                t.bds
                    .track_width_list
                    .iter()
                    .map(|w| iu_to_mm_json(*w))
                    .collect(),
            )
        },
        |t, a_json| {
            let Value::Array(entries) = a_json else {
                return Ok(());
            };

            t.bds.track_width_list.clear();

            for entry in entries {
                if json_empty(entry) {
                    continue;
                }

                t.bds
                    .track_width_list
                    .push(mm_to_iu(f64::from_json(entry)?));
            }

            Ok(())
        },
    ));

    p.push(json_lambda(
        "via_dimensions",
        |t| {
            Value::Array(
                t.bds
                    .vias_dimensions_list
                    .iter()
                    .map(|via| {
                        let mut entry = json::Map::new();
                        entry.insert("diameter".into(), iu_to_mm_json(via.diameter));
                        entry.insert("drill".into(), iu_to_mm_json(via.drill));
                        Value::Object(entry)
                    })
                    .collect(),
            )
        },
        |t, a_obj| {
            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            t.bds.vias_dimensions_list.clear();

            for entry in entries {
                if json_empty(entry) || !entry.is_object() {
                    continue;
                }

                if !contains(entry, "diameter") || !contains(entry, "drill") {
                    continue;
                }

                let diameter = mm_to_iu(f64::from_json(&entry["diameter"])?);
                let drill = mm_to_iu(f64::from_json(&entry["drill"])?);

                t.bds
                    .vias_dimensions_list
                    .push(ViaDimension { diameter, drill });
            }

            Ok(())
        },
    ));

    p.push(json_lambda(
        "via_stack_presets",
        |t| {
            Value::Array(
                t.bds
                    .via_stack_presets
                    .iter()
                    .map(|preset| {
                        let mut entry = json::Map::new();
                        entry.insert("name".into(), Value::from(preset.name.as_str()));
                        entry.insert(
                            "start_layer".into(),
                            Value::from(Lset::name(preset.start_layer)),
                        );
                        entry.insert(
                            "end_layer".into(),
                            Value::from(Lset::name(preset.end_layer)),
                        );
                        entry.insert("staggered".into(), Value::from(preset.staggered));
                        entry.insert("via_size".into(), iu_to_mm_json(preset.via_size));
                        entry.insert("via_drill".into(), iu_to_mm_json(preset.via_drill));
                        entry.insert("use_netclass".into(), Value::from(preset.use_netclass));
                        entry.insert("filled".into(), Value::from(preset.filled));
                        entry.insert("capped".into(), Value::from(preset.capped));
                        entry.insert("pitch".into(), iu_to_mm_json(preset.pitch));
                        Value::Object(entry)
                    })
                    .collect(),
            )
        },
        |t, a_obj| {
            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            t.bds.via_stack_presets.clear();

            // An odd or non-copper layer from a hand-edited file would make every
            // LAYER_RANGE built from the preset throw or never terminate.
            let copper_layer = |entry: &Value, key: &str, fallback: PcbLayerId| -> PcbLayerId {
                let Some(Value::String(s)) = entry.get(key) else {
                    return fallback;
                };

                let name = match s.find('\0') {
                    Some(p) => &s[..p],
                    None => s.as_str(),
                };

                let layer = Lset::name_to_layer(name);

                if layer >= 0
                    && layer < PCB_LAYER_ID_COUNT
                    && (layer & 1) == 0
                    && is_copper_layer(layer)
                {
                    return to_layer_id(layer);
                }

                fallback
            };

            let bool_or = |entry: &Value, key: &str, fallback: bool| -> bool {
                match entry.get(key) {
                    Some(Value::Bool(b)) => *b,
                    _ => fallback,
                }
            };

            let mm_or = |entry: &Value, key: &str| -> i32 {
                let Some(v @ Value::Number(_)) = entry.get(key) else {
                    return 0;
                };

                let mm = f64::from_json(v).unwrap_or(0.0);

                if mm < 0.0 || mm > PCB_IU_SCALE.iu_to_mm(i32::MAX) {
                    return 0;
                }

                mm_to_iu(mm)
            };

            // Bound in mm, before the conversion to IU, because an absurd hand-edited
            // value overflows int on the way in and lands past the limit as a negative
            let bounded_mm_or = |entry: &Value, key: &str, max_mm: f64| -> i32 {
                let Some(v @ Value::Number(_)) = entry.get(key) else {
                    return 0;
                };

                let mm = f64::from_json(v).unwrap_or(0.0);
                // std::clamp( v, 0.0, max )
                mm_to_iu(if mm < 0.0 {
                    0.0
                } else if max_mm < mm {
                    max_mm
                } else {
                    mm
                })
            };

            for entry in entries {
                if json_empty(entry) || !entry.is_object() {
                    continue;
                }

                let Some(Value::String(name)) = entry.get("name") else {
                    continue;
                };

                let preset = ViaStackPreset {
                    name: match name.find('\0') {
                        Some(p) => name[..p].to_string(),
                        None => name.clone(),
                    },
                    start_layer: copper_layer(entry, "start_layer", PcbLayerId::FCu),
                    end_layer: copper_layer(entry, "end_layer", PcbLayerId::In1Cu),
                    staggered: bool_or(entry, "staggered", false),
                    via_size: mm_or(entry, "via_size"),
                    via_drill: mm_or(entry, "via_drill"),
                    use_netclass: bool_or(entry, "use_netclass", false),
                    filled: bool_or(entry, "filled", true),
                    capped: bool_or(entry, "capped", false),
                    pitch: bounded_mm_or(entry, "pitch", MAX_MICROVIA_STACK_PITCH_MM),
                };

                // Presets are referenced by name, so a duplicate would be ambiguous.
                if !t
                    .bds
                    .via_stack_presets
                    .iter()
                    .any(|other| same_no_case(&other.name, &preset.name))
                {
                    t.bds.via_stack_presets.push(preset);
                }
            }

            Ok(())
        },
    ));

    p.push(json_lambda(
        "diff_pair_dimensions",
        |t| {
            Value::Array(
                t.bds
                    .diff_pair_dimensions_list
                    .iter()
                    .map(|pair| {
                        let mut entry = json::Map::new();
                        entry.insert("width".into(), iu_to_mm_json(pair.width));
                        entry.insert("gap".into(), iu_to_mm_json(pair.gap));
                        entry.insert("via_gap".into(), iu_to_mm_json(pair.via_gap));
                        Value::Object(entry)
                    })
                    .collect(),
            )
        },
        |t, a_obj| {
            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            t.bds.diff_pair_dimensions_list.clear();

            for entry in entries {
                if json_empty(entry) || !entry.is_object() {
                    continue;
                }

                if !contains(entry, "width")
                    || !contains(entry, "gap")
                    || !contains(entry, "via_gap")
                {
                    continue;
                }

                let width = mm_to_iu(f64::from_json(&entry["width"])?);
                let gap = mm_to_iu(f64::from_json(&entry["gap"])?);
                let via_gap = mm_to_iu(f64::from_json(&entry["via_gap"])?);

                t.bds.diff_pair_dimensions_list.push(DiffPairDimension {
                    width,
                    gap,
                    via_gap,
                });
            }

            Ok(())
        },
    ));

    // Handle options for teardrops (targets and some others):
    p.push(json_lambda(
        "teardrop_options",
        |t| {
            let tdl = &t.bds.teardrop_params_list;
            let mut entry = json::Map::new();
            entry.insert("td_onvia".into(), Value::from(tdl.target_vias));
            entry.insert("td_onpthpad".into(), Value::from(tdl.target_pth_pads));
            entry.insert("td_onsmdpad".into(), Value::from(tdl.target_smd_pads));
            entry.insert("td_ontrackend".into(), Value::from(tdl.target_track2track));
            entry.insert(
                "td_onroundshapesonly".into(),
                Value::from(tdl.use_round_shapes_only),
            );
            Value::Array(vec![Value::Object(entry)])
        },
        |t, a_obj| {
            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            let tdl = &mut t.bds.teardrop_params_list;

            for entry in entries {
                if json_empty(entry) || !entry.is_object() {
                    continue;
                }

                if let Some(v) = entry.get("td_onvia") {
                    tdl.target_vias = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_onpthpad") {
                    tdl.target_pth_pads = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_onsmdpad") {
                    tdl.target_smd_pads = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_ontrackend") {
                    tdl.target_track2track = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_onroundshapesonly") {
                    tdl.use_round_shapes_only = bool::from_json(v)?;
                }

                // Legacy settings
                for target in [TargetTd::Round, TargetTd::Rect, TargetTd::Track] {
                    let td_prm = tdl.parameters_mut(target);

                    if let Some(v) = entry.get("td_allow_use_two_tracks") {
                        td_prm.allow_use_two_tracks = bool::from_json(v)?;
                    }

                    if let Some(v) = entry.get("td_curve_segcount") {
                        if i32::from_json(v)? > 0 {
                            td_prm.curved_edges = true;
                        }
                    }

                    if let Some(v) = entry.get("td_on_pad_in_zone") {
                        td_prm.td_on_pads_in_zones = bool::from_json(v)?;
                    }
                }
            }

            Ok(())
        },
    ));

    // Handle parameters (sizes, shape) for each type of teardrop:
    p.push(json_lambda(
        "teardrop_parameters",
        |t| {
            let tdl = &t.bds.teardrop_params_list;
            let mut js = Vec::new();

            for ii in 0..tdl.parameters_count() {
                let target = match ii {
                    0 => TargetTd::Round,
                    1 => TargetTd::Rect,
                    _ => TargetTd::Track,
                };
                let td_prm = tdl.parameters(target);
                let mut entry = json::Map::new();
                entry.insert(
                    "td_target_name".into(),
                    Value::from(teardrop_target_canonical_name(target)),
                );
                entry.insert("td_enabled".into(), Value::from(td_prm.enabled));
                entry.insert("td_maxlen".into(), iu_to_mm_json(td_prm.td_max_len));
                entry.insert("td_maxheight".into(), iu_to_mm_json(td_prm.td_max_width));
                entry.insert(
                    "td_length_ratio".into(),
                    json::float_value(td_prm.best_length_ratio),
                );
                entry.insert(
                    "td_height_ratio".into(),
                    json::float_value(td_prm.best_width_ratio),
                );
                entry.insert(
                    "td_curve_segcount".into(),
                    Value::from(if td_prm.curved_edges { 1 } else { 0 }),
                );
                entry.insert(
                    "td_width_to_size_filter_ratio".into(),
                    json::float_value(td_prm.width_to_size_filter_ratio),
                );
                entry.insert(
                    "td_allow_use_two_tracks".into(),
                    Value::from(td_prm.allow_use_two_tracks),
                );
                entry.insert(
                    "td_on_pad_in_zone".into(),
                    Value::from(td_prm.td_on_pads_in_zones),
                );
                js.push(Value::Object(entry));
            }

            Value::Array(js)
        },
        |t, a_obj| {
            let Value::Array(entries) = a_obj else {
                return Ok(());
            };

            for entry in entries {
                if json_empty(entry) || !entry.is_object() {
                    continue;
                }

                let Some(name) = entry.get("td_target_name") else {
                    continue;
                };

                let idx = teardrop_target_type_from_canonical_name(&String::from_json(name)?);

                if !matches!(idx, TargetTd::Round | TargetTd::Rect | TargetTd::Track) {
                    continue;
                }

                let td_prm = t.bds.teardrop_params_list.parameters_mut(idx);

                // Pads and vias carry their enable in the board file; this one has
                // nowhere else to live.
                if let Some(v) = entry.get("td_enabled") {
                    td_prm.enabled = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_maxlen") {
                    td_prm.td_max_len = mm_to_iu(f64::from_json(v)?);
                }

                if let Some(v) = entry.get("td_maxheight") {
                    td_prm.td_max_width = mm_to_iu(f64::from_json(v)?);
                }

                if let Some(v) = entry.get("td_length_ratio") {
                    td_prm.best_length_ratio = f64::from_json(v)?;
                }

                if let Some(v) = entry.get("td_height_ratio") {
                    td_prm.best_width_ratio = f64::from_json(v)?;
                }

                if let Some(v) = entry.get("td_curve_segcount") {
                    if i32::from_json(v)? > 0 {
                        td_prm.curved_edges = true;
                    }
                }

                if let Some(v) = entry.get("td_width_to_size_filter_ratio") {
                    td_prm.width_to_size_filter_ratio = f64::from_json(v)?;
                }

                if let Some(v) = entry.get("td_allow_use_two_tracks") {
                    td_prm.allow_use_two_tracks = bool::from_json(v)?;
                }

                if let Some(v) = entry.get("td_on_pad_in_zone") {
                    td_prm.td_on_pads_in_zones = bool::from_json(v)?;
                }
            }

            Ok(())
        },
    ));

    const MEANDER_KEYS: [&str; 3] = [
        "single_track_defaults",
        "diff_pair_defaults",
        "diff_pair_skew_defaults",
    ];

    p.push(json_lambda(
        "tuning_pattern_settings",
        |t| {
            // The members are kept as the objects make_settings() writes.
            let mut js = json::Map::new();

            for (i, key) in MEANDER_KEYS.iter().enumerate() {
                js.insert((*key).to_string(), t.bds.meander_settings[i].clone());
            }

            Value::Object(js)
        },
        |t, a_obj| {
            // m_SingleTrackMeanderSettings, m_DiffPairMeanderSettings, m_SkewMeanderSettings
            for (i, key) in MEANDER_KEYS.iter().enumerate() {
                if let Some(entry) = a_obj.get(key) {
                    t.bds.meander_settings[i] = MeanderSettings::read(entry)?.to_json();
                }
            }

            Ok(())
        },
    ));

    let min_text_size = mm(TEXT_MIN_SIZE_MM);
    let max_text_size = mm(TEXT_MAX_SIZE_MM);
    let min_stroke = 1;
    let max_stroke = mm(100.0);

    macro_rules! layer_class {
        ($prefix:literal, $c:expr, $line:expr, $size:expr, $width:expr) => {
            p.push(scaled(
                concat!("defaults.", $prefix, "_line_width"),
                fld!(bds.line_thickness[$c]),
                mm($line),
                min_stroke,
                max_stroke,
            ));
            p.push(scaled(
                concat!("defaults.", $prefix, "_text_size_v"),
                fld!(bds.text_size[$c].y),
                mm($size),
                min_text_size,
                max_text_size,
            ));
            p.push(scaled(
                concat!("defaults.", $prefix, "_text_size_h"),
                fld!(bds.text_size[$c].x),
                mm($size),
                min_text_size,
                max_text_size,
            ));
            p.push(scaled(
                concat!("defaults.", $prefix, "_text_thickness"),
                fld!(bds.text_thickness[$c]),
                mm($width),
                min_stroke,
                max_stroke,
            ));
            p.push(Box::new(ParamT::new(
                concat!("defaults.", $prefix, "_text_italic"),
                fld!(bds.text_italic[$c]),
                false,
            )));
            p.push(Box::new(ParamT::new(
                concat!("defaults.", $prefix, "_text_upright"),
                fld!(bds.text_upright[$c]),
                true,
            )));
        };
    }

    layer_class!(
        "silk",
        0,
        DEFAULT_SILK_LINE_WIDTH,
        DEFAULT_SILK_TEXT_SIZE,
        DEFAULT_SILK_TEXT_WIDTH
    );
    layer_class!(
        "copper",
        1,
        DEFAULT_COPPER_LINE_WIDTH,
        DEFAULT_COPPER_TEXT_SIZE,
        DEFAULT_COPPER_TEXT_WIDTH
    );

    p.push(scaled(
        "defaults.board_outline_line_width",
        fld!(bds.line_thickness[LAYER_CLASS_EDGES]),
        mm(DEFAULT_EDGE_WIDTH),
        min_stroke,
        max_stroke,
    ));
    p.push(scaled(
        "defaults.courtyard_line_width",
        fld!(bds.line_thickness[LAYER_CLASS_COURTYARD]),
        mm(DEFAULT_COURTYARD_WIDTH),
        min_stroke,
        max_stroke,
    ));

    layer_class!(
        "fab",
        4,
        DEFAULT_LINE_WIDTH,
        DEFAULT_TEXT_SIZE,
        DEFAULT_TEXT_WIDTH
    );
    layer_class!(
        "other",
        5,
        DEFAULT_LINE_WIDTH,
        DEFAULT_TEXT_SIZE,
        DEFAULT_TEXT_WIDTH
    );

    p.push(Box::new(ParamEnum::new(
        "defaults.dimension_units",
        fld!(bds.dimension_units_mode),
        DimUnitsMode::Automatic,
        DimUnitsMode::Inch,
        DimUnitsMode::Automatic,
        |e| e as i32,
        dim_units_mode_from_int,
    )));

    p.push(Box::new(ParamEnum::new(
        "defaults.dimension_precision",
        fld!(bds.dimension_precision),
        DimPrecision::X_XXXX,
        DimPrecision::X,
        DimPrecision::V_VVVVV,
        |e| e as i32,
        dim_precision_from_int,
    )));

    p.push(Box::new(ParamEnum::new(
        "defaults.dimensions.units_format",
        fld!(bds.dimension_units_format),
        DimUnitsFormat::NoSuffix,
        DimUnitsFormat::NoSuffix,
        DimUnitsFormat::ParenSuffix,
        |e| e as i32,
        |v| match v {
            1 => DimUnitsFormat::BareSuffix,
            2 => DimUnitsFormat::ParenSuffix,
            _ => DimUnitsFormat::NoSuffix,
        },
    )));

    p.push(Box::new(ParamT::new(
        "defaults.dimensions.suppress_zeroes",
        fld!(bds.dimension_suppress_zeroes),
        true,
    )));

    // NOTE: excluding DIM_TEXT_POSITION::MANUAL from the valid range here
    p.push(Box::new(ParamEnum::new(
        "defaults.dimensions.text_position",
        fld!(bds.dimension_text_position),
        DimTextPosition::Outside,
        DimTextPosition::Outside,
        DimTextPosition::Inline,
        |e| e as i32,
        |v| match v {
            1 => DimTextPosition::Inline,
            2 => DimTextPosition::Manual,
            _ => DimTextPosition::Outside,
        },
    )));

    p.push(Box::new(ParamT::new(
        "defaults.dimensions.keep_text_aligned",
        fld!(bds.dimension_keep_text_aligned),
        true,
    )));

    p.push(Box::new(ParamT::new(
        "defaults.dimensions.arrow_length",
        fld!(bds.dimension_arrow_length),
        PCB_IU_SCALE.mils_to_iu(DEFAULT_DIMENSION_ARROW_LENGTH),
    )));

    p.push(Box::new(ParamT::new(
        "defaults.dimensions.extension_offset",
        fld!(bds.dimension_extension_offset),
        mm(DEFAULT_DIMENSION_EXTENSION_OFFSET),
    )));

    p.push(Box::new(ParamT::new(
        "defaults.apply_defaults_to_fp_fields",
        fld!(bds.style_fp_fields),
        false,
    )));
    p.push(Box::new(ParamT::new(
        "defaults.apply_defaults_to_fp_text",
        fld!(bds.style_fp_text),
        false,
    )));
    p.push(Box::new(ParamT::new(
        "defaults.apply_defaults_to_fp_shapes",
        fld!(bds.style_fp_shapes),
        false,
    )));
    p.push(Box::new(ParamT::new(
        "defaults.apply_defaults_to_fp_dimensions",
        fld!(bds.style_fp_dimensions),
        false,
    )));
    p.push(Box::new(ParamT::new(
        "defaults.apply_defaults_to_fp_barcodes",
        fld!(bds.style_fp_barcodes),
        false,
    )));

    p.push(scaled(
        "defaults.zones.min_clearance",
        fld!(bds.default_zone_settings.zone_clearance),
        mm(ZONE_CLEARANCE_MM),
        mm(0.0),
        mm(25.0),
    ));

    p.push(scaled(
        "defaults.zones.min_thickness",
        fld!(bds.default_zone_settings.zone_min_thickness),
        mm(ZONE_THICKNESS_MM),
        mm(ZONE_THICKNESS_MIN_VALUE_MM),
        mm(25.0),
    ));

    p.push(Box::new(ParamEnum::new(
        "defaults.zones.fill_mode",
        fld!(bds.default_zone_settings.fill_mode),
        ZoneFillMode::Polygons,
        ZoneFillMode::Polygons,
        ZoneFillMode::CopperThieving,
        |e| e as i32,
        |v| match v {
            1 => ZoneFillMode::HatchPattern,
            2 => ZoneFillMode::CopperThieving,
            _ => ZoneFillMode::Polygons,
        },
    )));

    p.push(scaled(
        "defaults.zones.hatch_thickness",
        fld!(bds.default_zone_settings.hatch_thickness),
        (mm(ZONE_THICKNESS_MM) * 4).max(mm(1.0)),
        mm(0.0),
        mm(25.0),
    ));

    p.push(scaled(
        "defaults.zones.hatch_gap",
        fld!(bds.default_zone_settings.hatch_gap),
        (mm(ZONE_THICKNESS_MM) * 6).max(mm(1.5)),
        mm(0.0),
        mm(25.0),
    ));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.hatch_orientation",
        |t: &BdsTarget| t.0.bds.default_zone_settings.hatch_orientation.as_degrees(),
        |t: &mut BdsTarget, val: f64| {
            t.0.bds.default_zone_settings.hatch_orientation =
                EdaAngle::new(val, EdaAngleT::Degrees);
        },
        0.0,
    )));

    p.push(Box::new(ParamT::with_range(
        "defaults.zones.hatch_smoothing_level",
        fld!(bds.default_zone_settings.hatch_smoothing_level),
        0,
        0,
        3,
    )));

    p.push(Box::new(ParamT::with_range(
        "defaults.zones.hatch_smoothing_value",
        fld!(bds.default_zone_settings.hatch_smoothing_value),
        0.1,
        0.0,
        1.0,
    )));

    p.push(Box::new(ParamEnum::new(
        "defaults.zones.border_display_style",
        fld!(bds.default_zone_settings.zone_border_display_style),
        ZoneBorderDisplayStyle::DiagonalEdge,
        ZoneBorderDisplayStyle::NoHatch,
        ZoneBorderDisplayStyle::InvisibleBorder,
        |e| e as i32,
        |v| match v {
            1 => ZoneBorderDisplayStyle::DiagonalFull,
            2 => ZoneBorderDisplayStyle::DiagonalEdge,
            3 => ZoneBorderDisplayStyle::InvisibleBorder,
            _ => ZoneBorderDisplayStyle::NoHatch,
        },
    )));

    p.push(scaled(
        "defaults.zones.border_hatch_pitch",
        fld!(bds.default_zone_settings.border_hatch_pitch),
        mm(ZONE_BORDER_HATCH_DIST_MM),
        mm(ZONE_BORDER_HATCH_MINDIST_MM),
        mm(ZONE_BORDER_HATCH_MAXDIST_MM),
    ));

    p.push(scaled_long(
        "defaults.zones.thermal_relief_gap",
        fld!(bds.default_zone_settings.thermal_relief_gap),
        mm(ZONE_THERMAL_RELIEF_GAP_MM),
        mm(0.0),
        mm(25.0),
    ));

    p.push(scaled_long(
        "defaults.zones.thermal_relief_spoke_width",
        fld!(bds.default_zone_settings.thermal_relief_spoke_width),
        mm(ZONE_THERMAL_RELIEF_COPPER_WIDTH_MM),
        mm(0.0),
        mm(25.0),
    ));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.pad_connection",
        |t: &BdsTarget| t.0.bds.default_zone_settings.pad_connection as i32,
        |t: &mut BdsTarget, val: i32| {
            // static_cast<ZONE_CONNECTION>( aVal ): values without an enumerator are not
            // representable (deviation SET-4-2: the member is left unchanged)
            if let Some(c) = zone_connection_from_int(val) {
                t.0.bds.default_zone_settings.pad_connection = c;
            }
        },
        ZoneConnection::Thermal as i32,
    )));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.corner_smoothing",
        |t: &BdsTarget| t.0.bds.default_zone_settings.corner_smoothing_type as i32,
        |t: &mut BdsTarget, val: i32| {
            // magic_enum::enum_cast<CORNER_SMOOTHING>( aVal ).value_or( NO_SMOOTHING )
            t.0.bds.default_zone_settings.corner_smoothing_type = match val {
                1 => CornerSmoothing::Chamfer,
                2 => CornerSmoothing::Fillet,
                _ => CornerSmoothing::NoSmoothing,
            };
        },
        CornerSmoothing::NoSmoothing as i32,
    )));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.corner_radius",
        |t: &BdsTarget| PCB_IU_SCALE.iu_to_mm(t.0.bds.default_zone_settings.corner_radius as i32),
        |t: &mut BdsTarget, val: f64| {
            t.0.bds
                .default_zone_settings
                .set_corner_radius(mm_to_iu(val));
        },
        0.0,
    )));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.remove_islands",
        |t: &BdsTarget| t.0.bds.default_zone_settings.remove_islands as i32,
        |t: &mut BdsTarget, val: i32| {
            // static_cast<ISLAND_REMOVAL_MODE>( aVal ): see pad_connection (deviation SET-4-2)
            t.0.bds.default_zone_settings.remove_islands = match val {
                0 => IslandRemovalMode::Always,
                1 => IslandRemovalMode::Never,
                2 => IslandRemovalMode::Area,
                _ => return,
            };
        },
        IslandRemovalMode::Always as i32,
    )));

    p.push(Box::new(ParamLambda::new(
        "defaults.zones.min_island_area",
        |t: &BdsTarget| {
            let iu_per_mm2 = PCB_IU_SCALE.iu_per_mm * PCB_IU_SCALE.iu_per_mm;
            t.0.bds.default_zone_settings.min_island_area as f64 / iu_per_mm2
        },
        |t: &mut BdsTarget, val: f64| {
            let iu_per_mm2 = PCB_IU_SCALE.iu_per_mm * PCB_IU_SCALE.iu_per_mm;
            t.0.bds.default_zone_settings.min_island_area = f64_to_i64_x86(val * iu_per_mm2);
        },
        10.0,
    )));

    p.push(scaled(
        "rules.max_error",
        fld!(bds.max_error),
        ARC_HIGH_DEF,
        mm(0.0001),
        mm(1.0),
    ));

    p.push(scaled(
        "rules.solder_mask_to_copper_clearance",
        fld!(bds.solder_mask_to_copper_clearance),
        mm(DEFAULT_SOLDERMASK_TO_COPPER_CLEARANCE),
        mm(0.0),
        mm(25.0),
    ));

    p.push(Box::new(ParamT::new(
        "zones_allow_external_fillets",
        fld!(bds.zone_keep_external_fillets),
        false,
    )));

    p
}

/// `static_cast<DIM_UNITS_MODE>( v )` for an in-range value.
fn dim_units_mode_from_int(v: i32) -> DimUnitsMode {
    match v {
        0 => DimUnitsMode::Inch,
        1 => DimUnitsMode::Mils,
        2 => DimUnitsMode::Mm,
        _ => DimUnitsMode::Automatic,
    }
}

/// `static_cast<DIM_PRECISION>( v )` for an in-range value.
fn dim_precision_from_int(v: i32) -> DimPrecision {
    match v {
        0 => DimPrecision::X,
        1 => DimPrecision::X_X,
        2 => DimPrecision::X_XX,
        3 => DimPrecision::X_XXX,
        4 => DimPrecision::X_XXXX,
        5 => DimPrecision::X_XXXXX,
        6 => DimPrecision::V_VV,
        7 => DimPrecision::V_VVV,
        8 => DimPrecision::V_VVVV,
        _ => DimPrecision::V_VVVVV,
    }
}

/// `static_cast<ZONE_CONNECTION>( v )` where it names an enumerator.
fn zone_connection_from_int(v: i32) -> Option<ZoneConnection> {
    Some(match v {
        -1 => ZoneConnection::Inherited,
        0 => ZoneConnection::None,
        1 => ZoneConnection::Thermal,
        2 => ZoneConnection::Full,
        3 => ZoneConnection::ThtThermal,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    //! Differential replay of `tests/oracle/bds_json_oracle.cpp` (goldens
    //! `tests/golden/bds_json_scenarios.txt` and `bds_json_corpus.txt`) and the BDS cases of
    //! `qa/tests/pcbnew/test_board_design_settings.cpp`.

    use super::*;
    use std::path::PathBuf;

    fn crate_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn raw_codec() -> Rc<dyn DrcExclusionCodec> {
        Rc::new(RawDrcExclusionCodec)
    }

    /// The oracle's `D()`: `nlohmann::json( double ).dump()`.
    fn d(v: f64) -> String {
        json::dump_float(v)
    }

    /// The oracle's `S()`: a JSON string literal.
    fn s(v: &str) -> String {
        json::dump(&Value::from(v), None, ObjectOrder::Sorted)
    }

    fn b(v: bool) -> String {
        if v { "1" } else { "0" }.to_string()
    }

    /// A meander member recovered from the stored `make_settings` object.
    fn meander_line(v: &Value) -> String {
        let iu = |k: &str| {
            let mm = f64::from_json(&v[k]).unwrap_or(0.0);
            (mm * 1e6).round() as i64
        };

        format!(
            "{}|{}|{}|{}|{}|{}",
            iu("min_amplitude"),
            iu("max_amplitude"),
            iu("spacing"),
            b(json::json_eq(&v["corner_style"], &Value::from(0))),
            i32::from_json(&v["corner_radius_percentage"]).unwrap_or(0),
            b(v["single_sided"] == Value::Bool(true))
        )
    }

    /// The oracle's `printBds`.
    fn print_bds(
        bds: &BoardDesignSettings,
        bom: &FieldsTableBomSettings,
        main: &mut Vec<String>,
        excl: &mut Vec<String>,
    ) {
        let mut l = |k: &str, v: String| main.push(format!("{k}={v}"));

        l(
            "use_height_for_length_calcs",
            b(bds.use_height_for_length_calcs),
        );

        for (k, v) in [
            ("min_clearance", bds.min_clearance),
            ("min_conn", bds.min_conn),
            ("track_min_width", bds.track_min_width),
            ("vias_min_annular_width", bds.vias_min_annular_width),
            ("vias_min_size", bds.vias_min_size),
            ("min_through_drill", bds.min_through_drill),
            ("micro_vias_min_size", bds.micro_vias_min_size),
            ("micro_vias_min_drill", bds.micro_vias_min_drill),
            ("hole_to_hole_min", bds.hole_to_hole_min),
            ("hole_clearance", bds.hole_clearance),
            ("silk_clearance", bds.silk_clearance),
            ("min_groove_width", bds.min_groove_width),
            ("min_resolved_spokes", bds.min_resolved_spokes),
            ("min_silk_text_height", bds.min_silk_text_height),
            ("min_silk_text_thickness", bds.min_silk_text_thickness),
            ("copper_edge_clearance", bds.copper_edge_clearance),
        ] {
            l(k, v.to_string());
        }

        for (code, sev) in &bds.drc_severities {
            l(
                &format!("severity.{code}"),
                severity_to_string(*sev).to_string(),
            );
        }

        excl.push(format!("exclusions={}", bds.drc_exclusions.len()));

        for ex in &bds.drc_exclusions {
            excl.push(format!(
                "exclusion={} {}",
                json::dump(&ex.json, None, ObjectOrder::Sorted),
                s(&ex.comment)
            ));
        }

        l(
            "track_widths",
            bds.track_width_list
                .iter()
                .map(|w| format!("{w},"))
                .collect(),
        );
        l(
            "via_dimensions",
            bds.vias_dimensions_list
                .iter()
                .map(|v| format!("{}/{},", v.diameter, v.drill))
                .collect(),
        );
        l(
            "diff_pairs",
            bds.diff_pair_dimensions_list
                .iter()
                .map(|v| format!("{}/{}/{},", v.width, v.gap, v.via_gap))
                .collect(),
        );

        for (i, v) in bds.via_stack_presets.iter().enumerate() {
            l(
                &format!("via_stack_preset.{i}"),
                format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
                    s(&v.name),
                    v.start_layer.0,
                    v.end_layer.0,
                    b(v.staggered),
                    v.via_size,
                    v.via_drill,
                    b(v.use_netclass),
                    b(v.filled),
                    b(v.capped),
                    v.pitch
                ),
            );
        }

        let tdl = &bds.teardrop_params_list;
        l(
            "teardrop.targets",
            [
                tdl.target_vias,
                tdl.target_pth_pads,
                tdl.target_smd_pads,
                tdl.target_track2track,
                tdl.use_round_shapes_only,
            ]
            .map(b)
            .concat(),
        );

        for (i, t) in [TargetTd::Round, TargetTd::Rect, TargetTd::Track]
            .into_iter()
            .enumerate()
        {
            let t = tdl.parameters(t);
            l(
                &format!("teardrop.{i}"),
                format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}|{}",
                    t.td_max_len,
                    t.td_max_width,
                    d(t.best_length_ratio),
                    d(t.best_width_ratio),
                    d(t.width_to_size_filter_ratio),
                    b(t.curved_edges),
                    b(t.enabled),
                    b(t.allow_use_two_tracks),
                    b(t.td_on_pads_in_zones)
                ),
            );
        }

        for i in 0..3 {
            l(
                &format!("meander.{i}"),
                meander_line(&bds.meander_settings[i]),
            );
        }

        for c in 0..LAYER_CLASS_COUNT {
            l(
                &format!("layer_class.{c}"),
                format!(
                    "{}|{}|{}|{}|{}|{}",
                    bds.line_thickness[c],
                    bds.text_size[c].x,
                    bds.text_size[c].y,
                    bds.text_thickness[c],
                    b(bds.text_italic[c]),
                    b(bds.text_upright[c])
                ),
            );
        }

        l(
            "dimension",
            format!(
                "{}|{}|{}|{}|{}|{}|{}|{}",
                bds.dimension_units_mode as i32,
                bds.dimension_precision as i32,
                bds.dimension_units_format as i32,
                b(bds.dimension_suppress_zeroes),
                bds.dimension_text_position as i32,
                b(bds.dimension_keep_text_aligned),
                bds.dimension_arrow_length,
                bds.dimension_extension_offset
            ),
        );
        l(
            "style_fp",
            [
                bds.style_fp_fields,
                bds.style_fp_text,
                bds.style_fp_shapes,
                bds.style_fp_dimensions,
                bds.style_fp_barcodes,
            ]
            .map(b)
            .concat(),
        );

        let z = &bds.default_zone_settings;
        l("zone.clearance", z.zone_clearance.to_string());
        l("zone.min_thickness", z.zone_min_thickness.to_string());
        l("zone.fill_mode", (z.fill_mode as i32).to_string());
        l(
            "zone.hatch",
            format!(
                "{}|{}|{}|{}|{}",
                z.hatch_thickness,
                z.hatch_gap,
                d(z.hatch_orientation.as_degrees()),
                z.hatch_smoothing_level,
                d(z.hatch_smoothing_value)
            ),
        );
        l(
            "zone.border",
            format!(
                "{}|{}",
                z.zone_border_display_style as i32, z.border_hatch_pitch
            ),
        );
        l(
            "zone.thermal",
            format!("{}|{}", z.thermal_relief_gap, z.thermal_relief_spoke_width),
        );
        l("zone.pad_connection", (z.pad_connection as i32).to_string());
        l(
            "zone.corner",
            format!("{}|{}", z.corner_smoothing_type as i32, z.corner_radius),
        );
        l(
            "zone.islands",
            format!("{}|{}", z.remove_islands as i32, z.min_island_area),
        );

        l("max_error", bds.max_error.to_string());
        l(
            "solder_mask_to_copper_clearance",
            bds.solder_mask_to_copper_clearance.to_string(),
        );
        l(
            "zone_keep_external_fillets",
            b(bds.zone_keep_external_fillets),
        );
        l(
            "solder_mask_expansion",
            bds.solder_mask_expansion.to_string(),
        );
        l(
            "solder_mask_min_width",
            bds.solder_mask_min_width.to_string(),
        );
        l("solder_paste_margin", bds.solder_paste_margin.to_string());
        l(
            "solder_paste_margin_ratio",
            d(bds.solder_paste_margin_ratio),
        );

        let dump = |v: Value| json::dump(&v, None, ObjectOrder::Sorted);
        l("bom.export_file_name", s(&bom.bom_export_file_name));
        l(
            "bom.settings",
            format!(
                "{}|{}",
                dump(bom.bom_settings.to_json()),
                b(bom.bom_settings.read_only)
            ),
        );
        l(
            "bom.presets",
            dump(Value::Array(
                bom.bom_presets.iter().map(ToJson::to_json).collect(),
            )),
        );
        l(
            "bom.fmt_settings",
            format!(
                "{}|{}",
                dump(bom.bom_fmt_settings.to_json()),
                b(bom.bom_fmt_settings.read_only)
            ),
        );
        l(
            "bom.fmt_presets",
            dump(Value::Array(
                bom.bom_fmt_presets.iter().map(ToJson::to_json).collect(),
            )),
        );
    }

    /// The oracle's `runFile` on a project text: `(main lines, exclusion lines)`, `None` when
    /// the project does not parse (`error|load`).
    fn run_project(text: &[u8]) -> Option<(Vec<String>, Vec<String>)> {
        let doc = json::parse_bytes(text, ObjectOrder::Sorted, true).ok()?;
        let mut parent = JsonSettingsInternals {
            original: doc.clone(),
            doc,
        };

        let mut main = Vec::new();
        let mut excl = Vec::new();
        let mut bds = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());

        let load = settings.load_from_file(Some(&mut parent), true, &mut bds);
        main.push(format!(
            "load={}",
            match load {
                Ok(v) => b(v),
                Err(_) => "exception".to_string(),
            }
        ));

        print_bds(&bds, &settings.bom, &mut main, &mut excl);

        main.push(format!(
            "saved={}",
            b(settings.save_to_file(&mut parent, &mut bds, false))
        ));

        // PROJECT_FILE::SerializeToString: flushToStore (the nested save again), then
        // formatFileContents (original merged with the document, for this subtree).
        settings.save_to_file(&mut parent, &mut bds, false);

        let ptr = JsonSettingsInternals::pointer_from_string(BDS_JSON_PATH);
        let original = if ptr.contains(&parent.original).unwrap_or(false) {
            ptr.at(&parent.original).ok().cloned()
        } else {
            None
        };
        let current = if ptr.contains(&parent.doc).unwrap_or(false) {
            ptr.at(&parent.doc).ok().cloned()
        } else {
            None
        };

        // update( ..., merge_objects ) one level up: a non-object subtree is assigned.
        let wrap = |v: Value| {
            let mut m = json::Map::new();
            m.insert("ds".into(), v);
            Value::Object(m)
        };
        let stored = match (original, current) {
            (_, None) => Ok(None),
            (None, Some(c)) => Ok(Some(c)),
            (Some(o), Some(c)) => {
                let mut o = wrap(o);
                json::update(&mut o, &wrap(c), true).map(|_| o.get("ds").cloned())
            }
        };

        match stored {
            Err(_) => main.push("stored=error".to_string()),
            Ok(None) => main.push("stored=none".to_string()),
            Ok(Some(mut ds)) => {
                if let Value::Object(m) = &mut ds {
                    if let Some(ex) = m.shift_remove("drc_exclusions") {
                        excl.push(format!(
                            "stored.drc_exclusions={}",
                            json::dump(&ex, Some(2), ObjectOrder::Sorted)
                        ));
                    }
                }

                main.push(format!(
                    "stored={}",
                    json::dump(&ds, Some(2), ObjectOrder::Sorted)
                ));
            }
        }

        Some((main, excl))
    }

    fn fnv(lines: &[String]) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;

        for l in lines {
            for c in l.bytes().chain(std::iter::once(b'\n')) {
                h ^= u64::from(c);
                h = h.wrapping_mul(0x100000001b3);
            }
        }

        h
    }

    /// Splits a golden into `(label, lines)` blocks.
    fn blocks(text: &str) -> Vec<(String, Vec<String>)> {
        let mut out: Vec<(String, Vec<String>)> = Vec::new();
        let mut cur: Option<(String, Vec<String>)> = None;

        // Multi-line values (the stored dump) continue until the next "key=" line; the oracle
        // only emits dump(2) as such a value, so join lines that do not look like a key.
        for line in text.lines() {
            if let Some(label) = line.strip_prefix("file|") {
                if let Some(c) = cur.take() {
                    out.push(c);
                }

                cur = Some((label.to_string(), Vec::new()));
            } else if let Some((_, lines)) = cur.as_mut() {
                lines.push(line.to_string());
            }
        }

        if let Some(c) = cur.take() {
            out.push(c);
        }

        out
    }

    /// The scenario files of `tests/data/bds_json`, line by line against the C++ output.
    #[test]
    fn oracle_scenarios() {
        let golden =
            std::fs::read_to_string(crate_dir().join("tests/golden/bds_json_scenarios.txt"))
                .expect("golden");
        let mut checked = 0;

        for (label, expected) in blocks(&golden) {
            let name = label.strip_prefix("scenario/").expect("scenario label");
            let text = std::fs::read(crate_dir().join("tests/data/bds_json").join(name))
                .expect("scenario file");
            let (main, excl) = run_project(&text).expect("scenario parses");
            let actual: Vec<String> = main
                .iter()
                .chain(excl.iter())
                .flat_map(|l| l.lines().map(str::to_string).collect::<Vec<_>>())
                .collect();

            // Deviation SET-4-2: an island removal mode without an enumerator is not
            // representable; the member keeps its value.
            const KNOWN_DEVIATIONS: &[(&str, &str, &str)] = &[(
                "s21_all_lambdas_null.kicad_pro",
                "zone.islands=9|-9223372036854775808",
                "zone.islands=0|-9223372036854775808",
            )];

            let expected: Vec<String> = expected
                .into_iter()
                .map(|l| {
                    KNOWN_DEVIATIONS
                        .iter()
                        .find(|(n, cxx, _)| *n == name && *cxx == l)
                        .map_or(l, |(_, _, rust)| (*rust).to_string())
                })
                .collect();

            assert_eq!(actual, expected, "scenario {name}");
            checked += 1;
        }

        assert_eq!(checked, 21);
    }

    /// Every `.kicad_pro` of `qa/data` and `demos`, by hash. Projects whose exclusions need the
    /// DRC codec (`kicad-drc`) are compared without the exclusion lines.
    #[test]
    fn oracle_corpus() {
        let golden = std::fs::read_to_string(crate_dir().join("tests/golden/bds_json_corpus.txt"))
            .expect("golden");
        let root = crate_dir().join("../../..");
        let mut checked = 0;
        let mut excl_skipped = 0;
        let mut failures = Vec::new();

        for (label, lines) in blocks(&golden) {
            let Some(hash) = lines.first().and_then(|l| l.strip_prefix("hash|")) else {
                // error|load: the C++ project loader rejected the file.
                continue;
            };

            let parts: Vec<&str> = hash.split('|').collect();
            let text = std::fs::read(root.join(&label)).expect("corpus file");
            let Some((main, excl)) = run_project(&text) else {
                failures.push(format!("{label}: does not parse"));
                continue;
            };

            let count = main.len() + excl.len();
            let split = |v: &[String]| -> Vec<String> {
                v.iter()
                    .flat_map(|l| l.lines().map(str::to_string).collect::<Vec<_>>())
                    .collect()
            };

            if format!("{:016x}", fnv(&split(&main))) != parts[0] {
                failures.push(format!("{label}: main lines differ"));
            }

            let has_exclusions = matches!(
                json::parse_bytes(&text, ObjectOrder::Sorted, true)
                    .ok()
                    .as_ref()
                    .and_then(|d| d.pointer("/board/design_settings/drc_exclusions")),
                Some(Value::Array(a)) if !a.is_empty()
            );

            if has_exclusions {
                excl_skipped += 1;
            } else if format!("{:016x}", fnv(&split(&excl))) != parts[1]
                || count.to_string() != parts[2]
            {
                failures.push(format!("{label}: exclusion lines differ"));
            }

            checked += 1;
        }

        assert!(
            failures.is_empty(),
            "{} mismatches:\n{}",
            failures.len(),
            failures.join("\n")
        );
        assert_eq!(checked, 384);
        assert_eq!(excl_skipped, 6);
    }

    /// Diagnostic for the `ds.*` comparison with the C++ `pcb-dump board --sections board`
    /// (QA load path, which binds the sibling project): for each board listed in the file
    /// `$SET4_BOARDS` (one path per line) parse it, bind its `.kicad_pro` with
    /// [`load_board_design_settings`] unless the board loaded legacy design settings
    /// (`BOARD::SetProject` skips the load then), and write the `ds.*` lines to `$SET4_OUT`.
    /// Run with `--features dump -- --ignored`; compared by `tests/oracle/bds_json_ds_compare.py`.
    #[cfg(feature = "dump")]
    #[test]
    #[ignore]
    fn pcb_dump_ds_lines() {
        use std::fmt::Write as _;

        let (Ok(list), Ok(out_path)) = (std::env::var("SET4_BOARDS"), std::env::var("SET4_OUT"))
        else {
            return;
        };

        let mut out = String::new();

        for path in std::fs::read_to_string(list).expect("list").lines() {
            let path = std::path::Path::new(path);
            let text = String::from_utf8_lossy(&std::fs::read(path).expect("board")).to_string();
            let options = crate::io::ParseOptions {
                allow_pending: true,
            };
            let _ = writeln!(out, "file|{}", path.display());

            let Ok(mut board) = crate::io::parse_board(&text, "", &options) else {
                let _ = writeln!(out, "error|parse");
                continue;
            };

            let _ = writeln!(
                out,
                "meta|version={} legacy={}",
                board.file_format_version_at_load, board.legacy_design_settings_loaded
            );

            if !board.legacy_design_settings_loaded {
                let pro = path.with_extension("kicad_pro");
                let doc = std::fs::read(&pro)
                    .ok()
                    .and_then(|t| json::parse_bytes(&t, ObjectOrder::Sorted, true).ok());

                if let Some(doc) = doc {
                    let mut parent = JsonSettingsInternals {
                        original: doc.clone(),
                        doc,
                    };
                    let _ = load_board_design_settings(
                        &mut parent,
                        board.design_settings_mut(),
                        raw_codec(),
                    );
                }
            }

            let mut o = crate::dump::Out::new(false);
            o.scope("board", "board", |o| {
                crate::dump::settings::dump_design_settings(o, "ds.", board.design_settings());
            });
            out.push_str(&o.text);
        }

        std::fs::write(out_path, out).expect("write");
    }

    // ---- qa/tests/pcbnew/test_board_design_settings.cpp ---------------------------------

    /// `NegativeSilkClearanceRoundTrip` (https://gitlab.com/kicad/code/kicad/-/issues/23327).
    #[test]
    fn negative_silk_clearance_round_trip() {
        let mut bds = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());

        let negative_value = PCB_IU_SCALE.mm_to_iu(-0.1);
        bds.silk_clearance = negative_value;

        // Store all params into the JSON backing store
        settings.store(&mut bds);

        // Reset to default
        bds.silk_clearance = 0;

        // Load back from JSON
        settings.load(&mut bds);

        assert_eq!(bds.silk_clearance, negative_value);
    }

    /// `SeededBoardValueSurvivesRoundTrip`: a non-default value absent from the project JSON
    /// must not be reset on load and must survive a save/reload round-trip through the parent.
    #[test]
    fn seeded_board_value_survives_round_trip() {
        let mut parent = JsonSettingsInternals::default();
        parent
            .set(
                "board.design_settings",
                &json::parse(r#"{"meta":{"version":2}}"#, ObjectOrder::Sorted).unwrap(),
            )
            .unwrap();

        let negative_value = PCB_IU_SCALE.mm_to_iu(-0.1);

        {
            let mut bds = BoardDesignSettings::default();
            let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());

            bds.silk_clearance = negative_value;
            settings
                .load_from_file(Some(&mut parent), false, &mut bds)
                .unwrap();
            assert_eq!(bds.silk_clearance, negative_value);

            settings.save_to_file(&mut parent, &mut bds, false);
        }

        let mut reloaded = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());
        settings
            .load_from_file(Some(&mut parent), false, &mut reloaded)
            .unwrap();
        assert_eq!(reloaded.silk_clearance, negative_value);
    }

    /// The three-preset document of `OneMalformedPresetDoesNotDropTheRest` (integer layer ids,
    /// which are not strings and fall back to `F_Cu` / `In1_Cu`).
    fn presets_json(entries: Vec<Value>) -> Value {
        Value::Array(entries)
    }

    /// `OneMalformedPresetDoesNotDropTheRest`: a hand-edited preset holding the wrong type
    /// must not take the presets after it down with it.
    #[test]
    fn one_malformed_preset_does_not_drop_the_rest() {
        let mut bds = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());

        let mut js = Vec::new();

        for i in 0..3 {
            let mut entry = json::Map::new();
            entry.insert("name".into(), Value::from(format!("preset{i}")));
            entry.insert("start_layer".into(), Value::from(PcbLayerId::FCu.0));
            entry.insert("end_layer".into(), Value::from(PcbLayerId::In1Cu.0));

            // The middle one carries its size as a string, the way a hand edit would leave it.
            entry.insert(
                "via_size".into(),
                if i == 1 {
                    Value::from("0.25")
                } else {
                    json::float_value(0.25)
                },
            );
            js.push(Value::Object(entry));
        }

        settings
            .internals_mut()
            .set("via_stack_presets", &presets_json(js))
            .unwrap();
        bds.via_stack_presets.clear();
        settings.load(&mut bds);

        assert_eq!(bds.via_stack_presets.len(), 3);
        assert_eq!(bds.via_stack_presets[2].name, "preset2");

        // The unreadable size falls back rather than carrying a garbage value.
        assert_eq!(bds.via_stack_presets[1].via_size, 0);
        assert_eq!(
            bds.via_stack_presets[2].via_size,
            PCB_IU_SCALE.mm_to_iu(0.25)
        );
    }

    /// `AnAbsurdPresetPitchIsClampedOnLoad`: a hand-edited pitch has to be bounded, or the
    /// hop position arithmetic overflows.
    #[test]
    fn an_absurd_preset_pitch_is_clamped_on_load() {
        let mut bds = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());

        let mut entry = json::Map::new();
        entry.insert("name".into(), Value::from("silly"));
        entry.insert("start_layer".into(), Value::from(PcbLayerId::FCu.0));
        entry.insert("end_layer".into(), Value::from(PcbLayerId::In1Cu.0));
        entry.insert("staggered".into(), Value::from(true));
        entry.insert("pitch".into(), json::float_value(100000.0));

        settings
            .internals_mut()
            .set(
                "via_stack_presets",
                &presets_json(vec![Value::Object(entry)]),
            )
            .unwrap();
        bds.via_stack_presets.clear();
        settings.load(&mut bds);

        assert_eq!(bds.via_stack_presets.len(), 1);
        assert_eq!(
            bds.via_stack_presets[0].pitch,
            PCB_IU_SCALE.mm_to_iu(MAX_MICROVIA_STACK_PITCH_MM)
        );
    }

    /// The BDS part of `qa/tests/common/test_project_file.cpp` `NoRewriteWhenUnchanged`: a
    /// project written by the store is not modified by a second load / save cycle.
    #[test]
    fn no_rewrite_when_unchanged() {
        let mut parent = JsonSettingsInternals::default();
        parent.set("meta.version", &3).unwrap();

        let mut bds = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());
        settings
            .load_from_file(Some(&mut parent), true, &mut bds)
            .unwrap();
        assert!(settings.save_to_file(&mut parent, &mut bds, false));

        let mut reloaded = BoardDesignSettings::default();
        let mut settings = BoardDesignSettingsJson::new(BDS_JSON_PATH, raw_codec());
        assert!(
            settings
                .load_from_file(Some(&mut parent), true, &mut reloaded)
                .unwrap()
        );
        assert!(!settings.save_to_file(&mut parent, &mut reloaded, false));
    }
}
