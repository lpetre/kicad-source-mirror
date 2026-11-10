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

//! Port of `pcbnew/drc/drc_engine.cpp`: `EvalRules`, `EvalZoneConnection`,
//! `EvalClearanceBatch`, `ProcessAssertions` and the own-clearance cache
//! (`GetCachedOwnClearance`, `InvalidateClearanceCache`, `ClearClearanceCache`) (package
//! DRC-04), with the explanatory `REPORT( ... )` texts of the reporter path (package DRC-34,
//! helpers in [`super::eval_rules_report`]).
//!
//! The control flow with a reporter is the C++ one (the netclass fast path and the condition
//! memo are only used without a reporter); the texts are built only when there is one.

use std::collections::BTreeMap;

use kicad_core::layer_ids::is_pcb_layer;
use kicad_core::{KicadT, Kiid, Lset, PcbLayerId};
use kicad_geom::math::util::ki_round;
use kicad_pcb::drc_api::{DrcImplicitSource, DrcRuleInfo, drc_disallow};
use kicad_pcb::netclass::Netclass;
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::zone_settings::ZoneConnection;
use kicad_pcb::{Board, BoardCtx, ItemRef};

use super::eval_rules_report::{
    ReportUnits, print_zone_connection, report, report_constraint_header,
};
use super::implicit_rules::is_keepout_zone;
use super::{DrcEngine, DrcEngineConstraint};
use crate::constraint_types::DrcConstraintT;
use crate::reporter::{Reporter, reborrow};
use crate::rule::DrcConstraintExt;
use kicad_core::string_utils::escape_html;
use kicad_pcb::drc_api::DrcConstraint;

/// `MAXIMUM_CLEARANCE`.
fn maximum_clearance() -> i32 {
    kicad_pcb::board_design_settings::maximum_clearance()
}

/// Port of `DRC_CLEARANCE_BATCH`: the minimums of the five clearance-type constraints for one
/// item pair (0 where a constraint has no minimum).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrcClearanceBatch {
    /// `clearance`.
    pub clearance: i32,
    /// `holeClearance`.
    pub hole_clearance: i32,
    /// `holeToHole`.
    pub hole_to_hole: i32,
    /// `edgeClearance`.
    pub edge_clearance: i32,
    /// `physicalClearance`.
    pub physical_clearance: i32,
}

/// `m_ownClearanceCache` with the board time stamp it belongs to (`board-model.md` section 7:
/// the model never calls into the engine from a mutator, so instead of the C++
/// `InvalidateClearanceCache` calls from the item setters the cache is dropped whenever the
/// board's time stamp moved).
#[derive(Debug, Default)]
pub struct OwnClearanceCache {
    /// `board.time_stamp()` the entries were computed at.
    stamp: Option<i32>,
    /// (uuid, layer) -> clearance.
    map: BTreeMap<(Kiid, PcbLayerId), i32>,
}

impl OwnClearanceCache {
    /// `m_ownClearanceCache.clear()`.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True without entries.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Drop the entries when the board time stamp moved.
    fn sync_stamp(&mut self, stamp: i32) {
        if self.stamp != Some(stamp) {
            self.map.clear();
            self.stamp = Some(stamp);
        }
    }
}

/// `BOARD_CONNECTED_ITEM::GetLocalClearance( wxString* aSource )` with the source text: only
/// `PAD` ("pad") and `ZONE` ("zone", none for rule areas) override it.
fn local_clearance_with_source(item: &ItemRef<'_>) -> Option<(i32, Option<String>)> {
    if let Some(pad) = item.as_pad() {
        pad.local_clearance_with_source(true)
    } else if let Some(zone) = item.as_zone() {
        zone.local_clearance_with_source(true)
    } else {
        None
    }
}

/// `BOARD_ITEM::IsOnCopperLayer()`.
fn is_on_copper_layer(item: &ItemRef<'_>) -> bool {
    item.as_dyn().is_on_copper_layer(item.ctx)
}

/// The state shared by the lambdas of `EvalRules` (`applyConstraint`, `checkCondition`,
/// `processConstraint`), which capture the locals by reference.
struct EvalState<'x, 'e> {
    engine: &'e DrcEngine,
    board: &'x Board,
    a: Option<ItemRef<'x>>,
    b: Option<ItemRef<'x>>,
    layer: PcbLayerId,
    ac_netclass: Option<&'x Netclass>,
    bc_netclass: Option<&'x Netclass>,
    a_is_non_copper: bool,
    b_is_non_copper: bool,
    constraint: DrcConstraint,
    /// `conditionCache`: expression -> result (lookups only).
    condition_cache: BTreeMap<&'e str, bool>,
    /// The engine's `UNITS_PROVIDER` for the report texts.
    units: ReportUnits,
}

impl<'x, 'e> EvalState<'x, 'e> {
    /// The `applyConstraint` lambda.
    // the switch of the C++ is kept as is
    #[allow(clippy::collapsible_match)]
    fn apply_constraint(&mut self, c: &DrcEngineConstraint) {
        let constraint = &mut self.constraint;
        let rule = c.rule_info.as_ref();
        let mut min_rule = constraint.min_rule.clone();
        let mut opt_rule = constraint.opt_rule.clone();
        let mut max_rule = constraint.max_rule.clone();

        if c.constraint.value.has_min() {
            constraint.implicit_min = c.parent_rule.is_implicit();
            constraint.value.set_min(c.constraint.value.min());
            min_rule = rule.cloned();
        }

        if c.constraint.value.has_opt() {
            constraint.value.set_opt(c.constraint.value.opt());
            opt_rule = rule.cloned();
        }

        if c.constraint.value.has_max() {
            constraint.value.set_max(c.constraint.value.max());
            max_rule = rule.cloned();
        }

        use DrcConstraintT::*;

        match c.constraint.type_ {
            ClearanceConstraint
            | CourtyardClearanceConstraint
            | SilkClearanceConstraint
            | HoleClearanceConstraint
            | EdgeClearanceConstraint
            | PhysicalClearanceConstraint
            | PhysicalHoleClearanceConstraint => {
                if constraint.value.min() > maximum_clearance() {
                    constraint.value.set_min(maximum_clearance());
                }
            }
            _ => {}
        }

        // While the expectation would be to OR the disallow flags, we've already
        // masked them down to aItem's type -- so we're really only looking for a
        // boolean here.
        constraint.disallow_flags = c.constraint.disallow_flags;

        constraint.zone_connection = c.constraint.zone_connection;

        constraint.set_parent_rule(rule);
        constraint.set_value_rules(min_rule.as_ref(), opt_rule.as_ref(), max_rule.as_ref());

        constraint.set_options_from_other(&c.constraint);
    }

    /// The `checkCondition` lambda: memoised by expression text without a reporter.
    fn check_condition(
        &mut self,
        c: &'e DrcEngineConstraint,
        reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        let condition = c.condition.as_ref().expect("condition");
        let type_ = c.constraint.type_ as i32;

        if reporter.is_some() {
            condition.evaluate_for(self.a, self.b, type_, self.layer, reporter)
        } else {
            let expr = condition.expression();

            if let Some(&matched) = self.condition_cache.get(expr) {
                matched
            } else {
                let matched = condition.evaluate_for(self.a, self.b, type_, self.layer, None);
                self.condition_cache.insert(expr, matched);
                matched
            }
        }
    }

    /// The `testAssertion` lambda (reporter path only: the result is only reported).
    fn test_assertion(&self, c: &DrcEngineConstraint, mut reporter: Option<&mut dyn Reporter>) {
        if let Some(test) = &c.test {
            report(&mut reporter, || {
                format!("Checking assertion '{}'.", escape_html(test.expression()))
            });

            if test.evaluate_for(
                self.a,
                self.b,
                c.constraint.type_ as i32,
                self.layer,
                reborrow(&mut reporter),
            ) {
                report(&mut reporter, || "Assertion passed.".to_string());
            } else {
                report(&mut reporter, || escape_html("--> Assertion failed. <--"));
            }
        }
    }

    /// The `processConstraint` lambda.
    fn process_constraint(
        &mut self,
        c: &'e DrcEngineConstraint,
        mut reporter: Option<&mut dyn Reporter>,
    ) {
        let implicit = c.parent_rule.is_implicit();

        if implicit && c.parent_rule.implicit_source() == DrcImplicitSource::NetClass {
            if c.constraint.type_ == DrcConstraintT::ClearanceConstraint
                && (self.a_is_non_copper || self.b_is_non_copper)
            {
                report_constraint_header(&mut reporter, &self.units, c);
                report(&mut reporter, || {
                    "Netclass clearances apply only between copper items.".to_string()
                });
                return;
            }

            // The condition starts with A.hasExactNetclass( tag ) and is tried both ways round, so
            // it cannot match unless one of the items is in that netclass
            let netclass_name = &c.parent_rule.implicit_netclass;

            if !netclass_name.is_empty()
                && !self
                    .ac_netclass
                    .is_some_and(|nc| nc.name_equals(netclass_name))
                && !self
                    .bc_netclass
                    .is_some_and(|nc| nc.name_equals(netclass_name))
            {
                return;
            }

            if !self.check_condition(c, None) {
                return;
            }
        }

        report_constraint_header(&mut reporter, &self.units, c);

        let a = self.a.expect("EvalRules: a");

        if c.constraint.type_ == DrcConstraintT::ClearanceConstraint {
            if self.a_is_non_copper {
                report(&mut reporter, || {
                    format!(
                        "{} contains no copper.  Constraint ignored.",
                        self.units.desc(&a)
                    )
                });
                return;
            } else if self.b_is_non_copper {
                let b = self.b.expect("EvalRules: b");
                report(&mut reporter, || {
                    format!(
                        "{} contains no copper.  Constraint ignored.",
                        self.units.desc(&b)
                    )
                });
                return;
            }
        } else if c.constraint.type_ == DrcConstraintT::DisallowConstraint {
            // A footprint's own keepout never applies to that footprint; decide
            // ownership from the rule's zone so a hijacked UUID cache can't defeat it
            if let Some(zone_id) = c.implicit_keepout_zone {
                let owner = a.ctx.item(zone_id).and_then(|z| z.parent_footprint());

                if owner.is_some_and(|fp| fp == a) {
                    report(&mut reporter, || {
                        "Keepout belongs to the footprint under test; constraint ignored."
                            .to_string()
                    });
                    return;
                }
            }

            let mask = if a.is_hole_proxy() {
                drc_disallow::HOLES
            } else if let Some(via) = a.as_via() {
                if via.is_micro_via() {
                    drc_disallow::MICRO_VIAS
                } else if via.is_blind_via() {
                    drc_disallow::BLIND_VIAS
                } else if via.is_buried_via() {
                    drc_disallow::BURIED_VIAS
                } else {
                    drc_disallow::THROUGH_VIAS
                }
            } else {
                match a.type_() {
                    KicadT::PcbTraceT => drc_disallow::TRACKS,
                    KicadT::PcbArcT => drc_disallow::TRACKS,
                    KicadT::PcbPadT => drc_disallow::PADS,
                    KicadT::PcbFootprintT => drc_disallow::FOOTPRINTS,
                    KicadT::PcbShapeT => drc_disallow::GRAPHICS,
                    KicadT::PcbBarcodeT => drc_disallow::GRAPHICS,
                    KicadT::PcbFieldT => drc_disallow::TEXTS,
                    KicadT::PcbTextT => drc_disallow::TEXTS,
                    KicadT::PcbTextboxT => drc_disallow::TEXTS,
                    KicadT::PcbTableT | KicadT::PcbDrillChartT | KicadT::PcbStackupTableT => {
                        drc_disallow::TEXTS
                    }
                    KicadT::PcbZoneT => {
                        // Treat teardrop areas as tracks for DRC purposes
                        if a.as_zone().is_some_and(|z| z.is_teardrop_area()) {
                            drc_disallow::TRACKS
                        } else {
                            drc_disallow::ZONES
                        }
                    }
                    KicadT::PcbLocateHoleT => drc_disallow::HOLES,
                    _ => 0,
                }
            };

            if (c.constraint.disallow_flags & mask) == 0 {
                report(&mut reporter, || {
                    if implicit {
                        "Keepout constraint not met.".to_string()
                    } else {
                        "Disallow constraint not met.".to_string()
                    }
                });

                return;
            }

            let mut item_layers = a.layer_set();

            if let Some(footprint) = a.as_footprint() {
                if !footprint.courtyard(a.ctx, PcbLayerId::FCrtYd).is_empty() {
                    item_layers |= Lset::front_mask();
                }

                if !footprint.courtyard(a.ctx, PcbLayerId::BCrtYd).is_empty() {
                    item_layers |= Lset::back_mask();
                }
            }

            if !(c.layer_test.clone() & &item_layers).any() {
                // (c->parentRule is never null in the port: no "Rule layer not matched"
                // branch without a rule)
                report(&mut reporter, || {
                    if implicit {
                        "Keepout layer(s) not matched.".to_string()
                    } else {
                        format!(
                            "Rule layer '{}' not matched; rule ignored.",
                            escape_html(&c.parent_rule.layer_source)
                        )
                    }
                });

                return;
            }
        }

        if (is_pcb_layer(self.layer) && !c.layer_test.contains(self.layer))
            || (self.board.enabled_layers() & &c.layer_test).count() == 0
        {
            report(&mut reporter, || {
                if implicit {
                    "Constraint layer not matched.".to_string()
                } else {
                    format!(
                        "Rule layer '{}' not matched; rule ignored.",
                        escape_html(&c.parent_rule.layer_source)
                    )
                }
            });
        } else if c.constraint.type_ == DrcConstraintT::HoleToHoleConstraint
            && (!a.has_hole() || !self.b.is_some_and(|b| b.has_hole()))
        {
            // Hole-to-hole only applies between two mechanical holes; this covers both
            // round drilled holes and milled (oval) slots, but not a hole paired with
            // a non-hole item.
            //
            // (A missing b dereferences null in C++; only the router's batch call passes
            // none. It is treated as an item without a hole, and named only when it is a.)
            let non_hole = if a.has_hole() { self.b } else { Some(a) };

            if let Some(non_hole) = non_hole {
                report(&mut reporter, || {
                    format!(
                        "{} does not have a hole; rule ignored.",
                        self.units.desc_raw(&non_hole)
                    )
                });
            }
        } else if c
            .condition
            .as_ref()
            .is_none_or(|cond| cond.expression().is_empty())
        {
            if reporter.is_some() {
                if implicit {
                    report(&mut reporter, || {
                        "Unconditional constraint applied.".to_string()
                    });
                } else if self.constraint.type_ == DrcConstraintT::AssertionConstraint {
                    report(&mut reporter, || "Unconditional rule applied.".to_string());
                    self.test_assertion(c, reborrow(&mut reporter));
                } else {
                    report(&mut reporter, || {
                        "Unconditional rule applied; overrides previous constraints.".to_string()
                    });
                }
            }

            self.apply_constraint(c);
        } else {
            // For implicit keepout rules with a pre-resolved zone pointer, skip the
            // expensive expression evaluation when the item doesn't overlap the
            // zone's bounding box. This avoids UUID string parsing and cache lock
            // contention for the vast majority of item-keepout pairs.
            if let Some(zone_id) = c.implicit_keepout_zone
                && reporter.is_none()
            {
                let item_bbox = a.bounding_box();

                if let Some(zone) = a.ctx.item(zone_id) {
                    let zone_bbox = zone.bounding_box();

                    if !item_bbox.intersects(&zone_bbox) {
                        return;
                    }
                }
            }

            if implicit {
                // Don't report on implicit rule conditions; they're synthetic.
            } else {
                report(&mut reporter, || {
                    format!(
                        "Checking rule condition '{}'.",
                        escape_html(c.condition.as_ref().expect("condition").expression())
                    )
                });
            }

            let cond_matched =
                if implicit && c.parent_rule.implicit_source() == DrcImplicitSource::NetClass {
                    true
                } else {
                    self.check_condition(c, reborrow(&mut reporter))
                };

            if cond_matched {
                if reporter.is_some() {
                    if implicit {
                        report(&mut reporter, || "Constraint applied.".to_string());
                    } else if self.constraint.type_ == DrcConstraintT::AssertionConstraint {
                        report(&mut reporter, || "Rule applied.".to_string());
                        self.test_assertion(c, reborrow(&mut reporter));
                    } else {
                        report(&mut reporter, || {
                            "Rule applied; overrides previous constraints.".to_string()
                        });
                    }
                }

                self.apply_constraint(c);
            } else {
                report(&mut reporter, || {
                    if implicit {
                        "Membership not satisfied; constraint ignored.".to_string()
                    } else {
                        "Condition not satisfied; rule ignored.".to_string()
                    }
                });
            }
        }
    }

    /// `for( DRC_ENGINE_CONSTRAINT* rule : *it->second ) processConstraint( rule );`
    fn process_all(&mut self, type_: DrcConstraintT, mut reporter: Option<&mut dyn Reporter>) {
        let engine = self.engine;

        if let Some(list) = engine.constraint_map.get(&type_) {
            for c in list {
                self.process_constraint(c, reborrow(&mut reporter));
            }
        }
    }

    /// `constraint.GetParentRule() && !constraint.GetParentRule()->IsImplicit()`.
    fn has_explicit_parent_rule(&self) -> bool {
        self.constraint
            .parent_rule
            .as_ref()
            .is_some_and(|r| !r.implicit)
    }
}

impl DrcEngine {
    /// Port of `DRC_ENGINE::EvalRules( aConstraintType, a, b, aLayer, aReporter )`.
    ///
    /// `a` is never `None` in DRC use (the C++ dereferences it in several branches; the port
    /// panics there); `b` is `None` for unary constraints. `ctx` must be the context of the
    /// board the engine was initialised for.
    pub fn eval_rules_reporting<'x>(
        &self,
        ctx: &'x BoardCtx<'x>,
        constraint_type: DrcConstraintT,
        a: Option<ItemRef<'x>>,
        b: Option<ItemRef<'x>>,
        layer: PcbLayerId,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> DrcConstraint {
        use DrcConstraintT::*;

        // NOTE: all string manipulation MUST BE KEPT INSIDE the REPORT macro.  It absolutely
        // kills performance when running bulk DRC tests (where aReporter is nullptr).

        let board = ctx.board;
        let bds = board.design_settings();
        let units = ReportUnits::new(self.user_units);

        let ac = a.filter(|a| a.is_connected());
        let bc = b.filter(|b| b.is_connected());
        let ac_netclass = ac.and_then(|ac| ac.effective_netclass());
        let bc_netclass = bc.and_then(|bc| bc.effective_netclass());

        let is_keepout =
            |item: &ItemRef<'_>| item.as_zone().is_some_and(|z| is_keepout_zone(z, false));

        let a_is_non_copper = a.is_some_and(|a| !is_on_copper_layer(&a) || is_keepout(&a));
        let b_is_non_copper = b.is_some_and(|b| !is_on_copper_layer(&b) || is_keepout(&b));

        let mut pad: Option<ItemRef<'x>> = None;
        let mut zone: Option<ItemRef<'x>> = None;
        let mut parent_footprint: Option<ItemRef<'x>> = None;

        if matches!(
            constraint_type,
            ZoneConnectionConstraint
                | ThermalReliefGapConstraint
                | ThermalSpokeWidthConstraint
                | SolderMaskExpansionConstraint
                | SolderPasteAbsMarginConstraint
                | SolderPasteRelMarginConstraint
        ) {
            if let Some(a) = a {
                if a.type_() == KicadT::PcbPadT {
                    pad = Some(a);
                } else if a.type_() == KicadT::PcbZoneT {
                    zone = Some(a);
                }
            }

            if let Some(b) = b {
                if b.type_() == KicadT::PcbPadT {
                    pad = Some(b);
                } else if b.type_() == KicadT::PcbZoneT {
                    zone = Some(b);
                }
            }

            if let Some(pad) = pad {
                parent_footprint = pad.parent_footprint();
            }
        }

        let mut constraint = DrcConstraint::default();
        constraint.type_ = constraint_type;

        let footprints = [
            a.and_then(|a| a.parent_footprint()),
            b.and_then(|b| b.parent_footprint()),
        ];

        // Handle Footprint net ties, which will zero out the clearance for footprint objects
        if constraint_type == ClearanceConstraint // Only zero clearance, other constraints still apply
            // Only apply to cases where we are comparing a connected item to a non-connected
            // item and not both connected.  Connected items of different nets still need to be
            // checked for their standard clearance value
            && ((ac.is_none() ^ bc.is_none())
                // Unless both items are in the same footprint, and that footprint exists
                || (footprints[0] == footprints[1] && footprints[0].is_some()))
            // Also, both elements need to be on copper layers
            && !a_is_non_copper
            && !b_is_non_copper
        {
            let child_items = [a, b];

            // These are the items being compared against, so the order is reversed
            let alt_items = [bc, ac];

            for ii in 0..2 {
                // We need both a footprint item and a connected item to check for a net tie
                let (Some(footprint), Some(alt)) = (footprints[ii], alt_items[ii]) else {
                    continue;
                };

                let footprint = footprint.as_footprint().expect("footprint");
                let found = child_items[ii]
                    .and_then(|child| child.id())
                    .is_some_and(|child| {
                        footprint
                            .net_tie_cache(child)
                            .contains(&alt.net_code().expect("connected item"))
                    });

                if found {
                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Net tie on {}; clearance: 0.",
                            units.desc(&footprints[ii].expect("footprint"))
                        )
                    });

                    constraint.name = "net tie".to_string();
                    constraint.value.set_min(0);
                    return constraint;
                }
            }
        }

        // Local overrides take precedence over everything *except* board min clearance
        if constraint_type == ClearanceConstraint || constraint_type == HoleClearanceConstraint {
            let mut override_val = 0;
            let mut override_a: Option<i32> = None;
            let mut override_b: Option<i32> = None;

            if let Some(ac) = ac
                && !b_is_non_copper
            {
                override_a = ac
                    .as_connected()
                    .and_then(|c| c.clearance_overrides(ac.ctx, false))
                    .map(|(v, _)| v);
            }

            if let Some(bc) = bc
                && !a_is_non_copper
            {
                override_b = bc
                    .as_connected()
                    .and_then(|c| c.clearance_overrides(bc.ctx, false))
                    .map(|(v, _)| v);
            }

            if override_a.is_some() || override_b.is_some() {
                let mut msg = String::new();

                if let Some(value_a) = override_a {
                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Local override on {}; clearance: {}.",
                            units.desc(&a.expect("EvalRules: a")),
                            units.int(value_a)
                        )
                    });

                    let ac = ac.expect("ac");
                    let (value, source) = ac
                        .as_connected()
                        .and_then(|c| c.clearance_overrides(ac.ctx, true))
                        .expect("override");
                    override_val = value;
                    msg = source.unwrap_or_default();
                }

                if let Some(value_b) = override_b {
                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Local override on {}; clearance: {}.",
                            units.desc(&b.expect("EvalRules: b")),
                            units.int(value_b)
                        )
                    });

                    if value_b > override_val {
                        let bc = bc.expect("bc");
                        let (value, source) = bc
                            .as_connected()
                            .and_then(|c| c.clearance_overrides(bc.ctx, true))
                            .expect("override");
                        override_val = value;
                        msg = source.unwrap_or_default();
                    }
                }

                if override_val != 0 {
                    if constraint_type == ClearanceConstraint {
                        if override_val < bds.min_clearance {
                            override_val = bds.min_clearance;
                            msg = "board minimum".to_string();

                            report(&mut reporter, String::new);
                            report(&mut reporter, || {
                                format!("Board minimum clearance: {}.", units.int(override_val))
                            });
                        }
                    } else if override_val < bds.hole_clearance {
                        override_val = bds.hole_clearance;
                        msg = "board minimum hole".to_string();

                        report(&mut reporter, String::new);
                        report(&mut reporter, || {
                            format!("Board minimum hole clearance: {}.", units.int(override_val))
                        });
                    }

                    constraint.name = msg;
                    constraint.value.set_min(override_val);
                    return constraint;
                }
            }
        } else if constraint_type == ZoneConnectionConstraint {
            if let Some(pad_ref) = pad {
                let p = pad_ref.as_pad().expect("pad");

                if p.local_zone_connection() != ZoneConnection::Inherited {
                    let (connection, msg) = p.zone_connection_overrides(pad_ref.ctx, true);

                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Local override on {}; zone connection: {}.",
                            units.desc(&pad_ref),
                            print_zone_connection(connection)
                        )
                    });

                    constraint.name = msg.unwrap_or_default();
                    constraint.zone_connection = connection;
                    return constraint;
                }
            }
        } else if constraint_type == ThermalReliefGapConstraint {
            if let Some(pad_ref) = pad {
                let p = pad_ref.as_pad().expect("pad");

                if p.local_thermal_gap_override(false).0 > 0 {
                    let (gap_override, msg) = p.local_thermal_gap_override(true);

                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Local override on {}; thermal relief gap: {}.",
                            units.desc(&pad_ref),
                            units.int(gap_override)
                        )
                    });

                    constraint.name = msg.unwrap_or_default();
                    constraint.value.set_min(gap_override);
                    return constraint;
                }
            }
        } else if constraint_type == ThermalSpokeWidthConstraint {
            if let Some(pad_ref) = pad {
                let p = pad_ref.as_pad().expect("pad");

                if p.local_spoke_width_override(false).0 > 0 {
                    let (mut spoke_override, msg) = p.local_spoke_width_override(true);

                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "Local override on {}; thermal spoke width: {}.",
                            units.desc(&pad_ref),
                            units.int(spoke_override)
                        )
                    });

                    if let Some(zone_ref) = zone
                        && let Some(z) = zone_ref.as_zone()
                        && z.min_thickness() > spoke_override
                    {
                        spoke_override = z.min_thickness();

                        report(&mut reporter, String::new);
                        report(&mut reporter, || {
                            format!(
                                "{} min thickness: {}.",
                                units.desc(&zone_ref),
                                units.int(spoke_override)
                            )
                        });
                    }

                    constraint.name = msg.unwrap_or_default();
                    constraint.value.set_min(spoke_override);
                    return constraint;
                }
            }
        } else if constraint_type == SolderMaskExpansionConstraint {
            let a_ref = a.expect("EvalRules: a");
            let mut override_: Option<i32> = None;
            let mut override_item = a_ref;

            if let Some(p) = pad.and_then(|p| p.as_pad()) {
                override_ = p.local_solder_mask_margin();
            } else if a_ref.type_() == KicadT::PcbShapeT {
                override_ = a_ref.as_shape().expect("shape").local_solder_mask_margin();
            } else if let Some(track) = a_ref.as_track() {
                override_ = track.as_track().local_solder_mask_margin();
            }

            if override_.is_none()
                && let Some(fp_ref) = parent_footprint
                && let Some(fp) = fp_ref.as_footprint()
            {
                override_ = fp.solder_mask_margin;
                override_item = fp_ref;
            }

            if let Some(value) = override_ {
                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "Local override on {}; solder mask expansion: {}.",
                        units.desc(&override_item),
                        units.int(value)
                    )
                });

                constraint.value.set_opt(value);
                return constraint;
            }
        } else if constraint_type == SolderPasteAbsMarginConstraint {
            let mut override_: Option<i32> = None;
            let mut override_item = a;

            if let Some(p) = pad.and_then(|p| p.as_pad()) {
                override_ = p.local_solder_paste_margin();
            }

            if override_.is_none()
                && pad.is_some()
                && let Some(fp_ref) = parent_footprint
                && let Some(fp) = fp_ref.as_footprint()
            {
                override_ = fp.solder_paste_margin;
                override_item = Some(fp_ref);
            }

            if let Some(value) = override_ {
                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "Local override on {}; solder paste absolute clearance: {}.",
                        units.desc(&override_item.expect("EvalRules: a")),
                        units.int(value)
                    )
                });

                constraint.value.set_opt(value);
                return constraint;
            }
        } else if constraint_type == SolderPasteRelMarginConstraint {
            let mut override_ratio: Option<f64> = None;
            let mut override_item = a;

            if let Some(p) = pad.and_then(|p| p.as_pad()) {
                override_ratio = p.local_solder_paste_margin_ratio();
            }

            if override_ratio.is_none()
                && pad.is_some()
                && let Some(fp_ref) = parent_footprint
                && let Some(fp) = fp_ref.as_footprint()
            {
                override_ratio = fp.solder_paste_margin_ratio;
                override_item = Some(fp_ref);
            }

            if let Some(ratio) = override_ratio {
                // MessageTextFromValue( double ): the percentage formatted as a distance
                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "Local override on {}; solder paste relative clearance: {}.",
                        units.desc(&override_item.expect("EvalRules: a")),
                        units.value(ratio * 100.0)
                    )
                });

                constraint.value.set_opt(ki_round(ratio * 1000.0));
                return constraint;
            }
        }

        let mut state = EvalState {
            engine: self,
            board,
            a,
            b,
            layer,
            ac_netclass,
            bc_netclass,
            a_is_non_copper,
            b_is_non_copper,
            constraint,
            condition_cache: BTreeMap::new(),
            units,
        };

        // Fast-path for netclass clearance when no explicit or diff pair override rules exist
        if constraint_type == ClearanceConstraint
            && !self.has_explicit_clearance_rules
            && !self.has_diff_pair_clearance_overrides
            && reporter.is_none()
            && !a_is_non_copper
            && (b.is_none() || !b_is_non_copper)
        {
            let mut netclass_rule: Option<DrcRuleInfo> = None;
            let mut clearance = 0;

            let nc_name_a = ac_netclass.map(|nc| nc.get_name()).unwrap_or_default();
            let nc_name_b = bc_netclass.map(|nc| nc.get_name()).unwrap_or_default();

            if !nc_name_a.is_empty() || !nc_name_b.is_empty() {
                if !nc_name_a.is_empty()
                    && let Some((rule_index, rule)) =
                        self.netclass_clearance_rule_indexed(&nc_name_a)
                {
                    netclass_rule = Some(rule.info_in_engine(rule_index));
                    clearance = rule.constraints[0].value.min();
                }

                if !nc_name_b.is_empty()
                    && let Some((rule_index, rule)) =
                        self.netclass_clearance_rule_indexed(&nc_name_b)
                    && rule.constraints[0].value.min() > clearance
                {
                    netclass_rule = Some(rule.info_in_engine(rule_index));
                    clearance = rule.constraints[0].value.min();
                }
            }

            if clearance > 0 {
                state.constraint.value.set_min(clearance);
                state.constraint.implicit_min = true;
                state.constraint.set_parent_rule(netclass_rule.as_ref());
            }
        } else {
            state.process_all(constraint_type, reborrow(&mut reporter));

            // DIFF_PAIR_GAP_CONSTRAINT must also respect CLEARANCE_CONSTRAINTs when called with
            // two items
            if constraint_type == DiffPairGapConstraint && b.is_some() {
                let clearance_constraint =
                    self.eval_rules_reporting(ctx, ClearanceConstraint, a, b, layer, None);

                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "Resolved minimum clearance: {}.",
                        state.units.int(clearance_constraint.value.min())
                    )
                });

                let mut constraint = state.constraint;

                if constraint.value.min() < clearance_constraint.value.min() {
                    constraint.value.set_min(clearance_constraint.value.min());
                    let opt_rule = constraint.opt_rule.clone();
                    let max_rule = constraint.max_rule.clone();
                    constraint.set_value_rules(
                        clearance_constraint.min_rule.as_ref(),
                        opt_rule.as_ref(),
                        max_rule.as_ref(),
                    );
                }

                return constraint;
            }
        }

        if state.has_explicit_parent_rule() {
            return state.constraint;
        }

        // Special case for properties which can be inherited from parent footprints.  We've
        // already checked for local overrides, and there were no rules targetting the item
        // itself, so we know we're inheriting and need to see if there are any rules targetting
        // the parent footprint.
        if let (Some(pad_ref), Some(parent_fp)) = (pad, parent_footprint)
            && matches!(
                constraint_type,
                ZoneConnectionConstraint
                    | ThermalReliefGapConstraint
                    | ThermalSpokeWidthConstraint
                    | SolderMaskExpansionConstraint
                    | SolderPasteAbsMarginConstraint
                    | SolderPasteRelMarginConstraint
            )
        {
            report(&mut reporter, String::new);
            report(&mut reporter, || {
                format!("Inheriting from parent: {}.", state.units.desc(&parent_fp))
            });

            if state.a == Some(pad_ref) {
                state.a = Some(parent_fp);
            } else {
                state.b = Some(parent_fp);
            }

            // a/b just changed, so conditions evaluated against the pad must not be reused for
            // the parent footprint.
            state.condition_cache.clear();

            if self.constraint_map.contains_key(&constraint_type) {
                state.process_all(constraint_type, reborrow(&mut reporter));

                if state.has_explicit_parent_rule() {
                    return state.constraint;
                }
            }

            // Found nothing again?  Return the defaults.
            if constraint_type == SolderMaskExpansionConstraint {
                state.constraint.set_parent_rule(None);
                state.constraint.name = "board setup".to_string();
                state.constraint.value.set_opt(bds.solder_mask_expansion);
                return state.constraint;
            } else if constraint_type == SolderPasteAbsMarginConstraint {
                state.constraint.set_parent_rule(None);
                state.constraint.name = "board setup".to_string();
                state.constraint.value.set_opt(bds.solder_paste_margin);
                return state.constraint;
            } else if constraint_type == SolderPasteRelMarginConstraint {
                state.constraint.set_parent_rule(None);
                state.constraint.name = "board setup".to_string();
                state
                    .constraint
                    .value
                    .set_opt(ki_round(bds.solder_paste_margin_ratio * 1000.0));
                return state.constraint;
            }
        }

        let units = state.units;
        let mut constraint = state.constraint;

        // Unfortunately implicit rules don't work for local clearances (such as zones) because
        // they have to be max'ed with netclass values (which are already implicit rules), and
        // our rule selection paradigm is "winner takes all".
        if constraint_type == ClearanceConstraint {
            let global = constraint.value.min();
            let mut clearance = global;
            let mut need_blank_line = true;

            if let Some(ac) = ac
                && let Some(local_a) = ac.as_connected().and_then(|c| c.local_clearance())
            {
                if need_blank_line {
                    report(&mut reporter, String::new);
                    need_blank_line = false;
                }

                report(&mut reporter, || {
                    format!(
                        "Local clearance on {}: {}.",
                        units.desc(&a.expect("EvalRules: a")),
                        units.int(local_a)
                    )
                });

                if local_a > clearance {
                    let (value, msg) = local_clearance_with_source(&ac).expect("local clearance");
                    clearance = value;
                    constraint.set_parent_rule(None);
                    constraint.name = msg.unwrap_or_default();
                    constraint.value.set_min(clearance);
                }
            }

            if let Some(bc) = bc
                && let Some(local_b) = bc.as_connected().and_then(|c| c.local_clearance())
            {
                if need_blank_line {
                    report(&mut reporter, String::new);
                    need_blank_line = false;
                }

                report(&mut reporter, || {
                    format!(
                        "Local clearance on {}: {}.",
                        units.desc(&b.expect("EvalRules: b")),
                        units.int(local_b)
                    )
                });

                if local_b > clearance {
                    let (value, msg) = local_clearance_with_source(&bc).expect("local clearance");
                    clearance = value;
                    constraint.set_parent_rule(None);
                    constraint.name = msg.unwrap_or_default();
                    constraint.value.set_min(clearance);
                }
            }

            if !a_is_non_copper && !b_is_non_copper {
                if need_blank_line {
                    report(&mut reporter, String::new);
                }

                report(&mut reporter, || {
                    format!("Board minimum clearance: {}.", units.int(bds.min_clearance))
                });

                if clearance < bds.min_clearance {
                    constraint.set_parent_rule(None);
                    constraint.name = "board minimum".to_string();
                    constraint.value.set_min(bds.min_clearance);
                }
            }

            return constraint;
        } else if constraint_type == ZoneConnectionConstraint {
            if pad.is_some()
                && let Some(fp_ref) = parent_footprint
                && let Some(fp) = fp_ref.as_footprint()
            {
                let local = fp.local_zone_connection();

                if local != ZoneConnection::Inherited {
                    report(&mut reporter, String::new);
                    report(&mut reporter, || {
                        format!(
                            "{} zone connection: {}.",
                            units.desc(&fp_ref),
                            print_zone_connection(local)
                        )
                    });

                    constraint.set_parent_rule(None);
                    constraint.name = "footprint".to_string();
                    constraint.zone_connection = local;
                    return constraint;
                }
            }

            if let Some(zone_ref) = zone
                && let Some(z) = zone_ref.as_zone()
            {
                let local = z.pad_connection();

                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "{} pad connection: {}.",
                        units.desc(&zone_ref),
                        print_zone_connection(local)
                    )
                });

                constraint.set_parent_rule(None);
                constraint.name = "zone".to_string();
                constraint.zone_connection = local;
                return constraint;
            }
        } else if constraint_type == ThermalReliefGapConstraint {
            if let Some(zone_ref) = zone
                && let Some(z) = zone_ref.as_zone()
            {
                let local = z.thermal_relief_gap();

                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!(
                        "{} thermal relief gap: {}.",
                        units.desc(&zone_ref),
                        units.int(local)
                    )
                });

                constraint.set_parent_rule(None);
                constraint.name = "zone".to_string();
                constraint.value.set_min(local);
                return constraint;
            }
        } else if constraint_type == ThermalSpokeWidthConstraint
            && let Some(zone_ref) = zone
            && let Some(z) = zone_ref.as_zone()
        {
            let local = z.thermal_relief_spoke_width();

            report(&mut reporter, String::new);
            report(&mut reporter, || {
                format!(
                    "{} thermal spoke width: {}.",
                    units.desc(&zone_ref),
                    units.int(local)
                )
            });

            constraint.set_parent_rule(None);
            constraint.name = "zone".to_string();
            constraint.value.set_min(local);
            return constraint;
        }

        if constraint.parent_rule.is_none() {
            constraint.type_ = NullConstraint;
            constraint.disallow_flags = 0;
        }

        constraint
    }

    /// Port of `DRC_ENGINE::EvalRules` without a reporter (the bulk DRC path).
    pub fn eval_rules<'x>(
        &self,
        ctx: &'x BoardCtx<'x>,
        constraint_type: DrcConstraintT,
        a: Option<ItemRef<'x>>,
        b: Option<ItemRef<'x>>,
        layer: PcbLayerId,
    ) -> DrcConstraint {
        self.eval_rules_reporting(ctx, constraint_type, a, b, layer, None)
    }

    /// Port of `DRC_ENGINE::EvalZoneConnection`: `EvalRules( ZONE_CONNECTION_CONSTRAINT )`,
    /// with `THT_THERMAL` resolved to `THERMAL` for a PTH pad and `FULL` otherwise.
    pub fn eval_zone_connection<'x>(
        &self,
        ctx: &'x BoardCtx<'x>,
        a: ItemRef<'x>,
        b: Option<ItemRef<'x>>,
        layer: PcbLayerId,
        mut reporter: Option<&mut dyn Reporter>,
    ) -> DrcConstraint {
        let mut constraint = self.eval_rules_reporting(
            ctx,
            DrcConstraintT::ZoneConnectionConstraint,
            Some(a),
            b,
            layer,
            reborrow(&mut reporter),
        );

        report(&mut reporter, String::new);
        report(&mut reporter, || {
            format!(
                "Resolved zone connection type: {}.",
                print_zone_connection(constraint.zone_connection)
            )
        });

        if constraint.zone_connection == ZoneConnection::ThtThermal {
            let pad = if a.type_() == KicadT::PcbPadT {
                a.as_pad()
            } else if b.expect("EvalZoneConnection: b").type_() == KicadT::PcbPadT {
                b.and_then(|b| b.as_pad())
            } else {
                None
            };

            if pad.is_some_and(|p| p.attribute() == PadAttrib::Pth) {
                constraint.zone_connection = ZoneConnection::Thermal;
            } else {
                report(&mut reporter, || {
                    format!(
                        "Pad is not a through hole pad; connection will be: {}.",
                        print_zone_connection(ZoneConnection::Full)
                    )
                });
                constraint.zone_connection = ZoneConnection::Full;
            }
        }

        constraint
    }

    /// Port of `DRC_ENGINE::EvalClearanceBatch`. A missing `b` is treated as an item without
    /// a hole by the hole-to-hole evaluation (the C++ dereferences it).
    pub fn eval_clearance_batch<'x>(
        &self,
        ctx: &'x BoardCtx<'x>,
        a: ItemRef<'x>,
        b: Option<ItemRef<'x>>,
        layer: PcbLayerId,
    ) -> DrcClearanceBatch {
        use DrcConstraintT::*;

        let mut result = DrcClearanceBatch::default();
        let min_of = |t: DrcConstraintT| {
            let c = self.eval_rules(ctx, t, Some(a), b, layer);
            c.value.has_min().then(|| c.value.min())
        };

        if let Some(v) = min_of(ClearanceConstraint) {
            result.clearance = v;
        }

        if let Some(v) = min_of(HoleClearanceConstraint) {
            result.hole_clearance = v;
        }

        if let Some(v) = min_of(HoleToHoleConstraint) {
            result.hole_to_hole = v;
        }

        if let Some(v) = min_of(EdgeClearanceConstraint) {
            result.edge_clearance = v;
        }

        if let Some(v) = min_of(PhysicalClearanceConstraint) {
            result.physical_clearance = v;
        }

        result
    }

    /// Port of `DRC_ENGINE::ProcessAssertions`: every `ASSERTION` constraint whose condition
    /// matches `a` (no condition: always) has its test evaluated; `failure_handler` receives
    /// the constraint of each failed assertion. The layer mismatch is only reported, it does
    /// not skip the rule (as in C++).
    pub fn process_assertions<'x>(
        &self,
        a: ItemRef<'x>,
        failure_handler: &mut dyn FnMut(&DrcConstraint),
        mut reporter: Option<&mut dyn Reporter>,
    ) {
        // NOTE: all string manipulation MUST BE KEPT INSIDE the REPORT macro.  It absolutely
        // kills performance when running bulk DRC tests (where aReporter is nullptr).

        let layer = a.layer();

        let mut test_assertion =
            |c: &DrcEngineConstraint, mut reporter: Option<&mut dyn Reporter>| {
                if let Some(test) = &c.test {
                    report(&mut reporter, || {
                        format!(
                            "Checking rule assertion '{}'.",
                            escape_html(test.expression())
                        )
                    });
                }

                let passed = c.test.as_ref().is_some_and(|t| {
                    t.evaluate_for(
                        Some(a),
                        None,
                        c.constraint.type_ as i32,
                        layer,
                        reborrow(&mut reporter),
                    )
                });

                if passed {
                    report(&mut reporter, || "Assertion passed.".to_string());
                } else {
                    report(&mut reporter, || escape_html("--> Assertion failed. <--"));
                    failure_handler(&c.constraint);
                }
            };

        if let Some(list) = self
            .constraint_map
            .get(&DrcConstraintT::AssertionConstraint)
        {
            for c in list {
                report(&mut reporter, String::new);
                report(&mut reporter, || {
                    format!("Checking {}.", c.constraint.name())
                });

                if !(a.layer_set() & &c.layer_test).any() {
                    report(&mut reporter, || {
                        format!(
                            "Rule layer '{}' not matched; rule ignored.",
                            escape_html(&c.parent_rule.layer_source)
                        )
                    });
                }

                match &c.condition {
                    Some(condition) if !condition.expression().is_empty() => {
                        report(&mut reporter, || {
                            format!(
                                "Checking rule condition '{}'.",
                                escape_html(condition.expression())
                            )
                        });

                        if condition.evaluate_for(
                            Some(a),
                            None,
                            c.constraint.type_ as i32,
                            layer,
                            reborrow(&mut reporter),
                        ) {
                            report(&mut reporter, || "Rule applied.".to_string());
                            test_assertion(c, reborrow(&mut reporter));
                        } else {
                            report(&mut reporter, || {
                                "Condition not satisfied; rule ignored.".to_string()
                            });
                        }
                    }
                    _ => {
                        report(&mut reporter, || "Unconditional rule applied.".to_string());
                        test_assertion(c, reborrow(&mut reporter));
                    }
                }
            }
        }
    }

    /// Port of `DRC_ENGINE::GetCachedOwnClearance`: the `CLEARANCE` minimum of `item` alone
    /// (`HOLE_CLEARANCE` for an NPTH pad), 0 without one; with `want_source` also the name of
    /// what supplied it (only on a cache miss, like the C++, which does not cache the source).
    pub fn cached_own_clearance_impl<'x>(
        &self,
        ctx: &'x BoardCtx<'x>,
        item: ItemRef<'x>,
        layer: PcbLayerId,
        want_source: bool,
    ) -> (i32, Option<String>) {
        let key = (item.uuid(), layer);
        let stamp = ctx.board.time_stamp();

        // Fast path: check cache
        {
            let mut cache = self.lock_own_clearance_cache();
            cache.sync_stamp(stamp);

            if let Some(&clearance) = cache.map.get(&key) {
                // Cache hit. We don't cache the source string since it's rarely requested
                // and caching it would add complexity.
                return (clearance, None);
            }
        }

        // Cache miss - evaluate the constraint (outside lock to avoid blocking other threads)
        let mut constraint_type = DrcConstraintT::ClearanceConstraint;

        if let Some(pad) = item.as_pad()
            && pad.attribute() == PadAttrib::Npth
        {
            constraint_type = DrcConstraintT::HoleClearanceConstraint;
        }

        let constraint = self.eval_rules(ctx, constraint_type, Some(item), None, layer);

        let mut clearance = 0;
        let mut source = None;

        if constraint.value.has_min() {
            clearance = constraint.value.min();

            if want_source {
                source = Some(constraint.name());
            }
        }

        // Store in cache using double-checked locking
        {
            let mut cache = self.lock_own_clearance_cache();
            cache.sync_stamp(stamp);
            cache.map.entry(key).or_insert(clearance);
        }

        (clearance, source)
    }

    /// Port of `DRC_ENGINE::InitializeClearanceCache`: pre-populates the own-clearance cache
    /// for every track (vias on each enabled copper layer, other tracks on their layer) and
    /// every footprint pad on each enabled copper layer, so that the first render of the
    /// clearance outlines does not have to wait.
    ///
    /// The C++ evaluates the (item, layer) list in blocks on the thread pool and merges the
    /// per-block maps with `insert` (an existing entry is kept); the port evaluates it
    /// sequentially in list order with the same keep-existing merge, which gives the same
    /// cache contents.
    pub fn initialize_clearance_cache<'x>(&self, ctx: &'x BoardCtx<'x>) {
        let board = ctx.board;

        // Pre-populate the cache for all connected items to avoid delays during first render.
        // We only need to cache copper layers since clearance outlines are only drawn on copper.
        let copper_layers = board.enabled_layers() & Lset::all_cu_mask();

        // Build flat list of (item, layer) pairs to process.
        let mut items_to_process: Vec<(ItemRef<'x>, PcbLayerId)> = Vec::new();

        for (id, _) in board.tracks() {
            let Some(track) = ctx.item(id.item_id()) else {
                continue;
            };

            if track.type_() == KicadT::PcbViaT {
                for layer in (track.layer_set() & &copper_layers).seq().iter() {
                    items_to_process.push((track, *layer));
                }
            } else {
                items_to_process.push((track, track.layer()));
            }
        }

        for (fp, _) in board.footprints() {
            for (pad_id, _) in board.footprint_pads(fp) {
                let Some(pad) = ctx.item(pad_id.item_id()) else {
                    continue;
                };

                for layer in (pad.layer_set() & &copper_layers).seq().iter() {
                    items_to_process.push((pad, *layer));
                }
            }
        }

        if items_to_process.is_empty() {
            return;
        }

        let mut local_cache: Vec<((Kiid, PcbLayerId), i32)> = Vec::new();

        for (item, layer) in items_to_process {
            let mut constraint_type = DrcConstraintT::ClearanceConstraint;

            if let Some(pad) = item.as_pad()
                && pad.attribute() == PadAttrib::Npth
            {
                constraint_type = DrcConstraintT::HoleClearanceConstraint;
            }

            let constraint = self.eval_rules(ctx, constraint_type, Some(item), None, layer);

            let mut clearance = 0;

            if constraint.value.has_min() {
                clearance = constraint.value.min();
            }

            local_cache.push(((item.uuid(), layer), clearance));
        }

        // Now merge with write lock held
        let mut cache = self.lock_own_clearance_cache();
        cache.sync_stamp(board.time_stamp());

        for (key, clearance) in local_cache {
            cache.map.entry(key).or_insert(clearance);
        }
    }

    /// Port of `DRC_ENGINE::InvalidateClearanceCache`: drop the entries of `uuid` (on the
    /// board's enabled copper layers when a board is given, else on every layer).
    pub fn invalidate_clearance_cache(&self, board: Option<&Board>, uuid: &Kiid) {
        let mut cache = self.lock_own_clearance_cache();

        if let Some(board) = board {
            let copper_layers = board.enabled_layers() & Lset::all_cu_mask();

            for layer in copper_layers.seq().iter() {
                cache.map.remove(&(*uuid, *layer));
            }
        } else {
            cache.map.retain(|(id, _), _| id != uuid);
        }
    }
}
