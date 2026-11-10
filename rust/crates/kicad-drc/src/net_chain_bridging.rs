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

//! Port of `pcbnew/net_chain_bridging.cpp` / `.h`: the length and delay that series passives
//! add to a net chain (the span between their cross-net pads).
//!
//! `PartitionNetChainAroundNet` (used by the router's tuner and the net inspector, not by DRC)
//! is not ported.

use kicad_core::base_units::PCB_IU_SCALE;
use kicad_geom::math::vector2::Vector2D;
use kicad_pcb::ids::{FootprintId, PadId};
use kicad_pcb::project::tuning_profiles_json::load_tuning_profiles;
use kicad_pcb::{BoardCtx, NetCode};

/// Port of `CHAIN_BRIDGE`: one bridge edge between two cross-net pads of a footprint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChainBridge {
    /// `padA`
    pub pad_a: PadId,
    /// `padB`
    pub pad_b: PadId,
    /// `length`
    pub length: f64,
    /// `delay`
    pub delay: f64,
}

/// The pads of footprint `fp` whose net belongs to chain `net_chain`, in `Pads()` order.
fn chain_pads(ctx: &BoardCtx<'_>, fp: FootprintId, net_chain: &str) -> Vec<PadId> {
    let board = ctx.board;

    board
        .footprint_pads(fp)
        .filter(|&(id, _)| {
            ctx.item(id.item_id())
                .and_then(|p| p.net())
                .is_some_and(|n| n.net_chain() == net_chain)
        })
        .map(|(id, _)| id)
        .collect()
}

fn pad_net(ctx: &BoardCtx<'_>, pad: PadId) -> Option<NetCode> {
    ctx.item(pad.item_id()).and_then(|p| p.net_code())
}

fn pad_center(ctx: &BoardCtx<'_>, pad: PadId) -> Vector2D {
    let pad_ref = ctx.item(pad.item_id()).expect("a board pad");
    Vector2D::from(pad_ref.as_pad().expect("a pad").center(ctx))
}

/// Port of `FootprintChainBridgingLength`: the largest centre-to-centre span between two pads
/// of different nets of the chain on the footprint (0 when no pad pair crosses nets).
pub fn footprint_chain_bridging_length(ctx: &BoardCtx<'_>, fp: FootprintId, net_chain: &str) -> f64 {
    if net_chain.is_empty() {
        return 0.0;
    }

    let chain_pads = chain_pads(ctx, fp, net_chain);

    if chain_pads.len() < 2 {
        return 0.0;
    }

    let first_net = pad_net(ctx, chain_pads[0]);

    if !chain_pads[1..]
        .iter()
        .any(|&p| pad_net(ctx, p) != first_net)
    {
        return 0.0;
    }

    let mut max_span: f64 = 0.0;

    for i in 0..chain_pads.len() {
        for j in i + 1..chain_pads.len() {
            if pad_net(ctx, chain_pads[i]) == pad_net(ctx, chain_pads[j]) {
                continue;
            }

            let delta = pad_center(ctx, chain_pads[i]) - pad_center(ctx, chain_pads[j]);
            max_span = max_span.max(delta.euclidean_norm());
        }
    }

    max_span
}

/// Port of `BoardChainBridgingLength`: the sum of [`footprint_chain_bridging_length`] over the
/// footprints of the board.
pub fn board_chain_bridging_length(ctx: &BoardCtx<'_>, net_chain: &str) -> f64 {
    if net_chain.is_empty() {
        return 0.0;
    }

    let mut total = 0.0;

    for (fp, _) in ctx.board.footprints() {
        total += footprint_chain_bridging_length(ctx, fp, net_chain);
    }

    total
}

/// Port of `ChainBridgingDelayPerMm`: the bridge propagation delay per mm of the tuning profile
/// of the first track of the chain whose net class has one, else the default.
pub fn chain_bridging_delay_per_mm(ctx: &BoardCtx<'_>, net_chain: &str, default_bridge_unit_delay: f64) -> f64 {
    if net_chain.is_empty() {
        return default_bridge_unit_delay;
    }

    let board = ctx.board;

    for (track, _) in board.tracks() {
        let Some(ninfo) = ctx.item(track.item_id()).and_then(|t| t.net()) else {
            continue;
        };

        if ninfo.net_chain() != net_chain {
            continue;
        }

        let netclass = match ninfo.netclass() {
            Some(nc) => nc.clone(),
            None => board.design_settings().net_settings.default_netclass().clone(),
        };

        if !netclass.has_tuning_profile() {
            continue;
        }

        let tuning_profile_name = netclass.tuning_profile();

        // aBoard->GetProject()->GetProjectFile().TuningProfileParameters(): GetTuningProfile
        // answers its null profile for an unknown name (bridge delay 0 in the port; the C++
        // member is not initialised).
        let (params, _) =
            load_tuning_profiles(board.project.as_ref().map(|p| &p.document));

        return params
            .tuning_profiles
            .iter()
            .find(|p| p.profile_name == tuning_profile_name)
            .map_or(0.0, |p| f64::from(p.net_chain_bridge_propagation_delay));
    }

    default_bridge_unit_delay
}

/// Port of `BoardChainBridging`: (length, delay) added by the series passives of the chain.
pub fn board_chain_bridging(ctx: &BoardCtx<'_>, net_chain: &str, default_bridge_unit_delay: f64) -> (f64, f64) {
    if net_chain.is_empty() {
        return (0.0, 0.0);
    }

    let length_iu = board_chain_bridging_length(ctx, net_chain);

    if length_iu <= 0.0 {
        return (0.0, 0.0);
    }

    let delay_iu = chain_bridging_delay_per_mm(ctx, net_chain, default_bridge_unit_delay) * length_iu
        / PCB_IU_SCALE.iu_per_mm;

    (length_iu, delay_iu)
}

/// Port of `EnumerateChainBridges`: one bridge per cross-net pad pair of the chain on every
/// footprint (footprint order, then pad pair order).
pub fn enumerate_chain_bridges(
    ctx: &BoardCtx<'_>,
    net_chain: &str,
    default_bridge_unit_delay: f64,
) -> Vec<ChainBridge> {
    let mut bridges = Vec::new();

    if net_chain.is_empty() {
        return bridges;
    }

    let delay_iu_per_mm = chain_bridging_delay_per_mm(ctx, net_chain, default_bridge_unit_delay);

    for (fp, _) in ctx.board.footprints() {
        let chain_pads = chain_pads(ctx, fp, net_chain);

        if chain_pads.len() < 2 {
            continue;
        }

        for i in 0..chain_pads.len() {
            for j in i + 1..chain_pads.len() {
                if pad_net(ctx, chain_pads[i]) == pad_net(ctx, chain_pads[j]) {
                    continue;
                }

                let delta = pad_center(ctx, chain_pads[i]) - pad_center(ctx, chain_pads[j]);
                let length = delta.euclidean_norm();
                let delay = delay_iu_per_mm * length / PCB_IU_SCALE.iu_per_mm;

                bridges.push(ChainBridge {
                    pad_a: chain_pads[i],
                    pad_b: chain_pads[j],
                    length,
                    delay,
                });
            }
        }
    }

    bridges
}
