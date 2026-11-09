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

//! Port of the generic parameter classes of `include/settings/parameters.h` and
//! `common/settings/parameters.cpp`: `PARAM<T>`, `PARAM_PATH`, `PARAM_ENUM`, `PARAM_LAMBDA`,
//! `PARAM_SCALED`, `PARAM_LIST`, `PARAM_PATH_LIST`, `PARAM_SET`, `PARAM_MAP` and
//! `PARAM_WXSTRING_MAP`.
//!
//! C++ parameters hold a pointer to the member they bind; here they hold accessor functions
//! of the settings struct `S` (see [`Field`]). `Load` returns `Err` where the C++ code lets a
//! nlohmann exception escape (it is then caught by `JSON_SETTINGS::Load` and the parameter is
//! skipped), so the "keeps the current value" cases behave the same.

use std::collections::{BTreeMap, BTreeSet};

use crate::json::{FromJson, JsonError, ToJson, Value, json_eq};
use crate::json_settings::JsonSettingsInternals;
use crate::wxstr::{ki_round_i32, ki_round_i64};

/// Port of `PARAM_BASE`: one entry of `JSON_SETTINGS::m_params`.
pub trait Param<S> {
    /// `GetJsonPath()`: dotted path in the document.
    fn json_path(&self) -> &str;

    /// `Load( settings, aResetIfMissing )`.
    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset_if_missing: bool) -> Result<(), JsonError>;

    /// `Store( settings )`.
    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError>;

    /// `SetDefault()`.
    fn set_default(&self, target: &mut S);

    /// `MatchesFile( settings )`; `resets_params_if_missing` is the owner's
    /// `ResetsParamsIfMissing()`.
    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets_params_if_missing: bool)
    -> Result<bool, JsonError>;

    /// `ClearUnknownKeys()`.
    fn clear_unknown_keys(&self) -> bool {
        false
    }
}

/// Accessors standing in for the member pointer of a C++ parameter.
pub struct Field<S, T> {
    /// Shared access (`*m_ptr` read).
    pub get: fn(&S) -> &T,
    /// Mutable access (`*m_ptr = ...`).
    pub get_mut: fn(&mut S) -> &mut T,
}

impl<S, T> Clone for Field<S, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S, T> Copy for Field<S, T> {}

/// `Field { get: |s| &s.member, get_mut: |s| &mut s.member }`.
#[macro_export]
macro_rules! settings_field {
    ($($member:tt)+) => {
        $crate::settings_parameters::Field { get: |s| &s.$($member)+, get_mut: |s| &mut s.$($member)+ }
    };
}

/// Value types of `PARAM<T>`: how `JSON_SETTINGS::Get<T>` reads them and how `==` compares
/// them.
pub trait ParamValue: Clone + PartialOrd + ToJson + 'static {
    /// `JSON_SETTINGS::Get<T>( path )`.
    fn get_setting(settings: &JsonSettingsInternals, path: &str) -> Result<Option<Self>, JsonError>;

    /// `operator==`.
    fn same(&self, other: &Self) -> bool;
}

macro_rules! generic_param_value {
    ($($t:ty),*) => {$(
        impl ParamValue for $t {
            fn get_setting(settings: &JsonSettingsInternals, path: &str) -> Result<Option<Self>, JsonError> {
                settings.get::<$t>(path)
            }

            #[allow(clippy::float_cmp)]
            fn same(&self, other: &Self) -> bool {
                self == other
            }
        }
    )*};
}

generic_param_value!(bool, i32, u32, i64, u64, f64, f32);

impl ParamValue for String {
    /// `JSON_SETTINGS::Get<wxString>` (a non-string value throws out of `Get`) - the same for
    /// `std::string` parameters in KiCad's use, which only ever hold strings.
    fn get_setting(settings: &JsonSettingsInternals, path: &str) -> Result<Option<Self>, JsonError> {
        settings.get_wx_string(path)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

/// `PARAM<nlohmann::json>`: compared with nlohmann's `operator==`.
#[derive(Clone, Debug)]
pub struct JsonValue(pub Value);

impl PartialEq for JsonValue {
    fn eq(&self, other: &Self) -> bool {
        json_eq(&self.0, &other.0)
    }
}

impl PartialOrd for JsonValue {
    fn partial_cmp(&self, _other: &Self) -> Option<std::cmp::Ordering> {
        None
    }
}

impl ToJson for JsonValue {
    fn to_json(&self) -> Value {
        self.0.clone()
    }
}

impl ParamValue for JsonValue {
    fn get_setting(settings: &JsonSettingsInternals, path: &str) -> Result<Option<Self>, JsonError> {
        Ok(settings.get_json(path)?.map(JsonValue))
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }
}

// ------------------------------------------------------------------------------------------

/// Port of `PARAM<ValueType>`.
pub struct ParamT<S, T: ParamValue> {
    path: String,
    read_only: bool,
    min_max: Option<(T, T)>,
    field: Field<S, T>,
    default: T,
}

impl<S, T: ParamValue> ParamT<S, T> {
    /// `PARAM( path, ptr, default, readOnly = false )`.
    pub fn new(path: &str, field: Field<S, T>, default: T) -> Self {
        Self { path: path.to_string(), read_only: false, min_max: None, field, default }
    }

    /// `PARAM( path, ptr, default, min, max, readOnly = false )`.
    pub fn with_range(path: &str, field: Field<S, T>, default: T, min: T, max: T) -> Self {
        Self { path: path.to_string(), read_only: false, min_max: Some((min, max)), field, default }
    }

    /// Sets `m_readOnly`.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// `GetDefault()`.
    pub fn default_value(&self) -> &T {
        &self.default
    }
}

impl<S, T: ParamValue> Param<S> for ParamT<S, T> {
    fn json_path(&self) -> &str {
        &self.path
    }

    /// Port of `PARAM::Load`: an out-of-range value (`m_max < val || val < m_min`) becomes the
    /// default; a missing / unconvertible value assigns the default only when resetting.
    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset_if_missing: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        if let Some(mut val) = T::get_setting(settings, &self.path)? {
            if let Some((min, max)) = &self.min_max {
                if *max < val || val < *min {
                    val = self.default.clone();
                }
            }

            *(self.field.get_mut)(target) = val;
        } else if reset_if_missing {
            *(self.field.get_mut)(target) = self.default.clone();
        }

        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        settings.set(&self.path, (self.field.get)(target))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default.clone();
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(v) = T::get_setting(settings, &self.path)? {
            return Ok(v.same((self.field.get)(target)));
        }

        Ok(resets && (self.field.get)(target).same(&self.default))
    }
}

/// `PARAM_PATH::toFileFormat`: backslashes become forward slashes.
fn to_file_format(s: &str) -> String {
    s.replace('\\', "/")
}

/// Port of `PARAM_PATH` (`PARAM<wxString>` storing paths with forward slashes; reading is
/// identity on Linux, `fromFileFormat` only converts on Windows).
pub struct ParamPath<S> {
    inner: ParamT<S, String>,
}

impl<S> ParamPath<S> {
    /// `PARAM_PATH( path, ptr, default, readOnly = false )`.
    pub fn new(path: &str, field: Field<S, String>, default: &str) -> Self {
        Self { inner: ParamT::new(path, field, default.to_string()) }
    }
}

impl<S> Param<S> for ParamPath<S> {
    fn json_path(&self) -> &str {
        &self.inner.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        self.inner.load(settings, target, reset)
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        settings.set(&self.inner.path, &to_file_format((self.inner.field.get)(target)))
    }

    fn set_default(&self, target: &mut S) {
        self.inner.set_default(target);
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(v) = settings.get_wx_string(&self.inner.path)? {
            return Ok(&v == (self.inner.field.get)(target));
        }

        Ok(resets && *(self.inner.field.get)(target) == self.inner.default)
    }
}

/// Port of `PARAM_ENUM<EnumType>`: stored as `int`, loaded only when within `[min, max]`.
pub struct ParamEnum<S, E: Copy + PartialEq + 'static> {
    path: String,
    read_only: bool,
    field: Field<S, E>,
    min: i32,
    max: i32,
    default: E,
    to_int: fn(E) -> i32,
    from_int: fn(i32) -> E,
}

impl<S, E: Copy + PartialEq + 'static> ParamEnum<S, E> {
    /// `PARAM_ENUM( path, ptr, default, min, max )`; `to_int` / `from_int` are the
    /// `static_cast`s between the enum and `int`.
    pub fn new(path: &str, field: Field<S, E>, default: E, min: E, max: E, to_int: fn(E) -> i32, from_int: fn(i32) -> E) -> Self {
        Self { path: path.to_string(), read_only: false, field, min: to_int(min), max: to_int(max), default, to_int, from_int }
    }
}

impl<S, E: Copy + PartialEq + 'static> Param<S> for ParamEnum<S, E> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        if let Some(v) = settings.get::<i32>(&self.path)? {
            if v >= self.min && v <= self.max {
                *(self.field.get_mut)(target) = (self.from_int)(v);
            } else if reset {
                *(self.field.get_mut)(target) = self.default;
            }
        } else if reset {
            *(self.field.get_mut)(target) = self.default;
        }

        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        settings.set(&self.path, &(self.to_int)(*(self.field.get)(target)))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default;
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(v) = settings.get::<i32>(&self.path)? {
            return Ok(v == (self.to_int)(*(self.field.get)(target)));
        }

        Ok(resets && *(self.field.get)(target) == self.default)
    }
}

/// Port of `PARAM_LAMBDA<ValueType>`: getter/setter pair; `Load` **ignores** the reset flag and
/// always calls the setter (with the default when the key is missing or unconvertible).
pub struct ParamLambda<S, T: ParamValue> {
    path: String,
    read_only: bool,
    default: T,
    getter: Box<dyn Fn(&S) -> T>,
    setter: Box<dyn Fn(&mut S, T)>,
}

impl<S, T: ParamValue> ParamLambda<S, T> {
    /// `PARAM_LAMBDA( path, getter, setter, default, readOnly = false )`. For
    /// `T = nlohmann::json` use [`JsonValue`]; C++'s `{}` default is a null value.
    pub fn new(path: &str, getter: impl Fn(&S) -> T + 'static, setter: impl Fn(&mut S, T) + 'static, default: T) -> Self {
        Self { path: path.to_string(), read_only: false, default, getter: Box::new(getter), setter: Box::new(setter) }
    }
}

impl<S, T: ParamValue> Param<S> for ParamLambda<S, T> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, _reset: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        match T::get_setting(settings, &self.path)? {
            Some(v) => (self.setter)(target, v),
            None => (self.setter)(target, self.default.clone()),
        }

        Ok(())
    }

    /// Port of `PARAM_LAMBDA::Store` (exceptions swallowed).
    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        let _ = settings.set(&self.path, &(self.getter)(target));
        Ok(())
    }

    fn set_default(&self, target: &mut S) {
        (self.setter)(target, self.default.clone());
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(v) = T::get_setting(settings, &self.path)? {
            return Ok(v.same(&(self.getter)(target)));
        }

        if !resets {
            return Ok(false);
        }

        let current = (self.getter)(target);

        if current.same(&self.default) {
            return Ok(true);
        }

        // if constexpr( ValueType == nlohmann::json ): current.empty() && default null/empty.
        let current_json = current.to_json();
        let default_json = self.default.to_json();

        if std::any::TypeId::of::<T>() == std::any::TypeId::of::<JsonValue>() {
            return Ok(json_empty(&current_json) && (default_json.is_null() || json_empty(&default_json)));
        }

        Ok(false)
    }
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

/// Integer types of `PARAM_SCALED<T>` (`int`, `long`).
pub trait ScaledValue: Copy + PartialOrd + 'static {
    /// `KiROUND<double, T>( v )`.
    fn ki_round(v: f64) -> Self;
    /// Conversion to `double` in `m_default / m_invScale`.
    fn to_f64(self) -> f64;
}

impl ScaledValue for i32 {
    fn ki_round(v: f64) -> Self {
        ki_round_i32(v)
    }

    fn to_f64(self) -> f64 {
        f64::from(self)
    }
}

impl ScaledValue for i64 {
    fn ki_round(v: f64) -> Self {
        ki_round_i64(v)
    }

    fn to_f64(self) -> f64 {
        self as f64
    }
}

/// Port of `PARAM_SCALED<ValueType>`: stored as `value / invScale` (a double, e.g. mm),
/// loaded as `KiROUND( file * invScale )`, then range-checked against the integer limits.
pub struct ParamScaled<S, T: ScaledValue> {
    path: String,
    read_only: bool,
    field: Field<S, T>,
    default: T,
    min_max: Option<(T, T)>,
    inv_scale: f64,
}

impl<S, T: ScaledValue> ParamScaled<S, T> {
    /// `PARAM_SCALED( path, ptr, default, scale = 1.0 )`.
    pub fn new(path: &str, field: Field<S, T>, default: T, scale: f64) -> Self {
        Self { path: path.to_string(), read_only: false, field, default, min_max: None, inv_scale: 1.0 / scale }
    }

    /// `PARAM_SCALED( path, ptr, default, min, max, scale = 1.0 )`.
    pub fn with_range(path: &str, field: Field<S, T>, default: T, min: T, max: T, scale: f64) -> Self {
        Self { path: path.to_string(), read_only: false, field, default, min_max: Some((min, max)), inv_scale: 1.0 / scale }
    }
}

impl<S, T: ScaledValue> Param<S> for ParamScaled<S, T> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        let mut dval = self.default.to_f64() / self.inv_scale;

        if let Some(v) = settings.get::<f64>(&self.path)? {
            dval = v;
        } else if !reset {
            return Ok(());
        }

        let mut val = T::ki_round(dval * self.inv_scale);

        if let Some((min, max)) = self.min_max {
            if val > max || val < min {
                val = self.default;
            }
        }

        *(self.field.get_mut)(target) = val;
        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        settings.set(&self.path, &((self.field.get)(target).to_f64() / self.inv_scale))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default;
    }

    #[allow(clippy::float_cmp)]
    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(v) = settings.get::<f64>(&self.path)? {
            return Ok(v == (self.field.get)(target).to_f64() / self.inv_scale);
        }

        Ok(resets && (self.field.get)(target).to_f64() == self.default.to_f64())
    }
}

/// Port of `PARAM_LIST<Type>`: an array; an element that does not convert aborts the load
/// (target unchanged); a non-array value gives an empty list.
pub struct ParamList<S, T: FromJson + ToJson + Clone + PartialEq + 'static> {
    path: String,
    field: Field<S, Vec<T>>,
    default: Vec<T>,
    reset_if_empty: bool,
    path_format: bool,
}

impl<S, T: FromJson + ToJson + Clone + PartialEq + 'static> ParamList<S, T> {
    /// `PARAM_LIST( path, ptr, default, resetIfEmpty = false )`.
    pub fn new(path: &str, field: Field<S, Vec<T>>, default: Vec<T>, reset_if_empty: bool) -> Self {
        Self { path: path.to_string(), field, default, reset_if_empty, path_format: false }
    }

    fn read(&self, js: &Value) -> Result<Vec<T>, JsonError> {
        let mut val = Vec::new();

        if let Value::Array(items) = js {
            for el in items {
                val.push(T::from_json(el)?);
            }
        }

        Ok(val)
    }
}

impl<S> ParamList<S, String> {
    /// Port of `PARAM_PATH_LIST`: a `PARAM_LIST<wxString>` storing paths with forward slashes.
    pub fn path_list(path: &str, field: Field<S, Vec<String>>, default: Vec<String>) -> Self {
        Self { path: path.to_string(), field, default, reset_if_empty: false, path_format: true }
    }
}

impl<S, T: FromJson + ToJson + Clone + PartialEq + 'static> Param<S> for ParamList<S, T> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        if let Some(js) = settings.get_json(&self.path)? {
            let val = self.read(&js)?;

            if val.is_empty() && self.reset_if_empty {
                *(self.field.get_mut)(target) = self.default.clone();
            } else {
                *(self.field.get_mut)(target) = val;
            }
        } else if reset {
            *(self.field.get_mut)(target) = self.default.clone();
        }

        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        let items: Vec<Value> = (self.field.get)(target)
            .iter()
            .map(|el| {
                let v = el.to_json();

                match (&v, self.path_format) {
                    (Value::String(s), true) => Value::String(to_file_format(s)),
                    _ => v,
                }
            })
            .collect();

        settings.set(&self.path, &Value::Array(items))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default.clone();
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(js) = settings.get_json(&self.path)? {
            if let Value::Array(items) = &js {
                let mut val = Vec::new();

                for el in items {
                    if self.path_format {
                        // PARAM_PATH_LIST::MatchesFile: no try, a non-string throws.
                        val.push(T::from_json(el)?);
                    } else if let Ok(v) = T::from_json(el) {
                        // "Probably typecast didn't work; skip this element"
                        val.push(v);
                    }
                }

                return Ok(val == *(self.field.get)(target));
            }
        }

        Ok(resets && *(self.field.get)(target) == self.default)
    }
}

/// Port of `PARAM_SET<Type>` (`std::set`, here a `BTreeSet`: `wxString`'s `operator<` compares
/// code points, which is the byte order of UTF-8).
pub struct ParamSet<S, T: FromJson + ToJson + Ord + Clone + 'static> {
    path: String,
    read_only: bool,
    field: Field<S, BTreeSet<T>>,
    default: BTreeSet<T>,
}

impl<S, T: FromJson + ToJson + Ord + Clone + 'static> ParamSet<S, T> {
    /// `PARAM_SET( path, ptr, default, readOnly = false )`.
    pub fn new(path: &str, field: Field<S, BTreeSet<T>>, default: BTreeSet<T>) -> Self {
        Self { path: path.to_string(), read_only: false, field, default }
    }

    fn read(js: &Value) -> Result<BTreeSet<T>, JsonError> {
        let mut val = BTreeSet::new();

        if let Value::Array(items) = js {
            for el in items {
                val.insert(T::from_json(el)?);
            }
        }

        Ok(val)
    }
}

impl<S, T: FromJson + ToJson + Ord + Clone + 'static> Param<S> for ParamSet<S, T> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        if let Some(js) = settings.get_json(&self.path)? {
            *(self.field.get_mut)(target) = Self::read(&js)?;
        } else if reset {
            *(self.field.get_mut)(target) = self.default.clone();
        }

        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        let items: Vec<Value> = (self.field.get)(target).iter().map(ToJson::to_json).collect();
        settings.set(&self.path, &Value::Array(items))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default.clone();
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(js) = settings.get_json(&self.path)? {
            if js.is_array() {
                return Ok(Self::read(&js)? == *(self.field.get)(target));
            }
        }

        Ok(resets && *(self.field.get)(target) == self.default)
    }
}

/// Port of `PARAM_MAP<Value>` and `PARAM_WXSTRING_MAP`: an object; loading clears the target
/// and inserts the members in key order, a member that does not convert aborts the load with
/// the map partially filled; a non-object value leaves the target unchanged.
/// `ClearUnknownKeys()` is true for `PARAM_MAP` and the `aArrayBehavior` flag for
/// `PARAM_WXSTRING_MAP`.
pub struct ParamMap<S, V: FromJson + ToJson + Clone + PartialEq + 'static> {
    path: String,
    read_only: bool,
    field: Field<S, BTreeMap<String, V>>,
    default: BTreeMap<String, V>,
    clear_unknown_keys: bool,
}

impl<S, V: FromJson + ToJson + Clone + PartialEq + 'static> ParamMap<S, V> {
    /// `PARAM_MAP( path, ptr, default, readOnly = false )`.
    pub fn new(path: &str, field: Field<S, BTreeMap<String, V>>, default: BTreeMap<String, V>) -> Self {
        Self { path: path.to_string(), read_only: false, field, default, clear_unknown_keys: true }
    }

    fn read_into(js: &Value, out: &mut BTreeMap<String, V>) -> Result<(), JsonError> {
        if let Value::Object(map) = js {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));

            for (k, v) in entries {
                let converted = V::from_json(v)?;
                out.insert(k.clone(), converted);
            }
        }

        Ok(())
    }
}

impl<S> ParamMap<S, String> {
    /// `PARAM_WXSTRING_MAP( path, ptr, default, readOnly = false, arrayBehavior = false )`.
    pub fn wx_string_map(
        path: &str,
        field: Field<S, BTreeMap<String, String>>,
        default: BTreeMap<String, String>,
        array_behavior: bool,
    ) -> Self {
        Self { path: path.to_string(), read_only: false, field, default, clear_unknown_keys: array_behavior }
    }
}

impl<S, V: FromJson + ToJson + Clone + PartialEq + 'static> Param<S> for ParamMap<S, V> {
    fn json_path(&self) -> &str {
        &self.path
    }

    fn load(&self, settings: &JsonSettingsInternals, target: &mut S, reset: bool) -> Result<(), JsonError> {
        if self.read_only {
            return Ok(());
        }

        if let Some(js) = settings.get_json(&self.path)? {
            if js.is_object() {
                let map = (self.field.get_mut)(target);
                map.clear();
                Self::read_into(&js, map)?;
            }
        } else if reset {
            *(self.field.get_mut)(target) = self.default.clone();
        }

        Ok(())
    }

    fn store(&self, settings: &mut JsonSettingsInternals, target: &S) -> Result<(), JsonError> {
        let obj: crate::json::Map<String, Value> =
            (self.field.get)(target).iter().map(|(k, v)| (k.clone(), v.to_json())).collect();
        settings.set(&self.path, &Value::Object(obj))
    }

    fn set_default(&self, target: &mut S) {
        *(self.field.get_mut)(target) = self.default.clone();
    }

    fn matches_file(&self, settings: &JsonSettingsInternals, target: &S, resets: bool) -> Result<bool, JsonError> {
        if let Some(js) = settings.get_json(&self.path)? {
            if let Value::Object(obj) = &js {
                if (self.field.get)(target).len() != obj.len() {
                    return Ok(false);
                }

                let mut val = BTreeMap::new();
                Self::read_into(&js, &mut val)?;
                return Ok(val == *(self.field.get)(target));
            }
        }

        Ok(resets && *(self.field.get)(target) == self.default)
    }

    fn clear_unknown_keys(&self) -> bool {
        self.clear_unknown_keys
    }
}
