# Proposal B - arena / handle based board model for `kicad-pcb`

Status: design proposal (one of several competing angles). Author role: architect, no production
code. C++ reference tree: `<kicad>` at file format `SEXPR_BOARD_FILE_VERSION 20261031`.

All C++ facts quoted below were read from this tree, not from memory of older KiCad releases.
This matters: the tree differs from KiCad 9/10 in ways that shape the model (section 1).

---

## 0. Summary of the proposal

| Topic | Decision |
|---|---|
| Storage | Every board item lives in a board-level typed arena `Arena<T>` (generational slot vector). One arena per concrete Rust item type, 21 arenas, all declared by one table-driven macro. |
| Identity | Two identities, used for different things: `Kiid` (persistent, in the file, in DRC reports) and a handle `Id<T>` / `ItemId` (8 bytes, `Copy`, in-memory navigation, R-tree payloads, cache keys, wasm boundary). |
| Order | Arena slot order has no meaning. The C++ containers (`m_tracks`, `m_drawings`, `FOOTPRINT::m_pads` ...) become *order lists of handles* that reproduce C++ insertion order exactly (`push_back` / `push_front`). |
| Parent / child | `parent: ParentRef` in every item, child handle lists in `Footprint` / `PcbTable` / `PcbGroup`. No `Rc`, no `RefCell` graph, no lifetimes in item types. |
| Nets | Items store `NetCode` (the C++ `GetNetCode()` value). `NetinfoList` owns `NetinfoItem`s keyed by code and by name. Effective netclass is an `Arc<Netclass>` stored per net by `synchronize_nets_and_netclasses`. |
| Polymorphism | `ItemRef<'a>` = context + resolved reference; `trait BoardItem` carries the `BOARD_ITEM` virtuals with the base-class bodies as default methods; `AnyItem<'a>` enum for `switch( Type() )` / `static_cast`. |
| Context | Read methods take `&BoardCtx` (board + optional `RuleResolver` + optional `ConnectivityQuery`), because the C++ items reach their parent footprint, the board settings, the DRC engine and the connectivity through raw pointers. |
| Mutation | Through `Board` methods only (`add_*`, `remove`, `modify_*`, `footprint_*`). Leaf mutators take a small `MutEnv` snapshot instead of `&Board`. Every mutation bumps the board time stamp and appends to a pull-based change journal that replaces `BOARD_LISTENER`. |
| Caches | Per-item caches stay in the item (as in C++) behind `OnceLock` / `Mutex`, are cleared automatically whenever the board hands out `&mut`. Board-wide DRC caches move to `kicad-drc`, keyed by `ItemId`. |
| Undo / router commits | `BoardCommit` built on *parking*: a removed item stays in its slot, invisible, so revert restores it with the same handle. |
| wasm | `ItemId` packs into 53 bits and crosses the boundary as a JS `number`. No Rust object with a lifetime is ever exported. |
| Rare item types | Typed data for everything that carries a UUID or takes part in save ordering. Opaque s-expression bodies for `grid_item`, `drill_map`, generated-table configuration and the drill symbol profile (section 3.17). |

The price list (section 8) is honest: context parameters on most methods, a runtime (not
compile-time) notion of "this handle is alive", cascade logic that the type system does not
enforce, and a handle-remap step when a footprint moves between two boards.

---

## 1. Facts about the C++ model that drive the design

1. **Footprint children are stored in the footprint's library frame.** `FOOTPRINT` has
   `TRANSFORM_TRS m_transform` (translate, rotate, scale X/Y; `libs/kimath/include/geometry/transform_trs.h`).
   `PAD` stores `m_libPos` / `m_libOrientation` and `PAD::GetPosition()` returns
   `fp->GetTransform().Apply( m_libPos )`; `PAD::GetSize()`, `GetOffset()`, `GetPrimaryDrillSize()`
   also go through the parent transform. `PCB_TEXT` keeps a lib-frame position/size/angle in its
   `EDA_TEXT` base and converts on read. `PCB_SHAPE` keeps *both*: lib-frame members
   (`m_libStart`, `m_libEnd`, `m_libPoly`, ...) and a baked board-frame `EDA_SHAPE`, re-baked by
   `OnFootprintTransformed()`. `ZONE::Outline()` of a footprint zone is in the lib frame,
   `GetBoardOutline()` applies the transform. The file stores
   `(transform (translate x y) (rotate a) (scale sx sy))`.
   Consequence: almost every geometric getter of a footprint child needs the parent footprint.
2. **Items call upwards and sideways through raw pointers**: `GetParentFootprint()`,
   `GetBoard()->GetDesignSettings()`, `bds.m_DRCEngine->EvalRules(...)` (in `pad.cpp`,
   `pcb_track.cpp`, `pcb_shape.cpp`, `board_connected_item.cpp`), and
   `GetBoard()->GetConnectivity()->IsConnectedOnLayer(...)` (`PAD::FlashLayer`,
   `PCB_VIA::FlashLayer`). In Rust, `kicad-drc` and `kicad-connectivity` sit *above* `kicad-pcb`,
   so these become trait objects handed in through a context.
3. **Consumers make detached copies** (`PAD dummy( *pad ); dummy.SetSize(...)` in
   `plot_board_layers.cpp`, `zone_filler.cpp`; `ZONE dummy( *zone )`). The copy keeps the parent
   pointer and must still answer `GetPosition()`.
4. **Iteration order is observable.** `DRC_TEST_PROVIDER::forEachGeometryItem` walks
   `Tracks()`, `Drawings()`, `Zones()`, then `Footprints()` with `GetFields()`, `Pads()`, ... in
   container order. `BOARD::Add` uses `push_front` for `ADD_MODE::INSERT` and `push_back` for
   `APPEND` / `BULK_APPEND` on tracks, footprints and drawings (`pcbnew/board.cpp:1520`).
5. **Saving re-sorts everything** into `std::set`s with comparators
   (`BOARD_ITEM::ptr_cmp`, `BOARD::cmp_drawings`, `PCB_TRACK::cmp_tracks`,
   `FOOTPRINT::cmp_pads / cmp_drawings / cmp_zones`, `PCB_POINT::cmp_points`,
   `PCB_GROUP::ptr_cmp`), all of which end with `m_Uuid` and then pointer order
   (`pcb_io_kicad_sexpr.cpp:908`). `cmp_tracks` sorts by **net code** first.
6. **Lookup by UUID is a first-class service** (`BOARD_ITEM_INDEX`, `BOARD::ResolveItem`), and
   duplicate UUIDs in files are tolerated and repaired (`RepairDuplicateItemUuids`).
7. **Groups, generators, constraints, net-chain terminal pads and zone link sets refer to other
   items**, by pointer (`EDA_GROUP::m_items`) or by `KIID` (`CONSTRAINT_MEMBER::m_item`,
   `NETINFO_ITEM::m_terminalPadUuids`).
8. **Zone filling and connectivity write back**: `ZONE::SetFilledPolysList`, `SetFillFlag`,
   `SetIsIsland`, `PAD/PCB_VIA::SetZoneLayerOverride`, `BOARD_CONNECTED_ITEM::SetNetCode`
   (net propagation onto vias/tracks).
9. New item kinds in this tree: `PCB_POINT`, `PCB_BARCODE`, `PCB_CONSTRAINT`, `PCB_GRID_ITEM`,
   `PCB_DRILL_MAP`, `PCB_DRILL_CHART`, `PCB_STACKUP_TABLE` (both `PCB_GENERATED_TABLE`),
   `PCB_3D_BODY`, `PCB_BOARD_OUTLINE`, ellipse shapes, line endings, copper thieving zones,
   net chains, board/footprint variants, custom per-item properties.

---

## 2. Ownership / identity architecture

### 2.1 Two identities

| | `Kiid` | handle (`Id<T>`, `ItemId`) |
|---|---|---|
| lifetime | persistent, written to the file | one `Board` value in memory (a `Board::clone()` keeps them valid) |
| uniqueness | *should* be unique; duplicates tolerated like C++ | unique among live items by construction |
| used for | file, DRC report, exclusions, constraints, cross references in text, stable JS ids across save/load | parent/child links, order lists, group members, R-tree payload, cache keys, connectivity items, wasm calls |
| lookup | `board.resolve_item(&kiid) -> Option<ItemId>` (hash map) | `board.get(id)` (array index + generation compare) |

Rule for implementers: **store handles, pass `ItemRef`, persist `Kiid`.** A `BOARD_ITEM*` kept in
a C++ member or container becomes an `ItemId` (or typed id); a `BOARD_ITEM*` function parameter
becomes an `ItemRef<'_>`.

### 2.2 Handles and the arena (`arena.rs`, `ids.rs`; new code, no C++ counterpart)

```rust
/// Typed handle into an `Arena<T>`. 8 bytes, `Copy`, no lifetime.
pub struct Id<T> {
    index: u32,
    generation: u16,
    _marker: core::marker::PhantomData<fn() -> T>,
}
// Manual impls (no bound on T): Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug.
impl<T> Id<T> {
    pub fn index(self) -> u32;
    pub fn generation(self) -> u16;
}

/// Which arena a handle belongs to. Values are part of the wasm ABI: append only.
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum ItemKind {
    Footprint = 1, Pad = 2, Track = 3, Zone = 4, Shape = 5, Text = 6, Field = 7, Textbox = 8,
    Table = 9, Tablecell = 10, Dimension = 11, Barcode = 12, Image = 13, Target = 14,
    Point = 15, Group = 16, Generator = 17, Marker = 18, Constraint = 19, Body3d = 20,
    Opaque = 21,
    /// Not an arena: `index` is the net code. Lets `ResolveItem` return a NETINFO_ITEM.
    Net = 22,
}

/// Untyped handle = "BOARD_ITEM*". 8 bytes, `Copy`, totally ordered (deterministic tie-break
/// wherever C++ falls back to comparing pointers).
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct ItemId {
    kind: ItemKind,
    generation: u16,
    index: u32,
}

impl ItemId {
    pub fn kind(self) -> ItemKind;
    /// `index | generation << 32 | kind << 48`; always < 2^53, exact as a JS number.
    pub fn to_bits(self) -> u64;
    pub fn from_bits(bits: u64) -> Option<ItemId>;
    pub fn as_footprint(self) -> Option<FootprintId>;
    pub fn as_pad(self) -> Option<PadId>;
    pub fn as_track(self) -> Option<TrackId>;
    pub fn as_zone(self) -> Option<ZoneId>;
    // ... one per kind, generated by the macro
    pub fn as_net(self) -> Option<NetCode>;
}
impl From<PadId> for ItemId { /* generated, one per kind */ }

pub type FootprintId  = Id<Footprint>;
pub type PadId        = Id<Pad>;
pub type TrackId      = Id<Track>;          // PCB_TRACK, PCB_ARC and PCB_VIA share m_tracks
pub type ZoneId       = Id<Zone>;
pub type ShapeId      = Id<PcbShape>;
pub type TextId       = Id<PcbText>;
pub type FieldId      = Id<PcbField>;
pub type TextboxId    = Id<PcbTextbox>;
pub type TableId      = Id<PcbTable>;
pub type TablecellId  = Id<PcbTablecell>;
pub type DimensionId  = Id<PcbDimensionBase>;
pub type BarcodeId    = Id<PcbBarcode>;
pub type ImageId      = Id<PcbReferenceImage>;
pub type TargetId     = Id<PcbTarget>;
pub type PointId      = Id<PcbPoint>;
pub type GroupId      = Id<PcbGroup>;
pub type GeneratorId  = Id<PcbGenerator>;
pub type MarkerId     = Id<PcbMarker>;
pub type ConstraintId = Id<PcbConstraint>;
pub type Body3dId     = Id<Pcb3dBody>;
pub type OpaqueId     = Id<OpaqueItem>;

/// Net code as returned by `BOARD_CONNECTED_ITEM::GetNetCode()`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default)]
pub struct NetCode(pub i32);
impl NetCode {
    pub const UNCONNECTED: NetCode = NetCode(0);   // NETINFO_LIST::UNCONNECTED
    pub const ORPHANED: NetCode = NetCode(-1);     // NETINFO_LIST::ORPHANED
}
```

```rust
/// Generational slot vector. `#![forbid(unsafe_code)]` compatible (plain `Vec` + enum).
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free_head: u32,          // u32::MAX = none
    live: u32,
}
struct Slot<T> { generation: u16, state: SlotState<T> }
enum SlotState<T> {
    Live(T),
    /// Removed by an open `BoardCommit`: invisible to `get`/`iter`, restorable with the
    /// same handle (the C++ equivalent is an item kept alive by the undo list).
    Parked(T),
    /// Moved out temporarily by `Board::with_item_taken` (section 2.7.4).
    Taken,
    Free { next_free: u32 },
    /// Generation counter exhausted (65535 reuses); the slot is never handed out again.
    Retired,
}

impl<T> Arena<T> {
    pub fn new() -> Self;
    pub fn insert(&mut self, value: T) -> Id<T>;
    pub fn remove(&mut self, id: Id<T>) -> Option<T>;        // frees the slot, bumps generation
    pub fn contains(&self, id: Id<T>) -> bool;               // Live only
    pub fn get(&self, id: Id<T>) -> Option<&T>;              // Live only
    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T>;
    pub fn get2_mut(&mut self, a: Id<T>, b: Id<T>) -> Option<(&mut T, &mut T)>; // split_at_mut
    pub fn park(&mut self, id: Id<T>) -> bool;
    pub fn unpark(&mut self, id: Id<T>) -> bool;
    pub fn get_parked(&self, id: Id<T>) -> Option<&T>;
    pub fn take(&mut self, id: Id<T>) -> Option<T>;          // Live -> Taken
    pub fn put_back(&mut self, id: Id<T>, value: T);         // Taken -> Live
    pub fn len(&self) -> usize;                              // live items
    pub fn slot_count(&self) -> usize;                       // for dense side tables
    /// Slot order. NOT a semantic order: semantic iteration uses the board's order lists.
    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)>;
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Id<T>, &mut T)>;
}
impl<T> core::ops::Index<Id<T>> for Arena<T> { type Output = T; /* panics on a stale handle */ }
impl<T> core::ops::IndexMut<Id<T>> for Arena<T> {}

/// Dense per-arena side table, generation checked. Replaces `std::map<BOARD_ITEM*, V>` and
/// scratch members such as `m_arflag`, `m_subRatsnest`, temp `EDA_ITEM_FLAGS`.
pub struct SideTable<T, V> { data: Vec<Option<(u16, V)>>, _marker: PhantomData<fn() -> T> }
impl<T, V> SideTable<T, V> {
    pub fn get(&self, id: Id<T>) -> Option<&V>;
    pub fn insert(&mut self, id: Id<T>, value: V) -> Option<V>;
    pub fn remove(&mut self, id: Id<T>) -> Option<V>;
}
```

`board[id]` (panicking) is for handles obtained from the board during the same borrow;
`board.get(id)` (`Option`) is for handles that crossed an API boundary (JS, a stored
connectivity item, a commit).

### 2.3 Board storage layout

One table declares all arenas. The macro expands to: the `ItemKind` <-> typed id conversions,
the arena fields of `Board`, `impl Index<XxxId> for Board`, the `AnyItem` / `AnyItemMut` /
`OwnedItem` enums and `Board::get(id)`. Adding an item type is one line here plus its struct.

```rust
board_items! {
//  kind        handle         arena field    Rust type            C++ classes
    Footprint   FootprintId    footprints     Footprint;        // FOOTPRINT
    Pad         PadId          pads           Pad;              // PAD
    Track       TrackId        tracks         Track;            // PCB_TRACK, PCB_ARC, PCB_VIA
    Zone        ZoneId         zones          Zone;             // ZONE
    Shape       ShapeId        shapes         PcbShape;         // PCB_SHAPE
    Text        TextId         texts          PcbText;          // PCB_TEXT
    Field       FieldId        fields         PcbField;         // PCB_FIELD
    Textbox     TextboxId      textboxes      PcbTextbox;       // PCB_TEXTBOX
    Table       TableId        tables         PcbTable;         // PCB_TABLE, PCB_DRILL_CHART, PCB_STACKUP_TABLE
    Tablecell   TablecellId    tablecells     PcbTablecell;     // PCB_TABLECELL
    Dimension   DimensionId    dimensions     PcbDimensionBase; // PCB_DIM_ALIGNED/ORTHOGONAL/RADIAL/LEADER/CENTER
    Barcode     BarcodeId      barcodes       PcbBarcode;       // PCB_BARCODE
    Image       ImageId        images         PcbReferenceImage;// PCB_REFERENCE_IMAGE
    Target      TargetId       targets        PcbTarget;        // PCB_TARGET
    Point       PointId        points         PcbPoint;         // PCB_POINT
    Group       GroupId        groups         PcbGroup;         // PCB_GROUP
    Generator   GeneratorId    generators     PcbGenerator;     // PCB_GENERATOR and subclasses
    Marker      MarkerId       markers        PcbMarker;        // PCB_MARKER
    Constraint  ConstraintId   constraints    PcbConstraint;    // PCB_CONSTRAINT
    Body3d      Body3dId       bodies         Pcb3dBody;        // PCB_3D_BODY
    Opaque      OpaqueId       opaque         OpaqueItem;       // PCB_GRID_ITEM, PCB_DRILL_MAP
}
```

Why typed arenas and not one `Arena<OwnedItem>`: (1) typed handles (`PadId`, `ZoneId`) make
most links self-checking at compile time (`Footprint::pads: VecDeque<PadId>` cannot hold a
zone); (2) hot loops over one type (`tracks`, `pads`) touch dense homogeneous memory; (3)
field-disjoint borrows between arenas are what makes footprint cascades (2.7.3) compile without
`unsafe`; (4) a segment costs the size of `Track`, not the size of the largest item type.
The untyped `ItemId` exists for the places where C++ really means "any `BOARD_ITEM*`"
(drawings lists, group members, R-tree payloads, cache keys, the JS boundary).

Generated enums:

```rust
/// Resolved shared reference: the Rust form of "a BOARD_ITEM* I am about to switch on".
#[derive(Clone, Copy)]
pub enum AnyItem<'a> {
    Footprint(&'a Footprint), Pad(&'a Pad), Track(&'a Track), Zone(&'a Zone),
    Shape(&'a PcbShape), Text(&'a PcbText), Field(&'a PcbField), Textbox(&'a PcbTextbox),
    Table(&'a PcbTable), Tablecell(&'a PcbTablecell), Dimension(&'a PcbDimensionBase),
    Barcode(&'a PcbBarcode), Image(&'a PcbReferenceImage), Target(&'a PcbTarget),
    Point(&'a PcbPoint), Group(&'a PcbGroup), Generator(&'a PcbGenerator),
    Marker(&'a PcbMarker), Constraint(&'a PcbConstraint), Body3d(&'a Pcb3dBody),
    Opaque(&'a OpaqueItem), Net(&'a NetinfoItem),
}
pub enum AnyItemMut<'a> { /* same variants with &'a mut, without Net */ }
/// Owned item outside any arena: before-images in commits, builder input, copies.
pub enum OwnedItem { Footprint(Box<Footprint>), Pad(Box<Pad>), Track(Box<Track>), /* ... */ }
```

Board-level order lists (the C++ member containers, same element order):

| C++ member (`board.h`) | Rust field | element | insert rule (`BOARD::Add`) |
|---|---|---|---|
| `TRACKS m_tracks` (`std::deque`) | `track_list: VecDeque<TrackId>` | segment, arc, via | `push_back` for `Append`/`BulkAppend`, else `push_front` |
| `FOOTPRINTS m_footprints` (`std::deque`) | `footprint_list: VecDeque<FootprintId>` | | same |
| `DRAWINGS m_drawings` (`std::deque<BOARD_ITEM*>`) | `drawing_list: VecDeque<ItemId>` | Shape, Text, Textbox, Table, Dimension, Barcode, Image, Target, Opaque | same |
| `ZONES m_zones` (`std::vector`) | `zone_list: Vec<ZoneId>` | | `push_back` |
| `GROUPS m_groups` | `group_list: Vec<GroupId>` | | `push_back` |
| `GENERATORS m_generators` | `generator_list: Vec<GeneratorId>` | | `push_back` |
| `CONSTRAINTS m_constraints` | `constraint_list: Vec<ConstraintId>` | | `push_back` |
| `PCB_POINTS m_points` | `point_list: Vec<PointId>` | | `push_back` |
| `MARKERS m_markers` | `marker_list: Vec<MarkerId>` | | `push_back` |
| `BOARD_ITEM_INDEX m_itemIndex` | `uuid_index: HashMap<Kiid, ItemId>` | never iterated | `bind_item` / `unbind` / `rebind` |

Footprint-level order lists live in `Footprint` (section 3.4): `fields: Vec<FieldId>`,
`drawings: VecDeque<ItemId>`, `pads: VecDeque<PadId>`, `zones: Vec<ZoneId>`,
`groups: Vec<GroupId>`, `constraints: Vec<ConstraintId>`, `points: Vec<PointId>`,
`bodies: VecDeque<Body3dId>`; table cells live in `PcbTable::cells: Vec<TablecellId>`.
Note the asymmetry that must be ported as is: `FOOTPRINT::Add` appends only for
`ADD_MODE::APPEND` (every other mode inserts at the front; fields always append), while
`BOARD::Add` also appends for `BULK_APPEND`.

Removal from an order list is a linear `retain`, the same cost as the C++ `std::erase` on a
deque; `Board::remove_many(&[ItemId])` does one pass per list for bulk deletes (router commits).

### 2.4 Parent / child, groups, nets

```rust
/// `EDA_ITEM::m_parent`. 8 bytes (the enum uses the niche in `ItemKind`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ParentRef {
    /// Detached item (builder, dummy for a temporary computation whose original had no parent).
    #[default]
    None,
    /// Direct child of the board.
    Board,
    /// Child of another item: a footprint (pads, fields, graphics, zones, groups, points,
    /// bodies), a table (cells), a pad (custom-shape primitives, stored inline in the padstack),
    /// a barcode (its inline text), a generator (template items).
    Item(ItemId),
}
```

* **pad -> footprint**: `pad.base.item.parent == ParentRef::Item(fp.into())`, and
  `fp.pads` contains the `PadId`. `Pad::parent_footprint(&self, ctx) -> Option<&Footprint>` is one
  arena index. The generic `ItemRef::parent_footprint` walks `ParentRef` exactly like
  `EDA_ITEM::findParent( PCB_FOOTPRINT_T )` (at most two steps: cell -> table -> footprint).
* **footprint graphics / zones / fields / points / groups**: same pattern; the child struct is
  the *same Rust type* as its board-level sibling (as in C++), only `parent` differs.
* **group membership**: `base.group: Option<ItemId>` (kind `Group` or `Generator`, mirrors
  `EDA_ITEM::m_group`) and `PcbGroup::members: Vec<ItemId>` (insertion ordered, unique). C++ uses
  `std::unordered_set<EDA_ITEM*>`; its iteration order is unspecified and every order-sensitive
  consumer sorts (the writer sorts member UUID strings), so an insertion-ordered `Vec` is a
  legal, deterministic refinement.
* **net -> items**: not stored (C++ does not store it either; `BOARD::TracksInNet` scans and
  connectivity keeps per-net lists). `Board::tracks_in_net( NetCode )` is the same scan;
  the indexed version belongs to `kicad-connectivity`.
* **item -> net**: `ConnectedItemBase::net: NetCode`. A new item has `NetCode::UNCONNECTED`
  (the C++ orphaned `NETINFO_ITEM` reports code 0). `set_net_code` ports
  `BOARD_CONNECTED_ITEM::SetNetCode` literally: code forced to 0 off copper, a negative code or a
  missing board gives the orphaned net (0), and a code the board does not know leaves the item
  with `NetCode(-1)`, which is what `GetNetCode()` returns for a null `m_netinfo`.
* **references by KIID stay KIIDs**: `ConstraintMember::item`, `NetinfoItem::terminal_pad_uuids`,
  `Zone::link_set`, `RepeatLayoutOptions::anchor`. They are resolved on demand with
  `board.resolve_item`, which is what C++ does, and they survive a dangling target (the
  constraint "round-trips in its error state").

### 2.5 Polymorphic access

Three layers, each with a clear job:

1. **Plain structs with inherent methods.** This is where ported code lives, one module per C++
   file. Methods that need anything outside the struct take `ctx: &BoardCtx`.
2. **`trait BoardItem` / `trait BoardConnectedItem`** (section 4): the `BOARD_ITEM` /
   `BOARD_CONNECTED_ITEM` virtual interface. Default method bodies are the C++ base-class
   bodies, so a type only overrides what its C++ class overrides. Implemented for every arena
   type (and for `PcbTrack` / `PcbArc` / `PcbVia` behind `Track`).
3. **`ItemRef<'a>`**: `(ctx, Option<ItemId>, AnyItem<'a>)`. It forwards the trait methods with
   the context already bound, so consumer code reads like the C++:
   `item.layer_set()`, `item.effective_shape( layer, Flashing::Default, usage )`,
   `item.parent_footprint()`, `item.net_code()`.

```rust
#[derive(Clone, Copy)]
pub struct ItemRef<'a> {
    pub ctx: &'a BoardCtx<'a>,
    id: Option<ItemId>,          // None for detached dummies and inline sub-items
    item: AnyItem<'a>,
}
impl<'a> BoardCtx<'a> {
    pub fn item(&'a self, id: ItemId) -> Option<ItemRef<'a>>;        // BOARD_ITEM* from a handle
    pub fn detached(&'a self, item: AnyItem<'a>) -> ItemRef<'a>;     // dummy copies
    pub fn resolve(&'a self, uuid: &Kiid) -> Option<ItemRef<'a>>;    // BOARD::ResolveItem
}
```

C++ idiom to Rust idiom:

| C++ | Rust |
|---|---|
| `BOARD_ITEM*` stored in a member / container / cache key | `ItemId` (or `PadId`, `ZoneId`, ...) |
| `BOARD_ITEM*` used right now | `ItemRef<'_>` |
| `item->Type()` | `item.type_()` (`KicadT`, same ordinals as C++) |
| `switch( item->Type() ) { case PCB_PAD_T: static_cast<PAD*>( item ) ...` | `match item.any() { AnyItem::Pad( pad ) => ..., AnyItem::Track( Track::Via( via ) ) => ... }` |
| `dynamic_cast<BOARD_CONNECTED_ITEM*>( item )` / `item->IsConnected()` | `item.connected()` -> `Option<&ConnectedItemBase>` / `item.is_connected()` |
| `item->GetParentFootprint()` | `item.parent_footprint()` -> `Option<ItemRef>`; typed: `pad.parent_footprint( ctx )` |
| `item->GetBoard()` | `ctx.board` |
| `item->GetParentGroup()` | `item.base().group` |
| `item->m_Uuid` | `item.uuid()` |
| `pad->GetNet()->GetNetname()` | `ctx.board.find_net( pad.net_code() )` then `.netname` (or `item.netname()`) |
| `item->GetEffectiveNetClass()` | `item.effective_netclass()` -> `Option<&Netclass>`; typed: `pad.effective_netclass( ctx )` -> `&Netclass` |
| `for( PCB_TRACK* t : board->Tracks() )` | `for ( id, t ) in board.tracks()` |
| `for( PAD* pad : fp->Pads() )` | `for ( id, pad ) in board.footprint_pads( fp_id )` |
| `board->Add( item, ADD_MODE::APPEND )` | `board.add_track( track, AddMode::Append )`, `board.add( owned, parent, mode )` |
| `board->Remove( item ); delete item;` | `board.remove( id )` |
| `PAD dummy( *pad ); dummy.SetSize( layer, size );` | `let mut dummy = pad.clone(); dummy.set_size( &env, layer, size ); ctx.detached( AnyItem::Pad( &dummy ) )` |
| `std::map<BOARD_ITEM*, X>`, `std::unordered_map<PTR_PTR_CACHE_KEY, bool>` | `BTreeMap<ItemId, X>` / `SideTable` / `HashMap<(ItemId, ItemId), bool>` (lookup only) |
| `std::set<BOARD_ITEM*, cmp>( begin, end )` | `let mut v: Vec<ItemId> = ...; v.sort_by( \|a, b\| cmp( ctx, *a, *b ) )` |
| `BOARD_LISTENER::OnBoardItemChanged` | `board.drain_changes()` (pull) |
| `item->SetFlags( SKIP_STRUCT )` as algorithm scratch | `SideTable` / `HashSet<ItemId>` owned by the algorithm |

### 2.6 The context

```rust
/// Everything a read-only item method may reach besides `self`.
pub struct BoardCtx<'a> {
    pub board: &'a Board,
    /// `BOARD_DESIGN_SETTINGS::m_DRCEngine`. `None` reproduces the C++ `if( !m_DRCEngine )`
    /// fallbacks (own clearance 0, mask expansion from footprint / board defaults, ...).
    pub rules: Option<&'a dyn RuleResolver>,
    /// `BOARD::GetConnectivity()`. `None` => conditionally flashed pads/vias are treated as
    /// C++ treats them when there is no board (always flashed).
    pub connectivity: Option<&'a dyn ConnectivityQuery>,
}
impl<'a> BoardCtx<'a> {
    pub fn bare(board: &'a Board) -> Self;       // parser, writer, simple tools
}

/// Implemented by `kicad_drc::DrcEngine`. Port targets named per method.
pub trait RuleResolver {
    /// `DRC_ENGINE::EvalRules`.
    fn eval_rules(&self, ctx: &BoardCtx<'_>, constraint: DrcConstraintT,
                  a: Option<ItemRef<'_>>, b: Option<ItemRef<'_>>,
                  layer: PcbLayerId) -> DrcConstraint;
    /// `DRC_ENGINE::HasRulesForConstraintType`.
    fn has_rules_for_constraint_type(&self, constraint: DrcConstraintT) -> bool;
    /// `DRC_ENGINE::GetCachedOwnClearance`; returns (clearance, source text).
    fn cached_own_clearance(&self, ctx: &BoardCtx<'_>, item: ItemRef<'_>,
                            layer: PcbLayerId, want_source: bool) -> (i32, Option<String>);
}

/// Implemented by `kicad_connectivity::ConnectivityData`.
pub trait ConnectivityQuery {
    /// `CONNECTIVITY_DATA::IsConnectedOnLayer`.
    fn is_connected_on_layer(&self, ctx: &BoardCtx<'_>, item: ItemId, layer: PcbLayerId,
                             types: &[KicadT]) -> bool;
}
```

`DrcConstraintT` (`DRC_CONSTRAINT_T`), `DrcConstraint` (value part of `DRC_CONSTRAINT`:
type, `MinOptMax<i32>`, disallow flags, zone connection, rule name, implicit flag),
`Flashing`, `ErrorLoc` and the DRC error-code table (`DRCE_*` code <-> settings key, needed by
`BoardDesignSettings::drc_severities` and by the `.kicad_pro` reader) are plain data and are
defined in `kicad-pcb/src/drc_api.rs`; `kicad-drc` re-exports them.

A top-level owner (CLI command, wasm `Design` object) holds the three peers side by side, so no
value borrows another:

```rust
pub struct Design {
    pub board: Board,
    pub connectivity: kicad_connectivity::ConnectivityData,   // holds ItemIds, no borrows
    pub drc: Option<kicad_drc::DrcEngine>,                    // holds ItemIds, no borrows
}
impl Design {
    pub fn ctx(&self) -> BoardCtx<'_>;
}
```

This is the main structural win of handles: **consumers store handles, not references**, so a
`ConnectivityData` or an R-tree can exist while the board is mutated, and re-synchronise from
the change journal afterwards.

### 2.7 Mutation under the borrow checker

All structural mutation goes through `Board`; arena fields are `pub(crate)`.

#### 2.7.1 Add / remove

```rust
pub enum AddMode { Insert, Append, BulkAppend, BulkInsert }      // ADD_MODE
pub enum RemoveMode { Normal, Bulk }                             // REMOVE_MODE

impl Board {
    // Typed adders (BOARD::Add). Set parent, push to the order list per `mode`, bind the UUID
    // index, bump the time stamp, journal `Added`. Item constructors that C++ gives a fresh
    // KIID take `&mut board.kiid_generator` (`PcbTrack::new( &mut gen, .. )`); `add_*` does not
    // touch the UUID (nil stays nil, as in C++).
    pub fn add_track(&mut self, track: Track, mode: AddMode) -> TrackId;
    pub fn add_zone(&mut self, zone: Zone, mode: AddMode) -> ZoneId;
    pub fn add_footprint(&mut self, footprint: Footprint, mode: AddMode) -> FootprintId;
    pub fn add_shape(&mut self, shape: PcbShape, mode: AddMode) -> ShapeId;
    pub fn add_text(&mut self, text: PcbText, mode: AddMode) -> TextId;
    // ... one per board-level kind
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<NetCode>;   // NETINFO_LIST::AppendNet

    // FOOTPRINT::Add equivalents: the parent comes first.
    pub fn footprint_add_pad(&mut self, fp: FootprintId, pad: Pad, mode: AddMode) -> PadId;
    pub fn footprint_add_field(&mut self, fp: FootprintId, field: PcbField) -> FieldId;
    pub fn footprint_add_shape(&mut self, fp: FootprintId, s: PcbShape, mode: AddMode) -> ShapeId;
    pub fn footprint_add_zone(&mut self, fp: FootprintId, z: Zone, mode: AddMode) -> ZoneId;
    // ... text, textbox, table, dimension, barcode, image, group, constraint, point, body

    /// Generic form used by the parser's `default:` paths, commits and the wasm layer.
    pub fn add(&mut self, item: OwnedItem, parent: ParentRef, mode: AddMode) -> ItemId;

    /// BOARD::Remove / FOOTPRINT::Remove + delete. Cascades to children (footprint -> pads,
    /// fields, graphics, zones, groups, points, bodies; table -> cells), removes the item from
    /// its parent group, unbinds UUIDs, bumps the time stamp, journals `Removed`.
    /// Returns false for a stale handle.
    pub fn remove(&mut self, id: ItemId) -> bool;
    pub fn remove_many(&mut self, ids: &[ItemId]) -> usize;            // one retain pass per list
    /// BOARD::RemoveAll.
    pub fn remove_all(&mut self, types: &[KicadT]);
}
```

Router / autorouter client:

```rust
let seg = PcbTrack::new(&mut board.kiid_generator, start, end, width, layer, net);
let id = board.add_track(Track::Trace(seg), AddMode::Insert);
board.remove(old_via.into());
connectivity.apply_changes(&design_ctx, board.drain_changes());
```

#### 2.7.2 Modify a leaf item: `MutEnv`

A `&mut Pad` borrowed from `board.pads` cannot coexist with `&Board`. Leaf mutators therefore
take a small *snapshot* of what C++ reads through `GetParentFootprint()` / `GetBoard()` while
mutating (parent transform, flipped state, copper layer count, layer flip table, max error):

```rust
/// Read-only environment for mutators. Built by `Board` before it borrows the item mutably.
#[derive(Clone)]
pub struct MutEnv {
    pub parent: Option<ParentFrame>,       // None: board-level item
    pub layers: Arc<LayerEnv>,             // shared, rebuilt when layers / stackup change
    pub max_error: i32,                    // BOARD_ITEM::GetMaxError
    pub is_footprint_holder: bool,         // BOARD_USE::FPHOLDER
}
#[derive(Clone, Copy)]
pub struct ParentFrame {
    pub footprint: FootprintId,
    pub transform: TransformTrs,           // FOOTPRINT::GetTransform()
    pub layer: PcbLayerId,                 // F_Cu / B_Cu
    pub flipped: bool,
}
pub struct LayerEnv {
    pub copper_layer_count: i32,
    pub enabled_layers: Lset,
    /// BOARD::FlipLayer for every layer id (user-layer opposites included).
    pub flipped: [PcbLayerId; PCB_LAYER_ID_COUNT],
}

impl Board {
    pub fn mut_env(&self, id: ItemId) -> MutEnv;

    /// The generic entry: clears the item's caches, runs `f`, then bumps the time stamp,
    /// invalidates the parent footprint's geometry caches and journals `Modified`.
    pub fn modify<R>(&mut self, id: ItemId,
                     f: impl FnOnce(AnyItemMut<'_>, &MutEnv) -> R) -> Option<R>;
    pub fn modify_track<R>(&mut self, id: TrackId, f: impl FnOnce(&mut Track, &MutEnv) -> R) -> Option<R>;
    pub fn modify_pad<R>(&mut self, id: PadId, f: impl FnOnce(&mut Pad, &MutEnv) -> R) -> Option<R>;
    pub fn modify_zone<R>(&mut self, id: ZoneId, f: impl FnOnce(&mut Zone, &MutEnv) -> R) -> Option<R>;
    // ... one per kind

    /// Polymorphic BOARD_ITEM virtuals that may cascade (footprint, group, generator, table).
    pub fn move_item(&mut self, id: ItemId, delta: Vector2I);                       // Move
    pub fn rotate_item(&mut self, id: ItemId, centre: Vector2I, angle: EdaAngle);   // Rotate
    pub fn flip_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection);  // Flip
    pub fn mirror_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection);
    pub fn set_item_layer(&mut self, id: ItemId, layer: PcbLayerId);
    pub fn set_item_net(&mut self, id: ItemId, net: NetCode) -> bool;               // SetNetCode
    pub fn set_item_locked(&mut self, id: ItemId, locked: bool);
}
```

Port example (`pad.cpp:244`):

```rust
/// Port of `PAD::GetPosition`.
pub fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I {
    if let Some(fp) = self.parent_footprint(ctx) {
        return fp.transform().apply(self.lib_pos);
    }
    self.lib_pos
}

/// Port of `PAD::SetPosition`.
pub fn set_position(&mut self, env: &MutEnv, pos: Vector2I) {
    if let Some(fp) = &env.parent {
        self.lib_pos = fp.transform.inverse_apply(pos);
    } else {
        self.lib_pos = pos;
    }
    self.set_dirty();
}
```

#### 2.7.3 Bulk transforms of a footprint

`FOOTPRINT::SetPosition / SetOrientation / Flip / SetTransformScale / MoveAnchorPosition`
update `m_transform` and then call `OnFootprintTransformed()` / `OnFootprintRescaled()` /
`Move()` / `Flip()` on every child. With arenas the footprint and its children are in
*different fields* of `Board`, so plain field-disjoint borrows are enough; no `unsafe`, no
`RefCell`:

```rust
impl Board {
    /// Port of `FOOTPRINT::SetPosition` (footprint.cpp:3708).
    pub fn footprint_set_position(&mut self, id: FootprintId, pos: Vector2I) {
        let layers = self.layer_env.clone();
        let max_error = self.design_settings.max_error;
        let holder = self.is_footprint_holder();
        let Board { footprints, fields, pads, zones, points, shapes, texts, textboxes,
                    barcodes, tables, tablecells, dimensions, images, .. } = self;
        let fp = &mut footprints[id];
        let delta = pos - fp.transform.translate();
        fp.transform.set_translate(pos);
        let env = MutEnv { parent: Some(fp.frame(id)), layers, max_error, is_footprint_holder: holder };

        for &f in &fp.fields { fields[f].on_footprint_transformed(&env); }
        for &p in &fp.pads   { pads[p].on_footprint_transformed(&env); }
        for &z in &fp.zones  { zones[z].on_footprint_transformed(&env); }
        for &p in &fp.points { points[p].on_footprint_transformed(&env); }
        for &d in &fp.drawings {
            match d.kind() {
                ItemKind::Text      => texts[d.as_text().unwrap()].on_footprint_transformed(&env),
                ItemKind::Shape     => shapes[d.as_shape().unwrap()].on_footprint_transformed(&env),
                ItemKind::Textbox   => textboxes[d.as_textbox().unwrap()].on_footprint_transformed(&env),
                ItemKind::Barcode   => barcodes[d.as_barcode().unwrap()].on_footprint_transformed(&env),
                ItemKind::Dimension => dimensions[d.as_dimension().unwrap()].on_footprint_transformed(&env),
                ItemKind::Table     => table_on_footprint_transformed(tables, tablecells, d, &env),
                _                   => images[d.as_image().unwrap()].move_by(&env, delta), // item->Move( delta )
            }
        }
        fp.geometry_cache_move(delta);     // bounding_box, text_excluded_bbox, hull
        fp.courtyard_cache_move(delta);    // + re-hash, as in C++
        self.touch(id.into());             // time stamp + journal
    }
    pub fn footprint_set_orientation(&mut self, id: FootprintId, angle: EdaAngle);
    pub fn footprint_flip(&mut self, id: FootprintId, centre: Vector2I, dir: FlipDirection);
    pub fn footprint_set_transform_scale(&mut self, id: FootprintId, sx: f64, sy: f64);
    pub fn footprint_rescale_around_point(&mut self, id: FootprintId, centre: Vector2I, sx: f64, sy: f64);
    pub fn footprint_move_anchor_position(&mut self, id: FootprintId, delta: Vector2I);
    pub fn footprint_set_layer_and_flip(&mut self, id: FootprintId, layer: PcbLayerId);
}
```

The loop bodies are the C++ loop bodies; the only structural change is that the function is a
`Board` method instead of a `FOOTPRINT` method. All footprint-cascading functions live in
`footprint/transform.rs` as `impl Board` blocks, with the C++ origin named in the doc comment.

#### 2.7.4 Escape hatch: take / put back

A few mutators need the whole board while mutating one item (`PCB_DIMENSION_BASE::Update`
resolves text variables and units; generator updates). For those:

```rust
impl Board {
    /// Moves the item out of its slot (state `Taken`), gives `f` the item and a full
    /// read-only context of the rest of the board, then puts it back.
    /// While taken, `get(id)` returns `None` and iteration helpers skip the item.
    pub fn with_item_taken<R>(&mut self, id: ItemId, rules: Option<&dyn RuleResolver>,
                              f: impl FnOnce(AnyItemMut<'_>, &BoardCtx<'_>) -> R) -> Option<R>;
}
```

Rule: use `MutEnv` wherever it suffices; `with_item_taken` only where the C++ body reads other
items, and never for footprints (children would not find their parent).

#### 2.7.5 Write-back from algorithms

* **Zone filler.** Each C++ phase maps to a borrow phase: (1) `&mut Board`: clear / set
  `zone_layer_overrides` on vias and pads, `un_fill`; (2) `&Board`: compute fills into a
  `Vec<(ZoneId, PcbLayerId, ShapePolySet)>`; (3) `&mut Board`:
  `board.zones[z].set_filled_polys_list(layer, polys)`, `set_fill_flag`, `set_is_filled`;
  (4) connectivity (a separate value holding handles) rebuilds from `&Board`;
  (5) `&mut Board`: island removal, `set_is_island`, `build_hash_value`. No phase needs a shared
  and a mutable borrow at the same time because nothing outside `Board` holds a reference into it.
* **Connectivity net propagation** (`CN_CONNECTIVITY_ALGO::PropagateNets` calls
  `SetNetCode` on vias/tracks): `fn propagate_nets(&mut self, board: &mut Board)` compiles
  directly; it calls `board.set_item_net(id, code)`.
* **DRC** never mutates items while providers run; markers are collected and added with
  `board.add_marker(...)` afterwards. The one C++ exception, toggling the `HOLE_PROXY` flag on
  the tested item in `drc_test_provider_disallow.cpp`, becomes a field of the rule-evaluation
  request (`ItemRef::as_hole_proxy()` sets a bit in the `ItemRef`, not in the item).

#### 2.7.6 Change journal (replaces `BOARD_LISTENER` and the direct `m_connectivity->Add/Remove`)

```rust
pub enum ChangeKind { Added, Removed, Modified }
pub struct Change {
    pub kind: ChangeKind,
    pub id: ItemId,
    pub uuid: Kiid,
    pub kicad_t: KicadT,
    /// State before the change (for `Removed` and `Modified`): what connectivity and R-tree
    /// owners need to find and dirty their old entries.
    pub net_before: NetCode,
    pub layers_before: Lset,
}
impl Board {
    pub fn set_journal_enabled(&mut self, on: bool);     // off while parsing a file
    pub fn drain_changes(&mut self) -> Vec<Change>;
    pub fn time_stamp(&self) -> i32;                     // BOARD::GetTimeStamp
    pub fn increment_time_stamp(&mut self);              // also clears BoardCaches
}
```

#### 2.7.7 Commits and undo (`commit.rs`)

```rust
pub struct BoardCommit { entries: Vec<CommitEntry> }
enum CommitEntry {
    Added(ItemId),
    Removed { id: ItemId, list_pos: usize },             // item is parked in its slot
    Modified { id: ItemId, before: OwnedItem },          // SwapItemData image
}
impl BoardCommit {
    pub fn new() -> Self;
    pub fn add(&mut self, board: &mut Board, item: OwnedItem, parent: ParentRef, mode: AddMode) -> ItemId;
    pub fn modify(&mut self, board: &Board, id: ItemId);          // snapshot before the edit
    pub fn remove(&mut self, board: &mut Board, id: ItemId);      // park (with children)
    pub fn push(self, board: &mut Board);                         // purge parked items
    pub fn revert(self, board: &mut Board);                       // unpark, restore images, remove added
}
```

A reverted or undone removal brings the item back **with the same handle**, so connectivity
entries, group member lists and JS-side ids stay valid; that is the handle equivalent of C++
keeping the object alive in the undo list. `Board::clone()` is the heavyweight alternative
(handles stay valid in the clone) for speculative autorouter runs and tests.

### 2.8 Derived caches and invalidation

Two cache cells, both `Clone` to *empty* (a cloned / dummy item never shares or copies a cache):

```rust
/// Build once from `&self`; cleared only through `&mut self`.
pub struct OnceCache<T>(std::sync::OnceLock<T>);
/// Re-validated from `&self` against a key or time stamp (C++ `mutable` + mutex).
pub struct KeyedCache<T>(std::sync::Mutex<Option<T>>);

pub trait Invalidate {
    /// Drop every derived cache of this item. Called by `Board` before it hands out `&mut`.
    fn invalidate_caches(&mut self);
}
```

Both are `Sync`, work on `wasm32-unknown-unknown`, and keep `Board: Sync` so an optional `rayon`
feature stays possible.

| C++ cache | Rust home | invalidated by |
|---|---|---|
| `PAD::m_drawCache` (effective shapes per layer, polygons, bbox, hole), `m_shapesDirty`, `m_polyDirty[2]`, `m_effectiveBoundingRadius`, `m_maskShapes` | `Pad::caches: PadCaches` (3.3): `shapes: OnceCache<PadEffectiveShapes>`, `polygons: [OnceCache<PadEffectivePolygons>; 2]`, `mask_shapes: KeyedCache<..>` | `Pad::set_dirty()` (called where C++ calls `SetDirty()`), any `modify_pad`, parent `on_footprint_transformed` |
| `FOOTPRINT::m_geometry_cache` (bbox, text-excluded bbox, hull + time stamps) | `Footprint::geometry_cache: KeyedCache<FootprintGeometryCacheData>` | board time stamp compare, as in C++ |
| `FOOTPRINT::m_courtyard_cache` (+ hashes, malformed flags) | `Footprint::courtyard_cache: Option<Box<FootprintCourtyardCacheData>>` | explicit `Board::build_courtyard_caches` (needs `&mut`, as `BuildCourtyardCaches` is non-const) |
| `FOOTPRINT::m_netTieCache` | `Footprint::net_tie_cache: BTreeMap<ItemId, BTreeSet<NetCode>>` | explicit `Board::build_net_tie_cache` |
| `ZONE::m_bboxCache` + `BOARD::m_ZoneBBoxCache` | `Zone::bbox_cache: KeyedCache<(Box2I, i32)>` | time stamp compare |
| `EDA_TEXT::m_shown_text` | plain field, updated eagerly by `set_text` (as `cacheShownText`) | setter |
| `EDA_TEXT::m_render_cache`, `m_bbox_cache` | `EdaText::render_cache`, `bbox_cache: KeyedCache<..>` | text setters, `clear_render_cache` |
| `PCB_TEXT::m_knockout_cache` | `PcbText::knockout_cache: KeyedCache<PcbTextKnockoutCacheData>` | key compare (text, attrs, angle, pos) |
| `EDA_SHAPE::m_hatchingCache`, `m_hatchingDirty` | `EdaShape::hatching_cache: KeyedCache<..>` | shape setters |
| `PCB_DIMENSION_BASE::m_shapes` | plain field rebuilt by `update()` | explicit, as in C++ |
| `PCB_BARCODE::m_cache` | `KeyedCache<PcbBarcodeCache>` | key hash |
| `BOARD::m_maxClearanceValue`, `m_footprintCourtyardIndex`, `m_StackedMicroviaCache`, `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`, `m_LayerExpressionCache` | `Board::caches: BoardCaches` (interior) | `increment_time_stamp()` |
| `BOARD::m_IntersectsXxxCache`, `m_ItemFieldCache`, `m_CopperZoneRTreeCache`, `m_CopperItemRTreeCache`, `m_DRCZones`, `m_DRCCopperZones*`, `m_DRCMaxClearance`, `m_ZoneIsolatedIslandsMap`, `m_SolderMaskBridges` | **`kicad-drc`** (`DrcBoardCaches`, keyed by `ItemId` / `(ItemId, ItemId, PcbLayerId)`) | DRC run lifetime |
| `BOARD::m_itemIndex` | `Board::uuid_index` | add / remove / `rebind_item_uuid` |
| `BOARD::m_connectivity` | `kicad_connectivity::ConnectivityData` (peer of the board) | change journal |
| `NET_SETTINGS::m_effectiveNetclassCache` | `NetSettings::effective_netclass_cache: KeyedCache<BTreeMap<..>>` + `NetinfoItem::netclass` | `synchronize_nets_and_netclasses` |

Guard against the one way the scheme can go wrong (someone changes a footprint transform
without running the child hooks): `Footprint::transform` is private to the crate and only the
`Board::footprint_*` functions write it; debug builds store the parent transform in
`PadEffectiveShapes` and `debug_assert_eq!` it on every cache hit.

### 2.9 Detached items, dummies, library footprints

* A struct outside any arena is a normal value. `pad.clone()` keeps
  `parent = Item(footprint)`, so `dummy.position(ctx)` works; `ctx.detached(AnyItem::Pad(&dummy))`
  gives an `ItemRef` with `id() == None`.
* Pad custom-shape primitives (`PADSTACK::COPPER_LAYER_PROPS::custom_shapes`), a barcode's
  text, marker path shapes and generator template items are **inline** values, not arena items
  (C++ does not register them with the board either).
* A library footprint (file parsed without a board) is a `Board` in `BoardUse::FpHolder` mode
  holding one footprint. `Board::copy_footprint_from(&mut self, src: &Board, fp: FootprintId,
  new_uuids: bool) -> FootprintId` deep-copies the subtree and remaps every handle
  (parent, child lists, group members), the handle analogue of the `ptrMap` in the C++
  `FOOTPRINT` copy constructor.

### 2.10 Invariants and their checker

`Board::check_invariants(&self) -> Result<(), String>` (debug builds and every model test):

1. every id in an order list is live, and every live item is in exactly one order list
   (board-level list, a footprint list, a table's cell list);
2. `child.parent` agrees with the list that contains the child;
3. `item.group == Some(g)` iff `g.members` contains the item; group graph is acyclic;
4. `uuid_index[kiid]` points at a live item carrying that `kiid`; every live item whose UUID is
   unique is indexed;
5. every `ConnectedItemBase::net` is `UNCONNECTED`, or present in `nets`;
6. no slot is `Taken` outside `with_item_taken`.

---

## 3. Complete type definitions

Conventions used in this section:

* The trailing comment names the C++ member the field ports (`// m_width`). A field without such
  a comment is new and explained.
* `String` ports `wxString` / `std::string` (UTF-8). `BTreeMap` / `BTreeSet` port `std::map` /
  `std::set`; they also replace `std::unordered_map` wherever the C++ container is iterated.
* All structs derive `Clone, Debug`. `Default` gives the field values of the C++ constructor
  (taken from the constructor bodies in the named `.cpp`, not invented) with a **nil** `uuid`;
  `Type::new( &mut KiidGenerator, .. )` is the C++ constructor including the fresh `KIID` that
  the `EDA_ITEM` constructor draws.
* Types expected from the foundation crates: `Vector2I`, `Vector2D`, `Vector3D`, `Box2I`,
  `EdaAngle`, `Seg`, `ShapeLineChain`, `ShapePolySet`, `ShapeSegment`, `ShapeCompound`,
  `trait Shape` (`kicad-geom`); `Kiid`, `KiidPath`, `PcbLayerId`, `Lset`, `LibId`, `KicadT`,
  `TextAttributes`, `StrokeParams`, `LineStyle`, `Color4D`, `Severity`, `EdaUnits`
  (`kicad-core`); `Sexpr` (`kicad-sexpr`); `FontRef`, `Glyph` (`kicad-font`).
* Types this design additionally **requires from `kicad-geom` / `kicad-core`** because the C++
  model uses them (they are not in the list of foundation types I was given; see section 9):
  `EdaOrientation` (`EDA_ORIENTATION`, `eda_angle.h`), `TransformTrs` (`TRANSFORM_TRS`),
  `Ellipse<i32>` (`ELLIPSE<int>`), `Hash128` (`HASH_128`), `MinOptMax<T>`.

### 3.1 Small shared types (`item.rs`)

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

pub enum FlipDirection { LeftRight, TopBottom }                 // FLIP_DIRECTION (core/mirror.h)
pub enum Flashing { Default, AlwaysFlashed, NeverFlashed }      // FLASHING
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

### 3.2 Tracks, arcs, vias (`pcb_track.rs`) and the padstack (`padstack.rs`)

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

### 3.3 Pad (`pad.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Pad {
    pub base: ConnectedItemBase,
    pub number: String,                               // m_number
    pub pin_function: String,                         // m_pinFunction
    pub pin_type: String,                             // m_pinType
    pub lib_pos: Vector2I,                            // m_libPos (footprint library frame)
    pub lib_orientation: EdaOrientation,              // m_libOrientation
    pub padstack: Padstack,                           // m_padStack (lib-frame sizes / offsets)
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
    pub shapes: OnceCache<PadEffectiveShapes>,                       // BuildEffectiveShapes
    pub polygons: [OnceCache<PadEffectivePolygons>; 2],              // BuildEffectivePolygon( ERROR_INSIDE / ERROR_OUTSIDE )
    pub mask_shapes: KeyedCache<BTreeMap<PcbLayerId, (i32, Arc<ShapeCompound>)>>,
}
pub(crate) struct PadEffectiveShapes {
    pub bounding_box: Box2I,                                         // m_effectiveBoundingBox
    pub shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>>,            // m_effectiveShapes
    pub hole_shape: Arc<ShapeSegment>,                               // m_effectiveHoleShape
}
pub(crate) struct PadEffectivePolygons {
    pub polygons: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,           // m_effectivePolygons[layer][errorLoc]
    pub bounding_radius: i32,                                        // m_effectiveBoundingRadius
}
```

Dropped: `m_dataMutex`, `m_maskShapeMutex` (inside the cache cells), `m_lastGalZoomLevel` (GAL).

### 3.4 Footprint (`footprint.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Footprint {
    /// `base.layer` is F_Cu or B_Cu (the side).
    pub base: BoardItemBase,

    // ---- children: order lists into the board arenas ------------------------------------
    pub fields: Vec<FieldId>,                 // m_fields   (push_back only)
    pub drawings: VecDeque<ItemId>,           // m_drawings (Shape, Text, Textbox, Table, Dimension, Barcode, Image)
    pub pads: VecDeque<PadId>,                // m_pads
    pub zones: Vec<ZoneId>,                   // m_zones
    pub groups: Vec<GroupId>,                 // m_groups
    pub constraints: Vec<ConstraintId>,       // m_constraints
    pub points: Vec<PointId>,                 // m_points
    pub bodies: VecDeque<Body3dId>,           // m_bodies

    // ---- placement: written only by Board::footprint_* (section 2.7.3) --------------------
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
    pub(crate) geometry_cache: KeyedCache<FootprintGeometryCacheData>,      // m_geometry_cache
    pub(crate) courtyard_cache: Option<Box<FootprintCourtyardCacheData>>,   // m_courtyard_cache
    pub(crate) net_tie_cache: BTreeMap<ItemId, BTreeSet<NetCode>>,          // m_netTieCache
    /// (ticker, dynamic class, final class) of COMPONENT_CLASS_CACHE_PROXY
    pub(crate) component_class_cache: KeyedCache<ComponentClassCache>,
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

### 3.5 Zone (`zone.rs`, `zone_settings.rs`, `zones.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Zone {
    pub base: ConnectedItemBase,
    /// m_Poly. Board frame for a board zone, footprint library frame for a footprint zone
    /// (`library_outline()` / `board_outline(ctx)` port GetLibraryOutline / GetBoardOutline).
    pub poly: ShapePolySet,
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
    pub is_filled: bool,                                     // m_isFilled
    pub need_refill: bool,                                   // m_needRefill (std::atomic<bool>)
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
    /// m_FilledPolysList. `Arc` like the C++ `shared_ptr`: connectivity / DRC may keep a
    /// snapshot while the filler installs a new one.
    pub filled_polys_list: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,
    pub fill_flags: Lset,                                    // m_fillFlags
    pub filled_polys_hash: BTreeMap<PcbLayerId, Hash128>,    // m_filledPolysHash
    pub border_style: ZoneBorderDisplayStyle,                // m_borderStyle
    pub border_hatch_pitch: i32,                             // m_borderHatchPitch
    pub border_hatch_lines: Vec<Seg>,                        // m_borderHatchLines
    pub insulated_islands: BTreeMap<PcbLayerId, BTreeSet<i32>>,   // m_insulatedIslands
    pub area: f64,                                           // m_area
    pub outline_area: f64,                                   // m_outlinearea
    pub(crate) bbox_cache: KeyedCache<(Box2I, i32)>,         // m_bboxCache + m_bboxCacheTimeStamp
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

### 3.6 Graphic shapes (`eda_shape.rs`, `pcb_shape.rs`, `line_ending.rs`)

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
    pub(crate) hatching_cache: KeyedCache<EdaShapeHatchCacheData>,   // m_hatchingCache + m_hatchingDirty
}
pub struct EdaShapeHatchCacheData { pub hatching: ShapePolySet, pub hatch_lines: Vec<Seg> }

/// PCB_SHAPE (pcbnew/pcb_shape.h): BOARD_CONNECTED_ITEM + EDA_SHAPE + library-frame copy.
#[derive(Clone, Debug)]
pub struct PcbShape {
    pub base: ConnectedItemBase,
    pub shape: EdaShape,
    pub has_solder_mask: bool,                    // m_hasSolderMask
    pub solder_mask_margin: Option<i32>,          // m_solderMaskMargin
    pub lib: PcbShapeLib,
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

### 3.7 Text family (`eda_text.rs`, `pcb_text.rs`, `pcb_field.rs`, `pcb_textbox.rs`, `pcb_tablecell.rs`, `pcb_table.rs`)

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
    pub pos: Vector2I,                            // m_pos
    pub visible: bool,                            // m_visible
    pub(crate) render_cache: KeyedCache<EdaTextRenderCacheData>,          // m_render_cache
    pub(crate) bbox_cache: KeyedCache<BTreeMap<i32, (Vector2I, Box2I)>>,  // m_bbox_cache
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
    pub lib_text_angle: EdaOrientation,           // m_libTextAngle
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
pub enum GeneratedTableKind { DrillChart, StackupTable }      // PCB_DRILL_CHART_T / PCB_STACKUP_TABLE_T
/// Configuration of a PCB_GENERATED_TABLE, kept as verbatim s-expression nodes in file order:
/// `(units ..)`, `(precision ..)`, `(column ..)*`, `(row_keys ..)`, `(filter ..)`, `(totals ..)`,
/// `(row_shapes ..)`. The cells themselves are ordinary `PcbTablecell`s.
#[derive(Clone, Debug)]
pub struct GeneratedTableData { pub kind: GeneratedTableKind, pub config: Vec<Sexpr> }
```

Dropped from `EDA_TEXT`: `m_activeUrl` (UI hover), `m_text_var_refs` (reactive text-variable
graph of the editor), `m_IuScale` (always `pcbIUScale`), `m_bbox_cacheMutex`.

### 3.8 Dimensions (`pcb_dimension.rs`)

```rust
/// PCB_DIMENSION_BASE : PCB_TEXT, with the five concrete classes as `kind`.
#[derive(Clone, Debug)]
pub struct PcbDimensionBase {
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
    pub start: Vector2I,                          // m_start (FP-relative inside a footprint)
    pub end: Vector2I,                            // m_end
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

### 3.9 Groups and generators (`pcb_group.rs`, `pcb_generator.rs`)

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

### 3.10 Small and rare items

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
    pub lib_pos: Vector2I,        // m_libPos
    pub pos: Vector2I,            // m_pos (baked board position, refreshed by on_footprint_transformed)
    pub size: i32,                // m_size
}

/// PCB_BARCODE
#[derive(Clone, Debug)]
pub struct PcbBarcode {
    pub base: BoardItemBase,
    pub width: i32,                               // m_width
    pub height: i32,                              // m_height
    pub lib_pos: Vector2I,                        // m_libPos
    pub margin: Vector2I,                         // m_margin
    pub text: PcbText,                            // m_text (inline; content, visibility = show text)
    pub kind: BarcodeT,                           // m_kind
    pub lib_angle: EdaOrientation,                // m_libAngle
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

/// Items whose behaviour is not ported: PCB_GRID_ITEM (`grid_item`), PCB_DRILL_MAP (`drill_map`).
#[derive(Clone, Debug)]
pub struct OpaqueItem {
    /// uuid, layer (drill_map), locked are extracted from the node; the rest stays in `body`.
    pub base: BoardItemBase,
    pub kicad_t: KicadT,                          // PcbGridItemT / PcbDrillMapT (save ordering)
    pub body: Sexpr,                              // the complete node, re-emitted verbatim
}
```

`PCB_BOARD_OUTLINE` (`BOARD::m_boardOutline`, a derived item holding the Edge.Cuts polygons) is
not an item here: it is the `board_outline` entry of `BoardCaches`.

Dropped in this group: `MARKER_BASE::m_scalingFactor`, `m_shapeBoundingBox` (marker glyph
drawing), `RC_ITEM::m_parent` (back pointer to the marker; the marker owns the `RcItem`),
`DRC_ITEM::m_violatingTest` (pointer to the provider), `BITMAP_BASE`'s `wxImage` / `wxBitmap`
members and preview state (pixels are never decoded), `PCB_DRILL_MAP`'s outline cache.

### 3.11 Nets and net classes (`netinfo.rs`, `netclass.rs`, `net_settings.rs`)

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

### 3.12 Design settings, stack-up, plot parameters, layers, page (`board_design_settings.rs`, `board_stackup.rs`, `pcb_plot_params.rs`, `title_block.rs`, `page_info.rs`)

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
(open question 9.2-c).

### 3.13 Embedded files and component classes (`embedded_files.rs`, `component_class.rs`)

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

### 3.14 The board (`board.rs`)

```rust
pub struct Board {
    // ---- item arenas (generated by `board_items!`, section 2.3) ---------------------------
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
    pub(crate) dimensions: Arena<PcbDimensionBase>,
    pub(crate) barcodes: Arena<PcbBarcode>,
    pub(crate) images: Arena<PcbReferenceImage>,
    pub(crate) targets: Arena<PcbTarget>,
    pub(crate) points: Arena<PcbPoint>,
    pub(crate) groups: Arena<PcbGroup>,
    pub(crate) generators: Arena<PcbGenerator>,
    pub(crate) markers: Arena<PcbMarker>,
    pub(crate) constraints: Arena<PcbConstraint>,
    pub(crate) bodies: Arena<Pcb3dBody>,
    pub(crate) opaque: Arena<OpaqueItem>,

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
    pub(crate) layer_env: Arc<LayerEnv>,                     // snapshot for MutEnv (section 2.7.2)
    pub(crate) journal: ChangeJournal,                       // replaces m_listeners
    /// `kicad_core::KiidGenerator` (seedable MT19937, `Clone`): source of UUIDs for new items.
    pub kiid_generator: KiidGenerator,                       // deterministic by default (section 9.1-h)
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

### 3.15 Members deliberately dropped from `BOARD`

| C++ member | reason |
|---|---|
| `m_connectivity` | lives beside the board (`ConnectivityData`), reached through `BoardCtx::connectivity` |
| `m_listeners`, `m_textVarAdapter` | replaced by the change journal; reactive text-variable graph is editor-only |
| `m_highLight`, `m_highLightPrevious`, `m_LegacyVisibleLayers`, `m_LegacyVisibleItems` | view state |
| `m_drillModelGeneration`, `m_boardOutlineGeneration`, `m_drillSymbolLayers`, `m_drillSymbolPlacements`, `m_drillSymbolCache(+Mutex)` | drill chart / drill map live rendering (not ported, section 3.17) |
| `m_IntersectsXxxCache`, `m_IntersectsXxxResultCache`, `m_EnclosedByAreaCache`, `m_ItemFieldCache`, `m_CopperZoneRTreeCache`, `m_CopperItemRTreeCache`, `m_ZoneBBoxCache`, `m_DRCZones`, `m_DRCCopperZones`, `m_DRCCopperZonesByLayer`, `m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance`, `m_SolderMaskBridges`, `m_ZoneIsolatedIslandsMap`, `m_CachesMutex` | owned by `kicad-drc` (`DrcBoardCaches`) and `kicad-zone`, keyed by `ItemId`; the zone bbox cache is per zone |
| `m_project` | replaced by `project: Option<BoardProject>` (plain data) |
| `m_lengthDelayCalc` | belongs with the length/delay calculation port (Tier 2, above the model) |
| `m_historyLifetime`, `m_embeddedFilesDelegate` | local history, footprint-editor delegate |
| `PROJECT::_ELEM` base | project element registry |

### 3.16 Dropped from `EDA_ITEM` / `BOARD_ITEM` (all item types)

`m_structType` (the Rust type / enum variant; `type_()` returns the `KicadT`),
`m_rolloverPos`, `m_isRollover`, `m_forceVisible`, `m_netHighlighted` (UI),
`m_customPropsStorageCache` (property grid), `m_owningIndex` (arena membership is the index),
the `KIGFX::VIEW_ITEM` and `SERIALIZABLE` bases (GAL view, protobuf IPC).
Not ported behaviour: `ViewGetLayers`, `ViewGetLOD`, `ViewBBox`, `GetMsgPanelInfo`,
`GetMenuImage`, `Serialize` / `Deserialize`, the `PROPERTY_MANAGER` registrations
(`*_DESC` structs), `Similarity`, `swapData` (replaced by whole-value before-images).
Ported, because DRC reports and the writer depend on them: `GetItemDescription`,
`GetFriendlyName`, `GetClass`, `LayerMaskDescribe`, every `Compare` / `cmp_*`.

### 3.17 Fully modelled or opaque: the explicit list

| C++ type | Model | Re-save | Plot (Gerber) | DRC |
|---|---|---|---|---|
| `FOOTPRINT`, `PAD`, `PADSTACK`, `PCB_TRACK`, `PCB_ARC`, `PCB_VIA`, `ZONE`, `PCB_SHAPE` (all 8 shape kinds), `PCB_TEXT`, `PCB_FIELD`, `PCB_TEXTBOX`, `PCB_TABLE`, `PCB_TABLECELL`, dimensions, `PCB_GROUP`, `PCB_TARGET`, `PCB_POINT`, nets, net chains, stack-up, plot params, variants, properties | full | exact | full | full |
| `PCB_GENERATOR` (tuning pattern, via stack, via stitch) | typed group + generic property map + template items; no regeneration | exact (with `normalize_properties` for `tuning_pattern`) | members are ordinary tracks/vias | members are ordinary items |
| `PCB_BARCODE` | data full; symbol polygons need a pure-Rust encoder for Code 39 / Code 128 / Data Matrix / QR / Micro QR with zint-identical module layout (C++ uses libzint) | exact (only parameters are saved) | missing until the encoder work package lands; documented gap | clearance against the barcode missing until then |
| `PCB_REFERENCE_IMAGE` | data full, bitmap kept as encoded text, header-only size probe | exact | not plotted on fabrication layers in C++ either | ignored in C++ too |
| `PCB_CONSTRAINT` | data full (KIID members); solver not ported | exact | none | none |
| `PCB_3D_BODY`, `FP_3DMODEL` | data full | exact | none | none |
| `PCB_DRILL_CHART`, `PCB_STACKUP_TABLE` | `PcbTable` + cells fully typed; configuration as verbatim nodes; **no regeneration** (`RefreshGeneratedTables` in `BOARD_LOADER::SaveBoard` is not ported in Tier 1) | exact when the stored cells are current, otherwise C++ would rewrite the cells and we would not: documented deviation | plotted as an ordinary table (cells as loaded); the drill chart's symbol-column marks (`PlotChartSymbols`) are a documented gap | as an ordinary table |
| `PCB_GRID_ITEM` | `OpaqueItem` | exact for files written by this format version (tokens re-emitted through the formatter) | none in C++ (editor aid) | none |
| `PCB_DRILL_MAP` | `OpaqueItem` | as above | **gap**: C++ draws drill symbols at the holes on the map's layer; needs the drill symbol assigner | none |
| `(setup (drill_symbol_profile ...))` | verbatim node | exact | only used by drill map/chart | none |
| embedded files | metadata typed, payload verbatim | exact | fonts: payload is decoded only if an outline font is requested | none |
| `PCB_MARKER` | full | not saved by C++ either; exclusions go to the project file | none | produced by DRC |
| `PCB_BOARD_OUTLINE` | derived cache | not saved | via `Board::board_outline` | edge clearance uses the same polygons |

Why typed headers even for opaque items: the writer sorts `Drawings()` with
`BOARD::cmp_drawings` (KICAD_T ordinal, then layer for single-layer types, then UUID), so the
type, layer and UUID of *every* drawing must be known even if its body is not interpreted.

---

## 4. The shared behaviour API

### 4.1 `trait BoardItem` (port of the `EDA_ITEM` / `BOARD_ITEM` virtual interface)

Default bodies are the C++ base-class bodies (`include/board_item.h`, `pcbnew/board_item.cpp`).
Every method that can reach beyond `self` takes the context; this is uniform on purpose so that
implementers never have to guess ("does `PCB_GROUP::GetLayerSet` need the board? yes, it unions
its members").

```rust
pub trait BoardItem: Invalidate {
    // ---- identity and structure (no context) ----------------------------------------------
    fn base(&self) -> &BoardItemBase;
    fn base_mut(&mut self) -> &mut BoardItemBase;
    fn type_(&self) -> KicadT;                                    // Type()
    fn class_name(&self) -> &'static str;                         // GetClass()
    /// The BOARD_CONNECTED_ITEM part, if this type derives from it.
    fn connected(&self) -> Option<&ConnectedItemBase> { None }
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> { None }

    // ---- layers ---------------------------------------------------------------------------
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId { self.base().layer }              // GetLayer
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset;                                     // GetLayerSet (default: { m_layer } or empty)
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool {               // IsOnLayer
        self.base().layer == layer
    }
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool;                            // IsOnCopperLayer
    fn is_layer_agnostic(&self) -> bool { false }                                        // IsLayerAgnostic
    fn fits_enabled_layers(&self, ctx: &BoardCtx<'_>, enabled: Lset,
                           copper_layer_count: i32) -> bool;                             // FitsEnabledLayers

    // ---- geometry -------------------------------------------------------------------------
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I;                                  // GetPosition
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I { self.bounding_box(ctx).centre() } // GetCenter
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I;                                 // GetBoundingBox
    /// GetEffectiveShape( aLayer, aFlash, aUsage ). `PcbLayerId::UNDEFINED_LAYER` = all layers.
    fn effective_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, flash: Flashing,
                       usage: DrcConstraintT) -> SharedShape;
    /// GetEffectiveHoleShape; `None` where C++ returns a null pointer.
    fn effective_hole_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId,
                            usage: DrcConstraintT) -> Option<Arc<ShapeSegment>> { None }
    /// TransformShapeToPolygon.
    fn transform_shape_to_polygon(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet,
                                  layer: PcbLayerId, clearance: i32, error: i32,
                                  error_loc: ErrorLoc, ignore_line_width: bool);
    /// TransformShapeToPolySet (hatched fills, dashed strokes); default forwards to the above.
    fn transform_shape_to_poly_set(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet,
                                   layer: PcbLayerId, clearance: i32, error: i32,
                                   error_loc: ErrorLoc, dashes: Option<&dyn DashSettings>);
    fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool { false }                             // HasHole
    fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool { false }                     // HasDrilledHole
    fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool { false }         // IsTented
    fn has_line_stroke(&self) -> bool { false }                                          // HasLineStroke
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams;                                // GetStroke
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool { false }
    fn hit_test_rect(&self, ctx: &BoardCtx<'_>, rect: &Box2I, contained: bool,
                     accuracy: i32) -> bool { false }

    // ---- state ----------------------------------------------------------------------------
    /// BOARD_ITEM::IsLocked: parent group locked, FPHOLDER rule, then m_isLocked. PAD overrides.
    fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool;
    fn is_knockout(&self) -> bool { self.base().is_knockout }

    // ---- structure ------------------------------------------------------------------------
    /// RunOnChildren, in the C++ child order.
    fn run_on_children(&self, ctx: &BoardCtx<'_>, mode: RecurseMode,
                       f: &mut dyn FnMut(ItemRef<'_>)) {}

    // ---- text used by DRC reports -----------------------------------------------------------
    fn item_description(&self, ctx: &BoardCtx<'_>, units: &dyn UnitsProvider, full: bool) -> String;
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String;                               // GetFriendlyName
    fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String;                         // LayerMaskDescribe
}

/// BOARD_CONNECTED_ITEM virtuals. Implemented by PcbTrack/PcbArc/PcbVia (via Track), Pad, Zone, PcbShape.
pub trait BoardConnectedItem: BoardItem {
    fn net_code(&self) -> NetCode { self.connected().map_or(NetCode::ORPHANED, |c| c.net) }   // GetNetCode
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool { true }      // IsConnected (ZONE: !rule area; PCB_SHAPE: copper && no parent footprint)
    fn effective_netclass<'a>(&self, ctx: &BoardCtx<'a>) -> &'a Netclass;                 // GetEffectiveNetClass
    /// GetOwnClearance( aLayer, &source ): through `ctx.rules`, 0 without an engine.
    fn own_clearance(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, want_source: bool) -> (i32, Option<String>);
    fn local_clearance(&self) -> Option<i32> { None }                                     // GetLocalClearance()
    /// GetLocalClearance( &source ) / GetClearanceOverrides( &source ).
    fn clearance_overrides(&self, ctx: &BoardCtx<'_>, want_source: bool) -> Option<(i32, Option<String>)> { None }
}

/// Mutating BOARD_ITEM virtuals for leaf items (containers cascade through `Board`, 2.7.2).
pub trait BoardItemMut: BoardItem {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I);                              // SetPosition
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I);                                 // Move
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle);                // Rotate
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);               // Flip
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);             // Mirror( centre, dir )
    fn mirror_axis(&mut self, env: &MutEnv, axis: &Seg);                                  // Mirror( SEG )
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId);                             // SetLayer
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset);                              // SetLayerSet
    fn normalize(&mut self) {}                                                            // Normalize
    fn normalize_for_compare(&mut self) { self.normalize() }                              // NormalizeForCompare
    fn on_footprint_transformed(&mut self, env: &MutEnv) {}                               // OnFootprintTransformed
    fn on_footprint_rescaled(&mut self, env: &MutEnv, ratio_x: f64, ratio_y: f64,
                             linear_factor: f64, anchor: Vector2I, parent_rotate: EdaAngle) {}
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {}
}
```

### 4.2 `ItemRef<'a>`: what consumers call

```rust
impl<'a> ItemRef<'a> {
    // identity
    pub fn id(&self) -> Option<ItemId>;                 // None for detached values
    pub fn uuid(&self) -> Kiid;                         // m_Uuid
    pub fn type_(&self) -> KicadT;
    pub fn base(&self) -> &'a BoardItemBase;
    pub fn any(&self) -> AnyItem<'a>;                   // for `match`
    pub fn as_dyn(&self) -> &'a dyn BoardItem;

    // downcasts (static_cast / dynamic_cast)
    pub fn as_footprint(&self) -> Option<&'a Footprint>;
    pub fn as_pad(&self) -> Option<&'a Pad>;
    pub fn as_track(&self) -> Option<&'a Track>;        // segment, arc or via
    pub fn as_via(&self) -> Option<&'a PcbVia>;
    pub fn as_arc(&self) -> Option<&'a PcbArc>;
    pub fn as_zone(&self) -> Option<&'a Zone>;
    pub fn as_shape(&self) -> Option<&'a PcbShape>;     // PCB_SHAPE, also the box of a textbox / cell
    pub fn as_text(&self) -> Option<&'a PcbText>;       // PCB_TEXT, also the text base of field / dimension
    pub fn as_eda_text(&self) -> Option<&'a EdaText>;   // any EDA_TEXT derivative
    pub fn as_field(&self) -> Option<&'a PcbField>;
    pub fn as_textbox(&self) -> Option<&'a PcbTextbox>;
    pub fn as_table(&self) -> Option<&'a PcbTable>;
    pub fn as_dimension(&self) -> Option<&'a PcbDimensionBase>;
    pub fn as_group(&self) -> Option<&'a PcbGroup>;     // group or the group base of a generator
    // ... barcode, image, target, point, generator, marker, constraint, body, net

    // relations
    pub fn parent(&self) -> Option<ItemRef<'a>>;                 // GetParent (None for board-level)
    pub fn parent_footprint(&self) -> Option<ItemRef<'a>>;       // GetParentFootprint
    pub fn parent_group(&self) -> Option<ItemRef<'a>>;           // GetParentGroup
    pub fn children(&self, mode: RecurseMode) -> Vec<ItemRef<'a>>;   // RunOnChildren

    // BOARD_ITEM interface with the context bound
    pub fn layer(&self) -> PcbLayerId;
    pub fn layer_set(&self) -> Lset;
    pub fn is_on_layer(&self, layer: PcbLayerId) -> bool;
    pub fn is_on_copper_layer(&self) -> bool;
    pub fn position(&self) -> Vector2I;
    pub fn center(&self) -> Vector2I;
    pub fn bounding_box(&self) -> Box2I;
    pub fn effective_shape(&self, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT) -> SharedShape;
    pub fn effective_hole_shape(&self, layer: PcbLayerId, usage: DrcConstraintT) -> Option<Arc<ShapeSegment>>;
    pub fn transform_shape_to_polygon(&self, buffer: &mut ShapePolySet, layer: PcbLayerId,
                                      clearance: i32, error: i32, error_loc: ErrorLoc,
                                      ignore_line_width: bool);
    pub fn has_hole(&self) -> bool;
    pub fn has_drilled_hole(&self) -> bool;
    pub fn is_tented(&self, layer: PcbLayerId) -> bool;
    pub fn is_locked(&self) -> bool;
    pub fn item_description(&self, units: &dyn UnitsProvider, full: bool) -> String;

    // non-virtual BOARD_ITEM helpers
    pub fn flipped_layer(&self, layer: PcbLayerId) -> PcbLayerId;   // GetFlippedLayer
    pub fn board_copper_layer_count(&self) -> i32;                  // BoardCopperLayerCount
    pub fn board_layer_set(&self) -> Lset;                          // BoardLayerSet
    pub fn max_error(&self) -> i32;                                 // GetMaxError
    pub fn layer_name(&self) -> String;                             // GetLayerName
    pub fn fp_relative_position(&self) -> Vector2I;                 // GetFPRelativePosition
    pub fn is_side_specific(&self) -> bool;                         // IsSideSpecific

    // BOARD_CONNECTED_ITEM
    pub fn is_connected(&self) -> bool;                             // IsConnected (false for non-connected types)
    pub fn connected(&self) -> Option<&'a ConnectedItemBase>;
    pub fn net_code(&self) -> Option<NetCode>;                      // None for non-connected types
    pub fn net(&self) -> Option<&'a NetinfoItem>;                   // GetNet
    pub fn netname(&self) -> &'a str;                               // GetNetname ("" if none)
    pub fn effective_netclass(&self) -> Option<&'a Netclass>;       // GetEffectiveNetClass
    pub fn own_clearance(&self, layer: PcbLayerId) -> i32;          // GetOwnClearance
    pub fn own_clearance_with_source(&self, layer: PcbLayerId) -> (i32, String);

    /// The same item, marked as standing in for its hole during rule evaluation
    /// (replaces toggling the HOLE_PROXY flag on the item).
    pub fn as_hole_proxy(self) -> ItemRef<'a>;
    pub fn is_hole_proxy(&self) -> bool;
}
```

Per-layer padstack geometry is reached exactly as in C++, through the typed item:
`pad.padstack.shape( layer )`, `pad.size( ctx, layer )`, `pad.offset( ctx, layer )`,
`pad.shape_pos( ctx, layer )`, `pad.flash_layer( ctx, layer )`,
`via.width( layer )`, `via.padstack.drill.start`, `via.flash_layer( ctx, layer )`,
`padstack.for_each_unique_layer( |layer| ... )`, `padstack.effective_layer_for( layer )`.

### 4.3 What consumer code looks like

The start of `DRC_TEST_PROVIDER::forEachGeometryItem` and a typical pair test, ported:

```rust
/// Port of `DRC_TEST_PROVIDER::forEachGeometryItem` (tracks part).
for (id, track) in ctx.board.tracks() {
    if (track.layer_set(ctx) & layers).any() && type_mask[track.type_() as usize] {
        func(ctx.item(id.into()).unwrap());
        n += 1;
    }
}
// ... drawings, zones, then:
for (fp_id, _fp) in ctx.board.footprints() {
    if type_mask[KicadT::PcbPadT as usize] {
        for (pad_id, pad) in ctx.board.footprint_pads(fp_id) {
            // Careful: if a pad has a hole then it pierces all layers
            if pad.has_hole(ctx) || (pad.layer_set(ctx) & layers).any() {
                if !func(ctx.item(pad_id.into()).unwrap()) { return n; }
                n += 1;
            }
        }
    }
}

/// Illustration only (the real code is the port of
/// `DRC_TEST_PROVIDER_COPPER_CLEARANCE::testTrackAgainstItem`): the shape of a pair test
/// between a track and whatever the R-tree returns.
fn test_track_against(ctx: &BoardCtx<'_>, engine: &DrcEngine, track: ItemRef<'_>,
                      other: ItemId, layer: PcbLayerId) -> Option<(Kiid, Kiid, i32)> {
    let other = ctx.item(other)?;                       // handle from the tree -> BOARD_ITEM*
    if let (Some(a), Some(b)) = (track.net_code(), other.net_code()) {
        if a == b && a != NetCode::UNCONNECTED { return None; }
    }
    if let AnyItem::Pad(pad) = other.any() {            // static_cast<PAD*>( other )
        if pad.attribute == PadAttrib::Npth && !pad.flash_layer(ctx, layer) { return None; }
    }
    let c = engine.eval_rules(ctx, DrcConstraintT::ClearanceConstraint, Some(track), Some(other), layer);
    let shape_a = track.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
    let shape_b = other.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
    let clearance = c.value.min();
    let actual = shape_a.collide(shape_b.as_ref(), clearance)?;   // SHAPE::Collide( other, clearance, &actual )
    Some((track.uuid(), other.uuid(), actual))
}
```

Nothing in this code holds a borrow of an item longer than the expression that uses it; the
only long-lived things are `ItemId`s (in the R-tree) and `Kiid`s (in the violation).

---

## 5. Board-level services

All signatures are `impl Board` unless stated; the C++ origin is `pcbnew/board.cpp` unless stated.

### 5.1 Iteration in the orders the C++ code relies on

```rust
// Container order (what `for( x : board->Xxx() )` sees).
pub fn tracks(&self) -> impl Iterator<Item = (TrackId, &Track)> + '_;              // Tracks()
pub fn footprints(&self) -> impl Iterator<Item = (FootprintId, &Footprint)> + '_;   // Footprints()
pub fn drawings(&self) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_;         // Drawings()
pub fn zones(&self) -> impl Iterator<Item = (ZoneId, &Zone)> + '_;                  // Zones()
pub fn groups(&self) -> impl Iterator<Item = (GroupId, &PcbGroup)> + '_;            // Groups()
pub fn generators(&self) -> impl Iterator<Item = (GeneratorId, &PcbGenerator)> + '_;
pub fn constraints(&self) -> impl Iterator<Item = (ConstraintId, &PcbConstraint)> + '_;
pub fn board_points(&self) -> impl Iterator<Item = (PointId, &PcbPoint)> + '_;      // Points()
pub fn markers(&self) -> impl Iterator<Item = (MarkerId, &PcbMarker)> + '_;         // Markers()

pub fn footprint_fields(&self, fp: FootprintId) -> impl Iterator<Item = (FieldId, &PcbField)> + '_;    // GetFields()
pub fn footprint_pads(&self, fp: FootprintId) -> impl Iterator<Item = (PadId, &Pad)> + '_;             // Pads()
pub fn footprint_drawings(&self, fp: FootprintId) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_; // GraphicalItems()
pub fn footprint_zones(&self, fp: FootprintId) -> impl Iterator<Item = (ZoneId, &Zone)> + '_;          // Zones()
pub fn footprint_groups(&self, fp: FootprintId) -> impl Iterator<Item = (GroupId, &PcbGroup)> + '_;
pub fn footprint_points(&self, fp: FootprintId) -> impl Iterator<Item = (PointId, &PcbPoint)> + '_;
pub fn table_cells(&self, table: TableId) -> impl Iterator<Item = (TablecellId, &PcbTablecell)> + '_;  // GetCells()

// Id-only snapshots (take one before a loop that mutates).
pub fn track_ids(&self) -> Vec<TrackId>;
pub fn footprint_ids(&self) -> Vec<FootprintId>;
pub fn zone_ids(&self) -> Vec<ZoneId>;

// Ported aggregate walks, same visiting order as C++.
pub fn get_pads(&self) -> Vec<PadId>;                           // BOARD::GetPads: footprints, then pads
pub fn all_connected_items(&self) -> Vec<ItemId>;               // BOARD::AllConnectedItems
pub fn collect_owned_items(&self) -> Vec<ItemId>;               // tracks, zones, generators, footprints,
                                                                //   drawings, markers, groups, constraints, points
pub fn get_item_set(&self) -> Vec<ItemId>;                      // BOARD::GetItemSet: sorted by CompareByUuid
pub fn run_on_children(&self, mode: RecurseMode, f: &mut dyn FnMut(ItemId));   // BOARD::RunOnChildren
pub fn get_zone_list(&self, include_zones_in_footprints: bool) -> Vec<ZoneId>; // BOARD::GetZoneList
pub fn tracks_in_net(&self, net: NetCode) -> Vec<TrackId>;      // BOARD::TracksInNet
pub fn get_sorted_pad_list_by_x_then_y_coord(&self, ctx: &BoardCtx<'_>, net: Option<NetCode>) -> Vec<PadId>;
```

Save-order comparators are free functions next to the type that owns the C++ comparator, all
with the shape `fn(ctx: &BoardCtx<'_>, a, b) -> Ordering`:
`board_item_ptr_cmp` (`BOARD_ITEM::ptr_cmp`), `board_cmp_items`, `board_cmp_drawings`,
`pcb_track_cmp_tracks`, `footprint_cmp_drawings`, `footprint_cmp_pads`, `footprint_cmp_zones`,
`pcb_point_cmp_points` (`PCB_GROUP::ptr_cmp` used by the footprint writer is the inherited
`BOARD_ITEM::ptr_cmp`). The final C++ tie-break `a < b` on pointers (only reachable with
duplicate UUIDs) becomes `ItemId` order; recorded as a deviation.

### 5.2 Item lookup

```rust
pub fn resolve_item(&self, uuid: &Kiid) -> Option<ItemId>;          // BOARD::ResolveItem (nil -> None)
pub fn get(&self, id: ItemId) -> Option<AnyItem<'_>>;               // handle -> item
pub fn contains(&self, id: ItemId) -> bool;
pub fn track(&self, id: TrackId) -> Option<&Track>;                 // typed getters, one per kind
pub fn pad(&self, id: PadId) -> Option<&Pad>;
pub fn footprint(&self, id: FootprintId) -> Option<&Footprint>;
pub fn zone(&self, id: ZoneId) -> Option<&Zone>;
pub fn uuid_of(&self, id: ItemId) -> Option<Kiid>;
pub fn rebind_item_uuid(&mut self, id: ItemId, new_uuid: Kiid);     // RebindItemUuid
pub fn repair_duplicate_item_uuids(&mut self) -> usize;             // RepairDuplicateItemUuids
pub fn find_footprint_by_reference(&self, reference: &str) -> Option<FootprintId>;
pub fn find_footprint_by_path(&self, path: &KiidPath) -> Option<FootprintId>;
pub fn find_pad_by_uuid(&self, uuid: &Kiid) -> Option<PadId>;
pub fn get_first_footprint(&self) -> Option<FootprintId>;
```

Unlike C++, the index is always complete (invariant 4 of 2.10), so the linear fallback scan of
`ResolveItem` is not needed; with duplicate UUIDs the later binding wins, as in
`BOARD_ITEM_INDEX::BindItem`.

### 5.3 Nets and net classes

```rust
pub fn net_info(&self) -> &NetinfoList;                              // GetNetInfo
pub fn nets(&self) -> impl Iterator<Item = &NetinfoItem> + '_;       // by name: `for( net : GetNetInfo() )`
pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem> + '_;   // NetsByNetcode()
pub fn net_count(&self) -> usize;                                    // GetNetCount
pub fn find_net(&self, code: NetCode) -> Option<&NetinfoItem>;       // FindNet( int )
pub fn find_net_by_name(&self, name: &str) -> Option<&NetinfoItem>;  // FindNet( wxString )
pub fn find_or_add_net(&mut self, name: &str, code: Option<NetCode>) -> NetCode;   // FindOrAddNet
pub fn remove_net(&mut self, code: NetCode);                         // Remove( NETINFO_ITEM ): items fall back to net 0
pub fn remove_unused_nets(&mut self);
pub fn build_list_of_nets(&mut self);
pub fn sanitize_netcodes(&mut self);
pub fn map_nets(&mut self, dest: &mut Board);
pub fn match_dp_suffix(net_name: &str) -> (i32, String);             // MatchDpSuffix: (polarity, complement)
pub fn dp_coupled_net(&self, net: NetCode) -> Option<NetCode>;       // DpCoupledNet
pub fn get_nodes_count(&self, net: Option<NetCode>) -> u32;          // GetNodesCount
pub fn set_areas_net_codes_from_net_names(&mut self) -> i32;
/// SynchronizeNetsAndNetClasses: stores the effective netclass in every NetinfoItem.
pub fn synchronize_nets_and_netclasses(&mut self, reset_track_and_via_sizes: bool);
pub fn apply_net_chain_netclasses(&mut self);
pub fn get_net_class_assignment_candidates(&self) -> BTreeSet<String>;

impl NetSettings {                                                   // common/project/net_settings.cpp
    /// GetEffectiveNetClass: explicit label assignments, pattern assignments (EdaCombinedMatcher),
    /// chain patterns, composite netclass by priority, missing fields filled from Default.
    pub fn get_effective_netclass(&self, net_name: &str) -> Arc<Netclass>;
    pub fn get_cached_effective_netclass(&self, net_name: &str) -> Option<Arc<Netclass>>;
    pub fn has_effective_netclass(&self, net_name: &str) -> bool;
    pub fn recompute_effective_netclasses(&mut self);
    pub fn clear_all_caches(&mut self);
    pub fn clear_cache_for_net(&mut self, net_name: &str);
    pub fn get_net_class_by_name(&self, name: &str) -> Option<Arc<Netclass>>;
    pub fn set_netclass_pattern_assignment(&mut self, pattern: &str, netclass: &str);
    pub fn load_from_json(&mut self, json: &JsonValue) -> Result<(), SettingsError>;   // "net_settings" of .kicad_pro
}
```

Effective netclass of an item: `ctx.board.find_net( code ).netclass`, else the C++ fallbacks
(`GetEffectiveNetClass` on the board's net settings, else the static default netclass), in
`BoardConnectedItem::effective_netclass`.

### 5.4 Design settings, DRC severities, exclusions

```rust
pub fn design_settings(&self) -> &BoardDesignSettings;
pub fn design_settings_mut(&mut self) -> &mut BoardDesignSettings;   // bumps the time stamp, rebuilds LayerEnv
pub fn get_max_clearance_value(&self, ctx: &BoardCtx<'_>) -> i32;    // GetMaxClearanceValue (cached)
pub fn resolve_drc_exclusions(&mut self, create_markers: bool) -> Vec<MarkerId>;
pub fn record_drc_exclusions(&mut self);
pub fn delete_markers(&mut self, warnings_and_errors: bool, exclusions: bool);

impl BoardDesignSettings {
    pub fn get_severity(&self, drc_error_code: i32) -> Severity;     // GetSeverity
    pub fn ignore(&self, drc_error_code: i32) -> bool;               // Ignore
    pub fn get_biggest_clearance_value(&self) -> i32;
    pub fn get_smallest_clearance_value(&self) -> i32;
    pub fn get_current_track_width(&self) -> i32;
    pub fn get_current_via_size(&self) -> i32;
    pub fn get_current_via_drill(&self) -> i32;
    pub fn get_hole_plating_thickness(&self) -> i32;
    pub fn get_line_thickness(&self, layer: PcbLayerId) -> i32;
    pub fn get_text_size(&self, layer: PcbLayerId) -> Vector2I;
    pub fn get_text_thickness(&self, layer: PcbLayerId) -> i32;
    pub fn get_layer_class(&self, layer: PcbLayerId) -> usize;
    /// The PARAM table of the BOARD_DESIGN_SETTINGS constructor: reads the "board.design_settings"
    /// object of a .kicad_pro (rule_severities, rules.*, defaults.*, track_widths, via_dimensions, ...).
    pub fn load_from_json(&mut self, json: &JsonValue) -> Result<(), SettingsError>;
}
```

### 5.5 Stack-up, layers

```rust
pub fn get_stackup_or_default(&self) -> BoardStackup;                // GetStackupOrDefault
pub fn copper_layer_count(&self) -> i32;                             // GetCopperLayerCount
pub fn set_copper_layer_count(&mut self, count: i32);
pub fn user_defined_layer_count(&self) -> i32;
pub fn copper_layer_stack_max_id(&self) -> PcbLayerId;               // GetCopperLayerStackMaxId
pub fn enabled_layers(&self) -> Lset;                                // GetEnabledLayers
pub fn set_enabled_layers(&mut self, layers: Lset);
pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool;
pub fn layer_name(&self, layer: PcbLayerId) -> String;               // GetLayerName (user name or standard)
pub fn set_layer_name(&mut self, layer: PcbLayerId, name: &str) -> bool;
pub fn layer_id(&self, name: &str) -> PcbLayerId;                    // GetLayerID
pub fn layer_type(&self, layer: PcbLayerId) -> LayerT;               // GetLayerType
pub fn set_layer_type(&mut self, layer: PcbLayerId, t: LayerT) -> bool;
pub fn set_layer_descr(&mut self, layer: PcbLayerId, descr: Layer) -> bool;
pub fn is_front_layer(&self, layer: PcbLayerId) -> bool;
pub fn is_back_layer(&self, layer: PcbLayerId) -> bool;
pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId;           // FlipLayer
pub fn layer_depth(&self, start: PcbLayerId, end: PcbLayerId) -> i32;   // LayerDepth
pub fn has_items_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool;
pub fn remove_all_items_on_layer(&mut self, layer: PcbLayerId) -> bool;
```

### 5.6 Title block, page, properties, variants, text variables, geometry

```rust
// plain public fields: titles, paper, plot_options, properties, variant_names, current_variant
pub fn has_variant(&self, name: &str) -> bool;
pub fn add_variant(&mut self, name: &str);
pub fn delete_variant(&mut self, name: &str);                        // cascades into every footprint
pub fn rename_variant(&mut self, old: &str, new: &str);
pub fn get_variant_description(&self, name: &str) -> String;
pub fn synchronize_properties(&mut self);                            // project text vars -> properties
/// BOARD::ResolveTextVar( token, variant, depth ).
pub fn resolve_text_var(&self, ctx: &BoardCtx<'_>, token: &mut String, variant: &str, depth: i32) -> bool;
pub fn convert_cross_references_to_kiids(&self, source: &str) -> String;
pub fn convert_kiids_to_cross_references(&self, source: &str) -> String;

pub fn compute_bounding_box(&self, ctx: &BoardCtx<'_>, board_edges_only: bool,
                            physical_layers_only: bool) -> Box2I;    // ComputeBoundingBox
pub fn board_edges_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I;
/// GetBoardPolygonOutlines; returns (outlines, success).
pub fn get_board_polygon_outlines(&self, ctx: &BoardCtx<'_>, infer_outline_if_necessary: bool,
                                  error_handler: Option<&mut dyn OutlineErrorHandler>,
                                  allow_use_arcs_in_polygons: bool,
                                  include_npth_as_outlines: bool) -> (ShapePolySet, bool);
pub fn get_pad_at(&self, ctx: &BoardCtx<'_>, pos: Vector2I, layers: Lset) -> Option<PadId>;   // GetPad
pub fn groups_sanity_check(&mut self, repair: bool) -> String;
pub fn build_courtyard_caches(&mut self, fp: FootprintId, error_handler: Option<&mut dyn OutlineErrorHandler>);
pub fn build_net_tie_cache(&mut self, fp: FootprintId);
pub fn fixup_embedded_data(&mut self);
```

---

## 6. How the design serves each consumer

**Parser (`pcb_io_kicad_sexpr_parser.cpp`).** `parseBOARD_unchecked` keeps its shape: one
`match` on the token, `board.add_xxx( parse_xxx( ... ), AddMode::BulkAppend )`. For a footprint
the parser first calls `board.add_footprint( Footprint::default(), BulkAppend )`, then adds each
child with `board.footprint_add_pad( fp, pad, Append )` etc., which is the order of
`new PAD( footprint ); footprint->Add( pad )`. Child parsers produce *lib-frame* values exactly
as the C++ parser does (`SetLibTextPos`, `SetLibSize`, `OverrideLibCoords`); when the
`(transform ...)` token is read the parser calls `footprint_set_position`,
`footprint_set_orientation`, `footprint_set_transform_scale` in that order, like
`parseFOOTPRINT_unchecked` (parser line 6427), which runs the hooks of the children that
already exist (the mandatory fields).
Group / generator members and net-chain terminal pads are read as `Kiid`s and resolved after the
loop with `board.resolve_item`, like `resolveGroups` / `ResolveTerminalPads`. Nets are created on
first use by name (`find_or_add_net`), in file order, so net codes match C++. The journal is off
during parsing. A footprint-library parse uses a `Board` in `FpHolder` mode.

**Writer (`pcb_io_kicad_sexpr.cpp`).** Takes `&BoardCtx`. `format( BOARD )` builds
`Vec<FootprintId>` / `Vec<ItemId>` / `Vec<TrackId>` from the order lists, sorts them with the
ported comparators, and walks them; `format( FOOTPRINT )` does the same with the footprint's
child lists. The nested file structure comes from the child lists, not from the arenas. Lib-frame
values are written directly (no `InverseApply` round trip where C++ writes
`GetLibraryStart()`), board-frame values go through the same `unbakeSize` / `InverseApply`
helpers as C++. Opaque items and verbatim nodes are emitted through the same formatter, so
`Prettify` treats them like everything else.

**Item shape generation (`GetEffectiveShape`, `TransformShapeToPolygon`).** Inherent methods on
the item structs taking `ctx`, exposed through `BoardItem` / `ItemRef`. Results are
`Arc<dyn Shape>`; pad shapes come from `PadCaches`. Conditional flashing asks
`ctx.connectivity` and the item's `zone_layer_overrides`, mask expansion asks `ctx.rules`,
exactly where C++ asks the board. A detached dummy (`pad.clone()` with a changed size) produces
shapes through the same code.

**Connectivity and ratsnest (`pcbnew/connectivity/`).** `CN_ITEM::m_parent` becomes `ItemId`
(plus `ZoneId` + layer + sub-polygon index for `CN_ZONE_LAYER`); `CN_CLUSTER`, the per-net lists
and the RN graphs store handles. `ConnectivityData::build( &ctx )` walks the board in C++ order;
`apply_changes( &ctx, changes )` consumes the journal (`Added` -> `Add`, `Removed` -> lazy
invalidation by handle, `Modified` -> `MarkNetAsDirty( net_before )` + re-add). Net propagation
takes `&mut Board`. Because the structure holds no borrow, it lives next to the board for the
whole session and implements `ConnectivityQuery` for `PAD::FlashLayer`.

**DRC engine and providers (`pcbnew/drc/`).** `EvalRules( type, a, b, layer )` takes
`Option<ItemRef>` for `a` / `b`; everything it reads (`GetEffectiveNetClass`,
`GetParentFootprint`, `GetNetTieCache( item )`, `GetClearanceOverrides`, pad / zone overrides)
is on `ItemRef` or the typed struct. `DRC_RTREE::ITEM_WITH_SHAPE { parent, shape, shapeStorage,
parentShape }` becomes `{ parent: ItemId, shape: SharedShape, parent_shape: SharedShape }`:
8 + 2 x 16 bytes, no lifetime, so the tree can be cached across runs. `forEachGeometryItem` is the
same sequence of loops over `board.tracks()`, `board.drawings()`, `board.zones()`,
`board.footprints()` + children. Pair caches (`PTR_PTR_CACHE_KEY` ...) key on `ItemId`s and are
deterministic. Violations carry `Kiid`s (as `DRC_ITEM::m_ids` does); markers are added after the
providers ran. The engine implements `RuleResolver`, so model code can call back into it.

**Zone filler (`zone_filler.cpp`).** Reads through `&BoardCtx` (tracks, pads with
`effective_polygon`, zones, board outline, design settings, `own_clearance`); writes in
`&mut Board` phases (2.7.5): `zone_layer_overrides` on pads / vias, `Zone::filled_polys_list`,
`fill_flags`, `is_filled`, `need_refill`, `insulated_islands`, `filled_polys_hash`. Dummy pads /
vias for thermal spokes are plain clones. Teardrop zones are added and removed with
`add_zone` / `remove_many`.

**Gerber / drill plotting (`plot_board_layers.cpp`, `plot_brditems_plotter.cpp`,
`gendrill_*`).** Pure readers with `&BoardCtx`. `BRDITEMS_PLOTTER::PlotFootprintGraphicItems`
iterates `board.footprint_drawings( fp )` and matches on `AnyItem`; pads are visited in
`board.footprint_pads( fp )` order; the "plot a modified copy of the pad" idiom is a clone.
Gerber net attributes use `item.netname()` and `pad.number` / `fp.reference()`. The drill file
generators iterate `board.tracks()` vias and `board.get_pads()` and read
`padstack.drill` / `secondary_drill` / `tertiary_drill`, `via_type`, `PadAttrib`.

**CLI.** Owns a `Design { board, connectivity, drc }`; each sub-command is a sequence of
borrow phases (load, `build_connectivity`, `synchronize_nets_and_netclasses`, run, write).

**External autorouter client.** Load -> enumerate (`tracks()`, `get_pads()`, `nets()`,
`zones()`, board outline) -> mutate with `add_track`, `add_via`, `remove`, `modify_track`
(handles stay stable across unrelated edits) -> `drain_changes` into connectivity ->
`refill zones` -> `run DRC` -> `export`. Speculative passes use `board.clone()` (handles remain
valid in the clone) or a `BoardCommit` with `revert`.

**Push-and-shove router (stretch).** `PNS_KICAD_IFACE::SyncWorld` creates PNS items with
`parent: Option<ItemId>`; `PNS::NODE` stays the router's own world. `Commit()` maps
`AddItem` / `RemoveItem` / `UpdateItem` to `BoardCommit::add` / `remove` / `modify`;
`BOARD_COMMIT::Revert` during interactive routing is `BoardCommit::revert`, which restores
removed items under their original handles so the PNS `parent` links stay valid.

**JavaScript / wasm.** wasm-bindgen cannot export a reference into a Rust collection, so every
design needs some id at the boundary; here the model's native id *is* that id.

```ts
// kicad-wasm, sketch of the exported surface. `Id` is a number (ItemId::to_bits, < 2^53).
class Design {
  static load(pcb: string, project?: string, rules?: string): Design;
  save(): string;
  // identity
  resolve(uuid: string): Id | undefined;            // Board::resolve_item
  uuidOf(id: Id): string;
  kindOf(id: Id): "footprint" | "pad" | "track" | "zone" | ...;   // ItemKind
  typeOf(id: Id): string;                           // KICAD_T name, e.g. "PCB_VIA_T"
  isAlive(id: Id): boolean;                         // generation check
  // enumeration: ids in C++ container order, plus packed geometry for bulk consumers
  trackIds(): Float64Array;  padIds(): Float64Array;  zoneIds(): Float64Array;  footprintIds(): Float64Array;
  tracksPacked(): Int32Array;                       // [kind, x1, y1, x2, y2, width, layer, net] * n
  padsPacked(): Int32Array;                         // [x, y, net, footprintIndex, ...] * n
  item(id: Id): object;                             // JSON snapshot of one item (serde)
  nets(): { code: number; name: string; netclass: string }[];
  // mutation
  addTrack(t: { start: [number, number]; end: [number, number]; width: number; layer: string; net: string | number }): Id;
  addVia(v: { at: [number, number]; diameter: number; drill: number; layers: [string, string]; net: string | number }): Id;
  addArc(a: { start: [number, number]; mid: [number, number]; end: [number, number]; width: number; layer: string; net: string | number }): Id;
  update(id: Id, patch: object): void;              // Board::modify
  remove(id: Id): boolean;
  beginCommit(): void;  commit(): void;  revert(): void;
  // services
  rebuildConnectivity(): void;   ratsnest(): Int32Array;
  refillZones(ids?: Id[]): void;
  runDrc(options?: object): { code: string; severity: string; message: string; items: string[]; pos: [number, number] }[];
  plotGerber(layer: string, options?: object): string;
  drillFiles(options?: object): { name: string; content: string }[];
}
```

A stale `Id` raises a JS exception (`get` returns `None`), never memory unsafety. Ids are valid
for one `Design`; UUID strings are the ids to keep across `save()` / `load()`.

---

## 7. Work packages for the model

Sizes are C++ line counts of the code actually ported (UI, property registration, protobuf,
`View*`, `GetMsgPanelInfo` excluded). "Model" packages contain data, structure, transforms,
queries, comparators and descriptions. Shape generation, which the task lists as a downstream
consumer, is split into separate "S" packages because it shares C++ files with the model.

Crate layout (`rust/crates/kicad-pcb/src/`; one module per C++ file, directories where a C++
file is split between packages):

```
arena.rs  ids.rs  cache.rs  item.rs  ctx.rs  drc_api.rs  items_macro.rs      (kernel, new)
board/{mod,containers,nets,layers,queries,outline}.rs   board_item_index.rs  commit.rs
board_connected_item.rs  netinfo.rs  netclass.rs  net_settings.rs  eda_pattern_match.rs
board_design_settings.rs  board_stackup.rs  pcb_plot_params.rs  title_block.rs  page_info.rs
teardrop_parameters.rs  zone_settings.rs  zones.rs  zone/{mod,shapes}.rs
pcb_track/{mod,shapes}.rs  padstack.rs  pad/{mod,derive,shapes}.rs  pad_utils.rs
footprint/{mod,transform,geometry,compare,shapes}.rs  footprint_courtyard_index.rs
eda_shape/{mod,geometry,shapes}.rs  line_ending.rs  pcb_shape/{mod,lib_frame,shapes}.rs
eda_text.rs  pcb_text.rs  pcb_field.rs  pcb_textbox.rs  pcb_tablecell.rs  pcb_table.rs
pcb_dimension.rs  pcb_group.rs  pcb_generator.rs  pcb_target.rs  pcb_point.rs  pcb_marker.rs
pcb_barcode.rs  pcb_reference_image.rs  pcb_constraint.rs  pcb_3d_body.rs  opaque_item.rs
embedded_files.rs  component_class.rs  convert_shape_list_to_polygon.rs
io/kicad_sexpr/{parser,writer}/...                                  (not part of this breakdown)
```

### Phase 0 - kernel (one agent, blocks everyone, about one sitting)

| WP | Content | C++ read | Notes |
|---|---|---|---|
| **K0** | `arena.rs`, `ids.rs`, `cache.rs`, `item.rs` (`BoardItemBase`, `ConnectedItemBase`, `ParentRef`, `ItemFlags`, the three traits with base-class default bodies), `ctx.rs` (`BoardCtx`, `MutEnv`, `LayerEnv`, `ParentFrame`, `RuleResolver`, `ConnectivityQuery`), `drc_api.rs` (`DrcConstraintT`, `DrcConstraint`, `Flashing`, DRC error-code table), the `board_items!` macro | `include/eda_item.h`, `include/eda_item_flags.h`, `include/board_item.h`, `pcbnew/board_item_container.h`, `pcbnew/drc/drc_rule.h` (enum + value part), `pcbnew/drc/drc_item.h` (code list) - about 1700 lines of headers | New code about 1500 lines of Rust with unit tests for the arena (insert / remove / park / take / generation retire / `to_bits` round trip). Publishes `ids.rs` first so Phase 1 can start. |

### Phase 1 - type definitions and container mechanics (three agents in parallel after `ids.rs` exists)

Output of this phase is a crate that compiles with **every struct and enum of section 3**,
`Default` impls matching the C++ constructors, trivial accessors, `Invalidate` impls, and the
`Board` container. Parser, writer and all Phase 2 packages start when this lands.

| WP | Content | C++ files |
|---|---|---|
| **T1** copper types | 3.1 (teardrop types), 3.2-3.5, 3.11 | `pcb_track.h`, `pcb_track_types.h`, `padstack.h`, `pad.h`, `footprint.h`, `zone.h`, `zone_settings.h`, `zones.h`, `teardrop/teardrop_parameters.h`, `teardrop/teardrop_types.h`, `netinfo.h`, `include/netclass.h`, `include/project/net_settings.h`, `include/jumper_group.h`; constructors in `pcb_track.cpp`, `pad.cpp`, `padstack.cpp`, `footprint.cpp`, `zone.cpp`, `zone_settings.cpp` (about 900 lines) |
| **T2** graphic types | 3.6-3.10 | `include/eda_shape.h`, `include/line_ending.h`, `pcb_shape.h`, `include/eda_text.h`, `pcb_text.h`, `pcb_field.h`, `pcb_textbox.h`, `pcb_tablecell.h`, `pcb_table.h`, `pcb_dimension.h`, `pcb_group.h`, `common/eda_group.h`, `pcb_generator.h`, `pcb_target.h`, `pcb_point.h`, `pcb_barcode.h`, `pcb_reference_image.h`, `include/reference_image.h`, `pcb_marker.h`, `include/marker_base.h`, `include/rc_item.h`, `constraints/pcb_constraint.h`, `pcb_3d_body.h`; constructors (about 800 lines) |
| **T3** board, settings, containers | 3.12-3.14; `Board::add*`, `footprint_add_*`, `remove`, `remove_many`, `remove_all`, cascade, UUID index, order-list iterators of 5.1, `resolve_item`, `get_item_set`, `collect_owned_items`, `run_on_children`, journal, `check_invariants` | `board.h`, `include/board_design_settings.h`, `board_stackup_manager/board_stackup.h`, `pcb_plot_params.h`, `include/title_block.h`, `include/page_info.h`, `include/embedded_files.h`, `component_classes/*.h`; `board.cpp` (`Add`, `Remove`, `RemoveAll`, `FinalizeBulk*`, `ResolveItem`, `RepairDuplicateItemUuids`, `GetItemSet`, `collectOwnedItems`, `RunOnChildren`: about 700 lines), `footprint.cpp` (`Add`, `Remove`, `RunOnChildren`: about 250), `board_item_index.cpp` (222), `common/eda_group.cpp` (103) |

### Phase 2 - behaviour (all packages can start together; listed dependencies are call-order only, the signatures are fixed by this document)

| WP | Content | C++ files (ported lines) | Uses |
|---|---|---|---|
| **B01** item base | `BOARD_ITEM` / `BOARD_CONNECTED_ITEM` non-virtual helpers, `ItemRef` forwarding, `is_locked`, `GetFlippedLayer`, custom properties | `board_item.cpp` (566), `board_connected_item.cpp` (340), `common/eda_item.cpp` (flags, custom properties: about 150) | - |
| **B02** nets | `NetinfoItem`, `NetinfoList`, board net bookkeeping (5.3 except net settings) | `netinfo_item.cpp` (237), `netinfo_list.cpp` (389), `common/netclass.cpp` (435), `board.cpp` net functions (about 450) | - |
| **B03** net settings | `NetSettings` incl. JSON load, pattern matcher, effective netclass | `common/project/net_settings.cpp` (1906), `common/eda_pattern_match.cpp` (530) | B02 |
| **B04** design settings | `BoardDesignSettings` (defaults, JSON PARAM table, severities, getters), `ZoneSettings`, teardrop params | `board_design_settings.cpp` (2093), `zone_settings.cpp` (493), `teardrop/teardrop_parameters.cpp` (58) | B03 |
| **B05** stack-up, plot params, page | | `board_stackup_manager/board_stackup.cpp` (1050), `stackup_predefined_prms.cpp` (138), `pcb_plot_params.cpp` (899), `common/title_block.cpp` (200), `common/page_info.cpp` (257) | - |
| **B06** board queries | 5.5, 5.6 (except outline), `get_pads` family, footprint lookup, `get_max_clearance_value`, `groups_sanity_check`, variants, text variables, `board_cmp_items` / `board_cmp_drawings` | `board.cpp` (about 2000 of 4428), `board_bounding_box.cpp` (85), `layer_utils.cpp` (124) | B02 |
| **B07** board outline | `get_board_polygon_outlines`, `ConvertOutlineToPolygon`, footprint courtyard builder helper | `convert_shape_list_to_polygon.cpp` (1964), `pcb_board_outline.cpp` (129) | B08 |
| **B08** `EdaShape` core | setters, arc / ellipse maths, `move` / `rotate` / `flip` / `mirror` / `scale`, `Compare`, bbox, `ShowShape` | `common/eda_shape.cpp` first half (about 2500 of 5001), `common/line_ending.cpp` (336, data part) | - |
| **B09** `PcbShape` model | lib-frame machinery (`sync_lib_coords`, `rebake_from_lib`, `rebake_from_transform`, `on_footprint_rescaled`), transforms, layer set, connection points, description | `pcb_shape.cpp` (about 1700 of 2223) | B08 |
| **B10** text | `EdaText` (attributes, shown text, positions, `Compare`), `PcbText` lib-frame accessors and transforms, `PcbField` | `common/eda_text.cpp` (about 1100 of 1467), `pcb_text.cpp` (about 600 of 952), `pcb_field.cpp` (397) | - |
| **B11** text boxes and tables | | `pcb_textbox.cpp` (1147), `pcb_table.cpp` (1165), `pcb_tablecell.cpp` (467): about 2000 ported | B09, B10 |
| **B12** dimensions | all five kinds incl. `update()` and shape construction | `pcb_dimension.cpp` (2546) | B10 |
| **B13** tracks and vias | transforms, layer logic (`SanitizeLayers`, `LayerPair`, `GetLayerSet`), `FlashLayer`, tenting / covering / plugging resolution, width / drill constraints, bbox, descriptions, `cmp_tracks` | `pcb_track.cpp` (about 2400 of 3940) | B14 |
| **B14** padstack | | `padstack.cpp` (about 1400 of 1950), `pad_utils.cpp` (452) | - |
| **B15** pad model | lib / board derivations (`derivePadBoardSize` ...), attribute / layer masks, `FlashLayer`, clearance / mask / paste / zone-connection overrides, flip / mirror / rotate, primitives, `Compare`, descriptions | `pad.cpp` (about 2000 of 4051) | B14 |
| **B16** footprint model A | fields, variants, attributes, the `Board::footprint_*` transform family (2.7.3), duplicate / `copy_footprint_from` with handle remap, `FixUuids`, pad lookup, net-tie / jumper groups | `footprint.cpp` (about 2500 of 5378) | B10, B15 (hooks) |
| **B17** footprint model B | bbox / hull / layer bbox, courtyard caches, net-tie cache, `cmp_drawings` / `cmp_pads` / `cmp_zones`, text variables, `is_on_layer`, description, courtyard index | `footprint.cpp` (about 2000), `footprint_courtyard_index.cpp` (73) | B07, B16 |
| **B18** zone model | data ops, layers, priority, outline transforms (lib / board), border hatch lines, fill storage / hash / islands, thermal getters, description | `zone.cpp` (about 1900 of 2699) | - |
| **B19** groups, generators, small items | `PcbGroup`, `PcbGenerator` (+ `normalize_properties`), `PcbTarget`, `PcbPoint`, `PcbMarker` / `RcItem`, `PcbConstraint` tokens, `Pcb3dBody` | `pcb_group.cpp` (637), `pcb_generator.cpp` (223), `pcb_target.cpp` (255), `pcb_point.cpp` (368), `pcb_marker.cpp` (569), `common/marker_base.cpp` (147), `common/rc_item.cpp` (data part, about 150), `constraints/pcb_constraint.cpp` (about 200), `pcb_3d_body.cpp` (198), `generators/pcb_tuning_pattern.cpp` (properties only, about 150) | - |
| **B20** images, embedded files, component classes | | `pcb_reference_image.cpp` (506), `common/reference_image.cpp` (about 200), `common/embedded_files.cpp` (748), `component_classes/*.cpp` (707) | - |
| **B21** commits and copies | `BoardCommit` (park / unpark / revert), `with_item_taken`, `Board::clone` tests, cross-board copy | `board_commit.cpp` (semantics only; about 600 lines of new Rust) | T3 |
| **B22** barcode (Tier 2) | data behaviour + symbol generation through a pure-Rust encoder | `pcb_barcode.cpp` (1124) | B10 |
| **B23** generated tables (Tier 2) | regeneration of drill chart / stack-up table, `RefreshGeneratedTables` | `pcb_generated_table.cpp` (1184), `pcb_drill_chart.cpp` (548), `pcb_stackup_table.cpp` (107), `drill/drill_chart_model.*` | B11, drill writer |

Shape-generation packages (consumer "item shapes"; listed so the file split is unambiguous):

| WP | Content | C++ files (ported lines) |
|---|---|---|
| **S1** | `EdaShape::make_effective_shapes`, `transform_shape_to_polygon`, hatching, bezier flattening, hit tests | `common/eda_shape.cpp` second half (about 2500) |
| **S2** | `PcbShape`, `PcbText`, `PcbTextbox`, `PcbTable` effective shapes and polygon transforms, knockouts | `pcb_shape.cpp` (about 500), `pcb_text.cpp` (about 350), `pcb_textbox.cpp` / `pcb_table.cpp` (about 500), `common/eda_text.cpp` (about 350) |
| **S3** | pad effective shapes / polygons, hole shape, mask and paste shapes, custom-shape merge | `pad.cpp` (about 1500) |
| **S4** | track / via shapes, zone smoothing and fill-to-polygon transforms, footprint `TransformPadsToPolySet` / `TransformFPShapesToPolySet` / hull | `pcb_track.cpp` (about 600), `zone.cpp` (about 800), `footprint.cpp` (about 500) |

Dependency order: `K0` -> `{T1, T2, T3}` -> everything else. Parallel sets after Phase 1:
`{B01, B02, B05, B08, B10, B14, B18, B19, B20, B21}` have no call dependencies;
`{B03, B04, B06, B07, B09, B11, B12, B13, B15}` call into the first set;
`{B16, B17}` last; `S1..S4` in parallel with all of Phase 2 once B08 / B10 / B14 exist.
Two agents never edit the same file: where a C++ file is split (`footprint.cpp`, `pad.cpp`,
`eda_shape.cpp`, `board.cpp`), the Rust side is a directory module
(`footprint/{mod,transform,geometry,compare,shapes}.rs`, `pad/{mod,derive,shapes}.rs`,
`eda_shape/{mod,geometry,shapes}.rs`, `board/{mod,containers,nets,layers,queries,outline}.rs`),
each file naming the C++ function ranges it ports.

Acceptance for every package: `Board::check_invariants()` holds after each mutating test; the
matching `qa/tests/pcbnew` cases are ported; no `todo!()` on reachable paths.

---

## 8. Where the arena approach hurts, and the mitigations

| Pain | Why it is real | Mitigation in this design |
|---|---|---|
| **Context parameter on most methods.** C++ `pad->GetPosition()` becomes `pad.position( ctx )`. | Children store lib-frame data and reach the parent transform, board settings, the DRC engine and connectivity. A struct in an arena cannot hold a reference to any of them. | One uniform rule ("every `BoardItem` method takes `ctx`"), and `ItemRef` binds it once so consumer code reads like C++. A parent-pointer design would hide the parameter but not the dependency. |
| **Two environments: `BoardCtx` for reads, `MutEnv` for writes.** | `&mut` to an arena slot excludes `&Board`. | `MutEnv` is a few copies and an `Arc` clone, built by `Board::modify*`; it contains exactly what the C++ mutators read (parent transform, layer flip table, copper count, max error). `with_item_taken` covers the rest. |
| **Bulk footprint transforms are `Board` methods, not `Footprint` methods.** | The footprint cannot reach its children from `&mut self`. | Field-disjoint borrows make the port mechanical (2.7.3); the loop bodies are unchanged. All such functions sit in `footprint/transform.rs`. |
| **Nested file structure is not the storage structure.** | Children of a footprint are spread over ten arenas. | Child order lists in `Footprint` drive parser and writer; arenas are never iterated for semantics. `check_invariants` proves the two views agree. |
| **Save ordering needs explicit sorting through the context.** | C++ sorts pointers with comparators that call virtuals. | The comparators are ported 1:1 as `fn( ctx, a, b ) -> Ordering` over handles; pointer tie-breaks become handle order. |
| **Liveness is a runtime property.** A stale `ItemId` is `None`, not a compile error. | Inherent to handles. | Generations make staleness detectable instead of undefined; `board[id]` only for same-borrow handles, `get` at boundaries; parked slots keep undo handles valid. |
| **Cascades are convention, not types.** Removing a footprint must remove its children; adding to a group must set `group` on the member. | Ownership is flat. | Only `Board` methods mutate structure (arena fields are `pub(crate)`), and `check_invariants` runs in every test. |
| **Moving a footprint between boards needs a handle remap.** | Handles are board-local. | One function (`copy_footprint_from`); C++ has the same step as `ptrMap` in the copy constructor. |
| **About twenty arenas and four enums over them.** | One per concrete type. | A single `board_items!` table generates all of it; adding a type is one line. |
| **`OnceLock` caches cannot be invalidated through `&self`.** | C++ flips `mutable` dirty flags from const code in a few places. | Invalidation happens on the `&mut` path (`Board::modify*`, child hooks); the timestamp-validated caches use `KeyedCache` (mutex). The transform-consistency `debug_assert` catches a missed hook. |
| **Listeners become a pull journal.** | No callbacks into borrowed state. | Deterministic, testable, and what a wasm client wants anyway; costs one explicit `apply_changes` call per mutation batch. |
| **Order-list removal is O(n).** | `VecDeque::retain`. | Same as C++ `std::erase` on a deque; `remove_many` batches. An intrusive position index can be added later without API change. |

What the approach buys in return: no lifetimes in item or consumer types, `Board: Clone + Sync`,
deterministic cache keys, O(1) UUID and handle lookup, consumers that survive board mutation,
a trivial wasm ABI, and dense memory.

---

## 9. Risks and open questions

### 9.1 Risks

a. **The lib-frame / baked duality is new and subtle** (non-uniform footprint scale turns circles
   into ellipses, `PcbTextbox` keeps its lib shape as a rectangle while the runtime shape may be
   a polygon, pad primitives live in the pad frame and are re-baked with `GetPrimitiveLibScale`).
   Any implementer working from KiCad 9 knowledge will get it wrong; packages B09, B10, B15, B16
   must port `OnFootprintTransformed` / `OnFootprintRescaled` / `syncLibCoords` line by line and
   test with scaled, rotated, flipped footprints against the oracle.
b. **Callbacks from the model into upper crates** (`RuleResolver`, `ConnectivityQuery`) create
   re-entrancy: `DrcEngine::eval_rules` -> item method -> `rules.cached_own_clearance` ->
   engine cache. Engine-side caches must tolerate re-entrant reads (no `RefCell` borrow held
   across an item call). The default behaviour with `rules == None` must equal the C++
   `!m_DRCEngine` branches; each is a separate test.
c. **Determinism where C++ is unordered**: `std::unordered_set<EDA_ITEM*>` group members,
   `std::unordered_map` padstack copper props, pointer tie-breaks in all comparators, pointer-keyed
   caches. The design fixes an order everywhere; each choice goes into `PORT_STATUS.md`. A
   residual risk is a C++ output that depends on hash iteration order (none found in the save
   path: member ids are sorted).
d. **Net code assignment order is observable** (`PCB_TRACK::cmp_tracks` sorts by net code).
   `find_or_add_net` during parsing, `build_list_of_nets`, `sanitize_netcodes` and
   `getFreeNetCode` must reproduce the C++ numbering exactly or tracks are saved in a different
   order.
e. **Generated tables and barcodes** (3.17): `BOARD_LOADER::SaveBoard` calls
   `RefreshGeneratedTables`, so byte-exact re-save of a board with a stale drill chart needs
   package B23; barcode symbols need encoders whose module layout equals zint's.
f. **Reference images** need pixel dimensions for `GetBoundingBox` (they are drawings, so they
   can enter `BOARD::ComputeBoundingBox`); the header probe must match what wxImage reports
   (PPI rounding).
g. **16-bit generations**: a slot retires after 65535 reuses. A client that adds and removes the
   same single track millions of times grows the arena by one slot per 65535 cycles; acceptable,
   but it must be the documented behaviour, not wrap-around.
h. **UUID creation without randomness** (PORTING.md section 3). New items need `Kiid`s; the
   board owns a `kicad_core::KiidGenerator` (`Default` = `seeded( 5489 )`, deterministic). Two
   boards with default generators produce colliding UUIDs, so the CLI and wasm hosts must call
   `KiidGenerator::from_entropy` before adding items to a board that will be saved; the parser
   must use the same generator for the nil-UUID fix-ups C++ performs while loading.
i. **Scratch flags in items** (`SKIP_STRUCT`, `CANDIDATE`, `IS_DELETED`, `m_arflag`,
   `m_subRatsnest`): algorithms that set them through shared pointers must be restructured to
   side tables; each occurrence is a small, local design decision during porting.
j. **Text and fonts**: `KIFONT::FONT*` becomes `FontRef`; outline fonts are unavailable on wasm
   unless embedded, so plotted text may fall back to the stored render cache or the stroke font.
   `GetShownText` needs text-variable resolution through board, footprint and project.
k. **Memory of `ConnectedItemBase`**: `TeardropParameters` (about 40 bytes) and the custom
   property map sit in every track, as in C++. If profiles show it matters, both can move behind
   an `Option<Box<..>>` without touching consumers (accessor methods).
l. **Handles across two boards**: an `ItemId` does not say which `Board` it belongs to; using it
   on another board may resolve to an unrelated live item. Debug builds can tag arenas with a
   board serial; the wasm layer keeps one `Design` per JS object.

### 9.2 Open questions for the plan owner

a. Foundation types this model needs but that were not in the list given to me:
   `EdaOrientation`, `TransformTrs`, `Ellipse<i32>` / `ShapeEllipse`, `Hash128`, `MinOptMax`,
   `Vector3D`, `StrNumCmp` / `ValueStringCompare` (used by `cmp_pads` and the net-chain writer),
   case-insensitive string compare with wx semantics. Who owns them (`kicad-geom` / `kicad-core`)?
b. Where do the DRC error codes live? `BoardDesignSettings::drc_severities` and the `.kicad_pro`
   reader need the code <-> key table below `kicad-drc`; this proposal puts it in
   `kicad-pcb/src/drc_api.rs`.
c. `PlotFormat`, `DrillMarks`, `PlotTextMode`, `DxfOutlineMode`, `DxfUnits` are needed by
   `PcbPlotParams` (in `kicad-pcb`) and by `kicad-plot`, which do not depend on each other:
   `kicad-core`?
d. Is undo beyond "revert the open commit" in scope? Parking supports a full undo stack, but
   nothing in Tier 1-3 needs more than revert.
e. Should the JS API expose handle numbers (fast, session-local) as primary ids, UUID strings
   (stable, slower), or both as sketched?
f. Markers: kept in the board (as C++, needed for exclusions round-tripping with the project
   file) or returned by the DRC run only? This proposal keeps them in the board.
g. Is the `.kicad_pro` written by the port, or read-only? Read-only lets `meander_settings`,
   `drc_exclusions` payloads and the drill symbol profile stay opaque.
h. Should `PCB_DRILL_MAP` plotting (drill symbols on a layer) be scheduled? It is the one opaque
   item with a visible effect on fabrication output.
