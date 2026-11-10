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

//! Port of `pcbnew/drc/drc_engine.cpp` (`MatchDpSuffix`, `IsNetADiffPair`,
//! `IsNetTieExclusion`).

use kicad_core::{Flashing, PcbLayerId};
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::netinfo::NetinfoItem;
use kicad_pcb::{Board, BoardCtx, ItemId, ItemRef, NetCode};

/// Port of `DRC_ENGINE::MatchDpSuffix`: check if the given net name is a diff pair.
///
/// Returns `(polarity, complement net, base name)`: polarity 1 if `net_name` is the positive
/// half of a pair (`DIFF_P`, `DIFF+`), -1 if negative, 0 if it is not a diff pair (then both
/// strings are empty; the C++ leaves its out-parameters untouched). Trailing digits and
/// underscores after the polarity character are kept: `D+12` -> (`1`, `D-12`, `D`).
pub fn match_dp_suffix(net_name: &str) -> (i32, String, String) {
    let mut rv = 0;
    let mut count = 0usize;
    let mut complement = String::new();
    let chars: Vec<char> = net_name.chars().collect();

    // for( auto it = aNetName.rbegin(); it != aNetName.rend() && rv == 0; ++it, ++count )
    for &ch in chars.iter().rev() {
        if ch.is_ascii_digit() || ch == '_' {
            count += 1;
            continue;
        } else if ch == '+' {
            complement = "-".to_string();
            rv = 1;
        } else if ch == '-' {
            complement = "+".to_string();
            rv = -1;
        } else if ch == 'N' {
            complement = "P".to_string();
            rv = -1;
        } else if ch == 'P' {
            complement = "N".to_string();
            rv = 1;
        } else {
            break;
        }

        // the loop increment still runs after the polarity character, then `rv == 0` ends it
        count += 1;
        break;
    }

    if rv != 0 && count >= 1 {
        let base: String = chars[..chars.len() - count].iter().collect();
        let right: String = chars[chars.len() - (count - 1)..].iter().collect();
        let complement_net = format!("{base}{complement}{right}");
        return (rv, complement_net, base);
    }

    (rv, String::new(), String::new())
}

/// Port of `DRC_ENGINE::IsNetADiffPair`: `Some( (positive net, negative net) )` if `net` and
/// its complement (by [`match_dp_suffix`]) both exist on the board.
pub fn is_net_a_diff_pair(board: &Board, net: &NetinfoItem) -> Option<(NetCode, NetCode)> {
    let (polarity, coupled_net_name, _) = match_dp_suffix(net.netname());

    if polarity != 0 {
        let coupled = board.find_net_by_name(&coupled_net_name)?;

        if polarity > 0 {
            return Some((net.net_code(), coupled.net_code()));
        } else {
            return Some((coupled.net_code(), net.net_code()));
        }
    }

    None
}

/// Port of `DRC_ENGINE::IsNetTieExclusion`: check if the given collision between a track and
/// another item occurs during the track's entry into a net-tie pad.
pub fn is_net_tie_exclusion(
    ctx: &BoardCtx<'_>,
    track_net_code: NetCode,
    track_layer: PcbLayerId,
    collision_pos: Vector2I,
    colliding_item: ItemRef<'_>,
) -> bool {
    let Some(parent_fp_ref) = colliding_item.parent_footprint() else {
        return false;
    };
    let Some(parent_footprint) = parent_fp_ref.as_footprint() else {
        return false;
    };
    let Some(fp_id) = parent_fp_ref.id().and_then(|id| id.typed()) else {
        return false;
    };

    if parent_footprint.is_net_tie() {
        let epsilon = ctx.board.design_settings().get_drc_epsilon();
        let pad_to_net_tie_group_map = parent_footprint.map_pad_numbers_to_net_tie_groups(ctx);

        for (pad_id, pad) in ctx.board.footprint_pads(fp_id) {
            // std::map::operator[] inserts 0 for a number that is not in the map
            let group = pad_to_net_tie_group_map
                .get(pad.number())
                .copied()
                .unwrap_or(0);

            let Some(pad_ref) = ctx.item(ItemId::from(pad_id)) else {
                continue;
            };

            if group >= 0 && Some(track_net_code) == pad_ref.net_code() {
                let shape = pad_ref.effective_shape(
                    track_layer,
                    Flashing::Default,
                    DrcConstraintT::NullConstraint,
                );

                if shape.collide_point(collision_pos, epsilon, None, None) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suffix table of `MatchDpSuffix` (analysis `drc.md`, DRC-03 unit tests).
    #[test]
    fn match_dp_suffix_table() {
        let cases: &[(&str, i32, &str, &str)] = &[
            ("DIFF_P", 1, "DIFF_N", "DIFF_"),
            ("DIFF_N", -1, "DIFF_P", "DIFF_"),
            ("USB+", 1, "USB-", "USB"),
            ("USB-", -1, "USB+", "USB"),
            ("D+12", 1, "D-12", "D"),
            ("D-1_2", -1, "D+1_2", "D"),
            ("CLKP0", 1, "CLKN0", "CLK"),
            ("/a/RXN", -1, "/a/RXP", "/a/RX"),
            ("P", 1, "N", ""),
            ("GND", 0, "", ""),
            ("NET_1", 0, "", ""),
            ("123", 0, "", ""),
            ("", 0, "", ""),
            ("Np", 0, "", ""),
            ("ÄP", 1, "ÄN", "Ä"),
        ];

        for &(name, polarity, complement, base) in cases {
            assert_eq!(
                match_dp_suffix(name),
                (polarity, complement.to_string(), base.to_string()),
                "{name}"
            );
        }
    }
}
