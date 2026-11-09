# Board model of `kicad-pcb` - type definitions

Companion of [`board-model.md`](board-model.md) (architecture, API, rules; read it first). This
file is the **complete data model**: every struct with all fields and every enum with all
variants, each field mapped to the C++ member it ports. Work package M0 transcribes it into
`rust/crates/kicad-pcb/src/` (module named in each section heading); later packages add
behaviour but do not change these definitions without the orchestrator.

Conventions:

* The trailing comment names the C++ member (`// m_width`). A field without one is new and
  explained. `String` ports `wxString` / `std::string`; `BTreeMap`/`BTreeSet` port
  `std::map`/`std::set` and every iterated `std::unordered_*`; `Arc<T>` ports `shared_ptr<T>`;
  `Option<T>` ports `std::optional<T>`; `int` -> `i32`, `long long` -> `i64`, `double` -> `f64`.
* All item structs derive `Clone, Debug` (no `PartialEq`: `operator==` / `Compare` are ported by
  hand). Small value types derive `Clone, Copy, PartialEq, Eq, Debug` where C++ has value
  semantics; enums derive `Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug` and keep the
  C++ discriminants where the C++ has explicit values or they are written to a file.
* `Default` = the C++ constructor's field values (from the constructor body in the named `.cpp`,
  not invented) with a **nil** `uuid`; `Type::new(&mut KiidGenerator, ..)` additionally draws the
  fresh `KIID` the `EDA_ITEM` constructor draws.
* Fields marked `pub(crate)` have invariants (frames, caches, containers) and are reached through
  accessors named after the C++ getters (`lib_pos()`, `transform()`, `filled_polys_list(layer)`).
* Foundation types: `kicad-geom`: `Vector2I`, `Vector2D`, `Vector3D`, `Box2I`, `EdaAngle`,
  `EdaOrientation`, `Seg`, `ShapeLineChain`, `ShapePolySet`, `ShapeSegment`, `ShapeCompound`,
  `ShapeArc`, `trait Shape` (`Send + Sync`), `TransformTrs`, `Ellipse<i32>`, `FlipDirection`;
  `kicad-core`: `Kiid`, `KiidPath`, `KiidGenerator`, `PcbLayerId` (newtype with `FCu` ...
  constants), `Lset`, `Lseq`, `LibId`, `KicadT`, `Flashing`, `Severity`, `MinOptMax<T>`,
  `EdaUnits`, `TextAttributes`, `StrokeParams`, `LineStyle`, `Color4D`, `Hash128`;
  `kicad-sexpr`: `Sexpr` (verbatim pass-through); `kicad-font`: `FontRef`, `Glyph`. Types not yet
  present there are listed as open questions of the design and are added by their owners;
  M0 may use local placeholder definitions behind a `foundation_stub` module until they land.

## 0 Kernel (`ids.rs`, `arena.rs`, `side_table.rs`, `cache.rs`, `item.rs`, `item_ref.rs`)

```rust
/// Typed handle into an `Arena<T>`. 8 bytes, `Copy`, no lifetime. Manual impls without a
/// bound on T: Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug.
pub struct Id<T> { index: u32, generation: u16, _marker: PhantomData<fn() -> T> }

/// Which arena a handle belongs to. Values are part of the wasm ABI: append only.
#[repr(u8)]
pub enum ItemKind {
    Footprint = 1, Pad = 2, Track = 3, Zone = 4, Shape = 5, Text = 6, Field = 7, Textbox = 8,
    Table = 9, Tablecell = 10, Dimension = 11, Barcode = 12, Image = 13, Target = 14,
    Point = 15, Group = 16, Generator = 17, Marker = 18, Constraint = 19, Body3d = 20,
    GridItem = 21, DrillMap = 22,
    /// Not an arena: `index` is the net code (lets ResolveItem return a NETINFO_ITEM).
    Net = 23,
}

/// Untyped handle = stored `BOARD_ITEM*`. 8 bytes, totally ordered.
/// `to_bits()` = `index | generation << 32 | kind << 48` (< 2^53, exact as a JS number).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ItemId { kind: ItemKind, generation: u16, index: u32 }

pub type FootprintId = Id<Footprint>;   pub type PadId = Id<Pad>;          pub type TrackId = Id<Track>;
pub type ZoneId = Id<Zone>;             pub type ShapeId = Id<PcbShape>;   pub type TextId = Id<PcbText>;
pub type FieldId = Id<PcbField>;        pub type TextboxId = Id<PcbTextbox>;
pub type TableId = Id<PcbTable>;        pub type TablecellId = Id<PcbTablecell>;
pub type DimensionId = Id<PcbDimension>; pub type BarcodeId = Id<PcbBarcode>;
pub type ImageId = Id<PcbReferenceImage>; pub type TargetId = Id<PcbTarget>;
pub type PointId = Id<PcbPoint>;        pub type GroupId = Id<PcbGroup>;
pub type GeneratorId = Id<PcbGenerator>; pub type MarkerId = Id<PcbMarker>;
pub type ConstraintId = Id<PcbConstraint>; pub type Body3dId = Id<Pcb3dBody>;
pub type GridItemId = Id<PcbGridItem>;  pub type DrillMapId = Id<PcbDrillMap>;

/// Net code as returned by BOARD_CONNECTED_ITEM::GetNetCode().
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct NetCode(pub i32);
impl NetCode {
    pub const UNCONNECTED: NetCode = NetCode(0);   // NETINFO_LIST::UNCONNECTED
    pub const ORPHANED: NetCode = NetCode(-1);     // NETINFO_LIST::ORPHANED / null m_netinfo
}

/// Generational slot vector (plain Vec + enum, no unsafe).
pub struct Arena<T> { slots: Vec<Slot<T>>, free_head: u32, live: u32 }
struct Slot<T> { generation: u16, state: SlotState<T> }
enum SlotState<T> {
    Live(T),
    Parked(T),               // removed by an open BoardCommit; restorable under the same handle
    Taken,                   // moved out by Board::with_item_taken
    Free { next_free: u32 },
    Retired,                 // generation exhausted; never handed out again
}

/// Dense per-arena side table, generation checked. Replaces std::map<BOARD_ITEM*, V> and
/// algorithm scratch members (m_arflag, m_subRatsnest, temporary EDA_ITEM_FLAGS).
pub struct SideTable<T, V> { data: Vec<Option<(u16, V)>>, _marker: PhantomData<fn() -> T> }

/// Built once from &self, cleared only through &mut self. Clone -> empty.
pub struct OnceCache<T>(std::sync::OnceLock<T>);
/// Re-validated from &self (C++ `mutable` + mutex + dirty flag / time stamp); `T` stores the
/// inputs it was built from (time stamp, parent transform, text, ...) and the getter compares
/// them, exactly like the C++ code does. Clone -> empty, ignored by equality.
pub struct KeyedCache<T>(std::sync::Mutex<Option<Arc<T>>>);

/// EDA_ITEM::m_parent.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ParentRef {
    #[default]
    None,                    // detached (builder, dummy)
    Board,                   // direct child of the board
    /// Child of another item: footprint (pads, fields, graphics, zones, groups, points,
    /// constraints, bodies), table (cells), pad (inline custom-shape primitives), barcode
    /// (its text), generator (template items), marker (path shapes).
    Item(ItemId),
}

/// Owned item outside any arena: commit before-images, builder input, clipboard, dummies.
#[derive(Clone, Debug)]
pub enum OwnedItem {
    Footprint(Box<FootprintTree>), Pad(Box<Pad>), Track(Track), Zone(Box<Zone>),
    Shape(Box<PcbShape>), Text(Box<PcbText>), Field(Box<PcbField>), Textbox(Box<PcbTextbox>),
    Table(Box<TableTree>), Dimension(Box<PcbDimension>), Barcode(Box<PcbBarcode>),
    Image(Box<PcbReferenceImage>), Target(PcbTarget), Point(PcbPoint), Group(PcbGroup),
    Generator(Box<PcbGenerator>), Marker(Box<PcbMarker>), Constraint(PcbConstraint),
    Body3d(Box<Pcb3dBody>), GridItem(Box<PcbGridItem>), DrillMap(Box<PcbDrillMap>),
}
/// A footprint with its children by value (the child-list fields of `footprint` are empty and
/// are rebuilt on insertion). Group members inside the tree are referenced by Kiid
/// (`group_members`), resolved when the tree is added. Port of the deep copy of the C++
/// FOOTPRINT copy constructor (its ptrMap becomes the Kiid map).
#[derive(Clone, Debug)]
pub struct FootprintTree {
    pub footprint: Footprint,
    pub fields: Vec<PcbField>,                 // C++ m_fields order
    pub pads: Vec<Pad>,                        // m_pads order
    pub drawings: Vec<OwnedItem>,              // m_drawings order (shapes, texts, textboxes, tables, dimensions, barcodes, images)
    pub zones: Vec<Zone>,
    pub groups: Vec<PcbGroup>,
    pub constraints: Vec<PcbConstraint>,
    pub points: Vec<PcbPoint>,
    pub bodies: Vec<Pcb3dBody>,
    pub group_members: Vec<(Kiid, Vec<Kiid>)>, // (group uuid, member uuids)
}
#[derive(Clone, Debug)]
pub struct TableTree { pub table: PcbTable, pub cells: Vec<PcbTablecell> }
```

`AnyItem<'a>` / `AnyItemMut<'a>` (one variant per `ItemKind`, `Net` only in `AnyItem`) and
`ItemRef<'a>` are specified in `board-model.md` section 6.3; `BoardCtx`, `MutEnv`,
`ParentFrame`, the hook traits, `Change`, `BoardCommit` in sections 6-8 there.

```rust
/// LayerEnv: the board facts leaf mutators need (shared by every MutEnv).
pub struct LayerEnv {
    pub copper_layer_count: i32,
    pub enabled_layers: Lset,
    /// BOARD::FlipLayer for every layer id (user-layer opposites included).
    pub flipped: Vec<PcbLayerId>,                   // indexed by layer id
}

/// Value part of DRC_CONSTRAINT (pcbnew/drc/drc_rule.h), returned by RuleResolver::eval_rules.
/// Rule back pointers become names / severities (the engine owns the rules).
#[derive(Clone, Debug, Default)]
pub struct DrcConstraint {
    pub type_: DrcConstraintT,                      // m_Type
    pub value: MinOptMax<i32>,                      // m_Value
    pub disallow_flags: i32,                        // m_DisallowFlags
    pub zone_connection: ZoneConnection,            // m_ZoneConnection
    pub implicit_min: bool,                         // m_ImplicitMin
    pub reference_layer: String,                    // m_ReferenceLayer
    pub reference_net: String,                      // m_ReferenceNet
    pub options: u32,                               // m_options (DRC_CONSTRAINT::OPTIONS bits)
    pub name: String,                               // m_name (just-in-time constraints)
    pub parent_rule: Option<DrcRuleInfo>,           // m_parentRule
    pub min_rule: Option<DrcRuleInfo>,              // m_minRule
    pub opt_rule: Option<DrcRuleInfo>,              // m_optRule
    pub max_rule: Option<DrcRuleInfo>,              // m_maxRule
}
/// What model and report code read from a DRC_RULE through a constraint.
#[derive(Clone, Debug, Default)]
pub struct DrcRuleInfo { pub name: String, pub severity: Severity, pub implicit: bool }
/// DRC_CONSTRAINT_T (pcbnew/drc/drc_constraint_types.h), same order as C++.
#[derive(Default)]
pub enum DrcConstraintT {
    #[default] NullConstraint = 0, ClearanceConstraint, CreepageConstraint,
    HoleClearanceConstraint, HoleToHoleConstraint, EdgeClearanceConstraint, HoleSizeConstraint,
    CourtyardClearanceConstraint, SilkClearanceConstraint, TextHeightConstraint,
    TextThicknessConstraint, TrackWidthConstraint, TrackSegmentLengthConstraint,
    AnnularWidthConstraint, ZoneConnectionConstraint, ThermalReliefGapConstraint,
    ThermalSpokeWidthConstraint, MinResolvedSpokesConstraint, SolderMaskExpansionConstraint,
    SolderPasteAbsMarginConstraint, SolderPasteRelMarginConstraint, DisallowConstraint,
    ViaDiameterConstraint, LengthConstraint, NetChainLengthConstraint,
    NetChainStubLengthConstraint, NetChainReturnPathConstraint, SkewConstraint,
    DiffPairGapConstraint, MaxUncoupledConstraint, DiffPairIntraSkewConstraint,
    ViaCountConstraint, PhysicalClearanceConstraint, PhysicalHoleClearanceConstraint,
    AssertionConstraint, ConnectionWidthConstraint, TrackAngleConstraint,
    ViaDanglingConstraint, BridgedMaskConstraint, SolderMaskSliverConstraint,
    MicroviaStackDepthConstraint, HoleAspectRatioConstraint,
}
/// ERROR_LOC (geometry/geometry_utils.h)
pub enum ErrorLoc { ErrorOutside, ErrorInside }
```

## 1 Shared item data (`item.rs`, `teardrop_parameters.rs`)

`Flashing` is `kicad_core::Flashing`.

```rust
pub type SharedShape = std::sync::Arc<dyn Shape>;      // std::shared_ptr<SHAPE>

/// `EDA_ITEM_FLAGS` (include/eda_item_flags.h); same bit values as C++.
#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub struct ItemFlags(pub u32);
impl ItemFlags {
    pub const IS_CHANGED: u32 = 1 << 0;      pub const IS_LINKED: u32 = 1 << 1;
    pub const IN_EDIT: u32 = 1 << 2;         pub const IS_MOVING: u32 = 1 << 3;
    pub const IS_NEW: u32 = 1 << 4;          pub const IS_BROKEN: u32 = 1 << 5;
    pub const IS_DELETED: u32 = 1 << 7;      pub const STRUCT_DELETED: u32 = 1 << 13;
    pub const CANDIDATE: u32 = 1 << 14;      pub const SKIP_STRUCT: u32 = 1 << 15;
    pub const IS_PASTED: u32 = 1 << 17;      pub const COURTYARD_CONFLICT: u32 = 1 << 19;
    pub const MALFORMED_F_COURTYARD: u32 = 1 << 20;
    pub const MALFORMED_B_COURTYARD: u32 = 1 << 21;
    pub const ROUTER_TRANSIENT: u32 = 1 << 22;
    pub const CONNECTIVITY_CANDIDATE: u32 = 1 << 23;
    pub const HOLE_PROXY: u32 = 1 << 24;     pub const UR_TRANSIENT: u32 = 1 << 28;
    // selection / brightening / rollover bits are not ported
}

pub enum RecurseMode { Recurse, NoRecurse }                     // RECURSE_MODE
pub enum BoardUse { Normal, FpHolder }                          // BOARD_USE

/// Data members of EDA_ITEM + BOARD_ITEM.
#[derive(Clone, Debug, Default)]
pub struct BoardItemBase {
    pub uuid: Kiid,                                      // EDA_ITEM::m_Uuid
    pub parent: ParentRef,                               // EDA_ITEM::m_parent
    pub group: Option<ItemId>,                           // EDA_ITEM::m_group (Group or Generator)
    pub flags: ItemFlags,                                // EDA_ITEM::m_flags
    pub custom_properties: BTreeMap<String, String>,     // EDA_ITEM::m_customProperties
    pub layer: PcbLayerId,                               // BOARD_ITEM::m_layer
    pub is_knockout: bool,                               // BOARD_ITEM::m_isKnockout
    pub is_locked: bool,                                 // BOARD_ITEM::m_isLocked
}

/// Data members of BOARD_CONNECTED_ITEM.
#[derive(Clone, Debug)]
pub struct ConnectedItemBase {
    pub item: BoardItemBase,
    pub net: NetCode,                                    // m_netinfo
    pub teardrop_params: TeardropParameters,             // m_teardropParams
    pub local_ratsnest_visible: bool,                    // m_localRatsnestVisible
}

/// TEARDROP_PARAMETERS (pcbnew/teardrop/teardrop_parameters.h).
#[derive(Clone, Debug, PartialEq)]
pub struct TeardropParameters {
    pub td_max_len: i32,                  // m_TdMaxLen
    pub td_max_width: i32,                // m_TdMaxWidth
    pub best_length_ratio: f64,           // m_BestLengthRatio
    pub best_width_ratio: f64,            // m_BestWidthRatio
    pub width_to_size_filter_ratio: f64,  // m_WidthtoSizeFilterRatio
    pub curved_edges: bool,               // m_CurvedEdges
    pub enabled: bool,                    // m_Enabled
    pub allow_use_two_tracks: bool,       // m_AllowUseTwoTracks
    pub td_on_pads_in_zones: bool,        // m_TdOnPadsInZones
}
pub enum TeardropType { TdNone = 0, TdUnspecified, TdViapad, TdTrackend }   // TEARDROP_TYPE

/// TEARDROP_PARAMETERS_LIST.
#[derive(Clone, Debug)]
pub struct TeardropParametersList {
    pub params_list: Vec<TeardropParameters>,  // m_params_list: [TARGET_ROUND, TARGET_RECT, TARGET_TRACK]
    pub target_vias: bool,                     // m_TargetVias
    pub target_pth_pads: bool,                 // m_TargetPTHPads
    pub target_smd_pads: bool,                 // m_TargetSMDPads
    pub target_track2track: bool,              // m_TargetTrack2Track
    pub use_round_shapes_only: bool,           // m_UseRoundShapesOnly
}
```

## 2 Tracks, arcs, vias (`pcb_track/mod.rs`) and the padstack (`padstack.rs`)

```rust
/// Element type of `Board::tracks` (C++ `TRACKS` holds PCB_TRACK*, PCB_ARC* and PCB_VIA*).
/// `Via` is boxed so that a segment stays small (a via carries a whole padstack).
#[derive(Clone, Debug)]
pub enum Track {
    Trace(PcbTrack),        // PCB_TRACE_T
    Arc(PcbArc),            // PCB_ARC_T
    Via(Box<PcbVia>),       // PCB_VIA_T
}
/// `Track` has no C++ counterpart: it is the tagged form of "PCB_TRACK* that may be an arc or a
/// via". `BoardItem`, `BoardConnectedItem` and `BoardItemMut` are implemented for `Track` by
/// delegating to the variant, and for `PcbTrack`, `PcbArc`, `PcbVia` themselves.
impl Track {
    pub fn as_track(&self) -> &PcbTrack;            // the PCB_TRACK base of any variant
    pub fn as_track_mut(&mut self) -> &mut PcbTrack;
    pub fn as_arc(&self) -> Option<&PcbArc>;
    pub fn as_via(&self) -> Option<&PcbVia>;
    pub fn as_via_mut(&mut self) -> Option<&mut PcbVia>;
    pub fn type_(&self) -> KicadT;
}

#[derive(Clone, Debug)]
pub struct PcbTrack {
    pub base: ConnectedItemBase,
    pub start: Vector2I,                       // m_Start
    pub end: Vector2I,                         // m_End
    pub has_solder_mask: bool,                 // m_hasSolderMask
    pub solder_mask_margin: Option<i32>,       // m_solderMaskMargin
    pub width: i32,                            // m_width
}

#[derive(Clone, Debug)]
pub struct PcbArc {
    pub track: PcbTrack,
    pub mid: Vector2I,                         // m_Mid
}

#[derive(Clone, Debug)]
pub struct PcbVia {
    pub track: PcbTrack,                       // start == end == position
    pub via_type: Viatype,                     // m_viaType
    pub padstack: Padstack,                    // m_padStack
    pub is_free: bool,                         // m_isFree
    /// m_zoneLayerOverrides (std::array<std::atomic<..>, MAX_CU_LAYERS>): plain data, written by
    /// the zone filler in its `&mut Board` phases.
    pub zone_layer_overrides: [ZoneLayerOverride; MAX_CU_LAYERS],
}

#[repr(i32)] pub enum Viatype { Through = 4, Buried = 3, Blind = 2, Microvia = 1, NotDefined = 0 }
pub enum ZoneLayerOverride { None, ForceFlashed, ForceNoZoneConnection }      // ZLO_*
pub enum EndpointT { Start = 0, End = 1 }
pub enum TentingMode  { FromBoard = 0, Tented = 1,  NotTented = 2 }
pub enum CoveringMode { FromBoard = 0, Covered = 1, NotCovered = 2 }
pub enum PluggingMode { FromBoard = 0, Plugged = 1, NotPlugged = 2 }
pub enum CappingMode  { FromBoard = 0, Capped = 1,  NotCapped = 2 }
pub enum FillingMode  { FromBoard = 0, Filled = 1,  NotFilled = 2 }
pub const UNDEFINED_DRILL_DIAMETER: i32 = -1;
pub const MAX_CU_LAYERS: usize = 32;
```

```rust
/// PADSTACK (pcbnew/padstack.h). `m_parent` is dropped: the owning pad / via passes itself or
/// the context to the few methods that used it (FlipLayers, MachiningKnockout, AddPrimitive).
/// Equality is the ported `PADSTACK::operator==` / `Compare`, not a derive.
#[derive(Clone, Debug)]
pub struct Padstack {
    pub mode: PadstackMode,                                        // m_mode
    pub layer_set: Lset,                                           // m_layerSet
    pub custom_name: Option<String>,                               // m_customName
    pub orientation: EdaOrientation,                               // m_orientation
    /// m_copperProps (std::unordered_map). Keys: F_Cu (ALL_LAYERS), In1_Cu (INNER_LAYERS),
    /// B_Cu, or any copper layer in CUSTOM mode.
    pub copper_props: BTreeMap<PcbLayerId, CopperLayerProps>,
    pub front_mask_props: MaskLayerProps,                          // m_frontMaskProps
    pub back_mask_props: MaskLayerProps,                           // m_backMaskProps
    pub unconnected_layer_mode: UnconnectedLayerMode,              // m_unconnectedLayerMode
    pub custom_shape_in_zone_mode: CustomShapeZoneMode,            // m_customShapeInZoneMode
    pub drill: DrillProps,                                         // m_drill
    pub secondary_drill: DrillProps,                               // m_secondaryDrill
    pub tertiary_drill: DrillProps,                                // m_tertiaryDrill
    pub front_post_machining: PostMachiningProps,                  // m_frontPostMachining
    pub back_post_machining: PostMachiningProps,                   // m_backPostMachining
}
impl Padstack {
    pub const ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;
    pub const INNER_LAYERS: PcbLayerId = PcbLayerId::In1Cu;
}

pub enum PadstackType { Normal, Via, Mounting }                    // PADSTACK::TYPE
pub enum PadstackMode { Normal, FrontInnerBack, Custom }           // PADSTACK::MODE
pub enum PadShape { Circle, Rectangle, Oval, Trapezoid, Roundrect, ChamferedRect, Custom }
pub enum PadDrillShape { Undefined, Circle, Oblong }
pub enum PadDrillPostMachiningMode { Unknown, NotPostMachined, Counterbore, Countersink }
pub enum BackdrillMode { NoBackdrill, BackdrillBottom, BackdrillTop, BackdrillBoth }
pub enum PadAttrib { Pth, Smd, Conn, Npth, NonComponent }
pub enum PadProp { None, Bga, FiducialGlbl, FiducialLocal, Testpoint, Heatsink, Castellated,
                   Mechanical, Pressfit }
pub enum UnconnectedLayerMode { KeepAll, StartEndOnly, RemoveAll, RemoveExceptStartAndEnd }
pub enum CustomShapeZoneMode { Outline, Convexhull }
pub enum PadSimElectricalType { None = 0, Source, Sink }

/// PADSTACK::SHAPE_PROPS
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeProps {
    pub shape: PadShape,
    pub anchor_shape: PadShape,
    pub size: Vector2I,                       // lib frame when the owner is a footprint pad
    pub offset: Vector2I,
    pub round_rect_radius_ratio: f64,
    pub chamfered_rect_ratio: f64,
    pub chamfered_rect_positions: i32,        // RECT_CHAMFER_POSITIONS bits
    pub trapezoid_delta_size: Vector2I,
}
/// PADSTACK::COPPER_LAYER_PROPS
#[derive(Clone, Debug)]
pub struct CopperLayerProps {
    pub shape: ShapeProps,
    pub zone_connection: Option<ZoneConnection>,
    pub thermal_spoke_width: Option<i32>,
    pub thermal_spoke_angle: Option<EdaOrientation>,
    pub thermal_gap: Option<i32>,
    pub clearance: Option<i32>,
    /// custom_shapes (std::vector<std::shared_ptr<PCB_SHAPE>>): inline values, pad frame,
    /// `parent == ParentRef::Item(pad)`. Not arena items, not in the UUID index.
    pub custom_shapes: Vec<PcbShape>,
}
/// PADSTACK::MASK_LAYER_PROPS
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MaskLayerProps {
    pub solder_mask_margin: Option<i32>,
    pub solder_paste_margin: Option<i32>,
    pub solder_paste_margin_ratio: Option<f64>,
    pub has_solder_mask: Option<bool>,
    pub has_solder_paste: Option<bool>,
    pub has_covering: Option<bool>,
    pub has_plugging: Option<bool>,
}
/// PADSTACK::DRILL_PROPS
#[derive(Clone, Debug, PartialEq)]
pub struct DrillProps {
    pub size: Vector2I,
    pub shape: PadDrillShape,                 // default Undefined
    pub start: PcbLayerId,                    // default UNDEFINED_LAYER
    pub end: PcbLayerId,
    pub is_filled: Option<bool>,
    pub is_capped: Option<bool>,
}
/// PADSTACK::POST_MACHINING_PROPS
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PostMachiningProps {
    pub mode: Option<PadDrillPostMachiningMode>,
    pub size: i32,
    pub depth: i32,
    pub angle: i32,
}
```

## 3 Pad (`pad/mod.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Pad {
    pub base: ConnectedItemBase,
    pub number: String,                               // m_number
    pub pin_function: String,                         // m_pinFunction
    pub pin_type: String,                             // m_pinType
    pub(crate) lib_pos: Vector2I,                     // m_libPos (footprint library frame)
    pub(crate) lib_orientation: EdaOrientation,       // m_libOrientation (library frame)
    /// m_padStack: sizes, offsets, drill, primitives in the library frame. Read through
    /// `lib_padstack()` (writer) or the board-frame accessors `size(ctx, layer)` ...
    pub(crate) padstack: Padstack,
    pub sub_ratsnest: i32,                            // m_subRatsnest
    pub attribute: PadAttrib,                         // m_attribute
    pub property: PadProp,                            // m_property
    pub length_pad_to_die: i32,                       // m_lengthPadToDie
    pub delay_pad_to_die: i32,                        // m_delayPadToDie
    pub sim_electrical_type: PadSimElectricalType,    // m_simElectricalType
    pub zone_layer_overrides: BTreeMap<PcbLayerId, ZoneLayerOverride>,   // m_zoneLayerOverrides
    pub(crate) caches: PadCaches,
}

/// m_drawCache + m_shapesDirty + m_polyDirty[2] + m_effectiveBoundingRadius + m_maskShapes.
/// An empty cell is the C++ "dirty" state.
#[derive(Clone, Debug, Default)]
pub(crate) struct PadCaches {
    pub shapes: KeyedCache<PadEffectiveShapes>,                      // BuildEffectiveShapes
    pub polygons: [KeyedCache<PadEffectivePolygons>; 2],             // BuildEffectivePolygon( ERROR_INSIDE / ERROR_OUTSIDE )
    pub mask_shapes: KeyedCache<PadMaskShapes>,                      // m_maskShapes
}
/// Every input of the pad caches that is not the pad itself. A cache entry built for another
/// key is rebuilt (insurance against a missed `set_dirty`; C++ relies on the dirty flags only,
/// which `set_dirty()` reproduces by clearing).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct PadCacheKey { pub transform: Option<TransformTrs>, pub flipped: bool, pub max_error: i32 }
pub(crate) struct PadMaskShapes { pub key: PadCacheKey, pub by_layer: BTreeMap<PcbLayerId, (i32, Arc<ShapeCompound>)> }
pub(crate) struct PadEffectiveShapes {
    pub key: PadCacheKey,
    pub bounding_box: Box2I,                                         // m_effectiveBoundingBox
    pub shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>>,            // m_effectiveShapes
    pub hole_shape: Arc<ShapeSegment>,                               // m_effectiveHoleShape
}
pub(crate) struct PadEffectivePolygons {
    pub key: PadCacheKey,
    pub polygons: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,           // m_effectivePolygons[layer][errorLoc]
    pub bounding_radius: i32,                                        // m_effectiveBoundingRadius
}
```

Dropped: `m_dataMutex`, `m_maskShapeMutex` (inside the cache cells), `m_lastGalZoomLevel` (GAL).

## 4 Footprint (`footprint/mod.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Footprint {
    /// `base.layer` is F_Cu or B_Cu (the side).
    pub base: BoardItemBase,

    // ---- children: order lists into the board arenas ------------------------------------
    // FOOTPRINT::Add inserts at the FRONT of every list unless the mode is exactly
    // ADD_MODE::APPEND; fields always push_back (footprint.cpp:1900). pub(crate): only
    // Board methods edit them; read with board.footprint_pads(fp) etc.
    pub(crate) fields: VecDeque<FieldId>,     // m_fields (std::deque)
    pub(crate) drawings: VecDeque<ItemId>,    // m_drawings (Shape, Text, Textbox, Table, Dimension, Barcode, Image)
    pub(crate) pads: VecDeque<PadId>,         // m_pads
    pub(crate) zones: VecDeque<ZoneId>,       // m_zones (std::vector, inserted at begin())
    pub(crate) groups: VecDeque<GroupId>,     // m_groups
    pub(crate) constraints: VecDeque<ConstraintId>,   // m_constraints
    pub(crate) points: VecDeque<PointId>,     // m_points
    pub(crate) bodies: VecDeque<Body3dId>,    // m_bodies

    // ---- placement: written only by Board::footprint_* (board-model.md 8.3) --------------------
    pub(crate) transform: TransformTrs,       // m_transform
    pub(crate) flipped: bool,                 // m_flipped

    pub fpid: LibId,                          // m_fpid
    pub attributes: i32,                      // m_attributes (FOOTPRINT_ATTR_T bits, below)
    pub fp_status: i32,                       // m_fpStatus (FP_is_PLACED 0x02, FP_to_PLACE 0x04)
    pub file_format_version_at_load: i32,     // m_fileFormatVersionAtLoad
    pub variants: CaseInsensitiveMap<FootprintVariant>,   // m_variants
    pub net_tie_pad_groups: Vec<String>,      // m_netTiePadGroups
    pub jumper_pad_groups: JumperGroupSet,    // m_jumperPadGroups
    pub duplicate_pad_numbers_are_jumpers: bool,          // m_duplicatePadNumbersAreJumpers
    pub allow_missing_courtyard: bool,        // m_allowMissingCourtyard
    pub allow_solder_mask_bridges: bool,      // m_allowSolderMaskBridges

    pub zone_connection: ZoneConnection,      // m_zoneConnection
    pub clearance: Option<i32>,               // m_clearance
    pub solder_mask_margin: Option<i32>,      // m_solderMaskMargin
    pub solder_paste_margin: Option<i32>,     // m_solderPasteMargin
    pub solder_paste_margin_ratio: Option<f64>,           // m_solderPasteMarginRatio

    pub stackup_layers: Lset,                 // m_stackupLayers
    pub stackup_mode: FootprintStackup,       // m_stackupMode
    pub private_layers: Lset,                 // m_privateLayers

    pub lib_description: String,              // m_libDescription
    pub keywords: String,                     // m_keywords
    pub path: KiidPath,                       // m_path
    pub sheetname: String,                    // m_sheetname
    pub sheetfile: String,                    // m_sheetfile
    pub filters: String,                      // m_filters
    pub models: Vec<Fp3dmodel>,               // m_3D_Drawings
    pub height: i32,                          // m_height
    pub initial_comments: Option<Vec<String>>,            // m_initial_comments
    pub unit_info: Vec<FpUnitInfo>,           // m_unitInfo
    pub embedded_files: EmbeddedFiles,        // EMBEDDED_FILES base class
    /// m_componentClassCacheProxy->m_staticComponentClass
    pub static_component_class: Option<Arc<ComponentClass>>,

    // ---- derived ---------------------------------------------------------------------------
    /// m_geometry_cache: three values each validated against its own board time stamp, as C++.
    pub(crate) geometry_cache: std::sync::Mutex<FootprintGeometryCacheData>,
    pub(crate) courtyard_cache: Option<Box<FootprintCourtyardCacheData>>,   // m_courtyard_cache
    pub(crate) net_tie_cache: BTreeMap<ItemId, BTreeSet<NetCode>>,          // m_netTieCache
    /// (ticker, dynamic class, final class) of COMPONENT_CLASS_CACHE_PROXY
    pub(crate) component_class_cache: std::sync::Mutex<ComponentClassCache>,
}

pub mod footprint_attr {            // FOOTPRINT_ATTR_T
    pub const FP_THROUGH_HOLE: i32 = 0x0001;           pub const FP_SMD: i32 = 0x0002;
    pub const FP_EXCLUDE_FROM_POS_FILES: i32 = 0x0004; pub const FP_EXCLUDE_FROM_BOM: i32 = 0x0008;
    pub const FP_BOARD_ONLY: i32 = 0x0010;             pub const FP_JUST_ADDED: i32 = 0x0020;
    pub const FP_DNP: i32 = 0x0040;                    pub const FP_EXCLUDE_FROM_SIM: i32 = 0x0080;
}
pub enum FootprintType { Unspecified, ThroughHole, Smd }             // FOOTPRINT_TYPE
pub enum FootprintStackup { ExpandInnerLayers, CustomLayers }        // FOOTPRINT_STACKUP

/// FP_3DMODEL
#[derive(Clone, Debug, PartialEq)]
pub struct Fp3dmodel {
    pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D,   // m_Scale, m_Rotation, m_Offset
    pub opacity: f64,                                                    // m_Opacity
    pub filename: String,                                                // m_Filename
    pub show: bool,                                                      // m_Show
}
/// FOOTPRINT_VARIANT
#[derive(Clone, Debug, PartialEq)]
pub struct FootprintVariant {
    pub name: String, pub dnp: bool, pub excluded_from_bom: bool, pub excluded_from_sim: bool,
    pub excluded_from_pos_files: bool,
    pub fields: BTreeMap<String, String>,                                // m_fields
}
/// FOOTPRINT::FP_UNIT_INFO
#[derive(Clone, Debug, PartialEq)]
pub struct FpUnitInfo { pub unit_name: String, pub pins: Vec<String> }
/// JUMPER_GROUP_SET / JUMPER_GROUP (include/jumper_group.h)
#[derive(Clone, Debug, PartialEq, Default)]
pub struct JumperGroupSet { pub groups: Vec<JumperGroup> }               // m_groups
#[derive(Clone, Debug, PartialEq)]
pub struct JumperGroup { pub names: BTreeSet<String> }                   // m_names (never empty)

/// `CASE_INSENSITIVE_MAP<V>`: ordered by wxString::CmpNoCase, keeps the original key spelling.
pub struct CaseInsensitiveMap<V> { entries: BTreeMap<CaseInsensitiveKey, V> }

/// FOOTPRINT_GEOMETRY_CACHE_DATA
pub struct FootprintGeometryCacheData {
    pub bounding_box: Box2I,            pub bounding_box_timestamp: i32,
    pub text_excluded_bbox: Box2I,      pub text_excluded_bbox_timestamp: i32,
    pub hull: ShapePolySet,             pub hull_timestamp: i32,
}
/// FOOTPRINT_COURTYARD_CACHE_DATA. The MALFORMED_F/B_COURTYARD item flags that
/// BuildCourtyardCaches sets are stored here as well (and mirrored into `base.flags`).
pub struct FootprintCourtyardCacheData {
    pub front: ShapePolySet, pub back: ShapePolySet,
    pub front_hash: Hash128, pub back_hash: Hash128,
    pub malformed_front: bool, pub malformed_back: bool,
}
```

Dropped: `m_lastEditTime`, `m_arflag` (algorithm scratch: use a `SideTable`), `m_link` (editor
temporary), `m_transientComponentClassNames` (paste only), `m_searchTerms` and the
`LIB_TREE_ITEM` base (library browser), `m_fieldPropsStorageCache` (property grid),
the two cache mutexes.

## 5 Zone (`zone/mod.rs`, `zone_settings.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Zone {
    pub base: ConnectedItemBase,
    /// m_Poly. Footprint library frame for a footprint zone, board frame for a board zone
    /// (`library_outline()` / `board_outline(ctx)` port GetLibraryOutline / GetBoardOutline;
    /// `outline_mut(env)` ports Outline() for editing).
    pub(crate) lib_outline: ShapePolySet,
    pub corner_smoothing_type: CornerSmoothing,              // m_cornerSmoothingType
    pub corner_radius: u32,                                  // m_cornerRadius
    pub zone_name: String,                                   // m_zoneName
    pub layer_set: Lset,                                     // m_layerSet
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,   // m_layerProperties
    pub priority: u32,                                       // m_priority
    pub is_rule_area: bool,                                  // m_isRuleArea
    pub placement_area_enabled: bool,                        // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT,        // m_placementAreaSourceType
    pub placement_area_source: String,                       // m_placementAreaSource
    pub link_set: Kiid,                                      // m_linkSet
    pub repeat_options: RepeatLayoutOptions,                 // m_repeatOptions
    pub teardrop_type: TeardropType,                         // m_teardropType
    pub do_not_allow_zone_fills: bool,                       // m_doNotAllowZoneFills
    pub do_not_allow_vias: bool,                             // m_doNotAllowVias
    pub do_not_allow_tracks: bool,                           // m_doNotAllowTracks
    pub do_not_allow_pads: bool,                             // m_doNotAllowPads
    pub do_not_allow_footprints: bool,                       // m_doNotAllowFootprints
    pub pad_connection: ZoneConnection,                      // m_PadConnection
    pub zone_clearance: i32,                                 // m_ZoneClearance
    pub zone_min_thickness: i32,                             // m_ZoneMinThickness
    pub fill_version: i32,                                   // m_fillVersion
    pub island_removal_mode: IslandRemovalMode,              // m_islandRemovalMode
    pub min_island_area: i64,                                // m_minIslandArea (long long)
    pub thermal_relief_gap: i32,                             // m_thermalReliefGap
    pub thermal_relief_spoke_width: i32,                     // m_thermalReliefSpokeWidth
    pub fill_mode: ZoneFillMode,                             // m_fillMode
    pub hatch_thickness: i32,                                // m_hatchThickness
    pub hatch_gap: i32,                                      // m_hatchGap
    pub hatch_orientation: EdaOrientation,                   // m_hatchOrientation
    pub hatch_smoothing_level: i32,                          // m_hatchSmoothingLevel
    pub hatch_smoothing_value: f64,                          // m_hatchSmoothingValue
    pub hatch_hole_min_area: f64,                            // m_hatchHoleMinArea
    pub hatch_border_algorithm: i32,                         // m_hatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,                 // m_thievingSettings
    pub local_flgs: i32,                                     // m_localFlgs
    /// Fill results (class-2 derived state, written through Board::zone_fill_mut).
    pub(crate) fill: ZoneFill,
    pub border_style: ZoneBorderDisplayStyle,                // m_borderStyle
    pub border_hatch_pitch: i32,                             // m_borderHatchPitch
    pub border_hatch_lines: Vec<Seg>,                        // m_borderHatchLines
    pub area: f64,                                           // m_area
    pub outline_area: f64,                                   // m_outlinearea
    pub(crate) bbox_cache: KeyedCache<(Box2I, i32)>,         // m_bboxCache + m_bboxCacheTimeStamp (board time stamp)
}

/// Written by the zone filler and connectivity; read by DRC, plotting and the writer.
#[derive(Clone, Debug, Default)]
pub struct ZoneFill {
    /// m_FilledPolysList. `Arc` like the C++ `shared_ptr`: consumers may keep a snapshot while
    /// the filler installs a new one.
    pub filled_polys_list: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,
    pub fill_flags: Lset,                                    // m_fillFlags
    pub filled_polys_hash: BTreeMap<PcbLayerId, Hash128>,    // m_filledPolysHash
    pub insulated_islands: BTreeMap<PcbLayerId, BTreeSet<i32>>,   // m_insulatedIslands
    pub is_filled: bool,                                     // m_isFilled
    pub need_refill: bool,                                   // m_needRefill
}

#[repr(i32)] pub enum ZoneConnection { Inherited = -1, None = 0, Thermal, Full, ThtThermal }
pub enum ZoneFillMode { Polygons = 0, HatchPattern = 1, CopperThieving = 2 }
pub enum ThievingPattern { Dots = 0, Squares = 1, Hatch = 2 }
pub enum ZoneBorderDisplayStyle { NoHatch, DiagonalFull, DiagonalEdge, InvisibleBorder }
pub enum IslandRemovalMode { Always = 0, Never, Area }
pub enum PlacementSourceT { Sheetname = 0, ComponentClass, GroupPlacement, DesignBlock }
pub enum CornerSmoothing { NoSmoothing = 0, Chamfer, Fillet }        // ZONE_SETTINGS::CORNER_SMOOTHING

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ZoneLayerProperties { pub hatching_offset: Option<Vector2I> }
#[derive(Clone, Debug, PartialEq)]
pub struct ThievingSettings {
    pub pattern: ThievingPattern, pub element_size: i32, pub gap: i32, pub line_width: i32,
    pub stagger: bool, pub orientation: EdaOrientation,
}
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatLayoutOptions {
    pub copy_routing: bool,            // m_copyRouting (default true)
    pub connected_routing_only: bool,  // m_connectedRoutingOnly
    pub copy_placement: bool,          // m_copyPlacement (default true)
    pub copy_other_items: bool,        // m_copyOtherItems (default true)
    pub group_items: bool,             // m_groupItems
    pub include_locked_items: bool,    // m_includeLockedItems (default true)
    pub anchor: Kiid,                  // m_anchor
}
/// ISOLATED_ISLANDS (zone.h) - produced by connectivity, consumed by the filler and DRC.
pub struct IsolatedIslands { pub isolated_outlines: Vec<i32>, pub single_connection_outlines: Vec<i32> }

/// ZONE_SETTINGS (default zone settings of the board; also the `(zone_defaults ..)` reader).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneSettings {
    pub zone_priority: u32,                        // m_ZonePriority
    pub fill_mode: ZoneFillMode,                   // m_FillMode
    pub zone_clearance: i32,                       // m_ZoneClearance
    pub zone_min_thickness: i32,                   // m_ZoneMinThickness
    pub hatch_thickness: i32,                      // m_HatchThickness
    pub hatch_gap: i32,                            // m_HatchGap
    pub hatch_orientation: EdaAngle,               // m_HatchOrientation
    pub hatch_smoothing_level: i32,                // m_HatchSmoothingLevel
    pub hatch_smoothing_value: f64,                // m_HatchSmoothingValue
    pub hatch_hole_min_area: f64,                  // m_HatchHoleMinArea
    pub hatch_border_algorithm: i32,               // m_HatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,       // m_ThievingSettings
    pub netcode: i32,                              // m_Netcode
    pub name: String,                              // m_Name
    pub layers: Lset,                              // m_Layers
    pub zone_border_display_style: ZoneBorderDisplayStyle,   // m_ZoneBorderDisplayStyle
    pub border_hatch_pitch: i32,                   // m_BorderHatchPitch
    pub thermal_relief_gap: i64,                   // m_ThermalReliefGap (long)
    pub thermal_relief_spoke_width: i64,           // m_ThermalReliefSpokeWidth (long)
    pub locked: bool,                              // m_Locked
    pub teardrop_type: TeardropType,               // m_TeardropType
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,   // m_LayerProperties
    pub corner_smoothing_type: CornerSmoothing,    // m_cornerSmoothingType
    pub corner_radius: u32,                        // m_cornerRadius
    pub pad_connection: ZoneConnection,            // m_padConnection
    pub is_rule_area: bool,                        // m_isRuleArea
    pub placement_area_enabled: bool,              // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT,
    pub placement_area_source: String,
    pub keepout_do_not_allow_zone_fills: bool,     // m_keepoutDoNotAllowZoneFills
    pub keepout_do_not_allow_vias: bool,
    pub keepout_do_not_allow_tracks: bool,
    pub keepout_do_not_allow_pads: bool,
    pub keepout_do_not_allow_footprints: bool,
    pub remove_islands: IslandRemovalMode,         // m_removeIslands
    pub min_island_area: i64,                      // m_minIslandArea
}
```

Dropped: the three mutexes (`m_layerSetMutex`, `m_filledPolysListMutex`, `m_fillFlagsMutex`):
the sequential port writes these fields only in `&mut Board` phases.

## 6 Graphic shapes (`eda_shape/mod.rs`, `pcb_shape/mod.rs`, `line_ending.rs`)

```rust
#[repr(i32)]
pub enum ShapeT { Undefined = -1, Segment = 0, Rectangle, Arc, Circle, Poly, Bezier, Ellipse, EllipseArc }
#[repr(i32)]
pub enum FillT { NoFill = 1, FilledShape, FilledWithBgBodycolor, FilledWithColor, Hatch, ReverseHatch, CrossHatch }
pub enum LineEndingStyle { None = 0, Arrow, Circle, Square, ArrowOpen }

/// LINE_ENDING (include/line_ending.h)
#[derive(Clone, Debug, PartialEq)]
pub struct LineEnding {
    pub style: LineEndingStyle,     // m_style
    pub length: i32,                // m_length (0 = auto)
    pub width: i32,                 // m_width  (0 = auto)
    pub stroke: StrokeParams,       // m_stroke
}

/// ARC_MID
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ArcMid { pub mid: Vector2I, pub start: Vector2I, pub end: Vector2I, pub center: Vector2I }

/// EDA_SHAPE data (include/eda_shape.h). Board frame (the "baked" geometry).
#[derive(Clone, Debug)]
pub struct EdaShape {
    pub ends_swapped: bool,                 // m_endsSwapped
    pub shape: ShapeT,                      // m_shape
    pub stroke: StrokeParams,               // m_stroke
    pub start_ending: LineEnding,           // m_startEnding
    pub end_ending: LineEnding,             // m_endEnding
    pub fill: FillT,                        // m_fill
    pub fill_color: Color4D,                // m_fillColor
    pub rectangle_height: i64,              // m_rectangleHeight (long long)
    pub rectangle_width: i64,               // m_rectangleWidth
    pub corner_radius: i32,                 // m_cornerRadius
    pub start: Vector2I,                    // m_start
    pub end: Vector2I,                      // m_end
    pub arc_center: Vector2I,               // m_arcCenter
    pub arc_mid_data: ArcMid,               // m_arcMidData
    pub bezier_c1: Vector2I,                // m_bezierC1
    pub bezier_c2: Vector2I,                // m_bezierC2
    pub bezier_points: Vec<Vector2I>,       // m_bezierPoints
    pub ellipse: Ellipse<i32>,              // m_ellipse
    pub poly: ShapePolySet,                 // m_poly (C++: lazily allocated unique_ptr)
    pub proxy_item: bool,                   // m_proxyItem
    pub(crate) hatching_cache: OnceCache<EdaShapeHatchCacheData>,    // m_hatchingCache + m_hatchingDirty
}
pub struct EdaShapeHatchCacheData { pub hatching: ShapePolySet, pub hatch_lines: Vec<Seg> }

/// PCB_SHAPE (pcbnew/pcb_shape.h): BOARD_CONNECTED_ITEM + EDA_SHAPE + library-frame copy.
#[derive(Clone, Debug)]
pub struct PcbShape {
    pub base: ConnectedItemBase,
    pub shape: EdaShape,
    pub has_solder_mask: bool,                    // m_hasSolderMask
    pub solder_mask_margin: Option<i32>,          // m_solderMaskMargin
    pub(crate) lib: PcbShapeLib,                  // authoritative lib-frame geometry (lib_*() accessors)
}
/// The m_lib* members: authoritative geometry in the parent footprint's library frame
/// (equal to the board frame for board-level shapes; pad frame for pad primitives).
#[derive(Clone, Debug)]
pub struct PcbShapeLib {
    pub shape: ShapeT,                            // m_libShape
    pub start: Vector2I,                          // m_libStart
    pub end: Vector2I,                            // m_libEnd
    pub corner_radius: i32,                       // m_libCornerRadius
    pub arc_mid: Vector2I,                        // m_libArcMid
    pub bezier_c1: Vector2I,                      // m_libBezierC1
    pub bezier_c2: Vector2I,                      // m_libBezierC2
    pub poly: ShapePolySet,                       // m_libPoly
    pub ellipse_center: Vector2I,                 // m_libEllipseCenter
    pub ellipse_major_radius: i32,                // m_libEllipseMajorRadius
    pub ellipse_minor_radius: i32,                // m_libEllipseMinorRadius
    pub ellipse_rotation: EdaAngle,               // m_libEllipseRotation
    pub ellipse_start_angle: EdaAngle,            // m_libEllipseStartAngle
    pub ellipse_end_angle: EdaAngle,              // m_libEllipseEndAngle
}
```

Dropped from `EDA_SHAPE`: `m_blankEnding` (a constant), `m_editState` (interactive drawing).
The C++ invariant "every board-frame setter ends with `syncLibCoords()`, every parent transform
change ends with `RebakeFromLib()`" is ported literally: `PcbShape` setters take `&MutEnv` and
call `sync_lib_coords(env)`; `on_footprint_transformed(env)` calls `rebake_from_lib(env)`.

## 7 Text family (`eda_text.rs`, `pcb_text.rs`, `pcb_field.rs`, `pcb_textbox.rs`, `pcb_tablecell.rs`, `pcb_table.rs`)

```rust
/// EDA_TEXT data (include/eda_text.h). For footprint children `pos`, the size and thickness in
/// `attributes` are library-frame values (PCB_TEXT::GetTextPos etc. apply the parent transform).
#[derive(Clone, Debug)]
pub struct EdaText {
    pub hyperlink: String,                        // m_hyperlink
    pub text: String,                             // m_text
    pub shown_text: String,                       // m_shown_text (eager cache of unescaped text)
    pub shown_text_has_text_var_refs: bool,       // m_shown_text_has_text_var_refs
    pub attributes: TextAttributes,               // m_attributes (font, align, angle, spacing, stroke width,
                                                  //   italic, bold, underlined, color, mirrored, multiline,
                                                  //   size, keep_upright)
    pub unresolved_font_name: String,             // m_unresolvedFontName
    /// m_pos: library frame for footprint children (board frame via PcbText::text_pos(ctx)).
    pub(crate) lib_pos: Vector2I,
    pub visible: bool,                            // m_visible
    /// m_render_cache: also what `(render_cache ...)` in the file loads into, so it is data
    /// that survives Clone (unlike the other caches); validated by its stored text/angle/offset.
    pub(crate) render_cache: Option<Arc<EdaTextRenderCacheData>>,
    pub(crate) bbox_cache: std::sync::Mutex<BTreeMap<i32, (Vector2I, Box2I)>>,  // m_bbox_cache (cleared by setters)
}
/// EDA_TEXT_RENDER_CACHE_DATA. Also what `(render_cache ...)` in the file loads into: it lets
/// outline-font text be plotted without the font.
pub struct EdaTextRenderCacheData {
    pub text: String, pub font: Option<FontRef>, pub angle: EdaAngle, pub offset: Vector2I,
    pub mirrored: bool, pub glyphs: Vec<Glyph>,
}

/// PCB_TEXT
#[derive(Clone, Debug)]
pub struct PcbText {
    pub base: BoardItemBase,
    pub text: EdaText,
    pub(crate) lib_text_angle: EdaOrientation,    // m_libTextAngle
    pub(crate) knockout_cache: KeyedCache<PcbTextKnockoutCacheData>,   // m_knockout_cache
}
pub struct PcbTextKnockoutCacheData {
    pub text: String, pub text_attrs: TextAttributes, pub angle: EdaAngle, pub pos: Vector2I,
    pub cache: ShapePolySet,
}

/// PCB_FIELD : PCB_TEXT
#[derive(Clone, Debug)]
pub struct PcbField {
    pub text: PcbText,
    pub id: FieldT,                               // m_id
    pub ordinal: i32,                             // m_ordinal
    pub name: String,                             // m_name
    pub private: bool,                            // m_private
}
#[repr(i32)]
pub enum FieldT { User, Reference, Value, Footprint, Datasheet, Description, IntersheetRefs,
                  SheetName, SheetFilename, SheetUser, SheetRefSuffix }

/// PCB_TEXTBOX : PCB_SHAPE, EDA_TEXT
#[derive(Clone, Debug)]
pub struct PcbTextbox {
    pub shape: PcbShape,                          // the box (rectangle or, rotated, polygon)
    pub text: EdaText,
    pub border_enabled: bool,                     // m_borderEnabled
    pub margin_left: i32,                         // m_marginLeft
    pub margin_top: i32,                          // m_marginTop
    pub margin_right: i32,                        // m_marginRight
    pub margin_bottom: i32,                       // m_marginBottom
    pub lib_text_angle: EdaOrientation,           // m_libTextAngle
}

/// PCB_TABLECELL : PCB_TEXTBOX. `parent == ParentRef::Item(table)`.
#[derive(Clone, Debug)]
pub struct PcbTablecell {
    pub textbox: PcbTextbox,
    pub col_span: i32,                            // m_colSpan
    pub row_span: i32,                            // m_rowSpan
}

/// PCB_TABLE (and, with `generated`, PCB_DRILL_CHART / PCB_STACKUP_TABLE).
#[derive(Clone, Debug)]
pub struct PcbTable {
    pub base: BoardItemBase,
    pub stroke_external: bool,                    // m_strokeExternal
    pub stroke_header_separator: bool,            // m_StrokeHeaderSeparator
    pub border_stroke: StrokeParams,              // m_borderStroke
    pub stroke_rows: bool,                        // m_strokeRows
    pub stroke_columns: bool,                     // m_strokeColumns
    pub separators_stroke: StrokeParams,          // m_separatorsStroke
    pub col_count: i32,                           // m_colCount
    pub col_widths: BTreeMap<i32, i32>,           // m_colWidths
    pub row_heights: BTreeMap<i32, i32>,          // m_rowHeights
    pub cells: Vec<TablecellId>,                  // m_cells
    pub generated: Option<Box<GeneratedTableData>>,
}
/// PCB_GENERATED_TABLE members (pcbnew/pcb_generated_table.h). The cells are ordinary
/// `PcbTablecell`s read from the file; refreshing them (RefreshGeneratedTables) is tier 2.
#[derive(Clone, Debug)]
pub struct GeneratedTableData {
    pub columns: Vec<GeneratedTableColumn>,       // m_columns
    pub units: GeneratedTableUnits,               // m_units
    pub precision: i32,                           // m_precision
    pub row_keys: BTreeMap<i32, String>,          // m_rowKeys
    pub kind: GeneratedTableKind,
}
#[derive(Clone, Debug)]
pub enum GeneratedTableKind {
    /// PCB_DRILL_CHART_T (pcbnew/pcb_drill_chart.h)
    DrillChart {
        filter: DrillChartFilter,                 // m_filter
        show_totals: bool,                        // m_showTotals
        row_shapes: BTreeMap<i32, i32>,           // m_rowShapes
        symbol_column: i32,                       // m_symbolColumn
    },
    StackupTable,                                 // PCB_STACKUP_TABLE_T (no own members)
}
/// GENERATED_TABLE_COLUMN (board_tables/generated_table_schema.h)
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTableColumn {
    pub id: i32, pub heading: String, pub align: GeneratedTableAlign, pub width: i32, pub visible: bool,
}
pub enum GeneratedTableUnits { Mm, Inch, Mils }
pub enum GeneratedTableAlign { Left, Center, Right }
/// DRILL_CHART_FILTER (drill/drill_chart_template.h); all default true.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillChartFilter { pub plated: bool, pub non_plated: bool, pub vias: bool,
                              pub slots: bool, pub backdrills: bool, pub castellated: bool }
```

Dropped from `EDA_TEXT`: `m_activeUrl` (UI hover), `m_text_var_refs` (reactive text-variable
graph of the editor), `m_IuScale` (always `pcbIUScale`), `m_bbox_cacheMutex`.

## 8 Dimensions (`pcb_dimension.rs`)

```rust
/// PCB_DIMENSION_BASE : PCB_TEXT, with the five concrete classes as `kind`.
#[derive(Clone, Debug)]
pub struct PcbDimension {
    pub text: PcbText,
    pub override_text_enabled: bool,              // m_overrideTextEnabled
    pub value_string: String,                     // m_valueString
    pub prefix: String,                           // m_prefix
    pub suffix: String,                           // m_suffix
    pub units: EdaUnits,                          // m_units
    pub auto_units: bool,                         // m_autoUnits
    pub units_format: DimUnitsFormat,             // m_unitsFormat
    pub arrow_direction: DimArrowDirection,       // m_arrowDirection
    pub precision: DimPrecision,                  // m_precision
    pub suppress_zeroes: bool,                    // m_suppressZeroes
    pub line_thickness: i32,                      // m_lineThickness
    pub arrow_length: i32,                        // m_arrowLength
    pub extension_offset: i32,                    // m_extensionOffset
    pub text_position: DimTextPosition,           // m_textPosition
    pub keep_text_aligned: bool,                  // m_keepTextAligned
    pub measured_value: i32,                      // m_measuredValue
    pub(crate) lib_start: Vector2I,               // m_start (FP-relative inside a footprint)
    pub(crate) lib_end: Vector2I,                 // m_end
    /// m_shapes: derived by `update()`, board frame.
    pub shapes: Vec<SharedShape>,
    pub kind: DimensionKind,
}
#[derive(Clone, Debug)]
pub enum DimensionKind {
    Aligned(DimAligned),                                   // PCB_DIM_ALIGNED_T
    Orthogonal { aligned: DimAligned, orientation: DimOrthogonalDir },   // PCB_DIM_ORTHOGONAL_T (: PCB_DIM_ALIGNED)
    Radial { leader_length: i32 },                         // PCB_DIM_RADIAL_T   m_leaderLength
    Leader { text_border: DimTextBorder },                 // PCB_DIM_LEADER_T   m_textBorder
    Center,                                                // PCB_DIM_CENTER_T
}
#[derive(Clone, Debug)]
pub struct DimAligned {
    pub height: i32,                              // m_height
    pub extension_height: i32,                    // m_extensionHeight
    pub cross_bar_start: Vector2I,                // m_crossBarStart
    pub cross_bar_end: Vector2I,                  // m_crossBarEnd
}
pub enum DimOrthogonalDir { Horizontal, Vertical }                    // PCB_DIM_ORTHOGONAL::DIR
pub enum DimUnitsFormat { NoSuffix, BareSuffix, ParenSuffix }
#[allow(non_camel_case_types)]   // keeps the C++ spelling: the names are the digit patterns
pub enum DimPrecision { X = 0, X_X, X_XX, X_XXX, X_XXXX, X_XXXXX, V_VV, V_VVV, V_VVVV, V_VVVVV }
pub enum DimTextPosition { Outside, Inline, Manual }
pub enum DimUnitsMode { Inch, Mils, Mm, Automatic }
pub enum DimArrowDirection { Inward, Outward }
pub enum DimTextBorder { None, Rectangle, Circle, Roundrect }
```

Dropped: `m_inClearRenderCache`, `m_busy` (re-entrancy guards of the C++ virtual-call chain;
the Rust `update()` is a plain function).

## 9 Groups and generators (`pcb_group.rs`, `pcb_generator.rs`)

```rust
/// PCB_GROUP : BOARD_ITEM, EDA_GROUP
#[derive(Clone, Debug)]
pub struct PcbGroup {
    pub base: BoardItemBase,
    pub name: String,                             // EDA_GROUP::m_name
    pub members: Vec<ItemId>,                     // EDA_GROUP::m_items (insertion ordered, unique)
    pub design_block_lib_id: LibId,               // EDA_GROUP::m_designBlockLibId
}

/// PCB_GENERATOR : PCB_GROUP (tuning patterns, via stacks, via stitching).
#[derive(Clone, Debug)]
pub struct PcbGenerator {
    pub group: PcbGroup,                          // `group.base.layer` is PCB_GENERATOR::m_layer
    pub generator_type: String,                   // m_generatorType ("tuning_pattern", "via_stack", ...)
    pub origin: Vector2I,                         // m_origin
    pub is_dirty: bool,                           // m_isDirty
    /// What `GetProperties()` returns / `SetProperties()` receives (STRING_ANY_MAP, ordered by key).
    pub properties: BTreeMap<String, GeneratorProp>,
    /// GetTemplateItems(): inline, `parent == ParentRef::Item(generator)`.
    pub templates: Vec<(String, OwnedItem)>,
}
#[derive(Clone, Debug)]
pub enum GeneratorProp {
    Number(f64),                  // written with "{:.10g}"
    Bool(bool),
    Vector2I(Vector2I),           // (key (xy x y))
    LineChain(ShapeLineChain),    // (key (pts ...))
    Cells(Vec<Vector2I>),         // (key (cells (ij i j) ...))
    String(String),
}
```

The generator subclasses (`PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_VIA_STITCH`) keep their
typed members only as the property map. For `tuning_pattern` the C++ `SetProperties` /
`GetProperties` pair converts between millimetre doubles and IU integers; that normalisation
(about 150 lines of `generators/pcb_tuning_pattern.cpp`) is ported as
`PcbGenerator::normalize_properties()` so a re-save prints what C++ prints. Regeneration of the
pattern geometry is router work (Tier 3) and not part of the model.
`PCB_GENERATOR_POLY::m_outline` is represented by its property entry.
Dropped: `EDA_GROUP::m_deserializedItems` (IPC), `m_updateOrder` (`#ifdef GENERATOR_ORDER`).

## 10 Small and rare items

```rust
/// PCB_TARGET
#[derive(Clone, Debug)]
pub struct PcbTarget {
    pub base: BoardItemBase,
    pub shape: i32,               // m_shape (bit 0: 0 = +, 1 = X)
    pub size: i32,                // m_size
    pub line_width: i32,          // m_lineWidth
    pub pos: Vector2I,            // m_pos
}

/// PCB_POINT
#[derive(Clone, Debug)]
pub struct PcbPoint {
    pub base: BoardItemBase,
    pub(crate) lib_pos: Vector2I, // m_libPos
    pub(crate) pos: Vector2I,     // m_pos (baked board position, refreshed by on_footprint_transformed)
    pub size: i32,                // m_size
}

/// PCB_BARCODE
#[derive(Clone, Debug)]
pub struct PcbBarcode {
    pub base: BoardItemBase,
    pub width: i32,                               // m_width
    pub height: i32,                              // m_height
    pub(crate) lib_pos: Vector2I,                 // m_libPos (FP-relative in a footprint)
    pub margin: Vector2I,                         // m_margin
    pub text: PcbText,                            // m_text (inline; content, visibility = show text)
    pub kind: BarcodeT,                           // m_kind
    pub(crate) lib_angle: EdaOrientation,         // m_libAngle (FP-relative in a footprint)
    pub error_correction: BarcodeEccT,            // m_errorCorrection
    pub(crate) cache: KeyedCache<PcbBarcodeCache>,   // m_cache
}
pub enum BarcodeT { Code39 = 0, Code128 = 1, DataMatrix = 2, QrCode = 3, MicroQrCode = 4 }
pub enum BarcodeEccT { L = 1, M = 2, Q = 3, H = 4 }
pub struct PcbBarcodeCache {
    pub key_hash: u64, pub poly: ShapePolySet, pub symbol_poly: ShapePolySet,
    pub text_poly: ShapePolySet, pub bbox: Box2I, pub last_error: String,
}

/// PCB_REFERENCE_IMAGE + REFERENCE_IMAGE + the parts of BITMAP_BASE that are data.
#[derive(Clone, Debug)]
pub struct PcbReferenceImage {
    pub base: BoardItemBase,
    pub pos: Vector2I,                            // REFERENCE_IMAGE::m_pos
    pub transform_origin_offset: Vector2I,        // m_transformOriginOffset
    pub orientation: EdaOrientation,              // m_orientation
    pub opacity: f64,                             // m_opacity
    pub scale: f64,                               // BITMAP_BASE::m_scale
    /// The `(data ...)` payload exactly as in the file (base64 text of the PNG/JPEG stream).
    pub encoded_data: String,
    /// Width, height in pixels and PPI read from the image header only (PNG IHDR/pHYs, JPEG
    /// SOF/JFIF): enough for GetBoundingBox. Pixels are never decoded.
    pub pixel_size: Vector2I,
    pub ppi: i32,                                 // BITMAP_BASE::m_ppi
}

/// PCB_MARKER + MARKER_BASE. Never written to the board file.
#[derive(Clone, Debug)]
pub struct PcbMarker {
    pub base: BoardItemBase,
    pub pos: Vector2I,                            // MARKER_BASE::m_Pos
    pub marker_type: MarkerT,                     // m_markerType
    pub excluded: bool,                           // m_excluded
    pub comment: String,                          // m_comment
    pub rc_item: Arc<RcItem>,                     // m_rcItem
    pub path_shapes: Vec<PcbShape>,               // m_pathShapes (inline)
    pub path_start: Vector2I,                     // m_pathStart
    pub path_end: Vector2I,                       // m_pathEnd
    pub path_length: i32,                         // m_pathLength
}
pub enum MarkerT { Unspec, Erc, Drc, DrawingSheet, Ratsnest, Parity, Simulation }
/// Data of RC_ITEM / DRC_ITEM. `kicad-drc` creates these; rule and test-provider back pointers
/// of DRC_ITEM are replaced by names.
#[derive(Clone, Debug)]
pub struct RcItem {
    pub error_code: i32,                          // m_errorCode (DRCE_*)
    pub error_message: String,                    // m_errorMessage
    pub error_title: String,                      // m_errorTitle
    pub settings_key: String,                     // m_settingsKey
    pub ids: Vec<Kiid>,                           // m_ids (main item, aux item, ...)
    pub violating_rule_name: Option<String>,      // DRC_ITEM::m_violatingRule->m_Name
}

/// PCB_CONSTRAINT (pcbnew/constraints/pcb_constraint.h): pure data, KIID references.
#[derive(Clone, Debug)]
pub struct PcbConstraint {
    pub base: BoardItemBase,
    pub type_: PcbConstraintType,                 // m_type
    pub members: Vec<ConstraintMember>,           // m_members
    pub value: Option<f64>,                       // m_value
    pub driving: bool,                            // m_driving
}
pub enum PcbConstraintType { Undefined = 0, Coincident, Horizontal, Vertical, Parallel,
    Perpendicular, Collinear, Symmetric, EqualLength, EqualRadius, PointOnLine, Midpoint,
    FixedPosition, FixedLength, Concentric, FixedRadius, AngularDimension, Tangent, ArcAngle }
pub enum ConstraintAnchor { Whole = 0, Start, End, Mid, Center, Radius, Vertex }
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintMember { pub item: Kiid, pub anchor: ConstraintAnchor, pub index: i32 }

/// PCB_3D_BODY + EXTRUDED_3D_BODY (footprint child; written as `(model (type extruded) ...)`).
#[derive(Clone, Debug)]
pub struct Pcb3dBody {
    pub base: BoardItemBase,
    pub kind: Body3dKind,                         // m_kind
    pub height: i32,                              // m_extruded.m_height
    pub standoff: i32,                            // m_standoff
    pub layer: PcbLayerId,                        // m_layer (UNDEFINED = auto, UNSELECTED = pad_bbox)
    pub color: Color4D,                           // m_color
    pub material: ExtrusionMaterial,              // m_material
    pub show: bool,                               // m_show
    pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D,
}
pub enum Body3dKind { Extruded = 0, Model }
pub enum ExtrusionMaterial { Plastic = 0, Matte, Metal, Copper }

/// PCB_GRID_ITEM (pcbnew/pcb_grid_item.h): local grid (editor / router hint, no artwork).
#[derive(Clone, Debug)]
pub struct PcbGridItem {
    pub base: BoardItemBase,
    pub type_: PcbGridType,                       // m_type
    pub pos: Vector2I,                            // m_pos
    pub extent: Vector2I,                         // m_extent (cartesian half x/y; polar (rMax, -))
    pub spacing: Vector2I,                        // m_spacing
    pub orientation: EdaOrientation,              // m_orientation
    pub phi_extent: EdaAngle,                     // m_phiExtent (polar only)
    pub phi_spacing: EdaAngle,                    // m_phiSpacing (polar only)
    pub color: Color4D,                           // m_color
    pub priority: u32,                            // m_priority (default 1)
    pub tick_interval: u32,                       // m_tickInterval
    pub affects: PcbGridAffects,                  // m_affects
}
pub enum PcbGridType { Cartesian = 0, Polar = 1 }
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PcbGridAffects { pub cursor: bool, pub routing: bool, pub placement: bool }   // all default true

/// PCB_DRILL_MAP (pcbnew/pcb_drill_map.h): drill symbols drawn on its layer (drawing is tier 2).
#[derive(Clone, Debug)]
pub struct PcbDrillMap {
    pub base: BoardItemBase,                      // base.layer = the map layer
    pub offset: Vector2I,                         // m_offset
    pub symbol_size: i32,                         // m_symbolSize
    pub span: DrillSpan,                          // m_span
    pub all_spans: bool,                          // m_allSpans
    pub outline_slots: bool,                      // m_outlineSlots
    pub guide_cross: bool,                        // m_guideCross
}
/// DRILL_SPAN (drill/drill_span.h)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrillSpan { pub start_layer: PcbLayerId, pub end_layer: PcbLayerId,
                       pub is_backdrill: bool, pub is_non_plated_file: bool }
```

`PCB_BOARD_OUTLINE` (`BOARD::m_boardOutline`, a derived item holding the Edge.Cuts polygons) is
not an item here: it is the `board_outline` entry of `BoardCaches`.

Dropped in this group: `MARKER_BASE::m_scalingFactor`, `m_shapeBoundingBox` (marker glyph
drawing), `RC_ITEM::m_parent` (back pointer to the marker; the marker owns the `RcItem`),
`DRC_ITEM::m_violatingTest` (pointer to the provider), `BITMAP_BASE`'s `wxImage` / `wxBitmap`
members and preview state (pixels are never decoded), `PCB_DRILL_MAP::m_outlineCache` (rendering cache).

## 11 Nets and net classes (`netinfo.rs`, `netclass.rs`, `net_settings.rs`)

```rust
/// NETINFO_ITEM
#[derive(Clone, Debug)]
pub struct NetinfoItem {
    pub uuid: Kiid,                               // EDA_ITEM::m_Uuid (it is a BOARD_ITEM in C++)
    pub net_code: NetCode,                        // m_netCode
    pub netname: String,                          // m_netname
    pub short_netname: String,                    // m_shortNetname
    pub display_netname: String,                  // m_displayNetname
    pub net_chain: String,                        // m_netChain
    pub terminal_pads: [Option<PadId>; 2],        // m_terminalPads
    pub terminal_pad_uuids: [Kiid; 2],            // m_terminalPadUuids
    pub netclass: Option<Arc<Netclass>>,          // m_netClass (effective class, set by synchronize)
    pub is_current: bool,                         // m_isCurrent
}

/// NETINFO_LIST. Owner map is by code; `names` reproduces NETNAMES_MAP iteration (by name).
#[derive(Clone, Debug)]
pub struct NetinfoList {
    pub(crate) nets: BTreeMap<NetCode, NetinfoItem>,      // m_netCodes (+ ownership of m_netNames)
    pub(crate) names: BTreeMap<String, NetCode>,          // m_netNames
    pub(crate) new_net_code: i32,                         // m_newNetCode
    pub display_netnames_dirty: bool,                     // m_DisplayNetnamesDirty
}

/// NETCLASS (include/netclass.h)
#[derive(Clone, Debug)]
pub struct Netclass {
    pub is_default: bool,                         // m_isDefault
    pub constituents: Vec<Arc<Netclass>>,         // m_constituents + m_constituentOwners
    pub name: String,                             // m_Name
    pub priority: i32,                            // m_Priority
    pub description: String,                      // m_Description
    pub clearance: Option<i32>,                   // m_Clearance
    pub track_width: Option<i32>,                 // m_TrackWidth
    pub via_dia: Option<i32>,                     // m_ViaDia
    pub via_drill: Option<i32>,                   // m_ViaDrill
    pub uvia_dia: Option<i32>,                    // m_uViaDia
    pub uvia_drill: Option<i32>,                  // m_uViaDrill
    pub diff_pair_width: Option<i32>,             // m_diffPairWidth
    pub diff_pair_gap: Option<i32>,               // m_diffPairGap
    pub diff_pair_via_gap: Option<i32>,           // m_diffPairViaGap
    pub wire_width: Option<i32>,                  // m_wireWidth
    pub bus_width: Option<i32>,                   // m_busWidth
    pub schematic_color: Color4D,                 // m_schematicColor
    pub line_style: Option<i32>,                  // m_lineStyle
    pub pcb_color: Color4D,                       // m_pcbColor
    pub tuning_profile: String,                   // m_tuningProfile
    pub parents: NetclassParents,
}
/// The m_xxxParent pointers: which (simple) netclass a value of an aggregate class came from.
/// `None` means "this class itself" (C++ initialises the pointers to `this`).
#[derive(Clone, Debug, Default)]
pub struct NetclassParents {
    pub clearance: Option<Arc<Netclass>>,         pub track_width: Option<Arc<Netclass>>,
    pub via_diameter: Option<Arc<Netclass>>,      pub via_drill: Option<Arc<Netclass>>,
    pub uvia_dia: Option<Arc<Netclass>>,          pub uvia_drill: Option<Arc<Netclass>>,
    pub diff_pair_width: Option<Arc<Netclass>>,   pub diff_pair_gap: Option<Arc<Netclass>>,
    pub diff_pair_via_gap: Option<Arc<Netclass>>, pub pcb_color: Option<Arc<Netclass>>,
    pub wire_width: Option<Arc<Netclass>>,        pub bus_width: Option<Arc<Netclass>>,
    pub schematic_color: Option<Arc<Netclass>>,   pub line_style: Option<Arc<Netclass>>,
    pub tuning_profile: Option<Arc<Netclass>>,
}

pub enum NetChainSource { Schematic, Board }

/// NET_SETTINGS (include/project/net_settings.h). Loaded from the `.kicad_pro` JSON.
#[derive(Clone, Debug)]
pub struct NetSettings {
    pub default_netclass: Arc<Netclass>,                                   // m_defaultNetClass
    pub netclasses: BTreeMap<String, Arc<Netclass>>,                       // m_netClasses
    pub netclass_label_assignments: BTreeMap<String, BTreeSet<String>>,    // m_netClassLabelAssignments
    pub netclass_pattern_assignments: Vec<(EdaCombinedMatcher, String)>,   // m_netClassPatternAssignments
    pub netclass_chain_pattern_assignments:
        BTreeMap<NetChainSource, Vec<(EdaCombinedMatcher, String)>>,       // m_netClassChainPatternAssignments
    pub net_color_assignments: BTreeMap<String, Color4D>,                  // m_netColorAssignments
    pub net_chain_classes: BTreeMap<String, String>,                       // m_netChainClasses
    pub net_chain_net_classes: BTreeMap<String, String>,                   // m_netChainNetClasses
    pub netclass_bus_members: BTreeMap<String, BTreeSet<String>>,          // m_netclassBusMembers
    // derived (C++ fills them from the non-const GetEffectiveNetClass):
    pub(crate) composite_netclasses: KeyedCache<BTreeMap<String, Arc<Netclass>>>,     // m_compositeNetClasses
    pub(crate) implicit_netclasses: KeyedCache<BTreeMap<String, Arc<Netclass>>>,      // m_impicitNetClasses
    pub(crate) effective_netclass_cache: KeyedCache<BTreeMap<String, Arc<Netclass>>>, // m_effectiveNetclassCache
}
```

`EdaCombinedMatcher` is the port of `EDA_COMBINED_MATCHER` (`common/eda_pattern_match.cpp`,
wildcard + regex matching of net names against pattern assignments).

## 12 Design settings, stack-up, plot parameters, layers, page (`board_design_settings.rs`, `board_stackup.rs`, `pcb_plot_params.rs`, `title_block.rs`, `page_info.rs`)

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct ViaDimension { pub diameter: i32, pub drill: i32 }                    // VIA_DIMENSION
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct DiffPairDimension { pub width: i32, pub gap: i32, pub via_gap: i32 }  // DIFF_PAIR_DIMENSION
#[derive(Clone, Debug, PartialEq)]
pub struct ViaStackPreset {                                                      // VIA_STACK_PRESET
    pub name: String, pub start_layer: PcbLayerId, pub end_layer: PcbLayerId, pub staggered: bool,
    pub via_size: i32, pub via_drill: i32, pub use_netclass: bool, pub filled: bool,
    pub capped: bool, pub pitch: i32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextItemInfo { pub text: String, pub visible: bool, pub layer: PcbLayerId }   // TEXT_ITEM_INFO
pub const LAYER_CLASS_SILK: usize = 0;      pub const LAYER_CLASS_COPPER: usize = 1;
pub const LAYER_CLASS_EDGES: usize = 2;     pub const LAYER_CLASS_COURTYARD: usize = 3;
pub const LAYER_CLASS_FAB: usize = 4;       pub const LAYER_CLASS_OTHERS: usize = 5;
pub const LAYER_CLASS_COUNT: usize = 6;

/// BOARD_DESIGN_SETTINGS (include/board_design_settings.h).
#[derive(Clone, Debug)]
pub struct BoardDesignSettings {
    pub track_width_list: Vec<i32>,                          // m_TrackWidthList
    pub vias_dimensions_list: Vec<ViaDimension>,             // m_ViasDimensionsList
    pub via_stack_presets: Vec<ViaStackPreset>,              // m_ViaStackPresets
    pub diff_pair_dimensions_list: Vec<DiffPairDimension>,   // m_DiffPairDimensionsList
    pub teardrop_params_list: TeardropParametersList,        // m_TeardropParamsList
    /// m_SingleTrackMeanderSettings, m_DiffPairMeanderSettings, m_SkewMeanderSettings
    /// (PNS::MEANDER_SETTINGS): kept as the JSON objects read from the project file; only the
    /// length tuner reads them.
    pub meander_settings: [JsonValue; 3],
    pub current_via_type: Viatype,                           // m_CurrentViaType
    pub use_connected_track_width: bool,                     // m_UseConnectedTrackWidth
    pub temp_override_track_width: bool,                     // m_TempOverrideTrackWidth
    pub min_clearance: i32,                                  // m_MinClearance
    pub min_groove_width: i32,                               // m_MinGrooveWidth
    pub min_conn: i32,                                       // m_MinConn
    pub track_min_width: i32,                                // m_TrackMinWidth
    pub vias_min_annular_width: i32,                         // m_ViasMinAnnularWidth
    pub vias_min_size: i32,                                  // m_ViasMinSize
    pub min_through_drill: i32,                              // m_MinThroughDrill
    pub micro_vias_min_size: i32,                            // m_MicroViasMinSize
    pub micro_vias_min_drill: i32,                           // m_MicroViasMinDrill
    pub copper_edge_clearance: i32,                          // m_CopperEdgeClearance
    pub hole_clearance: i32,                                 // m_HoleClearance
    pub hole_to_hole_min: i32,                               // m_HoleToHoleMin
    pub silk_clearance: i32,                                 // m_SilkClearance
    pub min_resolved_spokes: i32,                            // m_MinResolvedSpokes
    pub min_silk_text_height: i32,                           // m_MinSilkTextHeight
    pub min_silk_text_thickness: i32,                        // m_MinSilkTextThickness
    pub drc_severities: BTreeMap<i32, Severity>,             // m_DRCSeverities (DRCE code -> severity)
    pub drc_exclusions: BTreeSet<DrcExclusion>,              // m_DrcExclusions
    pub zone_keep_external_fillets: bool,                    // m_ZoneKeepExternalFillets
    pub max_error: i32,                                      // m_MaxError
    pub solder_mask_expansion: i32,                          // m_SolderMaskExpansion
    pub solder_mask_min_width: i32,                          // m_SolderMaskMinWidth
    pub solder_mask_to_copper_clearance: i32,                // m_SolderMaskToCopperClearance
    pub solder_paste_margin: i32,                            // m_SolderPasteMargin
    pub solder_paste_margin_ratio: f64,                      // m_SolderPasteMarginRatio
    pub allow_soldermask_bridges_in_fps: bool,               // m_AllowSoldermaskBridgesInFPs
    pub tent_vias_front: bool,  pub tent_vias_back: bool,    // m_TentViasFront / Back
    pub cover_vias_front: bool, pub cover_vias_back: bool,   // m_CoverViasFront / Back
    pub plug_vias_front: bool,  pub plug_vias_back: bool,    // m_PlugViasFront / Back
    pub cap_vias: bool,                                      // m_CapVias
    pub fill_vias: bool,                                     // m_FillVias
    pub net_settings: NetSettings,                           // m_NetSettings (shared_ptr)
    pub default_fp_text_items: Vec<TextItemInfo>,            // m_DefaultFPTextItems
    pub user_layer_names: BTreeMap<String, String>,          // m_UserLayerNames
    pub zone_layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,   // m_ZoneLayerProperties
    pub line_thickness: [i32; LAYER_CLASS_COUNT],            // m_LineThickness
    pub text_size: [Vector2I; LAYER_CLASS_COUNT],            // m_TextSize
    pub text_thickness: [i32; LAYER_CLASS_COUNT],            // m_TextThickness
    pub text_italic: [bool; LAYER_CLASS_COUNT],              // m_TextItalic
    pub text_upright: [bool; LAYER_CLASS_COUNT],             // m_TextUpright
    pub dimension_units_mode: DimUnitsMode,                  // m_DimensionUnitsMode
    pub dimension_precision: DimPrecision,                   // m_DimensionPrecision
    pub dimension_units_format: DimUnitsFormat,              // m_DimensionUnitsFormat
    pub dimension_suppress_zeroes: bool,                     // m_DimensionSuppressZeroes
    pub dimension_text_position: DimTextPosition,            // m_DimensionTextPosition
    pub dimension_keep_text_aligned: bool,                   // m_DimensionKeepTextAligned
    pub dimension_arrow_length: i32,                         // m_DimensionArrowLength
    pub dimension_extension_offset: i32,                     // m_DimensionExtensionOffset
    pub style_fp_fields: bool, pub style_fp_text: bool, pub style_fp_shapes: bool,
    pub style_fp_dimensions: bool, pub style_fp_barcodes: bool,          // m_StyleFP*
    pub has_stackup: bool,                                   // m_HasStackup
    pub use_height_for_length_calcs: bool,                   // m_UseHeightForLengthCalcs
    pub aux_origin: Vector2I,                                // m_auxOrigin
    pub grid_origin: Vector2I,                               // m_gridOrigin
    pub track_width_index: i32,                              // m_trackWidthIndex
    pub via_size_index: i32,                                 // m_viaSizeIndex
    pub diff_pair_index: i32,                                // m_diffPairIndex
    pub via_stack_index: i32,                                // m_viaStackIndex
    pub use_custom_track_via: bool,                          // m_useCustomTrackVia
    pub custom_track_width: i32,                             // m_customTrackWidth
    pub custom_via_size: ViaDimension,                       // m_customViaSize
    pub use_custom_diff_pair: bool,                          // m_useCustomDiffPair
    pub custom_diff_pair: DiffPairDimension,                 // m_customDiffPair
    pub copper_layer_count: i32,                             // m_copperLayerCount
    pub user_defined_layer_count: i32,                       // m_userDefinedLayerCount
    pub enabled_layers: Lset,                                // m_enabledLayers
    pub board_thickness: i32,                                // m_boardThickness
    pub current_net_class_name: String,                      // m_currentNetClassName
    pub stackup: BoardStackup,                               // m_stackup
    /// m_drillSymbolProfile: the `(drill_symbol_profile ...)` node of `(setup ...)`, verbatim.
    pub drill_symbol_profile: Option<Sexpr>,
    pub default_zone_settings: ZoneSettings,                 // m_defaultZoneSettings
}

/// DRC_EXCLUSION (C++: a wrapper around the kiapi `DrcExclusion` message, read from and written
/// to the project file JSON). `Eq` / `Ord` are implemented on `sort_key` only, like
/// DRC_EXCLUSION_COMPARE and operator==.
#[derive(Clone, Debug)]
pub struct DrcExclusion {
    pub sort_key: String,          // GetSortKey()
    pub comment: String,           // GetComment()
    pub json: JsonValue,           // the exclusion object as stored in the .kicad_pro
}
/// `JsonValue` is the JSON value type of the JSON crate chosen in THIRD_PARTY_EQUIVALENTS.md.

/// BOARD_STACKUP (pcbnew/board_stackup_manager/board_stackup.h)
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackup {
    pub finish_type: String,                                 // m_FinishType
    pub has_dielectric_constrains: bool,                     // m_HasDielectricConstrains
    pub has_thickness_constrains: bool,                      // m_HasThicknessConstrains
    pub edge_connector_constraints: BsEdgeConnectorConstraints,   // m_EdgeConnectorConstraints
    pub edge_plating: bool,                                  // m_EdgePlating
    pub list: Vec<BoardStackupItem>,                         // m_list
}
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackupItem {
    pub type_: BoardStackupItemType,                         // m_Type
    pub layer_name: String,                                  // m_LayerName
    pub type_name: String,                                   // m_TypeName
    pub layer_id: PcbLayerId,                                // m_LayerId
    pub dielectric_layer_id: i32,                            // m_DielectricLayerId
    pub dielectric_prms_list: Vec<DielectricPrms>,           // m_DielectricPrmsList
    pub enabled: bool,                                       // m_enabled
}
#[derive(Clone, Debug, PartialEq)]
pub struct DielectricPrms {
    pub material: String,                                    // m_Material
    pub thickness: i32,                                      // m_Thickness
    pub thickness_locked: bool,                              // m_ThicknessLocked
    pub epsilon_r: f64,                                      // m_EpsilonR
    pub loss_tangent: f64,                                   // m_LossTangent
    pub spec_freq: f64,                                      // m_SpecFreq
    pub dielectric_model: DielectricModel,                   // m_DielectricModel
    pub color: String,                                       // m_Color
}
pub enum BoardStackupItemType { Undefined, Copper, Dielectric, Solderpaste, Soldermask, Silkscreen }
pub enum BsEdgeConnectorConstraints { None, InUse, Bevelled }
pub enum DielectricModel { Constant, DjordjevicSarkar }      // DIELECTRIC_MODEL

/// PCB_PLOT_PARAMS (pcbnew/pcb_plot_params.h); the `(pcbplotparams ...)` block of `(setup ...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct PcbPlotParams {
    pub format: PlotFormat,                                  // m_format
    pub layer_selection: Lset,                               // m_layerSelection
    pub plot_on_all_layers_sequence: Vec<PcbLayerId>,        // m_plotOnAllLayersSequence (LSEQ)
    pub skip_npth_pads: bool,                                // m_skipNPTH_Pads
    pub plot_pad_numbers: bool,                              // m_plotPadNumbers
    pub drill_marks: DrillMarks,                             // m_drillMarks
    pub text_mode: PlotTextMode,                             // m_textMode
    pub dxf_plot_mode: DxfOutlineMode,                       // m_DXFPlotMode
    pub dxf_units: DxfUnits,                                 // m_DXFUnits
    pub dxf_polygon_mode: bool,                              // m_DXFPolygonMode
    pub a4_output: bool,                                     // m_A4Output
    pub auto_scale: bool,                                    // m_autoScale
    pub scale: f64,                                          // m_scale
    pub mirror: bool,                                        // m_mirror
    pub negative: bool,                                      // m_negative
    pub black_and_white: bool,                               // m_blackAndWhite
    pub plot_drawing_sheet: bool,                            // m_plotDrawingSheet
    pub plot_via_on_mask_layer: Option<bool>,                // m_plotViaOnMaskLayer (legacy read)
    pub subtract_mask_from_silk: bool,                       // m_subtractMaskFromSilk
    pub use_gerber_protel_extensions: bool,                  // m_useGerberProtelExtensions
    pub use_gerber_x2_format: bool,                          // m_useGerberX2format
    pub gerber_disable_apert_macros: bool,                   // m_gerberDisableApertMacros
    pub include_gerber_netlist_info: bool,                   // m_includeGerberNetlistInfo
    pub create_gerber_job_file: bool,                        // m_createGerberJobFile
    pub gerber_precision: i32,                               // m_gerberPrecision
    pub svg_precision: u32,                                  // m_svgPrecision
    pub svg_fit_page_to_board: bool,                         // m_svgFitPageToBoard
    pub png_dpi: i32,                                        // m_pngDPI
    pub png_antialias: bool,                                 // m_pngAntialias
    pub use_aux_origin: bool,                                // m_useAuxOrigin
    pub output_directory: String,                            // m_outputDirectory
    pub scale_selection: i32,                                // m_scaleSelection
    pub plot_reference: bool,                                // m_plotReference
    pub plot_value: bool,                                    // m_plotValue
    pub plot_fp_text: bool,                                  // m_plotFPText
    pub sketch_pads_on_fab_layers: bool,                     // m_sketchPadsOnFabLayers
    pub sketch_pad_line_width: i32,                          // m_sketchPadLineWidth
    pub hide_dnp_fps_on_fab_layers: bool,                    // m_hideDNPFPsOnFabLayers
    pub sketch_dnp_fps_on_fab_layers: bool,                  // m_sketchDNPFPsOnFabLayers
    pub crossout_dnp_fps_on_fab_layers: bool,                // m_crossoutDNPFPsOnFabLayers
    pub fine_scale_adjust_x: f64,                            // m_fineScaleAdjustX
    pub fine_scale_adjust_y: f64,                            // m_fineScaleAdjustY
    pub width_adjust: i32,                                   // m_widthAdjust
    pub dashed_line_dash_ratio: f64,                         // m_dashedLineDashRatio
    pub dashed_line_gap_ratio: f64,                          // m_dashedLineGapRatio
    pub dxf_export_as_multi_layered_file: bool,              // m_DXFExportAsMultiLayeredFile
    pub layers_to_export: Vec<(PcbLayerId, String)>,         // m_layersToExport
    pub layer: PcbLayerId,                                   // m_layer
}

/// LAYER / LAYER_T (pcbnew/board.h)
#[repr(i32)]
pub enum LayerT { Undefined = -1, Signal, Power, Mixed, Jumper, Aux, Front, Back }
#[derive(Clone, Debug)]
pub struct Layer {
    pub name: String,             // m_name
    pub user_name: String,        // m_userName
    pub type_: LayerT,            // m_type
    pub visible: bool,            // m_visible
    pub number: i32,              // m_number
    pub opposite: i32,            // m_opposite
}

/// TITLE_BLOCK: index 0 title, 1 date, 2 revision, 3 company, 4.. comments 1-9.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleBlock { pub tb_texts: Vec<String> }          // m_tbTexts

/// PAGE_INFO
#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    pub type_: PageSizeType,      // m_type
    pub size: Vector2D,           // m_size (mils)
    pub portrait: bool,           // m_portrait
    pub description: String,      // m_description
}
pub enum PageSizeType { A5, A4, A3, A2, A1, A0, A, B, C, D, E, Gerber, USLetter, USLegal, USLedger, User }
```

Dropped from `BOARD_DESIGN_SETTINGS`: `m_DRCEngine` (layering inversion; the engine reaches
items through `BoardCtx::rules`), the `NESTED_SETTINGS` / `FIELDS_TABLE_BOM_SETTINGS` bases
(JSON plumbing and BOM). Dropped from `PCB_PLOT_PARAMS`: `m_colors`, `m_default_colors` (colour
themes are supplied by the caller of the plotter). Dropped from `PAGE_INFO`: `m_paper_id`
(wx enum), the static user size. `PlotFormat`, `DrillMarks`, `PlotTextMode`, `DxfOutlineMode`,
`DxfUnits` must be visible to both `kicad-pcb` and `kicad-plot`; they belong in `kicad-core`
(open question of the design).

## 13 Embedded files and component classes (`embedded_files.rs`, `component_class.rs`)

```rust
/// EMBEDDED_FILES
#[derive(Clone, Debug, Default)]
pub struct EmbeddedFiles {
    pub files: BTreeMap<String, Arc<EmbeddedFile>>,   // m_files
    pub font_files: Vec<String>,                      // m_fontFiles
    pub embed_fonts: bool,                            // m_embedFonts
}
/// EMBEDDED_FILES::EMBEDDED_FILE
#[derive(Clone, Debug)]
pub struct EmbeddedFile {
    pub name: String,
    pub type_: EmbeddedFileType,
    pub is_valid: bool,
    pub compressed_encoded_data: String,              // base64( zstd( data ) ), verbatim from the file
    pub decompressed_data: Vec<u8>,                   // filled only if a consumer asks (fonts)
    pub data_hash: String,
}
pub enum EmbeddedFileType { Font, Model, Worksheet, Datasheet, Other }

/// COMPONENT_CLASS
#[derive(Clone, Debug)]
pub struct ComponentClass {
    pub name: String,                                 // m_name
    pub constituent_classes: Vec<Arc<ComponentClass>>,   // m_constituentClasses
    pub usage_context: ComponentClassUsage,           // m_usageContext
}
pub enum ComponentClassUsage { Static, Dynamic, StaticAndDynamic, Effective }

/// COMPONENT_CLASS_MANAGER
#[derive(Clone, Debug)]
pub struct ComponentClassManager {
    pub none_class: Arc<ComponentClass>,                              // m_noneClass
    pub constituent_classes: BTreeMap<String, Arc<ComponentClass>>,   // m_constituentClasses
    pub effective_classes: BTreeMap<String, Arc<ComponentClass>>,     // m_effectiveClasses
    pub static_class_names_cache: BTreeSet<String>,                   // m_staticClassNamesCache
    /// m_assignmentRules: name + condition text; compiled by kicad-drc (custom rule expressions).
    pub assignment_rules: Vec<ComponentClassAssignmentRule>,
    pub ticker: i64,                                                  // m_ticker
}
pub struct ComponentClassCache {                                      // COMPONENT_CLASS_CACHE_PROXY (mutable part)
    pub last_ticker_value: i64,
    pub dynamic_component_class: Option<Arc<ComponentClass>>,
    pub final_component_class: Option<Arc<ComponentClass>>,
}
```

Dropped: `EMBEDDED_FILES::m_fileAddedCallback`, `COMPONENT_CLASS_MANAGER::m_board` (passed as
context).

## 14 The board (`board/mod.rs`)

```rust
pub struct Board {
    // ---- item arenas (generated by `board_items!`, board-model.md 2) ---------------------------
    pub(crate) footprints: Arena<Footprint>,
    pub(crate) pads: Arena<Pad>,
    pub(crate) tracks: Arena<Track>,
    pub(crate) zones: Arena<Zone>,
    pub(crate) shapes: Arena<PcbShape>,
    pub(crate) texts: Arena<PcbText>,
    pub(crate) fields: Arena<PcbField>,
    pub(crate) textboxes: Arena<PcbTextbox>,
    pub(crate) tables: Arena<PcbTable>,
    pub(crate) tablecells: Arena<PcbTablecell>,
    pub(crate) dimensions: Arena<PcbDimension>,
    pub(crate) barcodes: Arena<PcbBarcode>,
    pub(crate) images: Arena<PcbReferenceImage>,
    pub(crate) targets: Arena<PcbTarget>,
    pub(crate) points: Arena<PcbPoint>,
    pub(crate) groups: Arena<PcbGroup>,
    pub(crate) generators: Arena<PcbGenerator>,
    pub(crate) markers: Arena<PcbMarker>,
    pub(crate) constraints: Arena<PcbConstraint>,
    pub(crate) bodies: Arena<Pcb3dBody>,
    pub(crate) grid_items: Arena<PcbGridItem>,
    pub(crate) drill_maps: Arena<PcbDrillMap>,

    // ---- board-level order lists (C++ member containers, C++ element order) ---------------
    pub(crate) marker_list: Vec<MarkerId>,                   // m_markers
    pub(crate) drawing_list: VecDeque<ItemId>,               // m_drawings
    pub(crate) footprint_list: VecDeque<FootprintId>,        // m_footprints
    pub(crate) track_list: VecDeque<TrackId>,                // m_tracks
    pub(crate) group_list: Vec<GroupId>,                     // m_groups
    pub(crate) constraint_list: Vec<ConstraintId>,           // m_constraints
    pub(crate) zone_list: Vec<ZoneId>,                       // m_zones
    pub(crate) generator_list: Vec<GeneratorId>,             // m_generators
    pub(crate) point_list: Vec<PointId>,                     // m_points
    pub(crate) uuid_index: HashMap<Kiid, ItemId>,            // m_itemIndex (lookup only, never iterated)

    // ---- nets -----------------------------------------------------------------------------
    pub(crate) net_info: NetinfoList,                        // m_NetInfo
    pub net_chain_colors: BTreeMap<String, Color4D>,         // m_netChainColors

    // ---- settings and metadata -------------------------------------------------------------
    pub design_settings: BoardDesignSettings,                // m_designSettings
    pub(crate) layers: BTreeMap<i32, Layer>,                 // m_layers
    pub paper: PageInfo,                                     // m_paper
    pub titles: TitleBlock,                                  // m_titles
    pub plot_options: PcbPlotParams,                         // m_plotOptions
    pub properties: BTreeMap<String, String>,                // m_properties
    pub current_variant: String,                             // m_currentVariant
    pub variant_names: Vec<String>,                          // m_variantNames
    pub variant_descriptions: BTreeMap<String, String>,      // m_variantDescriptions
    pub embedded_files: EmbeddedFiles,                       // EMBEDDED_FILES base + m_embedFonts
    pub component_class_manager: ComponentClassManager,      // m_componentClassManager
    pub file_name: String,                                   // m_fileName
    pub generator: String,                                   // m_generator
    pub file_format_version_at_load: i32,                    // m_fileFormatVersionAtLoad
    pub board_use: BoardUse,                                 // m_boardUse
    pub user_units: EdaUnits,                                // m_userUnits
    pub outlines_chaining_epsilon: i32,                      // m_outlinesChainingEpsilon
    pub legacy_teardrops: bool,                              // m_legacyTeardrops
    pub legacy_design_settings_loaded: bool,                 // m_LegacyDesignSettingsLoaded
    pub legacy_copper_edge_clearance_loaded: bool,           // m_LegacyCopperEdgeClearanceLoaded
    pub legacy_netclasses_loaded: bool,                      // m_LegacyNetclassesLoaded
    /// What C++ reaches through `m_project`: project text variables and the few local
    /// settings the core reads. `None` = "no project" (SynchronizeNetsAndNetClasses returns early).
    pub project: Option<BoardProject>,

    // ---- bookkeeping -----------------------------------------------------------------------
    pub(crate) time_stamp: i32,                              // m_timeStamp
    pub(crate) layer_env: Arc<LayerEnv>,                     // snapshot for MutEnv (board-model.md 8.2)
    pub(crate) journal: ChangeJournal,                       // replaces m_listeners
    /// `kicad_core::KiidGenerator` (seedable MT19937, `Clone`): source of UUIDs for new items.
    pub kiid_generator: KiidGenerator,                       // deterministic by default; hosts reseed
    pub(crate) caches: BoardCaches,
}
// `Board: Clone` (caches and journal start empty in the clone; every handle stays valid in it)
// and `Board: Sync`. C++ forbids copying a BOARD; here it is a handful of Vec clones.

pub struct BoardProject {
    pub text_vars: BTreeMap<String, String>,                 // PROJECT::GetTextVars()
    pub prototype_zone_fill: bool,                           // PROJECT_LOCAL_SETTINGS::m_PrototypeZoneFill
    pub design_rules_path: String,                           // BOARD::GetDesignRulesPath()
}

/// Run-time caches of BOARD that pcbnew code outside DRC uses. All cleared by
/// `increment_time_stamp()`.
#[derive(Default)]
pub(crate) struct BoardCaches {
    pub max_clearance_value: KeyedCache<i32>,                           // m_maxClearanceValue
    pub zones_by_name: KeyedCache<BTreeMap<String, Vec<ZoneId>>>,       // m_ZonesByNameCache
    pub deflated_zone_outlines: KeyedCache<BTreeMap<ZoneId, Arc<ShapePolySet>>>,   // m_DeflatedZoneOutlineCache
    pub stacked_microvias: KeyedCache<BTreeSet<TrackId>>,               // m_StackedMicroviaCache
    pub footprint_courtyard_index: KeyedCache<Arc<FootprintCourtyardIndex>>,       // m_footprintCourtyardIndex
    pub layer_expressions: KeyedCache<BTreeMap<String, Lset>>,          // m_LayerExpressionCache
    pub board_outline: KeyedCache<Arc<ShapePolySet>>,                   // m_boardOutline + generation
}
```


## 15 Members deliberately dropped from `BOARD`

| C++ member | reason |
|---|---|
| `m_connectivity` | lives beside the board (`ConnectivityData`), reached through `BoardCtx::connectivity` |
| `m_listeners`, `m_textVarAdapter` | replaced by the change journal; reactive text-variable graph is editor-only |
| `m_highLight`, `m_highLightPrevious`, `m_LegacyVisibleLayers`, `m_LegacyVisibleItems` | view state |
| `m_drillModelGeneration`, `m_boardOutlineGeneration`, `m_drillSymbolLayers`, `m_drillSymbolPlacements`, `m_drillSymbolCache(+Mutex)` | drill chart / drill map live rendering (tier 2) |
| `m_IntersectsXxxCache`, `m_IntersectsXxxResultCache`, `m_EnclosedByAreaCache`, `m_ItemFieldCache`, `m_CopperZoneRTreeCache`, `m_CopperItemRTreeCache`, `m_ZoneBBoxCache`, `m_DRCZones`, `m_DRCCopperZones`, `m_DRCCopperZonesByLayer`, `m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance`, `m_SolderMaskBridges`, `m_ZoneIsolatedIslandsMap`, `m_CachesMutex` | owned by `kicad-drc` (`DrcBoardCaches`) and `kicad-zone`, keyed by `ItemId`; the zone bbox cache is per zone |
| `m_project` | replaced by `project: Option<BoardProject>` (plain data) |
| `m_lengthDelayCalc` | belongs with the length/delay calculation port (Tier 2, above the model) |
| `m_historyLifetime`, `m_embeddedFilesDelegate` | local history, footprint-editor delegate |
| `PROJECT::_ELEM` base | project element registry |

## 16 Dropped from `EDA_ITEM` / `BOARD_ITEM` (all item types)

`m_structType` (the Rust type / enum variant; `type_()` returns the `KicadT`),
`m_rolloverPos`, `m_isRollover`, `m_forceVisible`, `m_netHighlighted` (UI),
`m_customPropsStorageCache` (property grid), `m_owningIndex` (arena membership is the index),
the `KIGFX::VIEW_ITEM` and `SERIALIZABLE` bases (GAL view, protobuf IPC).
Not ported behaviour: `ViewGetLayers`, `ViewGetLOD`, `ViewBBox`, `GetMsgPanelInfo`,
`GetMenuImage`, `Serialize` / `Deserialize`, the `PROPERTY_MANAGER` registrations
(`*_DESC` structs), `Similarity`, `swapData` (replaced by whole-value before-images).
Ported, because DRC reports and the writer depend on them: `GetItemDescription`,
`GetFriendlyName`, `GetClass`, `LayerMaskDescribe`, every `Compare` / `cmp_*`.


## 17 Modelling level per type, and what stays opaque

Levels: **full** = all persisted data typed and round-tripped byte-exactly, plus all non-GUI
behaviour; **data** = all persisted data typed and round-tripped, behaviour reduced as stated.
Nothing is an opaque board item: the C++ parser rejects unknown tokens, the writer re-formats
every number, and every item takes part in save ordering and UUID indexing, so a verbatim body
would be byte-exact only for already-canonical files. (Bring-up exception: until package M23 /
the parser lands a type, the parser may hold its node as `Sexpr`; the corpus test skip-lists such
boards. The exit criterion is "no verbatim item for format 20261031".)

| C++ type | level | notes |
|---|---|---|
| `FOOTPRINT`, `PAD`, `PADSTACK`, `PCB_TRACK`, `PCB_ARC`, `PCB_VIA`, `ZONE`, `PCB_SHAPE` (8 kinds), `PCB_TEXT`, `PCB_FIELD`, `PCB_TEXTBOX`, `PCB_TABLE`, `PCB_TABLECELL`, 5 dimensions, `PCB_GROUP`, `PCB_TARGET`, `PCB_POINT`, `PCB_MARKER`, nets, net chains, netclasses, net settings, design settings, zone settings, stackup, plot params, title block, page, properties, variants, embedded files | full | |
| `PCB_GENERATOR` (tuning pattern, via stack, via stitch) | data | typed group + ordered property map + inline templates; tuning-pattern property normalisation ported so re-save matches; no regeneration (members are ordinary tracks / vias) |
| `PCB_DRILL_CHART`, `PCB_STACKUP_TABLE` | full as table, data as generator | cells as loaded are plotted and checked; `RefreshGeneratedTables` is tier 2 (M26) |
| `PCB_BARCODE` | data (+ tier-2 symbol) | text part full; symbol polygons need a zint-identical encoder (M25); until then the shape is the bounding rectangle and plotting reports the gap |
| `PCB_REFERENCE_IMAGE` | data | encoded image text kept verbatim; bbox from a header-only size/PPI probe; never plotted to fabrication layers (C++ neither) |
| `PCB_CONSTRAINT` | data | KIID members, dangling members preserved; no solver |
| `PCB_3D_BODY`, `FP_3DMODEL` | data | no 2D artwork |
| `PCB_GRID_ITEM` | data | editor / router hint |
| `PCB_DRILL_MAP` | data | drill symbols on its layer are a documented tier-2 gap |
| `PCB_BOARD_OUTLINE` | derived | `Board::board_outline(ctx)` cache, not saved |
| component classes | static full, dynamic rules data | assignment rule conditions are compiled by `kicad-drc` (tier 2) |

Kept verbatim (not interpreted), by design:

| what | type | why |
|---|---|---|
| `(setup (drill_symbol_profile ...))` | `BoardDesignSettings::drill_symbol_profile: Option<Sexpr>` | only drill map / chart rendering reads it (tier 2) |
| meander settings, DRC exclusion payloads | `JsonValue` | `.kicad_pro` data no tier-1 consumer interprets |
| embedded file payloads | `EmbeddedFile::compressed_encoded_data` | decompressed only when a consumer asks (fonts) |
| reference image data | `PcbReferenceImage::encoded_data` | pixels are never decoded |
