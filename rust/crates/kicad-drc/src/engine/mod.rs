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

//! Port of `pcbnew/drc/drc_engine.h` / `pcbnew/drc/drc_engine.cpp`.
//!
//! Ported so far (package DRC-20): the rule file loading of `DRC_ENGINE::loadRules`
//! ([`load_rules`]). The engine itself is package DRC-03.

pub mod diff_pair;
pub mod eval_rules;
pub mod implicit_rules;

use kicad_core::text_vars::{ResolutionContext, expand_text_vars};
use kicad_pcb::Board;
use kicad_sexpr::IoError;

use crate::reporter::Reporter;
use crate::rule::{DrcRule, is_component_class_selector};
use crate::rule_parser::{DrcRulesParser, LayerChoices};

/// The text `DRC_ENGINE::loadRules` hands to the parser: every line of the file (as
/// `FILE_LINE_READER::ReadLine` returns it, i.e. *with* its line terminator) passed through
/// `BOARD::ConvertCrossReferencesToKIIDs` and `ExpandTextVars( line, resolver, INTERNAL )`,
/// followed by `'\n'`.
///
/// Note that every line therefore ends up followed by an empty line, so line numbers in
/// parse errors of rule files loaded by the engine are `2 * n - 1` for line `n` of the file;
/// this is what the C++ does.
///
/// `resolve` is `BOARD::ResolveTextVar( token, 0 )`; `${Class:...}` selectors are left alone.
pub fn prepare_rules_text(
    file_text: &str,
    convert_cross_references: impl Fn(&str) -> String,
    resolve: &dyn Fn(&mut String) -> bool,
) -> String {
    let resolver = |token: &mut String| -> bool {
        if is_component_class_selector(token) {
            return false;
        }

        resolve(token)
    };

    let mut rules_text = String::new();

    // FILE_LINE_READER::ReadLine: each line up to and including its '\n'.
    for line in file_text.split_inclusive('\n') {
        let mut str = convert_cross_references(line);
        str = expand_text_vars(&str, Some(&resolver), ResolutionContext::Internal);

        rules_text.push_str(&str);
        rules_text.push('\n');
    }

    rules_text
}

/// Port of `DRC_ENGINE::loadRules( const wxFileName& aPath )` without the file access: the
/// caller reads the file and passes its text (`None` when the file does not exist) and its
/// full path (the source name of parse errors). The rules are returned only if parsing
/// succeeds ("so that if Parse() throws then the possibly malformed rules won't contaminate
/// the current ruleset"); the caller appends them to the engine's rules.
///
/// `layer_choices` is the state of the global `ENUM_MAP<PCB_LAYER_ID>` the C++ parser matches
/// layer names against: [`board_layer_choices`](crate::rule_parser::board_layer_choices) after
/// `BOARD_LOADER::Load` (kicad-cli), [`default_layer_choices`](crate::rule_parser::default_layer_choices)
/// for boards loaded by the plain parser (QA tests).
pub fn load_rules(
    board: &Board,
    file_text: Option<&str>,
    path: &str,
    layer_choices: LayerChoices,
    log_reporter: Option<&mut dyn Reporter>,
) -> Result<Vec<DrcRule>, IoError> {
    let mut rules = Vec::new();

    if let Some(file_text) = file_text {
        let variant = board.current_variant.clone();
        let resolve = |token: &mut String| board.resolve_text_var(token, &variant, 0);
        let rules_text = prepare_rules_text(
            file_text,
            |line| board.convert_cross_references_to_kiids(line),
            &resolve,
        );

        let mut parser: DrcRulesParser<'_> = DrcRulesParser::new(&rules_text, path);
        parser.set_layer_choices(layer_choices);
        parser.parse(&mut rules, log_reporter)?;
    }

    Ok(rules)
}
