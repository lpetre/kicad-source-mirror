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

//! Port of `pcbnew/drc/drc_engine.cpp` (`isKeepoutZone`, `createImplicitRule`,
//! `loadImplicitRules`).
//!
//! The tuning profiles (`PROJECT_FILE::TuningProfileParameters()`, `TUNING_PROFILES`) are not
//! part of the Rust project model yet; [`TuningProfiles`] holds what `loadImplicitRules` reads
//! of them and is passed in by the caller (empty = a project without tuning profiles).

use std::collections::BTreeMap;
use std::sync::Arc;

use kicad_core::string_utils::escape_html;
use kicad_core::{Lset, PcbLayerId, Severity};
use kicad_libeval::libeval_compiler::escape_string_literal;
use kicad_pcb::drc_api::{DRCE_TUNING_PROFILE_IMPLICIT_RULES, drc_disallow};
use kicad_pcb::netclass::Netclass;
use kicad_pcb::zone::Zone;
use kicad_pcb::{Board, BoardCtx, ItemId};

use crate::constraint_types::{DrcConstraintT, DrcImplicitSource};
use crate::rule::{DrcConstraint, DrcRule};
use crate::rule_condition::DrcRuleCondition;

/// `TUNING_PROFILE::PROFILE_TYPE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TuningProfileType {
    /// `SINGLE`.
    #[default]
    Single,
    /// `DIFFERENTIAL`.
    Differential,
}

/// The members of `DELAY_PROFILE_TRACK_PROPAGATION_ENTRY` read by `loadImplicitRules`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelayProfileTrackPropagationEntry {
    /// `m_signalLayer`.
    pub signal_layer: PcbLayerId,
    /// `m_width`.
    pub width: i32,
    /// `m_diffPairGap`.
    pub diff_pair_gap: i32,
}

/// The members of `TUNING_PROFILE` read by `loadImplicitRules`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuningProfile {
    /// `m_ProfileName`.
    pub profile_name: String,
    /// `m_Type`.
    pub type_: TuningProfileType,
    /// `m_TrackPropagationEntries`.
    pub track_propagation_entries: Vec<DelayProfileTrackPropagationEntry>,
}

/// `TUNING_PROFILES` (the part read by `loadImplicitRules`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuningProfiles {
    /// `m_tuningProfiles`.
    pub tuning_profiles: Vec<TuningProfile>,
    /// `m_nullDelayProfile`.
    null_delay_profile: TuningProfile,
}

impl TuningProfiles {
    /// A profile list.
    pub fn new(tuning_profiles: Vec<TuningProfile>) -> Self {
        TuningProfiles {
            tuning_profiles,
            null_delay_profile: TuningProfile::default(),
        }
    }

    /// Port of `TUNING_PROFILES::GetTuningProfile`: the first profile of that name, or the
    /// (empty) null profile.
    pub fn tuning_profile(&self, profile_name: &str) -> &TuningProfile {
        self.tuning_profiles
            .iter()
            .find(|p| p.profile_name == profile_name)
            .unwrap_or(&self.null_delay_profile)
    }
}

/// What `loadImplicitRules` produces besides the rules appended to `m_rules`.
#[derive(Default)]
pub(crate) struct ImplicitRulesOutput {
    /// `m_netclassClearances` entries: netclass name -> index of its clearance rule in the
    /// engine's rule list, in assignment order.
    pub netclass_clearances: Vec<(String, usize)>,
    /// `m_hasDiffPairClearanceOverrides = true` was executed.
    pub has_diff_pair_clearance_overrides: bool,
}

/// Port of the static `isKeepoutZone( aItem, aCheckFlags )` for a zone.
pub(crate) fn is_keepout_zone(zone: &Zone, check_flags: bool) -> bool {
    if !zone.is_rule_area() {
        return false;
    }

    // ZONE::HasKeepoutParametersSet()
    if !(zone.do_not_allow_tracks
        || zone.do_not_allow_vias
        || zone.do_not_allow_pads
        || zone.do_not_allow_footprints
        || zone.do_not_allow_zone_fills)
    {
        return false;
    }

    if check_flags
        && !zone.do_not_allow_tracks
        && !zone.do_not_allow_vias
        && !zone.do_not_allow_pads
        && !zone.do_not_allow_zone_fills
        && !zone.do_not_allow_footprints
    {
        return false;
    }

    true
}

/// Port of `DRC_ENGINE::createImplicitRule` minus the `addRule` (the caller appends the rule
/// once it is complete; nothing reads the rule list in between).
fn create_implicit_rule(name: &str, implicit_source: DrcImplicitSource) -> DrcRule {
    let mut rule = DrcRule::new();

    rule.name = name.to_string();
    rule.set_implicit_source(implicit_source);

    rule
}

/// `DRC_CONSTRAINT( aType )` with a minimum.
fn min_constraint(type_: DrcConstraintT, min: i32) -> DrcConstraint {
    let mut constraint = DrcConstraint::new(type_, "");
    constraint.value.set_min(min);
    constraint
}

/// `rule->AddConstraint( constraint )`.
fn add(rule: &mut DrcRule, mut constraint: DrcConstraint) {
    rule.add_constraint(&mut constraint, None);
}

/// `new DRC_RULE_CONDITION( expr )`.
fn condition(expression: &str) -> Option<Arc<DrcRuleCondition>> {
    Some(Arc::new(DrcRuleCondition::new(expression)))
}

/// `aParent->GetHumanReadableName()` of a `Get...Parent()` (`None` = the class itself).
fn parent_name(nc: &Netclass, parent: &Option<Arc<Netclass>>) -> String {
    parent.as_deref().unwrap_or(nc).get_human_readable_name()
}

/// Port of `DRC_ENGINE::loadImplicitRules`: appends the implicit rules to `rules` in the C++
/// order (rule order is evaluation order).
///
/// Mutates the board like the C++ (`SynchronizeNetsAndNetClasses( false )` before the
/// netclass rules).
pub(crate) fn load_implicit_rules(
    board: &mut Board,
    tuning_profiles: &TuningProfiles,
    rules: &mut Vec<DrcRule>,
) -> ImplicitRulesOutput {
    let mut out = ImplicitRulesOutput::default();
    let bds = board.design_settings().clone();

    // 1) global defaults

    let mut rule = create_implicit_rule(
        "board setup constraints",
        DrcImplicitSource::BoardSetupConstraint,
    );

    add(
        &mut rule,
        min_constraint(DrcConstraintT::TrackWidthConstraint, bds.track_min_width),
    );
    add(
        &mut rule,
        min_constraint(DrcConstraintT::ConnectionWidthConstraint, bds.min_conn),
    );
    add(
        &mut rule,
        min_constraint(DrcConstraintT::HoleSizeConstraint, bds.min_through_drill),
    );
    add(
        &mut rule,
        min_constraint(
            DrcConstraintT::AnnularWidthConstraint,
            bds.vias_min_annular_width,
        ),
    );
    add(
        &mut rule,
        min_constraint(DrcConstraintT::ViaDiameterConstraint, bds.vias_min_size),
    );
    add(
        &mut rule,
        min_constraint(DrcConstraintT::HoleToHoleConstraint, bds.hole_to_hole_min),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints zone fill strategy",
        DrcImplicitSource::BoardSetupConstraint,
    );
    add(
        &mut rule,
        min_constraint(
            DrcConstraintT::MinResolvedSpokesConstraint,
            bds.min_resolved_spokes,
        ),
    );
    rules.push(rule);

    let silk_layers = || Lset::from([PcbLayerId::FSilkS, PcbLayerId::BSilkS]);

    let mut rule = create_implicit_rule(
        "board setup constraints silk",
        DrcImplicitSource::BoardSetupConstraint,
    );
    rule.layer_condition = silk_layers();
    add(
        &mut rule,
        min_constraint(DrcConstraintT::SilkClearanceConstraint, bds.silk_clearance),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints silk text height",
        DrcImplicitSource::BoardSetupConstraint,
    );
    rule.layer_condition = silk_layers();
    add(
        &mut rule,
        min_constraint(
            DrcConstraintT::TextHeightConstraint,
            bds.min_silk_text_height,
        ),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints silk text thickness",
        DrcImplicitSource::BoardSetupConstraint,
    );
    rule.layer_condition = silk_layers();
    add(
        &mut rule,
        min_constraint(
            DrcConstraintT::TextThicknessConstraint,
            bds.min_silk_text_thickness,
        ),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints hole",
        DrcImplicitSource::BoardSetupConstraint,
    );
    add(
        &mut rule,
        min_constraint(DrcConstraintT::HoleClearanceConstraint, bds.hole_clearance),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints edge",
        DrcImplicitSource::BoardSetupConstraint,
    );
    add(
        &mut rule,
        min_constraint(
            DrcConstraintT::EdgeClearanceConstraint,
            bds.copper_edge_clearance,
        ),
    );
    rules.push(rule);

    let mut rule = create_implicit_rule(
        "board setup constraints courtyard",
        DrcImplicitSource::BoardSetupConstraint,
    );
    // C++: `holeToHoleConstraint.Value().SetMin( 0 )` after the hole-to-hole constraint was
    // copied into the first rule, so the courtyard constraint is added without any value.
    add(
        &mut rule,
        DrcConstraint::new(DrcConstraintT::CourtyardClearanceConstraint, ""),
    );
    rules.push(rule);

    // 2a) micro-via specific defaults (new DRC doesn't treat microvias in any special way)

    let mut u_via_rule = create_implicit_rule(
        "board setup constraints micro-via",
        DrcImplicitSource::BoardSetupConstraint,
    );
    u_via_rule.condition = condition("A.Via_Type == 'Micro'");
    add(
        &mut u_via_rule,
        min_constraint(DrcConstraintT::HoleSizeConstraint, bds.micro_vias_min_drill),
    );
    add(
        &mut u_via_rule,
        min_constraint(DrcConstraintT::ViaDiameterConstraint, bds.micro_vias_min_size),
    );
    rules.push(u_via_rule);

    // 2b) barcode-specific defaults

    let mut barcode_rule = create_implicit_rule(
        "barcode visual separation default",
        DrcImplicitSource::BarcodeDefaults,
    );
    add(
        &mut barcode_rule,
        min_constraint(
            DrcConstraintT::PhysicalClearanceConstraint,
            kicad_core::base_units::PCB_IU_SCALE.mm_to_iu(1.0),
        ),
    );
    barcode_rule.condition = condition("A.Type == 'Barcode'");
    rules.push(barcode_rule);

    // 3) per-netclass rules

    // (rule, creation sequence number) so that m_netclassClearances can follow the sort
    let mut netclass_clearance_rules: Vec<(DrcRule, usize)> = Vec::new();
    let mut netclass_item_specific_rules: Vec<DrcRule> = Vec::new();
    let mut registrations: Vec<(String, usize)> = Vec::new();
    let mut seq = 0usize;

    let mut make_netclass_rules = |nc: &Netclass, _is_default: bool| {
        // DRC_TEST_PROVIDER_MISC reports the class, since a rule here cannot be compiled
        let Some(nc_name) = escape_string_literal(&nc.get_name()) else {
            return;
        };

        let new_rule = |name: String, expr: String| {
            let mut netclass_rule = DrcRule::new();
            netclass_rule.name = name;
            netclass_rule.set_implicit_source(DrcImplicitSource::NetClass);
            netclass_rule.condition = condition(&expr);
            netclass_rule.implicit_netclass = nc.get_name();
            netclass_rule
        };

        if nc.has_clearance() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}'", parent_name(nc, &nc.parents.clearance)),
                format!("A.hasExactNetclass('{nc_name}')"),
            );
            add(
                &mut netclass_rule,
                min_constraint(DrcConstraintT::ClearanceConstraint, nc.clearance()),
            );
            netclass_clearance_rules.push((netclass_rule, seq));
            registrations.push((nc.get_name(), seq));
            seq += 1;
        }

        if nc.has_track_width() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}'", parent_name(nc, &nc.parents.track_width)),
                format!("A.hasExactNetclass('{nc_name}')"),
            );
            let mut constraint =
                min_constraint(DrcConstraintT::TrackWidthConstraint, bds.track_min_width);
            constraint.value.set_opt(nc.track_width());
            add(&mut netclass_rule, constraint);
            netclass_clearance_rules.push((netclass_rule, seq));
            seq += 1;
        }

        if nc.has_diff_pair_width() {
            let mut netclass_rule = new_rule(
                format!(
                    "netclass '{}' diff pair",
                    parent_name(nc, &nc.parents.diff_pair_width)
                ),
                format!("A.hasExactNetclass('{nc_name}') && A.inDiffPair('*')"),
            );
            let mut constraint =
                min_constraint(DrcConstraintT::TrackWidthConstraint, bds.track_min_width);
            constraint.value.set_opt(nc.diff_pair_width());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);
        }

        if nc.has_diff_pair_gap() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}'", parent_name(nc, &nc.parents.diff_pair_gap)),
                format!("A.hasExactNetclass('{nc_name}')"),
            );
            let mut constraint =
                min_constraint(DrcConstraintT::DiffPairGapConstraint, bds.min_clearance);
            constraint.value.set_opt(nc.diff_pair_gap());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);

            // A narrower diffpair gap overrides the netclass min clearance
            if nc.has_clearance() && nc.diff_pair_gap() < nc.clearance() {
                let mut netclass_rule = new_rule(
                    format!(
                        "netclass '{}' diff pair",
                        parent_name(nc, &nc.parents.diff_pair_gap)
                    ),
                    format!("A.hasExactNetclass('{nc_name}') && AB.isCoupledDiffPair()"),
                );
                add(
                    &mut netclass_rule,
                    min_constraint(DrcConstraintT::ClearanceConstraint, nc.diff_pair_gap()),
                );
                netclass_item_specific_rules.push(netclass_rule);

                out.has_diff_pair_clearance_overrides = true;
            }
        }

        if nc.has_via_diameter() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}'", parent_name(nc, &nc.parents.via_diameter)),
                format!("A.hasExactNetclass('{nc_name}') && A.Via_Type != 'Micro'"),
            );
            let mut constraint =
                min_constraint(DrcConstraintT::ViaDiameterConstraint, bds.vias_min_size);
            constraint.value.set_opt(nc.via_diameter());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);
        }

        if nc.has_via_drill() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}'", parent_name(nc, &nc.parents.via_drill)),
                format!("A.hasExactNetclass('{nc_name}') && A.Via_Type != 'Micro'"),
            );
            let mut constraint =
                min_constraint(DrcConstraintT::HoleSizeConstraint, bds.min_through_drill);
            constraint.value.set_opt(nc.via_drill());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);
        }

        if nc.has_uvia_diameter() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}' uvia", parent_name(nc, &nc.parents.uvia_dia)),
                format!("A.hasExactNetclass('{nc_name}') && A.Via_Type == 'Micro'"),
            );
            let mut constraint = min_constraint(
                DrcConstraintT::ViaDiameterConstraint,
                bds.micro_vias_min_size,
            );
            constraint.value.set_opt(nc.uvia_diameter());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);
        }

        if nc.has_uvia_drill() {
            let mut netclass_rule = new_rule(
                format!("netclass '{}' uvia", parent_name(nc, &nc.parents.uvia_drill)),
                format!("A.hasExactNetclass('{nc_name}') && A.Via_Type == 'Micro'"),
            );
            let mut constraint = min_constraint(
                DrcConstraintT::HoleSizeConstraint,
                bds.micro_vias_min_drill,
            );
            constraint.value.set_opt(nc.uvia_drill());
            add(&mut netclass_rule, constraint);
            netclass_item_specific_rules.push(netclass_rule);
        }
    };

    board.synchronize_nets_and_netclasses(false);

    {
        let net_settings = &board.design_settings().net_settings;

        make_netclass_rules(net_settings.default_netclass(), true);

        for netclass in net_settings.netclasses().values() {
            make_netclass_rules(netclass, false);
        }

        for netclass in net_settings.composite_netclasses().values() {
            make_netclass_rules(netclass, false);
        }
    }

    // The netclass clearance rules have to be sorted by min clearance so the right one fires
    // if 'A' and 'B' belong to two different netclasses.
    //
    // The item-specific netclass rules are all unary, so there's no 'A' vs 'B' issue.

    kicad_geom::stdsort::std_sort(&mut netclass_clearance_rules, |lhs, rhs| {
        lhs.0.constraints[0].value.min() < rhs.0.constraints[0].value.min()
    });

    let mut index_of_seq: BTreeMap<usize, usize> = BTreeMap::new();

    for (nc_rule, rule_seq) in netclass_clearance_rules {
        index_of_seq.insert(rule_seq, rules.len());
        rules.push(nc_rule);
    }

    for (name, rule_seq) in registrations {
        out.netclass_clearances.push((name, index_of_seq[&rule_seq]));
    }

    rules.extend(netclass_item_specific_rules);

    // 4) tuning profile rules

    let tuning_severity = bds
        .drc_severities
        .get(&DRCE_TUNING_PROFILE_IMPLICIT_RULES)
        .copied()
        .unwrap_or(Severity::Undefined);

    let tuning_rule = |profile_name: &str, expr: String, constraint: DrcConstraint| {
        let mut tuning_rule = DrcRule::new();
        tuning_rule.severity = tuning_severity;
        tuning_rule.name = format!("tuning profile '{profile_name}'");
        tuning_rule.set_implicit_source(DrcImplicitSource::TuningProfile);
        tuning_rule.condition = condition(&expr);
        add(&mut tuning_rule, constraint);
        tuning_rule
    };

    let width_constraint = |entry: &DelayProfileTrackPropagationEntry| {
        let mut constraint = DrcConstraint::new(DrcConstraintT::TrackWidthConstraint, "");
        constraint
            .value
            .set_min(bds.track_min_width.max(entry.width));
        constraint.value.set_opt(entry.width);
        constraint.value.set_max(entry.width);
        constraint
    };

    if board.project.is_some() {
        let mut add_netclass_tuning_profile_rules = |netclass: &Netclass| {
            let literal = escape_string_literal(&netclass.get_name());

            let (true, Some(literal)) = (netclass.has_tuning_profile(), literal) else {
                return;
            };

            let delay_profile_name = netclass.tuning_profile();
            let profile = tuning_profiles.tuning_profile(delay_profile_name);

            for entry in &profile.track_propagation_entries {
                if entry.width <= 0 {
                    continue;
                }

                let layer = Lset::name(entry.signal_layer);

                if profile.type_ == TuningProfileType::Single {
                    // addTuningSingleRule
                    rules.push(tuning_rule(
                        delay_profile_name,
                        format!("A.hasExactNetclass('{literal}') && A.Layer == '{layer}'"),
                        width_constraint(entry),
                    ));
                } else {
                    // addTuningDifferentialRules
                    if entry.width <= 0 || entry.diff_pair_gap <= 0 {
                        continue;
                    }

                    rules.push(tuning_rule(
                        delay_profile_name,
                        format!(
                            "A.hasExactNetclass('{literal}') && A.Layer == '{layer}' && A.inDiffPair('*')"
                        ),
                        width_constraint(entry),
                    ));

                    let mut constraint2 =
                        DrcConstraint::new(DrcConstraintT::DiffPairGapConstraint, "");
                    constraint2
                        .value
                        .set_min(bds.min_clearance.max(entry.diff_pair_gap));
                    constraint2.value.set_opt(entry.diff_pair_gap);
                    constraint2.value.set_max(entry.diff_pair_gap);

                    rules.push(tuning_rule(
                        delay_profile_name,
                        format!(
                            "A.hasExactNetclass('{literal}') && A.Layer == '{layer}' && A.inDiffPair('*')"
                        ),
                        constraint2,
                    ));

                    // A narrower diffpair gap overrides the netclass min clearance
                    if netclass.has_clearance() && entry.diff_pair_gap < netclass.clearance() {
                        rules.push(tuning_rule(
                            delay_profile_name,
                            format!(
                                "A.hasExactNetclass('{literal}') && A.Layer == '{layer}' && AB.isCoupledDiffPair()"
                            ),
                            min_constraint(
                                DrcConstraintT::ClearanceConstraint,
                                entry.diff_pair_gap,
                            ),
                        ));
                    }
                }
            }
        };

        let net_settings = &board.design_settings().net_settings;

        add_netclass_tuning_profile_rules(net_settings.default_netclass());

        for netclass in net_settings.netclasses().values() {
            add_netclass_tuning_profile_rules(netclass);
        }

        for netclass in net_settings.composite_netclasses().values() {
            add_netclass_tuning_profile_rules(netclass);
        }
    }

    // 5) keepout area rules

    let ctx = BoardCtx::bare(board);

    let add_keepout_zone_rule = |rules: &mut Vec<DrcRule>,
                                 zone_id: ItemId,
                                 zone: &Zone,
                                 parent_fp: Option<&kicad_pcb::footprint::Footprint>| {
        let name = zone.zone_name();
        let describe_parent =
            |fp: &kicad_pcb::footprint::Footprint| describe_ref(&fp.reference(&ctx));

        let rule_name = if name.is_empty() {
            match parent_fp {
                Some(fp) => format!("keepout area of {}", describe_parent(fp)),
                None => "keepout area".to_string(),
            }
        } else {
            match parent_fp {
                Some(fp) => format!("keepout area '{}' of {}", name, describe_parent(fp)),
                None => format!("keepout area '{name}'"),
            }
        };

        let mut rule = create_implicit_rule(&rule_name, DrcImplicitSource::Keepout);

        rule.implicit_item_id = zone.base.item.uuid;
        rule.implicit_item = Some(zone_id);

        rule.condition = condition(&format!(
            "A.intersectsKeepout('{}')",
            zone.base.item.uuid.as_string()
        ));

        rule.layer_condition = zone.layer_set(&ctx);

        let mut disallow_flags = 0;

        if zone.do_not_allow_tracks {
            disallow_flags |= drc_disallow::TRACKS;
        }

        if zone.do_not_allow_vias {
            disallow_flags |= drc_disallow::VIAS;
        }

        if zone.do_not_allow_pads {
            disallow_flags |= drc_disallow::PADS;
        }

        if zone.do_not_allow_zone_fills {
            disallow_flags |= drc_disallow::ZONES;
        }

        if zone.do_not_allow_footprints {
            disallow_flags |= drc_disallow::FOOTPRINTS;
        }

        let mut disallow_constraint = DrcConstraint::new(DrcConstraintT::DisallowConstraint, "");
        disallow_constraint.disallow_flags = disallow_flags;
        add(&mut rule, disallow_constraint);

        rules.push(rule);
    };

    for (zone_id, zone) in board.zones() {
        if is_keepout_zone(zone, true) {
            add_keepout_zone_rule(rules, zone_id.into(), zone, None);
        }
    }

    for (fp_id, footprint) in board.footprints() {
        for (zone_id, zone) in board.footprint_zones(fp_id) {
            if is_keepout_zone(zone, true) {
                add_keepout_zone_rule(rules, zone_id.into(), zone, Some(footprint));
            }
        }
    }

    out
}

/// Port of `DescribeRef` (`common/common.cpp`).
pub fn describe_ref(reference: &str) -> String {
    if reference.is_empty() {
        "<i>unannotated footprint </i>".to_string()
    } else {
        escape_html(reference)
    }
}
