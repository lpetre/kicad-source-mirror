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

//! Port of `pcbnew/drc/drc_rule_parser.h` and `pcbnew/drc/drc_rule_parser.cpp`: the parser
//! of `.kicad_dru` design rule files (and of component class assignment rules).
//!
//! Error handling follows the C++ exactly: with a reporter every problem is reported as
//! `ERROR: <a href='line:offset'>message</a>rest` and parsing continues; without a reporter
//! the first problem is returned as a `PARSE_ERROR` (`Err(IoError::Parse(..))`) whose problem
//! text is `ERROR: messagerest` (the C++ throws). Errors of the lexer itself (and of the
//! `Need...()` calls of the `return_path` constraint) are returned in both modes.
//!
//! The parser is generic over the PCB expression binding used to compile conditions and
//! evaluate constraint values (see [`crate::pcbexpr::evaluator`]); the default is the real
//! one, [`PcbExprUcode`].

use std::marker::PhantomData;
use std::sync::Arc;

use kicad_core::{EdaDataType, EdaUnits, Lset, PcbLayerId, Severity};
use kicad_geom::math::util::ki_round;
use kicad_libeval::UnitResolver;
use kicad_libeval::string_utils::wild_compare_string;
use kicad_pcb::Board;
use kicad_pcb::zone_settings::ZoneConnection;
use kicad_sexpr::IoError;

use crate::constraint_types::{DrcConstraintT, drc_constraint_options, drc_disallow};
use crate::pcbexpr::evaluator::{
    PcbExprBinder, PcbExprEvaluator, PcbExprUcode, PcbExprUnitResolver, PcbExprUnitlessResolver,
    type_from_units,
};
use crate::reporter::Reporter;
use crate::rule::{DrcConstraint, DrcRule, is_component_class_selector};
use crate::rule_condition::{DrcRuleCondition, before_first};
use crate::rules_lexer::{DrcRulesLexer, T};

/// `DRC_RULE_FILE_VERSION`.
pub const DRC_RULE_FILE_VERSION: i32 = 20200610;

/// The choices of `ENUM_MAP<PCB_LAYER_ID>` (layer names and their layers, in order), which
/// the C++ parser matches `(layer ...)` names against.
pub type LayerChoices = Vec<(String, PcbLayerId)>;

/// The layer choices as registered at start-up by `BOARD_ITEM_DESC` (`board_item.cpp`): the
/// canonical name (`LSET::Name`) of every layer of `LSET::AllLayersMask()`.
pub fn default_layer_choices() -> LayerChoices {
    Lset::all_layers_mask()
        .seq()
        .iter()
        .map(|&layer| (Lset::name(layer), layer))
        .collect()
}

/// The layer choices after `BOARD_LOADER::Load` (`board_loader.cpp`) loaded `board`: for every
/// layer of `LSET::AllLayersMask()` its canonical name and then the board's name for it.
pub fn board_layer_choices(board: &Board) -> LayerChoices {
    let mut choices = LayerChoices::new();

    for &layer in Lset::all_layers_mask().seq().iter() {
        choices.push((Lset::name(layer), layer));
        choices.push((board.layer_name(layer), layer));
    }

    choices
}

/// Port of `COMPONENT_CLASS_ASSIGNMENT_RULE` as produced by
/// `DRC_RULES_PARSER::ParseComponentClassAssignmentRules` (the data part; `Matches` belongs to
/// the component class package).
#[derive(Debug, Clone)]
pub struct ComponentClassAssignmentRule {
    /// `m_componentClass`: the component class to assign to matching footprints.
    pub component_class: String,
    /// `m_condition`: the condition which specifies footprint matches.
    pub condition: Option<Arc<DrcRuleCondition>>,
}

/// C `strtol( text, nullptr, 10 )` converted to `int` as the C++ does with `(int) strtol(..)`:
/// leading white space, an optional sign and the leading decimal digits; 0 if there are none;
/// saturated to the range of `long`; then truncated to 32 bits.
fn strtol_int(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
    {
        i += 1;
    }

    let mut negative = false;

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    let mut value: i64 = 0;
    let mut overflow = false;

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        let digit = (bytes[i] - b'0') as i64;

        if !overflow {
            match value
                .checked_mul(10)
                .and_then(|v| if negative { v.checked_sub(digit) } else { v.checked_add(digit) })
            {
                Some(v) => value = v,
                None => overflow = true,
            }
        }

        i += 1;
    }

    if overflow {
        value = if negative { i64::MIN } else { i64::MAX };
    }

    // (int) of a long: the low 32 bits (g++ / x86-64).
    value as i32
}

/// Port of `DRC_RULES_PARSER`.
pub struct DrcRulesParser<'a, B: PcbExprBinder = PcbExprUcode>
where
    B::VarRef: Send + Sync,
{
    lexer: DrcRulesLexer,
    /// `m_requiredVersion` (read by nothing).
    required_version: i32,
    /// `m_tooRecent` (read by nothing).
    too_recent: bool,
    /// `m_reporter`.
    reporter: Option<&'a mut dyn Reporter>,
    /// `ENUM_MAP<PCB_LAYER_ID>::Instance().Choices()`.
    layer_choices: LayerChoices,
    _binder: PhantomData<fn() -> B>,
}

/// The result of one `(min ...)` / `(opt ...)` / `(max ...)` value, before validation.
struct ParsedValue {
    value: i32,
    units: EdaUnits,
}

impl<'a, B: PcbExprBinder> DrcRulesParser<'a, B>
where
    B::VarRef: Send + Sync,
{
    /// Port of `DRC_RULES_PARSER::DRC_RULES_PARSER( const wxString& aSource,
    /// const wxString& aSourceDescr )`: `source` is the rule text, `source_descr` its name in
    /// error messages. Layer names are matched against [`default_layer_choices`]; see
    /// [`set_layer_choices`](Self::set_layer_choices).
    pub fn new(source: &str, source_descr: &str) -> Self {
        DrcRulesParser {
            lexer: DrcRulesLexer::new(source, source_descr),
            required_version: 0,
            too_recent: false,
            reporter: None,
            layer_choices: default_layer_choices(),
            _binder: PhantomData,
        }
    }

    /// Replace the layer name table (C++: the global `ENUM_MAP<PCB_LAYER_ID>`, which
    /// `BOARD_LOADER` refills with the board's layer names, see [`board_layer_choices`]).
    pub fn set_layer_choices(&mut self, choices: LayerChoices) {
        self.layer_choices = choices;
    }

    /// `m_requiredVersion`: the number of the `(version N)` statement.
    pub fn required_version(&self) -> i32 {
        self.required_version
    }

    /// `m_tooRecent`: the version is newer than [`DRC_RULE_FILE_VERSION`].
    pub fn too_recent(&self) -> bool {
        self.too_recent
    }

    /* ----------------------------------------------------------------------------------------
     * Error reporting
     */

    /// Port of `DRC_RULES_PARSER::reportError( const wxString& aMessage, int aOffset )`.
    fn report_error(&mut self, message: &str, offset: i32) -> Result<(), IoError> {
        let line = self.lexer.cur_line_number();
        let off = self.lexer.cur_offset() + offset;
        let source_line = self.lexer.cur_line().to_string();
        self.report_error_at(message, line, off, &source_line)
    }

    /// Port of `DRC_RULES_PARSER::reportErrorAt`.
    fn report_error_at(
        &mut self,
        message: &str,
        line: i32,
        offset: i32,
        source_line: &str,
    ) -> Result<(), IoError> {
        let (first, rest) = before_first(message, '|');

        if let Some(reporter) = self.reporter.as_deref_mut() {
            let msg = format!("ERROR: <a href='{line}:{offset}'>{first}</a>{rest}");

            reporter.report(&msg, Severity::Error);
            Ok(())
        } else {
            let msg = format!("ERROR: {first}{rest}");

            Err(IoError::parse(
                msg,
                self.lexer.cur_source(),
                source_line,
                line,
                offset,
            ))
        }
    }

    /// Port of `DRC_RULES_PARSER::reportDeprecation`: a warning, only with a reporter.
    fn report_deprecation(&mut self, old_token: &str, new_token: &str) {
        if let Some(reporter) = self.reporter.as_deref_mut() {
            let msg = format!(
                "The '{old_token}' keyword has been deprecated.  Please use '{new_token}' instead."
            );

            reporter.report(&msg, Severity::Warning);
        }
    }

    /// Port of `DRC_RULES_PARSER::checkUnresolvedTextVariable`: report a `${...}` left in the
    /// current token (other than a `${Class:...}` selector); true if one was reported.
    fn check_unresolved_text_variable(&mut self) -> Result<bool, IoError> {
        let cur_text = self.lexer.cur_bytes().to_vec();
        let mut search = find_bytes(&cur_text, b"${", 0);

        while let Some(pos) = search {
            let end = find_byte(&cur_text, b'}', pos + 2);

            if let Some(end) = end {
                // wxString::FromUTF8: empty for invalid UTF-8.
                let token = String::from_utf8(cur_text[pos + 2..end].to_vec()).unwrap_or_default();

                // ${Class:X} is a DRC expression directive handled by testFootprintSelector(), so
                // it is left unexpanded on purpose and must not be reported as unresolved.
                if is_component_class_selector(&token) {
                    search = find_bytes(&cur_text, b"${", pos + 2);
                    continue;
                }
            }

            self.report_error("Unresolved text variable", pos as i32)?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Port of `DRC_RULES_PARSER::parseUnknown`: skip to the end of the current list.
    fn parse_unknown(&mut self) -> Result<(), IoError> {
        let mut depth = 1;
        let mut token = self.lexer.next_tok()?;

        while token != T::T_EOF {
            if token == T::T_LEFT {
                depth += 1;
            }

            if token == T::T_RIGHT {
                depth -= 1;

                if depth == 0 {
                    break;
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `DRC_RULES_PARSER::expected`.
    fn expected(&mut self, expected_tokens: &str) -> Result<(), IoError> {
        let msg = if self.lexer.cur_bytes().starts_with(b"${") {
            "Unresolved text variable.".to_string()
        } else {
            format!(
                "Unrecognized item '{}'.| Expected {}.",
                self.lexer.from_utf8(),
                expected_tokens
            )
        };

        self.report_error(&msg, 0)?;
        self.parse_unknown()
    }

    /// Port of `DRC_RULES_PARSER::parseExpression`: the tokens up to the closing parenthesis,
    /// re-joined with their original separators.
    fn parse_expression(&mut self) -> Result<String, IoError> {
        let mut expr = String::new();
        let mut depth = 1;
        let mut token = self.lexer.next_tok()?;

        while token != T::T_EOF {
            if token == T::T_LEFT {
                depth += 1;
            }

            if token == T::T_RIGHT {
                depth -= 1;

                if depth == 0 {
                    break;
                }
            }

            if !expr.is_empty() {
                expr.push_str(self.lexer.cur_separator());
            }

            self.check_unresolved_text_variable()?;
            expr.push_str(&self.lexer.from_utf8());

            token = self.lexer.next_tok()?;
        }

        Ok(expr)
    }

    /* ----------------------------------------------------------------------------------------
     * Top level
     */

    /// Port of `DRC_RULES_PARSER::Parse`: parse the rules into `rules` (appended in file
    /// order; on an error return the rules completed so far stay in `rules`).
    pub fn parse(
        &mut self,
        rules: &mut Vec<DrcRule>,
        reporter: Option<&'a mut dyn Reporter>,
    ) -> Result<(), IoError> {
        let mut have_version = false;

        self.reporter = reporter;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_EOF {
            if self.check_unresolved_text_variable()? {
                token = self.lexer.next_tok()?;
                continue;
            }

            if token != T::T_LEFT {
                self.report_error("Missing '('.", 0)?;
            }

            token = self.lexer.next_tok()?;

            if !have_version && token != T::T_version {
                self.report_error("Missing version statement.", 0)?;
                have_version = true; // don't keep on reporting it
            }

            match token {
                T::T_version => self.parse_version(&mut have_version)?,

                T::T_rule => {
                    let mut rule = self.parse_drc_rule()?;
                    // The C++ constraints point at their rule and see its final state.
                    rule.sync_constraint_rule_info();
                    rules.push(rule);
                }

                T::T_EOF => self.report_error("Incomplete statement.", 0)?,

                _ => self.expected("rule or version")?,
            }

            token = self.lexer.next_tok()?;
        }

        if let Some(reporter) = self.reporter.as_deref_mut()
            && !reporter.has_message()
        {
            reporter.report("No errors found.", Severity::Info);
        }

        self.reporter = None;
        Ok(())
    }

    /// The `T_version` case of `Parse` / `ParseComponentClassAssignmentRules`.
    fn parse_version(&mut self, have_version: &mut bool) -> Result<(), IoError> {
        *have_version = true;
        let token = self.lexer.next_tok()?;

        if token == T::T_RIGHT {
            self.report_error("Missing version number.", 0)?;
        } else if token == T::T_NUMBER {
            self.required_version = strtol_int(self.lexer.cur_text());
            self.too_recent = self.required_version > DRC_RULE_FILE_VERSION;

            if self.lexer.next_tok()? != T::T_RIGHT {
                self.report_error("Missing ')'.", 0)?;
            }
        } else {
            self.expected("version number")?; // translate "version number"; it is not a token
        }

        Ok(())
    }

    /// Port of `DRC_RULES_PARSER::ParseComponentClassAssignmentRules`.
    ///
    /// A statement cut short by the end of the input yields a `None` entry (the C++ appends a
    /// null pointer).
    pub fn parse_component_class_assignment_rules(
        &mut self,
        rules: &mut Vec<Option<ComponentClassAssignmentRule>>,
        reporter: Option<&'a mut dyn Reporter>,
    ) -> Result<(), IoError> {
        let mut have_version = false;

        self.reporter = reporter;

        let mut token = self.lexer.next_tok()?;

        while token != T::T_EOF {
            if token != T::T_LEFT {
                self.report_error("Missing '('.", 0)?;
            }

            token = self.lexer.next_tok()?;

            if !have_version && token != T::T_version {
                self.report_error("Missing version statement.", 0)?;
                have_version = true; // don't keep on reporting it
            }

            match token {
                T::T_version => self.parse_version(&mut have_version)?,

                T::T_assign_component_class => {
                    let rule = self.parse_component_class_assignment()?;
                    rules.push(rule);
                }

                T::T_EOF => self.report_error("Incomplete statement.", 0)?,

                _ => self.expected("assign_component_class or version")?,
            }

            token = self.lexer.next_tok()?;
        }

        if let Some(reporter) = self.reporter.as_deref_mut()
            && !reporter.has_message()
        {
            reporter.report("No errors found.", Severity::Info);
        }

        self.reporter = None;
        Ok(())
    }

    /// Port of `DRC_RULE_CONDITION::Compile( m_reporter, line, offset )` for a condition read
    /// at the current token.
    fn compile_condition(&mut self, condition: &mut DrcRuleCondition) -> bool {
        let line = self.lexer.cur_line_number();
        let offset = self.lexer.cur_offset();

        condition.compile_with::<B>(crate::reporter::reborrow(&mut self.reporter), line, offset)
    }

    /// Port of `DRC_RULES_PARSER::parseDRC_RULE`.
    fn parse_drc_rule(&mut self) -> Result<DrcRule, IoError> {
        let mut rule = DrcRule::new();
        let mut condition_line = 0;
        let mut condition_offset = 0;
        let mut condition_source = String::new();

        let token = self.lexer.next_tok()?;

        if !DrcRulesLexer::is_symbol(token) {
            self.report_error("Missing rule name.", 0)?;
        }

        self.check_unresolved_text_variable()?;
        rule.name = self.lexer.from_utf8();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if self.check_unresolved_text_variable()? {
                token = self.lexer.next_tok()?;
                continue;
            }

            if token != T::T_LEFT {
                self.report_error("Missing '('.", 0)?;
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_constraint => self.parse_constraint(&mut rule)?,

                T::T_condition => {
                    token = self.lexer.next_tok()?;

                    if token == T::T_RIGHT {
                        self.report_error("Missing condition expression.", 0)?;
                    } else if DrcRulesLexer::is_symbol(token) {
                        self.check_unresolved_text_variable()?;
                        let mut condition = DrcRuleCondition::new(&self.lexer.from_utf8());
                        condition_line = self.lexer.cur_line_number();
                        condition_offset = self.lexer.cur_offset();
                        condition_source = self.lexer.cur_line().to_string();

                        let ok = self.compile_condition(&mut condition);
                        rule.condition = Some(Arc::new(condition));

                        if !ok {
                            let msg =
                                format!("Could not parse expression '{}'.", self.lexer.from_utf8());
                            self.report_error(&msg, 0)?;
                        }

                        if self.lexer.next_tok()? != T::T_RIGHT {
                            self.report_error("Missing ')'.", 0)?;
                        }
                    } else {
                        // translate "quoted expression"; it is not a token
                        self.expected("quoted expression")?;
                    }
                }

                T::T_layer => {
                    if rule.layer_condition != *Lset::all_layers_mask() {
                        self.report_error("'layer' keyword already present.", 0)?;
                    }

                    let mut layer_source = std::mem::take(&mut rule.layer_source);
                    rule.layer_condition = self.parse_layer(&mut layer_source)?;
                    rule.layer_source = layer_source;
                }

                T::T_severity => rule.severity = self.parse_severity()?,

                T::T_EOF => {
                    self.report_error("Incomplete statement.", 0)?;
                    return Ok(rule);
                }

                _ => self.expected("constraint, condition, or disallow")?,
            }

            token = self.lexer.next_tok()?;
        }

        if self.lexer.cur_tok() != T::T_RIGHT {
            self.report_error("Missing ')'.", 0)?;
        }

        if let Some(condition) = &rule.condition
            && condition.requires_pair_items()
        {
            let references_item_b = condition.references_item_b();

            for constraint in &rule.constraints {
                // isCoupledDiffPair() can identify the pair from A's net for these constraints
                if !references_item_b
                    && matches!(
                        constraint.type_,
                        DrcConstraintT::LengthConstraint
                            | DrcConstraintT::NetChainLengthConstraint
                            | DrcConstraintT::SkewConstraint
                    )
                {
                    continue;
                }

                if crate::rule::DrcConstraintExt::is_unary(constraint) {
                    let msg = format!(
                        "Item 'B' is not available for a single-item constraint in rule '{}'.",
                        rule.name
                    );
                    self.report_error_at(
                        &msg,
                        condition_line,
                        condition_offset,
                        &condition_source,
                    )?;
                    break;
                }
            }
        }

        Ok(rule)
    }

    /// Port of `DRC_RULES_PARSER::parseComponentClassAssignment`.
    fn parse_component_class_assignment(
        &mut self,
    ) -> Result<Option<ComponentClassAssignmentRule>, IoError> {
        let mut condition: Option<Arc<DrcRuleCondition>> = None;

        let token = self.lexer.next_tok()?;

        if !DrcRulesLexer::is_symbol(token) {
            self.report_error("Missing component class name.", 0)?;
        }

        self.check_unresolved_text_variable()?;
        let component_class = self.lexer.from_utf8();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token != T::T_LEFT {
                self.report_error("Missing '('.", 0)?;
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_condition => {
                    token = self.lexer.next_tok()?;

                    if token == T::T_RIGHT {
                        self.report_error("Missing condition expression.", 0)?;
                    } else if DrcRulesLexer::is_symbol(token) {
                        self.check_unresolved_text_variable()?;
                        let mut new_condition = DrcRuleCondition::new(&self.lexer.from_utf8());

                        let ok = self.compile_condition(&mut new_condition);
                        condition = Some(Arc::new(new_condition));

                        if !ok {
                            let msg =
                                format!("Could not parse expression '{}'.", self.lexer.from_utf8());
                            self.report_error(&msg, 0)?;
                        }

                        if self.lexer.next_tok()? != T::T_RIGHT {
                            self.report_error("Missing ')'.", 0)?;
                        }
                    } else {
                        // translate "quoted expression"; it is not a token
                        self.expected("quoted expression")?;
                    }
                }

                T::T_EOF => {
                    self.report_error("Incomplete statement.", 0)?;
                    return Ok(None);
                }

                _ => self.expected("condition")?,
            }

            token = self.lexer.next_tok()?;
        }

        if self.lexer.cur_tok() != T::T_RIGHT {
            self.report_error("Missing ')'.", 0)?;
        }

        Ok(Some(ComponentClassAssignmentRule {
            component_class,
            condition,
        }))
    }

    /* ----------------------------------------------------------------------------------------
     * Constraints
     */

    /// The `validateAndSetValueWithUnits` lambda of `parseConstraint`.
    fn validate_and_set_value_with_units(
        &mut self,
        c: &mut DrcConstraint,
        allows_time_domain: bool,
        units_type: &mut EdaDataType,
        parsed: ParsedValue,
        setter: fn(&mut DrcConstraint, i32),
    ) -> Result<(), IoError> {
        let units_type_tmp = type_from_units(parsed.units);

        if !allows_time_domain && units_type_tmp == EdaDataType::Time {
            self.report_error("Time based units not allowed for constraint type.", 0)?;
        }

        if (c.value.has_min() || c.value.has_max() || c.value.has_opt())
            && *units_type != units_type_tmp
        {
            self.report_error("Mixed units for constraint values.", 0)?;
        }

        *units_type = units_type_tmp;
        setter(c, parsed.value);

        if allows_time_domain {
            if *units_type == EdaDataType::Time {
                c.set_option(drc_constraint_options::TIME_DOMAIN);
                c.clear_option(drc_constraint_options::SPACE_DOMAIN);
            } else {
                c.set_option(drc_constraint_options::SPACE_DOMAIN);
                c.clear_option(drc_constraint_options::TIME_DOMAIN);
            }
        }

        Ok(())
    }

    /// The constraint type of a constraint keyword (the `switch` of `parseConstraint`).
    fn constraint_type(token: T) -> Option<DrcConstraintT> {
        use DrcConstraintT::*;

        Some(match token {
            T::T_assertion => AssertionConstraint,
            T::T_clearance => ClearanceConstraint,
            T::T_creepage => CreepageConstraint,
            T::T_hole_clearance => HoleClearanceConstraint,
            T::T_edge_clearance => EdgeClearanceConstraint,
            T::T_hole_size => HoleSizeConstraint,
            T::T_hole_to_hole => HoleToHoleConstraint,
            T::T_courtyard_clearance => CourtyardClearanceConstraint,
            T::T_silk_clearance => SilkClearanceConstraint,
            T::T_text_height => TextHeightConstraint,
            T::T_text_thickness => TextThicknessConstraint,
            T::T_track_width => TrackWidthConstraint,
            T::T_track_angle => TrackAngleConstraint,
            T::T_track_segment_length => TrackSegmentLengthConstraint,
            T::T_connection_width => ConnectionWidthConstraint,
            T::T_annular_width => AnnularWidthConstraint,
            T::T_via_diameter => ViaDiameterConstraint,
            T::T_via_dangling => ViaDanglingConstraint,
            T::T_zone_connection => ZoneConnectionConstraint,
            T::T_thermal_relief_gap => ThermalReliefGapConstraint,
            T::T_thermal_spoke_width => ThermalSpokeWidthConstraint,
            T::T_microvia_stack_depth => MicroviaStackDepthConstraint,
            T::T_hole_aspect_ratio => HoleAspectRatioConstraint,
            T::T_min_resolved_spokes => MinResolvedSpokesConstraint,
            T::T_solder_mask_expansion => SolderMaskExpansionConstraint,
            T::T_solder_mask_sliver => SolderMaskSliverConstraint,
            T::T_solder_paste_abs_margin => SolderPasteAbsMarginConstraint,
            T::T_solder_paste_rel_margin => SolderPasteRelMarginConstraint,
            T::T_disallow => DisallowConstraint,
            T::T_length => LengthConstraint,
            T::T_net_chain_length => NetChainLengthConstraint,
            T::T_stub_length => NetChainStubLengthConstraint,
            T::T_return_path => NetChainReturnPathConstraint,
            T::T_skew => SkewConstraint,
            T::T_via_count => ViaCountConstraint,
            T::T_diff_pair_gap => DiffPairGapConstraint,
            T::T_diff_pair_uncoupled => MaxUncoupledConstraint,
            T::T_physical_clearance => PhysicalClearanceConstraint,
            T::T_physical_hole_clearance => PhysicalHoleClearanceConstraint,
            T::T_bridged_mask => BridgedMaskConstraint,
            _ => return None,
        })
    }

    /// Port of `DRC_RULES_PARSER::parseConstraint`.
    fn parse_constraint(&mut self, rule: &mut DrcRule) -> Result<(), IoError> {
        let mut c = DrcConstraint::default();
        let mut units_type = EdaDataType::Unitless;

        let mut token = self.lexer.next_tok()?;

        if self.check_unresolved_text_variable()? {
            return Ok(());
        }

        if token == T::T_mechanical_clearance {
            self.report_deprecation("mechanical_clearance", "physical_clearance");
            token = T::T_physical_clearance;
        } else if token == T::T_mechanical_hole_clearance {
            self.report_deprecation("mechanical_hole_clearance", "physical_hole_clearance");
            token = T::T_physical_hole_clearance;
        } else if token == T::T_hole {
            self.report_deprecation("hole", "hole_size");
            token = T::T_hole_size;
        } else if token == T::T_RIGHT || token == T::T_EOF {
            let msg = format!(
                "Missing constraint type.|  Expected {}.",
                "assertion, clearance, hole_clearance, edge_clearance, physical_clearance, \
                 physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, \
                 hole_to_hole, track_width, track_angle, track_segment_length, annular_width, \
                 disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, \
                 min_resolved_spokes, microvia_stack_depth, hole_aspect_ratio, solder_mask_expansion, \
                 solder_paste_abs_margin, \
                 solder_paste_rel_margin, length, net_chain_length, skew, via_count, \
                 via_dangling, via_diameter, diff_pair_gap or diff_pair_uncoupled"
            );

            self.report_error(&msg, 0)?;
            return Ok(());
        }

        let Some(type_) = Self::constraint_type(token) else {
            self.expected(
                "assertion, clearance, hole_clearance, edge_clearance, physical_clearance, \
                 physical_hole_clearance, courtyard_clearance, silk_clearance, hole_size, \
                 hole_to_hole, track_width, track_angle, track_segment_length, annular_width, \
                 disallow, zone_connection, thermal_relief_gap, thermal_spoke_width, \
                 min_resolved_spokes, microvia_stack_depth, hole_aspect_ratio, solder_mask_expansion, \
                 solder_mask_sliver, \
                 solder_paste_abs_margin, solder_paste_rel_margin, length, net_chain_length, \
                 skew, via_count, via_dangling, via_diameter, diff_pair_gap, \
                 diff_pair_uncoupled or bridged_mask",
            )?;
            return Ok(());
        };

        c.type_ = type_;

        if rule.find_constraint(c.type_).is_some() {
            let msg = format!(
                "Rule already has a '{}' constraint.",
                self.lexer.from_utf8()
            );
            self.report_error(&msg, 0)?;
        }

        let ratio = c.type_ == DrcConstraintT::HoleAspectRatioConstraint;

        let unitless = ratio
            || c.type_ == DrcConstraintT::ViaCountConstraint
            || c.type_ == DrcConstraintT::MinResolvedSpokesConstraint
            || c.type_ == DrcConstraintT::MicroviaStackDepthConstraint
            || c.type_ == DrcConstraintT::ViaDanglingConstraint
            || c.type_ == DrcConstraintT::BridgedMaskConstraint;

        // Because previous versions allowed non-united values (in fact, required them), we have
        // to interpret a track angle constraint value with no units as degrees.
        let implied_degrees = c.type_ == DrcConstraintT::TrackAngleConstraint;

        let allows_time_domain = c.type_ == DrcConstraintT::LengthConstraint
            || c.type_ == DrcConstraintT::NetChainLengthConstraint
            || c.type_ == DrcConstraintT::NetChainStubLengthConstraint
            || c.type_ == DrcConstraintT::SkewConstraint;

        if c.type_ == DrcConstraintT::DisallowConstraint {
            token = self.lexer.next_tok()?;

            while token != T::T_RIGHT {
                if token == T::T_STRING {
                    token = self.lexer.cur_str_as_token();
                }

                match token {
                    T::T_track => c.disallow_flags |= drc_disallow::TRACKS,
                    T::T_via => {
                        c.disallow_flags |= drc_disallow::THROUGH_VIAS
                            | drc_disallow::BLIND_VIAS
                            | drc_disallow::BURIED_VIAS
                            | drc_disallow::MICRO_VIAS
                    }
                    T::T_through_via => c.disallow_flags |= drc_disallow::THROUGH_VIAS,
                    T::T_blind_via => c.disallow_flags |= drc_disallow::BLIND_VIAS,
                    T::T_buried_via => c.disallow_flags |= drc_disallow::BURIED_VIAS,
                    T::T_micro_via => c.disallow_flags |= drc_disallow::MICRO_VIAS,
                    T::T_pad => c.disallow_flags |= drc_disallow::PADS,
                    T::T_zone => c.disallow_flags |= drc_disallow::ZONES,
                    T::T_text => c.disallow_flags |= drc_disallow::TEXTS,
                    T::T_graphic => c.disallow_flags |= drc_disallow::GRAPHICS,
                    T::T_hole => c.disallow_flags |= drc_disallow::HOLES,
                    T::T_footprint => c.disallow_flags |= drc_disallow::FOOTPRINTS,

                    T::T_EOF => {
                        self.report_error("Missing ')'.", 0)?;
                        return Ok(());
                    }

                    _ => {
                        self.expected(
                            "track, via, through_via, blind_via, micro_via, buried_via, pad, \
                             zone, text, graphic, hole, or footprint.",
                        )?;
                        return Ok(());
                    }
                }

                token = self.lexer.next_tok()?;
            }

            if self.lexer.cur_tok() != T::T_RIGHT {
                self.report_error("Missing ')'.", 0)?;
            }

            rule.add_constraint(&mut c, None);
            return Ok(());
        } else if c.type_ == DrcConstraintT::NetChainReturnPathConstraint {
            // (constraint return_path (layer "B.Cu") (net "GND"))
            token = self.lexer.next_tok()?;

            while token != T::T_RIGHT {
                if token == T::T_LEFT {
                    token = self.lexer.next_tok()?;
                }

                match token {
                    T::T_layer => {
                        self.lexer.need_symbol_or_number()?;
                        c.reference_layer = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    }

                    T::T_net => {
                        self.lexer.need_symbol_or_number()?;
                        c.reference_net = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    }

                    T::T_EOF => {
                        self.report_error("Missing ')'.", 0)?;
                        return Ok(());
                    }

                    _ => {
                        // Unknown sub-option; skip its subtree.  (The C++ loop `while( NextTok()
                        // != T_RIGHT );` never ends at the end of the input; here it stops
                        // there and the T_EOF case above reports the missing ')'.)
                        loop {
                            let t = self.lexer.next_tok()?;

                            if t == T::T_RIGHT || t == T::T_EOF {
                                break;
                            }
                        }
                    }
                }

                token = self.lexer.next_tok()?;
            }

            rule.add_constraint(&mut c, None);
            return Ok(());
        } else if c.type_ == DrcConstraintT::ZoneConnectionConstraint {
            token = self.lexer.next_tok()?;

            if token == T::T_STRING {
                token = self.lexer.cur_str_as_token();
            }

            match token {
                T::T_solid => c.zone_connection = ZoneConnection::Full,
                T::T_thermal_reliefs => c.zone_connection = ZoneConnection::Thermal,
                T::T_none => c.zone_connection = ZoneConnection::None,

                T::T_EOF => {
                    self.report_error("Missing ')'.", 0)?;
                    return Ok(());
                }

                _ => {
                    self.expected("solid, thermal_reliefs or none.")?;
                    return Ok(());
                }
            }

            if self.lexer.next_tok()? != T::T_RIGHT {
                self.report_error("Missing ')'.", 0)?;
            }

            rule.add_constraint(&mut c, None);
            return Ok(());
        } else if c.type_ == DrcConstraintT::MinResolvedSpokesConstraint {
            // We don't use a min/max/opt structure here because it would give a strong
            // implication that you could specify the optimal number of spokes.  We don't want
            // to open that door because the spoke generator is highly optimized around being
            // able to "cheat" off of a cartesian coordinate system.

            token = self.lexer.next_tok()?;

            if token == T::T_NUMBER {
                let value = strtol_int(self.lexer.cur_text());
                c.value.set_min(value);

                if self.lexer.next_tok()? != T::T_RIGHT {
                    self.report_error("Missing ')'.", 0)?;
                }
            } else {
                self.expected("number")?; // translate "number"; it is not a token
            }

            rule.add_constraint(&mut c, None);
            return Ok(());
        } else if c.type_ == DrcConstraintT::AssertionConstraint {
            token = self.lexer.next_tok()?;

            if token == T::T_RIGHT {
                self.report_error("Missing assertion expression.", 0)?;
            }

            let mut test = None;

            if DrcRulesLexer::is_symbol(token) {
                let mut condition = DrcRuleCondition::new(&self.lexer.from_utf8());
                self.compile_condition(&mut condition);
                let requires_pair_items = condition.requires_pair_items();
                test = Some(Arc::new(condition));

                if requires_pair_items {
                    self.report_error("Item 'B' is not available in assertion expressions.", 0)?;
                }

                if self.lexer.next_tok()? != T::T_RIGHT {
                    self.report_error("Missing ')'.", 0)?;
                }
            } else {
                // translate "quoted expression"; it is not a token
                self.expected("quoted expression")?;
            }

            rule.add_constraint(&mut c, test);
            return Ok(());
        }

        token = self.lexer.next_tok()?;

        while token != T::T_RIGHT && token != T::T_EOF {
            if token != T::T_LEFT {
                self.report_error("Missing '('.", 0)?;
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_within_diff_pairs => {
                    if c.type_ == DrcConstraintT::SkewConstraint {
                        c.set_option(drc_constraint_options::SKEW_WITHIN_DIFF_PAIRS);
                    } else {
                        self.report_error("within_diff_pairs option invalid for constraint type.", 0)?;
                    }

                    if self.lexer.next_tok()? != T::T_RIGHT {
                        self.report_error("Missing ')'.", 0)?;
                    }
                }

                T::T_min | T::T_max | T::T_opt => {
                    let (missing, setter): (&str, fn(&mut DrcConstraint, i32)) = match token {
                        T::T_min => ("Missing min value.", |c, v| c.value.set_min(v)),
                        T::T_max => ("Missing max value.", |c, v| c.value.set_max(v)),
                        _ => ("Missing opt value.", |c, v| c.value.set_opt(v)),
                    };

                    // size_t offset = CurOffset() + GetTokenString( token ).length();
                    let offset = self.lexer.cur_offset()
                        + self.lexer.token_string(token).chars().count() as i32;
                    let expr = self.parse_expression()?;

                    if expr.is_empty() {
                        self.report_error(missing, 0)?;
                    } else {
                        let parsed = self.parse_value_with_units(
                            offset,
                            &expr,
                            unitless,
                            ratio,
                            implied_degrees,
                        )?;
                        self.validate_and_set_value_with_units(
                            &mut c,
                            allows_time_domain,
                            &mut units_type,
                            parsed,
                            setter,
                        )?;
                    }
                }

                T::T_EOF => {
                    self.report_error("Incomplete statement.", 0)?;
                    return Ok(());
                }

                _ => self.expected("min, max, opt, or within_diff_pairs")?,
            }

            token = self.lexer.next_tok()?;
        }

        rule.add_constraint(&mut c, None);
        Ok(())
    }

    /// Port of `DRC_RULES_PARSER::parseValueWithUnits`: evaluate the constant expression
    /// `expr` of a `(min ...)`-style value starting at `offset` of the current line.
    ///
    /// `unitless`: the value is expected to be unscaled (and have no units); `ratio`: a ratio
    /// (kept with three decimals); `implied_degrees`: degrees, which earlier versions allowed
    /// to be written without the unit.
    fn parse_value_with_units(
        &mut self,
        offset: i32,
        expr: &str,
        unitless: bool,
        ratio: bool,
        implied_degrees: bool,
    ) -> Result<ParsedValue, IoError> {
        let mut result = 0;
        let mut units = EdaUnits::Unscaled;

        let resolver: Box<dyn UnitResolver> = if unitless {
            Box::new(PcbExprUnitlessResolver)
        } else {
            Box::new(PcbExprUnitResolver::new(implied_degrees))
        };

        // The C++ error handler reports (or throws) from inside the evaluation; the messages
        // are collected and handled in order afterwards (the evaluation has no other effect
        // that could be observed in between, and a throw ends it).
        let mut errors: Vec<(String, i32)> = Vec::new();
        let ok;
        let result_as_double;
        let result_int;
        let result_units;

        {
            let mut evaluator: PcbExprEvaluator<'_, B> = PcbExprEvaluator::new(resolver);
            evaluator.set_error_callback(Some(Box::new(|message: &str, err_offset: i32| {
                errors.push((message.to_string(), err_offset));
            })));

            ok = evaluator.evaluate(expr);
            result_as_double = evaluator.result_as_double();
            result_int = evaluator.result();
            result_units = evaluator.units();
        }

        for (message, err_offset) in &errors {
            let (first, rest) = before_first(message, '|');

            if let Some(reporter) = self.reporter.as_deref_mut() {
                let msg = format!(
                    "ERROR: <a href='{}:{}'>{}</a>{}",
                    self.lexer.cur_line_number(),
                    offset + err_offset,
                    first,
                    rest
                );

                reporter.report(&msg, Severity::Error);
            } else {
                let msg = format!("ERROR: {first}{rest}");

                return Err(IoError::parse(
                    msg,
                    self.lexer.cur_source(),
                    self.lexer.cur_line(),
                    self.lexer.cur_line_number(),
                    self.lexer.cur_offset() + offset,
                ));
            }
        }

        if ok {
            // A ratio is fractional, so keep three decimals rather than rounding to a whole
            // number.
            result = if ratio {
                ki_round(result_as_double * 1000.0)
            } else {
                result_int
            };
            units = result_units;
        }

        Ok(ParsedValue {
            value: result,
            units,
        })
    }

    /// Port of `DRC_RULES_PARSER::parseLayer`.
    fn parse_layer(&mut self, source: &mut String) -> Result<Lset, IoError> {
        let mut ret_val = Lset::new();
        let token = self.lexer.next_tok()?;

        if token == T::T_RIGHT {
            self.report_error("Missing layer name or type.", 0)?;
            return Ok(Lset::all_cu_mask().clone());
        } else if token == T::T_outer {
            *source = self.lexer.token_string(token);
            ret_val = Lset::external_cu_mask().clone();
        } else if token == T::T_inner {
            *source = self.lexer.token_string(token);
            ret_val = Lset::internal_cu_mask().clone();
        } else {
            let layer_name = self.lexer.from_utf8();

            for (text, layer) in &self.layer_choices {
                // entry.GetText().Matches( layerName ): wildcards, case sensitive
                if wild_compare_string(&layer_name, text, true) {
                    *source = layer_name.clone();
                    ret_val.set(*layer);
                }
            }

            if !ret_val.any() {
                if !self.check_unresolved_text_variable()? {
                    let msg = format!("Unrecognized layer '{layer_name}'.");
                    self.report_error(&msg, 0)?;
                }

                ret_val.set(PcbLayerId::Rescue);
            }
        }

        if self.lexer.next_tok()? != T::T_RIGHT {
            self.report_error("Missing ')'.", 0)?;
        }

        Ok(ret_val)
    }

    /// Port of `DRC_RULES_PARSER::parseSeverity`.
    fn parse_severity(&mut self) -> Result<Severity, IoError> {
        let mut ret_val = Severity::Undefined;

        let token = self.lexer.next_tok()?;

        if token == T::T_RIGHT || token == T::T_EOF {
            self.report_error("Missing severity name.", 0)?;
            return Ok(Severity::Undefined);
        }

        match token {
            T::T_ignore => ret_val = Severity::Ignore,
            T::T_warning => ret_val = Severity::Warning,
            T::T_error => ret_val = Severity::Error,
            T::T_exclusion => ret_val = Severity::Exclusion,

            _ => self.expected("ignore, warning, error, or exclusion")?,
        }

        if self.lexer.next_tok()? != T::T_RIGHT {
            self.report_error("Missing ')'.", 0)?;
        }

        Ok(ret_val)
    }
}

/// `std::string::find( needle, from )`.
fn find_bytes(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }

    haystack[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

/// `std::string::find( ch, from )`.
fn find_byte(haystack: &[u8], ch: u8, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }

    haystack[from..]
        .iter()
        .position(|&b| b == ch)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strtol_like_c() {
        assert_eq!(strtol_int("20200610"), 20200610);
        assert_eq!(strtol_int("1"), 1);
        assert_eq!(strtol_int("-3"), -3);
        assert_eq!(strtol_int("2.5"), 2);
        assert_eq!(strtol_int("1e3"), 1);
        assert_eq!(strtol_int(".5"), 0);
        assert_eq!(strtol_int("99999999999999999999"), -1);
        assert_eq!(strtol_int("4294967297"), 1);
    }

    #[test]
    fn find_helpers() {
        assert_eq!(find_bytes(b"a${b}${c}", b"${", 0), Some(1));
        assert_eq!(find_bytes(b"a${b}${c}", b"${", 3), Some(5));
        assert_eq!(find_bytes(b"a$", b"${", 0), None);
        assert_eq!(find_byte(b"a${b}", b'}', 3), Some(4));
    }
}
