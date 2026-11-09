//! The Vatti clipping engine (`clipper.engine.cpp`).

mod ael;
mod horizontal;
mod input;
mod intersect;
mod output;
mod results;
mod reuse;
mod scan;
pub(crate) mod types;

use std::collections::BinaryHeap;

#[cfg(feature = "usingz")]
use crate::core::Point64;
use crate::core::{ClipType, ClipperError, FillRule, Paths64, check_range};
use crate::polytree::PolyTree64;
use types::*;

pub use reuse::ReuseableDataContainer64;
pub use types::PathType;

/// Z callback: `(e1bot, e1top, e2bot, e2top, intersection_point)`; sets the point's `z`.
#[cfg(feature = "usingz")]
pub type ZCallback64 = Box<dyn FnMut(Point64, Point64, Point64, Point64, &mut Point64)>;

/// Engine state shared by [`Clipper64`] and [`crate::ClipperD`] (`ClipperBase`).
pub(crate) struct ClipperBase {
    cliptype: ClipType,
    fillrule: FillRule,
    bot_y: i64,
    /// Every active edge's `curr_x` is already `top_x(top_y)`. Set by `do_intersections` and
    /// only valid for the `do_top_of_scanbeam` call that immediately follows it.
    curr_x_at_top: bool,
    minima_list_sorted: bool,
    using_polytree: bool,
    actives: Idx,
    sel: Idx,
    vertices: Vec<Vertex>,
    minima: Vec<LocalMinima>,
    current_locmin: usize,
    scanline: BinaryHeap<i64>,
    intersect_nodes: Vec<IntersectNode>,
    horz_seg_list: Vec<HorzSegment>,
    horz_join_list: Vec<HorzJoin>,
    edges: Vec<Active>,
    links: Links,
    outpts: Vec<OutPt>,
    outrecs: Vec<OutRec>,
    pub(crate) preserve_collinear: bool,
    pub(crate) reverse_solution: bool,
    pub(crate) has_open_paths: bool,
    succeeded: bool,
    /// Set when an added path has a coordinate outside `MIN_COORD..=MAX_COORD`.
    pub(crate) range_error: bool,
    #[cfg(feature = "usingz")]
    pub(crate) z_callback: Option<ZCallback64>,
    #[cfg(feature = "usingz")]
    pub(crate) default_z: i64,
}

impl Default for ClipperBase {
    fn default() -> Self {
        ClipperBase {
            cliptype: ClipType::NoClip,
            fillrule: FillRule::EvenOdd,
            bot_y: 0,
            curr_x_at_top: false,
            minima_list_sorted: false,
            using_polytree: false,
            actives: NONE,
            sel: NONE,
            vertices: Vec::new(),
            minima: Vec::new(),
            current_locmin: 0,
            scanline: BinaryHeap::new(),
            intersect_nodes: Vec::new(),
            horz_seg_list: Vec::new(),
            horz_join_list: Vec::new(),
            edges: Vec::new(),
            links: Links::default(),
            outpts: Vec::new(),
            outrecs: Vec::new(),
            preserve_collinear: true,
            reverse_solution: false,
            has_open_paths: false,
            succeeded: true,
            range_error: false,
            #[cfg(feature = "usingz")]
            z_callback: None,
            #[cfg(feature = "usingz")]
            default_z: 0,
        }
    }
}

impl ClipperBase {
    /// `CleanUp`: drops the working state but keeps the added paths.
    pub(crate) fn clean_up(&mut self) {
        self.actives = NONE;
        self.sel = NONE;
        self.edges.clear();
        self.links.clear();
        self.scanline.clear();
        self.intersect_nodes.clear();
        self.outpts.clear();
        self.outrecs.clear();
        self.horz_seg_list.clear();
        self.horz_join_list.clear();
    }

    /// `Clear`: also drops the added paths.
    pub(crate) fn clear(&mut self) {
        self.clean_up();
        self.vertices.clear();
        self.minima.clear();
        self.current_locmin = 0;
        self.minima_list_sorted = false;
        self.has_open_paths = false;
        self.range_error = false;
    }

    pub(crate) fn add_paths(&mut self, paths: &Paths64, polytype: PathType, is_open: bool) {
        if check_range(paths).is_err() {
            self.range_error = true;
        }
        if is_open {
            self.has_open_paths = true;
        }
        self.minima_list_sorted = false;
        input::add_paths(
            paths,
            polytype,
            is_open,
            &mut self.vertices,
            &mut self.minima,
        );
    }

    /// Runs the sweep (`ExecuteInternal`); the caller builds the output and calls `clean_up`.
    pub(crate) fn run(
        &mut self,
        ct: ClipType,
        fr: FillRule,
        use_polytree: bool,
    ) -> Result<(), ClipperError> {
        if self.range_error {
            return Err(ClipperError::CoordinateOutOfRange);
        }
        if self.execute_internal(ct, fr, use_polytree) {
            Ok(())
        } else {
            Err(ClipperError::ExecuteFailed)
        }
    }

    #[inline]
    pub(crate) fn e(&self, i: Idx) -> &Active {
        &self.edges[i as usize]
    }

    #[inline]
    pub(crate) fn em(&mut self, i: Idx) -> &mut Active {
        &mut self.edges[i as usize]
    }

    #[inline]
    pub(crate) fn op(&self, i: Idx) -> &OutPt {
        &self.outpts[i as usize]
    }

    #[inline]
    pub(crate) fn opm(&mut self, i: Idx) -> &mut OutPt {
        &mut self.outpts[i as usize]
    }

    #[inline]
    pub(crate) fn or(&self, i: Idx) -> &OutRec {
        &self.outrecs[i as usize]
    }

    #[inline]
    pub(crate) fn orm(&mut self, i: Idx) -> &mut OutRec {
        &mut self.outrecs[i as usize]
    }

    #[inline]
    pub(crate) fn v(&self, i: Idx) -> &Vertex {
        &self.vertices[i as usize]
    }

    #[inline]
    pub(crate) fn lm(&self, e: Idx) -> &LocalMinima {
        &self.minima[self.e(e).local_min as usize]
    }

    #[inline]
    pub(crate) fn is_open(&self, e: Idx) -> bool {
        self.lm(e).is_open
    }

    #[inline]
    pub(crate) fn polytype(&self, e: Idx) -> PathType {
        self.lm(e).polytype
    }

    #[inline]
    pub(crate) fn is_hot(&self, e: Idx) -> bool {
        self.e(e).outrec != NONE
    }

    #[inline]
    pub(crate) fn is_horizontal(&self, e: Idx) -> bool {
        self.e(e).top.y == self.e(e).bot.y
    }

    #[inline]
    pub(crate) fn is_joined(&self, e: Idx) -> bool {
        self.e(e).join_with != JoinWith::NoJoin
    }

    #[inline]
    pub(crate) fn is_front(&self, e: Idx) -> bool {
        e == self.or(self.e(e).outrec).front_edge
    }

    #[inline]
    pub(crate) fn is_open_end_v(&self, v: Idx) -> bool {
        self.v(v).flags & (VF_OPEN_START | VF_OPEN_END) != 0
    }

    #[inline]
    pub(crate) fn is_open_end(&self, e: Idx) -> bool {
        self.is_open_end_v(self.e(e).vertex_top)
    }

    #[inline]
    pub(crate) fn is_maxima_v(&self, v: Idx) -> bool {
        self.v(v).flags & VF_LOCAL_MAX != 0
    }

    #[inline]
    pub(crate) fn is_maxima(&self, e: Idx) -> bool {
        self.is_maxima_v(self.e(e).vertex_top)
    }

    #[inline]
    pub(crate) fn next_vertex(&self, e: Idx) -> Idx {
        let a = self.e(e);
        if a.wind_dx > 0 {
            self.v(a.vertex_top).next
        } else {
            self.v(a.vertex_top).prev
        }
    }

    #[inline]
    pub(crate) fn prev_prev_vertex(&self, e: Idx) -> Idx {
        let a = self.e(e);
        let vt = self.v(a.vertex_top);
        if a.wind_dx > 0 {
            self.v(vt.prev).prev
        } else {
            self.v(vt.next).next
        }
    }

    /// `SetZ`: seeds the intersection's z from a matching edge end, then calls the callback.
    #[cfg(feature = "usingz")]
    pub(crate) fn set_z(&mut self, e1: Idx, e2: Idx, op: Idx) {
        let Some(cb) = self.z_callback.as_mut() else {
            return;
        };
        let (a, b) = (self.edges[e1 as usize], self.edges[e2 as usize]);
        let ip = &mut self.outpts[op as usize].pt;
        let subject_first = self.minima[a.local_min as usize].polytype == PathType::Subject;
        let (first, second) = if subject_first { (a, b) } else { (b, a) };
        ip.z = if *ip == first.bot {
            first.bot.z
        } else if *ip == first.top {
            first.top.z
        } else if *ip == second.bot {
            second.bot.z
        } else if *ip == second.top {
            second.top.z
        } else {
            self.default_z
        };
        cb(first.bot, first.top, second.bot, second.top, ip);
    }
}

/// Integer polygon clipper (`Clipper64`).
pub struct Clipper64 {
    pub(crate) base: ClipperBase,
}

/// Closed and open paths produced by [`Clipper64::execute`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Solution {
    /// Closed paths (polygons).
    pub closed: Paths64,
    /// Open paths (polylines) from open subjects.
    pub open: Paths64,
}

impl Default for Clipper64 {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipper64 {
    /// An empty clipper.
    pub fn new() -> Self {
        Clipper64 {
            base: ClipperBase::default(),
        }
    }

    /// Keep collinear vertices in closed paths (default `true`).
    pub fn set_preserve_collinear(&mut self, value: bool) {
        self.base.preserve_collinear = value;
    }

    /// Whether collinear vertices are kept; see [`Self::set_preserve_collinear`].
    pub fn preserve_collinear(&self) -> bool {
        self.base.preserve_collinear
    }

    /// Reverse the orientation of every solution path (default `false`).
    pub fn set_reverse_solution(&mut self, value: bool) {
        self.base.reverse_solution = value;
    }

    /// Whether solution paths are reversed; see [`Self::set_reverse_solution`].
    pub fn reverse_solution(&self) -> bool {
        self.base.reverse_solution
    }

    /// Adds closed subject paths.
    pub fn add_subject(&mut self, subjects: &Paths64) {
        self.base.add_paths(subjects, PathType::Subject, false);
    }

    /// Adds open subject paths (polylines).
    pub fn add_open_subject(&mut self, open_subjects: &Paths64) {
        self.base.add_paths(open_subjects, PathType::Subject, true);
    }

    /// Adds closed clip paths.
    pub fn add_clip(&mut self, clips: &Paths64) {
        self.base.add_paths(clips, PathType::Clip, false);
    }

    /// Adds paths prepared once in a [`ReuseableDataContainer64`].
    pub fn add_reuseable_data(&mut self, data: &ReuseableDataContainer64) {
        self.base.add_reuseable_data(data);
    }

    /// Removes every added path.
    pub fn clear(&mut self) {
        self.base.clear();
    }

    /// Sets the callback that assigns `z` to new intersection points.
    #[cfg(feature = "usingz")]
    pub fn set_z_callback(&mut self, cb: Option<ZCallback64>) {
        self.base.z_callback = cb;
    }

    /// `z` given to intersections that match no input vertex (default 0).
    #[cfg(feature = "usingz")]
    pub fn set_default_z(&mut self, z: i64) {
        self.base.default_z = z;
    }

    /// Performs the boolean operation.
    pub fn execute(
        &mut self,
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<Solution, ClipperError> {
        let result = self.base.run(clip_type, fill_rule, false).map(|_| {
            let mut s = Solution::default();
            self.base.build_paths(&mut s.closed, Some(&mut s.open));
            s
        });
        self.base.clean_up();
        result
    }

    /// Performs the boolean operation, nesting closed paths in a [`PolyTree64`]; the second
    /// value holds the open paths.
    pub fn execute_tree(
        &mut self,
        clip_type: ClipType,
        fill_rule: FillRule,
    ) -> Result<(PolyTree64, Paths64), ClipperError> {
        let result = self.base.run(clip_type, fill_rule, true).map(|_| {
            let mut tree = PolyTree64::default();
            let mut open = Vec::new();
            self.base.build_tree(&mut tree, &mut open);
            (tree, open)
        });
        self.base.clean_up();
        result
    }
}
