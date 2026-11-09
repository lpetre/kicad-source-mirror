//! Output records: building, joining, splitting and cleaning the result polygons.

use super::ClipperBase;
use super::types::*;
use crate::core::{
    Path64, Point64, PointInPolygonResult, Rect64, cross_product_sign_t, dot_product, get_bounds,
    get_line_intersect_pt, is_collinear, perpendic_dist_from_line_sqrd, point_in_polygon,
    segments_intersect,
};

/// `PtsReallyClose`.
#[inline]
fn pts_really_close(a: Point64, b: Point64) -> bool {
    a.x.wrapping_sub(b.x).abs() < 2 && a.y.wrapping_sub(b.y).abs() < 2
}

/// `AreaTriangle`.
#[inline]
fn area_triangle(pt1: Point64, pt2: Point64, pt3: Point64) -> f64 {
    (pt3.y.wrapping_add(pt1.y)) as f64 * (pt3.x.wrapping_sub(pt1.x)) as f64
        + (pt1.y.wrapping_add(pt2.y)) as f64 * (pt1.x.wrapping_sub(pt2.x)) as f64
        + (pt2.y.wrapping_add(pt3.y)) as f64 * (pt2.x.wrapping_sub(pt3.x)) as f64
}

/// `Path2ContainsPath1` for plain paths (`clipper.h`).
pub(crate) fn path2_contains_path1(path1: &Path64, path2: &Path64) -> bool {
    let mut pip = PointInPolygonResult::IsOn;
    for &pt in path1 {
        match point_in_polygon(pt, path2) {
            PointInPolygonResult::IsOutside => {
                if pip == PointInPolygonResult::IsOutside {
                    return false;
                }
                pip = PointInPolygonResult::IsOutside;
            }
            PointInPolygonResult::IsInside => {
                if pip == PointInPolygonResult::IsInside {
                    return true;
                }
                pip = PointInPolygonResult::IsInside;
            }
            PointInPolygonResult::IsOn => {}
        }
    }
    if pip != PointInPolygonResult::IsInside {
        return false;
    }
    let mp1 = get_bounds(path1).mid_point();
    point_in_polygon(mp1, path2) == PointInPolygonResult::IsInside
}

impl ClipperBase {
    pub(crate) fn new_op(&mut self, pt: Point64, outrec: Idx) -> Idx {
        let i = self.outpts.len() as Idx;
        self.outpts.push(OutPt {
            pt,
            next: i,
            prev: i,
            outrec,
            horz: NONE,
        });
        i
    }

    pub(crate) fn new_out_rec(&mut self) -> Idx {
        let i = self.outrecs.len();
        self.outrecs.push(OutRec {
            idx: i,
            owner: NONE,
            front_edge: NONE,
            back_edge: NONE,
            pts: NONE,
            polypath: NONE,
            splits: None,
            recursive_split: NONE,
            bounds: Rect64::default(),
            path: Vec::new(),
            is_open: false,
        });
        i as Idx
    }

    pub(crate) fn duplicate_op(&mut self, op: Idx, insert_after: bool) -> Idx {
        let (pt, outrec) = (self.op(op).pt, self.op(op).outrec);
        let result = self.new_op(pt, outrec);
        if insert_after {
            let next = self.op(op).next;
            self.opm(result).next = next;
            self.opm(next).prev = result;
            self.opm(result).prev = op;
            self.opm(op).next = result;
        } else {
            let prev = self.op(op).prev;
            self.opm(result).prev = prev;
            self.opm(prev).next = result;
            self.opm(result).next = op;
            self.opm(op).prev = result;
        }
        result
    }

    pub(crate) fn dispose_out_pt(&mut self, op: Idx) -> Idx {
        let (prev, next) = (self.op(op).prev, self.op(op).next);
        self.opm(prev).next = next;
        self.opm(next).prev = prev;
        next
    }

    #[inline]
    pub(crate) fn dispose_out_pts(&mut self, outrec: Idx) {
        self.orm(outrec).pts = NONE;
    }

    pub(crate) fn area_op(&self, op: Idx) -> f64 {
        let mut result = 0.0;
        let mut op2 = op;
        loop {
            let (p, c) = (self.op(self.op(op2).prev).pt, self.op(op2).pt);
            result += (p.y.wrapping_add(c.y)) as f64 * (p.x.wrapping_sub(c.x)) as f64;
            op2 = self.op(op2).next;
            if op2 == op {
                break;
            }
        }
        result * 0.5
    }

    pub(crate) fn is_very_small_triangle(&self, op: Idx) -> bool {
        let o = self.op(op);
        let (prev, next) = (self.op(o.prev), self.op(o.next));
        next.next == o.prev
            && (pts_really_close(prev.pt, next.pt)
                || pts_really_close(o.pt, next.pt)
                || pts_really_close(o.pt, prev.pt))
    }

    pub(crate) fn is_valid_closed_path(&self, op: Idx) -> bool {
        op != NONE
            && self.op(op).next != op
            && self.op(op).next != self.op(op).prev
            && !self.is_very_small_triangle(op)
    }

    pub(crate) fn get_real_out_rec(&self, mut outrec: Idx) -> Idx {
        while outrec != NONE && self.or(outrec).pts == NONE {
            outrec = self.or(outrec).owner;
        }
        outrec
    }

    pub(crate) fn is_valid_owner(&self, outrec: Idx, mut test_owner: Idx) -> bool {
        while test_owner != NONE && test_owner != outrec {
            test_owner = self.or(test_owner).owner;
        }
        test_owner == NONE
    }

    pub(crate) fn set_owner(&mut self, outrec: Idx, new_owner: Idx) {
        let real = self.get_real_out_rec(self.or(new_owner).owner);
        self.orm(new_owner).owner = real;
        let mut tmp = new_owner;
        while tmp != NONE && tmp != outrec {
            tmp = self.or(tmp).owner;
        }
        if tmp != NONE {
            let o = self.or(outrec).owner;
            self.orm(new_owner).owner = o;
        }
        self.orm(outrec).owner = new_owner;
    }

    #[inline]
    pub(crate) fn set_sides(&mut self, outrec: Idx, start_edge: Idx, end_edge: Idx) {
        let o = self.orm(outrec);
        o.front_edge = start_edge;
        o.back_edge = end_edge;
    }

    pub(crate) fn swap_outrecs(&mut self, e1: Idx, e2: Idx) {
        let (or1, or2) = (self.e(e1).outrec, self.e(e2).outrec);
        if or1 == or2 {
            let o = self.orm(or1);
            std::mem::swap(&mut o.front_edge, &mut o.back_edge);
            return;
        }
        if or1 != NONE {
            if e1 == self.or(or1).front_edge {
                self.orm(or1).front_edge = e2;
            } else {
                self.orm(or1).back_edge = e2;
            }
        }
        if or2 != NONE {
            if e2 == self.or(or2).front_edge {
                self.orm(or2).front_edge = e1;
            } else {
                self.orm(or2).back_edge = e1;
            }
        }
        self.em(e1).outrec = or2;
        self.em(e2).outrec = or1;
    }

    /// `SwapFrontBackSides` / `SwapSides`.
    pub(crate) fn swap_front_back_sides(&mut self, outrec: Idx) {
        let pts = self.or(outrec).pts;
        let next = self.op(pts).next;
        let o = self.orm(outrec);
        std::mem::swap(&mut o.front_edge, &mut o.back_edge);
        o.pts = next;
    }

    pub(crate) fn uncouple_out_rec(&mut self, e: Idx) {
        let outrec = self.e(e).outrec;
        if outrec == NONE {
            return;
        }
        let (fe, be) = (self.or(outrec).front_edge, self.or(outrec).back_edge);
        self.em(fe).outrec = NONE;
        self.em(be).outrec = NONE;
        let o = self.orm(outrec);
        o.front_edge = NONE;
        o.back_edge = NONE;
    }

    #[inline]
    pub(crate) fn outrec_is_ascending(&self, hot_edge: Idx) -> bool {
        hot_edge == self.or(self.e(hot_edge).outrec).front_edge
    }

    pub(crate) fn add_local_min_poly(
        &mut self,
        e1: Idx,
        e2: Idx,
        pt: Point64,
        is_new: bool,
    ) -> Idx {
        let outrec = self.new_out_rec();
        self.em(e1).outrec = outrec;
        self.em(e2).outrec = outrec;
        if self.is_open(e1) {
            self.orm(outrec).owner = NONE;
            self.orm(outrec).is_open = true;
            if self.e(e1).wind_dx > 0 {
                self.set_sides(outrec, e1, e2);
            } else {
                self.set_sides(outrec, e2, e1);
            }
        } else {
            let prev_hot_edge = self.get_prev_hot_edge(e1);
            if prev_hot_edge != NONE {
                if self.using_polytree {
                    let owner = self.e(prev_hot_edge).outrec;
                    self.set_owner(outrec, owner);
                }
                if self.outrec_is_ascending(prev_hot_edge) == is_new {
                    self.set_sides(outrec, e2, e1);
                } else {
                    self.set_sides(outrec, e1, e2);
                }
            } else {
                self.orm(outrec).owner = NONE;
                if is_new {
                    self.set_sides(outrec, e1, e2);
                } else {
                    self.set_sides(outrec, e2, e1);
                }
            }
        }
        let op = self.new_op(pt, outrec);
        self.orm(outrec).pts = op;
        op
    }

    /// Returns `NONE` when the operation failed (both edges on the same side).
    pub(crate) fn add_local_max_poly(&mut self, e1: Idx, e2: Idx, pt: Point64) -> Idx {
        if self.is_joined(e1) {
            self.split(e1, pt);
        }
        if self.is_joined(e2) {
            self.split(e2, pt);
        }
        if self.is_front(e1) == self.is_front(e2) {
            if self.is_open_end(e1) {
                self.swap_front_back_sides(self.e(e1).outrec);
            } else if self.is_open_end(e2) {
                self.swap_front_back_sides(self.e(e2).outrec);
            } else {
                self.succeeded = false;
                return NONE;
            }
        }
        let mut result = self.add_out_pt(e1, pt);
        if self.e(e1).outrec == self.e(e2).outrec {
            let outrec = self.e(e1).outrec;
            self.orm(outrec).pts = result;
            if self.using_polytree {
                let e = self.get_prev_hot_edge(e1);
                if e == NONE {
                    self.orm(outrec).owner = NONE;
                } else {
                    let owner = self.e(e).outrec;
                    self.set_owner(outrec, owner);
                }
            }
            self.uncouple_out_rec(e1);
            result = self.or(outrec).pts;
            let owner = self.or(outrec).owner;
            if owner != NONE && self.or(owner).front_edge == NONE {
                let real = self.get_real_out_rec(owner);
                self.orm(outrec).owner = real;
            }
        } else if self.is_open(e1) {
            if self.e(e1).wind_dx < 0 {
                self.join_outrec_paths(e1, e2);
            } else {
                self.join_outrec_paths(e2, e1);
            }
        } else if self.or(self.e(e1).outrec).idx < self.or(self.e(e2).outrec).idx {
            self.join_outrec_paths(e1, e2);
        } else {
            self.join_outrec_paths(e2, e1);
        }
        result
    }

    pub(crate) fn join_outrec_paths(&mut self, e1: Idx, e2: Idx) {
        let (or1, or2) = (self.e(e1).outrec, self.e(e2).outrec);
        let p1_st = self.or(or1).pts;
        let p2_st = self.or(or2).pts;
        let p1_end = self.op(p1_st).next;
        let p2_end = self.op(p2_st).next;
        if self.is_front(e1) {
            self.opm(p2_end).prev = p1_st;
            self.opm(p1_st).next = p2_end;
            self.opm(p2_st).next = p1_end;
            self.opm(p1_end).prev = p2_st;
            self.orm(or1).pts = p2_st;
            let fe = self.or(or2).front_edge;
            self.orm(or1).front_edge = fe;
            if fe != NONE {
                self.em(fe).outrec = or1;
            }
        } else {
            self.opm(p1_end).prev = p2_st;
            self.opm(p2_st).next = p1_end;
            self.opm(p1_st).next = p2_end;
            self.opm(p2_end).prev = p1_st;
            let be = self.or(or2).back_edge;
            self.orm(or1).back_edge = be;
            if be != NONE {
                self.em(be).outrec = or1;
            }
        }
        let o2 = self.orm(or2);
        o2.front_edge = NONE;
        o2.back_edge = NONE;
        o2.pts = NONE;
        if self.is_open_end(e1) {
            let pts = self.or(or1).pts;
            self.orm(or2).pts = pts;
            self.orm(or1).pts = NONE;
        } else {
            self.set_owner(or2, or1);
        }
        self.em(e1).outrec = NONE;
        self.em(e2).outrec = NONE;
    }

    pub(crate) fn add_out_pt(&mut self, e: Idx, pt: Point64) -> Idx {
        let outrec = self.e(e).outrec;
        let to_front = self.is_front(e);
        let op_front = self.or(outrec).pts;
        let op_back = self.op(op_front).next;
        if to_front {
            if pt == self.op(op_front).pt {
                return op_front;
            }
        } else if pt == self.op(op_back).pt {
            return op_back;
        }
        let new_op = self.new_op(pt, outrec);
        self.opm(op_back).prev = new_op;
        self.opm(new_op).prev = op_front;
        self.opm(new_op).next = op_back;
        self.opm(op_front).next = new_op;
        if to_front {
            self.orm(outrec).pts = new_op;
        }
        new_op
    }

    pub(crate) fn start_open_path(&mut self, e: Idx, pt: Point64) -> Idx {
        let outrec = self.new_out_rec();
        self.orm(outrec).is_open = true;
        if self.e(e).wind_dx > 0 {
            self.set_sides(outrec, e, NONE);
        } else {
            self.set_sides(outrec, NONE, e);
        }
        self.em(e).outrec = outrec;
        let op = self.new_op(pt, outrec);
        self.orm(outrec).pts = op;
        op
    }

    pub(crate) fn clean_collinear(&mut self, outrec: Idx) {
        let outrec = self.get_real_out_rec(outrec);
        if outrec == NONE || self.or(outrec).is_open {
            return;
        }
        if !self.is_valid_closed_path(self.or(outrec).pts) {
            self.dispose_out_pts(outrec);
            return;
        }
        let mut start_op = self.or(outrec).pts;
        let mut op2 = start_op;
        loop {
            let (prev, cur, next) = (
                self.op(self.op(op2).prev).pt,
                self.op(op2).pt,
                self.op(self.op(op2).next).pt,
            );
            if is_collinear(prev, cur, next)
                && (cur == prev
                    || cur == next
                    || !self.preserve_collinear
                    || dot_product(prev, cur, next) < 0.0)
            {
                if op2 == self.or(outrec).pts {
                    let p = self.op(op2).prev;
                    self.orm(outrec).pts = p;
                }
                op2 = self.dispose_out_pt(op2);
                if !self.is_valid_closed_path(op2) {
                    self.dispose_out_pts(outrec);
                    return;
                }
                start_op = op2;
                continue;
            }
            op2 = self.op(op2).next;
            if op2 == start_op {
                break;
            }
        }
        self.fix_self_intersects(outrec);
    }

    fn do_split_op(&mut self, outrec: Idx, split_op: Idx) {
        let prev_op = self.op(split_op).prev;
        let split_next = self.op(split_op).next;
        let next_next_op = self.op(split_next).next;
        self.orm(outrec).pts = prev_op;
        // `ip` is default-constructed (0, 0) in C++ and left unchanged for parallel segments.
        #[allow(unused_mut)]
        let mut ip = get_line_intersect_pt(
            self.op(prev_op).pt,
            self.op(split_op).pt,
            self.op(split_next).pt,
            self.op(next_next_op).pt,
        )
        .unwrap_or_default();
        #[cfg(feature = "usingz")]
        if let Some(cb) = self.z_callback.as_mut() {
            cb(
                self.outpts[prev_op as usize].pt,
                self.outpts[split_op as usize].pt,
                self.outpts[split_next as usize].pt,
                self.outpts[next_next_op as usize].pt,
                &mut ip,
            );
        }
        let area1 = self.area_op(self.or(outrec).pts);
        let abs_area1 = area1.abs();
        if abs_area1 < 2.0 {
            self.dispose_out_pts(outrec);
            return;
        }
        let area2 = area_triangle(ip, self.op(split_op).pt, self.op(split_next).pt);
        let abs_area2 = area2.abs();
        if ip == self.op(prev_op).pt || ip == self.op(next_next_op).pt {
            self.opm(next_next_op).prev = prev_op;
            self.opm(prev_op).next = next_next_op;
        } else {
            let o = self.op(prev_op).outrec;
            let new_op2 = self.new_op(ip, o);
            self.opm(new_op2).prev = prev_op;
            self.opm(new_op2).next = next_next_op;
            self.opm(next_next_op).prev = new_op2;
            self.opm(prev_op).next = new_op2;
        }
        if abs_area2 >= 1.0 && (abs_area2 > abs_area1 || (area2 > 0.0) == (area1 > 0.0)) {
            let new_or = self.new_out_rec();
            let owner = self.or(outrec).owner;
            self.orm(new_or).owner = owner;
            self.opm(split_op).outrec = new_or;
            self.opm(split_next).outrec = new_or;
            let new_op = self.new_op(ip, new_or);
            self.opm(new_op).prev = split_next;
            self.opm(new_op).next = split_op;
            self.orm(new_or).pts = new_op;
            self.opm(split_op).prev = new_op;
            self.opm(split_next).next = new_op;
            if self.using_polytree {
                if self.path2_contains_path1_op(prev_op, new_op) {
                    self.orm(new_or).splits = Some(vec![outrec]);
                } else {
                    self.orm(outrec)
                        .splits
                        .get_or_insert_with(Vec::new)
                        .push(new_or);
                }
            }
        }
    }

    pub(crate) fn fix_self_intersects(&mut self, outrec: Idx) {
        let mut op2 = self.or(outrec).pts;
        if self.op(op2).prev == self.op(self.op(op2).next).next {
            return; // triangles can't self-intersect
        }
        loop {
            let prev = self.op(op2).prev;
            let next = self.op(op2).next;
            let next_next = self.op(next).next;
            if segments_intersect(
                self.op(prev).pt,
                self.op(op2).pt,
                self.op(next).pt,
                self.op(next_next).pt,
                false,
            ) {
                let pts = self.or(outrec).pts;
                if op2 == pts || next == pts {
                    let p = self.op(pts).prev;
                    self.orm(outrec).pts = p;
                }
                self.do_split_op(outrec, op2);
                if self.or(outrec).pts == NONE {
                    break;
                }
                op2 = self.or(outrec).pts;
                if self.op(op2).prev == self.op(self.op(op2).next).next {
                    break;
                }
                continue;
            } else {
                op2 = self.op(op2).next;
            }
            if op2 == self.or(outrec).pts {
                break;
            }
        }
    }

    pub(crate) fn split(&mut self, e: Idx, pt: Point64) {
        if self.e(e).join_with == JoinWith::Right {
            let next = self.links.next_ael[e as usize];
            self.em(e).join_with = JoinWith::NoJoin;
            self.em(next).join_with = JoinWith::NoJoin;
            self.add_local_min_poly(e, next, pt, true);
        } else {
            let prev = self.links.prev_ael[e as usize];
            self.em(e).join_with = JoinWith::NoJoin;
            self.em(prev).join_with = JoinWith::NoJoin;
            self.add_local_min_poly(prev, e, pt, true);
        }
    }

    pub(crate) fn check_join_left(&mut self, e: Idx, pt: Point64, check_curr_x: bool) {
        let prev = self.links.prev_ael[e as usize];
        if prev == NONE
            || !self.is_hot(e)
            || !self.is_hot(prev)
            || self.is_horizontal(e)
            || self.is_horizontal(prev)
            || self.is_open(e)
            || self.is_open(prev)
        {
            return;
        }
        let (a, p) = (*self.e(e), *self.e(prev));
        if (pt.y < a.top.y + 2 || pt.y < p.top.y + 2) && (a.bot.y > pt.y || p.bot.y > pt.y) {
            return;
        }
        if check_curr_x {
            if perpendic_dist_from_line_sqrd(pt, p.bot, p.top) > 0.25 {
                return;
            }
        } else if a.curr_x != p.curr_x {
            return;
        }
        if !is_collinear(a.top, pt, p.top) {
            return;
        }
        let (i1, i2) = (self.or(a.outrec).idx, self.or(p.outrec).idx);
        if i1 == i2 {
            self.add_local_max_poly(prev, e, pt);
        } else if i1 < i2 {
            self.join_outrec_paths(e, prev);
        } else {
            self.join_outrec_paths(prev, e);
        }
        self.em(prev).join_with = JoinWith::Right;
        self.em(e).join_with = JoinWith::Left;
    }

    pub(crate) fn check_join_right(&mut self, e: Idx, pt: Point64, check_curr_x: bool) {
        let next = self.links.next_ael[e as usize];
        if next == NONE
            || !self.is_hot(e)
            || !self.is_hot(next)
            || self.is_horizontal(e)
            || self.is_horizontal(next)
            || self.is_open(e)
            || self.is_open(next)
        {
            return;
        }
        let (a, n) = (*self.e(e), *self.e(next));
        if (pt.y < a.top.y + 2 || pt.y < n.top.y + 2) && (a.bot.y > pt.y || n.bot.y > pt.y) {
            return;
        }
        if check_curr_x {
            if perpendic_dist_from_line_sqrd(pt, n.bot, n.top) > 0.35 {
                return;
            }
        } else if a.curr_x != n.curr_x {
            return;
        }
        if !is_collinear(a.top, pt, n.top) {
            return;
        }
        let (i1, i2) = (self.or(a.outrec).idx, self.or(n.outrec).idx);
        if i1 == i2 {
            self.add_local_max_poly(e, next, pt);
        } else if i1 < i2 {
            self.join_outrec_paths(e, next);
        } else {
            self.join_outrec_paths(next, e);
        }
        self.em(e).join_with = JoinWith::Right;
        self.em(next).join_with = JoinWith::Left;
    }

    /// `PointInOpPolygon`.
    pub(crate) fn point_in_op_polygon(&self, pt: Point64, mut op: Idx) -> PointInPolygonResult {
        if op == self.op(op).next || self.op(op).prev == self.op(op).next {
            return PointInPolygonResult::IsOutside;
        }
        let start = op;
        loop {
            if self.op(op).pt.y != pt.y {
                break;
            }
            op = self.op(op).next;
            if op == start {
                break;
            }
        }
        if self.op(op).pt.y == pt.y {
            return PointInPolygonResult::IsOutside;
        }
        let mut is_above = self.op(op).pt.y < pt.y;
        let starting_above = is_above;
        let mut val = 0;
        let mut op2 = self.op(op).next;
        while op2 != op {
            if is_above {
                while op2 != op && self.op(op2).pt.y < pt.y {
                    op2 = self.op(op2).next;
                }
            } else {
                while op2 != op && self.op(op2).pt.y > pt.y {
                    op2 = self.op(op2).next;
                }
            }
            if op2 == op {
                break;
            }
            let (c, p) = (self.op(op2).pt, self.op(self.op(op2).prev).pt);
            if c.y == pt.y {
                if c.x == pt.x || (c.y == p.y && ((pt.x < p.x) != (pt.x < c.x))) {
                    return PointInPolygonResult::IsOn;
                }
                op2 = self.op(op2).next;
                if op2 == op {
                    break;
                }
                continue;
            }
            if pt.x < c.x && pt.x < p.x {
                // no crossing
            } else if pt.x > p.x && pt.x > c.x {
                val = 1 - val;
            } else {
                let i = cross_product_sign_t(p, c, pt);
                if i == 0 {
                    return PointInPolygonResult::IsOn;
                }
                if (i < 0) == is_above {
                    val = 1 - val;
                }
            }
            is_above = !is_above;
            op2 = self.op(op2).next;
        }
        if is_above != starting_above {
            let i = cross_product_sign_t(self.op(self.op(op2).prev).pt, self.op(op2).pt, pt);
            if i == 0 {
                return PointInPolygonResult::IsOn;
            }
            if (i < 0) == is_above {
                val = 1 - val;
            }
        }
        if val == 0 {
            PointInPolygonResult::IsOutside
        } else {
            PointInPolygonResult::IsInside
        }
    }

    /// `GetCleanPath`: the ring without collinear horizontal/vertical intermediate points.
    fn get_clean_path(&self, op: Idx) -> Path64 {
        let mut result = Vec::new();
        let mut op2 = op;
        loop {
            let o = self.op(op2);
            let (n, p) = (self.op(o.next).pt, self.op(o.prev).pt);
            if o.next != op
                && ((o.pt.x == n.x && o.pt.x == p.x) || (o.pt.y == n.y && o.pt.y == p.y))
            {
                op2 = o.next;
            } else {
                break;
            }
        }
        result.push(self.op(op2).pt);
        let mut prev_op = op2;
        op2 = self.op(op2).next;
        while op2 != op {
            let (c, n, p) = (
                self.op(op2).pt,
                self.op(self.op(op2).next).pt,
                self.op(prev_op).pt,
            );
            if (c.x != n.x || c.x != p.x) && (c.y != n.y || c.y != p.y) {
                result.push(c);
                prev_op = op2;
            }
            op2 = self.op(op2).next;
        }
        result
    }

    /// `Path2ContainsPath1(OutPt*, OutPt*)`.
    pub(crate) fn path2_contains_path1_op(&self, op1: Idx, op2: Idx) -> bool {
        let mut pip = PointInPolygonResult::IsOn;
        let mut op = op1;
        loop {
            match self.point_in_op_polygon(self.op(op).pt, op2) {
                PointInPolygonResult::IsOutside => {
                    if pip == PointInPolygonResult::IsOutside {
                        return false;
                    }
                    pip = PointInPolygonResult::IsOutside;
                }
                PointInPolygonResult::IsInside => {
                    if pip == PointInPolygonResult::IsInside {
                        return true;
                    }
                    pip = PointInPolygonResult::IsInside;
                }
                PointInPolygonResult::IsOn => {}
            }
            op = self.op(op).next;
            if op == op1 {
                break;
            }
        }
        path2_contains_path1(&self.get_clean_path(op1), &self.get_clean_path(op2))
    }
}
