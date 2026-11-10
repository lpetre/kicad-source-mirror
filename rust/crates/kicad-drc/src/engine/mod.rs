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

//! Port of `pcbnew/drc/drc_engine.h` / `pcbnew/drc/drc_engine.cpp`: the engine core
//! (package DRC-03: constructor, `InitEngine`, `compileRules`, `RunTests`, error limits,
//! `ReportViolation`, the constraint queries) and the rule file loading of
//! `DRC_ENGINE::loadRules` ([`load_rules`], package DRC-20). `loadImplicitRules` is in
//! [`implicit_rules`], `EvalRules` and its helpers (package DRC-04) in [`eval_rules`].
//!
//! The C++ `DRC_ENGINE` is split in two (`rust/docs/design/board-model.md` section 7):
//!
//! * [`DrcEngine`]: the rule set, the compiled constraint lists and the memo caches. It is the
//!   board model's `RuleResolver` (shared, `Sync`; a DRC run holds `ctx.rules = Some(&engine)`).
//! * [`DrcRunState`]: what a run mutates (per-code error limits, violation handler, log and
//!   progress reporters, the run flags and units), passed as `&mut` to the providers.

pub mod diff_pair;
pub mod eval_rules;
pub mod implicit_condition;
pub mod implicit_rules;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use kicad_core::text_vars::{ResolutionContext, expand_text_vars};
use kicad_core::{EdaUnits, Lset, PcbLayerId, Severity};
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{
    DRCE_CLEARANCE, DRCE_FIRST, DRCE_LAST, DRCE_UNCONNECTED_ITEMS, DrcRuleInfo,
};
use kicad_pcb::pcb_marker::{PcbMarker, RcItem};
use kicad_pcb::{Board, BoardCtx, ConnectivityQuery, ItemId, ItemRef, RuleResolver};
use kicad_sexpr::{IoError, ParseError};

use crate::constraint_types::DrcConstraintT;
use crate::pcbexpr::evaluator::{PcbExprBinder, PcbExprUcode};
use crate::reporter::{Reporter, WxStringReporter, reborrow};
use crate::rule::{DrcConstraint, DrcRule, is_component_class_selector};
use crate::rule_condition::DrcRuleCondition;
use crate::rule_parser::{DrcRulesParser, LayerChoices};

pub use eval_rules::{DrcClearanceBatch, OwnClearanceCache};
use implicit_condition::{EngineCondition, ImplicitCondition};
use implicit_rules::{TuningProfiles, load_implicit_rules};

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
    load_rules_with::<PcbExprUcode>(board, file_text, path, layer_choices, log_reporter)
}

/// [`load_rules`] with the PCB expression binding `B` (see [`DrcEngine::init_engine_with`]).
pub fn load_rules_with<B: PcbExprBinder>(
    board: &Board,
    file_text: Option<&str>,
    path: &str,
    layer_choices: LayerChoices,
    log_reporter: Option<&mut dyn Reporter>,
) -> Result<Vec<DrcRule>, IoError>
where
    B::VarRef: Send + Sync,
{
    let mut rules = Vec::new();

    if let Some(file_text) = file_text {
        let variant = board.current_variant.clone();
        let resolve = |token: &mut String| board.resolve_text_var(token, &variant, 0);
        let rules_text = prepare_rules_text(
            file_text,
            |line| board.convert_cross_references_to_kiids(line),
            &resolve,
        );

        let mut parser: DrcRulesParser<'_, B> = DrcRulesParser::new(&rules_text, path);
        parser.set_layer_choices(layer_choices);
        parser.parse(&mut rules, log_reporter)?;
    }

    Ok(rules)
}

// ---- the engine ----------------------------------------------------------------------------

// wxListBox's performance degrades horrifically with very large datasets.  It's not clear
// they're useful to the user anyway.

/// `ERROR_LIMIT`: violations reported per error code.
pub const ERROR_LIMIT: i32 = 199;

/// `EXTENDED_ERROR_LIMIT`: violations reported for `DRCE_CLEARANCE` and
/// `DRCE_UNCONNECTED_ITEMS`.
pub const EXTENDED_ERROR_LIMIT: i32 = 499;

/// The names (`DRC_TEST_PROVIDER::GetName()`) of the test providers in the order of the C++
/// registry `DRC_TEST_PROVIDER_REGISTRY::Instance().GetTestProviders()`, which is the run order
/// of `RunTests`. Registration happens in the static initialisers of the provider translation
/// units, i.e. in link order (`PCBNEW_DRC_SRCS`); the order was read from the reference build
/// with `pcb-dump board --sections engine` (`engine|providers|...`). `schematic_parity` is
/// not ported (tier: out of scope) but keeps its slot.
pub const PROVIDER_ORDER: [&str; 30] = [
    "annular_width",
    "disallow",
    "creepage",
    "connectivity",
    "copper width",
    "clearance",
    "physical_clearance",
    "courtyard_clearance",
    "edge_clearance",
    "footprint checks",
    "hole_to_hole_clearance",
    "hole_size",
    "library_parity",
    "schematic_parity",
    "miscellaneous",
    "net chain tuning profiles",
    "text_dimensions",
    "track angle",
    "width",
    "segment_length",
    "zone connections",
    "diameter",
    "microvia_stack",
    "via_stitch_overlap",
    "solder_mask_issues",
    "silk_clearance",
    "length",
    "diff_pair_coupling",
    "sliver checker",
    "text_mirroring",
];

/// Port of `DRC_ENGINE::DRC_ENGINE_CONSTRAINT`: one entry of the per-constraint-type lists.
#[derive(Clone, Debug)]
pub struct DrcEngineConstraint {
    /// `layerTest`: a copy of the rule's layer condition.
    pub layer_test: Lset,
    /// `condition`: `None` when the rule has none or its expression is empty.
    pub condition: Option<EngineCondition>,
    /// `parentRule`.
    pub parent_rule: Arc<DrcRule>,
    /// `constraint` (a copy).
    pub constraint: DrcConstraint,
    /// `implicitKeepoutZone`: pre-resolved zone of an implicit keepout disallow rule.
    ///
    /// Duplicate zone UUIDs make an item-by-id lookup return the wrong same-UUID zone and
    /// defeat self-exclusion, so use the rule's own zone.
    pub implicit_keepout_zone: Option<ItemId>,
    /// `constraint.GetParentRule()` as the rule data the pointer gives access to (the
    /// current state of `parentRule`; `None` for a constraint without a parent rule).
    pub rule_info: Option<DrcRuleInfo>,
    /// `constraint.m_Test`: the assertion condition of an `ASSERTION` constraint.
    pub test: Option<Arc<DrcRuleCondition>>,
}

/// The per-constraint-type lists (`std::map<DRC_CONSTRAINT_T, std::vector<...>>`).
pub type ConstraintMap = BTreeMap<DrcConstraintT, Vec<Arc<DrcEngineConstraint>>>;

/// Port of the rule-set half of `DRC_ENGINE`: rules, compiled constraint lists, the netclass
/// clearance fast-path table and the memo caches. Implements the board model's
/// [`RuleResolver`]. See the module comment for the split.
pub struct DrcEngine {
    /// `m_rules`: implicit rules first, then the rule file's rules in file order.
    rules: Vec<Arc<DrcRule>>,
    /// `m_rulesValid`.
    rules_valid: bool,
    /// `m_constraintMap`.
    constraint_map: ConstraintMap,
    /// `m_explicitConstraints`: the entries of `m_constraintMap` from non-implicit rules.
    explicit_constraints: ConstraintMap,
    /// `m_netclassClearances`: netclass name -> its implicit clearance rule (lookups only).
    netclass_clearances: BTreeMap<String, Arc<DrcRule>>,
    /// The index in `rules` of each rule of `netclass_clearances` (the identity the fast path
    /// of `EvalRules` puts into `DrcRuleInfo::rule_index`, see [`DrcRule::info_in_engine`]).
    netclass_clearance_indices: BTreeMap<String, u32>,
    /// `m_ownClearanceCache`: (uuid, layer) -> clearance (lookups only), stamped with the board
    /// time stamp (see [`OwnClearanceCache`]).
    own_clearance_cache: Mutex<OwnClearanceCache>,
    /// `m_hasExplicitClearanceRules`.
    has_explicit_clearance_rules: bool,
    /// `m_hasGeometryDependentRules`.
    has_geometry_dependent_rules: bool,
    /// `m_hasDiffPairClearanceOverrides`.
    has_diff_pair_clearance_overrides: bool,
    /// How `compileRules` compiles a non-implicit condition (`condition->Compile(
    /// &error_semaphore )`); see [`DrcEngine::set_condition_compiler`].
    condition_compiler: ConditionCompiler,
}

/// `DRC_RULE_CONDITION::Compile( aReporter )` as the engine calls it for rule conditions
/// that are not native implicit conditions.
pub type ConditionCompiler = fn(&mut DrcRuleCondition, &mut dyn Reporter) -> bool;

/// The [`ConditionCompiler`] of the binding `B`: `condition.compile_with::<B>( reporter )`.
fn compile_with_binding<B: PcbExprBinder>(
    condition: &mut DrcRuleCondition,
    reporter: &mut dyn Reporter,
) -> bool
where
    B::VarRef: Send + Sync,
{
    condition.compile_with::<B>(Some(reporter), 0, 0)
}

impl Default for DrcEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DrcEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DrcEngine")
            .field("rules", &self.rules.len())
            .field("rules_valid", &self.rules_valid)
            .finish()
    }
}

/// `DRC_TEST_PROVIDER::RunTests` as the engine calls it: implemented by the test providers
/// (package DRC-02 and the provider packages).
pub trait DrcTestProviderRun {
    /// `GetName()`.
    fn name(&self) -> &str;

    /// `RunTests( aUnits )` (`SetUserUnits( aUnits ); return Run();`): false when cancelled.
    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool;
}

/// `PROGRESS_REPORTER`, the members the engine forwards to. Every method defaults to the
/// behaviour of a run without a progress reporter.
pub trait ProgressReporter {
    /// `KeepRefreshing( aWait )`: false when the user cancelled.
    fn keep_refreshing(&mut self, _wait: bool) -> bool {
        true
    }

    /// `AdvanceProgress()`.
    fn advance_progress(&mut self) {}

    /// `SetMaxProgress( aSize )`.
    fn set_max_progress(&mut self, _size: i32) {}

    /// `SetCurrentProgress( aProgress )`.
    fn set_current_progress(&mut self, _progress: f64) {}

    /// `AdvancePhase( aMessage )`.
    fn advance_phase(&mut self, _message: &str) {}

    /// `IsCancelled()`.
    fn is_cancelled(&self) -> bool {
        false
    }
}

/// Port of `DRC_VIOLATION_HANDLER`: receives each violation that passed the error limit with
/// its marker position, marker layer and the optional path generator (C++
/// `std::function<void( PCB_MARKER* )>`).
pub type DrcViolationHandler<'r> =
    Box<dyn FnMut(&Arc<RcItem>, Vector2I, i32, Option<&dyn Fn(&mut PcbMarker)>) + 'r>;

/// The run half of `DRC_ENGINE`: everything a DRC run mutates (see the module comment).
pub struct DrcRunState<'r> {
    /// `m_errorLimits`, indexed by error code (size `DRCE_LAST + 1`).
    error_limits: Vec<i32>,
    /// `m_violationHandler`.
    violation_handler: Option<DrcViolationHandler<'r>>,
    /// `m_logReporter`.
    log_reporter: Option<&'r mut dyn Reporter>,
    /// `m_progressReporter`.
    progress_reporter: Option<&'r mut dyn ProgressReporter>,
    /// `m_reportAllTrackErrors`.
    report_all_track_errors: bool,
    /// `m_testFootprints`.
    test_footprints: bool,
    /// `UNITS_PROVIDER::GetUserUnits()` (`EDA_UNITS::MM` initially).
    user_units: EdaUnits,
}

impl Default for DrcRunState<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'r> DrcRunState<'r> {
    /// The run members of `DRC_ENGINE::DRC_ENGINE()`: every error limit `ERROR_LIMIT`, no
    /// handler, no reporters, millimetres.
    pub fn new() -> Self {
        let mut state = DrcRunState {
            error_limits: vec![0; (DRCE_LAST + 1) as usize],
            violation_handler: None,
            log_reporter: None,
            progress_reporter: None,
            report_all_track_errors: false,
            test_footprints: false,
            user_units: EdaUnits::Mm,
        };

        state.reset_error_limits();
        state
    }

    /// The error limit reset at the end of `InitEngine` (both overloads): every code
    /// `ERROR_LIMIT`.
    pub fn reset_error_limits(&mut self) {
        for ii in DRCE_FIRST..=DRCE_LAST {
            self.error_limits[ii as usize] = ERROR_LIMIT;
        }
    }

    /// Port of `DRC_ENGINE::SetViolationHandler`.
    pub fn set_violation_handler(&mut self, handler: DrcViolationHandler<'r>) {
        self.violation_handler = Some(handler);
    }

    /// Port of `DRC_ENGINE::ClearViolationHandler`.
    pub fn clear_violation_handler(&mut self) {
        self.violation_handler = None;
    }

    /// Port of `DRC_ENGINE::SetLogReporter`.
    pub fn set_log_reporter(&mut self, reporter: Option<&'r mut dyn Reporter>) {
        self.log_reporter = reporter;
    }

    /// Port of `DRC_ENGINE::GetLogReporter`.
    pub fn log_reporter(&mut self) -> Option<&mut dyn Reporter> {
        reborrow(&mut self.log_reporter)
    }

    /// Port of `DRC_ENGINE::SetProgressReporter`.
    pub fn set_progress_reporter(&mut self, reporter: Option<&'r mut dyn ProgressReporter>) {
        self.progress_reporter = reporter;
    }

    /// Port of `DRC_ENGINE::GetReportAllTrackErrors`.
    pub fn report_all_track_errors(&self) -> bool {
        self.report_all_track_errors
    }

    /// Port of `DRC_ENGINE::GetTestFootprints`.
    pub fn test_footprints(&self) -> bool {
        self.test_footprints
    }

    /// `UNITS_PROVIDER::GetUserUnits`.
    pub fn user_units(&self) -> EdaUnits {
        self.user_units
    }

    /// `UNITS_PROVIDER::SetUserUnits`.
    pub fn set_user_units(&mut self, units: EdaUnits) {
        self.user_units = units;
    }

    /// Port of `DRC_ENGINE::IsErrorLimitExceeded`.
    pub fn is_error_limit_exceeded(&self, error_code: i32) -> bool {
        debug_assert!((0..=DRCE_LAST).contains(&error_code));
        self.error_limits[error_code as usize] <= 0
    }

    /// Port of `DRC_ENGINE::GetErrorLimit`: the number of violations of `error_code` that may
    /// still be reported.
    ///
    /// Providers which collect candidates before reporting them can use this to bound the
    /// collection instead of accumulating violations the report will discard.
    pub fn error_limit(&self, error_code: i32) -> i32 {
        debug_assert!((0..=DRCE_LAST).contains(&error_code));
        self.error_limits[error_code as usize].max(0)
    }

    /// Port of `DRC_ENGINE::ReportViolation`. `violating_test` is the name of the provider
    /// that found the violation (`aItem->GetViolatingTest()->GetName()`, only used by the log
    /// output).
    ///
    /// The per-code cap is enforced here, so at most `ERROR_LIMIT` (`EXTENDED_ERROR_LIMIT`)
    /// violations of a code reach the handler.
    pub fn report_violation(
        &mut self,
        item: &Arc<RcItem>,
        violating_test: &str,
        pos: Vector2I,
        marker_layer: i32,
        path_generator: Option<&dyn Fn(&mut PcbMarker)>,
    ) {
        {
            // Providers pre-check IsErrorLimitExceeded(), but that check is racy when items
            // are processed in parallel, letting many threads slip past the cap and overshoot
            // it.  Enforce the per-code limit atomically here so the reported count is exact
            // and reproducible regardless of worker scheduling.
            let code = item.error_code() as usize;

            if self.error_limits[code] <= 0 {
                return;
            }

            self.error_limits[code] -= 1;
        }

        if let Some(handler) = &mut self.violation_handler {
            handler(item, pos, marker_layer, path_generator);
        }

        if let Some(log_reporter) = &mut self.log_reporter {
            let mut msg = format!(
                "Test '{}': {} (code {})",
                violating_test,
                item.error_message(),
                item.error_code()
            );

            if let Some(rule) = item.violating_rule() {
                msg += &format!(", violating rule: '{}'", rule.name);
            }

            log_reporter.report(&msg, Severity::Undefined);

            log_reporter.report(
                &format!("  |- violating position ({}, {})", pos.x, pos.y),
                Severity::Undefined,
            );
        }
    }

    /// Port of `DRC_ENGINE::KeepRefreshing`.
    pub fn keep_refreshing(&mut self, wait: bool) -> bool {
        match &mut self.progress_reporter {
            None => true,
            Some(p) => p.keep_refreshing(wait),
        }
    }

    /// Port of `DRC_ENGINE::AdvanceProgress`.
    pub fn advance_progress(&mut self) {
        if let Some(p) = &mut self.progress_reporter {
            p.advance_progress();
        }
    }

    /// Port of `DRC_ENGINE::SetMaxProgress`.
    pub fn set_max_progress(&mut self, size: i32) {
        if let Some(p) = &mut self.progress_reporter {
            p.set_max_progress(size);
        }
    }

    /// Port of `DRC_ENGINE::ReportProgress`.
    pub fn report_progress(&mut self, progress: f64) -> bool {
        match &mut self.progress_reporter {
            None => true,
            Some(p) => {
                p.set_current_progress(progress);
                p.keep_refreshing(false)
            }
        }
    }

    /// Port of `DRC_ENGINE::ReportPhase`.
    pub fn report_phase(&mut self, message: &str) -> bool {
        match &mut self.progress_reporter {
            None => true,
            Some(p) => {
                p.advance_phase(message);
                p.keep_refreshing(false)
            }
        }
    }

    /// Port of `DRC_ENGINE::IsCancelled`.
    pub fn is_cancelled(&self) -> bool {
        self.progress_reporter
            .as_ref()
            .is_some_and(|p| p.is_cancelled())
    }
}

/// `PARSE_ERROR` / `FUTURE_FORMAT_ERROR` (a `PARSE_ERROR` subclass): what `InitEngine` catches.
fn is_parse_error(error: &IoError) -> bool {
    matches!(error, IoError::Parse(_) | IoError::FutureFormat(_))
}

/// A position in `m_rules` as `DrcRuleInfo::rule_index`. A rule list cannot reach 2^32
/// entries (each rule is parsed from text held in memory).
fn rule_index_u32(index: usize) -> u32 {
    u32::try_from(index).expect("rule index exceeds u32")
}

/// The marker loop at the start of both `InitEngine` overloads:
/// `drcItem->SetViolatingRule( nullptr )` for every board marker, so that `GetSeverity()`
/// falls back to the board design settings. The Rust marker holds a copy of the rule data, not
/// a pointer, but clearing it keeps the C++ severity behaviour. Markers without a rule are not
/// touched (the C++ assignment is a no-op for them).
fn clear_marker_violating_rules(board: &mut Board) {
    let ids: Vec<_> = board
        .markers()
        .filter(|(_, marker)| marker.rc_item().violating_rule().is_some())
        .map(|(id, _)| id)
        .collect();

    for id in ids {
        board.modify_marker(id, |marker, _env| {
            Arc::make_mut(&mut marker.rc_item).violating_rule = None;
        });
    }
}

impl DrcEngine {
    /// Port of `DRC_ENGINE::DRC_ENGINE`: no rules, rules not valid.
    pub fn new() -> Self {
        DrcEngine {
            rules: Vec::new(),
            rules_valid: false,
            constraint_map: BTreeMap::new(),
            explicit_constraints: BTreeMap::new(),
            netclass_clearances: BTreeMap::new(),
            netclass_clearance_indices: BTreeMap::new(),
            own_clearance_cache: Mutex::new(OwnClearanceCache::default()),
            has_explicit_clearance_rules: false,
            has_geometry_dependent_rules: false,
            has_diff_pair_clearance_overrides: false,
            condition_compiler: compile_with_binding::<PcbExprUcode>,
        }
    }

    /// Port of `DRC_ENGINE::InitEngine( const wxFileName& aRulePath )`.
    ///
    /// `rules_file` is the rule file's text and full path, `None` when the file does not exist
    /// (see [`load_rules`] for `layer_choices`). `tuning_profiles` are the project's tuning
    /// profiles (`PROJECT_FILE::TuningProfileParameters()`; not in the Rust project model yet).
    /// `log_reporter` is `m_logReporter`: with one, rule parse errors are reported to it
    /// instead of returned.
    ///
    /// On a parse error of the rule file the engine is left with the implicit rules only (and
    /// not valid), and the error is returned, like the C++ rethrow.
    pub fn init_engine(
        &mut self,
        board: &mut Board,
        rules_file: Option<(&str, &str)>,
        layer_choices: LayerChoices,
        tuning_profiles: &TuningProfiles,
        log_reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), IoError> {
        self.init_engine_with::<PcbExprUcode>(
            board,
            rules_file,
            layer_choices,
            tuning_profiles,
            log_reporter,
        )
    }

    /// [`init_engine`](Self::init_engine) with the PCB expression binding `B` for the user
    /// rule conditions (rule file parser and `compileRules`). Not a C++ API: it lets tests
    /// compile user rule conditions with a stand-in binding (e.g. a syntax-only one) while
    /// the expression binding (package DRC-21) is pending. Later `compileRules` passes
    /// ([`init_engine_with_rule`](Self::init_engine_with_rule)) keep using `B`.
    pub fn init_engine_with<B: PcbExprBinder>(
        &mut self,
        board: &mut Board,
        rules_file: Option<(&str, &str)>,
        layer_choices: LayerChoices,
        tuning_profiles: &TuningProfiles,
        mut log_reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), IoError>
    where
        B::VarRef: Send + Sync,
    {
        self.condition_compiler = compile_with_binding::<B>;

        // m_testProviders = DRC_TEST_PROVIDER_REGISTRY::Instance().GetTestProviders(): the
        // providers are created per run (see DrcTestProviderRun); only the log remains.
        for name in PROVIDER_ORDER {
            if let Some(r) = reborrow(&mut log_reporter) {
                r.report(
                    &format!("Create DRC provider: '{name}'"),
                    Severity::Undefined,
                );
            }
        }

        // Existing markers may hold raw pointers to DRC_RULEs we're about to destroy.
        // Null them out so GetSeverity() falls back to the board design settings.
        clear_marker_violating_rules(board);

        self.rules.clear();
        self.rules_valid = false;
        self.constraint_map.clear();

        {
            self.lock_own_clearance_cache().clear();
            self.netclass_clearances.clear();
            self.netclass_clearance_indices.clear();
        }

        self.has_explicit_clearance_rules = false;
        self.has_diff_pair_clearance_overrides = false;
        self.explicit_constraints.clear();

        board.increment_time_stamp(); // Clear board-level caches

        // attempt to load full set of rules (implicit + user rules)
        let result = (|| -> Result<(), IoError> {
            self.load_implicit_rules(board, tuning_profiles);
            self.load_rules_file::<B>(
                board,
                rules_file,
                layer_choices,
                reborrow(&mut log_reporter),
            )?;
            self.compile_rules(reborrow(&mut log_reporter))
        })();

        if let Err(original_parse_error) = result {
            if !is_parse_error(&original_parse_error) {
                return Err(original_parse_error);
            }

            self.rules.clear();

            // try again with just our implicit rules
            self.load_implicit_rules(board, tuning_profiles);

            if self.compile_rules(reborrow(&mut log_reporter)).is_err() {
                debug_assert!(false, "Compiling implicit rules failed.");
            }

            return Err(original_parse_error);
        }

        // (the error limits are reset to ERROR_LIMIT here in C++: they live in DrcRunState,
        // see DrcRunState::reset_error_limits)

        self.rules_valid = true;
        Ok(())
    }

    /// Port of `DRC_ENGINE::InitEngine( const std::shared_ptr<DRC_RULE>& rule )`: the engine
    /// with this single rule (used by the rule editor's "show matches").
    ///
    /// The C++ fills `m_testProviders` from `DRC_SHOWMATCHES_PROVIDER_REGISTRY`, in which no
    /// provider registers (no `DRC_REGISTER_SHOWMATCHES_PROVIDER` instance exists), so nothing
    /// is logged. Note that, unlike the file overload, the C++ does not clear
    /// `m_ownClearanceCache`, `m_explicitConstraints` and the `m_has*` flags here; neither does
    /// the port.
    pub fn init_engine_with_rule(
        &mut self,
        board: &mut Board,
        rule: Arc<DrcRule>,
        log_reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), IoError> {
        // Existing markers may hold raw pointers to DRC_RULEs we're about to destroy.
        // Null them out so GetSeverity() falls back to the board design settings.
        clear_marker_violating_rules(board);

        self.rules.clear();
        self.rules_valid = false;
        self.constraint_map.clear();

        // The netclass clearance cache points at the rules just destroyed
        self.netclass_clearances.clear();
        self.netclass_clearance_indices.clear();

        board.increment_time_stamp(); // Clear board-level caches

        self.rules.push(rule);
        self.compile_rules(log_reporter)?;

        // (error limits: see DrcRunState::reset_error_limits)

        self.rules_valid = true;
        Ok(())
    }

    /// `loadImplicitRules()`: appends the implicit rules and fills `m_netclassClearances`.
    fn load_implicit_rules(&mut self, board: &mut Board, tuning_profiles: &TuningProfiles) {
        let mut rules = Vec::new();
        let out = load_implicit_rules(board, tuning_profiles, &mut rules);
        let first = self.rules.len();

        self.rules.extend(rules.into_iter().map(Arc::new));

        for (name, index) in out.netclass_clearances {
            self.netclass_clearance_indices
                .insert(name.clone(), rule_index_u32(first + index));
            self.netclass_clearances
                .insert(name, Arc::clone(&self.rules[first + index]));
        }

        if out.has_diff_pair_clearance_overrides {
            self.has_diff_pair_clearance_overrides = true;
        }
    }

    /// `loadRules( aPath )`: appends the rule file's rules if it parses.
    fn load_rules_file<B: PcbExprBinder>(
        &mut self,
        board: &Board,
        rules_file: Option<(&str, &str)>,
        layer_choices: LayerChoices,
        log_reporter: Option<&mut dyn Reporter>,
    ) -> Result<(), IoError>
    where
        B::VarRef: Send + Sync,
    {
        let (text, path) = match rules_file {
            Some((text, path)) => (Some(text), path),
            None => (None, ""),
        };

        let rules = load_rules_with::<B>(board, text, path, layer_choices, log_reporter)?;
        self.rules.extend(rules.into_iter().map(Arc::new));
        Ok(())
    }

    /// Port of `DRC_ENGINE::compileRules`.
    ///
    /// Implicit rule conditions are recognised and evaluated natively
    /// ([`ImplicitCondition`]); all other conditions are compiled with the PCB expression
    /// binding. As in C++, a compile error stops the loop with a `PARSE_ERROR` and leaves the
    /// constraints of the rules before it in the lists.
    ///
    /// The rule copies of every entry (`rule_info` and the four rule slots of `constraint`)
    /// carry the rule's index in `m_rules` as its identity (`DrcRuleInfo::rule_index`). After a
    /// failed `InitEngine` the entries left over from the first pass keep the indices of the
    /// discarded rule list (in C++ they keep the discarded rules alive through their
    /// `shared_ptr`), so such an index may name a different rule of the current list.
    fn compile_rules(&mut self, log_reporter: Option<&mut dyn Reporter>) -> Result<(), IoError> {
        if let Some(r) = log_reporter {
            r.report("Compiling Rules", Severity::Undefined);
        }

        let mut error_semaphore = WxStringReporter::new();

        for index in 0..self.rules.len() {
            let mut condition: Option<EngineCondition> = None;

            let expression = self.rules[index]
                .condition
                .as_ref()
                .map(|c| c.expression().to_string())
                .unwrap_or_default();

            if !expression.is_empty() {
                let native = if self.rules[index].is_implicit() {
                    ImplicitCondition::parse(&expression)
                } else {
                    None
                };

                condition = Some(match native {
                    Some(native) => EngineCondition::Implicit {
                        expression: expression.clone(),
                        condition: Arc::new(native),
                    },
                    None => EngineCondition::Compiled(self.compile_condition(
                        index,
                        &expression,
                        &mut error_semaphore,
                    )),
                });
            }

            if error_semaphore.has_message_of_severity(Severity::Error as i32) {
                // THROW_PARSE_ERROR( wxT( "Parse error" ), rule->m_Name, expression, 0, 0 )
                let rule = &self.rules[index];
                return Err(IoError::Parse(Box::new(ParseError::new(
                    "Parse error",
                    &rule.name,
                    &expression,
                    0,
                    0,
                ))));
            }

            let rule = Arc::clone(&self.rules[index]);
            let rule_index = rule_index_u32(index);

            for (constraint_index, constraint) in rule.constraints.iter().enumerate() {
                let implicit_keepout_zone = if rule.is_implicit()
                    && constraint.type_ == DrcConstraintT::DisallowConstraint
                {
                    rule.implicit_item
                        .filter(|id| id.kind() == kicad_pcb::ItemKind::Zone)
                } else {
                    None
                };

                // The copy's rule pointers are pointers to this rule (`AddConstraint`).
                let mut constraint_copy = constraint.clone();

                for slot in [
                    &mut constraint_copy.parent_rule,
                    &mut constraint_copy.min_rule,
                    &mut constraint_copy.opt_rule,
                    &mut constraint_copy.max_rule,
                ]
                .into_iter()
                .flatten()
                {
                    slot.rule_index = Some(rule_index);
                }

                let engine_constraint = Arc::new(DrcEngineConstraint {
                    layer_test: rule.layer_condition.clone(),
                    condition: condition.clone(),
                    parent_rule: Arc::clone(&rule),
                    constraint: constraint_copy,
                    implicit_keepout_zone,
                    rule_info: constraint
                        .parent_rule
                        .as_ref()
                        .map(|_| rule.info_in_engine(rule_index)),
                    test: rule
                        .constraint_tests
                        .get(constraint_index)
                        .cloned()
                        .flatten(),
                });

                self.constraint_map
                    .entry(constraint.type_)
                    .or_default()
                    .push(engine_constraint);
            }
        }

        self.has_explicit_clearance_rules = false;
        self.has_geometry_dependent_rules = false;
        self.explicit_constraints.clear();

        for (&constraint_type, rule_list) in &self.constraint_map {
            for c in rule_list {
                if !c.parent_rule.is_implicit() {
                    self.explicit_constraints
                        .entry(constraint_type)
                        .or_default()
                        .push(Arc::clone(c));

                    if constraint_type == DrcConstraintT::ClearanceConstraint {
                        self.has_explicit_clearance_rules = true;
                    }

                    if !self.has_geometry_dependent_rules
                        && c.condition
                            .as_ref()
                            .is_some_and(|c| c.has_geometry_dependent_functions())
                    {
                        self.has_geometry_dependent_rules = true;
                    }
                }
            }
        }

        Ok(())
    }

    /// `condition->Compile( &error_semaphore )` on the compiled path. The rule's condition is
    /// compiled in place when the engine holds the only reference to it, else a compiled copy
    /// replaces it in the rule.
    fn compile_condition(
        &mut self,
        index: usize,
        expression: &str,
        error_semaphore: &mut WxStringReporter,
    ) -> Arc<DrcRuleCondition> {
        let compiler = self.condition_compiler;
        let compile = |c: &mut DrcRuleCondition, reporter: &mut WxStringReporter| {
            compiler(c, reporter);
        };

        let rule_arc = &mut self.rules[index];

        if Arc::get_mut(rule_arc).is_none() {
            // only possible after a failed first compile pass; rebuild an owned rule
            *rule_arc = Arc::new((**rule_arc).clone());
        }

        let rule = Arc::get_mut(rule_arc).expect("unique rule");
        let cond_arc = rule.condition.as_mut().expect("condition");

        match Arc::get_mut(cond_arc) {
            Some(c) => compile(c, error_semaphore),
            None => {
                let mut c = DrcRuleCondition::new(expression);
                compile(&mut c, error_semaphore);
                *cond_arc = Arc::new(c);
            }
        }

        Arc::clone(cond_arc)
    }

    fn lock_own_clearance_cache(&self) -> std::sync::MutexGuard<'_, OwnClearanceCache> {
        self.own_clearance_cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The first steps of `DRC_ENGINE::RunTests`, up to the board time stamp increment: user
    /// units, the two run flags, the per-code error limits (0 for ignored codes,
    /// `EXTENDED_ERROR_LIMIT` for clearance and unconnected items, else `ERROR_LIMIT`).
    pub fn prepare_run(
        &self,
        run: &mut DrcRunState<'_>,
        board: &mut Board,
        units: EdaUnits,
        report_all_track_errors: bool,
        test_footprints: bool,
    ) {
        run.set_user_units(units);

        run.report_all_track_errors = report_all_track_errors;
        run.test_footprints = test_footprints;

        let bds = board.design_settings();

        for ii in DRCE_FIRST..=DRCE_LAST {
            run.error_limits[ii as usize] = if bds.ignore(ii) {
                0
            } else if ii == DRCE_CLEARANCE || ii == DRCE_UNCONNECTED_ITEMS {
                EXTENDED_ERROR_LIMIT
            } else {
                ERROR_LIMIT
            };
        }

        // DRC_TEST_PROVIDER::Init(): the static item type lists of the provider base
        // (package DRC-02) are constants in the port.

        board.increment_time_stamp(); // Invalidate all caches...
    }

    /// The provider loop of `DRC_ENGINE::RunTests`: each provider in registry order; a provider
    /// returning false (cancelled) ends the loop.
    pub fn run_providers(
        &self,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
        providers: &mut [&mut dyn DrcTestProviderRun],
    ) {
        for provider in providers.iter_mut() {
            if let Some(r) = &mut run.log_reporter {
                r.report(
                    &format!("Run DRC provider: '{}'", provider.name()),
                    Severity::Undefined,
                );
            }

            if !provider.run_tests(self, run, ctx, units) {
                break;
            }
        }
    }

    /// Port of `DRC_ENGINE::RunTests( aUnits, aReportAllTrackErrors, aTestFootprints )`.
    ///
    /// `regenerate_caches` performs the steps between the time stamp increment and the
    /// providers, which belong to other packages: `DRC_CACHE_GENERATOR::Run()` (DRC-05),
    /// `GetComponentClassManager().ForceComponentClassRecalculation()` and the from-to cache
    /// rebuild of the connectivity; it returns false when cancelled (then no provider runs).
    /// `providers` must be in [`PROVIDER_ORDER`].
    #[allow(clippy::too_many_arguments)]
    pub fn run_tests(
        &self,
        run: &mut DrcRunState<'_>,
        board: &mut Board,
        connectivity: Option<&dyn ConnectivityQuery>,
        units: EdaUnits,
        report_all_track_errors: bool,
        test_footprints: bool,
        regenerate_caches: impl FnOnce(&mut Board, &DrcEngine) -> bool,
        providers: &mut [&mut dyn DrcTestProviderRun],
    ) {
        self.prepare_run(run, board, units, report_all_track_errors, test_footprints);

        if !regenerate_caches(board, self) {
            return;
        }

        let timestamp = board.time_stamp();

        {
            let mut ctx = BoardCtx::bare(board).with_rules(self);

            if let Some(c) = connectivity {
                ctx = ctx.with_connectivity(c);
            }

            self.run_providers(run, &ctx, units, providers);
        }

        // DRC tests are multi-threaded; anything that causes us to attempt to re-generate the
        // caches while DRC is running is problematic.
        debug_assert_eq!(timestamp, board.time_stamp());
    }

    // ---- accessors and queries ----------------------------------------------------------

    /// `m_rules`, in evaluation order.
    pub fn rules(&self) -> &[Arc<DrcRule>] {
        &self.rules
    }

    /// The rule behind a `DrcRuleInfo::rule_index` (what dereferencing the C++ `DRC_RULE*`
    /// gives access to: `m_Name`, the constraints, ...). `None` for an index that is not in
    /// the current rule list.
    pub fn rule(&self, rule_index: u32) -> Option<&Arc<DrcRule>> {
        self.rules.get(rule_index as usize)
    }

    /// The netclass fast path's `m_netclassClearances[ aName ]` lookup with the identity of
    /// the rule: its index in `m_rules`.
    pub(crate) fn netclass_clearance_rule_indexed(
        &self,
        netclass_name: &str,
    ) -> Option<(u32, &Arc<DrcRule>)> {
        let rule = self.netclass_clearances.get(netclass_name)?;
        let index = *self.netclass_clearance_indices.get(netclass_name)?;
        Some((index, rule))
    }

    /// `m_constraintMap`.
    pub fn constraint_map(&self) -> &ConstraintMap {
        &self.constraint_map
    }

    /// `m_explicitConstraints`.
    pub fn explicit_constraints(&self) -> &ConstraintMap {
        &self.explicit_constraints
    }

    /// `m_netclassClearances` (name order; the C++ map is only used for lookups).
    pub fn netclass_clearances(&self) -> &BTreeMap<String, Arc<DrcRule>> {
        &self.netclass_clearances
    }

    /// `m_netclassClearances[ aName ]` lookup.
    pub fn netclass_clearance_rule(&self, netclass_name: &str) -> Option<&Arc<DrcRule>> {
        self.netclass_clearances.get(netclass_name)
    }

    /// Port of `DRC_ENGINE::RulesValid`.
    pub fn rules_valid(&self) -> bool {
        self.rules_valid
    }

    /// `m_hasExplicitClearanceRules`.
    pub fn has_explicit_clearance_rules(&self) -> bool {
        self.has_explicit_clearance_rules
    }

    /// Port of `DRC_ENGINE::HasGeometryDependentRules`.
    pub fn has_geometry_dependent_rules(&self) -> bool {
        self.has_geometry_dependent_rules
    }

    /// `m_hasDiffPairClearanceOverrides`.
    pub fn has_diff_pair_clearance_overrides(&self) -> bool {
        self.has_diff_pair_clearance_overrides
    }

    /// Port of `DRC_ENGINE::ClearClearanceCache`.
    pub fn clear_clearance_cache(&self) {
        self.lock_own_clearance_cache().clear();
    }

    /// Port of `DRC_ENGINE::HasRulesForConstraintType`.
    pub fn has_rules_for_constraint_type(&self, constraint_id: DrcConstraintT) -> bool {
        self.constraint_map
            .get(&constraint_id)
            .is_some_and(|v| !v.is_empty())
    }

    /// Port of `DRC_ENGINE::QueryWorstConstraint`: the constraint with the largest minimum
    /// (first of equals), `None` unless that minimum is positive.
    pub fn query_worst_constraint(
        &self,
        constraint_id: DrcConstraintT,
        unconditional_only: bool,
    ) -> Option<DrcConstraint> {
        let mut worst = 0;
        let mut result = None;

        if let Some(list) = self.constraint_map.get(&constraint_id) {
            for c in list {
                if unconditional_only && c.condition.is_some() {
                    continue;
                }

                let current = c.constraint.value().min();

                if current > worst {
                    worst = current;
                    result = Some(c.constraint.clone());
                }
            }
        }

        if worst > 0 { result } else { None }
    }

    /// Port of `DRC_ENGINE::HasConditionalConstraint`: a user rule with a condition.
    pub fn has_conditional_constraint(&self, constraint_id: DrcConstraintT) -> bool {
        self.constraint_map.get(&constraint_id).is_some_and(|list| {
            list.iter()
                .any(|c| c.condition.is_some() && !c.parent_rule.is_implicit())
        })
    }

    /// Port of `DRC_ENGINE::HasUserDefinedConstraint`.
    pub fn has_user_defined_constraint(&self, constraint_id: DrcConstraintT) -> bool {
        self.constraint_map
            .get(&constraint_id)
            .is_some_and(|list| list.iter().any(|c| !c.parent_rule.is_implicit()))
    }

    /// Port of `DRC_ENGINE::HasUserDefinedPhysicalConstraint`.
    pub fn has_user_defined_physical_constraint(&self) -> bool {
        self.has_user_defined_constraint(DrcConstraintT::PhysicalClearanceConstraint)
            || self.has_user_defined_constraint(DrcConstraintT::PhysicalHoleClearanceConstraint)
    }

    /// Port of `DRC_ENGINE::QueryDistinctConstraints`: the distinct minimums, ascending.
    pub fn query_distinct_constraints(
        &self,
        constraint_id: DrcConstraintT,
    ) -> std::collections::BTreeSet<i32> {
        self.constraint_map
            .get(&constraint_id)
            .map(|list| list.iter().map(|c| c.constraint.value().min()).collect())
            .unwrap_or_default()
    }
}

impl RuleResolver for DrcEngine {
    fn eval_rules(
        &self,
        ctx: &BoardCtx<'_>,
        constraint: DrcConstraintT,
        a: Option<ItemRef<'_>>,
        b: Option<ItemRef<'_>>,
        layer: PcbLayerId,
    ) -> DrcConstraint {
        self.eval_rules(ctx, constraint, a, b, layer)
    }

    fn has_rules_for_constraint_type(&self, constraint: DrcConstraintT) -> bool {
        DrcEngine::has_rules_for_constraint_type(self, constraint)
    }

    fn cached_own_clearance(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        self.cached_own_clearance_impl(ctx, item, layer, want_source)
    }

    fn query_worst_constraint(&self, constraint: DrcConstraintT) -> Option<DrcConstraint> {
        DrcEngine::query_worst_constraint(self, constraint, false)
    }
}
