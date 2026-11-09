//! Fast clipping against an axis-aligned rectangle (`clipper.rectclip.cpp`).

use crate::core::{
    ClipperError, Path64, Paths64, PathsD, Point64, PointInPolygonResult, Rect64, RectD,
    check_precision, cross_product_sign_t, get_bounds, get_line_intersect_pt, is_collinear,
    point_in_polygon, scale_paths,
};

type Idx = u32;
const NONE: Idx = u32::MAX;
const NO_EDGE: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Location {
    Left = 0,
    Top = 1,
    Right = 2,
    Bottom = 3,
    Inside = 4,
}

impl Location {
    fn from_index(i: u8) -> Self {
        match i {
            0 => Location::Left,
            1 => Location::Top,
            2 => Location::Right,
            3 => Location::Bottom,
            _ => Location::Inside,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct OutPt2 {
    pt: Point64,
    owner_idx: usize,
    edge: u8,
    next: Idx,
    prev: Idx,
}

/// `Path1ContainsPath2` (the rectclip variant).
fn path1_contains_path2(path1: &Path64, path2: &Path64) -> bool {
    let mut io_count: i32 = 0;
    for &pt in path2 {
        match point_in_polygon(pt, path1) {
            PointInPolygonResult::IsOutside => io_count += 1,
            PointInPolygonResult::IsInside => io_count -= 1,
            PointInPolygonResult::IsOn => continue,
        }
        if io_count.abs() > 1 {
            break;
        }
    }
    io_count <= 0
}

/// `GetLocation`: false (with the edge in `loc`) when `pt` lies on the rectangle's boundary.
fn get_location(rec: &Rect64, pt: Point64, loc: &mut Location) -> bool {
    if pt.x == rec.left && pt.y >= rec.top && pt.y <= rec.bottom {
        *loc = Location::Left;
        return false;
    } else if pt.x == rec.right && pt.y >= rec.top && pt.y <= rec.bottom {
        *loc = Location::Right;
        return false;
    } else if pt.y == rec.top && pt.x >= rec.left && pt.x <= rec.right {
        *loc = Location::Top;
        return false;
    } else if pt.y == rec.bottom && pt.x >= rec.left && pt.x <= rec.right {
        *loc = Location::Bottom;
        return false;
    } else if pt.x < rec.left {
        *loc = Location::Left;
    } else if pt.x > rec.right {
        *loc = Location::Right;
    } else if pt.y < rec.top {
        *loc = Location::Top;
    } else if pt.y > rec.bottom {
        *loc = Location::Bottom;
    } else {
        *loc = Location::Inside;
    }
    true
}

#[inline]
fn is_horizontal(pt1: Point64, pt2: Point64) -> bool {
    pt1.y == pt2.y
}

/// `GetSegmentIntersection`. Like the C++ out-parameter, `ip` is left alone when no branch sets it.
fn get_segment_intersection(
    p1: Point64,
    p2: Point64,
    p3: Point64,
    p4: Point64,
    ip: &mut Point64,
) -> bool {
    let res1 = cross_product_sign_t(p1, p3, p4);
    let res2 = cross_product_sign_t(p2, p3, p4);
    if res1 == 0 {
        *ip = p1;
        if res2 == 0 {
            return false; // collinear
        } else if p1 == p3 || p1 == p4 {
            return true;
        } else if is_horizontal(p3, p4) {
            return (p1.x > p3.x) == (p1.x < p4.x);
        } else {
            return (p1.y > p3.y) == (p1.y < p4.y);
        }
    } else if res2 == 0 {
        *ip = p2;
        if p2 == p3 || p2 == p4 {
            return true;
        } else if is_horizontal(p3, p4) {
            return (p2.x > p3.x) == (p2.x < p4.x);
        } else {
            return (p2.y > p3.y) == (p2.y < p4.y);
        }
    }
    if (res1 > 0) == (res2 > 0) {
        return false;
    }
    let res3 = cross_product_sign_t(p3, p1, p2);
    let res4 = cross_product_sign_t(p4, p1, p2);
    if res3 == 0 {
        *ip = p3;
        if p3 == p1 || p3 == p2 {
            return true;
        } else if is_horizontal(p1, p2) {
            return (p3.x > p1.x) == (p3.x < p2.x);
        } else {
            return (p3.y > p1.y) == (p3.y < p2.y);
        }
    } else if res4 == 0 {
        *ip = p4;
        if p4 == p1 || p4 == p2 {
            return true;
        } else if is_horizontal(p1, p2) {
            return (p4.x > p1.x) == (p4.x < p2.x);
        } else {
            return (p4.y > p1.y) == (p4.y < p2.y);
        }
    }
    if (res3 > 0) == (res4 > 0) {
        return false;
    }
    match get_line_intersect_pt(p1, p2, p3, p4) {
        Some(p) => {
            *ip = p;
            true
        }
        None => false,
    }
}

fn get_intersection(
    rp: &Path64,
    p: Point64,
    p2: Point64,
    loc: &mut Location,
    ip: &mut Point64,
) -> bool {
    let gsi =
        |a: usize, b: usize, ip: &mut Point64| get_segment_intersection(p, p2, rp[a], rp[b], ip);
    match *loc {
        Location::Left => {
            if gsi(0, 3, ip) {
                true
            } else if p.y < rp[0].y && gsi(0, 1, ip) {
                *loc = Location::Top;
                true
            } else if gsi(2, 3, ip) {
                *loc = Location::Bottom;
                true
            } else {
                false
            }
        }
        Location::Top => {
            if gsi(0, 1, ip) {
                true
            } else if p.x < rp[0].x && gsi(0, 3, ip) {
                *loc = Location::Left;
                true
            } else if gsi(1, 2, ip) {
                *loc = Location::Right;
                true
            } else {
                false
            }
        }
        Location::Right => {
            if gsi(1, 2, ip) {
                true
            } else if p.y < rp[1].y && gsi(0, 1, ip) {
                *loc = Location::Top;
                true
            } else if gsi(2, 3, ip) {
                *loc = Location::Bottom;
                true
            } else {
                false
            }
        }
        Location::Bottom => {
            if gsi(2, 3, ip) {
                true
            } else if p.x < rp[3].x && gsi(0, 3, ip) {
                *loc = Location::Left;
                true
            } else if gsi(1, 2, ip) {
                *loc = Location::Right;
                true
            } else {
                false
            }
        }
        Location::Inside => {
            if gsi(0, 3, ip) {
                *loc = Location::Left;
                true
            } else if gsi(0, 1, ip) {
                *loc = Location::Top;
                true
            } else if gsi(1, 2, ip) {
                *loc = Location::Right;
                true
            } else if gsi(2, 3, ip) {
                *loc = Location::Bottom;
                true
            } else {
                false
            }
        }
    }
}

#[inline]
fn get_adjacent_location(loc: Location, is_clockwise: bool) -> Location {
    let delta = if is_clockwise { 1 } else { 3 };
    Location::from_index(((loc as u8) + delta) % 4)
}

#[inline]
fn heading_clockwise(prev: Location, curr: Location) -> bool {
    ((prev as u8) + 1) % 4 == curr as u8
}

#[inline]
fn are_opposites(prev: Location, curr: Location) -> bool {
    ((prev as i32) - (curr as i32)).abs() == 2
}

fn is_clockwise(
    prev: Location,
    curr: Location,
    prev_pt: Point64,
    curr_pt: Point64,
    rect_mp: Point64,
) -> bool {
    if are_opposites(prev, curr) {
        cross_product_sign_t(prev_pt, rect_mp, curr_pt) < 0
    } else {
        heading_clockwise(prev, curr)
    }
}

fn get_edges_for_pt(pt: Point64, rec: &Rect64) -> u32 {
    let mut result = 0;
    if pt.x == rec.left {
        result = 1;
    } else if pt.x == rec.right {
        result = 4;
    }
    if pt.y == rec.top {
        result += 2;
    } else if pt.y == rec.bottom {
        result += 8;
    }
    result
}

fn is_heading_clockwise(pt1: Point64, pt2: Point64, edge_idx: usize) -> bool {
    match edge_idx {
        0 => pt2.y < pt1.y,
        1 => pt2.x > pt1.x,
        2 => pt2.y > pt1.y,
        _ => pt2.x < pt1.x,
    }
}

#[inline]
fn has_horz_overlap(left1: Point64, right1: Point64, left2: Point64, right2: Point64) -> bool {
    left1.x < right2.x && right1.x > left2.x
}

#[inline]
fn has_vert_overlap(top1: Point64, bottom1: Point64, top2: Point64, bottom2: Point64) -> bool {
    top1.y < bottom2.y && bottom1.y > top2.y
}

fn start_locs_are_clockwise(startlocs: &[Location]) -> bool {
    let mut result = 0;
    for w in startlocs.windows(2) {
        match (w[1] as i32) - (w[0] as i32) {
            -1 => result -= 1,
            1 => result += 1,
            -3 => result += 1,
            3 => result -= 1,
            _ => {}
        }
    }
    result > 0
}

/// Shared state of [`RectClip64`] and [`RectClipLines64`].
struct RectClipBase {
    rect: Rect64,
    rect_as_path: Path64,
    rect_mp: Point64,
    path_bounds: Rect64,
    ops: Vec<OutPt2>,
    results: Vec<Idx>,
    edges: [Vec<Idx>; 8],
    start_locs: Vec<Location>,
}

impl RectClipBase {
    fn new(rect: Rect64) -> Self {
        RectClipBase {
            rect,
            rect_as_path: rect.as_path(),
            rect_mp: rect.mid_point(),
            path_bounds: Rect64::default(),
            ops: Vec::new(),
            results: Vec::new(),
            edges: Default::default(),
            start_locs: Vec::new(),
        }
    }

    fn reset(&mut self) {
        self.ops.clear();
        self.results.clear();
        for e in &mut self.edges {
            e.clear();
        }
        self.start_locs.clear();
    }

    #[inline]
    fn op(&self, i: Idx) -> &OutPt2 {
        &self.ops[i as usize]
    }

    #[inline]
    fn opm(&mut self, i: Idx) -> &mut OutPt2 {
        &mut self.ops[i as usize]
    }

    fn add(&mut self, pt: Point64, start_new: bool) -> Idx {
        let curr_idx = self.results.len();
        if curr_idx == 0 || start_new {
            let i = self.ops.len() as Idx;
            self.ops.push(OutPt2 {
                pt,
                owner_idx: 0,
                edge: NO_EDGE,
                next: i,
                prev: i,
            });
            self.results.push(i);
            return i;
        }
        let curr_idx = curr_idx - 1;
        let prev_op = self.results[curr_idx];
        if self.op(prev_op).pt == pt {
            return prev_op;
        }
        let i = self.ops.len() as Idx;
        let next = self.op(prev_op).next;
        self.ops.push(OutPt2 {
            pt,
            owner_idx: curr_idx,
            edge: NO_EDGE,
            next,
            prev: prev_op,
        });
        self.opm(next).prev = i;
        self.opm(prev_op).next = i;
        self.results[curr_idx] = i;
        i
    }

    fn add_corner_between(&mut self, prev: Location, curr: Location) {
        let corner = if heading_clockwise(prev, curr) {
            prev
        } else {
            curr
        };
        self.add(self.rect_as_path[corner as usize], false);
    }

    fn add_corner(&mut self, loc: &mut Location, is_clockwise: bool) {
        if is_clockwise {
            self.add(self.rect_as_path[*loc as usize], false);
            *loc = get_adjacent_location(*loc, true);
        } else {
            *loc = get_adjacent_location(*loc, false);
            self.add(self.rect_as_path[*loc as usize], false);
        }
    }

    fn get_next_location(
        &mut self,
        path: &Path64,
        loc: &mut Location,
        i: &mut usize,
        high_i: usize,
    ) {
        let r = self.rect;
        match *loc {
            Location::Left => {
                while *i <= high_i && path[*i].x <= r.left {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                let p = path[*i];
                *loc = if p.x >= r.right {
                    Location::Right
                } else if p.y <= r.top {
                    Location::Top
                } else if p.y >= r.bottom {
                    Location::Bottom
                } else {
                    Location::Inside
                };
            }
            Location::Top => {
                while *i <= high_i && path[*i].y <= r.top {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                let p = path[*i];
                *loc = if p.y >= r.bottom {
                    Location::Bottom
                } else if p.x <= r.left {
                    Location::Left
                } else if p.x >= r.right {
                    Location::Right
                } else {
                    Location::Inside
                };
            }
            Location::Right => {
                while *i <= high_i && path[*i].x >= r.right {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                let p = path[*i];
                *loc = if p.x <= r.left {
                    Location::Left
                } else if p.y <= r.top {
                    Location::Top
                } else if p.y >= r.bottom {
                    Location::Bottom
                } else {
                    Location::Inside
                };
            }
            Location::Bottom => {
                while *i <= high_i && path[*i].y >= r.bottom {
                    *i += 1;
                }
                if *i > high_i {
                    return;
                }
                let p = path[*i];
                *loc = if p.y <= r.top {
                    Location::Top
                } else if p.x <= r.left {
                    Location::Left
                } else if p.x >= r.right {
                    Location::Right
                } else {
                    Location::Inside
                };
            }
            Location::Inside => {
                while *i <= high_i {
                    let p = path[*i];
                    if p.x < r.left {
                        *loc = Location::Left;
                    } else if p.x > r.right {
                        *loc = Location::Right;
                    } else if p.y > r.bottom {
                        *loc = Location::Bottom;
                    } else if p.y < r.top {
                        *loc = Location::Top;
                    } else {
                        self.add(p, false);
                        *i += 1;
                        continue;
                    }
                    break;
                }
            }
        }
    }

    fn add_to_edge(&mut self, edge: usize, op: Idx) {
        if self.op(op).edge != NO_EDGE {
            return;
        }
        self.opm(op).edge = edge as u8;
        self.edges[edge].push(op);
    }

    fn uncouple_edge(&mut self, op: Idx) {
        let edge = self.op(op).edge;
        if edge == NO_EDGE {
            return;
        }
        let list = &mut self.edges[edge as usize];
        if let Some(slot) = list.iter_mut().find(|e| **e == op) {
            *slot = NONE;
        }
        self.opm(op).edge = NO_EDGE;
    }

    fn set_new_owner(&mut self, op: Idx, new_idx: usize) {
        self.opm(op).owner_idx = new_idx;
        let mut op2 = self.op(op).next;
        while op2 != op {
            self.opm(op2).owner_idx = new_idx;
            op2 = self.op(op2).next;
        }
    }

    fn unlink_op(&mut self, op: Idx) -> Idx {
        let o = *self.op(op);
        if o.next == op {
            return NONE;
        }
        self.opm(o.prev).next = o.next;
        self.opm(o.next).prev = o.prev;
        o.next
    }

    fn unlink_op_back(&mut self, op: Idx) -> Idx {
        let o = *self.op(op);
        if o.next == op {
            return NONE;
        }
        self.opm(o.prev).next = o.next;
        self.opm(o.next).prev = o.prev;
        o.prev
    }

    fn execute_internal(&mut self, path: &Path64) {
        if path.is_empty() {
            return;
        }
        let high_i = path.len() - 1;
        let mut prev = Location::Inside;
        let mut loc = Location::Inside;
        let mut crossing_loc = Location::Inside;
        let mut first_cross = Location::Inside;
        if !get_location(&self.rect, path[high_i], &mut loc) {
            let mut i = high_i;
            while i > 0 && !get_location(&self.rect, path[i - 1], &mut prev) {
                i -= 1;
            }
            if i == 0 {
                for &pt in path {
                    self.add(pt, false);
                }
                return;
            }
            if prev == Location::Inside {
                loc = Location::Inside;
            }
        }
        let starting_loc = loc;

        let mut i = 0;
        while i <= high_i {
            prev = loc;
            let crossing_prev = crossing_loc;
            self.get_next_location(path, &mut loc, &mut i, high_i);
            if i > high_i {
                break;
            }
            let mut ip = Point64::default();
            let mut ip2 = Point64::default();
            let prev_pt = if i > 0 { path[i - 1] } else { path[high_i] };
            crossing_loc = loc;
            if !get_intersection(
                &self.rect_as_path,
                path[i],
                prev_pt,
                &mut crossing_loc,
                &mut ip,
            ) {
                if crossing_prev == Location::Inside {
                    let is_clockw = is_clockwise(prev, loc, prev_pt, path[i], self.rect_mp);
                    loop {
                        self.start_locs.push(prev);
                        prev = get_adjacent_location(prev, is_clockw);
                        if prev == loc {
                            break;
                        }
                    }
                    crossing_loc = crossing_prev; // still not crossed
                } else if prev != Location::Inside && prev != loc {
                    let is_clockw = is_clockwise(prev, loc, prev_pt, path[i], self.rect_mp);
                    loop {
                        self.add_corner(&mut prev, is_clockw);
                        if prev == loc {
                            break;
                        }
                    }
                }
                i += 1;
                continue;
            }

            if loc == Location::Inside {
                // entering the rectangle
                if first_cross == Location::Inside {
                    first_cross = crossing_loc;
                    self.start_locs.push(prev);
                } else if prev != crossing_loc {
                    let is_clockw =
                        is_clockwise(prev, crossing_loc, prev_pt, path[i], self.rect_mp);
                    loop {
                        self.add_corner(&mut prev, is_clockw);
                        if prev == crossing_loc {
                            break;
                        }
                    }
                }
            } else if prev != Location::Inside {
                // passing right through the rectangle
                loc = prev;
                get_intersection(&self.rect_as_path, prev_pt, path[i], &mut loc, &mut ip2);
                if crossing_prev != Location::Inside && crossing_prev != loc {
                    self.add_corner_between(crossing_prev, loc); // #579
                }
                if first_cross == Location::Inside {
                    first_cross = loc;
                    self.start_locs.push(prev);
                }
                loc = crossing_loc;
                self.add(ip2, false);
                if ip == ip2 {
                    get_location(&self.rect, path[i], &mut loc);
                    self.add_corner_between(crossing_loc, loc);
                    crossing_loc = loc;
                    continue;
                }
            } else {
                // exiting the rectangle
                loc = crossing_loc;
                if first_cross == Location::Inside {
                    first_cross = crossing_loc;
                }
            }
            self.add(ip, false);
        }

        if first_cross == Location::Inside {
            if starting_loc != Location::Inside
                && self.path_bounds.contains_rect(&self.rect)
                && path1_contains_path2(path, &self.rect_as_path)
            {
                let is_clockwise_path = start_locs_are_clockwise(&self.start_locs);
                for j in 0..4 {
                    let k = if is_clockwise_path { j } else { 3 - j }; // reverses the result path
                    self.add(self.rect_as_path[k], false);
                    let first = self.results[0];
                    self.add_to_edge(k * 2, first);
                }
            }
        } else if loc != Location::Inside && (loc != first_cross || self.start_locs.len() > 2) {
            if !self.start_locs.is_empty() {
                prev = loc;
                let start_locs = self.start_locs.clone();
                for loc2 in start_locs {
                    if prev == loc2 {
                        continue;
                    }
                    let cw = heading_clockwise(prev, loc2);
                    self.add_corner(&mut prev, cw);
                    prev = loc2;
                }
                loc = prev;
            }
            if loc != first_cross {
                let cw = heading_clockwise(loc, first_cross);
                self.add_corner(&mut loc, cw);
            }
        }
    }

    fn check_edges(&mut self) {
        for i in 0..self.results.len() {
            let mut op = self.results[i];
            if op == NONE {
                continue;
            }
            let mut op2 = op;
            loop {
                let o = *self.op(op2);
                if is_collinear(self.op(o.prev).pt, o.pt, self.op(o.next).pt) {
                    if op2 == op {
                        op2 = self.unlink_op_back(op2);
                        if op2 == NONE {
                            break;
                        }
                        op = self.op(op2).prev;
                    } else {
                        op2 = self.unlink_op_back(op2);
                        if op2 == NONE {
                            break;
                        }
                    }
                } else {
                    op2 = o.next;
                }
                if op2 == op {
                    break;
                }
            }
            if op2 == NONE {
                self.results[i] = NONE;
                continue;
            }
            self.results[i] = op;

            let mut edge_set1 = get_edges_for_pt(self.op(self.op(op).prev).pt, &self.rect);
            op2 = op;
            loop {
                let edge_set2 = get_edges_for_pt(self.op(op2).pt, &self.rect);
                if edge_set2 != 0 && self.op(op2).edge == NO_EDGE {
                    let combined = edge_set1 & edge_set2;
                    for j in 0..4 {
                        if combined & (1 << j) != 0 {
                            let (a, b) = (self.op(self.op(op2).prev).pt, self.op(op2).pt);
                            if is_heading_clockwise(a, b, j) {
                                self.add_to_edge(j * 2, op2);
                            } else {
                                self.add_to_edge(j * 2 + 1, op2);
                            }
                        }
                    }
                }
                edge_set1 = edge_set2;
                op2 = self.op(op2).next;
                if op2 == op {
                    break;
                }
            }
        }
    }

    fn tidy_edges(&mut self, idx: usize) {
        let (cw, ccw) = (idx * 2, idx * 2 + 1);
        if self.edges[ccw].is_empty() {
            return;
        }
        let is_horz = idx == 1 || idx == 3;
        let cw_is_toward_larger = idx == 1 || idx == 2;
        let (mut i, mut j) = (0usize, 0usize);
        while i < self.edges[cw].len() {
            let p = self.edges[cw][i];
            if p == NONE || self.op(p).next == self.op(p).prev {
                self.edges[cw][i] = NONE;
                i += 1;
                j = 0;
                continue;
            }
            let j_lim = self.edges[ccw].len();
            while j < j_lim && {
                let q = self.edges[ccw][j];
                q == NONE || self.op(q).next == self.op(q).prev
            } {
                j += 1;
            }
            if j == j_lim {
                i += 1;
                j = 0;
                continue;
            }
            let (ci, cj) = (self.edges[cw][i], self.edges[ccw][j]);
            let (p1, p1a, p2, p2a) = if cw_is_toward_larger {
                (self.op(ci).prev, ci, cj, self.op(cj).prev)
            } else {
                (ci, self.op(ci).prev, self.op(cj).prev, cj)
            };
            let (a, b, c, d) = (
                self.op(p1).pt,
                self.op(p1a).pt,
                self.op(p2).pt,
                self.op(p2a).pt,
            );
            if (is_horz && !has_horz_overlap(a, b, c, d))
                || (!is_horz && !has_vert_overlap(a, b, c, d))
            {
                j += 1;
                continue;
            }

            let is_rejoining = self.op(ci).owner_idx != self.op(cj).owner_idx;
            if is_rejoining {
                let owner = self.op(p2).owner_idx;
                self.results[owner] = NONE;
                let new_owner = self.op(p1).owner_idx;
                self.set_new_owner(p2, new_owner);
            }
            if cw_is_toward_larger {
                self.opm(p1).next = p2;
                self.opm(p2).prev = p1;
                self.opm(p1a).prev = p2a;
                self.opm(p2a).next = p1a;
            } else {
                self.opm(p1).prev = p2;
                self.opm(p2).next = p1;
                self.opm(p1a).next = p2a;
                self.opm(p2a).prev = p1a;
            }
            if !is_rejoining {
                let new_idx = self.results.len();
                self.results.push(p1a);
                self.set_new_owner(p1a, new_idx);
            }
            let (op, op2) = if cw_is_toward_larger {
                (p2, p1a)
            } else {
                (p1, p2a)
            };
            let (o1, o2) = (self.op(op).owner_idx, self.op(op2).owner_idx);
            self.results[o1] = op;
            self.results[o2] = op2;

            let (op_is_larger, op2_is_larger) = {
                let (o, op_prev) = (self.op(op).pt, self.op(self.op(op).prev).pt);
                let (o2, op2_prev) = (self.op(op2).pt, self.op(self.op(op2).prev).pt);
                if is_horz {
                    (o.x > op_prev.x, o2.x > op2_prev.x)
                } else {
                    (o.y > op_prev.y, o2.y > op2_prev.y)
                }
            };

            let degenerate = |s: &Self, x: Idx| {
                s.op(x).next == s.op(x).prev || s.op(x).pt == s.op(s.op(x).prev).pt
            };
            if degenerate(self, op) {
                if op2_is_larger == cw_is_toward_larger {
                    self.edges[cw][i] = op2;
                    self.edges[ccw][j] = NONE;
                    j += 1;
                } else {
                    self.edges[ccw][j] = op2;
                    self.edges[cw][i] = NONE;
                    i += 1;
                }
            } else if degenerate(self, op2) {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw][i] = op;
                    self.edges[ccw][j] = NONE;
                    j += 1;
                } else {
                    self.edges[ccw][j] = op;
                    self.edges[cw][i] = NONE;
                    i += 1;
                }
            } else if op_is_larger == op2_is_larger {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw][i] = op;
                    self.uncouple_edge(op2);
                    self.add_to_edge(cw, op2);
                    self.edges[ccw][j] = NONE;
                    j += 1;
                } else {
                    self.edges[cw][i] = NONE;
                    i += 1;
                    self.edges[ccw][j] = op2;
                    self.uncouple_edge(op);
                    self.add_to_edge(ccw, op);
                    j = 0;
                }
            } else {
                if op_is_larger == cw_is_toward_larger {
                    self.edges[cw][i] = op;
                } else {
                    self.edges[ccw][j] = op;
                }
                if op2_is_larger == cw_is_toward_larger {
                    self.edges[cw][i] = op2;
                } else {
                    self.edges[ccw][j] = op2;
                }
            }
        }
    }

    /// `RectClip64::GetPath`: removes collinear points; updates the result slot.
    fn get_path(&mut self, slot: usize) -> Path64 {
        let mut op = self.results[slot];
        if op == NONE || self.op(op).next == self.op(op).prev {
            return Vec::new();
        }
        let mut op2 = self.op(op).next;
        while op2 != NONE && op2 != op {
            let o = *self.op(op2);
            if is_collinear(self.op(o.prev).pt, o.pt, self.op(o.next).pt) {
                op = o.prev;
                op2 = self.unlink_op(op2);
            } else {
                op2 = o.next;
            }
        }
        self.results[slot] = op2;
        if op2 == NONE {
            return Vec::new();
        }
        let op = op2;
        let mut result = vec![self.op(op).pt];
        let mut op2 = self.op(op).next;
        while op2 != op {
            result.push(self.op(op2).pt);
            op2 = self.op(op2).next;
        }
        result
    }

    fn execute_internal_lines(&mut self, path: &Path64) {
        if self.rect.is_empty() || path.len() < 2 {
            return;
        }
        self.reset();
        let mut i = 1;
        let high_i = path.len() - 1;
        let mut prev = Location::Inside;
        let mut loc = Location::Inside;
        if !get_location(&self.rect, path[0], &mut loc) {
            while i <= high_i && !get_location(&self.rect, path[i], &mut prev) {
                i += 1;
            }
            if i > high_i {
                for &pt in path {
                    self.add(pt, false);
                }
                return;
            }
            if prev == Location::Inside {
                loc = Location::Inside;
            }
            i = 1;
        }
        if loc == Location::Inside {
            self.add(path[0], false);
        }
        while i <= high_i {
            prev = loc;
            self.get_next_location(path, &mut loc, &mut i, high_i);
            if i > high_i {
                break;
            }
            let mut ip = Point64::default();
            let mut ip2 = Point64::default();
            let prev_pt = path[i - 1];
            let mut crossing_loc = loc;
            if !get_intersection(
                &self.rect_as_path,
                path[i],
                prev_pt,
                &mut crossing_loc,
                &mut ip,
            ) {
                i += 1;
                continue;
            }
            if loc == Location::Inside {
                // entering the rectangle
                self.add(ip, true);
            } else if prev != Location::Inside {
                // passing right through the rectangle
                crossing_loc = prev;
                get_intersection(
                    &self.rect_as_path,
                    prev_pt,
                    path[i],
                    &mut crossing_loc,
                    &mut ip2,
                );
                self.add(ip2, true);
                self.add(ip, false);
            } else {
                // exiting the rectangle
                self.add(ip, false);
            }
        }
    }

    /// `RectClipLines64::GetPath`.
    fn get_path_lines(&self, slot: usize) -> Path64 {
        let op = self.results[slot];
        if op == NONE || op == self.op(op).next {
            return Vec::new();
        }
        let op = self.op(op).next; // the path's beginning
        let mut result = vec![self.op(op).pt];
        let mut op2 = self.op(op).next;
        while op2 != op {
            result.push(self.op(op2).pt);
            op2 = self.op(op2).next;
        }
        result
    }
}

/// Clips closed polygons to a rectangle; much faster than a general boolean intersection.
pub struct RectClip64 {
    base: RectClipBase,
}

impl RectClip64 {
    /// A clipper for `rect`.
    pub fn new(rect: Rect64) -> Self {
        RectClip64 {
            base: RectClipBase::new(rect),
        }
    }

    /// Clips each closed path to the rectangle.
    pub fn execute(&mut self, paths: &Paths64) -> Paths64 {
        let b = &mut self.base;
        let mut result = Vec::new();
        if b.rect.is_empty() {
            return result;
        }
        for path in paths {
            if path.len() < 3 {
                continue;
            }
            b.path_bounds = get_bounds(path);
            if !b.rect.intersects(&b.path_bounds) {
                continue; // completely outside
            } else if b.rect.contains_rect(&b.path_bounds) {
                result.push(path.clone());
                continue;
            }
            b.execute_internal(path);
            b.check_edges();
            for i in 0..4 {
                b.tidy_edges(i);
            }
            for slot in 0..b.results.len() {
                let tmp = b.get_path(slot);
                if !tmp.is_empty() {
                    result.push(tmp);
                }
            }
            b.reset();
        }
        result
    }
}

/// Clips open polylines to a rectangle.
pub struct RectClipLines64 {
    base: RectClipBase,
}

impl RectClipLines64 {
    /// A clipper for `rect`.
    pub fn new(rect: Rect64) -> Self {
        RectClipLines64 {
            base: RectClipBase::new(rect),
        }
    }

    /// Clips each open path to the rectangle.
    pub fn execute(&mut self, paths: &Paths64) -> Paths64 {
        let b = &mut self.base;
        let mut result = Vec::new();
        if b.rect.is_empty() {
            return result;
        }
        for path in paths {
            let pathrec = get_bounds(path);
            if !b.rect.intersects(&pathrec) {
                continue;
            }
            b.execute_internal_lines(path);
            for slot in 0..b.results.len() {
                let tmp = b.get_path_lines(slot);
                if !tmp.is_empty() {
                    result.push(tmp);
                }
            }
            b.reset();
        }
        result
    }
}

/// Clips closed paths to `rect`.
pub fn rect_clip(rect: &Rect64, paths: &Paths64) -> Paths64 {
    if rect.is_empty() || paths.is_empty() {
        return Vec::new();
    }
    RectClip64::new(*rect).execute(paths)
}

/// Clips one closed path to `rect`.
pub fn rect_clip_path(rect: &Rect64, path: &Path64) -> Paths64 {
    if rect.is_empty() || path.is_empty() {
        return Vec::new();
    }
    RectClip64::new(*rect).execute(&vec![path.clone()])
}

/// Clips open paths to `rect`.
pub fn rect_clip_lines(rect: &Rect64, lines: &Paths64) -> Paths64 {
    if rect.is_empty() || lines.is_empty() {
        return Vec::new();
    }
    RectClipLines64::new(*rect).execute(lines)
}

/// Clips one open path to `rect`.
pub fn rect_clip_lines_path(rect: &Rect64, line: &Path64) -> Paths64 {
    rect_clip_lines(rect, &vec![line.clone()])
}

fn rect_clip_d_impl(
    rect: &RectD,
    paths: &PathsD,
    precision: i32,
    lines: bool,
) -> Result<PathsD, ClipperError> {
    if rect.is_empty() || paths.is_empty() {
        return Ok(Vec::new());
    }
    check_precision(precision)?;
    let scale = 10f64.powi(precision);
    let r: Rect64 = rect.scale_to(scale);
    let pp = scale_paths(paths, scale, scale)?;
    let out = if lines {
        RectClipLines64::new(r).execute(&pp)
    } else {
        RectClip64::new(r).execute(&pp)
    };
    scale_paths(&out, 1.0 / scale, 1.0 / scale)
}

/// Floating-point [`rect_clip`]; coordinates are scaled by `10^precision`.
pub fn rect_clip_d(rect: &RectD, paths: &PathsD, precision: i32) -> Result<PathsD, ClipperError> {
    rect_clip_d_impl(rect, paths, precision, false)
}

/// Floating-point [`rect_clip_lines`].
pub fn rect_clip_lines_d(
    rect: &RectD,
    lines: &PathsD,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    rect_clip_d_impl(rect, lines, precision, true)
}
