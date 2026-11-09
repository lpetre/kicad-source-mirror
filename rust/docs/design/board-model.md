# Board model of `kicad-pcb` - final design

Status: **binding** for every work package that implements or consumes the board model (model,
`.kicad_pcb` parser and writer, item shapes, connectivity, DRC, zone filler, plotting/fab,
settings, CLI, wasm, external clients). The complete type definitions are in
[`board-model-types.md`](board-model-types.md); this file is the architecture, the shared API and
the rules. C++ reference: this tree, board file format `SEXPR_BOARD_FILE_VERSION 20261031`.
Read `rust/PORTING.md` first; nothing here overrides it.

## 0. The design on one screen

| Topic | Decision |
|---|---|
| Storage | Every board item lives in a **board-level typed arena** `Arena<T>` (generational slot vector), one arena per concrete Rust item type, declared by one table (`board_items!`). Footprint children live in the same arenas as their board-level siblings. |
| Order | Arena slot order is meaningless. The C++ containers (`m_tracks`, `m_drawings`, `FOOTPRINT::m_pads`, ...) are **order lists of handles** (`VecDeque<TrackId>` ...) that reproduce C++ insertion order exactly (`push_back` / `push_front`). |
| Identity | `Kiid` (persistent: file, reports, JS across save/load) and a **`Copy` handle** `Id<T>` / `ItemId` (8 bytes, O(1), stays valid across all edits of other items, R-tree payload, cache key, wasm `number`). |
| Parent / child | `ParentRef` value in every item (`None` / `Board` / `Item(ItemId)`); child order lists in `Footprint`, `PcbTable`; group membership by handle on both sides. No `Rc`, no `RefCell`, no lifetimes in item types. |
| Polymorphism | Plain structs with inherent methods (one module per C++ file); `trait BoardItem` / `BoardConnectedItem` / `BoardItemMut` carry the C++ virtuals with base-class bodies as defaults; `AnyItem<'a>` enum for `switch( Type() )`; `ItemRef<'a>` = context + handle + `AnyItem`, the Rust spelling of a `BOARD_ITEM*` parameter. |
| Context | Anything that C++ reaches through `m_parent` / `GetBoard()` / `m_DRCEngine` / `GetConnectivity()` comes from an explicit `&BoardCtx` (board + optional rule resolver + optional connectivity). |
| Frames | Footprint children are stored in the **library frame**. Board-frame values exist only behind accessors that take `&BoardCtx` (read) or `&MutEnv` (write); frame-dependent fields are `pub(crate)` and named `lib_*`. |
| Mutation | Only through `Board` methods: `add_*`, `remove`, `modify_*` (leaf mutators get a `MutEnv` snapshot), `footprint_*` cascades, derived-state writers (fills, zone-layer overrides) that do not invalidate, `BoardCommit` (parking, revert keeps handles). A pull-based **change journal** replaces `BOARD_LISTENER`. |
| Caches | Per-item derived data stays in the item (`OnceCache` / `KeyedCache`, `Clone` to empty). Everything spanning items (connectivity, R-trees, DRC pair caches) lives in the consumer crate, keyed by `ItemId`. |
| Upward calls | `RuleResolver` and `ConnectivityQuery` traits defined in `kicad-pcb`, implemented in `kicad-drc` / `kicad-connectivity`, passed in through `BoardCtx`. |
| Above the model | A `Design { board, connectivity, drc }` owner in the CLI / wasm / client crates holds the peers side by side; none borrows another. |

Why this design (scored against two alternatives, nested ownership with borrowed views and a
mirrored `dyn` class hierarchy): handles that survive edits are what connectivity, R-trees, the
router, undo and JavaScript all need; the borrow checker is satisfied by construction because no
consumer stores a reference into the board; arenas give dense memory and O(1) lookups on 100k+
item boards; and consumer code still reads like the C++ (`item.layer_set()`,
`item.effective_shape( layer, .. )`, `pad.size( ctx, layer )`).

## 1. Facts about the C++ model that drive the design

Verified in this tree; several differ from released KiCad 9 (do not port from memory).

1. **Footprint children are stored in the library frame** (format 20260616): `FOOTPRINT` holds
   `TRANSFORM_TRS m_transform` (translate, rotate, scale x/y) + `m_flipped`; pads, texts, zones,
   dimensions and barcodes derive board values on read, `PCB_SHAPE` (and textbox, cell) and
   `PCB_POINT` keep lib members plus a baked board copy (section 5).
2. **Items call up** through `m_parent`: parent footprint, `GetBoard()` settings (about 100
   guarded `if( const BOARD* board = GetBoard() )` defaults), `bds.m_DRCEngine->EvalRules`
   (own clearance, mask expansion, width constraint, thermal gap) and
   `GetConnectivity()->IsConnectedOnLayer` (`PAD/PCB_VIA::FlashLayer`).
3. **Container order is observable** (DRC `forEachGeometryItem`, connectivity, zone filler):
   deques with `push_front` for `ADD_MODE::INSERT` (the default) on board tracks, footprints,
   drawings, and on *every* footprint list unless the mode is exactly `APPEND`
   (`board.cpp:1520`, `footprint.cpp:1900`). **Saving re-sorts** everything with comparators
   that end in `m_Uuid`, then pointer order (`PCB_TRACK::cmp_tracks` sorts by net code first).
4. **Identity** is `m_Uuid` + `BOARD_ITEM_INDEX`; duplicates are tolerated (later bind wins).
   Groups hold member pointers in an `unordered_set`; constraints, net-chain terminals, zone
   link sets and markers refer by `KIID`.
5. **Nets**: items hold `NETINFO_ITEM*`; the file names nets, codes follow parse order and
   influence save order; `GetNetInfo()` iterates by name; the effective netclass is assigned by
   `SynchronizeNetsAndNetClasses` (no-op without a project).
6. **Derived state on items** (pad shape caches, footprint bbox/courtyard caches, zone fills,
   `m_zoneLayerOverrides` written by the filler and read by `FlashLayer`, text caches) and **DRC
   scratch on `BOARD`** (`m_CopperItemRTreeCache`, `m_Intersects*Cache`, ...).
7. **Detached copies** (`PAD dummy( *pad )` in plotter and filler) keep `m_parent` and must still
   answer `GetPosition()`.
8. `GetEffectiveShape( layer, FLASHING, DRC_CONSTRAINT_T )` takes a constraint type, so
   `DRC_CONSTRAINT_T` lives below `kicad-drc`.
9. New in this tree: points, barcodes, 3D bodies, grid items, drill maps, generated tables,
   constraints, board outline item, generators, variants, net chains, custom properties,
   backdrill / tertiary drill / post-machining, copper thieving, ellipses, line endings.

## 2. Ownership and storage

```rust
board_items! {
//  kind        handle         arena        Rust type            C++ classes
    Footprint   FootprintId    footprints   Footprint;        // FOOTPRINT
    Pad         PadId          pads         Pad;              // PAD
    Track       TrackId        tracks       Track;            // PCB_TRACK, PCB_ARC, PCB_VIA (enum)
    Zone        ZoneId         zones        Zone;             // ZONE
    Shape       ShapeId        shapes       PcbShape;         // PCB_SHAPE
    Text        TextId         texts        PcbText;          // PCB_TEXT
    Field       FieldId        fields       PcbField;         // PCB_FIELD
    Textbox     TextboxId      textboxes    PcbTextbox;       // PCB_TEXTBOX
    Table       TableId        tables       PcbTable;         // PCB_TABLE, PCB_DRILL_CHART, PCB_STACKUP_TABLE
    Tablecell   TablecellId    tablecells   PcbTablecell;     // PCB_TABLECELL
    Dimension   DimensionId    dimensions   PcbDimension;     // PCB_DIM_ALIGNED/ORTHOGONAL/RADIAL/LEADER/CENTER
    Barcode     BarcodeId      barcodes     PcbBarcode;       // PCB_BARCODE
    Image       ImageId        images       PcbReferenceImage;// PCB_REFERENCE_IMAGE
    Target      TargetId       targets      PcbTarget;        // PCB_TARGET
    Point       PointId        points       PcbPoint;         // PCB_POINT
    Group       GroupId        groups       PcbGroup;         // PCB_GROUP
    Generator   GeneratorId    generators   PcbGenerator;     // PCB_GENERATOR + subclasses
    Marker      MarkerId       markers      PcbMarker;        // PCB_MARKER
    Constraint  ConstraintId   constraints  PcbConstraint;    // PCB_CONSTRAINT
    Body3d      Body3dId       bodies       Pcb3dBody;        // PCB_3D_BODY
    GridItem    GridItemId     grid_items   PcbGridItem;      // PCB_GRID_ITEM
    DrillMap    DrillMapId     drill_maps   PcbDrillMap;      // PCB_DRILL_MAP
}
```

The macro (in `items_macro.rs`, `macro_rules!` only) expands to the `ItemKind` enum and the
typed-id conversions, the arena fields of `Board`, `Index<XxxId> for Board`, `AnyItem`,
`AnyItemMut`, `OwnedItem`, `Board::get(ItemId)` and the per-kind `add_*` / `modify_*` /
`get_*` shells. Adding a type is one line plus its struct.

* Typed arenas (not one `Arena<OwnedItem>`): typed handles make links self-checking
  (`Footprint::pads: VecDeque<PadId>` cannot hold a zone), hot loops touch homogeneous memory,
  field-disjoint borrows between arenas let footprint cascades compile without `unsafe`, and a
  track segment costs `size_of::<Track>()` (vias are boxed inside `Track::Via`).
* Order lists: the board-level lists are fields of `Board`, the footprint-level lists fields of
  `Footprint`, table cells `PcbTable::cells`. Semantic iteration **always** goes through an
  order list, never through an arena.
* Inline (non-arena) sub-items, exactly where C++ does not register them with the board: pad
  custom-shape primitives (`Padstack` copper props), the barcode's text, marker path shapes,
  generator template items. They carry `ParentRef::Item(owner)` and no handle.
* Detached subtrees (commit before-images, clipboard, a footprint loaded from a library) are
  `OwnedItem` values; a footprint subtree is `OwnedItem::Footprint(Box<FootprintTree>)` (the
  footprint plus its children by value, group members by `Kiid`). `Board::add` /
  `Board::footprint_take` convert between the two forms and remap handles (port of the
  `ptrMap` in the C++ `FOOTPRINT` copy constructor).
* A footprint-library file (`.kicad_mod`) is parsed into a `Board` with
  `board_use == BoardUse::FpHolder` holding one footprint, as C++ does.

## 3. Identity

| | `Kiid` | `Id<T>` / `ItemId` |
|---|---|---|
| lifetime | persistent, written to the file | one `Board` value (a `Board::clone()` keeps them valid) |
| uniqueness | should be unique; duplicates tolerated like C++ | unique among live items by construction |
| use for | file, DRC reports, exclusions, constraints, JS ids across save/load, undo records kept across sessions | parent/child links, order lists, group members, R-tree payload, cache keys, connectivity nodes, router `parent`, wasm calls |
| lookup | `board.resolve_item(&kiid) -> Option<ItemId>` (hash) | `board.get(id)` (index + generation compare) |

**Rule: store handles, pass `ItemRef`, persist `Kiid`.** A `BOARD_ITEM*` kept in a C++ member or
container becomes an `ItemId` (or typed id); a `BOARD_ITEM*` parameter becomes `ItemRef<'_>`.

* A handle stays valid until *its own* item is removed (freed). Edits, additions and removals of
  other items never invalidate it. A stale handle resolves to `None` (generation mismatch), never
  to another item. `board[id]` panics on a stale handle and is only for handles obtained from the
  board during the current borrow; `board.get(id)` is for handles that crossed a boundary.
* `ItemId::to_bits()` < 2^53 (exact JS `number`); `from_bits` validates the kind.
* Ordering of `ItemId` is total and deterministic; it replaces every C++ pointer-order
  tie-break (only reachable with duplicate UUIDs; record as a deviation where it occurs).
* `NetCode(i32)` identifies nets (`NETINFO_ITEM` is not an arena item);
  `ItemId` has an `ItemKind::Net` form (index = code) so `ResolveItem` can return a net.
* A removed item that an open `BoardCommit` may restore is **parked**: invisible to `get` and
  iteration, restorable with the same handle.
* Generations are `u16`; a slot whose generation is exhausted is retired, never reused.
* New UUIDs come from `board.kiid_generator` (`kicad_core::KiidGenerator`, deterministic by
  default; the CLI and wasm hosts reseed it from entropy before creating items in a board that
  will be saved). `add_*` never changes a UUID.

## 4. Parent / child and cross references

| relation (C++) | Rust | how a consumer gets it |
|---|---|---|
| `m_parent` | `BoardItemBase::parent: ParentRef` (`None`, `Board`, `Item(ItemId)`) | `item.parent()` |
| child -> footprint (`GetParentFootprint()`, walks through a table) | `ParentRef::Item(fp)` (cells: `Item(table)` then the table's parent) | `item.parent_footprint()`, typed `pad.parent_footprint(ctx)` (one index, at most two steps) |
| footprint -> children | `Footprint::{fields, drawings, pads, zones, groups, constraints, points, bodies}` order lists | `board.footprint_pads(fp)` ... |
| table -> cells | `PcbTable::cells: Vec<TablecellId>` | `board.table_cells(t)` |
| item -> group (`m_group`) | `BoardItemBase::group: Option<ItemId>` (kind Group or Generator) | `item.parent_group()` |
| group -> members (`unordered_set<EDA_ITEM*>`) | `PcbGroup::members: Vec<ItemId>` (insertion order, unique; writer sorts UUID strings like C++) | `board.group_members(g)` |
| item -> net (`m_netinfo`) | `ConnectedItemBase::net: NetCode` | `item.net_code()`, `item.net()` |
| net -> effective netclass | `NetinfoItem::netclass: Option<Arc<Netclass>>` (set by `synchronize_nets_and_netclasses`) | `item.effective_netclass()` |
| net -> items | not stored (as in C++): `board.tracks_in_net(code)` scans, connectivity indexes | |
| by-`KIID` references (`CONSTRAINT_MEMBER::m_item`, `m_terminalPadUuids`, `m_linkSet`, `RC_ITEM::m_ids`) | stay `Kiid` | `board.resolve_item(&kiid)`; dangling targets survive a round trip |

Only `Board` methods write both sides of a relation (`add_to_group`, `remove_from_group`,
`add_*`, `remove`). `Board::check_invariants()` (debug builds, every model test) verifies:
order lists hold live ids and every live item is in exactly one list; `parent` agrees with the
containing list; `group` agrees with `members`, the group graph is acyclic; the UUID index points
at live items carrying that UUID; every `net` is known or `UNCONNECTED`; no slot is `Taken`.

## 5. Coordinate frames (read this twice)

Board frame = what C++ getters without `Lib` in their name return. Library frame = the
footprint's own frame before `TRANSFORM_TRS` (translate, rotate, non-uniform scale, then flip).
The C++ strategy is kept **per type**, so rounding happens at the same places:

| type | stored | board-frame value |
|---|---|---|
| `Pad` | `lib_pos`, `lib_orientation`, padstack sizes / offsets / drill in lib frame | derived on read: `pad.position(ctx)`, `pad.size(ctx, layer)`, `pad.offset(ctx, layer)`, `pad.orientation(ctx)`, `pad.drill_size(ctx)` |
| `PcbText`, `PcbField`, dimension text, barcode text | `EdaText::lib_pos`, attribute size / thickness, `lib_text_angle` in lib frame | derived: `text.text_pos(ctx)`, `text_size(ctx)`, `text_thickness(ctx)`, `text_angle(ctx)` |
| `Zone` in a footprint | `lib_outline` | derived: `zone.board_outline(ctx)`; board zones: lib == board |
| `PcbDimension`, `PcbBarcode` | lib frame | derived on read |
| `PcbShape`, `PcbTextbox`, `PcbTablecell` | **both**: `lib: PcbShapeLib` (authoritative) and baked `shape: EdaShape` | baked copy, refreshed by `on_footprint_transformed` |
| `PcbPoint` | **both**: `lib_pos` and baked `pos` | baked |
| `PcbVia`, `PcbTrack`, `PcbArc`, board-level items | board frame (no parent transform) | stored |

Rules that make confusion hard:

1. Every field whose frame depends on the parent is `pub(crate)` and named `lib_*` (or lives in
   `PcbShapeLib`). It is read outside the crate through `lib_*()` accessors only (the writer
   needs them) and never through a board-named accessor.
2. A board-frame getter of a footprint child **always** takes `&BoardCtx` (or is called on an
   `ItemRef`, which carries one); a board-frame setter takes `&MutEnv`. There is no context-free
   `position()` on `Pad`, `PcbText`, `Zone`, `PcbDimension`, `PcbBarcode`.
3. Baked copies (`PcbShape::shape`, `PcbPoint::pos`) are readable without context because they
   *are* board frame; they are written only by the C++-mirroring setters, which call
   `sync_lib_coords(env)` afterwards, and by `on_footprint_transformed(env)` /
   `on_footprint_rescaled(env, ..)`, which call `rebake_from_lib(env)`.
4. `Footprint::transform` and `flipped` are `pub(crate)`; only `Board::footprint_*` functions
   (ports of `FOOTPRINT::SetPosition`, `SetOrientation`, `Flip`, `SetTransformScale`,
   `RescaleAroundPoint`, `MoveAnchorPosition`, `SetLayerAndFlip`, `Move`, `Rotate`) write them,
   and each ends with the C++ loop over children calling the hooks.
5. The parser fills lib-frame values exactly as the C++ parser does (`SetLibTextPos`,
   `SetLibSize`, `OverrideLibCoords`); the writer writes `lib_*` values directly where C++ writes
   `GetLibrary*()` and uses the same `unbake` / `InverseApply` helpers where C++ does.
6. Pad custom-shape primitives are in the **pad** frame, re-baked with
   `PAD::GetPrimitiveLibScale()`; `PCB_SHAPE::transformFp()` is null for them.
7. Every model package that touches a frame-dependent type tests rotated, flipped **and
   non-uniformly scaled** footprints against the oracle (`test_footprint_transform_sync.cpp`).

## 6. Polymorphic access and the shared behaviour API

### 6.1 Context

```rust
/// Everything a read-only item method may reach besides `self` (C++ `GetBoard()`,
/// `bds.m_DRCEngine`, `GetBoard()->GetConnectivity()`). Cheap, `Copy`.
#[derive(Clone, Copy)]
pub struct BoardCtx<'a> {
    pub board: &'a Board,
    /// `None` reproduces the C++ `if( !m_DRCEngine )` branches (own clearance 0, mask expansion
    /// from item / footprint / board values, width constraint from the netclass, ...).
    pub rules: Option<&'a dyn RuleResolver>,
    /// `None` reproduces a board whose CONNECTIVITY_DATA is empty: conditionally flashed
    /// pads / vias are not connected on any layer (FlashLayer returns false where the C++ asks
    /// connectivity). Plotting copper and DRC must pass a real one. Note: an item whose parent
    /// chain does not reach the board (`ParentRef::None`) is "GetBoard() == nullptr" in C++ and
    /// takes the no-board branch (e.g. FlashLayer returns true) whatever the context holds.
    pub connectivity: Option<&'a dyn ConnectivityQuery>,
}
impl<'a> BoardCtx<'a> {
    pub fn bare(board: &'a Board) -> Self;                          // parser, writer, simple tools
    pub fn with_rules(self, r: &'a dyn RuleResolver) -> Self;
    pub fn with_connectivity(self, c: &'a dyn ConnectivityQuery) -> Self;
    pub fn item(&'a self, id: ItemId) -> Option<ItemRef<'a>>;      // handle -> BOARD_ITEM*
    pub fn resolve(&'a self, uuid: &Kiid) -> Option<ItemRef<'a>>;  // BOARD::ResolveItem
    /// A value outside the board (dummy pad of the filler, DRC helper shape). Its `parent`
    /// still resolves, so a clone of a footprint pad keeps its board-frame geometry.
    pub fn detached(&'a self, item: AnyItem<'a>) -> ItemRef<'a>;
    // forwards of the null-board defaults, used by item code:
    pub fn max_error(&self) -> i32;                 // BOARD_ITEM::GetMaxError
    pub fn copper_layer_count(&self) -> i32;        // BoardCopperLayerCount
    pub fn flip_layer(&self, layer: PcbLayerId) -> PcbLayerId;
}
```

`BoardCtx` is passed as `ctx: &BoardCtx<'_>`, always the first argument after `self`.

### 6.2 Traits (ports of the `EDA_ITEM` / `BOARD_ITEM` / `BOARD_CONNECTED_ITEM` virtuals)

Default bodies are the C++ base-class bodies (`include/board_item.h`, `pcbnew/board_item.cpp`,
`board_connected_item.cpp`); a type overrides exactly what its C++ class overrides. Every method
that can reach beyond `self` takes `ctx`, uniformly (so `PCB_GROUP::GetLayerSet`, which unions
its members, needs no special case). Implemented for every arena type, for `PcbTrack`, `PcbArc`,
`PcbVia` and for `Track` (delegating), and for inline items.

```rust
pub trait BoardItem: Invalidate {
    // identity / structure (no context)
    fn base(&self) -> &BoardItemBase;
    fn base_mut(&mut self) -> &mut BoardItemBase;
    fn type_(&self) -> KicadT;                                    // Type()
    fn class_name(&self) -> &'static str;                         // GetClass()
    fn connected(&self) -> Option<&ConnectedItemBase> { None }    // BOARD_CONNECTED_ITEM part
    fn connected_mut(&mut self) -> Option<&mut ConnectedItemBase> { None }
    fn uuid(&self) -> &Kiid { &self.base().uuid }
    // layers
    fn layer(&self, ctx: &BoardCtx<'_>) -> PcbLayerId { self.base().layer }        // GetLayer
    fn layer_set(&self, ctx: &BoardCtx<'_>) -> Lset;                               // GetLayerSet
    fn is_on_layer(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool;           // IsOnLayer
    fn is_on_copper_layer(&self, ctx: &BoardCtx<'_>) -> bool;                       // IsOnCopperLayer
    fn is_side_specific(&self, ctx: &BoardCtx<'_>) -> bool;                         // IsSideSpecific
    fn is_layer_agnostic(&self) -> bool { false }
    fn fits_enabled_layers(&self, ctx: &BoardCtx<'_>, enabled: Lset, copper_layer_count: i32) -> bool;
    // geometry (board frame)
    fn position(&self, ctx: &BoardCtx<'_>) -> Vector2I;                             // GetPosition
    fn center(&self, ctx: &BoardCtx<'_>) -> Vector2I { self.bounding_box(ctx).centre() }
    fn bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I;                            // GetBoundingBox
    /// GetEffectiveShape( aLayer, aFlash, aUsage ); `PcbLayerId::UndefinedLayer` = all layers.
    fn effective_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, flash: Flashing,
                       usage: DrcConstraintT) -> SharedShape;
    /// GetEffectiveHoleShape; `None` where C++ returns null.
    fn effective_hole_shape(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId,
                            usage: DrcConstraintT) -> Option<Arc<ShapeSegment>> { None }
    fn transform_shape_to_polygon(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet,
        layer: PcbLayerId, clearance: i32, error: i32, error_loc: ErrorLoc, ignore_line_width: bool);
    fn transform_shape_to_poly_set(&self, ctx: &BoardCtx<'_>, buffer: &mut ShapePolySet,
        layer: PcbLayerId, clearance: i32, error: i32, error_loc: ErrorLoc);   // default forwards
    fn has_hole(&self, ctx: &BoardCtx<'_>) -> bool { false }
    fn has_drilled_hole(&self, ctx: &BoardCtx<'_>) -> bool { false }
    fn is_tented(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId) -> bool { false }
    fn has_line_stroke(&self) -> bool { false }
    fn stroke(&self, ctx: &BoardCtx<'_>) -> StrokeParams;
    fn hit_test(&self, ctx: &BoardCtx<'_>, pos: Vector2I, accuracy: i32) -> bool { false }
    fn hit_test_rect(&self, ctx: &BoardCtx<'_>, rect: &Box2I, contained: bool, accuracy: i32) -> bool { false }
    // state
    fn is_locked(&self, ctx: &BoardCtx<'_>) -> bool;      // parent group, FPHOLDER, m_isLocked; PAD overrides
    fn is_knockout(&self) -> bool { self.base().is_knockout }
    // structure
    fn run_on_children(&self, ctx: &BoardCtx<'_>, mode: RecurseMode, f: &mut dyn FnMut(ItemRef<'_>)) {}
    // text used verbatim in DRC reports
    fn item_description(&self, ctx: &BoardCtx<'_>, units: &dyn UnitsProvider, full: bool) -> String;
    fn friendly_name(&self, ctx: &BoardCtx<'_>) -> String;
    fn layer_mask_describe(&self, ctx: &BoardCtx<'_>) -> String;
}

pub trait BoardConnectedItem: BoardItem {
    fn net_code(&self) -> NetCode { self.connected().map_or(NetCode::ORPHANED, |c| c.net) }
    fn is_connected(&self, ctx: &BoardCtx<'_>) -> bool { true }   // ZONE: !rule area; PCB_SHAPE: copper && no fp
    fn effective_netclass<'a>(&self, ctx: &BoardCtx<'a>) -> &'a Netclass;   // see 9.2
    /// GetOwnClearance( aLayer, aSource ): `ctx.rules.cached_own_clearance`, 0 without rules.
    fn own_clearance(&self, ctx: &BoardCtx<'_>, layer: PcbLayerId, want_source: bool) -> (i32, Option<String>);
    fn local_clearance(&self) -> Option<i32> { None }
    fn clearance_overrides(&self, ctx: &BoardCtx<'_>, want_source: bool) -> Option<(i32, Option<String>)> { None }
}

/// Mutating virtuals of leaf items. Containers (footprint, group, generator, table) cascade
/// through `Board` methods instead (section 8.3).
pub trait BoardItemMut: BoardItem {
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I);
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I);                              // Move
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle);
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId);
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset) {}
    fn normalize(&mut self) {}
    fn on_footprint_transformed(&mut self, env: &MutEnv) {}
    fn on_footprint_rescaled(&mut self, env: &MutEnv, ratio_x: f64, ratio_y: f64,
                             linear_factor: f64, anchor: Vector2I, parent_rotate: EdaAngle) {}
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {}
}

/// Every item clears its derived caches here; `Board` calls it before handing out `&mut`.
pub trait Invalidate { fn invalidate_caches(&mut self); }
```

### 6.3 `ItemRef` and `AnyItem`

```rust
/// Resolved shared reference, for `match` (= `switch( Type() )` + `static_cast`).
#[derive(Clone, Copy)]
pub enum AnyItem<'a> {
    Footprint(&'a Footprint), Pad(&'a Pad), Track(&'a Track), Zone(&'a Zone),
    Shape(&'a PcbShape), Text(&'a PcbText), Field(&'a PcbField), Textbox(&'a PcbTextbox),
    Table(&'a PcbTable), Tablecell(&'a PcbTablecell), Dimension(&'a PcbDimension),
    Barcode(&'a PcbBarcode), Image(&'a PcbReferenceImage), Target(&'a PcbTarget),
    Point(&'a PcbPoint), Group(&'a PcbGroup), Generator(&'a PcbGenerator),
    Marker(&'a PcbMarker), Constraint(&'a PcbConstraint), Body3d(&'a Pcb3dBody),
    GridItem(&'a PcbGridItem), DrillMap(&'a PcbDrillMap), Net(&'a NetinfoItem),
}

/// The Rust spelling of a `BOARD_ITEM*` parameter: context + handle + item.
#[derive(Clone, Copy)]
pub struct ItemRef<'a> { pub ctx: &'a BoardCtx<'a>, id: Option<ItemId>, item: AnyItem<'a>, hole_proxy: bool }

impl<'a> ItemRef<'a> {
    pub fn id(&self) -> Option<ItemId>;                 // None for detached / inline values
    pub fn uuid(&self) -> Kiid;   pub fn type_(&self) -> KicadT;   pub fn base(&self) -> &'a BoardItemBase;
    pub fn any(&self) -> AnyItem<'a>;                   pub fn as_dyn(&self) -> &'a dyn BoardItem;
    // downcasts (static_cast / dynamic_cast), one per kind plus the families:
    pub fn as_pad(&self) -> Option<&'a Pad>;            pub fn as_footprint(&self) -> Option<&'a Footprint>;
    pub fn as_track(&self) -> Option<&'a Track>;        // PCB_TRACK*: segment, arc or via
    pub fn as_via(&self) -> Option<&'a PcbVia>;         pub fn as_arc(&self) -> Option<&'a PcbArc>;
    pub fn as_zone(&self) -> Option<&'a Zone>;
    pub fn as_shape(&self) -> Option<&'a PcbShape>;     // PCB_SHAPE*: also the box of a textbox / cell
    pub fn as_text(&self) -> Option<&'a PcbText>;       // PCB_TEXT*: also field, dimension text
    pub fn as_eda_text(&self) -> Option<&'a EdaText>;   // EDA_TEXT*
    pub fn as_eda_shape(&self) -> Option<&'a EdaShape>; // EDA_SHAPE*
    pub fn as_textbox(&self) -> Option<&'a PcbTextbox>; // PCB_TEXTBOX*: also cell
    pub fn as_table(&self) -> Option<&'a PcbTable>;     pub fn as_dimension(&self) -> Option<&'a PcbDimension>;
    pub fn as_group(&self) -> Option<&'a PcbGroup>;     // EDA_GROUP*: group or generator's group
    pub fn as_connected(&self) -> Option<&'a dyn BoardConnectedItem>;
    // relations
    pub fn parent(&self) -> Option<ItemRef<'a>>;        pub fn parent_footprint(&self) -> Option<ItemRef<'a>>;
    pub fn parent_footprint_id(&self) -> Option<FootprintId>;
    pub fn parent_group(&self) -> Option<ItemRef<'a>>;  pub fn children(&self, mode: RecurseMode) -> Vec<ItemRef<'a>>;
    // every BoardItem / BoardConnectedItem method with the context bound, same names:
    pub fn layer(&self) -> PcbLayerId;  pub fn layer_set(&self) -> Lset;  pub fn is_on_layer(&self, l: PcbLayerId) -> bool;
    pub fn position(&self) -> Vector2I; pub fn bounding_box(&self) -> Box2I;
    pub fn effective_shape(&self, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT) -> SharedShape;
    pub fn effective_hole_shape(&self, layer: PcbLayerId, usage: DrcConstraintT) -> Option<Arc<ShapeSegment>>;
    pub fn transform_shape_to_polygon(&self, buf: &mut ShapePolySet, layer: PcbLayerId, clearance: i32,
                                      error: i32, error_loc: ErrorLoc, ignore_line_width: bool);
    pub fn has_hole(&self) -> bool;  pub fn is_locked(&self) -> bool;  pub fn is_tented(&self, l: PcbLayerId) -> bool;
    pub fn item_description(&self, units: &dyn UnitsProvider, full: bool) -> String;
    pub fn max_error(&self) -> i32;  pub fn flipped_layer(&self, l: PcbLayerId) -> PcbLayerId;
    pub fn board_copper_layer_count(&self) -> i32;  pub fn layer_name(&self) -> String;
    pub fn fp_relative_position(&self) -> Vector2I;
    pub fn is_connected(&self) -> bool;                 // false for non-connected kinds
    pub fn net_code(&self) -> Option<NetCode>;          // None for non-connected kinds
    pub fn net(&self) -> Option<&'a NetinfoItem>;       pub fn netname(&self) -> &'a str;
    pub fn effective_netclass(&self) -> Option<&'a Netclass>;
    pub fn own_clearance(&self, layer: PcbLayerId) -> i32;
    pub fn own_clearance_with_source(&self, layer: PcbLayerId) -> (i32, String);
    /// Replaces toggling the HOLE_PROXY flag on the tested item (drc_test_provider_disallow).
    pub fn as_hole_proxy(self) -> ItemRef<'a>;          pub fn is_hole_proxy(&self) -> bool;
}
impl PartialEq for ItemRef<'_> { /* same id (or same address for detached); = pointer equality */ }
```

Typed per-class behaviour (`pad.flash_layer(ctx, layer)`, `via.layer_pair()`,
`zone.filled_polys_list(layer)`, `padstack.for_each_unique_layer(..)`) is inherent methods on
the structs, in the module that ports the C++ file, taking `ctx` iff the C++ body reaches beyond
`self` (rule C1). Per-layer padstack access mirrors C++: `pad.shape(layer)`,
`pad.size(ctx, layer)`, `pad.offset(ctx, layer)`, `pad.shape_pos(ctx, layer)`,
`via.width(layer)`, `padstack.effective_layer_for(layer)`, `padstack.copper_props(layer)`.

### 6.4 C++ idiom -> Rust idiom

| C++ | Rust |
|---|---|
| `BOARD_ITEM*` stored in a member / container / cache key | `ItemId` (or `PadId`, `ZoneId`, ...) |
| `BOARD_ITEM*` parameter / local | `ItemRef<'_>` |
| `switch( item->Type() ) { case PCB_PAD_T: static_cast<PAD*>( item ) ..` | `match item.any() { AnyItem::Pad(pad) => .., AnyItem::Track(Track::Via(via)) => .. }` |
| `dynamic_cast<BOARD_CONNECTED_ITEM*>( item )` | `item.as_connected()` |
| `item->GetParentFootprint()` | `item.parent_footprint()`; typed `pad.parent_footprint(ctx)` |
| `a->GetParent() == b->GetParent()` | `a.parent() == b.parent()` on `ParentRef` values |
| `item->GetBoard()->GetDesignSettings()` | `ctx.board.design_settings()` |
| `a == b` (pointers) | `a == b` on `ItemRef`, or on `ItemId` |
| `std::map<BOARD_ITEM*, X>` | `BTreeMap<ItemId, X>` (iterated) / `HashMap<ItemId, X>` (lookup only) / `SideTable<T, X>` |
| `std::set<BOARD_ITEM*, cmp>` | collect `Vec<ItemId>`, `sort_by(|a, b| cmp(ctx, *a, *b))` (stable sort is fine: set semantics, comparators total) |
| `for( PCB_TRACK* t : board->Tracks() )` | `for (id, t) in board.tracks()` |
| `for( PAD* p : fp->Pads() )` | `for (pad_id, pad) in board.footprint_pads(fp_id)` |
| `PAD dummy( *pad ); dummy.SetSize( l, s );` | `let mut d = pad.clone(); d.set_size(&env, l, s); ctx.detached(AnyItem::Pad(&d))` |

## 7. Upward hooks (no crate cycle)

Defined in `kicad-pcb/src/ctx.rs`; `DrcConstraintT` (port of `drc_constraint_types.h`),
`DrcConstraint` (value part of `DRC_CONSTRAINT`: type, `MinOptMax<i32>`, disallow flags,
zone connection, rule name, implicit flag), the `DRCE_*` code table (code <-> settings key, needed
by `BoardDesignSettings::drc_severities` and the `.kicad_pro` reader) and `ErrorLoc` live in
`kicad-pcb/src/drc_api.rs`; `kicad-drc` re-exports them.

```rust
/// Implemented by kicad_drc::DrcEngine.
pub trait RuleResolver {
    fn eval_rules(&self, ctx: &BoardCtx<'_>, constraint: DrcConstraintT,
                  a: Option<ItemRef<'_>>, b: Option<ItemRef<'_>>, layer: PcbLayerId) -> DrcConstraint;
    fn has_rules_for_constraint_type(&self, constraint: DrcConstraintT) -> bool;
    /// DRC_ENGINE::GetCachedOwnClearance; (clearance, source text if asked).
    fn cached_own_clearance(&self, ctx: &BoardCtx<'_>, item: ItemRef<'_>, layer: PcbLayerId,
                            want_source: bool) -> (i32, Option<String>);
    /// DRC_ENGINE::QueryWorstConstraint (BOARD_DESIGN_SETTINGS::GetBiggestClearanceValue).
    fn query_worst_constraint(&self, constraint: DrcConstraintT) -> Option<DrcConstraint>;
}

/// Implemented by kicad_connectivity::ConnectivityData.
pub trait ConnectivityQuery {
    /// CONNECTIVITY_DATA::IsConnectedOnLayer (PAD::FlashLayer, PCB_VIA::FlashLayer).
    fn is_connected_on_layer(&self, ctx: &BoardCtx<'_>, item: ItemId, layer: PcbLayerId,
                             types: &[KicadT]) -> bool;
    /// CONNECTIVITY_DATA::GetNodeCount / GetPadCount (BOARD::GetNodesCount, net statistics).
    fn node_count(&self, net: Option<NetCode>) -> u32;
    fn pad_count(&self, net: Option<NetCode>) -> u32;
}
```

* Re-entrancy: `eval_rules` calls item methods, which may call `cached_own_clearance`. Engine
  caches use `Mutex` (works on wasm32) and never hold a lock across an item call.
* `DRC_ENGINE::InvalidateClearanceCache` (called from C++ setters) is replaced by the engine
  reading the change journal; the model never calls into the engine from a mutator.
* Connectivity's own `IsConnectedOnLayer` asks for `Flashing::AlwaysFlashed` shapes exactly like
  C++, so it never recurses into itself.
* Each `None` default (`rules`, `connectivity`) is the C++ null branch and has its own test.

## 8. Mutation

All structural mutation goes through `Board`; arena fields and order lists are `pub(crate)`.
While any `BoardCtx` / `ItemRef` borrowed from a board is alive the board cannot be mutated; this
is the design, not an obstacle: "compute with `&Board`, then apply with `&mut Board`".

### 8.1 Add / remove

```rust
pub enum AddMode { Insert, Append, BulkAppend, BulkInsert }    // ADD_MODE
pub enum RemoveMode { Normal, Bulk }                           // REMOVE_MODE
impl Board {
    // BOARD::Add: list choice per kind, push_front / push_back per mode (fact 3), parent,
    // UUID bind (subtree, BindSubtree order), time stamp, journal `Added`. Tracks on a
    // non-copper layer and other C++ refusals return Err with the item.
    pub fn add_track(&mut self, track: Track, mode: AddMode) -> TrackId;
    pub fn add_zone(&mut self, zone: Zone, mode: AddMode) -> ZoneId;
    pub fn add_footprint(&mut self, fp: FootprintTree, mode: AddMode) -> FootprintId;
    pub fn add_shape(&mut self, s: PcbShape, mode: AddMode) -> ShapeId;   // ... one per board-level kind
    pub fn add(&mut self, item: OwnedItem, parent: ParentRef, mode: AddMode) -> Result<ItemId, OwnedItem>;
    // FOOTPRINT::Add on a board member (front-insert unless mode == Append, fields append):
    pub fn footprint_add(&mut self, fp: FootprintId, item: OwnedItem, mode: AddMode) -> Result<ItemId, OwnedItem>;
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<NetCode>;        // NETINFO_LIST::AppendNet
    pub fn add_marker(&mut self, m: PcbMarker) -> MarkerId;
    // BOARD::Remove / FOOTPRINT::Remove + delete: cascades to children, leaves its group,
    // unbinds UUIDs, time stamp, journal `Removed`. `false` for a stale handle.
    pub fn remove(&mut self, id: ItemId) -> bool;
    pub fn remove_many(&mut self, ids: &[ItemId]) -> usize;               // one retain pass per list
    pub fn remove_all(&mut self, types: &[KicadT]);                       // BOARD::RemoveAll
    pub fn footprint_take(&mut self, fp: FootprintId) -> Option<FootprintTree>;   // remove, keep data
    pub fn add_to_group(&mut self, group: ItemId, member: ItemId);
    pub fn remove_from_group(&mut self, member: ItemId);
    pub fn rebind_item_uuid(&mut self, id: ItemId, new_uuid: Kiid);       // BOARD_ITEM::SetUuid
}
```

### 8.2 Leaf edits: `modify_*` and `MutEnv`

```rust
/// Snapshot of what C++ mutators read through GetParentFootprint() / GetBoard().
#[derive(Clone)]
pub struct MutEnv {
    pub parent: Option<ParentFrame>,       // None: board-level item
    pub layers: Arc<LayerEnv>,             // copper count, enabled layers, BOARD::FlipLayer table
    pub max_error: i32,
    pub board_use: BoardUse,
}
#[derive(Clone, Copy)]
pub struct ParentFrame { pub footprint: FootprintId, pub transform: TransformTrs,
                         pub layer: PcbLayerId, pub flipped: bool }
impl MutEnv { pub fn detached() -> MutEnv; }   // no board: the C++ null-board defaults

impl Board {
    pub fn mut_env(&self, id: ItemId) -> MutEnv;
    /// Invalidate the item's caches, run `f`, bump the time stamp, invalidate the parent
    /// footprint's geometry caches, journal `Modified` (with the pre-edit net and layers).
    pub fn modify<R>(&mut self, id: ItemId, f: impl FnOnce(AnyItemMut<'_>, &MutEnv) -> R) -> Option<R>;
    pub fn modify_track<R>(&mut self, id: TrackId, f: impl FnOnce(&mut Track, &MutEnv) -> R) -> Option<R>;
    pub fn modify_pad<R>(&mut self, id: PadId, f: impl FnOnce(&mut Pad, &MutEnv) -> R) -> Option<R>;
    // ... one per kind (generated)
    // polymorphic virtuals that may cascade (footprint, group, generator, table):
    pub fn move_item(&mut self, id: ItemId, delta: Vector2I);
    pub fn rotate_item(&mut self, id: ItemId, centre: Vector2I, angle: EdaAngle);
    pub fn flip_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection);
    pub fn mirror_item(&mut self, id: ItemId, centre: Vector2I, dir: FlipDirection);
    pub fn set_item_layer(&mut self, id: ItemId, layer: PcbLayerId);
    /// BOARD_CONNECTED_ITEM::SetNetCode with the board lookup: 0 off copper, unknown code -> -1.
    pub fn set_item_net(&mut self, id: ItemId, net: NetCode, no_assert: bool) -> bool;
    pub fn set_item_locked(&mut self, id: ItemId, locked: bool);
    /// Escape hatch for mutators whose C++ body reads other items (PCB_DIMENSION_BASE::Update
    /// with text variables, generator updates): the item is moved out (`Taken`), `f` gets it and
    /// a full read-only context of the rest, then it is put back. Never for footprints.
    pub fn with_item_taken<R>(&mut self, id: ItemId, rules: Option<&dyn RuleResolver>,
                              f: impl FnOnce(AnyItemMut<'_>, &BoardCtx<'_>) -> R) -> Option<R>;
}
```

### 8.3 Footprint cascades

`FOOTPRINT::SetPosition/SetOrientation/Flip/SetTransformScale/RescaleAroundPoint/
MoveAnchorPosition/SetLayerAndFlip/Move/Rotate` become `Board::footprint_set_position(id, pos)`
etc. in `footprint/transform.rs`. The body destructures `Board` into disjoint arena borrows,
updates `transform`, builds one `MutEnv` with the new `ParentFrame` and runs the C++ loop body on
each child list in C++ order (fields, pads, zones, points, drawings by kind, table cells), then
patches the footprint's geometry / courtyard caches exactly where C++ patches them instead of
dropping them. Groups and generators cascade the same way (`move_item` on a group moves members).

### 8.4 Derived-state writers (do not invalidate, do not bump the time stamp)

Results that another subsystem computes and the file stores: zone fills
(`set_filled_polys_list`, `set_fill_flag`, `set_is_filled`, `set_need_refill`,
`set_is_island` / `insulated_islands`, `build_hash_value`, `cache_triangulation`),
`zone_layer_overrides` on pads and vias, courtyard / net-tie caches
(`build_courtyard_caches`, `build_net_tie_cache`, `MALFORMED_*_COURTYARD` flags). They are
reached through `board.zone_fill_mut(id) -> &mut ZoneFill`,
`board.set_zone_layer_override(id, layer, value)`, `board.clear_zone_layer_overrides(id)`,
`board.footprint_caches_mut(id)`. C++ does not bump `m_timeStamp` for these either.

### 8.5 Change journal and commits

```rust
pub enum ChangeKind { Added, Removed, Modified }
pub struct Change { pub kind: ChangeKind, pub id: ItemId, pub uuid: Kiid, pub kicad_t: KicadT,
                    pub net_before: Option<NetCode>, pub layers_before: Lset }
impl Board {
    pub fn set_journal_enabled(&mut self, on: bool);    // off while parsing (aSkipConnectivity)
    pub fn drain_changes(&mut self) -> Vec<Change>;
    pub fn time_stamp(&self) -> i32;                     // BOARD::GetTimeStamp
    pub fn increment_time_stamp(&mut self);              // also clears BoardCaches
}

/// Non-GUI core of BOARD_COMMIT / COMMIT (board_commit.cpp, common/commit.cpp).
pub struct BoardCommit { /* Added(ItemId) | Removed { id, list_pos } (parked) | Modified { id, before: OwnedItem } */ }
impl BoardCommit {
    pub fn new() -> Self;
    pub fn add(&mut self, board: &mut Board, item: OwnedItem, parent: ParentRef, mode: AddMode) -> ItemId;
    pub fn modify(&mut self, board: &Board, id: ItemId);          // snapshot before the edit
    pub fn remove(&mut self, board: &mut Board, id: ItemId);      // park item and children
    pub fn push(self, board: &mut Board) -> Vec<Change>;          // purge parked items
    pub fn revert(self, board: &mut Board);                       // restore under the same handles
}
```

Consumers that live next to the board (connectivity, a persistent DRC R-tree, the router world,
the wasm id cache) consume `drain_changes()` after each batch. `Board: Clone` (caches and journal
start empty in the clone, all handles stay valid) is the heavyweight alternative for speculative
autorouter passes.

Scratch flags: `BoardItemBase::flags` exists for non-algorithmic code (`STRUCT_DELETED`,
`IS_NEW`, `MALFORMED_*`); algorithms marking items while reading (`SKIP_STRUCT`, `CANDIDATE`,
`m_arflag`, `PAD::m_subRatsnest`) use a `SideTable` or `BTreeSet<ItemId>` of their own.

## 9. Derived caches and invalidation

Cache cells (in `cache.rs`), both `Sync`, both `Clone` to **empty**, both ignored by equality:

```rust
/// Built once from `&self`, cleared only through `&mut self` (Invalidate).
pub struct OnceCache<T>(std::sync::OnceLock<T>);
/// Re-validated from `&self` (C++ `mutable` + mutex + time stamp / dirty check). `T` stores the
/// inputs it was built from (time stamp, parent transform, text ...); `valid` compares them, so
/// a forgotten invalidation cannot produce a stale read.
pub struct KeyedCache<T>(std::sync::Mutex<Option<Arc<T>>>);
impl<T> KeyedCache<T> {
    pub fn get_or_build(&self, valid: impl Fn(&T) -> bool, build: impl FnOnce() -> T) -> Arc<T>; // build runs unlocked
    pub fn invalidate(&self);  pub fn clear(&mut self);
    pub fn update(&mut self, f: impl FnOnce(&mut T)) where T: Clone;  // patch in place (FOOTPRINT::Flip on caches)
}
```

Three classes, three rules:

1. **Derived from the item and its parent** (C++ `mutable` members): stored in the item.
   Pad shapes / polygons / mask shapes (`KeyedCache` validated against `PadCacheKey` = parent
   transform + flip + `max_error`, also cleared by `set_dirty()` where C++ calls `SetDirty()`), footprint bbox /
   text-excluded bbox / hull (keyed by board time stamp, as C++), zone bbox (time stamp), text
   render / bbox / knockout caches (key = the inputs C++ compares), shape hatching, barcode
   cache. `EDA_TEXT::m_shown_text` stays an eagerly maintained plain field (as
   `cacheShownText`). Dimension `shapes` are a plain field rebuilt by `update()`.
2. **Results written by another subsystem and saved in the file**: section 8.4.
3. **Spanning items**: connectivity graph, ratsnest, DRC R-trees, `m_DRCZones`,
   `m_Intersects*Cache`, island maps, own-clearance cache, drill symbol cache. **Not in
   `Board`.** They live in the consumer (`kicad_connectivity::ConnectivityData`,
   `kicad_drc::DrcBoardCaches`), keyed by `ItemId` (iterated maps: `BTreeMap`), stamped with
   `board.time_stamp()` and rebuilt or updated from the journal. Board-level caches used by
   model code itself (`m_maxClearanceValue`, `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`,
   `m_StackedMicroviaCache`, `m_footprintCourtyardIndex`, `m_LayerExpressionCache`, board
   outline) are `Board::caches: BoardCaches`, keyed by time stamp.

## 10. Board-level services

All `impl Board` unless stated; C++ origin `pcbnew/board.cpp` unless stated.

```rust
// ---- iteration in C++ container order ----
pub fn tracks(&self) -> impl Iterator<Item = (TrackId, &Track)> + '_;              // Tracks()
pub fn footprints(&self) -> impl Iterator<Item = (FootprintId, &Footprint)> + '_;   // Footprints()
pub fn drawings(&self) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_;         // Drawings()
pub fn zones(&self) -> impl Iterator<Item = (ZoneId, &Zone)> + '_;                  // Zones()
pub fn groups(&self) -> ..; pub fn generators(&self) -> ..; pub fn constraints(&self) -> ..;
pub fn board_points(&self) -> ..; pub fn markers(&self) -> ..;
pub fn footprint_fields(&self, fp: FootprintId) -> impl Iterator<Item = (FieldId, &PcbField)> + '_;
pub fn footprint_pads(&self, fp: FootprintId) -> impl Iterator<Item = (PadId, &Pad)> + '_;
pub fn footprint_drawings(&self, fp: FootprintId) -> impl Iterator<Item = (ItemId, AnyItem<'_>)> + '_;
pub fn footprint_zones(&self, fp: FootprintId) -> ..; footprint_groups, footprint_points,
    footprint_constraints, footprint_bodies;
pub fn table_cells(&self, t: TableId) -> impl Iterator<Item = (TablecellId, &PcbTablecell)> + '_;
pub fn group_members(&self, g: ItemId) -> impl Iterator<Item = ItemId> + '_;
pub fn track_ids(&self) -> Vec<TrackId>; pub fn footprint_ids(&self) -> Vec<FootprintId>;  // snapshot before mutating loops
pub fn get_pads(&self) -> Vec<PadId>;                       // GetPads: footprint order x pad order
pub fn all_connected_items(&self) -> Vec<ItemId>;           // AllConnectedItems (order below)
pub fn collect_owned_items(&self) -> Vec<ItemId>;
pub fn get_item_set(&self) -> Vec<ItemId>;                  // GetItemSet: by UUID, ties by ItemId
pub fn run_on_children(&self, mode: RecurseMode, f: &mut dyn FnMut(ItemId));
pub fn get_zone_list(&self, include_zones_in_footprints: bool) -> Vec<ZoneId>;
pub fn tracks_in_net(&self, net: NetCode) -> Vec<TrackId>;
pub fn get_sorted_pad_list_by_x_then_y_coord(&self, ctx: &BoardCtx<'_>, net: Option<NetCode>) -> Vec<PadId>;

// ---- lookup ----
pub fn get(&self, id: ItemId) -> Option<AnyItem<'_>>;  pub fn contains(&self, id: ItemId) -> bool;
pub fn track(&self, id: TrackId) -> Option<&Track>;    // typed getters, one per kind
pub fn resolve_item(&self, uuid: &Kiid) -> Option<ItemId>;          // ResolveItem (nil -> None)
pub fn uuid_of(&self, id: ItemId) -> Option<Kiid>;  pub fn repair_duplicate_item_uuids(&mut self) -> usize;
pub fn find_footprint_by_reference(&self, reference: &str) -> Option<FootprintId>;
pub fn find_footprint_by_path(&self, path: &KiidPath) -> Option<FootprintId>;
pub fn find_pad_by_uuid(&self, uuid: &Kiid) -> Option<PadId>;
pub fn get_pad_at(&self, ctx: &BoardCtx<'_>, pos: Vector2I, layers: Lset) -> Option<PadId>;   // GetPad
pub fn convert_cross_references_to_kiids(&self, source: &str) -> String;
pub fn convert_kiids_to_cross_references(&self, source: &str) -> String;
pub fn groups_sanity_check(&mut self, repair: bool) -> String;
pub fn check_invariants(&self) -> Result<(), String>;

// ---- nets and netclasses ----
pub fn net_info(&self) -> &NetinfoList;
pub fn nets(&self) -> impl Iterator<Item = &NetinfoItem> + '_;              // by NAME (`for( net : GetNetInfo() )`)
pub fn nets_by_netcode(&self) -> impl Iterator<Item = &NetinfoItem> + '_;   // NetsByNetcode()
pub fn find_net(&self, code: NetCode) -> Option<&NetinfoItem>;
pub fn find_net_by_name(&self, name: &str) -> Option<&NetinfoItem>;
pub fn find_or_add_net(&mut self, name: &str, code: Option<NetCode>) -> NetCode;   // parser: file order
pub fn remove_net(&mut self, code: NetCode);                    // items fall back to net 0
pub fn remove_unused_nets(&mut self); pub fn build_list_of_nets(&mut self);
pub fn sanitize_netcodes(&mut self); pub fn map_nets(&mut self, dest: &mut Board);
pub fn match_dp_suffix(net_name: &str) -> (i32, String);     pub fn dp_coupled_net(&self, net: NetCode) -> Option<NetCode>;
pub fn get_nodes_count(&self, ctx: &BoardCtx<'_>, net: Option<NetCode>) -> u32;
pub fn set_areas_net_codes_from_net_names(&mut self) -> i32;
pub fn synchronize_nets_and_netclasses(&mut self, reset_track_and_via_sizes: bool);
pub fn apply_net_chain_netclasses(&mut self);  pub fn resolve_net_chain_terminal_pads(&mut self);
pub fn get_net_class_assignment_candidates(&self) -> BTreeSet<String>;
impl NetSettings {                                              // common/project/net_settings.cpp
    pub fn get_effective_netclass(&self, net_name: &str) -> Arc<Netclass>;
    pub fn get_cached_effective_netclass(&self, net_name: &str) -> Option<Arc<Netclass>>;
    pub fn has_effective_netclass(&self, net_name: &str) -> bool;
    pub fn get_net_class_by_name(&self, name: &str) -> Option<Arc<Netclass>>;
    pub fn clear_all_caches(&mut self); pub fn clear_cache_for_net(&mut self, net_name: &str);
    pub fn set_netclass_pattern_assignment(&mut self, pattern: &str, netclass: &str);
}

// ---- design settings, severities, exclusions, markers ----
pub fn design_settings(&self) -> &BoardDesignSettings;
pub fn design_settings_mut(&mut self) -> &mut BoardDesignSettings;   // bumps time stamp, rebuilds LayerEnv
pub fn get_max_clearance_value(&self, ctx: &BoardCtx<'_>) -> i32;    // cached per time stamp
pub fn resolve_drc_exclusions(&mut self, create_markers: bool) -> Vec<MarkerId>;
pub fn record_drc_exclusions(&mut self);
pub fn delete_markers(&mut self, warnings_and_errors: bool, exclusions: bool);
impl BoardDesignSettings {
    pub fn get_severity(&self, drc_error_code: i32) -> Severity;  pub fn ignore(&self, drc_error_code: i32) -> bool;
    pub fn get_biggest_clearance_value(&self, rules: Option<&dyn RuleResolver>) -> i32;
    pub fn get_smallest_clearance_value(&self) -> i32;  pub fn get_drc_epsilon(&self) -> i32;
    pub fn get_hole_plating_thickness(&self) -> i32;    pub fn get_layer_class(&self, l: PcbLayerId) -> usize;
    pub fn get_line_thickness(&self, l: PcbLayerId) -> i32;  pub fn get_text_size(&self, l: PcbLayerId) -> Vector2I;
    pub fn get_text_thickness(&self, l: PcbLayerId) -> i32;
    pub fn get_current_track_width(&self) -> i32;  pub fn get_current_via_size(&self) -> i32;
}

// ---- stackup and layers ----
pub fn get_stackup_or_default(&self) -> BoardStackup;        // design_settings.stackup if has_stackup
pub fn layer_depth(&self, start: PcbLayerId, end: PcbLayerId) -> i32;
pub fn copper_layer_count(&self) -> i32;  pub fn set_copper_layer_count(&mut self, n: i32);
pub fn enabled_layers(&self) -> Lset;     pub fn set_enabled_layers(&mut self, l: Lset);
pub fn is_layer_enabled(&self, l: PcbLayerId) -> bool;
pub fn layer_name(&self, l: PcbLayerId) -> String;           // user name or standard
pub fn standard_layer_name(l: PcbLayerId) -> String;
pub fn layer_id(&self, name: &str) -> PcbLayerId;            // GetLayerID
pub fn layer_type(&self, l: PcbLayerId) -> LayerT;
pub fn set_layer_descr(&mut self, l: PcbLayerId, d: Layer) -> bool;
pub fn flip_layer(&self, l: PcbLayerId) -> PcbLayerId;       // copper-count aware
pub fn copper_layer_stack_max_id(&self) -> PcbLayerId;
pub fn is_front_layer(&self, l: PcbLayerId) -> bool;  pub fn is_back_layer(&self, l: PcbLayerId) -> bool;
impl BoardStackup {
    pub fn build_default_stackup_list(&mut self, settings: Option<&BoardDesignSettings>, copper_layers: i32);
    pub fn build_board_thickness_from_stackup(&self) -> i32;
    pub fn get_layer_distance(&self, a: PcbLayerId, b: PcbLayerId) -> i32;
    pub fn synchronize_with_board(&mut self, settings: &BoardDesignSettings) -> bool;
}

// ---- title block, page, properties, variants, text variables ----
// plain pub fields: titles, paper, plot_options, properties, current_variant, variant_names
pub fn has_variant(&self, name: &str) -> bool;  pub fn add_variant(&mut self, name: &str);
pub fn delete_variant(&mut self, name: &str);   pub fn rename_variant(&mut self, old: &str, new: &str);
pub fn get_variant_description(&self, name: &str) -> String;  pub fn synchronize_properties(&mut self);
/// BOARD::ResolveTextVar( token, variant, depth ): title block, board properties,
/// built-ins, project text variables.
pub fn resolve_text_var(&self, token: &mut String, variant: &str, depth: i32) -> bool;
pub fn embedded_files(&self) -> &EmbeddedFiles;  pub fn fixup_embedded_data(&mut self);
pub fn component_class_manager(&self) -> &ComponentClassManager;

// ---- geometry ----
pub fn compute_bounding_box(&self, ctx: &BoardCtx<'_>, board_edges_only: bool, physical_layers_only: bool) -> Box2I;
pub fn board_edges_bounding_box(&self, ctx: &BoardCtx<'_>) -> Box2I;
pub fn get_board_polygon_outlines(&self, ctx: &BoardCtx<'_>, infer_outline_if_necessary: bool,
    error_handler: Option<&mut dyn OutlineErrorHandler>, allow_use_arcs_in_polygons: bool,
    include_npth_as_outlines: bool) -> (ShapePolySet, bool);
pub fn board_outline(&self, ctx: &BoardCtx<'_>) -> Arc<ShapePolySet>;    // PCB_BOARD_OUTLINE (cached)
pub fn get_track_length(&self, ctx: &BoardCtx<'_>, track: TrackId) -> (i32, f64, f64, f64, f64);

// ---- save-order comparators (free functions next to the C++ comparator's type) ----
pub fn board_item_ptr_cmp(ctx: &BoardCtx<'_>, a: ItemId, b: ItemId) -> Ordering;  // BOARD_ITEM::ptr_cmp
pub fn board_cmp_items(..); pub fn board_cmp_drawings(..); pub fn pcb_track_cmp_tracks(..);
pub fn footprint_cmp_pads(..); pub fn footprint_cmp_drawings(..); pub fn footprint_cmp_zones(..);
pub fn pcb_point_cmp_points(..);                                     // all: (ctx, a, b) -> Ordering
```

**Effective netclass, end to end**: the project-settings loader fills
`design_settings.net_settings` (classes, label / pattern / chain assignments) from `.kicad_pro`;
`Board::synchronize_nets_and_netclasses` (no-op when `board.project` is `None`, like C++) runs
`apply_net_chain_netclasses`, clears the net-settings caches and stores
`net_settings.get_effective_netclass(name)` (explicit labels, then patterns in file order, then
chain classes; several matches build a cached composite class ordered by priority, missing fields
filled from Default) into each `NetinfoItem::netclass`. `item.effective_netclass(ctx)` is then
net code -> `NetinfoItem::netclass` -> else `net_settings.default_netclass` -> else
`Netclass::fallback()` (static default). A consumer that needs *why* (DRC rule source names) uses
`Netclass::constituents` and `Netclass::parents.<field>` like C++ `GetClearanceParent()`.
**Hosts must set `board.project = Some(BoardProject::default())` when loading without a
`.kicad_pro`**: `kicad-cli` always has a project, so the oracle always synchronizes.

### 10.1 Iteration orders the C++ code relies on (normative)

| consumer / service | order |
|---|---|
| `tracks()`, `footprints()`, `drawings()` | deque order; a loaded board is file order; `Insert` prepends |
| `zones()`, `groups()`, `generators()`, `points()`, `markers()`, `constraints()` (board) | always appended |
| footprint `pads`, `drawings`, `zones`, `groups`, `constraints`, `points`, `bodies` | `Append` appends, every other mode prepends; `fields` always append |
| `all_connected_items()` | tracks; per footprint: pads, zones, connected graphics; board zones; connected board drawings (`board.cpp:3757`) |
| `Board::run_on_children` | tracks, zones, markers, groups, constraints, points, footprints (+ children), drawings (+ children) (`board.cpp:837`) |
| footprint children (`run_on_children`) | fields, pads, zones, groups, bodies, constraints, points, drawings (+ children) (`footprint.cpp:3342`) |
| DRC `for_each_geometry_item` (in `kicad-drc`) | tracks, drawings (tables then their cells), zones, per footprint: fields, pads, graphics, zones |
| `nets()` / `nets_by_netcode()` | by name / by code |
| `get_item_set()` | by UUID, ties by `ItemId` |
| writer | the ported comparators; group members as sorted UUID strings |
| group members (C++ `unordered_set`) | insertion order; record in `PORT_STATUS.md` |
| any consumer map keyed by item and iterated | `BTreeMap<ItemId, _>`; record the deviation from pointer order |

## 11. Module layout (`rust/crates/kicad-pcb/src/`)

One module per C++ file; where one C++ file is split between work packages the Rust side is a
directory whose files name the C++ function ranges they port.

```
lib.rs                     re-exports; `pub mod` list
pending.rs                 port_pending!("CLASS::Method") macro (panics with the C++ name)
arena.rs  ids.rs  items_macro.rs  side_table.rs  cache.rs           (kernel, new code)
item.rs                    BoardItemBase, ConnectedItemBase, ParentRef, ItemFlags, small enums
traits.rs                  BoardItem, BoardConnectedItem, BoardItemMut, Invalidate
item_ref.rs                ItemRef, AnyItem, AnyItemMut, OwnedItem, FootprintTree
ctx.rs                     BoardCtx, MutEnv, ParentFrame, LayerEnv, RuleResolver, ConnectivityQuery
drc_api.rs                 DrcConstraintT, DrcConstraint, ErrorLoc, DRCE_* code table
board/{mod,containers,index,journal,commit,nets,layers,queries,outline,text_vars,markers,compare}.rs
board_item.rs  board_connected_item.rs  eda_item.rs
netinfo.rs  netclass.rs  net_settings.rs  eda_pattern_match.rs
board_design_settings.rs  zone_settings.rs  teardrop_parameters.rs  board_stackup.rs
pcb_plot_params.rs  title_block.rs  page_info.rs  embedded_files.rs  component_class.rs
pcb_track/{mod,arc,via,shapes}.rs  padstack.rs  pad/{mod,derive,shapes}.rs  pad_utils.rs
footprint/{mod,transform,geometry,compare,shapes}.rs  footprint_courtyard_index.rs
zone/{mod,shapes}.rs  zone_utils.rs
eda_shape/{mod,geometry,shapes}.rs  line_ending.rs  stroke_params.rs  pcb_shape/{mod,lib_frame,shapes}.rs
eda_text.rs  pcb_text.rs  pcb_field.rs  pcb_textbox.rs  pcb_tablecell.rs  pcb_table.rs
pcb_generated_table.rs  pcb_dimension.rs  pcb_group.rs  pcb_generator.rs  pcb_target.rs
pcb_point.rs  pcb_marker.rs  pcb_barcode.rs  pcb_reference_image.rs  pcb_constraint.rs
pcb_3d_body.rs  pcb_grid_item.rs  pcb_drill_map.rs  convert_shape_list_to_polygon.rs
io/kicad_sexpr/...         parser and writer (their own work packages, see pcb-io.md)
project/...                .kicad_pro / settings loaders (see settings.md)
```

## 12. Conventions

* Naming per `PORTING.md` section 2 (`PCB_TRACK` -> `PcbTrack`, `GetEffectiveShape` ->
  `effective_shape`, `m_libPos` -> `lib_pos`, `F_Cu` -> `PcbLayerId::FCu` on the `kicad-core`
  newtype). Type mapping rules (`i32` nm, `i64` for `long long`/`ecoord`, `Option` for
  `std::optional`, `BTreeMap` for anything iterated, `Arc` for `shared_ptr`, `EdaAngle` /
  `EdaOrientation`) are listed at the top of `board-model-types.md`. C++ "inherit" sentinels stay
  enum variants (`ZoneConnection::Inherited`, `TentingMode::FromBoard`).
* Per-layer padstack access goes through `Padstack` methods that port the C++ mode logic
  (`effective_layer_for`, `for_each_unique_layer`, `ALL_LAYERS` / `INNER_LAYERS` keys); never
  index `copper_props` directly outside `padstack.rs`.
* No derived `PartialEq` on items (`operator==` / `Compare` are ported as `eq_item` / `compare`).
  `Default` = C++ constructor with a nil UUID; `Type::new(&mut KiidGenerator, ..)` draws a UUID.
* Every public fn names its C++ origin in its doc comment; behaviour stubs use
  `port_pending!("PAD::BuildEffectiveShapes")` until their package lands;
  `rg port_pending! crates/kicad-pcb` must be empty before the crate is declared done.

## 13. How each consumer uses the model

**Parser** (`pcb_io_kicad_sexpr_parser.cpp`, spec `analysis/pcb-io.md`). Journal off. Board-level
items are built as values and added with `AddMode::BulkAppend`. A footprint is built as a
`FootprintTree` value (children in C++ add order, lib-frame values as read; `(at)`/`(transform)`
applied through the same functions C++ calls, in the same order, so the mandatory fields'
hooks run) and moved in with `add_footprint`, which binds UUIDs in `BindSubtree` order. Nets are
created by name on first use (`find_or_add_net`) so codes match C++. Group members, generator
members and net-chain terminal pads are read as `Kiid`s and resolved after the loop
(`resolve_groups`, `resolve_net_chain_terminal_pads`) like C++. Post-load fix-ups use
`modify_*`.

**Writer** (`pcb_io_kicad_sexpr.cpp`). Takes `&BoardCtx` (bare). Builds `Vec<ItemId>` from the
order lists, sorts with the ported comparators, formats; footprints from their child lists.
Lib-frame values via `lib_*()` accessors, verbatim payloads (embedded files, image data, drill
symbol profile) re-emitted as stored.

**Item shapes** (`GetEffectiveShape`, `TransformShapeToPolygon`). Inherent methods in the
`*/shapes.rs` modules, exposed through `BoardItem`. Results `SharedShape` (`Arc<dyn Shape>`,
`Send + Sync`), pad results from `PadCaches`. Flashing asks `ctx.connectivity` and
`zone_layer_overrides`, mask expansion asks `ctx.rules`, exactly where C++ asks the board.

**Connectivity + ratsnest** (`pcbnew/connectivity/`). `CN_ITEM::m_parent` -> `ItemId` (zone
layers: `ZoneId` + layer + outline index); clusters, per-net lists and RN graphs hold handles.
`ConnectivityData::build(&ctx)` walks `all_connected_items()`; `apply_changes(&ctx, changes)`
consumes the journal (`Added` -> add, `Removed` -> invalidate by handle, `Modified` -> mark
`net_before` dirty + re-add). Net propagation (`PropagateNets` -> `SetNetCode`) collects
`(ItemId, NetCode)` pairs and applies them with `board.set_item_net` afterwards. Implements
`ConnectivityQuery`.

**DRC engine + providers** (`pcbnew/drc/`, spec `analysis/drc.md`). A run holds `&BoardCtx`
(with connectivity and `rules = Some(&engine)`) for its whole duration, after a `&mut Board`
preparation phase (`build_courtyard_caches`, `build_net_tie_cache`, triangulation,
`synchronize_nets_and_netclasses`). `DRC_RTREE::ITEM_WITH_SHAPE` ->
`{ parent: ItemId, shape: SharedShape, parent_shape: Option<SharedShape> }`; visitors resolve
with `ctx.item(id)`. Pair caches key on `(ItemId, ItemId, PcbLayerId)`. Violations carry `Kiid`s
(`RcItem::ids`); markers are added with `board.add_marker` after the providers ran. Dummy
shapes are locals viewed with `ctx.detached`. The engine implements `RuleResolver`.

**Zone filler** (`zone_filler.cpp`). Port pass by pass: each read pass is
`fn pass_n(ctx: &BoardCtx, ..) -> PassNResult`, each write pass `apply_pass_n(&mut Board, ..)`
through the section 8.4 writers (zone-layer overrides, fills, flags, islands, hashes). Earlier
fills are visible to later passes because they are stored before the next read pass, exactly as
the C++ ordering requires; never reorder a write relative to the reads that follow it. Dummy pads
and vias for thermal spokes are clones viewed with `ctx.detached`. Teardrop zones are added with
`add_zone` and removed with `remove_many`.

**Gerber / drill plotting** (`plot_board_layers.cpp`, `plot_brditems_plotter.cpp`, `gendrill_*`,
spec `analysis/plot-fab.md`). Pure readers over a `BoardCtx` *with connectivity* (flashing of
unconnected pad layers affects copper plots). Footprint graphics via `footprint_drawings` +
`match AnyItem`; the "plot a modified copy of the pad" idiom is a clone + `detached`. Gerber X2
attributes from `item.netname()`, `pad.number`, the parent footprint's reference field. Drill
files iterate vias in `tracks()` and `get_pads()`, read padstack drills, `via_type`, `PadAttrib`.

**Settings** (`analysis/settings.md`). The project-settings packages own JSON parsing and write
into the plain-data structs here (`BoardDesignSettings`, `NetSettings`, severities, exclusions,
`BoardProject::text_vars`); the model defines data, defaults (C++ constructors) and logic.

**CLI** (`kicad-rs pcb drc | export gerbers | export drill`). Owns
`Design { board, connectivity, drc }`; each sub-command is a sequence of borrow phases: load,
`synchronize_nets_and_netclasses`, build connectivity, optional refill, run, write.

**External autorouter client.** Load -> enumerate (`tracks()`, `get_pads()`, `nets()`, zones,
`board_outline`) -> `BoardCommit` with `add` / `remove` / `modify_track` (handles of untouched
items stay valid) -> `drain_changes` into connectivity -> refill zones -> DRC -> export.
Speculative passes use `BoardCommit::revert` (same handles come back) or `board.clone()`.

**Push-and-shove router (stretch).** `PNS::ITEM::m_parent` -> `Option<ItemId>`;
`PNS_KICAD_IFACE::SyncWorld` walks the board; the rule resolver adapter calls the same
`RuleResolver`; `Commit()` maps to `BoardCommit`; interactive revert restores under the original
handles so PNS parent links stay valid. `PNS::NODE` remains the router's own world.

## 14. wasm / JS

wasm-bindgen cannot export references into Rust collections, so every design needs an id at the
boundary; here the model's native id *is* that id. `kicad-wasm` exports one `Design` class:

```ts
class Design {                                   // Id = number (ItemId::to_bits); stale ids throw
  static load(pcb: string, project?: string, rules?: string): Design;  save(): string;
  resolve(uuid: string): Id | undefined;  uuidOf(id: Id): string;  isAlive(id: Id): boolean;  kindOf(id: Id): string;
  trackIds(): Float64Array;  padIds(): Float64Array;  zoneIds(): Float64Array;  footprintIds(): Float64Array;
  tracksPacked(): Int32Array;   // [kind, x1, y1, x2, y2, width, layer, net] * n, C++ order; also viasPacked, padsPacked
  item(id: Id): object;  nets(): { code: number; name: string; netclass: string }[];   // JSON from ItemRef, board frame
  addTrack(t: object): Id;  addArc(a: object): Id;  addVia(v: object): Id;  update(id: Id, patch: object): void;
  remove(id: Id): boolean;  beginCommit(): void;  commit(): void;  revert(): void;
  rebuildConnectivity(): void;  ratsnest(): Int32Array;  refillZones(ids?: Id[]): void;
  runDrc(options?: object): object[];   // violations carry UUIDs, positions, severities, texts
  plotGerber(layer: string, options?: object): string;  drillFiles(options?: object): object[];
}
```

Ids are valid for one `Design`; UUID strings are what JS keeps across `save()` / `load()`.

## 15. Rules for implementers (checklist)

1. Store `ItemId`s, pass `ItemRef` / `&BoardCtx`, persist `Kiid`. Never store `&Board`, `&T`
   or `ItemRef` in a struct that outlives one read pass.
2. Never iterate an arena for semantics; use the order lists / `Board` iterators.
3. Board-frame getters of footprint children take `ctx`; lib-frame values are `lib_*`. Never
   apply a footprint transform by hand outside `footprint/transform.rs` and the per-type
   derive functions.
4. A method takes `ctx` iff its C++ body reaches the parent, the board, another item, the net
   object or a hook (rule C1); trait methods always take it.
5. Mutate only through `Board` methods; derived results through the section 8.4 writers. Do not
   add `pub` to arena fields or order lists.
6. `rules == None` / `connectivity == None` must behave exactly like the C++ null branches; test
   each.
7. Keep C++ container order: `push_front` where C++ does, `Append`-only exceptions of
   `FOOTPRINT::Add` included.
8. `BTreeMap`/`BTreeSet` for anything iterated; pointer-order tie-breaks become `ItemId` order
   and are recorded in `PORT_STATUS.md`.
9. Default = C++ constructor with nil UUID; new UUIDs only from `board.kiid_generator`.
10. Caches: per-item derived data in `OnceCache`/`KeyedCache` with every external input in the
    key; nothing spanning items inside `Board`.
11. Algorithm scratch flags go to side tables, not `base.flags`.
12. `Board::check_invariants()` after every mutating test step.
13. Test frame-dependent behaviour with rotated, flipped and non-uniformly scaled footprints.
14. Replace `port_pending!` markers of your package; leave none on reachable paths.
15. Do not change a signature in this document or in `board-model-types.md` without the
    orchestrator; add new methods freely in your own modules.

## 16. Work packages for the model

Line counts are C++ lines actually ported (UI, property registration, protobuf, `View*`,
`GetMsgPanelInfo`, `Serialize` excluded). Every package ports the matching `qa/tests/pcbnew`
tests, keeps `check_invariants` green and replaces its `port_pending!` markers. File ownership is
disjoint (Rust allows `impl` blocks for one type in several modules of the crate).

| WP | Title | Rust modules | C++ sources | ~lines | depends on |
|---|---|---|---|---|---|
| **M0** | Types-only skeleton | every module of section 11 except `io/`, `project/`: all structs/enums of `board-model-types.md` with `Default` = C++ constructor defaults, the kernel (`arena`, `ids`, `items_macro`, `side_table`, `cache`, `ctx`, `item_ref`, `traits`, `drc_api`, `pending`), every public signature of this document with `port_pending!` bodies, trait impls for every type | all item/settings headers (`board.h`, `footprint.h`, `pad.h`, `padstack.h`, `pcb_track.h`, `zone.h`, `zone_settings.h`, `pcb_shape.h`, `eda_shape.h`, `eda_text.h`, `pcb_text.h`, `pcb_field.h`, `pcb_textbox.h`, `pcb_table*.h`, `pcb_dimension.h`, `pcb_group.h`, `eda_group.h`, `pcb_generator.h`, small items, `netinfo.h`, `netclass.h`, `net_settings.h`, `board_design_settings.h`, `board_stackup.h`, `pcb_plot_params.h`, `eda_item.h`, `board_item.h`, `drc_constraint_types.h`, `drc_rule.h`, `drc_item.h` code list), constructors | 3000 (headers) | foundation type names |
| M1 | Containers, index, journal, commit | `board/{containers,index,journal,commit}.rs` | `board.cpp` Add/Remove/RemoveAll/FinalizeBulk*/ResolveItem/RepairDuplicateItemUuids/GetItemSet/collectOwnedItems/RunOnChildren/AllConnectedItems, `footprint.cpp` Add/Remove/RunOnChildren, `board_item_index.cpp`, `common/eda_group.cpp`, `board_commit.cpp` (semantics) | 1800 | M0 |
| M2 | Item base | `board_item.rs`, `board_connected_item.rs`, `eda_item.rs`, `item_ref.rs` forwards | `board_item.cpp`, `board_connected_item.cpp`, `common/eda_item.cpp` (flags, custom properties) | 1100 | M0 |
| M3 | Nets and netclasses | `netinfo.rs`, `netclass.rs`, `board/nets.rs` | `netinfo_item.cpp`, `netinfo_list.cpp`, `common/netclass.cpp`, `board.cpp` net functions | 1500 | M0 |
| M4 | Net settings logic | `net_settings.rs`, `eda_pattern_match.rs` | `common/project/net_settings.cpp` (non-JSON), `common/eda_pattern_match.cpp` | 2000 | M3 |
| M5 | Design settings | `board_design_settings.rs`, `zone_settings.rs`, `teardrop_parameters.rs` | `board_design_settings.cpp` (defaults, getters, validation), `zone_settings.cpp`, `teardrop_parameters.cpp` | 2000 | M0 |
| M6 | Stackup, plot params, page | `board_stackup.rs`, `pcb_plot_params.rs`, `title_block.rs`, `page_info.rs` | `board_stackup.cpp`, `stackup_predefined_prms.cpp`, `pcb_plot_params.cpp` (data), `title_block.cpp`, `page_info.cpp` | 2500 | M0 |
| M7 | EdaShape core | `eda_shape/{mod,geometry}.rs`, `line_ending.rs` | `common/eda_shape.cpp` setters, arc/ellipse/rect/poly/bezier maths, move/rotate/flip/mirror/scale, bbox, Compare; `line_ending.cpp` data | 2800 | M0 |
| M8 | EdaShape shapes | `eda_shape/shapes.rs`, `stroke_params.rs` | `eda_shape.cpp` makeEffectiveShapes, TransformShapeToPolygon, hatching, line endings; `stroke_params.cpp` | 2600 | M7 |
| M9 | PcbShape | `pcb_shape/*` | `pcb_shape.cpp` (lib-frame machinery, transforms, shapes, mask) | 2200 | M8 |
| M10 | Text | `eda_text.rs`, `pcb_text.rs`, `pcb_field.rs` | `common/eda_text.cpp`, `pcb_text.cpp`, `pcb_field.cpp` | 2800 | M0, kicad-font |
| M11 | Text boxes, tables | `pcb_textbox.rs`, `pcb_table.rs`, `pcb_tablecell.rs`, `pcb_generated_table.rs` (data, defaults schema) | `pcb_textbox.cpp`, `pcb_table.cpp`, `pcb_tablecell.cpp`, `generated_table_schema.cpp` | 2800 | M9, M10 |
| M12 | Dimensions | `pcb_dimension.rs` | `pcb_dimension.cpp` | 2550 | M10, M8 |
| M13 | Padstack | `padstack.rs`, `pad_utils.rs` | `padstack.cpp`, `pad_utils.cpp` | 2400 | M0 |
| M14 | Pad model | `pad/{mod,derive}.rs` | `pad.cpp`: frames, layers, attributes, FlashLayer, clearance/mask/paste/zone-connection overrides, flip/rotate/mirror, OnFootprint*, Compare, descriptions | 2200 | M13, M2 |
| M15 | Pad shapes | `pad/shapes.rs` | `pad.cpp`: BuildEffectiveShapes/Polygon, TransformShapeToPolygon, TransformHoleToPolygon, mask shapes, MergePrimitivesAsPolygon | 1800 | M14, M9 |
| M16 | Tracks and arcs | `pcb_track/{mod,arc}.rs` | `pcb_track.cpp`: PCB_TRACK, PCB_ARC, cmp_tracks, shapes | 1600 | M2 |
| M17 | Vias | `pcb_track/via.rs`, `pcb_track/shapes.rs` | `pcb_track.cpp`: PCB_VIA layers, tenting/covering/plugging/capping/filling, backdrill, post-machining, FlashLayer, hole shapes, ValidateViaParameters | 2400 | M13, M16 |
| M18 | Zones | `zone/*`, `zone_utils.rs` | `zone.cpp`, `zone_utils.cpp` | 2700 | M2, M5 |
| M19 | Footprint A | `footprint/{mod,transform}.rs` | `footprint.cpp`: fields, variants, attributes, the `Board::footprint_*` cascade family, FootprintTree copy/remap, FixUuids, net-tie / jumper groups | 2500 | M14, M9, M10, M18 |
| M20 | Footprint B | `footprint/{geometry,compare,shapes}.rs`, `footprint_courtyard_index.rs` | `footprint.cpp`: bbox/hull/courtyard caches, net-tie cache, cmp_*, TransformPadsToPolySet, TransformFPShapesToPolySet, text vars, FixUpPadsForBoard | 2500 | M19, M15, M21 |
| M21 | Board outline | `convert_shape_list_to_polygon.rs`, `board/outline.rs` | `convert_shape_list_to_polygon.cpp`, `pcb_board_outline.cpp` | 2100 | M9 |
| M22 | Board queries | `board/{layers,queries,text_vars,markers,compare}.rs` | `board.cpp` rest: layers, properties, variants, text vars, GetPad*, bbox, max clearance, exclusions/markers, groups sanity, comparators; `board_bounding_box.cpp`, `layer_utils.cpp` | 2600 | M1, M3 |
| M23 | Groups, generators, small items | `pcb_group.rs`, `pcb_generator.rs`, `pcb_point.rs`, `pcb_target.rs`, `pcb_marker.rs`, `pcb_constraint.rs`, `pcb_3d_body.rs`, `pcb_grid_item.rs`, `pcb_drill_map.rs` | the matching `.cpp`, `marker_base.cpp`, `rc_item.cpp` (data), tuning-pattern property normalisation | 2800 | M2 |
| M24 | Images, embedded files, component classes | `pcb_reference_image.rs`, `embedded_files.rs`, `component_class.rs` | `pcb_reference_image.cpp`, `reference_image.cpp` (+ PNG/JPEG header probe), `embedded_files.cpp`, `component_classes/*.cpp` | 2200 | M0 |
| M25 | Barcode (tier 2) | `pcb_barcode.rs` | `pcb_barcode.cpp` + encoder decision | 1100 | M10 |
| M26 | Generated tables refresh (tier 2) | `pcb_generated_table.rs` | `pcb_generated_table.cpp`, `pcb_drill_chart.cpp`, `pcb_stackup_table.cpp` | 1900 | M11, drill writer |

Order and parallelism:

```
wave 0:  M0                                                          (1 agent; blocks everyone)
wave 1:  M1 M2 M3 M5 M6 M7 M10 M13 M24                               (9 agents)
wave 2:  M4 M8 M14 M16 M18 M22 M23                                   (7)
wave 3:  M9 M12 M17                                                  (3)
wave 4:  M11 M15 M21 M19 M25                                         (5)
wave 5:  M20 M26
```

Downstream packages compile against M0 immediately. The parser and writer start after M0 + M1
(board-level items) and need M19 for footprints at run time; connectivity after M14/M17/M18;
DRC providers and plotting after their shape packages, with `port_pending!` making missing
behaviour loud instead of wrong.

## 17. Deviations to record and open questions

Deviations (each into `kicad-pcb/PORT_STATUS.md`): pointer-order tie-breaks -> `ItemId` order;
group member order -> insertion order; an item removed outside a commit and re-added gets a new
handle (C++ keeps the address); the change journal replaces listeners; `BOARD` caches used only
by DRC moved to `kicad-drc`; generated tables are not refreshed before save in tier 1 (C++
`RefreshGeneratedTables`); barcode symbols and drill-map symbols are not drawn until their tier-2
packages land; generator geometry is not regenerated.

Open questions are returned to the orchestrator with this document (foundation-type ownership,
JSON crate, regex flavour for netclass patterns, barcode encoder, plot enums' crate).
