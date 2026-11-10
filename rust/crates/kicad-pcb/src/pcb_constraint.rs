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

//! Port of `pcbnew/constraints/pcb_constraint.h` and `pcbnew/constraints/pcb_constraint.cpp`
//! (`PCB_CONSTRAINT`, `CONSTRAINT_MEMBER`, `PCB_CONSTRAINT_TYPE`, `CONSTRAINT_ANCHOR`),
//! `board-model-types.md` section 10: pure data with `KIID` member references (dangling
//! members survive a round trip); no solver. Behaviour: package M23.

use std::sync::Arc;

use kicad_core::units_provider::UnitsProvider;
use kicad_core::{KicadT, Kiid, KiidGenerator, PcbLayerId};
use kicad_geom::{Box2I, EdaAngle, FlipDirection, ShapeCompound, Vector2I};

use crate::ctx::{BoardCtx, MutEnv};
use crate::drc_api::DrcConstraintT;
use crate::item::{BoardItemBase, SharedShape};
use crate::traits::{BoardItem, BoardItemMut, Flashing, Invalidate};

/// `PCB_CONSTRAINT`.
#[derive(Clone, Debug)]
pub struct PcbConstraint {
    pub base: BoardItemBase,
    pub type_: PcbConstraintType,       // m_type
    pub members: Vec<ConstraintMember>, // m_members
    pub value: Option<f64>,             // m_value
    pub driving: bool,                  // m_driving
}

/// `PCB_CONSTRAINT_TYPE`; same order and values as C++.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PcbConstraintType {
    #[default]
    Undefined = 0,
    /// Two points are made to coincide.
    Coincident,
    /// A segment (or two points) is horizontal.
    Horizontal,
    /// A segment (or two points) is vertical.
    Vertical,
    /// Two segments are parallel.
    Parallel,
    /// Two segments are perpendicular.
    Perpendicular,
    /// Two segments lie on the same line.
    Collinear,
    /// Two points are mirror images about an axis.
    Symmetric,
    /// Two segments have equal length.
    EqualLength,
    /// Two arcs/circles have equal radius.
    EqualRadius,
    /// A point lies on a segment's supporting line.
    PointOnLine,
    /// A point is the midpoint of a segment.
    Midpoint,
    /// A point is locked at its current location.
    FixedPosition,
    /// A segment has a driving length value.
    FixedLength,
    /// Two arcs/circles share a center.
    Concentric,
    /// An arc/circle has a driving radius value.
    FixedRadius,
    /// An angle between members (driving or reference).
    AngularDimension,
    /// A line and a curve, or two curves, touch tangentially.
    Tangent,
    /// An arc has a driving or reference swept-angle value.
    ArcAngle,
}

/// `CONSTRAINT_ANCHOR`; same order and values as C++.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum ConstraintAnchor {
    /// The item as a whole (a segment as a line, a circle).
    #[default]
    Whole = 0,
    /// First endpoint of a segment or arc.
    Start,
    /// Second endpoint of a segment or arc.
    End,
    /// Midpoint of a segment or arc.
    Mid,
    /// Center of an arc or circle.
    Center,
    /// The radius (scalar feature) of an arc or circle.
    Radius,
    /// An indexed rectangle corner or polygon outline vertex; pairs with
    /// `ConstraintMember::index`.
    Vertex,
}

/// `CONSTRAINT_MEMBER`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintMember {
    /// m_item: referenced board item, usually a PCB_SHAPE.
    pub item: Kiid,
    /// m_anchor: which feature of that item participates.
    pub anchor: ConstraintAnchor,
    /// m_index: vertex ordinal; only meaningful for the VERTEX anchor.
    pub index: i32,
}

impl Default for ConstraintMember {
    /// `CONSTRAINT_MEMBER()`: `niluuid`, `WHOLE`, index -1.
    fn default() -> Self {
        ConstraintMember {
            item: Kiid::default(),
            anchor: ConstraintAnchor::Whole,
            index: -1,
        }
    }
}

impl ConstraintMember {
    /// `CONSTRAINT_MEMBER( aItem, aAnchor, aIndex = -1 )`.
    pub fn new(item: Kiid, anchor: ConstraintAnchor, index: i32) -> Self {
        ConstraintMember {
            item,
            anchor,
            index,
        }
    }
}

// ---- constructors ------------------------------------------------------------------------

impl Default for PcbConstraint {
    /// `PCB_CONSTRAINT( aParent )` = `PCB_CONSTRAINT( aParent, UNDEFINED )`
    /// (`pcb_constraint.cpp`): layer `UNDEFINED_LAYER`, no members, no value, driving; nil
    /// UUID.
    fn default() -> Self {
        PcbConstraint {
            base: BoardItemBase::with_layer(PcbLayerId::UndefinedLayer),
            type_: PcbConstraintType::Undefined,
            members: Vec::new(),
            value: None,
            driving: true,
        }
    }
}

impl PcbConstraint {
    /// `new PCB_CONSTRAINT( aParent, aType )`: the constructor values with a fresh UUID.
    pub fn new(kiid_generator: &mut KiidGenerator, type_: PcbConstraintType) -> Self {
        let mut constraint = PcbConstraint {
            type_,
            ..PcbConstraint::default()
        };
        constraint.base.uuid = Kiid::new(kiid_generator);
        constraint
    }
}

// ---- delegation shims (frozen after M0) ---------------------------------------------------

impl Invalidate for PcbConstraint {
    fn invalidate_caches(&mut self) {}
}

impl BoardItem for PcbConstraint {
    fn base(&self) -> &BoardItemBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut BoardItemBase {
        &mut self.base
    }
    fn type_(&self) -> KicadT {
        KicadT::PcbConstraintT
    }
    fn class_name(&self) -> &'static str {
        "PCB_CONSTRAINT"
    }
    fn is_layer_agnostic(&self) -> bool {
        PcbConstraint::is_layer_agnostic(self)
    }
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
        PcbConstraint::position(self, ctx)
    }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I {
        PcbConstraint::bounding_box(self, ctx)
    }
    fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        usage: DrcConstraintT,
    ) -> SharedShape {
        PcbConstraint::effective_shape(self, ctx, layer, flash, usage)
    }
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool {
        PcbConstraint::hit_test(self, ctx, pos, accuracy)
    }
    fn hit_test_rect(
        &self,
        ctx: &BoardCtx<'_>,
        rect: &Box2I,
        contained: bool,
        accuracy: i32,
    ) -> bool {
        PcbConstraint::hit_test_rect(self, ctx, rect, contained, accuracy)
    }
    fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        full: bool,
    ) -> String {
        PcbConstraint::item_description(self, ctx, units, full)
    }
}

impl BoardItemMut for PcbConstraint {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
        PcbConstraint::set_position(self, env, pos)
    }
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I) {
        PcbConstraint::move_by(self, env, delta)
    }
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle) {
        PcbConstraint::rotate(self, env, centre, angle)
    }
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbConstraint::flip(self, env, centre, dir)
    }
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection) {
        PcbConstraint::mirror(self, env, centre, dir)
    }
}

// ---- inherent methods (package M23) -------------------------------------------------------

impl PcbConstraint {
    /// `GetConstraintType()`.
    pub fn constraint_type(&self) -> PcbConstraintType {
        self.type_
    }

    /// `SetConstraintType()`.
    pub fn set_constraint_type(&mut self, type_: PcbConstraintType) {
        self.type_ = type_;
    }

    /// `GetMembers()`.
    pub fn members(&self) -> &[ConstraintMember] {
        &self.members
    }

    /// `AddMember( aItem, aAnchor = WHOLE, aIndex = -1 )`: `m_members.emplace_back( .. )`.
    pub fn add_member(&mut self, item: Kiid, anchor: ConstraintAnchor, index: i32) {
        self.members
            .push(ConstraintMember::new(item, anchor, index));
    }

    /// `HasValue()`.
    pub fn has_value(&self) -> bool {
        self.value.is_some()
    }

    /// `GetValue()`.
    pub fn value(&self) -> Option<f64> {
        self.value
    }

    /// `SetValue()`.
    pub fn set_value(&mut self, value: Option<f64>) {
        self.value = value;
    }

    /// `IsDriving()`.
    pub fn is_driving(&self) -> bool {
        self.driving
    }

    /// `SetDriving()`.
    pub fn set_driving(&mut self, driving: bool) {
        self.driving = driving;
    }

    /// `PCB_CONSTRAINT::IsLayerAgnostic()`: true.
    pub fn is_layer_agnostic(&self) -> bool {
        true
    }

    /// `PCB_CONSTRAINT::GetPosition()`: `VECTOR2I()`.
    pub fn position(&self, _ctx: &BoardCtx<'_>) -> Vector2I {
        Vector2I::default()
    }

    /// `PCB_CONSTRAINT::SetPosition()`: does nothing.
    pub fn set_position(&mut self, _env: &MutEnv, _pos: Vector2I) {}

    /// `PCB_CONSTRAINT::GetBoundingBox()`: `BOX2I()`.
    pub fn bounding_box(&self, _ctx: &BoardCtx<'_>) -> Box2I {
        Box2I::default()
    }

    /// `PCB_CONSTRAINT::HitTest( aPosition, aAccuracy )`: false.
    pub fn hit_test(&self, _ctx: &BoardCtx<'_>, _pos: Vector2I, _accuracy: i32) -> bool {
        false
    }

    /// `PCB_CONSTRAINT::HitTest( aRect, aContained, aAccuracy )`: false.
    pub fn hit_test_rect(
        &self,
        _ctx: &BoardCtx<'_>,
        _rect: &Box2I,
        _contained: bool,
        _accuracy: i32,
    ) -> bool {
        false
    }

    /// `PCB_CONSTRAINT::Move()`: does nothing.
    pub fn move_by(&mut self, _env: &MutEnv, _delta: Vector2I) {}

    /// `PCB_CONSTRAINT::Rotate()`: does nothing.
    pub fn rotate(&mut self, _env: &MutEnv, _centre: Vector2I, _angle: EdaAngle) {}

    /// `PCB_CONSTRAINT::Flip()`: does nothing.
    pub fn flip(&mut self, _env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {}

    /// `PCB_CONSTRAINT::Mirror( aCentre, aFlipDirection )`: does nothing.
    pub fn mirror(&mut self, _env: &MutEnv, _centre: Vector2I, _dir: FlipDirection) {}

    /// `PCB_CONSTRAINT::GetEffectiveShape()`.
    pub fn effective_shape(
        &self,
        ctx: &BoardCtx<'_>,
        layer: PcbLayerId,
        flash: Flashing,
        _usage: DrcConstraintT,
    ) -> SharedShape {
        // A constraint has no geometry: an empty compound.
        let _ = (ctx, layer, flash);
        Arc::new(ShapeCompound::new())
    }

    /// `PCB_CONSTRAINT::GetItemDescription()`.
    pub fn item_description(
        &self,
        ctx: &BoardCtx<'_>,
        units: &UnitsProvider<'_>,
        _full: bool,
    ) -> String {
        let _ = (ctx, units);
        format!(
            "Constraint: {} ({} members)",
            constraint_type_label(self.type_),
            self.members.len()
        )
    }

    /// `PCB_CONSTRAINT::RemapKIIDs( aIdMap )`: members whose item is in the map get the new
    /// UUID.
    pub fn remap_kiids(&mut self, id_map: &std::collections::BTreeMap<Kiid, Kiid>) {
        for member in &mut self.members {
            if let Some(new_id) = id_map.get(&member.item) {
                member.item = *new_id;
            }
        }
    }

    /// `PCB_CONSTRAINT::operator==( const PCB_CONSTRAINT& )`: members are positional (a
    /// symmetric constraint's axis is not interchangeable with its mirrored points), so they are
    /// compared in order.
    pub fn eq_item(&self, other: &PcbConstraint) -> bool {
        self.type_ == other.type_
            && self.driving == other.driving
            && self.value == other.value
            && self.members == other.members
    }
}

// ---- free functions of pcb_constraint.h (file tokens: data tables, ported as such) ---------

/// `ConstraintTypeToken( aType )`: the file token.
pub fn constraint_type_token(type_: PcbConstraintType) -> &'static str {
    match type_ {
        PcbConstraintType::Coincident => "coincident",
        PcbConstraintType::Horizontal => "horizontal",
        PcbConstraintType::Vertical => "vertical",
        PcbConstraintType::Parallel => "parallel",
        PcbConstraintType::Perpendicular => "perpendicular",
        PcbConstraintType::Collinear => "collinear",
        PcbConstraintType::Symmetric => "symmetric",
        PcbConstraintType::EqualLength => "equal_length",
        PcbConstraintType::EqualRadius => "equal_radius",
        PcbConstraintType::PointOnLine => "point_on_line",
        PcbConstraintType::Midpoint => "midpoint",
        PcbConstraintType::FixedPosition => "fixed_position",
        PcbConstraintType::FixedLength => "fixed_length",
        PcbConstraintType::Concentric => "concentric",
        PcbConstraintType::FixedRadius => "fixed_radius",
        PcbConstraintType::AngularDimension => "angular_dimension",
        PcbConstraintType::Tangent => "tangent",
        PcbConstraintType::ArcAngle => "arc_angle",
        PcbConstraintType::Undefined => "undefined",
    }
}

/// Every `PCB_CONSTRAINT_TYPE` in value order (the C++ loops `0 ..= ARC_ANGLE`).
const CONSTRAINT_TYPES: [PcbConstraintType; 19] = [
    PcbConstraintType::Undefined,
    PcbConstraintType::Coincident,
    PcbConstraintType::Horizontal,
    PcbConstraintType::Vertical,
    PcbConstraintType::Parallel,
    PcbConstraintType::Perpendicular,
    PcbConstraintType::Collinear,
    PcbConstraintType::Symmetric,
    PcbConstraintType::EqualLength,
    PcbConstraintType::EqualRadius,
    PcbConstraintType::PointOnLine,
    PcbConstraintType::Midpoint,
    PcbConstraintType::FixedPosition,
    PcbConstraintType::FixedLength,
    PcbConstraintType::Concentric,
    PcbConstraintType::FixedRadius,
    PcbConstraintType::AngularDimension,
    PcbConstraintType::Tangent,
    PcbConstraintType::ArcAngle,
];

/// `ConstraintTypeFromToken( aToken )`: `UNDEFINED` for an unknown token.
pub fn constraint_type_from_token(token: &str) -> PcbConstraintType {
    CONSTRAINT_TYPES
        .iter()
        .copied()
        .find(|t| token == constraint_type_token(*t))
        .unwrap_or(PcbConstraintType::Undefined)
}

/// `ConstraintValueIsLength( aType )`: fixed length and fixed radius carry a length; the
/// angular types are angles; everything else is valueless.
pub fn constraint_value_is_length(type_: PcbConstraintType) -> bool {
    matches!(
        type_,
        PcbConstraintType::FixedLength | PcbConstraintType::FixedRadius
    )
}

/// `ConstraintAnchorToken( aAnchor )`: the file token.
pub fn constraint_anchor_token(anchor: ConstraintAnchor) -> &'static str {
    match anchor {
        ConstraintAnchor::Whole => "whole",
        ConstraintAnchor::Start => "start",
        ConstraintAnchor::End => "end",
        ConstraintAnchor::Mid => "mid",
        ConstraintAnchor::Center => "center",
        ConstraintAnchor::Radius => "radius",
        ConstraintAnchor::Vertex => "vertex",
    }
}

/// `ConstraintAnchorFromToken( aToken )`: `WHOLE` for an unknown token.
pub fn constraint_anchor_from_token(token: &str) -> ConstraintAnchor {
    [
        ConstraintAnchor::Whole,
        ConstraintAnchor::Start,
        ConstraintAnchor::End,
        ConstraintAnchor::Mid,
        ConstraintAnchor::Center,
        ConstraintAnchor::Radius,
        ConstraintAnchor::Vertex,
    ]
    .into_iter()
    .find(|a| token == constraint_anchor_token(*a))
    .unwrap_or(ConstraintAnchor::Whole)
}

/// `ConstraintsAreDuplicate( aA, aB )`.
pub fn constraints_are_duplicate(a: &PcbConstraint, b: &PcbConstraint) -> bool {
    if a.type_ != b.type_ {
        return false;
    }

    let (a, b) = (&a.members, &b.members);

    if a.is_empty() || a.len() != b.len() {
        return false;
    }

    // Match members as a multiset so member order does not matter (A-B is the same as B-A).
    // Roles survive this because a member's anchor encodes them; in SYMMETRIC and the point
    // families the mirror/probe points carry endpoint anchors while the axis/line is WHOLE, so
    // two constraints with the same multiset genuinely relate the same geometry the same way.
    // (std::is_permutation with operator==)
    a.iter()
        .all(|m| a.iter().filter(|x| *x == m).count() == b.iter().filter(|x| *x == m).count())
}

/// `ConstraintTypeLabel( aType )` (untranslated).
pub fn constraint_type_label(type_: PcbConstraintType) -> &'static str {
    match type_ {
        PcbConstraintType::Coincident => "Coincident",
        PcbConstraintType::Horizontal => "Horizontal",
        PcbConstraintType::Vertical => "Vertical",
        PcbConstraintType::Parallel => "Parallel",
        PcbConstraintType::Perpendicular => "Perpendicular",
        PcbConstraintType::Collinear => "Collinear",
        PcbConstraintType::Symmetric => "Symmetric",
        PcbConstraintType::EqualLength => "Equal length",
        PcbConstraintType::EqualRadius => "Equal radius",
        PcbConstraintType::PointOnLine => "Point on line",
        PcbConstraintType::Midpoint => "Midpoint",
        PcbConstraintType::FixedPosition => "Fixed position",
        PcbConstraintType::FixedLength => "Fixed length",
        PcbConstraintType::Concentric => "Concentric",
        PcbConstraintType::FixedRadius => "Fixed radius",
        PcbConstraintType::AngularDimension => "Angular dimension",
        PcbConstraintType::Tangent => "Tangent",
        PcbConstraintType::ArcAngle => "Arc angle",
        PcbConstraintType::Undefined => "Undefined",
    }
}

/// `DIM_VALUE_MODE` (`pcbnew/constraints/constraint_builder.h`): Driven mirrors measured
/// geometry, Driving forces geometry to the entered length, Arbitrary shows custom text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimValueMode {
    Driven,
    Driving,
    Arbitrary,
}

/// Port of `CollectResolvedConstraints( aBoard )` (constraint_builder.cpp): the board's
/// constraints, plus those of every footprint when the board is a footprint holder.
pub fn collect_resolved_constraints(board: &crate::board::Board) -> Vec<&PcbConstraint> {
    let mut constraints: Vec<&PcbConstraint> = board.constraints().map(|(_, c)| c).collect();

    if !board.is_footprint_holder() {
        return constraints;
    }

    for (fp, _) in board.footprints() {
        constraints.extend(board.footprint_constraints(fp).map(|(_, c)| c));
    }

    constraints
}

/// Port of `FindDimensionLengthConstraint( aBoard, aDimension )`: the first resolved
/// `FIXED_LENGTH` constraint whose two members are the dimension's START and END anchors (in
/// either order).
pub fn find_dimension_length_constraint<'a>(
    board: Option<&'a crate::board::Board>,
    dimension: Option<&crate::pcb_dimension::PcbDimension>,
) -> Option<&'a PcbConstraint> {
    let (board, dimension) = (board?, dimension?);
    let uuid = dimension.base().uuid;
    let start_member = ConstraintMember::new(uuid, ConstraintAnchor::Start, -1);
    let end_member = ConstraintMember::new(uuid, ConstraintAnchor::End, -1);

    let matches = |constraint: &PcbConstraint| {
        if constraint.constraint_type() != PcbConstraintType::FixedLength {
            return false;
        }

        let members = constraint.members();

        members.len() == 2
            && ((members[0] == start_member && members[1] == end_member)
                || (members[0] == end_member && members[1] == start_member))
    };

    collect_resolved_constraints(board)
        .into_iter()
        .find(|c| matches(c))
}

/// Port of `DimensionValueMode( aBoard, aDimension )` (= `PCB_DIMENSION_BASE::GetValueMode`).
pub fn dimension_value_mode(
    board: Option<&crate::board::Board>,
    dimension: Option<&crate::pcb_dimension::PcbDimension>,
) -> DimValueMode {
    let length_constraint = find_dimension_length_constraint(board, dimension);

    if length_constraint.is_some_and(|c| c.is_driving()) {
        return DimValueMode::Driving;
    }

    if dimension.is_some_and(|d| d.override_text_enabled) {
        return DimValueMode::Arbitrary;
    }

    DimValueMode::Driven
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- qa/tests/pcbnew/constraints/test_pcb_constraint.cpp (package M23 part) ----------

    fn kiid(g: &mut KiidGenerator) -> Kiid {
        Kiid::new(g)
    }

    /// `CloneIsAFaithfulCopy`: the clone keeps the UUID and compares equal (operator==).
    #[test]
    fn clone_is_a_faithful_copy() {
        let mut g = KiidGenerator::default();
        let mut c = PcbConstraint::new(&mut g, PcbConstraintType::EqualRadius);
        c.add_member(kiid(&mut g), ConstraintAnchor::Radius, -1);
        c.add_member(kiid(&mut g), ConstraintAnchor::Radius, -1);
        c.set_value(Some(2.5));

        let clone = c.clone();
        assert!(clone.base.uuid == c.base.uuid);
        assert!(clone.eq_item(&c));
        assert!(c.eq_item(&clone));
    }

    /// `EqualityAndSimilarity` (the operator== checks; `Similarity` is deferred, see
    /// PORT_STATUS.md).
    #[test]
    fn equality() {
        let mut g = KiidGenerator::default();
        let (a, b) = (kiid(&mut g), kiid(&mut g));
        let make = |g: &mut KiidGenerator, t, members: &[Kiid]| {
            let mut c = PcbConstraint::new(g, t);
            for m in members {
                c.add_member(*m, ConstraintAnchor::Whole, -1);
            }
            c
        };

        let c1 = make(&mut g, PcbConstraintType::Parallel, &[a, b]);
        let c2 = make(&mut g, PcbConstraintType::Parallel, &[a, b]);
        assert!(c1.eq_item(&c2));

        // A different type is wholly dissimilar.
        let perp = make(&mut g, PcbConstraintType::Perpendicular, &[a, b]);
        assert!(!c1.eq_item(&perp));

        // Same type, different members compares unequal.
        let other = kiid(&mut g);
        let c3 = make(&mut g, PcbConstraintType::Parallel, &[other]);
        assert!(!c1.eq_item(&c3));
    }

    /// `DuplicateDetection`.
    #[test]
    fn duplicate_detection() {
        let mut g = KiidGenerator::default();
        let (a, b, c) = (kiid(&mut g), kiid(&mut g), kiid(&mut g));
        let make = |g: &mut KiidGenerator, t, first: Kiid, second: Kiid| {
            let mut constraint = PcbConstraint::new(g, t);
            constraint.add_member(first, ConstraintAnchor::Whole, -1);
            constraint.add_member(second, ConstraintAnchor::Whole, -1);
            constraint
        };

        let parallel_ab = make(&mut g, PcbConstraintType::Parallel, a, b);

        // Same type and members is a duplicate, and member order does not matter (A-B == B-A).
        let x = make(&mut g, PcbConstraintType::Parallel, a, b);
        assert!(constraints_are_duplicate(&parallel_ab, &x));
        let x = make(&mut g, PcbConstraintType::Parallel, b, a);
        assert!(constraints_are_duplicate(&parallel_ab, &x));

        // A different type or a different member is not a duplicate.
        let x = make(&mut g, PcbConstraintType::Perpendicular, a, b);
        assert!(!constraints_are_duplicate(&parallel_ab, &x));
        let x = make(&mut g, PcbConstraintType::Parallel, a, c);
        assert!(!constraints_are_duplicate(&parallel_ab, &x));

        // Value and driving are ignored.
        let mut len1 = PcbConstraint::new(&mut g, PcbConstraintType::FixedLength);
        len1.add_member(a, ConstraintAnchor::Whole, -1);
        len1.set_value(Some(5.0));
        let mut len2 = PcbConstraint::new(&mut g, PcbConstraintType::FixedLength);
        len2.add_member(a, ConstraintAnchor::Whole, -1);
        len2.set_value(Some(9.0));
        len2.set_driving(false);
        assert!(constraints_are_duplicate(&len1, &len2));

        // An empty-member constraint is never a duplicate.
        let empty1 = PcbConstraint::new(&mut g, PcbConstraintType::Parallel);
        let empty2 = PcbConstraint::new(&mut g, PcbConstraintType::Parallel);
        assert!(!constraints_are_duplicate(&empty1, &empty2));
    }

    /// `VertexIndexInDuplicateDetection`.
    #[test]
    fn vertex_index_in_duplicate_detection() {
        let mut g = KiidGenerator::default();
        let (item, other) = (kiid(&mut g), kiid(&mut g));
        let make = |g: &mut KiidGenerator, first: i32, second: i32| {
            let mut constraint = PcbConstraint::new(g, PcbConstraintType::Coincident);
            constraint.add_member(item, ConstraintAnchor::Vertex, first);
            constraint.add_member(other, ConstraintAnchor::Vertex, second);
            constraint
        };

        let vertex02 = make(&mut g, 0, 2);
        let x = make(&mut g, 0, 2);
        assert!(constraints_are_duplicate(&vertex02, &x));
        let x = make(&mut g, 2, 2);
        assert!(!constraints_are_duplicate(&vertex02, &x));
        let x = make(&mut g, 0, 3);
        assert!(!constraints_are_duplicate(&vertex02, &x));
    }

    /// `RemapKIIDs` and the description.
    #[test]
    fn remap_kiids_and_description() {
        let mut g = KiidGenerator::default();
        let (a, b, n) = (kiid(&mut g), kiid(&mut g), kiid(&mut g));
        let mut c = PcbConstraint::new(&mut g, PcbConstraintType::EqualLength);
        c.add_member(a, ConstraintAnchor::Whole, -1);
        c.add_member(b, ConstraintAnchor::Start, -1);
        let map = std::collections::BTreeMap::from([(b, n)]);
        c.remap_kiids(&map);
        assert!(c.members[0].item == a && c.members[1].item == n);

        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let units = UnitsProvider::new(&kicad_core::PCB_IU_SCALE, kicad_core::EdaUnits::Mm);
        assert_eq!(
            BoardItem::item_description(&c, &ctx, &units, false),
            "Constraint: Equal length (2 members)"
        );
    }
    use crate::ids::ItemKind;
    use crate::test_support::rotated_flipped_scaled;

    /// `PCB_CONSTRAINT::PCB_CONSTRAINT( aParent, aType )` (pcbnew/constraints/pcb_constraint.cpp)
    /// and `CONSTRAINT_MEMBER()` (pcb_constraint.h).
    #[test]
    fn constraint_constructor_defaults() {
        let c = PcbConstraint::default();
        assert_eq!(c.constraint_type(), PcbConstraintType::Undefined);
        assert_eq!(c.base.layer, PcbLayerId::UndefinedLayer);
        assert!(c.members().is_empty());
        assert!(!c.has_value());
        assert!(c.is_driving());
        assert!(c.base.uuid == Kiid::default());
        assert_eq!(BoardItem::class_name(&c), "PCB_CONSTRAINT");

        let m = ConstraintMember::default();
        assert!(m.item == Kiid::default());
        assert_eq!(m.anchor, ConstraintAnchor::Whole);
        assert_eq!(m.index, -1);
        assert_eq!(PcbConstraintType::ArcAngle as i32, 18);
        assert_eq!(ConstraintAnchor::Vertex as i32, 6);

        let mut kiid_generator = KiidGenerator::default();
        let c = PcbConstraint::new(&mut kiid_generator, PcbConstraintType::Tangent);
        assert_eq!(c.type_, PcbConstraintType::Tangent);
        assert!(c.driving);
        assert!(c.base.uuid != Kiid::default());
    }

    /// The constant overrides through the traits, on a footprint child.
    #[test]
    fn constraint_constant_virtuals() {
        let mut fx = rotated_flipped_scaled();
        let id = fx
            .child(ItemKind::Constraint)
            .typed::<PcbConstraint>()
            .unwrap();
        let mut c = PcbConstraint::default();
        c.add_member(Kiid::default(), ConstraintAnchor::Vertex, 3);
        c.set_value(Some(1.5));
        fx.set_child(id, c);
        fx.board.check_invariants().unwrap();

        let ctx = BoardCtx::bare(&fx.board);
        let c: &dyn BoardItem = &fx.board[id];
        assert!(c.is_layer_agnostic());
        assert_eq!(c.position(&ctx), Vector2I::default());
        assert!(!c.hit_test(&ctx, Vector2I::new(0, 0), 10));
        let c = &fx.board[id];
        assert_eq!(c.members()[0].index, 3);
        assert_eq!(c.value(), Some(1.5));

        let mut c = c.clone();
        let env = MutEnv::detached();
        BoardItemMut::move_by(&mut c, &env, Vector2I::new(5, 5));
        BoardItemMut::set_position(&mut c, &env, Vector2I::new(5, 5));
        assert_eq!(c.members()[0].anchor, ConstraintAnchor::Vertex);
    }

    // Ports of qa/tests/pcbnew/constraints/test_pcb_constraint.cpp (the cases that exercise
    // construction, defaults and the data accessors; the rest needs M23 behaviour).

    /// `BOOST_AUTO_TEST_CASE( BasicProperties )`.
    #[test]
    fn basic_properties() {
        let board = crate::board::Board::new();
        let ctx = BoardCtx::bare(&board);
        let mut kiid_generator = KiidGenerator::default();
        let c = PcbConstraint::new(&mut kiid_generator, PcbConstraintType::Parallel);

        assert_eq!(BoardItem::type_(&c), KicadT::PcbConstraintT);
        assert_eq!(c.constraint_type(), PcbConstraintType::Parallel);

        // A constraint carries no geometry and is on no layer.
        assert!(BoardItem::layer_set(&c, &ctx) == kicad_core::Lset::new());
        assert!(!BoardItem::hit_test(
            &c,
            &ctx,
            Vector2I::new(0, 0),
            1_000_000
        ));

        // Driving by default.
        assert!(c.is_driving());
        assert!(!c.has_value());
    }

    /// `BOOST_AUTO_TEST_CASE( GeometrySurfaceIsSafe )` (the transforms; the compound
    /// effective shape is M23).
    #[test]
    fn geometry_surface_is_safe() {
        let mut c = PcbConstraint::default();
        c.set_constraint_type(PcbConstraintType::Parallel);
        let env = MutEnv::detached();
        BoardItemMut::move_by(&mut c, &env, Vector2I::new(1000, 1000));
        BoardItemMut::rotate(&mut c, &env, Vector2I::new(0, 0), kicad_geom::ANGLE_90);
    }

    /// `BOOST_AUTO_TEST_CASE( MembersAndValue )`.
    #[test]
    fn members_and_value() {
        let mut kiid_generator = KiidGenerator::default();
        let mut c = PcbConstraint::new(&mut kiid_generator, PcbConstraintType::FixedLength);

        let a = Kiid::new(&mut kiid_generator);
        let b = Kiid::new(&mut kiid_generator);
        c.add_member(a, ConstraintAnchor::Start, -1);
        c.add_member(b, ConstraintAnchor::End, -1);
        c.set_value(Some(5.0));
        c.set_driving(false);

        assert_eq!(c.members().len(), 2);
        assert!(c.members()[0].item == a);
        assert_eq!(c.members()[0].anchor, ConstraintAnchor::Start);
        assert_eq!(c.members()[1].anchor, ConstraintAnchor::End);
        assert!(c.has_value());
        assert!((c.value().unwrap() - 5.0).abs() <= 5.0 * 1e-11);
        assert!(!c.is_driving());
    }

    /// `BOOST_AUTO_TEST_CASE( VertexMemberIdentity )`.
    #[test]
    fn vertex_member_identity() {
        let mut kiid_generator = KiidGenerator::default();
        let id = Kiid::new(&mut kiid_generator);
        let a = ConstraintMember::new(id, ConstraintAnchor::Vertex, 2);
        let b = ConstraintMember::new(id, ConstraintAnchor::Vertex, 2);
        let c = ConstraintMember::new(id, ConstraintAnchor::Vertex, 3);

        assert!(a == b);
        assert!(a != c);
        assert_eq!(
            constraint_anchor_from_token("vertex"),
            ConstraintAnchor::Vertex
        );
    }

    /// Token tables round trip; unknown tokens fall back like C++.
    #[test]
    fn constraint_tokens() {
        for t in CONSTRAINT_TYPES {
            assert_eq!(constraint_type_from_token(constraint_type_token(t)), t);
        }
        assert_eq!(
            constraint_type_from_token("bogus"),
            PcbConstraintType::Undefined
        );
        assert_eq!(
            constraint_anchor_from_token("bogus"),
            ConstraintAnchor::Whole
        );
        assert!(constraint_value_is_length(PcbConstraintType::FixedRadius));
        assert!(!constraint_value_is_length(PcbConstraintType::ArcAngle));
    }
}
