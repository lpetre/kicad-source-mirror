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

//! The read half of the router API: board, nets, net classes, pads, footprints, tracks, zones,
//! copper obstacles, Edge.Cuts shapes and connectivity, from the ported model getters (each
//! field names the C++ getter it reads; the same getters the reference dump prints).

use std::collections::BTreeSet;

use kicad_core::PcbLayerId;
use kicad_core::layer_ids::is_copper_layer;
use kicad_core::lset::Lset;
use kicad_geom::ShapePolySet;
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_pcb::ItemId;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::item_ref::{AnyItem, ItemRef};
use kicad_pcb::netclass::Netclass;
use kicad_pcb::padstack::PadDrillShape;
use kicad_pcb::pcb_track::{Track, Viatype};
use serde::{Deserialize, Serialize};

use super::{Polygons, Pt, enum_name, handle, layer_name, layer_names, polygons, pt};
use crate::session::Session;

/// A copper layer of [`BoardInfo::copper_layers`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CopperLayer {
    /// `LSET::Name` (`F.Cu`, `In1.Cu`, ..., `B.Cu`).
    pub name: String,
    /// `BOARD::GetLayerName` (the user name).
    pub user_name: String,
    /// `LAYER::ShowType( BOARD::GetLayerType )`: `signal`, `power`, `mixed`, `jumper`.
    pub layer_type: String,
    /// `PCB_LAYER_ID` value.
    pub layer_id: i32,
    /// `BOARD_STACKUP::GetLayerDistance( F_Cu, layer )` of `GetStackupOrDefault()`: the depth
    /// of the layer below the top copper (nm).
    pub depth: i32,
}

/// The design rule minimums of `BOARD_DESIGN_SETTINGS`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleMinimums {
    /// `m_MinClearance`.
    pub min_clearance: i32,
    /// `m_TrackMinWidth`.
    pub track_min_width: i32,
    /// `m_MinConn`.
    pub min_connection: i32,
    /// `m_ViasMinAnnularWidth`.
    pub vias_min_annular_width: i32,
    /// `m_ViasMinSize`.
    pub vias_min_size: i32,
    /// `m_MinThroughDrill`.
    pub min_through_drill: i32,
    /// `m_MicroViasMinSize`.
    pub micro_vias_min_size: i32,
    /// `m_MicroViasMinDrill`.
    pub micro_vias_min_drill: i32,
    /// `m_CopperEdgeClearance`.
    pub copper_edge_clearance: i32,
    /// `m_HoleClearance`.
    pub hole_clearance: i32,
    /// `m_HoleToHoleMin`.
    pub hole_to_hole_min: i32,
    /// `m_MaxError`.
    pub max_error: i32,
}

/// One layer of the stackup (`BOARD_STACKUP_ITEM`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StackupLayer {
    /// `GetLayerName()`.
    pub name: String,
    /// `GetTypeName()` (`copper`, `core`, `prepreg`, `Top Solder Mask`, ...).
    pub type_name: String,
    /// `LSET::Name( GetBrdLayerId() )`, `None` for a dielectric.
    pub layer: Option<String>,
    /// The sum of `GetThickness( i )` over the sublayers (nm).
    pub thickness: i32,
}

/// What [`Session::board_info`] returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BoardInfo {
    /// `BOARD::GetFileName()`.
    pub name: String,
    /// `GetCopperLayerCount()`.
    pub copper_layer_count: i32,
    /// `LSET::AllCuMask( count ).CuStack()`: top to bottom.
    pub copper_layers: Vec<CopperLayer>,
    /// `GetEnabledLayers().Seq()`.
    pub enabled_layers: Vec<String>,
    /// `GetBoardPolygonOutlines( outlines, true )` (inferred from the footprints when there
    /// is no closed Edge.Cuts outline).
    pub outline: Polygons,
    /// The return value of `GetBoardPolygonOutlines`.
    pub outline_ok: bool,
    /// `ComputeBoundingBox( false )`.
    pub bbox: [i64; 4],
    /// `GetBoardEdgesBoundingBox()`.
    pub edges_bbox: [i64; 4],
    /// The rule minimums.
    pub rules: RuleMinimums,
    /// `BOARD_DESIGN_SETTINGS::GetBoardThickness()`.
    pub board_thickness: i32,
    /// `GetStackupOrDefault().GetList()`.
    pub stackup: Vec<StackupLayer>,
}

/// A net (`NETINFO_ITEM`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetDetail {
    /// `GetNetCode()`.
    pub code: i32,
    /// `GetNetname()`.
    pub name: String,
    /// `GetNetClass()->GetName()` (the effective, possibly composite, net class).
    pub netclass: String,
    /// The names of the constituent net classes of the effective class
    /// (`NETCLASS::GetConstituentNetclasses()`), highest priority first.
    pub netclasses: Vec<String>,
}

/// A net class (`NETCLASS`); `None` = not set in this class.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetclassInfo {
    /// `GetName()`.
    pub name: String,
    /// `GetPriority()`.
    pub priority: i32,
    /// `m_Clearance`.
    pub clearance: Option<i32>,
    /// `m_TrackWidth`.
    pub track_width: Option<i32>,
    /// `m_ViaDia`.
    pub via_diameter: Option<i32>,
    /// `m_ViaDrill`.
    pub via_drill: Option<i32>,
    /// `m_uViaDia`.
    pub microvia_diameter: Option<i32>,
    /// `m_uViaDrill`.
    pub microvia_drill: Option<i32>,
    /// `m_diffPairWidth`.
    pub diff_pair_width: Option<i32>,
    /// `m_diffPairGap`.
    pub diff_pair_gap: Option<i32>,
    /// `m_diffPairViaGap`.
    pub diff_pair_via_gap: Option<i32>,
}

impl NetclassInfo {
    fn of(nc: &Netclass) -> NetclassInfo {
        NetclassInfo {
            name: nc.name.clone(),
            priority: nc.priority,
            clearance: nc.clearance,
            track_width: nc.track_width,
            via_diameter: nc.via_dia,
            via_drill: nc.via_drill,
            microvia_diameter: nc.uvia_dia,
            microvia_drill: nc.uvia_drill,
            diff_pair_width: nc.diff_pair_width,
            diff_pair_gap: nc.diff_pair_gap,
            diff_pair_via_gap: nc.diff_pair_via_gap,
        }
    }
}

/// A pad hole.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DrillInfo {
    /// `GetDrillShape()`: `CIRCLE` or `OBLONG` (a slot).
    pub shape: String,
    /// `GetDrillSize()` (board frame, nm).
    pub size: Pt,
}

/// The primitive description of a pad on one padstack layer (board frame getters).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PadShapeInfo {
    /// The padstack layer (`F.Cu` stands for every copper layer in `NORMAL` mode).
    pub layer: String,
    /// `GetShape( layer )`: `CIRCLE`, `RECTANGLE`, `OVAL`, `TRAPEZOID`, `ROUNDRECT`,
    /// `CHAMFERED_RECT`, `CUSTOM`.
    pub shape: String,
    /// `GetSize( layer )`.
    pub size: Pt,
    /// `GetOffset( layer )`: the shape offset from the hole.
    pub offset: Pt,
    /// `GetRoundRectCornerRadius( layer )`.
    pub corner_radius: i32,
    /// `GetChamferRectRatio( layer )`.
    pub chamfer_ratio: f64,
    /// `GetChamferPositions( layer )` (`RECT_CHAMFER_POSITIONS` bits).
    pub chamfer_positions: i32,
}

/// The effective copper polygon of a pad on one copper layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PadPolygon {
    /// The copper layer.
    pub layer: String,
    /// `FlashLayer( layer )`: false where the pad has no copper (unconnected layer removal).
    pub flashed: bool,
    /// `GetEffectivePolygon( layer, ERROR_INSIDE )` (board max error).
    pub polygon: Polygons,
}

/// A pad.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PadInfo {
    /// The handle.
    pub id: u64,
    /// `m_Uuid`.
    pub uuid: String,
    /// The parent footprint's handle.
    pub footprint: u64,
    /// The parent footprint's UUID.
    pub footprint_uuid: String,
    /// The parent footprint's `GetReference()`.
    pub reference: String,
    /// `GetNumber()`.
    pub number: String,
    /// `GetNetCode()`.
    pub net: i32,
    /// `GetPosition()`.
    pub position: Pt,
    /// `GetOrientation().AsDegrees()`.
    pub orientation: f64,
    /// `GetLayerSet().Seq()`.
    pub layers: Vec<String>,
    /// `GetAttribute()`: `PTH`, `SMD`, `CONN`, `NPTH`.
    pub attribute: String,
    /// The hole, `None` without one (`GetDrillSize()` zero).
    pub drill: Option<DrillInfo>,
    /// The primitive per unique padstack layer.
    pub shapes: Vec<PadShapeInfo>,
    /// The effective polygon per copper layer the pad is on (only with polygons requested).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub polygons: Vec<PadPolygon>,
    /// `GetTeardropParams().m_Enabled`.
    pub teardrops: bool,
    /// `IsLocked()`.
    pub locked: bool,
}

/// A footprint.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FootprintInfo {
    /// The handle.
    pub id: u64,
    /// `m_Uuid`.
    pub uuid: String,
    /// `GetReference()`.
    pub reference: String,
    /// `GetValue()`.
    pub value: String,
    /// `GetPosition()`.
    pub position: Pt,
    /// `GetOrientation().AsDegrees()`.
    pub orientation: f64,
    /// `GetSide()`: `F.Cu` or `B.Cu`.
    pub side: String,
    /// `GetCourtyard( F_CrtYd )`.
    pub courtyard_front: Polygons,
    /// `GetCourtyard( B_CrtYd )`.
    pub courtyard_back: Polygons,
    /// `GetBoundingBox()`.
    pub bbox: [i64; 4],
    /// `IsLocked()`.
    pub locked: bool,
    /// `IsDNP()`.
    pub dnp: bool,
    /// The pad handles in `Pads()` order.
    pub pads: Vec<u64>,
}

/// Via details of a [`RouteItem`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ViaInfo {
    /// `GetViaType()`: `through`, `blind`, `buried`, `micro`.
    pub via_type: String,
    /// `TopLayer()`.
    pub top: String,
    /// `BottomLayer()`.
    pub bottom: String,
    /// `GetWidth( layer )` for every copper layer the via is on (`IsOnLayer`), top to bottom.
    pub diameters: Vec<(String, i32)>,
    /// `GetDrillValue()` (the drill, or the default drill when unset).
    pub drill: i32,
    /// `GetIsFree()`.
    pub free: bool,
    /// The mask / hole treatment modes ([`ViaFinish`]).
    pub finish: ViaFinish,
    /// `Padstack().UnconnectedLayerMode()`: `KEEP_ALL`, `START_END_ONLY`, `REMOVE_ALL`,
    /// `REMOVE_EXCEPT_START_AND_END`.
    pub unconnected_layers: String,
    /// The copper layers with `GetZoneLayerOverride( layer ) == ZLO_FORCE_FLASHED` (derived
    /// by the zone filler; the file's `zone_layer_connections`).
    pub zone_layer_connections: Vec<String>,
    /// `GetTeardropParams()`.
    pub teardrops: super::edit::TeardropParams,
}

/// The via mask / hole treatment modes: `from_board`, `yes` or `no` each
/// (`TENTING_MODE`, `COVERING_MODE`, `PLUGGING_MODE`, `CAPPING_MODE`, `FILLING_MODE`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ViaFinish {
    /// `GetFrontTentingMode()`, `GetBackTentingMode()`.
    pub tenting: [String; 2],
    /// `GetFrontCoveringMode()`, `GetBackCoveringMode()`.
    pub covering: [String; 2],
    /// `GetFrontPluggingMode()`, `GetBackPluggingMode()`.
    pub plugging: [String; 2],
    /// `GetCappingMode()`.
    pub capping: String,
    /// `GetFillingMode()`.
    pub filling: String,
}

impl Default for ViaFinish {
    fn default() -> Self {
        let f = || "from_board".to_string();
        ViaFinish {
            tenting: [f(), f()],
            covering: [f(), f()],
            plugging: [f(), f()],
            capping: f(),
            filling: f(),
        }
    }
}

/// `from_board` / `yes` / `no` of a mode value 0 / 1 / 2.
pub(crate) fn mode_name(v: i32) -> String {
    match v {
        1 => "yes",
        2 => "no",
        _ => "from_board",
    }
    .to_string()
}

impl ViaFinish {
    /// The modes of a via.
    pub fn of(v: &kicad_pcb::pcb_track::PcbVia) -> ViaFinish {
        ViaFinish {
            tenting: [
                mode_name(v.front_tenting_mode() as i32),
                mode_name(v.back_tenting_mode() as i32),
            ],
            covering: [
                mode_name(v.front_covering_mode() as i32),
                mode_name(v.back_covering_mode() as i32),
            ],
            plugging: [
                mode_name(v.front_plugging_mode() as i32),
                mode_name(v.back_plugging_mode() as i32),
            ],
            capping: mode_name(v.capping_mode() as i32),
            filling: mode_name(v.filling_mode() as i32),
        }
    }
}

/// A track segment, arc or via.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteItem {
    /// The handle.
    pub id: u64,
    /// `m_Uuid`.
    pub uuid: String,
    /// `track`, `arc` or `via`.
    pub kind: String,
    /// `GetNetCode()`.
    pub net: i32,
    /// `GetStart()` (the position of a via).
    pub start: Pt,
    /// `GetEnd()`.
    pub end: Pt,
    /// `GetMid()` of an arc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mid: Option<Pt>,
    /// `GetWidth()` (a via: `GetWidth( F_Cu )`, as `PCB_VIA::GetWidth()` without layer).
    pub width: i32,
    /// `GetLayer()` (a via: its top layer).
    pub layer: String,
    /// `IsLocked()`.
    pub locked: bool,
    /// `HasSolderMask()` (a segment / arc on an outer layer with a mask opening).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub solder_mask: bool,
    /// `GetLocalSolderMaskMargin()`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub solder_mask_margin: Option<i32>,
    /// Via details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub via: Option<ViaInfo>,
}

/// The keepout flags of a rule area.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Keepouts {
    /// `GetDoNotAllowTracks()`.
    pub tracks: bool,
    /// `GetDoNotAllowVias()`.
    pub vias: bool,
    /// `GetDoNotAllowPads()`.
    pub pads: bool,
    /// `GetDoNotAllowZoneFills()`.
    pub copper_pour: bool,
    /// `GetDoNotAllowFootprints()`.
    pub footprints: bool,
}

/// The fill of a zone on one layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneFill {
    /// The layer.
    pub layer: String,
    /// `GetFilledPolysList( layer )`.
    pub polygons: Polygons,
}

/// A zone or rule area.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ZoneInfo {
    /// The handle.
    pub id: u64,
    /// `m_Uuid`.
    pub uuid: String,
    /// The parent footprint's handle for a footprint zone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<u64>,
    /// `GetZoneName()`.
    pub name: String,
    /// `GetNetCode()`.
    pub net: i32,
    /// `GetLayerSet().Seq()`.
    pub layers: Vec<String>,
    /// `GetAssignedPriority()`.
    pub priority: u32,
    /// `GetIsRuleArea()`.
    pub rule_area: bool,
    /// The keepout flags (rule areas).
    pub keepout: Keepouts,
    /// `IsTeardropArea()`.
    pub teardrop: bool,
    /// `IsLocked()`.
    pub locked: bool,
    /// `GetLocalClearance()`.
    pub clearance: Option<i32>,
    /// `GetMinThickness()`.
    pub min_thickness: i32,
    /// `GetPadConnection()`: `INHERITED`, `NONE`, `THERMAL`, `FULL`, `THT_THERMAL`.
    pub connection: String,
    /// `GetIslandRemovalMode()`: `ALWAYS`, `NEVER`, `AREA`.
    pub island_mode: String,
    /// `GetBoardOutline()` (board frame).
    pub outline: Polygons,
    /// `IsFilled()`.
    pub filled: bool,
    /// The fill per layer that has one (only with polygons requested).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fills: Vec<ZoneFill>,
}

/// A copper obstacle that is neither pad, track nor zone: a graphic shape or text on a copper
/// layer (board or footprint), or an Edge.Cuts shape.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ObstacleInfo {
    /// The handle.
    pub id: u64,
    /// `m_Uuid`.
    pub uuid: String,
    /// `KICAD_T` name (`PCB_SHAPE_T`, `PCB_TEXT_T`, `PCB_TEXTBOX_T`, ...).
    pub kind: String,
    /// The layer of [`ObstacleInfo::polygon`].
    pub layer: String,
    /// `GetNetCode()` of a connected shape, else 0.
    pub net: i32,
    /// The parent footprint's handle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<u64>,
    /// `TransformShapeToPolygon( buf, layer, 0, maxError, ERROR_INSIDE )`.
    pub polygon: Polygons,
}

/// One unconnected edge of the ratsnest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RatsnestEdge {
    /// The net code.
    pub net: i32,
    /// The handle of the source anchor's item.
    pub source: u64,
    /// The handle of the target anchor's item.
    pub target: u64,
    /// The source anchor position.
    pub source_pos: Pt,
    /// The target anchor position.
    pub target_pos: Pt,
}

/// The connection state of one net.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetConnection {
    /// The net code.
    pub net: i32,
    /// `GetNodeCount( net )`: pads of the net.
    pub pads: u32,
    /// The number of unconnected ratsnest edges.
    pub unconnected: u32,
    /// The clusters of the net (`CN_CONNECTIVITY_ALGO::GetClusters()` with an origin net):
    /// each the sorted item handles that are already connected to each other.
    pub clusters: Vec<Vec<u64>>,
}

/// What [`Session::connectivity_info`] returns.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectivityInfo {
    /// `GetUnconnectedCount( false )`.
    pub unconnected: u32,
    /// The unconnected edges (`RunOnUnconnectedEdges` order).
    pub edges: Vec<RatsnestEdge>,
    /// Per net with a code > 0, in code order.
    pub nets: Vec<NetConnection>,
}

fn bbox4(b: kicad_geom::Box2I) -> [i64; 4] {
    let p = b.position();
    let s = b.size();
    [i64::from(p.x), i64::from(p.y), s.x, s.y]
}

fn via_type_name(t: Viatype) -> &'static str {
    match t {
        Viatype::Through => "through",
        Viatype::Blind => "blind",
        Viatype::Buried => "buried",
        Viatype::Microvia => "micro",
        Viatype::NotDefined => "not_defined",
    }
}

fn transformed(it: &ItemRef<'_>, layer: PcbLayerId, max_error: i32) -> Polygons {
    let mut buf = ShapePolySet::new();
    it.transform_shape_to_polygon(&mut buf, layer, 0, max_error, ErrorLoc::Inside, false);
    polygons(&buf)
}

impl Session {
    /// The read context: board, connectivity and (when the load created one) the DRC engine.
    pub fn api_ctx(&self) -> BoardCtx<'_> {
        let ctx = BoardCtx::bare(&self.board).with_connectivity(&self.connectivity);

        match self.drc_engine.as_ref() {
            Some(engine) => ctx.with_rules(engine),
            None => ctx,
        }
    }

    /// The board: layers, outline, bounding boxes, rule minimums, stackup.
    pub fn board_info(&self) -> BoardInfo {
        let board = &self.board;
        let ctx = self.api_ctx();
        let bds = board.design_settings();
        let stackup = board.get_stackup_or_default();
        let count = board.copper_layer_count();

        let copper_layers = Lset::all_cu_mask_n(count)
            .cu_stack()
            .iter()
            .map(|&l| CopperLayer {
                name: layer_name(l),
                user_name: board.layer_name(l),
                layer_type: board.layer_type(l).show_type().to_string(),
                layer_id: l.as_i32(),
                depth: stackup.get_layer_distance(PcbLayerId::FCu, l),
            })
            .collect();

        let (outline, outline_ok) = board.get_board_polygon_outlines(&ctx, true, None, false, false);

        BoardInfo {
            name: self.source_name.clone(),
            copper_layer_count: count,
            copper_layers,
            enabled_layers: layer_names(&board.enabled_layers()),
            outline: polygons(&outline),
            outline_ok,
            bbox: bbox4(board.compute_bounding_box(&ctx, false, false)),
            edges_bbox: bbox4(board.board_edges_bounding_box(&ctx)),
            rules: RuleMinimums {
                min_clearance: bds.min_clearance,
                track_min_width: bds.track_min_width,
                min_connection: bds.min_conn,
                vias_min_annular_width: bds.vias_min_annular_width,
                vias_min_size: bds.vias_min_size,
                min_through_drill: bds.min_through_drill,
                micro_vias_min_size: bds.micro_vias_min_size,
                micro_vias_min_drill: bds.micro_vias_min_drill,
                copper_edge_clearance: bds.copper_edge_clearance,
                hole_clearance: bds.hole_clearance,
                hole_to_hole_min: bds.hole_to_hole_min,
                max_error: bds.max_error(),
            },
            board_thickness: bds.board_thickness,
            stackup: stackup
                .list()
                .iter()
                .map(|item| StackupLayer {
                    name: item.layer_name().to_string(),
                    type_name: item.type_name().to_string(),
                    layer: (item.brd_layer_id() != PcbLayerId::UndefinedLayer)
                        .then(|| layer_name(item.brd_layer_id())),
                    thickness: item.dielectric_prms_list.iter().map(|p| p.thickness).sum(),
                })
                .collect(),
        }
    }

    /// The nets in net code order with their effective net class and its constituents.
    pub fn net_details(&self) -> Vec<NetDetail> {
        self.board
            .nets_by_netcode()
            .map(|net| {
                let nc = net.netclass();
                NetDetail {
                    code: net.net_code().0,
                    name: net.netname().to_string(),
                    netclass: nc.map(|nc| nc.name.clone()).unwrap_or_default(),
                    netclasses: nc
                        .map(|nc| {
                            nc.constituent_netclasses()
                                .iter()
                                .map(|c| c.name.clone())
                                .collect()
                        })
                        .unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The net classes of the design settings: `Default` first, then the others in name
    /// order (`NET_SETTINGS::GetNetclasses()`, a `std::map`).
    pub fn netclasses(&self) -> Vec<NetclassInfo> {
        let ns = &self.board.design_settings().net_settings;
        std::iter::once(NetclassInfo::of(ns.default_netclass()))
            .chain(ns.netclasses().values().map(|nc| NetclassInfo::of(nc)))
            .collect()
    }

    /// The pads of every footprint (`Footprints()`, then `Pads()` order); with `polygons`
    /// the effective copper polygon per copper layer.
    pub fn pads(&self, with_polygons: bool) -> Vec<PadInfo> {
        let ctx = self.api_ctx();
        let mut out = Vec::new();

        for (fp_id, fp) in self.board.footprints() {
            let reference = fp.reference(&ctx);
            let fp_uuid = fp.base.uuid.as_string();

            for (pad_id, pad) in self.board.footprint_pads(fp_id) {
                let Some(it) = ctx.item(pad_id.into()) else {
                    continue;
                };
                let ps = pad.lib_padstack();
                let drill_size = pad.drill_size(&ctx);

                let shapes = ps
                    .unique_layers()
                    .into_iter()
                    .map(|l| PadShapeInfo {
                        layer: layer_name(l),
                        shape: enum_name(pad.shape(l)),
                        size: pt(pad.size(&ctx, l)),
                        offset: pt(pad.offset(&ctx, l)),
                        corner_radius: pad.round_rect_corner_radius(l),
                        chamfer_ratio: ps.chamfer_ratio(l),
                        chamfer_positions: ps.chamfer_positions(l),
                    })
                    .collect();

                let mut polys = Vec::new();

                if with_polygons {
                    for l in Lset::all_cu_mask_n(self.board.copper_layer_count())
                        .cu_stack()
                        .iter()
                        .copied()
                    {
                        if !pad.is_on_layer(&ctx, l) {
                            continue;
                        }

                        polys.push(PadPolygon {
                            layer: layer_name(l),
                            flashed: pad.flash_layer(&ctx, l),
                            polygon: polygons(&pad.effective_polygon(&ctx, l, ErrorLoc::Inside)),
                        });
                    }
                }

                out.push(PadInfo {
                    id: handle(pad_id),
                    uuid: pad.base.item.uuid.as_string(),
                    footprint: handle(fp_id),
                    footprint_uuid: fp_uuid.clone(),
                    reference: reference.clone(),
                    number: pad.number().to_string(),
                    net: pad.base.net.0,
                    position: pt(pad.position(&ctx)),
                    orientation: pad.orientation(&ctx).as_degrees(),
                    layers: layer_names(&it.layer_set()),
                    attribute: enum_name(pad.attribute()),
                    drill: (drill_size.x > 0 && drill_size.y > 0).then(|| DrillInfo {
                        shape: match ps.drill_shape() {
                            PadDrillShape::Oblong => "OBLONG".to_string(),
                            other => enum_name(other),
                        },
                        size: pt(drill_size),
                    }),
                    shapes,
                    polygons: polys,
                    teardrops: pad.base.teardrop_params.enabled,
                    locked: it.is_locked(),
                });
            }
        }

        out
    }

    /// The footprints in `Footprints()` order.
    pub fn footprints_info(&self) -> Vec<FootprintInfo> {
        let ctx = self.api_ctx();

        self.board
            .footprints()
            .filter_map(|(id, fp)| {
                let it = ctx.item(id.into())?;
                Some(FootprintInfo {
                    id: handle(id),
                    uuid: fp.base.uuid.as_string(),
                    reference: fp.reference(&ctx),
                    value: fp.value(&ctx),
                    position: pt(fp.position(&ctx)),
                    orientation: fp.orientation().as_degrees(),
                    side: layer_name(fp.side(&ctx)),
                    courtyard_front: polygons(&fp.courtyard(&ctx, PcbLayerId::FCrtYd)),
                    courtyard_back: polygons(&fp.courtyard(&ctx, PcbLayerId::BCrtYd)),
                    bbox: bbox4(it.bounding_box()),
                    locked: it.is_locked(),
                    dnp: fp.is_dnp(),
                    pads: self
                        .board
                        .footprint_pads(id)
                        .map(|(p, _)| handle(p))
                        .collect(),
                })
            })
            .collect()
    }

    /// The track segments, arcs and vias in `Tracks()` order.
    pub fn route_items(&self) -> Vec<RouteItem> {
        let ctx = self.api_ctx();

        self.board
            .tracks()
            .filter_map(|(id, track)| {
                let it = ctx.item(id.into())?;
                Some(route_item(&ctx, &it, track))
            })
            .collect()
    }

    /// One track / arc / via by handle.
    pub fn route_item(&self, id: u64) -> Option<RouteItem> {
        let ctx = self.api_ctx();
        let item = ItemId::from_bits(id)?;
        let track = self.board.track(item.typed()?)?;
        let it = ctx.item(item)?;
        Some(route_item(&ctx, &it, track))
    }

    /// Board zones (`Zones()`), then the zones of every footprint; with `polygons` the
    /// fills.
    pub fn zones_info(&self, with_polygons: bool) -> Vec<ZoneInfo> {
        let ctx = self.api_ctx();
        let mut ids: Vec<(kicad_pcb::ids::ZoneId, Option<u64>)> =
            self.board.zones().map(|(id, _)| (id, None)).collect();

        for (fp, _) in self.board.footprints() {
            ids.extend(
                self.board
                    .footprint_zones(fp)
                    .map(|(id, _)| (id, Some(handle(fp)))),
            );
        }

        ids.into_iter()
            .filter_map(|(id, footprint)| {
                let z = self.board.zone(id)?;
                let it = ctx.item(id.into())?;
                let layer_set = it.layer_set();
                let fills = if with_polygons {
                    layer_set
                        .seq()
                        .iter()
                        .copied()
                        .filter(|&l| z.has_filled_polys_for_layer(l))
                        .map(|l| ZoneFill {
                            layer: layer_name(l),
                            polygons: polygons(&z.filled_polys_list(l)),
                        })
                        .collect()
                } else {
                    Vec::new()
                };

                Some(ZoneInfo {
                    id: handle(id),
                    uuid: z.base.item.uuid.as_string(),
                    footprint,
                    name: z.zone_name().to_string(),
                    net: z.base.net.0,
                    layers: layer_names(&layer_set),
                    priority: z.assigned_priority(),
                    rule_area: z.is_rule_area(),
                    keepout: Keepouts {
                        tracks: z.do_not_allow_tracks,
                        vias: z.do_not_allow_vias,
                        pads: z.do_not_allow_pads,
                        copper_pour: z.do_not_allow_zone_fills,
                        footprints: z.do_not_allow_footprints,
                    },
                    teardrop: z.is_teardrop_area(),
                    locked: it.is_locked(),
                    clearance: z.local_clearance(),
                    min_thickness: z.min_thickness(),
                    connection: enum_name(z.pad_connection()),
                    island_mode: enum_name(z.island_removal_mode),
                    outline: polygons(&z.board_outline(&ctx)),
                    filled: z.is_filled(),
                    fills,
                })
            })
            .collect()
    }

    /// Graphic shapes and texts on copper layers (board drawings, then footprint graphics in
    /// `Footprints()` order), one entry per copper layer of the item.
    pub fn copper_obstacles(&self) -> Vec<ObstacleInfo> {
        self.drawing_polygons(|l| is_copper_layer(l.as_i32()))
    }

    /// The shapes on Edge.Cuts (board drawings, then footprint graphics).
    pub fn edge_shapes(&self) -> Vec<ObstacleInfo> {
        self.drawing_polygons(|l| l == PcbLayerId::EdgeCuts)
    }

    fn drawing_polygons(&self, wanted: impl Fn(PcbLayerId) -> bool) -> Vec<ObstacleInfo> {
        let ctx = self.api_ctx();
        let max_error = self.board.design_settings().max_error();
        let mut items: Vec<(ItemId, Option<u64>)> =
            self.board.drawings().map(|(id, _)| (id, None)).collect();

        for (fp, _) in self.board.footprints() {
            items.extend(
                self.board
                    .footprint_drawings(fp)
                    .map(|(id, _)| (id, Some(handle(fp)))),
            );
        }

        let mut out = Vec::new();

        for (id, footprint) in items {
            let Some(it) = ctx.item(id) else { continue };

            if !matches!(
                it.any(),
                AnyItem::Shape(_) | AnyItem::Text(_) | AnyItem::Textbox(_)
            ) {
                continue;
            }

            for l in it.layer_set().seq().iter().copied() {
                if !wanted(l) {
                    continue;
                }

                out.push(ObstacleInfo {
                    id: id.to_bits(),
                    uuid: it.uuid().as_string(),
                    kind: enum_name(it.type_()),
                    layer: layer_name(l),
                    net: it.net_code().map_or(0, |n| n.0.max(0)),
                    footprint,
                    polygon: transformed(&it, l, max_error),
                });
            }
        }

        out
    }

    /// `RecalculateRatsnest()`, then the unconnected edges and the per-net state.
    pub fn connectivity_info(&mut self) -> ConnectivityInfo {
        let ctx = BoardCtx::bare(&self.board);
        self.connectivity.recalculate_ratsnest(&ctx);

        let edges: Vec<RatsnestEdge> = self
            .connectivity
            .unconnected_edges()
            .into_iter()
            .map(|e| RatsnestEdge {
                net: e.net,
                source: e.source_item.to_bits(),
                target: e.target_item.to_bits(),
                source_pos: pt(e.source),
                target_pos: pt(e.target),
            })
            .collect();

        let cluster_ids = self.connectivity.algo_mut().get_clusters(&ctx).to_vec();
        let net_count = self.connectivity.net_count().max(0) as usize;
        let mut clusters: Vec<Vec<Vec<u64>>> = vec![Vec::new(); net_count.max(1)];

        {
            let algo = self.connectivity.algo();
            let list = algo.item_list();

            for c in cluster_ids {
                let cluster = algo.cluster(c);
                let net = cluster.origin_net();

                if net <= 0 || net as usize >= clusters.len() {
                    continue;
                }

                let items: BTreeSet<u64> = cluster
                    .items()
                    .iter()
                    .map(|&i| list.item(i).parent().to_bits())
                    .collect();
                clusters[net as usize].push(items.into_iter().collect());
            }
        }

        let nets = (1..net_count)
            .map(|net| NetConnection {
                net: net as i32,
                pads: self.connectivity.node_count_of(net as i32),
                unconnected: edges.iter().filter(|e| e.net == net as i32).count() as u32,
                clusters: std::mem::take(&mut clusters[net]),
            })
            .collect();

        ConnectivityInfo {
            unconnected: self.connectivity.unconnected_count(false),
            edges,
            nets,
        }
    }
}

fn route_item(ctx: &BoardCtx<'_>, it: &ItemRef<'_>, track: &Track) -> RouteItem {
    let t = track.as_track();
    let mut item = RouteItem {
        id: it.id().map_or(0, |id| id.to_bits()),
        uuid: t.base.item.uuid.as_string(),
        kind: "track".to_string(),
        net: t.base.net.0,
        start: pt(t.start),
        end: pt(t.end),
        mid: None,
        width: t.width,
        layer: layer_name(t.base.item.layer),
        locked: it.is_locked(),
        solder_mask: false,
        solder_mask_margin: None,
        via: None,
    };

    if !matches!(track, Track::Via(_)) {
        item.solder_mask = t.has_solder_mask;
        item.solder_mask_margin = t.solder_mask_margin;
    }

    match track {
        Track::Trace(_) => {}
        Track::Arc(arc) => {
            item.kind = "arc".to_string();
            item.mid = Some(pt(arc.mid));
        }
        Track::Via(via) => {
            item.kind = "via".to_string();
            item.width = via.width(PcbLayerId::FCu);
            item.layer = layer_name(via.top_layer());
            let span = Lset::all_cu_mask_n(ctx.copper_layer_count())
                .cu_stack()
                .iter()
                .copied()
                .filter(|&l| it.is_on_layer(l))
                .map(|l| (layer_name(l), via.width(l)))
                .collect();
            item.via = Some(ViaInfo {
                via_type: via_type_name(via.via_type()).to_string(),
                top: layer_name(via.top_layer()),
                bottom: layer_name(via.bottom_layer()),
                diameters: span,
                drill: via.drill_value(ctx),
                free: via.is_free(),
                finish: ViaFinish::of(via),
                unconnected_layers: enum_name(via.padstack.unconnected_layer_mode()),
                zone_layer_connections: Lset::all_cu_mask_n(ctx.copper_layer_count())
                    .cu_stack()
                    .iter()
                    .copied()
                    .filter(|&l| {
                        via.zone_layer_override(l)
                            == kicad_pcb::pcb_track::ZoneLayerOverride::ForceFlashed
                    })
                    .map(layer_name)
                    .collect(),
                teardrops: super::edit::TeardropParams::of(&via.track.base.teardrop_params),
            });
        }
    }

    item
}
