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

//! Test support for the rule parser:
//!
//! * [`SyntaxBinder`]: a stand-in for the PCB expression binding (package DRC-21, not ported
//!   yet) that is exact for everything the parser can observe without a reporter, and
//! * [`dump_rules`]: the Rust twin of the `pcb-dump rules` command
//!   (`rust/tools/pcb-dump/dump_rules.cpp`, format in `rust/docs/PCB_DUMP_FORMAT.md` section 7).
//!
//! What the parser observes of a compiled condition is the `Compile()` result, which only
//! depends on the grammar (the codegen stage never fails a compilation), and the usage flags
//! of `PCBEXPR_UCODE`, which `PCBEXPR_UCODE::CreateVarRef` sets from the variable name alone
//! before it looks at properties, and `CreateFuncCall` from the function name alone. The
//! binder ports exactly those parts of `PCBEXPR_UCODE` (statement by statement) and accepts
//! every property; only the codegen *messages* of a reporter run (e.g. "Unrecognized
//! property") can therefore differ from the C++, and the test files avoid them.

#![allow(dead_code)]

use kicad_core::{Lset, PcbLayerId, Severity};
use kicad_drc::constraint_types::drc_constraint_options;
use kicad_drc::pcbexpr::evaluator::PcbExprBinder;
use kicad_drc::reporter::WxStringReporter;
use kicad_drc::rule::DrcConstraintExt;
use kicad_drc::rule_condition::DrcRuleCondition;
use kicad_drc::rule_parser::{ComponentClassAssignmentRule, DrcRulesParser};
use kicad_drc::{DrcConstraintT, DrcRule};
use kicad_libeval::{Context, FuncCallRef, NoCustomValue, UcodeBinder, Value, VarRef, VarType};
use kicad_pcb::ItemRef;
use kicad_pcb::drc_api::DrcRuleInfo;
use kicad_pcb::zone_settings::ZoneConnection;
use kicad_sexpr::IoError;

/* --------------------------------------------------------------------------------------------
 * Stand-in binding
 */

/// `PCBEXPR_VAR_REF` reduced to its type (properties are not looked up).
pub struct SyntaxVarRef {
    type_: VarType,
}

impl VarRef<SyntaxBinder> for SyntaxVarRef {
    fn type_(&self) -> VarType {
        self.type_
    }

    fn value<'e>(&self, _ctx: &mut Context<'e, SyntaxBinder>) -> Value<'e, SyntaxBinder> {
        // No items: undefined, as PCBEXPR_VAR_REF::GetValue without an item.
        Value::new()
    }
}

/// The names of `PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions` (lower case) and whether
/// they are geometry dependent.
const BUILTIN_FUNCTIONS: &[(&str, bool)] = &[
    ("existsonlayer", false),
    ("isplated", false),
    ("insidecourtyard", true),
    ("insidefrontcourtyard", true),
    ("insidebackcourtyard", true),
    ("intersectscourtyard", true),
    ("intersectsfrontcourtyard", true),
    ("intersectsbackcourtyard", true),
    ("insidearea", true),
    ("intersectsarea", true),
    ("intersectskeepout", true),
    ("enclosedbyarea", true),
    ("ismicrovia", false),
    ("isblindvia", false),
    ("isburiedvia", false),
    ("isblindburiedvia", false),
    ("isstackedvia", false),
    ("memberof", false),
    ("memberofgroup", false),
    ("memberoffootprint", false),
    ("memberofsheet", false),
    ("memberofsheetorchildren", false),
    ("fromto", false),
    ("iscoupleddiffpair", false),
    ("indiffpair", false),
    ("innetchain", false),
    ("hasnetchain", false),
    ("innetchainclass", false),
    ("getfield", false),
    ("hasnetclass", false),
    ("hasexactnetclass", false),
    ("hascomponentclass", false),
    ("customproperty", false),
    ("hascustomproperty", false),
];

/// A dummy function body: returns 0 (C++ functions return 0 without items, too).
fn dummy_function(ctx: &mut Context<'_, SyntaxBinder>, _self: Option<&SyntaxVarRef>) {
    let result = ctx.alloc_value();
    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);
}

/// See the module documentation.
#[derive(Clone, Copy, Debug, Default)]
pub struct SyntaxBinder {
    has_geometry_dependent_functions: bool,
    requires_pair_items: bool,
    references_item_b: bool,
}

/// `wxString::CmpNoCase( b ) == 0`.
fn same_no_case(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl UcodeBinder for SyntaxBinder {
    type Env<'e> = ();
    type Custom<'e> = NoCustomValue;
    type VarRef = SyntaxVarRef;

    /// The variable-name part of `PCBEXPR_UCODE::CreateVarRef`, statement by statement.
    fn create_var_ref(&mut self, var: &str, field: &str) -> Option<SyntaxVarRef> {
        if same_no_case(var, "null") {
            return Some(SyntaxVarRef {
                type_: VarType::Null,
            });
        }

        // The receiver may be a navigation chain such as "A.Parent".  Split off the base
        // variable and translate each remaining segment into a navigation step.  Only "Parent"
        // is supported.
        let mut base_var = var.to_string();

        if var.contains('.') {
            let tokens: Vec<&str> = var.split('.').collect();
            base_var = tokens.first().map(|s| s.to_string()).unwrap_or_default();

            for token in &tokens[1..] {
                if !same_no_case(token, "Parent") {
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
        if base_var == "L" && same_no_case(field, "Layer") {
            return Some(SyntaxVarRef {
                type_: VarType::Undefined,
            });
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

        for special in ["NetClass", "ComponentClass", "NetName", "Type"] {
            if same_no_case(field, special) {
                if base_var == "A" || base_var == "B" {
                    return Some(SyntaxVarRef {
                        type_: VarType::String,
                    });
                }

                return None;
            }
        }

        if !(base_var == "A" || base_var == "AB" || base_var == "B" || base_var == "L") {
            return None;
        }

        // (property lookup through the PROPERTY_MANAGER: not modelled, every property exists)
        Some(SyntaxVarRef {
            type_: VarType::Undefined,
        })
    }

    /// `PCBEXPR_UCODE::CreateFuncCall`.
    fn create_func_call(&mut self, name: &str) -> Option<FuncCallRef<Self>> {
        let lower = name.to_lowercase();
        let (_, geometry) = BUILTIN_FUNCTIONS.iter().find(|(n, _)| *n == lower)?;

        if *geometry {
            self.has_geometry_dependent_functions = true;
        }

        Some(FuncCallRef::new(dummy_function))
    }
}

impl PcbExprBinder for SyntaxBinder {
    fn has_geometry_dependent_functions(&self) -> bool {
        self.has_geometry_dependent_functions
    }

    fn requires_pair_items(&self) -> bool {
        self.requires_pair_items
    }

    fn references_item_b(&self) -> bool {
        self.references_item_b
    }

    fn new_env<'e>(_constraint: i32, _layer: PcbLayerId) -> Self::Env<'e> {}

    fn item_env<'e>(
        _a: Option<ItemRef<'e>>,
        _b: Option<ItemRef<'e>>,
        _constraint: i32,
        _layer: PcbLayerId,
    ) -> Self::Env<'e> {
    }
}

/// The parser with the stand-in binding.
pub type TestParser<'a> = DrcRulesParser<'a, SyntaxBinder>;

/// Parse `text` with a reporter; returns the rules, the reporter and the parse result.
pub fn parse_with_reporter(
    text: &str,
    source: &str,
) -> (Vec<DrcRule>, WxStringReporter, Result<(), IoError>) {
    let mut rules = Vec::new();
    let mut reporter = WxStringReporter::new();
    let result = {
        let mut parser = TestParser::new(text, source);
        parser.parse(&mut rules, Some(&mut reporter))
    };

    (rules, reporter, result)
}

/// Parse `text` without a reporter (the throwing mode).
pub fn parse_throwing(text: &str, source: &str) -> (Vec<DrcRule>, Result<(), IoError>) {
    let mut rules = Vec::new();
    let result = TestParser::new(text, source).parse(&mut rules, None);
    (rules, result)
}

/* --------------------------------------------------------------------------------------------
 * The `pcb-dump rules` format
 */

/// `V( const std::string& )` of pcb-dump (multi-byte UTF-8 sequences are copied verbatim,
/// like the C++ copies every byte >= 0x80).
pub fn v_str(value: &str) -> String {
    let mut out = String::from("\"");

    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }

    out.push('"');
    out
}

/// `V( bool )`.
fn v_bool(value: bool) -> String {
    if value { "true" } else { "false" }.to_string()
}

/// `V( const LSET& )` of pcb-dump: hex digits, most significant first.
pub fn v_lset(set: &Lset) -> String {
    let size = set.size();
    let nbits = size.max(kicad_core::layer_ids::PCB_LAYER_ID_COUNT as usize);
    let nbits = nbits.div_ceil(4) * 4;
    let mut r = String::new();

    for nib in (0..nbits / 4).rev() {
        let mut v = 0;

        for b in (0..4).rev() {
            let bit = nib * 4 + b;
            v = v * 2 + usize::from(bit < size && set.test(bit));
        }

        r.push(b"0123456789abcdef"[v] as char);
    }

    r
}

/// The `severityName` of `dump_rules.cpp`.
pub fn severity_name(severity: Severity) -> &'static str {
    match severity {
        Severity::Undefined => "RPT_SEVERITY_UNDEFINED",
        Severity::Info => "RPT_SEVERITY_INFO",
        Severity::Exclusion => "RPT_SEVERITY_EXCLUSION",
        Severity::Action => "RPT_SEVERITY_ACTION",
        Severity::Warning => "RPT_SEVERITY_WARNING",
        Severity::Error => "RPT_SEVERITY_ERROR",
        Severity::Ignore => "RPT_SEVERITY_IGNORE",
        Severity::Debug => "RPT_SEVERITY_DEBUG",
    }
}

/// The C++ enumerator names of `DRC_CONSTRAINT_T`, in enum order.
const CONSTRAINT_NAMES: &[&str] = &[
    "NULL_CONSTRAINT",
    "CLEARANCE_CONSTRAINT",
    "CREEPAGE_CONSTRAINT",
    "HOLE_CLEARANCE_CONSTRAINT",
    "HOLE_TO_HOLE_CONSTRAINT",
    "EDGE_CLEARANCE_CONSTRAINT",
    "HOLE_SIZE_CONSTRAINT",
    "COURTYARD_CLEARANCE_CONSTRAINT",
    "SILK_CLEARANCE_CONSTRAINT",
    "TEXT_HEIGHT_CONSTRAINT",
    "TEXT_THICKNESS_CONSTRAINT",
    "TRACK_WIDTH_CONSTRAINT",
    "TRACK_SEGMENT_LENGTH_CONSTRAINT",
    "ANNULAR_WIDTH_CONSTRAINT",
    "ZONE_CONNECTION_CONSTRAINT",
    "THERMAL_RELIEF_GAP_CONSTRAINT",
    "THERMAL_SPOKE_WIDTH_CONSTRAINT",
    "MIN_RESOLVED_SPOKES_CONSTRAINT",
    "SOLDER_MASK_EXPANSION_CONSTRAINT",
    "SOLDER_PASTE_ABS_MARGIN_CONSTRAINT",
    "SOLDER_PASTE_REL_MARGIN_CONSTRAINT",
    "DISALLOW_CONSTRAINT",
    "VIA_DIAMETER_CONSTRAINT",
    "LENGTH_CONSTRAINT",
    "NET_CHAIN_LENGTH_CONSTRAINT",
    "NET_CHAIN_STUB_LENGTH_CONSTRAINT",
    "NET_CHAIN_RETURN_PATH_CONSTRAINT",
    "SKEW_CONSTRAINT",
    "DIFF_PAIR_GAP_CONSTRAINT",
    "MAX_UNCOUPLED_CONSTRAINT",
    "DIFF_PAIR_INTRA_SKEW_CONSTRAINT",
    "VIA_COUNT_CONSTRAINT",
    "PHYSICAL_CLEARANCE_CONSTRAINT",
    "PHYSICAL_HOLE_CLEARANCE_CONSTRAINT",
    "ASSERTION_CONSTRAINT",
    "CONNECTION_WIDTH_CONSTRAINT",
    "TRACK_ANGLE_CONSTRAINT",
    "VIA_DANGLING_CONSTRAINT",
    "BRIDGED_MASK_CONSTRAINT",
    "SOLDER_MASK_SLIVER_CONSTRAINT",
    "MICROVIA_STACK_DEPTH_CONSTRAINT",
    "HOLE_ASPECT_RATIO_CONSTRAINT",
];

/// The C++ enumerator name of a constraint type.
pub fn constraint_name(type_: DrcConstraintT) -> &'static str {
    CONSTRAINT_NAMES[type_ as usize]
}

fn zone_connection_name(zc: ZoneConnection) -> &'static str {
    match zc {
        ZoneConnection::Inherited => "INHERITED",
        ZoneConnection::None => "NONE",
        ZoneConnection::Thermal => "THERMAL",
        ZoneConnection::Full => "FULL",
        ZoneConnection::ThtThermal => "THT_THERMAL",
    }
}

/// Output lines of one mode, prefixed `rules|<mode>|`.
struct Out {
    prefix: String,
    lines: Vec<String>,
}

impl Out {
    fn emit(&mut self, field: &str, value: &str) {
        self.lines
            .push(format!("{}{}={}", self.prefix, field, value));
    }
}

fn dump_condition(out: &mut Out, prefix: &str, condition: Option<&DrcRuleCondition>) {
    let Some(condition) = condition else {
        out.emit(prefix, "none");
        return;
    };

    out.emit(prefix, &v_str(condition.expression()));
    out.emit(
        &format!("{prefix}.pair"),
        &v_bool(condition.requires_pair_items()),
    );
    out.emit(
        &format!("{prefix}.item_b"),
        &v_bool(condition.references_item_b()),
    );
    out.emit(
        &format!("{prefix}.geometry"),
        &v_bool(condition.has_geometry_dependent_functions()),
    );
}

fn rule_ref(rule: Option<&DrcRuleInfo>, own: &DrcRuleInfo) -> &'static str {
    match rule {
        None => "none",
        Some(r) if r == own => "self",
        Some(_) => "other",
    }
}

fn dump_rule(out: &mut Out, prefix: &str, rule: &DrcRule) {
    out.emit(&format!("{prefix}.name"), &v_str(&rule.name));
    out.emit(&format!("{prefix}.implicit"), &v_bool(rule.is_implicit()));
    out.emit(&format!("{prefix}.unary"), &v_bool(rule.unary));
    out.emit(
        &format!("{prefix}.layer_source"),
        &v_str(&rule.layer_source),
    );
    out.emit(&format!("{prefix}.layers"), &v_lset(&rule.layer_condition));
    out.emit(&format!("{prefix}.severity"), severity_name(rule.severity));
    dump_condition(
        out,
        &format!("{prefix}.condition"),
        rule.condition.as_deref(),
    );
    out.emit(
        &format!("{prefix}.constraints"),
        &rule.constraints.len().to_string(),
    );

    let own = rule.info();

    for (ii, c) in rule.constraints.iter().enumerate() {
        let p = format!("{prefix}.c.{ii}");
        let v = &c.value;
        let opt = |has: bool, val: i32| {
            if has {
                val.to_string()
            } else {
                "none".to_string()
            }
        };

        out.emit(&format!("{p}.type"), constraint_name(c.type_));
        out.emit(&format!("{p}.min"), &opt(v.has_min(), v.min()));
        out.emit(&format!("{p}.opt"), &opt(v.has_opt(), v.opt()));
        out.emit(&format!("{p}.max"), &opt(v.has_max(), v.max()));
        out.emit(&format!("{p}.disallow"), &c.disallow_flags.to_string());
        out.emit(
            &format!("{p}.zone_connection"),
            zone_connection_name(c.zone_connection),
        );
        dump_condition(
            out,
            &format!("{p}.test"),
            rule.constraint_tests[ii].as_deref(),
        );
        out.emit(&format!("{p}.implicit_min"), &v_bool(c.implicit_min));
        out.emit(&format!("{p}.reference_layer"), &v_str(&c.reference_layer));
        out.emit(&format!("{p}.reference_net"), &v_str(&c.reference_net));
        out.emit(
            &format!("{p}.option.skew_within_diff_pairs"),
            &v_bool(c.option(drc_constraint_options::SKEW_WITHIN_DIFF_PAIRS)),
        );
        out.emit(
            &format!("{p}.option.space_domain"),
            &v_bool(c.option(drc_constraint_options::SPACE_DOMAIN)),
        );
        out.emit(
            &format!("{p}.option.time_domain"),
            &v_bool(c.option(drc_constraint_options::TIME_DOMAIN)),
        );
        out.emit(&format!("{p}.name"), &v_str(&DrcConstraintExt::name(c)));
        out.emit(&format!("{p}.severity"), severity_name(c.severity()));
        out.emit(
            &format!("{p}.parent_rule"),
            rule_ref(c.parent_rule.as_ref(), &own),
        );
        out.emit(
            &format!("{p}.min_rule"),
            rule_ref(c.min_rule.as_ref(), &own),
        );
        out.emit(
            &format!("{p}.opt_rule"),
            rule_ref(c.opt_rule.as_ref(), &own),
        );
        out.emit(
            &format!("{p}.max_rule"),
            rule_ref(c.max_rule.as_ref(), &own),
        );
    }
}

fn dump_error(out: &mut Out, error: &IoError) {
    if let Some(pe) = error.as_parse_error() {
        out.emit("status", "parse_error");
        out.emit("error", &v_str(pe.what()));
        out.emit("error.problem", &v_str(pe.parse_problem()));
        out.emit("error.line", &pe.line_number.to_string());
        out.emit("error.offset", &pe.byte_index.to_string());
        out.emit("error.input_line", &v_str(&pe.input_line));
    } else {
        out.emit("status", "io_error");
        out.emit("error", &v_str(error.what()));
    }
}

fn dump_parse(
    text: &str,
    source: &str,
    with_reporter: bool,
    component_classes: bool,
) -> Vec<String> {
    let mut out = Out {
        prefix: format!("rules|{}|", if with_reporter { "report" } else { "throw" }),
        lines: Vec::new(),
    };
    let mut reporter = WxStringReporter::new();
    let mut rules: Vec<DrcRule> = Vec::new();
    let mut cc_rules: Vec<Option<ComponentClassAssignmentRule>> = Vec::new();

    let result = {
        let mut parser = TestParser::new(text, source);
        let reporter_arg = if with_reporter {
            Some(&mut reporter as &mut dyn kicad_drc::Reporter)
        } else {
            None
        };

        if component_classes {
            parser.parse_component_class_assignment_rules(&mut cc_rules, reporter_arg)
        } else {
            parser.parse(&mut rules, reporter_arg)
        }
    };

    match &result {
        Ok(()) => out.emit("status", "ok"),
        Err(e) => dump_error(&mut out, e),
    }

    for (ii, (text, severity)) in reporter.messages().iter().enumerate() {
        out.emit(
            &format!("msg.{ii}"),
            &format!("{} {}", severity_name(*severity), v_str(text)),
        );
    }

    if component_classes {
        out.emit("count", &cc_rules.len().to_string());

        for (ii, rule) in cc_rules.iter().enumerate() {
            let p = format!("ccrule.{ii}");

            match rule {
                None => out.emit(&p, "null"),
                Some(rule) => out.emit(&format!("{p}.class"), &v_str(&rule.component_class)),
            }
        }
    } else {
        out.emit("count", &rules.len().to_string());

        for (ii, rule) in rules.iter().enumerate() {
            dump_rule(&mut out, &format!("rule.{ii}"), rule);
        }
    }

    out.lines
}

/// The output of `pcb-dump rules [--component-classes] <source>` for one file with content
/// `text` (without the `file|...|` line): the "report" lines, then the "throw" lines.
pub fn dump_rules(text: &str, source: &str, component_classes: bool) -> Vec<String> {
    let mut lines = dump_parse(text, source, true, component_classes);
    lines.extend(dump_parse(text, source, false, component_classes));
    lines
}

/// 64-bit FNV-1a (as `Fnv1a64` of pcb-dump).
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;

    for &b in data {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }

    hash
}

/// The compact golden form of one file's dump: per mode
/// `<mode>|status=<s>|count=<n>|msgs=<m>|#<hash of the mode's lines, each + '\n'>`.
pub fn compact(lines: &[String]) -> Vec<String> {
    let mut result = Vec::new();

    for mode in ["report", "throw"] {
        let prefix = format!("rules|{mode}|");
        let mode_lines: Vec<&String> = lines.iter().filter(|l| l.starts_with(&prefix)).collect();
        let field = |name: &str| {
            mode_lines
                .iter()
                .find_map(|l| l.strip_prefix(&format!("{prefix}{name}=")))
                .unwrap_or("?")
                .to_string()
        };
        let msgs = mode_lines
            .iter()
            .filter(|l| l.starts_with(&format!("{prefix}msg.")))
            .count();
        let mut joined = String::new();

        for l in &mode_lines {
            joined.push_str(l);
            joined.push('\n');
        }

        result.push(format!(
            "{mode}|status={}|count={}|msgs={}|#{:016x}",
            field("status"),
            field("count"),
            msgs,
            fnv1a64(joined.as_bytes())
        ));
    }

    result
}

/// The golden form of the broken-file dumps: the lines without rule fields, then
/// [`compact`].
pub fn summary(lines: &[String]) -> Vec<String> {
    let mut result: Vec<String> = lines
        .iter()
        .filter(|l| !l.contains("|rule."))
        .cloned()
        .collect();
    result.extend(compact(lines));
    result
}
