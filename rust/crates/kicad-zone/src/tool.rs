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

//! Port of the headless parts of `pcbnew/tools/zone_filler_tool.cpp`
//! (`ZONE_FILLER_TOOL::FillAllZones( nullptr, reporter, true )`, `PostFillRefresh`,
//! `rebuildConnectivity`): what `kicad-cli pcb drc --refill-zones` and the plot jobs'
//! `--check-zones` run (`pcbnew/pcbnew_jobs_handler.cpp`).
//!
//! The teardrop regeneration that precedes the fill (`TEARDROP_MANAGER::UpdateTeardrops`) is
//! [`update_teardrops`] (crate::teardrop).

use kicad_connectivity::data::ConnectivityData;
use kicad_pcb::Board;
use kicad_pcb::ctx::RuleResolver;
use kicad_pcb::ids::ZoneId;

use crate::filler::ZoneFiller;
use crate::teardrop::TeardropManager;

/// What [`fill_all_zones`] reports.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FillAllZonesOutcome {
    /// The return value of `FillAllZones`: `ZONE_FILLER::Fill` succeeded.
    pub filled: bool,
    /// The `wxLogWarning` texts of the filler (non-convergence of the iterative refill).
    pub warnings: Vec<String>,
}

/// Port of `ZONE_FILLER_TOOL::FillAllZones( nullptr, aReporter, true /* aHeadless */ )`
/// followed by its `PostFillRefresh( true )` (`BOARD::BuildConnectivity`).
///
/// `rules` is `BOARD_DESIGN_SETTINGS::m_DRCEngine` (the caller creates and initialises one
/// when the board has none, as `ZONE_FILLER::Fill` does).
///
/// Headless differences that the C++ itself makes: no edit frame, no infobar about invalid
/// DRC rules, no progress dialog. The tool manager of the jobs handler registers no
/// `GENERATOR_TOOL`, so `RegenerateDirtyGenerators` regenerates nothing and
/// `refillAroundGenerators` has nothing to refill.
pub fn fill_all_zones(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    rules: &dyn RuleResolver,
) -> FillAllZonesOutcome {
    // if( m_fillInProgress ) return false: the port is not re-entrant by construction.

    // TEARDROP_MANAGER teardropMgr( board(), m_toolMgr );
    // teardropMgr.UpdateTeardrops( commit, nullptr, nullptr, true /* forceFullUpdate */ );
    update_teardrops(board, connectivity, rules, true);

    board.increment_time_stamp(); // Clear caches

    let to_fill: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    let mut filler = ZoneFiller::new(board);
    let mut outcome = FillAllZonesOutcome::default();

    if filler.fill(board, connectivity, rules, &to_fill) {
        // First pass on allowing generators like via-stitch to update:
        // board()->OnZonesFilled( toFill ) only calls PCB_GENERATOR::OnZoneFillChanged, which
        // marks a via-stitch generator dirty (in-memory state, not saved); with no
        // GENERATOR_TOOL in the headless tool manager RegenerateDirtyGenerators returns an
        // empty list, so refillAroundGenerators( regenerated ) returns at once.

        // commit.Push( _( "Fill Zone(s)" ), SKIP_CONNECTIVITY | ZONE_FILL_OP ): the fills are
        // derived state the filler has already written into the board (board-model.md 8.4).
        outcome.filled = true;
    } else {
        // commit.Revert(): unreachable in the port, `ZoneFiller::fill` fails only on a
        // cancelled progress reporter, which the port does not have.
        debug_assert!(
            false,
            "ZoneFiller::fill has no failure path without a reporter"
        );
    }

    outcome.warnings = filler.warnings().to_vec();

    // PostFillRefresh( aHeadless ) -> rebuildConnectivity( aHeadless ):
    // board()->BuildConnectivity(); the ConnectivityChangedEvent has no listener headless.
    connectivity.build_connectivity(board);

    outcome
}

/// `TEARDROP_MANAGER teardropMgr( board(), m_toolMgr );`
/// `teardropMgr.UpdateTeardrops( commit, nullptr, nullptr, aForceFullUpdate )` (TD-01).
///
/// `rules` is the board's DRC engine (`bds.m_DRCEngine`), which the teardrop width fit asks
/// for clearances.
pub fn update_teardrops(
    board: &mut Board,
    connectivity: &mut ConnectivityData,
    rules: &dyn RuleResolver,
    force_full_update: bool,
) {
    let mut manager = TeardropManager::new();
    manager.update_teardrops(
        board,
        connectivity,
        Some(rules),
        None,
        None,
        force_full_update,
    );
}
