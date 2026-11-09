//! Horizontal edges and the joins they create.

use super::ClipperBase;
use super::ael::top_x;
use super::types::*;
use crate::core::Point64;

impl ClipperBase {
    fn get_curr_y_maxima_vertex_open(&self, e: Idx) -> Idx {
        let mut result = self.e(e).vertex_top;
        if self.e(e).wind_dx > 0 {
            while self.v(self.v(result).next).pt.y == self.v(result).pt.y
                && self.v(result).flags & (VF_OPEN_END | VF_LOCAL_MAX) == 0
            {
                result = self.v(result).next;
            }
        } else {
            while self.v(self.v(result).prev).pt.y == self.v(result).pt.y
                && self.v(result).flags & (VF_OPEN_END | VF_LOCAL_MAX) == 0
            {
                result = self.v(result).prev;
            }
        }
        if !self.is_maxima_v(result) {
            NONE
        } else {
            result
        }
    }

    fn get_curr_y_maxima_vertex(&self, e: Idx) -> Idx {
        let mut result = self.e(e).vertex_top;
        if self.e(e).wind_dx > 0 {
            while self.v(self.v(result).next).pt.y == self.v(result).pt.y {
                result = self.v(result).next;
            }
        } else {
            while self.v(self.v(result).prev).pt.y == self.v(result).pt.y {
                result = self.v(result).prev;
            }
        }
        if !self.is_maxima_v(result) {
            NONE
        } else {
            result
        }
    }

    pub(crate) fn get_maxima_pair(&self, e: Idx) -> Idx {
        let vt = self.e(e).vertex_top;
        let mut e2 = self.links.next_ael[e as usize];
        while e2 != NONE {
            if self.e(e2).vertex_top == vt {
                return e2;
            }
            e2 = self.links.next_ael[e2 as usize];
        }
        NONE
    }

    fn get_last_op(&self, hot_edge: Idx) -> Idx {
        let outrec = self.e(hot_edge).outrec;
        let result = self.or(outrec).pts;
        if hot_edge != self.or(outrec).front_edge {
            self.op(result).next
        } else {
            result
        }
    }

    fn add_trial_horz_join(&mut self, op: Idx) {
        if self.or(self.op(op).outrec).is_open {
            return;
        }
        self.horz_seg_list.push(HorzSegment {
            left_op: op,
            right_op: NONE,
            left_to_right: true,
        });
    }

    /// Returns `(is_left_to_right, horz_left, horz_right)`.
    fn reset_horz_direction(&self, horz: Idx, max_vertex: Idx) -> (bool, i64, i64) {
        let h = self.e(horz);
        if h.bot.x == h.top.x {
            let mut e = self.links.next_ael[horz as usize];
            while e != NONE && self.e(e).vertex_top != max_vertex {
                e = self.links.next_ael[e as usize];
            }
            (e != NONE, h.curr_x, h.curr_x)
        } else if h.curr_x < h.top.x {
            (true, h.curr_x, h.top.x)
        } else {
            (false, h.top.x, h.curr_x)
        }
    }

    /// `DoHorizontal`: horizontals at a scanline are processed as if layered.
    pub(crate) fn do_horizontal(&mut self, horz: Idx) {
        let horz_is_open = self.is_open(horz);
        let y = self.e(horz).bot.y;
        let vertex_max = if horz_is_open {
            self.get_curr_y_maxima_vertex_open(horz)
        } else {
            self.get_curr_y_maxima_vertex(horz)
        };
        let (mut is_left_to_right, mut horz_left, mut horz_right) =
            self.reset_horz_direction(horz, vertex_max);

        if self.is_hot(horz) {
            let h = self.e(horz);
            let pt = h.bot.with_xy(h.curr_x, y);
            let op = self.add_out_pt(horz, pt);
            self.add_trial_horz_join(op);
        }

        loop {
            let mut e = if is_left_to_right {
                self.links.next_ael[horz as usize]
            } else {
                self.links.prev_ael[horz as usize]
            };
            while e != NONE {
                if self.e(e).vertex_top == vertex_max {
                    if self.is_hot(horz) && self.is_joined(e) {
                        let top = self.e(e).top;
                        self.split(e, top);
                    }
                    if self.is_hot(horz) {
                        while self.e(horz).vertex_top != vertex_max {
                            let top = self.e(horz).top;
                            self.add_out_pt(horz, top);
                            self.update_edge_into_ael(horz);
                        }
                        let top = self.e(horz).top;
                        if is_left_to_right {
                            self.add_local_max_poly(horz, e, top);
                        } else {
                            self.add_local_max_poly(e, horz, top);
                        }
                    }
                    self.delete_from_ael(e);
                    self.delete_from_ael(horz);
                    return;
                }

                if vertex_max != self.e(horz).vertex_top || self.is_open_end(horz) {
                    let cx = self.e(e).curr_x;
                    if (is_left_to_right && cx > horz_right)
                        || (!is_left_to_right && cx < horz_left)
                    {
                        break;
                    }
                    if cx == self.e(horz).top.x && !self.is_horizontal(e) {
                        let pt = self.v(self.next_vertex(horz)).pt;
                        let special = self.is_open(e)
                            && self.polytype(e) != self.polytype(horz)
                            && !self.is_hot(e);
                        let tx = top_x(self.e(e), pt.y);
                        if is_left_to_right {
                            if special {
                                if tx > pt.x {
                                    break;
                                }
                            } else if tx >= pt.x {
                                break;
                            }
                        } else if special {
                            if tx < pt.x {
                                break;
                            }
                        } else if tx <= pt.x {
                            break;
                        }
                    }
                }

                let pt = Point64::new(self.e(e).curr_x, self.e(horz).bot.y);
                if is_left_to_right {
                    self.intersect_edges(horz, e, pt);
                    self.swap_positions_in_ael(horz, e);
                    self.check_join_left(e, pt, false);
                    self.em(horz).curr_x = self.e(e).curr_x;
                    e = self.links.next_ael[horz as usize];
                } else {
                    self.intersect_edges(e, horz, pt);
                    self.swap_positions_in_ael(e, horz);
                    self.check_join_right(e, pt, false);
                    self.em(horz).curr_x = self.e(e).curr_x;
                    e = self.links.prev_ael[horz as usize];
                }
                if self.e(horz).outrec != NONE {
                    let op = self.get_last_op(horz);
                    self.add_trial_horz_join(op);
                }
            }

            if horz_is_open && self.is_open_end(horz) {
                if self.is_hot(horz) {
                    let top = self.e(horz).top;
                    self.add_out_pt(horz, top);
                    let outrec = self.e(horz).outrec;
                    if self.is_front(horz) {
                        self.orm(outrec).front_edge = NONE;
                    } else {
                        self.orm(outrec).back_edge = NONE;
                    }
                    self.em(horz).outrec = NONE;
                }
                self.delete_from_ael(horz);
                return;
            } else if self.v(self.next_vertex(horz)).pt.y != self.e(horz).top.y {
                break;
            }

            if self.is_hot(horz) {
                let top = self.e(horz).top;
                self.add_out_pt(horz, top);
            }
            self.update_edge_into_ael(horz);
            (is_left_to_right, horz_left, horz_right) = self.reset_horz_direction(horz, vertex_max);
        }

        if self.is_hot(horz) {
            let top = self.e(horz).top;
            let op = self.add_out_pt(horz, top);
            self.add_trial_horz_join(op);
        }
        self.update_edge_into_ael(horz);
    }

    fn set_horz_seg_heading_forward(&self, hs: &mut HorzSegment, op_p: Idx, op_n: Idx) -> bool {
        let (p, n) = (self.op(op_p).pt, self.op(op_n).pt);
        if p.x == n.x {
            return false;
        }
        if p.x < n.x {
            hs.left_op = op_p;
            hs.right_op = op_n;
            hs.left_to_right = true;
        } else {
            hs.left_op = op_n;
            hs.right_op = op_p;
            hs.left_to_right = false;
        }
        true
    }

    fn update_horz_segment(&mut self, i: usize) -> bool {
        let mut hs = self.horz_seg_list[i];
        let op = hs.left_op;
        let outrec = self.get_real_out_rec(self.op(op).outrec);
        let outrec_has_edges = self.or(outrec).front_edge != NONE;
        let curr_y = self.op(op).pt.y;
        let (mut op_p, mut op_n) = (op, op);
        if outrec_has_edges {
            let op_a = self.or(outrec).pts;
            let op_z = self.op(op_a).next;
            while op_p != op_z && self.op(self.op(op_p).prev).pt.y == curr_y {
                op_p = self.op(op_p).prev;
            }
            while op_n != op_a && self.op(self.op(op_n).next).pt.y == curr_y {
                op_n = self.op(op_n).next;
            }
        } else {
            while self.op(op_p).prev != op_n && self.op(self.op(op_p).prev).pt.y == curr_y {
                op_p = self.op(op_p).prev;
            }
            while self.op(op_n).next != op_p && self.op(self.op(op_n).next).pt.y == curr_y {
                op_n = self.op(op_n).next;
            }
        }
        let result = self.set_horz_seg_heading_forward(&mut hs, op_p, op_n)
            && self.op(hs.left_op).horz == NONE;
        if result {
            self.opm(hs.left_op).horz = i as Idx;
        } else {
            hs.right_op = NONE; // for sorting
        }
        self.horz_seg_list[i] = hs;
        result
    }

    pub(crate) fn convert_horz_segs_to_joins(&mut self) {
        let mut j = 0;
        for i in 0..self.horz_seg_list.len() {
            if self.update_horz_segment(i) {
                j += 1;
            }
        }
        if j < 2 {
            return;
        }
        // HorzSegSorter: segments with a right_op first, then by left x.
        let outpts = &self.outpts;
        self.horz_seg_list.sort_by(|a, b| {
            let less = |h1: &HorzSegment, h2: &HorzSegment| {
                if h1.right_op == NONE || h2.right_op == NONE {
                    h1.right_op != NONE
                } else {
                    outpts[h2.left_op as usize].pt.x > outpts[h1.left_op as usize].pt.x
                }
            };
            if less(a, b) {
                std::cmp::Ordering::Less
            } else if less(b, a) {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });

        for i1 in 0..j - 1 {
            for i2 in i1 + 1..j {
                let (hs1, hs2) = (self.horz_seg_list[i1], self.horz_seg_list[i2]);
                if self.op(hs2.left_op).pt.x >= self.op(hs1.right_op).pt.x
                    || hs2.left_to_right == hs1.left_to_right
                    || self.op(hs2.right_op).pt.x <= self.op(hs1.left_op).pt.x
                {
                    continue;
                }
                let curr_y = self.op(hs1.left_op).pt.y;
                let (mut l1, mut l2) = (hs1.left_op, hs2.left_op);
                if hs1.left_to_right {
                    while self.op(self.op(l1).next).pt.y == curr_y
                        && self.op(self.op(l1).next).pt.x <= self.op(l2).pt.x
                    {
                        l1 = self.op(l1).next;
                    }
                    while self.op(self.op(l2).prev).pt.y == curr_y
                        && self.op(self.op(l2).prev).pt.x <= self.op(l1).pt.x
                    {
                        l2 = self.op(l2).prev;
                    }
                    self.horz_seg_list[i1].left_op = l1;
                    self.horz_seg_list[i2].left_op = l2;
                    let op1 = self.duplicate_op(l1, true);
                    let op2 = self.duplicate_op(l2, false);
                    self.horz_join_list.push(HorzJoin { op1, op2 });
                } else {
                    while self.op(self.op(l1).prev).pt.y == curr_y
                        && self.op(self.op(l1).prev).pt.x <= self.op(l2).pt.x
                    {
                        l1 = self.op(l1).prev;
                    }
                    while self.op(self.op(l2).next).pt.y == curr_y
                        && self.op(self.op(l2).next).pt.x <= self.op(l1).pt.x
                    {
                        l2 = self.op(l2).next;
                    }
                    self.horz_seg_list[i1].left_op = l1;
                    self.horz_seg_list[i2].left_op = l2;
                    let op1 = self.duplicate_op(l2, true);
                    let op2 = self.duplicate_op(l1, false);
                    self.horz_join_list.push(HorzJoin { op1, op2 });
                }
            }
        }
    }

    fn fix_out_rec_pts(&mut self, outrec: Idx) {
        let start = self.or(outrec).pts;
        let mut op = start;
        loop {
            self.opm(op).outrec = outrec;
            op = self.op(op).next;
            if op == start {
                break;
            }
        }
    }

    fn move_splits(&mut self, from_or: Idx, to_or: Idx) {
        let from = self.orm(from_or).splits.take().unwrap_or_default();
        let to = self.orm(to_or).splits.get_or_insert_with(Vec::new);
        to.extend(from.iter().copied().filter(|&s| s != to_or));
        self.orm(from_or).splits = Some(Vec::new());
    }

    pub(crate) fn process_horz_joins(&mut self) {
        for k in 0..self.horz_join_list.len() {
            let j = self.horz_join_list[k];
            let or1 = self.get_real_out_rec(self.op(j.op1).outrec);
            let mut or2 = self.get_real_out_rec(self.op(j.op2).outrec);
            let op1b = self.op(j.op1).next;
            let op2b = self.op(j.op2).prev;
            self.opm(j.op1).next = j.op2;
            self.opm(j.op2).prev = j.op1;
            self.opm(op1b).prev = op2b;
            self.opm(op2b).next = op1b;

            if or1 == or2 {
                // the 'join' is really a split
                or2 = self.new_out_rec();
                self.orm(or2).pts = op1b;
                self.fix_out_rec_pts(or2);
                if self.op(self.or(or1).pts).outrec == or2 {
                    self.orm(or1).pts = j.op1;
                    self.opm(j.op1).outrec = or1;
                }
                if self.using_polytree {
                    if self.path2_contains_path1_op(self.or(or1).pts, self.or(or2).pts) {
                        let (p1, p2) = (self.or(or1).pts, self.or(or2).pts);
                        self.orm(or1).pts = p2;
                        self.orm(or2).pts = p1;
                        self.fix_out_rec_pts(or1);
                        self.fix_out_rec_pts(or2);
                        self.orm(or2).owner = or1;
                    } else if self.path2_contains_path1_op(self.or(or2).pts, self.or(or1).pts) {
                        self.orm(or2).owner = or1;
                    } else {
                        let o = self.or(or1).owner;
                        self.orm(or2).owner = o;
                    }
                    self.orm(or1).splits.get_or_insert_with(Vec::new).push(or2);
                } else {
                    self.orm(or2).owner = or1;
                }
            } else {
                self.orm(or2).pts = NONE;
                if self.using_polytree {
                    self.set_owner(or2, or1);
                    if self.or(or2).splits.is_some() {
                        self.move_splits(or2, or1);
                    }
                } else {
                    self.orm(or2).owner = or1;
                }
            }
        }
    }
}
