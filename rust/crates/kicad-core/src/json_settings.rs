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

//! Port of the board-model independent mechanics of `common/settings/json_settings.cpp`,
//! `include/settings/json_settings.h`, `include/settings/json_settings_internals.h` and
//! `common/settings/nested_settings.cpp`.
//!
//! A [`JsonSettings`] owns the JSON document (`JSON_SETTINGS_INTERNALS`, a `nlohmann::json`,
//! i.e. sorted objects), a list of parameters ([`crate::settings_parameters`]) that bind
//! dotted paths to fields of a settings struct `S`, a schema version and migrators. Where C++
//! binds a parameter to a member through a pointer, the port binds it to accessor functions of
//! `S` and every operation takes the settings struct explicitly.
//!
//! File I/O (`LoadFromFile`, `SaveToFile` to disk, legacy wxConfig migration, the settings
//! manager) is not ported; callers read and write the bytes themselves (`PORT_STATUS.md`).

use std::collections::BTreeMap;

use crate::json::{
    self, FromJson, JsonError, JsonPointer, ObjectOrder, ToJson, Value, json_eq,
};
use crate::settings_parameters::Param;

/// Port of `JSON_SETTINGS_INTERNALS`: the document plus the copy of what was loaded.
#[derive(Clone, Debug, Default)]
pub struct JsonSettingsInternals {
    /// The document (`*m_internals`, a `nlohmann::json`).
    pub doc: Value,
    /// `m_original`: the document as parsed, before migration.
    pub original: Value,
}

impl JsonSettingsInternals {
    /// An internals object holding `doc`.
    pub fn new(doc: Value) -> Self {
        Self { doc, original: Value::Null }
    }

    /// Port of `JSON_SETTINGS_INTERNALS::PointerFromString`.
    pub fn pointer_from_string(path: &str) -> JsonPointer {
        JsonPointer::from_dotted(path)
    }

    /// Port of `JSON_SETTINGS::Contains`. `Err` where `json_pointer::contains` throws.
    pub fn contains(&self, path: &str) -> Result<bool, JsonError> {
        Self::pointer_from_string(path).contains(&self.doc)
    }

    /// Port of `JSON_SETTINGS::GetJson`: a copy of the value at `path`, `None` when missing.
    /// A `null` value is present. `Err` where `contains` throws (an empty or overflowing array
    /// index token) - C++ lets that exception escape.
    pub fn get_json(&self, path: &str) -> Result<Option<Value>, JsonError> {
        let ptr = Self::pointer_from_string(path);

        if ptr.contains(&self.doc)? {
            if let Ok(v) = ptr.at(&self.doc) {
                return Ok(Some(v.clone()));
            }
        }

        Ok(None)
    }

    /// Port of `JSON_SETTINGS::Get<T>`: `None` when missing or when `get<T>()` throws.
    pub fn get<T: FromJson>(&self, path: &str) -> Result<Option<T>, JsonError> {
        Ok(self.get_json(path)?.and_then(|v| T::from_json(&v).ok()))
    }

    /// Port of the `JSON_SETTINGS::Get<wxString>` specialisation: unlike the generic `Get`, a
    /// value that is not a string makes `get<std::string>()` throw *out of* `Get`.
    pub fn get_wx_string(&self, path: &str) -> Result<Option<String>, JsonError> {
        match self.get_json(path)? {
            Some(v) => String::from_json(&v).map(Some),
            None => Ok(None),
        }
    }

    /// Port of `JSON_SETTINGS_INTERNALS::Get<T>`: `at( pointer ).get<T>()`, throwing when
    /// missing or not convertible.
    pub fn internals_get<T: FromJson>(&self, path: &str) -> Result<T, JsonError> {
        T::from_json(Self::pointer_from_string(path).at(&self.doc)?)
    }

    /// Port of `JSON_SETTINGS_INTERNALS::At`.
    pub fn at_mut(&mut self, path: &str) -> Result<&mut Value, JsonError> {
        Self::pointer_from_string(path).at_mut(&mut self.doc)
    }

    /// Port of `JSON_SETTINGS_INTERNALS::operator[]( path )` (creates missing members).
    pub fn index_mut(&mut self, path: &str) -> Result<&mut Value, JsonError> {
        Self::pointer_from_string(path).get_unchecked(&mut self.doc)
    }

    /// Port of `JSON_SETTINGS::Set<T>` / `SetFromString`.
    pub fn set<T: ToJson + ?Sized>(&mut self, path: &str, value: &T) -> Result<(), JsonError> {
        *self.index_mut(path)? = value.to_json();
        Ok(())
    }

    /// Port of `nlohmann::json::clear()` on the document: containers are emptied, scalars get
    /// their default value (`null` stays `null`).
    pub fn clear(&mut self) {
        clear_value(&mut self.doc);
    }
}

/// Port of `basic_json::clear()`.
pub fn clear_value(v: &mut Value) {
    match v {
        Value::Object(m) => m.clear(),
        Value::Array(a) => a.clear(),
        Value::String(s) => s.clear(),
        Value::Bool(b) => *b = false,
        Value::Number(n) => {
            *n = if n.is_f64() { json::Number::from_f64(0.0).unwrap_or(0.into()) } else { 0.into() };
        }
        Value::Null => {}
    }
}

/// A schema migrator (`std::function<bool()>` registered with `registerMigration`). It edits
/// the document and returns whether it succeeded; `Err` models an exception thrown from it.
pub type Migrator = Box<dyn Fn(&mut JsonSettingsInternals) -> Result<bool, JsonError>>;

/// Port of the generic part of `JSON_SETTINGS` for a settings struct `S`.
pub struct JsonSettings<S> {
    /// `m_internals`.
    pub internals: JsonSettingsInternals,
    /// `m_params`, in registration order (the read-only `meta.version` parameter of the C++
    /// constructor is implicit, see [`JsonSettings::store`]).
    pub params: Vec<Box<dyn Param<S>>>,
    /// `m_schemaVersion`.
    pub schema_version: i32,
    /// `m_resetParamsIfMissing`: whether a missing key resets a parameter to its default.
    pub reset_params_if_missing: bool,
    /// `m_isFutureFormat`: the loaded file has a newer schema version.
    pub is_future_format: bool,
    /// `m_modified`.
    pub modified: bool,
    /// `m_migrators`: old version -> (new version, migrator).
    migrators: BTreeMap<i32, (i32, Migrator)>,
}

impl<S> JsonSettings<S> {
    /// Port of the `JSON_SETTINGS` constructor: the document starts as
    /// `{ "meta": { "filename": <full file name> } }` when a file name is given.
    pub fn new(schema_version: i32, full_filename: Option<&str>) -> Self {
        let mut internals = JsonSettingsInternals::default();

        if let Some(name) = full_filename {
            let _ = internals.set("meta.filename", &name);
        }

        Self {
            internals,
            params: Vec::new(),
            schema_version,
            reset_params_if_missing: true,
            is_future_format: false,
            modified: false,
            migrators: BTreeMap::new(),
        }
    }

    /// Adds a parameter (`m_params.emplace_back( new PARAM... )`).
    pub fn add_param(&mut self, param: Box<dyn Param<S>>) {
        self.params.push(param);
    }

    /// Port of `JSON_SETTINGS::registerMigration`.
    pub fn register_migration(&mut self, old_version: i32, new_version: i32, migrator: Migrator) {
        debug_assert!(new_version > old_version);
        debug_assert!(new_version <= self.schema_version);
        self.migrators.insert(old_version, (new_version, migrator));
    }

    /// Port of `JSON_SETTINGS::Load`: every parameter in registration order; a parameter whose
    /// load fails (an exception in C++) is skipped, side effects already made stay.
    pub fn load(&self, target: &mut S) {
        for param in &self.params {
            let _ = param.load(&self.internals, target, self.reset_params_if_missing);
        }
    }

    /// Port of `JSON_SETTINGS::Migrate`.
    pub fn migrate(&mut self) -> Result<bool, JsonError> {
        let mut filever: i32 = self.internals.internals_get("meta.version")?;

        while filever < self.schema_version {
            let Some((new_version, migrator)) = self.migrators.get(&filever) else {
                // "Migrator missing for %s version %d!"
                return Ok(false);
            };

            let new_version = *new_version;

            if migrator(&mut self.internals)? {
                filever = new_version;
                *self.internals.at_mut("meta.version")? = filever.to_json();
            } else {
                return Ok(false);
            }
        }

        Ok(true)
    }

    /// Port of `JSON_SETTINGS::parseAndMigrate` after a successful parse into the document.
    /// Returns `(success, migrated)`.
    fn after_parse(&mut self) -> (bool, bool) {
        let mut success = true;
        let mut migrated = false;

        self.internals.original = self.internals.doc.clone();

        let filever = match self.internals.internals_get::<i32>("meta.version") {
            Ok(v) => v,
            Err(_) => {
                success = false;
                -1
            }
        };

        if filever >= 0 && filever < self.schema_version {
            // An exception from a migrator escapes parseAndMigrate's handler in C++ (only
            // parse_error is caught there); the port treats it as a failed migration.
            if let Ok(true) = self.migrate() {
                migrated = true;
            }
        } else if filever > self.schema_version {
            self.is_future_format = true;
        }

        (success, migrated)
    }

    /// Port of `JSON_SETTINGS::LoadFromBuffer`: clear, load defaults, parse (nlohmann with
    /// comments allowed), migrate, load. Returns `m_fileSynced` (false on a parse error or a
    /// missing / unreadable `meta.version`).
    pub fn load_from_buffer(&mut self, contents: &[u8], target: &mut S) -> bool {
        self.internals.clear();
        self.load(target);

        let success = match json::parse_bytes(contents, ObjectOrder::Sorted, true) {
            Ok(doc) => {
                self.internals.doc = doc;
                self.after_parse().0
            }
            // A parse error leaves the cleared document in place. nlohmann reports a number
            // overflow as out_of_range (not caught by KiCad's parse_error handler); the port
            // treats it like a parse error.
            Err(_) => false,
        };

        self.load(target);
        self.modified = false;
        success
    }

    /// Port of `JSON_SETTINGS::Store`: for each parameter, `modified |= !MatchesFile()`, then
    /// `Store()`. The first parameter is the read-only `PARAM<int>( "meta.version",
    /// &m_schemaVersion )` of the constructor, which still stores the schema version.
    pub fn store(&mut self, target: &S) -> bool {
        // PARAM<int>( "meta.version", &m_schemaVersion, m_schemaVersion, true )
        let matches = match self.internals.get::<i32>("meta.version") {
            Ok(Some(v)) => v == self.schema_version,
            _ => self.reset_params_if_missing,
        };

        self.modified |= !matches;
        let _ = self.internals.set("meta.version", &self.schema_version);

        for param in &self.params {
            let Ok(matches) = param.matches_file(&self.internals, target, self.reset_params_if_missing) else {
                continue;
            };

            self.modified |= !matches;
            let _ = param.store(&mut self.internals, target);
        }

        self.modified
    }

    /// Port of `JSON_SETTINGS::ResetToDefaults`.
    pub fn reset_to_defaults(&self, target: &mut S) {
        for param in &self.params {
            param.set_default(target);
        }
    }

    /// Port of `JSON_SETTINGS::formatFileContents`: the original document with the subtrees
    /// of "clear unknown keys" parameters emptied, merged (`update( ..., true )`) with the
    /// current document, `dump( 2 ) + '\n'`.
    pub fn format_file_contents(&self) -> String {
        let mut to_save = self.internals.original.clone();

        for param in &self.params {
            if param.clear_unknown_keys() {
                let p = JsonSettingsInternals::pointer_from_string(param.json_path());

                if let Ok(slot) = p.get_unchecked(&mut to_save) {
                    *slot = Value::Object(json::Map::new());
                }
            }
        }

        let _ = json::update(&mut to_save, &self.internals.doc, true);
        json::dump(&to_save, Some(2), ObjectOrder::Sorted) + "\n"
    }

    /// Port of `JSON_SETTINGS::FormatAsString`: `Store()`, then `std::setw( 2 ) << internals
    /// << std::endl`.
    pub fn format_as_string(&mut self, target: &S) -> String {
        self.store(target);
        json::dump_stream(&self.internals.doc, 2, ObjectOrder::Sorted)
    }

    /// Port of `JSON_SETTINGS::fetchOrDefault`: `json.at( key ).get<T>()` when `json` contains
    /// `key`, `default` otherwise or when the conversion throws.
    pub fn fetch_or_default<T: FromJson>(json: &Value, key: &str, default: T) -> T {
        match json {
            Value::Object(map) => map.get(key).and_then(|v| T::from_json(v).ok()).unwrap_or(default),
            _ => default,
        }
    }
}

/// Port of the four `JSON_SETTINGS::SetIfPresent` overloads: assign when `obj` contains the
/// dotted path and the value has the required type (`is_string`, `is_boolean`,
/// `is_number_integer`, `is_number_unsigned`).
pub mod set_if_present {
    use super::*;

    fn value_at<'v>(obj: &'v Value, path: &str) -> Option<&'v Value> {
        let ptr = JsonSettingsInternals::pointer_from_string(path);

        if ptr.contains(obj).unwrap_or(false) { ptr.at(obj).ok() } else { None }
    }

    /// `SetIfPresent( obj, path, wxString& )`.
    pub fn string(obj: &Value, path: &str, target: &mut String) -> bool {
        match value_at(obj, path) {
            Some(Value::String(s)) => {
                *target = s.clone();
                true
            }
            _ => false,
        }
    }

    /// `SetIfPresent( obj, path, bool& )`.
    pub fn boolean(obj: &Value, path: &str, target: &mut bool) -> bool {
        match value_at(obj, path) {
            Some(Value::Bool(b)) => {
                *target = *b;
                true
            }
            _ => false,
        }
    }

    /// `SetIfPresent( obj, path, int& )` (`is_number_integer()`: signed or unsigned).
    pub fn int(obj: &Value, path: &str, target: &mut i32) -> bool {
        match value_at(obj, path) {
            Some(v) if json::is_number_integer(v) => match i32::from_json(v) {
                Ok(i) => {
                    *target = i;
                    true
                }
                Err(_) => false,
            },
            _ => false,
        }
    }

    /// `SetIfPresent( obj, path, unsigned int& )` (`is_number_unsigned()`; see
    /// [`json::classify_number`] for the non-negative `number_integer` caveat).
    pub fn unsigned(obj: &Value, path: &str, target: &mut u32) -> bool {
        match value_at(obj, path) {
            Some(v) if json::is_number_unsigned(v) => match u32::from_json(v) {
                Ok(u) => {
                    *target = u;
                    true
                }
                Err(_) => false,
            },
            _ => false,
        }
    }
}

/// Port of `NESTED_SETTINGS`: a [`JsonSettings`] whose document is the sub-object at `path` of
/// a parent document.
pub struct NestedSettings<S> {
    /// The settings engine of the nested object.
    pub settings: JsonSettings<S>,
    /// `m_path`: dotted path of the sub-object in the parent.
    pub path: String,
}

impl<S> NestedSettings<S> {
    /// A nested settings object at `path` (`NESTED_SETTINGS( name, schema, parent, path )`).
    pub fn new(schema_version: i32, path: &str) -> Self {
        Self { settings: JsonSettings::new(schema_version, None), path: path.to_string() }
    }

    /// Port of `NESTED_SETTINGS::LoadFromFile`: copy the parent's sub-object, migrate (a
    /// migrator error aborts the chain but keeps its modifications), then load all parameters
    /// (always, also without a parent object). Returns whether the object was found and is
    /// at the current schema version or was migrated to it.
    pub fn load_from_parent(&mut self, parent: Option<&JsonSettingsInternals>, target: &mut S) -> bool {
        let s = &mut self.settings;
        s.internals.clear();
        let mut success = false;

        if let Some(parent) = parent {
            let ptr = JsonSettingsInternals::pointer_from_string(&self.path);

            if ptr.contains(&parent.doc).unwrap_or(false) {
                if let Ok(sub) = ptr.at(&parent.doc) {
                    let sub = sub.clone();

                    if json::update(&mut s.internals.doc, &sub, false).is_ok() {
                        success = true;
                    }
                }
            }
        }

        if success {
            let filever = match s.internals.internals_get::<i32>("meta.version") {
                Ok(v) => v,
                Err(_) => {
                    success = false;
                    -1
                }
            };

            if filever >= 0 && filever < s.schema_version {
                let migrated = s.migrate().unwrap_or_else(|_| {
                    success = false;
                    false
                });

                if !migrated {
                    success = false;
                }
            }
        }

        s.load(target);
        success
    }

    /// Port of `NESTED_SETTINGS::SaveToFile`: reports whether the nested object differs from
    /// the parent's copy or a parameter changed, and if so (or with `force`) merges it into
    /// the parent (`parent[path].update( internals )`).
    pub fn save_to_parent(&mut self, parent: &mut JsonSettingsInternals, target: &S, force: bool) -> bool {
        let s = &mut self.settings;

        let Ok(in_parent) = parent.get_json(&self.path) else {
            return false;
        };

        let mut modified = match &in_parent {
            None => true,
            Some(p) => !json_eq(&s.internals.doc, p),
        };

        modified |= s.store(target);

        for param in &s.params {
            if !param.clear_unknown_keys() {
                continue;
            }

            let ptr = JsonSettingsInternals::pointer_from_string(&format!("{}.{}", self.path, param.json_path()));

            if ptr.contains(&parent.original).unwrap_or(false) {
                if let Ok(slot) = ptr.get_unchecked(&mut parent.original) {
                    *slot = Value::Object(json::Map::new());
                }
            }
        }

        if modified || force {
            match parent.index_mut(&self.path) {
                Ok(slot) => {
                    if json::update(slot, &s.internals.doc, false).is_err() {
                        return false;
                    }
                }
                Err(_) => return false,
            }
        }

        modified
    }
}

