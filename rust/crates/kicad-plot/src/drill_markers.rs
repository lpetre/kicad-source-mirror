// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `include/plotters/drill_markers.h` and `common/plotters/drill_markers.cpp`: drill
//! marker outlines, as plain geometry.
//!
//! The plotter and the canvas painter both build their marks from here so a symbol on screen
//! and the same symbol in a plotted fabrication drawing cannot drift apart.

use kicad_geom::{Vector2I, ki_round};

/// `DRILL_MARKERS::MARKER_COUNT`: total marks the pattern table can express.
pub const MARKER_COUNT: u32 = 58;

/// The literal `1.4142` the C++ divides by (not `std::f64::consts::SQRT_2`: the results must
/// match the C++ to the last bit).
#[allow(clippy::approx_constant)]
pub const SQRT2_APPROX: f64 = 1.4142;

/// Port of `DRILL_MARKERS::MARKER_PART::TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum MarkerPartType {
    /// `SEGMENT`
    #[default]
    Segment,
    /// `POLYLINE`
    Polyline,
    /// `CIRCLE`
    Circle,
}

/// Port of `DRILL_MARKERS::MARKER_PART`: one stroke of a mark.
///
/// The primitive matters, not just the shape. The plotters emit a polygon differently from a
/// run of separate segments, so a mark that used to be a polygon has to stay one.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct MarkerPart {
    /// `m_Type`
    pub type_: MarkerPartType,
    /// `m_Points`
    pub points: Vec<Vector2I>,
    /// `m_Radius`
    pub radius: i32,
}

/// `marker_patterns`. Bit order is O Square Lozenge - | \ /
const MARKER_PATTERNS: [u8; MARKER_COUNT as usize] = [
    // Simple shapes first
    0o003, // X
    0o100, // O
    0o014, // +
    0o040, // Sq
    0o020, // Lz
    // Two simple shapes
    0o103, // X O
    0o017, // X +
    0o043, // X Sq
    0o023, // X Lz
    0o114, // O +
    0o140, // O Sq
    0o120, // O Lz
    0o054, // + Sq
    0o034, // + Lz
    0o060, // Sq Lz
    // Three simple shapes
    0o117, // X O +
    0o143, // X O Sq
    0o123, // X O Lz
    0o057, // X + Sq
    0o037, // X + Lz
    0o063, // X Sq Lz
    0o154, // O + Sq
    0o134, // O + Lz
    0o074, // + Sq Lz
    // Four simple shapes
    0o174, // O Sq Lz +
    0o163, // X O Sq Lz
    0o157, // X O Sq +
    0o137, // X O Lz +
    0o077, // X Sq Lz +
    // This draws *everything *
    0o177, // X O Sq Lz +
    // Here we use the single bars... so the cross is forbidden
    0o110, // O -
    0o104, // O |
    0o101, // O /
    0o050, // Sq -
    0o044, // Sq |
    0o041, // Sq /
    0o030, // Lz -
    0o024, // Lz |
    0o021, // Lz /
    0o150, // O Sq -
    0o144, // O Sq |
    0o141, // O Sq /
    0o130, // O Lz -
    0o124, // O Lz |
    0o121, // O Lz /
    0o070, // Sq Lz -
    0o064, // Sq Lz |
    0o061, // Sq Lz /
    0o170, // O Sq Lz -
    0o164, // O Sq Lz |
    0o161, // O Sq Lz /
    // The backlash component is the last resort, because it is easy to confound
    0o102, // \ O
    0o042, // \ Sq
    0o022, // \ Lz
    0o142, // \ O Sq
    0o122, // \ O Lz
    0o062, // \ Sq Lz
    0o162, // \ O Sq Lz
];

/// Single-part and two-part marks only. The three-part ones are too easily confounded at
/// chart size.
const CURATED: [u32; 12] = [0, 1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 16];

fn add_segment(a_parts: &mut Vec<MarkerPart>, a_start: Vector2I, a_end: Vector2I) {
    a_parts.push(MarkerPart {
        type_: MarkerPartType::Segment,
        points: vec![a_start, a_end],
        radius: 0,
    });
}

fn add_polyline(a_parts: &mut Vec<MarkerPart>, a_points: Vec<Vector2I>) {
    a_parts.push(MarkerPart {
        type_: MarkerPartType::Polyline,
        points: a_points,
        radius: 0,
    });
}

/// Port of `DRILL_MARKERS::CuratedShapeCount`: marks that stay legible at chart size and in
/// monochrome.
///
/// The later patterns in the table stack up to three parts on top of each other and are hard
/// to tell apart on paper, so symbol assignment works through this subset first and falls
/// back to letters rather than reaching for them.
pub fn curated_shape_count() -> i32 {
    CURATED.len() as i32
}

/// Port of `DRILL_MARKERS::CuratedShape`: pattern index for the nth curated mark.
pub fn curated_shape(a_index: i32) -> u32 {
    if a_index < 0 || a_index >= curated_shape_count() {
        return 0;
    }

    CURATED[a_index as usize]
}

/// Port of `DRILL_MARKERS::BuildMarker`: decompose one mark, in the order the parts must be
/// drawn.
///
/// An out-of-range shape id yields a plain circle, matching the plotter's long-standing
/// fallback.
pub fn build_marker(a_position: Vector2I, a_radius: i32, a_shape_id: u32) -> Vec<MarkerPart> {
    let mut parts = Vec::new();

    let add_circle = |parts: &mut Vec<MarkerPart>| {
        parts.push(MarkerPart {
            type_: MarkerPartType::Circle,
            points: Vec::new(),
            radius: a_radius,
        });
    };

    if a_shape_id >= MARKER_COUNT {
        add_circle(&mut parts);
        return parts;
    }

    let pat = MARKER_PATTERNS[a_shape_id as usize];
    let r = a_radius;
    let p = a_position;

    if pat & 0o001 != 0 {
        add_segment(
            &mut parts,
            Vector2I::new(p.x - r, p.y - r),
            Vector2I::new(p.x + r, p.y + r),
        );
    }

    if pat & 0o002 != 0 {
        add_segment(
            &mut parts,
            Vector2I::new(p.x + r, p.y - r),
            Vector2I::new(p.x - r, p.y + r),
        );
    }

    if pat & 0o004 != 0 {
        add_segment(
            &mut parts,
            Vector2I::new(p.x, p.y - r),
            Vector2I::new(p.x, p.y + r),
        );
    }

    if pat & 0o010 != 0 {
        add_segment(
            &mut parts,
            Vector2I::new(p.x - r, p.y),
            Vector2I::new(p.x + r, p.y),
        );
    }

    if pat & 0o020 != 0 {
        add_polyline(
            &mut parts,
            vec![
                Vector2I::new(p.x, p.y + r),
                Vector2I::new(p.x + r, p.y),
                Vector2I::new(p.x, p.y - r),
                Vector2I::new(p.x - r, p.y),
                Vector2I::new(p.x, p.y + r),
            ],
        );
    }

    if pat & 0o040 != 0 {
        // The square is inscribed in the marker circle, so its half-side is r/sqrt(2)
        let s: i32 = ki_round(r as f64 / SQRT2_APPROX);

        add_polyline(
            &mut parts,
            vec![
                Vector2I::new(p.x + s, p.y + s),
                Vector2I::new(p.x + s, p.y - s),
                Vector2I::new(p.x - s, p.y - s),
                Vector2I::new(p.x - s, p.y + s),
                Vector2I::new(p.x + s, p.y + s),
            ],
        );
    }

    if pat & 0o100 != 0 {
        add_circle(&mut parts);
    }

    parts
}
