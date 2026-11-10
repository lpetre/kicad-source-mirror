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
//! Package DRC-20: the unit resolvers (`PCBEXPR_UNIT_RESOLVER`, `PCBEXPR_UNITLESS_RESOLVER`),
//! `PCBEXPR_COMPILER` (a `kicad_libeval::Compiler` with one of them) and `PCBEXPR_EVALUATOR`
//! (constant expressions such as the `(min 0.2mm + 1mil)` values of rule files), plus the
//! trait boundary [`PcbExprBinder`] through which the rule parser,
//! [`crate::rule_condition::DrcRuleCondition`] and [`PcbExprEvaluator`] use the PCB binding.
//!
//! Package DRC-21: the binding itself. [`PcbExprUcode`] (`PCBEXPR_UCODE::CreateVarRef` /
//! `CreateFuncCall`) is a `kicad_libeval::UcodeBinder` whose `Env` is [`PcbExprContext`]
//! (`PCBEXPR_CONTEXT`), whose variable references are [`PcbExprVarRef`] (`PCBEXPR_VAR_REF` and
//! its "object code" subclasses) and whose specialised values are [`PcbExprValue`]
//! (`PCBEXPR_LAYER_VALUE`, `PCBEXPR_PINTYPE_VALUE`, `PCBEXPR_NETCLASS_VALUE`,
//! `PCBEXPR_COMPONENT_CLASS_VALUE`, `PCBEXPR_NET_VALUE`). Properties are looked up in the
//! property tables of `kicad_pcb::properties`. [`PcbExprBuiltinFunctions`] is the function
//! registry; the function bodies live in [`super::functions`] (`pcbexpr_functions.cpp`).
//! Compiled code is type-erased into [`crate::rule_condition::CompiledCondition`].

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use kicad_core::eda_units::ui;
use kicad_core::layer_ids::{layer_name, to_layer_id};
use kicad_core::string_utils::{wild_compare_string, wx_string_matches};
use kicad_core::units_provider::UnitsProvider;
use kicad_core::wxstr::{self, cmp_no_case, is_same_as_no_case};
use kicad_core::{EdaDataType, EdaUnits, KicadT, Lset, PcbLayerId};
use kicad_geom::math::util::ki_round;
use kicad_libeval::eda_units::EdaDataType as LibevalDataType;
use kicad_libeval::eda_units::{PCB_IU_SCALE, double_value_from_string_with_units};
use kicad_libeval::{
    Compiler, Context, CustomValue, ErrorStatus, FuncCallRef, Ucode, UcodeBinder, UnitResolver,
    Value, VarRef, VarType,
};
use kicad_pcb::ItemRef;
use kicad_pcb::board::Board;
use kicad_pcb::component_class::ComponentClass;
use kicad_pcb::netclass::Netclass;
use kicad_pcb::properties::{
    ClassId, EnumId, PropId, Property, PropertyValue, ValueType, class_of, property_manager,
};

use crate::constraint_types::DrcConstraintT;
use crate::rule_condition::before_first;
use crate::rule_parser::{LayerChoices, board_layer_choices, default_layer_choices};

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
        let mut preflight_context: Context<'_, B> = Context::new(B::new_env(
            DrcConstraintT::NullConstraint as i32,
            PcbLayerId::FCu,
        ));

        if !self
            .compiler
            .compile(expr, &mut ucode, &mut preflight_context)
        {
            return false;
        }

        let mut evaluation_context: Context<'_, B> = Context::new(B::new_env(
            DrcConstraintT::NullConstraint as i32,
            PcbLayerId::FCu,
        ));
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
 * Specialized Expression References (package DRC-21)
 */

/// The evaluation values of the PCB binding (`LIBEVAL::VALUE<'e>` with the PCB payload).
type PcbValue<'e> = Value<'e, PcbExprUcode>;

/// The evaluation context of the PCB binding.
type PcbContext<'e> = Context<'e, PcbExprUcode>;

/// Port of `PCBEXPR_NAV_STEP`: a navigation step applied to a variable reference before its
/// property or method is resolved. "A.Parent.getField('x')" resolves item A, then steps to
/// its parent before calling getField().
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcbExprNavStep {
    /// `PCBEXPR_NAV_STEP::PARENT`.
    Parent,
}

/// Port of `PCBEXPR_PROPERTY_KIND`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcbExprPropertyKind {
    /// `UNSUPPORTED`.
    Unsupported,
    /// `INT_KIND`.
    IntKind,
    /// `OPTIONAL_INT`.
    OptionalInt,
    /// `UNSIGNED`.
    Unsigned,
    /// `LONG_LONG`.
    LongLong,
    /// `DOUBLE`.
    Double,
    /// `OPTIONAL_DOUBLE`.
    OptionalDouble,
    /// `BOOL_KIND`.
    BoolKind,
    /// `STRING`.
    String,
    /// `ENUM`.
    Enum,
    /// `ANGLE`.
    Angle,
    /// `COLOR`.
    Color,
}

/// Port of `PCBEXPR_CONTEXT`: the items, the layer and the constraint an expression is
/// evaluated for.
#[derive(Clone, Debug)]
pub struct PcbExprContext<'e> {
    /// `m_constraint`.
    constraint: i32,
    /// `m_items`.
    items: [Option<ItemRef<'e>>; 2],
    /// `m_layer`.
    layer: PcbLayerId,
    /// `m_typeOverrides` (a `std::map` keyed by item pointer; at most two entries).
    type_overrides: Vec<(ItemRef<'e>, KicadT)>,
}

impl<'e> PcbExprContext<'e> {
    /// `PCBEXPR_CONTEXT( int aConstraint = 0, PCB_LAYER_ID aLayer = F_Cu )`.
    pub fn new(constraint: i32, layer: PcbLayerId) -> Self {
        PcbExprContext {
            constraint,
            items: [None, None],
            layer,
            type_overrides: Vec::new(),
        }
    }

    /// Port of `PCBEXPR_CONTEXT::SetItems`.
    pub fn set_items(&mut self, a: Option<ItemRef<'e>>, b: Option<ItemRef<'e>>) {
        self.items = [a, b];
    }

    /// Port of `PCBEXPR_CONTEXT::SetConstraint`.
    pub fn set_constraint(&mut self, constraint: i32) {
        self.constraint = constraint;
    }

    /// Port of `PCBEXPR_CONTEXT::SetLayer`.
    pub fn set_layer(&mut self, layer: PcbLayerId) {
        self.layer = layer;
    }

    /// Port of `PCBEXPR_CONTEXT::Reset` (the client part; the caller resets the
    /// `LIBEVAL::CONTEXT` with `Context::reset`).
    pub fn reset(&mut self) {
        self.constraint = 0;
        self.layer = PcbLayerId::FCu;
        self.items = [None, None];
        self.type_overrides.clear();
    }

    /// Port of `PCBEXPR_CONTEXT::SetTypeOverride`.
    pub fn set_type_override(&mut self, item: ItemRef<'e>, type_: KicadT) {
        match self.type_overrides.iter_mut().find(|(i, _)| *i == item) {
            Some(entry) => entry.1 = type_,
            None => self.type_overrides.push((item, type_)),
        }
    }

    /// Port of `PCBEXPR_CONTEXT::GetEffectiveType`.
    pub fn effective_type(&self, item: ItemRef<'_>) -> KicadT {
        self.type_overrides
            .iter()
            .find(|(i, _)| *i == item)
            .map_or_else(|| item.type_(), |(_, t)| *t)
    }

    /// Port of `PCBEXPR_CONTEXT::GetBoard`: the board of item A (`None` without an item A or
    /// when it is not attached to a board).
    pub fn board(&self) -> Option<&'e Board> {
        self.items[0]
            .filter(|item| item.has_board())
            .map(|item| item.ctx.board)
    }

    /// Port of `PCBEXPR_CONTEXT::GetConstraint`.
    pub fn constraint(&self) -> i32 {
        self.constraint
    }

    /// Port of `PCBEXPR_CONTEXT::GetItem`. C++ reads `m_items[index]` unchecked; an index
    /// outside 0..2 (the layer pseudo-item `L` used as a function receiver) yields `None`.
    pub fn item(&self, index: i32) -> Option<ItemRef<'e>> {
        usize::try_from(index)
            .ok()
            .and_then(|i| self.items.get(i).copied().flatten())
    }

    /// Port of `PCBEXPR_CONTEXT::GetLayer`.
    pub fn layer(&self) -> PcbLayerId {
        self.layer
    }
}

/// Which C++ class a [`PcbExprVarRef`] is: `PCBEXPR_VAR_REF` or one of the "object code"
/// subclasses for the very common fields.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcbExprRefClass {
    /// `PCBEXPR_VAR_REF`: a property looked up through the property tables.
    VarRef,
    /// `PCBEXPR_NETCLASS_REF`.
    NetclassRef,
    /// `PCBEXPR_COMPONENT_CLASS_REF`.
    ComponentClassRef,
    /// `PCBEXPR_NETNAME_REF`.
    NetnameRef,
    /// `PCBEXPR_TYPE_REF`.
    TypeRef,
}

/// `PCBEXPR_VAR_REF::MATCHED_PROPERTY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MatchedProperty {
    property: PropId,
    kind: PcbExprPropertyKind,
}

/// Port of `PCBEXPR_VAR_REF` and its subclasses `PCBEXPR_NETCLASS_REF`,
/// `PCBEXPR_COMPONENT_CLASS_REF`, `PCBEXPR_NETNAME_REF` and `PCBEXPR_TYPE_REF`
/// (see [`PcbExprRefClass`]).
#[derive(Clone, Debug)]
pub struct PcbExprVarRef {
    /// The C++ class of the reference.
    class: PcbExprRefClass,
    /// `m_matchingTypes` (an `unordered_map`; only looked up, never iterated).
    matching_types: BTreeMap<ClassId, MatchedProperty>,
    /// `m_itemIndex`: 0 = A, 1 = B, 2 = the layer `L`.
    item_index: i32,
    /// `m_type`.
    type_: VarType,
    /// `m_navigation`.
    navigation: Vec<PcbExprNavStep>,
}

impl PcbExprVarRef {
    /// `PCBEXPR_VAR_REF( int aItemIndex )` (type undefined) and the subclass constructors
    /// (type string).
    pub fn new(class: PcbExprRefClass, item_index: i32) -> Self {
        PcbExprVarRef {
            class,
            matching_types: BTreeMap::new(),
            item_index,
            type_: match class {
                PcbExprRefClass::VarRef => VarType::Undefined,
                _ => VarType::String,
            },
            navigation: Vec::new(),
        }
    }

    /// The C++ class of the reference.
    pub fn ref_class(&self) -> PcbExprRefClass {
        self.class
    }

    /// `m_itemIndex`.
    pub fn item_index(&self) -> i32 {
        self.item_index
    }

    /// Port of `PCBEXPR_VAR_REF::ClassifyProperty`.
    pub fn classify_property(property: &Property) -> PcbExprPropertyKind {
        match property.value_type {
            ValueType::Int => PcbExprPropertyKind::IntKind,
            ValueType::OptInt => PcbExprPropertyKind::OptionalInt,
            ValueType::Unsigned => PcbExprPropertyKind::Unsigned,
            ValueType::LongLong => PcbExprPropertyKind::LongLong,
            ValueType::Double => PcbExprPropertyKind::Double,
            ValueType::OptDouble => PcbExprPropertyKind::OptionalDouble,
            ValueType::Bool => PcbExprPropertyKind::BoolKind,
            ValueType::String => PcbExprPropertyKind::String,
            _ if property.has_choices() => PcbExprPropertyKind::Enum,
            ValueType::Angle => PcbExprPropertyKind::Angle,
            ValueType::Color => PcbExprPropertyKind::Color,
            _ => PcbExprPropertyKind::Unsupported,
        }
    }

    /// Port of `PCBEXPR_VAR_REF::ExpressionType`.
    pub fn expression_type(kind: PcbExprPropertyKind) -> VarType {
        use PcbExprPropertyKind as K;

        match kind {
            K::IntKind | K::OptionalInt | K::Unsigned | K::LongLong | K::BoolKind => {
                VarType::Numeric
            }
            K::Double | K::OptionalDouble | K::Angle => VarType::NumericDouble,
            K::String | K::Enum | K::Color => VarType::String,
            K::Unsupported => VarType::ParseError,
        }
    }

    /// Port of `PCBEXPR_VAR_REF::SetType`.
    pub fn set_type(&mut self, type_: VarType) {
        self.type_ = type_;
    }

    /// Port of `PCBEXPR_VAR_REF::AddAllowedClass`.
    fn add_allowed_class(&mut self, type_hash: ClassId, property: PropId, kind: PcbExprPropertyKind) {
        self.matching_types
            .insert(type_hash, MatchedProperty { property, kind });
    }

    /// Port of `PCBEXPR_VAR_REF::SetNavigation`.
    pub fn set_navigation(&mut self, navigation: Vec<PcbExprNavStep>) {
        self.navigation = navigation;
    }

    /// Port of `PCBEXPR_VAR_REF::GetObject`: the item of the context (A or B) after the
    /// navigation steps.
    pub fn object<'e>(&self, env: &PcbExprContext<'e>) -> Option<ItemRef<'e>> {
        let mut item = env.item(self.item_index);

        for step in &self.navigation {
            let Some(current) = item else {
                break;
            };

            match step {
                // The direct parent, matching the semantics of the "Parent" string property so
                // that "A.Parent" used as an object refers to the same item as "A.Parent" used
                // as a string.
                PcbExprNavStep::Parent => item = current.parent(),
            }
        }

        item
    }

    /// Port of `PCBEXPR_VAR_REF::GetValue`.
    fn var_value<'e>(&self, env: &PcbExprContext<'e>) -> PcbValue<'e> {
        if self.type_ == VarType::Null {
            return PcbValue::make_null_value();
        }

        if self.item_index == 2 {
            return layer_value(env.layer());
        }

        let Some(mut item) = self.object(env) else {
            return PcbValue::new();
        };

        let mut it = class_of(item).and_then(|c| self.matching_types.get(&c).copied());

        if it.is_none() {
            // If the property isn't defined on the item itself but is defined on its parent
            // footprint (e.g. Reference, Value), resolve against the parent so that conditions
            // like "A.Reference == 'J1'" match pads and graphics belonging to J1.
            if let Some(parent_fp) = item.parent_footprint() {
                let parent_it =
                    class_of(parent_fp).and_then(|c| self.matching_types.get(&c).copied());

                if parent_it.is_some() {
                    item = parent_fp;
                    it = parent_it;
                }
            }
        }

        let Some(matched) = it else {
            // Don't force user to type "A.Type == 'via' && A.Via_Type == 'buried'" when the
            // simpler "A.Via_Type == 'buried'" is perfectly clear.  Instead, return an undefined
            // value when the property doesn't appear on a particular object.
            return PcbValue::new();
        };

        let pm = property_manager();
        let property = pm.property(matched.property);

        // `TYPE_HASH( *item )` is known to be a matching class here.
        let Some(item_class) = class_of(item) else {
            return PcbValue::new();
        };

        if !pm.is_available_for(item_class, matched.property, item) {
            return PcbValue::new();
        }

        if item.type_() == KicadT::PcbFootprintT && item.has_board() {
            let variant = &item.ctx.board.current_variant;

            if !variant.is_empty() {
                if let Some(fp) = item.as_footprint() {
                    let name = property.name;
                    let flag = match name {
                        "Do not Populate" => Some(fp.dnp_for_variant(variant)),
                        "Exclude From Bill of Materials" => {
                            Some(fp.excluded_from_bom_for_variant(variant))
                        }
                        "Exclude From Simulation" => Some(fp.excluded_from_sim_for_variant(variant)),
                        "Exclude From Position Files" => {
                            Some(fp.excluded_from_pos_files_for_variant(variant))
                        }
                        _ => None,
                    };

                    if let Some(flag) = flag {
                        return PcbValue::from_double(f64::from(u8::from(flag)));
                    }
                }
            }
        }

        let value = property.get(item);

        match matched.kind {
            PcbExprPropertyKind::IntKind => match value {
                PropertyValue::Int(v) | PropertyValue::Enum(_, v) => {
                    PcbValue::from_double(f64::from(v))
                }
                _ => mistyped(property),
            },

            PcbExprPropertyKind::OptionalInt => match value {
                PropertyValue::OptInt(Some(v)) => PcbValue::from_double(f64::from(v)),
                PropertyValue::OptInt(None) => PcbValue::make_null_value(),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::Unsigned => match value {
                PropertyValue::Unsigned(v) => PcbValue::from_double(f64::from(v)),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::LongLong => match value {
                PropertyValue::LongLong(v) => PcbValue::from_double(v as f64),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::Double => match value {
                PropertyValue::Double(v) => PcbValue::from_double(v),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::OptionalDouble => match value {
                PropertyValue::OptDouble(Some(v)) => PcbValue::from_double(v),
                PropertyValue::OptDouble(None) => PcbValue::make_null_value(),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::BoolKind => match value {
                PropertyValue::Bool(v) => PcbValue::from_double(f64::from(u8::from(v))),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::String => {
                let PropertyValue::String(str) = value else {
                    return mistyped(property);
                };

                if property.name == "Pin Type" {
                    return PcbValue::from_string(str, false).with_custom(PcbExprValue::PinType);
                }

                // If it quacks like a duck, it is a duck
                if let Some(double_val) =
                    ui::double_value_from_string_with_units(&kicad_core::PCB_IU_SCALE, &str)
                {
                    return PcbValue::from_double(double_val);
                }

                PcbValue::from_string(str, false)
            }

            PcbExprPropertyKind::Enum => {
                if property.name == "Layer"
                    || property.name == "Layer Top"
                    || property.name == "Layer Bottom"
                {
                    // any.GetAs<PCB_LAYER_ID>( &layer ), else any.GetAs<wxString>( &str )
                    match &value {
                        PropertyValue::Enum(EnumId::PcbLayerId, v) => {
                            return layer_value(to_layer_id(*v));
                        }
                        PropertyValue::String(str) => {
                            return layer_value(board_layer_id(env, str));
                        }
                        _ => {
                            if let Some(str) = value.enum_as_string() {
                                return layer_value(board_layer_id(env, str));
                            }
                        }
                    }
                } else {
                    match &value {
                        PropertyValue::String(str) => {
                            return PcbValue::from_string(str.clone(), false);
                        }
                        _ => {
                            if let Some(str) = value.enum_as_string() {
                                return PcbValue::from_string(str, false);
                            }
                        }
                    }
                }

                PcbValue::new()
            }

            PcbExprPropertyKind::Angle => match value {
                PropertyValue::Angle(a) => PcbValue::from_double(a.as_degrees()),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::Color => match value {
                PropertyValue::Color(c) => PcbValue::from_string(c.to_css_string(), false),
                _ => mistyped(property),
            },

            PcbExprPropertyKind::Unsupported => PcbValue::new(),
        }
    }
}

/// A getter whose value does not have the C++ type the property is registered with (the
/// C++ `PROPERTY_BASE::get<T>` would throw). Not reachable with the reference tables.
fn mistyped<'e>(property: &Property) -> PcbValue<'e> {
    debug_assert!(false, "property '{}' returned a mistyped value", property.name);
    PcbValue::new()
}

/// `context->GetBoard()->GetLayerID( str )` (`UNDEFINED_LAYER` without a board, where the
/// C++ dereferences null).
fn board_layer_id(env: &PcbExprContext<'_>, name: &str) -> PcbLayerId {
    env.board()
        .map_or(PcbLayerId::UndefinedLayer, |board| board.layer_id(name))
}

/// `new PCBEXPR_LAYER_VALUE( aLayer )`.
fn layer_value<'e>(layer: PcbLayerId) -> PcbValue<'e> {
    PcbValue::from_string(layer_name(layer), false).with_custom(PcbExprValue::Layer(layer))
}

impl VarRef<PcbExprUcode> for PcbExprVarRef {
    /// Port of `PCBEXPR_VAR_REF::GetType`.
    fn type_(&self) -> VarType {
        self.type_
    }

    /// Port of `GetValue` of `PCBEXPR_VAR_REF`, `PCBEXPR_NETCLASS_REF`,
    /// `PCBEXPR_COMPONENT_CLASS_REF`, `PCBEXPR_NETNAME_REF` and `PCBEXPR_TYPE_REF`.
    fn value<'e>(&self, ctx: &mut PcbContext<'e>) -> PcbValue<'e> {
        let env = ctx.env();

        match self.class {
            PcbExprRefClass::VarRef => self.var_value(env),

            // PCBEXPR_NETCLASS_REF::GetValue
            PcbExprRefClass::NetclassRef => match self.object(env) {
                Some(item) if item.as_connected().is_some() => {
                    PcbValue::from_string("", false).with_custom(PcbExprValue::Netclass(item))
                }
                _ => PcbValue::new(),
            },

            // PCBEXPR_COMPONENT_CLASS_REF::GetValue
            PcbExprRefClass::ComponentClassRef => {
                let Some(mut item) = self.object(env) else {
                    return PcbValue::new();
                };

                // Resolve component class via the parent footprint so that conditions like
                // "A.ComponentClass == 'X'" match pads and graphics inside the footprint.
                if item.type_() != KicadT::PcbFootprintT {
                    match item.parent_footprint() {
                        Some(fp) => item = fp,
                        None => return PcbValue::new(),
                    }
                }

                // m_item( dynamic_cast<FOOTPRINT*>( aItem ) )
                let footprint = item.as_footprint().is_some().then_some(item);

                PcbValue::from_string("", false)
                    .with_custom(PcbExprValue::ComponentClass(footprint))
            }

            // PCBEXPR_NETNAME_REF::GetValue
            PcbExprRefClass::NetnameRef => match self.object(env) {
                Some(item) if item.as_connected().is_some() => {
                    PcbValue::from_string("", false).with_custom(PcbExprValue::Net(item))
                }
                _ => PcbValue::new(),
            },

            // PCBEXPR_TYPE_REF::GetValue
            PcbExprRefClass::TypeRef => {
                let Some(item) = self.object(env) else {
                    return PcbValue::new();
                };

                let type_ = env.effective_type(item);

                PcbValue::from_string(EnumId::KicadT.map().to_string(type_ as i32), false)
            }
        }
    }
}

/// The specialised values of the PCB binding (the C++ subclasses of `LIBEVAL::VALUE`).
#[derive(Clone, Copy, Debug)]
pub enum PcbExprValue<'e> {
    /// `PCBEXPR_LAYER_VALUE` (`m_layer`).
    Layer(PcbLayerId),
    /// `PCBEXPR_PINTYPE_VALUE`.
    PinType,
    /// `PCBEXPR_NETCLASS_VALUE` (`m_item`, a connected item).
    Netclass(ItemRef<'e>),
    /// `PCBEXPR_COMPONENT_CLASS_VALUE` (`m_item`, a footprint or null).
    ComponentClass(Option<ItemRef<'e>>),
    /// `PCBEXPR_NET_VALUE` (`m_item`, a connected item).
    Net(ItemRef<'e>),
}

/// `m_item->GetEffectiveNetClass()` of a connected item.
fn effective_netclass<'e>(item: ItemRef<'e>) -> &'e Netclass {
    item.effective_netclass().unwrap_or_else(|| Netclass::fallback())
}

/// The names of `NETCLASS::GetConstituentNetclasses()` (a root class lists itself).
fn constituent_netclass_names(netclass: &Netclass) -> Vec<String> {
    if netclass.constituents.is_empty() {
        vec![netclass.get_name()]
    } else {
        netclass.constituents.iter().map(|nc| nc.get_name()).collect()
    }
}

/// `m_item->GetComponentClass()` of a `PCBEXPR_COMPONENT_CLASS_VALUE`.
fn component_class_of(footprint: Option<ItemRef<'_>>) -> Option<Arc<ComponentClass>> {
    let fp = footprint?;
    fp.as_footprint()?.component_class(fp.ctx)
}

/// The constituent test shared by the netclass and component class values: does a
/// constituent name match the string `b` (wildcard or case-insensitive)?
fn matches_constituent<'e>(names: &[&str], ctx: &PcbContext<'e>, b: &PcbValue<'e>) -> bool {
    for name in names {
        if b.string_is_wildcard() {
            if wild_compare_string(b.as_string(ctx), name, false) {
                return true;
            }
        } else if is_same_as_no_case(name, b.as_string(ctx)) {
            return true;
        }
    }

    false
}

/// Port of `PCBEXPR_LAYER_VALUE::EqualTo`.
fn layer_equal_to<'e>(layer: PcbLayerId, ctx: &PcbContext<'e>, b: &PcbValue<'e>) -> bool {
    // For boards with user-defined layer names there will be 2 entries for each layer
    // in the ENUM_MAP: one for the canonical layer name and one for the user layer name.
    // We need to check against both.

    let layer_name = b.as_string(ctx);

    // BOARD::m_LayerExpressionCache memoises the mask per pattern; the mask is a function of
    // the board's layer names only, so it is recomputed here (the Rust board cache is
    // crate-private to kicad-pcb).
    let mask = match ctx.env().board() {
        Some(board) => layer_mask(&board_layer_choices(board), layer_name),
        // The C++ dereferences the null board; match against the start-up layer map.
        None => layer_mask(&default_layer_choices(), layer_name),
    };

    mask.contains(layer)
}

/// The layers of `layer_map` whose name matches the wildcard pattern `layer_name`.
fn layer_mask(layer_map: &LayerChoices, layer_name: &str) -> Lset {
    let mut mask = Lset::new();

    for (text, layer) in layer_map {
        if wx_string_matches(text, layer_name) {
            mask.set(*layer);
        }
    }

    mask
}

/// Port of `PCBEXPR_PINTYPE_VALUE::EqualTo`.
fn pin_type_equal_to<'e>(value: &PcbValue<'e>, ctx: &PcbContext<'e>, b: &PcbValue<'e>) -> bool {
    let this_str = value.as_string(ctx);
    let other_str = b.as_string(ctx);

    // Case insensitive
    if is_same_as_no_case(this_str, other_str) {
        return true;
    }

    // Wildcards
    if wx_string_matches(this_str, other_str) {
        return true;
    }

    // Handle cases where the netlist token is different from the EEschema token
    let alt_str = match this_str {
        "tri_state" => "Tri-state",
        "power_in" => "Power input",
        "power_out" => "Power output",
        "no_connect" => "Unconnected",
        _ => "",
    };

    if !alt_str.is_empty() {
        // Case insensitive
        if is_same_as_no_case(alt_str, other_str) {
            return true;
        }

        // Wildcards
        if wx_string_matches(alt_str, other_str) {
            return true;
        }
    }

    false
}

impl<'e> CustomValue<'e, PcbExprUcode> for PcbExprValue<'e> {
    /// `AsString` of `PCBEXPR_NETCLASS_VALUE`, `PCBEXPR_COMPONENT_CLASS_VALUE` and
    /// `PCBEXPR_NET_VALUE`.
    fn as_string(&self, _value: &PcbValue<'e>, _ctx: &PcbContext<'e>) -> Option<String> {
        match *self {
            PcbExprValue::Netclass(item) => Some(effective_netclass(item).get_name()),
            PcbExprValue::ComponentClass(item) => {
                component_class_of(item).map(|cc| cc.name().to_string())
            }
            PcbExprValue::Net(item) => Some(item.netname().to_string()),
            PcbExprValue::Layer(_) | PcbExprValue::PinType => None,
        }
    }

    /// `EqualTo` of the five value classes.
    fn equal_to(&self, value: &PcbValue<'e>, ctx: &PcbContext<'e>, b: &PcbValue<'e>) -> bool {
        match *self {
            PcbExprValue::Layer(layer) => layer_equal_to(layer, ctx, b),

            PcbExprValue::PinType => pin_type_equal_to(value, ctx, b),

            PcbExprValue::Netclass(item) => {
                if let Some(PcbExprValue::Netclass(b_item)) = b.custom() {
                    return effective_netclass(item).same_constituents(effective_netclass(*b_item));
                }

                if b.type_() == VarType::String {
                    // Test constituent net class names. The effective net class name (e.g.
                    // CLASS1,CLASS2,OTHER_CLASS) is tested in the fallthrough condition.
                    let names = constituent_netclass_names(effective_netclass(item));
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();

                    if matches_constituent(&names, ctx, b) {
                        return true;
                    }
                }

                value.base_equal_to(ctx, b)
            }

            PcbExprValue::ComponentClass(item) => {
                if let Some(PcbExprValue::ComponentClass(b_item)) = b.custom() {
                    if item.is_none() || b_item.is_none() {
                        return value.base_equal_to(ctx, b);
                    }

                    return component_class_name(item) == component_class_name(*b_item);
                }

                if b.type_() == VarType::String {
                    // Test constituent component class names. The effective component class
                    // name (e.g. CLASS1,CLASS2,OTHER_CLASS) is tested in the fallthrough
                    // condition.
                    if let Some(cc) = component_class_of(item) {
                        if matches_constituent(&cc.constituent_names(), ctx, b) {
                            return true;
                        }
                    }
                }

                value.base_equal_to(ctx, b)
            }

            PcbExprValue::Net(item) => match b.custom() {
                Some(PcbExprValue::Net(b_item)) => item.net_code() == b_item.net_code(),
                _ => value.base_equal_to(ctx, b),
            },
        }
    }

    /// `NotEqualTo` of the value classes that override it (netclass, component class, net);
    /// the others use the base class (`!EqualTo` for defined values).
    fn not_equal_to(&self, value: &PcbValue<'e>, ctx: &PcbContext<'e>, b: &PcbValue<'e>) -> bool {
        match *self {
            PcbExprValue::Netclass(item) => {
                if let Some(PcbExprValue::Netclass(b_item)) = b.custom() {
                    return !effective_netclass(item).same_constituents(effective_netclass(*b_item));
                }

                if b.type_() == VarType::String {
                    // Test constituent net class names
                    let names = constituent_netclass_names(effective_netclass(item));
                    let names: Vec<&str> = names.iter().map(String::as_str).collect();
                    let is_in_constituents = matches_constituent(&names, ctx, b);

                    // Test effective net class name
                    let is_full_name = value.base_equal_to(ctx, b);

                    return !is_in_constituents && !is_full_name;
                }

                value.base_not_equal_to(ctx, b)
            }

            PcbExprValue::ComponentClass(item) => {
                if let Some(PcbExprValue::ComponentClass(b_item)) = b.custom() {
                    if item.is_none() || b_item.is_none() {
                        return value.base_not_equal_to(ctx, b);
                    }

                    return component_class_name(item) != component_class_name(*b_item);
                }

                if b.type_() == VarType::String {
                    // Test constituent component class names
                    let is_in_constituents = component_class_of(item)
                        .is_some_and(|cc| matches_constituent(&cc.constituent_names(), ctx, b));

                    // Test effective component class name
                    let is_full_name = value.base_equal_to(ctx, b);

                    return !is_in_constituents && !is_full_name;
                }

                value.base_not_equal_to(ctx, b)
            }

            PcbExprValue::Net(item) => match b.custom() {
                Some(PcbExprValue::Net(b_item)) => item.net_code() != b_item.net_code(),
                _ => value.base_not_equal_to(ctx, b),
            },

            PcbExprValue::Layer(_) | PcbExprValue::PinType => value.base_not_equal_to(ctx, b),
        }
    }
}

/// `COMPONENT_CLASS::operator==` compares `GetName()`; a footprint without a class compares
/// as the empty name (the C++ dereferences the null class).
fn component_class_name(footprint: Option<ItemRef<'_>>) -> String {
    component_class_of(footprint).map_or_else(String::new, |cc| cc.name().to_string())
}

/* --------------------------------------------------------------------------------------------
 * PCBEXPR_BUILTIN_FUNCTIONS
 */

/// Port of `PCBEXPR_BUILTIN_FUNCTIONS`: the functions callable as `A.name( args )`.
pub struct PcbExprBuiltinFunctions {
    /// `m_funcs` (keyed by the lower-case name).
    funcs: BTreeMap<String, FuncCallRef<PcbExprUcode>>,
    /// `m_geometryDependentFuncs`.
    geometry_dependent_funcs: BTreeSet<String>,
    /// `m_funcSigs`.
    func_sigs: Vec<String>,
}

impl PcbExprBuiltinFunctions {
    /// `PCBEXPR_BUILTIN_FUNCTIONS::PCBEXPR_BUILTIN_FUNCTIONS()`.
    fn new() -> Self {
        let mut functions = PcbExprBuiltinFunctions {
            funcs: BTreeMap::new(),
            geometry_dependent_funcs: BTreeSet::new(),
            func_sigs: Vec::new(),
        };

        functions.register_all_functions();
        functions
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::Instance`.
    pub fn instance() -> &'static PcbExprBuiltinFunctions {
        static INSTANCE: OnceLock<PcbExprBuiltinFunctions> = OnceLock::new();
        INSTANCE.get_or_init(PcbExprBuiltinFunctions::new)
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::Get` (`None` = the empty `std::function` the
    /// C++ `operator[]` yields for an unknown name).
    pub fn get(&self, name: &str) -> Option<FuncCallRef<PcbExprUcode>> {
        self.funcs.get(name).cloned()
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::IsGeometryDependent`.
    pub fn is_geometry_dependent(&self, name: &str) -> bool {
        self.geometry_dependent_funcs.contains(name)
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::GetSignatures`.
    pub fn signatures(&self) -> &[String] {
        &self.func_sigs
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::RegisterFunc`.
    pub fn register_func(
        &mut self,
        func_signature: &str,
        func_ptr: FuncCallRef<PcbExprUcode>,
        is_geometry_dependent: bool,
    ) {
        let func_name = before_first(func_signature, '(').0;
        let lower = wxstr::lower(func_name);
        self.funcs.insert(lower.clone(), func_ptr);
        self.func_sigs.push(func_signature.to_string());

        if is_geometry_dependent {
            self.geometry_dependent_funcs.insert(lower);
        }
    }

    /// Port of `PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions` (`pcbexpr_functions.cpp`).
    pub fn register_all_functions(&mut self) {
        self.funcs.clear();
        super::functions::register_all_functions(self);
    }
}

/* --------------------------------------------------------------------------------------------
 * PCBEXPR_UCODE
 */

/// Port of `PCBEXPR_UCODE`: the PCB binding of the expression language (variables `A`, `B`,
/// `AB`, `L`, `null`, their properties and the built-in functions).
#[derive(Clone, Copy, Debug, Default)]
pub struct PcbExprUcode {
    /// `m_hasGeometryDependentFunctions`.
    has_geometry_dependent_functions: bool,
    /// `m_requiresPairItems`.
    requires_pair_items: bool,
    /// `m_referencesItemB`.
    references_item_b: bool,
}

impl UcodeBinder for PcbExprUcode {
    type Env<'e> = PcbExprContext<'e>;
    type Custom<'e> = PcbExprValue<'e>;
    type VarRef = PcbExprVarRef;

    /// Port of `PCBEXPR_UCODE::CreateVarRef`.
    fn create_var_ref(&mut self, var: &str, field: &str) -> Option<PcbExprVarRef> {
        let prop_mgr = property_manager();

        if is_same_as_no_case(var, "null") {
            let mut vref = PcbExprVarRef::new(PcbExprRefClass::VarRef, 0);
            vref.set_type(VarType::Null);
            return Some(vref);
        }

        // The receiver may be a navigation chain such as "A.Parent".  Split off the base
        // variable and translate each remaining segment into a navigation step.  Only "Parent"
        // is supported.

        let mut base_var = var;
        let mut navigation: Vec<PcbExprNavStep> = Vec::new();

        if var.contains('.') {
            let mut tokens = var.split('.');
            base_var = tokens.next().unwrap_or("");

            for token in tokens {
                if cmp_no_case(token, "Parent") == 0 {
                    navigation.push(PcbExprNavStep::Parent);
                } else {
                    return None;
                }
            }

            // Navigation requires one concrete item; combined-item "AB" and layer "L" have no
            // parent.
            if base_var != "A" && base_var != "B" {
                return None;
            }
        }

        // Existing rules spell the layer under test "L.Layer"
        if base_var == "L" && cmp_no_case(field, "Layer") == 0 {
            return Some(PcbExprVarRef::new(PcbExprRefClass::VarRef, 2));
        }

        if !field.is_empty() && (base_var == "AB" || base_var == "L") {
            return None;
        }

        if base_var == "B" || base_var == "AB" {
            self.requires_pair_items = true;
        }

        if base_var == "B" {
            self.references_item_b = true;
        }

        let with_nav = |mut vref: PcbExprVarRef| {
            if !navigation.is_empty() {
                vref.set_navigation(navigation.clone());
            }

            Some(vref)
        };

        // Check for a couple of very common cases and compile them straight to "object code".

        let special = if cmp_no_case(field, "NetClass") == 0 {
            Some(PcbExprRefClass::NetclassRef)
        } else if cmp_no_case(field, "ComponentClass") == 0 {
            Some(PcbExprRefClass::ComponentClassRef)
        } else if cmp_no_case(field, "NetName") == 0 {
            Some(PcbExprRefClass::NetnameRef)
        } else if cmp_no_case(field, "Type") == 0 {
            Some(PcbExprRefClass::TypeRef)
        } else {
            None
        };

        if let Some(class) = special {
            return match base_var {
                "A" => with_nav(PcbExprVarRef::new(class, 0)),
                "B" => with_nav(PcbExprVarRef::new(class, 1)),
                _ => None,
            };
        }

        let mut vref = match base_var {
            "A" | "AB" => PcbExprVarRef::new(PcbExprRefClass::VarRef, 0),
            "B" => PcbExprVarRef::new(PcbExprRefClass::VarRef, 1),
            "L" => PcbExprVarRef::new(PcbExprRefClass::VarRef, 2),
            _ => return None,
        };

        vref.set_navigation(navigation);

        if field.is_empty() {
            // return reference to base object
            return Some(vref);
        }

        let mut field = field.replace('_', " ");

        // Alias renamed properties so that older custom rules keep working.
        if cmp_no_case(&field, "Origin X") == 0 {
            field = "Start X".to_string();
        } else if cmp_no_case(&field, "Origin Y") == 0 {
            field = "Start Y".to_string();
        }

        for cls in prop_mgr.all_classes() {
            if prop_mgr.is_of_type(cls.type_, ClassId::BoardItem) {
                if let Some(prop) = prop_mgr.get_property(cls.type_, &field) {
                    let kind = PcbExprVarRef::classify_property(prop_mgr.property(prop));
                    let expression_type = PcbExprVarRef::expression_type(kind);

                    if expression_type == VarType::ParseError
                        || (vref.type_ != VarType::Undefined && vref.type_ != expression_type)
                    {
                        vref.set_type(VarType::ParseError);
                        return Some(vref);
                    }

                    vref.add_allowed_class(cls.type_, prop, kind);
                    vref.set_type(expression_type);
                }
            }
        }

        if vref.type_ == VarType::Undefined {
            vref.set_type(VarType::ParseError);
        }

        Some(vref)
    }

    /// Port of `PCBEXPR_UCODE::CreateFuncCall`.
    fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<Self>> {
        let name_lower = wxstr::lower(name);

        let registry = PcbExprBuiltinFunctions::instance();

        if registry.is_geometry_dependent(&name_lower) {
            self.has_geometry_dependent_functions = true;
        }

        registry.get(&name_lower)
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
        PcbExprContext::new(constraint, layer)
    }

    /// The context set-up of `DRC_RULE_CONDITION::EvaluateFor`.
    fn item_env<'e>(
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
    ) -> PcbExprContext<'e> {
        let mut ctx = PcbExprContext::new(constraint, layer);

        // Treat teardrop areas as tracks for DRC rule matching
        for item in [a, b].into_iter().flatten() {
            if item.type_() == KicadT::PcbZoneT
                && item.as_zone().is_some_and(|zone| zone.is_teardrop_area())
            {
                ctx.set_type_override(item, KicadT::PcbTraceT);
            }
        }

        ctx.set_items(a, b);
        ctx
    }
}

/// `A.<field> == '<literal>'` as the compiled expression evaluates it for item `a`: the value
/// of `PCBEXPR_UCODE::CreateVarRef( "A", field )` compared (`EqualTo` of the left operand)
/// with the string literal (a wildcard when it contains `*` or `?`, as the compiler marks
/// it). `false` when the field is not recognised. Used by the native implicit rule
/// conditions of the engine (`A.Layer == 'In1.Cu'` of the tuning-profile rules).
pub fn item_field_equals(
    a: Option<ItemRef<'_>>,
    field: &str,
    literal: &str,
    constraint: i32,
    layer: PcbLayerId,
) -> bool {
    let mut binder = PcbExprUcode::default();

    let Some(vref) = binder
        .create_var_ref("A", field)
        .filter(|v| v.type_() != VarType::ParseError)
    else {
        return false;
    };

    let mut env = PcbExprContext::new(constraint, layer);
    env.set_items(a, None);
    let mut ctx: PcbContext<'_> = Context::new(env);

    let value = vref.value(&mut ctx);
    let is_wildcard = literal.contains('?') || literal.contains('*');
    let other = PcbValue::from_string(literal, is_wildcard);

    value.equal_to(&ctx, &other)
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
