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

//! Port of `pcbnew/drill/drill_operation.h`: `DRILL_OP_KIND`, `DRILL_OPERATION_ID`,
//! `DRILL_POST_MACHINING`, `DRILL_OPERATION`, `DRILL_QUERY`.

use kicad_core::Kiid;
use kicad_core::layer_ids::PcbLayerId;
use kicad_geom::{ANGLE_0, EdaAngle, Vector2I};
use kicad_pcb::ids::{PadId, TrackId};
use kicad_pcb::padstack::PadDrillPostMachiningMode;

use super::drill_span::{DrillSpan, HoleAttribute};

/// `DRILL_OP_KIND`. Declaration order is the sort order of `DRILL_OPERATION_ID`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum DrillOpKind {
    #[default]
    PrimaryDrill,
    SecondaryDrill,
    TertiaryDrill,
}

/// `DRILL_OPERATION_ID`: owner UUID and kind. The derived `Ord` (owner, then kind) is the C++
/// `operator<` (`m_Owner == other.m_Owner ? m_Kind < .. : m_Owner < ..`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct DrillOperationId {
    pub owner: Kiid,       // m_Owner
    pub kind: DrillOpKind, // m_Kind
}

/// `DRILL_POST_MACHINING`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrillPostMachining {
    pub mode: PadDrillPostMachiningMode, // m_Mode
    pub size: i32,                       // m_Size
    pub depth: i32,                      // m_Depth
    /// Countersink angle in tenths of a degree (`m_Angle`).
    pub angle: i32,
}

impl Default for DrillPostMachining {
    /// The in-class initialisers: mode `UNKNOWN`, zero size, depth and angle.
    fn default() -> Self {
        DrillPostMachining {
            mode: PadDrillPostMachiningMode::Unknown,
            size: 0,
            depth: 0,
            angle: 0,
        }
    }
}

impl DrillPostMachining {
    /// `DRILL_POST_MACHINING::IsSet()`.
    pub fn is_set(&self) -> bool {
        self.mode != PadDrillPostMachiningMode::Unknown
    }
}

/// `DRILL_OPERATION::m_SourceItem` (a non-owning `BOARD_ITEM*` that the writers test with
/// `dyn_cast<PCB_VIA*>`): the handle of the via or pad the hole belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DrillSource {
    Via(TrackId),
    Pad(PadId),
}

/// `DRILL_OPERATION`: one hole.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillOperation {
    pub kind: DrillOpKind, // m_Kind

    /// `m_SourceItem`; `None` is the C++ `nullptr` default (hand-built lists).
    pub source_item: Option<DrillSource>,
    pub source_id: Kiid, // m_SourceId

    pub position: Vector2I, // m_Position
    /// Source axes, never normalized to (width, length). The Excellon writer picks slot
    /// endpoints from whichever of X and Y is longer.
    pub size_xy: Vector2I,
    pub orientation: EdaAngle, // m_Orientation
    pub diameter: i32,         // m_Diameter
    pub is_slot: bool,         // m_IsSlot

    pub top_layer: PcbLayerId,    // m_TopLayer
    pub bottom_layer: PcbLayerId, // m_BottomLayer
    pub drill_start: PcbLayerId,  // m_DrillStart
    pub drill_end: PcbLayerId,    // m_DrillEnd

    pub not_plated: bool,         // m_NotPlated
    pub attribute: HoleAttribute, // m_Attribute

    pub filled: bool,         // m_Filled
    pub capped: bool,         // m_Capped
    pub top_covered: bool,    // m_TopCovered
    pub bottom_covered: bool, // m_BottomCovered
    pub top_plugged: bool,    // m_TopPlugged
    pub bottom_plugged: bool, // m_BottomPlugged
    pub top_tented: bool,     // m_TopTented
    pub bottom_tented: bool,  // m_BottomTented

    pub front_post_machining: DrillPostMachining, // m_FrontPostMachining
    pub back_post_machining: DrillPostMachining,  // m_BackPostMachining

    pub stub_length: Option<i32>, // m_StubLength
}

impl Default for DrillOperation {
    /// The in-class initialisers of `DRILL_OPERATION`.
    fn default() -> Self {
        DrillOperation {
            kind: DrillOpKind::PrimaryDrill,
            source_item: None,
            source_id: Kiid::default(),
            position: Vector2I::new(0, 0),
            size_xy: Vector2I::new(0, 0),
            orientation: ANGLE_0,
            diameter: 0,
            is_slot: false,
            top_layer: PcbLayerId::FCu,
            bottom_layer: PcbLayerId::BCu,
            drill_start: PcbLayerId::UndefinedLayer,
            drill_end: PcbLayerId::UndefinedLayer,
            not_plated: false,
            attribute: HoleAttribute::HoleUnknown,
            filled: false,
            capped: false,
            top_covered: false,
            bottom_covered: false,
            top_plugged: false,
            bottom_plugged: false,
            top_tented: false,
            bottom_tented: false,
            front_post_machining: DrillPostMachining::default(),
            back_post_machining: DrillPostMachining::default(),
            stub_length: None,
        }
    }
}

impl DrillOperation {
    /// `DRILL_OPERATION::Id()`.
    pub fn id(&self) -> DrillOperationId {
        DrillOperationId {
            owner: self.source_id,
            kind: self.kind,
        }
    }

    /// `DRILL_OPERATION::IsBackdrill()`.
    pub fn is_backdrill(&self) -> bool {
        self.kind != DrillOpKind::PrimaryDrill
    }
}

/// `DRILL_QUERY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DrillQuery {
    pub span: DrillSpan, // m_Span

    /// Emit only non-plated holes. Vias are suppressed outright. For pads the flag is
    /// ignored when `merge_pth_npth` is set.
    pub non_plated_only: bool,

    /// Emit plated and non-plated together, as the merged drill file does.
    pub merge_pth_npth: bool,

    /// Include pad backdrills and post-machining, kept off for NC drill output.
    pub pad_machining: bool,
}
