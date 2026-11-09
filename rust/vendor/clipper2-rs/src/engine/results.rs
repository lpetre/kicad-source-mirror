//! Turning output records into paths and trees.

use super::ClipperBase;
use super::types::*;
use crate::core::{Path64, PathD, Paths64, PathsD, get_bounds};
use crate::polytree::PolyTree64;

impl ClipperBase {
    /// The distinct points of a ring in output order, and the op the walk ended on.
    fn walk_ring(&self, op: Idx, reverse: bool, is_open: bool) -> Option<(Path64, Idx)> {
        if op == NONE
            || self.op(op).next == op
            || (!is_open && self.op(op).next == self.op(op).prev)
        {
            return None;
        }
        let mut path = Vec::new();
        let (start, mut op2, mut last_pt);
        if reverse {
            start = op;
            last_pt = self.op(op).pt;
            op2 = self.op(op).prev;
        } else {
            start = self.op(op).next;
            last_pt = self.op(start).pt;
            op2 = self.op(start).next;
        }
        path.push(last_pt);
        while op2 != start {
            if self.op(op2).pt != last_pt {
                last_pt = self.op(op2).pt;
                path.push(last_pt);
            }
            op2 = if reverse {
                self.op(op2).prev
            } else {
                self.op(op2).next
            };
        }
        Some((path, op2))
    }

    /// `BuildPath64`: `None` when the ring is degenerate.
    pub(crate) fn build_path64(&self, op: Idx, reverse: bool, is_open: bool) -> Option<Path64> {
        let (path, end) = self.walk_ring(op, reverse, is_open)?;
        if !is_open && path.len() == 3 && self.is_very_small_triangle(end) {
            None
        } else {
            Some(path)
        }
    }

    /// `BuildPathD`. Unlike `BuildPath64` it drops very small triangles of open paths too.
    pub(crate) fn build_path_d(
        &self,
        op: Idx,
        reverse: bool,
        is_open: bool,
        inv_scale: f64,
    ) -> Option<PathD> {
        let (path, end) = self.walk_ring(op, reverse, is_open)?;
        if path.len() == 3 && self.is_very_small_triangle(end) {
            return None;
        }
        Some(
            path.iter()
                .map(|p| p.with_xy(p.x as f64 * inv_scale, p.y as f64 * inv_scale))
                .collect(),
        )
    }

    pub(crate) fn check_bounds(&mut self, outrec: Idx) -> bool {
        if self.or(outrec).pts == NONE {
            return false;
        }
        if !self.or(outrec).bounds.is_empty() {
            return true;
        }
        self.clean_collinear(outrec);
        if self.or(outrec).pts == NONE {
            return false;
        }
        match self.build_path64(self.or(outrec).pts, self.reverse_solution, false) {
            Some(path) => {
                let o = self.orm(outrec);
                o.bounds = get_bounds(&path);
                o.path = path;
                true
            }
            None => false,
        }
    }

    fn check_split_owner(&mut self, outrec: Idx, splits: &[Idx]) -> bool {
        for &s in splits {
            let mut split = s;
            if self.or(split).pts == NONE {
                if let Some(inner) = self.or(split).splits.clone() {
                    if self.or(split).recursive_split == outrec {
                        continue;
                    }
                    // emptied OutRecs can reference each other
                    self.orm(split).recursive_split = outrec;
                    if self.check_split_owner(outrec, &inner) {
                        return true; // #942
                    }
                }
            }
            split = self.get_real_out_rec(split);
            if split == NONE || split == outrec || self.or(split).recursive_split == outrec {
                continue;
            }
            self.orm(split).recursive_split = outrec; // prevents infinite loops
            if let Some(inner) = self.or(split).splits.clone() {
                if self.check_split_owner(outrec, &inner) {
                    return true;
                }
            }
            if !self.check_bounds(split)
                || !self.or(split).bounds.contains_rect(&self.or(outrec).bounds)
                || !self.path2_contains_path1_op(self.or(outrec).pts, self.or(split).pts)
            {
                continue;
            }
            if !self.is_valid_owner(outrec, split) {
                // split is owned by outrec (#957)
                let o = self.or(outrec).owner;
                self.orm(split).owner = o;
            }
            self.orm(outrec).owner = split;
            return true;
        }
        false
    }

    pub(crate) fn recursive_check_owners(&mut self, outrec: Idx, tree: &mut PolyTree64) {
        if self.or(outrec).polypath != NONE || self.or(outrec).bounds.is_empty() {
            return;
        }
        while self.or(outrec).owner != NONE {
            let owner = self.or(outrec).owner;
            if let Some(splits) = self.or(owner).splits.clone() {
                if self.check_split_owner(outrec, &splits) {
                    break;
                }
            }
            let owner = self.or(outrec).owner;
            if self.or(owner).pts != NONE
                && self.check_bounds(owner)
                && self.or(owner).bounds.contains_rect(&self.or(outrec).bounds)
                && self.path2_contains_path1_op(self.or(outrec).pts, self.or(owner).pts)
            {
                break;
            }
            let next = self.or(owner).owner;
            self.orm(outrec).owner = next;
        }
        let owner = self.or(outrec).owner;
        let path = self.or(outrec).path.clone();
        let node = if owner != NONE {
            if self.or(owner).polypath == NONE {
                self.recursive_check_owners(owner, tree);
            }
            let parent = self.or(owner).polypath as usize;
            tree.add_child(Some(parent), path)
        } else {
            tree.add_child(None, path)
        };
        self.orm(outrec).polypath = node as Idx;
    }

    /// `BuildPaths64`.
    pub(crate) fn build_paths(&mut self, closed: &mut Paths64, mut open: Option<&mut Paths64>) {
        closed.clear();
        // Cleaning can split a record and append new ones, which are visited too.
        let mut i: Idx = 0;
        while (i as usize) < self.outrecs.len() {
            i += 1;
            let i = i - 1;
            if self.or(i).pts == NONE {
                continue;
            }
            if let (Some(open), true) = (open.as_deref_mut(), self.or(i).is_open) {
                if let Some(path) = self.build_path64(self.or(i).pts, self.reverse_solution, true) {
                    open.push(path);
                }
            } else {
                self.clean_collinear(i);
                if let Some(path) = self.build_path64(self.or(i).pts, self.reverse_solution, false)
                {
                    closed.push(path);
                }
            }
        }
    }

    /// `BuildTree64`.
    pub(crate) fn build_tree(&mut self, tree: &mut PolyTree64, open: &mut Paths64) {
        tree.clear();
        open.clear();
        let mut i: Idx = 0;
        while (i as usize) < self.outrecs.len() {
            i += 1;
            let i = i - 1;
            if self.or(i).pts == NONE {
                continue;
            }
            if self.or(i).is_open {
                if let Some(path) = self.build_path64(self.or(i).pts, self.reverse_solution, true) {
                    open.push(path);
                }
                continue;
            }
            if self.check_bounds(i) {
                self.recursive_check_owners(i, tree);
            }
        }
    }
}

impl ClipperBase {
    /// `BuildPathsD`.
    pub(crate) fn build_paths_d(&mut self, closed: &mut PathsD, open: &mut PathsD, inv_scale: f64) {
        closed.clear();
        open.clear();
        let mut i: Idx = 0;
        while (i as usize) < self.outrecs.len() {
            i += 1;
            let i = i - 1;
            if self.or(i).pts == NONE {
                continue;
            }
            if self.or(i).is_open {
                if let Some(path) =
                    self.build_path_d(self.or(i).pts, self.reverse_solution, true, inv_scale)
                {
                    open.push(path);
                }
            } else {
                self.clean_collinear(i);
                if let Some(path) =
                    self.build_path_d(self.or(i).pts, self.reverse_solution, false, inv_scale)
                {
                    closed.push(path);
                }
            }
        }
    }

    /// The open-path part of `BuildTreeD` (closed paths go through [`Self::build_tree`]).
    pub(crate) fn build_tree_d(
        &mut self,
        tree: &mut PolyTree64,
        open: &mut PathsD,
        inv_scale: f64,
    ) {
        tree.clear();
        open.clear();
        let mut i: Idx = 0;
        while (i as usize) < self.outrecs.len() {
            i += 1;
            let i = i - 1;
            if self.or(i).pts == NONE {
                continue;
            }
            if self.or(i).is_open {
                if let Some(path) =
                    self.build_path_d(self.or(i).pts, self.reverse_solution, true, inv_scale)
                {
                    open.push(path);
                }
                continue;
            }
            if self.check_bounds(i) {
                self.recursive_check_owners(i, tree);
            }
        }
    }
}
