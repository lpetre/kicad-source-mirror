//! `ClipperOffset`: inflating and deflating polygons and polylines (`clipper.offset.cpp`).

use std::f64::consts::PI;

use crate::core::{
    ClipType, ClipperError, FillRule, MAX_COORD, Path64, PathD, Paths64, Point64, PointD, Rect64,
    area, check_range, cross_product_vec, dot_product_vec, get_line_intersect_pt, reflect_point,
    strip_duplicates, translate_point,
};
use crate::engine::Clipper64;
use crate::polytree::PolyTree64;
use crate::utils::ellipse;

const FLOATING_POINT_TOLERANCE: f64 = 1e-12;
const ARC_CONST: f64 = 0.002; // 1/500

/// How corners are joined.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JoinType {
    /// Joins are squared at exactly the offset distance.
    Square = 0,
    /// Like `Square`, but the offset distance varies with the angle (simpler and faster).
    Bevel = 1,
    /// Convex joins are rounded at the offset distance.
    Round = 2,
    /// Joins are mitered, falling back to `Square` beyond the miter limit.
    Miter = 3,
}

/// How paths are closed or their ends capped.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndType {
    /// Closed polygons.
    Polygon = 0,
    /// Polylines joined end to end and offset on both sides.
    Joined = 1,
    /// Open paths offset on both sides with square blunt ends.
    Butt = 2,
    /// Open paths offset on both sides with square extended ends.
    Square = 3,
    /// Open paths offset on both sides with round extended ends.
    Round = 4,
}

/// Delta callback: `(path, path_normals, curr_idx, prev_idx) -> delta` for each vertex.
pub type DeltaCallback64<'a> = &'a mut dyn FnMut(&Path64, &PathD, usize, usize) -> f64;

#[derive(Clone, Debug)]
struct Group {
    paths_in: Paths64,
    lowest_path_idx: Option<usize>,
    is_reversed: bool,
    join_type: JoinType,
    end_type: EndType,
}

fn get_lowest_closed_path_info(paths: &Paths64) -> (Option<usize>, bool) {
    let mut idx = None;
    let mut is_neg_area = false;
    let mut bot = Point64::new(i64::MAX, i64::MIN);
    for (i, path) in paths.iter().enumerate() {
        let mut a = f64::MAX;
        for pt in path {
            if pt.y < bot.y || (pt.y == bot.y && pt.x >= bot.x) {
                continue;
            }
            if a == f64::MAX {
                a = area(path);
                if a == 0.0 {
                    break; // invalid closed path
                }
                is_neg_area = a < 0.0;
            }
            idx = Some(i);
            bot.x = pt.x;
            bot.y = pt.y;
        }
    }
    (idx, is_neg_area)
}

impl Group {
    fn new(paths: &Paths64, join_type: JoinType, end_type: EndType) -> Self {
        let is_joined = matches!(end_type, EndType::Polygon | EndType::Joined);
        // Empty paths would make the offset index past the end (undefined behaviour upstream).
        let mut paths_in: Paths64 = paths.iter().filter(|p| !p.is_empty()).cloned().collect();
        for p in &mut paths_in {
            strip_duplicates(p, is_joined);
        }
        let (lowest_path_idx, is_reversed) = if end_type == EndType::Polygon {
            let (idx, neg) = get_lowest_closed_path_info(&paths_in);
            (idx, idx.is_some() && neg)
        } else {
            (None, false)
        };
        Group {
            paths_in,
            lowest_path_idx,
            is_reversed,
            join_type,
            end_type,
        }
    }
}

#[inline]
fn hypot(x: f64, y: f64) -> f64 {
    (x * x + y * y).sqrt()
}

fn get_unit_normal(pt1: Point64, pt2: Point64) -> PointD {
    if pt1 == pt2 {
        return PointD::new(0.0, 0.0);
    }
    let mut dx = pt2.x.wrapping_sub(pt1.x) as f64;
    let mut dy = pt2.y.wrapping_sub(pt1.y) as f64;
    let inverse_hypot = 1.0 / hypot(dx, dy);
    dx *= inverse_hypot;
    dy *= inverse_hypot;
    PointD::new(dy, -dx)
}

fn normalize_vector(vec: PointD) -> PointD {
    let h = hypot(vec.x, vec.y);
    if h.abs() < 0.001 {
        return PointD::new(0.0, 0.0);
    }
    let inverse_hypot = 1.0 / h;
    PointD::new(vec.x * inverse_hypot, vec.y * inverse_hypot)
}

fn get_avg_unit_vector(vec1: PointD, vec2: PointD) -> PointD {
    normalize_vector(PointD::new(vec1.x + vec2.x, vec1.y + vec2.y))
}

/// `GetPerpendic`: rounds to integers, keeps `pt.z`.
#[inline]
fn get_perpendic(pt: Point64, norm: PointD, delta: f64) -> Point64 {
    pt.with_xy(
        (pt.x as f64 + norm.x * delta).round() as i64,
        (pt.y as f64 + norm.y * delta).round() as i64,
    )
}

#[inline]
fn get_perpendic_d(pt: Point64, norm: PointD, delta: f64) -> PointD {
    pt.with_xy(pt.x as f64 + norm.x * delta, pt.y as f64 + norm.y * delta)
}

/// `Point64(double x, double y[, z])`.
#[inline]
fn p64(x: f64, y: f64, like: Point64) -> Point64 {
    like.with_xy(x.round() as i64, y.round() as i64)
}

/// Offsets closed polygons and open polylines by a distance.
pub struct ClipperOffset {
    groups: Vec<Group>,
    delta: f64,
    group_delta: f64,
    temp_lim: f64,
    steps_per_rad: f64,
    step_sin: f64,
    step_cos: f64,
    norms: PathD,
    path_out: Path64,
    solution: Paths64,
    join_type: JoinType,
    end_type: EndType,
    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: bool,
    reverse_solution: bool,
    #[cfg(feature = "usingz")]
    z_callback: Option<std::rc::Rc<std::cell::RefCell<crate::engine::ZCallback64>>>,
}

impl Default for ClipperOffset {
    /// `miter_limit` 2, `arc_tolerance` 0 (automatic), no collinear preservation, no reversal.
    fn default() -> Self {
        Self::new(2.0, 0.0, false, false)
    }
}

impl ClipperOffset {
    /// An empty offsetter; see the accessors below for each setting.
    pub fn new(
        miter_limit: f64,
        arc_tolerance: f64,
        preserve_collinear: bool,
        reverse_solution: bool,
    ) -> Self {
        ClipperOffset {
            groups: Vec::new(),
            delta: 0.0,
            group_delta: 0.0,
            temp_lim: 0.0,
            steps_per_rad: 0.0,
            step_sin: 0.0,
            step_cos: 0.0,
            norms: Vec::new(),
            path_out: Vec::new(),
            solution: Vec::new(),
            join_type: JoinType::Bevel,
            end_type: EndType::Polygon,
            miter_limit,
            arc_tolerance,
            preserve_collinear,
            reverse_solution,
            #[cfg(feature = "usingz")]
            z_callback: None,
        }
    }

    /// Maximum miter length, in multiples of `delta`, before `JoinType::Miter` squares a join.
    pub fn miter_limit(&self) -> f64 {
        self.miter_limit
    }

    /// Sets [`Self::miter_limit`].
    pub fn set_miter_limit(&mut self, value: f64) {
        self.miter_limit = value;
    }

    /// Maximum deviation of flattened round joins and ends from the true arc; `0` picks
    /// `|delta| / 500`.
    pub fn arc_tolerance(&self) -> f64 {
        self.arc_tolerance
    }

    /// Sets [`Self::arc_tolerance`].
    pub fn set_arc_tolerance(&mut self, value: f64) {
        self.arc_tolerance = value;
    }

    /// Whether the final union keeps collinear vertices.
    pub fn preserve_collinear(&self) -> bool {
        self.preserve_collinear
    }

    /// Sets [`Self::preserve_collinear`].
    pub fn set_preserve_collinear(&mut self, value: bool) {
        self.preserve_collinear = value;
    }

    /// Whether solution paths have reversed orientation.
    pub fn reverse_solution(&self) -> bool {
        self.reverse_solution
    }

    /// Sets [`Self::reverse_solution`].
    pub fn set_reverse_solution(&mut self, value: bool) {
        self.reverse_solution = value;
    }

    /// Sets the callback used for `z` of new vertices the final union creates.
    #[cfg(feature = "usingz")]
    pub fn set_z_callback(&mut self, cb: Option<crate::engine::ZCallback64>) {
        self.z_callback = cb.map(|cb| std::rc::Rc::new(std::cell::RefCell::new(cb)));
    }

    /// Adds one path to be offset with the given join and end types.
    pub fn add_path(&mut self, path: &Path64, join_type: JoinType, end_type: EndType) {
        self.groups
            .push(Group::new(&vec![path.clone()], join_type, end_type));
    }

    /// Adds paths to be offset together with the given join and end types.
    pub fn add_paths(&mut self, paths: &Paths64, join_type: JoinType, end_type: EndType) {
        if paths.is_empty() {
            return;
        }
        self.groups.push(Group::new(paths, join_type, end_type));
    }

    /// Removes every added path.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.norms.clear();
    }

    /// Offsets every added path by `delta` and unions the result.
    pub fn execute(&mut self, delta: f64) -> Result<Paths64, ClipperError> {
        self.execute_internal(delta, None)?;
        Ok(self.union(false)?.0)
    }

    /// Like [`Self::execute`], nesting the result in a [`PolyTree64`].
    pub fn execute_tree(&mut self, delta: f64) -> Result<PolyTree64, ClipperError> {
        self.execute_internal(delta, None)?;
        Ok(self.union(true)?.1)
    }

    /// Offsets with a per-vertex delta from `cb` (`Execute(DeltaCallback64, ...)`).
    pub fn execute_with_callback(&mut self, cb: DeltaCallback64) -> Result<Paths64, ClipperError> {
        self.execute_internal(1.0, Some(cb))?;
        Ok(self.union(false)?.0)
    }

    fn check_reverse_orientation(&self) -> bool {
        self.groups
            .iter()
            .find(|g| g.end_type == EndType::Polygon)
            .is_some_and(|g| g.is_reversed)
    }

    fn execute_internal(
        &mut self,
        delta: f64,
        cb: Option<DeltaCallback64>,
    ) -> Result<(), ClipperError> {
        self.solution.clear();
        if !delta.is_finite() {
            return Err(ClipperError::InvalidDelta);
        }
        for g in &self.groups {
            check_range(&g.paths_in)?;
        }
        if self.groups.is_empty() {
            return Ok(());
        }
        if delta.abs() < 0.5 {
            // the offset is insignificant
            for g in &self.groups {
                self.solution.extend(g.paths_in.iter().cloned());
            }
        } else {
            self.temp_lim = if self.miter_limit <= 1.0 {
                2.0
            } else {
                2.0 / (self.miter_limit * self.miter_limit)
            };
            self.delta = delta;
            let mut invalid_delta = false;
            let mut checked = cb.map(|f| {
                let invalid = &mut invalid_delta;
                move |path: &Path64, norms: &PathD, j: usize, k: usize| {
                    let d = f(path, norms, j, k);
                    if d.is_finite() {
                        d
                    } else {
                        *invalid = true;
                        0.0
                    }
                }
            });
            let mut cb: Option<DeltaCallback64> = checked.as_mut().map(|f| f as DeltaCallback64);
            let groups = std::mem::take(&mut self.groups);
            let result = groups
                .iter()
                .try_for_each(|g| self.do_group_offset(g, &mut cb));
            self.groups = groups;
            if invalid_delta {
                self.solution.clear();
                return Err(ClipperError::InvalidDelta);
            }
            result?;
        }
        Ok(())
    }

    /// The final union (`Positive` or, for reversed input, `Negative` fill).
    fn union(&mut self, tree: bool) -> Result<(Paths64, PolyTree64), ClipperError> {
        if self.solution.is_empty() {
            return Ok((Vec::new(), PolyTree64::default()));
        }
        let paths_reversed = self.check_reverse_orientation();
        let mut c = Clipper64::new();
        c.set_preserve_collinear(self.preserve_collinear);
        c.set_reverse_solution(self.reverse_solution != paths_reversed);
        #[cfg(feature = "usingz")]
        {
            let user = self.z_callback.clone();
            c.set_z_callback(Some(Box::new(
                move |bot1, top1, bot2, top2, ip: &mut Point64| {
                    if bot1.z != 0 && (bot1.z == bot2.z || bot1.z == top2.z) {
                        ip.z = bot1.z;
                    } else if bot2.z != 0 && bot2.z == top1.z {
                        ip.z = bot2.z;
                    } else if top1.z != 0 && top1.z == top2.z {
                        ip.z = top1.z;
                    } else if let Some(cb) = &user {
                        (cb.borrow_mut())(bot1, top1, bot2, top2, ip);
                    }
                },
            )));
        }
        c.add_subject(&self.solution);
        let fill = if paths_reversed {
            FillRule::Negative
        } else {
            FillRule::Positive
        };
        if tree {
            Ok((Vec::new(), c.execute_tree(ClipType::Union, fill)?.0))
        } else {
            Ok((
                c.execute(ClipType::Union, fill)?.closed,
                PolyTree64::default(),
            ))
        }
    }

    fn build_normals(&mut self, path: &Path64) {
        self.norms.clear();
        if path.is_empty() {
            return;
        }
        for w in path.windows(2) {
            self.norms.push(get_unit_normal(w[0], w[1]));
        }
        self.norms
            .push(get_unit_normal(path[path.len() - 1], path[0]));
    }

    fn do_bevel(&mut self, path: &Path64, j: usize, k: usize) {
        let pj = path[j];
        let (x, y) = (pj.x as f64, pj.y as f64);
        let (pt1, pt2) = if j == k {
            let abs_delta = self.group_delta.abs();
            let n = self.norms[j];
            (
                p64(x - abs_delta * n.x, y - abs_delta * n.y, pj),
                p64(x + abs_delta * n.x, y + abs_delta * n.y, pj),
            )
        } else {
            let (nk, nj) = (self.norms[k], self.norms[j]);
            let d = self.group_delta;
            (
                p64(x + d * nk.x, y + d * nk.y, pj),
                p64(x + d * nj.x, y + d * nj.y, pj),
            )
        };
        self.path_out.push(pt1);
        self.path_out.push(pt2);
    }

    fn do_square(&mut self, path: &Path64, j: usize, k: usize) {
        let (nj, nk) = (self.norms[j], self.norms[k]);
        let vec = if j == k {
            PointD::new(nj.y, -nj.x)
        } else {
            get_avg_unit_vector(PointD::new(-nk.y, nk.x), PointD::new(nj.y, -nj.x))
        };
        let gd = self.group_delta;
        let abs_delta = gd.abs();
        let mut pt_q: PointD = path[j].convert();
        pt_q = translate_point(pt_q, abs_delta * vec.x, abs_delta * vec.y);
        let pt1 = translate_point(pt_q, gd * vec.y, gd * -vec.x);
        let pt2 = translate_point(pt_q, gd * -vec.y, gd * vec.x);
        let pt3 = get_perpendic_d(path[k], nk, gd);
        if j == k {
            let pt4 = PointD::new(pt3.x + vec.x * gd, pt3.y + vec.y * gd);
            let pt = get_line_intersect_pt(pt1, pt2, pt3, pt4).unwrap_or(pt_q);
            self.path_out.push(reflect_point(pt, pt_q).convert());
            self.path_out.push(pt.convert());
        } else {
            let pt4 = get_perpendic_d(path[j], nk, gd);
            let pt = get_line_intersect_pt(pt1, pt2, pt3, pt4).unwrap_or(pt_q);
            self.path_out.push(pt.convert());
            self.path_out.push(reflect_point(pt, pt_q).convert());
        }
    }

    fn do_miter(&mut self, path: &Path64, j: usize, k: usize, cos_a: f64) {
        let q = self.group_delta / (cos_a + 1.0);
        let (nk, nj, pj) = (self.norms[k], self.norms[j], path[j]);
        self.path_out.push(p64(
            pj.x as f64 + (nk.x + nj.x) * q,
            pj.y as f64 + (nk.y + nj.y) * q,
            pj,
        ));
    }

    fn set_round_steps(&mut self) {
        let abs_delta = self.group_delta.abs();
        let arc_tol = if self.arc_tolerance > FLOATING_POINT_TOLERANCE {
            abs_delta.min(self.arc_tolerance)
        } else {
            abs_delta * ARC_CONST
        };
        let steps_per_360 = (PI / (1.0 - arc_tol / abs_delta).acos()).min(abs_delta * PI);
        self.step_sin = (2.0 * PI / steps_per_360).sin();
        self.step_cos = (2.0 * PI / steps_per_360).cos();
        if self.group_delta < 0.0 {
            self.step_sin = -self.step_sin;
        }
        self.steps_per_rad = steps_per_360 / (2.0 * PI);
    }

    fn do_round(&mut self, path: &Path64, j: usize, k: usize, angle: f64, has_cb: bool) {
        if has_cb {
            self.set_round_steps();
        }
        let pt = path[j];
        let nk = self.norms[k];
        let mut offset_vec = PointD::new(nk.x * self.group_delta, nk.y * self.group_delta);
        if j == k {
            offset_vec = PointD::new(-offset_vec.x, -offset_vec.y);
        }
        let (px, py) = (pt.x as f64, pt.y as f64);
        self.path_out
            .push(p64(px + offset_vec.x, py + offset_vec.y, pt));
        let steps = (self.steps_per_rad * angle.abs()).ceil() as i32;
        for _ in 1..steps {
            offset_vec = PointD::new(
                offset_vec.x * self.step_cos - self.step_sin * offset_vec.y,
                offset_vec.x * self.step_sin + offset_vec.y * self.step_cos,
            );
            self.path_out
                .push(p64(px + offset_vec.x, py + offset_vec.y, pt));
        }
        self.path_out
            .push(get_perpendic(path[j], self.norms[j], self.group_delta));
    }

    fn offset_point(
        &mut self,
        group: &Group,
        path: &Path64,
        j: usize,
        k: usize,
        cb: &mut Option<DeltaCallback64>,
    ) {
        if path[j] == path[k] {
            return;
        }
        let (nj, nk) = (self.norms[j], self.norms[k]);
        let mut sin_a = cross_product_vec(nj, nk);
        let cos_a = dot_product_vec(nj, nk);
        sin_a = sin_a.clamp(-1.0, 1.0);
        if let Some(cb) = cb.as_deref_mut() {
            self.group_delta = cb(path, &self.norms, j, k);
            if group.is_reversed {
                self.group_delta = -self.group_delta;
            }
        }
        if self.group_delta.abs() <= FLOATING_POINT_TOLERANCE {
            self.path_out.push(path[j]);
            return;
        }
        if cos_a > -0.999 && sin_a * self.group_delta < 0.0 {
            // concave (#593)
            self.path_out
                .push(get_perpendic(path[j], nk, self.group_delta));
            self.path_out.push(path[j]); // #405, #873, #916
            self.path_out
                .push(get_perpendic(path[j], nj, self.group_delta));
        } else if cos_a > 0.999 && self.join_type != JoinType::Round {
            self.do_miter(path, j, k, cos_a);
        } else if self.join_type == JoinType::Miter {
            if cos_a > self.temp_lim - 1.0 {
                self.do_miter(path, j, k, cos_a);
            } else {
                self.do_square(path, j, k);
            }
        } else if self.join_type == JoinType::Round {
            self.do_round(path, j, k, sin_a.atan2(cos_a), cb.is_some());
        } else if self.join_type == JoinType::Bevel {
            self.do_bevel(path, j, k);
        } else {
            self.do_square(path, j, k);
        }
    }

    fn offset_polygon(&mut self, group: &Group, path: &Path64, cb: &mut Option<DeltaCallback64>) {
        self.path_out.clear();
        let mut k = path.len() - 1;
        for j in 0..path.len() {
            self.offset_point(group, path, j, k, cb);
            k = j;
        }
        self.solution.push(std::mem::take(&mut self.path_out));
    }

    fn offset_open_joined(
        &mut self,
        group: &Group,
        path: &Path64,
        cb: &mut Option<DeltaCallback64>,
    ) {
        self.offset_polygon(group, path, cb);
        let reverse_path: Path64 = path.iter().rev().copied().collect();
        self.norms.reverse();
        self.norms.rotate_left(1);
        for n in &mut self.norms {
            n.x = -n.x;
            n.y = -n.y;
        }
        self.offset_polygon(group, &reverse_path, cb);
    }

    fn cap(&mut self, path: &Path64, i: usize, cb: &mut Option<DeltaCallback64>) {
        if let Some(cb) = cb.as_deref_mut() {
            self.group_delta = cb(path, &self.norms, i, i);
        }
        if self.group_delta.abs() <= FLOATING_POINT_TOLERANCE {
            self.path_out.push(path[i]);
            return;
        }
        match self.end_type {
            EndType::Butt => self.do_bevel(path, i, i),
            EndType::Round => self.do_round(path, i, i, PI, cb.is_some()),
            _ => self.do_square(path, i, i),
        }
    }

    fn offset_open_path(&mut self, group: &Group, path: &Path64, cb: &mut Option<DeltaCallback64>) {
        self.cap(path, 0, cb);
        let high_i = path.len() - 1;
        let mut k = 0;
        for j in 1..high_i {
            self.offset_point(group, path, j, k, cb);
            k = j;
        }
        for i in (1..=high_i).rev() {
            self.norms[i] = PointD::new(-self.norms[i - 1].x, -self.norms[i - 1].y);
        }
        self.norms[0] = self.norms[high_i];
        self.cap(path, high_i, cb);
        let mut k = high_i;
        let mut j = high_i.wrapping_sub(1);
        while j > 0 && j != usize::MAX {
            self.offset_point(group, path, j, k, cb);
            k = j;
            j -= 1;
        }
        self.solution.push(std::mem::take(&mut self.path_out));
    }

    fn do_group_offset(
        &mut self,
        group: &Group,
        cb: &mut Option<DeltaCallback64>,
    ) -> Result<(), ClipperError> {
        if group.end_type == EndType::Polygon {
            // KiCad port patch: like upstream, a group without a lowest path (all paths have zero
            // area) makes delta_ positive for this *and every later* group.
            if group.lowest_path_idx.is_none() {
                self.delta = self.delta.abs();
            }
            let delta = self.delta;
            self.group_delta = if group.is_reversed { -delta } else { delta };
        } else {
            self.group_delta = self.delta.abs();
        }
        let mut abs_delta = self.group_delta.abs();
        self.join_type = group.join_type;
        self.end_type = group.end_type;
        if group.join_type == JoinType::Round || group.end_type == EndType::Round {
            self.set_round_steps();
        }

        for path in &group.paths_in {
            let path_len = path.len();
            self.path_out.clear();
            if path_len == 1 {
                if let Some(cb) = cb.as_deref_mut() {
                    self.group_delta = cb(path, &self.norms, 0, 0);
                    if group.is_reversed {
                        self.group_delta = -self.group_delta;
                    }
                    abs_delta = self.group_delta.abs();
                }
                if self.group_delta < 1.0 {
                    continue;
                }
                if abs_delta.ceil() >= MAX_COORD as f64 {
                    return Err(ClipperError::CoordinateOutOfRange);
                }
                let pt = path[0];
                let mut out = if group.join_type == JoinType::Round {
                    let steps = if self.steps_per_rad > 0.0 {
                        (self.steps_per_rad * 2.0 * PI).ceil() as usize
                    } else {
                        0
                    };
                    ellipse(pt, abs_delta, abs_delta, steps)
                } else {
                    let d = abs_delta.ceil() as i64;
                    Rect64::new(pt.x - d, pt.y - d, pt.x + d, pt.y + d).as_path()
                };
                for p in &mut out {
                    p.set_z(pt.z_value());
                }
                self.solution.push(out);
                continue;
            }
            if path_len == 2 && group.end_type == EndType::Joined {
                self.end_type = if group.join_type == JoinType::Round {
                    EndType::Round
                } else {
                    EndType::Square
                };
            }
            self.build_normals(path);
            match self.end_type {
                EndType::Polygon => self.offset_polygon(group, path, cb),
                EndType::Joined => self.offset_open_joined(group, path, cb),
                _ => self.offset_open_path(group, path, cb),
            }
        }
        Ok(())
    }
}

/// Offsets paths by `delta` in one call (`InflatePaths`).
pub fn inflate_paths(
    paths: &Paths64,
    delta: f64,
    join_type: JoinType,
    end_type: EndType,
    miter_limit: f64,
    arc_tolerance: f64,
) -> Result<Paths64, ClipperError> {
    if delta == 0.0 {
        check_range(paths)?;
        return Ok(paths.clone());
    }
    let mut co = ClipperOffset::new(miter_limit, arc_tolerance, false, false);
    co.add_paths(paths, join_type, end_type);
    co.execute(delta)
}

/// Floating-point [`inflate_paths`]; coordinates are scaled by `10^precision`.
pub fn inflate_paths_d(
    paths: &crate::PathsD,
    delta: f64,
    join_type: JoinType,
    end_type: EndType,
    miter_limit: f64,
    precision: i32,
    arc_tolerance: f64,
) -> Result<crate::PathsD, ClipperError> {
    let scale = crate::core::check_precision(precision)?;
    let scaled = crate::core::scale_paths(paths, scale, scale)?;
    if delta == 0.0 {
        return Ok(paths.clone());
    }
    let mut co = ClipperOffset::new(miter_limit, arc_tolerance * scale, false, false);
    co.add_paths(&scaled, join_type, end_type);
    let solution = co.execute(delta * scale)?;
    crate::core::scale_paths(&solution, 1.0 / scale, 1.0 / scale)
}
