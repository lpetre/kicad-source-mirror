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

//! Port of `libs/kimath/include/geometry/fracture_edge_index_utils.h`
//! (`KIGEOM::FRACTURE_INDEX`): sizing helpers of the Y-stripe edge index used by
//! `SHAPE_POLY_SET::Fracture()`.

/// `MAX_STRIPES`
pub const MAX_STRIPES: u32 = 65536;
/// `MAX_BUCKET_SPAN`
pub const MAX_BUCKET_SPAN: u32 = 8;

/// `EDGE_SET_BUDGET_MULTIPLE`: the index is at most ~1.6x the edge set it serves, so a budget
/// proportional to the edge set bounds the footprint without dropping the index on the large
/// planes that need it most.
pub const EDGE_SET_BUDGET_MULTIPLE: usize = 2;
/// `MIN_EDGE_VISITS`
pub const MIN_EDGE_VISITS: u64 = 32768;
/// `MIN_HOLE_COUNT`
pub const MIN_HOLE_COUNT: usize = 8;

/// Port of `StripeCountFor()`.
pub fn stripe_count_for(a_edge_count: usize) -> u32 {
    let count = (a_edge_count as f64).sqrt().clamp(1.0, MAX_STRIPES as f64);
    count as u32
}

/// Port of `MapYToStripe()`.
pub fn map_y_to_stripe(a_y: i32, a_min_y: i32, a_max_y: i32, a_stripe_count: u32) -> u32 {
    // An inverted extent would divide by a non-positive range and violate the clamp bounds
    if a_max_y < a_min_y {
        return 0;
    }

    let range: i64 = a_max_y as i64 - a_min_y as i64 + 1;
    let offset: i64 = (a_y as i64 - a_min_y as i64).clamp(0, range - 1);
    let mapped: u64 = (offset as u64).wrapping_mul(a_stripe_count as u64) / range as u64;
    (mapped as u32).min(a_stripe_count - 1)
}

/// Port of `StripeSpan()`.
pub fn stripe_span(
    a_y1: i32,
    a_y2: i32,
    a_min_y: i32,
    a_max_y: i32,
    a_stripe_count: u32,
) -> (u32, u32) {
    (
        map_y_to_stripe(a_y1.min(a_y2), a_min_y, a_max_y, a_stripe_count),
        map_y_to_stripe(a_y1.max(a_y2), a_min_y, a_max_y, a_stripe_count),
    )
}

/// Port of `CheckedAdd()` (`size_t` is 64 bits on the reference platform).
pub fn checked_add(a_total: &mut u64, a_count: u64, a_element_size: u64) -> bool {
    if a_element_size == 0 {
        return true;
    }

    if a_count > (u64::MAX - *a_total) / a_element_size {
        return false;
    }

    *a_total += a_count * a_element_size;
    true
}

/// Port of `CapacityBudget()`.
pub fn capacity_budget(a_edge_count: u64, a_edge_size: u64) -> u64 {
    let mut budget = 0;

    for _ in 0..EDGE_SET_BUDGET_MULTIPLE {
        if !checked_add(&mut budget, a_edge_count, a_edge_size) {
            return u64::MAX;
        }
    }

    budget
}

/// Port of `CapacityFits()`.
pub fn capacity_fits(
    a_bucket_ids: u64,
    a_long_ids: u64,
    a_stripe_count: u64,
    a_hole_count: u64,
    a_node_size: u64,
    a_budget: u64,
) -> bool {
    let mut bytes = 0;
    checked_add(&mut bytes, a_bucket_ids, 4)
        && checked_add(&mut bytes, a_long_ids, 4)
        && checked_add(&mut bytes, a_stripe_count + 1, 4)
        && checked_add(&mut bytes, a_stripe_count, 4)
        && checked_add(&mut bytes, a_stripe_count + 1, 4)
        && checked_add(
            &mut bytes,
            (MAX_BUCKET_SPAN as u64 + 2) * a_hole_count,
            a_node_size,
        )
        && bytes <= a_budget
}

/// Port of `ActualCapacityFits()`; returns the fit and the allocated byte count (`aBytes`).
pub fn actual_capacity_fits(
    a_bucket_ids: u64,
    a_long_ids: u64,
    a_offsets: u64,
    a_scratch: u64,
    a_heads: u64,
    a_nodes: u64,
    a_node_size: u64,
    a_budget: u64,
) -> (bool, u64) {
    let mut bytes = 0;
    let valid = checked_add(&mut bytes, a_bucket_ids, 4)
        && checked_add(&mut bytes, a_long_ids, 4)
        && checked_add(&mut bytes, a_offsets, 4)
        && checked_add(&mut bytes, a_scratch, 4)
        && checked_add(&mut bytes, a_heads, 4)
        && checked_add(&mut bytes, a_nodes, a_node_size);

    (valid && bytes <= a_budget, if valid { bytes } else { 0 })
}

/// Port of `ShouldIndex()`.
pub fn should_index(a_estimated_visits: u64, a_hole_count: usize) -> bool {
    a_hole_count >= MIN_HOLE_COUNT && a_estimated_visits >= MIN_EDGE_VISITS
}
