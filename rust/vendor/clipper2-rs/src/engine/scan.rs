//! The sweep: scanbeams, tops of edges and maxima.

use super::ClipperBase;
use super::ael::top_x;
use super::types::*;
use crate::core::{ClipType, FillRule};

impl ClipperBase {
    /// `ExecuteInternal`.
    pub(crate) fn execute_internal(
        &mut self,
        ct: ClipType,
        fill_rule: FillRule,
        use_polytrees: bool,
    ) -> bool {
        self.cliptype = ct;
        self.fillrule = fill_rule;
        self.using_polytree = use_polytrees;
        self.reset();
        if ct == ClipType::NoClip {
            return true;
        }
        let Some(mut y) = self.pop_scanline() else {
            return true;
        };
        while self.succeeded {
            self.insert_local_minima_into_ael(y);
            while let Some(e) = self.pop_horz() {
                self.do_horizontal(e);
            }
            if !self.horz_seg_list.is_empty() {
                self.convert_horz_segs_to_joins();
                self.horz_seg_list.clear();
            }
            self.bot_y = y;
            match self.pop_scanline() {
                Some(next) => y = next,
                None => break,
            }
            self.do_intersections(y);
            self.do_top_of_scanbeam(y);
            while let Some(e) = self.pop_horz() {
                self.do_horizontal(e);
            }
        }
        if self.succeeded {
            self.process_horz_joins();
        }
        self.succeeded
    }

    fn do_top_of_scanbeam(&mut self, y: i64) {
        self.sel = NONE; // reused to flag horizontals (see push_horz)
        let mut e = self.actives;
        while e != NONE {
            if self.e(e).top.y == y {
                let tx = self.e(e).top.x;
                self.em(e).curr_x = tx;
                if self.is_maxima(e) {
                    e = self.do_maxima(e);
                    continue;
                }
                if self.is_hot(e) {
                    let top = self.e(e).top;
                    self.add_out_pt(e, top);
                }
                self.update_edge_into_ael(e);
                if self.is_horizontal(e) {
                    self.push_horz(e);
                }
            } else if !self.curr_x_at_top {
                let x = top_x(self.e(e), y);
                self.em(e).curr_x = x;
            }
            e = self.links.next_ael[e as usize];
        }
    }

    fn do_maxima(&mut self, e: Idx) -> Idx {
        let prev_e = self.links.prev_ael[e as usize];
        let mut next_e = self.links.next_ael[e as usize];
        if self.is_open_end(e) {
            if self.is_hot(e) {
                let top = self.e(e).top;
                self.add_out_pt(e, top);
            }
            if !self.is_horizontal(e) {
                if self.is_hot(e) {
                    let outrec = self.e(e).outrec;
                    if self.is_front(e) {
                        self.orm(outrec).front_edge = NONE;
                    } else {
                        self.orm(outrec).back_edge = NONE;
                    }
                    self.em(e).outrec = NONE;
                }
                self.delete_from_ael(e);
            }
            return next_e;
        }
        let max_pair = self.get_maxima_pair(e);
        if max_pair == NONE {
            return next_e; // max_pair is horizontal
        }
        if self.is_joined(e) {
            let top = self.e(e).top;
            self.split(e, top);
        }
        if self.is_joined(max_pair) {
            let top = self.e(max_pair).top;
            self.split(max_pair, top);
        }
        while next_e != max_pair {
            let top = self.e(e).top;
            self.intersect_edges(e, next_e, top);
            self.swap_positions_in_ael(e, next_e);
            next_e = self.links.next_ael[e as usize];
        }
        let top = self.e(e).top;
        if self.is_open(e) {
            if self.is_hot(e) {
                self.add_local_max_poly(e, max_pair, top);
            }
            self.delete_from_ael(max_pair);
            self.delete_from_ael(e);
        } else {
            if self.is_hot(e) {
                self.add_local_max_poly(e, max_pair, top);
            }
            self.delete_from_ael(e);
            self.delete_from_ael(max_pair);
        }
        if prev_e != NONE {
            self.links.next_ael[prev_e as usize]
        } else {
            self.actives
        }
    }
}
