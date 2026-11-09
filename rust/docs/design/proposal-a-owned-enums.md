# Proposal A - board model as plain nested ownership + enums

Status: design proposal for the `kicad-pcb` crate (board data model only; parser, writer and the
consumers are separate work packages that build on it).
C++ reference: this source tree, board file format `SEXPR_BOARD_FILE_VERSION 20261031`.
Angle: `Board` owns its items in plain containers, a `Footprint` owns its children, there is no
arena, no `Rc<RefCell<..>>`, no back-pointers. Polymorphism is done with enums. Everything a
C++ item finds by walking `m_parent` is supplied by a *borrowed view* that carries the parent
context. Identity is the item UUID (`Kiid`) plus cheap positional handles.

The document is deliberately blunt about where this approach is uncomfortable (section 2.9).

---------------------------------------------------------------------------------------------

## 0. Summary

```
Board
  +-- ItemList<Footprint>
  |     +-- ItemList<Pad>, ItemList<PcbField>, ItemList<Drawing>, ItemList<Zone>,
  |         ItemList<PcbGroup>, ItemList<PcbPoint>, ItemList<PcbConstraint>, ItemList<Pcb3dBody>
  +-- ItemList<Track>      enum Track { Segment(PcbTrack), Arc(PcbArc), Via(Box<PcbVia>) }
  +-- ItemList<Zone>
  +-- ItemList<Drawing>    enum Drawing { Shape, Text, Textbox, Table, Dimension, Barcode,
  |                                       ReferenceImage, Target, DrillMap, GridItem, Opaque }
  +-- ItemList<PcbGroup>, ItemList<PcbGenerator>, ItemList<PcbPoint>,
  |   ItemList<PcbMarker>, ItemList<PcbConstraint>
  +-- NetinfoList, BoardDesignSettings { NetSettings, BoardStackup, .. }, layers, title block,
  |   page, plot params, properties, variants, embedded files, component classes
  +-- UuidIndex (Kiid -> stable sequence key), time stamp, structure epoch
```

* **Data**: every C++ class becomes one plain `struct` with the same members (section 3). Structs
  are `Clone + Debug`; a `Board` can be cloned for snapshots.
* **Read access**: consumers never see a bare `&Pad`; they see `PadRef<'a> = View<'a, Pad>`, a
  `Copy` bundle of `(context, parent, handle, &Pad)`. All board-frame getters
  (`position()`, `effective_shape()`, `is_locked()`, `net()`, ...) live on views. `ItemRef<'a>`
  is the enum over all views and is the Rust spelling of `BOARD_ITEM*`.
* **Context**: `BoardCtx<'a>` = `&Board` + optional connectivity hook + optional rule-engine hook.
  It replaces `GetBoard()`, `GetBoard()->GetConnectivity()` and `m_DRCEngine`.
* **Write access**: `&mut Board` and explicit accessors (`board.edit(handle)`,
  `footprint.pad_mut(i)`), structural edits through `Board::add/remove/apply(commit)` which return
  a `ChangeSet` for incremental consumers.
* **Identity**: `Kiid` (durable, what files and JavaScript use), `ItemHandle` (positional, O(1),
  valid until the next removal/front-insert), `ItemKey` (list-local sequence number, stable for the
  life of the item, O(log n) to resolve; what the UUID index stores).
* **Caches**: per-item lazy caches (`LazyCache`, the port of C++ `mutable` + dirty flag + mutex)
  for data derived from the item and its parent; everything that spans items (connectivity,
  R-trees, DRC caches) lives outside `Board` in the consumer that owns it.

Why this is attractive for a statement-by-statement port: the structs are the C++ classes, the
borrow checker proves that no derived structure observes a half-mutated board, a board is
`Send + Sync` and trivially serialisable/cloneable, and wasm bindings have no lifetime puzzles
because nothing in `Board` borrows anything.

Where it costs: parent access needs views everywhere, positional handles are short-lived,
removal is O(n) per list, and three places where the C++ model calls *up* into other subsystems
(connectivity, DRC engine, project) need explicit hooks.

---------------------------------------------------------------------------------------------

## 1. Facts about the C++ model that drive the design

These were verified in this tree; several differ from released KiCad 9 and matter a lot here.

1. **Footprint children are stored in the footprint's library frame** (format 20260616,
   "Footprint affine transform"). `FOOTPRINT` holds a `TRANSFORM_TRS m_transform`
   (translate, rotate, scale x/y) and `m_flipped`. `PAD::GetPosition()` is
   `fp->GetTransform().Apply( m_libPos )`; `PAD::GetSize()`, `GetOffset()`, `GetPrimaryDrillSize()`,
   `PCB_TEXT::GetTextPos()/GetTextSize()/GetTextThickness()/GetTextAngle()`, `ZONE::GetBoardOutline()`,
   `PCB_BARCODE`, `PCB_DIMENSION_BASE` all derive board-frame values *on read* through
   `GetParentFootprint()`. `PCB_SHAPE` (hence `PCB_TEXTBOX`, `PCB_TABLECELL`) and `PCB_POINT` keep
   lib-frame members (`m_libStart`, ...) *plus* a baked board-frame copy refreshed by
   `OnFootprintTransformed()`. Consequence: a child without its parent cannot answer the most
   basic geometry question. Parent context is not optional.
2. **Children reach far up**: `IsLocked()` (parent footprint, parent group, board use),
   `GetLayerName()`, `GetMaxError()`, `BoardCopperLayerCount()`, text variable resolution
   (`PCB_TEXT::GetShownText` -> footprint -> board -> project), clearance overrides
   (`PAD::GetClearanceOverrides` -> footprint), net-tie groups, component class.
3. **The model calls up into other subsystems**: `PAD::FlashLayer` / `PCB_VIA::FlashLayer` call
   `board->GetConnectivity()->IsConnectedOnLayer(..)`; `BOARD_CONNECTED_ITEM::GetOwnClearance`,
   `PAD/PCB_TRACK/PCB_SHAPE::GetSolderMaskExpansion`, `PCB_TRACK::GetWidthConstraint` call
   `bds.m_DRCEngine->EvalRules(..)` / `GetCachedOwnClearance(..)`;
   `BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue` calls `m_DRCEngine->QueryWorstConstraint`.
   In Rust `kicad-pcb` sits *below* `kicad-connectivity` and `kicad-drc`, so these become trait
   hooks.
4. **Containers**: `m_tracks`, `m_footprints`, `m_drawings`, `FOOTPRINT::m_pads/m_drawings/m_fields`
   are `std::deque<T*>`; `ADD_MODE::INSERT` (the default, used by `BOARD_COMMIT::Push`) is
   `push_front`, the parser uses `BULK_APPEND` (`push_back`). `Remove` is `std::erase` (order
   preserving). No list is ever sorted in place or inserted into the middle.
5. **The writer sorts everything** before formatting (`BOARD_ITEM::ptr_cmp`, `BOARD::cmp_drawings`,
   `PCB_TRACK::cmp_tracks`, `FOOTPRINT::cmp_pads/cmp_drawings/cmp_zones`), so in-memory order does
   not influence saved bytes; it does influence iteration order of DRC, connectivity and the zone
   filler, so it must be preserved.
6. **Identity**: `m_Uuid`; `BOARD_ITEM_INDEX` is a `KIID -> BOARD_ITEM*` map behind
   `BOARD::ResolveItem`. Groups hold member *pointers* but are written as UUID lists; each item has
   a back pointer `m_group`. Constraints (`PCB_CONSTRAINT`) and net-chain terminal pads already
   reference by `KIID`.
7. **Nets**: items hold `NETINFO_ITEM*`. Since format 20251028 the file refers to nets by *name*;
   net codes are an in-memory detail. `NETINFO_LIST` iterates in net-name order.
8. **Derived state stored on items**: pad effective shapes/polygons, footprint bbox/hull/courtyard
   caches, zone fills + island flags + fill hashes, `m_zoneLayerOverrides` on pads/vias (written by
   the zone filler, read by `FlashLayer`, saved to the file), text render/bbox caches, zone bbox
   cache keyed by `BOARD::m_timeStamp`.
9. **DRC state stored on `BOARD`**: `m_CopperItemRTreeCache`, `m_DRCZones`, `m_IntersectsAreaCache`
   etc. are public members keyed by item pointers and cleared by `IncrementTimeStamp()`.
10. **New item types** in this tree: `PCB_POINT`, `PCB_BARCODE`, `PCB_3D_BODY`, `PCB_GRID_ITEM`,
    `PCB_DRILL_MAP`, `PCB_DRILL_CHART` / `PCB_STACKUP_TABLE` (both `PCB_GENERATED_TABLE : PCB_TABLE`),
    `PCB_CONSTRAINT`, `PCB_BOARD_OUTLINE`, generators `tuning_pattern`, `via_stack`, `via_stitch`;
    new data: variants, net chains, custom properties on every `EDA_ITEM`, backdrill / tertiary
    drill / post-machining in `PADSTACK`, copper thieving zone fill, ellipse shapes, line endings.
11. `GetEffectiveShape( layer, FLASHING, DRC_CONSTRAINT_T aUsage )` has a third parameter now, so
    the `DRC_CONSTRAINT_T` enum has to live in `kicad-pcb`.

---------------------------------------------------------------------------------------------

## 2. Ownership and identity architecture

### 2.1 Ownership

One owner per item, containers as in C++:

| C++ container | Rust | notes |
|---|---|---|
| `BOARD::m_footprints` (deque) | `ItemList<Footprint>` | footprints are large; list moves are still cheap enough (see 2.9) |
| `BOARD::m_tracks` (deque of `PCB_TRACK*`) | `ItemList<Track>` | enum, vias boxed so a segment stays small |
| `BOARD::m_zones` (vector) | `ItemList<Zone>` | |
| `BOARD::m_drawings` (deque of `BOARD_ITEM*`) | `ItemList<Drawing>` | enum over graphic item kinds |
| `BOARD::m_groups`, `m_generators`, `m_points`, `m_markers`, `m_constraints` | `ItemList<PcbGroup>` ... | |
| `FOOTPRINT::m_pads`, `m_fields`, `m_drawings`, `m_zones`, `m_groups`, `m_points`, `m_constraints`, `m_bodies` | `ItemList<..>` inside `Footprint` | |
| `PCB_TABLE::m_cells` | `Vec<PcbTablecell>` inside `PcbTable` | cells never move individually |
| `PADSTACK::COPPER_LAYER_PROPS::custom_shapes` (`shared_ptr<PCB_SHAPE>`) | `Vec<PcbShape>` | owned by the padstack |
| `NETINFO_LIST` | `NetinfoList` owning `NetinfoItem`s | |
| `shared_ptr<BOARD_DESIGN_SETTINGS>`, `shared_ptr<NET_SETTINGS>` | owned fields | sharing with `PROJECT` is replaced by explicit load/store functions |
| `shared_ptr<NETCLASS>` | `Arc<Netclass>` | immutable once built; the only `Arc` in the model besides shape caches and zone fills |
| `shared_ptr<CONNECTIVITY_DATA>` | not in `Board` | owned by the caller, handed in through `BoardCtx` |

No raw parent pointer survives: `EDA_ITEM::m_parent` is dropped from every struct.

### 2.2 `ItemList<T>`

A thin wrapper so that every list behaves the same and keeps the bookkeeping the identity scheme
needs. It is a `VecDeque` because the C++ default add mode is `push_front`.

```rust
/// Ordered owning container for board items. Port of the `std::deque<T*>` / `std::vector<T*>`
/// item containers of BOARD and FOOTPRINT (pcb_item_containers.h).
#[derive(Clone, Debug)]
pub struct ItemList<T> {
    items: VecDeque<T>,
    /// `seqs[i]` is the list-local sequence number of `items[i]`. Strictly increasing:
    /// `push_back` assigns `last + 1`, `push_front` assigns `first - 1`, removal keeps order.
    /// (C++ never sorts or mid-inserts these lists, see fact 4.)
    seqs: VecDeque<i32>,
    /// Bumped whenever the position of an existing element changes (push_front, remove, retain).
    shift_epoch: u64,
}

impl<T> ItemList<T> {
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;
    pub fn get(&self, pos: usize) -> Option<&T>;
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T>;        // C++ iteration order
    pub fn seq_at(&self, pos: usize) -> i32;
    pub fn pos_of_seq(&self, seq: i32) -> Option<usize>;                   // binary search
    pub fn shift_epoch(&self) -> u64;
    // mutation: crate-visible, used by Board / Footprint accessors only
    pub(crate) fn push_back(&mut self, item: T) -> usize;                  // ADD_MODE::APPEND
    pub(crate) fn push_front(&mut self, item: T) -> usize;                 // ADD_MODE::INSERT
    pub(crate) fn remove(&mut self, pos: usize) -> T;                      // std::erase, order kept
    pub(crate) fn remove_many(&mut self, sorted_pos: &[usize]) -> Vec<T>;  // one compaction pass
    pub(crate) fn get_mut(&mut self, pos: usize) -> Option<&mut T>;
    pub(crate) fn iter_mut(&mut self) -> std::collections::vec_deque::IterMut<'_, T>;
}
impl<T> std::ops::Index<usize> for ItemList<T> { type Output = T; /* panics like operator[] */ }
```

### 2.3 Identity: `Kiid`, `ItemHandle`, `ItemKey`

Three identities with distinct jobs. Implementers should memorise the table.

| | type | size | resolve | survives | use for |
|---|---|---|---|---|---|
| UUID | `Kiid` | 16 B | hash + binary search | everything, also save/load | files, groups, constraints, markers, JavaScript, undo records, anything long-lived |
| stable key | `ItemKey` | 16 B | binary search (O(log n)) | any mutation except removal of the item itself | connectivity graph nodes, router world items, wasm id table |
| positional handle | `ItemHandle` | 16 B | index (O(1)) | appends; dies on removal / front insert in a list it indexes | everything inside one analysis pass: R-tree payloads, hash/BTree keys, work lists |

```rust
/// Positional address of an item inside a Board. `Copy`, totally ordered (deterministic order
/// for `BTreeSet`/`BTreeMap` keys, replaces pointer-keyed std::set/std::map), hashable.
/// Valid for the `Board::structure_epoch()` it was obtained in.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemHandle {
    Footprint(u32),
    Pad { fp: u32, idx: u32 },
    FpField { fp: u32, idx: u32 },
    FpDrawing { fp: u32, idx: u32 },
    FpTablecell { fp: u32, idx: u32, cell: u32 },
    FpZone { fp: u32, idx: u32 },
    FpGroup { fp: u32, idx: u32 },
    FpPoint { fp: u32, idx: u32 },
    FpConstraint { fp: u32, idx: u32 },
    FpBody { fp: u32, idx: u32 },
    /// Custom-shape primitive of a pad (`PADSTACK::Primitives`), parent is the pad.
    PadPrimitive { fp: u32, pad: u32, layer: PcbLayerId, idx: u16 },
    Track(u32),
    Zone(u32),
    Drawing(u32),
    Tablecell { idx: u32, cell: u32 },
    Group(u32),
    Generator(u32),
    Point(u32),
    Marker(u32),
    Constraint(u32),
    /// A NETINFO_ITEM (it is a BOARD_ITEM in C++ and can be the target of ResolveItem).
    Net(i32),
    /// An item that is not stored in the board (DRC dummy shapes, zone-filler dummy pads,
    /// library footprints). The number is chosen by the creator so several dummies can be
    /// told apart in sets and maps.
    Detached(u32),
}

/// Stable address: the same shape as ItemHandle but with list sequence numbers instead of
/// positions. Obtained from and resolved by the board:
///   Board::key_of(ItemHandle) -> ItemKey          O(1)
///   Board::handle_of_key(ItemKey) -> Option<ItemHandle>   O(log n), None once the item is removed
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ItemKey { pub list: ListKind, pub parent_seq: i32, pub seq: i32, pub sub: u32 }

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum ListKind { Footprints, Pads, FpFields, FpDrawings, FpZones, FpGroups, FpPoints,
                    FpConstraints, FpBodies, Tracks, Zones, Drawings, Groups, Generators,
                    Points, Markers, Constraints, Nets }

/// Port of BOARD_ITEM_INDEX (board_item_index.h). `HashMap` is fine here: it is only ever
/// used for point lookups, never iterated (PORTING.md section 3).
#[derive(Clone, Debug, Default)]
pub(crate) struct UuidIndex { by_id: HashMap<Kiid, ItemKey> }
```

Why the sequence column: with a purely positional index every removal would have to rewrite the
index entry of every later element (or rebuild lazily under `&self`). Storing `ItemKey`s makes
`add` and `remove` O(1) on the index and makes `Board::handle_of(&Kiid)` a hash lookup plus one or
two binary searches over a dense `i32` column. It also gives consumers a compact stable id
without hashing UUIDs.

Epoch discipline (enforced with `debug_assert!`, mirrors "do not keep `BOARD_ITEM*` across a
delete"):

```rust
impl Board {
    /// Changes whenever any positional handle may have become invalid.
    pub fn structure_epoch(&self) -> u64;
    /// BOARD::GetTimeStamp(): bumped by every mutation that can change derived data.
    pub fn time_stamp(&self) -> i32;
}
```

A consumer that stores `ItemHandle`s (a DRC R-tree, a work list) stores the epoch next to the
container and asserts on use; consumers that live across edits store `ItemKey` or `Kiid`.

Duplicate UUIDs are tolerated exactly like `BOARD_ITEM_INDEX` (the later bind wins in the forward
map); `Board::repair_duplicate_item_uuids()` ports `BOARD::RepairDuplicateItemUuids`.

### 2.4 Parent / child and cross relations

| relation (C++) | Rust representation | how a consumer gets it |
|---|---|---|
| pad -> footprint (`GetParentFootprint()`) | ownership; `View::parent = ParentRef::Footprint(&fp)` | `pad.parent_footprint()` |
| footprint text/shape/zone/point/... -> footprint | same | same |
| table cell -> table (`GetParent()`) -> footprint/board | `ParentRef::Table { table, fp }` | `cell.parent_table()`, `cell.parent_footprint()` |
| pad primitive -> pad | `ParentRef::Pad { pad, fp }` | `prim.parent_pad()` |
| anything -> board (`GetBoard()`) | `View::cx: Option<&BoardCtx>` | `item.board()` |
| item -> group (`m_group`) | `ItemBase::group: Option<Kiid>` | `item.parent_group()` (UUID lookup) |
| group -> members (`EDA_GROUP::m_items`, pointers) | `PcbGroup::items: BTreeSet<Kiid>` | `group.members()` yields `ItemRef`s |
| generator -> members | same (generator embeds a `PcbGroup`) | |
| item -> net (`m_netinfo`) | `ConnectedBase::net_code: i32` | `item.net()`, `item.net_code()` |
| net -> netclass (`shared_ptr<NETCLASS>`) | `NetinfoItem::netclass: Option<Arc<Netclass>>` | `item.effective_netclass()` |
| net -> items | not stored in the model (as in C++); connectivity provides it | `cx.items_in_net(code)` linear, or connectivity |
| net -> terminal pads (`m_terminalPads[2]`) | `terminal_pad_uuids: [Kiid; 2]` only | `cx.item_by_uuid` |
| constraint -> items | `ConstraintMember::item: Kiid` (already so in C++) | |
| footprint -> component class | name + `Arc<ComponentClass>` cache | `fp.component_class()` |
| marker -> items (`RC_ITEM::m_ids`) | `Vec<Kiid>` (already so in C++) | |
| zone-filler/DRC `ZONE*` keyed maps on BOARD | consumer-side maps keyed by `ItemHandle` | |

The two back references kept as data (`group`, `net_code`) are plain values, never borrows.
`Board::groups_sanity_check()` ports `BOARD::GroupsSanityCheck` and additionally verifies that
`ItemBase::group` and `PcbGroup::items` agree; `Board::add_to_group / remove_from_group` are the
only functions that touch both sides.

### 2.5 Polymorphism: `Item`, `ItemRef<'a>`, `View<'a, T>`, `BoardCtx<'a>`

There are three enum families and one generic view struct.

```rust
/// Owned item, for moving things in and out of a board (Add/Remove/commit/undo/clipboard).
pub enum Item {
    Footprint(Box<Footprint>), Pad(Box<Pad>), Field(Box<PcbField>), Track(Track),
    Zone(Box<Zone>), Drawing(Drawing), Tablecell(Box<PcbTablecell>), Group(PcbGroup),
    Generator(Box<PcbGenerator>), Point(PcbPoint), Marker(Box<PcbMarker>),
    Constraint(PcbConstraint), Body(Box<Pcb3dBody>), Net(Box<NetinfoItem>),
}

/// Everything a view needs to answer "who is my parent" (EDA_ITEM::m_parent chain).
#[derive(Copy, Clone)]
pub enum ParentRef<'a> {
    None,                                                   // detached item
    Board,                                                  // direct child of the board
    Footprint(&'a Footprint),
    Table { table: &'a PcbTable, fp: Option<&'a Footprint> },
    Pad { pad: &'a Pad, fp: Option<&'a Footprint> },
}

/// A borrowed item together with its context. `Copy`; this is what replaces `T*`.
/// Derefs to `T`, so plain data (`pad.number`, `via.via_type`, `zone.priority`) is read
/// directly; everything that needs the parent or the board is a method on the view.
#[derive(Copy, Clone)]
pub struct View<'a, T> {
    pub cx: Option<&'a BoardCtx<'a>>,       // None only for items outside any board
    pub parent: ParentRef<'a>,
    pub handle: ItemHandle,
    pub item: &'a T,
}
impl<'a, T> std::ops::Deref for View<'a, T> { type Target = T; }

pub type FootprintRef<'a> = View<'a, Footprint>;
pub type PadRef<'a>       = View<'a, Pad>;
pub type ViaRef<'a>       = View<'a, PcbVia>;
pub type ZoneRef<'a>      = View<'a, Zone>;
pub type ShapeRef<'a>     = View<'a, PcbShape>;
// ... one alias per concrete struct

/// The Rust spelling of `BOARD_ITEM*` / `const BOARD_ITEM*`.
#[derive(Copy, Clone)]
pub enum ItemRef<'a> {
    Footprint(View<'a, Footprint>),
    Pad(View<'a, Pad>),
    Track(View<'a, PcbTrack>),            // PCB_TRACE_T
    Arc(View<'a, PcbArc>),                // PCB_ARC_T
    Via(View<'a, PcbVia>),                // PCB_VIA_T
    Zone(View<'a, Zone>),
    Shape(View<'a, PcbShape>),
    Text(View<'a, PcbText>),
    Field(View<'a, PcbField>),
    Textbox(View<'a, PcbTextbox>),
    Table(View<'a, PcbTable>),            // PCB_TABLE_T, PCB_DRILL_CHART_T, PCB_STACKUP_TABLE_T
    Tablecell(View<'a, PcbTablecell>),
    Dimension(View<'a, PcbDimension>),    // PCB_DIM_*_T
    Barcode(View<'a, PcbBarcode>),
    ReferenceImage(View<'a, PcbReferenceImage>),
    Target(View<'a, PcbTarget>),
    Point(View<'a, PcbPoint>),
    Group(View<'a, PcbGroup>),
    Generator(View<'a, PcbGenerator>),
    Marker(View<'a, PcbMarker>),
    Body(View<'a, Pcb3dBody>),
    GridItem(View<'a, PcbGridItem>),
    DrillMap(View<'a, PcbDrillMap>),
    Constraint(View<'a, PcbConstraint>),
    Netinfo(View<'a, NetinfoItem>),
    Opaque(View<'a, OpaqueItem>),
}

/// Subset enums with `From`/`TryFrom` conversions to and from ItemRef; they implement the same
/// traits, so code that iterates `board->Tracks()` keeps a precise type.
#[derive(Copy, Clone)] pub enum TrackRef<'a>     { Track(View<'a, PcbTrack>), Arc(View<'a, PcbArc>), Via(View<'a, PcbVia>) }
#[derive(Copy, Clone)] pub enum DrawingRef<'a>   { Shape(..), Text(..), Textbox(..), Table(..), Dimension(..), Barcode(..), ReferenceImage(..), Target(..), DrillMap(..), GridItem(..), Opaque(..) }
#[derive(Copy, Clone)] pub enum ConnectedRef<'a> { Pad(..), Track(..), Arc(..), Via(..), Zone(..), Shape(..) }   // BOARD_CONNECTED_ITEM*
#[derive(Copy, Clone)] pub enum GroupRef<'a>     { Group(View<'a, PcbGroup>), Generator(View<'a, PcbGenerator>) } // EDA_GROUP*
```

`ItemRef` implements `PartialEq, Eq, Hash, PartialOrd, Ord` **by handle** (two views of the same
board are the same item iff their handles are equal). This is the replacement for pointer
comparison (`a == b`, `aItem->parent == aRefItem`, `std::unordered_set<BOARD_ITEM*>`) and it is
deterministic, which pointer order in C++ is not.

The context:

```rust
/// What C++ reaches through GetBoard(): the board itself and the two subsystems the model
/// calls up into. Built by whoever runs an analysis; cheap (three words + two fat pointers).
#[derive(Copy, Clone)]
pub struct BoardCtx<'a> {
    pub board: &'a Board,
    pub connectivity: Option<&'a dyn ConnectivityQuery>,     // CONNECTIVITY_DATA facade
    pub rules: Option<&'a dyn RuleResolver>,                 // DRC_ENGINE facade
}

impl<'a> BoardCtx<'a> {
    /// No connectivity, no rule engine. `flash_layer()` then behaves like C++ with an empty
    /// CONNECTIVITY_DATA (nothing is connected), `own_clearance()` returns 0 like C++ with
    /// `m_DRCEngine == nullptr`.
    pub fn bare(board: &'a Board) -> Self;
    pub fn with_connectivity(self, c: &'a dyn ConnectivityQuery) -> Self;
    pub fn with_rules(self, r: &'a dyn RuleResolver) -> Self;

    pub fn item(&'a self, h: ItemHandle) -> ItemRef<'a>;                 // panics on a stale handle
    pub fn try_item(&'a self, h: ItemHandle) -> Option<ItemRef<'a>>;
    pub fn item_by_uuid(&'a self, id: &Kiid) -> Option<ItemRef<'a>>;     // BOARD::ResolveItem
    pub fn item_by_key(&'a self, k: ItemKey) -> Option<ItemRef<'a>>;
    pub fn footprint(&'a self, idx: usize) -> FootprintRef<'a>;
    pub fn track(&'a self, idx: usize) -> TrackRef<'a>;
    pub fn zone(&'a self, idx: usize) -> ZoneRef<'a>;
    pub fn footprints(&'a self) -> impl ExactSizeIterator<Item = FootprintRef<'a>> + 'a;
    pub fn tracks(&'a self) -> impl ExactSizeIterator<Item = TrackRef<'a>> + 'a;
    pub fn zones(&'a self) -> impl ExactSizeIterator<Item = ZoneRef<'a>> + 'a;
    pub fn drawings(&'a self) -> impl ExactSizeIterator<Item = DrawingRef<'a>> + 'a;
    pub fn groups(&'a self) -> ..; pub fn generators(&'a self) -> ..;
    pub fn points(&'a self) -> ..; pub fn markers(&'a self) -> ..; pub fn constraints(&'a self) -> ..;
    /// A view of an item that is not in the board (dummy pad of the zone filler, DRC helper
    /// shapes). `parent` lets a dummy pad borrow a real footprint's transform.
    pub fn detached<T>(&'a self, item: &'a T, parent: ParentRef<'a>, id: u32) -> View<'a, T>;
}
impl<'a> View<'a, Footprint> {
    pub fn pads(self) -> impl ExactSizeIterator<Item = PadRef<'a>> + 'a;
    pub fn fields(self) -> ..; pub fn graphical_items(self) -> impl Iterator<Item = DrawingRef<'a>>;
    pub fn zones(self) -> ..; pub fn groups(self) -> ..; pub fn points(self) -> ..;
    pub fn bodies(self) -> ..; pub fn constraints(self) -> ..;
}
```

Only `BoardCtx` creates views. `Board` itself exposes the raw lists (`board.tracks()` returns
`&ItemList<Track>`), which give lib-frame/own data only. This is deliberate: there is exactly one
way to obtain an item with board-frame behaviour, and that way forces the caller to decide which
connectivity and rule engine the answer depends on.

Typical translations:

| C++ | Rust |
|---|---|
| `BOARD_ITEM* item` | `item: ItemRef<'a>` |
| `item->Type() == PCB_PAD_T` then `static_cast<PAD*>( item )` | `if let ItemRef::Pad(pad) = item` / `item.as_pad()` |
| `dynamic_cast<BOARD_CONNECTED_ITEM*>( item )` | `item.as_connected()` -> `Option<ConnectedRef<'a>>` |
| `dynamic_cast<PCB_TRACK*>( item )` (track, arc or via) | `item.as_track()` -> `Option<TrackRef<'a>>` |
| `BaseType( item->Type() ) == PCB_DIMENSION_T` | `matches!(item, ItemRef::Dimension(_))` |
| `pad->GetParentFootprint()->GetReference()` | `pad.parent_footprint().map(|fp| fp.reference())` |
| `other->GetParent() == pad->GetParent()` | `other.parent_handle() == pad.parent_handle()` |
| `item->GetBoard()->GetDesignSettings()` | `item.board().map(|b| &b.design_settings)` |
| `a->m_Uuid > b->m_Uuid` | `a.uuid() > b.uuid()` |
| `std::unordered_map<BOARD_ITEM*, bool>` | `HashMap<ItemHandle, bool>` (lookup only) or `BTreeMap<ItemHandle, bool>` |
| `for( PCB_TRACK* t : board->Tracks() )` | `for t in cx.tracks()` |
| `PAD dummy( nullptr ); ...` | `let dummy = Pad::new(); let v = cx.detached(&dummy, ParentRef::None, 0);` |

Name resolution rule (relied on throughout): a method defined on `View<'a, T>` (inherent or
through `BoardItem`) is found *before* auto-deref reaches a same-named method of the raw struct.
So `shape_view.bounding_box()` is the context-aware one and `footprint_view.pads()` yields
`PadRef`s, while `footprint_view.item.pads()` is the raw `&ItemList<Pad>`. View methods shadow
raw methods of the same name on purpose; where both exist they must return the same value for
an item without a parent.

Cost of a view: 56 bytes (`Option<&BoardCtx>` 8, `ParentRef` 24, `ItemHandle` 16, `&T` 8), built
by two or three bounds-checked index operations. Views are transient values; nothing stores
them beyond one borrow of the board.

### 2.6 Mutation

All mutation goes through `&mut Board`. While any `BoardCtx`/view/R-tree payload borrowed from the
board is alive the board cannot be mutated - the borrow checker makes "an analysis sees a
half-edited board" unrepresentable. Three levels:

**(a) In-place edit of one item.**

```rust
/// Copy of the few board facts setters need (GetBoard()->GetCopperLayerCount() etc.).
#[derive(Copy, Clone, Debug)]
pub struct BoardEnv { pub copper_layer_count: i32, pub enabled_layers: Lset, pub max_error: i32,
                      pub board_use: BoardUse }

/// Mutable counterpart of View. Holds a *copy* of the parent transform because the parent
/// cannot be borrowed while its child is borrowed mutably.
pub struct ViewMut<'a, T> {
    pub item: &'a mut T,
    pub xf: Option<TransformTrs>,           // parent footprint transform, None on board level
    pub fp_layer: Option<PcbLayerId>,       // parent footprint side (for IsFlipped())
    pub env: BoardEnv,
}
impl<'a, T> std::ops::Deref for ViewMut<'a, T> { type Target = T; }
impl<'a, T> std::ops::DerefMut for ViewMut<'a, T> {}

pub enum ItemMut<'a> { Footprint(FootprintMut<'a>), Pad(ViewMut<'a, Pad>), Track(ViewMut<'a, PcbTrack>),
                       Arc(..), Via(..), Zone(..), Shape(..), Text(..), /* one per ItemRef variant */ }
pub type FootprintMut<'a> = ViewMut<'a, Footprint>;
pub enum TrackMut<'a>   { Track(ViewMut<'a, PcbTrack>), Arc(ViewMut<'a, PcbArc>), Via(ViewMut<'a, PcbVia>) }
pub enum DrawingMut<'a> { Shape(ViewMut<'a, PcbShape>), Text(ViewMut<'a, PcbText>), /* one per Drawing variant */ }

impl Board {
    /// Mutable access to one item. Before handing out the reference this
    ///  - bumps the time stamp (BOARD::IncrementTimeStamp),
    ///  - clears the item's own lazy caches,
    ///  - clears the geometry/courtyard caches of the parent footprint.
    /// Because nothing can read the board while the returned borrow lives, invalidating on
    /// *acquisition* is sufficient; no Drop guard is needed.
    pub fn edit(&mut self, h: ItemHandle) -> Option<ItemMut<'_>>;
    pub fn edit_by_uuid(&mut self, id: &Kiid) -> Option<ItemMut<'_>>;
    pub fn footprint_mut(&mut self, idx: usize) -> FootprintMut<'_>;
    pub fn track_mut(&mut self, idx: usize) -> TrackMut<'_>;
    pub fn zone_mut(&mut self, idx: usize) -> ViewMut<'_, Zone>;
    /// Derived-state writers that must NOT invalidate geometry caches or bump the time stamp
    /// (zone filler results, fill flags). See 2.7.
    pub fn zone_fill_mut(&mut self, idx: usize) -> &mut ZoneFill;
    pub fn set_zone_layer_override(&mut self, h: ItemHandle, layer: PcbLayerId, v: ZoneLayerOverride);
    pub fn clear_zone_layer_overrides(&mut self, h: ItemHandle);
}
impl Footprint {
    pub fn pad_mut(&mut self, idx: usize, env: BoardEnv) -> ViewMut<'_, Pad>;   // same invalidation rule
    pub fn drawing_mut(&mut self, idx: usize, env: BoardEnv) -> DrawingMut<'_>;
    pub fn field_mut(&mut self, idx: usize, env: BoardEnv) -> ViewMut<'_, PcbField>;
    // set_position / set_orientation / flip / rotate / move_ / set_transform_scale propagate to
    // the children exactly like FOOTPRINT::SetPosition .. SetTransformScale (OnFootprintTransformed)
}
```

Board-frame setters are methods on `ViewMut` and are line-for-line ports of the C++ setters:

```rust
impl ViewMut<'_, Pad> {
    /// Port of `PAD::SetPosition`.
    pub fn set_position(&mut self, pos: Vector2I) {
        self.item.lib_pos = match &self.xf { Some(xf) => xf.inverse_apply(pos), None => pos };
        self.item.set_dirty();
    }
}
```

Setters that only touch the item's own data (`PCB_TRACK::SetWidth`, `PCB_VIA::SetLayerPair`,
`PADSTACK::SetSize`) are methods on the raw struct.

**(b) Structural edits.**

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AddMode { Insert, Append, BulkAppend, BulkInsert }      // ADD_MODE
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RemoveMode { Normal, Bulk }                             // REMOVE_MODE

impl Board {
    /// Port of BOARD::Add (list selection, copper-layer check for tracks, index bind incl.
    /// children, ClearEditFlags). Does not touch connectivity (that is the caller's ChangeSet).
    pub fn add(&mut self, item: Item, mode: AddMode) -> Result<ItemHandle, Item>;
    /// Port of FOOTPRINT::Add for a footprint that lives in this board.
    pub fn add_to_footprint(&mut self, fp: usize, item: Item, mode: AddMode) -> Result<ItemHandle, Item>;
    /// Port of BOARD::Remove / FOOTPRINT::Remove: detaches from its group, unbinds the index
    /// (and children), bumps time stamp and structure epoch, returns the item (undo owns it).
    pub fn remove(&mut self, h: ItemHandle) -> Option<Item>;
    /// Batch form: one compaction per list, one epoch bump.
    pub fn remove_many(&mut self, hs: &[ItemHandle]) -> Vec<(Kiid, Item)>;
    pub fn remove_all(&mut self, types: &[KicadT]);                     // BOARD::RemoveAll
    pub fn rebind_item_uuid(&mut self, h: ItemHandle, new_id: Kiid);    // BOARD_ITEM::SetUuid
}
```

**(c) Commits** (router results, API clients, undo/redo). `BoardCommit` is the data-only
equivalent of `BOARD_COMMIT` (staging + `Push`); `ChangeSet` is what `BOARD_LISTENER` callbacks
and `CONNECTIVITY_DATA::Add/Remove/Update` receive in C++.

```rust
#[derive(Default)]
pub struct BoardCommit { staged: Vec<Staged> }
pub enum Staged {
    Add { item: Item, parent_footprint: Option<Kiid>, mode: AddMode },   // COMMIT::Add
    Remove { id: Kiid },                                                 // COMMIT::Remove
    Modify { id: Kiid, new_value: Item },                                // COMMIT::Modify + edited copy
    ModifyWith { id: Kiid, f: Box<dyn FnOnce(ItemMut<'_>)> },            // edit in place
}
impl BoardCommit {
    pub fn add(&mut self, item: Item) -> Kiid;
    pub fn add_to_footprint(&mut self, fp: Kiid, item: Item) -> Kiid;
    pub fn remove(&mut self, id: Kiid);
    pub fn modify(&mut self, id: Kiid, new_value: Item);
    pub fn modify_with(&mut self, id: Kiid, f: impl FnOnce(ItemMut<'_>) + 'static);
}

pub struct ChangeSet {
    pub added: Vec<Kiid>,
    pub removed: Vec<(Kiid, Item)>,          // the removed items (their geometry is still needed
                                             // by listeners; undo re-adds them)
    pub modified: Vec<(Kiid, Item)>,         // before-images
    pub epoch_before: u64, pub epoch_after: u64,
    pub time_stamp_before: i32, pub time_stamp_after: i32,
}
impl Board {
    /// Port of the item bookkeeping half of BOARD_COMMIT::Push: removes first, then modifies,
    /// then adds (ADD_MODE::INSERT unless staged otherwise), one epoch bump.
    pub fn apply(&mut self, commit: BoardCommit) -> ChangeSet;
    /// Inverse operation; returns the ChangeSet of the revert (redo = revert of the revert).
    pub fn revert(&mut self, changes: ChangeSet) -> ChangeSet;
}
```

Incremental consumers implement

```rust
pub trait BoardListener {                 // BOARD_LISTENER, data-only
    fn on_board_changed(&mut self, cx: &BoardCtx<'_>, changes: &ChangeSet);
}
```

and are driven by the session object that owns board + consumers (section 5.9). The model does
not store listeners (C++ `m_listeners` holds raw pointers into GUI objects).

Flags used as scratch space by C++ algorithms (`SKIP_STRUCT`, `CANDIDATE`, `IS_LINKED`,
`m_arflag`, `PAD::m_subRatsnest`) cannot be written while the board is borrowed for reading.
Ports of such algorithms use side tables keyed by `ItemHandle` (`BTreeSet<ItemHandle>` or a
`Vec<bool>` indexed by track position). The fields still exist for fidelity of non-algorithmic
code.

### 2.7 Derived data and caches

Three classes of derived data, three rules.

**Class 1 - derived from the item itself and its parent** (C++ `mutable` members guarded by a
dirty flag and a mutex). Stored inside the item in a `LazyCache`:

```rust
/// Interior-mutable, self-validating cache slot: the port of
/// `mutable std::unique_ptr<X> m_cache; mutable bool m_dirty; mutable std::mutex m_mutex;`.
/// `std::sync::Mutex` works on wasm32-unknown-unknown (it never blocks there).
pub struct LazyCache<K, V> { slot: std::sync::Mutex<Option<(K, std::sync::Arc<V>)>> }

impl<K: PartialEq + Clone, V> LazyCache<K, V> {
    pub const fn new() -> Self;
    /// Returns the cached value if it was built for `key`, otherwise builds it. `build` runs
    /// outside the lock (the C++ builders re-enter the item through the rule engine).
    pub fn get_or_build(&self, key: &K, build: impl FnOnce() -> V) -> std::sync::Arc<V>;
    pub fn invalidate(&self);          // callable through `&self` (C++ const invalidation)
    pub fn clear(&mut self);           // from `&mut` setters, no locking
    /// Transform a present value in place and store it under a new key (C++ code that patches
    /// a cache instead of dropping it, e.g. FOOTPRINT::Flip mirroring the courtyard cache).
    pub fn update(&mut self, new_key: K, f: impl FnOnce(&mut V)) where V: Clone;
}
impl<K, V> Clone for LazyCache<K, V> { /* a clone starts empty, like the C++ copy ctors */ }
impl<K, V> PartialEq for LazyCache<K, V> { /* always equal: caches are not part of the value */ }
```

The key holds every *external* input of the cached value, so a stale read is impossible even if
an invalidation hook is forgotten:

| cache (C++) | Rust field | key | cleared by |
|---|---|---|---|
| `PAD::m_drawCache`, `m_shapesDirty`, `m_polyDirty[2]`, `m_effectiveBoundingRadius` | `Pad::draw_cache: LazyCache<PadCacheKey, PadDrawCacheData>` | `(parent TransformTrs, max_error)` | `Pad::set_dirty()`, any `pad_mut()` |
| `PAD::m_maskShapes` | `Pad::mask_shapes: LazyCache<PadCacheKey, BTreeMap<PcbLayerId,(i32, Arc<ShapeCompound>)>>` + per-layer expansion check | same | same |
| `FOOTPRINT::m_geometry_cache` (bbox, text-excluded bbox, hull + 3 timestamps) | `Footprint::bbox_cache`, `text_excluded_bbox_cache`, `hull_cache`: one `LazyCache<i32, _>` each | board `time_stamp` | `invalidate_geometry_caches()`, any child `*_mut()` |
| `FOOTPRINT::m_courtyard_cache` (+ mutex, lazily rebuilt in the const `GetCourtyard`) | `Footprint::courtyard_cache: LazyCache<CourtyardCacheKey, FootprintCourtyardCacheData>` | parent transform, `max_error`, chaining epsilon | any child `*_mut()`; `build_courtyard_caches()` refills it with error reporting; `flip` swaps/mirrors the cached polygons in place and re-keys the slot through `LazyCache::update(&mut self, new_key, f)`, exactly as C++ does, so no rebuild-vs-mirror rounding difference can appear |
| `FOOTPRINT::m_netTieCache` | `Footprint::net_tie_cache: BTreeMap<Kiid, BTreeSet<i32>>` | - | `build_net_tie_cache()` (explicit, called by DRC like C++) |
| `ZONE::m_bboxCache`, `BOARD::m_ZoneBBoxCache` | `Zone::bbox_cache: LazyCache<(i32, Option<TransformTrs>), Box2I>` | board `time_stamp`, parent transform | automatic |
| `ZONE::m_borderHatchLines` | plain field, rebuilt by `hatch_border()` | | |
| `EDA_TEXT::m_shown_text`, `m_text_var_refs` | plain fields, rebuilt eagerly by `set_text()` (as C++ `cacheShownText`) | | |
| `EDA_TEXT::m_bbox_cache`, `m_render_cache` | `EdaText::bbox_cache: LazyCache<..>`, `render_cache` | text pos/attributes hash | `clear_bounding_box_cache()`, `clear_render_cache()` |
| `PCB_TEXT::m_knockout_cache` | `PcbText::knockout_cache: LazyCache<KnockoutKey, ShapePolySet>` | resolved text, attributes, pos, max_error | automatic |
| `EDA_SHAPE::m_hatchingCache`, `m_hatchingDirty` | `EdaShape::hatching_cache: LazyCache<(), EdaShapeHatchCacheData>` | - | `set_hatching_dirty()` |
| `PCB_BARCODE::m_cache` | `PcbBarcode::cache: LazyCache<u64, PcbBarcodeCache>` | `compute_cache_key()` | automatic |
| `PCB_DIMENSION_BASE::m_shapes` | plain field, recomputed by `update()` (as C++) | | |
| `PCB_SHAPE` baked board coords, `PCB_POINT::m_pos` | plain fields, rebaked by `on_footprint_transformed(&TransformTrs)` | | `Footprint` transform setters |
| `BOARD::m_maxClearanceValue` | `Board::max_clearance_value: LazyCache<i32, i32>` | `time_stamp` | automatic |
| `SHAPE_POLY_SET` triangulation caches | inside `kicad-geom` | | |

**Class 2 - results written by another subsystem and stored on the item** because the file
format stores them: zone fills, island flags, fill hashes, `need_refill`, `is_filled`;
`zone_layer_overrides` on pads and vias. They are ordinary fields grouped in a sub-struct
(`Zone::fill: ZoneFill`) and written through dedicated accessors (`zone_fill_mut`,
`set_zone_layer_override`) that do not invalidate class-1 caches and do not bump the time stamp
(C++ does not either).

**Class 3 - everything that spans items**: connectivity graph, ratsnest, DRC R-trees
(`m_CopperItemRTreeCache`, `m_CopperZoneRTreeCache`), `m_DRCZones`, `m_DRCCopperZonesByLayer`,
`m_DRCMaxClearance`, the `m_Intersects*Cache` / `m_EnclosedByAreaCache` / `m_ItemFieldCache` /
`m_LayerExpressionCache` families, `m_ZoneIsolatedIslandsMap`, `m_StackedMicroviaCache`,
`m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`, `m_footprintCourtyardIndex`, the DRC
own-clearance cache, the drill symbol cache. **None of these live in `Board`.** They live in the
consumer that builds them (`kicad_connectivity::ConnectivityData`, `kicad_drc::DrcCache`), keyed
by `ItemHandle`, stamped with `(time_stamp, structure_epoch)` and dropped when the stamp differs
(the port of the big `if` in `BOARD::IncrementTimeStamp`).

### 2.8 Upward hooks

```rust
/// What the model needs from CONNECTIVITY_DATA. Implemented in kicad-connectivity.
pub trait ConnectivityQuery: Send + Sync {
    /// CONNECTIVITY_DATA::IsConnectedOnLayer( aItem, aLayer, aTypes ).
    fn is_connected_on_layer(&self, cx: &BoardCtx<'_>, item: ItemHandle, layer: PcbLayerId,
                             types: &[KicadT]) -> bool;
    /// CONNECTIVITY_DATA::GetNodeCount / GetPadCount (BOARD::GetNodesCount, statistics).
    fn node_count(&self, net: i32) -> u32;
    fn pad_count(&self, net: i32) -> u32;
    /// CONNECTIVITY_DATA::GetConnectedItems (teardrops, net ties, length calculation).
    fn connected_items(&self, cx: &BoardCtx<'_>, item: ItemHandle, types: &[KicadT]) -> Vec<ItemHandle>;
}

/// Result of a rule evaluation as far as the model cares (DRC_CONSTRAINT::m_Value + name).
#[derive(Clone, Debug, Default)]
pub struct RuleValue { pub value: MinOptMax<i32>, pub source: Option<String> }

/// What the model needs from DRC_ENGINE. Implemented in kicad-drc.
pub trait RuleResolver: Send + Sync {
    fn has_rules_for_constraint_type(&self, t: DrcConstraintT) -> bool;
    /// DRC_ENGINE::EvalRules( aConstraintType, a, b, aLayer ).
    fn eval_rules(&self, t: DrcConstraintT, a: ItemRef<'_>, b: Option<ItemRef<'_>>,
                  layer: PcbLayerId) -> RuleValue;
    /// DRC_ENGINE::GetCachedOwnClearance( aItem, aLayer, aSource ).
    fn cached_own_clearance(&self, item: ItemRef<'_>, layer: PcbLayerId) -> (i32, Option<String>);
    /// DRC_ENGINE::QueryWorstConstraint.
    fn query_worst_constraint(&self, t: DrcConstraintT) -> Option<MinOptMax<i32>>;
    /// DRC_ENGINE::InvalidateClearanceCache (called from set_net_code / set_layer in C++).
    fn invalidate_clearance_cache(&self, id: &Kiid);
}
```

`DrcConstraintT` (port of `pcbnew/drc/drc_constraint_types.h`), `MinOptMax<T>`
(`include/core/minoptmax.h`), `Flashing` and `ZoneLayerOverride` are defined in `kicad-pcb` (or
`kicad-core`) and re-exported by `kicad-drc`.

### 2.9 Where this approach hurts, and the mitigation for each

| # | pain | concrete cost | mitigation |
|---|---|---|---|
| 1 | **Parent access from a child.** A `&Pad` cannot compute its board position, size, drill, lock state, clearance override, description. | Every API that C++ writes as `const PAD*` takes `PadRef<'a>` (56 bytes, `Copy`). Raw structs expose lib-frame data only; two "levels" of each type exist (struct and view). | The raw struct has **no** `position()`/`size()`/`bounding_box()` at all, so using the wrong frame does not compile; lib-frame fields are literally named `lib_*`. `View` derefs to the struct so plain fields need no boilerplate. Views are only built by `BoardCtx`, in one file. |
| 2 | **Mutable access plus parent data.** While a pad is `&mut`, its footprint is not readable. | `ViewMut` copies the parent transform (40 bytes) and a `BoardEnv`; setters that need more parent state (net-tie groups, reference) must be `Footprint`/`Board` methods. | Copying is what the C++ does logically; setters needing more than transform + env are rare (they surface in M8a and M11a) and become methods one level up, using disjoint field borrows (`let Footprint { transform, pads, .. } = self`). |
| 3 | **Positional handles die on removal / front insertion.** | Any structure that holds `ItemHandle`s must be rebuilt or remapped after `remove`, `push_front`. | (a) Handles are meant for one analysis pass, during which the board is immutably borrowed anyway. (b) `ItemKey` survives every mutation and resolves in O(log n) with no index maintenance. (c) `Kiid` for everything external. (d) `structure_epoch` + `debug_assert!` catch misuse in tests. (e) API clients are told to add with `AddMode::Append`, which invalidates nothing. |
| 4 | **Removal is O(n) per list** (`VecDeque::remove` memmove; `Track` is ~170 bytes inline). | ~1-2 ms to remove one track from a 100k-track board; ruinous if a router removes 10k tracks one call at a time. | `remove_many` / `apply(commit)` compact each list once (one pass, like `std::erase_if`). If profiling of the PNS port demands it, `ItemList<Track>` can switch to `VecDeque<Box<Track>>` (C++ layout, 8-byte moves) without touching any consumer, because consumers only see the `ItemList` API. |
| 5 | **Enum size.** `Track` is as big as its largest variant; `Drawing` likewise. | Without care a via (padstack, ~500 bytes) would bloat every segment. | Large variants are boxed: `Track::Via(Box<PcbVia>)`, `Drawing::Table/Dimension/Barcode/ReferenceImage(Box<..>)`. `PcbShape` stays inline (it dominates `Drawing`). |
| 6 | **R-tree payloads / long-lived item sets.** C++ stores `BOARD_ITEM*`. | Payload cannot be `&T` across mutations; a payload of `ItemRef` is 64 bytes. | Analysis-time trees store `ItemHandle` (16 bytes) and resolve through `cx.item(h)` (two bounds-checked indexings) when the visitor needs the item. Persistent trees (connectivity, router) store their own dense node index; the node stores `ItemKey`. |
| 7 | **Router / commit style edits** want "modify this item in place, tell everyone". | No listener pointers inside the board; someone must route the `ChangeSet`. | `Board::apply -> ChangeSet`, `BoardListener`, and a `Session` owner above the model (5.9). `Board: Clone` makes "try a change, discard it" (PNS branch semantics, DRC what-if) a clone away for small boards and `revert(ChangeSet)` for big ones. |
| 8 | **Model -> connectivity / DRC engine calls.** | `effective_shape(layer, Flashing::Default)` silently depends on which connectivity is in the context. | Explicit `BoardCtx`; `BoardCtx::bare` documents the C++-equivalent degraded behaviour; the DRC/zone/plot entry points take a full context by signature. |
| 9 | **Nets by code instead of pointer.** | `SanitizeNetcodes` / `MapNets` / net removal must rewrite `net_code` in every connected item (C++ gets this for free). | One helper `Board::remap_net_codes(&BTreeMap<i32, i32>)` walking `all_connected_items_mut()`; net lookup by code is an O(1) vector index. |
| 10 | **Groups by UUID.** `IsLocked()` needs the parent group. | One hash lookup + binary search per `is_locked()` on a grouped item. | Only grouped items pay; ungrouped items test `group.is_none()`. |
| 11 | **Scratch flags on items** (`SKIP_STRUCT`, `m_arflag`, `m_subRatsnest`) cannot be written under `&Board`. | Algorithms that mark items must be restructured around side tables. | Side tables keyed by position/handle are deterministic and usually faster; the affected C++ algorithms are few (tracks cleaner, teardrops, legacy ratsnest). |
| 12 | **Two simultaneous `&mut` into the same list** (swap data of two tracks, footprint exchange). | Needs `split_at_mut`-style code or take/put-back. | `ItemList::get_two_mut(a, b)`; `std::mem::swap` on whole items covers `SwapItemData`. |
| 13 | **Interior mutability is still present** (the `LazyCache` mutexes). | Not "pure" plain data; `Board` equality/clone semantics need hand-written impls for cache fields. | Confined to one generic type with trivial `Clone`/`PartialEq`; the alternative (external cache maps keyed by handle) would re-introduce invalidation by hand everywhere. |
| 14 | **`Board` cannot own a typed `ConnectivityData`** (crate order). | Callers must keep board and connectivity together themselves. | `Session` struct in `kicad-cli`/`kicad-wasm` (or a tiny `kicad-session` crate) owns `Board`, `ConnectivityData`, `DrcEngine` state and offers the convenience methods C++ has on `BOARD` (`build_connectivity`, `fill_zones`, `run_drc`). |

---------------------------------------------------------------------------------------------

## 3. Type definitions

Conventions used below:

* Field comments name the C++ member. Types from the foundation crates: `Vector2I`, `Box2I`,
  `EdaAngle`, `EdaOrientation` (the normalised angle wrapper of `eda_angle.h`), `Seg`,
  `ShapeLineChain`, `ShapePolySet`, `ShapeSegment`, `ShapeCompound`, `TransformTrs`, `Vector3D`,
  `Hash128`, `Ellipse<i32>` (`kicad-geom`); `Kiid`, `KiidPath`, `PcbLayerId`, `Lset`, `Lseq`,
  `LibId`, `KicadT`, `TextAttributes`, `StrokeParams`, `LineEnding`, `Color4D`, `Severity`,
  `EdaUnits` (`kicad-core`); `Sexpr` (`kicad-sexpr`). `TransformTrs`, `EdaOrientation`,
  `Vector3D`, `Hash128`, `Ellipse`, `LineEnding` are requested from those crates if not already
  planned (section 8, Q1).
* `wxString` -> `String`; `std::map` -> `BTreeMap`; `std::set` -> `BTreeSet`;
  `std::unordered_map/set` -> `BTreeMap/BTreeSet` unless only point lookups happen;
  `std::optional<T>` -> `Option<T>`; `int` -> `i32`; `long long` -> `i64`; `double` -> `f64`.
* All structs derive `Clone, Debug`; value-like ones also `PartialEq`. Fields are `pub` unless a
  cache or an index must stay consistent with them, in which case they are `pub(crate)` with
  accessors (marked `// private`).
* "dropped" members are collected in 3.17.

### 3.1 Common item data

```rust
/// Port of the EDA_ITEM flag bits (include/eda_item_flags.h). Same bit values.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct EdaItemFlags(pub u32);
impl EdaItemFlags {
    pub const IS_CHANGED: u32 = 1 << 0;          pub const IS_LINKED: u32 = 1 << 1;
    pub const IN_EDIT: u32 = 1 << 2;             pub const IS_MOVING: u32 = 1 << 3;
    pub const IS_NEW: u32 = 1 << 4;              pub const IS_BROKEN: u32 = 1 << 5;
    pub const IS_DELETED: u32 = 1 << 7;          pub const STARTPOINT: u32 = 1 << 9;
    pub const ENDPOINT: u32 = 1 << 10;           pub const SELECTED: u32 = 1 << 11;
    pub const SELECTED_BY_DRAG: u32 = 1 << 12;   pub const STRUCT_DELETED: u32 = 1 << 13;
    pub const CANDIDATE: u32 = 1 << 14;          pub const SKIP_STRUCT: u32 = 1 << 15;
    pub const IS_PASTED: u32 = 1 << 17;          pub const IS_SHOWN_AS_BITMAP: u32 = 1 << 18;
    pub const COURTYARD_CONFLICT: u32 = 1 << 19; pub const MALFORMED_F_COURTYARD: u32 = 1 << 20;
    pub const MALFORMED_B_COURTYARD: u32 = 1 << 21; pub const ROUTER_TRANSIENT: u32 = 1 << 22;
    pub const CONNECTIVITY_CANDIDATE: u32 = 1 << 23; pub const HOLE_PROXY: u32 = 1 << 24;
    pub const BRIGHTENED: u32 = 1 << 26;         pub const UR_TRANSIENT: u32 = 1 << 28;
    pub const ENTERED: u32 = 1 << 30;            pub const SELECTION_CANDIDATE: u32 = 1 << 31;
    // has_flag / set_flags / clear_flags / edit_flags / temp_flags as in EDA_ITEM
}

/// `EDA_ITEM::m_customProperties` (format 20260831). Keys keep their spelling; `set` replaces an
/// entry whose key matches case-insensitively, like EDA_ITEM::SetCustomProperty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomProperties(pub Option<Box<BTreeMap<String, String>>>);

/// The data members of EDA_ITEM + BOARD_ITEM. Embedded as field `item` in every item struct.
#[derive(Clone, Debug, PartialEq)]
pub struct ItemBase {
    pub(crate) uuid: Kiid,                  // EDA_ITEM::m_Uuid          // private: UuidIndex
    pub flags: EdaItemFlags,                // EDA_ITEM::m_flags
    pub(crate) group: Option<Kiid>,         // EDA_ITEM::m_group (EDA_GROUP*)   // private: PcbGroup::items
    pub layer: PcbLayerId,                  // BOARD_ITEM::m_layer
    pub is_knockout: bool,                  // BOARD_ITEM::m_isKnockout
    pub is_locked: bool,                    // BOARD_ITEM::m_isLocked (raw flag; effective lock is View::is_locked)
    pub custom_properties: CustomProperties,// EDA_ITEM::m_customProperties
}
impl ItemBase {
    pub fn new(layer: PcbLayerId) -> Self;                 // fresh Kiid from the caller-supplied generator
    pub fn uuid(&self) -> Kiid;
    pub fn set_uuid_direct(&mut self, id: Kiid);           // BOARD_ITEM::SetUuidDirect: detached items only
    pub fn parent_group_id(&self) -> Option<Kiid>;         // EDA_ITEM::GetParentGroupId
}

/// The data members of BOARD_CONNECTED_ITEM. Embedded as field `conn`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectedBase {
    /// `m_netinfo` (NETINFO_ITEM*) as a net code. `NetinfoList::UNCONNECTED` (0) doubles as
    /// the static "orphaned" net of C++; -1 mirrors `m_netinfo == nullptr`.
    pub net_code: i32,
    pub teardrop_params: TeardropParameters,   // m_teardropParams
    pub local_ratsnest_visible: bool,          // m_localRatsnestVisible
}

/// TEARDROP_PARAMETERS (pcbnew/teardrop/teardrop_parameters.h).
#[derive(Clone, Debug, PartialEq)]
pub struct TeardropParameters {
    pub td_max_len: i32,                   // m_TdMaxLen           (default 1.0 mm)
    pub td_max_width: i32,                 // m_TdMaxWidth         (default 2.0 mm)
    pub best_length_ratio: f64,            // m_BestLengthRatio    (0.5)
    pub best_width_ratio: f64,             // m_BestWidthRatio     (1.0)
    pub width_to_size_filter_ratio: f64,   // m_WidthtoSizeFilterRatio (0.9)
    pub curved_edges: bool,                // m_CurvedEdges
    pub enabled: bool,                     // m_Enabled
    pub allow_use_two_tracks: bool,        // m_AllowUseTwoTracks
    pub td_on_pads_in_zones: bool,         // m_TdOnPadsInZones
}

/// TEARDROP_PARAMETERS_LIST.
#[derive(Clone, Debug, PartialEq)]
pub struct TeardropParametersList {
    pub params_list: [TeardropParameters; 3],   // m_params_list indexed by TargetTd (ROUND, RECT, TRACK)
    pub target_vias: bool, pub target_pth_pads: bool, pub target_smd_pads: bool,
    pub target_track2track: bool, pub use_round_shapes_only: bool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ZoneLayerOverride { None, ForceFlashed, ForceNoZoneConnection }   // ZONE_LAYER_OVERRIDE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum Flashing { Default, AlwaysFlashed, NeverFlashed }                // FLASHING
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum BoardUse { Normal, Fpholder }                                     // BOARD_USE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum RecurseMode { Recurse, NoRecurse }                                // RECURSE_MODE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum FlipDirection { LeftRight, TopBottom }                            // FLIP_DIRECTION

/// DRC_CONSTRAINT_T (pcbnew/drc/drc_constraint_types.h), all 42 values in C++ order:
/// NullConstraint, ClearanceConstraint, CreepageConstraint, HoleClearanceConstraint,
/// HoleToHoleConstraint, EdgeClearanceConstraint, HoleSizeConstraint,
/// CourtyardClearanceConstraint, SilkClearanceConstraint, TextHeightConstraint,
/// TextThicknessConstraint, TrackWidthConstraint, TrackSegmentLengthConstraint,
/// AnnularWidthConstraint, ZoneConnectionConstraint, ThermalReliefGapConstraint,
/// ThermalSpokeWidthConstraint, MinResolvedSpokesConstraint, SolderMaskExpansionConstraint,
/// SolderPasteAbsMarginConstraint, SolderPasteRelMarginConstraint, DisallowConstraint,
/// ViaDiameterConstraint, LengthConstraint, NetChainLengthConstraint,
/// NetChainStubLengthConstraint, NetChainReturnPathConstraint, SkewConstraint,
/// DiffPairGapConstraint, MaxUncoupledConstraint, DiffPairIntraSkewConstraint,
/// ViaCountConstraint, PhysicalClearanceConstraint, PhysicalHoleClearanceConstraint,
/// AssertionConstraint, ConnectionWidthConstraint, TrackAngleConstraint, ViaDanglingConstraint,
/// BridgedMaskConstraint, SolderMaskSliverConstraint, MicroviaStackDepthConstraint,
/// HoleAspectRatioConstraint.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum DrcConstraintT { NullConstraint = 0, ClearanceConstraint, /* ... as listed ... */ HoleAspectRatioConstraint }
```

### 3.2 `Board`

```rust
/// Port of BOARD (pcbnew/board.h). Owns everything.
#[derive(Clone, Debug)]
pub struct Board {
    pub item: ItemBase,                               // BOARD is a BOARD_ITEM: m_Uuid (layer unused)
    pub board_use: BoardUse,                          // m_boardUse
    pub(crate) time_stamp: i32,                       // m_timeStamp (std::atomic<int>, a modification counter)
    pub(crate) structure_epoch: u64,                  // Rust only, see 2.3
    pub file_name: String,                            // m_fileName
    pub file_format_version_at_load: i32,             // m_fileFormatVersionAtLoad
    pub generator: String,                            // m_generator

    // ---- items (all private: mutate through Board methods) ----
    pub(crate) markers: ItemList<PcbMarker>,          // m_markers
    pub(crate) drawings: ItemList<Drawing>,           // m_drawings
    pub(crate) footprints: ItemList<Footprint>,       // m_footprints
    pub(crate) tracks: ItemList<Track>,               // m_tracks
    pub(crate) groups: ItemList<PcbGroup>,            // m_groups
    pub(crate) constraints: ItemList<PcbConstraint>,  // m_constraints
    pub(crate) zones: ItemList<Zone>,                 // m_zones
    pub(crate) generators: ItemList<PcbGenerator>,    // m_generators
    pub(crate) points: ItemList<PcbPoint>,            // m_points
    pub(crate) uuid_index: UuidIndex,                 // m_itemIndex

    // ---- layers ----
    pub layers: BTreeMap<i32, Layer>,                 // m_layers (std::map<int, LAYER>)

    // ---- nets ----
    pub net_info: NetinfoList,                        // m_NetInfo
    pub net_chain_colors: BTreeMap<String, Color4D>,  // m_netChainColors

    // ---- settings ----
    pub design_settings: BoardDesignSettings,         // m_designSettings (shared_ptr) -> owned
    pub paper: PageInfo,                              // m_paper
    pub titles: TitleBlock,                           // m_titles
    pub plot_options: PcbPlotParams,                  // m_plotOptions
    pub user_units: EdaUnits,                         // m_userUnits (dimensions with auto units read it)
    pub properties: BTreeMap<String, String>,         // m_properties
    pub current_variant: String,                      // m_currentVariant
    pub variant_names: Vec<String>,                   // m_variantNames
    pub variant_descriptions: BTreeMap<String, String>, // m_variantDescriptions
    pub legacy_teardrops: bool,                       // m_legacyTeardrops
    pub outlines_chaining_epsilon: i32,               // m_outlinesChainingEpsilon
    pub embedded_files: EmbeddedFiles,                // base class EMBEDDED_FILES (incl. m_embedFonts)
    pub component_class_manager: ComponentClassManager, // m_componentClassManager
    pub project: ProjectContext,                      // replaces PROJECT* m_project (see below)

    // ---- flags set by the parser for legacy files ----
    pub legacy_visible_layers: Lset,                  // m_LegacyVisibleLayers
    pub legacy_design_settings_loaded: bool,          // m_LegacyDesignSettingsLoaded
    pub legacy_copper_edge_clearance_loaded: bool,    // m_LegacyCopperEdgeClearanceLoaded
    pub legacy_netclasses_loaded: bool,               // m_LegacyNetclassesLoaded

    // ---- class-1 caches ----
    pub(crate) max_clearance_value: LazyCache<i32, i32>,          // m_maxClearanceValue
    pub(crate) board_outline: LazyCache<i32, PcbBoardOutline>,    // m_boardOutline + m_boardOutlineGeneration
}

/// What the model needs from PROJECT / PROJECT_FILE / PROJECT_LOCAL_SETTINGS, as plain data
/// supplied by the caller (no file access in library crates, PORTING.md section 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectContext {
    pub has_project: bool,                            // m_project != nullptr (several functions early-return)
    pub project_name: String,                         // PROJECT::GetProjectName  (${PROJECTNAME})
    pub project_path: String,                         // PROJECT::GetProjectPath  (${KIPRJMOD}), text only
    pub text_vars: BTreeMap<String, String>,          // PROJECT::GetTextVars
    pub prototype_zone_fill: bool,                    // PROJECT_LOCAL_SETTINGS::m_PrototypeZoneFill
    pub tuning_profiles: Vec<TuningProfile>,          // PROJECT_FILE tuning profiles (data only)
    pub component_class_settings: ComponentClassSettings, // PROJECT_FILE::ComponentClassSettings
    pub current_date: Option<String>,                 // for ${CURRENT_DATE}; never read from a clock
    pub design_rules_path: String,                    // BOARD::GetDesignRulesPath() result, text only
}

/// LAYER_T.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum LayerT { Undefined = -1, Signal, Power, Mixed, Jumper, Aux, Front, Back }

/// LAYER (board.h).
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,        // m_name      canonical name
    pub user_name: String,   // m_userName
    pub type_: LayerT,       // m_type
    pub visible: bool,       // m_visible
    pub number: i32,         // m_number    (PCB_LAYER_ID)
    pub opposite: i32,       // m_opposite
}

/// TITLE_BLOCK: m_tbTexts[0..12] = title, date, revision, company, comment1..comment9.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TitleBlock { pub tb_texts: Vec<String> }

/// PAGE_INFO.
#[derive(Clone, Debug, PartialEq)]
pub struct PageInfo {
    pub type_: PageSizeType,     // m_type: A5, A4, A3, A2, A1, A0, A, B, C, D, E, Gerber, USLetter, USLegal, USLedger, User
    pub size: Vector2D,          // m_size (mils)
    pub portrait: bool,          // m_portrait
    pub description: String,     // m_description
}

/// The synthetic outline item BOARD::m_boardOutline (PCB_BOARD_OUTLINE). Derived, never saved.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PcbBoardOutline { pub outlines: ShapePolySet }
```

### 3.3 Nets, netclasses, net settings

```rust
/// NETINFO_ITEM (pcbnew/netinfo.h).
#[derive(Clone, Debug)]
pub struct NetinfoItem {
    pub uuid: Kiid,                         // EDA_ITEM::m_Uuid (never saved; ResolveItem can return a net)
    pub net_code: i32,                      // m_netCode
    pub netname: String,                    // m_netname
    pub short_netname: String,              // m_shortNetname
    pub display_netname: String,            // m_displayNetname
    pub net_chain: String,                  // m_netChain
    pub terminal_pad_uuids: [Kiid; 2],      // m_terminalPadUuids  (m_terminalPads pointers dropped)
    pub netclass: Option<Arc<Netclass>>,    // m_netClass
    pub is_current: bool,                   // m_isCurrent
}

/// NETINFO_LIST. `by_name` reproduces the iteration order of `NETNAMES_MAP`
/// (std::map<wxString, NETINFO_ITEM*>), which is what BOARD::BeginNets()/EndNets() and
/// `for( NETINFO_ITEM* net : board->GetNetInfo() )` walk.
#[derive(Clone, Debug)]
pub struct NetinfoList {
    pub(crate) by_code: Vec<Option<NetinfoItem>>,    // m_netCodes (owner here; index = net code)
    pub(crate) by_name: BTreeMap<String, i32>,       // m_netNames
    pub(crate) new_net_code: i32,                    // m_newNetCode
    pub display_netnames_dirty: bool,                // m_DisplayNetnamesDirty
}
impl NetinfoList {
    pub const UNCONNECTED: i32 = 0;
    pub const ORPHANED: i32 = -1;
    pub fn net_item_by_code(&self, code: i32) -> Option<&NetinfoItem>;     // GetNetItem( int )   O(1)
    pub fn net_item_by_name(&self, name: &str) -> Option<&NetinfoItem>;    // GetNetItem( wxString )
    pub fn net_count(&self) -> usize;                                      // GetNetCount
    pub fn iter(&self) -> impl Iterator<Item = &NetinfoItem>;              // name order
    pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem>;   // NetsByNetcode() order
    pub(crate) fn append_net(&mut self, net: NetinfoItem) -> bool;         // AppendNet (dedup by name)
    pub(crate) fn remove_net(&mut self, code: i32) -> Option<NetinfoItem>; // RemoveNet
    pub(crate) fn build_list_of_nets(&mut self);                           // buildListOfNets
    pub(crate) fn get_free_net_code(&mut self) -> i32;                     // getFreeNetCode
    pub fn rebuild_display_netnames(&mut self);                            // RebuildDisplayNetnames
}

/// NETCLASS (include/netclass.h). Immutable once shared in an Arc.
#[derive(Clone, Debug, PartialEq)]
pub struct Netclass {
    pub is_default: bool,                    // m_isDefault
    pub constituents: Vec<Arc<Netclass>>,    // m_constituents (+ m_constituentOwners)
    pub name: String,                        // m_Name
    pub priority: i32,                       // m_Priority
    pub description: String,                 // m_Description
    pub clearance: Option<i32>,              // m_Clearance
    pub track_width: Option<i32>,            // m_TrackWidth
    pub via_dia: Option<i32>,                // m_ViaDia
    pub via_drill: Option<i32>,              // m_ViaDrill
    pub u_via_dia: Option<i32>,              // m_uViaDia
    pub u_via_drill: Option<i32>,            // m_uViaDrill
    pub diff_pair_width: Option<i32>,        // m_diffPairWidth
    pub diff_pair_gap: Option<i32>,          // m_diffPairGap
    pub diff_pair_via_gap: Option<i32>,      // m_diffPairViaGap
    pub wire_width: Option<i32>,             // m_wireWidth
    pub bus_width: Option<i32>,              // m_busWidth
    pub schematic_color: Color4D,            // m_schematicColor
    pub line_style: Option<i32>,             // m_lineStyle
    pub pcb_color: Color4D,                  // m_pcbColor
    pub tuning_profile: String,              // m_tuningProfile
    /// m_clearanceParent ... m_tuningProfileParent: the netclass that provides each value of an
    /// aggregate. C++ stores NETCLASS*; here the index into `constituents`, None = this class.
    pub parents: NetclassParents,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetclassParents {
    pub clearance: Option<u16>, pub track_width: Option<u16>, pub via_diameter: Option<u16>,
    pub via_drill: Option<u16>, pub u_via_dia: Option<u16>, pub u_via_drill: Option<u16>,
    pub diff_pair_width: Option<u16>, pub diff_pair_gap: Option<u16>, pub diff_pair_via_gap: Option<u16>,
    pub pcb_color: Option<u16>, pub wire_width: Option<u16>, pub bus_width: Option<u16>,
    pub schematic_color: Option<u16>, pub line_style: Option<u16>, pub tuning_profile: Option<u16>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum NetChainSource { /* NET_CHAIN_SOURCE values of net_settings.h */ }

/// NET_SETTINGS (include/project/net_settings.h) without the JSON plumbing (the JSON
/// load/store is the project-settings work package).
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
    pub(crate) netclass_bus_members: BTreeMap<String, BTreeSet<String>>,   // m_netclassBusMembers
    // caches, rebuilt by recompute_effective_netclasses()/clear_all_caches()
    pub(crate) composite_netclasses: BTreeMap<String, Arc<Netclass>>,      // m_compositeNetClasses
    pub(crate) implicit_netclasses: BTreeMap<String, Arc<Netclass>>,       // m_impicitNetClasses
    pub(crate) effective_netclass_cache: BTreeMap<String, Arc<Netclass>>,  // m_effectiveNetclassCache
}
impl NetSettings {
    pub fn get_default_netclass(&self) -> &Arc<Netclass>;
    pub fn has_netclass(&self, name: &str) -> bool;
    pub fn get_netclass_by_name(&self, name: &str) -> Option<Arc<Netclass>>;      // GetNetClassByName
    /// GetEffectiveNetClass (non-const in C++: fills the caches).
    pub fn get_effective_netclass(&mut self, net_name: &str) -> Arc<Netclass>;
    pub fn get_cached_effective_netclass(&self, net_name: &str) -> Option<Arc<Netclass>>;
    pub fn has_effective_netclass(&self, net_name: &str) -> bool;
    pub fn recompute_effective_netclasses(&mut self);
    pub fn clear_cache_for_net(&mut self, net_name: &str);
    pub fn clear_all_caches(&mut self);
    pub fn set_netclass_pattern_assignment(&mut self, pattern: &str, netclass: &str);
    pub fn parse_bus_vector(bus: &str) -> Option<(String, Vec<String>)>;          // static helpers
    pub fn parse_bus_group(group: &str) -> Option<(String, Vec<String>)>;
}
```

Effective netclass resolution (section 5.2) is the C++ algorithm: `Board::synchronize_nets_and_net_classes`
asks `NetSettings::get_effective_netclass(name)` for every net (label assignments, pattern
assignments via `EdaCombinedMatcher`, net-chain classes, priority-ordered aggregation in
`make_effective_netclass`) and stores the resulting `Arc<Netclass>` in `NetinfoItem::netclass`.
After that, `item.effective_netclass()` is a vector index plus an `Arc` deref - the hot path
(`DRC_ENGINE::EvalRules`) never allocates, matching the C++ comment "do NOT return a
std::shared_ptr from this".

### 3.4 Design settings, zone settings, stackup, plot parameters

```rust
/// VIA_DIMENSION / DIFF_PAIR_DIMENSION / VIA_STACK_PRESET / TEXT_ITEM_INFO (board_design_settings.h).
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ViaDimension { pub diameter: i32, pub drill: i32 }
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiffPairDimension { pub width: i32, pub gap: i32, pub via_gap: i32 }
#[derive(Clone, Debug, PartialEq)]
pub struct ViaStackPreset { pub name: String, pub start_layer: PcbLayerId, pub end_layer: PcbLayerId,
    pub staggered: bool, pub via_size: i32, pub via_drill: i32, pub use_netclass: bool,
    pub filled: bool, pub capped: bool, pub pitch: i32 }
#[derive(Clone, Debug, PartialEq)]
pub struct TextItemInfo { pub text: String, pub visible: bool, pub layer: PcbLayerId }

pub const LAYER_CLASS_SILK: usize = 0;  pub const LAYER_CLASS_COPPER: usize = 1;
pub const LAYER_CLASS_EDGES: usize = 2; pub const LAYER_CLASS_COURTYARD: usize = 3;
pub const LAYER_CLASS_FAB: usize = 4;   pub const LAYER_CLASS_OTHERS: usize = 5;
pub const LAYER_CLASS_COUNT: usize = 6;

/// PNS::MEANDER_SETTINGS (pcbnew/router/pns_meander.h). Data only; lives here because
/// BOARD_DESIGN_SETTINGS embeds three of them and the `setup`/project files store them.
#[derive(Clone, Debug, PartialEq)]
pub struct MeanderSettings {
    pub min_amplitude: i32, pub max_amplitude: i32, pub spacing: i32, pub step: i32,
    pub len_pad_to_die: i32, pub signal_extra_length: i64, pub signal_extra_delay: i64,
    pub target_length: MinOptMax<i64>, pub target_length_delay: MinOptMax<i64>,
    pub target_signal_length: MinOptMax<i64>, pub target_signal_length_delay: MinOptMax<i64>,
    pub target_skew: MinOptMax<i32>, pub target_skew_delay: MinOptMax<i32>,
    pub override_custom_rules: bool, pub corner_style: MeanderStyle /* ROUND, CHAMFER */,
    pub corner_radius_percentage: i32, pub single_sided: bool,
    pub initial_side: MeanderSide /* MEANDER_SIDE */, pub length_tolerance: i32,
    pub keep_endpoints: bool, pub is_time_domain: bool,
}

/// DRC_EXCLUSION: a serialised marker + comment (proto `DrcExclusion { DrcMarker marker; string comment }`).
/// Ordered by `sort_key` like DRC_EXCLUSION_COMPARE.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DrcExclusion { pub sort_key: String, pub marker: DrcExclusionMarker, pub comment: String }

/// BOARD_DESIGN_SETTINGS (include/board_design_settings.h) without NESTED_SETTINGS/JSON.
#[derive(Clone, Debug)]
pub struct BoardDesignSettings {
    pub track_width_list: Vec<i32>,                        // m_TrackWidthList
    pub vias_dimensions_list: Vec<ViaDimension>,           // m_ViasDimensionsList
    pub via_stack_presets: Vec<ViaStackPreset>,            // m_ViaStackPresets
    pub diff_pair_dimensions_list: Vec<DiffPairDimension>, // m_DiffPairDimensionsList
    pub teardrop_params_list: TeardropParametersList,      // m_TeardropParamsList
    pub single_track_meander_settings: MeanderSettings,    // m_SingleTrackMeanderSettings
    pub diff_pair_meander_settings: MeanderSettings,       // m_DiffPairMeanderSettings
    pub skew_meander_settings: MeanderSettings,            // m_SkewMeanderSettings
    pub current_via_type: Viatype,                         // m_CurrentViaType
    pub use_connected_track_width: bool,                   // m_UseConnectedTrackWidth
    pub temp_override_track_width: bool,                   // m_TempOverrideTrackWidth
    pub min_clearance: i32,                                // m_MinClearance
    pub min_groove_width: i32,                             // m_MinGrooveWidth
    pub min_conn: i32,                                     // m_MinConn
    pub track_min_width: i32,                              // m_TrackMinWidth
    pub vias_min_annular_width: i32,                       // m_ViasMinAnnularWidth
    pub vias_min_size: i32,                                // m_ViasMinSize
    pub min_through_drill: i32,                            // m_MinThroughDrill
    pub micro_vias_min_size: i32,                          // m_MicroViasMinSize
    pub micro_vias_min_drill: i32,                         // m_MicroViasMinDrill
    pub copper_edge_clearance: i32,                        // m_CopperEdgeClearance
    pub hole_clearance: i32,                               // m_HoleClearance
    pub hole_to_hole_min: i32,                             // m_HoleToHoleMin
    pub silk_clearance: i32,                               // m_SilkClearance
    pub min_resolved_spokes: i32,                          // m_MinResolvedSpokes
    pub min_silk_text_height: i32,                         // m_MinSilkTextHeight
    pub min_silk_text_thickness: i32,                      // m_MinSilkTextThickness
    pub drc_severities: BTreeMap<i32, Severity>,           // m_DRCSeverities (DRCErrorCode -> SEVERITY)
    pub drc_exclusions: BTreeSet<DrcExclusion>,            // m_DrcExclusions
    pub zone_keep_external_fillets: bool,                  // m_ZoneKeepExternalFillets
    pub max_error: i32,                                    // m_MaxError
    pub solder_mask_expansion: i32,                        // m_SolderMaskExpansion
    pub solder_mask_min_width: i32,                        // m_SolderMaskMinWidth
    pub solder_mask_to_copper_clearance: i32,              // m_SolderMaskToCopperClearance
    pub solder_paste_margin: i32,                          // m_SolderPasteMargin
    pub solder_paste_margin_ratio: f64,                    // m_SolderPasteMarginRatio
    pub allow_soldermask_bridges_in_fps: bool,             // m_AllowSoldermaskBridgesInFPs
    pub tent_vias_front: bool, pub tent_vias_back: bool,   // m_TentViasFront/Back
    pub cover_vias_front: bool, pub cover_vias_back: bool, // m_CoverViasFront/Back
    pub plug_vias_front: bool, pub plug_vias_back: bool,   // m_PlugViasFront/Back
    pub cap_vias: bool, pub fill_vias: bool,               // m_CapVias, m_FillVias
    pub net_settings: NetSettings,                         // m_NetSettings (shared_ptr) -> owned
    pub default_fp_text_items: Vec<TextItemInfo>,          // m_DefaultFPTextItems
    pub user_layer_names: BTreeMap<String, String>,        // m_UserLayerNames
    pub zone_layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_ZoneLayerProperties
    pub line_thickness: [i32; LAYER_CLASS_COUNT],          // m_LineThickness
    pub text_size: [Vector2I; LAYER_CLASS_COUNT],          // m_TextSize
    pub text_thickness: [i32; LAYER_CLASS_COUNT],          // m_TextThickness
    pub text_italic: [bool; LAYER_CLASS_COUNT],            // m_TextItalic
    pub text_upright: [bool; LAYER_CLASS_COUNT],           // m_TextUpright
    pub dimension_units_mode: DimUnitsMode,                // m_DimensionUnitsMode
    pub dimension_precision: DimPrecision,                 // m_DimensionPrecision
    pub dimension_units_format: DimUnitsFormat,            // m_DimensionUnitsFormat
    pub dimension_suppress_zeroes: bool,                   // m_DimensionSuppressZeroes
    pub dimension_text_position: DimTextPosition,          // m_DimensionTextPosition
    pub dimension_keep_text_aligned: bool,                 // m_DimensionKeepTextAligned
    pub dimension_arrow_length: i32,                       // m_DimensionArrowLength
    pub dimension_extension_offset: i32,                   // m_DimensionExtensionOffset
    pub style_fp_fields: bool, pub style_fp_text: bool, pub style_fp_shapes: bool,
    pub style_fp_dimensions: bool, pub style_fp_barcodes: bool,   // m_StyleFP*
    pub has_stackup: bool,                                 // m_HasStackup
    pub use_height_for_length_calcs: bool,                 // m_UseHeightForLengthCalcs
    pub aux_origin: Vector2I,                              // m_auxOrigin
    pub grid_origin: Vector2I,                             // m_gridOrigin
    pub track_width_index: i32,                            // m_trackWidthIndex
    pub via_size_index: i32,                               // m_viaSizeIndex
    pub diff_pair_index: i32,                              // m_diffPairIndex
    pub via_stack_index: i32,                              // m_viaStackIndex
    pub use_custom_track_via: bool,                        // m_useCustomTrackVia
    pub custom_track_width: i32,                           // m_customTrackWidth
    pub custom_via_size: ViaDimension,                     // m_customViaSize
    pub use_custom_diff_pair: bool,                        // m_useCustomDiffPair
    pub custom_diff_pair: DiffPairDimension,               // m_customDiffPair
    pub copper_layer_count: i32,                           // m_copperLayerCount
    pub user_defined_layer_count: i32,                     // m_userDefinedLayerCount
    pub enabled_layers: Lset,                              // m_enabledLayers
    pub board_thickness: i32,                              // m_boardThickness
    pub current_net_class_name: String,                    // m_currentNetClassName
    pub stackup: BoardStackup,                             // m_stackup
    pub drill_symbol_profile: DrillSymbolProfile,          // m_drillSymbolProfile
    pub default_zone_settings: ZoneSettings,               // m_defaultZoneSettings
}
// dropped: m_DRCEngine (-> BoardCtx::rules), NESTED_SETTINGS / FIELDS_TABLE_BOM_SETTINGS bases.
// Methods kept (same names, snake_case): get_severity, ignore, get_stackup_descriptor,
// get_drc_epsilon, get_hole_plating_thickness, get_text_size/get_line_thickness( layer ),
// get_layer_class, is_layer_enabled, set_enabled_layers, set_copper_layer_count,
// get_current_track_width / via size / via drill / diff pair, get_smallest_clearance_value,
// get_biggest_clearance_value( Option<&dyn RuleResolver> ), use_net_class_track ...

/// ZONE_SETTINGS (pcbnew/zone_settings.h).
#[derive(Clone, Debug, PartialEq)]
pub struct ZoneSettings {
    pub zone_priority: u32,                       // m_ZonePriority
    pub fill_mode: ZoneFillMode,                  // m_FillMode
    pub zone_clearance: i32,                      // m_ZoneClearance
    pub zone_min_thickness: i32,                  // m_ZoneMinThickness
    pub hatch_thickness: i32,                     // m_HatchThickness
    pub hatch_gap: i32,                           // m_HatchGap
    pub hatch_orientation: EdaAngle,              // m_HatchOrientation
    pub hatch_smoothing_level: i32,               // m_HatchSmoothingLevel
    pub hatch_smoothing_value: f64,               // m_HatchSmoothingValue
    pub hatch_hole_min_area: f64,                 // m_HatchHoleMinArea
    pub hatch_border_algorithm: i32,              // m_HatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,      // m_ThievingSettings
    pub netcode: i32,                             // m_Netcode
    pub name: String,                             // m_Name
    pub layers: Lset,                             // m_Layers
    pub zone_border_display_style: ZoneBorderDisplayStyle, // m_ZoneBorderDisplayStyle
    pub border_hatch_pitch: i32,                  // m_BorderHatchPitch
    pub thermal_relief_gap: i64,                  // m_ThermalReliefGap (long)
    pub thermal_relief_spoke_width: i64,          // m_ThermalReliefSpokeWidth (long)
    pub locked: bool,                             // m_Locked
    pub teardrop_type: TeardropType,              // m_TeardropType
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_LayerProperties
    pub corner_smoothing_type: CornerSmoothing,   // m_cornerSmoothingType
    pub corner_radius: u32,                       // m_cornerRadius
    pub pad_connection: ZoneConnection,           // m_padConnection
    pub is_rule_area: bool,                       // m_isRuleArea
    pub placement_area_enabled: bool,             // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT, // m_placementAreaSourceType
    pub placement_area_source: String,            // m_placementAreaSource
    pub keepout_do_not_allow_zone_fills: bool, pub keepout_do_not_allow_vias: bool,
    pub keepout_do_not_allow_tracks: bool, pub keepout_do_not_allow_pads: bool,
    pub keepout_do_not_allow_footprints: bool,    // m_keepoutDoNotAllow*
    pub remove_islands: IslandRemovalMode,        // m_removeIslands
    pub min_island_area: i64,                     // m_minIslandArea
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ZoneFillMode { Polygons, HatchPattern, CopperThieving }  // ZONE_FILL_MODE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] #[repr(i32)]
pub enum ZoneConnection { Inherited = -1, None, Thermal, Full, ThtThermal }                                    // ZONE_CONNECTION
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ZoneBorderDisplayStyle { NoHatch, DiagonalFull, DiagonalEdge, InvisibleBorder }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum IslandRemovalMode { Always, Never, Area }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PlacementSourceT { Sheetname, ComponentClass, GroupPlacement, DesignBlock }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum CornerSmoothing { NoSmoothing, Chamfer, Fillet }         // ZONE_SETTINGS::CORNER_SMOOTHING
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum TeardropType { TdNone, TdUnspecified, TdViapad, TdTrackend }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ThievingPattern { Dots, Squares, Hatch }
#[derive(Clone, Debug, Default, PartialEq)] pub struct ZoneLayerProperties { pub hatching_offset: Option<Vector2I> }
#[derive(Clone, Debug, PartialEq)]
pub struct ThievingSettings { pub pattern: ThievingPattern, pub element_size: i32, pub gap: i32,
    pub line_width: i32, pub stagger: bool, pub orientation: EdaOrientation }
#[derive(Clone, Debug, PartialEq)]
pub struct RepeatLayoutOptions { pub copy_routing: bool, pub connected_routing_only: bool,
    pub copy_placement: bool, pub copy_other_items: bool, pub group_items: bool,
    pub include_locked_items: bool, pub anchor: Kiid }

/// BOARD_STACKUP_ITEM_TYPE / BS_EDGE_CONNECTOR_CONSTRAINTS / DIELECTRIC_PRMS / BOARD_STACKUP_ITEM / BOARD_STACKUP.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BoardStackupItemType { Undefined, Copper, Dielectric, Solderpaste, Soldermask, Silkscreen }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BsEdgeConnectorConstraints { None, InUse, Bevelled }
#[derive(Clone, Debug, PartialEq)]
pub struct DielectricPrms {
    pub material: String, pub thickness: i32, pub thickness_locked: bool, pub epsilon_r: f64,
    pub loss_tangent: f64, pub spec_freq: f64, pub dielectric_model: DielectricModel, pub color: String,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackupItem {
    pub type_: BoardStackupItemType,              // m_Type
    pub layer_name: String,                       // m_LayerName
    pub type_name: String,                        // m_TypeName
    pub layer_id: PcbLayerId,                     // m_LayerId
    pub dielectric_layer_id: i32,                 // m_DielectricLayerId
    pub dielectric_prms_list: Vec<DielectricPrms>,// m_DielectricPrmsList
    pub enabled: bool,                            // m_enabled
}
#[derive(Clone, Debug, PartialEq)]
pub struct BoardStackup {
    pub finish_type: String,                      // m_FinishType
    pub has_dielectric_constrains: bool,          // m_HasDielectricConstrains
    pub has_thickness_constrains: bool,           // m_HasThicknessConstrains
    pub edge_connector_constraints: BsEdgeConnectorConstraints, // m_EdgeConnectorConstraints
    pub edge_plating: bool,                       // m_EdgePlating
    pub list: Vec<BoardStackupItem>,              // m_list (vector of owning pointers)
}

/// DRILL_SYMBOL_PROFILE (pcbnew/drill/drill_symbol_profile.h), data only.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSymbolProfile {
    pub uuid: Kiid, pub name: String,
    pub group_by: BTreeSet<DrillGroupKey>,        // SIZE_GRP, SLOT, PLATING, SPAN, OPERATION, HOLE_FUNCTION, PROTECTION, POST_MACHINING
    pub mark_policy: DrillMarkPolicy,             // SHAPES, LETTERS, SHAPES_THEN_LETTERS, SIZE_TEXT
    pub symbol_size: i32, pub symbol_width: i32, pub freeze_assignments: bool,
    pub assignments: BTreeMap<String, DrillSymbolAssignment>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSymbolAssignment { pub mark_mode: DrillMarkMode /* SHAPE, LETTER, SIZE_TEXT */,
    pub shape_index: i32, pub letter: String, pub description: String }

/// PCB_PLOT_PARAMS (pcbnew/pcb_plot_params.h). All persisted members; pointer members to
/// colour settings are dropped (plot consumers pass colours explicitly).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbPlotParams {
    pub pdf_front_fp_property_popups: bool, pub pdf_back_fp_property_popups: bool,
    pub pdf_metadata: bool, pub pdf_single: bool, pub background_color: Color4D,
    pub format: PlotFormat,                       // m_format
    pub layer_selection: Lset,                    // m_layerSelection
    pub plot_on_all_layers_sequence: Lseq,        // m_plotOnAllLayersSequence
    pub skip_npth_pads: bool,                     // m_skipNPTH_Pads
    pub plot_pad_numbers: bool,                   // m_plotPadNumbers
    pub drill_marks: DrillMarks,                  // m_drillMarks
    pub text_mode: PlotTextMode,                  // m_textMode
    pub dxf_plot_mode: DxfOutlineMode, pub dxf_units: DxfUnits, pub dxf_polygon_mode: bool,
    pub a4_output: bool, pub auto_scale: bool, pub scale: f64, pub mirror: bool,
    pub negative: bool, pub black_and_white: bool, pub plot_drawing_sheet: bool,
    pub plot_via_on_mask_layer: Option<bool>,     // m_plotViaOnMaskLayer (legacy read only)
    pub subtract_mask_from_silk: bool,
    pub use_gerber_protel_extensions: bool, pub use_gerber_x2_format: bool,
    pub gerber_disable_apert_macros: bool, pub include_gerber_netlist_info: bool,
    pub create_gerber_job_file: bool, pub gerber_precision: i32,
    pub svg_precision: u32, pub svg_fit_page_to_board: bool, pub png_dpi: i32, pub png_antialias: bool,
    pub use_aux_origin: bool, pub output_directory: String, pub scale_selection: i32,
    pub plot_reference: bool, pub plot_value: bool, pub plot_fp_text: bool,
    pub sketch_pads_on_fab_layers: bool, pub sketch_pad_line_width: i32,
    pub hide_dnp_fps_on_fab_layers: bool, pub sketch_dnp_fps_on_fab_layers: bool,
    pub crossout_dnp_fps_on_fab_layers: bool,
    pub fine_scale_adjust_x: f64, pub fine_scale_adjust_y: f64, pub width_adjust: i32,
    pub dashed_line_dash_ratio: f64, pub dashed_line_gap_ratio: f64,
    pub dxf_export_as_multi_layered_file: bool,
    pub layers_to_export: Vec<(PcbLayerId, String)>,
    pub layer: PcbLayerId,
}

/// TUNING_PROFILE (include/project/tuning_profiles.h), data only (length/delay calculation is tier 2).
#[derive(Clone, Debug, PartialEq)]
pub struct TuningProfile {
    pub profile_name: String, pub type_: TuningProfileType /* SINGLE, DIFFERENTIAL */,
    pub target_impedance: f64, pub frequency: f64, pub enable_time_domain_tuning: bool,
    pub model_solder_mask: bool,
    pub track_propagation_entries: Vec<DelayProfileTrackPropagationEntry>,
    pub via_propagation_delay: i32, pub via_overrides: Vec<DelayProfileViaOverrideEntry>,
    pub net_chain_bridge_propagation_delay: i32,
}
```

### 3.5 `Footprint`

```rust
/// FOOTPRINT_ATTR_T bits (m_attributes).
pub const FP_THROUGH_HOLE: i32 = 0x0001;  pub const FP_SMD: i32 = 0x0002;
pub const FP_EXCLUDE_FROM_POS_FILES: i32 = 0x0004; pub const FP_EXCLUDE_FROM_BOM: i32 = 0x0008;
pub const FP_BOARD_ONLY: i32 = 0x0010;    pub const FP_JUST_ADDED: i32 = 0x0020;
pub const FP_DNP: i32 = 0x0040;           pub const FP_EXCLUDE_FROM_SIM: i32 = 0x0080;

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum FootprintStackup { ExpandInnerLayers, CustomLayers }  // FOOTPRINT_STACKUP

/// FP_3DMODEL.
#[derive(Clone, Debug, PartialEq)]
pub struct Fp3dModel { pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D,
                       pub opacity: f64, pub filename: String, pub show: bool }

/// FOOTPRINT_VARIANT.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FootprintVariant { pub name: String, pub dnp: bool, pub excluded_from_bom: bool,
    pub excluded_from_sim: bool, pub excluded_from_pos_files: bool,
    pub fields: BTreeMap<String, String> }

/// FOOTPRINT::FP_UNIT_INFO.
#[derive(Clone, Debug, PartialEq)]
pub struct FpUnitInfo { pub unit_name: String, pub pins: Vec<String> }

/// FOOTPRINT_COURTYARD_CACHE_DATA. `malformed` carries the MALFORMED_F/B_COURTYARD bits that
/// C++ sets on the footprint from inside the const `GetCourtyard()` (via const_cast); a lazy
/// build under `&self` cannot write `ItemBase::flags`, so the bits travel with the cache and
/// `FootprintRef::courtyard_malformed()` reads them.
#[derive(Clone, Debug, Default)]
pub struct FootprintCourtyardCacheData { pub front: ShapePolySet, pub back: ShapePolySet,
                                         pub front_hash: Hash128, pub back_hash: Hash128,
                                         pub malformed: u32 }
/// Key of the courtyard cache: everything outside the footprint's own children it depends on.
#[derive(Clone, Debug, PartialEq)]
pub struct CourtyardCacheKey { pub xf: TransformTrs, pub max_error: i32, pub chaining_epsilon: i32 }

/// Port of FOOTPRINT (pcbnew/footprint.h).
#[derive(Clone, Debug)]
pub struct Footprint {
    pub item: ItemBase,                                   // m_layer = F_Cu / B_Cu
    // ---- children (private: go through accessors, 2.6) ----
    pub(crate) fields: ItemList<PcbField>,                // m_fields
    pub(crate) drawings: ItemList<Drawing>,               // m_drawings
    pub(crate) pads: ItemList<Pad>,                       // m_pads
    pub(crate) zones: ItemList<Zone>,                     // m_zones
    pub(crate) groups: ItemList<PcbGroup>,                // m_groups
    pub(crate) constraints: ItemList<PcbConstraint>,      // m_constraints
    pub(crate) points: ItemList<PcbPoint>,                // m_points
    pub(crate) bodies: ItemList<Pcb3dBody>,               // m_bodies
    // ---- placement (private: setters propagate to children) ----
    pub(crate) transform: TransformTrs,                   // m_transform
    pub(crate) flipped: bool,                             // m_flipped
    // ---- identity / schematic link ----
    pub fpid: LibId,                                      // m_fpid
    pub attributes: i32,                                  // m_attributes (FP_* bits)
    pub fp_status: i32,                                   // m_fpStatus (FP_is_PLACED, FP_to_PLACE)
    pub file_format_version_at_load: i32,                 // m_fileFormatVersionAtLoad
    pub variants: CaseInsensitiveMap<FootprintVariant>,   // m_variants (CASE_INSENSITIVE_MAP)
    pub net_tie_pad_groups: Vec<String>,                  // m_netTiePadGroups
    pub jumper_pad_groups: BTreeSet<BTreeSet<String>>,    // m_jumperPadGroups (JUMPER_GROUP_SET)
    pub duplicate_pad_numbers_are_jumpers: bool,          // m_duplicatePadNumbersAreJumpers
    pub allow_missing_courtyard: bool,                    // m_allowMissingCourtyard
    pub allow_solder_mask_bridges: bool,                  // m_allowSolderMaskBridges
    pub zone_connection: ZoneConnection,                  // m_zoneConnection
    pub clearance: Option<i32>,                           // m_clearance
    pub solder_mask_margin: Option<i32>,                  // m_solderMaskMargin
    pub solder_paste_margin: Option<i32>,                 // m_solderPasteMargin
    pub solder_paste_margin_ratio: Option<f64>,           // m_solderPasteMarginRatio
    pub stackup_layers: Lset,                             // m_stackupLayers
    pub stackup_mode: FootprintStackup,                   // m_stackupMode
    pub private_layers: Lset,                             // m_privateLayers
    pub lib_description: String,                          // m_libDescription
    pub keywords: String,                                 // m_keywords
    pub path: KiidPath,                                   // m_path
    pub sheetname: String,                                // m_sheetname
    pub sheetfile: String,                                // m_sheetfile
    pub filters: String,                                  // m_filters
    pub last_edit_time: i64,                              // m_lastEditTime (timestamp_t; never read from a clock)
    pub arflag: i32,                                      // m_arflag
    pub link: Kiid,                                       // m_link
    pub models: Vec<Fp3dModel>,                           // m_3D_Drawings
    pub height: i32,                                      // m_height
    pub initial_comments: Option<Vec<String>>,            // m_initial_comments
    pub transient_component_class_names: BTreeSet<String>,// m_transientComponentClassNames
    pub unit_info: Vec<FpUnitInfo>,                       // m_unitInfo
    pub embedded_files: EmbeddedFiles,                    // base class EMBEDDED_FILES
    // ---- component class (COMPONENT_CLASS_CACHE_PROXY) ----
    pub(crate) static_component_class: Option<Arc<ComponentClass>>,          // m_staticComponentClass
    pub(crate) dynamic_component_class: LazyCache<i64, Option<Arc<ComponentClass>>>, // m_dynamicComponentClass + ticker
    // ---- caches (2.7) ----
    pub(crate) bbox_cache: LazyCache<i32, Box2I>,                 // m_geometry_cache->bounding_box (+timestamp)
    pub(crate) text_excluded_bbox_cache: LazyCache<i32, Box2I>,   // ->text_excluded_bbox (+timestamp)
    pub(crate) hull_cache: LazyCache<i32, ShapePolySet>,          // ->hull (+timestamp)
    pub(crate) courtyard_cache: LazyCache<CourtyardCacheKey, FootprintCourtyardCacheData>, // m_courtyard_cache + mutex
    pub(crate) net_tie_cache: BTreeMap<Kiid, BTreeSet<i32>>,      // m_netTieCache (keyed by item pointer in C++)
}
```

Raw-struct API (no board needed): `position()` / `orientation()` / `transform()` / `scale_x()`
(they read `transform`), `is_flipped()`, `side()`, `reference()`, `value()`, `field(FieldT)`,
`field_by_name()`, `next_field_ordinal()`, `pads()`, `find_pad_by_number()`, `pad_count()`,
`unique_pad_numbers()`, `is_net_tie()`, `map_pad_numbers_to_net_tie_groups()`, attribute getters,
variant getters, `add(item, mode)`, `remove(kind, idx)`, the transform mutators
(`set_position`, `set_orientation`, `move_`, `rotate`, `flip(centre, dir, env)`,
`set_layer_and_flip`, `set_transform_scale`, `rescale_around_point`, `move_anchor_position`),
`build_net_tie_cache()`, `invalidate_geometry_caches()`.
View API (needs board): `bounding_box()`, `bounding_box_incl_text(bool)`, `layer_bounding_box(lset)`,
`bounding_hull()`, `courtyard(layer)` (lazy, like the const `GetCourtyard`), `cached_courtyard(layer)`,
`build_courtyard_caches(error_handler)` (explicit rebuild with error reporting, fills the same
cache), `courtyard_malformed()`, `effective_shape`,
`transform_pads_to_poly_set`, `transform_fp_shapes_to_poly_set`, `resolve_text_var`,
`component_class()`, `is_locked()`, `item_description()`, `footprint_needs_update(lib_fp)`.

### 3.6 `Padstack` and `Pad`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum PadShape { Circle, Rectangle, Oval, Trapezoid, Roundrect, ChamferedRect, Custom }       // PAD_SHAPE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadDrillShape { Undefined, Circle, Oblong }               // PAD_DRILL_SHAPE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadDrillPostMachiningMode { Unknown, NotPostMachined, Counterbore, Countersink }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum BackdrillMode { NoBackdrill, BackdrillBottom, BackdrillTop, BackdrillBoth }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadAttrib { Pth, Smd, Conn, Npth, NonComponent }          // PAD_ATTRIB
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PadProp { None, Bga, FiducialGlbl, FiducialLocal, Testpoint, Heatsink, Castellated, Mechanical, Pressfit }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UnconnectedLayerMode { KeepAll, StartEndOnly, RemoveAll, RemoveExceptStartAndEnd }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum CustomShapeZoneMode { Outline, Convexhull }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadstackType { Normal, Via, Mounting }                    // PADSTACK::TYPE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadstackMode { Normal, FrontInnerBack, Custom }           // PADSTACK::MODE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PadSimElectricalType { None, Source, Sink }

/// PADSTACK::SHAPE_PROPS.
#[derive(Clone, Debug, PartialEq)]
pub struct ShapeProps {
    pub shape: PadShape, pub anchor_shape: PadShape, pub size: Vector2I, pub offset: Vector2I,
    pub round_rect_radius_ratio: f64, pub chamfered_rect_ratio: f64,
    pub chamfered_rect_positions: i32,            // RECT_CHAMFER_POSITIONS bit set
    pub trapezoid_delta_size: Vector2I,
}
/// PADSTACK::COPPER_LAYER_PROPS.
#[derive(Clone, Debug, PartialEq)]
pub struct CopperLayerProps {
    pub shape: ShapeProps,
    pub zone_connection: Option<ZoneConnection>,
    pub thermal_spoke_width: Option<i32>,
    pub thermal_spoke_angle: Option<EdaOrientation>,
    pub thermal_gap: Option<i32>,
    pub clearance: Option<i32>,
    pub custom_shapes: Vec<PcbShape>,             // vector<shared_ptr<PCB_SHAPE>>, pad-frame primitives
}
/// PADSTACK::MASK_LAYER_PROPS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MaskLayerProps {
    pub solder_mask_margin: Option<i32>, pub solder_paste_margin: Option<i32>,
    pub solder_paste_margin_ratio: Option<f64>, pub has_solder_mask: Option<bool>,
    pub has_solder_paste: Option<bool>, pub has_covering: Option<bool>, pub has_plugging: Option<bool>,
}
/// PADSTACK::DRILL_PROPS.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillProps { pub size: Vector2I, pub shape: PadDrillShape, pub start: PcbLayerId,
                        pub end: PcbLayerId, pub is_filled: Option<bool>, pub is_capped: Option<bool> }
/// PADSTACK::POST_MACHINING_PROPS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PostMachiningProps { pub mode: Option<PadDrillPostMachiningMode>, pub size: i32,
                                pub depth: i32, pub angle: i32 }

/// Port of PADSTACK (pcbnew/padstack.h).
#[derive(Clone, Debug, PartialEq)]
pub struct Padstack {
    pub mode: PadstackMode,                                   // m_mode
    pub layer_set: Lset,                                      // m_layerSet
    pub custom_name: Option<String>,                          // m_customName
    pub orientation: EdaOrientation,                          // m_orientation (board-frame, baked by the pad)
    pub copper_props: BTreeMap<PcbLayerId, CopperLayerProps>, // m_copperProps (unordered_map)
    pub front_mask_props: MaskLayerProps,                     // m_frontMaskProps
    pub back_mask_props: MaskLayerProps,                      // m_backMaskProps
    pub unconnected_layer_mode: UnconnectedLayerMode,         // m_unconnectedLayerMode
    pub custom_shape_in_zone_mode: CustomShapeZoneMode,       // m_customShapeInZoneMode
    pub drill: DrillProps,                                    // m_drill
    pub secondary_drill: DrillProps,                          // m_secondaryDrill
    pub tertiary_drill: DrillProps,                           // m_tertiaryDrill
    pub front_post_machining: PostMachiningProps,             // m_frontPostMachining
    pub back_post_machining: PostMachiningProps,              // m_backPostMachining
}
impl Padstack {
    pub const ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;
    pub const INNER_LAYERS: PcbLayerId = PcbLayerId::In1Cu;
    // All PADSTACK accessors keep their names: shape(layer), size(layer), offset(layer),
    // copper_layer(layer), effective_layer_for(layer), for_each_unique_layer(f), unique_layers(),
    // start_layer(), end_layer(), drill_shape(), round_rect_radius(layer), chamfer_ratio(layer),
    // clearance(layer), solder_mask_margin(layer), zone_connection(layer), thermal_gap(layer),
    // thermal_spoke_angle(layer), is_tented(side), is_covered(side), is_plugged(side),
    // get_backdrill_mode(), get_backdrill_size(top), get_max_hole_size(), primitives(layer), ...
    // Methods that used m_parent take what they need:
    pub fn flip_layers(&mut self, copper_layer_count: i32);                           // FlipLayers( BOARD* )
    pub fn post_machining_knockout(&self, stackup: &BoardStackup, layer: PcbLayerId) -> i32;
    pub fn machining_knockout(&self, board: &Board, layer: PcbLayerId) -> i32;
}

#[derive(Clone, Debug, PartialEq)]
pub struct PadCacheKey { pub xf: Option<TransformTrs>, pub max_error: i32 }

/// PAD::PAD_DRAW_CACHE_DATA plus the values C++ keeps beside it.
#[derive(Clone, Debug)]
pub struct PadDrawCacheData {
    pub effective_bounding_box: Box2I,                                      // m_effectiveBoundingBox
    pub effective_shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>>,         // m_effectiveShapes
    pub effective_hole_shape: Option<Arc<ShapeSegment>>,                    // m_effectiveHoleShape
    pub effective_polygons: BTreeMap<PcbLayerId, [Arc<ShapePolySet>; 2]>,   // m_effectivePolygons [ERROR_INSIDE, ERROR_OUTSIDE]
    pub effective_bounding_radius: i32,                                     // m_effectiveBoundingRadius
}

/// Port of PAD (pcbnew/pad.h).
#[derive(Clone, Debug)]
pub struct Pad {
    pub item: ItemBase,                                   // m_layer is the principal layer of SMD/CONN pads
    pub conn: ConnectedBase,
    pub number: String,                                   // m_number
    pub pin_function: String,                             // m_pinFunction
    pub pin_type: String,                                 // m_pinType
    pub lib_pos: Vector2I,                                // m_libPos           (footprint library frame)
    pub lib_orientation: EdaOrientation,                  // m_libOrientation   (footprint library frame)
    pub padstack: Padstack,                               // m_padStack         (sizes/offsets/drill in library frame)
    pub attribute: PadAttrib,                             // m_attribute
    pub property: PadProp,                                // m_property
    pub length_pad_to_die: i32,                           // m_lengthPadToDie
    pub delay_pad_to_die: i32,                            // m_delayPadToDie
    pub sim_electrical_type: PadSimElectricalType,        // m_simElectricalType
    pub sub_ratsnest: i32,                                // m_subRatsnest
    pub zone_layer_overrides: BTreeMap<PcbLayerId, ZoneLayerOverride>,   // m_zoneLayerOverrides (class-2)
    pub(crate) draw_cache: LazyCache<PadCacheKey, PadDrawCacheData>,     // m_drawCache, m_shapesDirty, m_polyDirty, m_dataMutex
    pub(crate) mask_shapes: LazyCache<PadCacheKey, BTreeMap<PcbLayerId, (i32, Arc<ShapeCompound>)>>, // m_maskShapes, m_maskShapeMutex
}
```

Raw-struct API: `layer_set()`, `is_on_layer()`, `can_have_number()`, `is_no_connect_pad()`,
`is_aperture_pad()` (`has_hole()` is a view method: the drill size is derived through the
parent transform),
`conditionally_flashed(layer)`, `principal_layer()`, `set_attribute`, `set_property`,
`set_layer_set`, `set_lib_size`, `set_lib_drill_size`, `set_lib_offset`, `set_dirty()`,
`fp_relative_orientation()`, static masks `pth_mask()`, `smd_mask()`, `conn_smd_mask()`,
`unplated_hole_mask()`, `aperture_mask()`, `compare(a, b)` (lib-frame compare used by
`cmp_pads`).
View API (`PadRef`): `position()`, `orientation()`, `size(layer)`, `offset(layer)`,
`drill_size()`, `shape_pos(layer)`, `has_hole()`, `has_drilled_hole()`, `is_flipped()`,
`is_locked()`, `flash_layer(layer)`, `flash_layer_permitted(layer)` (`aOnlyCheckIfPermitted`),
`flash_layer_any(lset)`, `effective_shape`, `effective_polygon(layer, error_loc)`,
`effective_hole_shape`, `bounding_box()`, `bounding_box_on(layer)`, `bounding_radius()`,
`transform_shape_to_polygon`, `transform_hole_to_polygon`, `merge_primitives_as_polygon`,
`own_clearance(layer)`, `local_clearance()`, `clearance_overrides()`,
`solder_mask_expansion(layer)`, `solder_paste_margin(layer)`, `zone_connection_overrides()`,
`local_spoke_width_override()`, `local_thermal_gap_override()`, `zone_layer_override(layer)`,
`same_logical_pad_as(other)`, `shares_net_tie_group(other)`, `is_free_pad()`,
`is_backdrilled_or_post_machined(layer)`, `post_machining_knockout(layer)`,
`is_npth_with_no_copper()`, `item_description()`.

### 3.7 Tracks, arcs, vias

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)] #[repr(i32)]
pub enum Viatype { Through = 4, Buried = 3, Blind = 2, Microvia = 1, NotDefined = 0 }           // VIATYPE
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum TentingMode { FromBoard, Tented, NotTented }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum CoveringMode { FromBoard, Covered, NotCovered }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PluggingMode { FromBoard, Plugged, NotPlugged }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum CappingMode { FromBoard, Capped, NotCapped }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum FillingMode { FromBoard, Filled, NotFilled }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum EndpointT { Start = 0, End = 1 }           // ENDPOINT_T
pub const UNDEFINED_DRILL_DIAMETER: i32 = -1;
pub const MAX_CU_LAYERS: usize = 32;

/// The members PCB_TRACK contributes to PCB_TRACK, PCB_ARC and PCB_VIA.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackBase {
    pub item: ItemBase,
    pub conn: ConnectedBase,
    pub start: Vector2I,                       // m_Start
    pub end: Vector2I,                         // m_End
    pub width: i32,                            // m_width (not the via diameter)
    pub has_solder_mask: bool,                 // m_hasSolderMask
    pub solder_mask_margin: Option<i32>,       // m_solderMaskMargin
}
/// PCB_TRACK (PCB_TRACE_T).
#[derive(Clone, Debug, PartialEq)] pub struct PcbTrack { pub base: TrackBase }
/// PCB_ARC.
#[derive(Clone, Debug, PartialEq)] pub struct PcbArc { pub base: TrackBase, pub mid: Vector2I /* m_Mid */ }
/// PCB_VIA.
#[derive(Clone, Debug, PartialEq)]
pub struct PcbVia {
    pub base: TrackBase,                                           // start == end == position
    pub via_type: Viatype,                                         // m_viaType
    pub padstack: Padstack,                                        // m_padStack
    pub is_free: bool,                                             // m_isFree
    pub zone_layer_overrides: [ZoneLayerOverride; MAX_CU_LAYERS],  // m_zoneLayerOverrides (atomics in C++; class-2)
}
/// Element of BOARD::m_tracks.
#[derive(Clone, Debug, PartialEq)]
pub enum Track { Segment(PcbTrack), Arc(PcbArc), Via(Box<PcbVia>) }
impl Track {
    pub fn base(&self) -> &TrackBase;           pub fn base_mut(&mut self) -> &mut TrackBase;
    pub fn type_(&self) -> KicadT;              // PcbTraceT / PcbArcT / PcbViaT
}
```

Tracks never live in footprints, so almost everything is on the raw structs: `start/end/width`,
`layer_set()`, `is_on_layer()`, `length()`, `is_null()`, `is_point_on_ends()`, `approx_collinear()`,
`move_`, `rotate`, `mirror`, arc `radius()/angle()/arc_angle_start()/is_ccw()/is_degenerated()`,
via `layer_pair()`, `top_layer()`, `bottom_layer()`, `set_layer_pair()`, `sanitize_layers()`,
`width_on(layer)`, `drill()`, `has_valid_layer_pair(copper_layer_count)`, tenting/covering/
plugging/capping/filling mode getters and setters, backdrill and post-machining accessors,
`zone_layer_override(layer)`, `conditionally_flashed(layer)`, `bounding_box()`,
`effective_shape_always_flashed(layer)` helpers.
View API (needs board/connectivity/rules): `flash_layer(layer)` (via), `effective_shape` (via,
because of `FlashLayer`), `drill_value()` (netclass fallback), `resolve_front_tenting_mode()` &
co. (board defaults), `is_tented(layer)`, `solder_mask_expansion()`, `width_constraint()`,
`drill_constraint()`, `min_annulus(layer)`, `flip(centre, dir)` (board layer count),
`delay()`, `own_clearance(layer)`, `item_description()`, `is_locked()`.

### 3.8 `Zone`

```rust
/// The fill results the zone filler writes and the file stores (class-2 data, 2.7).
#[derive(Clone, Debug, Default)]
pub struct ZoneFill {
    pub filled_polys_list: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,   // m_FilledPolysList
    pub fill_flags: Lset,                                             // m_fillFlags
    pub filled_polys_hash: BTreeMap<PcbLayerId, Hash128>,             // m_filledPolysHash
    pub insulated_islands: BTreeMap<PcbLayerId, BTreeSet<i32>>,       // m_insulatedIslands
    pub is_filled: bool,                                              // m_isFilled
    pub need_refill: bool,                                            // m_needRefill (atomic in C++)
    pub area: f64,                                                    // m_area
    pub outline_area: f64,                                            // m_outlinearea
}

/// Port of ZONE (pcbnew/zone.h).
#[derive(Clone, Debug)]
pub struct Zone {
    pub item: ItemBase,
    pub conn: ConnectedBase,
    pub poly: ShapePolySet,                                // m_Poly (library frame when owned by a footprint)
    pub corner_smoothing_type: CornerSmoothing,            // m_cornerSmoothingType
    pub corner_radius: u32,                                // m_cornerRadius
    pub zone_name: String,                                 // m_zoneName
    pub layer_set: Lset,                                   // m_layerSet (+ m_layerSetMutex)
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>, // m_layerProperties
    pub priority: u32,                                     // m_priority
    pub is_rule_area: bool,                                // m_isRuleArea
    pub placement_area_enabled: bool,                      // m_placementAreaEnabled
    pub placement_area_source_type: PlacementSourceT,      // m_placementAreaSourceType
    pub placement_area_source: String,                     // m_placementAreaSource
    pub link_set: Kiid,                                    // m_linkSet
    pub repeat_options: RepeatLayoutOptions,               // m_repeatOptions
    pub teardrop_type: TeardropType,                       // m_teardropType
    pub do_not_allow_zone_fills: bool,                     // m_doNotAllowZoneFills
    pub do_not_allow_vias: bool,                           // m_doNotAllowVias
    pub do_not_allow_tracks: bool,                         // m_doNotAllowTracks
    pub do_not_allow_pads: bool,                           // m_doNotAllowPads
    pub do_not_allow_footprints: bool,                     // m_doNotAllowFootprints
    pub pad_connection: ZoneConnection,                    // m_PadConnection
    pub zone_clearance: i32,                               // m_ZoneClearance
    pub zone_min_thickness: i32,                           // m_ZoneMinThickness
    pub fill_version: i32,                                 // m_fillVersion
    pub island_removal_mode: IslandRemovalMode,            // m_islandRemovalMode
    pub min_island_area: i64,                              // m_minIslandArea
    pub thermal_relief_gap: i32,                           // m_thermalReliefGap
    pub thermal_relief_spoke_width: i32,                   // m_thermalReliefSpokeWidth
    pub fill_mode: ZoneFillMode,                           // m_fillMode
    pub hatch_thickness: i32,                              // m_hatchThickness
    pub hatch_gap: i32,                                    // m_hatchGap
    pub hatch_orientation: EdaOrientation,                 // m_hatchOrientation
    pub hatch_smoothing_level: i32,                        // m_hatchSmoothingLevel
    pub hatch_smoothing_value: f64,                        // m_hatchSmoothingValue
    pub hatch_hole_min_area: f64,                          // m_hatchHoleMinArea
    pub hatch_border_algorithm: i32,                       // m_hatchBorderAlgorithm
    pub thieving_settings: ThievingSettings,               // m_thievingSettings
    pub local_flgs: i32,                                   // m_localFlgs
    pub border_style: ZoneBorderDisplayStyle,              // m_borderStyle
    pub border_hatch_pitch: i32,                           // m_borderHatchPitch
    pub border_hatch_lines: Vec<Seg>,                      // m_borderHatchLines
    pub fill: ZoneFill,                                    // the class-2 members listed above
    pub(crate) bbox_cache: LazyCache<(i32, Option<TransformTrs>), Box2I>,   // m_bboxCache, m_bboxCacheTimeStamp, BOARD::m_ZoneBBoxCache
}
```

Raw-struct API: `library_outline()`, layer queries (`layer_set()`, `is_on_layer`, `first_layer()`,
`layer()`), `assigned_priority()`, keepout getters, `has_keepout_parameters_set()`,
`is_teardrop_area()`, `is_copper_thieving()`, corner/vertex editing (`append_corner`, `new_hole`,
`add_polygon`, `remove_cutout`, `corner_position`), the setters that flag `need_refill`
(`set_min_thickness`, `set_thermal_relief_gap`, ...), fill storage (`has_filled_polys_for_layer`,
`filled_polys_list(layer)`, `set_filled_polys_list`, `is_island`, `set_is_island`,
`build_hash_value`, `hash_value`, `un_fill`, `calculate_filled_area`, `calculate_outline_area`),
`hatch_border()/un_hatch_border()/hatch_lines()`, `local_clearance()`, `higher_priority(other)`
and `same_net(other)` (both only compare own data).
View API: `board_outline()` (applies the parent transform; expensive, as in C++),
`bounding_box()`, `position()`, `effective_shape`, `build_smoothed_poly(layer, board_outline)`,
`transform_smoothed_outline_to_polygon`, `transform_solid_areas_shapes_to_polygon`,
`transform_shape_to_polygon`, `interacting_zones(layer)`, `thermal_relief_gap_for(pad)`,
`thermal_spoke_width_for(pad)`, `pad_connection_for(pad)`, `own_clearance(layer)`,
`is_on_copper_layer()`, `is_conflicting()`, `item_description()`.

### 3.9 `EdaShape` and `PcbShape`

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)] #[repr(i32)]
pub enum ShapeT { Undefined = -1, Segment = 0, Rectangle, Arc, Circle, Poly, Bezier, Ellipse, EllipseArc }   // SHAPE_T
#[derive(Copy, Clone, Debug, PartialEq, Eq)] #[repr(i32)]
pub enum FillT { NoFill = 1, FilledShape, FilledWithBgBodycolor, FilledWithColor, Hatch, ReverseHatch, CrossHatch } // FILL_T

/// ARC_MID: originating mid point data of an arc.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcMid { pub mid: Vector2I, pub start: Vector2I, pub end: Vector2I, pub center: Vector2I }

#[derive(Clone, Debug, Default)]
pub struct EdaShapeHatchCacheData { pub hatching: ShapePolySet, pub hatch_lines: Vec<Seg> }

/// Port of EDA_SHAPE (include/eda_shape.h). In a PcbShape these are the *board-frame* values.
#[derive(Clone, Debug)]
pub struct EdaShape {
    pub ends_swapped: bool,                    // m_endsSwapped
    pub shape: ShapeT,                         // m_shape
    pub stroke: StrokeParams,                  // m_stroke (width, line style, colour)
    pub start_ending: LineEnding,              // m_startEnding
    pub end_ending: LineEnding,                // m_endEnding
    pub fill: FillT,                           // m_fill
    pub fill_color: Color4D,                   // m_fillColor
    pub rectangle_height: i64,                 // m_rectangleHeight
    pub rectangle_width: i64,                  // m_rectangleWidth
    pub corner_radius: i32,                    // m_cornerRadius
    pub start: Vector2I,                       // m_start   (line start or circle centre)
    pub end: Vector2I,                         // m_end     (line end or circle 3 o'clock point)
    pub arc_center: Vector2I,                  // m_arcCenter
    pub arc_mid_data: ArcMid,                  // m_arcMidData
    pub bezier_c1: Vector2I,                   // m_bezierC1
    pub bezier_c2: Vector2I,                   // m_bezierC2
    pub bezier_points: Vec<Vector2I>,          // m_bezierPoints
    pub ellipse: Ellipse<i32>,                 // m_ellipse (ELLIPSE / ELLIPSE_ARC)
    pub poly: Option<Box<ShapePolySet>>,       // m_poly (mutable unique_ptr, lazily created)
    pub edit_state: i32,                       // m_editState
    pub proxy_item: bool,                      // m_proxyItem
    pub(crate) hatching_cache: LazyCache<(), EdaShapeHatchCacheData>,   // m_hatchingCache, m_hatchingDirty
}
// dropped: m_blankEnding (a static blank value returned for closed shapes).

/// Port of PCB_SHAPE (pcbnew/pcb_shape.h): BOARD_CONNECTED_ITEM + EDA_SHAPE + library-frame copy.
#[derive(Clone, Debug)]
pub struct PcbShape {
    pub item: ItemBase,
    pub conn: ConnectedBase,
    pub shape: EdaShape,                               // base class EDA_SHAPE (board frame, baked)
    pub has_solder_mask: bool,                         // m_hasSolderMask
    pub solder_mask_margin: Option<i32>,               // m_solderMaskMargin
    pub lib_start: Vector2I,                           // m_libStart
    pub lib_end: Vector2I,                             // m_libEnd
    pub lib_corner_radius: i32,                        // m_libCornerRadius
    pub lib_arc_mid: Vector2I,                         // m_libArcMid
    pub lib_bezier_c1: Vector2I,                       // m_libBezierC1
    pub lib_bezier_c2: Vector2I,                       // m_libBezierC2
    pub lib_poly: Option<Box<ShapePolySet>>,           // m_libPoly
    pub lib_ellipse_center: Vector2I,                  // m_libEllipseCenter
    pub lib_ellipse_major_radius: i32,                 // m_libEllipseMajorRadius
    pub lib_ellipse_minor_radius: i32,                 // m_libEllipseMinorRadius
    pub lib_ellipse_rotation: EdaAngle,                // m_libEllipseRotation
    pub lib_ellipse_start_angle: EdaAngle,             // m_libEllipseStartAngle
    pub lib_ellipse_end_angle: EdaAngle,               // m_libEllipseEndAngle
    pub lib_shape: ShapeT,                             // m_libShape
}
impl PcbShape {
    /// PCB_SHAPE::RebakeFromLib / rebakeFromTransform: recompute `shape` from the lib_* fields.
    pub fn rebake_from_transform(&mut self, xf: Option<&TransformTrs>);
    /// PCB_SHAPE::syncLibCoords: inverse, after a board-frame edit.
    pub fn sync_lib_coords(&mut self, xf: Option<&TransformTrs>);
}
```

Because the baked copy exists, a `PcbShape` answers board-frame geometry (`start()`, `end()`,
`center()`, `bounding_box()`, `corners()`, `connection_points()`, `effective_shape`,
`transform_shape_to_polygon(.., max_error)`) from the raw struct; the view is only needed for
`is_locked()`, `max_error()` defaulting, `solder_mask_expansion()` (rule engine, board default),
`own_clearance`, hatching knockouts (`getHatchingKnockouts` looks at neighbours) and
`item_description()`. Board-frame setters (`set_start`, `set_end`, `set_center`, `set_poly_shape`,
...) are on `ViewMut<PcbShape>` because they end in `sync_lib_coords(xf)`.

### 3.10 Text family and tables

```rust
/// Port of EDA_TEXT (include/eda_text.h).
#[derive(Clone, Debug)]
pub struct EdaText {
    pub hyperlink: String,                     // m_hyperlink
    pub(crate) text: String,                   // m_text                  // private: shown_text cache
    pub(crate) shown_text: String,             // m_shown_text
    pub(crate) shown_text_has_text_var_refs: bool, // m_shown_text_has_text_var_refs
    pub(crate) text_var_refs: Vec<TextVarRefKey>,  // m_text_var_refs
    pub attributes: TextAttributes,            // m_attributes (font, h/v align, angle, line spacing, stroke width,
                                               //   italic, bold, underlined, hover, colour, mirrored, multiline,
                                               //   size, keep upright) - kicad-core type
    pub unresolved_font_name: String,          // m_unresolvedFontName
    pub pos: Vector2I,                         // m_pos (library frame in a footprint child)
    pub visible: bool,                         // m_visible
    pub(crate) render_cache: LazyCache<RenderCacheKey, EdaTextRenderCacheData>, // m_render_cache
    pub(crate) bbox_cache: LazyCache<BboxCacheKey, BTreeMap<i32, Box2I>>,       // m_bbox_cache + mutex
}
// dropped: m_activeUrl (GUI hover), m_IuScale (always pcbIUScale in this port).

/// Port of PCB_TEXT (BOARD_ITEM + EDA_TEXT).
#[derive(Clone, Debug)]
pub struct PcbText {
    pub item: ItemBase,
    pub text: EdaText,                         // base class EDA_TEXT
    pub lib_text_angle: EdaOrientation,        // m_libTextAngle
    pub(crate) knockout_cache: LazyCache<KnockoutCacheKey, ShapePolySet>,   // m_knockout_cache
}

/// FIELD_T (include/template_fieldnames.h).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldT { User, Reference, Value, Footprint, Datasheet, Description, IntersheetRefs,
                  SheetName, SheetFilename, SheetUser, SheetRefSuffix }

/// Port of PCB_FIELD (PCB_TEXT + field identity).
#[derive(Clone, Debug)]
pub struct PcbField {
    pub text: PcbText,                         // base class PCB_TEXT
    pub id: FieldT,                            // m_id
    pub ordinal: i32,                          // m_ordinal
    pub name: String,                          // m_name
    pub private: bool,                         // m_private
}

/// Port of PCB_TEXTBOX (PCB_SHAPE + EDA_TEXT).
#[derive(Clone, Debug)]
pub struct PcbTextbox {
    pub shape: PcbShape,                       // base class PCB_SHAPE (owns ItemBase/ConnectedBase)
    pub text: EdaText,                         // base class EDA_TEXT
    pub border_enabled: bool,                  // m_borderEnabled
    pub margin_left: i32, pub margin_top: i32, pub margin_right: i32, pub margin_bottom: i32, // m_margin*
    pub lib_text_angle: EdaOrientation,        // m_libTextAngle
}

/// Port of PCB_TABLECELL (PCB_TEXTBOX + spans).
#[derive(Clone, Debug)]
pub struct PcbTablecell { pub textbox: PcbTextbox, pub col_span: i32, pub row_span: i32 }

/// Port of PCB_TABLE, including the PCB_GENERATED_TABLE subclasses.
#[derive(Clone, Debug)]
pub struct PcbTable {
    pub item: ItemBase,
    pub stroke_external: bool,                 // m_strokeExternal
    pub stroke_header_separator: bool,         // m_StrokeHeaderSeparator
    pub border_stroke: StrokeParams,           // m_borderStroke
    pub stroke_rows: bool,                     // m_strokeRows
    pub stroke_columns: bool,                  // m_strokeColumns
    pub separators_stroke: StrokeParams,       // m_separatorsStroke
    pub col_count: i32,                        // m_colCount
    pub col_widths: BTreeMap<i32, i32>,        // m_colWidths
    pub row_heights: BTreeMap<i32, i32>,       // m_rowHeights
    pub cells: Vec<PcbTablecell>,              // m_cells
    pub generated: Option<Box<GeneratedTable>>,// Some for PCB_DRILL_CHART_T / PCB_STACKUP_TABLE_T
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum GeneratedTableAlign { Left, Center, Right }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum GeneratedTableUnits { Mm, Inch, Mils }
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTableColumn { pub id: i32, pub heading: String, pub align: GeneratedTableAlign,
                                  pub width: i32, pub visible: bool }
#[derive(Clone, Debug, PartialEq)]
pub struct DrillChartFilter { pub plated: bool, pub non_plated: bool, pub vias: bool, pub slots: bool,
                              pub backdrills: bool, pub castellated: bool }
/// Members of PCB_GENERATED_TABLE and its two subclasses. Data only: the tables are not
/// regenerated by this port (see 3.16).
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratedTable {
    pub kind: GeneratedTableKind,                          // DrillChart | StackupTable  (-> KicadT)
    pub columns: Vec<GeneratedTableColumn>,                // m_columns
    pub units: GeneratedTableUnits,                        // m_units
    pub precision: i32,                                    // m_precision
    pub row_keys: BTreeMap<i32, String>,                   // m_rowKeys
    pub hidden: Vec<(GeneratedTableColumn, i32)>,          // m_hidden (HIDDEN_COLUMN{ m_Column, m_Position })
    // PCB_DRILL_CHART
    pub filter: DrillChartFilter,                          // m_filter
    pub show_totals: bool,                                 // m_showTotals
    pub row_shapes: BTreeMap<i32, i32>,                    // m_rowShapes
    pub symbol_column: i32,                                // m_symbolColumn
}
```

Frames: `PcbText`/`PcbField` store `text.pos`, `text.attributes.size`, `stroke_width` and
`lib_text_angle` in the library frame; `text.attributes.angle` holds the baked board angle
(`PCB_TEXT::OnFootprintTransformed` does `SetTextAngle( GetTextAngle() )`). So `text_pos()`,
`text_size()`, `text_thickness()`, `text_angle()`, `draw_rotation()`, `bounding_box()`,
`shown_text(context, depth)`, `effective_shape`, `transform_text_to_poly_set`,
`knockout_cache(font, text, max_error)` are view methods, as in fact 1. A `PcbTextbox` gets its
box geometry from the baked `PcbShape` and its text placement from the view.

### 3.11 Dimensions

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimUnitsFormat { NoSuffix, BareSuffix, ParenSuffix }
/// DIM_PRECISION. The C++ spellings are kept verbatim (they are pictures of the format:
/// "0.00", "0.000 / 0 / 0.00"), which is the one place the UpperCamelCase rule would destroy
/// the meaning.
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DimPrecision { X, X_X, X_XX, X_XXX, X_XXXX, X_XXXXX, V_VV, V_VVV, V_VVVV, V_VVVVV }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimTextPosition { Outside, Inline, Manual }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimUnitsMode { Inch, Mils, Mm, Automatic }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimArrowDirection { Inward, Outward }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimTextBorder { None, Rectangle, Circle, Roundrect }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum DimOrthogonalDir { Horizontal, Vertical }   // PCB_DIM_ORTHOGONAL::DIR

/// PCB_DIMENSION_BASE members (it derives from PCB_TEXT).
#[derive(Clone, Debug)]
pub struct PcbDimensionBase {
    pub text: PcbText,                         // base class PCB_TEXT (owns ItemBase)
    pub override_text_enabled: bool,           // m_overrideTextEnabled
    pub value_string: String,                  // m_valueString
    pub prefix: String,                        // m_prefix
    pub suffix: String,                        // m_suffix
    pub units: EdaUnits,                       // m_units
    pub auto_units: bool,                      // m_autoUnits
    pub units_format: DimUnitsFormat,          // m_unitsFormat
    pub arrow_direction: DimArrowDirection,    // m_arrowDirection
    pub precision: DimPrecision,               // m_precision
    pub suppress_zeroes: bool,                 // m_suppressZeroes
    pub line_thickness: i32,                   // m_lineThickness
    pub arrow_length: i32,                     // m_arrowLength
    pub extension_offset: i32,                 // m_extensionOffset
    pub text_position: DimTextPosition,        // m_textPosition
    pub keep_text_aligned: bool,               // m_keepTextAligned
    pub measured_value: i32,                   // m_measuredValue
    pub start: Vector2I,                       // m_start (footprint-relative in a footprint)
    pub end: Vector2I,                         // m_end
    pub shapes: Vec<Arc<dyn Shape>>,           // m_shapes (recomputed by update())
}
// dropped: m_inClearRenderCache, m_busy (re-entrancy guards of the GUI update path).

#[derive(Clone, Debug)]
pub enum DimensionKind {
    /// PCB_DIM_ALIGNED
    Aligned { height: i32, extension_height: i32, cross_bar_start: Vector2I, cross_bar_end: Vector2I },
    /// PCB_DIM_ORTHOGONAL (derives from PCB_DIM_ALIGNED)
    Orthogonal { height: i32, extension_height: i32, cross_bar_start: Vector2I, cross_bar_end: Vector2I,
                 orientation: DimOrthogonalDir },
    /// PCB_DIM_RADIAL
    Radial { leader_length: i32 },
    /// PCB_DIM_LEADER
    Leader { text_border: DimTextBorder },
    /// PCB_DIM_CENTER
    Center,
}

/// One struct for PCB_DIM_ALIGNED_T / ORTHOGONAL_T / RADIAL_T / LEADER_T / CENTER_T.
#[derive(Clone, Debug)]
pub struct PcbDimension { pub base: PcbDimensionBase, pub kind: DimensionKind }
```

### 3.12 Groups and generators

```rust
/// Port of PCB_GROUP (BOARD_ITEM + EDA_GROUP).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbGroup {
    pub item: ItemBase,
    pub name: String,                          // EDA_GROUP::m_name
    pub(crate) items: BTreeSet<Kiid>,          // EDA_GROUP::m_items (unordered_set<EDA_ITEM*>)  // private: ItemBase::group
    pub design_block_lib_id: LibId,            // EDA_GROUP::m_designBlockLibId
}
// dropped: m_deserializedItems (protobuf staging).

/// Value of one generator property (the wxAny alternatives the writer distinguishes).
#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorValue {
    Number(f64),                   // double / int / long / long long, written with "{:.10g}"
    Bool(bool),
    Vector(Vector2I),              // (key (xy x y))
    LineChain(ShapeLineChain),     // (key (pts ...))
    Cells(Vec<Vector2I>),          // (key (cells (ij i j) ...))
    Text(String),                  // wxString / std::string
}

/// Port of PCB_GENERATOR / PCB_GENERATOR_POLY and its registered kinds (tuning_pattern,
/// via_stack, via_stitch) as data. The typed members of the C++ subclasses are exactly what
/// their GetProperties() publishes; this port keeps the published map.
#[derive(Clone, Debug, PartialEq)]
pub struct PcbGenerator {
    pub group: PcbGroup,                                   // base class PCB_GROUP (members = generated tracks/vias)
    pub generator_type: String,                            // m_generatorType
    pub origin: Vector2I,                                  // m_origin (also published as property "origin")
    pub properties: BTreeMap<String, GeneratorValue>,      // GetProperties() / SetProperties()
    pub templates: Vec<(String, Item)>,                    // GetTemplateItems() (via_stitch template items)
    pub outline: Option<ShapePolySet>,                     // PCB_GENERATOR_POLY::m_outline
}
// dropped: m_isDirty, m_updateOrder (GENERATOR_ORDER builds only).
```

### 3.13 Remaining item types

```rust
/// Port of PCB_REFERENCE_IMAGE + REFERENCE_IMAGE + BITMAP_BASE (data).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbReferenceImage {
    pub item: ItemBase,
    pub pos: Vector2I,                         // REFERENCE_IMAGE::m_pos
    pub transform_origin_offset: Vector2I,     // m_transformOriginOffset
    pub orientation: EdaOrientation,           // m_orientation
    pub opacity: f64,                          // m_opacity
    pub scale: f64,                            // BITMAP_BASE::m_scale
    pub image_type: BitmapType,                // BITMAP_BASE::m_imageType (Png, Jpeg, ...)
    pub image_data: Vec<u8>,                   // BITMAP_BASE::m_imageData (encoded bytes, never decoded to pixels)
    pub image_id: Kiid,                        // BITMAP_BASE::m_imageId
    pub is_mirrored_x: bool, pub is_mirrored_y: bool, pub rotation: EdaAngle,   // BITMAP_BASE mirror/rotation
    pub pixel_size: Option<Vector2I>,          // width/height in pixels read from the PNG/JPEG header
    pub ppi: i32,                              // BITMAP_BASE::m_ppi (from the header, default 300)
}
// dropped: wxImage m_image / m_originalImage / wxBitmap m_bitmap / m_previewSave (pixels, GUI).

#[derive(Copy, Clone, Debug, PartialEq, Eq)] #[repr(i32)]
pub enum BarcodeT { Code39 = 0, Code128 = 1, DataMatrix = 2, QrCode = 3, MicroQrCode = 4 }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] #[repr(i32)]
pub enum BarcodeEccT { L = 1, M = 2, Q = 3, H = 4 }
#[derive(Clone, Debug, Default)]
pub struct PcbBarcodeCache { pub poly: ShapePolySet, pub symbol_poly: ShapePolySet,
                             pub text_poly: ShapePolySet, pub bbox: Box2I, pub last_error: String }
/// Port of PCB_BARCODE.
#[derive(Clone, Debug)]
pub struct PcbBarcode {
    pub item: ItemBase,
    pub width: i32,                            // m_width
    pub height: i32,                           // m_height
    pub lib_pos: Vector2I,                     // m_libPos
    pub margin: Vector2I,                      // m_margin
    pub text: PcbText,                         // m_text (owned helper item, no own uuid in the file)
    pub kind: BarcodeT,                        // m_kind
    pub lib_angle: EdaOrientation,             // m_libAngle
    pub error_correction: BarcodeEccT,         // m_errorCorrection
    pub(crate) cache: LazyCache<u64, PcbBarcodeCache>,   // m_cache (keyHash = computeCacheKey())
}

/// Port of PCB_POINT.
#[derive(Clone, Debug, PartialEq)]
pub struct PcbPoint { pub item: ItemBase, pub lib_pos: Vector2I /* m_libPos */,
                      pub pos: Vector2I /* m_pos, baked */, pub size: i32 /* m_size */ }

/// Port of PCB_TARGET (legacy).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbTarget { pub item: ItemBase, pub shape: i32, pub size: i32, pub line_width: i32, pub pos: Vector2I }

/// MARKER_BASE::MARKER_T.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MarkerT { Unspec, Erc, Drc, DrawingSheet, Ratsnest, Parity, Simulation }

/// RC_ITEM + DRC_ITEM as data. The rule/test-provider pointers become names.
#[derive(Clone, Debug, PartialEq)]
pub struct RcItem {
    pub error_code: i32,                       // m_errorCode (DRCErrorCode)
    pub error_message: String,                 // m_errorMessage
    pub error_title: String,                   // m_errorTitle
    pub settings_key: String,                  // m_settingsKey (error type key used in reports/settings)
    pub ids: Vec<Kiid>,                        // m_ids (main, aux, aux2, aux3)
    pub violating_rule: Option<String>,        // DRC_ITEM::m_violatingRule -> rule name
    pub violating_rule_implicit: bool,         //   and whether that rule is implicit (report text)
    pub violating_test: Option<String>,        // DRC_ITEM::m_violatingTest -> provider name
}
/// Port of PCB_MARKER (BOARD_ITEM + MARKER_BASE).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbMarker {
    pub item: ItemBase,                        // m_layer = marker layer (F_Cu default)
    pub pos: Vector2I,                         // MARKER_BASE::m_Pos
    pub marker_type: MarkerT,                  // m_markerType
    pub excluded: bool,                        // m_excluded
    pub comment: String,                       // m_comment
    pub rc_item: RcItem,                       // m_rcItem (shared_ptr<RC_ITEM>)
    pub path_shapes: Vec<PcbShape>,            // m_pathShapes
    pub path_start: Vector2I, pub path_end: Vector2I, pub path_length: i32,   // m_pathStart/End/Length
}
// dropped: m_scalingFactor, m_shapeBoundingBox (marker glyph drawing).

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ExtrusionMaterial { Plastic, Matte, Metal, Copper }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum Body3dKind { Extruded, Model }
/// EXTRUDED_3D_BODY.
#[derive(Clone, Debug, PartialEq)]
pub struct Extruded3dBody { pub height: i32, pub standoff: i32, pub layer: PcbLayerId, pub color: Color4D,
    pub material: ExtrusionMaterial, pub show: bool, pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D }
/// Port of PCB_3D_BODY (footprint child).
#[derive(Clone, Debug, PartialEq)]
pub struct Pcb3dBody { pub item: ItemBase, pub kind: Body3dKind, pub extruded: Extruded3dBody }

#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum PcbGridType { Cartesian, Polar }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PcbGridAffects { pub cursor: bool, pub routing: bool, pub placement: bool }
/// Port of PCB_GRID_ITEM (layer-free drawing).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbGridItem {
    pub item: ItemBase,
    pub type_: PcbGridType,                    // m_type
    pub pos: Vector2I,                         // m_pos
    pub extent: Vector2I,                      // m_extent
    pub spacing: Vector2I,                     // m_spacing
    pub orientation: EdaOrientation,           // m_orientation
    pub phi_extent: EdaAngle,                  // m_phiExtent
    pub phi_spacing: EdaAngle,                 // m_phiSpacing
    pub color: Color4D,                        // m_color
    pub priority: u32,                         // m_priority
    pub tick_interval: u32,                    // m_tickInterval
    pub affects: PcbGridAffects,               // m_affects
}

/// DRILL_SPAN (pcbnew/drill/drill_span.h).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DrillSpan { pub start_layer: PcbLayerId, pub end_layer: PcbLayerId,
                       pub is_backdrill: bool, pub is_non_plated_file: bool }
/// Port of PCB_DRILL_MAP (configuration item: turns on drill symbols for one layer).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbDrillMap {
    pub item: ItemBase,
    pub offset: Vector2I,                      // m_offset
    pub symbol_size: i32,                      // m_symbolSize
    pub span: DrillSpan,                       // m_span
    pub all_spans: bool,                       // m_allSpans
    pub outline_slots: bool,                   // m_outlineSlots
    pub guide_cross: bool,                     // m_guideCross
}
// dropped: m_outlineCache, m_outlineCacheOffset, m_outlineCacheGeneration, m_outlineCacheMutex.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PcbConstraintType { Undefined, Coincident, Horizontal, Vertical, Parallel, Perpendicular,
    Collinear, Symmetric, EqualLength, EqualRadius, PointOnLine, Midpoint, FixedPosition,
    FixedLength, Concentric, FixedRadius, AngularDimension, Tangent, ArcAngle }
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConstraintAnchor { Whole, Start, End, Mid, Center, Radius, Vertex }
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintMember { pub item: Kiid, pub anchor: ConstraintAnchor, pub index: i32 }
/// Port of PCB_CONSTRAINT (geometric constraint between items, by UUID).
#[derive(Clone, Debug, PartialEq)]
pub struct PcbConstraint {
    pub item: ItemBase,
    pub type_: PcbConstraintType,              // m_type
    pub members: Vec<ConstraintMember>,        // m_members
    pub value: Option<f64>,                    // m_value (IU for lengths, degrees for angles)
    pub driving: bool,                         // m_driving
}
```

### 3.14 Component classes and embedded files

```rust
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ComponentClassUsage { Static, Dynamic, StaticAndDynamic, Effective }       // COMPONENT_CLASS::USAGE
/// COMPONENT_CLASS.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentClass { pub name: String, pub constituent_classes: Vec<Arc<ComponentClass>>,
                            pub usage_context: ComponentClassUsage }
/// COMPONENT_CLASS_MANAGER (owned by Board).
#[derive(Clone, Debug)]
pub struct ComponentClassManager {
    pub none_class: Arc<ComponentClass>,                                   // m_noneClass
    pub constituent_classes: BTreeMap<String, Arc<ComponentClass>>,        // m_constituentClasses
    pub effective_classes: BTreeMap<String, Arc<ComponentClass>>,          // m_effectiveClasses
    pub static_class_names_cache: BTreeSet<String>,                        // m_staticClassNamesCache
    pub assignment_rules: Vec<ComponentClassAssignmentRule>,               // m_assignmentRules
    pub ticker: i64,                                                       // m_ticker
}
/// COMPONENT_CLASS_ASSIGNMENT_DATA (project settings) and the compiled rule.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ConditionType { Reference, Footprint, Side, Rotation, FootprintField, Custom, SheetName }
#[derive(Copy, Clone, Debug, PartialEq, Eq)] pub enum ConditionsOperator { All, Any }
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentClassAssignmentData { pub component_class: String,
    pub conditions: Vec<(ConditionType, String, String)>, pub conditions_operator: ConditionsOperator }
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComponentClassSettings { pub enable_sheet_component_classes: bool,
                                    pub component_class_assignments: Vec<ComponentClassAssignmentData> }
/// COMPONENT_CLASS_ASSIGNMENT_RULE: class name + condition in DRC-rule language. Evaluating it
/// needs kicad-libeval, so the evaluator is injected (tier 2); the model stores the text.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentClassAssignmentRule { pub component_class: String, pub condition: String }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum EmbeddedFileType { Font, Model, Worksheet, Datasheet, Other }     // EMBEDDED_FILE::FILE_TYPE
/// EMBEDDED_FILES::EMBEDDED_FILE.
#[derive(Clone, Debug, PartialEq)]
pub struct EmbeddedFile {
    pub name: String, pub type_: EmbeddedFileType, pub is_valid: bool,
    pub compressed_encoded_data: String,       // kept verbatim -> byte-exact re-save without re-compressing
    pub decompressed_data: Vec<u8>,            // filled on demand
    pub data_hash: String,
}
/// EMBEDDED_FILES.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmbeddedFiles {
    pub files: BTreeMap<String, EmbeddedFile>, // m_files
    pub font_files: Vec<String>,               // m_fontFiles
    pub embed_fonts: bool,                     // m_embedFonts
}
// dropped: m_fileAddedCallback.
```

### 3.15 `Drawing` and `OpaqueItem`

```rust
/// Element of BOARD::m_drawings and FOOTPRINT::m_drawings (deque<BOARD_ITEM*>).
#[derive(Clone, Debug)]
pub enum Drawing {
    Shape(PcbShape),                           // PCB_SHAPE_T
    Text(PcbText),                             // PCB_TEXT_T
    Textbox(Box<PcbTextbox>),                  // PCB_TEXTBOX_T
    Table(Box<PcbTable>),                      // PCB_TABLE_T, PCB_DRILL_CHART_T, PCB_STACKUP_TABLE_T
    Dimension(Box<PcbDimension>),              // PCB_DIM_*_T
    Barcode(Box<PcbBarcode>),                  // PCB_BARCODE_T
    ReferenceImage(Box<PcbReferenceImage>),    // PCB_REFERENCE_IMAGE_T
    Target(PcbTarget),                         // PCB_TARGET_T          (board only)
    DrillMap(PcbDrillMap),                     // PCB_DRILL_MAP_T       (board only)
    GridItem(Box<PcbGridItem>),                // PCB_GRID_ITEM_T       (board only)
    Opaque(Box<OpaqueItem>),                   // bring-up escape hatch, see 3.16
}
impl Drawing { pub fn base(&self) -> &ItemBase; pub fn type_(&self) -> KicadT; }

/// An item kept as the s-expression it was read from. Only what the writer's sort and the
/// UUID index need is extracted.
#[derive(Clone, Debug)]
pub struct OpaqueItem {
    pub item: ItemBase,            // uuid, layer (UNDEFINED_LAYER if none), locked
    pub kicad_t: KicadT,           // the type the C++ parser would have created
    pub sexpr: Sexpr,              // verbatim subtree (token spelling preserved)
}
```

### 3.16 Modelling level per type (full / data / opaque)

Levels: **F** = all data plus geometry behaviour (bounding box, effective shape, polygon
conversion, comparators); **D** = all persisted data typed and round-tripped, geometry behaviour
missing or reduced as stated; **O** = `OpaqueItem`.

| type | level | re-save | plotting / fabrication | DRC / connectivity |
|---|---|---|---|---|
| `Footprint`, `Pad`, `Padstack`, `PcbTrack`, `PcbArc`, `PcbVia`, `Zone`, `PcbShape`, `PcbText`, `PcbField`, `PcbTextbox`, `PcbTable`/`PcbTablecell`, `PcbDimension` (5 kinds), `PcbGroup`, nets, netclasses, design settings, stackup, plot params, variants, properties, title block | **F** | byte-exact (writer WP) | complete | complete |
| `PcbTarget` (legacy), `PcbPoint` | **F** | byte-exact | target plotted like C++; points have no artwork | ignored like C++ |
| `PcbGenerator` (`tuning_pattern`, `via_stack`, `via_stitch`) | **D** | byte-exact for files written by this format version: properties are re-emitted from the typed map in key order with the C++ `{:.10g}` number format. Files from older versions that C++ would migrate inside the generator class are *not* migrated (PORT_STATUS). | unaffected: the generated tracks/vias are ordinary board items, the generator is only a group | unaffected (same reason). Regeneration/tuning is not ported; editing member tracks through the API leaves the stored properties stale, exactly like editing with generators disabled |
| `PcbTable` with `generated` (`drill_chart`, `stackup_table`) | **F** as table, **D** for the generator part | byte-exact; needs the schema defaults table (`GENERATED_TABLE_SCHEMA`) because defaults are omitted on write | cell text/geometry are stored in the file, so the artwork plots correctly as loaded | DRC treats it as a table. The content is **not refreshed** when the board changes (C++ rebuilds on change) |
| `PcbDrillMap` | **D** | byte-exact | drill symbols on that layer are **not drawn** in tier 1 (C++ draws them from the pads/vias when a map exists for the plotted layer); listed as known gap | none in C++ either |
| `PcbBarcode` | **D** + geometry behind a feature | byte-exact | text part plots; the symbol polygon needs a zint-equivalent encoder (Code 39/128, Data Matrix, QR, Micro QR). Until one is approved in `THIRD_PARTY_EQUIVALENTS.md` the barcode contributes its bounding rectangle outline only and the gap is reported at plot time | effective shape = bounding rectangle (conservative) until the encoder exists |
| `PcbReferenceImage` | **D** | byte-exact (encoded image bytes are re-emitted in the same base64 chunking) | never plotted to Gerber (same as C++) | ignored like C++; bounding box from header-decoded pixel size |
| `Pcb3dBody`, `Fp3dModel` | **D** | byte-exact | no 2D artwork | none |
| `PcbGridItem` | **D** | byte-exact | none | none (router grid hint only) |
| `PcbConstraint` | **D** | byte-exact (members by UUID, dangling members preserved) | none | none; the constraint solver (`constraint_system_2d`, planegcs) is not ported |
| `PcbMarker` | **D** | not saved (exclusions live in project settings) | not plotted | produced by DRC |
| net chains (`net_chain`, terminal pads) | **D** | byte-exact | none | chain length rules are tier 2 |
| `EmbeddedFiles` | **D** | byte-exact (compressed text verbatim) | fonts are not extracted (outline fonts are tier 2) | none |
| component classes | **F** static, **D** dynamic rules | n/a (project file) | none | `A.ComponentClass` expressions: static classes work, dynamic assignment needs libeval (tier 2) |
| `PcbBoardOutline` | derived | not saved | used by plot/3D | used by DRC |

**Opaque is a bring-up tool, not an end state.** `Drawing::Opaque` exists so the parser/writer
work packages can land before every D-level struct is finished: an item type on the list
{`drill_chart`, `stackup_table`, `drill_map`, `grid_item`, `barcode`, `image`} may temporarily be
parsed into `OpaqueItem`. Consequences while a type is opaque: re-save is byte-exact only for
files that are already in the current canonical format (no version migration, no default
elision changes); the item has no bounding box, is skipped by plot, DRC and
`compute_bounding_box`, and cannot be edited; its children (table cells) are not in the UUID
index. `group` members and constraints referring to its UUID still resolve to the opaque item.
`constraint` and `generated` are never opaque because they live in their own lists and are cheap
to type. The exit criterion of the parser WP is "no `Opaque` produced for format 20261031".

Unknown tokens are a parse error exactly as in C++ (`Unknown token '%s'`); there is no silent
pass-through of unknown board-level constructs.

### 3.17 Members dropped everywhere

| C++ member | why it is safe to drop |
|---|---|
| `EDA_ITEM::m_parent` | ownership + `ParentRef` |
| `EDA_ITEM::m_structType` | enum variant / `type_()` |
| `EDA_ITEM::m_rolloverPos`, `m_isRollover`, `m_forceVisible`, `m_netHighlighted`, `m_customPropsStorageCache` | GUI and property-grid state |
| `BOARD_ITEM::m_owningIndex` | the index is maintained by `Board` |
| `KIGFX::VIEW_ITEM` base, `SERIALIZABLE` base (protobuf), `INSPECTOR`/`Visit`, `GetMsgPanelInfo`, `GetMenuImage`, `ViewGetLayers/LOD/BBox`, `HitTest` (except where an algorithm uses it: `PCB_TRACK`, `ZONE::HitTestFilledArea`, connectivity) | GUI / IPC |
| `PADSTACK::m_parent` | passed as argument where needed |
| `PAD::m_dataMutex`, `m_maskShapeMutex`, `ZONE` mutexes, atomics | inside `LazyCache`, or unnecessary because writes need `&mut` |
| `BOARD::m_listeners`, `m_highLight*`, `m_historyLifetime`, `m_embeddedFilesDelegate`, `m_textVarAdapter`, `m_lengthDelayCalc`, `m_LegacyVisibleItems`, drill-model generation counters, `m_drillSymbol*` caches | GUI / editor services; length calculation is a tier-2 service outside the model |
| `BOARD::m_connectivity` | `BoardCtx::connectivity` |
| all public DRC cache members of `BOARD` (fact 9) | owned by `kicad-drc` |
| `BOARD::m_project` | `ProjectContext` data |
| `BOARD_DESIGN_SETTINGS::m_DRCEngine` | `BoardCtx::rules` |
| `FOOTPRINT::m_fieldPropsStorageCache`, `m_searchTerms`, `LIB_TREE_ITEM` base | property grid / library browser |
| `NETINFO_ITEM::m_parent`, `m_terminalPads[2]` | board is context; pads resolved from `terminal_pad_uuids` |
| `PCB_PLOT_PARAMS::m_colors`, `m_default_colors` | colours are plot-call parameters |
| `EDA_GROUP::m_deserializedItems`, `PCB_GENERATOR::m_isDirty` | protobuf staging / interactive state |

---------------------------------------------------------------------------------------------

## 4. Shared behaviour API

Four traits. The first two are the consumer-facing ports of the `BOARD_ITEM` and
`BOARD_CONNECTED_ITEM` virtual interfaces and are implemented by every `View<'a, T>`, by
`ItemRef<'a>` and by the subset enums (the enums dispatch with a `match`, generated by one
`macro_rules!`). The last two are implemented by the raw structs.

```rust
/// Read interface of BOARD_ITEM (include/board_item.h) + the EDA_ITEM parts used headless.
/// `Self` is a `Copy` view, so methods take `self` by value and may return `'a` borrows.
pub trait BoardItem<'a>: Copy + Into<ItemRef<'a>> {
    // ---- identity ----
    fn type_(self) -> KicadT;                                   // Type()
    fn class_name(self) -> &'static str;                        // GetClass()
    fn uuid(self) -> Kiid;                                      // m_Uuid
    fn handle(self) -> ItemHandle;                              // identity for ==, hashing, ordering
    fn base(self) -> &'a ItemBase;

    // ---- context (what C++ reaches through m_parent) ----
    fn cx(self) -> Option<&'a BoardCtx<'a>>;
    fn board(self) -> Option<&'a Board>;                        // GetBoard()
    fn parent(self) -> ParentRef<'a>;                           // GetParent()
    fn parent_handle(self) -> Option<ItemHandle>;               // for `a->GetParent() == b->GetParent()`
    fn parent_footprint(self) -> Option<FootprintRef<'a>>;      // GetParentFootprint()
    fn parent_group(self) -> Option<GroupRef<'a>>;              // GetParentGroup(): group or generator

    // ---- state ----
    fn is_locked(self) -> bool;                                 // IsLocked(): own flag, parent group, footprint
                                                                //   (pads), BOARD_USE::FPHOLDER, no board => false
    fn is_knockout(self) -> bool;                               // IsKnockout()
    fn flags(self) -> EdaItemFlags;                             // GetFlags()

    // ---- layers ----
    fn layer(self) -> PcbLayerId;                               // GetLayer()
    fn layer_set(self) -> Lset;                                 // GetLayerSet()
    fn is_on_layer(self, layer: PcbLayerId) -> bool;            // IsOnLayer()
    fn is_on_copper_layer(self) -> bool;                        // IsOnCopperLayer()
    fn is_side_specific(self) -> bool;                          // IsSideSpecific()
    fn is_layer_agnostic(self) -> bool;                         // IsLayerAgnostic()
    fn fits_enabled_layers(self, enabled: Lset, copper_layer_count: i32) -> bool;
    fn layer_name(self) -> String;                              // GetLayerName()
    fn layer_mask_describe(self) -> String;                     // LayerMaskDescribe()
    fn flipped_layer(self, layer: PcbLayerId) -> PcbLayerId;    // GetFlippedLayer()
    fn board_copper_layer_count(self) -> i32;                   // BoardCopperLayerCount() (32 without a board)
    fn board_layer_set(self) -> Lset;                           // BoardLayerSet()

    // ---- geometry (board frame) ----
    fn position(self) -> Vector2I;                              // GetPosition()
    fn center(self) -> Vector2I;                                // GetCenter()
    fn focus_position(self) -> Vector2I;                        // GetFocusPosition()
    fn fp_relative_position(self) -> Vector2I;                  // GetFPRelativePosition()
    fn bounding_box(self) -> Box2I;                             // GetBoundingBox()
    fn max_error(self) -> i32;                                  // GetMaxError() (ARC_HIGH_DEF without a board)
    /// GetEffectiveShape( aLayer, aFlash, aUsage ). Items without a shape return ShapeNull.
    fn effective_shape(self, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT) -> Arc<dyn Shape>;
    /// GetEffectiveHoleShape( aLayer, aUsage ). None where C++ would assert.
    fn effective_hole_shape(self, layer: PcbLayerId, usage: DrcConstraintT) -> Option<Arc<ShapeSegment>>;
    fn has_hole(self) -> bool;                                  // HasHole()
    fn has_drilled_hole(self) -> bool;                          // HasDrilledHole()
    fn is_tented(self, layer: PcbLayerId) -> bool;              // IsTented()
    /// TransformShapeToPolygon( aBuffer, aLayer, aClearance, aError, aErrorLoc, ignoreLineWidth ).
    fn transform_shape_to_polygon(self, buffer: &mut ShapePolySet, layer: PcbLayerId, clearance: i32,
                                  error: i32, error_loc: ErrorLoc, ignore_line_width: bool);
    /// TransformShapeToPolySet (text-like items render glyph outlines; default forwards).
    fn transform_shape_to_poly_set(self, buffer: &mut ShapePolySet, layer: PcbLayerId, clearance: i32,
                                   error: i32, error_loc: ErrorLoc);
    fn has_line_stroke(self) -> bool;                           // HasLineStroke()
    fn stroke(self) -> Option<StrokeParams>;                    // GetStroke()

    // ---- connectivity ----
    fn is_connected(self) -> bool;                              // IsConnected()
    fn as_connected(self) -> Option<ConnectedRef<'a>>;          // dynamic_cast<BOARD_CONNECTED_ITEM*>

    // ---- reporting (DRC reports contain these strings verbatim) ----
    fn item_description(self, units: &dyn UnitsProvider, full: bool) -> String;   // GetItemDescription()
    fn friendly_name(self) -> String;                           // GetFriendlyName()

    // ---- children ----
    /// RunOnChildren( aFunction, aMode ): footprint children, table cells, group members.
    fn run_on_children(self, mode: RecurseMode, f: &mut dyn FnMut(ItemRef<'a>));
}

/// Read interface of BOARD_CONNECTED_ITEM. Implemented by views of Pad, PcbTrack, PcbArc,
/// PcbVia, Zone, PcbShape (and PcbTextbox/PcbTablecell through their shape), ConnectedRef,
/// TrackRef.
pub trait BoardConnectedItem<'a>: BoardItem<'a> {
    fn conn(self) -> &'a ConnectedBase;
    fn net_code(self) -> i32;                                   // GetNetCode()
    fn net(self) -> Option<&'a NetinfoItem>;                    // GetNet() (None: no board / unknown code)
    fn netname(self) -> &'a str;                                // GetNetname()
    fn short_netname(self) -> &'a str;                          // GetShortNetname()
    fn display_netname(self) -> &'a str;                        // GetDisplayNetname()
    fn netname_msg(self) -> String;                             // GetNetnameMsg()
    /// GetEffectiveNetClass(): net's class, else the board default, else the static fallback
    /// default netclass (no board). Never allocates.
    fn effective_netclass(self) -> &'a Netclass;
    fn netclass_name(self) -> &'a str;                          // GetNetClassName()
    /// GetOwnClearance( aLayer, aSource ): through `cx.rules`, 0 without a rule resolver.
    fn own_clearance(self, layer: PcbLayerId) -> (i32, Option<String>);
    fn local_clearance(self) -> Option<i32>;                    // GetLocalClearance()
    fn local_clearance_with_source(self) -> Option<(i32, String)>;   // GetLocalClearance( wxString* )
    fn clearance_overrides(self) -> Option<(i32, String)>;      // GetClearanceOverrides( aSource )
    fn teardrop_params(self) -> &'a TeardropParameters;
    fn local_ratsnest_visible(self) -> bool;
}

/// Context-free interface of the raw structs (owned or `&mut` situations: parser, commits,
/// clipboard, undo). Implemented by every item struct and by `Item`, `Track`, `Drawing`.
pub trait ItemData {
    fn class_name(&self) -> &'static str;                       // GetClass()
    fn type_(&self) -> KicadT;
    fn base(&self) -> &ItemBase;
    fn base_mut(&mut self) -> &mut ItemBase;
    fn conn(&self) -> Option<&ConnectedBase> { None }
    fn conn_mut(&mut self) -> Option<&mut ConnectedBase> { None }
    /// Clear every LazyCache of the item (and of owned children).
    fn clear_caches(&mut self);
    /// BOARD_ITEM::RemapKIIDs.
    fn remap_kiids(&mut self, id_map: &BTreeMap<Kiid, Kiid>) {}
    /// UUIDs of this item and of everything it owns, in the order BOARD_ITEM_INDEX::BindSubtree
    /// visits them (the board derives each child's ItemKey from its list position while binding).
    fn subtree_uuids(&self) -> Vec<Kiid>;
}

/// Write interface shared by all `ViewMut<'_, T>` and by `ItemMut<'_>`
/// (BOARD_ITEM::Move/Rotate/Flip/Mirror/SetLayer/..., BOARD_CONNECTED_ITEM::SetNetCode).
pub trait BoardItemMut {
    fn set_position(&mut self, pos: Vector2I);                  // SetPosition()
    fn move_(&mut self, v: Vector2I);                           // Move()
    fn rotate(&mut self, centre: Vector2I, angle: EdaAngle);    // Rotate()
    fn flip(&mut self, centre: Vector2I, dir: FlipDirection);   // Flip() (uses ViewMut::env for layer flipping)
    fn mirror(&mut self, centre: Vector2I, dir: FlipDirection); // Mirror( aCentre, aFlipDirection )
    fn mirror_axis(&mut self, axis: Seg);                       // Mirror( const SEG& )
    fn set_layer(&mut self, layer: PcbLayerId);                 // SetLayer()
    fn set_layer_set(&mut self, layers: Lset);                  // SetLayerSet()
    fn set_locked(&mut self, locked: bool);                     // SetLocked()
    fn normalize(&mut self) {}                                  // Normalize()
    /// SetNetCode( aNetCode, aNoAssert ): forces 0 when not on copper; returns false for
    /// non-connected items. Validation against the net list is done by Board::set_item_net.
    fn set_net_code(&mut self, net_code: i32) -> bool;
    fn set_stroke(&mut self, stroke: StrokeParams) {}           // SetStroke()
    fn on_footprint_transformed(&mut self) {}                   // OnFootprintTransformed() (uses ViewMut::xf)
    fn on_footprint_rescaled(&mut self, ratio_x: f64, ratio_y: f64, linear_factor: f64,
                             anchor: Vector2I, parent_rotate: EdaAngle) {}
}
```

"Upcasts" of the C++ hierarchy are view projections, not traits:

```rust
impl<'a, T> View<'a, T> {
    /// Re-view an embedded base struct with the same context and handle.
    pub fn project<U>(self, f: impl FnOnce(&'a T) -> &'a U) -> View<'a, U>;
}
impl<'a> View<'a, PcbField>     { pub fn as_text(self) -> View<'a, PcbText> }        // PCB_FIELD -> PCB_TEXT
impl<'a> View<'a, PcbDimension> { pub fn as_text(self) -> View<'a, PcbText> }        // PCB_DIMENSION_BASE -> PCB_TEXT
impl<'a> View<'a, PcbTextbox>   { pub fn as_shape(self) -> View<'a, PcbShape> }      // PCB_TEXTBOX -> PCB_SHAPE
impl<'a> View<'a, PcbTablecell> { pub fn as_textbox(self) -> View<'a, PcbTextbox> }  // PCB_TABLECELL -> PCB_TEXTBOX
impl<'a> View<'a, PcbGenerator> { pub fn as_group(self) -> View<'a, PcbGroup> }      // PCB_GENERATOR -> PCB_GROUP
```

Text behaviour shared by `PcbText`, `PcbField`, `PcbTextbox`, `PcbTablecell`, dimensions and the
barcode's text is one more view trait, the port of the `EDA_TEXT` virtuals that depend on the
parent:

```rust
pub trait TextItem<'a>: BoardItem<'a> {
    fn eda_text(self) -> &'a EdaText;
    fn text(self) -> &'a str;                                              // GetText()
    fn shown_text(self, context: ResolutionContext, depth: i32) -> String; // GetShownText(): ${LAYER}, footprint, board, project vars
    fn text_pos(self) -> Vector2I;                                         // GetTextPos()
    fn text_size(self) -> Vector2I;                                        // GetTextSize()
    fn text_thickness(self) -> i32;                                        // GetTextThickness()
    fn effective_text_pen_width(self) -> i32;                              // GetEffectiveTextPenWidth()
    fn text_angle(self) -> EdaAngle;                                       // GetTextAngle()
    fn draw_rotation(self) -> EdaAngle;                                    // GetDrawRotation()
    fn draw_pos(self) -> Vector2I;                                         // GetDrawPos()
    fn text_box(self, line: i32) -> Box2I;                                 // GetTextBox()
    fn is_visible(self) -> bool;
    fn effective_text_shape(self, triangulate: bool) -> Arc<ShapeCompound>;// GetEffectiveTextShape()
    fn transform_text_to_poly_set(self, buffer: &mut ShapePolySet, clearance: i32, max_error: i32,
                                  error_loc: ErrorLoc);
}
```

Per-type methods (the long lists in 3.5-3.8) are inherent `impl<'a> View<'a, Pad> { .. }` blocks
in the file that ports the corresponding C++ `.cpp`.

---------------------------------------------------------------------------------------------

## 5. Board-level services

All read services exist twice only where it matters: on `Board` when they need nothing but the
board's own data, on `BoardCtx` when they hand out views or depend on a hook.

### 5.1 Item lookup and iteration

```rust
impl Board {
    // raw lists (C++ order), lib-frame/own data only
    pub fn tracks(&self) -> &ItemList<Track>;            pub fn footprints(&self) -> &ItemList<Footprint>;
    pub fn drawings(&self) -> &ItemList<Drawing>;        pub fn zones(&self) -> &ItemList<Zone>;
    pub fn groups(&self) -> &ItemList<PcbGroup>;         pub fn generators(&self) -> &ItemList<PcbGenerator>;
    pub fn points(&self) -> &ItemList<PcbPoint>;         pub fn markers(&self) -> &ItemList<PcbMarker>;
    pub fn constraints(&self) -> &ItemList<PcbConstraint>;
    pub fn is_empty(&self) -> bool;                                       // BOARD::IsEmpty
    pub fn first_footprint(&self) -> Option<&Footprint>;                  // GetFirstFootprint

    // identity
    pub fn handle_of(&self, id: &Kiid) -> Option<ItemHandle>;             // BOARD::ResolveItem (index + scan fallback)
    pub fn key_of(&self, h: ItemHandle) -> ItemKey;
    pub fn handle_of_key(&self, k: ItemKey) -> Option<ItemHandle>;
    pub fn uuid_of(&self, h: ItemHandle) -> Option<Kiid>;
    pub fn is_item_indexed_by_id(&self, id: &Kiid) -> bool;               // IsItemIndexedById
    pub fn repair_duplicate_item_uuids(&mut self) -> i32;                 // RepairDuplicateItemUuids
    pub fn convert_cross_references_to_kiids(&self, source: &str) -> String;
    pub fn convert_kiids_to_cross_references(&self, source: &str) -> String;

    // finders that only need own data
    pub fn find_footprint_by_reference(&self, reference: &str) -> Option<usize>;
    pub fn find_footprint_by_path(&self, path: &KiidPath) -> Option<usize>;
    pub fn find_pad_by_uuid(&self, id: &Kiid) -> Option<ItemHandle>;
    pub fn area_count(&self) -> usize;                                    // GetAreaCount
    pub fn has_items_on_layer(&self, layer: PcbLayerId) -> bool;          // HasItemsOnLayer
    pub fn groups_sanity_check(&mut self, repair: bool) -> String;        // GroupsSanityCheck
    pub fn add_to_group(&mut self, group: ItemHandle, member: ItemHandle);
    pub fn remove_from_group(&mut self, member: ItemHandle);
}

impl<'a> BoardCtx<'a> {
    /// BOARD::AllConnectedItems(): tracks; per footprint pads, zones, connected drawings;
    /// board zones; connected board drawings.
    pub fn all_connected_items(&'a self) -> Vec<ConnectedRef<'a>>;
    /// BOARD::RunOnChildren(): tracks, zones, markers, groups, constraints, points, footprints
    /// (RECURSE: fields, pads, zones, groups, bodies, constraints, points, drawings), drawings.
    pub fn run_on_children(&'a self, mode: RecurseMode, f: &mut dyn FnMut(ItemRef<'a>));
    /// BOARD::GetItemSet(): every item ordered by UUID (CompareByUuid).
    pub fn item_set(&'a self) -> BTreeMap<Kiid, ItemRef<'a>>;
    /// BOARD::collectOwnedItems(): tracks, zones, generators, footprints, drawings, markers,
    /// groups, constraints, points.
    pub fn owned_items(&'a self) -> Vec<ItemRef<'a>>;
    pub fn pads(&'a self) -> Vec<PadRef<'a>>;                             // GetPads(): footprint order x pad order
    pub fn zone_list(&'a self, include_zones_in_footprints: bool) -> Vec<ZoneRef<'a>>;   // GetZoneList
    pub fn tracks_in_net(&'a self, net_code: i32) -> Vec<TrackRef<'a>>;   // TracksInNet
    pub fn sorted_pad_list_by_x_then_y(&'a self, net_code: i32) -> Vec<PadRef<'a>>;      // GetSortedPadListByXthenYCoord
    pub fn pad_at(&'a self, pos: Vector2I, layers: Lset) -> Option<PadRef<'a>>;          // GetPad( pos, mask )
    pub fn pad_at_track_end(&'a self, track: TrackRef<'a>, end: EndpointT) -> Option<PadRef<'a>>;
    pub fn footprint_at(&'a self, pos: Vector2I, active_layer: PcbLayerId, visible_only: bool,
                        ignore_locked: bool) -> Option<FootprintRef<'a>>;                // GetFootprint
}
```

Sort orders used when saving are free functions over views, ports of the C++ comparators, so the
writer does `let mut v: Vec<_> = cx.tracks().collect(); v.sort_by(cmp_tracks);`:

```rust
pub fn ptr_cmp(a: ItemRef<'_>, b: ItemRef<'_>) -> Ordering;          // BOARD_ITEM::ptr_cmp (type, layer set, uuid)
pub fn cmp_items(a: ItemRef<'_>, b: ItemRef<'_>) -> Ordering;        // BOARD::cmp_items
pub fn cmp_drawings(a: DrawingRef<'_>, b: DrawingRef<'_>) -> Ordering;   // BOARD::cmp_drawings
pub fn cmp_tracks(a: TrackRef<'_>, b: TrackRef<'_>) -> Ordering;     // PCB_TRACK::cmp_tracks (net code, layer, type, uuid)
pub fn cmp_points(a: View<'_, PcbPoint>, b: View<'_, PcbPoint>) -> Ordering;   // PCB_POINT::cmp_points
pub fn fp_cmp_drawings(a: DrawingRef<'_>, b: DrawingRef<'_>) -> Ordering;      // FOOTPRINT::cmp_drawings
pub fn fp_cmp_pads(a: PadRef<'_>, b: PadRef<'_>) -> Ordering;        // FOOTPRINT::cmp_pads
pub fn fp_cmp_zones(a: ZoneRef<'_>, b: ZoneRef<'_>) -> Ordering;     // FOOTPRINT::cmp_zones
```

The final `return a < b` pointer tie-break of every C++ comparator becomes `a.handle().cmp(&b.handle())`
(only reachable with duplicate UUIDs).

### 5.2 Nets and netclasses

```rust
impl Board {
    pub fn find_net(&self, net_code: i32) -> Option<&NetinfoItem>;        // FindNet( int )
    pub fn find_net_by_name(&self, name: &str) -> Option<&NetinfoItem>;   // FindNet( wxString )
    pub fn find_or_add_net(&mut self, name: &str, net_code: i32) -> i32;  // FindOrAddNet
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<i32>;           // Add( NETINFO_ITEM* )
    /// Remove( NETINFO_ITEM* ): re-points every connected item to net 0, then removes.
    pub fn remove_net(&mut self, net_code: i32) -> Option<NetinfoItem>;
    pub fn net_count(&self) -> usize;                                     // GetNetCount
    pub fn build_list_of_nets(&mut self);                                 // BuildListOfNets
    pub fn remove_unused_nets(&mut self) -> Vec<NetinfoItem>;             // RemoveUnusedNets
    pub fn rename_nets(&mut self, new_names: &BTreeMap<String, String>, reporter: &mut dyn Reporter) -> bool;
    pub fn sanitize_netcodes(&mut self);                                  // SanitizeNetcodes (rewrites item net codes)
    pub fn map_nets(&mut self, dest: &mut Board);                         // MapNets
    pub(crate) fn remap_net_codes(&mut self, map: &BTreeMap<i32, i32>);   // helper for the three above
    pub fn set_item_net(&mut self, item: ItemHandle, net_code: i32, no_assert: bool) -> bool;   // SetNetCode with lookup
    pub fn set_areas_net_codes_from_net_names(&mut self) -> i32;          // SetAreasNetCodesFromNetNames
    pub fn match_dp_suffix(&self, net_name: &str) -> (i32, String);       // MatchDpSuffix
    pub fn dp_coupled_net(&self, net_code: i32) -> Option<i32>;           // DpCoupledNet
    pub fn net_class_assignment_candidates(&self) -> BTreeSet<String>;    // GetNetClassAssignmentCandidates
    /// SynchronizeNetsAndNetClasses( aResetTrackAndViaSizes ): ApplyNetChainNetclasses, clear
    /// caches, assign `NetinfoItem::netclass` for every net from
    /// `design_settings.net_settings.get_effective_netclass(name)`, optionally reset the
    /// custom track/via/diff-pair sizes from the default netclass. No-op without a project.
    pub fn synchronize_nets_and_net_classes(&mut self, reset_track_and_via_sizes: bool);
    pub fn apply_net_chain_netclasses(&mut self);                         // ApplyNetChainNetclasses
    pub fn resolve_net_chain_terminal_pads(&mut self);                    // NETINFO_ITEM::ResolveTerminalPads for all nets
}
```

Resolution chain for one item, identical to C++:
`item.conn.net_code` -> `net_info.by_code[code]` -> `NetinfoItem::netclass` (the *effective*,
possibly aggregate class computed by `synchronize_nets_and_net_classes` from explicit label
assignments, pattern assignments `(EdaCombinedMatcher, class)` in file order, net-chain classes
and priorities) -> else `net_settings.default_netclass` -> else the static fallback default
netclass. Pattern-assigned classes are therefore visible to every consumer through
`effective_netclass()` without knowing how the assignment came about; a consumer that wants the
*reason* (DRC "netclass 'X'" rule names) uses `Netclass::constituents` and
`Netclass::parents.<field>` like the C++ `GetClearanceParent()->GetName()`.

### 5.3 Design settings, severities, exclusions

```rust
impl Board {
    pub fn design_settings(&self) -> &BoardDesignSettings;                // GetDesignSettings() const
    pub fn design_settings_mut(&mut self) -> &mut BoardDesignSettings;    // bumps the time stamp
    /// GetMaxClearanceValue(): cached; needs the worst rule clearance when a resolver exists.
    pub fn max_clearance_value(&self, rules: Option<&dyn RuleResolver>) -> i32;
    pub fn resolve_drc_exclusions(&mut self, create_markers: bool) -> Vec<PcbMarker>;   // ResolveDRCExclusions
    pub fn record_drc_exclusions(&mut self);                              // RecordDRCExclusions
    pub fn delete_markers(&mut self);                                     // DeleteMARKERs()
    pub fn delete_markers_filtered(&mut self, warnings_and_errors: bool, exclusions: bool);
}
impl BoardDesignSettings {
    pub fn get_severity(&self, drc_error_code: i32) -> Severity;          // GetSeverity
    pub fn ignore(&self, drc_error_code: i32) -> bool;                    // Ignore
    pub fn get_drc_epsilon(&self) -> i32;                                 // GetDRCEpsilon
    pub fn get_hole_plating_thickness(&self) -> i32;                      // GetHolePlatingThickness
    pub fn get_smallest_clearance_value(&self) -> i32;
    pub fn get_biggest_clearance_value(&self, rules: Option<&dyn RuleResolver>) -> i32;
    pub fn validate_design_rules(&self, units: Option<EdaUnits>) -> Vec<ValidationError>;
}
```

DRC severities are plain data (`drc_severities: BTreeMap<i32, Severity>`) loaded by the
project-settings work package from `.kicad_pro` (`board.design_settings.rule_severities`), with
the C++ defaults installed by `BoardDesignSettings::new()`.

### 5.4 Stackup

```rust
impl Board {
    pub fn stackup_or_default(&self) -> BoardStackup;                     // GetStackupOrDefault
    pub fn layer_depth(&self, start: PcbLayerId, end: PcbLayerId) -> i32; // LayerDepth
}
impl BoardStackup {
    pub fn build_default_stackup_list(&mut self, settings: Option<&BoardDesignSettings>, active_copper_layers_count: i32);
    pub fn build_board_thickness_from_stackup(&self) -> i32;
    pub fn stackup_allowed_brd_layers() -> Lset;
    pub fn synchronize_with_board(&mut self, settings: &BoardDesignSettings) -> bool;
    pub fn get_layer_distance(&self, first: PcbLayerId, second: PcbLayerId) -> i32;
    pub fn count(&self) -> usize;  pub fn get_stackup_layer(&self, index: usize) -> Option<&BoardStackupItem>;
}
```

### 5.5 Layers

```rust
impl Board {
    pub fn copper_layer_count(&self) -> i32;                              // GetCopperLayerCount
    pub fn set_copper_layer_count(&mut self, count: i32);
    pub fn user_defined_layer_count(&self) -> i32;
    pub fn copper_layer_stack_max_id(&self) -> PcbLayerId;                // GetCopperLayerStackMaxId
    pub fn enabled_layers(&self) -> Lset;                                 // GetEnabledLayers
    pub fn set_enabled_layers(&mut self, mask: Lset);
    pub fn is_layer_enabled(&self, layer: PcbLayerId) -> bool;
    pub fn layer_id(&self, name: &str) -> PcbLayerId;                     // GetLayerID
    pub fn layer_name(&self, layer: PcbLayerId) -> String;                // GetLayerName (user name or standard)
    pub fn standard_layer_name(layer: PcbLayerId) -> String;              // GetStandardLayerName
    pub fn set_layer_name(&mut self, layer: PcbLayerId, name: &str) -> bool;
    pub fn set_layer_descr(&mut self, layer: PcbLayerId, descr: Layer) -> bool;
    pub fn layer_type(&self, layer: PcbLayerId) -> LayerT;                // GetLayerType
    pub fn set_layer_type(&mut self, layer: PcbLayerId, t: LayerT) -> bool;
    pub fn is_front_layer(&self, layer: PcbLayerId) -> bool;              // IsFrontLayer / IsBackLayer
    pub fn is_back_layer(&self, layer: PcbLayerId) -> bool;
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId;            // FlipLayer (copper count aware)
    pub fn env(&self) -> BoardEnv;                                        // snapshot for ViewMut
}
```

### 5.6 Title block, properties, variants, text variables

```rust
impl Board {
    pub fn title_block(&self) -> &TitleBlock;        pub fn page_settings(&self) -> &PageInfo;
    pub fn plot_options(&self) -> &PcbPlotParams;    pub fn properties(&self) -> &BTreeMap<String, String>;
    pub fn has_variant(&self, name: &str) -> bool;   pub fn add_variant(&mut self, name: &str);
    pub fn delete_variant(&mut self, name: &str);    pub fn rename_variant(&mut self, old: &str, new: &str);
    pub fn copy_variant(&mut self, old: &str, new: &str, new_description: &str);
    pub fn variant_description(&self, name: &str) -> String;
    pub fn set_current_variant(&mut self, name: &str);
    pub fn contextual_text_vars(&self) -> Vec<String>;                    // GetContextualTextVars
    /// ResolveTextVar( token, aDepth ) / ( token, aVariantName, aDepth ): title block fields,
    /// board properties, ${LAYER}/${FILENAME}-style built-ins, `project.text_vars`.
    pub fn resolve_text_var(&self, token: &str, variant: Option<&str>, depth: i32) -> Option<String>;
    pub fn synchronize_properties(&mut self);                             // SynchronizeProperties
    pub fn fixup_embedded_data(&mut self);                                // FixupEmbeddedData
}
```

### 5.7 Geometry services

```rust
impl<'a> BoardCtx<'a> {
    pub fn compute_bounding_box(&'a self, board_edges_only: bool, physical_layers_only: bool) -> Box2I;
    pub fn bounding_box(&'a self) -> Box2I;                               // GetBoundingBox
    pub fn board_edges_bounding_box(&'a self) -> Box2I;                   // GetBoardEdgesBoundingBox
    /// GetBoardPolygonOutlines (convert_shape_list_to_polygon.cpp).
    pub fn board_polygon_outlines(&'a self, infer_outline_if_necessary: bool,
        error_handler: Option<&mut dyn FnMut(&str, ItemRef<'a>, Option<ItemRef<'a>>, Vector2I)>,
        allow_use_arcs_in_polygons: bool, include_npth_as_outlines: bool) -> (bool, ShapePolySet);
    pub fn convert_brd_layer_to_polygonal_contours(&'a self, layer: PcbLayerId) -> ShapePolySet;
    pub fn track_length(&'a self, track: TrackRef<'a>) -> (i32, f64, f64, f64, f64);   // GetTrackLength
    pub fn test_zone_intersection(&'a self, a: ZoneRef<'a>, b: ZoneRef<'a>) -> bool;   // TestZoneIntersection
}
```

### 5.8 Iteration orders the C++ code relies on (normative)

| consumer | order | Rust source |
|---|---|---|
| everything | list order of `m_tracks`, `m_footprints`, `m_drawings`, `m_zones`, pads within a footprint | `ItemList` order: parser appends, `AddMode::Insert` prepends |
| connectivity build, net propagation | `AllConnectedItems()` | `cx.all_connected_items()` |
| DRC `forEachGeometryItem` | tracks; drawings (dimension, shape, text, textbox, target, table then its cells, barcode); zones; footprints (fields, pads, drawings, zones, ...) | ported in `kicad-drc` on top of the `cx` iterators |
| net iteration (`for net : GetNetInfo()`) | net **name** order | `net_info.iter()` |
| `NetsByNetcode()` | net code order | `net_info.nets_by_netcode()` |
| writer | sorted copies (5.1 comparators); groups write member UUID *strings* sorted | writer WP |
| `GetItemSet()`, group/marker id lists | UUID order | `cx.item_set()` |
| zone fill | zones by priority via the filler's own sort; tie-breaks by UUID | `kicad-zone` |
| C++ iteration over `unordered_set<EDA_ITEM*>` group members (order unspecified) | deterministic: UUID order | `BTreeSet<Kiid>`; noted in PORT_STATUS |

### 5.9 `Session` (above the model)

Not part of `kicad-pcb`; specified here because it is the answer to "where did
`BOARD::BuildConnectivity()` go". It lives in a small crate on top of connectivity/DRC/zone
(or in `kicad-cli` + `kicad-wasm` if the orchestrator prefers no extra crate).

```rust
pub struct Session {
    pub board: Board,
    pub connectivity: ConnectivityData,        // kicad-connectivity
    pub drc: Option<DrcEngine>,                // kicad-drc (rules compiled once, caches inside)
}
impl Session {
    pub fn load(pcb_text: &str, project: Option<ProjectInputs>) -> Result<Session, LoadError>;
    pub fn ctx(&self) -> BoardCtx<'_>;                         // board + connectivity + rules
    pub fn apply(&mut self, commit: BoardCommit) -> ChangeSet; // board.apply, then connectivity/drc listeners
    pub fn revert(&mut self, changes: ChangeSet) -> ChangeSet; // board.revert, then the same listeners
    pub fn build_connectivity(&mut self);                      // BOARD::BuildConnectivity
    pub fn fill_zones(&mut self, zones: Option<&[Kiid]>) -> FillReport;
    pub fn run_drc(&mut self, options: &DrcOptions) -> DrcReport;
    pub fn save(&self) -> String;
}
```

---------------------------------------------------------------------------------------------

## 6. How the design serves each consumer

**`.kicad_pcb` parser** (`pcb_io_kicad_sexpr_parser.cpp`). The parser builds items as detached
values and moves them into their owner, the natural direction for nested ownership: a `Pad` is
filled with the library-frame numbers the file contains and pushed into a local `Footprint`; the
finished footprint is rebaked once (`footprint.on_transformed()`, covering whatever order the
`(at ..)`/`(transform ..)` tokens came in) and moved into the board with
`board.add(Item::Footprint(..), AddMode::BulkAppend)`, which also binds the UUIDs of the whole
subtree. Nets are resolved by name while parsing (`board.find_or_add_net(name, -1)`), so items
carry net codes from the start. The C++ post-processing maps one to one:
`board.resolve_groups()` fills `ItemBase::group` from the parsed member UUID lists (C++ resolves
UUIDs to pointers at the same point), `resolve_net_chain_terminal_pads()`, the barcode
re-assembly loop and the undefined-layer rescue pass use `board.edit(handle)`,
`finalize_bulk_add` is a time-stamp bump. No view is needed while parsing, so the parser never
fights the borrow checker.

**Byte-exact writer** (`pcb_io_kicad_sexpr.cpp`). Works on `BoardCtx::bare(&board)`. Collects
views into vectors, sorts them with the ported comparators of 5.1 (which need board-frame values
for pads and net codes for tracks - both available on views) and formats. Footprint children are
written from their library-frame fields (`pad.lib_pos`, `shape.lib_start`), which are stored
untouched, so no round trip through the transform can perturb a digit. Verbatim payloads
(embedded files, reference image data, `OpaqueItem::sexpr`) are re-emitted as read.

**Item shape generation** (`GetEffectiveShape` / `TransformShapeToPolygon`). Lives in `kicad-pcb`
as view methods next to the port of each class, with the C++ caches reproduced by `LazyCache`.
The view supplies exactly the inputs the C++ code pulls through pointers: parent transform,
`max_error`, the board stackup for back-drill knockouts, connectivity for `FlashLayer`, the rule
engine for mask expansion. Results are `Arc<dyn Shape>` shared with the cache, like
`std::shared_ptr<SHAPE>`.

**Connectivity + ratsnest** (`pcbnew/connectivity/`). `ConnectivityData` is a separate value
owned next to the board. `CN_ITEM::m_parent` becomes an `ItemKey` (stable) plus the cached
geometry `CN_ITEM` already keeps (anchors, bbox, layers); `m_itemMap` becomes
`BTreeMap<ItemKey, ItemMapEntry>`. `build(cx)` walks `cx.all_connected_items()`; incremental
`Add/Remove/Update` are driven by a `ChangeSet`. It implements `ConnectivityQuery`, and its
`IsConnectedOnLayer` asks for `effective_shape(layer, Flashing::AlwaysFlashed, ..)` exactly like
C++ to avoid recursing into itself. The two places where C++ writes into board items
(`PropagateNets` -> `Parent()->SetNetCode`) return a `Vec<(Kiid, i32)>` that the caller applies
with `board.set_item_net`.

**DRC engine and test providers** (`pcbnew/drc/`). `DrcEngine::run(&self, cx: &BoardCtx)` holds the
board immutably for the whole run, so providers may keep `ItemRef<'a>`s and R-trees freely.
`DRC_RTREE::ITEM_WITH_SHAPE { parent, shape, parentShape }` becomes
`{ parent: ItemHandle, shape: Arc<dyn Shape>, .. }`; filters and visitors receive
`cx.item(handle)`. `EvalRules( type, a, b, layer )` reads `a.effective_netclass()`,
`a.parent_footprint()`, `pad.clearance_overrides()` on views. The BOARD-hosted caches of fact 9
move into a `DrcCache` owned by the engine, keyed by `ItemHandle`, stamped with
`(time_stamp, structure_epoch)`. The engine implements `RuleResolver` so that the model's
`own_clearance()` / `solder_mask_expansion()` reach it; its own-clearance cache is keyed by
`(Kiid, layer)` as in C++. Violations are returned as `Vec<PcbMarker>` (a marker stores item
UUIDs, as `RC_ITEM::m_ids` does); the caller decides whether to `board.add` them. Dummy items the
providers construct (edge segments, test shapes, the implicit keepout zone) are locals viewed
with `cx.detached(..)`.

**Zone filler** (`zone_filler.cpp`). Signature
`fill(board: &mut Board, conn: &mut ConnectivityData, rules: &DrcEngine, zones: &[Kiid])`. The
C++ flow alternates reads of the whole board with small writes (clear/set `zoneLayerOverrides`
on pads and vias, `UnFill`, `SetFilledPolysList`, `SetFillFlag`, `SetIsIsland`, `BuildHashValue`,
`SetNeedRefill`); in Rust each read phase builds a `BoardCtx` on `&*board`, computes into locals,
drops the context and performs the writes through the class-2 accessors
(`zone_fill_mut`, `set_zone_layer_override`), which neither bump the time stamp nor clear
geometry caches. Reads of *other* zones' fills during the priority knock-out work unchanged
because earlier results are already stored. The optional `parallel` feature computes fills for
independent zones with `&Board` and writes sequentially. This is the consumer with the most
borrow-scope bookkeeping; it is mechanical but must be done carefully (risk 3 in section 8).

**Gerber / drill plotting** (`plot_board_layers.cpp`, `plot_brditems_plotter.cpp`,
`gendrill_*`). Read-only over `BoardCtx` with connectivity (flashing of unconnected pad layers
affects copper plots). `BRDITEMS_PLOTTER` methods take views; Gerber X2 attributes come from
`pad.parent_footprint().reference()`, `pad.number`, `item.netname()`; aperture decisions use
`pad.shape(layer)`, `pad.size(layer)`, `pad.orientation()` (board frame). Drill file generation
iterates `cx.pads()` and vias and reads `Padstack::drill` & co. `PcbPlotParams` and the aux
origin come from the board.

**CLI** (`kicad-rs pcb drc | export gerbers | export drill`). `Session::load` from the file
text(s) the CLI read, then the calls of 5.9. No lifetimes cross the CLI boundary: the session
owns everything.

**External autorouter client** (load, add/remove/modify tracks and vias, refill, re-run DRC,
export). The whole client API is `Session` + `BoardCommit` + views:

```rust
let mut s = Session::load(&pcb_text, project)?;
let gnd = s.board.find_net_by_name("GND").map(|n| n.net_code).unwrap_or(0);
let mut c = BoardCommit::default();
let id = c.add(Item::Track(Track::Segment(PcbTrack::new_segment(start, end, width, PcbLayerId::FCu, gnd))));
c.add(Item::Track(Track::Via(Box::new(PcbVia::new_through(pos, diameter, drill, gnd)))));
c.remove(old_track_uuid);
let changes = s.apply(c);                 // board edited, connectivity updated incrementally
s.fill_zones(None);
let report = s.run_drc(&DrcOptions::default());
let gerbers = kicad_fab::export_gerbers(&s.ctx(), &opts);
s.revert(changes);                        // undo, if the result was worse
```

Read access for the router's own data structures is a bulk walk over `s.ctx().tracks()`,
`.pads()`, `.zones()`; the client keeps `Kiid`s (or `ItemKey`s) for anything it wants to modify
later.

**Push-and-shove router port (stretch).** `PNS::ITEM::m_parent` (`BOARD_ITEM*`) becomes
`Option<Kiid>`; `PNS_KICAD_IFACE::SyncWorld` walks views; `PNS_PCBNEW_RULE_RESOLVER` calls the
same `RuleResolver`/`DrcEngine` with views (router-transient items are `cx.detached` locals, the
port of `ROUTER_TRANSIENT`); `PNS_KICAD_IFACE::AddItem/RemoveItem/UpdateItem/Commit` stage into a
`BoardCommit`. The router's world (`PNS::NODE`) is its own data structure, as in C++, so the
board's ownership model does not leak into it.

### JavaScript / wasm

`kicad-wasm` exposes one class wrapping `Session`. Nothing borrowed ever crosses the boundary.

* **Addressing.** Public identity is the UUID string. For chatty clients there is a session-local
  integer id: the binding keeps `ids: Vec<ItemKey>` + `HashMap<ItemKey, u32>`; an id stays valid
  across edits and resolves to `None` after the item is removed.
  `board.idOf(uuid) -> number`, `board.uuidOf(id) -> string`.
* **Reading.** `board.item(idOrUuid)` returns a plain object built from the view
  (`{ type, uuid, layer, layers, net, netName, locked, bbox, ...type-specific }`), so JS never
  holds a pointer into wasm memory. Bulk readers return typed arrays for hot paths:
  `board.tracksFlat()` -> `{ ids: Uint32Array, geom: Int32Array /* x1,y1,x2,y2,width,layer,net,kind */ }`,
  likewise `viasFlat()`, `padsFlat()`; `board.zoneOutline(id)` / `zoneFill(id, layer)` return
  coordinate arrays; `board.nets()`, `board.netclasses()`, `board.layers()`, `board.stackup()`
  return JSON.
* **Writing.** `board.commit([{ op: "add", item: {...} }, { op: "remove", id }, { op: "modify", id, patch: {...} }])`
  maps to `BoardCommit` -> `Session::apply`; it returns the new ids and a change token usable
  with `board.revert(token)`.
* **Services.** `board.buildConnectivity()`, `board.ratsnest()`, `board.fillZones()`,
  `board.runDrc(options)` (violations carry UUIDs, positions, severities, messages),
  `board.exportGerbers(options)` / `exportDrill(options)` -> `{ [fileName]: Uint8Array }`,
  `board.save()` -> string.
* Because `Board` is plain data, `board.clone()` (snapshot for speculative routing in a worker)
  is a derive away, and a later `serde` feature can serialise the model without design changes.

---------------------------------------------------------------------------------------------

## 7. Work packages for the model

Rules that make parallel work possible:

* **Types first.** WP M0 creates every struct/enum of section 3 in its final module, with
  constructors that reproduce the C++ constructor defaults, plus `ItemList`, handles, the index,
  `LazyCache`, `View`/`ItemRef`/`BoardCtx` and the trait definitions. Behaviour WPs then only add
  `impl` blocks in the files they own; Rust allows any number of `impl` blocks per type across
  modules of the crate, so file ownership is disjoint.
* **Pending markers.** M0 implements the four traits for every type with bodies that call
  `port_pending!("PAD::GetEffectiveShape")` (a crate macro: panics with the C++ name). Each WP
  replaces the markers of its type. `rg port_pending! crates/kicad-pcb` must be empty before the
  crate is declared done; until then the workspace always compiles.
* **Module = C++ file.** `pad.rs` ports `pad.h/.cpp`; when a C++ file is split over two WPs the
  second half lives in a sibling module (`pad_shapes.rs`).
* Skipped in every file: `*_DESC` property registration structs, `Serialize/Deserialize`
  (protobuf), `GetMsgPanelInfo`, `GetMenuImage`, `ViewGetLayers/ViewGetLOD/ViewBBox`, `Visit`,
  GUI `HitTest` overloads not used by an algorithm, `swapData`. The line counts below are the
  C++ lines that remain after those are skipped (estimates).
* Each WP ports the matching `qa/tests/pcbnew` tests named in its row.

| WP | module(s) in `crates/kicad-pcb/src` | C++ sources | lines to port | depends on | tests to port |
|---|---|---|---|---|---|
| **M0** scaffolding + all type definitions | `item.rs`, `item_list.rs`, `handle.rs`, `uuid_index.rs`, `cache.rs`, `ctx.rs`, `view.rs`, `traits.rs`, `drc_constraint_types.rs`, every type's file with struct + `new()` | `include/eda_item.h`, `eda_item_flags.h`, `include/board_item.h`, `pcbnew/board_item.cpp`, `board_item_container.h`, `board_item_index.h/.cpp`, `pcb_item_containers.h`, `drc/drc_constraint_types.h`, `core/minoptmax.h`, constructors of all item classes | ~2500 | kicad-core, kicad-geom type names | `test_board_item_index.cpp`, `test_board_item.cpp` (structure parts) |
| **M1** nets and netclasses | `netinfo.rs`, `netclass.rs`, `net_settings.rs` | `netinfo.h`, `netinfo_item.cpp`, `netinfo_list.cpp`, `include/netclass.h`, `common/netclass.cpp`, `common/project/net_settings.cpp` (no JSON), `EDA_COMBINED_MATCHER` if not in kicad-core | ~2800 | M0 | netclass/net settings tests in `qa/tests/common` |
| **M2** settings | `board_design_settings.rs`, `zone_settings.rs`, `teardrop_parameters.rs`, `board_stackup.rs`, `pcb_plot_params.rs`, `title_block.rs`, `page_info.rs`, `drill_symbol_profile.rs` | `board_design_settings.h/.cpp` (defaults, accessors, validation; JSON params excluded), `zone_settings.h/.cpp`, `teardrop/teardrop_parameters.h`, `board_stackup_manager/board_stackup.h/.cpp`, `pcb_plot_params.h/.cpp` (data + defaults), `common/page_info.cpp`, `drill/drill_symbol_profile.cpp` | ~3000 | M0, M1 (NetSettings field) | `test_board_design_settings.cpp`, `test_fab_stackup.cpp` (data parts) |
| **M3a** EDA_SHAPE core | `eda_shape.rs` | `include/eda_shape.h`, `common/eda_shape.cpp`: setters, arc/rectangle/poly/bezier/ellipse geometry, bbox, move/rotate/flip/mirror/scale, compare | ~2500 | M0 | `test_shape_corner_radius.cpp`, `test_ellipse_roundtrip.cpp` (geometry parts) |
| **M3b** EDA_SHAPE shapes | `eda_shape_shapes.rs`, `stroke_params.rs` | `common/eda_shape.cpp`: `makeEffectiveShapes`, `TransformShapeToPolygon`, hatching, line endings / shortened bodies, algorithmic hit tests; `common/stroke_params.cpp` | ~2600 | M3a | `test_shape_hatch_knockout.cpp`, `test_line_ending_load_save.cpp` (geometry) |
| **M4** PCB_SHAPE | `pcb_shape.rs` | `pcb_shape.h/.cpp` | ~1800 | M3a, M3b | `test_connect_board_shapes.cpp`, `test_footprint_transform_sync.cpp` (shape parts) |
| **M5** text | `eda_text.rs`, `pcb_text.rs`, `pcb_field.rs` | `include/eda_text.h`, `common/eda_text.cpp`, `pcb_text.h/.cpp`, `pcb_field.h/.cpp` | ~2600 | M0, kicad-font | `test_text_variable_resolution.cpp`, `test_plot_footprint_scaled_text.cpp` (model parts) |
| **M6** text boxes and tables | `pcb_textbox.rs`, `pcb_table.rs`, `pcb_tablecell.rs`, `pcb_generated_table.rs` (data) | `pcb_textbox.h/.cpp`, `pcb_table.h/.cpp`, `pcb_tablecell.h/.cpp`, data members of `pcb_generated_table.h`, `pcb_drill_chart.h`, `pcb_stackup_table.h`, `board_tables/generated_table_schema.h/.cpp` (defaults table) | ~2700 | M4, M5 | `test_pcb_textbox.cpp` |
| **M7** PADSTACK | `padstack.rs` | `padstack.h/.cpp` | ~2000 | M0 (PcbShape as data) | `test_padstack_clone.cpp`, `test_backdrill_properties.cpp` |
| **M8a** PAD core | `pad.rs` | `pad.h`, `pad.cpp`: data, frames, layers, attributes, `FlashLayer`, clearance/margin/zone-connection getters, flip/rotate/mirror, `OnFootprint*`, `Compare` | ~2500 | M7 | `test_pad_setters.cpp`, `test_pad_flashing.cpp`, `test_pad_tenting.cpp`, `test_pad_swap_positions.cpp` |
| **M8b** PAD shapes | `pad_shapes.rs`, `pad_utils.rs` | `pad.cpp`: `BuildEffectiveShapes`, `BuildEffectivePolygon`, `TransformShapeToPolygon`, `TransformHoleToPolygon`, hole/mask shapes, `MergePrimitivesAsPolygon`; `pad_utils.cpp` | ~2000 | M8a, M4 | `test_fab_pad_geometry.cpp`, `test_plot_pad_mask.cpp` (shape parts), `test_custom_pad_primitive_roundtrip.cpp` |
| **M9a** tracks and arcs | `pcb_track.rs` | `pcb_track.h/.cpp`: `PCB_TRACK`, `PCB_ARC`, `cmp_tracks` | ~1500 | M0 | `test_issue25650_collinear_arc.cpp`, track parts of `test_board_item.cpp` |
| **M9b** vias | `pcb_via.rs` | `pcb_track.h/.cpp`: `PCB_VIA` (layers, padstack access, tenting/covering/plugging/capping/filling, backdrill, post-machining, `FlashLayer`, hole shapes, `ValidateViaParameters`, `CollectMicroviaColumns`) | ~2400 | M7, M9a | `test_via_validation.cpp`, `test_issue25621_via_flashing_contact_layer.cpp` |
| **M10** zones | `zone.rs`, `zone_utils.rs` | `zone.h/.cpp`, `zone_utils.h/.cpp` | ~2900 | M0, M2 | `test_zone.cpp` |
| **M11a** footprint core | `footprint.rs` | `footprint.h`, `footprint.cpp`: children, `Add/Remove`, fields, transform propagation (`SetPosition`..`Flip`, `SetTransformScale`), variants, attributes, net ties, jumper groups, `FixUuids`, `cmp_pads/cmp_drawings/cmp_zones` | ~2900 | M8a, M5, M4, M10 (structs) | `test_footprint.cpp`, `test_footprint_transform_sync.cpp`, `test_footprint_net_tie.cpp`, `test_variant.cpp` |
| **M11b** footprint geometry | `footprint_geometry.rs`, `footprint_utils.rs` | `footprint.cpp`: bbox/hull/courtyard caches, `TransformPadsToPolySet`, `TransformFPShapesToPolySet`, text variables, `GetCoverageArea`, `FixUpPadsForBoard`; `footprint_utils.cpp` | ~2500 | M11a, M8b, M12 | `test_footprint_pad_fixup.cpp`, `test_footprint_rescue_layer.cpp` |
| **M12** outlines | `convert_shape_list_to_polygon.rs`, `pcb_board_outline.rs` | `convert_shape_list_to_polygon.h/.cpp`, `pcb_board_outline.h/.cpp` | ~2100 | M4, M8a | outline tests in `qa/tests/pcbnew` (`test_board_...`), DRC edge tests later |
| **M13a** board core | `board.rs`, `board_layers.rs`, `board_groups.rs` | `board.h`, `board.cpp`: `Add/Remove/RemoveAll`, `FinalizeBulk*`, index facade, layers, properties, variants, groups sanity, comparators, iteration helpers, `ComputeBoundingBox` | ~2800 | M0 (can start with M1/M2 in flight) | `test_board_item.cpp`, `test_group_load_save.cpp` (model parts), `test_headless_pcb_context.cpp` |
| **M13b** board services | `board_nets.rs`, `board_text_vars.rs`, `board_markers.rs` | `board.cpp`: nets facade, `SynchronizeNetsAndNetClasses`, `SanitizeNetcodes`, `MapNets`, `GetPad*`, `GetTrackLength`, `GetMaxClearanceValue`, exclusions/markers, `GetStackupOrDefault`, text vars; `board_text_var_adapter.cpp` | ~2500 | M13a, M1, M2, M8a, M10 (footprint data comes from M0; `GetPad( pos )` hit-testing lands with M8b) | `test_board_connected_items.cpp`, `test_text_var_reactive.cpp` (model parts) |
| **M14** dimensions | `pcb_dimension.rs` | `pcb_dimension.h/.cpp` | ~3000 | M5, M3b | `test_dimension_zero_suppression.cpp` |
| **M15** small items | `pcb_group.rs`, `pcb_generator.rs`, `pcb_point.rs`, `pcb_target.rs`, `pcb_marker.rs`, `pcb_3d_body.rs`, `pcb_grid_item.rs`, `pcb_drill_map.rs`, `pcb_constraint.rs`, `pcb_reference_image.rs` | the corresponding `.h/.cpp`, `common/eda_group.h/.cpp`, `common/marker_base.cpp`, `common/rc_item.cpp` + `drc/drc_item.cpp` (data), `common/reference_image.cpp`, PNG/JPEG header size reader | ~2800 (about half is data) | M0, M4 (marker path shapes) | `test_reference_image_load.cpp`, `test_group_mirror.cpp` (model parts), `test_custom_property_roundtrip.cpp` (model parts) |
| **M16** barcode | `pcb_barcode.rs` | `pcb_barcode.h/.cpp` (+ encoder per THIRD_PARTY_EQUIVALENTS decision) | ~1100 | M5 | `test_barcode_load_save.cpp` (model parts) |
| **M17** component classes, embedded files | `component_class.rs`, `component_class_manager.rs`, `embedded_files.rs` | `component_classes/*.h/.cpp`, `include/project/component_class_settings.h`, `include/embedded_files.h`, `common/embedded_files.cpp` | ~1500 | M0 | `test_component_classes.cpp` (static parts) |
| **M18** commits | `board_commit.rs` | concept of `board_commit.cpp` / `common/commit.cpp`; `apply`, `revert`, `ChangeSet`, `BoardListener` | ~600 Rust | M13a | `test_board_commit.cpp` (bookkeeping parts) + new epoch/handle tests |

Dependency order and parallelism:

```
wave 0:  M0
wave 1:  M1  M2  M3a  M7  M9a  M13a  M15  M17          (8 agents)
wave 2:  M3b  M5  M8a  M9b  M10  M18                    (6 agents)
wave 3:  M4  M13b  M14  M16                             (4 agents; M4 needs M3b)
wave 4:  M6  M8b  M11a  M12                             (4 agents)
wave 5:  M11b
```

The parser and writer work packages can start after wave 1 for board-level items
(tracks, zones outline data, nets, setup) because they only need the data definitions of M0 plus
`Board::add`; they need M11a for footprints. Connectivity can start after M8a/M9b/M10 with
`port_pending!` shapes filled in by the time its tests run.

---------------------------------------------------------------------------------------------

## 8. Risks and open questions

**Risks**

1. **Frame mistakes (lib vs board) are the number one correctness risk** of any port of this
   tree, independent of the ownership model, because C++ mixes derive-on-read (`PAD`, `PCB_TEXT`,
   `ZONE`, `PCB_BARCODE`, dimensions) with baked copies (`PCB_SHAPE`, `PCB_POINT`). This design
   keeps exactly the C++ strategy per type (so rounding happens at the same places) and makes the
   wrong call uncompilable (no board-frame getters on raw structs). Reviewers should still diff
   `position()/size()/orientation()` against the oracle on rotated, flipped and *scaled*
   footprints (`qa/tests/pcbnew/test_footprint_transform_sync.cpp`).
2. **View plumbing cost.** Every consumer signature carries `'a` and views. This is verbose but
   uniform. If an implementer reaches for `Rc<RefCell<Board>>` or stores `&Board` in a struct
   that also wants to mutate it, the design is being fought; the fix is always "compute with a
   context, drop it, then write".
3. **Zone filler restructuring.** The C++ filler interleaves reads and writes and runs lambdas
   on a thread pool. The sequential port needs disciplined borrow scopes; the risk is not
   feasibility but accidental reordering of writes relative to reads (e.g. reading a
   `zone_layer_override` before the pass that sets it). Mitigation: port pass by pass, each pass
   a function `fn pass_n(cx: &BoardCtx, ..) -> PassNResult` followed by `apply_pass_n(&mut Board, ..)`.
4. **Handle misuse.** A stale `ItemHandle` after a removal silently addresses a different item of
   the same list (the safe-Rust analogue of a dangling pointer). Mitigation: epoch assertions in
   debug/test builds, `try_item` returning `None` when out of range, `ItemKey`/`Kiid` in every
   long-lived structure, and a clippy-style review rule: "no `ItemHandle` field in a struct that
   outlives one `&Board` borrow".
5. **Removal cost on very large boards** (2.9 #4). Batch removal is O(n) once; single removals
   are O(n) each. Acceptable for CLI and commit-based clients; to be measured for the PNS port,
   with the `Box`ed-element fallback ready.
6. **Memory.** Inline `PcbShape` (EDA_SHAPE + library copy) is roughly 450-500 bytes, `Pad` with
   one copper layer roughly 700 bytes plus heap; a 20k-pad, 100k-track board is on the order of
   60-80 MB, comparable to C++. wasm32 has a 4 GB ceiling but practical browser limits are lower;
   boxing `poly`/`lib_poly`/custom properties is already in the definitions. To be measured on
   the largest `demos/` and `qa/data` boards early.
7. **Nets by code.** Any code path that renumbers or removes nets without going through
   `Board::remap_net_codes`/`remove_net` leaves dangling codes. All such paths are in M13b; items
   expose no public setter that could desynchronise silently except `conn.net_code` itself, which
   is why `set_item_net` exists and `ItemMut::set_net_code` documents the contract.
8. **`LazyCache` contention under the optional `parallel` feature.** One `Mutex` per pad cache is
   fine sequentially; with rayon the first touch of many pads serialises only per pad, not
   globally (C++ has the same per-pad mutex). The board-wide C++ `m_CachesMutex` has no
   equivalent here because board-wide caches left the model.
9. **Barcodes and drill maps on fabrication layers** are the two places where a D-level type can
   change fabrication output (3.16). They are rare in existing test boards (new in this format
   series) but must be reported loudly at plot time, not silently skipped.
10. **Generated tables go stale** after API edits because the generators are not ported. A board
    saved after such edits contains the old cell text; KiCad will refresh it on load. Documented
    deviation.
11. **Group member order.** C++ iterates `std::unordered_set<EDA_ITEM*>`; any C++ behaviour that
    depends on that order is already non-deterministic. We fix UUID order and note it.
12. **`Board: Clone` is a deep copy** (zone fills and netclasses are shared `Arc`s, everything
    else is copied). It is a convenience for small boards and tests, not the undo mechanism;
    `ChangeSet` is.

**Open questions for the orchestrator / other designers**

* Q1. Foundation types this model needs that are not in the PORTING.md list: `TransformTrs`
  (`libs/kimath/.../transform_trs.h`), `EdaOrientation`, `Vector3D`, `Vector2D`, `Hash128`,
  `Ellipse<i32>` / `ShapeEllipse`, `ShapeSegment`, `ShapeNull`, `ShapeSimple`, `ErrorLoc`,
  `MinOptMax<T>`, `LineEnding`, `EdaCombinedMatcher`, `Lseq`, `CaseInsensitiveMap`, and a
  `UnitsProvider` trait for `item_description`. Which crate owns each?
* Q2. `trait Shape` must be `Send + Sync` (shapes sit in `LazyCache` inside a `Board` that should
  stay `Send + Sync`). Is that already the case in `kicad-geom`?
* Q3. Where do `KicadT`'s PCB values new in this tree (`PcbConstraintT`, `PcbGridItemT`,
  `PcbDrillChartT`, `PcbDrillMapT`, `Pcb3dBodyT`, `PcbStackupTableT`, `PcbPointT`,
  `PcbBoardOutlineT`) live - assumed `kicad-core`, with the C++ ordinals.
* Q4. Text attributes hold a font (`KIFONT::FONT*`). Proposed: `TextAttributes::font:
  Option<FontId>` resolved by `kicad-font`, so `kicad-core` does not depend on the font crate.
* Q5. Kiid generation: C++ creates random UUIDs in constructors. PORTING.md forbids randomness
  in library crates; proposed: `ItemBase::new(layer)` takes the UUID from an explicit
  `KiidGenerator` passed by the caller (the parser never needs one except for legacy files and
  items without a `uuid` token; commits get one from the session).
* Q6. Barcode symbol generation: is a pure-Rust zint equivalent acceptable (several crates
  cover QR / Data Matrix / Code 128 / Code 39; Micro QR is the gap), or is porting the needed
  zint encoders by hand in scope?
* Q7. Project settings: this proposal assumes one separate work package loads `.kicad_pro`
  (`board.design_settings.*`, `net_settings`, severities, exclusions, text variables, tuning
  profiles, component class rules) and `.kicad_dru` into the structs defined here. Confirm it is
  not expected to be part of the model WPs.
* Q8. `Session` placement: separate tiny crate vs duplicated glue in `kicad-cli` and
  `kicad-wasm`. The crate graph in `docs/PLAN.md` has no slot for it yet.
* Q9. Should `ItemList<Track>` start as `VecDeque<Track>` (inline, cache friendly, O(n) removal
  moves ~170 bytes per element) or `VecDeque<Box<Track>>` (C++ layout)? Proposed: inline, decide
  again with PNS measurements; the API hides the choice.
* Q10. Regex flavour for netclass pattern assignments (`EDA_COMBINED_MATCHER` uses wxRegEx
  advanced syntax plus wildcards): which Rust engine is approved, and are look-arounds needed for
  parity?

---------------------------------------------------------------------------------------------

## Appendix A. Cheat sheet for implementers

```
C++                                           Rust (this design)
--------------------------------------------  ---------------------------------------------------
BOARD* board                                  cx: &BoardCtx<'a>   (read)   /  board: &mut Board (write)
BOARD_ITEM* / const BOARD_ITEM*               ItemRef<'a>
PAD* / PCB_VIA* / ZONE* / FOOTPRINT*          PadRef<'a> / ViaRef<'a> / ZoneRef<'a> / FootprintRef<'a>
item->GetBoard()                              item.board()
item->GetParentFootprint()                    item.parent_footprint()
item->Type()                                  item.type_()            (or match on the enum)
static_cast<PAD*>( item )                     item.as_pad().unwrap()  (or `if let ItemRef::Pad(p)`)
item->m_Uuid                                  item.uuid()
item == other  (pointers)                     item.handle() == other.handle()   (ItemRef: ==)
std::set<BOARD_ITEM*>                         BTreeSet<ItemHandle>
keep a pointer for later                      keep item.uuid() (Kiid) or cx.board.key_of(handle)
pad->GetPosition()                            pad.position()          (view)
pad->GetLibraryPosition()                     pad.lib_pos             (field)
pad->SetPosition( p )                         board.edit(h) -> ItemMut::Pad(mut pad) -> pad.set_position(p)
zone->SetFilledPolysList( l, polys )          board.zone_fill_mut(i).filled_polys_list.insert(l, Arc::new(polys))
board->Add( item )                            board.add(Item::.., AddMode::Insert)
board->Remove( item )                         board.remove(handle)    -> Option<Item>
commit.Add/Remove/Modify + Push               BoardCommit::add/remove/modify + Board::apply -> ChangeSet
board->GetConnectivity()->X( item )           cx.connectivity.map(|c| c.x(cx, item.handle()))
bds.m_DRCEngine->EvalRules( t, a, b, l )      cx.rules.map(|r| r.eval_rules(t, a, b, l))
mutable cache + dirty flag + mutex            LazyCache<K, V>
item->SetFlags( SKIP_STRUCT ) in an algorithm BTreeSet<ItemHandle> local to the algorithm
PAD dummy( footprint )                        let dummy = Pad::new(..); cx.detached(&dummy, ParentRef::Footprint(fp), 0)
```
