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

//! Port of `common/properties/property_mgr.cpp`, `include/properties/property_mgr.h` and
//! `include/properties/property.h` (the parts that are not wxPropertyGrid UI), plus the
//! `*_DESC` registrations of the board item classes (one module per C++ item family).
//!
//! The custom DRC rule language reads item properties by their registered English names
//! (`A.Net_Class`, `A.Pad_Type == 'SMD'`), so the tables below are part of the rule-file
//! contract. Model:
//!
//! * [`ClassId`] stands for a C++ `TYPE_HASH( class )`; [`class_of`] is `TYPE_HASH( *item )`.
//! * [`Property`] is a `PROPERTY` / `PROPERTY_ENUM`: name, owner and base class, value type,
//!   display, coordinate type, group, the availability / writeability predicates and the getter
//!   (`fn( ItemRef ) -> PropertyValue`, calling the model method the C++ getter calls). Setters
//!   are recorded as `has_setter` only (tier 3).
//! * [`PropertyManager`] is `PROPERTY_MANAGER`: registration (`AddProperty`, `ReplaceProperty`,
//!   `InheritsAfter`, `Mask`, `OverrideAvailability`, `OverrideWriteability`, `AddTypeCast`),
//!   the `CLASS_DESC::rebuild` / `collectPropsRecur` algorithm, and the lookups.
//! * [`enums`] holds every `ENUM_MAP` a property uses.
//!
//! The tables are built once ([`property_manager`]), after all registrations, which is what
//! the C++ lazy `Rebuild()` amounts to.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use kicad_core::color4d::Color4D;
use kicad_geom::EdaAngle;

use crate::item_ref::{AnyItem, ItemRef};
use kicad_core::KicadT;

pub mod board_item;
pub mod eda_item;
pub mod eda_shape;
pub mod eda_text;
pub mod enums;
pub mod footprint;
pub mod generators;
pub mod misc;
pub mod pad;
pub mod pcb_dimension;
pub mod pcb_shape;
pub mod pcb_table;
pub mod pcb_text;
pub mod pcb_track;
pub mod zone;

#[cfg(test)]
mod tests;

pub use enums::{EnumId, EnumMap};

/// A `PCB_LAYER_ID` property value.
pub(crate) fn layer_value(layer: kicad_core::PcbLayerId) -> PropertyValue {
    PropertyValue::Enum(EnumId::PcbLayerId, i32::from(layer))
}

/// Panics like `port_pending!` for a C++ getter the Rust model has no method for yet (reported
/// as pending by the dump; the owning package has to add the method and wire it up here).
pub(crate) fn no_rust_api(cxx: &str) -> ! {
    panic!("kicad-pcb: port pending: {cxx} (no Rust API)")
}

macro_rules! class_ids {
    ( $( $id:ident = $cxx:literal, $registered:expr; )* ) => {
        /// A C++ class known to the property system (`TYPE_HASH( class )`).
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub enum ClassId {
            $( #[doc = concat!("`", $cxx, "`.")] $id, )*
        }

        impl ClassId {
            /// Every class, in declaration order.
            pub const ALL: &'static [ClassId] = &[ $( ClassId::$id, )* ];

            /// The C++ class name.
            pub fn cxx_name(self) -> &'static str {
                match self {
                    $( ClassId::$id => $cxx, )*
                }
            }

            /// Whether the C++ registers the type (`REGISTER_TYPE`); the others exist only as
            /// a base or an owner (`getClass` creates them).
            fn is_registered(self) -> bool {
                match self {
                    $( ClassId::$id => $registered, )*
                }
            }
        }
    };
}

class_ids! {
    EdaItem = "EDA_ITEM", true;
    BoardItem = "BOARD_ITEM", true;
    BoardConnectedItem = "BOARD_CONNECTED_ITEM", true;
    BoardItemContainer = "BOARD_ITEM_CONTAINER", false;
    EdaShape = "EDA_SHAPE", true;
    EdaText = "EDA_TEXT", true;
    EdaGroup = "EDA_GROUP", false;
    MarkerBase = "MARKER_BASE", false;
    Pad = "PAD", true;
    PcbTrack = "PCB_TRACK", true;
    PcbArc = "PCB_ARC", true;
    PcbVia = "PCB_VIA", true;
    Zone = "ZONE", true;
    Footprint = "FOOTPRINT", true;
    PcbShape = "PCB_SHAPE", true;
    PcbText = "PCB_TEXT", true;
    PcbTextbox = "PCB_TEXTBOX", true;
    PcbField = "PCB_FIELD", true;
    PcbDimensionBase = "PCB_DIMENSION_BASE", true;
    PcbDimAligned = "PCB_DIM_ALIGNED", true;
    PcbDimOrthogonal = "PCB_DIM_ORTHOGONAL", true;
    PcbDimRadial = "PCB_DIM_RADIAL", true;
    PcbDimLeader = "PCB_DIM_LEADER", true;
    PcbDimCenter = "PCB_DIM_CENTER", true;
    PcbGroup = "PCB_GROUP", true;
    // PCB_GENERATOR_DESC is compiled only `#ifdef GENERATOR_ORDER` (not defined): the class
    // exists as a base of the generator subclasses only.
    PcbGenerator = "PCB_GENERATOR", false;
    PcbTable = "PCB_TABLE", true;
    PcbTablecell = "PCB_TABLECELL", true;
    PcbBarcode = "PCB_BARCODE", true;
    PcbReferenceImage = "PCB_REFERENCE_IMAGE", true;
    Pcb3dBody = "PCB_3D_BODY", true;
    PcbDrillChart = "PCB_DRILL_CHART", true;
    PcbDrillMap = "PCB_DRILL_MAP", true;
    PcbGeneratedTable = "PCB_GENERATED_TABLE", true;
    PcbGridItem = "PCB_GRID_ITEM", true;
    PcbMarker = "PCB_MARKER", true;
    PcbPoint = "PCB_POINT", true;
    PcbStackupTable = "PCB_STACKUP_TABLE", true;
    PcbTarget = "PCB_TARGET", true;
    PcbConstraint = "PCB_CONSTRAINT", true;
    PcbTuningPattern = "PCB_TUNING_PATTERN", true;
    PcbViaStack = "PCB_VIA_STACK", true;
    PcbViaStitch = "PCB_VIA_STITCH", true;
}

impl ClassId {
    /// `TYPE_NAME( class )` as `REGISTER_TYPE` stores it (`typeid( x ).name()`, the Itanium
    /// ABI mangled name, e.g. `3PAD`); `None` for a class that is not registered.
    pub fn registered_name(self) -> Option<String> {
        self.is_registered()
            .then(|| format!("{}{}", self.cxx_name().len(), self.cxx_name()))
    }
}

/// `TYPE_HASH( *aItem )`: the most derived C++ class of a board item; `None` for a net
/// (`NETINFO_ITEM` registers no properties).
pub fn class_of(item: ItemRef<'_>) -> Option<ClassId> {
    use crate::pcb_generator::{
        GENERATOR_TYPE_TUNING_PATTERN, GENERATOR_TYPE_VIA_STACK, GENERATOR_TYPE_VIA_STITCH,
    };

    Some(match item.any() {
        AnyItem::Footprint(_) => ClassId::Footprint,
        AnyItem::Pad(_) => ClassId::Pad,
        AnyItem::Track(_) => match item.type_() {
            KicadT::PcbArcT => ClassId::PcbArc,
            KicadT::PcbViaT => ClassId::PcbVia,
            _ => ClassId::PcbTrack,
        },
        AnyItem::Zone(_) => ClassId::Zone,
        AnyItem::Shape(_) => ClassId::PcbShape,
        AnyItem::Text(_) => ClassId::PcbText,
        AnyItem::Field(_) => ClassId::PcbField,
        AnyItem::Textbox(_) => ClassId::PcbTextbox,
        AnyItem::Table(_) => match item.type_() {
            KicadT::PcbDrillChartT => ClassId::PcbDrillChart,
            KicadT::PcbStackupTableT => ClassId::PcbStackupTable,
            _ => ClassId::PcbTable,
        },
        AnyItem::Tablecell(_) => ClassId::PcbTablecell,
        AnyItem::Dimension(_) => match item.type_() {
            KicadT::PcbDimOrthogonalT => ClassId::PcbDimOrthogonal,
            KicadT::PcbDimRadialT => ClassId::PcbDimRadial,
            KicadT::PcbDimLeaderT => ClassId::PcbDimLeader,
            KicadT::PcbDimCenterT => ClassId::PcbDimCenter,
            _ => ClassId::PcbDimAligned,
        },
        AnyItem::Barcode(_) => ClassId::PcbBarcode,
        AnyItem::Image(_) => ClassId::PcbReferenceImage,
        AnyItem::Target(_) => ClassId::PcbTarget,
        AnyItem::Point(_) => ClassId::PcbPoint,
        AnyItem::Group(_) => ClassId::PcbGroup,
        AnyItem::Generator(g) => match g.generator_type.as_str() {
            GENERATOR_TYPE_TUNING_PATTERN => ClassId::PcbTuningPattern,
            GENERATOR_TYPE_VIA_STACK => ClassId::PcbViaStack,
            GENERATOR_TYPE_VIA_STITCH => ClassId::PcbViaStitch,
            _ => ClassId::PcbGenerator,
        },
        AnyItem::Marker(_) => ClassId::PcbMarker,
        AnyItem::Constraint(_) => ClassId::PcbConstraint,
        AnyItem::Body3d(_) => ClassId::Pcb3dBody,
        AnyItem::GridItem(_) => ClassId::PcbGridItem,
        AnyItem::DrillMap(_) => ClassId::PcbDrillMap,
        AnyItem::Net(_) => return None,
    })
}

/// `PROPERTY_DISPLAY`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertyDisplay {
    /// Default property for a given type.
    PtDefault,
    /// Size expressed in distance units (mm/inch).
    PtSize,
    /// Area expressed in distance units-squared (mm/inch).
    PtArea,
    /// Coordinate expressed in distance units (mm/inch).
    PtCoord,
    /// Angle expressed in degrees.
    PtDegree,
    /// Angle expressed in decidegrees.
    PtDecidegree,
    PtRatio,
    /// Time expressed in ps.
    PtTime,
    /// Net selection property.
    PtNet,
}

impl PropertyDisplay {
    /// The C++ enumerator name.
    pub fn cxx_name(self) -> &'static str {
        match self {
            PropertyDisplay::PtDefault => "PT_DEFAULT",
            PropertyDisplay::PtSize => "PT_SIZE",
            PropertyDisplay::PtArea => "PT_AREA",
            PropertyDisplay::PtCoord => "PT_COORD",
            PropertyDisplay::PtDegree => "PT_DEGREE",
            PropertyDisplay::PtDecidegree => "PT_DECIDEGREE",
            PropertyDisplay::PtRatio => "PT_RATIO",
            PropertyDisplay::PtTime => "PT_TIME",
            PropertyDisplay::PtNet => "PT_NET",
        }
    }
}

/// `ORIGIN_TRANSFORMS::COORD_TYPES_T`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CoordType {
    /// A non-coordinate value, never transformed.
    NotACoord,
    /// An absolute X coordinate.
    AbsXCoord,
    /// An absolute Y coordinate.
    AbsYCoord,
    /// A relative X coordinate.
    RelXCoord,
    /// A relative Y coordinate.
    RelYCoord,
}

impl CoordType {
    /// The C++ enumerator name.
    pub fn cxx_name(self) -> &'static str {
        match self {
            CoordType::NotACoord => "NOT_A_COORD",
            CoordType::AbsXCoord => "ABS_X_COORD",
            CoordType::AbsYCoord => "ABS_Y_COORD",
            CoordType::RelXCoord => "REL_X_COORD",
            CoordType::RelYCoord => "REL_Y_COORD",
        }
    }
}

/// The C++ value type `T` of a property (`TypeHash()`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ValueType {
    Int,
    /// `unsigned` (also `uint32_t`).
    Unsigned,
    /// `long long int`.
    LongLong,
    Double,
    Bool,
    /// `wxString`.
    String,
    /// `std::optional<int>`.
    OptInt,
    /// `std::optional<double>`.
    OptDouble,
    /// `EDA_ANGLE`.
    Angle,
    /// `KIGFX::COLOR4D`.
    Color,
    /// An enum with its `ENUM_MAP`.
    Enum(EnumId),
}

impl ValueType {
    /// The C++ type name.
    pub fn cxx_name(self) -> &'static str {
        match self {
            ValueType::Int => "int",
            ValueType::Unsigned => "unsigned",
            ValueType::LongLong => "long long",
            ValueType::Double => "double",
            ValueType::Bool => "bool",
            ValueType::String => "wxString",
            ValueType::OptInt => "std::optional<int>",
            ValueType::OptDouble => "std::optional<double>",
            ValueType::Angle => "EDA_ANGLE",
            ValueType::Color => "COLOR4D",
            ValueType::Enum(e) => e.cxx_name(),
        }
    }
}

/// The value a property getter returns (the `wxAny` of `PROPERTY::getter`).
#[derive(Clone, Debug, PartialEq)]
pub enum PropertyValue {
    Int(i32),
    Unsigned(u32),
    LongLong(i64),
    Double(f64),
    Bool(bool),
    String(String),
    OptInt(Option<i32>),
    OptDouble(Option<f64>),
    Angle(EdaAngle),
    Color(Color4D),
    /// An enum value as its C++ integer (`static_cast<int>`), with its map.
    Enum(EnumId, i32),
}

impl PropertyValue {
    /// `wxAny::GetAs<wxString>` of an enum value: the `DECLARE_ENUM_TO_WXANY` conversion
    /// (`None` when the value is neither mapped nor the map's undefined value; `"UNDEFINED"`
    /// for an unmapped undefined value). `None` for other value kinds.
    pub fn enum_as_string(&self) -> Option<&'static str> {
        match *self {
            PropertyValue::Enum(id, v) => id.map().any_to_string(v),
            _ => None,
        }
    }
}

/// A property getter: computes the value from the item the way the C++ member getter does.
pub type Getter = fn(ItemRef<'_>) -> PropertyValue;

/// An availability / writeability predicate (`std::function<bool( INSPECTABLE* )>`).
pub type Predicate = fn(ItemRef<'_>) -> bool;

/// Index of a property in [`PropertyManager::property`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct PropId(pub usize);

/// `PROPERTY_BASE` + `PROPERTY` / `PROPERTY_ENUM`.
#[derive(Clone, Debug)]
pub struct Property {
    /// m_name: the permanent (English) identifier; an API contract of the rule language.
    pub name: &'static str,
    /// OwnerHash(): the class the property is registered for.
    pub owner: ClassId,
    /// BaseHash(): the class whose method the getter calls (usually the owner).
    pub base: ClassId,
    /// TypeHash().
    pub value_type: ValueType,
    pub display: PropertyDisplay, // m_display
    pub coord_type: CoordType,    // m_coordType
    pub group: String,            // m_group (set by AddProperty)
    /// Constructed as `PROPERTY_ENUM` (even with `T = int`).
    pub is_enum_property: bool,
    /// PROPERTY_ENUM::m_choices as (value, label).
    pub choices: Vec<(i32, String)>,
    /// m_choicesFunc is set (a dynamic list, e.g. the fonts).
    pub has_choices_func: bool,
    pub hide_from_properties_manager: bool, // m_hideFromPropertiesManager
    pub hide_from_library_editors: bool,    // m_hideFromLibraryEditors
    pub hide_from_design_editors: bool,     // m_hideFromDesignEditors
    pub hide_from_rules_editor: bool,       // m_hideFromRulesEditor
    pub copyable: bool,                     // m_copyable
    /// A setter was given (not `NO_SETTER`); the setter itself is not ported.
    pub has_setter: bool,
    /// A validator was set (setter side; not ported).
    pub has_validator: bool,
    /// m_indeterminateFunc is set (property panel only; not ported).
    pub has_indeterminate_func: bool,
    /// m_availFunc (`None` = always available).
    pub avail: Option<Predicate>,
    /// m_writeableFunc (`None` = always writeable).
    pub writeable: Option<Predicate>,
    /// The getter.
    pub getter: Getter,
}

impl Property {
    /// `PROPERTY<Owner, T, Base>( aName, aSetter, aGetter, aDisplay, aCoordType )`.
    pub fn new(
        name: &'static str,
        owner: ClassId,
        value_type: ValueType,
        has_setter: bool,
        getter: Getter,
    ) -> Self {
        Property {
            name,
            owner,
            base: owner,
            value_type,
            display: PropertyDisplay::PtDefault,
            coord_type: CoordType::NotACoord,
            group: String::new(),
            is_enum_property: false,
            choices: Vec::new(),
            has_choices_func: false,
            hide_from_properties_manager: false,
            hide_from_library_editors: false,
            hide_from_design_editors: false,
            hide_from_rules_editor: false,
            copyable: false,
            has_setter,
            has_validator: false,
            has_indeterminate_func: false,
            avail: None,
            writeable: None,
            getter,
        }
    }

    /// `PROPERTY_ENUM<Owner, T, Base>(...)`: for an enum `T` the choices are copied from its
    /// `ENUM_MAP` at construction.
    pub fn new_enum(
        name: &'static str,
        owner: ClassId,
        value_type: ValueType,
        has_setter: bool,
        getter: Getter,
    ) -> Self {
        let mut p = Property::new(name, owner, value_type, has_setter, getter);
        p.is_enum_property = true;

        if let ValueType::Enum(e) = value_type {
            p.choices = e.map().choices_owned();
            debug_assert!(!p.choices.is_empty(), "No enum choices defined");
        }

        p
    }

    /// The `Base` template argument when it differs from the owner.
    pub fn with_base(mut self, base: ClassId) -> Self {
        self.base = base;
        self
    }

    /// `SetDisplay` / the constructor's `aDisplay`.
    pub fn with_display(mut self, display: PropertyDisplay) -> Self {
        self.display = display;
        self
    }

    /// The constructor's `aCoordType`.
    pub fn with_coord(mut self, coord: CoordType) -> Self {
        self.coord_type = coord;
        self
    }

    /// `SetAvailableFunc`.
    pub fn set_available_func(&mut self, f: Predicate) -> &mut Self {
        self.avail = Some(f);
        self
    }

    /// `SetWriteableFunc`.
    pub fn set_writeable_func(&mut self, f: Predicate) -> &mut Self {
        self.writeable = Some(f);
        self
    }

    /// `SetIsHiddenFromPropertiesManager()`.
    pub fn set_is_hidden_from_properties_manager(&mut self) -> &mut Self {
        self.hide_from_properties_manager = true;
        self
    }

    /// `SetIsHiddenFromRulesEditor()`.
    pub fn set_is_hidden_from_rules_editor(&mut self) -> &mut Self {
        self.hide_from_rules_editor = true;
        self
    }

    /// `SetIsHiddenFromLibraryEditors()`.
    pub fn set_is_hidden_from_library_editors(&mut self) -> &mut Self {
        self.hide_from_library_editors = true;
        self
    }

    /// `SetIsCopyable()`.
    pub fn set_is_copyable(&mut self) -> &mut Self {
        self.copyable = true;
        self
    }

    /// `SetValidator(...)` (recorded only).
    pub fn set_validator(&mut self) -> &mut Self {
        self.has_validator = true;
        self
    }

    /// `SetIndeterminateFunc(...)` (recorded only).
    pub fn set_indeterminate_func(&mut self) -> &mut Self {
        self.has_indeterminate_func = true;
        self
    }

    /// `SetChoicesFunc(...)` (recorded only: the lists are UI data).
    pub fn set_choices_func(&mut self) -> &mut Self {
        self.has_choices_func = true;
        self
    }

    /// `PROPERTY_ENUM::SetChoices` (`wxFAIL` on a plain `PROPERTY`).
    pub fn set_choices(&mut self, choices: Vec<(i32, String)>) -> &mut Self {
        debug_assert!(self.is_enum_property, "only possible for PROPERTY_ENUM");
        self.choices = choices;
        self
    }

    /// `Choices()`: `PROPERTY_ENUM` returns its own choices, else those of `ENUM_MAP<T>`
    /// (empty for `T = int`); a plain `PROPERTY` has none.
    pub fn choices(&self) -> Vec<(i32, String)> {
        if !self.is_enum_property {
            return Vec::new();
        }

        if !self.choices.is_empty() {
            return self.choices.clone();
        }

        match self.value_type {
            ValueType::Enum(e) => e.map().choices_owned(),
            _ => Vec::new(),
        }
    }

    /// `HasChoices()`.
    pub fn has_choices(&self) -> bool {
        if self.is_enum_property {
            !self.choices().is_empty()
        } else {
            self.has_choices_func
        }
    }

    /// `Available( aObject )`.
    pub fn available(&self, item: ItemRef<'_>) -> bool {
        self.avail.is_none_or(|f| f(item))
    }

    /// `Writeable( aObject )`: `m_setter && PROPERTY_BASE::Writeable`.
    pub fn is_writeable(&self, item: ItemRef<'_>) -> bool {
        self.has_setter && self.writeable.is_none_or(|f| f(item))
    }

    /// `PROPERTY_BASE::get` through the getter.
    pub fn get(&self, item: ItemRef<'_>) -> PropertyValue {
        (self.getter)(item)
    }
}

/// `PROPERTY_MANAGER::CLASS_DESC`.
#[derive(Clone, Debug)]
struct ClassDesc {
    /// m_bases, in `InheritsAfter` order.
    bases: Vec<ClassId>,
    /// m_ownProperties (keyed by name).
    own_properties: BTreeMap<&'static str, PropId>,
    /// m_typeCasts (the derived classes a cast is registered for).
    type_casts: BTreeSet<ClassId>,
    /// m_maskedBaseProperties.
    masked_base_properties: BTreeSet<(ClassId, &'static str)>,
    /// m_availabilityOverrides.
    availability_overrides: BTreeMap<(ClassId, &'static str), Predicate>,
    /// m_writeabilityOverrides.
    writeability_overrides: BTreeMap<(ClassId, &'static str), Predicate>,
    /// m_allProperties (collection order; C++ sorts it by address afterwards).
    all_properties: Vec<PropId>,
    /// m_displayOrder.
    display_order: BTreeMap<PropId, i32>,
    /// m_groupDisplayOrder.
    group_display_order: Vec<String>,
    /// m_ownDisplayOrder.
    own_display_order: Vec<PropId>,
    /// m_groups.
    groups: BTreeSet<String>,
    /// m_replaced.
    replaced: BTreeSet<(ClassId, &'static str)>,
}

impl ClassDesc {
    /// `CLASS_DESC( TYPE_ID )`.
    fn new() -> Self {
        ClassDesc {
            bases: Vec::new(),
            own_properties: BTreeMap::new(),
            type_casts: BTreeSet::new(),
            masked_base_properties: BTreeSet::new(),
            availability_overrides: BTreeMap::new(),
            writeability_overrides: BTreeMap::new(),
            all_properties: Vec::new(),
            display_order: BTreeMap::new(),
            group_display_order: vec![String::new()],
            own_display_order: Vec::new(),
            groups: BTreeSet::from([String::new()]),
            replaced: BTreeSet::new(),
        }
    }
}

/// `PROPERTY_MANAGER::CLASS_INFO`.
#[derive(Clone, Debug)]
pub struct ClassInfo {
    /// The `REGISTER_TYPE` name (empty for an unregistered class).
    pub name: String,
    pub type_: ClassId,
    pub properties: Vec<PropId>,
}

/// `PROPERTY_MANAGER`.
#[derive(Clone, Debug, Default)]
pub struct PropertyManager {
    /// m_classNames.
    class_names: BTreeMap<ClassId, String>,
    /// m_classes.
    classes: BTreeMap<ClassId, ClassDesc>,
    /// The property objects (C++: heap objects owned by `m_ownProperties`).
    props: Vec<Property>,
}

/// The process-wide instance (`PROPERTY_MANAGER::Instance()`) with every board item class
/// registered and rebuilt.
pub fn property_manager() -> &'static PropertyManager {
    static PM: OnceLock<PropertyManager> = OnceLock::new();

    PM.get_or_init(|| {
        let mut pm = PropertyManager::default();
        eda_item::register(&mut pm);
        eda_shape::register(&mut pm);
        eda_text::register(&mut pm);
        board_item::register_board_item(&mut pm);
        board_item::register_board_connected_item(&mut pm);
        pad::register(&mut pm);
        pcb_track::register(&mut pm);
        zone::register(&mut pm);
        footprint::register(&mut pm);
        pcb_shape::register(&mut pm);
        pcb_text::register_pcb_text(&mut pm);
        pcb_text::register_pcb_textbox(&mut pm);
        pcb_text::register_pcb_field(&mut pm);
        pcb_dimension::register(&mut pm);
        misc::register_pcb_group(&mut pm);
        pcb_table::register_pcb_table(&mut pm);
        pcb_table::register_pcb_tablecell(&mut pm);
        misc::register_pcb_barcode(&mut pm);
        misc::register_pcb_reference_image(&mut pm);
        misc::register_pcb_3d_body(&mut pm);
        pcb_table::register_pcb_drill_chart(&mut pm);
        misc::register_pcb_drill_map(&mut pm);
        pcb_table::register_pcb_generated_table(&mut pm);
        misc::register_pcb_grid_item(&mut pm);
        misc::register_pcb_marker(&mut pm);
        misc::register_pcb_point(&mut pm);
        pcb_table::register_pcb_stackup_table(&mut pm);
        misc::register_pcb_target(&mut pm);
        misc::register_pcb_constraint(&mut pm);
        generators::register_tuning_pattern(&mut pm);
        generators::register_via_stack(&mut pm);
        generators::register_via_stitch(&mut pm);
        pm.rebuild();
        pm
    })
}

/// `wxString::CmpNoCase( .. ) == 0` for the (ASCII) property names.
fn eq_no_case(a: &str, b: &str) -> bool {
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

impl PropertyManager {
    /// `getClass`: the description of `id`, created when missing.
    fn get_class(&mut self, id: ClassId) -> &mut ClassDesc {
        self.classes.entry(id).or_insert_with(ClassDesc::new)
    }

    /// `RegisterType` (`REGISTER_TYPE( x )`).
    pub fn register_type(&mut self, id: ClassId) {
        debug_assert!(!self.class_names.contains_key(&id));
        let name = id.registered_name().unwrap_or_default();
        self.class_names.insert(id, name);
    }

    /// The property object.
    pub fn property(&self, id: PropId) -> &Property {
        &self.props[id.0]
    }

    /// `AddProperty( aProperty, aGroup )`: a property whose name the owner already has is
    /// dropped and the existing one returned.
    pub fn add_property(&mut self, property: Property, group: &str) -> &mut Property {
        let name = property.name;
        let owner = property.owner;
        let new_id = PropId(self.props.len());

        let class_desc = self.get_class(owner);

        if let Some(&existing) = class_desc.own_properties.get(name) {
            return &mut self.props[existing.0];
        }

        class_desc.own_properties.insert(name, new_id);
        class_desc.own_display_order.push(new_id);

        if !class_desc.groups.contains(group) {
            class_desc.group_display_order.push(group.to_string());
            class_desc.groups.insert(group.to_string());
        }

        let mut property = property;
        property.group = group.to_string();
        self.props.push(property);
        &mut self.props[new_id.0]
    }

    /// `ReplaceProperty( aBase, aName, aNew, aGroup )`.
    pub fn replace_property(
        &mut self,
        base: ClassId,
        name: &'static str,
        new: Property,
        group: &str,
    ) -> &mut Property {
        self.get_class(new.owner).replaced.insert((base, name));
        self.add_property(new, group)
    }

    /// `AddTypeCast( new TYPE_CAST<From, To> )`: registered with the class `From` (the
    /// C++ template calls it `Base`), keyed by `To`.
    pub fn add_type_cast(&mut self, from: ClassId, to: ClassId) {
        let class_desc = self.get_class(from);
        debug_assert!(
            !class_desc.type_casts.contains(&to),
            "Such converter already exists"
        );
        class_desc.type_casts.insert(to);
    }

    /// `InheritsAfter( aDerived, aBase )`.
    pub fn inherits_after(&mut self, derived: ClassId, base: ClassId) {
        debug_assert!(derived != base, "Class cannot inherit from itself");
        self.get_class(base);
        let d = self.get_class(derived);
        d.bases.push(base);
        debug_assert!(
            d.bases.len() == 1 || d.type_casts.contains(&base),
            "You need to add a TYPE_CAST for classes inheriting from multiple bases"
        );
    }

    /// `Mask( aDerived, aBase, aName )`.
    pub fn mask(&mut self, derived: ClassId, base: ClassId, name: &'static str) {
        debug_assert!(derived != base, "Class cannot mask from itself");
        self.get_class(derived)
            .masked_base_properties
            .insert((base, name));
    }

    /// `OverrideAvailability( aDerived, aBase, aName, aFunc )`.
    pub fn override_availability(
        &mut self,
        derived: ClassId,
        base: ClassId,
        name: &'static str,
        f: Predicate,
    ) {
        debug_assert!(derived != base, "Class cannot override from itself");
        self.get_class(derived)
            .availability_overrides
            .insert((base, name), f);
    }

    /// `OverrideWriteability( aDerived, aBase, aName, aFunc )`.
    pub fn override_writeability(
        &mut self,
        derived: ClassId,
        base: ClassId,
        name: &'static str,
        f: Predicate,
    ) {
        debug_assert!(derived != base, "Class cannot override from itself");
        self.get_class(derived)
            .writeability_overrides
            .insert((base, name), f);
    }

    /// `Rebuild()`: `CLASS_DESC::rebuild` for every class.
    pub fn rebuild(&mut self) {
        let ids: Vec<ClassId> = self.classes.keys().copied().collect();

        for id in ids {
            let mut all = Vec::new();
            let mut replaced = BTreeSet::new();
            let mut display_order = BTreeMap::new();
            let mut masked = BTreeSet::new();
            self.collect_props_recur(id, &mut all, &mut replaced, &mut display_order, &mut masked);

            // The C++ then sorts m_allProperties by address (for std::set_* functions); the
            // collection order is kept here, see `properties`.
            let mut groups = BTreeSet::new();
            let mut group_order = Vec::new();
            self.collect_groups_recursive(id, &mut groups, &mut group_order);

            let c = self.classes.get_mut(&id).expect("class exists");
            c.all_properties = all;
            c.display_order = display_order;
            c.group_display_order = group_order;
        }
    }

    /// The `collectGroupsRecursive` lambda of `CLASS_DESC::rebuild`.
    fn collect_groups_recursive(
        &self,
        id: ClassId,
        set: &mut BTreeSet<String>,
        result: &mut Vec<String>,
    ) {
        let c = &self.classes[&id];

        for group in &c.group_display_order {
            if !set.contains(group) {
                set.insert(group.clone());
                result.push(group.clone());
            }
        }

        for base in &c.bases {
            self.collect_groups_recursive(*base, set, result);
        }
    }

    /// `CLASS_DESC::collectPropsRecur`.
    fn collect_props_recur(
        &self,
        id: ClassId,
        result: &mut Vec<PropId>,
        replaced: &mut BTreeSet<(ClassId, &'static str)>,
        display_order: &mut BTreeMap<PropId, i32>,
        masked: &mut BTreeSet<(ClassId, &'static str)>,
    ) {
        let c = &self.classes[&id];

        for entry in &c.replaced {
            replaced.insert(*entry);
        }

        for entry in &c.masked_base_properties {
            masked.insert(*entry);
        }

        // We want to insert our own properties in forward order, but earlier than anything
        // already in the list (which will have been added by a subclass of us)
        let mut display_order_start = 0;

        if let Some(first_so_far) = display_order.values().copied().min() {
            display_order_start = first_so_far - c.own_properties.len() as i32;
        }

        let mut idx = 0;

        for &property in &c.own_display_order {
            let p = &self.props[property.0];
            let key = (p.owner, p.name);

            // Do not store replaced properties
            if replaced.contains(&key) {
                continue;
            }

            // Do not store masked properties
            if masked.contains(&key) {
                continue;
            }

            display_order.insert(property, display_order_start + idx);
            idx += 1;
            result.push(property);
        }

        // Iterate backwards so that replaced properties appear before base properties
        for base in c.bases.iter().rev() {
            self.collect_props_recur(*base, result, replaced, display_order, masked);
        }
    }

    /// `GetProperty( aType, aProperty )`: case-insensitive name lookup (the rule language
    /// replaces `_` by a space before calling it). The C++ returns the first match of
    /// `m_allProperties` in address order. Within a class names are unique except `Text` of the
    /// dimension classes (`EDA_TEXT` and `PCB_DIMENSION_BASE`), where the reference build
    /// returns the `PCB_DIMENSION_BASE` one (oracle table, `lookup=`); the collection order
    /// used here (own properties first, then the bases) gives the same answer.
    pub fn get_property(&self, class: ClassId, name: &str) -> Option<PropId> {
        let c = self.classes.get(&class)?;
        c.all_properties
            .iter()
            .copied()
            .find(|&p| eq_no_case(name, self.props[p.0].name))
    }

    /// `GetProperties( aType )` as a list: every property once, in display order
    /// (`GetDisplayOrder`). The C++ vector is sorted by address and holds a property twice when
    /// a class reaches a base along two paths; see [`Self::all_properties_count`].
    pub fn properties(&self, class: ClassId) -> Vec<PropId> {
        let Some(c) = self.classes.get(&class) else {
            return Vec::new();
        };

        let mut v: Vec<(i32, PropId)> = c.display_order.iter().map(|(p, o)| (*o, *p)).collect();
        v.sort();
        v.into_iter().map(|(_, p)| p).collect()
    }

    /// `GetProperties( aType ).size()` (duplicates included).
    pub fn all_properties_count(&self, class: ClassId) -> usize {
        self.classes
            .get(&class)
            .map_or(0, |c| c.all_properties.len())
    }

    /// `GetDisplayOrder( aType )`.
    pub fn display_order(&self, class: ClassId) -> BTreeMap<PropId, i32> {
        self.classes
            .get(&class)
            .map(|c| c.display_order.clone())
            .unwrap_or_default()
    }

    /// `GetGroupDisplayOrder( aType )`.
    pub fn group_display_order(&self, class: ClassId) -> &[String] {
        self.classes
            .get(&class)
            .map_or(&[], |c| c.group_display_order.as_slice())
    }

    /// `IsOfType( aDerived, aBase )`.
    pub fn is_of_type(&self, derived: ClassId, base: ClassId) -> bool {
        if derived == base {
            return true;
        }

        let Some(d) = self.classes.get(&derived) else {
            // wxCHECK( derived != m_classes.end(), false ): missing class description
            return false;
        };

        // traverse the hierarchy seeking for the base class
        d.bases.iter().any(|b| self.is_of_type(*b, base))
    }

    /// Whether `TypeCast( aSource, aBase, aTarget )` finds a converter (an explicit
    /// `TYPE_CAST`, or `IsOfType`).
    pub fn has_type_cast(&self, base: ClassId, target: ClassId) -> bool {
        if base == target {
            return true;
        }

        match self.classes.get(&base) {
            None => true,
            Some(c) => c.type_casts.contains(&target) || self.is_of_type(base, target),
        }
    }

    /// `GetAllClasses()` (in `ClassId` order; C++: unordered).
    pub fn all_classes(&self) -> Vec<ClassInfo> {
        self.classes
            .iter()
            .map(|(id, c)| ClassInfo {
                name: self.class_names.get(id).cloned().unwrap_or_default(),
                type_: *id,
                properties: c.all_properties.clone(),
            })
            .collect()
    }

    /// The classes that are of type `base` (`IsOfType( cls, base )`), `base` included.
    pub fn derived_classes(&self, base: ClassId) -> Vec<ClassId> {
        self.classes
            .keys()
            .copied()
            .filter(|c| self.is_of_type(*c, base))
            .collect()
    }

    /// `IsAvailableFor( aItemClass, aProp, aItem )`.
    pub fn is_available_for(&self, item_class: ClassId, prop: PropId, item: ItemRef<'_>) -> bool {
        let p = &self.props[prop.0];

        if !p.available(item) {
            return false;
        }

        let Some(derived) = self.classes.get(&item_class) else {
            return true;
        };

        match derived.availability_overrides.get(&(p.base, p.name)) {
            Some(f) => f(item),
            None => true,
        }
    }

    /// `IsWriteableFor( aItemClass, aProp, aItem )`.
    pub fn is_writeable_for(&self, item_class: ClassId, prop: PropId, item: ItemRef<'_>) -> bool {
        let p = &self.props[prop.0];

        if !p.is_writeable(item) {
            return false;
        }

        let Some(derived) = self.classes.get(&item_class) else {
            return true;
        };

        match derived.writeability_overrides.get(&(p.base, p.name)) {
            Some(f) => f(item),
            None => true,
        }
    }
}
