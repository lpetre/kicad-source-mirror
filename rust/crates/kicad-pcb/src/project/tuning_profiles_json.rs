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

//! Port of `common/project/tuning_profiles.cpp` and `include/project/tuning_profiles.h`
//! (`TUNING_PROFILES`, the nested `tuning_profiles` object of the `.kicad_pro`), load side
//! only (package SET-11, done by WIRE-1): the `tuning_profiles_impedance_geometric` parameter
//! and the schema migrations 0 -> 1 -> 2. `PROJECT_FILE` registers it after `NET_SETTINGS`;
//! the nested objects are independent, so [`load_tuning_profiles`] reads it from the parsed
//! project document (`ProjectFile::internals` / `BoardProject::document`) on its own.
//!
//! The writer (`saveUserDefinedProfileConfigurationLine`) is not ported (SET-13).

use std::collections::BTreeMap;

use kicad_core::PcbLayerId;
use kicad_core::json::{FromJson, JsonError, Value};
use kicad_core::json_settings::{JsonSettingsInternals, NestedSettings};
use kicad_core::lset::Lset;
use kicad_core::settings_parameters::{JsonValue, ParamLambda};

/// `tuningParametersSchemaVersion`.
pub const TUNING_PARAMETERS_SCHEMA_VERSION: i32 = 2;

/// The path of the nested object in the project file.
pub const TUNING_PROFILES_PATH: &str = "tuning_profiles";

/// Port of `DELAY_PROFILE_VIA_OVERRIDE_ENTRY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayProfileViaOverrideEntry {
    /// `m_SignalLayerFrom`
    pub signal_layer_from: PcbLayerId,
    /// `m_SignalLayerTo`
    pub signal_layer_to: PcbLayerId,
    /// `m_ViaLayerFrom`
    pub via_layer_from: PcbLayerId,
    /// `m_ViaLayerTo`
    pub via_layer_to: PcbLayerId,
    /// `m_Delay`
    pub delay: i32,
}

/// Port of `DELAY_PROFILE_TRACK_PROPAGATION_ENTRY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayProfileTrackPropagationEntry {
    /// `m_signalLayer`
    pub signal_layer: PcbLayerId,
    /// `m_topReferenceLayer`
    pub top_reference_layer: PcbLayerId,
    /// `m_bottomReferenceLayer`
    pub bottom_reference_layer: PcbLayerId,
    /// `m_width`
    pub width: i32,
    /// `m_diffPairGap`
    pub diff_pair_gap: i32,
    /// `m_delay`
    pub delay: i32,
    /// `m_enableTimeDomainTuning`
    pub enable_time_domain_tuning: bool,
}

impl Default for DelayProfileTrackPropagationEntry {
    fn default() -> Self {
        DelayProfileTrackPropagationEntry {
            signal_layer: PcbLayerId::UndefinedLayer,
            top_reference_layer: PcbLayerId::UndefinedLayer,
            bottom_reference_layer: PcbLayerId::UndefinedLayer,
            width: 0,
            diff_pair_gap: 0,
            delay: 0,
            enable_time_domain_tuning: false,
        }
    }
}

impl DelayProfileTrackPropagationEntry {
    /// `GetDelay( aForce )`.
    pub fn delay(&self, force: bool) -> i32 {
        if self.enable_time_domain_tuning || force {
            self.delay
        } else {
            0
        }
    }
}

/// `TUNING_PROFILE::PROFILE_TYPE` as stored (`static_cast<PROFILE_TYPE>( int )`: 0 = `SINGLE`,
/// 1 = `DIFFERENTIAL`; other values are kept as read).
pub type ProfileTypeValue = i32;

/// Port of `TUNING_PROFILE`.
#[derive(Clone, Debug, PartialEq)]
pub struct TuningProfile {
    /// `m_ProfileName`
    pub profile_name: String,
    /// `m_Type`
    pub type_: ProfileTypeValue,
    /// `m_TargetImpedance`
    pub target_impedance: f64,
    /// `m_Frequency`
    pub frequency: f64,
    /// `m_EnableTimeDomainTuning`
    pub enable_time_domain_tuning: bool,
    /// `m_ModelSolderMask`
    pub model_solder_mask: bool,
    /// `m_TrackPropagationEntries`
    pub track_propagation_entries: Vec<DelayProfileTrackPropagationEntry>,
    /// `m_ViaPropagationDelay`
    pub via_propagation_delay: i32,
    /// `m_ViaOverrides`
    pub via_overrides: Vec<DelayProfileViaOverrideEntry>,
    /// `m_NetChainBridgePropagationDelay`
    pub net_chain_bridge_propagation_delay: i32,
    /// `m_TrackPropagationEntriesMap` (keyed by the layer number)
    pub track_propagation_entries_map: BTreeMap<i32, DelayProfileTrackPropagationEntry>,
}

/// Port of `TUNING_PROFILES` (load side): the profiles in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TuningProfiles {
    /// `m_tuningProfiles`
    pub tuning_profiles: Vec<TuningProfile>,
}

/// `const nlohmann::json& entry[ key ]` followed by a conversion: a missing key or a value of
/// the wrong type throws (the C++ `operator[]` on a const object with a missing key is
/// undefined; the port treats it as a throw).
fn field<T: FromJson>(entry: &Value, key: &str) -> Result<T, JsonError> {
    match entry.get(key) {
        Some(v) => T::from_json(v),
        None => Err(JsonError::type_must_be("object", &Value::Null)),
    }
}

/// The values a range-for over a `nlohmann::json` visits: the elements of an array, the
/// values of an object, nothing for null, the value itself otherwise.
fn range_for(value: Option<&Value>) -> Vec<&Value> {
    match value {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(a)) => a.iter().collect(),
        Some(Value::Object(o)) => o.values().collect(),
        Some(v) => vec![v],
    }
}

/// `static_cast<PCB_LAYER_ID>( LSET::NameToLayer( name ) )`.
fn layer(entry: &Value, key: &str) -> Result<PcbLayerId, JsonError> {
    let name: String = field(entry, key)?;
    Ok(PcbLayerId::from(Lset::name_to_layer(&name)))
}

/// `readViaOverrideConfigurationLine`.
fn read_via_override_configuration_line(
    entry: &Value,
) -> Result<DelayProfileViaOverrideEntry, JsonError> {
    let signal_layer_from = layer(entry, "signal_layer_from")?;
    let signal_layer_to = layer(entry, "signal_layer_to")?;
    let via_layer_from = layer(entry, "via_layer_from")?;
    let via_layer_to = layer(entry, "via_layer_to")?;
    let delay: i32 = field(entry, "delay")?;

    Ok(DelayProfileViaOverrideEntry {
        signal_layer_from,
        signal_layer_to,
        via_layer_from,
        via_layer_to,
        delay,
    })
}

/// `readUserDefinedProfileConfigurationLine`.
fn read_user_defined_profile_configuration_line(entry: &Value) -> Result<TuningProfile, JsonError> {
    let profile_name: String = field(entry, "profile_name")?;
    let profile_type: i32 = field(entry, "type")?;
    let target_impedance: f64 = field(entry, "target_impedance")?;
    let frequency: f64 = field(entry, "frequency")?;
    let model_solder_mask: bool = field(entry, "model_solder_mask")?;
    let enable_time_domain_tuning: bool = field(entry, "enable_time_domain_tuning")?;
    let via_prop_delay: i32 = field(entry, "via_prop_delay")?;
    let net_chain_bridge_prop_delay: i32 = field(entry, "net_chain_bridge_prop_delay")?;

    let mut track_entries = Vec::new();
    let mut track_entries_map = BTreeMap::new();

    for layer_entry in range_for(entry.get("layer_entries")) {
        if !layer_entry.is_object() {
            continue;
        }

        let signal_layer = layer(layer_entry, "signal_layer")?;
        let top_ref_layer = layer(layer_entry, "top_reference_layer")?;
        let bottom_ref_layer = layer(layer_entry, "bottom_reference_layer")?;

        let track_entry = DelayProfileTrackPropagationEntry {
            signal_layer,
            top_reference_layer: top_ref_layer,
            bottom_reference_layer: bottom_ref_layer,
            width: field(layer_entry, "width")?,
            diff_pair_gap: field(layer_entry, "diff_pair_gap")?,
            delay: field(layer_entry, "delay")?,
            enable_time_domain_tuning,
        };

        track_entries.push(track_entry);
        track_entries_map.insert(i32::from(signal_layer), track_entry);
    }

    let mut via_overrides = Vec::new();

    for via_entry in range_for(entry.get("via_overrides")) {
        if !via_entry.is_object() || via_entry.get("signal_layer_from").is_none() {
            continue;
        }

        via_overrides.push(read_via_override_configuration_line(via_entry)?);
    }

    Ok(TuningProfile {
        profile_name,
        type_: profile_type,
        target_impedance,
        frequency,
        enable_time_domain_tuning,
        model_solder_mask,
        track_propagation_entries: track_entries,
        via_propagation_delay: via_prop_delay,
        via_overrides,
        net_chain_bridge_propagation_delay: net_chain_bridge_prop_delay,
        track_propagation_entries_map: track_entries_map,
    })
}

/// The setter of the `tuning_profiles_impedance_geometric` `PARAM_LAMBDA`. An exception thrown
/// by a profile entry ends the load (`JSON_SETTINGS::Load` catches it) with the profiles read
/// before it kept.
fn set_profiles(target: &mut TuningProfiles, json: &Value) {
    let Value::Array(entries) = json else {
        return;
    };

    // ClearTuningProfiles()
    target.tuning_profiles.clear();

    for entry in entries {
        if !entry.is_object() || entry.get("profile_name").is_none() {
            continue;
        }

        match read_user_defined_profile_configuration_line(entry) {
            Ok(profile) => target.tuning_profiles.push(profile),
            Err(_) => return,
        }
    }
}

/// Port of `TUNING_PROFILES::migrateSchema0to1`: add frequency and model solder mask fields to
/// tuning profiles.
fn migrate_schema_0_to_1(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    if let Some(Value::Array(_)) = internals.get_json("tuning_profiles_impedance_geometric")? {
        if let Value::Array(profiles) = internals.at_mut("tuning_profiles_impedance_geometric")? {
            for profile in profiles.iter_mut() {
                set_member(profile, "frequency", kicad_core::json::float_value(1e9));
                set_member(profile, "model_solder_mask", Value::Bool(false));
            }
        }
    }

    Ok(true)
}

/// Port of `TUNING_PROFILES::migrateSchema1to2`: add net chain bridge propagation delay
/// entries.
fn migrate_schema_1_to_2(internals: &mut JsonSettingsInternals) -> Result<bool, JsonError> {
    if let Some(Value::Array(_)) = internals.get_json("tuning_profiles_impedance_geometric")? {
        if let Value::Array(profiles) = internals.at_mut("tuning_profiles_impedance_geometric")? {
            for profile in profiles.iter_mut() {
                set_member(profile, "net_chain_bridge_prop_delay", Value::from(0));
            }
        }
    }

    Ok(true)
}

/// `profile.value()[ key ] = value` (nlohmann turns a null into an object; other non-objects
/// throw, which the migration loop does not catch: the port skips them).
fn set_member(profile: &mut Value, key: &str, value: Value) {
    if profile.is_null() {
        *profile = Value::Object(Default::default());
    }

    if let Value::Object(map) = profile {
        map.insert(key.to_string(), value);
    }
}

/// Port of `TUNING_PROFILES::LoadFromFile` (`NESTED_SETTINGS`) on the parent project document
/// (`None`: no project document): the profiles and whether the nested object was found and
/// current or migrated.
pub fn load_tuning_profiles(parent: Option<&JsonSettingsInternals>) -> (TuningProfiles, bool) {
    let mut nested: NestedSettings<TuningProfiles> =
        NestedSettings::new(TUNING_PARAMETERS_SCHEMA_VERSION, TUNING_PROFILES_PATH);

    nested.settings.add_param(Box::new(ParamLambda::new(
        "tuning_profiles_impedance_geometric",
        |_: &TuningProfiles| JsonValue(Value::Null),
        |target: &mut TuningProfiles, json: JsonValue| set_profiles(target, &json.0),
        JsonValue(Value::Null),
    )));

    nested
        .settings
        .register_migration(0, 1, Box::new(migrate_schema_0_to_1));
    nested
        .settings
        .register_migration(1, 2, Box::new(migrate_schema_1_to_2));

    let mut profiles = TuningProfiles::default();
    let ok = nested.load_from_parent(parent, &mut profiles);
    (profiles, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(text: &str) -> JsonSettingsInternals {
        JsonSettingsInternals::new(
            kicad_core::json::parse(text, kicad_core::json::ObjectOrder::Sorted).expect("json"),
        )
    }

    #[test]
    fn no_project_document_gives_no_profiles() {
        let (p, ok) = load_tuning_profiles(None);
        assert!(!ok);
        assert!(p.tuning_profiles.is_empty());
    }

    #[test]
    fn profiles_are_read_and_migrated() {
        let d = doc(r#"{ "tuning_profiles": { "meta": { "version": 0 },
                 "tuning_profiles_impedance_geometric": [
                   { "profile_name": "P1", "type": 1, "target_impedance": 90.0,
                     "enable_time_domain_tuning": true, "via_prop_delay": 3,
                     "layer_entries": [
                       { "signal_layer": "F.Cu", "top_reference_layer": "In1.Cu",
                         "bottom_reference_layer": "", "width": 100000,
                         "diff_pair_gap": 120000, "delay": 7 } ],
                     "via_overrides": [] },
                   { "no_name": 1 } ] } }"#);
        let (p, ok) = load_tuning_profiles(Some(&d));
        assert!(ok);
        assert_eq!(p.tuning_profiles.len(), 1);
        let t = &p.tuning_profiles[0];
        assert_eq!(t.profile_name, "P1");
        assert_eq!(t.type_, 1);
        assert_eq!(t.frequency, 1e9);
        assert!(!t.model_solder_mask);
        assert_eq!(t.net_chain_bridge_propagation_delay, 0);
        assert_eq!(t.track_propagation_entries.len(), 1);
        let e = t.track_propagation_entries[0];
        assert_eq!(e.signal_layer, PcbLayerId::FCu);
        assert_eq!(e.top_reference_layer, PcbLayerId::In1Cu);
        assert_eq!(e.bottom_reference_layer, PcbLayerId::UndefinedLayer);
        assert_eq!(
            (e.width, e.diff_pair_gap, e.delay(false)),
            (100000, 120000, 7)
        );
    }

    #[test]
    fn a_bad_entry_ends_the_load() {
        let d = doc(r#"{ "tuning_profiles": { "meta": { "version": 2 },
                 "tuning_profiles_impedance_geometric": [
                   { "profile_name": "A", "type": 0, "target_impedance": 50.0,
                     "frequency": 1e9, "model_solder_mask": false,
                     "enable_time_domain_tuning": false, "via_prop_delay": 0,
                     "net_chain_bridge_prop_delay": 0, "layer_entries": [],
                     "via_overrides": [] },
                   { "profile_name": "B", "type": "x" },
                   { "profile_name": "C", "type": 0, "target_impedance": 50.0,
                     "frequency": 1e9, "model_solder_mask": false,
                     "enable_time_domain_tuning": false, "via_prop_delay": 0,
                     "net_chain_bridge_prop_delay": 0, "layer_entries": [],
                     "via_overrides": [] } ] } }"#);
        let (p, _) = load_tuning_profiles(Some(&d));
        let names: Vec<_> = p
            .tuning_profiles
            .iter()
            .map(|t| t.profile_name.as_str())
            .collect();
        assert_eq!(names, vec!["A"]);
    }
}
