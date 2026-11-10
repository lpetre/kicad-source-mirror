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
// You should have received a copy of the GNU General Public License
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Native evaluation of the conditions of the implicit rules (analysis `drc.md` 4.8.5).
//!
//! `DRC_ENGINE::loadImplicitRules` gives its rules conditions written in the PCB expression
//! language (`A.hasExactNetclass('x')`, `A.Via_Type == 'Micro'`, ...) which the C++ compiles
//! with `PCBEXPR_COMPILER`. The set of shapes is closed, so the engine recognises them and
//! evaluates them natively with the semantics of the compiled form (the bodies of
//! `hasExactNetclassFunc`, `inDiffPairFunc`, `isCoupledDiffPairFunc` of
//! `pcbnew/pcbexpr_functions.cpp`, `PCBEXPR_VAR_REF::GetValue` / `PCBEXPR_TYPE_REF::GetValue` of
//! `pcbnew/pcbexpr_evaluator.cpp` and `LIBEVAL::VALUE::EqualTo` / `NotEqualTo`). This keeps the
//! implicit rules independent of the expression binding (package DRC-21). User rule conditions
//! stay on the compiled path ([`DrcRuleCondition`]).
//!
//! [`ImplicitCondition`] implements the same [`CompiledCondition`] interface as the compiled
//! microcode; [`EngineCondition`] is what the engine's constraint lists hold (C++
//! `DRC_ENGINE_CONSTRAINT::condition`).

use std::sync::Arc;

use kicad_core::{KicadT, PcbLayerId};
use kicad_pcb::ItemRef;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::pcb_track::Viatype;

use crate::engine::diff_pair::match_dp_suffix;
use crate::reporter::{Reporter, reborrow};
use crate::rule_condition::{CompiledCondition, DrcRuleCondition};

/// One conjunct of an implicit rule condition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImplicitAtom {
    /// `A.hasExactNetclass('<name>')` (the unescaped literal).
    HasExactNetclass(String),
    /// `A.inDiffPair('<pattern>')`; the implicit rules only use `'*'`.
    InDiffPair(String),
    /// `AB.isCoupledDiffPair()`.
    IsCoupledDiffPair,
    /// `A.Via_Type == 'Micro'`.
    ViaTypeIsMicro,
    /// `A.Via_Type != 'Micro'`.
    ViaTypeIsNotMicro,
    /// `A.Type == 'Barcode'`.
    TypeIsBarcode,
    /// `A.Layer == '<name>'` (tuning-profile rules).
    LayerIs(String),
    /// `A.intersectsKeepout('<uuid>')` (keepout rules).
    IntersectsKeepout(String),
}

/// A conjunction (`&&`) of [`ImplicitAtom`]s: the condition of an implicit rule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImplicitCondition {
    /// The conjuncts in expression order.
    pub atoms: Vec<ImplicitAtom>,
}

/// `TOKENIZER::GetString`: the string literal starting at `pos` (just after the opening quote),
/// with `\'` read as `'`. Returns the text and the position after the closing quote; `None`
/// when the literal is not closed.
fn read_literal(text: &str, pos: usize) -> Option<(String, usize)> {
    let chars: Vec<(usize, char)> = text[pos..].char_indices().collect();
    let mut rv = String::new();
    let mut i = 0;

    while i < chars.len() && chars[i].1 != '\'' {
        if chars[i].1 == '\\' && i + 1 < chars.len() && chars[i + 1].1 == '\'' {
            i += 1;
        }

        rv.push(chars[i].1);
        i += 1;
    }

    if i >= chars.len() {
        return None;
    }

    Some((rv, pos + chars[i].0 + 1))
}

impl ImplicitCondition {
    /// Recognise the expression text of an implicit rule condition (exactly as
    /// `loadImplicitRules` formats it). `None` for anything outside the closed set; such a
    /// condition takes the compiled path.
    pub fn parse(expression: &str) -> Option<ImplicitCondition> {
        // (prefix, suffix, constructor) of the atoms taking a string literal
        type Ctor = fn(String) -> ImplicitAtom;
        const LITERAL_ATOMS: &[(&str, &str, Ctor)] = &[
            ("A.hasExactNetclass('", ")", ImplicitAtom::HasExactNetclass),
            ("A.inDiffPair('", ")", ImplicitAtom::InDiffPair),
            ("A.Layer == '", "", ImplicitAtom::LayerIs),
            (
                "A.intersectsKeepout('",
                ")",
                ImplicitAtom::IntersectsKeepout,
            ),
        ];
        const PLAIN_ATOMS: &[(&str, ImplicitAtom)] = &[
            ("AB.isCoupledDiffPair()", ImplicitAtom::IsCoupledDiffPair),
            ("A.Via_Type == 'Micro'", ImplicitAtom::ViaTypeIsMicro),
            ("A.Via_Type != 'Micro'", ImplicitAtom::ViaTypeIsNotMicro),
            ("A.Type == 'Barcode'", ImplicitAtom::TypeIsBarcode),
        ];

        let mut atoms = Vec::new();
        let mut pos = 0;

        loop {
            let rest = &expression[pos..];
            let mut matched = false;

            for (text, atom) in PLAIN_ATOMS {
                if rest.starts_with(text) {
                    atoms.push(atom.clone());
                    pos += text.len();
                    matched = true;
                    break;
                }
            }

            if !matched {
                for (prefix, suffix, ctor) in LITERAL_ATOMS {
                    if rest.starts_with(prefix) {
                        let (literal, end) = read_literal(expression, pos + prefix.len())?;

                        if !expression[end..].starts_with(suffix) {
                            return None;
                        }

                        atoms.push(ctor(literal));
                        pos = end + suffix.len();
                        matched = true;
                        break;
                    }
                }
            }

            if !matched {
                return None;
            }

            if pos == expression.len() {
                return Some(ImplicitCondition { atoms });
            }

            if !expression[pos..].starts_with(" && ") {
                return None;
            }

            pos += " && ".len();
        }
    }

    /// The value of one atom for the context items `a` (`A`) and `b` (`B`).
    fn eval_atom(
        atom: &ImplicitAtom,
        a: Option<ItemRef<'_>>,
        b: Option<ItemRef<'_>>,
        constraint: i32,
        reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        match atom {
            ImplicitAtom::HasExactNetclass(name) => has_exact_netclass(a, name, reporter),
            ImplicitAtom::InDiffPair(pattern) => in_diff_pair(a, pattern, reporter),
            ImplicitAtom::IsCoupledDiffPair => is_coupled_diff_pair(a, b, constraint),
            ImplicitAtom::ViaTypeIsMicro => via_type_is_micro(a) == Some(true),
            // LIBEVAL::VALUE::NotEqualTo is false when the property is undefined (not a via)
            ImplicitAtom::ViaTypeIsNotMicro => via_type_is_micro(a) == Some(false),
            ImplicitAtom::TypeIsBarcode => type_is_barcode(a),
            ImplicitAtom::LayerIs(name) => {
                // PCBEXPR_VAR_REF::GetValue for the "Layer" property (property availability,
                // parent-footprint fallback, PCBEXPR_LAYER_VALUE matching) through the
                // expression binding (DRC-21). The context layer does not enter `A.Layer`.
                crate::pcbexpr::evaluator::item_field_equals(
                    a,
                    "Layer",
                    name,
                    constraint,
                    PcbLayerId::FCu,
                )
            }
            ImplicitAtom::IntersectsKeepout(uuid) => {
                crate::port_pending!("intersectsKeepoutFunc (DRC-10)", a, uuid)
            }
        }
    }
}

/// Body of `hasExactNetclassFunc` (`pcbnew/pcbexpr_functions.cpp`).
fn has_exact_netclass(
    item: Option<ItemRef<'_>>,
    arg: &str,
    reporter: Option<&mut dyn Reporter>,
) -> bool {
    if arg.is_empty() {
        if let Some(reporter) = reporter {
            // aCtx->ReportError(): the condition's error callback prefixes "ERROR: "
            reporter.report(
                "ERROR: Missing netclass name argument to hasExactNetclass().",
                kicad_core::Severity::Undefined,
            );
        }

        return false;
    }

    let Some(item) = item else {
        return false;
    };

    if !item.is_connected() {
        return false;
    }

    item.effective_netclass()
        .is_some_and(|netclass| netclass.name_equals(arg))
}

/// Body of `inDiffPairFunc`.
fn in_diff_pair(item: Option<ItemRef<'_>>, arg: &str, reporter: Option<&mut dyn Reporter>) -> bool {
    if arg.is_empty() {
        if let Some(reporter) = reporter {
            reporter.report(
                "ERROR: Missing diff-pair name argument to inDiffPair().",
                kicad_core::Severity::Undefined,
            );
        }

        return false;
    }

    let Some(item) = item else {
        return false;
    };

    if !item.has_board() {
        return false;
    }

    if item.is_connected() {
        let Some(netinfo) = item.net() else {
            return false;
        };

        let (polarity, coupled_net, base_name) = match_dp_suffix(netinfo.netname());

        if polarity != 0 && item.ctx.board.find_net_by_name(&coupled_net).is_some() {
            if kicad_core::string_utils::wx_string_matches(&base_name, arg) {
                return true;
            }

            if base_name.ends_with('_') {
                // baseName.BeforeLast( '_' )
                let before_last = &base_name[..base_name.rfind('_').unwrap_or(0)];

                if kicad_core::string_utils::wx_string_matches(before_last, arg) {
                    return true;
                }
            }
        }
    }

    false
}

/// Body of `isCoupledDiffPairFunc`.
fn is_coupled_diff_pair(a: Option<ItemRef<'_>>, b: Option<ItemRef<'_>>, constraint: i32) -> bool {
    // dynamic_cast<BOARD_CONNECTED_ITEM*>
    let a = a.filter(|a| a.as_connected().is_some());
    let b = b.filter(|b| b.as_connected().is_some());

    let Some(netinfo) = a.and_then(|a| a.net()) else {
        return false;
    };

    let (polarity, coupled_net, _) = match_dp_suffix(netinfo.netname());

    if polarity == 0 {
        return false;
    }

    use DrcConstraintT::*;

    if constraint == DiffPairGapConstraint as i32
        || constraint == LengthConstraint as i32
        || constraint == NetChainLengthConstraint as i32
        || constraint == SkewConstraint as i32
    {
        // DRC engine evaluates these only in the context of a diffpair, but doesn't
        // always supply the second (B) item.
        if let Some(a) = a
            && a.has_board()
            && a.ctx.board.find_net_by_name(&coupled_net).is_some()
        {
            return true;
        }
    }

    // b->GetNetname() == coupledNet
    if let Some(b) = b {
        let netname = b.net().map_or("", |n| n.netname());

        if netname == coupled_net {
            return true;
        }
    }

    false
}

/// `A.Via_Type` compared with `'Micro'`: `None` when the property is undefined for the item
/// (no item, or not a via: `PCBEXPR_VAR_REF::GetValue` returns an undefined value), else whether
/// the via is a microvia (`IsSameAs( "Micro", false )` of the enum choice name).
fn via_type_is_micro(item: Option<ItemRef<'_>>) -> Option<bool> {
    let via = item?.as_via()?;
    Some(via.via_type() == Viatype::Microvia)
}

/// `A.Type == 'Barcode'`: `ENUM_MAP<KICAD_T>::ToString( GetEffectiveType( item ) )` compared
/// case-insensitively; only `PCB_BARCODE_T` maps to "Barcode" and the teardrop type override
/// only concerns zones.
fn type_is_barcode(item: Option<ItemRef<'_>>) -> bool {
    item.is_some_and(|item| item.type_() == KicadT::PcbBarcodeT)
}

impl CompiledCondition for ImplicitCondition {
    fn has_geometry_dependent_functions(&self) -> bool {
        // intersectsKeepout() is registered as geometry dependent
        self.atoms
            .iter()
            .any(|a| matches!(a, ImplicitAtom::IntersectsKeepout(_)))
    }

    fn requires_pair_items(&self) -> bool {
        // the "AB" variable
        self.atoms
            .iter()
            .any(|a| matches!(a, ImplicitAtom::IsCoupledDiffPair))
    }

    fn references_item_b(&self) -> bool {
        false
    }

    fn run<'e>(
        &self,
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        _layer: PcbLayerId,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> f64 {
        // TR_OP_BOOL_AND: the right operand is only evaluated if the left one is true
        for atom in &self.atoms {
            if !Self::eval_atom(atom, a, b, constraint, reborrow(&mut reporter)) {
                return 0.0;
            }
        }

        1.0
    }
}

/// `DRC_ENGINE_CONSTRAINT::condition`: the compiled condition of a user rule, or the native
/// form of an implicit rule's condition. Both keep the expression text (`GetExpression()`, the
/// memo key of `EvalRules`).
#[derive(Clone, Debug)]
pub enum EngineCondition {
    /// A condition on the compiled path (`DRC_RULE_CONDITION` with `PCBEXPR_UCODE`).
    Compiled(Arc<DrcRuleCondition>),
    /// A recognised implicit rule condition.
    Implicit {
        /// `GetExpression()`.
        expression: String,
        /// The native form.
        condition: Arc<ImplicitCondition>,
    },
}

impl EngineCondition {
    /// Port of `DRC_RULE_CONDITION::GetExpression`.
    pub fn expression(&self) -> &str {
        match self {
            EngineCondition::Compiled(c) => c.expression(),
            EngineCondition::Implicit { expression, .. } => expression,
        }
    }

    /// Port of `DRC_RULE_CONDITION::EvaluateFor`: an empty expression is true; conditions are
    /// commutative, so if `(a, b)` does not match and there is a `b`, `(b, a)` is tried.
    pub fn evaluate_for<'e>(
        &self,
        item_a: Option<ItemRef<'e>>,
        item_b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        match self {
            EngineCondition::Compiled(c) => {
                c.evaluate_for(item_a, item_b, constraint, layer, reporter)
            }
            EngineCondition::Implicit {
                expression,
                condition,
            } => {
                if expression.is_empty() {
                    return true;
                }

                if condition.run(item_a, item_b, constraint, layer, reborrow(&mut reporter)) != 0.0
                {
                    return true;
                } else if item_b.is_some() {
                    // Conditions are commutative
                    if condition.run(item_b, item_a, constraint, layer, reporter) != 0.0 {
                        return true;
                    }
                }

                false
            }
        }
    }

    /// Port of `DRC_RULE_CONDITION::HasGeometryDependentFunctions`.
    pub fn has_geometry_dependent_functions(&self) -> bool {
        match self {
            EngineCondition::Compiled(c) => c.has_geometry_dependent_functions(),
            EngineCondition::Implicit { condition, .. } => {
                condition.has_geometry_dependent_functions()
            }
        }
    }

    /// Port of `DRC_RULE_CONDITION::RequiresPairItems`.
    pub fn requires_pair_items(&self) -> bool {
        match self {
            EngineCondition::Compiled(c) => c.requires_pair_items(),
            EngineCondition::Implicit { condition, .. } => condition.requires_pair_items(),
        }
    }

    /// Port of `DRC_RULE_CONDITION::ReferencesItemB`.
    pub fn references_item_b(&self) -> bool {
        match self {
            EngineCondition::Compiled(c) => c.references_item_b(),
            EngineCondition::Implicit { condition, .. } => condition.references_item_b(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_implicit_shape() {
        use ImplicitAtom::*;

        let cases: &[(&str, Vec<ImplicitAtom>)] = &[
            ("A.Via_Type == 'Micro'", vec![ViaTypeIsMicro]),
            ("A.Type == 'Barcode'", vec![TypeIsBarcode]),
            (
                "A.hasExactNetclass('Default')",
                vec![HasExactNetclass("Default".into())],
            ),
            (
                "A.hasExactNetclass('a\\'b') && A.inDiffPair('*')",
                vec![HasExactNetclass("a'b".into()), InDiffPair("*".into())],
            ),
            (
                "A.hasExactNetclass('x && y') && AB.isCoupledDiffPair()",
                vec![HasExactNetclass("x && y".into()), IsCoupledDiffPair],
            ),
            (
                "A.hasExactNetclass('P,Q') && A.Via_Type != 'Micro'",
                vec![HasExactNetclass("P,Q".into()), ViaTypeIsNotMicro],
            ),
            (
                "A.hasExactNetclass('n') && A.Layer == 'In1.Cu' && A.inDiffPair('*')",
                vec![
                    HasExactNetclass("n".into()),
                    LayerIs("In1.Cu".into()),
                    InDiffPair("*".into()),
                ],
            ),
            (
                "A.intersectsKeepout('00000000-0000-0000-0000-000000000001')",
                vec![IntersectsKeepout(
                    "00000000-0000-0000-0000-000000000001".into(),
                )],
            ),
        ];

        for (text, atoms) in cases {
            assert_eq!(
                ImplicitCondition::parse(text),
                Some(ImplicitCondition {
                    atoms: atoms.clone()
                }),
                "{text}"
            );
        }

        for text in [
            "",
            "A.NetClass == 'x'",
            "A.hasExactNetclass('x'",
            "A.hasExactNetclass('x') &&",
            "A.Via_Type == 'Micro' || A.Type == 'Barcode'",
        ] {
            assert_eq!(ImplicitCondition::parse(text), None, "{text}");
        }
    }

    #[test]
    fn usage_flags_follow_the_compiled_form() {
        let c =
            ImplicitCondition::parse("A.hasExactNetclass('n') && AB.isCoupledDiffPair()").unwrap();
        assert!(c.requires_pair_items());
        assert!(!c.references_item_b());
        assert!(!c.has_geometry_dependent_functions());

        let k = ImplicitCondition::parse("A.intersectsKeepout('u')").unwrap();
        assert!(k.has_geometry_dependent_functions());
        assert!(!k.requires_pair_items());
    }
}
