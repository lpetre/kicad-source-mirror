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

//! Port of the DRC run flow: `DRC_ENGINE::RunTests` (`pcbnew/drc/drc_engine.cpp`) driving
//! the real cache generator and every test provider of the registry in C++ order, and the
//! DRC half of `PCBNEW_JOBS_HANDLER::JobExportDrc` (`pcbnew/pcbnew_jobs_handler.cpp`): marker
//! creation in the violation handler, `RecordDRCExclusions` / `DeleteMARKERs` /
//! `ResolveDRCExclusions( false )`, the three `DRC_ITEMS_PROVIDER`s and the text / JSON report
//! of DRC-11. Work package DRC-12.
//!
//! The rest of `JobExportDrc` (output path, `InitEngine` retry and its fatal error, schematic
//! parity netlist, zone refill, the reporter messages, exit codes, `--save-board`) lives in
//! `kicad-session` (`jobs_handler::run_drc`), which calls [`run_drc_job`].
//!
//! # Providers that are not ported yet
//!
//! The registry slots whose provider has not landed ([`UNPORTED_PROVIDERS`]) are never run
//! (their modules are placeholders). Each run reports such a slot as a [`CheckNotRun`] unless
//! the provider's own first statements prove that it would report nothing on this board (for
//! instance `DRC_TEST_PROVIDER_CREEPAGE::Run` returns at once without a `creepage` rule): the
//! guards in [`unported_check_would_run`] only read the board and the engine and never
//! produce a violation. [`with_checks_not_run_text`] / [`with_checks_not_run_json`] add the
//! list to the reports, so a report never silently omits a check; with an empty list the
//! reports are byte for byte those of the C++.

use std::sync::Arc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::eda_units::EdaUnits;
use kicad_core::json::{ObjectOrder, Value, dump_stream};
use kicad_core::{Kiid, Severity};
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::{
    DRCE_CREEPAGE, DRCE_DP_GAP_OUT_OF_RANGE, DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG,
    DRCE_DUPLICATE_FOOTPRINT, DRCE_EXTRA_FOOTPRINT, DRCE_FOOTPRINT_FILTERS,
    DRCE_LENGTH_OUT_OF_RANGE, DRCE_LIB_FOOTPRINT_ISSUES, DRCE_LIB_FOOTPRINT_MISMATCH,
    DRCE_MISSING_FOOTPRINT, DRCE_NET_CHAIN_RETURN_PATH_BREAK, DRCE_NET_CHAIN_STUB_TOO_LONG,
    DRCE_NET_CHAIN_TUNING_PROFILES, DRCE_NET_CONFLICT, DRCE_SCHEMATIC_FIELDS_PARITY,
    DRCE_SCHEMATIC_PARITY, DRCE_SKEW_OUT_OF_RANGE, DRCE_VIA_COUNT_OUT_OF_RANGE,
    DRCE_VIA_STITCH_OVERLAP,
};
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker, RcItem};
use kicad_pcb::{Board, BoardCtx, ConnectivityQuery};

use crate::cache_generator::{CacheGeneratorConnectivity, DrcCacheGenerator};
use crate::caches::{DrcBoardCaches, ZoneIslandsMap};
use crate::constraint_types::DrcConstraintT;
use crate::engine::diff_pair::is_net_a_diff_pair;
use crate::engine::implicit_rules::TuningProfiles;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun, PROVIDER_ORDER};
use crate::exclusion::{record_drc_exclusions, resolve_drc_exclusions};
use crate::item::DrcItemsProvider;
use crate::pcbexpr::functions::PcbExprCacheScope;
use crate::providers::annular_width::DrcTestProviderAnnularWidth;
use crate::providers::connection_width::DrcTestProviderConnectionWidth;
use crate::providers::connectivity::DrcTestProviderConnectivity;
use crate::providers::copper_clearance::DrcTestProviderCopperClearance;
use crate::providers::courtyard_clearance::DrcTestProviderCourtyardClearance;
use crate::providers::disallow::DrcTestProviderDisallow;
use crate::providers::edge_clearance::DrcTestProviderEdgeClearance;
use crate::providers::footprint_checks::DrcTestProviderFootprintChecks;
use crate::providers::hole_size::DrcTestProviderHoleSize;
use crate::providers::hole_to_hole::DrcTestProviderHoleToHole;
use crate::providers::misc::DrcTestProviderMisc;
use crate::providers::physical_clearance::DrcTestProviderPhysicalClearance;
use crate::providers::run_host::RunHost;
use crate::providers::silk_clearance::DrcTestProviderSilkClearance;
use crate::providers::sliver_checker::DrcTestProviderSliverChecker;
use crate::providers::solder_mask::DrcTestProviderSolderMask;
use crate::providers::text_dims::DrcTestProviderTextDims;
use crate::providers::text_mirroring::DrcTestProviderTextMirroring;
use crate::providers::track_angle::DrcTestProviderTrackAngle;
use crate::providers::track_segment_length::DrcTestProviderTrackSegmentLength;
use crate::providers::track_width::DrcTestProviderTrackWidth;
use crate::providers::via_diameter::DrcTestProviderViaDiameter;
use crate::providers::via_stack::DrcTestProviderViaStack;
use crate::providers::zone_connections::DrcTestProviderZoneConnections;
use crate::report::DrcReport;

/// A registry slot whose test provider is not ported yet: its `GetName()`, the work package
/// that ports it and the error codes it can report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnportedProvider {
    /// `GetName()` (an entry of [`PROVIDER_ORDER`]).
    pub name: &'static str,
    /// The work package of `rust/docs/analysis/drc.md` section 10.2.
    pub package: &'static str,
    /// The `DRCE_*` codes the C++ provider creates.
    pub codes: &'static [i32],
}

/// The providers of [`PROVIDER_ORDER`] that are not ported yet (registry order).
pub const UNPORTED_PROVIDERS: [UnportedProvider; 7] = [
    UnportedProvider {
        name: "creepage",
        package: "DRC-33",
        codes: &[DRCE_CREEPAGE],
    },
    UnportedProvider {
        name: "library_parity",
        package: "DRC-34",
        codes: &[DRCE_LIB_FOOTPRINT_ISSUES, DRCE_LIB_FOOTPRINT_MISMATCH],
    },
    UnportedProvider {
        name: "schematic_parity",
        package: "out of scope",
        codes: &[
            DRCE_DUPLICATE_FOOTPRINT,
            DRCE_EXTRA_FOOTPRINT,
            DRCE_FOOTPRINT_FILTERS,
            DRCE_MISSING_FOOTPRINT,
            DRCE_NET_CONFLICT,
            DRCE_SCHEMATIC_FIELDS_PARITY,
            DRCE_SCHEMATIC_PARITY,
        ],
    },
    UnportedProvider {
        name: "net chain tuning profiles",
        package: "DRC-34",
        codes: &[DRCE_NET_CHAIN_TUNING_PROFILES],
    },
    UnportedProvider {
        name: "via_stitch_overlap",
        package: "DRC-34",
        codes: &[DRCE_VIA_STITCH_OVERLAP],
    },
    UnportedProvider {
        name: "length",
        package: "DRC-31",
        codes: &[
            DRCE_LENGTH_OUT_OF_RANGE,
            DRCE_NET_CHAIN_RETURN_PATH_BREAK,
            DRCE_NET_CHAIN_STUB_TOO_LONG,
            DRCE_SKEW_OUT_OF_RANGE,
            DRCE_VIA_COUNT_OUT_OF_RANGE,
        ],
    },
    UnportedProvider {
        name: "diff_pair_coupling",
        package: "DRC-30",
        codes: &[DRCE_DP_GAP_OUT_OF_RANGE, DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG],
    },
];

/// The unported provider of registry slot `name`, if it is one.
pub fn unported_provider(name: &str) -> Option<&'static UnportedProvider> {
    UNPORTED_PROVIDERS.iter().find(|p| p.name == name)
}

/// A check of the C++ run that the port did not perform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckNotRun {
    /// The provider's `GetName()`.
    pub provider: &'static str,
    /// Why it did not run (e.g. `not ported yet (DRC-33)`).
    pub reason: String,
}

/// The options of `DRC_ENGINE::RunTests( aUnits, aReportAllTrackErrors, aTestFootprints )`
/// plus the inputs the providers read from the project.
#[derive(Clone, Debug)]
pub struct RunTestsOptions<'a> {
    /// `aUnits`.
    pub units: EdaUnits,
    /// `aReportAllTrackErrors` (`--all-track-errors`).
    pub report_all_track_errors: bool,
    /// `aTestFootprints` (schematic parity).
    pub test_footprints: bool,
    /// The project's `TUNING_PROFILES` (misc provider).
    pub tuning_profiles: &'a TuningProfiles,
}

/// The result of [`run_tests`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunTestsOutcome {
    /// The run was cancelled (cache generator or a provider returned false).
    pub cancelled: bool,
    /// The registry slots that the C++ would have run but the port did not, in registry
    /// order.
    pub checks_not_run: Vec<CheckNotRun>,
}

/// `board->GetConnectivity()` as the cache generator uses it: the load-time connectivity for
/// the item shapes (flashing), then `ClearRatsnest()`, `Build( board )` and
/// `FillIsolatedIslandsMap()` on the same object.
///
/// `Build` records the net propagation; [`run_tests`] applies it to the board right after
/// the generator (`FillIsolatedIslandsMap( map, true )` in between reads only the connectivity
/// graph and the zone fills, which propagation does not change).
struct GeneratorConnectivity<'a> {
    conn: &'a mut ConnectivityData,
}

impl CacheGeneratorConnectivity for GeneratorConnectivity<'_> {
    fn query(&self) -> Option<&dyn ConnectivityQuery> {
        Some(&*self.conn)
    }

    fn clear_ratsnest(&mut self) {
        self.conn.clear_ratsnest();
    }

    fn build(&mut self, board: &Board) -> bool {
        // aBoard->CacheTriangulation() ran in the generator's prepare_board().
        self.conn.build(&BoardCtx::bare(board))
    }

    fn fill_isolated_islands_map(
        &mut self,
        board: &Board,
        map: &mut ZoneIslandsMap,
        connectivity_already_rebuilt: bool,
    ) {
        let mut islands: kicad_connectivity::algo::IsolatedIslandsMap =
            map.iter().map(|(z, m)| (z, m.clone())).collect();

        self.conn.fill_isolated_islands_map(
            &BoardCtx::bare(board),
            &mut islands,
            connectivity_already_rebuilt,
        );

        for (zone, layers) in islands {
            *map.entry(zone) = layers;
        }
    }
}

/// Whether the unported provider `p` would test anything on this board: its C++ `Run()`
/// up to the first statement that can report (the early returns only). `None` when it would
/// return without testing, else the reason for the [`CheckNotRun`].
pub fn unported_check_would_run(
    p: &UnportedProvider,
    engine: &DrcEngine,
    run: &DrcRunState<'_>,
    board: &Board,
) -> Option<String> {
    let not_ported = || Some(format!("not ported yet ({})", p.package));

    // Every code of the provider is ignored (error limit 0): `ReportViolation` drops all its
    // violations, whatever it would test.
    if p.codes
        .iter()
        .all(|&code| run.is_error_limit_exceeded(code))
    {
        return None;
    }

    match p.name {
        // if( !m_drcEngine->HasRulesForConstraintType( CREEPAGE_CONSTRAINT ) ) return true;
        "creepage" => {
            if !engine.has_rules_for_constraint_type(DrcConstraintT::CreepageConstraint) {
                return None;
            }

            not_ported()
        }
        // if( !project ) return true; then every footprint is looked up in the library table.
        "library_parity" => {
            if board.project.is_none() || board.footprints().next().is_none() {
                return None;
            }

            not_ported()
        }
        // if( m_drcEngine->GetTestFootprints() ) { ... }
        "schematic_parity" => {
            if !run.test_footprints() {
                return None;
            }

            Some("not ported (schematic parity is out of scope)".to_string())
        }
        // if( IsErrorLimitExceeded( DRCE_NET_CHAIN_TUNING_PROFILES ) ) return true; then the
        // nets with a net chain.
        "net chain tuning profiles" => {
            if run.is_error_limit_exceeded(DRCE_NET_CHAIN_TUNING_PROFILES)
                || board.nets().all(|net| net.net_chain().is_empty())
            {
                return None;
            }

            not_ported()
        }
        // if( IsErrorLimitExceeded( DRCE_VIA_STITCH_OVERLAP ) ) return true; then the
        // PCB_VIA_STITCH generators of the board.
        "via_stitch_overlap" => {
            if run.is_error_limit_exceeded(DRCE_VIA_STITCH_OVERLAP)
                || board
                    .generators()
                    .all(|(_, g)| g.generator_type != "via_stitch")
            {
                return None;
            }

            not_ported()
        }
        // Every report needs a resolved length, skew, via count or net chain constraint.
        "length" => {
            let types = [
                DrcConstraintT::LengthConstraint,
                DrcConstraintT::NetChainLengthConstraint,
                DrcConstraintT::NetChainStubLengthConstraint,
                DrcConstraintT::NetChainReturnPathConstraint,
                DrcConstraintT::SkewConstraint,
                DrcConstraintT::ViaCountConstraint,
            ];

            if !types
                .iter()
                .any(|&t| engine.has_rules_for_constraint_type(t))
            {
                return None;
            }

            not_ported()
        }
        // Every report needs a track, arc or via whose net is a diff pair
        // (DRC_ENGINE::IsNetADiffPair) with a resolved diff pair gap or max uncoupled
        // constraint.
        "diff_pair_coupling" => {
            if (!engine.has_rules_for_constraint_type(DrcConstraintT::DiffPairGapConstraint)
                && !engine.has_rules_for_constraint_type(DrcConstraintT::MaxUncoupledConstraint))
                || board
                    .nets()
                    .all(|net| is_net_a_diff_pair(board, net).is_none())
            {
                return None;
            }

            not_ported()
        }
        _ => not_ported(),
    }
}

/// Runs one provider of the registry (`provider->RunTests( aUnits )`): false when it was
/// cancelled. The providers are created per run with the caches of the generator.
#[allow(clippy::too_many_arguments)]
fn run_provider(
    name: &str,
    engine: &DrcEngine,
    run: &mut DrcRunState<'_>,
    board: &Board,
    conn: &mut ConnectivityData,
    caches: &DrcBoardCaches,
    options: &RunTestsOptions<'_>,
) -> bool {
    let units = options.units;

    // The two providers that query the connectivity through `&mut` get a context without
    // it: the connectivity provider builds its own from the same object; for the zone
    // connections provider the item shapes see a second build of the same connectivity.
    match name {
        "connectivity" => {
            let ctx = BoardCtx::bare(board).with_rules(engine);
            return DrcTestProviderConnectivity::new(conn, caches)
                .run_tests(engine, run, &ctx, units);
        }
        "zone connections" => {
            let mut query = ConnectivityData::new();

            if !caches.copper_zones.is_empty() {
                query.build(&BoardCtx::bare(board));
            }

            let ctx = BoardCtx::bare(board)
                .with_rules(engine)
                .with_connectivity(&query);
            return DrcTestProviderZoneConnections::new(conn, caches)
                .run_tests(engine, run, &ctx, units);
        }
        _ => {}
    }

    let ctx = BoardCtx::bare(board)
        .with_rules(engine)
        .with_connectivity(&*conn);

    let mut provider: Box<dyn DrcTestProviderRun + '_> = match name {
        "annular_width" => Box::new(DrcTestProviderAnnularWidth::default()),
        "disallow" => Box::new(DrcTestProviderDisallow::new(caches)),
        "copper width" => Box::new(DrcTestProviderConnectionWidth::new(caches)),
        "clearance" => Box::new(DrcTestProviderCopperClearance::new(caches)),
        "physical_clearance" => Box::new(DrcTestProviderPhysicalClearance::new(caches)),
        "courtyard_clearance" => Box::new(DrcTestProviderCourtyardClearance::default()),
        "edge_clearance" => Box::new(DrcTestProviderEdgeClearance::default()),
        "footprint checks" => Box::new(DrcTestProviderFootprintChecks::default()),
        "hole_to_hole_clearance" => Box::new(DrcTestProviderHoleToHole::default()),
        "hole_size" => Box::new(DrcTestProviderHoleSize::default()),
        "miscellaneous" => Box::new(DrcTestProviderMisc::new(options.tuning_profiles.clone())),
        "text_dimensions" => Box::new(DrcTestProviderTextDims::default()),
        "track angle" => Box::new(DrcTestProviderTrackAngle::new(&*conn)),
        "width" => Box::new(DrcTestProviderTrackWidth::default()),
        "segment_length" => Box::new(DrcTestProviderTrackSegmentLength::default()),
        "diameter" => Box::new(DrcTestProviderViaDiameter::default()),
        "microvia_stack" => Box::new(DrcTestProviderViaStack::default()),
        "solder_mask_issues" => Box::new(DrcTestProviderSolderMask::new(caches)),
        "silk_clearance" => Box::new(DrcTestProviderSilkClearance::default()),
        "sliver checker" => Box::new(DrcTestProviderSliverChecker::default()),
        "text_mirroring" => Box::new(DrcTestProviderTextMirroring::default()),
        _ => unreachable!("provider '{name}' is neither ported nor in UNPORTED_PROVIDERS"),
    };

    provider.run_tests(engine, run, &ctx, units)
}

/// Port of `DRC_ENGINE::RunTests( aUnits, aReportAllTrackErrors, aTestFootprints )` with
/// the real cache generator and every provider of [`PROVIDER_ORDER`].
///
/// `connectivity` is `board->GetConnectivity()` as the board load built it; the cache
/// generator rebuilds it (`ClearRatsnest`, `Build`) and the providers query the rebuilt one.
/// Violations go to the run state's violation handler. Unported registry slots are skipped
/// and returned as [`CheckNotRun`]s (see the module comment).
///
/// `GetComponentClassManager().ForceComponentClassRecalculation()` is not needed in the
/// port: a footprint's component class is a cache keyed on the manager's ticker and is
/// recomputed from its inputs on use, and nothing between the load and the run changes a
/// footprint. The from-to cache (`GetFromToCache()->Rebuild`) is not ported (`fromTo()`
/// conditions report their missing cache, DRC-22).
pub fn run_tests(
    engine: &DrcEngine,
    run: &mut DrcRunState<'_>,
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    options: &RunTestsOptions<'_>,
) -> RunTestsOutcome {
    let mut outcome = RunTestsOutcome::default();

    // SetUserUnits, the run flags, the error limits, DRC_TEST_PROVIDER::Init() and
    // m_board->IncrementTimeStamp(): invalidate all caches...
    engine.prepare_run(
        run,
        board,
        options.units,
        options.report_all_track_errors,
        options.test_footprints,
    );

    // ... and regenerate them.
    let caches = {
        let mut generator_conn = GeneratorConnectivity {
            conn: &mut *connectivity,
        };
        let mut host = RunHost(run);
        DrcCacheGenerator::new().run(board, engine, &mut generator_conn, &mut host)
    };

    let Some(mut caches) = caches else {
        outcome.cancelled = true;
        return outcome;
    };

    // The SetNetCode calls of the connectivity build (CN_CONNECTIVITY_ALGO::PropagateNets).
    connectivity.apply_net_changes(board);
    caches.time_stamp = board.time_stamp();

    let board: &Board = board;
    let timestamp = board.time_stamp();
    let _expression_caches = PcbExprCacheScope::enter(board);

    for name in PROVIDER_ORDER {
        if let Some(p) = unported_provider(name) {
            if let Some(reason) = unported_check_would_run(p, engine, run, board) {
                outcome.checks_not_run.push(CheckNotRun {
                    provider: p.name,
                    reason,
                });
            }

            continue;
        }

        if let Some(r) = run.log_reporter() {
            r.report(&format!("Run DRC provider: '{name}'"), Severity::Undefined);
        }

        if !run_provider(name, engine, run, board, connectivity, &caches, options) {
            outcome.cancelled = true;
            break;
        }
    }

    // DRC tests are multi-threaded; anything that causes us to attempt to re-generate the
    // caches while DRC is running is problematic.
    debug_assert_eq!(timestamp, board.time_stamp());

    outcome
}

/// The report format of the job (`JOB_PCB_DRC::OUTPUT_FORMAT`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrcReportFormat {
    /// `REPORT`: `DRC_REPORT::WriteTextReport`.
    Text,
    /// `JSON`: `DRC_REPORT::WriteJsonReport`.
    Json,
}

/// The inputs of the DRC part of `JobExportDrc`.
#[derive(Clone, Debug)]
pub struct DrcJobOptions<'a> {
    /// The run options (units, `--all-track-errors`, `checkParity`, tuning profiles).
    pub run: RunTestsOptions<'a>,
    /// `drcJob->m_severity`: the severity mask of the three marker providers.
    pub severities: i32,
    /// The report format.
    pub format: DrcReportFormat,
    /// `GetISO8601CurrentDateTime()` (the host's clock).
    pub date: String,
    /// `GetMajorMinorPatchVersion()` ([`crate::report::KICAD_MAJOR_MINOR_PATCH_VERSION`]).
    pub kicad_version: String,
    /// Seed of the marker UUIDs (`PCB_MARKER` KIIDs are random in C++; the host supplies the
    /// seed). Markers never appear in a report by UUID; see [`run_drc_job`] for collisions.
    pub marker_uuid_seed: u32,
}

/// The result of [`run_drc_job`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrcJobResult {
    /// `markersProvider->GetCount()`.
    pub violations: usize,
    /// `ratsnestProvider->GetCount()`.
    pub unconnected_items: usize,
    /// `fpWarningsProvider->GetCount()`.
    pub parity_issues: usize,
    /// The report file contents (with the [`CheckNotRun`] list, if any).
    pub report: String,
    /// The run was cancelled.
    pub cancelled: bool,
    /// The checks the port did not run.
    pub checks_not_run: Vec<CheckNotRun>,
}

/// A violation as the job's violation handler receives it, turned into its marker.
struct Found {
    marker: PcbMarker,
}

/// The DRC part of `PCBNEW_JOBS_HANDLER::JobExportDrc`, from `SetViolationHandler` to the
/// report writer:
///
/// * violation handler: `new PCB_MARKER( aItem, aPos, aLayer )`, `aPathGenerator( marker )`,
///   `commit.Add( marker )`;
/// * `brd->RecordDRCExclusions()`, `brd->DeleteMARKERs( true, true )`,
///   `drcEngine->RunTests( units, m_reportAllTrackErrors, checkParity )`,
///   `ClearViolationHandler()`, `commit.Push()` (markers appended in report order);
/// * `brd->ResolveDRCExclusions( false )`;
/// * the three `DRC_ITEMS_PROVIDER`s with `SetSeverities( m_severity )`;
/// * `DRC_REPORT::WriteJsonReport` / `WriteTextReport`.
///
/// Marker UUIDs: the C++ `PCB_MARKER` gets a random KIID. Here they come from a generator
/// seeded with `marker_uuid_seed`, and a UUID that names an item of the board (or an earlier
/// marker) is drawn again, so a marker can never take over the resolution of a report item.
pub fn run_drc_job(
    engine: &DrcEngine,
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    options: &DrcJobOptions<'_>,
) -> DrcJobResult {
    let mut found: Vec<Found> = Vec::new();
    // The handler only builds the marker; its final UUID is drawn below.
    let mut scratch_generator = kicad_core::KiidGenerator::seeded(options.marker_uuid_seed);

    record_drc_exclusions(board);
    board.delete_markers(true, true);

    let outcome = {
        let mut run = DrcRunState::new();
        run.set_violation_handler(Box::new(
            |item: &Arc<RcItem>,
             pos: Vector2I,
             layer: i32,
             path: Option<&dyn Fn(&mut PcbMarker)>| {
                let mut marker = PcbMarker::new(&mut scratch_generator, item.clone(), pos, layer);

                if let Some(path) = path {
                    path(&mut marker);
                }

                found.push(Found { marker });
            },
        ));

        let outcome = run_tests(engine, &mut run, board, connectivity, &options.run);
        run.clear_violation_handler();
        outcome
    };

    // commit.Push(): the markers are appended to the board in report order.
    let mut generator = kicad_core::KiidGenerator::seeded(options.marker_uuid_seed.wrapping_add(1));

    for Found { mut marker } in found {
        let mut uuid = Kiid::new(&mut generator);

        while board.resolve_item(&uuid).is_some() {
            uuid = Kiid::new(&mut generator);
        }

        marker.base.uuid = uuid;
        board.add_marker(marker);
    }

    // Update the exclusion status on any excluded markers that still exist.
    resolve_drc_exclusions(board, false);

    let ctx = BoardCtx::bare(board).with_rules(engine);
    let mut markers_provider = DrcItemsProvider::new(MarkerT::Drc, MarkerT::DrawingSheet);
    let mut ratsnest_provider = DrcItemsProvider::new(MarkerT::Ratsnest, MarkerT::Unspec);
    let mut fp_warnings_provider = DrcItemsProvider::new(MarkerT::Parity, MarkerT::Unspec);

    markers_provider.set_severities(&ctx, options.severities);
    ratsnest_provider.set_severities(&ctx, options.severities);
    fp_warnings_provider.set_severities(&ctx, options.severities);

    let report_writer = DrcReport::new(
        &ctx,
        options.run.units,
        &markers_provider,
        &ratsnest_provider,
        &fp_warnings_provider,
    );

    let report = match options.format {
        DrcReportFormat::Json => with_checks_not_run_json(
            report_writer
                .json_report(&options.date, &options.kicad_version)
                .to_json(),
            &outcome.checks_not_run,
        ),
        DrcReportFormat::Text => with_checks_not_run_text(
            report_writer.write_text_report(&options.date),
            &outcome.checks_not_run,
        ),
    };

    DrcJobResult {
        violations: markers_provider.count(),
        unconnected_items: ratsnest_provider.count(),
        parity_issues: fp_warnings_provider.count(),
        report,
        cancelled: outcome.cancelled,
        checks_not_run: outcome.checks_not_run,
    }
}

/// The heading of the not-run list in the text report.
pub const CHECKS_NOT_RUN_HEADING: &str = "** Checks not run (not ported yet) **";

/// The JSON report key of the not-run list.
pub const CHECKS_NOT_RUN_KEY: &str = "checks_not_run";

/// The text report with the [`CheckNotRun`] list as a last section before
/// `** End of Report **` (unchanged when the list is empty):
///
/// ```text
/// ** Checks not run (not ported yet) **
///     - creepage: not ported yet (DRC-33)
///
/// ** End of Report **
/// ```
pub fn with_checks_not_run_text(report: String, checks: &[CheckNotRun]) -> String {
    if checks.is_empty() {
        return report;
    }

    let end = "\n** End of Report **\n";
    let mut section = format!("\n{CHECKS_NOT_RUN_HEADING}\n");

    for c in checks {
        section += &format!("    - {}: {}\n", c.provider, c.reason);
    }

    match report.rfind(end) {
        Some(at) => format!("{}{section}{}", &report[..at], &report[at..]),
        None => report + &section,
    }
}

/// The JSON report (`WriteJsonReport` layout) with the [`CheckNotRun`] list under
/// [`CHECKS_NOT_RUN_KEY`] (`[{"provider": ..., "reason": ...}]`); without the key when the
/// list is empty, i.e. byte for byte the C++ report then.
pub fn with_checks_not_run_json(mut json: Value, checks: &[CheckNotRun]) -> String {
    if !checks.is_empty()
        && let Some(object) = json.as_object_mut()
    {
        let list = checks
            .iter()
            .map(|c| {
                let mut entry = kicad_core::json::Map::new();
                entry.insert("provider".to_string(), Value::from(c.provider));
                entry.insert("reason".to_string(), Value::from(c.reason.clone()));
                Value::Object(entry)
            })
            .collect();
        object.insert(CHECKS_NOT_RUN_KEY.to_string(), Value::Array(list));
    }

    dump_stream(&json, 4, ObjectOrder::Sorted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unported_providers_are_registry_slots_in_order() {
        let mut last = 0;

        for p in &UNPORTED_PROVIDERS {
            let at = PROVIDER_ORDER
                .iter()
                .position(|n| *n == p.name)
                .expect("registry slot");
            assert!(at >= last);
            last = at;
        }
    }

    #[test]
    fn not_run_sections() {
        let checks = vec![CheckNotRun {
            provider: "creepage",
            reason: "not ported yet (DRC-33)".to_string(),
        }];
        let text = "** Ignored checks **\n    - x\n\n** End of Report **\n".to_string();
        assert_eq!(with_checks_not_run_text(text.clone(), &[]), text);
        assert_eq!(
            with_checks_not_run_text(text, &checks),
            "** Ignored checks **\n    - x\n\n** Checks not run (not ported yet) **\n    - \
             creepage: not ported yet (DRC-33)\n\n** End of Report **\n"
        );

        let mut map = kicad_core::json::Map::new();
        map.insert("a".to_string(), Value::from(1));
        let json = Value::Object(map);
        assert_eq!(
            with_checks_not_run_json(json.clone(), &[]),
            dump_stream(&json, 4, ObjectOrder::Sorted)
        );
        assert!(with_checks_not_run_json(json, &checks).contains("\"checks_not_run\""));
    }
}
