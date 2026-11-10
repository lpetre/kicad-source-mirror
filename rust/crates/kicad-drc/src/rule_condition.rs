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

//! Port of `pcbnew/drc/drc_rule_condition.h` and `pcbnew/drc/drc_rule_condition.cpp`.
//!
//! A condition keeps its expression text and, once compiled, the microcode. The microcode is
//! produced by the PCB expression binding (package DRC-21) through the
//! [`PcbExprBinder`](crate::pcbexpr::evaluator::PcbExprBinder) boundary and stored
//! type-erased as a [`CompiledCondition`], so rules do not carry the binder type.
//!
//! Not ported: the native implicit conditions of analysis section 4.8.5 (package DRC-03
//! decides whether it needs them once DRC-21 exists).

use std::cell::RefCell;
use std::rc::Rc;

use kicad_core::{PcbLayerId, Severity};
use kicad_libeval::{Context, Ucode};
use kicad_pcb::ItemRef;

use crate::pcbexpr::evaluator::{
    PcbExprBinder, PcbExprUcode, PcbExprUnitResolver, pcbexpr_compiler,
};
use crate::reporter::Reporter;

/// Compiled condition microcode (`PCBEXPR_UCODE`), type-erased over the binder.
pub trait CompiledCondition: Send + Sync {
    /// Port of `PCBEXPR_UCODE::HasGeometryDependentFunctions`.
    fn has_geometry_dependent_functions(&self) -> bool;

    /// Port of `PCBEXPR_UCODE::RequiresPairItems`.
    fn requires_pair_items(&self) -> bool;

    /// Port of `PCBEXPR_UCODE::ReferencesItemB`.
    fn references_item_b(&self) -> bool;

    /// One run of the microcode as `DRC_RULE_CONDITION::EvaluateFor` does it: a context for
    /// `constraint` and `layer` holding the items (with teardrop zones typed as tracks),
    /// errors reported to `reporter` as `"ERROR: <message>"`, `m_ucode->Run( ctx )->AsDouble()`.
    fn run<'e>(
        &self,
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
        reporter: Option<&mut dyn Reporter>,
    ) -> f64;
}

impl<B: PcbExprBinder> CompiledCondition for Ucode<B>
where
    B::VarRef: Send + Sync,
{
    fn has_geometry_dependent_functions(&self) -> bool {
        self.binder().has_geometry_dependent_functions()
    }

    fn requires_pair_items(&self) -> bool {
        self.binder().requires_pair_items()
    }

    fn references_item_b(&self) -> bool {
        self.binder().references_item_b()
    }

    fn run<'e>(
        &self,
        a: Option<ItemRef<'e>>,
        b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
        reporter: Option<&mut dyn Reporter>,
    ) -> f64 {
        let mut ctx: Context<'e, B> = Context::new(B::item_env(a, b, constraint, layer));

        // The context's error callback cannot borrow the reporter for the context's lifetime;
        // collect the messages and forward them in order once the run is over (nothing else
        // reports in between).
        let messages: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

        if reporter.is_some() {
            let sink = Rc::clone(&messages);
            ctx.set_error_callback(Some(Box::new(move |message: &str, _offset: i32| {
                // aReporter->Report( _( "ERROR:" ) + wxS( " " ) + aMessage )
                sink.borrow_mut().push(format!("ERROR: {message}"));
            })));
        }

        let result = Ucode::run(self, &mut ctx);
        let value = ctx.value(result).as_double(&ctx);

        if let Some(reporter) = reporter {
            for message in messages.borrow().iter() {
                reporter.report(message, Severity::Undefined);
            }
        }

        value
    }
}

/// Port of `DRC_RULE_CONDITION`.
pub struct DrcRuleCondition {
    /// `m_expression`.
    expression: String,
    /// `m_ucode`.
    ucode: Option<Box<dyn CompiledCondition>>,
}

impl std::fmt::Debug for DrcRuleCondition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrcRuleCondition")
            .field("expression", &self.expression)
            .field("compiled", &self.ucode.is_some())
            .finish()
    }
}

impl DrcRuleCondition {
    /// Port of `DRC_RULE_CONDITION::DRC_RULE_CONDITION( const wxString& aExpression )`.
    pub fn new(expression: &str) -> Self {
        DrcRuleCondition {
            expression: expression.to_string(),
            ucode: None,
        }
    }

    /// Port of `DRC_RULE_CONDITION::EvaluateFor`.
    ///
    /// An empty expression is true; a condition that was never compiled is false (reporting
    /// "ERROR in expression."). Conditions are commutative: if `(a, b)` does not match and
    /// there is a `b`, `(b, a)` is tried.
    pub fn evaluate_for<'e>(
        &self,
        item_a: Option<ItemRef<'e>>,
        item_b: Option<ItemRef<'e>>,
        constraint: i32,
        layer: PcbLayerId,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        if self.expression.is_empty() {
            return true;
        }

        let Some(ucode) = &self.ucode else {
            if let Some(reporter) = reporter {
                reporter.report("ERROR in expression.", Severity::Undefined);
            }

            return false;
        };

        // (The C++ reuses a per-thread context on the no-reporter path; that is a performance
        // measure only.)
        if ucode.run(
            item_a,
            item_b,
            constraint,
            layer,
            crate::reporter::reborrow(&mut reporter),
        ) != 0.0
        {
            return true;
        } else if item_b.is_some() {
            // Conditions are commutative
            if ucode.run(item_b, item_a, constraint, layer, reporter) != 0.0 {
                return true;
            }
        }

        false
    }

    /// Port of `DRC_RULE_CONDITION::Compile` with the default binding ([`PcbExprUcode`]).
    pub fn compile(
        &mut self,
        reporter: Option<&mut dyn Reporter>,
        source_line: i32,
        source_offset: i32,
    ) -> bool {
        self.compile_with::<PcbExprUcode>(reporter, source_line, source_offset)
    }

    /// Port of `DRC_RULE_CONDITION::Compile( REPORTER* aReporter, int aSourceLine,
    /// int aSourceOffset )`, generic over the binding.
    ///
    /// Errors go to `reporter` as `ERROR: <a href='line:offset'>message</a>` (the part of the
    /// message after a `|` outside the link) with severity error. The microcode is kept even
    /// if compilation fails, as in C++.
    pub fn compile_with<B: PcbExprBinder>(
        &mut self,
        reporter: Option<&mut dyn Reporter>,
        source_line: i32,
        source_offset: i32,
    ) -> bool
    where
        B::VarRef: Send + Sync,
    {
        let mut compiler = pcbexpr_compiler(Box::new(PcbExprUnitResolver::new(false)));

        if let Some(reporter) = reporter {
            compiler.set_error_callback(Some(Box::new(move |message: &str, offset: i32| {
                let (first, rest) = before_first(message, '|');
                let msg = format!(
                    "ERROR: <a href='{}:{}'>{}</a>{}",
                    source_line,
                    source_offset + offset,
                    first,
                    rest
                );

                reporter.report(&msg, Severity::Error);
            })));
        }

        let mut ucode: Ucode<B> = Ucode::new(B::default());
        let mut preflight_context: Context<'_, B> = Context::new(B::new_env(0, PcbLayerId::FCu));

        let ok = compiler.compile(&self.expression, &mut ucode, &mut preflight_context);
        drop(compiler);

        self.ucode = Some(Box::new(ucode));
        ok
    }

    /// Port of `DRC_RULE_CONDITION::SetExpression`.
    pub fn set_expression(&mut self, expression: &str) {
        self.expression = expression.to_string();
    }

    /// Port of `DRC_RULE_CONDITION::GetExpression`.
    pub fn expression(&self) -> &str {
        &self.expression
    }

    /// True once [`compile`](Self::compile) ran (C++ `m_ucode != nullptr`).
    pub fn is_compiled(&self) -> bool {
        self.ucode.is_some()
    }

    /// Port of `DRC_RULE_CONDITION::HasGeometryDependentFunctions`.
    pub fn has_geometry_dependent_functions(&self) -> bool {
        self.ucode
            .as_ref()
            .is_some_and(|u| u.has_geometry_dependent_functions())
    }

    /// Port of `DRC_RULE_CONDITION::RequiresPairItems`.
    pub fn requires_pair_items(&self) -> bool {
        self.ucode.as_ref().is_some_and(|u| u.requires_pair_items())
    }

    /// Port of `DRC_RULE_CONDITION::ReferencesItemB`.
    pub fn references_item_b(&self) -> bool {
        self.ucode.as_ref().is_some_and(|u| u.references_item_b())
    }
}

/// `wxString::BeforeFirst( ch, &rest )`: the text before the first `ch` and the text after it
/// (empty if there is no `ch`).
pub(crate) fn before_first(text: &str, ch: char) -> (&str, &str) {
    match text.find(ch) {
        Some(pos) => (&text[..pos], &text[pos + ch.len_utf8()..]),
        None => (text, ""),
    }
}
