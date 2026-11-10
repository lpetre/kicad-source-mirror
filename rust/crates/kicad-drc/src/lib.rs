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

//! Port of KiCad's design rule checker (`pcbnew/drc/`).
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md`, the analysis document
//! `rust/docs/analysis/drc.md` (module layout: section 10.1) and this crate's
//! `PORT_STATUS.md`. One module per C++ file; modules whose work package has not landed yet
//! are empty.
//!
//! Ported so far (work package DRC-20): the `.kicad_dru` rule file parser
//! ([`rule_parser::DrcRulesParser`]), the rule and constraint types ([`rule`]), the rule
//! condition with its compile / evaluate boundary to the PCB expression binding
//! ([`rule_condition`], [`pcbexpr::evaluator`]) and the constant-expression evaluator used for
//! constraint values ([`pcbexpr::evaluator::PcbExprEvaluator`]).

/// Placeholder for a C++ function whose work package has not landed yet: panics with the C++
/// name (see `PORT_STATUS.md`).
#[macro_export]
macro_rules! port_pending {
    ($cxx:literal $(, $arg:expr)* $(,)?) => {{
        $( let _ = &$arg; )*
        panic!(concat!("kicad-drc: port pending: ", $cxx))
    }};
}

pub mod constraint_types;
pub mod reporter;
pub mod rule;
pub mod rule_condition;
pub mod rule_parser;
pub mod rules_lexer;

pub mod item;
pub mod marker;
pub mod rc_item;

pub mod engine;

pub mod cache_generator;
pub mod caches;
pub mod exclusion;
pub mod footprint_utils;
pub mod report;
pub mod rtree;
pub mod test_provider;
pub mod zone_index;

pub mod pcbexpr;
pub mod providers;

pub mod chain_topology;
pub mod creepage_engine;
pub mod creepage_utils;
pub mod length_report;

pub use constraint_types::{DrcConstraintT, DrcImplicitSource};
pub use reporter::{Reporter, WxStringReporter};
pub use rule::{DrcConstraint, DrcConstraintExt, DrcRule, is_component_class_selector};
pub use rule_condition::{CompiledCondition, DrcRuleCondition};
pub use rule_parser::{ComponentClassAssignmentRule, DRC_RULE_FILE_VERSION, DrcRulesParser};
