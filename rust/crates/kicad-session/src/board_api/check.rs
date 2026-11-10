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
    /// Nets the run's connectivity build gave to items (the cache generator's
    /// `Build` + `PropagateNets`): damage for [`Session::refill_zones_affected`].
    #[serde(default, skip_serializing_if = "super::refill::ChangeDamage::is_empty")]
    pub damage: super::refill::ChangeDamage,
    /// [`Session::check_items`]: the providers of the DRC (`PROVIDER_ORDER`) it did not run,
    /// whose violations involving the items are therefore not in the result (see
    /// [`CHECK_ITEMS_PROVIDERS_DOC`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers_not_run: Vec<String>,
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

        let tracking_valid = self.fill_tracking_valid();
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
                    self.rules_text.as_deref().map(|t| (t, rules_path.as_str())),
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
        let nets_before = super::refill::net_snapshot(&self.board);
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
        summary.damage = super::refill::net_damage(&self.board, &nets_before);

        // A newly compiled engine: the rules the next refill reads are not known to be the
        // last refill's.
        if needs_init {
            summary.damage = super::refill::ChangeDamage::full();
        }

        self.fill_tracking_advance(tracking_valid, &summary.damage);
        summary.checks_not_run = outcome
            .checks_not_run
            .iter()
            .map(|c| format!("{}: {}", c.provider, c.reason))
            .collect();
        Ok(summary)
    }
}

/// The providers [`Session::check_items`] runs when none are named: those whose entries are
/// well defined per item (see [`CHECK_ITEMS_PROVIDERS_DOC`]).
pub const CHECK_ITEMS_PROVIDERS: [&str; 8] = [
    "annular_width",
    "disallow",
    "connectivity",
    "clearance",
    "edge_clearance",
    "hole_to_hole_clearance",
    "width",
    "diameter",
];

/// How each provider of [`CHECK_ITEMS_PROVIDERS`] reports in C++ (`pcbnew/drc/
/// drc_test_provider_*.cpp`), and why the scoped result keeps the same entries. The scoped
/// check runs the providers in full with `aReportAllTrackErrors = true` and keeps the
/// entries that name a given item, so every kept entry is byte for byte the entry of the
/// full run (same item order, position, layer, message and rule), whichever item of a pair
/// the C++ loop happened to visit first:
///
/// | provider | C++ loop | entries |
/// |---|---|---|
/// | `clearance` | every copper item against the r-tree; track-track pairs once (the track with the smaller UUID reports), track against pads / zones from the track, pad-pad once per pair, zone-zone once per pair and layer; without all-track-errors a track stops after its first error | once per pair and layer |
/// | `edge_clearance` | every item against the edge / margin shapes | once per item (per edge shape with all-track-errors) |
/// | `hole_to_hole_clearance` | via holes then pad holes against the hole r-tree, `checkedPairs` shared by both passes | once per hole pair |
/// | `width` | every track and arc | once per item |
/// | `diameter` | every via (and pad stack layer) | once per item |
/// | `annular_width` | every via and plated pad | once per item |
/// | `disallow` | every item against the disallow rules (keepout rule areas compile to implicit rules) | once per item and rule |
/// | `connectivity` | dangling tracks and vias per item; unconnected items per ratsnest edge (two items); isolated copper per zone island | per item / per edge / per island; the entries of [`NET_SCOPED_TYPES`] are kept when an item they name is a given item or has the net of one (after the run) |
///
/// Entries the scoped check keeps without their naming a given item: `isolated_copper` names
/// the zone only (`drc_test_provider_connectivity.cpp`), so it is kept when the zone has the
/// net of a given item or when the zone's bounding box meets a given item's bounding box
/// inflated by the board's worst clearance on a shared layer (the island a track of another
/// net cuts off); a track left dangling by a removal names the remaining track, kept through
/// its net; unconnected entries name the two ends of a ratsnest edge, kept through their
/// nets.
///
/// Not in the result unless named in [`CheckItemsOptions::providers`] (each run is reported
/// in [`DrcSummary::providers_not_run`]): every other provider of the DRC, in particular
/// `physical_clearance` (whose entries of `physical_clearance` / `physical_hole_clearance`
/// rules carry the same `clearance` / `hole_clearance` type keys as the clearance
/// provider's), `solder_mask_issues` (`solder_mask_bridge`), `hole_size`, `copper width`
/// (connection width), `miscellaneous` (disabled layer, assertions), `track angle`,
/// `segment_length`, `microvia_stack`, `via_stitch_overlap`, `zone connections`, `length`,
/// `diff_pair_coupling`, `creepage` and `sliver checker`. Exclusions are not
/// resolved: an entry the board excludes keeps its rule severity.
pub const CHECK_ITEMS_PROVIDERS_DOC: () = ();

/// The connectivity entries [`Session::check_items`] keeps when one of their items has the
/// net of a given item (after the run), besides those naming a given item.
pub const NET_SCOPED_TYPES: [&str; 4] = [
    "unconnected_items",
    "isolated_copper",
    "track_dangling",
    "via_dangling",
];

/// Options of [`Session::check_items`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CheckItemsOptions {
    /// The providers (by `GetName()`); empty = [`CHECK_ITEMS_PROVIDERS`].
    pub providers: Vec<String>,
    /// Severity names kept; empty = error and warning.
    pub severities: Vec<String>,
}

impl Session {
    /// The violations that involve at least one of the items `handles`: the subset of a run
    /// of the chosen providers with `aReportAllTrackErrors = true` (`kicad-cli pcb drc
    /// --all-track-errors`) whose entries name one of the items; the entries of
    /// [`NET_SCOPED_TYPES`] are also kept when one of their items has the net of a given item
    /// (the nets after the run, whose connectivity build can re-net items). The board's
    /// markers are not touched. The run is a full run of the chosen providers (no persistent
    /// DRC state); [`DrcSummary::providers_not_run`] lists the providers it skipped. See
    /// [`CHECK_ITEMS_PROVIDERS_DOC`].
    pub fn check_items(
        &mut self,
        handles: &[u64],
        options: &CheckItemsOptions,
    ) -> Result<DrcSummary, SessionError> {
        {
            let ctx = self.api_ctx();

            for &h in handles {
                let id = super::item_id(h)?;
                ctx.item(id).ok_or(SessionError::StaleId { id: h })?;
            }
        }

        let providers = if options.providers.is_empty() {
            CHECK_ITEMS_PROVIDERS
                .iter()
                .map(|p| p.to_string())
                .collect()
        } else {
            options.providers.clone()
        };
        let full = self.quick_drc(&QuickDrcOptions {
            providers: providers.clone(),
            all_track_errors: true,
            severities: options.severities.clone(),
        })?;

        let wanted: std::collections::BTreeSet<u64> = handles.iter().copied().collect();
        let ctx = self.api_ctx();

        // The nets of the items after the run: its connectivity build can re-net them
        // (`PropagateNets`), and the entries name the items with those nets.
        let nets: std::collections::BTreeSet<i32> = handles
            .iter()
            .filter_map(|&h| ctx.item(kicad_pcb::ItemId::from_bits(h)?)?.net_code())
            .map(|n| n.0)
            .filter(|&n| n > 0)
            .collect();
        // The given items' areas, inflated by the board's worst clearance: an isolated island
        // of a zone of another net that an item splits off lies within its zone's bounding
        // box, which then meets the item's area on a shared layer.
        let worst = self.board.get_max_clearance_value(&ctx);
        let areas: Vec<(kicad_geom::Box2I, kicad_core::lset::Lset)> = handles
            .iter()
            .filter_map(|&h| ctx.item(kicad_pcb::ItemId::from_bits(h)?))
            .map(|it| {
                let mut b = it.bounding_box();
                b.inflate(worst);
                (b, it.layer_set())
            })
            .collect();
        let zone_area = |v: &ViolationItem| {
            let it = ctx.item(kicad_pcb::ItemId::from_bits(v.id?)?)?;
            (it.type_() == kicad_core::typeinfo::KicadT::PcbZoneT)
                .then(|| (it.bounding_box(), it.layer_set()))
        };
        let net_of = |v: &ViolationItem| {
            v.id.and_then(kicad_pcb::ItemId::from_bits)
                .and_then(|id| ctx.item(id))
                .and_then(|it| it.net_code())
                .map(|n| n.0)
        };
        let mut s = DrcSummary {
            checks_not_run: full.checks_not_run,
            damage: full.damage,
            providers_not_run: kicad_drc::engine::PROVIDER_ORDER
                .iter()
                .filter(|p| !providers.iter().any(|q| q == *p))
                .map(|p| p.to_string())
                .collect(),
            ..DrcSummary::default()
        };

        for v in full.violations {
            let named = v
                .items
                .iter()
                .any(|i| i.id.is_some_and(|id| wanted.contains(&id)));
            let by_net = NET_SCOPED_TYPES.contains(&v.violation_type.as_str())
                && v.items
                    .iter()
                    .any(|i| net_of(i).is_some_and(|n| nets.contains(&n)));
            let zone_near = v.violation_type == "isolated_copper"
                && v.items.iter().any(|i| {
                    zone_area(i).is_some_and(|(bbox, layers)| {
                        areas
                            .iter()
                            .any(|(b, l)| (&layers & l).any() && bbox.intersects(b))
                    })
                });

            if !(named || by_net || zone_near) {
                continue;
            }

            match v.section.as_str() {
                "marker" => s.markers += 1,
                "unconnected" => s.unconnected += 1,
                _ => s.parity += 1,
            }

            s.violations.push(v);
        }

        Ok(s)
    }
}
