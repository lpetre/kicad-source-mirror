//! Input: vertices, local minima and the scanline queue (`AddPaths_`, `Reset`).

use super::ClipperBase;
use super::types::*;
use crate::core::{Paths64, Point64};

fn add_loc_min(
    minima: &mut Vec<LocalMinima>,
    vertices: &mut [Vertex],
    vert: Idx,
    polytype: PathType,
    is_open: bool,
) {
    let v = &mut vertices[vert as usize];
    if v.flags & VF_LOCAL_MIN != 0 {
        return;
    }
    v.flags |= VF_LOCAL_MIN;
    minima.push(LocalMinima {
        vertex: vert,
        polytype,
        is_open,
    });
}

/// `AddPaths_`: links each path's vertices into a ring and records its local minima. Like the
/// C++ pointer walk, the slots of a path that is dropped early are reused by the next path.
pub(crate) fn add_paths(
    paths: &Paths64,
    polytype: PathType,
    is_open: bool,
    vertices: &mut Vec<Vertex>,
    minima: &mut Vec<LocalMinima>,
) {
    let total: usize = paths.iter().map(|p| p.len()).sum();
    if total == 0 {
        return;
    }
    let base = vertices.len() as Idx;
    vertices.resize(
        vertices.len() + total,
        Vertex {
            pt: Point64::default(),
            next: NONE,
            prev: NONE,
            flags: 0,
        },
    );
    let mut v = base;
    for path in paths {
        let v0 = v;
        let mut curr_v = v;
        let mut prev_v = NONE;
        if path.is_empty() {
            continue;
        }
        vertices[v as usize].prev = NONE;
        let mut cnt = 0;
        for &pt in path {
            if prev_v != NONE {
                if vertices[prev_v as usize].pt == pt {
                    continue;
                }
                vertices[prev_v as usize].next = curr_v;
            }
            let c = &mut vertices[curr_v as usize];
            c.prev = prev_v;
            c.pt = pt;
            c.flags = 0;
            prev_v = curr_v;
            curr_v += 1;
            cnt += 1;
        }
        if prev_v == NONE || vertices[prev_v as usize].prev == NONE {
            continue;
        }
        if !is_open && vertices[prev_v as usize].pt == vertices[v0 as usize].pt {
            prev_v = vertices[prev_v as usize].prev;
        }
        vertices[prev_v as usize].next = v0;
        vertices[v0 as usize].prev = prev_v;
        v = curr_v;
        if cnt < 2 || (cnt == 2 && !is_open) {
            continue;
        }

        let mut going_up;
        if is_open {
            curr_v = vertices[v0 as usize].next;
            while curr_v != v0 && vertices[curr_v as usize].pt.y == vertices[v0 as usize].pt.y {
                curr_v = vertices[curr_v as usize].next;
            }
            going_up = vertices[curr_v as usize].pt.y <= vertices[v0 as usize].pt.y;
            if going_up {
                vertices[v0 as usize].flags = VF_OPEN_START;
                add_loc_min(minima, vertices, v0, polytype, true);
            } else {
                vertices[v0 as usize].flags = VF_OPEN_START | VF_LOCAL_MAX;
            }
        } else {
            prev_v = vertices[v0 as usize].prev;
            while prev_v != v0 && vertices[prev_v as usize].pt.y == vertices[v0 as usize].pt.y {
                prev_v = vertices[prev_v as usize].prev;
            }
            if prev_v == v0 {
                continue; // only open paths can be completely flat
            }
            going_up = vertices[prev_v as usize].pt.y > vertices[v0 as usize].pt.y;
        }

        let going_up0 = going_up;
        prev_v = v0;
        curr_v = vertices[v0 as usize].next;
        while curr_v != v0 {
            let (cy, py) = (
                vertices[curr_v as usize].pt.y,
                vertices[prev_v as usize].pt.y,
            );
            if cy > py && going_up {
                vertices[prev_v as usize].flags |= VF_LOCAL_MAX;
                going_up = false;
            } else if cy < py && !going_up {
                going_up = true;
                add_loc_min(minima, vertices, prev_v, polytype, is_open);
            }
            prev_v = curr_v;
            curr_v = vertices[curr_v as usize].next;
        }

        if is_open {
            vertices[prev_v as usize].flags |= VF_OPEN_END;
            if going_up {
                vertices[prev_v as usize].flags |= VF_LOCAL_MAX;
            } else {
                add_loc_min(minima, vertices, prev_v, polytype, is_open);
            }
        } else if going_up != going_up0 {
            if going_up0 {
                add_loc_min(minima, vertices, prev_v, polytype, false);
            } else {
                vertices[prev_v as usize].flags |= VF_LOCAL_MAX;
            }
        }
    }
}

impl ClipperBase {
    /// `Reset`: sorts the minima (stably, bottom-up then left to right). Their y values are not
    /// queued; `pop_scanline` reads them from the sorted list instead.
    pub(crate) fn reset(&mut self) {
        if !self.minima_list_sorted {
            let vertices = &self.vertices;
            // LocMinSorter: higher y first, then smaller x.
            self.minima.sort_by(|a, b| {
                let (pa, pb) = (
                    vertices[a.vertex as usize].pt,
                    vertices[b.vertex as usize].pt,
                );
                if pa.y != pb.y {
                    pb.y.cmp(&pa.y)
                } else {
                    pa.x.cmp(&pb.x)
                }
            });
            self.minima_list_sorted = true;
        }
        self.current_locmin = 0;
        self.actives = NONE;
        self.sel = NONE;
        self.succeeded = true;
    }

    #[inline]
    pub(crate) fn insert_scanline(&mut self, y: i64) {
        self.scanline.push(y);
    }

    /// The next (lower) scanline: the larger of the heap's top and the next unconsumed minimum.
    pub(crate) fn pop_scanline(&mut self) -> Option<i64> {
        let minimum_y = self
            .minima
            .get(self.current_locmin)
            .map(|lm| self.vertices[lm.vertex as usize].pt.y);
        let y = match (self.scanline.peek().copied(), minimum_y) {
            (Some(a), Some(b)) => a.max(b),
            (a, b) => a.or(b)?,
        };
        while self.scanline.peek() == Some(&y) {
            self.scanline.pop();
        }
        Some(y)
    }

    pub(crate) fn pop_local_minima(&mut self, y: i64) -> Option<Idx> {
        let lm = self.minima.get(self.current_locmin)?;
        if self.vertices[lm.vertex as usize].pt.y != y {
            return None;
        }
        self.current_locmin += 1;
        Some((self.current_locmin - 1) as Idx)
    }
}
