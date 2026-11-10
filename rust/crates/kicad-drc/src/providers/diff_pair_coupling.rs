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

//! Port of `pcbnew/drc/drc_test_provider_diff_pair_coupling.cpp`.
//!
//! Differential pair gap/coupling test. Errors generated:
//! - `DRCE_DIFF_PAIR_GAP_OUT_OF_RANGE`
//! - `DRCE_DIFF_PAIR_UNCOUPLED_LENGTH_TOO_LONG`
//!
//! Ordering notes (the C++ is sequential, but some containers are pointer keyed):
//! - `DIFF_PAIR_ITEMS::itemsP` / `itemsN` are `std::set<BOARD_CONNECTED_ITEM*>` (address
//!   order); the port keeps them in `ItemId` order (board track order). This decides the order
//!   of the coupled segments (and so of the gap violations of one pair), the order of the items
//!   of an uncoupled-length violation and which item gives its marker position. The C++ order
//!   is heap dependent, so these are compared as sets / tie-aware.
//! - `dpRuleMatches` is a `std::map` keyed by `DIFF_PAIR_KEY` (net codes and rule names, no
//!   pointers): the port reproduces its order with the same comparator (see [`key_less`]).
//! - `m_board->GetConnectivity()->GetFromToCache()->Rebuild( m_board )`, the first statement
//!   of `Run()`, needs the connectivity mutably; the caller does it right before
//!   [`DrcTestProviderDiffPairCoupling::run`] (`run_flow::run_tests`).

use std::collections::BTreeSet;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{PcbLayerId, UNDEFINED_LAYER};
use kicad_core::lset::Lset;
use kicad_core::minoptmax::MinOptMax;
use kicad_core::typeinfo::KicadT;
use kicad_geom::cmath::{f64_to_i32_x86, f64_to_i64_x86};
use kicad_geom::geometry::eda_angle::{EdaAngle, EdaAngleT};
use kicad_geom::geometry::seg::Seg;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_arc::ShapeArc;
use kicad_geom::geometry::shape_segment::ShapeSegment;
use kicad_geom::math::util::rescale;
use kicad_geom::math::vector2::{Vector2I, ki_round_xy};
use kicad_geom::stdsort::std_sort_default;
use kicad_pcb::drc_api::{
    DRCE_DP_GAP_OUT_OF_RANGE, DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG, DrcRuleInfo,
};
use kicad_pcb::pcb_generator::GENERATOR_TYPE_TUNING_PATTERN;
use kicad_pcb::pcb_track::{PcbArc, Track};
use kicad_pcb::{AnyItem, BoardCtx, ItemId, ItemRef};

use super::run_host::{RunHost, report_aux};
use crate::caches::DrcBoardCaches;
use crate::constraint_types::DrcConstraintT;
use crate::engine::diff_pair::is_net_a_diff_pair;
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, for_each_geometry_item, no_path};

/// Default of `ADVANCED_CFG::m_MinParallelAngle` (degrees, `common/advanced_config.cpp`); the
/// port has no advanced config file.
pub const ADVANCED_CFG_MIN_PARALLEL_ANGLE: f64 = 0.001;

/// Port of `commonParallelProjection( SEG p, SEG n, SEG &pClip, SEG& nClip )`: the parts of
/// `p` and `n` that face each other, `None` when their projections do not overlap.
pub fn common_parallel_projection_seg(p: Seg, n: Seg) -> Option<(Seg, Seg)> {
    let n_proj_p = Seg::new(p.line_project(n.a), p.line_project(n.b));

    let mut t_a: i64 = 0;
    let mut t_b: i64 = p.t_coef(p.b);

    let mut tproj_a: i64 = p.t_coef(n_proj_p.a);
    let mut tproj_b: i64 = p.t_coef(n_proj_p.b);

    if t_b < t_a {
        std::mem::swap(&mut t_b, &mut t_a);
    }

    if tproj_b < tproj_a {
        std::mem::swap(&mut tproj_b, &mut tproj_a);
    }

    if t_b <= tproj_a {
        return None;
    }

    if t_a >= tproj_b {
        return None;
    }

    let mut tv: Vec<i64> = vec![0, p.t_coef(p.b), p.t_coef(n_proj_p.a), p.t_coef(n_proj_p.b)];
    std_sort_default(&mut tv); // fixme: awful and disgusting way of finding 2 midpoints

    let p_len_sq: i64 = p.squared_length();

    let dp = p.b - p.a;
    let mut p_clip = Seg::new(Vector2I::new(0, 0), Vector2I::new(0, 0));

    // int = int + int64_t: the sum is computed in 64 bits and truncated to int.
    p_clip.a.x = (i64::from(p.a.x) + rescale(i64::from(dp.x), tv[1], p_len_sq)) as i32;
    p_clip.a.y = (i64::from(p.a.y) + rescale(i64::from(dp.y), tv[1], p_len_sq)) as i32;

    p_clip.b.x = (i64::from(p.a.x) + rescale(i64::from(dp.x), tv[2], p_len_sq)) as i32;
    p_clip.b.y = (i64::from(p.a.y) + rescale(i64::from(dp.y), tv[2], p_len_sq)) as i32;

    let n_clip = Seg::new(n.line_project(p_clip.a), n.line_project(p_clip.b));

    Some((p_clip, n_clip))
}

/// `std::max( a, b )` on `EDA_ANGLE`: `( a < b ) ? b : a`.
fn angle_max(a: EdaAngle, b: EdaAngle) -> EdaAngle {
    if a < b { b } else { a }
}

/// `std::min( a, b )` on `EDA_ANGLE`: `( b < a ) ? b : a`.
fn angle_min(a: EdaAngle, b: EdaAngle) -> EdaAngle {
    if b < a { b } else { a }
}

/// `std::max( a, b )` on `double`.
fn f64_max(a: f64, b: f64) -> f64 {
    if a < b { b } else { a }
}

/// Port of `commonParallelProjection( const PCB_ARC& p, const PCB_ARC& n, SHAPE_ARC &pClip,
/// SHAPE_ARC& nClip )`: the overlapping parts of two roughly concentric arcs, `None` when
/// they are not coupled.
pub fn common_parallel_projection_arc(
    ctx: &BoardCtx<'_>,
    p: &PcbArc,
    n: &PcbArc,
) -> Option<(ShapeArc, ShapeArc)> {
    let p_center = p.center(ctx);
    let n_center = n.center(ctx);
    let p_radius = p.radius();
    let n_radius = n.radius();
    let p_is_ccw = p.is_ccw();
    let n_is_ccw = n.is_ccw();

    // Quick check to ensure arcs are of similar size and close enough to be considered coupled
    let radius_diff_ratio = (p_radius - n_radius).abs() / f64_max(p_radius, n_radius);
    let center_distance = f64::from((p_center - n_center).euclidean_norm());

    if radius_diff_ratio > 0.5 || center_distance > f64_max(p_radius, n_radius) * 0.5 {
        return None;
    }

    let mut p_start = p.track.start();
    let mut p_end = p.track.end();

    if !p_is_ccw {
        std::mem::swap(&mut p_start, &mut p_end);
    }

    let mut n_start = n.track.start();
    let mut n_end = n.track.end();

    if !n_is_ccw {
        std::mem::swap(&mut n_start, &mut n_end);
    }

    let mut p_arc = ShapeArc::from_start_mid_end(p_start, p.mid(), p_end, 0);
    let mut n_arc = ShapeArc::from_start_mid_end(n_start, n.mid(), n_end, 0);

    let p_start_angle = p_arc.get_start_angle();

    // Rotate the arcs to a common 0 starting angle
    p_arc.rotate(p_start_angle, p_center);
    n_arc.rotate(p_start_angle, n_center);

    let p_end_angle = p_arc.get_end_angle();
    let n_start_angle = n_arc.get_start_angle();
    let n_end_angle = n_arc.get_end_angle();

    // No overlap when n starts after p ends or n ends before p starts
    if n_start_angle >= p_end_angle || n_end_angle <= EdaAngle::new(0.0, EdaAngleT::Degrees) {
        return None;
    }

    // Calculate the start and end angles of the overlap
    let mut clip_start_angle = angle_max(EdaAngle::new(0.0, EdaAngleT::Degrees), n_start_angle);
    let mut clip_end_angle = angle_min(p_end_angle, n_end_angle);

    // Calculate the total angle of the overlap
    let clip_total_angle = clip_end_angle - clip_start_angle;

    // Now we reset the angles.  However, note that the convention here for adding angles
    // is OPPOSITE the Rotate convention above.  So this undoes the rotation.
    clip_start_angle += p_start_angle;
    clip_end_angle += p_start_angle;
    clip_start_angle.normalize();
    clip_end_angle.normalize();

    // One arc starts approximately where the other ends or overlap is too small
    if clip_total_angle <= EdaAngle::new(ADVANCED_CFG_MIN_PARALLEL_ANGLE, EdaAngleT::Degrees) {
        return None;
    }

    // For CCW arcs, we start at clip_start_angle and sweep through clip_total_angle
    // For CW arcs, we start at clip_end_angle and sweep through -clip_total_angle
    let p_clip = if p_is_ccw {
        let p_clip_point = p_center
            + ki_round_xy(
                p_radius * clip_start_angle.cos(),
                p_radius * clip_start_angle.sin(),
            );
        ShapeArc::from_center_start_angle(p_center, p_clip_point, clip_total_angle, 0)
    } else {
        let p_clip_point = p_center
            + ki_round_xy(
                p_radius * clip_end_angle.cos(),
                p_radius * clip_end_angle.sin(),
            );
        ShapeArc::from_center_start_angle(p_center, p_clip_point, -clip_total_angle, 0)
    };

    let n_clip = if n_is_ccw {
        let n_clip_point = n_center
            + ki_round_xy(
                n_radius * clip_start_angle.cos(),
                n_radius * clip_start_angle.sin(),
            );
        ShapeArc::from_center_start_angle(n_center, n_clip_point, clip_total_angle, 0)
    } else {
        let n_clip_point = n_center
            + ki_round_xy(
                n_radius * clip_end_angle.cos(),
                n_radius * clip_end_angle.sin(),
            );
        ShapeArc::from_center_start_angle(n_center, n_clip_point, -clip_total_angle, 0)
    };

    // Ensure the resulting arcs are not degenerate
    if p_clip.get_length() < 1.0 || n_clip.get_length() < 1.0 {
        return None;
    }

    Some((p_clip, n_clip))
}

/// Port of `DIFF_PAIR_KEY`. The rule pointers become rule copies (`DrcRuleInfo` with its
/// `rule_index` identity); they are not part of the ordering, as in C++.
#[derive(Clone, Debug, Default)]
pub struct DiffPairKey {
    /// `netP`
    pub net_p: i32,
    /// `netN`
    pub net_n: i32,
    /// `gapRuleName`
    pub gap_rule_name: String,
    /// `uncoupledRuleName`
    pub uncoupled_rule_name: String,
    /// `gapConstraint`
    pub gap_constraint: Option<MinOptMax<i32>>,
    /// `gapRule`
    pub gap_rule: Option<DrcRuleInfo>,
    /// `uncoupledConstraint`
    pub uncoupled_constraint: Option<MinOptMax<i32>>,
    /// `uncoupledRule`
    pub uncoupled_rule: Option<DrcRuleInfo>,
}

/// `wxString::operator<` (`wxString::compare`: code point order, the same as UTF-8 byte
/// order).
fn wx_less(a: &str, b: &str) -> bool {
    a < b
}

/// Port of `DIFF_PAIR_KEY::operator<`, verbatim: with equal nets, the gap rule names decide
/// when `a`'s gap rule name is empty, else the uncoupled rule names.
pub fn key_less(a: &DiffPairKey, b: &DiffPairKey) -> bool {
    if a.net_p < b.net_p {
        true
    } else if a.net_p > b.net_p {
        false
    } else if a.net_n < b.net_n {
        // netP == b.netP
        true
    } else if a.net_n > b.net_n {
        false
    } else if a.gap_rule_name.is_empty() {
        wx_less(&a.gap_rule_name, &b.gap_rule_name)
    } else {
        wx_less(&a.uncoupled_rule_name, &b.uncoupled_rule_name)
    }
}

/// Port of `DIFF_PAIR_COUPLED_SEGMENTS`.
#[derive(Clone, Debug)]
pub struct DiffPairCoupledSegments {
    /// `coupledN`
    pub coupled_n: Seg,
    /// `coupledP`
    pub coupled_p: Seg,
    /// `isArc`
    pub is_arc: bool,
    /// `coupledArcN`
    pub coupled_arc_n: ShapeArc,
    /// `coupledArcP`
    pub coupled_arc_p: ShapeArc,
    /// `parentN`
    pub parent_n: ItemId,
    /// `parentP`
    pub parent_p: ItemId,
    /// `computedGap`
    pub computed_gap: i64,
    /// `nearestN`
    pub nearest_n: Vector2I,
    /// `nearestP`
    pub nearest_p: Vector2I,
    /// `layer`
    pub layer: PcbLayerId,
    /// `couplingFailMin`
    pub coupling_fail_min: bool,
    /// `couplingFailMax`
    pub coupling_fail_max: bool,
}

impl DiffPairCoupledSegments {
    /// `DIFF_PAIR_COUPLED_SEGMENTS()` with the two parents (never null in the port).
    fn new(parent_p: ItemId, parent_n: ItemId) -> Self {
        let zero = Vector2I::new(0, 0);

        DiffPairCoupledSegments {
            coupled_n: Seg::new(zero, zero),
            coupled_p: Seg::new(zero, zero),
            is_arc: false,
            coupled_arc_n: ShapeArc::new(),
            coupled_arc_p: ShapeArc::new(),
            parent_n,
            parent_p,
            computed_gap: 0,
            nearest_n: zero,
            nearest_p: zero,
            layer: UNDEFINED_LAYER,
            coupling_fail_min: false,
            coupling_fail_max: false,
        }
    }
}

/// Port of `DIFF_PAIR_ITEMS`. The pointer sets are kept in `ItemId` order (see the module
/// comment).
#[derive(Clone, Debug, Default)]
pub struct DiffPairItems {
    /// `itemsP`
    pub items_p: BTreeSet<ItemId>,
    /// `itemsN`
    pub items_n: BTreeSet<ItemId>,
    /// `coupled`
    pub coupled: Vec<DiffPairCoupledSegments>,
    /// `totalCoupled`
    pub total_coupled: i32,
    /// `totalLengthN`
    pub total_length_n: i32,
    /// `totalLengthP`
    pub total_length_p: i32,
}

/// Null link of [`DpRuleMatches`].
const RB_NIL: usize = usize::MAX;

/// The header node index of [`DpRuleMatches`] (`_M_impl._M_header`).
const RB_HEADER: usize = 0;

/// A node of [`DpRuleMatches`] (`_Rb_tree_node_base` plus the value; the header has none).
#[derive(Clone, Debug)]
struct RbNode {
    entry: Option<(DiffPairKey, DiffPairItems)>,
    parent: usize,
    left: usize,
    right: usize,
    red: bool,
}

/// `std::map<DIFF_PAIR_KEY, DIFF_PAIR_ITEMS>` with the comparator [`key_less`].
///
/// `DIFF_PAIR_KEY::operator<` is not a strict weak ordering (a key with an empty gap rule name
/// and one with a gap rule and a different uncoupled rule name are each less than the other),
/// so which entry `operator[]` finds depends on the shape of the libstdc++ red-black tree and
/// on its exact sequence of comparisons. Both are reproduced: `operator[]` (`lower_bound`,
/// then `_M_emplace_hint_unique`) and `_Rb_tree_insert_and_rebalance` of GCC 13
/// (`bits/stl_map.h`, `bits/stl_tree.h`, `src/c++98/tree.cc`), on an index arena. The
/// rebalancing is the one of `kicad_geom::stdsort::StdSet` (verified against libstdc++ there),
/// which has no `lower_bound` / hinted insertion.
#[derive(Clone, Debug)]
pub struct DpRuleMatches {
    nodes: Vec<RbNode>,
}

impl Default for DpRuleMatches {
    fn default() -> Self {
        Self::new()
    }
}

impl DpRuleMatches {
    /// An empty map (`_M_reset()`: the header is red and links to itself as leftmost and
    /// rightmost).
    pub fn new() -> Self {
        DpRuleMatches {
            nodes: vec![RbNode {
                entry: None,
                parent: RB_NIL,
                left: RB_HEADER,
                right: RB_HEADER,
                red: true,
            }],
        }
    }

    /// `size()`
    pub fn len(&self) -> usize {
        self.nodes.len() - 1
    }

    /// `empty()`
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn key(&self, n: usize) -> &DiffPairKey {
        &self.nodes[n].entry.as_ref().expect("value node").0
    }

    fn root(&self) -> usize {
        self.nodes[RB_HEADER].parent
    }

    fn leftmost(&self) -> usize {
        self.nodes[RB_HEADER].left
    }

    fn rightmost(&self) -> usize {
        self.nodes[RB_HEADER].right
    }

    /// Port of `map::operator[]( k )`: `lower_bound( k )`; unless the found entry is
    /// equivalent (`!comp( k, found )`), a new entry with a copy of `k` is inserted with that
    /// position as hint (`_M_emplace_hint_unique`). An existing entry keeps the key it was
    /// created with.
    pub fn entry(&mut self, k: &DiffPairKey) -> &mut DiffPairItems {
        let mut i = self.lower_bound(k);

        if i == RB_HEADER || key_less(k, self.key(i)) {
            i = self.emplace_hint_unique(i, k);
        }

        &mut self.nodes[i].entry.as_mut().expect("value node").1
    }

    /// `_M_lower_bound( _M_begin(), _M_end(), k )`.
    fn lower_bound(&self, k: &DiffPairKey) -> usize {
        let mut x = self.root();
        let mut y = RB_HEADER;

        while x != RB_NIL {
            if !key_less(self.key(x), k) {
                y = x;
                x = self.nodes[x].left;
            } else {
                x = self.nodes[x].right;
            }
        }

        y
    }

    /// `_M_emplace_hint_unique( pos, k )`: the node of `k` (new, or the equivalent one the
    /// hint search found).
    fn emplace_hint_unique(&mut self, pos: usize, k: &DiffPairKey) -> usize {
        match self.get_insert_hint_unique_pos(pos, k) {
            (x, Some(p)) => self.insert_node(x, p, k.clone()),
            (existing, None) => existing,
        }
    }

    /// `_M_get_insert_hint_unique_pos( position, k )`: `(x, Some( p ))` of the insert position,
    /// or `(node, None)` for an equivalent node.
    fn get_insert_hint_unique_pos(&self, pos: usize, k: &DiffPairKey) -> (usize, Option<usize>) {
        if pos == RB_HEADER {
            // end()
            if !self.is_empty() && key_less(self.key(self.rightmost()), k) {
                return (RB_NIL, Some(self.rightmost()));
            }

            return self.get_insert_unique_pos(k);
        }

        if key_less(k, self.key(pos)) {
            // First, try before...
            if pos == self.leftmost() {
                // begin()
                return (self.leftmost(), Some(self.leftmost()));
            }

            let before = self.decrement(pos);

            if key_less(self.key(before), k) {
                if self.nodes[before].right == RB_NIL {
                    return (RB_NIL, Some(before));
                }

                return (pos, Some(pos));
            }

            return self.get_insert_unique_pos(k);
        }

        if key_less(self.key(pos), k) {
            // ... then try after.
            if pos == self.rightmost() {
                return (RB_NIL, Some(self.rightmost()));
            }

            let after = self.increment(pos);

            if key_less(k, self.key(after)) {
                if self.nodes[pos].right == RB_NIL {
                    return (RB_NIL, Some(pos));
                }

                return (after, Some(after));
            }

            return self.get_insert_unique_pos(k);
        }

        // Equivalent keys.
        (pos, None)
    }

    /// `_M_get_insert_unique_pos( k )`.
    fn get_insert_unique_pos(&self, k: &DiffPairKey) -> (usize, Option<usize>) {
        let mut x = self.root();
        let mut y = RB_HEADER;
        let mut comp = true;

        while x != RB_NIL {
            y = x;
            comp = key_less(k, self.key(x));
            x = if comp {
                self.nodes[x].left
            } else {
                self.nodes[x].right
            };
        }

        let mut j = y;

        if comp {
            if j == self.leftmost() {
                // j == begin()
                return (x, Some(y));
            }

            j = self.decrement(j);
        }

        if key_less(self.key(j), k) {
            return (x, Some(y));
        }

        (j, None)
    }

    /// `_M_insert_node( x, p, z )` with a new node for `k`: its index.
    fn insert_node(&mut self, x: usize, p: usize, k: DiffPairKey) -> usize {
        let insert_left = x != RB_NIL || p == RB_HEADER || key_less(&k, self.key(p));
        self.nodes.push(RbNode {
            entry: Some((k, DiffPairItems::default())),
            parent: RB_NIL,
            left: RB_NIL,
            right: RB_NIL,
            red: true,
        });
        let z = self.nodes.len() - 1;
        self.insert_and_rebalance(insert_left, z, p);
        z
    }

    /// `_Rb_tree_increment( x )`.
    fn increment(&self, mut x: usize) -> usize {
        if self.nodes[x].right != RB_NIL {
            x = self.nodes[x].right;

            while self.nodes[x].left != RB_NIL {
                x = self.nodes[x].left;
            }

            return x;
        }

        let mut y = self.nodes[x].parent;

        while x == self.nodes[y].right {
            x = y;
            y = self.nodes[y].parent;
        }

        if self.nodes[x].right != y {
            x = y;
        }

        x
    }

    /// `_Rb_tree_decrement( x )`.
    fn decrement(&self, mut x: usize) -> usize {
        if self.nodes[x].red
            && self.nodes[x].parent != RB_NIL
            && self.nodes[self.nodes[x].parent].parent == x
        {
            // x is the header (end()): its predecessor is the rightmost node
            return self.nodes[x].right;
        }

        if self.nodes[x].left != RB_NIL {
            let mut y = self.nodes[x].left;

            while self.nodes[y].right != RB_NIL {
                y = self.nodes[y].right;
            }

            return y;
        }

        let mut y = self.nodes[x].parent;

        while x == self.nodes[y].left {
            x = y;
            y = self.nodes[y].parent;
        }

        y
    }

    /// `local_Rb_tree_rotate_left( x, root )`.
    fn rotate_left(&mut self, x: usize) {
        let y = self.nodes[x].right;
        self.nodes[x].right = self.nodes[y].left;

        if self.nodes[y].left != RB_NIL {
            let yl = self.nodes[y].left;
            self.nodes[yl].parent = x;
        }

        self.nodes[y].parent = self.nodes[x].parent;
        let xp = self.nodes[x].parent;

        if x == self.root() {
            self.nodes[RB_HEADER].parent = y;
        } else if x == self.nodes[xp].left {
            self.nodes[xp].left = y;
        } else {
            self.nodes[xp].right = y;
        }

        self.nodes[y].left = x;
        self.nodes[x].parent = y;
    }

    /// `local_Rb_tree_rotate_right( x, root )`.
    fn rotate_right(&mut self, x: usize) {
        let y = self.nodes[x].left;
        self.nodes[x].left = self.nodes[y].right;

        if self.nodes[y].right != RB_NIL {
            let yr = self.nodes[y].right;
            self.nodes[yr].parent = x;
        }

        self.nodes[y].parent = self.nodes[x].parent;
        let xp = self.nodes[x].parent;

        if x == self.root() {
            self.nodes[RB_HEADER].parent = y;
        } else if x == self.nodes[xp].right {
            self.nodes[xp].right = y;
        } else {
            self.nodes[xp].left = y;
        }

        self.nodes[y].right = x;
        self.nodes[x].parent = y;
    }

    /// `_Rb_tree_insert_and_rebalance( insert_left, x, p, header )`.
    fn insert_and_rebalance(&mut self, insert_left: bool, mut x: usize, p: usize) {
        // Initialize fields in new node to insert.
        self.nodes[x].parent = p;
        self.nodes[x].left = RB_NIL;
        self.nodes[x].right = RB_NIL;
        self.nodes[x].red = true;

        // Insert.
        // Make new node child of parent and maintain root, leftmost and rightmost nodes.
        // N.B. First node is always inserted left.
        if insert_left {
            self.nodes[p].left = x; // also makes leftmost = x when p == &header

            if p == RB_HEADER {
                self.nodes[RB_HEADER].parent = x;
                self.nodes[RB_HEADER].right = x;
            } else if p == self.nodes[RB_HEADER].left {
                self.nodes[RB_HEADER].left = x; // maintain leftmost pointing to min node
            }
        } else {
            self.nodes[p].right = x;

            if p == self.nodes[RB_HEADER].right {
                self.nodes[RB_HEADER].right = x; // maintain rightmost pointing to max node
            }
        }

        // Rebalance.
        while x != self.root() && self.nodes[self.nodes[x].parent].red {
            let xp = self.nodes[x].parent;
            let xpp = self.nodes[xp].parent;

            if xp == self.nodes[xpp].left {
                let y = self.nodes[xpp].right;

                if y != RB_NIL && self.nodes[y].red {
                    self.nodes[xp].red = false;
                    self.nodes[y].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp].right {
                        x = xp;
                        self.rotate_left(x);
                    }

                    let xp = self.nodes[x].parent;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    self.rotate_right(xpp);
                }
            } else {
                let y = self.nodes[xpp].left;

                if y != RB_NIL && self.nodes[y].red {
                    self.nodes[xp].red = false;
                    self.nodes[y].red = false;
                    self.nodes[xpp].red = true;
                    x = xpp;
                } else {
                    if x == self.nodes[xp].left {
                        x = xp;
                        self.rotate_right(x);
                    }

                    let xp = self.nodes[x].parent;
                    self.nodes[xp].red = false;
                    self.nodes[xpp].red = true;
                    self.rotate_left(xpp);
                }
            }
        }

        let root = self.root();
        self.nodes[root].red = false;
    }

    /// The node indices in iteration order (`begin()` .. `end()`).
    fn in_order(&self) -> Vec<usize> {
        let mut out = Vec::with_capacity(self.len());
        let mut x = self.leftmost();

        while x != RB_HEADER && !self.is_empty() {
            out.push(x);
            x = self.increment(x);
        }

        out
    }

    /// The entries in iteration order, consuming the map.
    pub fn into_entries(mut self) -> Vec<(DiffPairKey, DiffPairItems)> {
        self.in_order()
            .into_iter()
            .map(|n| self.nodes[n].entry.take().expect("value node"))
            .collect()
    }
}

/// Port of `isInTuningPattern`: the item's parent group is a tuning pattern generator.
fn is_in_tuning_pattern(item: ItemRef<'_>) -> bool {
    if let Some(parent) = item.parent_group() {
        if let AnyItem::Generator(generator) = parent.any() {
            if generator.generator_type() == GENERATOR_TYPE_TUNING_PATTERN {
                return true;
            }
        }
    }

    false
}

/// The `( a.GetLayerSet() & b.GetLayerSet() ).none()` test.
fn no_common_layer(a: ItemRef<'_>, b: ItemRef<'_>) -> bool {
    !(&a.layer_set() & &b.layer_set()).any()
}

/// `aItem->HitTest( aPos, 0 )` (virtual).
fn hit_test(ctx: &BoardCtx<'_>, item: ItemRef<'_>, pos: Vector2I) -> bool {
    item.as_dyn().hit_test(ctx, pos, 0)
}

/// The start and end of a track (`PCB_TRACK::GetStart()` / `GetEnd()`).
fn track_ends(item: ItemRef<'_>) -> (Vector2I, Vector2I) {
    let t = item.as_track().expect("a track").as_track();
    (t.start(), t.end())
}

/// `pad->HitTest( track->GetStart(), 0 ) ^ pad->HitTest( track->GetEnd(), 0 )`: the
/// `trackExitsPad` / `arcExitsPad` lambdas.
fn track_exits_pad(ctx: &BoardCtx<'_>, pad: ItemRef<'_>, track: ItemRef<'_>) -> bool {
    let (start, end) = track_ends(track);
    let start_in = hit_test(ctx, pad, start);
    let end_in = hit_test(ctx, pad, end);

    start_in ^ end_in
}

/// `std::sqrt( int64_t )` assigned to an `int64_t` (truncation).
fn sqrt_i64(v: i64) -> i64 {
    f64_to_i64_x86((v as f64).sqrt())
}

/// Port of `extractDiffPairCoupledItems`.
pub fn extract_diff_pair_coupled_items(
    ctx: &BoardCtx<'_>,
    caches: &DrcBoardCaches,
    dp: &mut DiffPairItems,
) {
    let tree = caches.copper_item_rtree();

    for &item_p in &dp.items_p {
        let Some(ref_p) = ctx.item(item_p) else {
            continue;
        };

        if ref_p.type_() != KicadT::PcbTraceT || is_in_tuning_pattern(ref_p) {
            continue;
        }

        let sp = ref_p.as_track().expect("a track").as_track();
        let mut coupled_vec: Vec<DiffPairCoupledSegments> = Vec::new();

        for &item_n in &dp.items_n {
            let Some(ref_n) = ctx.item(item_n) else {
                continue;
            };

            if ref_n.type_() != KicadT::PcbTraceT || is_in_tuning_pattern(ref_n) {
                continue;
            }

            let sn = ref_n.as_track().expect("a track").as_track();

            if no_common_layer(ref_n, ref_p) {
                continue;
            }

            let ssp = Seg::new(sp.start(), sp.end());
            let ssn = Seg::new(sn.start(), sn.end());

            // Segments that are ~ 1 IU in length per side are approximately parallel (tolerance is 1 IU)
            // with everything and their parallel projection is < 1 IU, leading to bad distance calculations
            if ssp.squared_length() > 2
                && ssn.squared_length() > 2
                && ssp.intersect_with(&ssn, false, true).is_none()
            {
                let mut cpair = DiffPairCoupledSegments::new(item_p, item_n);
                let coupled = common_parallel_projection_seg(ssp, ssn);

                if let Some((coupled_p, coupled_n)) = coupled {
                    cpair.coupled_p = coupled_p;
                    cpair.coupled_n = coupled_n;
                    cpair.layer = ref_p.layer();
                    let (nearest_p, nearest_n, dist_sq) =
                        cpair.coupled_p.nearest_points(&cpair.coupled_n);
                    cpair.nearest_p = nearest_p;
                    cpair.nearest_n = nearest_n;
                    cpair.computed_gap = sqrt_i64(dist_sq); // NearestPoints returns squared distance
                    cpair.computed_gap -= i64::from((sp.width() + sn.width()) / 2);
                    coupled_vec.push(cpair);
                }
            }
        }

        for coupled in &coupled_vec {
            let mut exclude_self = |id: ItemId| -> bool {
                if id == coupled.parent_n || id == coupled.parent_p {
                    return false;
                }

                let Some(item) = ctx.item(id) else {
                    return true;
                };

                let t = item.type_();

                if t == KicadT::PcbTraceT || t == KicadT::PcbViaT || t == KicadT::PcbArcT {
                    // Directly connected items don't count
                    if hit_test(ctx, item, coupled.coupled_n.a)
                        || hit_test(ctx, item, coupled.coupled_n.b)
                        || hit_test(ctx, item, coupled.coupled_p.a)
                        || hit_test(ctx, item, coupled.coupled_p.b)
                    {
                        return false;
                    }
                } else if t == KicadT::PcbPadT {
                    let parent_p = ctx.item(coupled.parent_p).expect("parentP");
                    let parent_n = ctx.item(coupled.parent_n).expect("parentN");

                    if track_exits_pad(ctx, item, parent_p) || track_exits_pad(ctx, item, parent_n)
                    {
                        return false;
                    }
                }

                true
            };

            let check_seg = ShapeSegment::new(coupled.nearest_n, coupled.nearest_p, 0);

            // check if there's anything in between the segments suspected to be coupled. If
            // there's nothing, assume they are really coupled.

            if !tree.check_colliding(&check_seg, ref_p.layer(), 0, Some(&mut exclude_self)) {
                dp.coupled.push(coupled.clone());
            }
        }
    }

    for &item_p in &dp.items_p {
        let Some(ref_p) = ctx.item(item_p) else {
            continue;
        };

        if ref_p.type_() != KicadT::PcbArcT || is_in_tuning_pattern(ref_p) {
            continue;
        }

        let sp = ref_p.as_arc().expect("an arc");
        let mut coupled_vec: Vec<DiffPairCoupledSegments> = Vec::new();

        for &item_n in &dp.items_n {
            let Some(ref_n) = ctx.item(item_n) else {
                continue;
            };

            if ref_n.type_() != KicadT::PcbArcT || is_in_tuning_pattern(ref_n) {
                continue;
            }

            let sn = ref_n.as_arc().expect("an arc");

            if no_common_layer(ref_n, ref_p) {
                continue;
            }

            // Segments that are ~ 1 IU in length per side are approximately parallel (tolerance is 1 IU)
            // with everything and their parallel projection is < 1 IU, leading to bad distance calculations
            let sq_width: i64 = i64::from(sp.track.width()) * i64::from(sp.track.width());

            if sp.length() > 2.0
                && sn.length() > 2.0
                && sp.center(ctx).squared_distance(sn.center(ctx)) < sq_width
            {
                let mut cpair = DiffPairCoupledSegments::new(item_p, item_n);
                cpair.is_arc = true;
                let coupled = common_parallel_projection_arc(ctx, sp, sn);

                if let Some((coupled_arc_p, coupled_arc_n)) = coupled {
                    cpair.coupled_arc_p = coupled_arc_p;
                    cpair.coupled_arc_n = coupled_arc_n;
                    cpair.layer = ref_p.layer();
                    let mut nearest_p = cpair.nearest_p;
                    let mut nearest_n = cpair.nearest_n;
                    let mut dist_sq = cpair.computed_gap;
                    cpair.coupled_arc_p.nearest_points_arc(
                        &cpair.coupled_arc_n,
                        &mut nearest_p,
                        &mut nearest_n,
                        &mut dist_sq,
                    );
                    cpair.nearest_p = nearest_p;
                    cpair.nearest_n = nearest_n;
                    cpair.computed_gap = sqrt_i64(dist_sq); // NearestPoints returns squared distance
                    cpair.computed_gap -= i64::from((sp.track.width() + sn.track.width()) / 2);
                    coupled_vec.push(cpair);
                }
            }
        }

        for coupled in &coupled_vec {
            let parent_n_net = ctx.item(coupled.parent_n).and_then(|i| i.net_code());
            let parent_p_net = ctx.item(coupled.parent_p).and_then(|i| i.net_code());

            let mut exclude_self = |id: ItemId| -> bool {
                if id == coupled.parent_n || id == coupled.parent_p {
                    return false;
                }

                let Some(item) = ctx.item(id) else {
                    return true;
                };

                let t = item.type_();

                if t == KicadT::PcbTraceT || t == KicadT::PcbViaT || t == KicadT::PcbArcT {
                    let net = item.net_code();

                    if net == parent_n_net || net == parent_p_net {
                        return false;
                    }
                } else if t == KicadT::PcbPadT {
                    let parent_p = ctx.item(coupled.parent_p).expect("parentP");
                    let parent_n = ctx.item(coupled.parent_n).expect("parentN");

                    if track_exits_pad(ctx, item, parent_p) || track_exits_pad(ctx, item, parent_n)
                    {
                        return false;
                    }
                }

                true
            };

            let check_arc_mid = ShapeSegment::new(
                coupled.coupled_arc_n.get_arc_mid(),
                coupled.coupled_arc_p.get_arc_mid(),
                0,
            );

            // check if there's anything in between the segments suspected to be coupled. If
            // there's nothing, assume they are really coupled.

            if !tree.check_colliding(&check_arc_mid, ref_p.layer(), 0, Some(&mut exclude_self)) {
                dp.coupled.push(coupled.clone());
            }
        }
    }
}

/// Port of `DRC_TEST_PROVIDER_DIFF_PAIR_COUPLING`. It reads the copper item tree of the run's
/// board caches (`board->m_CopperItemRTreeCache`).
#[derive(Clone)]
pub struct DrcTestProviderDiffPairCoupling<'c> {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
    /// The board caches of the run (`DRC_CACHE_GENERATOR`).
    caches: &'c DrcBoardCaches,
}

impl<'c> DrcTestProviderDiffPairCoupling<'c> {
    /// `DRC_TEST_PROVIDER_DIFF_PAIR_COUPLING()` with the run's board caches.
    pub fn new(caches: &'c DrcBoardCaches) -> Self {
        DrcTestProviderDiffPairCoupling {
            base: DrcTestProviderBase::new("diff_pair_coupling"),
            caches,
        }
    }

    /// Port of `DRC_TEST_PROVIDER_DIFF_PAIR_COUPLING::Run`.
    pub fn run(&self, engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        let board = ctx.board;

        let epsilon = board.design_settings().get_drc_epsilon();
        let board_copper_layers = Lset::all_cu_mask_n(board.copper_layer_count());

        let mut dp_rule_matches = DpRuleMatches::default();

        let evaluate_dp_constraints = |item: ItemRef<'_>| -> bool {
            let mut key = DiffPairKey::default();
            let ref_net = item.net();

            let Some(ref_net) = ref_net else {
                return true;
            };

            let Some((net_p, net_n)) = is_net_a_diff_pair(board, ref_net) else {
                return true;
            };

            key.net_p = net_p.0;
            key.net_n = net_n.0;

            for constraint_type in [
                DrcConstraintT::DiffPairGapConstraint,
                DrcConstraintT::MaxUncoupledConstraint,
            ] {
                let constraint =
                    engine.eval_rules(ctx, constraint_type, Some(item), None, item.layer());

                if constraint.is_null() || constraint.severity() == Severity::Ignore {
                    continue;
                }

                let parent_rule = constraint.parent_rule.clone();
                let rule_name = match &parent_rule {
                    Some(rule) => rule.name.clone(),
                    None => constraint.name(),
                };

                match constraint_type {
                    DrcConstraintT::DiffPairGapConstraint => {
                        key.gap_constraint = Some(*constraint.value());
                        key.gap_rule = parent_rule;
                        key.gap_rule_name = rule_name;
                    }
                    DrcConstraintT::MaxUncoupledConstraint => {
                        key.uncoupled_constraint = Some(*constraint.value());
                        key.uncoupled_rule = parent_rule;
                        key.uncoupled_rule_name = rule_name;
                    }
                    _ => {}
                }

                let id = item.id().expect("a board item");

                if ref_net.net_code().0 == key.net_n {
                    dp_rule_matches.entry(&key).items_n.insert(id);
                } else {
                    dp_rule_matches.entry(&key).items_p.insert(id);
                }
            }

            true
        };

        for_each_geometry_item(
            ctx,
            &[KicadT::PcbTraceT, KicadT::PcbViaT, KicadT::PcbArcT],
            &board_copper_layers,
            evaluate_dp_constraints,
        );

        report_aux(run, "DPs evaluated:");

        let units = self.base.units_provider();
        let mut host = RunHost(run);

        for (key, item_set) in dp_rule_matches.into_entries().iter_mut() {
            let ni_p = board.find_net(kicad_pcb::NetCode(key.net_p));
            let ni_n = board.find_net(kicad_pcb::NetCode(key.net_n));

            debug_assert!(ni_p.is_some());
            debug_assert!(ni_n.is_some());

            let name_p = ni_p.map_or("", |n| n.netname());
            let name_n = ni_n.map_or("", |n| n.netname());

            if let Some(r) = host.log_reporter() {
                r.report(
                    &format!(
                        "Rule '{}', DP: (+) {} - (-) {}",
                        key.gap_rule_name, name_p, name_n
                    ),
                    Severity::Info,
                );
            }

            extract_diff_pair_coupled_items(ctx, self.caches, item_set);

            item_set.total_coupled = 0;
            item_set.total_length_n = 0;
            item_set.total_length_p = 0;

            let mut all_items: BTreeSet<ItemId> = BTreeSet::new();

            for &id in &item_set.items_n {
                let Some(track) = ctx.item(id) else {
                    continue;
                };

                if let Some(t) = track.as_track() {
                    if is_in_tuning_pattern(track) {
                        continue;
                    }

                    if all_items.insert(id) {
                        item_set.total_length_n =
                            f64_to_i32_x86(f64::from(item_set.total_length_n) + track_length(t));
                    }
                }
            }

            for &id in &item_set.items_p {
                let Some(track) = ctx.item(id) else {
                    continue;
                };

                if let Some(t) = track.as_track() {
                    if is_in_tuning_pattern(track) {
                        continue;
                    }

                    if all_items.insert(id) {
                        item_set.total_length_p =
                            f64_to_i32_x86(f64::from(item_set.total_length_p) + track_length(t));
                    }
                }
            }

            for dp in item_set.coupled.iter_mut() {
                // int length = dp.isArc ? GetLength() (double) : Length() (int);
                let length = if dp.is_arc {
                    f64_to_i32_x86(dp.coupled_arc_n.get_length())
                } else {
                    dp.coupled_n.length()
                };

                // The debug overlay of the C++ is GUI only.

                if let Some(gap) = &key.gap_constraint {
                    if gap.has_min()
                        && gap.min() >= 0
                        && (dp.computed_gap < i64::from(gap.min() - epsilon))
                    {
                        dp.coupling_fail_min = true;
                    }

                    if gap.has_max()
                        && gap.max() >= 0
                        && (dp.computed_gap > i64::from(gap.max() + epsilon))
                    {
                        dp.coupling_fail_max = true;
                    }
                }

                if !dp.coupling_fail_min && !dp.coupling_fail_max {
                    item_set.total_coupled += length;
                }
            }

            let total_len = std::cmp::max(item_set.total_length_n, item_set.total_length_p);

            if let Some(r) = host.log_reporter() {
                r.report(
                    &format!(
                        "   - coupled length: {}, total length: {}",
                        units.message_text_from_value(
                            f64::from(item_set.total_coupled),
                            true,
                            EdaDataType::Distance
                        ),
                        units.message_text_from_value(
                            f64::from(total_len),
                            true,
                            EdaDataType::Distance
                        )
                    ),
                    Severity::Info,
                );
            }

            let total_uncoupled = total_len - item_set.total_coupled;
            let mut uncoupled_violation = false;

            if let Some(val) = &key.uncoupled_constraint
                && (!item_set.items_p.is_empty() || !item_set.items_n.is_empty())
                && val.has_max()
                && val.max() >= 0
                && total_uncoupled > val.max()
            {
                let mut drce = item::create(DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG)
                    .expect("DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG item");
                drce.set_error_detail(&self.base.format_msg(
                    "(%s maximum uncoupled length %s; actual %s)",
                    &key.uncoupled_rule_name,
                    f64::from(val.max()),
                    f64::from(total_uncoupled),
                    EdaDataType::Distance,
                ));

                let mut item: Option<ItemId> = None;
                let mut ids = Vec::new();
                let mut p_it = item_set.items_p.iter().peekable();
                let mut n_it = item_set.items_n.iter().peekable();

                if let Some(&p) = p_it.next() {
                    item = Some(p);
                    ids.push(p);
                }

                if let Some(&n) = n_it.next() {
                    item = Some(n);
                    ids.push(n);
                }

                ids.extend(p_it.copied());
                ids.extend(n_it.copied());

                drce.set_item_ids(
                    ids.iter()
                        .map(|&id| ctx.item(id).expect("an item").uuid())
                        .collect(),
                );

                uncoupled_violation = true;

                drce.violating_rule = key.uncoupled_rule.clone();

                let item = ctx.item(item.expect("an item")).expect("an item");

                self.base.report_violation(
                    &mut host,
                    Arc::new(drce),
                    item.position(),
                    item.layer().0,
                    &no_path,
                );
            }

            if key.gap_constraint.is_some()
                && (uncoupled_violation || key.uncoupled_constraint.is_none())
            {
                for dp in item_set.coupled.iter_mut() {
                    if dp.coupling_fail_min || dp.coupling_fail_max {
                        let parent_p = ctx.item(dp.parent_p).expect("parentP");
                        let parent_n = ctx.item(dp.parent_n).expect("parentN");

                        // We have a candidate violation, now we need to re-query for a constraint
                        // given the actual items, because there may be a location-based rule in play.
                        let constraint = engine.eval_rules(
                            ctx,
                            DrcConstraintT::DiffPairGapConstraint,
                            Some(parent_p),
                            Some(parent_n),
                            parent_p.layer(),
                        );
                        let val = *constraint.value();

                        if !val.has_min()
                            || val.min() < 0
                            || dp.computed_gap >= i64::from(val.min())
                        {
                            dp.coupling_fail_min = false;
                        }

                        if !val.has_max()
                            || val.max() < 0
                            || dp.computed_gap <= i64::from(val.max())
                        {
                            dp.coupling_fail_max = false;
                        }

                        if !dp.coupling_fail_min && !dp.coupling_fail_max {
                            continue;
                        }

                        let mut drc_item = item::create(DRCE_DP_GAP_OUT_OF_RANGE)
                            .expect("DRCE_DP_GAP_OUT_OF_RANGE item");

                        if dp.coupling_fail_min {
                            drc_item.set_error_detail(&self.base.format_msg(
                                "(%s minimum gap %s; actual %s)",
                                &key.gap_rule_name,
                                f64::from(val.min()),
                                dp.computed_gap as f64,
                                EdaDataType::Distance,
                            ));
                        } else if dp.coupling_fail_max {
                            drc_item.set_error_detail(&self.base.format_msg(
                                "(%s maximum gap %s; actual %s)",
                                &key.gap_rule_name,
                                f64::from(val.max()),
                                dp.computed_gap as f64,
                                EdaDataType::Distance,
                            ));
                        }

                        drc_item.violating_rule = key.gap_rule.clone();

                        // Both parents exist: item = parentN.
                        drc_item.set_item_ids(vec![parent_p.uuid(), parent_n.uuid()]);

                        self.base.report_violation(
                            &mut host,
                            Arc::new(drc_item),
                            focus_position(parent_n),
                            parent_n.layer().0,
                            &no_path,
                        );
                    }
                }
            }
        }

        true
    }
}

/// `PCB_TRACK::GetLength()` (virtual: `PCB_ARC` overrides it).
fn track_length(t: &Track) -> f64 {
    t.length()
}

/// `GetFocusPosition()` of a track: `( m_Start + m_End ) / 2` for a segment (and a via),
/// `m_Mid` for an arc.
fn focus_position(item: ItemRef<'_>) -> Vector2I {
    match item.as_track() {
        Some(Track::Arc(a)) => a.mid(),
        Some(t) => {
            let t = t.as_track();
            (t.start() + t.end()) / 2
        }
        None => item.position(),
    }
}

impl DrcTestProviderRun for DrcTestProviderDiffPairCoupling<'_> {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(engine, run, ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(net_p: i32, net_n: i32, gap: &str, unc: &str) -> DiffPairKey {
        DiffPairKey {
            net_p,
            net_n,
            gap_rule_name: gap.to_string(),
            uncoupled_rule_name: unc.to_string(),
            ..DiffPairKey::default()
        }
    }

    /// `DIFF_PAIR_KEY::operator<` and the two entries the C++ makes per item (after the gap
    /// constraint, then with both constraints).
    #[test]
    fn dp_rule_matches_order() {
        let mut m = DpRuleMatches::default();
        let net = ItemId::net(kicad_pcb::NetCode(1)).unwrap();
        m.entry(&key(3, 4, "g", "")).items_p.insert(net);
        m.entry(&key(3, 4, "g", "u"));
        m.entry(&key(1, 2, "g", ""));
        m.entry(&key(3, 4, "g", ""));
        m.entry(&key(3, 4, "x", ""));

        let entries = m.into_entries();
        let order: Vec<(i32, i32, &str, &str)> = entries
            .iter()
            .map(|(k, _)| {
                (
                    k.net_p,
                    k.net_n,
                    k.gap_rule_name.as_str(),
                    k.uncoupled_rule_name.as_str(),
                )
            })
            .collect();

        // ("g", "") and ("x", "") are equivalent: only the uncoupled names are compared.
        assert_eq!(
            order,
            vec![(1, 2, "g", ""), (3, 4, "g", ""), (3, 4, "g", "u")]
        );
        assert_eq!(entries[1].1.items_p.len(), 1);
    }

    /// Two parallel horizontal segments: the facing parts.
    #[test]
    fn parallel_projection_seg() {
        let p = Seg::new(Vector2I::new(0, 0), Vector2I::new(1000, 0));
        let n = Seg::new(Vector2I::new(500, 100), Vector2I::new(1500, 100));
        let (pc, nc) = common_parallel_projection_seg(p, n).unwrap();
        assert_eq!(
            (pc.a, pc.b),
            (Vector2I::new(500, 0), Vector2I::new(1000, 0))
        );
        assert_eq!(
            (nc.a, nc.b),
            (Vector2I::new(500, 100), Vector2I::new(1000, 100))
        );

        let far = Seg::new(Vector2I::new(1000, 100), Vector2I::new(2000, 100));
        assert!(common_parallel_projection_seg(p, far).is_none());
    }
}
