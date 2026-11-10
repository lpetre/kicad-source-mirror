//! Edge intersections within a scanbeam.

use super::ClipperBase;
use super::ael::top_x;
use super::types::*;
use crate::core::{
    ClipType, FillRule, Point64, get_closest_point_on_segment, get_line_intersect_pt,
};

impl ClipperBase {
    fn find_edge_with_matching_loc_min(&self, e: Idx) -> Idx {
        let lm = self.e(e).local_min;
        let bot = self.e(e).bot;
        let mut result = self.links.next_ael[e as usize];
        while result != NONE {
            if self.e(result).local_min == lm {
                return result;
            } else if !self.is_horizontal(result) && bot != self.e(result).bot {
                result = NONE;
            } else {
                result = self.links.next_ael[result as usize];
            }
        }
        result = self.links.prev_ael[e as usize];
        while result != NONE {
            if self.e(result).local_min == lm {
                return result;
            } else if !self.is_horizontal(result) && bot != self.e(result).bot {
                return NONE;
            } else {
                result = self.links.prev_ael[result as usize];
            }
        }
        result
    }

    /// `IntersectEdges`.
    pub(crate) fn intersect_edges(&mut self, e1: Idx, e2: Idx, pt: Point64) {
        if self.has_open_paths && (self.is_open(e1) || self.is_open(e2)) {
            self.intersect_open_edges(e1, e2, pt);
            return;
        }
        if self.is_joined(e1) {
            self.split(e1, pt);
        }
        if self.is_joined(e2) {
            self.split(e2, pt);
        }

        if self.polytype(e1) == self.polytype(e2) {
            if self.fillrule == FillRule::EvenOdd {
                let w1 = self.e(e1).wind_cnt;
                let w2 = self.e(e2).wind_cnt;
                self.em(e1).wind_cnt = w2;
                self.em(e2).wind_cnt = w1;
            } else {
                let (a, b) = (*self.e(e1), *self.e(e2));
                self.em(e1).wind_cnt = if a.wind_cnt + b.wind_dx == 0 {
                    -a.wind_cnt
                } else {
                    a.wind_cnt + b.wind_dx
                };
                self.em(e2).wind_cnt = if b.wind_cnt - a.wind_dx == 0 {
                    -b.wind_cnt
                } else {
                    b.wind_cnt - a.wind_dx
                };
            }
        } else if self.fillrule != FillRule::EvenOdd {
            let (d1, d2) = (self.e(e1).wind_dx, self.e(e2).wind_dx);
            self.em(e1).wind_cnt2 += d2;
            self.em(e2).wind_cnt2 -= d1;
        } else {
            let a = self.em(e1);
            a.wind_cnt2 = if a.wind_cnt2 == 0 { 1 } else { 0 };
            let b = self.em(e2);
            b.wind_cnt2 = if b.wind_cnt2 == 0 { 1 } else { 0 };
        }

        let (w1, w2) = (self.e(e1).wind_cnt, self.e(e2).wind_cnt);
        let (old_e1_windcnt, old_e2_windcnt) = match self.fillrule {
            FillRule::EvenOdd | FillRule::NonZero => (w1.abs(), w2.abs()),
            FillRule::Positive => (w1, w2),
            FillRule::Negative => (-w1, -w2),
        };

        let e1_in_01 = old_e1_windcnt == 0 || old_e1_windcnt == 1;
        let e2_in_01 = old_e2_windcnt == 0 || old_e2_windcnt == 1;
        if (!self.is_hot(e1) && !e1_in_01) || (!self.is_hot(e2) && !e2_in_01) {
            return;
        }

        if self.is_hot(e1) && self.is_hot(e2) {
            if (old_e1_windcnt != 0 && old_e1_windcnt != 1)
                || (old_e2_windcnt != 0 && old_e2_windcnt != 1)
                || (self.polytype(e1) != self.polytype(e2) && self.cliptype != ClipType::Xor)
            {
                let _r = self.add_local_max_poly(e1, e2, pt);
                #[cfg(feature = "usingz")]
                if _r != NONE {
                    self.set_z(e1, e2, _r);
                }
            } else if self.is_front(e1) || self.e(e1).outrec == self.e(e2).outrec {
                let _r = self.add_local_max_poly(e1, e2, pt);
                let _op2 = self.add_local_min_poly(e1, e2, pt, false);
                #[cfg(feature = "usingz")]
                {
                    if _r != NONE {
                        self.set_z(e1, e2, _r);
                    }
                    self.set_z(e1, e2, _op2);
                }
            } else {
                let _r = self.add_out_pt(e1, pt);
                let _op2 = self.add_out_pt(e2, pt);
                #[cfg(feature = "usingz")]
                {
                    self.set_z(e1, e2, _r);
                    self.set_z(e1, e2, _op2);
                }
                self.swap_outrecs(e1, e2);
            }
        } else if self.is_hot(e1) {
            let _r = self.add_out_pt(e1, pt);
            #[cfg(feature = "usingz")]
            self.set_z(e1, e2, _r);
            self.swap_outrecs(e1, e2);
        } else if self.is_hot(e2) {
            let _r = self.add_out_pt(e2, pt);
            #[cfg(feature = "usingz")]
            self.set_z(e1, e2, _r);
            self.swap_outrecs(e1, e2);
        } else {
            let (c1, c2) = (self.e(e1).wind_cnt2 as i64, self.e(e2).wind_cnt2 as i64);
            let (e1_wc2, e2_wc2) = match self.fillrule {
                FillRule::EvenOdd | FillRule::NonZero => (c1.abs(), c2.abs()),
                FillRule::Positive => (c1, c2),
                FillRule::Negative => (-c1, -c2),
            };
            if self.polytype(e1) != self.polytype(e2) {
                let _r = self.add_local_min_poly(e1, e2, pt, false);
                #[cfg(feature = "usingz")]
                self.set_z(e1, e2, _r);
            } else if old_e1_windcnt == 1 && old_e2_windcnt == 1 {
                let add = match self.cliptype {
                    ClipType::Union => e1_wc2 <= 0 && e2_wc2 <= 0,
                    ClipType::Difference => {
                        (self.polytype(e1) == PathType::Clip && e1_wc2 > 0 && e2_wc2 > 0)
                            || (self.polytype(e1) == PathType::Subject
                                && e1_wc2 <= 0
                                && e2_wc2 <= 0)
                    }
                    ClipType::Xor => true,
                    _ => e1_wc2 > 0 && e2_wc2 > 0,
                };
                if add {
                    let _r = self.add_local_min_poly(e1, e2, pt, false);
                    #[cfg(feature = "usingz")]
                    self.set_z(e1, e2, _r);
                }
            }
        }
    }

    /// The open-path branch of `IntersectEdges`.
    fn intersect_open_edges(&mut self, e1: Idx, e2: Idx, pt: Point64) {
        if self.is_open(e1) && self.is_open(e2) {
            return;
        }
        let (edge_o, edge_c) = if self.is_open(e1) { (e1, e2) } else { (e2, e1) };
        if self.is_joined(edge_c) {
            self.split(edge_c, pt);
        }
        if self.e(edge_c).wind_cnt.abs() != 1 {
            return;
        }
        match self.cliptype {
            ClipType::Union => {
                if !self.is_hot(edge_c) {
                    return;
                }
            }
            _ => {
                if self.polytype(edge_c) == PathType::Subject {
                    return;
                }
            }
        }
        let wc = self.e(edge_c).wind_cnt;
        match self.fillrule {
            FillRule::Positive => {
                if wc != 1 {
                    return;
                }
            }
            FillRule::Negative => {
                if wc != -1 {
                    return;
                }
            }
            _ => {
                if wc.abs() != 1 {
                    return;
                }
            }
        }

        let _result_op;
        if self.is_hot(edge_o) {
            _result_op = self.add_out_pt(edge_o, pt);
            let outrec = self.e(edge_o).outrec;
            if self.is_front(edge_o) {
                self.orm(outrec).front_edge = NONE;
            } else {
                self.orm(outrec).back_edge = NONE;
            }
            self.em(edge_o).outrec = NONE;
        } else {
            let lm_vertex = self.minima[self.e(edge_o).local_min as usize].vertex;
            if pt == self.v(lm_vertex).pt && !self.is_open_end_v(lm_vertex) {
                let e3 = self.find_edge_with_matching_loc_min(edge_o);
                if e3 != NONE && self.is_hot(e3) {
                    let outrec = self.e(e3).outrec;
                    self.em(edge_o).outrec = outrec;
                    if self.e(edge_o).wind_dx > 0 {
                        self.set_sides(outrec, edge_o, e3);
                    } else {
                        self.set_sides(outrec, e3, edge_o);
                    }
                    return;
                }
                _result_op = self.start_open_path(edge_o, pt);
            } else {
                _result_op = self.start_open_path(edge_o, pt);
            }
        }
        #[cfg(feature = "usingz")]
        self.set_z(edge_o, edge_c, _result_op);
    }

    pub(crate) fn do_intersections(&mut self, top_y: i64) {
        self.curr_x_at_top = false;
        if self.build_intersect_list(top_y) {
            self.process_intersect_list(top_y);
            self.intersect_nodes.clear();
        }
    }

    fn add_new_intersect_node(&mut self, e1: Idx, e2: Idx, top_y: i64) {
        let (a, b) = (*self.e(e1), *self.e(e2));
        let mut ip = get_line_intersect_pt(a.bot, a.top, b.bot, b.top)
            .unwrap_or_else(|| Point64::new(a.curr_x, top_y));
        if ip.y > self.bot_y || ip.y < top_y {
            let abs_dx1 = a.dx.abs();
            let abs_dx2 = b.dx.abs();
            if abs_dx1 > 100.0 && abs_dx2 > 100.0 {
                ip = if abs_dx1 > abs_dx2 {
                    get_closest_point_on_segment(ip, a.bot, a.top)
                } else {
                    get_closest_point_on_segment(ip, b.bot, b.top)
                };
            } else if abs_dx1 > 100.0 {
                ip = get_closest_point_on_segment(ip, a.bot, a.top);
            } else if abs_dx2 > 100.0 {
                ip = get_closest_point_on_segment(ip, b.bot, b.top);
            } else {
                ip.y = if ip.y < top_y { top_y } else { self.bot_y };
                ip.x = if abs_dx1 < abs_dx2 {
                    top_x(&a, ip.y)
                } else {
                    top_x(&b, ip.y)
                };
            }
        }
        self.intersect_nodes.push(IntersectNode {
            pt: ip,
            edge1: e1,
            edge2: e2,
        });
    }

    fn extract_from_sel(&mut self, ae: Idx) -> Idx {
        let res = self.links.next_sel[ae as usize];
        let prev = self.links.prev_sel[ae as usize];
        if res != NONE {
            self.links.prev_sel[res as usize] = prev;
        }
        self.links.next_sel[prev as usize] = res;
        res
    }

    fn insert1_before2_in_sel(&mut self, ae1: Idx, ae2: Idx) {
        let prev = self.links.prev_sel[ae2 as usize];
        self.links.prev_sel[ae1 as usize] = prev;
        if prev != NONE {
            self.links.next_sel[prev as usize] = ae1;
        }
        self.links.next_sel[ae1 as usize] = ae2;
        self.links.prev_sel[ae2 as usize] = ae1;
    }

    /// `BuildIntersectList`: a merge sort of the edges by their x at `top_y`; every swap is
    /// an intersection.
    fn build_intersect_list(&mut self, top_y: i64) -> bool {
        if self.actives == NONE || self.links.next_ael[self.actives as usize] == NONE {
            return false;
        }
        if self.adjust_curr_x_and_copy_to_sel(top_y) {
            self.curr_x_at_top = true;
            return false;
        }
        let mut left = self.sel;
        while left != NONE && self.links.jump[left as usize] != NONE {
            let mut prev_base = NONE;
            while left != NONE && self.links.jump[left as usize] != NONE {
                let mut curr_base = left;
                let mut right = self.links.jump[left as usize];
                let mut l_end = right;
                let r_end = self.links.jump[right as usize];
                self.links.jump[left as usize] = r_end;
                // Both runs are sorted: if the left run's last edge is not past the right run's
                // first, the merge below would neither report a crossing nor move an edge.
                if self.e(right).curr_x >= self.e(self.links.prev_sel[right as usize]).curr_x {
                    left = l_end;
                }
                while left != l_end && right != r_end {
                    if self.e(right).curr_x < self.e(left).curr_x {
                        let mut tmp = self.links.prev_sel[right as usize];
                        loop {
                            self.add_new_intersect_node(tmp, right, top_y);
                            if tmp == left {
                                break;
                            }
                            tmp = self.links.prev_sel[tmp as usize];
                        }
                        tmp = right;
                        right = self.extract_from_sel(tmp);
                        l_end = right;
                        self.insert1_before2_in_sel(tmp, left);
                        if left == curr_base {
                            curr_base = tmp;
                            self.links.jump[curr_base as usize] = r_end;
                            if prev_base == NONE {
                                self.sel = curr_base;
                            } else {
                                self.links.jump[prev_base as usize] = curr_base;
                            }
                        }
                    } else {
                        left = self.links.next_sel[left as usize];
                    }
                }
                prev_base = curr_base;
                left = r_end;
            }
            left = self.sel;
        }
        !self.intersect_nodes.is_empty()
    }

    fn edges_adjacent_in_ael(&self, node: &IntersectNode) -> bool {
        self.links.next_ael[node.edge1 as usize] == node.edge2
            || self.links.prev_ael[node.edge1 as usize] == node.edge2
    }

    fn process_intersect_list(&mut self, top_y: i64) {
        // IntersectListSort: higher y first, then smaller x.  KiCad port patch: sorted like
        // libstdc++ std::sort (unstable) instead of a stable sort.
        crate::gnu_sort::gnu_sort(&mut self.intersect_nodes, |a, b| {
            if a.pt.y == b.pt.y {
                a.pt.x < b.pt.x
            } else {
                a.pt.y > b.pt.y
            }
        });
        for i in 0..self.intersect_nodes.len() {
            if !self.edges_adjacent_in_ael(&self.intersect_nodes[i]) {
                let mut j = i + 1;
                // Upstream reads past the end if no later node is adjacent.
                while j < self.intersect_nodes.len()
                    && !self.edges_adjacent_in_ael(&self.intersect_nodes[j])
                {
                    j += 1;
                }
                if j == self.intersect_nodes.len() {
                    self.succeeded = false;
                    return;
                }
                self.intersect_nodes.swap(i, j);
            }
            let node = self.intersect_nodes[i];
            self.intersect_edges(node.edge1, node.edge2, node.pt);
            self.swap_positions_in_ael(node.edge1, node.edge2);
            let (pos1, pos2) = (self.e(node.edge1).ael_pos, self.e(node.edge2).ael_pos);
            self.em(node.edge1).ael_pos = pos2;
            self.em(node.edge2).ael_pos = pos1;
            self.em(node.edge1).curr_x = node.pt.x;
            self.em(node.edge2).curr_x = node.pt.x;
            self.check_join_left(node.edge2, node.pt, true);
            self.check_join_right(node.edge1, node.pt, true);
        }
        // Only the edges that crossed have left the position `adjust_curr_x_and_copy_to_sel`
        // gave them, and only they have changed rank.
        for i in 0..self.intersect_nodes.len() {
            let node = self.intersect_nodes[i];
            for e in [node.edge1, node.edge2] {
                let x = top_x(self.e(e), top_y);
                self.em(e).curr_x = x;
            }
        }
        let edges = &self.edges;
        self.edges_ending
            .sort_unstable_by_key(|&e| edges[e as usize].ael_pos);
        self.curr_x_at_top = true;
    }
}
