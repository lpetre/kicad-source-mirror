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
//! The teardrop regeneration that precedes the fill (`TEARDROP_MANAGER::UpdateTeardrops`,
//! packages TD-01 / TD-02) is not ported yet: [`update_teardrops`] returns at once where the
//! C++ has nothing to do (legacy teardrops, or no teardrop zone and no teardrop-enabled
//! pad / via / track target) and stops with `port_pending!` otherwise.

use kicad_connectivity::data::ConnectivityData;
use kicad_pcb::ctx::RuleResolver;
use kicad_pcb::pcb_track::Track;
use kicad_pcb::teardrop_parameters::TargetTd;
use kicad_pcb::Board;
use kicad_pcb::ids::ZoneId;

use crate::filler::ZoneFiller;

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
    update_teardrops(board, connectivity, true);

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
        debug_assert!(false, "ZoneFiller::fill has no failure path without a reporter");
    }

    outcome.warnings = filler.warnings().to_vec();

    // PostFillRefresh( aHeadless ) -> rebuildConnectivity( aHeadless ):
    // board()->BuildConnectivity(); the ConnectivityChangedEvent has no listener headless.
    connectivity.build_connectivity(board);

    outcome
}

/// `TEARDROP_MANAGER::UpdateTeardrops( aCommit, nullptr, nullptr, aForceFullUpdate )` (TD-01).
///
/// Ported so far: the early return of a board with legacy teardrops, and the cases in which
/// the C++ does nothing at all - no teardrop zone to delete (the forced full update deletes
/// every one before rebuilding), no pad or via with enabled teardrop parameters, and the
/// track-to-track target disabled (`m_prmsList->GetParameters( TARGET_TRACK )->m_Enabled`);
/// `setTeardropPriorities` then has no teardrop zone to touch. Every other board stops in
/// `port_pending!` rather than losing its teardrops silently (connectivity-zones.md 10.2,
/// interim guard for ZF-06).
pub fn update_teardrops(board: &Board, connectivity: &ConnectivityData, force_full_update: bool) {
    if board.legacy_teardrops {
        return;
    }

    let has_teardrop_zone = board.zones().any(|(_, zone)| zone.is_teardrop_area());

    let pad_or_via_enabled = board
        .footprints()
        .flat_map(|(fp, _)| board.footprint_pads(fp))
        .any(|(_, pad)| pad.base.teardrop_params.enabled)
        || board.tracks().any(|(_, track)| match track {
            Track::Via(via) => via.track.base.teardrop_params.enabled,
            _ => false,
        });

    let track_target_enabled = board
        .design_settings()
        .teardrop_params_list()
        .parameters(TargetTd::Track)
        .enabled;

    if !has_teardrop_zone && !pad_or_via_enabled && !track_target_enabled {
        return;
    }

    crate::port_pending!(
        "TEARDROP_MANAGER::UpdateTeardrops (TD-01 / TD-02)",
        connectivity,
        force_full_update
    );
}
