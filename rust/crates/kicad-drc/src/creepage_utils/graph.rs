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
//
// Additional copyright lines carried over from the C++ original:
//
// Copyright (C) 2024 Fabien Corona f.corona<at>laposte.net

//! Port of the creepage graph model of `pcbnew/drc/drc_creepage_utils.h` / `.cpp`:
//! `GRAPH_NODE`, `GRAPH_CONNECTION` (with `GetShapes`), and the part of `CREEPAGE_GRAPH` that
//! builds the board-edge graph: `TransformEdgeToCreepShapes`, `RemoveDuplicatedShapes`,
//! `TransformCreepShapesToNodes`, the `ConnectChildren` overrides of the board-edge shapes with
//! `BE_SHAPE_CIRCLE::ShortenChildDueToGV`, `SetTarget`, and the node / connection management
//! (`AddNode`, `AddNodeVirtual`, `AddConnection`, `FindNode`, `detachConnection`,
//! `TruncateToPrefix`).
//!
//! Ownership: the `shared_ptr` / `unique_ptr` graph becomes index arenas. A node is a
//! [`NodeId`] into `nodes`, a connection a [`ConnId`] into `connections`, a shape a [`ShapeId`]
//! into the shape arena (never reordered, so a `ShapeId` keeps the identity of the
//! `CREEP_SHAPE*`); `shape_collection` is `m_shapeCollection` (the owning, sortable list).
//! `GRAPH_NODE::m_node_conns` (a `std::set` ordered by heap address in C++) is ordered by
//! connection index, see `PORT_STATUS.md`.

use std::collections::{BTreeMap, BTreeSet};

use kicad_geom::{
    ANGLE_0, ANGLE_360, Box2I, EdaAngle, RADIANS_T, ShapePolySet, Vector2D, Vector2I, cmath,
};
use kicad_pcb::BoardCtx;
use kicad_pcb::MutEnv;
use kicad_pcb::eda_shape::ShapeT;
use kicad_pcb::pcb_shape::PcbShape;

use super::board_edges::{board_edge_bounding_box, board_edge_shape};
use super::paths::PathConnection;
use super::shapes::{
    BeShapeArc, BeShapeCircle, BeShapePoint, CreepItem, CreepShape, CreepShapeType, are_equivalent,
    compare_shapes, to_int, vd,
};

/// Index of a shape in the graph's shape arena (the `CREEP_SHAPE*` identity).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeId(pub usize);

/// Index of a node in `CreepageGraph::nodes` (a `std::shared_ptr<GRAPH_NODE>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub usize);

/// Index of a connection in `CreepageGraph::connections` (a
/// `std::shared_ptr<GRAPH_CONNECTION>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnId(pub usize);

/// Port of `GRAPH_NODE::TYPE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GraphNodeType {
    /// `POINT`
    Point = 0,
    /// `CIRCLE`
    Circle,
    /// `ARC`
    Arc,
    /// `SEGMENT`
    Segment,
    /// `VIRTUAL`
    Virtual,
}

/// Port of `GRAPH_NODE`: a node in a [`CreepageGraph`].
#[derive(Clone, Debug, PartialEq)]
pub struct GraphNode {
    /// `m_parent`
    pub parent: Option<ShapeId>,
    /// `m_node_conns`
    pub node_conns: BTreeSet<ConnId>,
    /// `m_pos`
    pub pos: Vector2I,
    /// `m_virtual`: virtual nodes are connected with a 0 weight connection to equivalent net
    /// (same net or netclass)
    pub virtual_: bool,
    /// `m_connectDirectly`
    pub connect_directly: bool,
    /// `m_net`
    pub net: i32,
    /// `m_type`
    pub type_: GraphNodeType,
}

impl GraphNode {
    /// Port of `GRAPH_NODE( GRAPH_NODE::TYPE aType, CREEP_SHAPE* aParent, const VECTOR2I& aPos )`.
    pub fn new(a_type: GraphNodeType, a_parent: Option<ShapeId>, a_pos: Vector2I) -> Self {
        GraphNode {
            parent: a_parent,
            node_conns: BTreeSet::new(),
            pos: a_pos,
            virtual_: false,
            connect_directly: true,
            net: -1,
            type_: a_type,
        }
    }
}

/// Port of `GRAPH_CONNECTION`: a connection in a [`CreepageGraph`].
#[derive(Clone, Debug, PartialEq)]
pub struct GraphConnection {
    /// `n1`
    pub n1: NodeId,
    /// `n2`
    pub n2: NodeId,
    /// `m_path`
    pub path: PathConnection,
    /// `m_forceStraightLine`
    pub force_straight_line: bool,
}

/// The `GraphNodeHash` / `GraphNodeEqual` key of `m_nodeset`.
type NodeKey = (GraphNodeType, Option<ShapeId>, i32, i32);

/// Port of `CREEPAGE_GRAPH`: a graph with nodes and connections for creepage calculation.
pub struct CreepageGraph<'a> {
    /// `m_board`
    pub ctx: &'a BoardCtx<'a>,
    /// `m_boardEdge`
    pub board_edge: Vec<CreepItem>,
    /// `m_ownedBoardEdges`
    pub owned_board_edges: Vec<PcbShape>,
    /// `m_boardOutline` (the C++ points to the caller's polygon set; the graph owns a copy)
    pub board_outline: Option<ShapePolySet>,
    /// `m_hasOverlappingCutouts`
    pub has_overlapping_cutouts: bool,
    /// `m_nodes`
    pub nodes: Vec<GraphNode>,
    /// `m_connections`
    pub connections: Vec<GraphConnection>,
    /// The storage of every `CREEP_SHAPE` the graph has owned (indexed by [`ShapeId`]).
    pub shapes: Vec<CreepShape>,
    /// `m_shapeCollection`
    pub shape_collection: Vec<ShapeId>,
    /// `m_nodeset`: this is a duplicate of `nodes`, but it is used to quickly find a node
    /// rather than iterating through `nodes`
    pub nodeset: BTreeMap<NodeKey, NodeId>,
    /// `m_minGrooveWidth`
    pub min_groove_width: i32,
    /// `m_creepageTarget`
    creepage_target: f64,
    /// `m_creepageTargetSquared`
    creepage_target_squared: f64,
}

impl<'a> CreepageGraph<'a> {
    /// Port of `CREEPAGE_GRAPH( BOARD& aBoard )`.
    pub fn new(ctx: &'a BoardCtx<'a>) -> Self {
        CreepageGraph {
            ctx,
            board_edge: Vec::new(),
            owned_board_edges: Vec::new(),
            board_outline: None,
            has_overlapping_cutouts: false,
            nodes: Vec::new(),
            connections: Vec::new(),
            shapes: Vec::new(),
            shape_collection: Vec::new(),
            nodeset: BTreeMap::new(),
            min_groove_width: 0,
            creepage_target: -1.0,
            creepage_target_squared: -1.0,
        }
    }

    /// The `CREEP_SHAPE` behind a [`ShapeId`].
    pub fn shape(&self, id: ShapeId) -> &CreepShape {
        &self.shapes[id.0]
    }

    /// `m_shapeCollection.push_back( std::move( aShape ) )`: take ownership of a shape.
    pub fn push_shape(&mut self, a_shape: CreepShape) -> ShapeId {
        let id = ShapeId(self.shapes.len());
        self.shapes.push(a_shape);
        self.shape_collection.push(id);
        id
    }

    /// `m_creepageTargetSquared` (read by the path generation of DRC-33).
    pub fn target_squared(&self) -> f64 {
        self.creepage_target_squared
    }

    /// Port of `CREEPAGE_GRAPH::SetTarget()`.
    pub fn set_target(&mut self, a_target: f64) {
        self.creepage_target = a_target;
        self.creepage_target_squared = a_target * a_target;
    }

    /// Port of `CREEPAGE_GRAPH::GetTarget()`.
    pub fn target(&self) -> f64 {
        self.creepage_target
    }

    /// Port of `CREEPAGE_GRAPH::TransformCreepShapesToNodes()` for `m_shapeCollection` (the
    /// only list the C++ passes).
    pub fn transform_creep_shapes_to_nodes(&mut self) {
        let collection = self.shape_collection.clone();
        self.transform_creep_shapes_to_nodes_of(&collection);
    }

    /// Port of `CREEPAGE_GRAPH::TransformCreepShapesToNodes( aShapes )`.
    pub fn transform_creep_shapes_to_nodes_of(&mut self, a_shapes: &[ShapeId]) {
        for &p1 in a_shapes {
            let (type_, pos) = {
                let shape = self.shape(p1);
                (shape.type_(), shape.pos())
            };

            match type_ {
                CreepShapeType::PointType => {
                    self.add_node(GraphNodeType::Point, Some(p1), pos);
                }
                CreepShapeType::Circle => {
                    self.add_node(GraphNodeType::Circle, Some(p1), pos);
                }
                CreepShapeType::Arc => {
                    self.add_node(GraphNodeType::Arc, Some(p1), pos);
                }
                _ => {}
            }
        }
    }

    /// Port of `CREEPAGE_GRAPH::RemoveDuplicatedShapes()`.
    pub fn remove_duplicated_shapes(&mut self) {
        let shapes = &self.shapes;
        kicad_geom::stdsort::std_sort(&mut self.shape_collection, |a, b| {
            compare_shapes(Some(&shapes[a.0]), Some(&shapes[b.0]))
        });

        let mut kept = 0;

        for i in 0..self.shape_collection.len() {
            // Keep the last equivalent shape: its parent metadata may differ from earlier
            // shapes.
            if i + 1 < self.shape_collection.len()
                && are_equivalent(
                    Some(self.shape(self.shape_collection[i])),
                    Some(self.shape(self.shape_collection[i + 1])),
                )
            {
                continue;
            }

            if kept != i {
                self.shape_collection[kept] = self.shape_collection[i];
            }

            kept += 1;
        }

        self.shape_collection.truncate(kept);
    }

    /// `static_cast<PCB_SHAPE*>( aItem )` of a board-edge entry.
    pub fn board_edge_shape(&self, item: CreepItem) -> Option<&PcbShape> {
        board_edge_shape(self.ctx, &self.owned_board_edges, item)
    }

    /// Port of `CREEPAGE_GRAPH::TransformEdgeToCreepShapes()`.
    pub fn transform_edge_to_creep_shapes(&mut self) {
        // Flag overlapping cutouts so the arc void check below only runs when needed.
        let mut cutouts: Vec<Box2I> = Vec::new();

        for &be in &self.board_edge {
            let Some(s) = self.board_edge_shape(be) else {
                continue;
            };

            if matches!(
                s.shape.shape(),
                ShapeT::Rectangle | ShapeT::Circle | ShapeT::Poly
            ) && let Some(bbox) = board_edge_bounding_box(self.ctx, &self.owned_board_edges, be)
            {
                cutouts.push(bbox);
            }
        }

        let mut i = 0;

        while i < cutouts.len() && !self.has_overlapping_cutouts {
            for j in (i + 1)..cutouts.len() {
                if cutouts[i].intersects(&cutouts[j])
                    && !cutouts[i].contains_box(&cutouts[j])
                    && !cutouts[j].contains_box(&cutouts[i])
                {
                    self.has_overlapping_cutouts = true;
                    break;
                }
            }

            i += 1;
        }

        let board_edge = self.board_edge.clone();

        for drawing in board_edge {
            let Some(d) = self.board_edge_shape(drawing) else {
                continue;
            };
            let d = d.shape.clone();
            let parent = Some(drawing);

            let mut new_shapes: Vec<CreepShape> = Vec::new();
            let mut push_point = |pos: Vector2I| {
                let mut a = BeShapePoint::new(pos);
                a.base.parent = parent;
                new_shapes.push(CreepShape::BePoint(a));
            };

            match d.shape() {
                ShapeT::Segment => {
                    push_point(d.start());
                    push_point(d.end());
                }

                ShapeT::Rectangle => {
                    let r = d.corner_radius();

                    if r > 0 {
                        // Rounded rectangle: decompose into arcs.
                        // Normalize coordinates so x1 < x2 and y1 < y2.
                        let x1 = d.start().x.min(d.end().x);
                        let y1 = d.start().y.min(d.end().y);
                        let x2 = d.start().x.max(d.end().x);
                        let y2 = d.start().y.max(d.end().y);

                        let w = x2 - x1;
                        let h = y2 - y1;

                        let mut add_arc =
                            |center: (i32, i32), start_pt: (i32, i32), end_pt: (i32, i32)| {
                                let center = Vector2I::new(center.0, center.1);
                                let start_pt = Vector2I::new(start_pt.0, start_pt.1);
                                let end_pt = Vector2I::new(end_pt.0, end_pt.1);
                                let start_angle = EdaAngle::from_vector(vd(start_pt - center));
                                let mut end_angle = EdaAngle::from_vector(vd(end_pt - center));

                                while end_angle < start_angle {
                                    end_angle += ANGLE_360;
                                }

                                let mut arc = BeShapeArc::new(
                                    center,
                                    r,
                                    start_angle,
                                    end_angle,
                                    vd(start_pt),
                                    vd(end_pt),
                                );
                                arc.circle.base.parent = parent;
                                new_shapes.push(CreepShape::BeArc(arc));
                            };

                        if h == 2 * r {
                            // Horizontal stadium: left and right semicircles. The endpoint
                            // order makes addArc sweep the outer half of each circle so the
                            // caps bulge away from the slot.
                            add_arc((x1 + r, y1 + r), (x1 + r, y2), (x1 + r, y1));
                            add_arc((x2 - r, y1 + r), (x2 - r, y1), (x2 - r, y2));
                        } else if w == 2 * r {
                            // Vertical stadium: top and bottom semicircles
                            add_arc((x1 + r, y1 + r), (x1, y1 + r), (x2, y1 + r));
                            add_arc((x1 + r, y2 - r), (x2, y2 - r), (x1, y2 - r));
                        } else {
                            // General rounded rectangle: four quarter-circle arcs
                            add_arc((x1 + r, y1 + r), (x1, y1 + r), (x1 + r, y1));
                            add_arc((x2 - r, y1 + r), (x2 - r, y1), (x2, y1 + r));
                            add_arc((x2 - r, y2 - r), (x2, y2 - r), (x2 - r, y2));
                            add_arc((x1 + r, y2 - r), (x1 + r, y2), (x1, y2 - r));
                        }
                    } else {
                        push_point(d.start());
                        push_point(d.end());
                        push_point(Vector2I::new(d.end().x, d.start().y));
                        push_point(Vector2I::new(d.start().x, d.end().y));
                    }
                }

                ShapeT::Poly => {
                    for p in d.poly_points() {
                        push_point(p);
                    }
                }

                ShapeT::Circle => {
                    let mut a = BeShapeCircle::new(d.center(), d.radius());
                    a.base.parent = parent;
                    new_shapes.push(CreepShape::BeCircle(a));
                }

                ShapeT::Arc => {
                    // If the arc is not locally convex, only use the endpoints
                    // (the C++ computes a `dir` vector here that it never uses)
                    let (alpha, beta) = d.calc_arc_angles();
                    let mut a = BeShapeArc::new(
                        d.center(),
                        d.radius(),
                        alpha,
                        beta,
                        vd(d.start()),
                        vd(d.end()),
                    );
                    a.circle.base.parent = parent;

                    new_shapes.push(CreepShape::BeArc(a));
                }

                _ => {}
            }

            for s in new_shapes {
                self.push_shape(s);
            }
        }
    }

    /// Port of `CREEP_SHAPE::ConnectChildren()` (virtual): connect two child nodes of the
    /// shape `a_shape`. Only the board-edge circles and arcs do anything.
    pub fn connect_children(&mut self, a_shape: ShapeId, a1: NodeId, a2: NodeId) {
        match self.shape(a_shape).clone() {
            CreepShape::BeCircle(c) => self.connect_children_be_circle(&c, a1, a2),
            CreepShape::BeArc(arc) => self.connect_children_be_arc(&arc, a1, a2),
            // CREEP_SHAPE::ConnectChildren and BE_SHAPE_POINT::ConnectChildren are empty
            _ => {}
        }
    }

    /// Whether the outline contains `p` or has it on an edge, with tolerance `tol`.
    fn outline_has_point(&self, p: Vector2I, tol: i32) -> bool {
        let outline = self.board_outline.as_ref().expect("checked by the caller");

        outline.contains(p, -1, tol, false) || outline.point_on_edge(p, tol)
    }

    /// Port of `BE_SHAPE_CIRCLE::ShortenChildDueToGV()`.
    pub fn shorten_child_due_to_gv(
        &mut self,
        circle: &BeShapeCircle,
        a1: NodeId,
        a2: NodeId,
        a_normal_weight: f64,
    ) {
        let m_pos = circle.pos();
        let m_radius = circle.radius();
        let a1_pos = self.nodes[a1.0].pos;
        let a2_pos = self.nodes[a2.0].pos;

        let mut angle1 = EdaAngle::from_vector(a1_pos - m_pos);
        let mut angle2 = EdaAngle::from_vector(a2_pos - m_pos);

        while angle1 < ANGLE_0 {
            angle1 += ANGLE_360;
        }
        while angle2 < ANGLE_0 {
            angle2 += ANGLE_360;
        }
        while angle1 > ANGLE_360 {
            angle1 -= ANGLE_360;
        }
        while angle2 > ANGLE_360 {
            angle2 -= ANGLE_360;
        }

        let max_angle = if angle1 > angle2 { angle1 } else { angle2 };
        // asin( float ) is the float overload (asinf) in the reference build
        let ratio: f32 = self.min_groove_width as f32 / (2 * m_radius) as f32;
        let mut skip_angle = EdaAngle::new(asinf(ratio) as f64, RADIANS_T);
        skip_angle += skip_angle; // Cannot multiply EDA_ANGLE by scalar, but this really is angle *2
        let point_angle = max_angle - skip_angle;

        let mut skip_point = m_pos;
        skip_point.x =
            to_int(skip_point.x as f64 + m_radius as f64 * cmath::cos(point_angle.as_radians()));
        skip_point.y =
            to_int(skip_point.y as f64 + m_radius as f64 * cmath::sin(point_angle.as_radians()));

        let a1_parent = self.nodes[a1.0].parent;
        let gnt = self.add_node(GraphNodeType::Point, a1_parent, skip_point);

        let max_is_angle2 = max_angle == angle2;

        let mut pc = PathConnection {
            a1: vd(if max_is_angle2 { a1_pos } else { a2_pos }),
            a2: vd(skip_point),
            weight: a_normal_weight - self.min_groove_width as f64,
            ..PathConnection::default()
        };
        self.add_connection(if max_is_angle2 { a1 } else { a2 }, gnt, &pc);

        pc.a1 = vd(skip_point);
        pc.a2 = vd(if max_is_angle2 { a2_pos } else { a1_pos });
        pc.weight = self.min_groove_width as f64;

        let gc = self.add_connection(gnt, if max_is_angle2 { a2 } else { a1 }, &pc);

        self.connections[gc.0].force_straight_line = true;
    }

    /// Port of `BE_SHAPE_CIRCLE::ConnectChildren()`.
    pub fn connect_children_be_circle(&mut self, circle: &BeShapeCircle, a1: NodeId, a2: NodeId) {
        let m_pos = circle.pos();
        let m_radius = circle.radius();

        if m_radius == 0 {
            return;
        }

        let a1_pos = self.nodes[a1.0].pos;
        let a2_pos = self.nodes[a2.0].pos;

        // When cutouts overlap, part of this wall runs inside the merged void and is not
        // a real edge to hug. Check the shorter arc, the one the solver measures and draws.
        if self.has_overlapping_cutouts && self.board_outline.is_some() {
            let tol = self.ctx.board.design_settings().max_error() + 1000;
            let a1r = EdaAngle::from_vector(a1_pos - m_pos).as_radians();
            let a2r = EdaAngle::from_vector(a2_pos - m_pos).as_radians();
            let mut delta = a2r - a1r;

            while delta > std::f64::consts::PI {
                delta -= 2.0 * std::f64::consts::PI;
            }
            while delta < -std::f64::consts::PI {
                delta += 2.0 * std::f64::consts::PI;
            }

            for i in 0..=8 {
                let a = a1r + delta * i as f64 / 8.0;
                let p = Vector2I::new(
                    to_int(m_pos.x as f64 + m_radius as f64 * cmath::cos(a)),
                    to_int(m_pos.y as f64 + m_radius as f64 * cmath::sin(a)),
                );

                if !self.outline_has_point(p, tol) {
                    return;
                }
            }
        }

        let dist_i: Vector2D = vd(a1_pos - a2_pos);
        let dist_d = Vector2D::new(dist_i.x, dist_i.y);

        let weight =
            (m_radius * 2) as f64 * cmath::asin(dist_d.euclidean_norm() / (2.0 * m_radius as f64));

        if weight > self.target() {
            return;
        }

        if self.min_groove_width <= 0 {
            let pc = PathConnection {
                a1: vd(a1_pos),
                a2: vd(a2_pos),
                weight: weight.max(0.0),
                ..PathConnection::default()
            };

            self.add_connection(a1, a2, &pc);
            return;
        }

        if weight > self.min_groove_width as f64 {
            self.shorten_child_due_to_gv(circle, a1, a2, weight);
        }
        // Else well.. this paths will be "shorted" by another one
    }

    /// Port of `BE_SHAPE_ARC::ConnectChildren()`.
    pub fn connect_children_be_arc(&mut self, arc: &BeShapeArc, a1: NodeId, a2: NodeId) {
        let m_pos = arc.pos();
        let m_radius = arc.radius();
        let a1_pos = self.nodes[a1.0].pos;
        let a2_pos = self.nodes[a2.0].pos;

        let angle1 = arc.angle_between_start_and_end(a1_pos);
        let angle2 = arc.angle_between_start_and_end(a2_pos);

        // Skip an arc that dips into an overlapping cutout, it is not a real edge to hug.
        // Sample the whole sub-arc, the tolerance clears the outline arc-to-segment error.
        if self.has_overlapping_cutouts && self.board_outline.is_some() {
            let tol = self.ctx.board.design_settings().max_error() + 1000;
            let a1r = angle1.as_radians();
            let a2r = angle2.as_radians();

            for i in 0..=8 {
                let a = a1r + (a2r - a1r) * i as f64 / 8.0;
                let p = Vector2I::new(
                    to_int(m_pos.x as f64 + m_radius as f64 * cmath::cos(a)),
                    to_int(m_pos.y as f64 + m_radius as f64 * cmath::sin(a)),
                );

                if !self.outline_has_point(p, tol) {
                    return;
                }
            }
        }

        let weight = (m_radius as f64 * (angle2 - angle1).as_radians()).abs();

        if self.min_groove_width <= 0 {
            if weight > self.target() {
                return;
            }

            let pc = PathConnection {
                a1: vd(a1_pos),
                a2: vd(a2_pos),
                weight,
                ..PathConnection::default()
            };

            self.add_connection(a1, a2, &pc);
            return;
        }

        if weight > self.min_groove_width as f64 {
            self.shorten_child_due_to_gv(&arc.circle, a1, a2, weight);
        }
    }

    /// Port of `CREEPAGE_GRAPH::detachConnection()`.
    fn detach_connection(&mut self, a_gc: ConnId) {
        let (n1, n2) = {
            let gc = &self.connections[a_gc.0];
            (gc.n1, gc.n2)
        };

        for gn in [n1, n2] {
            if let Some(node) = self.nodes.get_mut(gn.0) {
                node.node_conns.remove(&a_gc);
            }
        }
    }

    /// Port of `CREEPAGE_GRAPH::TruncateToPrefix()`: remove every node and connection added
    /// after the given prefix sizes, then rebuild the node lookup set from the surviving
    /// prefix. Used to reuse a graph across solves without leaking stale per-solve nodes into
    /// `find_node` / `add_node` lookups.
    pub fn truncate_to_prefix(&mut self, a_node_count: usize, a_connection_count: usize) {
        let vector_size = self.connections.len();

        // Detach each connection from its endpoints' lists; the bulk resize drops them in one
        // shot
        for i in a_connection_count..vector_size {
            self.detach_connection(ConnId(i));
        }

        // (The C++ resize can also grow the vectors with null entries; the callers only shrink.)
        self.connections.truncate(a_connection_count);
        self.nodes.truncate(a_node_count);

        // Without this, stale per-solve nodes corrupt subsequent FindNode/AddNode lookups
        self.nodeset.clear();

        for i in 0..self.nodes.len() {
            let key = node_key(&self.nodes[i]);
            self.nodeset.entry(key).or_insert(NodeId(i));
        }
    }

    /// Port of `CREEPAGE_GRAPH::AddNode()`: add a node to the graph. If an equivalent node
    /// exists, returns the existing node instead.
    pub fn add_node(
        &mut self,
        a_type: GraphNodeType,
        parent: Option<ShapeId>,
        pos: Vector2I,
    ) -> NodeId {
        if let Some(gn) = self.find_node(a_type, parent, pos) {
            return gn;
        }

        let gn = NodeId(self.nodes.len());
        let node = GraphNode::new(a_type, parent, pos);
        self.nodeset.entry(node_key(&node)).or_insert(gn);
        self.nodes.push(node);
        gn
    }

    /// Port of `CREEPAGE_GRAPH::AddNodeVirtual()`. Virtual nodes are always unique, they are
    /// not looked up (`m_nodeset` keeps the first one, as the `unordered_set` insert does).
    pub fn add_node_virtual(&mut self) -> NodeId {
        let gn = NodeId(self.nodes.len());
        let node = GraphNode::new(GraphNodeType::Virtual, None, Vector2I::new(0, 0));
        self.nodeset.entry(node_key(&node)).or_insert(gn);
        self.nodes.push(node);
        gn
    }

    /// Port of `CREEPAGE_GRAPH::AddConnection( aN1, aN2, aPc )`. (The C++ null checks have no
    /// counterpart; its `wxASSERT` against a node connected to itself is a no-op in the
    /// reference build.)
    pub fn add_connection(&mut self, a_n1: NodeId, a_n2: NodeId, a_pc: &PathConnection) -> ConnId {
        let gc = ConnId(self.connections.len());
        self.connections.push(GraphConnection {
            n1: a_n1,
            n2: a_n2,
            path: a_pc.clone(),
            force_straight_line: false,
        });
        self.nodes[a_n1.0].node_conns.insert(gc);
        self.nodes[a_n2.0].node_conns.insert(gc);

        gc
    }

    /// Port of `CREEPAGE_GRAPH::AddConnection( aN1, aN2 )`: a zero-weight connection between
    /// the node positions.
    pub fn add_connection_default(&mut self, a_n1: NodeId, a_n2: NodeId) -> ConnId {
        let pc = PathConnection {
            a1: vd(self.nodes[a_n1.0].pos),
            a2: vd(self.nodes[a_n2.0].pos),
            weight: 0.0,
            ..PathConnection::default()
        };

        self.add_connection(a_n1, a_n2, &pc)
    }

    /// Port of `CREEPAGE_GRAPH::FindNode()`.
    pub fn find_node(
        &self,
        a_type: GraphNodeType,
        a_parent: Option<ShapeId>,
        a_pos: Vector2I,
    ) -> Option<NodeId> {
        self.nodeset
            .get(&(a_type, a_parent, a_pos.x, a_pos.y))
            .copied()
    }

    /// Port of `GRAPH_CONNECTION::GetShapes()`: the drawable shapes of a connection (an arc
    /// along a board-edge circle or arc, else a segment), appended to `a_shapes`.
    pub fn connection_shapes(&self, a_gc: ConnId, a_shapes: &mut Vec<PcbShape>) {
        let gc = &self.connections[a_gc.0];
        let env = MutEnv::detached();

        if !gc.path.show {
            return;
        }

        let n1 = &self.nodes[gc.n1.0];
        let n2 = &self.nodes[gc.n2.0];

        if n1.type_ == GraphNodeType::Virtual || n2.type_ == GraphNodeType::Virtual {
            return;
        }

        let same_parent = n1.parent.is_some() && n1.parent == n2.parent;

        if !gc.force_straight_line
            && same_parent
            && self.shape(n1.parent.unwrap()).type_() == CreepShapeType::Circle
        {
            let center = self.shape(n1.parent.unwrap()).pos();
            let r1 = n1.pos - center;
            let r2 = n2.pos - center;
            let mut s = PcbShape::with_shape(ShapeT::Arc);

            if r1.cross(r2) > 0 {
                s.set_start(&env, n1.pos);
                s.set_end(&env, n2.pos);
            } else {
                s.set_start(&env, n2.pos);
                s.set_end(&env, n1.pos);
            }

            s.set_center(&env, center);
            a_shapes.push(s);
            return;
        }

        if !gc.force_straight_line
            && same_parent
            && self.shape(n1.parent.unwrap()).type_() == CreepShapeType::Arc
            && let CreepShape::BeArc(arc) = self.shape(n1.parent.unwrap())
        {
            let center = arc.pos();
            let r1 = n1.pos - center;
            let r2 = n2.pos - center;
            let mut s = PcbShape::with_shape(ShapeT::Arc);

            if r1.cross(r2) > 0 {
                s.set_start(&env, n1.pos);
                s.set_end(&env, n2.pos);
            } else {
                s.set_start(&env, n2.pos);
                s.set_end(&env, n1.pos);
            }

            s.set_center(&env, center);

            //Check that we are on the correct side of the arc.
            let mid = s.shape.arc_mid();
            let mid_angle = arc.angle_between_start_and_end(mid);

            if mid_angle > arc.end_angle() {
                let tmp = s.shape.start();
                let end = s.shape.end();
                s.set_start(&env, end);
                s.set_end(&env, tmp);
                s.set_center(&env, center);
            }

            a_shapes.push(s);
            return;
        }

        let mut s = PcbShape::with_shape(ShapeT::Segment);
        s.set_start(&env, super::shapes::vi(gc.path.a1));
        s.set_end(&env, super::shapes::vi(gc.path.a2));
        a_shapes.push(s);
    }
}

/// The `m_nodeset` key of a node (`GraphNodeHash` / `GraphNodeEqual`: type, parent, position).
fn node_key(node: &GraphNode) -> NodeKey {
    (node.type_, node.parent, node.pos.x, node.pos.y)
}

/// `asinf`: the `float` overload of `asin` the reference build calls in
/// `ShortenChildDueToGV`. Natively Rust's `f32::asin` is the platform `asinf` (glibc, as the
/// reference); on wasm32 it is Rust's bundled libm (the documented wasm-only libm deviation).
fn asinf(x: f32) -> f32 {
    x.asin()
}
