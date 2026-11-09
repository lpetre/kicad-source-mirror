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
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/geometry/distribute.h` and
//! `libs/kimath/src/geometry/distribute.cpp`.
//!
//! The `int` arithmetic of the C++ wraps on overflow like the reference build.

use crate::math::util::ki_round;

/// Port of `GetDeltasForDistributeByGaps`.
///
/// Given a list of 'n' item spans (e.g. left-right edge positions), return the deltas for
/// each item to produce even gaps between each item.  The first and last items will not be
/// moved.  `a_item_extents` should be sorted in a meaningful way to the caller, for example,
/// left-right based on the item's left edge or by anchor.  Returns a vector of n deltas, the
/// first and last will be 0.
pub fn get_deltas_for_distribute_by_gaps(a_item_extents: &[(i32, i32)]) -> Vec<i32> {
    let mut deltas: Vec<i32> = vec![0; a_item_extents.len()];

    // This only makes sense for 3 or more items
    if a_item_extents.len() < 3 {
        return deltas;
    }

    let n = a_item_extents.len();

    // The space between the first and last items' inner edges
    let total_space: i32 = a_item_extents[n - 1].0.wrapping_sub(a_item_extents[0].1);
    let mut total_gap: i32 = total_space;

    for &(start, end) in &a_item_extents[1..n - 1] {
        total_gap = total_gap.wrapping_sub(end.wrapping_sub(start));
    }

    let per_item_gap: f64 = total_gap as f64 / (n - 1) as f64;

    // Start counting at the end of the first item
    let mut target_pos: i32 = a_item_extents[0].1;

    // End-cap items don't need to be changed
    for i in 1..n - 1 {
        let (start, end) = a_item_extents[i];

        // Take care not to stack rounding errors by keeping the integer accumulator
        // separate and always re-multiplying the gap
        let accumulated_gaps: f64 = i as f64 * per_item_gap;
        let delta: i32 = target_pos
            .wrapping_sub(start)
            .wrapping_add(ki_round(accumulated_gaps));
        deltas[i] = delta;

        // Step over one item span (width or height)
        target_pos = target_pos.wrapping_add(end.wrapping_sub(start));
    }

    deltas
}

/// Port of `GetDeltasForDistributeByPoints`: the deltas that space the positions evenly
/// between the first and the last one (which are not moved).
pub fn get_deltas_for_distribute_by_points(a_item_positions: &[i32]) -> Vec<i32> {
    let mut deltas: Vec<i32> = vec![0; a_item_positions.len()];

    // This only makes sense for 3 or more items
    if a_item_positions.len() < 3 {
        return deltas;
    }

    let n = a_item_positions.len();
    let start_pos: i32 = a_item_positions[0];
    let total_gaps: i32 = a_item_positions[n - 1].wrapping_sub(start_pos);
    let item_gap: f64 = total_gaps as f64 / (n - 1) as f64;

    // End-cap items don't need to be changed
    for i in 1..n - 1 {
        let target_pos: i32 = start_pos.wrapping_add(ki_round(i as f64 * item_gap));
        let delta: i32 = target_pos.wrapping_sub(a_item_positions[i]);
        deltas[i] = delta;
    }

    deltas
}
