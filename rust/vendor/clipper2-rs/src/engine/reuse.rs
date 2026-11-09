//! `ReuseableDataContainer64`: paths turned into vertices and local minima once, then added
//! to any number of clippers.

use super::ClipperBase;
use super::input::add_paths;
use super::types::*;
use crate::core::{Paths64, check_range};

/// Pre-processed input paths that can be added to several [`super::Clipper64`] runs.
#[derive(Clone, Debug, Default)]
pub struct ReuseableDataContainer64 {
    vertices: Vec<Vertex>,
    minima: Vec<LocalMinima>,
    range_error: bool,
}

impl ReuseableDataContainer64 {
    /// An empty container.
    pub fn new() -> Self {
        Self::default()
    }

    /// Removes every added path.
    pub fn clear(&mut self) {
        self.vertices.clear();
        self.minima.clear();
        self.range_error = false;
    }

    /// Adds `paths` as subjects or clips; `is_open` marks them as open polylines.
    pub fn add_paths(&mut self, paths: &Paths64, polytype: PathType, is_open: bool) {
        if check_range(paths).is_err() {
            self.range_error = true;
        }
        add_paths(
            paths,
            polytype,
            is_open,
            &mut self.vertices,
            &mut self.minima,
        );
    }
}

impl ClipperBase {
    pub(crate) fn add_reuseable_data(&mut self, data: &ReuseableDataContainer64) {
        self.minima_list_sorted = false;
        self.range_error |= data.range_error;
        let offset = self.vertices.len() as Idx;
        let shift = |i: Idx| if i == NONE { NONE } else { i + offset };
        self.vertices.extend(data.vertices.iter().map(|v| Vertex {
            next: shift(v.next),
            prev: shift(v.prev),
            ..*v
        }));
        for lm in &data.minima {
            self.minima.push(LocalMinima {
                vertex: lm.vertex + offset,
                ..*lm
            });
            if lm.is_open {
                self.has_open_paths = true;
            }
        }
    }
}
