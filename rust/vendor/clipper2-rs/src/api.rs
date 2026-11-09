//! Free functions of `clipper.h`: boolean operations in one call.

use crate::clipper_d::ClipperD;
use crate::core::{ClipType, ClipperError, FillRule, Paths64, PathsD};
use crate::engine::Clipper64;
use crate::polytree::{PolyTree64, PolyTreeD};

/// One boolean operation on integer paths.
pub fn boolean_op(
    clip_type: ClipType,
    fill_rule: FillRule,
    subjects: &Paths64,
    clips: &Paths64,
) -> Result<Paths64, ClipperError> {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    c.add_clip(clips);
    Ok(c.execute(clip_type, fill_rule)?.closed)
}

/// [`boolean_op`] into a [`PolyTree64`].
pub fn boolean_op_tree(
    clip_type: ClipType,
    fill_rule: FillRule,
    subjects: &Paths64,
    clips: &Paths64,
) -> Result<PolyTree64, ClipperError> {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    c.add_clip(clips);
    Ok(c.execute_tree(clip_type, fill_rule)?.0)
}

/// Intersection of `subjects` and `clips` (`Intersect`).
pub fn intersect(
    subjects: &Paths64,
    clips: &Paths64,
    fill_rule: FillRule,
) -> Result<Paths64, ClipperError> {
    boolean_op(ClipType::Intersection, fill_rule, subjects, clips)
}

/// Union of `subjects` and `clips` (`Union`).
pub fn union(
    subjects: &Paths64,
    clips: &Paths64,
    fill_rule: FillRule,
) -> Result<Paths64, ClipperError> {
    boolean_op(ClipType::Union, fill_rule, subjects, clips)
}

/// The union of a single set of paths: removes self-intersections and overlaps.
pub fn union_self(subjects: &Paths64, fill_rule: FillRule) -> Result<Paths64, ClipperError> {
    let mut c = Clipper64::new();
    c.add_subject(subjects);
    Ok(c.execute(ClipType::Union, fill_rule)?.closed)
}

/// `subjects` minus `clips` (`Difference`).
pub fn difference(
    subjects: &Paths64,
    clips: &Paths64,
    fill_rule: FillRule,
) -> Result<Paths64, ClipperError> {
    boolean_op(ClipType::Difference, fill_rule, subjects, clips)
}

/// Exclusive-or of `subjects` and `clips` (`Xor`).
pub fn xor(
    subjects: &Paths64,
    clips: &Paths64,
    fill_rule: FillRule,
) -> Result<Paths64, ClipperError> {
    boolean_op(ClipType::Xor, fill_rule, subjects, clips)
}

/// One boolean operation on floating-point paths with `precision` decimal places.
pub fn boolean_op_d(
    clip_type: ClipType,
    fill_rule: FillRule,
    subjects: &PathsD,
    clips: &PathsD,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    let mut c = ClipperD::new(precision)?;
    c.add_subject(subjects)?;
    c.add_clip(clips)?;
    Ok(c.execute(clip_type, fill_rule)?.closed)
}

/// [`boolean_op_d`] into a [`PolyTreeD`].
pub fn boolean_op_tree_d(
    clip_type: ClipType,
    fill_rule: FillRule,
    subjects: &PathsD,
    clips: &PathsD,
    precision: i32,
) -> Result<PolyTreeD, ClipperError> {
    let mut c = ClipperD::new(precision)?;
    c.add_subject(subjects)?;
    c.add_clip(clips)?;
    Ok(c.execute_tree(clip_type, fill_rule)?.0)
}

/// [`intersect`] on floating-point paths with `precision` decimal places.
pub fn intersect_d(
    subjects: &PathsD,
    clips: &PathsD,
    fill_rule: FillRule,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    boolean_op_d(
        ClipType::Intersection,
        fill_rule,
        subjects,
        clips,
        precision,
    )
}

/// [`union`] on floating-point paths with `precision` decimal places.
pub fn union_d(
    subjects: &PathsD,
    clips: &PathsD,
    fill_rule: FillRule,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    boolean_op_d(ClipType::Union, fill_rule, subjects, clips, precision)
}

/// [`union_self`] on floating-point paths with `precision` decimal places.
pub fn union_self_d(
    subjects: &PathsD,
    fill_rule: FillRule,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    let mut c = ClipperD::new(precision)?;
    c.add_subject(subjects)?;
    Ok(c.execute(ClipType::Union, fill_rule)?.closed)
}

/// [`difference`] on floating-point paths with `precision` decimal places.
pub fn difference_d(
    subjects: &PathsD,
    clips: &PathsD,
    fill_rule: FillRule,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    boolean_op_d(ClipType::Difference, fill_rule, subjects, clips, precision)
}

/// [`xor`] on floating-point paths with `precision` decimal places.
pub fn xor_d(
    subjects: &PathsD,
    clips: &PathsD,
    fill_rule: FillRule,
    precision: i32,
) -> Result<PathsD, ClipperError> {
    boolean_op_d(ClipType::Xor, fill_rule, subjects, clips, precision)
}
