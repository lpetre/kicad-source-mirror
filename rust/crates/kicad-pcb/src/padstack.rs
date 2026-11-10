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
use crate::item::DetachedCopy;
use crate::item::ParentRef;
use crate::pcb_shape::PcbShape;
use crate::pcb_track::UNDEFINED_DRILL_DIAMETER;
use crate::traits::EdaShapeItem;
use crate::zone_settings::ZoneConnection;

/// The C++ `TEST( a, b )` macro of padstack.cpp: `if( a != b ) return a - b;` (the int
/// difference; the reference build wraps on overflow).
macro_rules! cmp_test {
    ($a:expr, $b:expr) => {{
        let a: i32 = $a;
        let b: i32 = $b;

        if a != b {
            return a.wrapping_sub(b);
        }
    }};
}

/// The C++ `TEST_OPT( a, b, v )` macro: unset sorts before set (`has_value()` difference),
/// then the int difference of `value_or( v )`.
macro_rules! test_opt {
    ($a:expr, $b:expr, $v:expr, $to_int:expr) => {{
        let a = $a;
        let b = $b;

        if a.is_some() != b.is_some() {
            return i32::from(a.is_some()) - i32::from(b.is_some());
        }

        let to_int = $to_int;
        let av: i32 = to_int(a.unwrap_or($v));
        let bv: i32 = to_int(b.unwrap_or($v));

        if av.wrapping_sub(bv) != 0 {
            return av.wrapping_sub(bv);
        }
    }};
}

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

    /// Port of `PADSTACK::CopperLayer( aLayer ) const`: the props that apply to `layer` under
    /// the current mode (FRONT_INNER_BACK / CUSTOM fallbacks to the outer, inner and
    /// ALL_LAYERS entries, and finally to `begin()`).
    pub fn copper_props(&self, layer: PcbLayerId) -> &CopperLayerProps {
        let props = &self.copper_props;

        if self.mode == PadstackMode::FrontInnerBack {
            if is_front_layer(layer) && props.contains_key(&PcbLayerId::FCu) {
                return &props[&PcbLayerId::FCu];
            } else if is_back_layer(layer) && props.contains_key(&PcbLayerId::BCu) {
                return &props[&PcbLayerId::BCu];
            } else if props.contains_key(&Padstack::INNER_LAYERS) {
                return &props[&Padstack::INNER_LAYERS];
            }
        } else if self.mode == PadstackMode::Custom {
            if is_front_layer(layer) && props.contains_key(&PcbLayerId::FCu) {
                return &props[&PcbLayerId::FCu];
            } else if is_back_layer(layer) && props.contains_key(&PcbLayerId::BCu) {
                return &props[&PcbLayerId::BCu];
            }

            if let Some(found) = props.get(&layer) {
                return found;
            }

            // For CUSTOM mode, fall back to ALL_LAYERS if available (e.g. for layers not yet
            // explicitly defined).  If ALL_LAYERS is also absent (e.g. after a FlipLayers()
            // that renamed the only entry from F_Cu to B_Cu), return whatever entry is first.
            if let Some(found) = props.get(&Padstack::ALL_LAYERS) {
                return found;
            }

            debug_assert!(!props.is_empty());
            return props
                .first()
                .map(|(_, p)| p)
                .expect("PADSTACK::CopperLayer: CUSTOM padstack without copper props");
        }

        // m_copperProps.at( ALL_LAYERS ): throws std::out_of_range in C++ when missing.
        props
            .get(&Padstack::ALL_LAYERS)
            .expect("PADSTACK::CopperLayer: no ALL_LAYERS entry")
    }

    /// Port of `PADSTACK::CopperLayer( aLayer )`: the entry for `layer`'s key, created from what
    /// a read would have returned (with owned copies of the primitives) when it does not exist
    /// yet and the layer reads through to another entry; otherwise `operator[]` (default props).
    pub fn copper_props_mut(&mut self, layer: PcbLayerId) -> &mut CopperLayerProps {
        if self.mode == PadstackMode::Normal {
            return self.copper_props.index_or_default(Padstack::ALL_LAYERS);
        }

        let mut key = layer;

        if self.mode == PadstackMode::FrontInnerBack {
            if is_front_layer(layer) {
                key = PcbLayerId::FCu;
            } else if is_back_layer(layer) {
                key = PcbLayerId::BCu;
            } else {
                key = Padstack::INNER_LAYERS;
            }
        }

        // A layer without an entry reads through to another one, so a new entry starts from
        // what was read
        let reads_through = self.copper_props.contains_key(&Padstack::ALL_LAYERS)
            || (self.mode == PadstackMode::Custom && !self.copper_props.is_empty());

        if !self.copper_props.contains_key(&key) && reads_through {
            let mut inherited = self.copper_props(layer).clone();

            // the new layer owns its primitives, as flipping or editing one layer must not
            // change the other. (`shape->SetParent( m_parent )`: the copies keep the parent of
            // the entry they were read from, which is the owner of this padstack.)
            for shape in &mut inherited.custom_shapes {
                *shape = shape.detached_copy();
            }

            return self.copper_props.emplace(key, inherited);
        }

        self.copper_props.index_or_default(key)
    }

    /// Port of `PADSTACK::ForEachUniqueLayer()`: ALL_LAYERS (NORMAL), F_Cu / INNER_LAYERS /
    /// B_Cu (FRONT_INNER_BACK), or every key of `copper_props` in map order (CUSTOM).
    pub fn for_each_unique_layer(&self, mut f: impl FnMut(PcbLayerId)) {
        if self.mode == PadstackMode::Normal {
            f(Padstack::ALL_LAYERS);
        } else if self.mode == PadstackMode::FrontInnerBack {
            f(PcbLayerId::FCu);
            f(Padstack::INNER_LAYERS);
            f(PcbLayerId::BCu);
        } else {
            for (layer, _) in self.copper_props.iter() {
                f(*layer);
            }
        }
    }

    /// Port of `PADSTACK::UniqueLayers()`.
    pub fn unique_layers(&self) -> Vec<PcbLayerId> {
        let mut layers = Vec::new();
        self.for_each_unique_layer(|layer| layers.push(layer));
        layers
    }

    /// Port of `PADSTACK::EffectiveLayerFor( aLayer )`: the `copper_props` key that holds the
    /// shape of `layer`.
    pub fn effective_layer_for(&self, layer: PcbLayerId) -> PcbLayerId {
        if self.mode == PadstackMode::Normal {
            return Padstack::ALL_LAYERS;
        }

        if self.mode == PadstackMode::FrontInnerBack || is_non_copper_layer(layer) {
            let candidate = if is_front_layer(layer) {
                PcbLayerId::FCu
            } else if is_back_layer(layer) {
                PcbLayerId::BCu
            } else {
                Padstack::INNER_LAYERS
            };

            // FRONT_INNER_BACK always has all three sides.
            // In CUSTOM mode only return the side if the pad actually defines it.
            if self.mode == PadstackMode::FrontInnerBack
                || self.copper_props.contains_key(&candidate)
            {
                return candidate;
            }
        }

        if self.copper_props.contains_key(&layer) {
            return layer;
        }

        // For CUSTOM mode, if ALL_LAYERS is present use it as the default; otherwise return
        // the first available layer (e.g. after FlipLayers renamed ALL_LAYERS from F_Cu to
        // B_Cu).
        if self.copper_props.contains_key(&Padstack::ALL_LAYERS) {
            return Padstack::ALL_LAYERS;
        }

        debug_assert!(!self.copper_props.is_empty());
        self.copper_props
            .first()
            .map(|(l, _)| *l)
            .expect("PADSTACK::EffectiveLayerFor: padstack without copper props")
    }

    /// Port of `PADSTACK::RelevantShapeLayers( aOther )`: the layers on which the two
    /// padstacks' shapes must be compared.
    pub fn relevant_shape_layers(&self, other: &Padstack) -> Lset {
        let mut layers = Lset::new();

        if self.mode == PadstackMode::Normal && other.mode == PadstackMode::Normal {
            layers.set(Padstack::ALL_LAYERS);
        } else {
            self.for_each_unique_layer(|layer| {
                layers.set(layer);
            });
            other.for_each_unique_layer(|layer| {
                layers.set(layer);
            });
        }

        layers
    }

    /// Port of `PADSTACK::operator=( aOther )` without `m_parent` ("m_parent is not copied
    /// from operator=, because this operator is commonly used to update the padstack
    /// properties"): copies every member, then runs the consistency pass (primitives cleared /
    /// replaced, zero-radius round rects become rectangles). `parent` is the C++ `m_parent`
    /// of `self`, the owner the copied primitives are re-parented to.
    pub fn assign_from(&mut self, other: &Padstack, parent: ParentRef) {
        self.mode = other.mode;
        self.layer_set = other.layer_set.clone();
        self.set_custom_name(other.custom_name());
        self.orientation = other.orientation;
        self.copper_props = other.copper_props.clone();
        self.front_mask_props = other.front_mask_props.clone();
        self.back_mask_props = other.back_mask_props.clone();
        self.unconnected_layer_mode = other.unconnected_layer_mode;
        self.custom_shape_in_zone_mode = other.custom_shape_in_zone_mode;
        self.drill = other.drill.clone();
        self.secondary_drill = other.secondary_drill.clone();
        self.tertiary_drill = other.tertiary_drill.clone();
        self.front_post_machining = other.front_post_machining.clone();
        self.back_post_machining = other.back_post_machining.clone();

        // Data consistency enforcement logic that used to live in the pad properties dialog.
        // While it might be tempting to put these in the individual property setters, there's
        // no well-defined order in which they're called, and many of the consistency checks
        // are between multiple properties.
        //
        // (The lambda never adds a key in CUSTOM mode and FRONT_INNER_BACK visits a fixed
        // list, so collecting the layers first visits the same layers in the same order.)
        for layer in self.unique_layers() {
            let shape = self.shape(layer);

            // Make sure leftover primitives don't stick around
            self.clear_primitives(layer);

            // For custom pad shape, duplicate primitives of the pad to copy
            if shape == PadShape::Custom {
                self.replace_primitives(other.primitives(layer), layer, parent);
            }

            // rounded rect pads with radius ratio = 0 are in fact rect pads.
            // So set the right shape (and perhaps issues with a radius = 0)
            if shape == PadShape::Roundrect && self.round_rect_radius_ratio(layer) == 0.0 {
                self.set_shape(PadShape::Rectangle, layer);
            }
        }
    }

    /// Port of `PADSTACK::operator==`.
    pub fn eq_padstack(&self, other: &Padstack) -> bool {
        if self.mode != other.mode {
            return false;
        }

        if self.layer_set != other.layer_set {
            return false;
        }

        if let (Some(a), Some(b)) = (&self.custom_name, &other.custom_name) {
            if a != b {
                return false;
            }
        }
        // C++: `else if( (bool) m_customName != (bool) m_customName )`, which compares the
        // member with itself and is never true: a name on only one side is not a difference.

        if self.orientation != other.orientation {
            return false;
        }

        if self.front_mask_props != other.front_mask_props {
            return false;
        }

        if self.back_mask_props != other.back_mask_props {
            return false;
        }

        if self.unconnected_layer_mode != other.unconnected_layer_mode {
            return false;
        }

        if self.custom_shape_in_zone_mode != other.custom_shape_in_zone_mode {
            return false;
        }

        if self.drill != other.drill {
            return false;
        }

        if self.secondary_drill != other.secondary_drill {
            return false;
        }

        if self.tertiary_drill != other.tertiary_drill {
            return false;
        }

        if self.front_post_machining != other.front_post_machining {
            return false;
        }

        if self.back_post_machining != other.back_post_machining {
            return false;
        }

        let mut copper_matches = true;

        self.for_each_unique_layer(|layer| {
            if !self.copper_props(layer).eq_props(other.copper_props(layer)) {
                copper_matches = false;
            }
        });

        copper_matches
    }

    /// Port of the static `PADSTACK::Compare( aLeft, aRight )`.
    pub fn compare(padstack_ref: &Padstack, padstack_cmp: &Padstack) -> i32 {
        let left = padstack_ref;
        let right = padstack_cmp;

        cmp_test!(left.mode as i32, right.mode as i32);

        if left.layer_set != right.layer_set {
            // `return aLeft->m_layerSet.Seq() < aRight->m_layerSet.Seq();` (a bool)
            return i32::from(left.layer_set.seq().as_slice() < right.layer_set.seq().as_slice());
        }

        // wxString::Cmp (code-point order, -1 / 0 / 1)
        let diff = match left.custom_name().cmp(right.custom_name()) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        };

        if diff != 0 {
            return diff;
        }

        cmp_test!(
            left.orientation.angle().as_tenths_of_a_degree(),
            right.orientation.angle().as_tenths_of_a_degree()
        );

        let mut diff = left.front_mask_props.compare(&right.front_mask_props);

        if diff != 0 {
            return diff;
        }

        diff = left.back_mask_props.compare(&right.back_mask_props);

        if diff != 0 {
            return diff;
        }

        cmp_test!(
            left.unconnected_layer_mode as i32,
            right.unconnected_layer_mode as i32
        );
        cmp_test!(
            left.custom_shape_in_zone_mode as i32,
            right.custom_shape_in_zone_mode as i32
        );

        for (a, b) in [
            (&left.drill, &right.drill),
            (&left.secondary_drill, &right.secondary_drill),
            (&left.tertiary_drill, &right.tertiary_drill),
        ] {
            diff = a.compare(b);

            if diff != 0 {
                return diff;
            }
        }

        diff = left
            .front_post_machining
            .compare(&right.front_post_machining);

        if diff != 0 {
            return diff;
        }

        diff = left.back_post_machining.compare(&right.back_post_machining);

        if diff != 0 {
            return diff;
        }

        left.for_each_unique_layer(|layer| {
            if diff != 0 {
                // we want to return the first non-matching layer
                return;
            }

            diff = left.copper_props(layer).compare(right.copper_props(layer));
        });

        if diff != 0 {
            return diff;
        }

        0
    }

    /// Port of `PADSTACK::Similarity( aOther )`. `COPPER_LAYER_PROPS::Similarity` compares the
    /// `shared_ptr` vectors of the primitives by pointer; the port's primitives are values, so
    /// two padstacks' non-empty primitive lists always differ (as two distinct C++ objects
    /// do) and two empty lists are equal.
    pub fn similarity(&self, other: &Padstack) -> f64 {
        let mut similarity = 1.0;

        if self.mode != other.mode {
            similarity *= 0.9;
        }

        if self.layer_set != other.layer_set {
            similarity *= 0.9;
        }

        if self.custom_name() != other.custom_name() {
            similarity *= 0.9;
        }

        if self.orientation != other.orientation {
            similarity *= 0.9;
        }

        if self.front_mask_props != other.front_mask_props {
            similarity *= 0.9;
        }

        if self.back_mask_props != other.back_mask_props {
            similarity *= 0.9;
        }

        if self.unconnected_layer_mode != other.unconnected_layer_mode {
            similarity *= 0.9;
        }

        if self.custom_shape_in_zone_mode != other.custom_shape_in_zone_mode {
            similarity *= 0.9;
        }

        if self.drill != other.drill {
            similarity *= 0.9;
        }

        if self.secondary_drill != other.secondary_drill {
            similarity *= 0.9;
        }

        if self.tertiary_drill != other.tertiary_drill {
            similarity *= 0.9;
        }

        if self.front_post_machining != other.front_post_machining {
            similarity *= 0.9;
        }

        if self.back_post_machining != other.back_post_machining {
            similarity *= 0.9;
        }

        self.for_each_unique_layer(|layer| {
            similarity *= self
                .copper_props(layer)
                .similarity(other.copper_props(layer));
        });

        similarity
    }

    /// Port of `PADSTACK::FlipLayers( aBoard )`: renames the outer copper keys and drill layers
    /// through the board's flip table (`LayerEnv::no_board()` for the C++ null board, which
    /// uses `::FlipLayer( aLayer )`) and swaps the front and back mask / post-machining props.
    pub fn flip_layers(&mut self, layers: &LayerEnv) {
        let flip_layer = |layer: PcbLayerId| layers.flip_layer(layer);

        if self.mode == PadstackMode::FrontInnerBack {
            let mut old_copper_props = self.copper_props.clone();
            self.copper_props.clear();

            // `m_copperProps[flipLayer( F_Cu )] = oldCopperProps[F_Cu];` and so on: the right
            // operand (operator[] on the copy) is evaluated first.
            for layer in [PcbLayerId::FCu, Padstack::INNER_LAYERS, PcbLayerId::BCu] {
                let props = old_copper_props.index_or_default(layer).clone();
                let target = if layer == Padstack::INNER_LAYERS {
                    layer
                } else {
                    flip_layer(layer)
                };
                *self.copper_props.index_or_default(target) = props;
            }
        } else if self.mode == PadstackMode::Custom {
            let old_copper_props = self.copper_props.clone();
            self.copper_props.clear();

            for (layer, props) in old_copper_props.iter() {
                *self.copper_props.index_or_default(flip_layer(*layer)) = props.clone();
            }
        }

        std::mem::swap(&mut self.front_mask_props, &mut self.back_mask_props);

        self.drill.start = flip_layer(self.drill.start);
        self.drill.end = flip_layer(self.drill.end);

        self.secondary_drill.start = flip_layer(self.secondary_drill.start);
        self.secondary_drill.end = flip_layer(self.secondary_drill.end);

        self.tertiary_drill.start = flip_layer(self.tertiary_drill.start);
        self.tertiary_drill.end = flip_layer(self.tertiary_drill.end);

        std::mem::swap(
            &mut self.front_post_machining,
            &mut self.back_post_machining,
        );
    }

    /// Port of `PADSTACK::GetMaxHoleSize()`.
    pub fn max_hole_size(&self) -> i32 {
        let mut max_hole_size = self.drill().size.x;

        let front_pm = self.front_post_machining();
        let back_pm = self.back_post_machining();

        // (an unset optional compares unequal to both modes, so it counts)
        if front_pm.mode != Some(PadDrillPostMachiningMode::NotPostMachined)
            && front_pm.mode != Some(PadDrillPostMachiningMode::Unknown)
        {
            max_hole_size = max_hole_size.max(front_pm.size);
        }

        if back_pm.mode != Some(PadDrillPostMachiningMode::NotPostMachined)
            && back_pm.mode != Some(PadDrillPostMachiningMode::Unknown)
        {
            max_hole_size = max_hole_size.max(back_pm.size);
        }

        let secondary_drill = self.secondary_drill();
        let tertiary_drill = self.tertiary_drill();

        if secondary_drill.start != PcbLayerId::UndefinedLayer
            && secondary_drill.end != PcbLayerId::UndefinedLayer
        {
            max_hole_size = max_hole_size.max(secondary_drill.size.x);
        }

        if tertiary_drill.start != PcbLayerId::UndefinedLayer
            && tertiary_drill.end != PcbLayerId::UndefinedLayer
        {
            max_hole_size = max_hole_size.max(tertiary_drill.size.x);
        }

        max_hole_size
    }

    /// Port of `PADSTACK::PostMachiningKnockout( aStackup, aLayer )`.
    pub fn post_machining_knockout(&self, stackup: &BoardStackup, layer: PcbLayerId) -> i32 {
        surface_knockout(stackup, &self.front_post_machining, PcbLayerId::FCu, layer).max(
            surface_knockout(stackup, &self.back_post_machining, PcbLayerId::BCu, layer),
        )
    }

    /// Port of `PADSTACK::MachiningKnockout( aBoard, aLayer )`.
    pub fn machining_knockout(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> i32 {
        if self.secondary_drill.size.x <= 0
            && self.tertiary_drill.size.x <= 0
            && self.front_post_machining.mode.is_none()
            && self.back_post_machining.mode.is_none()
        {
            return 0;
        }

        let board = ctx.board;
        let mut knockout =
            self.post_machining_knockout(board.design_settings().stackup_descriptor(), layer);

        for drill in [&self.secondary_drill, &self.tertiary_drill] {
            if drill.size.x <= 0
                || drill.start == PcbLayerId::UndefinedLayer
                || drill.end == PcbLayerId::UndefinedLayer
            {
                continue;
            }

            // An off-stack endpoint would widen the span onto layers the drill never reaches
            if !board.is_layer_enabled(drill.start) || !board.is_layer_enabled(drill.end) {
                continue;
            }

            // Contains honours copper Z-order; comparing raw PCB_LAYER_ID values instead puts
            // B_Cu above the inner layers and mis-spans any bottom-anchored backdrill
            if LayerRange::contains_range(drill.start.0, drill.end.0, layer.0) {
                knockout = knockout.max(effective_drill_diameter(drill.size));
            }
        }

        knockout
    }

    /// Port of `PADSTACK::IsTented( aSide )`.
    pub fn is_tented(&self, side: PcbLayerId) -> Option<bool> {
        if is_front_layer(side) {
            self.front_outer_layers().has_solder_mask
        } else if is_back_layer(side) {
            self.back_outer_layers().has_solder_mask
        } else {
            None
        }
    }

    /// Port of `PADSTACK::IsCovered( aSide )`.
    pub fn is_covered(&self, side: PcbLayerId) -> Option<bool> {
        if is_front_layer(side) {
            self.front_outer_layers().has_covering
        } else if is_back_layer(side) {
            self.back_outer_layers().has_covering
        } else {
            None
        }
    }

    /// Port of `PADSTACK::IsPlugged( aSide )`.
    pub fn is_plugged(&self, side: PcbLayerId) -> Option<bool> {
        if is_front_layer(side) {
            self.front_outer_layers().has_plugging
        } else if is_back_layer(side) {
            self.back_outer_layers().has_plugging
        } else {
            None
        }
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

    /// Port of `PADSTACK::SetSize( aSize, aLayer )`: negative components are clamped to 0.
    pub fn set_size(&mut self, size: Vector2I, layer: PcbLayerId) {
        let mut size = size;

        if size.x < 0 {
            size.x = 0;
        }

        if size.y < 0 {
            size.y = 0;
        }

        self.copper_props_mut(layer).shape.size = size;
    }

    /// `Offset( aLayer )`.
    pub fn offset(&self, layer: PcbLayerId) -> Vector2I {
        self.copper_props(layer).shape.offset
    }

    /// Mutable `Offset( aLayer )`.
    pub fn offset_mut(&mut self, layer: PcbLayerId) -> &mut Vector2I {
        &mut self.copper_props_mut(layer).shape.offset
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

    /// Port of `PADSTACK::RoundRectRadius( aLayer )`.
    pub fn round_rect_radius(&self, layer: PcbLayerId) -> i32 {
        let size = self.size(layer);
        ki_round(f64::from(size.x.min(size.y)) * self.round_rect_radius_ratio(layer))
    }

    /// Port of `PADSTACK::SetRoundRectRadius( aRadius, aLayer )`: no change for a pad without
    /// a positive minimum size.
    pub fn set_round_rect_radius(&mut self, radius: f64, layer: PcbLayerId) {
        let size = self.size(layer);
        let min_r = size.x.min(size.y);

        if min_r > 0 {
            self.set_round_rect_radius_ratio(radius / f64::from(min_r), layer);
        }
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

    /// Mutable `ChamferPositions( aLayer )`.
    pub fn chamfer_positions_mut(&mut self, layer: PcbLayerId) -> &mut i32 {
        &mut self.copper_props_mut(layer).shape.chamfered_rect_positions
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

    /// The outer-layer props `SolderMaskMargin` & co. use for `layer`: front / back by side,
    /// front for a layer that is neither ("Should not happen").
    fn outer_props_for(&self, layer: PcbLayerId) -> &MaskLayerProps {
        if is_front_layer(layer) {
            &self.front_mask_props
        } else if is_back_layer(layer) {
            &self.back_mask_props
        } else {
            &self.front_mask_props // Should not happen
        }
    }

    /// Mutable [`Padstack::outer_props_for`].
    fn outer_props_for_mut(&mut self, layer: PcbLayerId) -> &mut MaskLayerProps {
        if is_front_layer(layer) {
            &mut self.front_mask_props
        } else if is_back_layer(layer) {
            &mut self.back_mask_props
        } else {
            &mut self.front_mask_props // Should not happen
        }
    }

    /// Port of `PADSTACK::SolderMaskMargin( aLayer ) const` (C++ default layer F_Cu).
    pub fn solder_mask_margin(&self, layer: PcbLayerId) -> Option<i32> {
        self.outer_props_for(layer).solder_mask_margin
    }

    /// Port of `PADSTACK::SolderMaskMargin( aLayer )`.
    pub fn solder_mask_margin_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        &mut self.outer_props_for_mut(layer).solder_mask_margin
    }

    /// Port of `PADSTACK::SolderPasteMargin( aLayer ) const` (C++ default layer F_Cu).
    pub fn solder_paste_margin(&self, layer: PcbLayerId) -> Option<i32> {
        self.outer_props_for(layer).solder_paste_margin
    }

    /// Port of `PADSTACK::SolderPasteMargin( aLayer )`.
    pub fn solder_paste_margin_mut(&mut self, layer: PcbLayerId) -> &mut Option<i32> {
        &mut self.outer_props_for_mut(layer).solder_paste_margin
    }

    /// Port of `PADSTACK::SolderPasteMarginRatio( aLayer ) const` (C++ default layer F_Cu).
    pub fn solder_paste_margin_ratio(&self, layer: PcbLayerId) -> Option<f64> {
        self.outer_props_for(layer).solder_paste_margin_ratio
    }

    /// Port of `PADSTACK::SolderPasteMarginRatio( aLayer )`.
    pub fn solder_paste_margin_ratio_mut(&mut self, layer: PcbLayerId) -> &mut Option<f64> {
        &mut self.outer_props_for_mut(layer).solder_paste_margin_ratio
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

    /// Port of `PADSTACK::DefaultThermalSpokeAngleForShape( aLayer )`: 45 degrees for circles
    /// (or circle anchors of custom pads), 90 otherwise.
    pub fn default_thermal_spoke_angle_for_shape(&self, layer: PcbLayerId) -> EdaAngle {
        let mut shape = self.shape(layer);

        if shape == PadShape::Custom {
            shape = self.anchor_shape(layer);
        }

        if shape == PadShape::Circle {
            ANGLE_45
        } else {
            ANGLE_90
        }
    }

    /// Port of `PADSTACK::ThermalSpokeAngle( aLayer )`: the stored angle, else the default for
    /// the shape.
    pub fn thermal_spoke_angle(&self, layer: PcbLayerId) -> EdaAngle {
        if let Some(angle) = self.copper_props(layer).thermal_spoke_angle {
            return angle.angle();
        }

        self.default_thermal_spoke_angle_for_shape(layer)
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

    /// Port of `PADSTACK::AddPrimitive( aShape, aLayer )`: takes ownership of the shape (its
    /// parent is set by the caller, as in C++).
    pub fn add_primitive(&mut self, shape: PcbShape, layer: PcbLayerId) {
        self.copper_props_mut(layer).custom_shapes.push(shape);
    }

    /// Port of `PADSTACK::AppendPrimitives( aPrimitivesList, aLayer )`: copies (`Clone()`),
    /// re-parented to `parent` (the C++ `m_parent`).
    pub fn append_primitives(&mut self, list: &[PcbShape], layer: PcbLayerId, parent: ParentRef) {
        let target = &mut self.copper_props_mut(layer).custom_shapes;

        for item in list {
            let mut new_shape = item.detached_copy();
            new_shape.base.item.parent = parent;
            target.push(new_shape);
        }
    }

    /// Port of `PADSTACK::ReplacePrimitives( aPrimitivesList, aLayer )`.
    pub fn replace_primitives(&mut self, list: &[PcbShape], layer: PcbLayerId, parent: ParentRef) {
        self.clear_primitives(layer);
        self.append_primitives(list, layer, parent);
    }

    /// Port of `PADSTACK::ClearPrimitives( aLayer )`.
    pub fn clear_primitives(&mut self, layer: PcbLayerId) {
        self.copper_props_mut(layer).custom_shapes.clear();
    }

    // ---- backdrill --------------------------------------------------------------------------
    //
    // A backdrill's side is identified by its start layer (F_Cu = top, B_Cu = bottom), not by
    // which drill slot it occupies: reads accept the layout written by KiCad 10.0 (top
    // backdrill in the tertiary slot), and writes always stamp the start layer so a backdrill
    // saved here is read back correctly by older KiCad. Presence is keyed on the drill size.

    /// Port of `PADSTACK::findBackdrillDrill( aTop )`.
    fn find_backdrill_drill(&self, top: bool) -> Option<BackdrillSlot> {
        let side = if top {
            PcbLayerId::FCu
        } else {
            PcbLayerId::BCu
        };

        if self.secondary_drill.size.x > 0 && self.secondary_drill.start == side {
            return Some(BackdrillSlot::Secondary);
        }

        if self.tertiary_drill.size.x > 0 && self.tertiary_drill.start == side {
            return Some(BackdrillSlot::Tertiary);
        }

        None
    }

    fn backdrill_slot(&self, slot: BackdrillSlot) -> &DrillProps {
        match slot {
            BackdrillSlot::Secondary => &self.secondary_drill,
            BackdrillSlot::Tertiary => &self.tertiary_drill,
        }
    }

    fn backdrill_slot_mut(&mut self, slot: BackdrillSlot) -> &mut DrillProps {
        match slot {
            BackdrillSlot::Secondary => &mut self.secondary_drill,
            BackdrillSlot::Tertiary => &mut self.tertiary_drill,
        }
    }

    /// Port of `PADSTACK::backdrillWriteSlot( aTop )`.
    fn backdrill_write_slot(&self, top: bool) -> BackdrillSlot {
        if let Some(existing) = self.find_backdrill_drill(top) {
            return existing;
        }

        let (home, other) = if top {
            (BackdrillSlot::Secondary, BackdrillSlot::Tertiary)
        } else {
            (BackdrillSlot::Tertiary, BackdrillSlot::Secondary)
        };
        let other_side = if top {
            PcbLayerId::BCu
        } else {
            PcbLayerId::FCu
        };
        let home_drill = self.backdrill_slot(home);

        // Displace to the other slot only when the canonical home is already taken by the
        // opposite side, which happens when extending a backdrill on a board written by KiCad
        // 10.0.
        if home_drill.size.x > 0 && home_drill.start == other_side {
            return other;
        }

        home
    }

    /// Port of `PADSTACK::clearBackdrillSide( aTop )`.
    fn clear_backdrill_side(&mut self, top: bool) {
        let side = if top {
            PcbLayerId::FCu
        } else {
            PcbLayerId::BCu
        };

        if self.secondary_drill.start == side {
            self.secondary_drill.size = Vector2I::new(0, 0);
        }

        if self.tertiary_drill.start == side {
            self.tertiary_drill.size = Vector2I::new(0, 0);
        }
    }

    /// Port of `PADSTACK::GetBackdrillMode()`.
    pub fn backdrill_mode(&self) -> BackdrillMode {
        let has_top = self.find_backdrill_drill(true).is_some();
        let has_bottom = self.find_backdrill_drill(false).is_some();

        if has_top && has_bottom {
            return BackdrillMode::BackdrillBoth;
        }

        if has_top {
            return BackdrillMode::BackdrillTop;
        }

        if has_bottom {
            return BackdrillMode::BackdrillBottom;
        }

        BackdrillMode::NoBackdrill
    }

    /// Port of `PADSTACK::SetBackdrillMode( aMode )`.
    pub fn set_backdrill_mode(&mut self, mode: BackdrillMode) {
        let apply = |this: &mut Padstack, top: bool, want: bool| {
            if want {
                let main_size = this.drill.size;
                let slot = this.backdrill_write_slot(top);
                let drill = this.backdrill_slot_mut(slot);
                drill.start = if top {
                    PcbLayerId::FCu
                } else {
                    PcbLayerId::BCu
                };
                drill.shape = PadDrillShape::Circle;

                if drill.end == PcbLayerId::UndefinedLayer {
                    drill.end = drill.start; // Must-cut a single copper layer
                }

                if drill.size.x <= 0 {
                    // Backdrill slightly larger than main drill (VECTOR2I * double gives a
                    // VECTOR2D, converted back with clamping and truncation)
                    drill.size =
                        Vector2D::new(f64::from(main_size.x) * 1.1, f64::from(main_size.y) * 1.1)
                            .cast::<i32>();
                }
            } else {
                this.clear_backdrill_side(top);
            }
        };

        apply(
            self,
            true,
            mode == BackdrillMode::BackdrillTop || mode == BackdrillMode::BackdrillBoth,
        );
        apply(
            self,
            false,
            mode == BackdrillMode::BackdrillBottom || mode == BackdrillMode::BackdrillBoth,
        );
    }

    /// Port of `PADSTACK::GetBackdrillSize( aTop )`.
    pub fn backdrill_size(&self, top: bool) -> Option<i32> {
        self.find_backdrill_drill(top)
            .map(|slot| self.backdrill_slot(slot).size.x)
    }

    /// Port of `PADSTACK::SetBackdrillSize( aTop, aSize )`.
    pub fn set_backdrill_size(&mut self, top: bool, size: Option<i32>) {
        if let Some(size) = size {
            let slot = self.backdrill_write_slot(top);
            let target = self.backdrill_slot_mut(slot);
            target.size = Vector2I::new(size, size);
            target.shape = PadDrillShape::Circle;
            target.start = if top {
                PcbLayerId::FCu
            } else {
                PcbLayerId::BCu
            };
        } else {
            self.clear_backdrill_side(top);
        }
    }

    /// Port of `PADSTACK::GetBackdrillEndLayer( aTop )`.
    pub fn backdrill_end_layer(&self, top: bool) -> PcbLayerId {
        match self.find_backdrill_drill(top) {
            Some(slot) => self.backdrill_slot(slot).end,
            None => PcbLayerId::UndefinedLayer,
        }
    }

    /// Port of `PADSTACK::SetBackdrillEndLayer( aTop, aLayer )`.
    pub fn set_backdrill_end_layer(&mut self, top: bool, layer: PcbLayerId) {
        // A backdrill with no must-cut layer does not exist (matching the via layer
        // sanitizer), so clearing the must-cut clears the side rather than leaving a sizeful
        // drill with no end.
        if layer == PcbLayerId::UndefinedLayer {
            self.clear_backdrill_side(top);
            return;
        }

        let slot = self.backdrill_write_slot(top);
        let target = self.backdrill_slot_mut(slot);

        target.end = layer;
        target.start = if top {
            PcbLayerId::FCu
        } else {
            PcbLayerId::BCu
        };
    }
}

/// Which of the two backdrill-capable drill slots a `DRILL_PROPS*` of the backdrill helpers
/// points to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BackdrillSlot {
    Secondary,
    Tertiary,
}

/// Port of the file-static `effectiveDrillDiameter`: only an oblong drill's narrow axis is
/// cleared all the way round the hole.
fn effective_drill_diameter(size: Vector2I) -> i32 {
    if size.y > 0 {
        size.x.min(size.y)
    } else {
        size.x
    }
}

/// Port of the file-static `surfaceKnockout( aStackup, aProps, aSurface, aLayer )`.
fn surface_knockout(
    stackup: &BoardStackup,
    props: &PostMachiningProps,
    surface: PcbLayerId,
    layer: PcbLayerId,
) -> i32 {
    let Some(mode) = props.mode else {
        return 0;
    };

    if mode == PadDrillPostMachiningMode::NotPostMachined
        || mode == PadDrillPostMachiningMode::Unknown
        || props.size <= 0
    {
        return 0;
    }

    let tapered = mode == PadDrillPostMachiningMode::Countersink && props.angle > 0;
    let tan_half_angle = if tapered {
        cmath::tan((f64::from(props.angle) / 10.0) * std::f64::consts::PI / 360.0)
    } else {
        0.0
    };
    let mut depth = props.depth;

    // A countersink given no depth bottoms out where its taper closes on the drill
    if depth <= 0 && tapered {
        depth = cmath::f64_to_i32_x86((f64::from(props.size) / 2.0) / tan_half_angle);
    }

    if depth <= 0 {
        return 0;
    }

    let layer_dist = stackup.get_layer_distance(surface, layer);

    if layer_dist >= depth {
        return 0;
    }

    0.max(props.size.wrapping_sub(cmath::f64_to_i32_x86(
        2.0 * f64::from(layer_dist) * tan_half_angle,
    )))
}

impl ShapeProps {
    /// Port of `PADSTACK::SHAPE_PROPS::Compare`.
    pub fn compare(&self, other: &ShapeProps) -> i32 {
        cmp_test!(self.shape as i32, other.shape as i32);
        cmp_test!(self.anchor_shape as i32, other.anchor_shape as i32);
        cmp_test!(self.size.x, other.size.x);

        if self.shape != PadShape::Circle
            && (self.shape != PadShape::Custom || self.anchor_shape != PadShape::Circle)
        {
            cmp_test!(self.size.y, other.size.y);
        }

        cmp_test!(self.offset.x, other.offset.x);
        cmp_test!(self.offset.y, other.offset.y);

        if (self.round_rect_radius_ratio - other.round_rect_radius_ratio).abs() > 0.0001 {
            return if self.round_rect_radius_ratio > other.round_rect_radius_ratio {
                1
            } else {
                -1
            };
        }

        if (self.chamfered_rect_ratio - other.chamfered_rect_ratio).abs() > 0.0001 {
            return if self.chamfered_rect_ratio > other.chamfered_rect_ratio {
                1
            } else {
                -1
            };
        }

        cmp_test!(
            self.chamfered_rect_positions,
            other.chamfered_rect_positions
        );

        0
    }
}

impl CopperLayerProps {
    /// Port of `COPPER_LAYER_PROPS::operator==` (custom shapes compared by count only, as
    /// C++: "Deep compare of shapes? For now, just check pointers or size").
    pub fn eq_props(&self, other: &CopperLayerProps) -> bool {
        if self.shape != other.shape {
            return false;
        }

        if self.zone_connection != other.zone_connection {
            return false;
        }

        if self.thermal_spoke_width != other.thermal_spoke_width {
            return false;
        }

        if self.thermal_spoke_angle != other.thermal_spoke_angle {
            return false;
        }

        if self.thermal_gap != other.thermal_gap {
            return false;
        }

        if self.clearance != other.clearance {
            return false;
        }

        if self.custom_shapes.len() != other.custom_shapes.len() {
            return false;
        }

        true
    }

    /// Port of `COPPER_LAYER_PROPS::Similarity`. The C++ compares the primitive vectors of
    /// `shared_ptr` by pointer; distinct value lists are "different" unless both are empty.
    pub fn similarity(&self, other: &CopperLayerProps) -> f64 {
        let mut similarity = 1.0;

        if self.shape != other.shape {
            similarity *= 0.5;
        }

        if self.zone_connection != other.zone_connection {
            similarity *= 0.9;
        }

        if self.thermal_spoke_width != other.thermal_spoke_width {
            similarity *= 0.9;
        }

        if self.thermal_spoke_angle != other.thermal_spoke_angle {
            similarity *= 0.9;
        }

        if self.thermal_gap != other.thermal_gap {
            similarity *= 0.9;
        }

        if self.clearance != other.clearance {
            similarity *= 0.9;
        }

        if !(self.custom_shapes.is_empty() && other.custom_shapes.is_empty()) {
            similarity *= 0.5;
        }

        similarity
    }

    /// Port of `COPPER_LAYER_PROPS::Compare`.
    pub fn compare(&self, other: &CopperLayerProps) -> i32 {
        let diff = self.shape.compare(&other.shape);

        if diff != 0 {
            return diff;
        }

        test_opt!(
            self.zone_connection,
            other.zone_connection,
            ZoneConnection::None,
            |z: ZoneConnection| z as i32
        );
        test_opt!(
            self.thermal_spoke_width,
            other.thermal_spoke_width,
            0,
            |v: i32| v
        );

        // TEST_OPT_ORIENTATION( thermal_spoke_angle, aOther.thermal_spoke_angle, ANGLE_0 )
        {
            let a = self.thermal_spoke_angle;
            let b = other.thermal_spoke_angle;

            if a.is_some() != b.is_some() {
                return i32::from(a.is_some()) - i32::from(b.is_some());
            }

            let a_val = a.unwrap_or(EdaOrientation::new(ANGLE_0)).angle();
            let b_val = b.unwrap_or(EdaOrientation::new(ANGLE_0)).angle();

            if (a_val.as_degrees() - b_val.as_degrees()).abs() > 0.001 {
                return if a_val.as_degrees() > b_val.as_degrees() {
                    1
                } else {
                    -1
                };
            }
        }

        test_opt!(self.thermal_gap, other.thermal_gap, 0, |v: i32| v);
        test_opt!(self.clearance, other.clearance, 0, |v: i32| v);

        let diff = (self.custom_shapes.len() as i32).wrapping_sub(other.custom_shapes.len() as i32);

        if diff != 0 {
            return diff;
        }

        for (mine, theirs) in self.custom_shapes.iter().zip(&other.custom_shapes) {
            // custom_shapes[ii]->Compare( aOther.custom_shapes[ii].get() ): `EDA_SHAPE::Compare`
            let diff = mine.eda_shape().compare(theirs.eda_shape());

            if diff != 0 {
                return diff;
            }
        }

        0
    }
}

impl MaskLayerProps {
    /// Port of `MASK_LAYER_PROPS::Compare`.
    pub fn compare(&self, other: &MaskLayerProps) -> i32 {
        test_opt!(
            self.solder_mask_margin,
            other.solder_mask_margin,
            0,
            |v: i32| v
        );
        test_opt!(
            self.solder_paste_margin,
            other.solder_paste_margin,
            0,
            |v: i32| v
        );

        let a = self.solder_paste_margin_ratio;
        let b = other.solder_paste_margin_ratio;

        if a.is_some() != b.is_some() {
            return i32::from(a.is_some()) - i32::from(b.is_some());
        }

        if (a.unwrap_or(0.0) - b.unwrap_or(0.0)).abs() > 0.0001 {
            return if a.unwrap_or(0.0) > b.unwrap_or(0.0) {
                1
            } else {
                -1
            };
        }

        let bool_int = |v: bool| i32::from(v);
        test_opt!(self.has_solder_mask, other.has_solder_mask, false, bool_int);
        test_opt!(
            self.has_solder_paste,
            other.has_solder_paste,
            false,
            bool_int
        );
        test_opt!(self.has_covering, other.has_covering, false, bool_int);
        test_opt!(self.has_plugging, other.has_plugging, false, bool_int);

        0
    }
}

impl DrillProps {
    /// Port of `DRILL_PROPS::Compare`.
    pub fn compare(&self, other: &DrillProps) -> i32 {
        cmp_test!(self.shape as i32, other.shape as i32);

        cmp_test!(self.size.x, other.size.x);

        if self.shape != PadDrillShape::Circle {
            cmp_test!(self.size.y, other.size.y);
        }

        cmp_test!(self.start.0, other.start.0);
        cmp_test!(self.end.0, other.end.0);

        let bool_int = |v: bool| i32::from(v);
        test_opt!(self.is_filled, other.is_filled, false, bool_int);
        test_opt!(self.is_capped, other.is_capped, false, bool_int);

        0
    }
}

impl PostMachiningProps {
    /// Port of `POST_MACHINING_PROPS::Compare`.
    pub fn compare(&self, other: &PostMachiningProps) -> i32 {
        test_opt!(
            self.mode,
            other.mode,
            PadDrillPostMachiningMode::Unknown,
            |m: PadDrillPostMachiningMode| m as i32
        );
        cmp_test!(self.size, other.size);
        cmp_test!(self.depth, other.depth);
        cmp_test!(self.angle, other.angle);

        0
    }
}

// ---- m_copperProps: std::unordered_map<PCB_LAYER_ID, COPPER_LAYER_PROPS> (libstdc++) ---------

/// libstdc++ `__prime_list` (hashtable-aux.cc), up to the sizes a layer-keyed map can reach.
const LIBSTDCXX_PRIMES: [usize; 60] = [
    2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    103, 109, 113, 127, 137, 139, 149, 157, 167, 179, 193, 199, 211, 227, 241, 257, 277, 293, 313,
    337, 359, 383, 409, 439, 467, 503, 541, 577, 619, 661, 709, 761, 823, 887, 953,
];

/// `std::unordered_map<PCB_LAYER_ID, COPPER_LAYER_PROPS>` with the exact libstdc++ (g++ 13)
/// node order, so that iteration (`ForEachUniqueLayer` in CUSTOM mode, `begin()`) matches the
/// reference build.
///
/// Model: `std::hash` of an enum is the identity, bucket = `key % bucket_count`; a new node is
/// inserted at the beginning of its bucket, or at the front of the whole list when its bucket
/// is empty (`_M_insert_bucket_begin`); a rehash re-inserts every node in list order with the
/// same rule (`_M_rehash_aux`); the `_Prime_rehash_policy` (max load factor 1.0, first
/// allocation 11 -> 13 buckets) decides when. Copies keep the node order and the bucket count
/// (`_M_assign`), `clear()` keeps the bucket count and the resize threshold. The nodes of one
/// bucket are always adjacent, so the list is a plain vector.
#[derive(Clone, Debug)]
pub struct CopperPropsMap {
    /// The nodes in iteration order (`_M_before_begin._M_nxt` chain).
    nodes: Vec<(PcbLayerId, CopperLayerProps)>,
    /// `_M_bucket_count` (1 = the single bucket of an empty, never-filled map).
    bucket_count: usize,
    /// `_Prime_rehash_policy::_M_next_resize`.
    next_resize: usize,
}

impl Default for CopperPropsMap {
    fn default() -> Self {
        Self::new()
    }
}

impl CopperPropsMap {
    /// A default-constructed, empty map.
    pub fn new() -> Self {
        CopperPropsMap {
            nodes: Vec::new(),
            bucket_count: 1,
            next_resize: 0,
        }
    }

    /// `std::hash<PCB_LAYER_ID>` (the value as `size_t`) modulo the bucket count.
    fn bucket(key: PcbLayerId, bucket_count: usize) -> usize {
        (i64::from(key.0) as u64 % bucket_count as u64) as usize
    }

    /// `_Prime_rehash_policy::_M_next_bkt( n )`.
    fn next_bkt(&mut self, n: usize) -> usize {
        const FAST_BKT: [usize; 14] = [2, 2, 2, 3, 5, 5, 7, 7, 11, 11, 11, 11, 13, 13];

        if n < FAST_BKT.len() {
            if n == 0 {
                return 1;
            }

            self.next_resize = FAST_BKT[n];
            return FAST_BKT[n];
        }

        // lower_bound( __prime_list + 6, __last_prime, n )
        let next = LIBSTDCXX_PRIMES[6..]
            .iter()
            .copied()
            .find(|&p| p >= n)
            .expect("CopperPropsMap: more keys than layer ids");
        self.next_resize = next;
        next
    }

    /// `_Prime_rehash_policy::_M_need_rehash( n_bkt, n_elt, n_ins )` with one insertion.
    fn need_rehash(&mut self) -> Option<usize> {
        let n_bkt = self.bucket_count;
        let n_elt = self.nodes.len();

        if n_elt + 1 > self.next_resize {
            // If _M_next_resize is 0 it means that we have nothing allocated so far and that we
            // start inserting elements. In this case we start with an initial bucket size of
            // 11.
            let min_bkts = (n_elt + 1).max(if self.next_resize != 0 { 0 } else { 11 });

            if min_bkts >= n_bkt {
                return Some(self.next_bkt((min_bkts + 1).max(n_bkt * 2)));
            }

            self.next_resize = n_bkt;
            return None;
        }

        None
    }

    /// `_M_insert_bucket_begin`: before the first node of the bucket, else at the front.
    fn insert_bucket_begin(
        nodes: &mut Vec<(PcbLayerId, CopperLayerProps)>,
        bucket_count: usize,
        key: PcbLayerId,
        value: CopperLayerProps,
    ) -> usize {
        let bkt = Self::bucket(key, bucket_count);
        let pos = nodes
            .iter()
            .position(|(k, _)| Self::bucket(*k, bucket_count) == bkt)
            .unwrap_or(0);
        nodes.insert(pos, (key, value));
        pos
    }

    /// `_M_insert_unique_node`: rehash if the policy asks for it, then insert.
    fn insert_unique_node(&mut self, key: PcbLayerId, value: CopperLayerProps) -> usize {
        if let Some(new_count) = self.need_rehash() {
            let old = std::mem::take(&mut self.nodes);

            for (k, v) in old {
                Self::insert_bucket_begin(&mut self.nodes, new_count, k, v);
            }

            self.bucket_count = new_count;
        }

        Self::insert_bucket_begin(&mut self.nodes, self.bucket_count, key, value)
    }

    fn position(&self, key: &PcbLayerId) -> Option<usize> {
        self.nodes.iter().position(|(k, _)| k == key)
    }

    /// `count( key ) > 0` / `contains( key )`.
    pub fn contains_key(&self, key: &PcbLayerId) -> bool {
        self.position(key).is_some()
    }

    /// `find( key )`.
    pub fn get(&self, key: &PcbLayerId) -> Option<&CopperLayerProps> {
        self.position(key).map(|i| &self.nodes[i].1)
    }

    /// Mutable `find( key )`.
    pub fn get_mut(&mut self, key: &PcbLayerId) -> Option<&mut CopperLayerProps> {
        self.position(key).map(|i| &mut self.nodes[i].1)
    }

    /// `operator[]( key )`: the entry, inserted with default props when missing.
    pub fn index_or_default(&mut self, key: PcbLayerId) -> &mut CopperLayerProps {
        let i = match self.position(&key) {
            Some(i) => i,
            None => self.insert_unique_node(key, CopperLayerProps::default()),
        };

        &mut self.nodes[i].1
    }

    /// `emplace( key, value ).first->second`: an existing entry is kept unchanged.
    pub fn emplace(&mut self, key: PcbLayerId, value: CopperLayerProps) -> &mut CopperLayerProps {
        let i = match self.position(&key) {
            Some(i) => i,
            None => self.insert_unique_node(key, value),
        };

        &mut self.nodes[i].1
    }

    /// `clear()`: keeps the bucket count and the rehash policy state, as libstdc++.
    pub fn clear(&mut self) {
        self.nodes.clear();
    }

    /// `size()`.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// `empty()`.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// `*begin()`.
    pub fn first(&self) -> Option<(&PcbLayerId, &CopperLayerProps)> {
        self.nodes.first().map(|(k, v)| (k, v))
    }

    /// Iteration in libstdc++ node order.
    pub fn iter(&self) -> impl Iterator<Item = (&PcbLayerId, &CopperLayerProps)> {
        self.nodes.iter().map(|(k, v)| (k, v))
    }

    /// The keys in iteration order.
    pub fn keys(&self) -> impl Iterator<Item = &PcbLayerId> {
        self.nodes.iter().map(|(k, _)| k)
    }
}

impl Index<&PcbLayerId> for CopperPropsMap {
    type Output = CopperLayerProps;

    /// `at( key )`: panics when missing (C++ throws `std::out_of_range`).
    fn index(&self, key: &PcbLayerId) -> &CopperLayerProps {
        self.get(key).expect("CopperPropsMap: no entry for key")
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

    // ---- CopperPropsMap: libstdc++ unordered_map order ---------------------------------------

    fn keys_of(m: &CopperPropsMap) -> Vec<i32> {
        m.keys().map(|k| k.0).collect()
    }

    /// Golden orders printed by g++ 13 / libstdc++ for `std::unordered_map<enum, int>` with the
    /// same operations (`m[k]` inserts, copy, `clear()`); driver kept in the PORT_STATUS notes.
    #[test]
    fn copper_props_map_matches_libstdcxx_order() {
        let mut m = CopperPropsMap::new();

        for k in [0, 4, 6, 2] {
            m.index_or_default(PcbLayerId(k));
        }

        assert_eq!(keys_of(&m), vec![2, 6, 4, 0]);
        assert_eq!(m.bucket_count, 13);

        let mut m2 = CopperPropsMap::new();
        m2.index_or_default(PcbLayerId(0));

        for k in 1..=31 {
            m2.index_or_default(PcbLayerId(2 * k + 2));
        }

        assert_eq!(
            keys_of(&m2),
            vec![
                64, 62, 60, 24, 22, 20, 18, 16, 14, 12, 10, 8, 6, 4, 26, 0, 58, 28, 30, 32, 34, 36,
                38, 40, 42, 44, 46, 48, 50, 52, 54, 56
            ]
        );
        assert_eq!(m2.bucket_count, 59);

        let old = m2.clone();
        m2.clear();

        for (k, _) in old.iter() {
            let flipped = match k.0 {
                0 => 2,
                2 => 0,
                other => other,
            };
            m2.index_or_default(PcbLayerId(flipped));
        }

        assert_eq!(
            keys_of(&m2),
            vec![
                56, 54, 52, 50, 48, 46, 44, 42, 40, 38, 36, 34, 32, 30, 28, 58, 2, 26, 4, 6, 8, 10,
                12, 14, 16, 18, 20, 22, 24, 60, 62, 64
            ]
        );
        assert_eq!(m2.bucket_count, 59);

        let mut m3 = CopperPropsMap::new();

        for k in [0, 26, 2, 52, 13, 4, 39] {
            m3.index_or_default(PcbLayerId(k));
        }

        assert_eq!(keys_of(&m3), vec![4, 2, 39, 13, 52, 26, 0]);
    }

    /// The parser's padstack sequence for pad 05e66fff-73b7-4418-9e88-371b772d1746 of
    /// `qa/data/pcbnew/padstacks_complex.kicad_pcb` (CUSTOM, layers F.Cu, In1.Cu, In2.Cu,
    /// B.Cu, each first written through `SetLibOffset`): oracle `pcb-dump board --sections
    /// pad` gives `padstack.unique_layers=[B.Cu,In2.Cu,In1.Cu,F.Cu]`, In1.Cu
    /// `round_rect_radius=254000`, `thermal_spoke_angle=45`, anchor CIRCLE inherited from the
    /// ALL_LAYERS entry.
    #[test]
    fn custom_padstack_unique_layer_order_matches_oracle() {
        let mut p = Padstack::default();
        // the PAD constructor's ALL_LAYERS values that matter here
        p.set_anchor_shape(PadShape::Circle, Padstack::ALL_LAYERS);
        p.set_round_rect_radius_ratio(0.25, Padstack::ALL_LAYERS);
        p.set_mode(PadstackMode::Custom);

        let in1 = PcbLayerId::In1Cu;
        let in2 = PcbLayerId(PcbLayerId::In1Cu.0 + 2);

        for (layer, size) in [
            (PcbLayerId::FCu, 762_000),
            (in1, 1_016_000),
            (in2, 1_270_000),
            (PcbLayerId::BCu, 1_524_000),
        ] {
            p.set_offset(Vector2I::new(0, 0), layer);
            *p.trapezoid_delta_size_mut(layer) = Vector2I::new(0, 0);
            p.set_shape(PadShape::Rectangle, layer);
            p.set_size(Vector2I::new(size, size), layer);
        }

        assert_eq!(
            p.unique_layers(),
            vec![PcbLayerId::BCu, in2, in1, PcbLayerId::FCu]
        );
        assert_eq!(p.round_rect_radius(in1), 254_000);
        assert_eq!(p.round_rect_radius(PcbLayerId::BCu), 381_000);
        assert_eq!(p.anchor_shape(in1), PadShape::Circle);
        assert_eq!(p.thermal_spoke_angle(in1), ANGLE_45);
        assert_eq!(p.zone_connection(in1), Some(ZoneConnection::Inherited));
        assert_eq!(p.size(PcbLayerId::FMask), Vector2I::new(762_000, 762_000));
        assert_eq!(p.effective_layer_for(PcbLayerId::BMask), PcbLayerId::BCu);
        assert_eq!(p.effective_layer_for(in2), in2);
        assert_eq!(p.max_hole_size(), 0);
    }

    // ---- qa/tests/pcbnew/test_padstack_clone.cpp (padstack part of each case) ---------------

    /// `InnerLayerKeepsInheritedSize`: a front/inner/back padstack without an inner entry
    /// reads the inner layer through the base entry, and a copy keeps that.
    #[test]
    fn inner_layer_keeps_inherited_size() {
        let mut p = Padstack::default();
        p.set_size(Vector2I::new(1_500_000, 1_200_000), PcbLayerId::FCu);
        p.set_shape(PadShape::Oval, PcbLayerId::FCu);
        p.set_mode(PadstackMode::FrontInnerBack);

        assert_eq!(p.size(PcbLayerId::In1Cu).x, 1_500_000);

        let mut clone = Padstack::default();
        clone.assign_from(&p, ParentRef::None);

        assert_eq!(clone.size(PcbLayerId::In1Cu).x, 1_500_000);
        assert_eq!(clone.size(PcbLayerId::In1Cu).y, 1_200_000);
        assert_eq!(clone.shape(PcbLayerId::In1Cu), PadShape::Oval);
    }

    /// `NewLayerOwnsItsPrimitives`: writing the back layer seeds it from the base entry with
    /// its own copies of the primitives.
    #[test]
    fn new_layer_owns_its_primitives() {
        let mut p = Padstack::default();
        p.add_primitive(PcbShape::default(), PcbLayerId::FCu);
        p.set_mode(PadstackMode::FrontInnerBack);

        // writing the back layer seeds it from the base entry
        p.set_size(Vector2I::new(1_000_000, 1_000_000), PcbLayerId::BCu);

        assert_eq!(p.primitives(PcbLayerId::BCu).len(), 1);
        // (values, so distinct objects by construction; the F_Cu entry is unchanged)
        assert_eq!(p.primitives(PcbLayerId::FCu).len(), 1);
        assert!(p.has_explicit_definition_for_layer(PcbLayerId::BCu));
    }

    /// `CustomLayerEditKeepsInheritedSize`: after a flip renamed the only CUSTOM entry to
    /// B_Cu, editing F_Cu starts from what F_Cu read through to (`begin()`).
    #[test]
    fn custom_layer_edit_keeps_inherited_size() {
        let mut p = Padstack::default();
        p.set_size(Vector2I::new(1_500_000, 1_200_000), PcbLayerId::FCu);
        p.set_mode(PadstackMode::Custom);
        p.flip_layers(&LayerEnv::no_board());

        assert!(!p.has_explicit_definition_for_layer(PcbLayerId::FCu));
        assert_eq!(p.effective_layer_for(PcbLayerId::FCu), PcbLayerId::BCu);

        p.set_shape(PadShape::Oval, PcbLayerId::FCu);

        assert_eq!(p.size(PcbLayerId::FCu).x, 1_500_000);
        assert_eq!(p.size(PcbLayerId::FCu).y, 1_200_000);
    }

    // ---- qa/tests/pcbnew/test_backdrill_properties.cpp --------------------------------------

    fn mm(v: f64) -> i32 {
        kicad_core::PCB_IU_SCALE.mm_to_iu(v)
    }

    fn circle_drill(size: f64, start: PcbLayerId, end: PcbLayerId) -> DrillProps {
        DrillProps {
            size: Vector2I::new(mm(size), mm(size)),
            start,
            end,
            shape: PadDrillShape::Circle,
            ..DrillProps::default()
        }
    }

    const IN2: PcbLayerId = PcbLayerId(PcbLayerId::In1Cu.0 + 2);

    #[test]
    fn top_backdrill_uses_secondary_drill() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));

        stack.set_backdrill_mode(BackdrillMode::BackdrillTop);
        stack.set_backdrill_size(true, Some(mm(0.7)));
        stack.set_backdrill_end_layer(true, PcbLayerId::In1Cu);

        assert!(stack.secondary_drill().size.x > 0);
        assert_eq!(stack.secondary_drill().start, PcbLayerId::FCu);
        assert_eq!(stack.secondary_drill().end, PcbLayerId::In1Cu);
        assert_eq!(stack.tertiary_drill().size.x, 0);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillTop);
        assert_eq!(stack.backdrill_size(true).unwrap_or(0), mm(0.7));
        assert_eq!(stack.backdrill_end_layer(true), PcbLayerId::In1Cu);
    }

    #[test]
    fn bottom_backdrill_uses_tertiary_drill() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));

        stack.set_backdrill_mode(BackdrillMode::BackdrillBottom);
        stack.set_backdrill_size(false, Some(mm(0.7)));
        stack.set_backdrill_end_layer(false, IN2);

        assert!(stack.tertiary_drill().size.x > 0);
        assert_eq!(stack.tertiary_drill().start, PcbLayerId::BCu);
        assert_eq!(stack.tertiary_drill().end, IN2);
        assert_eq!(stack.secondary_drill().size.x, 0);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillBottom);
        assert_eq!(stack.backdrill_size(false).unwrap_or(0), mm(0.7));
        assert_eq!(stack.backdrill_end_layer(false), IN2);
    }

    #[test]
    fn backdrill_mode_default_size_is_main_drill_times_1_1() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));
        stack.set_backdrill_mode(BackdrillMode::BackdrillTop);
        // 400000 * 1.1 = 440000.00000000006, truncated
        assert_eq!(
            stack.secondary_drill().size,
            Vector2I::new(440_000, 440_000)
        );
        assert_eq!(stack.secondary_drill().end, PcbLayerId::FCu);
    }

    #[test]
    fn dialog_to_panel_round_trip() {
        let mut stack = Padstack::default();
        *stack.secondary_drill_mut() = circle_drill(0.9, PcbLayerId::FCu, PcbLayerId::In1Cu);
        *stack.tertiary_drill_mut() = circle_drill(0.7, PcbLayerId::BCu, IN2);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillBoth);
        assert_eq!(stack.backdrill_size(true).unwrap_or(0), mm(0.9));
        assert_eq!(stack.backdrill_end_layer(true), PcbLayerId::In1Cu);
        assert_eq!(stack.backdrill_size(false).unwrap_or(0), mm(0.7));
        assert_eq!(stack.backdrill_end_layer(false), IN2);
    }

    #[test]
    fn editing_size_preserves_slot_and_value() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));

        stack.set_backdrill_mode(BackdrillMode::BackdrillBottom);
        stack.set_backdrill_size(false, Some(mm(0.7)));
        stack.set_backdrill_end_layer(false, IN2);

        stack.set_backdrill_size(false, Some(mm(0.8)));

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillBottom);
        assert_eq!(stack.backdrill_size(false).unwrap_or(0), mm(0.8));
        assert_eq!(stack.backdrill_end_layer(false), IN2);
        assert_eq!(stack.secondary_drill().size.x, 0);
    }

    #[test]
    fn clearing_mode_removes_backdrill() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));

        stack.set_backdrill_mode(BackdrillMode::BackdrillTop);
        stack.set_backdrill_size(true, Some(mm(0.7)));

        stack.set_backdrill_mode(BackdrillMode::NoBackdrill);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::NoBackdrill);
        assert_eq!(stack.secondary_drill().size.x, 0);
        assert_eq!(stack.tertiary_drill().size.x, 0);
    }

    #[test]
    fn reads_legacy_v10_slot_layout() {
        let mut stack = Padstack::default();
        *stack.secondary_drill_mut() = circle_drill(0.8, PcbLayerId::BCu, IN2);
        *stack.tertiary_drill_mut() = circle_drill(0.6, PcbLayerId::FCu, PcbLayerId::In1Cu);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillBoth);
        assert_eq!(stack.backdrill_size(true).unwrap_or(0), mm(0.6));
        assert_eq!(stack.backdrill_end_layer(true), PcbLayerId::In1Cu);
        assert_eq!(stack.backdrill_size(false).unwrap_or(0), mm(0.8));
        assert_eq!(stack.backdrill_end_layer(false), IN2);
    }

    #[test]
    fn reads_legacy_v10_top_only() {
        let mut stack = Padstack::default();
        *stack.tertiary_drill_mut() = circle_drill(0.6, PcbLayerId::FCu, PcbLayerId::In1Cu);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillTop);
        assert_eq!(stack.backdrill_size(true).unwrap_or(0), mm(0.6));
        assert_eq!(stack.backdrill_end_layer(true), PcbLayerId::In1Cu);
        assert!(stack.backdrill_size(false).is_none());
    }

    #[test]
    fn editing_legacy_backdrill_stays_in_place_and_readable() {
        let mut stack = Padstack::default();
        *stack.tertiary_drill_mut() = circle_drill(0.6, PcbLayerId::FCu, PcbLayerId::In1Cu);

        stack.set_backdrill_size(true, Some(mm(0.75)));

        assert_eq!(stack.tertiary_drill().size.x, mm(0.75));
        assert_eq!(stack.tertiary_drill().start, PcbLayerId::FCu);
        assert_eq!(stack.secondary_drill().size.x, 0);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::BackdrillTop);
        assert_eq!(stack.backdrill_size(true).unwrap_or(0), mm(0.75));
    }

    #[test]
    fn clearing_must_cut_removes_backdrill() {
        let mut stack = Padstack::default();
        stack.drill_mut().size = Vector2I::new(mm(0.4), mm(0.4));

        stack.set_backdrill_mode(BackdrillMode::BackdrillTop);
        stack.set_backdrill_size(true, Some(mm(0.7)));
        stack.set_backdrill_end_layer(true, PcbLayerId::In1Cu);

        stack.set_backdrill_end_layer(true, PcbLayerId::UndefinedLayer);

        assert_eq!(stack.backdrill_mode(), BackdrillMode::NoBackdrill);
        assert!(stack.backdrill_size(true).is_none());
        assert_eq!(stack.secondary_drill().size.x, 0);
    }

    #[test]
    fn clearing_collapses_duplicate_side_slots() {
        let mut stack = Padstack::default();
        *stack.secondary_drill_mut() = circle_drill(0.7, PcbLayerId::FCu, PcbLayerId::In1Cu);
        *stack.tertiary_drill_mut() = circle_drill(0.6, PcbLayerId::FCu, PcbLayerId::In1Cu);

        stack.set_backdrill_size(true, None);

        assert_eq!(stack.secondary_drill().size.x, 0);
        assert_eq!(stack.tertiary_drill().size.x, 0);
        assert_eq!(stack.backdrill_mode(), BackdrillMode::NoBackdrill);
    }

    // ---- mode logic, compare, flip -----------------------------------------------------------

    #[test]
    fn front_inner_back_reads_and_flip() {
        let mut p = Padstack::default();
        p.set_mode(PadstackMode::FrontInnerBack);
        p.set_size(Vector2I::new(100, 100), PcbLayerId::FCu);
        p.set_size(Vector2I::new(200, 200), PcbLayerId::In1Cu);
        p.set_size(Vector2I::new(300, 300), PcbLayerId::BCu);
        p.front_outer_layers_mut().has_solder_mask = Some(true);

        assert_eq!(p.size(PcbLayerId::FMask), Vector2I::new(100, 100));
        assert_eq!(p.size(IN2), Vector2I::new(200, 200));
        assert_eq!(p.effective_layer_for(IN2), Padstack::INNER_LAYERS);
        assert_eq!(p.effective_layer_for(PcbLayerId::BPaste), PcbLayerId::BCu);
        assert_eq!(p.is_tented(PcbLayerId::FCu), Some(true));
        assert_eq!(p.is_tented(PcbLayerId::In1Cu), None);

        let before = p.clone();
        p.flip_layers(&LayerEnv::no_board());

        assert_eq!(p.size(PcbLayerId::FCu), Vector2I::new(300, 300));
        assert_eq!(p.size(PcbLayerId::BCu), Vector2I::new(100, 100));
        assert_eq!(p.is_tented(PcbLayerId::BCu), Some(true));
        assert_eq!(
            (p.drill.start, p.drill.end),
            (PcbLayerId::BCu, PcbLayerId::FCu)
        );
        assert!(!p.eq_padstack(&before));
        assert_ne!(Padstack::compare(&p, &before), 0);

        p.flip_layers(&LayerEnv::no_board());
        assert!(p.eq_padstack(&before));
        assert_eq!(Padstack::compare(&p, &before), 0);
    }

    #[test]
    fn compare_and_relevant_layers() {
        let a = Padstack::default();
        let mut b = Padstack::default();
        assert_eq!(Padstack::compare(&a, &b), 0);

        b.set_size(Vector2I::new(10, 20), PcbLayerId::FCu);
        // SHAPE_PROPS::Compare: size.x difference (0 - 10)
        assert_eq!(Padstack::compare(&a, &b), -10);

        let mut c = Padstack::default();
        c.set_mode(PadstackMode::Custom);
        assert_eq!(Padstack::compare(&c, &a), 2);
        c.set_size(Vector2I::new(1, 1), PcbLayerId::In1Cu);
        let layers = a.relevant_shape_layers(&c);
        assert!(layers.contains(PcbLayerId::FCu) && layers.contains(PcbLayerId::In1Cu));
        assert_eq!(layers.count(), 2);

        // A custom name on one side only is not a difference for operator== (C++ compares
        // the member with itself), but Compare sees it.
        let mut d = Padstack::default();
        d.set_custom_name("x");
        assert!(d.eq_padstack(&a));
        assert_eq!(Padstack::compare(&d, &a), 1);
    }

    #[test]
    fn assign_from_turns_zero_radius_roundrect_into_rect() {
        let mut src = Padstack::default();
        src.set_shape(PadShape::Roundrect, PcbLayerId::FCu);
        let mut dst = Padstack::default();
        dst.assign_from(&src, ParentRef::None);
        assert_eq!(dst.shape(PcbLayerId::FCu), PadShape::Rectangle);
        assert_eq!(src.shape(PcbLayerId::FCu), PadShape::Roundrect);
    }

    #[test]
    fn thermal_angle_and_mask_accessors() {
        let mut p = Padstack::default();
        p.copper_props_mut(PcbLayerId::FCu).thermal_spoke_angle = None;
        p.set_shape(PadShape::Rectangle, PcbLayerId::FCu);
        assert_eq!(p.thermal_spoke_angle(PcbLayerId::FCu), ANGLE_90);
        p.set_shape(PadShape::Custom, PcbLayerId::FCu);
        p.set_anchor_shape(PadShape::Circle, PcbLayerId::FCu);
        assert_eq!(p.thermal_spoke_angle(PcbLayerId::FCu), ANGLE_45);

        *p.solder_mask_margin_mut(PcbLayerId::BMask) = Some(5);
        assert_eq!(p.solder_mask_margin(PcbLayerId::BCu), Some(5));
        assert_eq!(p.solder_mask_margin(PcbLayerId::In1Cu), None);
        *p.solder_paste_margin_ratio_mut(PcbLayerId::In1Cu) = Some(0.5);
        assert_eq!(p.solder_paste_margin_ratio(PcbLayerId::FPaste), Some(0.5));

        p.set_size(Vector2I::new(-5, 40), PcbLayerId::FCu);
        assert_eq!(p.size(PcbLayerId::FCu), Vector2I::new(0, 40));
        p.set_round_rect_radius(10.0, PcbLayerId::FCu);
        assert_eq!(p.round_rect_radius_ratio(PcbLayerId::FCu), 0.0);
        p.set_size(Vector2I::new(40, 80), PcbLayerId::FCu);
        p.set_round_rect_radius(10.0, PcbLayerId::FCu);
        assert_eq!(p.round_rect_radius_ratio(PcbLayerId::FCu), 0.25);
        assert_eq!(p.round_rect_radius(PcbLayerId::FCu), 10);
    }

    #[test]
    fn max_hole_size_counts_unset_post_machining() {
        let mut p = Padstack::default();
        p.drill.size = Vector2I::new(100, 100);
        p.front_post_machining.size = 300;
        // mode unset compares unequal to NOT_POST_MACHINED and UNKNOWN
        assert_eq!(p.max_hole_size(), 300);
        p.front_post_machining.mode = Some(PadDrillPostMachiningMode::NotPostMachined);
        assert_eq!(p.max_hole_size(), 100);
        *p.secondary_drill_mut() = DrillProps {
            size: Vector2I::new(500, 500),
            start: PcbLayerId::FCu,
            end: PcbLayerId::In1Cu,
            ..DrillProps::default()
        };
        assert_eq!(p.max_hole_size(), 500);
    }
}
