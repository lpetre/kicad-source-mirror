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

//! Port of the JSON part of `common/project/net_settings.cpp`: the `m_params` of the
//! `NET_SETTINGS` constructor (the `PARAM_LAMBDA` readers and writers of `classes`,
//! `net_colors`, `net_chain_classes`, `net_chain_netclasses`, `netclass_assignments` and
//! `netclass_patterns`), the schema migrations 0 -> 5 and `CopyFrom`.
//!
//! The settings data (`NetSettings`, `crate::net_settings`) stays plain data; this module holds
//! the `NESTED_SETTINGS` engine (`kicad_core::json_settings::NestedSettings`) whose parameters
//! read from / write to a `NetSettings` passed in by the caller. A `.kicad_pro` is loaded with
//! [`NetSettingsJson::load_from_parent`] (the parent document is the parsed project file) and
//! stored back with [`NetSettingsJson::save_to_parent`], after which the parent's
//! `JsonSettings::format_file_contents` produces the bytes the C++ writes.

use std::sync::Arc;

use kicad_core::Color4D;
use kicad_core::base_units::{PCB_IU_SCALE, SCH_IU_SCALE};
use kicad_core::json::{FromJson, JsonError, JsonErrorKind, Map, ToJson, Value, float_value};
use kicad_core::json_settings::{JsonSettingsInternals, Migrator, NestedSettings};
use kicad_core::settings_parameters::{JsonValue, Param, ParamLambda};
use kicad_core::string_utils::convert_to_new_overbar_notation;
use kicad_geom::cmath::f64_to_i32_x86;

use crate::net_settings::NetSettings;
use crate::netclass::{NETCLASS_DEFAULT_NAME, Netclass};

// const int netSettingsSchemaVersion = 0;
// const int netSettingsSchemaVersion = 1;     // new overbar syntax
// const int netSettingsSchemaVersion = 2;     // exclude buses from netclass members
// const int netSettingsSchemaVersion = 3;     // netclass assignment patterns
// const int netSettingsSchemaVersion = 4;     // netclass ordering
/// `netSettingsSchemaVersion`: tuning profile names.
pub const NET_SETTINGS_SCHEMA_VERSION: i32 = 5;

/// The path of the net settings in the project file (`PROJECT_FILE` constructor:
/// `NET_SETTINGS( this, "net_settings" )`).
pub const NET_SETTINGS_PATH: &str = "net_settings";

/// Port of the `NET_SETTINGS` constructor's `NESTED_SETTINGS` part: the settings engine with the
/// six JSON parameters and the five migrators registered, in the C++ order.
pub struct NetSettingsJson {
    /// `NESTED_SETTINGS( "net_settings", netSettingsSchemaVersion, aParent, aPath, false )`.
    pub nested: NestedSettings<NetSettings>,
}

impl Default for NetSettingsJson {
    fn default() -> Self {
        Self::new(NET_SETTINGS_PATH)
    }
}

impl NetSettingsJson {
    /// Port of `NET_SETTINGS::NET_SETTINGS( aParent, aPath )` (the JSON parameters and
    /// migrations; the data defaults are `NetSettings::default()`).
    pub fn new(path: &str) -> Self {
        let mut nested = NestedSettings::new(NET_SETTINGS_SCHEMA_VERSION, path);
        let s = &mut nested.settings;

        s.add_param(Box::new(ParamLambda::new(
            "classes",
            classes_getter,
            classes_setter,
            JsonValue(Value::Null),
        )));

        // Let the save drop removed colors instead of merging them back in
        s.add_param(Box::new(ClearUnknownKeys(ParamLambda::new(
            "net_colors",
            net_colors_getter,
            net_colors_setter,
            JsonValue(Value::Null),
        ))));

        // Let the save drop removed chains instead of merging them back in
        s.add_param(Box::new(ClearUnknownKeys(ParamLambda::new(
            "net_chain_classes",
            |ns: &NetSettings| chain_map_getter(&ns.net_chain_classes),
            |ns: &mut NetSettings, json: JsonValue| {
                chain_map_setter(&mut ns.net_chain_classes, &json.0)
            },
            JsonValue(Value::Null),
        ))));

        // Let the save drop removed chain netclasses instead of merging them back in
        s.add_param(Box::new(ClearUnknownKeys(ParamLambda::new(
            "net_chain_netclasses",
            |ns: &NetSettings| chain_map_getter(&ns.net_chain_net_classes),
            |ns: &mut NetSettings, json: JsonValue| {
                chain_map_setter(&mut ns.net_chain_net_classes, &json.0)
            },
            JsonValue(Value::Null),
        ))));

        // Let the save drop stale netclass assignments instead of merging them back in
        s.add_param(Box::new(ClearUnknownKeys(ParamLambda::new(
            "netclass_assignments",
            netclass_assignments_getter,
            netclass_assignments_setter,
            JsonValue(Value::Null),
        ))));

        s.add_param(Box::new(ParamLambda::new(
            "netclass_patterns",
            netclass_patterns_getter,
            netclass_patterns_setter,
            JsonValue(Value::Null),
        )));

        s.register_migration(0, 1, boxed(migrate_schema_0_to_1));
        s.register_migration(1, 2, boxed(migrate_schema_1_to_2));
        s.register_migration(2, 3, boxed(migrate_schema_2_to_3));
        s.register_migration(3, 4, boxed(migrate_schema_3_to_4));
        s.register_migration(4, 5, boxed(migrate_schema_4_to_5));

        Self { nested }
    }

    /// `NESTED_SETTINGS::LoadFromFile` of the net settings: copies the `net_settings` object of
    /// the parent document (the parsed project file; `None` = no parent), migrates it and loads
    /// every parameter into `target`. Returns whether the object was found and is current or
    /// was migrated.
    pub fn load_from_parent(
        &mut self,
        parent: Option<&JsonSettingsInternals>,
        target: &mut NetSettings,
    ) -> bool {
        self.nested.load_from_parent(parent, target)
    }

    /// `JSON_SETTINGS::Load`: loads every parameter from the current document.
    pub fn load(&self, target: &mut NetSettings) {
        self.nested.settings.load(target);
    }

    /// `JSON_SETTINGS::Store`: writes every parameter into the document; returns whether the
    /// document changed.
    pub fn store(&mut self, source: &NetSettings) -> bool {
        self.nested.settings.store(source)
    }

    /// `NESTED_SETTINGS::SaveToFile`: stores and merges the object into the parent document.
    pub fn save_to_parent(
        &mut self,
        parent: &mut JsonSettingsInternals,
        source: &NetSettings,
        force: bool,
    ) -> bool {
        self.nested.save_to_parent(parent, source, force)
    }

    /// `JSON_SETTINGS::FormatAsString`.
    pub fn format_as_string(&mut self, source: &NetSettings) -> String {
        self.nested.settings.format_as_string(source)
    }

    /// `JSON_SETTINGS::Internals()`.
    pub fn internals(&self) -> &JsonSettingsInternals {
        &self.nested.settings.internals
    }

    /// `JSON_SETTINGS::Internals()` (mutable).
    pub fn internals_mut(&mut self) -> &mut JsonSettingsInternals {
        &mut self.nested.settings.internals
    }

    /// Port of `NET_SETTINGS::CopyFrom( aOther )`: `target` (owned by `self`) becomes a deep
    /// copy of `other_settings` (owned by `other`) through the JSON document. The C++
    /// self-assignment guard has no counterpart: the borrow rules exclude `other == self`.
    pub fn copy_from(
        &mut self,
        target: &mut NetSettings,
        other: &mut NetSettingsJson,
        other_settings: &NetSettings,
    ) {
        // Flush the source's live field state into its JSON cache so the cache
        // mirrors what we want to copy.
        other.store(other_settings);

        // CloneFrom() copies the JSON tree but leaves m_parent / m_path untouched,
        // which is what we need: this instance must stay attached to its host
        // project file so SaveToFile() walks the right m_nested_settings entry.
        self.nested.settings.internals.doc = other.nested.settings.internals.doc.clone();

        // Repopulate our in-memory NETCLASS / pattern / color state from the cloned
        // JSON.  Load() rebuilds m_defaultNetClass, m_netClasses, fresh
        // EDA_COMBINED_MATCHER instances for m_netClassPatternAssignments, and the
        // remaining maps, decoupling our instance from aOther's lifetime.
        self.load(target);

        // Load() repopulates persisted state but doesn't clear the derived caches.
        // Drop them so stale lookups from this instance's pre-CopyFrom life
        // (composite resolutions, implicit netclasses, chain-derived patterns
        // rebuilt on every netlist update, and the per-net effective-class cache)
        // don't survive the swap.
        target.composite_netclasses.clear();
        target.implicit_netclasses.clear();
        target.netclass_chain_pattern_assignments.clear();
        target.clear_all_caches();
    }
}

/// Loads the `net_settings` object of a parsed project document (`PROJECT_FILE` creates its
/// `NET_SETTINGS` with the constructor defaults, then `LoadFromFile` of the nested settings).
/// Returns the settings and the `LoadFromFile` result.
pub fn load_net_settings(parent: Option<&JsonSettingsInternals>) -> (NetSettings, bool) {
    let mut settings = NetSettings::default();
    let ok = NetSettingsJson::new(NET_SETTINGS_PATH).load_from_parent(parent, &mut settings);
    (settings, ok)
}

fn boxed(f: fn(&mut JsonSettingsInternals) -> Result<bool, JsonError>) -> Migrator {
    Box::new(f)
}

/// A parameter with `SetClearUnknownKeys()` (`PARAM_BASE::m_clearUnknownKeys = true`).
struct ClearUnknownKeys<P>(P);

impl<S, P: Param<S>> Param<S> for ClearUnknownKeys<P> {
    fn json_path(&self) -> &str {
        self.0.json_path()
    }

    fn load(
        &self,
        settings: &JsonSettingsInternals,
        target: &mut S,
        reset_if_missing: bool,
    ) -> Result<(), JsonError> {
        self.0.load(settings, target, reset_if_missing)
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        self.0.store(settings, target)
    }

    fn set_default(&self, target: &mut S) {
        self.0.set_default(target)
    }

    fn matches_file(
        &self,
        settings: &JsonSettingsInternals,
        target: &S,
        resets_params_if_missing: bool,
    ) -> Result<bool, JsonError> {
        self.0
            .matches_file(settings, target, resets_params_if_missing)
    }

    fn clear_unknown_keys(&self) -> bool {
        true
    }
}

// ---- conversions -------------------------------------------------------------------------

/// `wxString( s.c_str(), wxConvUTF8 )` / `wxString::FromUTF8( s.c_str() )`: the C string
/// conversion stops at the first NUL character.
fn wx_from_c_str(s: &str) -> String {
    match s.find('\0') {
        Some(end) => s[..end].to_string(),
        None => s.to_string(),
    }
}

/// `j.get<wxString>()` (`from_json( j, wxString& )`): only strings, cut at the first NUL.
fn get_wx_string(value: &Value) -> Result<String, JsonError> {
    Ok(wx_from_c_str(&String::from_json(value)?))
}

/// `j.get<KIGFX::COLOR4D>()`: a default `COLOR4D` set with `SetFromWxString`; throws for
/// non-strings.
fn get_color(value: &Value) -> Result<Color4D, JsonError> {
    let text = String::from_json(value)?;
    let mut color = Color4D::default();
    color.from_json_string(&text);
    Ok(color)
}

/// `pcbIUScale.mmToIU( double )`, with the conversion of an out-of-range result as the
/// reference build executes it (`cvttsd2si`, PORTING.md section 9).
fn mm_to_iu_x86(mm: f64) -> i32 {
    let iu_per_mm = PCB_IU_SCALE.iu_per_mm;

    f64_to_i32_x86(if mm < 0.0 {
        mm * iu_per_mm - 0.5
    } else {
        mm * iu_per_mm + 0.5
    })
}

/// `schIUScale.MilsToIU( int )` called with a `double` (implicitly truncated to `int`), both
/// conversions as the reference build executes them.
fn mils_to_iu_x86(mils: f64) -> i32 {
    let mils = f64_to_i32_x86(mils);
    let x = f64::from(mils) * SCH_IU_SCALE.iu_per_mils;

    f64_to_i32_x86(if x < 0.0 { x - 0.5 } else { x + 0.5 })
}

/// Port of the static `getInPcbUnits( aObj, aKey )`.
fn get_in_pcb_units(obj: &Map<String, Value>, key: &str) -> Option<i32> {
    match obj.get(key) {
        Some(v @ Value::Number(_)) => f64::from_json(v).ok().map(mm_to_iu_x86),
        _ => None,
    }
}

/// Port of the static `getInSchUnits( aObj, aKey )`.
fn get_in_sch_units(obj: &Map<String, Value>, key: &str) -> Option<i32> {
    match obj.get(key) {
        Some(v @ Value::Number(_)) => f64::from_json(v).ok().map(mils_to_iu_x86),
        _ => None,
    }
}

/// The members of a JSON object in nlohmann iteration order (byte order of the keys); the
/// readers stop at the first conversion error, so the order decides what was read.
fn sorted_items(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut items: Vec<(&String, &Value)> = map.iter().collect();
    items.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    items
}

/// Range-for over a `nlohmann::json`: array elements, object values (in key order), nothing
/// for `null`, the value itself for a primitive.
fn json_iter_values(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(a) => a.iter().collect(),
        Value::Object(m) => sorted_items(m).into_iter().map(|(_, v)| v).collect(),
        Value::Null => Vec::new(),
        v => vec![v],
    }
}

/// Non-const `basic_json::operator[]( key )`: a `null` becomes an object, a missing member is
/// inserted as `null`; any other type throws `type_error.305`.
fn index_key<'v>(value: &'v mut Value, key: &str) -> Result<&'v mut Value, JsonError> {
    if value.is_null() {
        *value = Value::Object(Map::new());
    }

    match value {
        Value::Object(m) => Ok(m.entry(key.to_string()).or_insert(Value::Null)),
        v => Err(JsonError {
            kind: JsonErrorKind::TypeError,
            id: 305,
            byte: 0,
            message: format!(
                "cannot use operator[] with a string argument with {}",
                kicad_core::json::type_name(v)
            ),
        }),
    }
}

// ---- classes -----------------------------------------------------------------------------

/// The `saveNetclass` lambda: appends the JSON object of `nc` to `json_array`.
fn save_netclass(json_array: &mut Vec<Value>, nc: &Netclass) {
    // Note: we're in common/, but we do happen to know which of these
    // fields are used in which units system.
    let mut nc_json = Map::new();
    nc_json.insert("name".into(), nc.get_name().to_json());
    nc_json.insert("priority".into(), nc.priority().to_json());
    // GetSchematicColor( true ) / GetPcbColor( true ): the stored colour.
    nc_json.insert(
        "schematic_color".into(),
        nc.schematic_color.to_json_string().to_json(),
    );
    nc_json.insert("pcb_color".into(), nc.pcb_color.to_json_string().to_json());
    nc_json.insert("tuning_profile".into(), nc.tuning_profile().to_json());

    let save_in_pcb_units = |json: &mut Map<String, Value>, key: &str, value: i32| {
        json.insert(key.into(), float_value(PCB_IU_SCALE.iu_to_mm(value)));
    };

    if nc.has_wire_width() {
        nc_json.insert(
            "wire_width".into(),
            SCH_IU_SCALE.iu_to_mils(nc.wire_width()).to_json(),
        );
    }

    if nc.has_bus_width() {
        nc_json.insert(
            "bus_width".into(),
            SCH_IU_SCALE.iu_to_mils(nc.bus_width()).to_json(),
        );
    }

    if nc.has_line_style() {
        nc_json.insert("line_style".into(), nc.line_style().to_json());
    }

    if nc.has_clearance() {
        save_in_pcb_units(&mut nc_json, "clearance", nc.clearance());
    }

    if nc.has_track_width() {
        save_in_pcb_units(&mut nc_json, "track_width", nc.track_width());
    }

    if nc.has_via_diameter() {
        save_in_pcb_units(&mut nc_json, "via_diameter", nc.via_diameter());
    }

    if nc.has_via_drill() {
        save_in_pcb_units(&mut nc_json, "via_drill", nc.via_drill());
    }

    if nc.has_uvia_diameter() {
        save_in_pcb_units(&mut nc_json, "microvia_diameter", nc.uvia_diameter());
    }

    if nc.has_uvia_drill() {
        save_in_pcb_units(&mut nc_json, "microvia_drill", nc.uvia_drill());
    }

    if nc.has_diff_pair_width() {
        save_in_pcb_units(&mut nc_json, "diff_pair_width", nc.diff_pair_width());
    }

    if nc.has_diff_pair_gap() {
        save_in_pcb_units(&mut nc_json, "diff_pair_gap", nc.diff_pair_gap());
    }

    if nc.has_diff_pair_via_gap() {
        save_in_pcb_units(&mut nc_json, "diff_pair_via_gap", nc.diff_pair_via_gap());
    }

    json_array.push(Value::Object(nc_json));
}

/// The `readNetClass` lambda. `Err` where the C++ throws (a `name` that is not a string).
fn read_netclass(entry: &Map<String, Value>) -> Result<Netclass, JsonError> {
    let name = get_wx_string(entry.get("name").unwrap_or(&Value::Null))?;

    let mut nc = Netclass::new(&name, name == NETCLASS_DEFAULT_NAME);

    if let Some(v @ Value::Number(_)) = entry.get("priority") {
        nc.priority = i32::from_json(v)?;
    }

    if let Some(v @ Value::String(_)) = entry.get("tuning_profile") {
        nc.tuning_profile = get_wx_string(v)?;
    }

    if let Some(value) = get_in_pcb_units(entry, "clearance") {
        nc.clearance = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "track_width") {
        nc.track_width = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "via_diameter") {
        nc.via_dia = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "via_drill") {
        nc.via_drill = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "microvia_diameter") {
        nc.uvia_dia = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "microvia_drill") {
        nc.uvia_drill = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "diff_pair_width") {
        nc.diff_pair_width = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "diff_pair_gap") {
        nc.diff_pair_gap = Some(value);
    }

    if let Some(value) = get_in_pcb_units(entry, "diff_pair_via_gap") {
        nc.diff_pair_via_gap = Some(value);
    }

    if let Some(value) = get_in_sch_units(entry, "wire_width") {
        nc.wire_width = Some(value);
    }

    if let Some(value) = get_in_sch_units(entry, "bus_width") {
        nc.bus_width = Some(value);
    }

    if let Some(v @ Value::Number(_)) = entry.get("line_style") {
        nc.line_style = Some(i32::from_json(v)?);
    }

    if let Some(v @ Value::String(_)) = entry.get("pcb_color") {
        nc.pcb_color = get_color(v)?;
    }

    if let Some(v @ Value::String(_)) = entry.get("schematic_color") {
        nc.schematic_color = get_color(v)?;
    }

    Ok(nc)
}

/// Getter of the `classes` parameter: the default class first, then the classes by name.
fn classes_getter(ns: &NetSettings) -> JsonValue {
    let mut ret = Vec::new();

    save_netclass(&mut ret, &ns.default_netclass);

    for netclass in ns.netclasses.values() {
        save_netclass(&mut ret, netclass);
    }

    JsonValue(Value::Array(ret))
}

/// Setter of the `classes` parameter. A non-string `name` throws out of the setter in C++
/// (the classes read so far stay).
fn classes_setter(ns: &mut NetSettings, json: JsonValue) {
    let Value::Array(entries) = &json.0 else {
        return;
    };

    ns.netclasses.clear();

    for entry in entries {
        let Value::Object(entry) = entry else {
            continue;
        };

        if !entry.contains_key("name") {
            continue;
        }

        let Ok(nc) = read_netclass(entry) else {
            return;
        };

        if nc.is_default() {
            ns.default_netclass = Arc::new(nc);
        } else {
            ns.netclasses.insert(nc.get_name(), Arc::new(nc));
        }
    }
}

// ---- colours, chains, assignments, patterns ----------------------------------------------

fn net_colors_getter(ns: &NetSettings) -> JsonValue {
    let mut ret = Map::new();

    for (netname, color) in &ns.net_color_assignments {
        ret.insert(netname.clone(), color.to_json_string().to_json());
    }

    JsonValue(Value::Object(ret))
}

fn net_colors_setter(ns: &mut NetSettings, json: JsonValue) {
    let Value::Object(map) = &json.0 else {
        return;
    };

    ns.net_color_assignments.clear();

    for (key, value) in sorted_items(map) {
        // The value is converted before the map entry is created (C++17 sequencing of `=`).
        let Ok(color) = get_color(value) else {
            return;
        };

        ns.net_color_assignments.insert(wx_from_c_str(key), color);
    }
}

/// Getter of `net_chain_classes` / `net_chain_netclasses`. Forces object type so an empty map
/// round-trips as {} rather than null; the reader rejects non-objects, which would otherwise
/// leave stale chain assignments in place after the user clears them all.
fn chain_map_getter(map: &std::collections::BTreeMap<String, String>) -> JsonValue {
    let mut ret = Map::new();

    for (chain, class_name) in map {
        ret.insert(chain.clone(), class_name.to_json());
    }

    JsonValue(Value::Object(ret))
}

/// Setter of `net_chain_classes` / `net_chain_netclasses`: entries with an empty value are
/// dropped; a non-string value throws (entries read so far stay).
fn chain_map_setter(target: &mut std::collections::BTreeMap<String, String>, json: &Value) {
    let Value::Object(map) = json else {
        return;
    };

    target.clear();

    for (key, value) in sorted_items(map) {
        let chain = wx_from_c_str(key);

        let Ok(class_name) = get_wx_string(value) else {
            return;
        };

        if !class_name.is_empty() {
            target.insert(chain, class_name);
        }
    }
}

fn netclass_assignments_getter(ns: &NetSettings) -> JsonValue {
    let mut ret = Map::new();

    for (netname, netclass_names) in &ns.netclass_label_assignments {
        let netclasses_json: Vec<Value> = netclass_names.iter().map(|n| n.to_json()).collect();
        ret.insert(netname.clone(), Value::Array(netclasses_json));
    }

    JsonValue(Value::Object(ret))
}

fn netclass_assignments_setter(ns: &mut NetSettings, json: JsonValue) {
    let Value::Object(map) = &json.0 else {
        return;
    };

    ns.netclass_label_assignments.clear();

    for (key, value) in sorted_items(map) {
        let key = wx_from_c_str(key);

        for netclass_name in json_iter_values(value) {
            // m_netClassLabelAssignments[key].insert( netclassName.get<wxString>() ): the map
            // entry is created before the argument is converted (C++17 sequencing).
            let set = ns
                .netclass_label_assignments
                .entry(key.clone())
                .or_default();

            let Ok(name) = get_wx_string(netclass_name) else {
                return;
            };

            set.insert(name);
        }
    }
}

fn netclass_patterns_getter(ns: &NetSettings) -> JsonValue {
    let mut ret = Vec::new();

    for (matcher, netclass_name) in &ns.netclass_pattern_assignments {
        let mut pattern_json = Map::new();
        pattern_json.insert("pattern".into(), matcher.pattern().to_json());
        pattern_json.insert("netclass".into(), netclass_name.to_json());
        ret.push(Value::Object(pattern_json));
    }

    JsonValue(Value::Array(ret))
}

fn netclass_patterns_setter(ns: &mut NetSettings, json: JsonValue) {
    let Value::Array(entries) = &json.0 else {
        return;
    };

    ns.netclass_pattern_assignments.clear();

    for entry in entries {
        let Value::Object(entry) = entry else {
            continue;
        };

        if let (Some(Value::String(pattern)), Some(Value::String(netclass))) =
            (entry.get("pattern"), entry.get("netclass"))
        {
            let pattern = wx_from_c_str(pattern);
            let netclass = wx_from_c_str(netclass);

            // Expand bus patterns so individual bus member nets can be matched
            NetSettings::for_each_bus_member(&pattern, &mut |member_pattern| {
                ns.add_single_pattern_assignment(member_pattern, &netclass);
            });
        }
    }
}

// ---- migrations --------------------------------------------------------------------------

/// `m_internals->contains( "classes" ) && m_internals->At( "classes" ).is_array()`: the
/// `classes` array of the document.
fn classes_array(doc: &mut Value) -> Option<&mut Vec<Value>> {
    match doc {
        Value::Object(m) => match m.get_mut("classes") {
            Some(Value::Array(a)) => Some(a),
            _ => None,
        },
        _ => None,
    }
}

/// Port of `NET_SETTINGS::migrateSchema0to1`: nets of the classes in the new overbar notation.
fn migrate_schema_0_to_1(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    if let Some(classes) = classes_array(&mut internals.doc) {
        for net_class in classes.iter_mut() {
            let Value::Object(net_class) = net_class else {
                continue;
            };

            if let Some(Value::Array(nets)) = net_class.get("nets") {
                let mut migrated = Vec::new();

                for net in nets {
                    migrated.push(convert_to_new_overbar_notation(&get_wx_string(net)?).to_json());
                }

                net_class.insert("nets".into(), Value::Array(migrated));
            }
        }
    }

    Ok(true)
}

/// Port of `NET_SETTINGS::migrateSchema1to2`.
fn migrate_schema_1_to_2(_internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    Ok(true)
}

/// Port of `NET_SETTINGS::migrateSchema2to3`: the class `nets` lists become
/// `netclass_patterns`.
fn migrate_schema_2_to_3(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    let Some(classes) = classes_array(&mut internals.doc) else {
        return Ok(true);
    };

    let mut patterns = Vec::new();

    for net_class in classes.iter() {
        let Value::Object(net_class) = net_class else {
            continue;
        };

        if let (Some(name), Some(Value::Array(nets))) =
            (net_class.get("name"), net_class.get("nets"))
        {
            let net_class_name = get_wx_string(name)?;

            for net in nets {
                let mut pattern_json = Map::new();
                pattern_json.insert("pattern".into(), get_wx_string(net)?.to_json());
                pattern_json.insert("netclass".into(), net_class_name.to_json());
                patterns.push(Value::Object(pattern_json));
            }
        }
    }

    internals.set("netclass_patterns", &Value::Array(patterns))?;
    Ok(true)
}

/// Port of `NET_SETTINGS::migrateSchema3to4`: class priorities and assignment lists.
fn migrate_schema_3_to_4(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    // Add priority field to netclasses
    if let Some(classes) = classes_array(&mut internals.doc) {
        let mut priority: i32 = 0;

        for net_class in classes.iter_mut() {
            let name = get_wx_string(index_key(net_class, "name")?)?;

            let value = if name == NETCLASS_DEFAULT_NAME {
                i32::MAX
            } else {
                let p = priority;
                priority += 1;
                p
            };

            *index_key(net_class, "priority")? = value.to_json();
        }
    }

    // Move netclass assignments to a list
    let assignments = match &internals.doc {
        Value::Object(m) => match m.get("netclass_assignments") {
            Some(Value::Object(a)) => Some(a.clone()),
            _ => None,
        },
        _ => None,
    };

    if let Some(assignments) = assignments {
        // nlohmann::json migrated = {};  (null until the first member is added)
        let mut migrated = Value::Null;

        for (key, value) in sorted_items(&assignments) {
            let mut netclasses_json = Vec::new();

            if !get_wx_string(value)?.is_empty() {
                netclasses_json.push(value.clone());
            }

            *index_key(&mut migrated, key)? = Value::Array(netclasses_json);
        }

        internals.set("netclass_assignments", &migrated)?;
    }

    Ok(true)
}

/// Port of `NET_SETTINGS::migrateSchema4to5`: an empty tuning profile name for every class.
fn migrate_schema_4_to_5(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    // Add tuning profile name field to netclasses
    if let Some(classes) = classes_array(&mut internals.doc) {
        for net_class in classes.iter_mut() {
            *index_key(net_class, "tuning_profile")? = "".to_json();
        }
    }

    Ok(true)
}
