// This file is part of libeval, a simple math expression evaluator
//
// Copyright (C) 2007 Michael Geselbracht, mgeselbracht3@gmail.com
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

//! Port of `LIBEVAL::VALUE` from `include/libeval_compiler/libeval_compiler.h` and
//! `common/libeval_compiler/libeval_compiler.cpp`.

use std::cell::{Cell, OnceCell};
use std::fmt;

use super::context::Context;
use super::ucode::UcodeBinder;
use super::VarType;
use crate::crt;
use crate::eda_units::EdaUnits;
use crate::string_utils::wild_compare_string;

/// The overridable part of `LIBEVAL::VALUE`.
///
/// C++ clients subclass `VALUE` and override `AsString`, `EqualTo` and `NotEqualTo`
/// (pcbnew: `PCBEXPR_LAYER_VALUE`, `PCBEXPR_PINTYPE_VALUE`, `PCBEXPR_NETCLASS_VALUE`,
/// `PCBEXPR_COMPONENT_CLASS_VALUE`, `PCBEXPR_NET_VALUE`).  In Rust a value optionally carries
/// a payload of the binder's [`UcodeBinder::Custom`] type (typically an enum with one variant
/// per C++ subclass) and the payload's implementation of this trait plays the role of the
/// overrides.  A C++ `dynamic_cast` of the other operand becomes a `match` on
/// [`Value::custom`].
///
/// Every method defaults to the base class behaviour.  (`VALUE::AsDouble` is virtual in C++
/// as well, but no subclass overrides it, so it has no hook.)
pub trait CustomValue<'e, B: UcodeBinder>: Sized {
    /// Override of `VALUE::AsString()`.  Return `None` to use the base implementation.
    ///
    /// The result is cached in the value: the C++ overrides recompute the same string on
    /// every call (`PCBEXPR_NETCLASS_VALUE::AsString` stores it with `Set()`).
    fn as_string(&self, _value: &Value<'e, B>, _ctx: &Context<'e, B>) -> Option<String> {
        None
    }

    /// Override of `VALUE::EqualTo( CONTEXT* aCtx, const VALUE* b )`.
    fn equal_to(&self, value: &Value<'e, B>, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        value.base_equal_to(ctx, b)
    }

    /// Override of `VALUE::NotEqualTo( CONTEXT* aCtx, const VALUE* b )`.
    fn not_equal_to(&self, value: &Value<'e, B>, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        value.base_not_equal_to(ctx, b)
    }
}

/// The [`UcodeBinder::Custom`] type of binders that do not specialise values: it has no
/// instances.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoCustomValue {}

impl<'e, B: UcodeBinder> CustomValue<'e, B> for NoCustomValue {}

/// A deferred numeric evaluation (C++: `INPLACE_FUNCTION<double()>`).
type LambdaDbl<'e, B> = Box<dyn Fn(&Context<'e, B>) -> f64 + 'e>;

/// A deferred string evaluation (C++: `INPLACE_FUNCTION<wxString()>`).
type LambdaStr<'e, B> = Box<dyn Fn(&Context<'e, B>) -> String + 'e>;

/// Port of `class LIBEVAL::VALUE`: a string, numeric, null or undefined value on the
/// evaluation stack.
///
/// Differences in shape from the C++ class (behaviour is the same):
///
/// * `AsDouble()` / `AsString()` / `EqualTo()` take the [`Context`], because deferred
///   evaluations and the [`CustomValue`] hooks receive the context as an argument instead of
///   capturing a `CONTEXT*` (C++ lambdas capture `aCtx` and other `VALUE*`s; a Rust closure
///   captures [`super::ValueId`]s and whatever it needs from the environment and is handed
///   the context when it runs).
/// * the virtual methods are replaced by the optional [`CustomValue`] payload.
pub struct Value<'e, B: UcodeBinder> {
    type_: VarType,
    units: EdaUnits,
    /// `m_valueDbl` (mutable to support deferred evaluation)
    value_dbl: Cell<f64>,
    /// `m_valueStr` as long as no deferred string evaluation has replaced it
    value_str: String,
    lambda_dbl: Option<LambdaDbl<'e, B>>,
    lambda_str: Option<LambdaStr<'e, B>>,
    /// The result of `lambda_str`, i.e. `m_valueStr` after `m_isDeferredStr` was cleared.
    deferred_str: OnceCell<String>,
    string_is_wildcard: bool,
    string_is_literal: bool,
    /// `m_isDeferredDbl`
    is_deferred_dbl: Cell<bool>,
    custom: Option<B::Custom<'e>>,
    /// Cache of `CustomValue::as_string`.
    custom_str: OnceCell<String>,
}

impl<'e, B: UcodeBinder> Default for Value<'e, B> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'e, B: UcodeBinder> Value<'e, B> {
    /// Port of `VALUE::VALUE()`: an undefined value.
    pub fn new() -> Self {
        Value {
            type_: VarType::Undefined,
            units: EdaUnits::Unscaled,
            value_dbl: Cell::new(0.0),
            value_str: String::new(),
            lambda_dbl: None,
            lambda_str: None,
            deferred_str: OnceCell::new(),
            string_is_wildcard: false,
            string_is_literal: false,
            is_deferred_dbl: Cell::new(false),
            custom: None,
            custom_str: OnceCell::new(),
        }
    }

    /// Port of `VALUE::VALUE( const wxString& aStr, bool aIsWildcard = false )`.
    ///
    /// Note that this constructor is the only place that sets `m_stringIsLiteral`.
    pub fn from_string(s: impl Into<String>, is_wildcard: bool) -> Self {
        Value {
            type_: VarType::String,
            value_str: s.into(),
            string_is_wildcard: is_wildcard,
            string_is_literal: true,
            ..Self::new()
        }
    }

    /// Port of `VALUE::VALUE( const double aVal )`.
    pub fn from_double(val: f64) -> Self {
        Value { type_: VarType::Numeric, value_dbl: Cell::new(val), ..Self::new() }
    }

    /// Port of `VALUE::MakeNullValue()`.
    pub fn make_null_value() -> Self {
        Value { type_: VarType::Null, ..Self::new() }
    }

    /// Attach the payload that makes this value an instance of a "subclass" (see
    /// [`CustomValue`]).  C++: constructing a class derived from `VALUE`.
    pub fn with_custom(mut self, custom: B::Custom<'e>) -> Self {
        self.custom = Some(custom);
        self
    }

    /// The payload attached with [`Value::with_custom`], if any.  C++: `dynamic_cast` to a
    /// class derived from `VALUE`.
    pub fn custom(&self) -> Option<&B::Custom<'e>> {
        self.custom.as_ref()
    }

    /// Port of `VALUE::AsDouble()`.
    pub fn as_double(&self, ctx: &Context<'e, B>) -> f64 {
        if self.is_deferred_dbl.get() {
            if let Some(lambda) = &self.lambda_dbl {
                self.value_dbl.set(lambda(ctx));
            }

            self.is_deferred_dbl.set(false);
        }

        self.value_dbl.get()
    }

    /// Port of `VALUE::AsString()` including the dispatch to an overriding subclass
    /// ([`CustomValue::as_string`]).
    pub fn as_string<'a>(&'a self, ctx: &Context<'e, B>) -> &'a str {
        if let Some(custom) = &self.custom {
            if let Some(s) = self.custom_str.get() {
                return s;
            }

            if let Some(s) = custom.as_string(self, ctx) {
                return self.custom_str.get_or_init(|| s);
            }
        }

        self.base_as_string(ctx)
    }

    /// Port of the base class implementation `LIBEVAL::VALUE::AsString()`.
    pub fn base_as_string<'a>(&'a self, ctx: &Context<'e, B>) -> &'a str {
        if let Some(lambda) = &self.lambda_str {
            return self.deferred_str.get_or_init(|| lambda(ctx));
        }

        &self.value_str
    }

    /// Port of `VALUE::EqualTo( CONTEXT* aCtx, const VALUE* b )` including the dispatch to an
    /// overriding subclass ([`CustomValue::equal_to`]).
    pub fn equal_to(&self, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        match &self.custom {
            Some(custom) => custom.equal_to(self, ctx, b),
            None => self.base_equal_to(ctx, b),
        }
    }

    /// Port of the base class implementation `LIBEVAL::VALUE::EqualTo`.
    pub fn base_equal_to(&self, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        if self.type_ == VarType::Undefined || b.type_ == VarType::Undefined {
            return false;
        }

        if self.type_ == VarType::Null && b.type_ == VarType::Null {
            return true;
        }

        if self.type_ == VarType::Numeric && b.type_ == VarType::Numeric {
            return self.as_double(ctx) == b.as_double(ctx);
        } else if self.type_ == VarType::String && b.type_ == VarType::String {
            if b.string_is_wildcard {
                return wild_compare_string(b.as_string(ctx), self.as_string(ctx), false);
            } else {
                return crt::is_same_as_no_case(self.as_string(ctx), b.as_string(ctx));
            }
        }

        false
    }

    /// Port of `VALUE::NotEqualTo( CONTEXT* aCtx, const VALUE* b )` including the dispatch to
    /// an overriding subclass ([`CustomValue::not_equal_to`]).
    ///
    /// NB: this is not an inverse of EqualTo as they both return false for undefined values.
    pub fn not_equal_to(&self, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        match &self.custom {
            Some(custom) => custom.not_equal_to(self, ctx, b),
            None => self.base_not_equal_to(ctx, b),
        }
    }

    /// Port of the base class implementation `LIBEVAL::VALUE::NotEqualTo`.
    pub fn base_not_equal_to(&self, ctx: &Context<'e, B>, b: &Value<'e, B>) -> bool {
        if self.type_ == VarType::Undefined || b.type_ == VarType::Undefined {
            return false;
        }

        // EqualTo is a virtual call in C++, so an overriding subclass is honoured here.
        !self.equal_to(ctx, b)
    }

    /// Port of `VALUE::GetType()`.
    pub fn type_(&self) -> VarType {
        self.type_
    }

    /// Port of `VALUE::Set( double aValue )`.
    pub fn set_double(&mut self, value: f64) {
        self.type_ = VarType::Numeric;
        self.value_dbl.set(value);
    }

    /// Port of `VALUE::SetDeferredEval( INPLACE_FUNCTION<double()> aLambda )`: make the value
    /// numeric and compute it only when (and if) it is read.
    pub fn set_deferred_eval_double(&mut self, lambda: impl Fn(&Context<'e, B>) -> f64 + 'e) {
        self.type_ = VarType::Numeric;
        self.lambda_dbl = Some(Box::new(lambda));
        self.is_deferred_dbl.set(true);
    }

    /// Port of `VALUE::SetDeferredEval( INPLACE_FUNCTION<wxString()> aLambda )`: make the
    /// value a string and compute it only when (and if) it is read.
    pub fn set_deferred_eval_string(&mut self, lambda: impl Fn(&Context<'e, B>) -> String + 'e) {
        self.type_ = VarType::String;
        self.lambda_str = Some(Box::new(lambda));
        self.deferred_str = OnceCell::new();
    }

    /// Port of `VALUE::Set( const wxString& aValue )`.
    pub fn set_string(&mut self, value: impl Into<String>) {
        self.type_ = VarType::String;

        // C++ assigns m_valueStr and leaves m_isDeferredStr alone: a pending deferred
        // evaluation still replaces the string later, a finished one does not.
        if self.lambda_str.is_some() && self.deferred_str.get().is_some() {
            self.lambda_str = None;
            self.deferred_str = OnceCell::new();
        }

        self.value_str = value.into();
    }

    /// Port of `VALUE::Set( const VALUE& val )`.
    ///
    /// As in C++ this copies the type, the raw numeric value, the wildcard flag, the units
    /// and (for strings) the raw string, but neither `m_stringIsLiteral` nor a deferred
    /// evaluation nor a [`CustomValue`] payload.
    pub fn set_value(&mut self, val: &Value<'e, B>) {
        self.type_ = val.type_;
        self.value_dbl.set(val.value_dbl.get());
        self.string_is_wildcard = val.string_is_wildcard;
        self.units = val.units;

        if self.type_ == VarType::String {
            self.assign_raw_str(val.raw_str().to_string());
        }
    }

    /// `Set( const VALUE& )` applied to a temporary `VALUE( aStr, aIsWildcard )`, which is
    /// what `UOP::Exec` does to push a per-evaluation copy of a string literal.
    pub(crate) fn set_string_copy(&mut self, s: &str, is_wildcard: bool) {
        self.type_ = VarType::String;
        self.value_dbl.set(0.0);
        self.string_is_wildcard = is_wildcard;
        self.units = EdaUnits::Unscaled;
        self.assign_raw_str(s.to_string());
    }

    /// The raw member `m_valueStr`.
    fn raw_str(&self) -> &str {
        // An overriding AsString() stores its result with Set() in C++.
        if let Some(s) = self.custom_str.get() {
            return s;
        }

        match (&self.lambda_str, self.deferred_str.get()) {
            (Some(_), Some(s)) => s,
            _ => &self.value_str,
        }
    }

    /// `m_valueStr = ...`
    fn assign_raw_str(&mut self, s: String) {
        if self.lambda_str.is_some() && self.deferred_str.get().is_some() {
            self.lambda_str = None;
            self.deferred_str = OnceCell::new();
        }

        self.value_str = s;
    }

    /// Port of `VALUE::SetUnits`.
    pub fn set_units(&mut self, units: EdaUnits) {
        self.units = units;
    }

    /// Port of `VALUE::GetUnits`.
    pub fn units(&self) -> EdaUnits {
        self.units
    }

    /// Port of `VALUE::StringIsWildcard`.
    pub fn string_is_wildcard(&self) -> bool {
        self.string_is_wildcard
    }

    /// Port of `VALUE::StringIsLiteral`.
    pub fn string_is_literal(&self) -> bool {
        self.string_is_literal
    }
}

impl<'e, B: UcodeBinder> fmt::Debug for Value<'e, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Value")
            .field("type", &self.type_)
            .field("units", &self.units)
            .field("value_dbl", &self.value_dbl.get())
            .field("value_str", &self.raw_str())
            .field("string_is_wildcard", &self.string_is_wildcard)
            .field("string_is_literal", &self.string_is_literal)
            .field("is_deferred_dbl", &self.is_deferred_dbl.get())
            .field(
                "is_deferred_str",
                &(self.lambda_str.is_some() && self.deferred_str.get().is_none()),
            )
            .field("has_custom", &self.custom.is_some())
            .finish()
    }
}
