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

use std::sync::{Arc, OnceLock};

use kicad_connectivity::data::ConnectivityData;
use kicad_core::eda_units::EdaUnits;
use kicad_core::json::{ObjectOrder, Value, dump_stream};
use kicad_core::{KicadT, Kiid, PcbLayerId, Severity};
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::board::index::UuidPresence;
use kicad_pcb::drc_api::{
    DRCE_DUPLICATE_FOOTPRINT, DRCE_EXTRA_FOOTPRINT, DRCE_FOOTPRINT_FILTERS, DRCE_MISSING_FOOTPRINT,
    DRCE_NET_CONFLICT, DRCE_SCHEMATIC_FIELDS_PARITY, DRCE_SCHEMATIC_PARITY,
};
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker, RcItem};
use kicad_pcb::{Board, BoardCtx, ConnectivityQuery, ItemId, ItemRef, NetCode};

use crate::cache_generator::{CacheGeneratorConnectivity, DrcCacheGenerator};
use crate::caches::{DrcBoardCaches, ZoneIslandsMap};
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
use crate::providers::creepage::DrcTestProviderCreepage;
use crate::providers::diff_pair_coupling::DrcTestProviderDiffPairCoupling;
use crate::providers::disallow::DrcTestProviderDisallow;
use crate::providers::edge_clearance::DrcTestProviderEdgeClearance;
use crate::providers::footprint_checks::DrcTestProviderFootprintChecks;
use crate::providers::hole_size::DrcTestProviderHoleSize;
use crate::providers::hole_to_hole::DrcTestProviderHoleToHole;
use crate::providers::library_parity::DrcTestProviderLibraryParity;
use crate::providers::matched_length::DrcTestProviderMatchedLength;
use crate::providers::misc::DrcTestProviderMisc;
use crate::providers::net_chain_tuning_profiles::DrcTestProviderNetChainTuningProfiles;
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
use crate::providers::via_stitch::DrcTestProviderViaStitch;
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
pub const UNPORTED_PROVIDERS: [UnportedProvider; 1] = [UnportedProvider {
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
}];

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
    /// `PROJECT_PCB::FootprintLibAdapter( project )` after `AsyncLoad()` +
    /// `BlockUntilLoaded()` (library parity provider); `None` behaves as no library tables.
    pub footprint_libraries: Option<&'a kicad_pcb::libraries::FootprintLibraryAdapter>,
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
    _engine: &DrcEngine,
    run: &DrcRunState<'_>,
    _board: &Board,
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
        // if( m_drcEngine->GetTestFootprints() ) { ... }
        "schematic_parity" => {
            if !run.test_footprints() {
                return None;
            }

            Some("not ported (schematic parity is out of scope)".to_string())
        }
        _ => not_ported(),
    }
}

/// The second connectivity of the zone connections provider (see [`run_provider`]), built
/// on its first query instead of before the provider runs. Not a C++ type: C++ has one
/// connectivity; the port needs a second one only because the provider holds the run's
/// connectivity as `&mut` while the context needs a shared [`ConnectivityQuery`].
///
/// Invariant that makes the laziness output-neutral: the value is a function of `board`
/// only, and `board` is an immutable `&Board` for the whole provider loop of
/// [`run_tests_selected`], so building it at the first query gives the same object as
/// building it before the provider; when nothing queries it, nothing observes it (the bare
/// build only warms self-validating item caches, which the cache generator's own bare
/// builds warm on the same board state). If the board ever becomes mutable during the
/// provider loop, this must go back to an eager build.
pub static TMP_LAZY_BUILDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
struct LazyConnectivity<'b> {
    board: &'b Board,
    /// False when the board has no copper zones: the query is the empty `ConnectivityData`.
    build: bool,
    /// `OnceLock` because `ConnectivityQuery: Sync` (providers may query from several threads).
    cell: OnceLock<ConnectivityData>,
}

impl<'b> LazyConnectivity<'b> {
    fn new(board: &'b Board, build: bool) -> Self {
        LazyConnectivity {
            board,
            build,
            cell: OnceLock::new(),
        }
    }

    /// The connectivity, built on the first call exactly as the eager code built it.
    fn get(&self) -> &ConnectivityData {
        self.cell.get_or_init(|| {
            let mut query = ConnectivityData::new();

            if self.build {
                TMP_LAZY_BUILDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                query.build(&BoardCtx::bare(self.board));
                // The same from-to cache as the run's connectivity (rebuilt in run_tests).
                query.rebuild_from_to_cache(&BoardCtx::bare(self.board));
            }

            query
        })
    }
}

/// Forwards every method (`is_on_from_to_path` included: the trait default would answer
/// `None`) to the built connectivity.
impl ConnectivityQuery for LazyConnectivity<'_> {
    fn is_connected_on_layer(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        layer: PcbLayerId,
        types: &[KicadT],
    ) -> bool {
        ConnectivityQuery::is_connected_on_layer(self.get(), ctx, item, layer, types)
    }

    fn node_count(&self, net: Option<NetCode>) -> u32 {
        ConnectivityQuery::node_count(self.get(), net)
    }

    fn pad_count(&self, net: Option<NetCode>) -> u32 {
        ConnectivityQuery::pad_count(self.get(), net)
    }

    fn net_items(&self, net: NetCode, types: &[KicadT]) -> Vec<ItemId> {
        ConnectivityQuery::net_items(self.get(), net, types)
    }

    fn is_on_from_to_path(
        &self,
        ctx: &BoardCtx<'_>,
        item: ItemRef<'_>,
        from: &str,
        to: &str,
    ) -> Option<bool> {
        ConnectivityQuery::is_on_from_to_path(self.get(), ctx, item, from, to)
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
    // it: the connectivity provider puts the same object into the context it evaluates its
    // rule with (`check_tracks`: `VIA_DANGLING_CONSTRAINT`, fromTo() conditions see the
    // from-to cache rebuilt by `run_tests`); for the zone connections provider the item
    // shapes and the rules see a second build of the same connectivity.
    match name {
        "connectivity" => {
            let ctx = BoardCtx::bare(board).with_rules(engine);
            return DrcTestProviderConnectivity::new(conn, caches)
                .run_tests(engine, run, &ctx, units);
        }
        "diff_pair_coupling" => {
            // Run() starts with m_board->GetConnectivity()->GetFromToCache()->Rebuild( m_board ),
            // which needs the connectivity mutably.
            conn.rebuild_from_to_cache(&BoardCtx::bare(board));
            let ctx = BoardCtx::bare(board)
                .with_rules(engine)
                .with_connectivity(&*conn);
            return DrcTestProviderDiffPairCoupling::new(caches)
                .run_tests(engine, run, &ctx, units);
        }
        "length" => {
            // runInternal() rebuilds the from-to cache (GetFromToCache()->Rebuild( m_board )),
            // which needs the connectivity mutably.
            conn.rebuild_from_to_cache(&BoardCtx::bare(board));
            let ctx = BoardCtx::bare(board)
                .with_rules(engine)
                .with_connectivity(&*conn);
            return DrcTestProviderMatchedLength::new(conn).run_tests(engine, run, &ctx, units);
        }
        "zone connections" => {
            // The second connectivity is built at its first query only (most boards never
            // query it): see LazyConnectivity.
            let query = LazyConnectivity::new(board, !caches.copper_zones.is_empty());

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
        "creepage" => Box::new(DrcTestProviderCreepage::default()),
        "disallow" => Box::new(DrcTestProviderDisallow::new(caches)),
        "copper width" => Box::new(DrcTestProviderConnectionWidth::new(caches)),
        "clearance" => Box::new(DrcTestProviderCopperClearance::new(caches)),
        "physical_clearance" => Box::new(DrcTestProviderPhysicalClearance::new(caches)),
        "courtyard_clearance" => Box::new(DrcTestProviderCourtyardClearance::default()),
        "edge_clearance" => Box::new(DrcTestProviderEdgeClearance::default()),
        "footprint checks" => Box::new(DrcTestProviderFootprintChecks::default()),
        "hole_to_hole_clearance" => Box::new(DrcTestProviderHoleToHole::default()),
        "hole_size" => Box::new(DrcTestProviderHoleSize::default()),
        "miscellaneous" => {
            let mut misc = DrcTestProviderMisc::new(options.tuning_profiles.clone());
            // m_drcEngine->GetDrawingSheet()
            misc.drawing_sheet = engine.drawing_sheet().cloned();
            Box::new(misc)
        }
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
        "library_parity" => Box::new(DrcTestProviderLibraryParity::new(
            options.footprint_libraries,
        )),
        "net chain tuning profiles" => Box::new(DrcTestProviderNetChainTuningProfiles::default()),
        "via_stitch_overlap" => Box::new(DrcTestProviderViaStitch::default()),
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
/// footprint. The from-to cache is rebuilt right after the generator
/// (`connectivity->GetFromToCache()->Rebuild( m_board )`), so `fromTo()` conditions work in
/// every provider that evaluates rules with the run's connectivity.
pub fn run_tests(
    engine: &DrcEngine,
    run: &mut DrcRunState<'_>,
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    options: &RunTestsOptions<'_>,
) -> RunTestsOutcome {
    run_tests_selected(engine, run, board, connectivity, options, &|_| true)
}

/// [`run_tests`] with only the registry slots whose `GetName()` `selected` accepts (not a C++
/// API: the provider tests run their providers through the real run path, cache generator,
/// connectivity build and expression cache scope included, like
/// `qa/tests/pcbnew/drc` does by disabling the other providers with `SetEnabled( false )`).
/// Slots not selected are skipped without a [`CheckNotRun`].
pub fn run_tests_selected(
    engine: &DrcEngine,
    run: &mut DrcRunState<'_>,
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    options: &RunTestsOptions<'_>,
    selected: &dyn Fn(&str) -> bool,
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

    // Recompute component classes: not needed in the port (see the function comment).

    // fromTo() conditions are evaluated by every provider, not only the length ones
    connectivity.rebuild_from_to_cache(&BoardCtx::bare(board));

    let board: &Board = board;
    let timestamp = board.time_stamp();
    let _expression_caches = PcbExprCacheScope::enter(board);

    for name in PROVIDER_ORDER {
        if !selected(name) {
            continue;
        }

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
    /// Not used any more: the marker UUIDs (random KIIDs in C++) are drawn from the board's
    /// generator ([`run_drc_job`]), which the host seeds. Kept for the callers' API.
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
/// Marker UUIDs: the C++ `PCB_MARKER` gets a random KIID. Here they come from the board's
/// generator (`Board::kiid_generator`, reserved file UUIDs and the board's own skipped), and a
/// UUID that names an item of the board is drawn again, so a marker can never take over the
/// resolution of a report item.
pub fn run_drc_job(
    engine: &DrcEngine,
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    options: &DrcJobOptions<'_>,
) -> DrcJobResult {
    let mut found: Vec<Found> = Vec::new();
    // The handler only builds the marker (the board is borrowed by the run); its UUID is
    // drawn from the board's generator below.
    let mut scratch_generator = kicad_core::KiidGenerator::default();
    scratch_generator.create_nil_uuids(true);

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

    // commit.Push(): the markers are appended to the board in report order. The UUIDs come
    // from the board's generator (review R2-11), which never returns the UUIDs of the loaded
    // file nor the board's own; the loop only guards against an item UUID a host assigned by
    // hand.
    //
    // `presence.resolves( board, uuid )` is `board.resolve_item( &uuid ).is_some()` without a
    // linear scan of the board per marker (a fresh UUID always misses the index). It stays
    // exact because the loop only adds markers, each noted right after `add_marker`.
    let mut presence = UuidPresence::new(board);

    for Found { mut marker } in found {
        let mut uuid = Kiid::new(&mut board.kiid_generator);

        while presence.resolves(board, &uuid) || uuid == board.uuid() {
            uuid = Kiid::new(&mut board.kiid_generator);
        }

        marker.base.uuid = uuid;
        board.add_marker(marker);
        presence.note_added(uuid);
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
