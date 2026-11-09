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

//! Port of `pcbnew/pcbexpr_evaluator.h` / `pcbnew/pcbexpr_evaluator.cpp`.
//!
//! Ported here (package DRC-20): the unit resolvers (`PCBEXPR_UNIT_RESOLVER`,
//! `PCBEXPR_UNITLESS_RESOLVER`), `PCBEXPR_COMPILER` (a `kicad_libeval::Compiler` with one of
//! them) and `PCBEXPR_EVALUATOR` (constant expressions such as the `(min 0.2mm + 1mil)` values
//! of rule files), plus the trait boundary [`PcbExprBinder`] through which the rule parser,
//! [`crate::rule_condition::DrcRuleCondition`] and [`PcbExprEvaluator`] use the PCB binding.
//!
//! Not ported yet (package DRC-21): the binding itself, i.e. `PCBEXPR_UCODE::CreateVarRef` /
//! `CreateFuncCall`, `PCBEXPR_CONTEXT`, `PCBEXPR_VAR_REF` and the value classes. [`PcbExprUcode`]
//! is its placeholder: everything that does not reference an item, a property or a function
//! (all constraint values of real rule files) works through it already; anything else ends in
//! a `port_pending!` panic until DRC-21 lands.
//!
//! # How DRC-21 plugs in
//!
//! The binding is a `kicad_libeval::UcodeBinder` (the virtual methods of `PCBEXPR_UCODE`,
//! with `PCBEXPR_CONTEXT`'s members as its `Env`) that also implements [`PcbExprBinder`]
//! (the usage flags of `PCBEXPR_UCODE` and the two ways a `PCBEXPR_CONTEXT` is constructed).
//! Every user is generic over it with [`PcbExprUcode`] as the default, so DRC-21 replaces
//! the body of `PcbExprUcode` (or its `Env` / `VarRef` / `Custom` types) and nothing else
//! changes. Compiled code is type-erased into [`crate::rule_condition::CompiledCondition`].

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{EdaDataType, EdaUnits, PcbLayerId};
use kicad_geom::math::util::ki_round;
use kicad_libeval::eda_units::{PCB_IU_SCALE, double_value_from_string_with_units};
use kicad_libeval::eda_units::EdaDataType as LibevalDataType;
use kicad_libeval::{
    Compiler, Context, ErrorStatus, FuncCallRef, NoCustomValue, NoVarRef, Ucode, UcodeBinder,
    UnitResolver, VarType,
};
use kicad_pcb::ItemRef;

use crate::constraint_types::DrcConstraintT;

/// The usage flags and context constructors of the PCB binding (`PCBEXPR_UCODE`,
/// `PCBEXPR_CONTEXT`); see the module documentation.
pub trait PcbExprBinder: UcodeBinder + Default + Send + Sync
where
    Self::VarRef: Send + Sync,
{
    /// Port of `PCBEXPR_UCODE::HasGeometryDependentFunctions`.
    fn has_geometry_dependent_functions(&self) -> bool;

    /// Port of `PCBEXPR_UCODE::RequiresPairItems`: the expression references `B` or `AB`.
    fn requires_pair_items(&self) -> bool;

    /// Port of `PCBEXPR_UCODE::ReferencesItemB`.
    fn references_item_b(&self) -> bool;

    /// `PCBEXPR_CONTEXT( aConstraint, aLayer )`: a context without items, as used for the
    /// compile-time preflight and for constant expressions.
    fn new_env<'e>(constraint: i32, layer: PcbLayerId) -> Self::Env<'e>;

    /// The evaluation context of `DRC_RULE_CONDITION::EvaluateFor`:
    /// `PCBEXPR_CONTEXT( aConstraint, aLayer )`, the teardrop type overrides of both items
    /// ("treat teardrop areas as tracks for DRC rule matching") and `SetItems( a, b )`.
    fn item_env<'e>(
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
    ) -> Self::Env<'e>;
}

/// Convert the evaluator's unit enum to the `kicad-core` one (same C++ enum, two ports).
pub fn to_core_units(units: kicad_libeval::EdaUnits) -> EdaUnits {
    EdaUnits::ALL[units.as_i32() as usize]
}

/* --------------------------------------------------------------------------------------------
 * Unit Resolvers
 */

/// Port of `PCBEXPR_UNIT_RESOLVER`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PcbExprUnitResolver {
    // This little hack is required because the first several versions which supported
    // track angle constraints required their values to be specified with no units --
    // though they were interpreted as degrees.
    implied_degrees: bool,
}

impl PcbExprUnitResolver {
    /// `PCBEXPR_UNIT_RESOLVER( bool aImpliedDegrees = false )`.
    pub fn new(implied_degrees: bool) -> Self {
        PcbExprUnitResolver { implied_degrees }
    }
}

/// `wxAtof`: `strtod` in the C locale (the reference runs with `LC_ALL=C.UTF-8`); 0 if the
/// text does not start with a number.
fn wx_atof(text: &str) -> f64 {
    kicad_sexpr::lexer::c_numeric::strtod_prefix(text.as_bytes()).map_or(0.0, |(v, _)| v)
}

impl UnitResolver for PcbExprUnitResolver {
    /// Port of `PCBEXPR_UNIT_RESOLVER::GetSupportedUnits`.
    fn supported_units(&self) -> &[&str] {
        &["mil", "mm", "in", "deg", "fs", "ps"]
    }

    /// Port of `PCBEXPR_UNIT_RESOLVER::GetSupportedUnitsTypes`.
    fn supported_units_types(&self) -> &[kicad_libeval::EdaUnits] {
        use kicad_libeval::EdaUnits as U;

        &[U::Mils, U::Mm, U::Inch, U::Degrees, U::Fs, U::Ps]
    }

    /// Port of `PCBEXPR_UNIT_RESOLVER::GetSupportedUnitsMessage`.
    fn supported_units_message(&self) -> String {
        if self.implied_degrees {
            return String::new();
        }

        "must be mm, in, mil, deg, fs, or ps".to_string()
    }

    /// Port of `PCBEXPR_UNIT_RESOLVER::Convert`.
    fn convert(&self, string: &str, unit_id: i32) -> f64 {
        use kicad_libeval::EdaUnits as U;

        let v = wx_atof(string);
        let iu = |units: U| {
            double_value_from_string_with_units(
                &PCB_IU_SCALE,
                units,
                string,
                LibevalDataType::Distance,
            )
        };

        match unit_id {
            0 => iu(U::Mils),
            1 => iu(U::Mm),
            2 => iu(U::Inch),
            3 => v,
            4 => iu(U::Fs),
            5 => iu(U::Ps),
            _ => v,
        }
    }
}

/// Port of `PCBEXPR_UNITLESS_RESOLVER`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PcbExprUnitlessResolver;

impl UnitResolver for PcbExprUnitlessResolver {
    /// Port of `PCBEXPR_UNITLESS_RESOLVER::GetSupportedUnits` (none).
    fn supported_units(&self) -> &[&str] {
        &[]
    }

    /// Port of `PCBEXPR_UNITLESS_RESOLVER::GetSupportedUnitsTypes` (none).
    fn supported_units_types(&self) -> &[kicad_libeval::EdaUnits] {
        &[]
    }

    /// Port of `PCBEXPR_UNITLESS_RESOLVER::Convert`.
    fn convert(&self, string: &str, _unit_id: i32) -> f64 {
        wx_atof(string)
    }
}

/// Port of `PCBEXPR_COMPILER::PCBEXPR_COMPILER( LIBEVAL::UNIT_RESOLVER* aUnitResolver )`.
pub fn pcbexpr_compiler<'a>(unit_resolver: Box<dyn UnitResolver + 'a>) -> Compiler<'a> {
    Compiler::with_unit_resolver(unit_resolver)
}

/* --------------------------------------------------------------------------------------------
 * PCB Expression Evaluator
 */

/// Port of `PCBEXPR_EVALUATOR`: evaluates a constant expression (no items) to a number.
pub struct PcbExprEvaluator<'a, B: PcbExprBinder = PcbExprUcode>
where
    B::VarRef: Send + Sync,
{
    result: i32,
    result_as_double: f64,
    units: EdaUnits,
    compiler: Compiler<'a>,
    error_status: ErrorStatus,
    _binder: std::marker::PhantomData<fn() -> B>,
}

impl<'a, B: PcbExprBinder> PcbExprEvaluator<'a, B>
where
    B::VarRef: Send + Sync,
{
    /// Port of `PCBEXPR_EVALUATOR::PCBEXPR_EVALUATOR( LIBEVAL::UNIT_RESOLVER* aUnitResolver )`.
    pub fn new(unit_resolver: Box<dyn UnitResolver + 'a>) -> Self {
        PcbExprEvaluator {
            result: 0,
            result_as_double: 0.0,
            units: EdaUnits::Mm,
            compiler: pcbexpr_compiler(unit_resolver),
            error_status: ErrorStatus::default(),
            _binder: std::marker::PhantomData,
        }
    }

    /// Port of `PCBEXPR_EVALUATOR::Evaluate`: false if the expression does not parse; the
    /// result is only updated when the expression evaluates to a number.
    pub fn evaluate(&mut self, expr: &str) -> bool {
        let mut ucode: Ucode<B> = Ucode::new(B::default());
        let mut preflight_context: Context<'_, B> =
            Context::new(B::new_env(DrcConstraintT::NullConstraint as i32, PcbLayerId::FCu));

        if !self
            .compiler
            .compile(expr, &mut ucode, &mut preflight_context)
        {
            return false;
        }

        let mut evaluation_context: Context<'_, B> =
            Context::new(B::new_env(DrcConstraintT::NullConstraint as i32, PcbLayerId::FCu));
        let result = ucode.run(&mut evaluation_context);
        let value = evaluation_context.value(result);

        if value.type_() == VarType::Numeric {
            self.result_as_double = value.as_double(&evaluation_context);
            self.result = ki_round(self.result_as_double);
            self.units = to_core_units(value.units());
        }

        true
    }

    /// Port of `PCBEXPR_EVALUATOR::Result`.
    pub fn result(&self) -> i32 {
        self.result
    }

    /// Port of `PCBEXPR_EVALUATOR::ResultAsDouble`: the unrounded result, for constraints
    /// whose value is a ratio rather than a count or a length.
    pub fn result_as_double(&self) -> f64 {
        self.result_as_double
    }

    /// Port of `PCBEXPR_EVALUATOR::Units`.
    pub fn units(&self) -> EdaUnits {
        self.units
    }

    /// Port of `PCBEXPR_EVALUATOR::SetErrorCallback`.
    pub fn set_error_callback(&mut self, callback: Option<Box<dyn FnMut(&str, i32) + 'a>>) {
        self.compiler.set_error_callback(callback);
    }

    /// Port of `PCBEXPR_EVALUATOR::IsErrorPending` (C++ never updates `m_errorStatus`, so this
    /// is always false; errors arrive through the error callback).
    pub fn is_error_pending(&self) -> bool {
        self.error_status.pending_error
    }

    /// Port of `PCBEXPR_EVALUATOR::GetError`.
    pub fn error(&self) -> &ErrorStatus {
        &self.error_status
    }
}

/// The data type of a unit: `UNITS_PROVIDER::GetTypeFromUnits`.
pub fn type_from_units(units: EdaUnits) -> EdaDataType {
    UnitsProvider::type_from_units(units)
}

/* --------------------------------------------------------------------------------------------
 * PCBEXPR_UCODE / PCBEXPR_CONTEXT placeholder (package DRC-21)
 */

/// Placeholder of `PCBEXPR_CONTEXT`: the members DRC-21 will give meaning to.
pub struct PcbExprContext<'e> {
    /// `m_constraint`.
    pub constraint: i32,
    /// `m_layer`.
    pub layer: PcbLayerId,
    /// `m_items`.
    pub items: [Option<ItemRef<'e>>; 2],
}

/// Placeholder of `PCBEXPR_UCODE` (package DRC-21): constant expressions work, variable
/// references and function calls are `port_pending!`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PcbExprUcode {
    has_geometry_dependent_functions: bool,
    requires_pair_items: bool,
    references_item_b: bool,
}

impl UcodeBinder for PcbExprUcode {
    type Env<'e> = PcbExprContext<'e>;
    type Custom<'e> = NoCustomValue;
    type VarRef = NoVarRef;

    /// `PCBEXPR_UCODE::CreateVarRef` (package DRC-21).
    fn create_var_ref(&mut self, var: &str, field: &str) -> Option<NoVarRef> {
        crate::port_pending!("PCBEXPR_UCODE::CreateVarRef (DRC-21)", var, field)
    }

    /// `PCBEXPR_UCODE::CreateFuncCall` (package DRC-21).
    fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<Self>> {
        crate::port_pending!("PCBEXPR_UCODE::CreateFuncCall (DRC-21)", name)
    }
}

impl PcbExprBinder for PcbExprUcode {
    fn has_geometry_dependent_functions(&self) -> bool {
        self.has_geometry_dependent_functions
    }

    fn requires_pair_items(&self) -> bool {
        self.requires_pair_items
    }

    fn references_item_b(&self) -> bool {
        self.references_item_b
    }

    fn new_env<'e>(constraint: i32, layer: PcbLayerId) -> PcbExprContext<'e> {
        PcbExprContext {
            constraint,
            layer,
            items: [None, None],
        }
    }

    fn item_env<'e>(
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
    ) -> PcbExprContext<'e> {
        crate::port_pending!(
            "PCBEXPR_CONTEXT teardrop type overrides (DRC-21)",
            a,
            b,
            constraint,
            layer
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval(expr: &str, unitless: bool, implied_degrees: bool) -> (bool, i32, f64, EdaUnits) {
        let resolver: Box<dyn UnitResolver> = if unitless {
            Box::new(PcbExprUnitlessResolver)
        } else {
            Box::new(PcbExprUnitResolver::new(implied_degrees))
        };
        let mut evaluator: PcbExprEvaluator<'_> = PcbExprEvaluator::new(resolver);
        let ok = evaluator.evaluate(expr);
        (
            ok,
            evaluator.result(),
            evaluator.result_as_double(),
            evaluator.units(),
        )
    }

    #[test]
    fn constant_values() {
        assert_eq!(eval("0.2mm", false, false).1, 200_000);
        assert_eq!(eval("0.2mm", false, false).3, EdaUnits::Mm);
        assert_eq!(eval("10mil", false, false).1, 254_000);
        assert_eq!(eval("1in", false, false).3, EdaUnits::Inch);
        assert_eq!(eval("45deg", false, false).1, 45);
        assert_eq!(eval("100ps", false, false).1, 100_000_000);
        assert_eq!(eval("2", true, false).1, 2);
        assert_eq!(eval("0.25mm + 1mil", false, false).1, 275_400);
        assert!(!eval("0.2mm +", false, false).0);
    }
}
