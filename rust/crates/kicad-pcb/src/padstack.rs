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

//! Port of `pcbnew/padstack.h` and `pcbnew/padstack.cpp` (`PADSTACK` and the pad enums
//! declared there), `board-model-types.md` section 2.
//!
//! Data and constructor defaults (M0b), mode logic, compare, flip, knockouts, backdrill and
//! primitives (M13). Per-layer access always goes through `copper_props(layer)` /
//! `copper_props_mut(layer)`; nothing outside this file indexes `copper_props` directly
//! (`board-model.md` 12).
//!
//! `m_copperProps` is a `std::unordered_map<PCB_LAYER_ID, COPPER_LAYER_PROPS>` whose iteration
//! order is observable (CUSTOM-mode `ForEachUniqueLayer`, hence `Compare`, `UniqueLayers`,
//! `PAD_UTILS::CheckPad` report order, and the `begin()` fallbacks). [`CopperPropsMap`]
//! reproduces the libstdc++ (g++ 13) node order exactly, which is the reference platform
//! (`PORTING.md` 9).

use std::ops::Index;

use kicad_core::layer_ids::{is_back_layer, is_front_layer, is_non_copper_layer};
use kicad_core::layer_range::LayerRange;
use kicad_core::{Lset, PcbLayerId};
use kicad_geom::cmath;
use kicad_geom::{
    ANGLE_0, ANGLE_45, ANGLE_90, EdaAngle, EdaOrientation, Vector2D, Vector2I, ki_round,
};

use crate::board_stackup::BoardStackup;
use crate::ctx::{BoardCtx, LayerEnv};
use crate::item::ParentRef;
use crate::pcb_shape::PcbShape;
use crate::pcb_track::UNDEFINED_DRILL_DIAMETER;
use crate::zone_settings::ZoneConnection;

/// `PAD_SHAPE`: the set of pad shapes, used with `PAD::{Set,Get}Shape()`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadShape {
    #[default]
    Circle,
    Rectangle,
    Oval,
    Trapezoid,
    Roundrect,
    ChamferedRect,
    /// A shape defined by the user, using a set of basic shapes.
    Custom,
}

/// `PAD_DRILL_SHAPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadDrillShape {
    #[default]
    Undefined,
    Circle,
    Oblong,
}

/// `PAD_DRILL_POST_MACHINING_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum PadDrillPostMachiningMode {
    Unknown,
    NotPostMachined,
    Counterbore,
    Countersink,
}

/// `BACKDRILL_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum BackdrillMode {
    #[default]
    NoBackdrill,
    BackdrillBottom,
    BackdrillTop,
    BackdrillBoth,
}

/// `PAD_ATTRIB`: the set of pad types, used with `PAD::{Set,Get}Attribute()`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadAttrib {
    /// Plated through hole pad.
    #[default]
    Pth,
    /// SMD pad, appears on the solder paste layer.
    Smd,
    /// Like SMD, does not appear on the solder paste layer.
    Conn,
    /// Like PTH, but not plated (mechanical use only).
    Npth,
    /// Pad with no hole that does not connect to a physical component.
    NonComponent,
}

/// `PAD_PROP`: special fabrication properties.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadProp {
    #[default]
    None,
    Bga,
    FiducialGlbl,
    FiducialLocal,
    Testpoint,
    Heatsink,
    Castellated,
    Mechanical,
    Pressfit,
}

/// `UNCONNECTED_LAYER_MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum UnconnectedLayerMode {
    #[default]
    KeepAll,
    StartEndOnly,
    RemoveAll,
    RemoveExceptStartAndEnd,
}

/// `CUSTOM_SHAPE_ZONE_MODE`: how a custom pad is knocked out of a zone.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum CustomShapeZoneMode {
    #[default]
    Outline,
    Convexhull,
}

/// `PAD_SIM_ELECTRICAL_TYPE` (`pad.h`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadSimElectricalType {
    #[default]
    None = 0,
    Source,
    Sink,
}

/// `PADSTACK::TYPE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(i32)]
pub enum PadstackType {
    /// Padstack for a footprint pad.
    Normal,
    /// Padstack for a via.
    Via,
    /// A mounting hole (plated or unplated, not associated with a footprint).
    Mounting,
}

/// `PADSTACK::MODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
#[repr(i32)]
pub enum PadstackMode {
    /// Shape is the same on all layers.
    #[default]
    Normal,
    /// Up to three shapes can be defined (F_Cu, inner copper layers, B_Cu).
    FrontInnerBack,
    /// Shapes can be defined on arbitrary layers.
    Custom,
}

/// `PADSTACK::SHAPE_PROPS`. Equality = the C++ `operator==` (all members).
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeProps {
    /// Shape of the pad.
    pub shape: PadShape,
    /// Shape of the anchor when `shape == PadShape::Custom`.
    pub anchor_shape: PadShape,
    /// Size of the shape, or of the anchor pad for custom shape pads; library frame when the
    /// owner is a footprint pad.
    pub size: Vector2I,
    /// Offset of the shape center from the pad center.
    pub offset: Vector2I,
    pub round_rect_radius_ratio: f64,
    /// Size of the chamfer: ratio of the smallest of X, Y size.
    pub chamfered_rect_ratio: f64,
    /// `RECT_CHAMFER_POSITIONS` bits.
    pub chamfered_rect_positions: i32,
    pub trapezoid_delta_size: Vector2I,
}

impl Default for ShapeProps {
    /// `SHAPE_PROPS::SHAPE_PROPS()`: circle with a rectangle anchor, zero size.
    fn default() -> Self {
        ShapeProps {
            shape: PadShape::Circle,
            anchor_shape: PadShape::Rectangle,
            size: Vector2I::new(0, 0),
            offset: Vector2I::new(0, 0),
            round_rect_radius_ratio: 0.0,
            chamfered_rect_ratio: 0.0,
            chamfered_rect_positions: 0,
            trapezoid_delta_size: Vector2I::new(0, 0),
        }
    }
}

/// `PADSTACK::COPPER_LAYER_PROPS`. Equality is `eq_props` (the C++ `operator==` compares only
/// the number of custom shapes), not a derive.
#[derive(Clone, Debug, Default)]
pub struct CopperLayerProps {
    pub shape: ShapeProps,
    pub zone_connection: Option<ZoneConnection>,
    pub thermal_spoke_width: Option<i32>,
    pub thermal_spoke_angle: Option<EdaOrientation>,
    pub thermal_gap: Option<i32>,
    pub clearance: Option<i32>,
    /// custom_shapes (`std::vector<std::shared_ptr<PCB_SHAPE>>`): inline values in the pad
    /// frame, `parent == ParentRef::Item(pad)`. Not arena items, not in the UUID index.
    pub custom_shapes: Vec<PcbShape>,
}

/// `PADSTACK::MASK_LAYER_PROPS`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MaskLayerProps {
    pub solder_mask_margin: Option<i32>,
    pub solder_paste_margin: Option<i32>,
    pub solder_paste_margin_ratio: Option<f64>,
    /// True if this outer layer has mask (is not tented).
    pub has_solder_mask: Option<bool>,
    /// True if this outer layer has solder paste.
    pub has_solder_paste: Option<bool>,
    /// True if the pad on this side should have covering.
    pub has_covering: Option<bool>,
    /// True if the drill hole should be plugged on this side.
    pub has_plugging: Option<bool>,
}

/// `PADSTACK::DRILL_PROPS`.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillProps {
    /// Drill diameter (x == y) or slot dimensions (x != y).
    pub size: Vector2I,
    pub shape: PadDrillShape,
    pub start: PcbLayerId,
    pub end: PcbLayerId,
    /// True if the drill hole should be filled completely.
    pub is_filled: Option<bool>,
    /// True if the drill hole should be capped.
    pub is_capped: Option<bool>,
}

impl Default for DrillProps {
    /// The in-class initialisers: undefined shape, `UNDEFINED_LAYER` start and end.
    fn default() -> Self {
        DrillProps {
            size: Vector2I::new(0, 0),
            shape: PadDrillShape::Undefined,
            start: PcbLayerId::UndefinedLayer,
            end: PcbLayerId::UndefinedLayer,
            is_filled: None,
            is_capped: None,
        }
    }
}

/// `PADSTACK::POST_MACHINING_PROPS`.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PostMachiningProps {
    pub mode: Option<PadDrillPostMachiningMode>,
    pub size: i32,
    pub depth: i32,
    /// Countersink angle in tenths of a degree.
    pub angle: i32,
}

/// `PADSTACK`. `m_parent` is dropped: the owning pad / via passes itself or the context to
/// the few methods that used it (`FlipLayers`, `MachiningKnockout`, `AppendPrimitives`).
/// Equality is the ported `operator==` (`eq_padstack`) / `Compare`, not a derive.
#[derive(Clone, Debug)]
pub struct Padstack {
    pub mode: PadstackMode, // m_mode
    pub layer_set: Lset,    // m_layerSet
    /// m_customName (`std::unique_ptr<wxString>`; never holds an empty string).
    pub custom_name: Option<String>,
    pub orientation: EdaOrientation, // m_orientation
    /// m_copperProps (`std::unordered_map`, libstdc++ iteration order). Keys: F_Cu
    /// (`ALL_LAYERS`), In1_Cu (`INNER_LAYERS`), B_Cu, or any copper layer in CUSTOM mode.
    pub copper_props: CopperPropsMap,
    pub front_mask_props: MaskLayerProps, // m_frontMaskProps
    pub back_mask_props: MaskLayerProps,  // m_backMaskProps
    pub unconnected_layer_mode: UnconnectedLayerMode, // m_unconnectedLayerMode
    pub custom_shape_in_zone_mode: CustomShapeZoneMode, // m_customShapeInZoneMode
    pub drill: DrillProps,                // m_drill
    pub secondary_drill: DrillProps,      // m_secondaryDrill
    pub tertiary_drill: DrillProps,       // m_tertiaryDrill
    pub front_post_machining: PostMachiningProps, // m_frontPostMachining
    pub back_post_machining: PostMachiningProps, // m_backPostMachining
}

impl Default for Padstack {
    /// `PADSTACK( aParent )`: NORMAL mode, one ALL_LAYERS entry (default shape, inherited zone
    /// connection, 45 degree spoke angle), circular drill from F_Cu to B_Cu, secondary and
    /// tertiary drills undefined.
    fn default() -> Self {
        // m_copperProps[ALL_LAYERS].shape = SHAPE_PROPS(); ... (the first operator[] creates
        // the entry with default props).
        let mut copper_props = CopperPropsMap::new();
        *copper_props.index_or_default(Padstack::ALL_LAYERS) = CopperLayerProps {
            shape: ShapeProps::default(),
            zone_connection: Some(ZoneConnection::Inherited),
            thermal_spoke_width: None,
            thermal_spoke_angle: Some(EdaOrientation::new(ANGLE_45)),
            thermal_gap: None,
            clearance: None,
            custom_shapes: Vec::new(),
        };

        Padstack {
            mode: PadstackMode::Normal,
            layer_set: Lset::new(),
            custom_name: None,
            orientation: EdaOrientation::new(ANGLE_0),
            copper_props,
            front_mask_props: MaskLayerProps::default(),
            back_mask_props: MaskLayerProps::default(),
            unconnected_layer_mode: UnconnectedLayerMode::KeepAll,
            custom_shape_in_zone_mode: CustomShapeZoneMode::Outline,
            drill: DrillProps {
                shape: PadDrillShape::Circle,
                start: PcbLayerId::FCu,
                end: PcbLayerId::BCu,
                ..DrillProps::default()
            },
            secondary_drill: DrillProps::default(),
            tertiary_drill: DrillProps::default(),
            front_post_machining: PostMachiningProps::default(),
            back_post_machining: PostMachiningProps::default(),
        }
    }
}

impl Padstack {
    /// `PADSTACK::TEMP_ALL_LAYERS`.
    pub const TEMP_ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;
    /// `PADSTACK::ALL_LAYERS`: the key of the shape used on every layer in NORMAL mode.
    pub const ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;
    /// `PADSTACK::INNER_LAYERS`: the key of the inner-layer shape in FRONT_INNER_BACK mode.
    pub const INNER_LAYERS: PcbLayerId = PcbLayerId::In1Cu;

    /// The padstack of a new `PCB_VIA`: `PADSTACK( this )` followed by the padstack
    /// statements of the `PCB_VIA` constructor (`pcb_track.cpp`): drill start F_Cu, end B_Cu,
    /// `SetDrillDefault()` (undefined drill diameter), `UNCONNECTED_LAYER_MODE::KEEP_ALL`,
    /// layer set reset ("padstack layerset is not used for vias right now"), circle shape on
    /// ALL_LAYERS ("for now, vias are always circles").
    pub fn for_new_via() -> Padstack {
        let mut padstack = Padstack::default();
        padstack.drill.start = PcbLayerId::FCu;
        padstack.drill.end = PcbLayerId::BCu;
        padstack.drill.size = Vector2I::new(UNDEFINED_DRILL_DIAMETER, UNDEFINED_DRILL_DIAMETER);
        padstack.unconnected_layer_mode = UnconnectedLayerMode::KeepAll;
        padstack.layer_set = Lset::new();
        // SetShape( CIRCLE, ALL_LAYERS ) on a NORMAL padstack writes m_copperProps[ALL_LAYERS].
        padstack
            .copper_props
            .index_or_default(Padstack::ALL_LAYERS)
            .shape
            .shape = PadShape::Circle;
        padstack
    }

    // ---- trivial accessors (padstack.h inline members) --------------------------------------

    /// `LayerSet()`.
    pub fn layer_set(&self) -> &Lset {
        &self.layer_set
    }

    /// Mutable `LayerSet()`.
    pub fn layer_set_mut(&mut self) -> &mut Lset {
        &mut self.layer_set
    }

    /// `SetLayerSet()`.
    pub fn set_layer_set(&mut self, set: Lset) {
        self.layer_set = set;
    }

    /// `Mode()`.
    pub fn mode(&self) -> PadstackMode {
        self.mode
    }

    /// `SetMode()`.
    pub fn set_mode(&mut self, mode: PadstackMode) {
        self.mode = mode;
    }

    /// `StartLayer()`: the drill start.
    pub fn start_layer(&self) -> PcbLayerId {
        self.drill.start
    }

    /// `EndLayer()`: the drill end.
    pub fn end_layer(&self) -> PcbLayerId {
        self.drill.end
    }

    /// `Name()` (= `CustomName()`).
    pub fn name(&self) -> String {
        self.custom_name().to_string()
    }

    /// `CustomName()`: empty when no custom name is set.
    pub fn custom_name(&self) -> &str {
        self.custom_name.as_deref().unwrap_or("")
    }

    /// `SetCustomName()`: an empty name resets it.
    pub fn set_custom_name(&mut self, custom_name: &str) {
        if custom_name.is_empty() {
            self.custom_name = None;
        } else {
            self.custom_name = Some(custom_name.to_string());
        }
    }

    /// `GetOrientation()`.
    pub fn orientation(&self) -> EdaAngle {
        self.orientation.angle()
    }

    /// `SetOrientation()`.
    pub fn set_orientation(&mut self, angle: EdaAngle) {
        self.orientation = EdaOrientation::new(angle);
    }

    /// `Drill()`.
    pub fn drill(&self) -> &DrillProps {
        &self.drill
    }

    /// Mutable `Drill()`.
    pub fn drill_mut(&mut self) -> &mut DrillProps {
        &mut self.drill
    }

    /// `SecondaryDrill()`.
    pub fn secondary_drill(&self) -> &DrillProps {
        &self.secondary_drill
    }

    /// Mutable `SecondaryDrill()`.
    pub fn secondary_drill_mut(&mut self) -> &mut DrillProps {
        &mut self.secondary_drill
    }

    /// `TertiaryDrill()`.
    pub fn tertiary_drill(&self) -> &DrillProps {
        &self.tertiary_drill
    }

    /// Mutable `TertiaryDrill()`.
    pub fn tertiary_drill_mut(&mut self) -> &mut DrillProps {
        &mut self.tertiary_drill
    }

    /// `FrontPostMachining()`.
    pub fn front_post_machining(&self) -> &PostMachiningProps {
        &self.front_post_machining
    }

    /// Mutable `FrontPostMachining()`.
    pub fn front_post_machining_mut(&mut self) -> &mut PostMachiningProps {
        &mut self.front_post_machining
    }

    /// `BackPostMachining()`.
    pub fn back_post_machining(&self) -> &PostMachiningProps {
        &self.back_post_machining
    }

    /// Mutable `BackPostMachining()`.
    pub fn back_post_machining_mut(&mut self) -> &mut PostMachiningProps {
        &mut self.back_post_machining
    }

    /// `UnconnectedLayerMode()`.
    pub fn unconnected_layer_mode(&self) -> UnconnectedLayerMode {
        self.unconnected_layer_mode
    }

    /// `SetUnconnectedLayerMode()`.
    pub fn set_unconnected_layer_mode(&mut self, mode: UnconnectedLayerMode) {
        self.unconnected_layer_mode = mode;
    }

    /// `FrontOuterLayers()`.
    pub fn front_outer_layers(&self) -> &MaskLayerProps {
        &self.front_mask_props
    }

    /// Mutable `FrontOuterLayers()`.
    pub fn front_outer_layers_mut(&mut self) -> &mut MaskLayerProps {
        &mut self.front_mask_props
    }

    /// `BackOuterLayers()`.
    pub fn back_outer_layers(&self) -> &MaskLayerProps {
        &self.back_mask_props
    }

    /// Mutable `BackOuterLayers()`.
    pub fn back_outer_layers_mut(&mut self) -> &mut MaskLayerProps {
        &mut self.back_mask_props
    }

    /// `CustomShapeInZoneMode()`.
    pub fn custom_shape_in_zone_mode(&self) -> CustomShapeZoneMode {
        self.custom_shape_in_zone_mode
    }

    /// `SetCustomShapeInZoneMode()`.
    pub fn set_custom_shape_in_zone_mode(&mut self, mode: CustomShapeZoneMode) {
        self.custom_shape_in_zone_mode = mode;
    }

    /// `DrillShape()`.
    pub fn drill_shape(&self) -> PadDrillShape {
        self.drill.shape
    }

    /// `SetDrillShape()`.
    pub fn set_drill_shape(&mut self, shape: PadDrillShape) {
        self.drill.shape = shape;
    }

    /// `IsCapped()`.
    pub fn is_capped(&self) -> Option<bool> {
        self.drill.is_capped
    }

    /// `IsFilled()`.
    pub fn is_filled(&self) -> Option<bool> {
        self.drill.is_filled
    }

    /// `HasExplicitDefinitionForLayer()`.
    pub fn has_explicit_definition_for_layer(&self, layer: PcbLayerId) -> bool {
        self.copper_props.contains_key(&layer)
    }

    // ---- mode logic (package M13) -----------------------------------------------------------

    /// `CopperLayer( aLayer ) const`: the props that apply to `layer` under the current mode
    /// (FRONT_INNER_BACK / CUSTOM fallbacks to the outer, inner and ALL_LAYERS entries).
    pub fn copper_props(&self, layer: PcbLayerId) -> &CopperLayerProps {
        crate::port_pending!("PADSTACK::CopperLayer (const)", layer)
    }

    /// `CopperLayer( aLayer )`: the entry for `layer`'s key, created from what a read would
    /// have returned (with owned copies of the primitives) when it does not exist yet.
    pub fn copper_props_mut(&mut self, layer: PcbLayerId) -> &mut CopperLayerProps {
        crate::port_pending!("PADSTACK::CopperLayer", layer)
    }

    /// `ForEachUniqueLayer()`: ALL_LAYERS (NORMAL), F_Cu / INNER_LAYERS / B_Cu
    /// (FRONT_INNER_BACK), or every key of `copper_props` (CUSTOM).
    pub fn for_each_unique_layer(&self, f: impl FnMut(PcbLayerId)) {
        crate::port_pending!("PADSTACK::ForEachUniqueLayer", f)
    }

    /// `UniqueLayers()`.
    pub fn unique_layers(&self) -> Vec<PcbLayerId> {
        crate::port_pending!("PADSTACK::UniqueLayers")
    }

    /// `EffectiveLayerFor( aLayer )`: the `copper_props` key that holds the shape of `layer`.
    pub fn effective_layer_for(&self, layer: PcbLayerId) -> PcbLayerId {
        crate::port_pending!("PADSTACK::EffectiveLayerFor", layer)
    }

    /// `RelevantShapeLayers( aOther )`: the layers on which the two padstacks' shapes must be
    /// compared.
    pub fn relevant_shape_layers(&self, other: &Padstack) -> Lset {
        crate::port_pending!("PADSTACK::RelevantShapeLayers", other)
    }

    /// `PADSTACK::operator=( aOther )` without `m_parent`: copies every member, then runs the
    /// C++ consistency pass (primitives cleared / replaced, zero-radius round rects become
    /// rectangles). `parent` is the owner the copied primitives are re-parented to.
    pub fn assign_from(&mut self, other: &Padstack, parent: ParentRef) {
        crate::port_pending!("PADSTACK::operator=", other, parent)
    }

    /// `PADSTACK::operator==`.
    pub fn eq_padstack(&self, other: &Padstack) -> bool {
        crate::port_pending!("PADSTACK::operator==", other)
    }

    /// `PADSTACK::Compare( aPadstackRef, aPadstackCmp )`.
    pub fn compare(padstack_ref: &Padstack, padstack_cmp: &Padstack) -> i32 {
        crate::port_pending!("PADSTACK::Compare", padstack_ref, padstack_cmp)
    }

    /// `FlipLayers( aBoard )`: renames the outer copper keys and drill layers through the
    /// board's flip table (`LayerEnv::no_board()` for the C++ null board) and swaps the front
    /// and back mask / post-machining props.
    pub fn flip_layers(&mut self, layers: &LayerEnv) {
        crate::port_pending!("PADSTACK::FlipLayers", layers)
    }

    /// `GetMaxHoleSize()`.
    pub fn max_hole_size(&self) -> i32 {
        crate::port_pending!("PADSTACK::GetMaxHoleSize")
    }

    /// `PostMachiningKnockout( aStackup, aLayer )`.
    pub fn post_machining_knockout(&self, stackup: &BoardStackup, layer: PcbLayerId) -> i32 {
        crate::port_pending!("PADSTACK::PostMachiningKnockout", stackup, layer)
    }

    /// `MachiningKnockout( aBoard, aLayer )`.
    pub fn machining_knockout(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        crate::port_pending!("PADSTACK::MachiningKnockout", ctx, layer)
    }

    /// `IsTented( aSide )`.
    pub fn is_tented(&self, side: PcbLayerId) -> Option<bool> {
        crate::port_pending!("PADSTACK::IsTented", side)
    }

    /// `IsCovered( aSide )`.
    pub fn is_covered(&self, side: PcbLayerId) -> Option<bool> {
        crate::port_pending!("PADSTACK::IsCovered", side)
    }

    /// `IsPlugged( aSide )`.
    pub fn is_plugged(&self, side: PcbLayerId) -> Option<bool> {
        crate::port_pending!("PADSTACK::IsPlugged", side)
    }

    // ---- per-layer shape accessors (forward to copper_props) --------------------------------

    /// `Shape( aLayer )`.
    pub fn shape(&self, layer: PcbLayerId) -> PadShape {
        self.copper_props(layer).shape.shape
    }

    /// `SetShape( aShape, aLayer )`.
    pub fn set_shape(&mut self, shape: PadShape, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.shape = shape;
    }

    /// `Size( aLayer )`.
    pub fn size(&self, layer: PcbLayerId) -> Vector2I {
        self.copper_props(layer).shape.size
    }

    /// `SetSize( aSize, aLayer )`: negative components are clamped to 0.
    pub fn set_size(&mut self, size: Vector2I, layer: PcbLayerId) {
        crate::port_pending!("PADSTACK::SetSize", size, layer)
    }

    /// `Offset( aLayer )`.
    pub fn offset(&self, layer: PcbLayerId) -> Vector2I {
        self.copper_props(layer).shape.offset
    }

    /// `SetOffset( aOffset, aLayer )`.
    pub fn set_offset(&mut self, offset: Vector2I, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.offset = offset;
    }

    /// `AnchorShape( aLayer )`.
    pub fn anchor_shape(&self, layer: PcbLayerId) -> PadShape {
        self.copper_props(layer).shape.anchor_shape
    }

    /// `SetAnchorShape( aShape, aLayer )`.
    pub fn set_anchor_shape(&mut self, shape: PadShape, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.anchor_shape = shape;
    }

    /// `TrapezoidDeltaSize( aLayer )`.
    pub fn trapezoid_delta_size(&self, layer: PcbLayerId) -> Vector2I {
        self.copper_props(layer).shape.trapezoid_delta_size
    }

    /// Mutable `TrapezoidDeltaSize( aLayer )`.
    pub fn trapezoid_delta_size_mut(&mut self, layer: PcbLayerId) -> &mut Vector2I {
        &mut self.copper_props_mut(layer).shape.trapezoid_delta_size
    }

    /// `RoundRectRadiusRatio( aLayer )`.
    pub fn round_rect_radius_ratio(&self, layer: PcbLayerId) -> f64 {
        self.copper_props(layer).shape.round_rect_radius_ratio
    }

    /// `SetRoundRectRadiusRatio( aRatio, aLayer )`.
    pub fn set_round_rect_radius_ratio(&mut self, ratio: f64, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.round_rect_radius_ratio = ratio;
    }

    /// `RoundRectRadius( aLayer )`.
    pub fn round_rect_radius(&self, layer: PcbLayerId) -> i32 {
        crate::port_pending!("PADSTACK::RoundRectRadius", layer)
    }

    /// `SetRoundRectRadius( aRadius, aLayer )`.
    pub fn set_round_rect_radius(&mut self, radius: f64, layer: PcbLayerId) {
        crate::port_pending!("PADSTACK::SetRoundRectRadius", radius, layer)
    }

    /// `ChamferRatio( aLayer )`.
    pub fn chamfer_ratio(&self, layer: PcbLayerId) -> f64 {
        self.copper_props(layer).shape.chamfered_rect_ratio
    }

    /// `SetChamferRatio( aRatio, aLayer )`.
    pub fn set_chamfer_ratio(&mut self, ratio: f64, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.chamfered_rect_ratio = ratio;
    }

    /// `ChamferPositions( aLayer )`.
    pub fn chamfer_positions(&self, layer: PcbLayerId) -> i32 {
        self.copper_props(layer).shape.chamfered_rect_positions
    }

    /// `SetChamferPositions( aPositions, aLayer )`.
    pub fn set_chamfer_positions(&mut self, positions: i32, layer: PcbLayerId) {
        self.copper_props_mut(layer).shape.chamfered_rect_positions = positions;
    }

    /// `Clearance( aLayer )`.
    pub fn clearance(&self, layer: PcbLayerId) -> Option<i32> {
        self.copper_props(layer).clearance
    }

    /// Mutable `Clearance( aLayer )`.
    pub fn clearance_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        &mut self.copper_props_mut(layer).clearance
    }

    /// `SolderMaskMargin( aLayer )`: front / back mask props by side (front for a layer that
    /// is neither, "should not happen").
    pub fn solder_mask_margin(&self, layer: PcbLayerId) -> Option<i32> {
        crate::port_pending!("PADSTACK::SolderMaskMargin", layer)
    }

    /// Mutable `SolderMaskMargin( aLayer )`.
    pub fn solder_mask_margin_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        crate::port_pending!("PADSTACK::SolderMaskMargin", layer)
    }

    /// `SolderPasteMargin( aLayer )`.
    pub fn solder_paste_margin(&self, layer: PcbLayerId) -> Option<i32> {
        crate::port_pending!("PADSTACK::SolderPasteMargin", layer)
    }

    /// Mutable `SolderPasteMargin( aLayer )`.
    pub fn solder_paste_margin_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        crate::port_pending!("PADSTACK::SolderPasteMargin", layer)
    }

    /// `SolderPasteMarginRatio( aLayer )`.
    pub fn solder_paste_margin_ratio(&self, layer: PcbLayerId) -> Option<f64> {
        crate::port_pending!("PADSTACK::SolderPasteMarginRatio", layer)
    }

    /// Mutable `SolderPasteMarginRatio( aLayer )`.
    pub fn solder_paste_margin_ratio_mut(&mut self, layer: PcbLayerId) -> &mut Option<f64> {
        crate::port_pending!("PADSTACK::SolderPasteMarginRatio", layer)
    }

    /// `ZoneConnection( aLayer )`.
    pub fn zone_connection(&self, layer: PcbLayerId) -> Option<ZoneConnection> {
        self.copper_props(layer).zone_connection
    }

    /// Mutable `ZoneConnection( aLayer )`.
    pub fn zone_connection_mut(&mut self, layer: PcbLayerId) -> &mut Option<ZoneConnection> {
        &mut self.copper_props_mut(layer).zone_connection
    }

    /// `ThermalSpokeWidth( aLayer )`.
    pub fn thermal_spoke_width(&self, layer: PcbLayerId) -> Option<i32> {
        self.copper_props(layer).thermal_spoke_width
    }

    /// Mutable `ThermalSpokeWidth( aLayer )`.
    pub fn thermal_spoke_width_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        &mut self.copper_props_mut(layer).thermal_spoke_width
    }

    /// `ThermalGap( aLayer )`.
    pub fn thermal_gap(&self, layer: PcbLayerId) -> Option<i32> {
        self.copper_props(layer).thermal_gap
    }

    /// Mutable `ThermalGap( aLayer )`.
    pub fn thermal_gap_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        &mut self.copper_props_mut(layer).thermal_gap
    }

    /// `DefaultThermalSpokeAngleForShape( aLayer )`: 45 degrees for circles (or circle
    /// anchors), 90 otherwise.
    pub fn default_thermal_spoke_angle_for_shape(&self, layer: PcbLayerId) -> EdaAngle {
        crate::port_pending!("PADSTACK::DefaultThermalSpokeAngleForShape", layer)
    }

    /// `ThermalSpokeAngle( aLayer )`.
    pub fn thermal_spoke_angle(&self, layer: PcbLayerId) -> EdaAngle {
        crate::port_pending!("PADSTACK::ThermalSpokeAngle", layer)
    }

    /// `SetThermalSpokeAngle( aAngle, aLayer )`.
    pub fn set_thermal_spoke_angle(&mut self, angle: EdaAngle, layer: PcbLayerId) {
        self.copper_props_mut(layer).thermal_spoke_angle = Some(EdaOrientation::new(angle));
    }

    // ---- custom-shape primitives ------------------------------------------------------------

    /// `Primitives( aLayer )`.
    pub fn primitives(&self, layer: PcbLayerId) -> &[PcbShape] {
        &self.copper_props(layer).custom_shapes
    }

    /// Mutable `Primitives( aLayer )`.
    pub fn primitives_mut(&mut self, layer: PcbLayerId) -> &mut Vec<PcbShape> {
        &mut self.copper_props_mut(layer).custom_shapes
    }

    /// `AddPrimitive( aShape, aLayer )`: takes ownership of the shape (its parent is set by the
    /// caller, as in C++).
    pub fn add_primitive(&mut self, shape: PcbShape, layer: PcbLayerId) {
        crate::port_pending!("PADSTACK::AddPrimitive", shape, layer)
    }

    /// `AppendPrimitives( aPrimitivesList, aLayer )`: copies, re-parented to `parent` (the
    /// C++ `m_parent`).
    pub fn append_primitives(&mut self, list: &[PcbShape], layer: PcbLayerId, parent: ParentRef) {
        crate::port_pending!("PADSTACK::AppendPrimitives", list, layer, parent)
    }

    /// `ReplacePrimitives( aPrimitivesList, aLayer )`.
    pub fn replace_primitives(&mut self, list: &[PcbShape], layer: PcbLayerId, parent: ParentRef) {
        crate::port_pending!("PADSTACK::ReplacePrimitives", list, layer, parent)
    }

    /// `ClearPrimitives( aLayer )`.
    pub fn clear_primitives(&mut self, layer: PcbLayerId) {
        crate::port_pending!("PADSTACK::ClearPrimitives", layer)
    }

    // ---- backdrill --------------------------------------------------------------------------

    /// `GetBackdrillMode()`.
    pub fn backdrill_mode(&self) -> BackdrillMode {
        crate::port_pending!("PADSTACK::GetBackdrillMode")
    }

    /// `SetBackdrillMode( aMode )`.
    pub fn set_backdrill_mode(&mut self, mode: BackdrillMode) {
        crate::port_pending!("PADSTACK::SetBackdrillMode", mode)
    }

    /// `GetBackdrillSize( aTop )`.
    pub fn backdrill_size(&self, top: bool) -> Option<i32> {
        crate::port_pending!("PADSTACK::GetBackdrillSize", top)
    }

    /// `SetBackdrillSize( aTop, aSize )`.
    pub fn set_backdrill_size(&mut self, top: bool, size: Option<i32>) {
        crate::port_pending!("PADSTACK::SetBackdrillSize", top, size)
    }

    /// `GetBackdrillEndLayer( aTop )`.
    pub fn backdrill_end_layer(&self, top: bool) -> PcbLayerId {
        crate::port_pending!("PADSTACK::GetBackdrillEndLayer", top)
    }

    /// `SetBackdrillEndLayer( aTop, aLayer )`.
    pub fn set_backdrill_end_layer(&mut self, top: bool, layer: PcbLayerId) {
        crate::port_pending!("PADSTACK::SetBackdrillEndLayer", top, layer)
    }
}

impl ShapeProps {
    /// `SHAPE_PROPS::Compare`.
    pub fn compare(&self, other: &ShapeProps) -> i32 {
        crate::port_pending!("PADSTACK::SHAPE_PROPS::Compare", other)
    }
}

impl CopperLayerProps {
    /// `COPPER_LAYER_PROPS::operator==` (custom shapes compared by count only, as C++).
    pub fn eq_props(&self, other: &CopperLayerProps) -> bool {
        crate::port_pending!("PADSTACK::COPPER_LAYER_PROPS::operator==", other)
    }

    /// `COPPER_LAYER_PROPS::Compare`.
    pub fn compare(&self, other: &CopperLayerProps) -> i32 {
        crate::port_pending!("PADSTACK::COPPER_LAYER_PROPS::Compare", other)
    }
}

impl MaskLayerProps {
    /// `MASK_LAYER_PROPS::Compare`.
    pub fn compare(&self, other: &MaskLayerProps) -> i32 {
        crate::port_pending!("PADSTACK::MASK_LAYER_PROPS::Compare", other)
    }
}

impl DrillProps {
    /// `DRILL_PROPS::Compare`.
    pub fn compare(&self, other: &DrillProps) -> i32 {
        crate::port_pending!("PADSTACK::DRILL_PROPS::Compare", other)
    }
}

impl PostMachiningProps {
    /// `POST_MACHINING_PROPS::Compare`.
    pub fn compare(&self, other: &PostMachiningProps) -> i32 {
        crate::port_pending!("PADSTACK::POST_MACHINING_PROPS::Compare", other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `PADSTACK::PADSTACK( aParent )` and `SHAPE_PROPS::SHAPE_PROPS()` (padstack.cpp).
    #[test]
    fn padstack_constructor_defaults() {
        let p = Padstack::default();
        assert_eq!(p.mode, PadstackMode::Normal);
        assert!(p.layer_set.iter().next().is_none());
        assert_eq!(p.custom_name(), "");
        assert_eq!(p.orientation(), ANGLE_0);
        assert_eq!(p.copper_props.len(), 1);
        let all = &p.copper_props[&Padstack::ALL_LAYERS];
        assert_eq!(all.shape, ShapeProps::default());
        assert_eq!(all.shape.shape, PadShape::Circle);
        assert_eq!(all.shape.anchor_shape, PadShape::Rectangle);
        assert_eq!(all.zone_connection, Some(ZoneConnection::Inherited));
        assert_eq!(all.thermal_spoke_width, None);
        assert_eq!(all.thermal_spoke_angle, Some(EdaOrientation::new(ANGLE_45)));
        assert_eq!(all.thermal_gap, None);
        assert_eq!(all.clearance, None);
        assert_eq!(p.front_mask_props, MaskLayerProps::default());
        assert_eq!(p.unconnected_layer_mode, UnconnectedLayerMode::KeepAll);
        assert_eq!(p.custom_shape_in_zone_mode, CustomShapeZoneMode::Outline);
        assert_eq!(p.drill.shape, PadDrillShape::Circle);
        assert_eq!(
            (p.drill.start, p.drill.end),
            (PcbLayerId::FCu, PcbLayerId::BCu)
        );
        assert_eq!(p.drill.size, Vector2I::new(0, 0));
        for d in [&p.secondary_drill, &p.tertiary_drill] {
            assert_eq!(d.shape, PadDrillShape::Undefined);
            assert_eq!(d.start, PcbLayerId::UndefinedLayer);
            assert_eq!(d.end, PcbLayerId::UndefinedLayer);
        }
        assert_eq!(p.front_post_machining, PostMachiningProps::default());
        assert_eq!(p.back_post_machining.mode, None);
    }

    /// The padstack statements of `PCB_VIA::PCB_VIA( aParent )` (pcb_track.cpp).
    #[test]
    fn padstack_for_new_via() {
        let p = Padstack::for_new_via();
        assert_eq!(p.drill.size, Vector2I::new(-1, -1));
        assert_eq!(
            (p.start_layer(), p.end_layer()),
            (PcbLayerId::FCu, PcbLayerId::BCu)
        );
        assert_eq!(p.unconnected_layer_mode(), UnconnectedLayerMode::KeepAll);
        assert!(p.layer_set().iter().next().is_none());
        assert_eq!(
            p.copper_props[&Padstack::ALL_LAYERS].shape.shape,
            PadShape::Circle
        );
    }

    #[test]
    fn padstack_custom_name() {
        let mut p = Padstack::default();
        p.set_custom_name("abc");
        assert_eq!(p.name(), "abc");
        p.set_custom_name("");
        assert_eq!(p.custom_name, None);
    }
}
