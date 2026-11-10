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

//! Structured DRC for the router loop.
//!
//! * [`Session::drc_violations`]: the markers a full `JobExportDrc` run
//!   ([`crate::run_drc`]) left on the board, filtered by the job's severities as the three
//!   `DRC_ITEMS_PROVIDER`s of the report do.
//! * [`Session::quick_drc`]: `DRC_ENGINE::RunTests` restricted to chosen providers (the
//!   `qa/tests/pcbnew/drc` pattern of disabling the others,
//!   `kicad_drc::run_flow::run_tests_selected`), violations collected by the violation
//!   handler; the board's markers are not touched.

use std::sync::Arc;

use kicad_core::Severity;
use kicad_geom::Vector2I;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker, RcItem};
use serde::{Deserialize, Serialize};

use super::{Pt, enum_name, layer_name, pt};
use crate::error::SessionError;
use crate::session::Session;

/// An item named by a violation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationItem {
    /// The UUID (`RC_ITEM::GetIDs()`).
    pub uuid: String,
    /// The handle when the UUID names a board item.
    pub id: Option<u64>,
}

/// A DRC violation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    /// `GetSettingsKey()` (the JSON report's `type`: `clearance`, `unconnected_items`, ...).
    pub violation_type: String,
    /// `GetErrorCode()` (`DRCE_*`).
    pub code: i32,
    /// `PCB_MARKER::GetSeverity()`: `error`, `warning`, `exclusion`, `ignore`.
    pub severity: String,
    /// `GetErrorMessage()` (title plus detail).
    pub description: String,
    /// `GetPosition()`.
    pub position: Pt,
    /// The marker layer (`LSET::Name`).
    pub layer: String,
    /// The items in `GetIDs()` order (nil UUIDs left out).
    pub items: Vec<ViolationItem>,
    /// The violated rule (`GetViolatingRule()->m_Name`).
    pub rule: Option<String>,
    /// `marker`, `unconnected` or `parity`: the report section.
    pub section: String,
}

/// What the structured DRC calls return.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrcSummary {
    /// The violations in report order.
    pub violations: Vec<Violation>,
    /// Count of the `marker` section.
    pub markers: u32,
    /// Count of the `unconnected` section.
    pub unconnected: u32,
    /// Count of the `parity` section.
    pub parity: u32,
    /// The providers of the C++ registry the port did not run (`checks_not_run`).
    pub checks_not_run: Vec<String>,
}

/// Options of [`Session::quick_drc`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct QuickDrcOptions {
    /// The providers to run, by `GetName()` (`clearance`, `connectivity`, `hole_to_hole_clearance`,
    /// `edge_clearance`, `width`, `diameter`, `annular_width`, `disallow`, ...); empty = all.
    pub providers: Vec<String>,
    /// `aReportAllTrackErrors`.
    pub all_track_errors: bool,
    /// Severity names (`error`, `warning`, `exclusions`) kept; empty = error and warning.
    pub severities: Vec<String>,
}

/// The severity mask of a list of names (`RPT_SEVERITY_*` bits); `default` when empty.
pub fn severity_mask(names: &[String], default: i32) -> i32 {
    let mut mask = 0;

    for s in names {
        mask |= match s.as_str() {
            "error" => Severity::Error.as_i32(),
            "warning" => Severity::Warning.as_i32(),
            "exclusions" | "exclusion" => Severity::Exclusion.as_i32(),
            "ignore" => Severity::Ignore.as_i32(),
            _ => 0,
        };
    }

    if mask == 0 { default } else { mask }
}

fn section_of(m: MarkerT) -> &'static str {
    match m {
        MarkerT::Ratsnest => "unconnected",
        MarkerT::Parity => "parity",
        _ => "marker",
    }
}

fn violation(ctx: &BoardCtx<'_>, marker: &PcbMarker) -> Violation {
    let rc: &RcItem = marker.rc_item();
    let severity = marker.severity(ctx);

    Violation {
        violation_type: rc.settings_key().to_string(),
        code: rc.error_code(),
        severity: enum_name(severity).to_ascii_lowercase(),
        description: rc.error_message(),
        position: pt(marker.pos()),
        layer: layer_name(marker.base.layer),
        items: rc
            .ids()
            .iter()
            .filter(|k| **k != kicad_core::kiid::NILUUID)
            .map(|k| ViolationItem {
                uuid: k.as_string(),
                id: ctx.board.resolve_item(k).map(|i| i.to_bits()),
            })
            .collect(),
        rule: rc.violating_rule().map(|r| r.name.clone()),
        section: section_of(marker.marker_type()).to_string(),
    }
}

fn summarize(ctx: &BoardCtx<'_>, markers: &[&PcbMarker], mask: i32) -> DrcSummary {
    let mut s = DrcSummary::default();

    // DRC_ITEMS_PROVIDER order: the three report sections in turn.
    for section in ["marker", "unconnected", "parity"] {
        for m in markers {
            let v = violation(ctx, m);

            if v.section != section || marker_severity_bits(ctx, m) & mask == 0 {
                continue;
            }

            match section {
                "marker" => s.markers += 1,
                "unconnected" => s.unconnected += 1,
                _ => s.parity += 1,
            }

            s.violations.push(v);
        }
    }

    s
}

/// `DRC_ITEMS_PROVIDER::SetSeverities` filter: an excluded marker counts as `EXCLUSION`.
fn marker_severity_bits(ctx: &BoardCtx<'_>, m: &PcbMarker) -> i32 {
    if m.is_excluded() {
        Severity::Exclusion.as_i32()
    } else {
        m.severity(ctx).as_i32()
    }
}

impl Session {
    /// The violations of the markers on the board (after [`crate::run_drc`]), kept when their
    /// severity is in `severities` (`RPT_SEVERITY_*` mask).
    pub fn drc_violations(&self, severities: i32) -> DrcSummary {
        let ctx = self.api_ctx();
        let markers: Vec<&PcbMarker> = self.board.markers().map(|(_, m)| m).collect();
        summarize(&ctx, &markers, severities)
    }

    /// `DRC_ENGINE::RunTests` with only the chosen providers; the violations as markers would
    /// show them, the board's markers unchanged. Initialises the DRC engine from the
    /// session's rules first when it has none or its rules are not valid (as `JobExportDrc`).
    pub fn quick_drc(&mut self, options: &QuickDrcOptions) -> Result<DrcSummary, SessionError> {
        use kicad_drc::engine::DrcRunState;
        use kicad_drc::run_flow::{RunTestsOptions, run_tests_selected};

        for p in &options.providers {
            if !kicad_drc::engine::PROVIDER_ORDER.contains(&p.as_str()) {
                return Err(SessionError::invalid_argument(format!(
                    "unknown DRC provider '{p}'"
                )));
            }
        }

        let needs_init = !self
            .drc_engine
            .as_ref()
            .is_some_and(|engine| engine.rules_valid());

        if needs_init {
            let mut engine = kicad_drc::engine::DrcEngine::new();
            let mut rules_path = self.board.get_design_rules_path();

            if rules_path.is_empty() {
                rules_path = crate::board_loader::with_extension(&self.source_name, "kicad_dru");
            }

            let layer_choices = kicad_drc::rule_parser::board_layer_choices(&self.board);
            let tuning_profiles = crate::board_loader::tuning_profiles(&self.board);
            engine
                .init_engine(
                    &mut self.board,
                    self.rules_text
                        .as_deref()
                        .map(|t| (t, rules_path.as_str())),
                    layer_choices,
                    &tuning_profiles,
                    None,
                )
                .map_err(|e| {
                    SessionError::invalid_argument(format!(
                        "DRC incomplete: could not compile custom design rules.\n{}",
                        e.what()
                    ))
                })?;
            self.drc_engine = Some(engine);
        }

        let tuning_profiles = crate::board_loader::tuning_profiles(&self.board);
        let mut found: Vec<(Arc<RcItem>, Vector2I, i32)> = Vec::new();
        let engine = self.drc_engine.take().expect("engine initialised above");

        let outcome = {
            let mut run = DrcRunState::new();
            run.set_violation_handler(Box::new(
                |item: &Arc<RcItem>,
                 pos: Vector2I,
                 layer: i32,
                 _path: Option<&dyn Fn(&mut PcbMarker)>| {
                    found.push((item.clone(), pos, layer));
                },
            ));

            let run_options = RunTestsOptions {
                units: kicad_core::EdaUnits::Mm,
                report_all_track_errors: options.all_track_errors,
                test_footprints: false,
                tuning_profiles: &tuning_profiles,
                footprint_libraries: self.footprint_libraries.as_ref(),
            };
            let wanted = &options.providers;
            let selected = |name: &str| wanted.is_empty() || wanted.iter().any(|w| w == name);
            let outcome = run_tests_selected(
                &engine,
                &mut run,
                &mut self.board,
                &mut self.connectivity,
                &run_options,
                &selected,
            );
            run.clear_violation_handler();
            outcome
        };

        self.drc_engine = Some(engine);

        let mut scratch = kicad_core::KiidGenerator::default();
        scratch.create_nil_uuids(true);
        let markers: Vec<PcbMarker> = found
            .into_iter()
            .map(|(item, pos, layer)| PcbMarker::new(&mut scratch, item, pos, layer))
            .collect();
        let refs: Vec<&PcbMarker> = markers.iter().collect();
        let ctx = self.api_ctx();
        let mask = severity_mask(
            &options.severities,
            Severity::Error.as_i32() | Severity::Warning.as_i32(),
        );
        let mut summary = summarize(&ctx, &refs, mask);
        summary.checks_not_run = outcome
            .checks_not_run
            .iter()
            .map(|c| format!("{}: {}", c.provider, c.reason))
            .collect();
        Ok(summary)
    }
}
