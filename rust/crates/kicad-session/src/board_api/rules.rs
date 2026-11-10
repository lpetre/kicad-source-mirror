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

//! Resolved design rules for a router: `DRC_ENGINE::EvalRules` on board items or on the
//! dummy items the KiCad router builds for a bare net (`PNS_PCBNEW_RULE_RESOLVER::
//! getBoardItem`: a `PCB_TRACK` / `PCB_VIA` with the net and layer set), and the router's
//! default track / via sizes (`PNS_KICAD_IFACE_BASE::ImportSizes`, the net class branches,
//! and `ROUTER_TOOL`'s micro via sizing through `PCB_VIA::SetSizeFromRules`).

use kicad_core::PcbLayerId;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::{DrcConstraint, DrcConstraintT};
use kicad_pcb::item_ref::{AnyItem, ItemRef};
use kicad_pcb::padstack::Padstack;
use kicad_pcb::pcb_track::{PcbTrack, PcbVia, Track, Viatype};
use kicad_pcb::{ItemId, NetCode};
use serde::{Deserialize, Serialize};

use super::{enum_name, item_id, parse_layer};
use crate::error::SessionError;
use crate::session::Session;

/// One side of a constraint query: a board item (`id`) or a bare net (`net`, evaluated on a
/// dummy item of `dummy` kind: `track` (default) or `via`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ItemSel {
    /// The item handle.
    pub id: Option<u64>,
    /// The net code (when `id` is absent).
    pub net: Option<i32>,
    /// `track` or `via`: the kind of dummy item for a net.
    pub dummy: Option<String>,
}

/// A constraint query.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ConstraintQuery {
    /// The constraint: a `DRC_CONSTRAINT_T` name without `_CONSTRAINT`, lower case
    /// ([`constraint_kind`]): `clearance`, `hole_clearance`, `hole_to_hole`,
    /// `edge_clearance`, `track_width`, `via_diameter`, `hole_size`, `annular_width`,
    /// `physical_clearance`, `physical_hole_clearance`, `diff_pair_gap`, `disallow`, ...
    pub kind: String,
    /// Item A.
    pub a: ItemSel,
    /// Item B (binary constraints such as clearance).
    pub b: Option<ItemSel>,
    /// The layer (empty: `UNDEFINED_LAYER`).
    pub layer: String,
}

/// A resolved constraint (`DRC_CONSTRAINT`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConstraintResult {
    /// `m_Type` as a `DRC_CONSTRAINT_T` name (`NULL_CONSTRAINT` when no rule applies).
    pub constraint_type: String,
    /// `m_Value.Min()` if set.
    pub min: Option<i32>,
    /// `m_Value.Opt()` if set.
    pub opt: Option<i32>,
    /// `m_Value.Max()` if set.
    pub max: Option<i32>,
    /// `GetName()` (the rule name, `netclass 'Default'`, ...).
    pub name: String,
    /// `GetRuleName( GetMinRule() )` when the minimum came from another rule.
    pub min_rule: Option<String>,
    /// `m_DisallowFlags`.
    pub disallow: i32,
    /// `m_ImplicitMin`.
    pub implicit_min: bool,
    /// `GetSeverity()` (`RPT_SEVERITY_*` name).
    pub severity: String,
    /// `GetSeverity()` as its `SEVERITY` value.
    pub severity_code: i32,
}

impl ConstraintResult {
    /// The result of a `DRC_CONSTRAINT`.
    pub fn of(c: &DrcConstraint) -> ConstraintResult {
        let v = &c.value;
        ConstraintResult {
            constraint_type: enum_name(c.type_),
            min: v.has_min().then(|| v.min()),
            opt: v.has_opt().then(|| v.opt()),
            max: v.has_max().then(|| v.max()),
            name: c.name(),
            min_rule: c.min_rule.as_ref().map(|r| c.rule_name(Some(r))),
            disallow: c.disallow_flags,
            implicit_min: c.implicit_min,
            severity: format!("RPT_SEVERITY_{}", enum_name(c.severity())),
            severity_code: c.severity().as_i32(),
        }
    }
}

/// The router's default sizes for a net on a layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouterSizes {
    /// `SIZES_SETTINGS::Clearance()`.
    pub clearance: i32,
    /// `TrackWidth()`.
    pub track_width: i32,
    /// `ViaDiameter()`.
    pub via_diameter: i32,
    /// `ViaDrill()`.
    pub via_drill: i32,
    /// The micro via diameter `ROUTER_TOOL` places (`SetSizeFromRules` on a micro via
    /// F.Cu - In1.Cu, or the board's two outer layers on a two layer board).
    pub microvia_diameter: i32,
    /// The micro via drill.
    pub microvia_drill: i32,
}

/// Every `DRC_CONSTRAINT_T` in declaration order (the index is the C++ value).
pub const ALL_CONSTRAINT_TYPES: [DrcConstraintT; 42] = {
    use DrcConstraintT::*;
    [
        NullConstraint,
        ClearanceConstraint,
        CreepageConstraint,
        HoleClearanceConstraint,
        HoleToHoleConstraint,
        EdgeClearanceConstraint,
        HoleSizeConstraint,
        CourtyardClearanceConstraint,
        SilkClearanceConstraint,
        TextHeightConstraint,
        TextThicknessConstraint,
        TrackWidthConstraint,
        TrackSegmentLengthConstraint,
        AnnularWidthConstraint,
        ZoneConnectionConstraint,
        ThermalReliefGapConstraint,
        ThermalSpokeWidthConstraint,
        MinResolvedSpokesConstraint,
        SolderMaskExpansionConstraint,
        SolderPasteAbsMarginConstraint,
        SolderPasteRelMarginConstraint,
        DisallowConstraint,
        ViaDiameterConstraint,
        LengthConstraint,
        NetChainLengthConstraint,
        NetChainStubLengthConstraint,
        NetChainReturnPathConstraint,
        SkewConstraint,
        DiffPairGapConstraint,
        MaxUncoupledConstraint,
        DiffPairIntraSkewConstraint,
        ViaCountConstraint,
        PhysicalClearanceConstraint,
        PhysicalHoleClearanceConstraint,
        AssertionConstraint,
        ConnectionWidthConstraint,
        TrackAngleConstraint,
        ViaDanglingConstraint,
        BridgedMaskConstraint,
        SolderMaskSliverConstraint,
        MicroviaStackDepthConstraint,
        HoleAspectRatioConstraint,
    ]
};

/// The query name of a constraint type: the `DRC_CONSTRAINT_T` name without `_CONSTRAINT`,
/// lower case (`TRACK_WIDTH_CONSTRAINT` -> `track_width`).
pub fn constraint_kind(t: DrcConstraintT) -> String {
    enum_name(t)
        .trim_end_matches("_CONSTRAINT")
        .to_ascii_lowercase()
}

/// The constraint type of a [`ConstraintQuery::kind`].
pub fn constraint_type(kind: &str) -> Result<DrcConstraintT, SessionError> {
    let wanted = format!("{}_CONSTRAINT", kind.to_ascii_uppercase());

    ALL_CONSTRAINT_TYPES
        .iter()
        .copied()
        .find(|t| enum_name(*t) == wanted)
        .ok_or_else(|| SessionError::invalid_argument(format!("unknown constraint '{kind}'")))
}

/// A dummy item for a bare net (`m_dummyTracks` / `m_dummyVias` of the router's rule
/// resolver).
enum Dummy {
    Track(Track),
}

fn dummy_for(
    session: &Session,
    net: i32,
    kind: Option<&str>,
    layer: PcbLayerId,
) -> Result<Dummy, SessionError> {
    if session.board.find_net(NetCode(net)).is_none() {
        return Err(SessionError::invalid_argument(format!(
            "no net with code {net}"
        )));
    }

    // The dummies never reach the board: a scratch generator keeps the board's UUID sequence
    // (and with it the UUIDs of items added later) independent of the queries.
    let mut scratch = kicad_core::KiidGenerator::default();
    scratch.create_nil_uuids(true);

    match kind.unwrap_or("track") {
        "track" => {
            let mut t = PcbTrack::new(&mut scratch);
            t.base.item.layer = layer;
            t.base.net = NetCode(net);
            Ok(Dummy::Track(Track::Trace(t)))
        }
        "via" => {
            let mut v = PcbVia::new(&mut scratch);
            v.track.base.net = NetCode(net);
            Ok(Dummy::Track(Track::Via(Box::new(v))))
        }
        other => Err(SessionError::invalid_argument(format!(
            "unknown dummy item kind '{other}'"
        ))),
    }
}

impl Session {
    /// `DRC_ENGINE::EvalRules( type, a, b, layer )` for a query. Needs the DRC engine of the
    /// load (`NotYetPorted` for a [`Session::new_empty`] board without one).
    pub fn constraint(&mut self, q: &ConstraintQuery) -> Result<ConstraintResult, SessionError> {
        let t = constraint_type(&q.kind)?;
        let layer = if q.layer.is_empty() {
            PcbLayerId::UndefinedLayer
        } else {
            parse_layer(&q.layer)?
        };

        let mut dummies: Vec<Option<Dummy>> = Vec::new();
        let mut ids: Vec<Option<ItemId>> = Vec::new();

        for sel in std::iter::once(Some(&q.a)).chain(std::iter::once(q.b.as_ref())) {
            match sel {
                None => {
                    ids.push(None);
                    dummies.push(None);
                }
                Some(ItemSel { id: Some(id), .. }) => {
                    let item = item_id(*id)?;

                    if !self.board.contains(item) {
                        return Err(SessionError::StaleId { id: *id });
                    }

                    ids.push(Some(item));
                    dummies.push(None);
                }
                Some(ItemSel {
                    net: Some(net),
                    dummy,
                    ..
                }) => {
                    ids.push(None);
                    dummies.push(Some(dummy_for(self, *net, dummy.as_deref(), layer)?));
                }
                Some(_) => {
                    return Err(SessionError::invalid_argument(
                        "an item selector needs an id or a net",
                    ));
                }
            }
        }

        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported(
                "DRC engine (a board created with newBoard has none)",
            ));
        };
        let ctx = BoardCtx::bare(&self.board)
            .with_connectivity(&self.connectivity)
            .with_rules(engine);

        let resolve = |i: usize| -> Option<ItemRef<'_>> {
            match (&ids[i], &dummies[i]) {
                (Some(id), _) => ctx.item(*id),
                (None, Some(Dummy::Track(t))) => Some(ctx.view(AnyItem::Track(t))),
                (None, None) => None,
            }
        };

        let c = engine.eval_rules(&ctx, t, resolve(0), resolve(1), layer);
        Ok(ConstraintResult::of(&c))
    }

    /// The sizes the KiCad router starts a route of `net` on `layer` with: port of the net
    /// class branches of `PNS_KICAD_IFACE_BASE::ImportSizes` (start item = a segment of the
    /// net; `m_UseConnectedTrackWidth` has no start track here) and of `ROUTER_TOOL`'s micro
    /// via sizing (`PCB_VIA::SetSizeFromRules`).
    pub fn router_sizes(&mut self, net: i32, layer: &str) -> Result<RouterSizes, SessionError> {
        let layer_id = parse_layer(layer)?;
        let Dummy::Track(track) = dummy_for(self, net, Some("track"), layer_id)?;
        let Dummy::Track(via) = dummy_for(self, net, Some("via"), layer_id)?;
        let Some(engine) = self.drc_engine.as_ref() else {
            return Err(SessionError::not_yet_ported(
                "DRC engine (a board created with newBoard has none)",
            ));
        };
        let board = &self.board;
        let bds = board.design_settings();
        let ctx = BoardCtx::bare(board)
            .with_connectivity(&self.connectivity)
            .with_rules(engine);
        let track_ref = ctx.view(AnyItem::Track(&track));
        let via_ref = ctx.view(AnyItem::Track(&via));
        // QueryConstraint returns false for a null constraint.
        let query = |t: DrcConstraintT, item: ItemRef<'_>| {
            let c = engine.eval_rules(&ctx, t, Some(item), None, layer_id);
            (c.type_ != DrcConstraintT::NullConstraint).then_some(c)
        };

        let mut clearance = bds.min_clearance;

        if let Some(c) = query(DrcConstraintT::ClearanceConstraint, track_ref) {
            if c.value.min() >= bds.min_clearance {
                clearance = c.value.min();
            }
        }

        let mut track_width = bds.track_min_width;
        let mut found = false;

        if bds.use_net_class_track() {
            if let Some(c) = query(DrcConstraintT::TrackWidthConstraint, track_ref) {
                track_width = track_width.max(c.value.pinned_opt());
                found = true;
            }
        }

        if !found {
            track_width = track_width.max(bds.get_current_track_width());
        }

        let mut via_diameter = bds.vias_min_size;
        let mut via_drill = bds.min_through_drill;

        if bds.use_net_class_via() {
            if let Some(c) = query(DrcConstraintT::ViaDiameterConstraint, via_ref) {
                via_diameter = via_diameter.max(c.value.pinned_opt());
            }

            if let Some(c) = query(DrcConstraintT::HoleSizeConstraint, via_ref) {
                via_drill = via_drill.max(c.value.pinned_opt());
            }
        } else {
            via_diameter = bds.get_current_via_size();
            via_drill = bds.get_current_via_drill();
        }

        // ROUTER_TOOL, micro via with preset.m_UseNetclass.
        let nc = board.find_net(NetCode(net)).and_then(|n| n.netclass());
        let u_size = nc
            .filter(|nc| nc.has_uvia_diameter())
            .map_or_else(|| bds.get_current_via_size(), |nc| nc.uvia_diameter());
        let u_drill = nc
            .filter(|nc| nc.has_uvia_drill())
            .map_or_else(|| bds.get_current_via_drill(), |nc| nc.uvia_drill());
        let mut scratch = kicad_core::KiidGenerator::default();
        scratch.create_nil_uuids(true);
        let mut micro = PcbVia::new(&mut scratch);
        let env = board.child_env(kicad_pcb::ParentRef::Board);
        micro.set_via_type(&env, Viatype::Microvia);
        let second = if board.copper_layer_count() > 2 {
            PcbLayerId::In1Cu
        } else {
            PcbLayerId::BCu
        };
        micro.set_layer_pair(&env, PcbLayerId::FCu, second);
        micro.track.base.net = NetCode(net);
        micro.set_size_from_rules(&ctx, u_size, u_drill);

        Ok(RouterSizes {
            clearance,
            track_width,
            via_diameter,
            via_drill,
            microvia_diameter: micro.width(Padstack::ALL_LAYERS),
            microvia_drill: micro.drill_value(&ctx),
        })
    }
}
