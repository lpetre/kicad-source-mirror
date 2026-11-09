//! The active edge list: winding counts, insertion order, local minima.

use super::ClipperBase;
use super::types::*;
use crate::core::{ClipType, FillRule, Point64, cross_product_sign_t, is_collinear};

/// `GetDx`: horizontal edges get `∓f64::MAX` (heading right / left).
#[inline]
pub(crate) fn get_dx(pt1: Point64, pt2: Point64) -> f64 {
    let dy = (pt2.y.wrapping_sub(pt1.y)) as f64;
    if dy != 0.0 {
        (pt2.x.wrapping_sub(pt1.x)) as f64 / dy
    } else if pt2.x > pt1.x {
        -f64::MAX
    } else {
        f64::MAX
    }
}

/// `TopX`: the edge's x at `current_y`.
#[inline]
pub(crate) fn top_x(ae: &Active, current_y: i64) -> i64 {
    if current_y == ae.top.y || ae.top.x == ae.bot.x {
        ae.top.x
    } else if current_y == ae.bot.y {
        ae.bot.x
    } else {
        ae.bot.x.wrapping_add(
            (ae.dx * (current_y.wrapping_sub(ae.bot.y)) as f64).round_ties_even() as i64,
        )
    }
}

#[inline]
fn cps(a: Point64, b: Point64, c: Point64) -> i32 {
    cross_product_sign_t(a, b, c)
}

impl ClipperBase {
    #[inline]
    pub(crate) fn set_dx(&mut self, e: Idx) {
        let a = self.em(e);
        a.dx = get_dx(a.bot, a.top);
    }

    #[inline]
    pub(crate) fn is_heading_right_horz(&self, e: Idx) -> bool {
        self.e(e).dx == -f64::MAX
    }

    #[inline]
    pub(crate) fn is_heading_left_horz(&self, e: Idx) -> bool {
        self.e(e).dx == f64::MAX
    }

    pub(crate) fn get_prev_hot_edge(&self, e: Idx) -> Idx {
        let mut prev = self.links.prev_ael[e as usize];
        while prev != NONE && (self.is_open(prev) || !self.is_hot(prev)) {
            prev = self.links.prev_ael[prev as usize];
        }
        prev
    }

    pub(crate) fn is_contributing_closed(&self, e: Idx) -> bool {
        let a = self.e(e);
        match self.fillrule {
            FillRule::EvenOdd => {}
            FillRule::NonZero => {
                if a.wind_cnt.abs() != 1 {
                    return false;
                }
            }
            FillRule::Positive => {
                if a.wind_cnt != 1 {
                    return false;
                }
            }
            FillRule::Negative => {
                if a.wind_cnt != -1 {
                    return false;
                }
            }
        }
        match self.cliptype {
            ClipType::NoClip => false,
            ClipType::Intersection => match self.fillrule {
                FillRule::Positive => a.wind_cnt2 > 0,
                FillRule::Negative => a.wind_cnt2 < 0,
                _ => a.wind_cnt2 != 0,
            },
            ClipType::Union => match self.fillrule {
                FillRule::Positive => a.wind_cnt2 <= 0,
                FillRule::Negative => a.wind_cnt2 >= 0,
                _ => a.wind_cnt2 == 0,
            },
            ClipType::Difference => {
                let result = match self.fillrule {
                    FillRule::Positive => a.wind_cnt2 <= 0,
                    FillRule::Negative => a.wind_cnt2 >= 0,
                    _ => a.wind_cnt2 == 0,
                };
                if self.polytype(e) == PathType::Subject {
                    result
                } else {
                    !result
                }
            }
            ClipType::Xor => true,
        }
    }

    pub(crate) fn is_contributing_open(&self, e: Idx) -> bool {
        let a = self.e(e);
        let (is_in_clip, is_in_subj) = match self.fillrule {
            FillRule::Positive => (a.wind_cnt2 > 0, a.wind_cnt > 0),
            FillRule::Negative => (a.wind_cnt2 < 0, a.wind_cnt < 0),
            _ => (a.wind_cnt2 != 0, a.wind_cnt != 0),
        };
        match self.cliptype {
            ClipType::Intersection => is_in_clip,
            ClipType::Union => !is_in_subj && !is_in_clip,
            _ => !is_in_clip,
        }
    }

    pub(crate) fn set_wind_count_for_closed_path_edge(&mut self, e: Idx) {
        let mut e2 = self.links.prev_ael[e as usize];
        let pt = self.polytype(e);
        while e2 != NONE && (self.polytype(e2) != pt || self.is_open(e2)) {
            e2 = self.links.prev_ael[e2 as usize];
        }
        if e2 == NONE {
            let wd = self.e(e).wind_dx;
            self.em(e).wind_cnt = wd;
            e2 = self.actives;
        } else if self.fillrule == FillRule::EvenOdd {
            let (wd, wc2) = (self.e(e).wind_dx, self.e(e2).wind_cnt2);
            let a = self.em(e);
            a.wind_cnt = wd;
            a.wind_cnt2 = wc2;
            e2 = self.links.next_ael[e2 as usize];
        } else {
            let b = *self.e(e2);
            let wind_dx = self.e(e).wind_dx;
            let is_open = self.is_open(e);
            let wc = if b.wind_cnt * b.wind_dx < 0 {
                if b.wind_cnt.abs() > 1 {
                    if b.wind_dx * wind_dx < 0 {
                        b.wind_cnt
                    } else {
                        b.wind_cnt + wind_dx
                    }
                } else if is_open {
                    1
                } else {
                    wind_dx
                }
            } else if b.wind_dx * wind_dx < 0 {
                b.wind_cnt
            } else {
                b.wind_cnt + wind_dx
            };
            let a = self.em(e);
            a.wind_cnt = wc;
            a.wind_cnt2 = b.wind_cnt2;
            e2 = self.links.next_ael[e2 as usize];
        }
        if self.fillrule == FillRule::EvenOdd {
            while e2 != e {
                if self.polytype(e2) != pt && !self.is_open(e2) {
                    let a = self.em(e);
                    a.wind_cnt2 = if a.wind_cnt2 == 0 { 1 } else { 0 };
                }
                e2 = self.links.next_ael[e2 as usize];
            }
        } else {
            while e2 != e {
                if self.polytype(e2) != pt && !self.is_open(e2) {
                    let d = self.e(e2).wind_dx;
                    self.em(e).wind_cnt2 += d;
                }
                e2 = self.links.next_ael[e2 as usize];
            }
        }
    }

    pub(crate) fn set_wind_count_for_open_path_edge(&mut self, e: Idx) {
        let mut e2 = self.actives;
        if self.fillrule == FillRule::EvenOdd {
            let (mut cnt1, mut cnt2) = (0, 0);
            while e2 != e {
                if self.polytype(e2) == PathType::Clip {
                    cnt2 += 1;
                } else if !self.is_open(e2) {
                    cnt1 += 1;
                }
                e2 = self.links.next_ael[e2 as usize];
            }
            let a = self.em(e);
            a.wind_cnt = if cnt1 & 1 == 1 { 1 } else { 0 };
            a.wind_cnt2 = if cnt2 & 1 == 1 { 1 } else { 0 };
        } else {
            while e2 != e {
                let d = self.e(e2).wind_dx;
                if self.polytype(e2) == PathType::Clip {
                    self.em(e).wind_cnt2 += d;
                } else if !self.is_open(e2) {
                    self.em(e).wind_cnt += d;
                }
                e2 = self.links.next_ael[e2 as usize];
            }
        }
    }

    /// `IsValidAelOrder`: true if `newcomer` belongs to the right of `resident`.
    pub(crate) fn is_valid_ael_order(&self, resident: Idx, newcomer: Idx) -> bool {
        let (r, n) = (self.e(resident), self.e(newcomer));
        if n.curr_x != r.curr_x {
            return n.curr_x > r.curr_x;
        }
        let i = cps(r.top, n.bot, n.top);
        if i != 0 {
            return i < 0;
        }
        if !self.is_maxima(resident) && r.top.y > n.top.y {
            return cps(n.bot, r.top, self.v(self.next_vertex(resident)).pt) <= 0;
        } else if !self.is_maxima(newcomer) && n.top.y > r.top.y {
            return cps(n.bot, n.top, self.v(self.next_vertex(newcomer)).pt) >= 0;
        }
        let y = n.bot.y;
        let newcomer_is_left = n.is_left_bound;
        if r.bot.y != y || self.v(self.minima[r.local_min as usize].vertex).pt.y != y {
            n.is_left_bound
        } else if r.is_left_bound != newcomer_is_left {
            newcomer_is_left
        } else if is_collinear(self.v(self.prev_prev_vertex(resident)).pt, r.bot, r.top) {
            true
        } else {
            (cps(
                self.v(self.prev_prev_vertex(resident)).pt,
                n.bot,
                self.v(self.prev_prev_vertex(newcomer)).pt,
            ) > 0)
                == newcomer_is_left
        }
    }

    pub(crate) fn insert_left_edge(&mut self, e: Idx) {
        if self.actives == NONE {
            self.links.prev_ael[e as usize] = NONE;
            self.links.next_ael[e as usize] = NONE;
            self.actives = e;
        } else if !self.is_valid_ael_order(self.actives, e) {
            let first = self.actives;
            self.links.prev_ael[e as usize] = NONE;
            self.links.next_ael[e as usize] = first;
            self.links.prev_ael[first as usize] = e;
            self.actives = e;
        } else {
            let mut e2 = self.actives;
            while self.links.next_ael[e2 as usize] != NONE
                && self.is_valid_ael_order(self.links.next_ael[e2 as usize], e)
            {
                e2 = self.links.next_ael[e2 as usize];
            }
            if self.e(e2).join_with == JoinWith::Right {
                e2 = self.links.next_ael[e2 as usize];
            }
            if e2 == NONE {
                return;
            }
            let next = self.links.next_ael[e2 as usize];
            self.links.next_ael[e as usize] = next;
            if next != NONE {
                self.links.prev_ael[next as usize] = e;
            }
            self.links.prev_ael[e as usize] = e2;
            self.links.next_ael[e2 as usize] = e;
        }
    }

    pub(crate) fn insert_right_edge(&mut self, e: Idx, e2: Idx) {
        let next = self.links.next_ael[e as usize];
        self.links.next_ael[e2 as usize] = next;
        if next != NONE {
            self.links.prev_ael[next as usize] = e2;
        }
        self.links.prev_ael[e2 as usize] = e;
        self.links.next_ael[e as usize] = e2;
    }

    fn new_bound(&mut self, lm: Idx, wind_dx: i32) -> Idx {
        let v = self.minima[lm as usize].vertex;
        let bot = self.v(v).pt;
        let vertex_top = if wind_dx < 0 {
            self.v(v).prev
        } else {
            self.v(v).next
        };
        let mut a = Active {
            bot,
            curr_x: bot.x,
            wind_dx,
            vertex_top,
            top: self.v(vertex_top).pt,
            local_min: lm,
            ..Active::default()
        };
        a.dx = get_dx(a.bot, a.top);
        self.edges.push(a);
        self.links.push_unlinked();
        (self.edges.len() - 1) as Idx
    }

    pub(crate) fn insert_local_minima_into_ael(&mut self, bot_y: i64) {
        while let Some(lm) = self.pop_local_minima(bot_y) {
            let vflags = self.v(self.minima[lm as usize].vertex).flags;
            let mut left_bound = if vflags & VF_OPEN_START != 0 {
                NONE
            } else {
                self.new_bound(lm, -1)
            };
            let mut right_bound = if vflags & VF_OPEN_END != 0 {
                NONE
            } else {
                self.new_bound(lm, 1)
            };

            if left_bound != NONE && right_bound != NONE {
                if self.is_horizontal(left_bound) {
                    if self.is_heading_right_horz(left_bound) {
                        std::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if self.is_horizontal(right_bound) {
                    if self.is_heading_left_horz(right_bound) {
                        std::mem::swap(&mut left_bound, &mut right_bound);
                    }
                } else if self.e(left_bound).dx < self.e(right_bound).dx {
                    std::mem::swap(&mut left_bound, &mut right_bound);
                }
            } else if left_bound == NONE {
                left_bound = right_bound;
                right_bound = NONE;
            }

            self.em(left_bound).is_left_bound = true;
            self.insert_left_edge(left_bound);
            let contributing = if self.is_open(left_bound) {
                self.set_wind_count_for_open_path_edge(left_bound);
                self.is_contributing_open(left_bound)
            } else {
                self.set_wind_count_for_closed_path_edge(left_bound);
                self.is_contributing_closed(left_bound)
            };

            if right_bound != NONE {
                let (wc, wc2) = (self.e(left_bound).wind_cnt, self.e(left_bound).wind_cnt2);
                let r = self.em(right_bound);
                r.is_left_bound = false;
                r.wind_cnt = wc;
                r.wind_cnt2 = wc2;
                self.insert_right_edge(left_bound, right_bound);
                if contributing {
                    let bot = self.e(left_bound).bot;
                    self.add_local_min_poly(left_bound, right_bound, bot, true);
                    if !self.is_horizontal(left_bound) {
                        self.check_join_left(left_bound, bot, false);
                    }
                }
                while self.links.next_ael[right_bound as usize] != NONE
                    && self
                        .is_valid_ael_order(self.links.next_ael[right_bound as usize], right_bound)
                {
                    let next = self.links.next_ael[right_bound as usize];
                    let bot = self.e(right_bound).bot;
                    self.intersect_edges(right_bound, next, bot);
                    self.swap_positions_in_ael(right_bound, next);
                }
                if self.is_horizontal(right_bound) {
                    self.push_horz(right_bound);
                } else {
                    let bot = self.e(right_bound).bot;
                    self.check_join_right(right_bound, bot, false);
                    let y = self.e(right_bound).top.y;
                    self.insert_scanline(y);
                }
            } else if contributing {
                let bot = self.e(left_bound).bot;
                self.start_open_path(left_bound, bot);
            }

            if self.is_horizontal(left_bound) {
                self.push_horz(left_bound);
            } else {
                let y = self.e(left_bound).top.y;
                self.insert_scanline(y);
            }
        }
    }

    #[inline]
    pub(crate) fn push_horz(&mut self, e: Idx) {
        let sel = self.sel;
        self.links.next_sel[e as usize] = sel;
        self.sel = e;
    }

    #[inline]
    pub(crate) fn pop_horz(&mut self) -> Option<Idx> {
        let e = self.sel;
        if e == NONE {
            return None;
        }
        self.sel = self.links.next_sel[e as usize];
        Some(e)
    }

    /// `DeleteFromAEL`. The record stays in the arena with both links cleared, which is how
    /// a second call recognises an already deleted edge.
    pub(crate) fn delete_from_ael(&mut self, e: Idx) {
        let (prev, next) = (
            self.links.prev_ael[e as usize],
            self.links.next_ael[e as usize],
        );
        if prev == NONE && next == NONE && e != self.actives {
            return;
        }
        if prev != NONE {
            self.links.next_ael[prev as usize] = next;
        } else {
            self.actives = next;
        }
        if next != NONE {
            self.links.prev_ael[next as usize] = prev;
        }
        self.links.prev_ael[e as usize] = NONE;
        self.links.next_ael[e as usize] = NONE;
    }

    pub(crate) fn swap_positions_in_ael(&mut self, e1: Idx, e2: Idx) {
        let next = self.links.next_ael[e2 as usize];
        if next != NONE {
            self.links.prev_ael[next as usize] = e1;
        }
        let prev = self.links.prev_ael[e1 as usize];
        if prev != NONE {
            self.links.next_ael[prev as usize] = e2;
        }
        self.links.prev_ael[e2 as usize] = prev;
        self.links.next_ael[e2 as usize] = e1;
        self.links.prev_ael[e1 as usize] = e2;
        self.links.next_ael[e1 as usize] = next;
        if prev == NONE {
            self.actives = e2;
        }
    }

    pub(crate) fn trim_horz(&mut self, horz: Idx, preserve_collinear: bool) {
        let mut was_trimmed = false;
        let mut pt = self.v(self.next_vertex(horz)).pt;
        while pt.y == self.e(horz).top.y {
            let (top, bot) = (self.e(horz).top, self.e(horz).bot);
            if preserve_collinear && ((pt.x < top.x) != (bot.x < top.x)) {
                break;
            }
            let nv = self.next_vertex(horz);
            let a = self.em(horz);
            a.vertex_top = nv;
            a.top = pt;
            was_trimmed = true;
            if self.is_maxima(horz) {
                break;
            }
            pt = self.v(self.next_vertex(horz)).pt;
        }
        if was_trimmed {
            self.set_dx(horz);
        }
    }

    pub(crate) fn update_edge_into_ael(&mut self, e: Idx) {
        let nv = self.next_vertex(e);
        let top_pt = self.v(nv).pt;
        let a = self.em(e);
        a.bot = a.top;
        a.vertex_top = nv;
        a.top = top_pt;
        a.curr_x = a.bot.x;
        self.set_dx(e);
        if self.is_joined(e) {
            let bot = self.e(e).bot;
            self.split(e, bot);
        }
        if self.is_horizontal(e) {
            if !self.is_open(e) {
                self.trim_horz(e, self.preserve_collinear);
            }
            return;
        }
        let (top_y, bot) = (self.e(e).top.y, self.e(e).bot);
        self.insert_scanline(top_y);
        self.check_join_left(e, bot, false);
        self.check_join_right(e, bot, true);
    }

    /// Returns whether the edges are still in x order at `top_y`. If so, no two of them cross
    /// in this scanbeam and the merge sort in `build_intersect_list` would swap nothing, so it
    /// can be skipped.
    pub(crate) fn adjust_curr_x_and_copy_to_sel(&mut self, top_y: i64) -> bool {
        let (edges, links) = (&mut self.edges[..], &mut self.links);
        let mut e = self.actives;
        self.sel = e;
        let mut prev_x = i64::MIN;
        let mut sorted = true;
        while e != NONE {
            let i = e as usize;
            let next = links.next_ael[i];
            links.prev_sel[i] = links.prev_ael[i];
            links.next_sel[i] = next;
            links.jump[i] = next;
            let a = &mut edges[i];
            a.curr_x = top_x(a, top_y);
            sorted &= a.curr_x >= prev_x;
            prev_x = a.curr_x;
            e = next;
        }
        sorted
    }
}
