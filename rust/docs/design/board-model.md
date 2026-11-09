# Board model of `kicad-pcb` - final design

Status: **binding** for every work package that implements or consumes the board model (model,
`.kicad_pcb` parser and writer, item shapes, connectivity, DRC, zone filler, plotting/fab,
settings, CLI, wasm, external clients). The complete type definitions are in
[`board-model-types.md`](board-model-types.md); this file is the architecture, the shared API and
the rules. C++ reference: this tree, board file format `SEXPR_BOARD_FILE_VERSION 20261031`.
Read `rust/PORTING.md` first; nothing here overrides it.

Revision 2 (senior review, approved with changes). What changed against revision 1, so that
nobody works from memory of the first draft: every item knows its own handle
(`BoardItemBase::id`, section 3); containers are **created, filled, then attached** (the parser
no longer builds detached footprint trees, 8.1); the `EDA_TEXT` / `EDA_SHAPE` mixins have their
own traits (6.2a); pointer-order replacements are spelled out (3.1); `BoardCommit`, parking,
`with_item_taken`, `SideTable`, `OnceCache` and detached footprint trees are **deferred**
(section 17) and the change journal is off by default; the owner of board + connectivity + DRC
engine is the crate `kicad-session`; M0 is split into a reviewed kernel and parallel type
packages (section 16); the orchestrator's foundation decisions are recorded in section 17.

Revision 2.1 (review of the M0a kernel, the gate before the type packages). Corrections to
revision 2, all reflected in the code: `EdaTextItem::shown_text` takes the `RESOLUTION_CONTEXT`
of this tree, not the KiCad 9 flag (6.2a); `BoardItem::parent_group` is a virtual
(`PCB_TABLECELL` overrides `GetParentGroup`, 6.2); every non-constant base-class body is a free
generic function, so that an override can call its base class (6.2); a derived class (arc, via,
field, dimension, textbox, cell, generator) gets a shim for every virtual it or its base
overrides and is its own `EDA_TEXT` / `EDA_SHAPE` host (6.2, 6.2a); the non-virtual mixin
members live in the extension traits `EdaTextExt` / `EdaShapeExt`, not in `traits.rs`, which is
frozen (6.2a); virtuals introduced by a concrete class are inherent methods plus an enum
dispatcher (6.3); `modify_*` also re-stamps `base.group` (8.2); `UnitsProvider` is a struct;
the kernel files are frozen from wave 0b on and the wave-0b ownership rules are in section 16.

Revision 2.2 (integration review of wave 0b, the four type packages). Changes, all reflected
in the code: `EdaShapeItem::width`, `effective_width`, `hatch_line_width` and
`hatch_line_spacing` take `ctx: &BoardCtx<'_>` (all four are C++ virtuals; `PCB_SHAPE::GetWidth`
scales by the parent footprint, 6.2a), and so do `top_left` / `bot_right` (the `PCB_TEXTBOX`
overrides read the draw rotation, which reads the parent footprint); `Padstack::copper_props` iterates in the order of the
reference build's `std::unordered_map`, not in key order (10.1); `EdaCombinedMatcher` is
`Clone + Send + Sync` and `NetSettings` holds compiled matchers, as designed;
`OutlineErrorHandler` has one definition, in `convert_shape_list_to_polygon.rs`;
`ComponentClass::is_constituent` replaces "listing itself" (a root `Netclass` has an empty
constituent list for the same reason); `GeneratedTableData::hidden` is added;
`LayerEnv::default_board()` is gone (a board always derives its snapshot from its settings);
`tools/check_item_shims.py` reads the subclasses folded into one Rust type (generators,
dimensions, generated tables), whose overrides need a shim that dispatches on the subclass.

## 0. The design on one screen

| Topic | Decision |
|---|---|
| Storage | Every board item lives in a **board-level typed arena** `Arena<T>` (generational slot vector), one arena per concrete Rust item type, declared by one table (`board_items!`). Footprint children live in the same arenas as their board-level siblings. |
| Order | Arena slot order is meaningless. The C++ containers (`m_tracks`, `m_drawings`, `FOOTPRINT::m_pads`, ...) are **order lists of handles** (`VecDeque<TrackId>` ...) that reproduce C++ insertion order exactly (`push_back` / `push_front`). |
| Identity | `Kiid` (persistent: file, reports, JS across save/load) and a **`Copy` handle** `Id<T>` / `ItemId` (8 bytes, O(1), stays valid across all edits of other items, R-tree payload, cache key, wasm `number`). Every stored item carries its own handle in `BoardItemBase::id` (the Rust `this`). |
| Parent / child | `ParentRef` value in every item (`None` / `Board` / `Item(ItemId)`); child order lists in `Footprint`, `PcbTable`; group membership by handle on both sides. No `Rc`, no `RefCell`, no lifetimes in item types. |
| Create / attach | Like C++ `new X( parent )` then `parent->Add( x )`: leaf items are local values added with `add_*`; containers (footprint, table) are **created** in the arena (resolvable through `ctx`, not yet listed or UUID-indexed), filled, then **attached**. |
| Polymorphism | Plain structs with inherent methods (one module per C++ file); `trait BoardItem` / `BoardConnectedItem` / `BoardItemMut` carry the C++ virtuals with base-class bodies as defaults; `AnyItem<'a>` enum for `switch( Type() )`; `ItemRef<'a>` = context + handle + `AnyItem`, the Rust spelling of a `BOARD_ITEM*` parameter. |
| Context | Anything that C++ reaches through `m_parent` / `GetBoard()` / `m_DRCEngine` / `GetConnectivity()` comes from an explicit `&BoardCtx` (board + optional rule resolver + optional connectivity). |
| Frames | Footprint children are stored in the **library frame**. Board-frame values exist only behind accessors that take `&BoardCtx` (read) or `&MutEnv` (write); frame-dependent fields are `pub(crate)` and named `lib_*`. |
| Mutation | Only through `Board` methods: `add_*`, `remove`, `modify_*` (leaf mutators get a `MutEnv` snapshot), `footprint_*` cascades, derived-state writers (fills, zone-layer overrides) that do not invalidate. Consumers revalidate against `board.time_stamp()` and rebuild; an optional pull-based change journal (off by default) replaces `BOARD_LISTENER`. Undo / `BoardCommit` is deferred (section 17). |
| Caches | Per-item derived data stays in the item (`KeyedCache` / `CacheCell`, `Clone` to empty). Everything spanning items (connectivity, R-trees, DRC pair caches) lives in the consumer crate, keyed by `ItemId`. |
| Upward calls | `RuleResolver` and `ConnectivityQuery` traits defined in `kicad-pcb`, implemented in `kicad-drc` / `kicad-connectivity`, passed in through `BoardCtx`. |
| Above the model | `kicad_session::Session { board, connectivity, drc }` (new small crate used by `kicad-cli` and `kicad-wasm`, also home of the `BOARD_LOADER` glue) holds the peers side by side; none borrows another. |

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
    Table       TableId        tables       PcbTable;         // PCB_TABLE, PCB_DRILL_CHART, PCB_STACKUP_TABLE
    Dimension   DimensionId    dimensions   PcbDimension;     // the five PCB_DIM_* classes
    // ... 22 rows in all: also Text, Field, Textbox, Tablecell, Barcode, Image, Target, Point,
    // Group, Generator, Marker, Constraint, Body3d, GridItem, DrillMap (the full list is
    // `ItemKind` and the arena fields of `Board` in board-model-types.md sections 0 and 14).
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
* Leaf items outside a board (a value being built by the parser, a dummy copy) are plain
  struct values, polymorphically `OwnedItem`. **A footprint or table never exists outside a
  board**: its children need the arena to resolve their parent, so containers are created in
  the arena first (8.1). `FootprintTree` / `TableTree` (a container with its children by value)
  are data-only carriers for a future deep copy between boards; nothing in tier 1/2 builds one
  (section 17, deferred).
* A footprint-library file (`.kicad_mod`) is parsed into its own `Board` with
  `board_use == BoardUse::FpHolder` holding one attached footprint (what the C++ footprint
  editor and `FootprintLoad` callers end up with). Known difference to record when PCBIO-14
  lands: the C++ library parser runs with a null board, so children take the `GetBoard() ==
  nullptr` defaults while the footprint is parsed; the holder board is a default-constructed
  board, and each such default (copper layer count, max error, flip table) must be shown equal.

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
* **Every stored item knows its own handle**: `BoardItemBase::id: Option<ItemId>`, written only
  by `Board` (on insertion, and re-stamped after every `modify_*`), `None` for a value that is
  not in an arena. This is the Rust `this`: `PAD::FlashLayer` passing `this` to
  `IsConnectedOnLayer`, the net-tie cache keyed by item pointer and `a == b` on pointers all
  need it from inside a `&self` method. Consequences:
  * `#[derive(Clone)]` keeps `id` (so `Board::clone()` and the clone-edit-store idiom of 8.2
    work). The C++ copy-constructor idiom (`PAD dummy( *pad )`: a *different* object with the
    same UUID and parent) is `item.detached_copy()` = clone + `id = None`. Never use a bare
    `clone()` for a dummy: connectivity would treat it as the original, where C++ finds no
    entry for the copy's address.
  * `ItemRef::id()` reads the base; an `ItemRef` over a detached copy has `id() == None`.
  * `check_invariants()` verifies `base.id == Some(handle)` for every live item.
* `ItemId::to_bits()` < 2^53 (exact JS `number`); `from_bits` validates the kind.
* `NetCode(i32)` identifies nets (`NETINFO_ITEM` is not an arena item);
  `ItemId` has an `ItemKind::Net` form (index = code) so `ResolveItem` can return a net.
* Generations are `u16`; a slot whose generation is exhausted is retired, never reused.
* New UUIDs come from `board.kiid_generator` (`kicad_core::KiidGenerator`, deterministic by
  default; the CLI and wasm hosts reseed it from entropy before creating items in a board that
  will be saved). `add_*` never changes a UUID.

### 3.1 Replacing pointer-address order (normative)

C++ orders by heap address in more places than the last line of the save comparators:
`std::set<T*>` / `std::map<T*, ..>` that are iterated, `std::sort` of pointers
(`CONNECTIVITY_DATA::GetNetItems`, `GetConnectedTracks/Pads`), `CN_ITEM::m_connected`. Heap
order is not reproducible; on a freshly loaded board it approximates *allocation order*, which
is file order. The replacements, in order of precedence:

1. Where an analysis document names the replacement (drc.md 4.12, connectivity-zones.md R-1 /
   R-2: `CN_ITEM` list index, cluster creation index, board zone order), use that.
2. Otherwise **`ItemId` order** (`Ord` = kind, generation, index; identical to the numeric
   order of `to_bits()`). M1 guarantees that arena indices are handed out in insertion order
   while no slot has been freed, and the parser adds items in file order, so on a loaded board
   `ItemId` order within one kind *is* file order = C++ allocation order; across kinds it is
   footprints, pads, tracks, zones, then graphics.
3. Maps keyed by item that are only looked up (never iterated) may be `HashMap<ItemId, _>`.

Every use of rule 1 or 2 is listed in the consumer crate's `PORT_STATUS.md` with what the
oracle comparison showed. Sorting `ItemId`s inside a comparator that also has a `Kiid` step
(the save comparators) only matters for duplicate UUIDs.

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
order lists hold live ids and every live item is in exactly one list (no created-but-unattached
container is left over); `parent` agrees with the containing list; `base.id` is the item's
handle; `group` agrees with `members`, the group graph is acyclic; the UUID index points at
live items carrying that UUID; every `net` is known, `UNCONNECTED` or `ORPHANED`.

## 5. Coordinate frames (read this twice)

Board frame = what C++ getters without `Lib` in their name return. Library frame = the
footprint's own frame before `TRANSFORM_TRS` (translate, rotate, non-uniform scale, then flip).
The C++ strategy is kept **per type**, so rounding happens at the same places:

| type | stored | board-frame value |
|---|---|---|
| `Pad` | `lib_pos`, `lib_orientation`, padstack sizes / offsets / drill in lib frame; `padstack.orientation` (`PADSTACK::m_orientation`) is a **baked board-frame copy** kept by `set_orientation`, `set_fp_relative_orientation` and `on_footprint_transformed`, exactly as C++ | derived on read: `pad.position(ctx)`, `pad.size(ctx, layer)`, `pad.offset(ctx, layer)`, `pad.orientation(ctx)`, `pad.drill_size(ctx)` |
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
    /// Connectivity's own `&mut self` methods (build, islands) necessarily run with `None`;
    /// that is equivalent because the C++ algorithm never calls FlashLayer from inside (it uses
    /// ConditionallyFlashed, the zone-layer overrides and ALWAYS_FLASHED shapes).
    pub connectivity: Option<&'a dyn ConnectivityQuery>,
}
impl<'a> BoardCtx<'a> {
    pub fn bare(board: &'a Board) -> Self;                          // parser, writer, simple tools
    pub fn with_rules(self, r: &'a dyn RuleResolver) -> Self;
    pub fn with_connectivity(self, c: &'a dyn ConnectivityQuery) -> Self;
    pub fn item(&'a self, id: ItemId) -> Option<ItemRef<'a>>;      // handle -> BOARD_ITEM*
    pub fn resolve(&'a self, uuid: &Kiid) -> Option<ItemRef<'a>>;  // BOARD::ResolveItem
    /// `&item` -> `BOARD_ITEM*`: wraps any item reference. Used by item code to pass `this`
    /// (`ctx.view(AnyItem::Pad(self))`, identity from `base.id`) and by consumers for values
    /// outside the board (dummy pad of the filler, DRC helper shape: `id() == None`, but the
    /// `parent` still resolves, so a copy of a footprint pad keeps its board-frame geometry).
    pub fn view(&'a self, item: AnyItem<'a>) -> ItemRef<'a>;
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

**How the impl blocks are written (binding, it is what lets packages work in parallel).** An
`impl BoardItem for T` block can only live in one file, so it contains *no behaviour*: for each
virtual the C++ class overrides it has a one-line shim `fn layer_set(&self, ctx: &BoardCtx<'_>)
-> Lset { T::layer_set(self, ctx) }` that forwards to an **inherent method with the same name
and the same signature** (inherent methods win name resolution; if the inherent method is
missing the shim recurses, and `#![deny(unconditional_recursion)]` in `lib.rs` makes that a
build error). The impl blocks are written once by M0 in the type's main module file
(`pad/mod.rs`, `pcb_text.rs`) and are not edited afterwards; the inherent methods live in
whichever module the owning package has (`pad/shapes.rs` for `Pad::effective_shape`), first as
`port_pending!` stubs. Consumers holding a concrete type call the inherent method
(`pad.layer_set(ctx)`), consumers holding `ItemRef` / `dyn BoardItem` get the same code
through the shim. A *forgotten* shim is not detected by the compiler (the trait default, i.e.
the base-class body, is used silently): each type package lists the `override`s of its headers
and the wave-0b review checks the list against the impl blocks. `pcb_track/mod.rs` is the
worked example of everything in this section.

Three consequences of "a Rust struct does not inherit" (binding):

* **Base-class bodies are free functions.** A trait default that is more than a constant
  forwards to a generic function in the module that ports the base class
  (`board_item::fits_enabled_layers(item, ctx, ..)`, `board_connected_item::own_clearance`,
  `eda_text::text_pos(host, ctx)`). An override that calls its base class explicitly (C++
  `BOARD_ITEM::FitsEnabledLayers( .. ) && HasValidLayerPair( .. )`) calls that function; a
  trait default cannot be called from the type that overrides it.
* **Derived classes** (`PCB_ARC`, `PCB_VIA` : `PCB_TRACK`; `PCB_FIELD`,
  `PCB_DIMENSION_BASE` : `PCB_TEXT`; `PCB_TEXTBOX` : `PCB_SHAPE`; `PCB_TABLECELL` :
  `PCB_TEXTBOX`; `PCB_GENERATOR` : `PCB_GROUP`) embed the base struct and get a shim for every
  virtual that the class **or any of its bases** overrides, each forwarding to the derived
  type's **own** inherent method. For an inherited virtual that method is one line calling
  the base (`self.text.x(ctx)`) when the base body calls no virtual the derived class
  overrides; otherwise the owning package writes the base body once as a generic function
  over the host (`fn bounding_box<H: ..>(host: &H, ctx)`) and base and derived methods both
  call it, so that the derived overrides are reached exactly as C++ virtual dispatch reaches
  them. The embedded base struct returned by a downcast (`ItemRef::as_shape()` of a textbox)
  is for data and non-virtual members only.
* **Virtuals introduced by a concrete class** (`PCB_TRACK::GetWidth` / `SetWidth` /
  `GetLength` / `GetWidthConstraint`, overridden by `PCB_VIA` / `PCB_ARC`;
  `PCB_SHAPE::GetCorners`, overridden by `PCB_TEXTBOX`) are not in the traits: each struct has
  the inherent method and the tagged form dispatches (`Track::width()`); a base body that
  calls one follows the generic-function rule above.

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
    /// GetParentGroup() as a handle; PCB_TABLECELL answers with its table's group.
    fn parent_group(&self, ctx: &BoardCtx<'_>) -> Option<ItemId> { self.base().group }
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
    fn item_description(&self, ctx: &BoardCtx<'_>, units: &UnitsProvider<'_>, full: bool) -> String;
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
    // The base bodies of the next five are no-ops (C++: nothing, wxFAIL or a message box).
    fn set_position(&mut self, env: &MutEnv, pos: Vector2I);
    fn move_by(&mut self, env: &MutEnv, delta: Vector2I);                              // Move
    fn rotate(&mut self, env: &MutEnv, centre: Vector2I, angle: EdaAngle);
    fn flip(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);
    fn mirror(&mut self, env: &MutEnv, centre: Vector2I, dir: FlipDirection);
    fn set_layer(&mut self, env: &MutEnv, layer: PcbLayerId);
    fn set_layer_set(&mut self, env: &MutEnv, layers: Lset);   // base: one layer -> set_layer
    fn normalize(&mut self) {}
    fn on_footprint_transformed(&mut self, env: &MutEnv) {}
    fn on_footprint_rescaled(&mut self, env: &MutEnv, ratio_x: f64, ratio_y: f64,
                             linear_factor: f64, anchor: Vector2I, parent_rotate: EdaAngle) {}
    fn style_from_settings(&mut self, settings: &BoardDesignSettings, check_side: bool) {}
}

/// Every item clears its derived caches here; `Board` calls it before handing out `&mut`.
pub trait Invalidate { fn invalidate_caches(&mut self); }
```

### 6.2a The `EDA_TEXT` and `EDA_SHAPE` mixins

`EDA_TEXT` and `EDA_SHAPE` are second base classes whose non-virtual code calls virtuals that
the PCB classes override (`EDA_TEXT::GetTextBox` calls `GetTextPos()`, which `PCB_TEXT`
overrides to apply the footprint transform; `EDA_SHAPE::makeEffectiveShapes` calls `GetWidth()`
and `getMaxError()`). The data lives in the plain structs `EdaText` / `EdaShape`; the virtual
interface is a trait implemented by the **host** item, and the base-class algorithms are
generic functions over the host. Never call an algorithm on the bare data struct.

```rust
/// Virtual interface of EDA_TEXT. Hosts: every type whose C++ class is an EDA_TEXT, each with
/// its own impl so that its overrides apply: PcbText, PcbField, PcbDimension, PcbTextbox,
/// PcbTablecell (the barcode's text is a member object: its inner PcbText is the host).
/// Defaults = the EDA_TEXT bodies (generic functions in eda_text.rs).
pub trait EdaTextItem {
    fn eda_text(&self) -> &EdaText;
    fn eda_text_mut(&mut self) -> &mut EdaText;
    fn text_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I;             // GetTextPos   (stored value)
    fn text_size(&self, ctx: &BoardCtx<'_>) -> Vector2I;            // GetTextSize
    fn text_thickness(&self, ctx: &BoardCtx<'_>) -> i32;            // GetTextThickness
    fn text_angle(&self, ctx: &BoardCtx<'_>) -> EdaAngle;           // GetTextAngle
    fn draw_pos(&self, ctx: &BoardCtx<'_>) -> Vector2I { self.text_pos(ctx) }        // GetDrawPos
    fn draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle { self.text_angle(ctx) } // GetDrawRotation
    /// GetShownText( aContext, aDepth ): this tree passes a RESOLUTION_CONTEXT
    /// (`kicad_core::text_vars::ResolutionContext`), not the `aAllowExtraText` flag of KiCad 9.
    fn shown_text(&self, ctx: &BoardCtx<'_>, context: ResolutionContext, depth: i32) -> String;
    fn font_metrics(&self, ctx: &BoardCtx<'_>) -> &'static kicad_font::Metrics;      // getFontMetrics
    fn set_text_pos(&mut self, env: &MutEnv, pos: Vector2I);        // SetTextPos, Offset, SetTextSize,
    fn set_text_size(&mut self, env: &MutEnv, size: Vector2I, enforce_min: bool);  // SetTextThickness,
    fn set_text_thickness(&mut self, env: &MutEnv, width: i32);     // SetTextAngle: the overrides
    fn set_text_angle(&mut self, env: &MutEnv, angle: EdaAngle);    // convert board -> lib frame
    fn offset(&mut self, env: &MutEnv, delta: Vector2I);
}
/// Non-virtual EDA_TEXT members that call the virtuals: generic functions over the host in
/// eda_text.rs (signatures M0c, bodies M10) with method syntax through this extension trait,
/// which is defined in eda_text.rs and blanket-implemented (`impl<T: EdaTextItem + ?Sized>
/// EdaTextExt for T {}`), so that the methods cannot be overridden and `traits.rs` stays frozen.
pub trait EdaTextExt: EdaTextItem {
    fn text_box(&self, ctx: &BoardCtx<'_>, line: Option<i32>) -> Box2I { eda_text::text_box(self, ctx, line) }
    // likewise: effective_text_pen_width, effective_text_shape, line_positions, interline,
    // compare_text, format-relevant getters. (TextHitTest is virtual and overridden by
    // PCB_TEXT only: an inherent method there.)
}

/// Virtual interface of EDA_SHAPE. Hosts, each with its own impl: PcbShape, PcbTextbox,
/// PcbTablecell. Defaults = the EDA_SHAPE bodies (generic functions in eda_shape/mod.rs).
pub trait EdaShapeItem {
    fn eda_shape(&self) -> &EdaShape;
    fn eda_shape_mut(&mut self) -> &mut EdaShape;
    // The four width virtuals take the context: PCB_SHAPE::GetWidth (inherited by PCB_TEXTBOX
    // and PCB_TABLECELL; the only PCB override of the four) clamps the stored library width
    // at 0 and scales it by fpScaleLinear( transformFp() ), the parent footprint's mean scale
    // (no footprint for a pad primitive). The stored value is eda_shape().stroke.width().
    fn width(&self, ctx: &BoardCtx<'_>) -> i32 { self.eda_shape().stroke.width() }    // GetWidth
    fn effective_width(&self, ctx: &BoardCtx<'_>) -> i32 { self.width(ctx) }          // GetEffectiveWidth
    fn hatch_line_width(&self, ctx: &BoardCtx<'_>) -> i32 { self.effective_width(ctx) }
    fn hatch_line_spacing(&self, ctx: &BoardCtx<'_>) -> i32 { self.hatch_line_width(ctx) * 10 }
    fn is_proxy_item(&self) -> bool { self.eda_shape().proxy_item }
    fn is_filled_for_hit_testing(&self) -> bool;                    // IsSolidFill(); PCB_TABLECELL: true
    // GetTopLeft / GetBotRight: the PCB_TEXTBOX overrides pick the corner by GetDrawRotation()
    // = GetTextAngle(), which adds the parent footprint's orientation: `ctx` as well.
    fn top_left(&self, ctx: &BoardCtx<'_>) -> Vector2I;
    fn bot_right(&self, ctx: &BoardCtx<'_>) -> Vector2I;
    fn corners_in_sequence(&self, angle: EdaAngle) -> Vec<Vector2I>;
    fn shape_draw_rotation(&self, ctx: &BoardCtx<'_>) -> EdaAngle { ANGLE_0 }   // getDrawRotation
    fn shape_max_error(&self, ctx: &BoardCtx<'_>) -> i32 { 100 }    // getMaxError (PCB_SHAPE: board)
    fn hatching_knockouts(&self, ctx: &BoardCtx<'_>) -> ShapePolySet { ShapePolySet::new() } // PCB_SHAPE reads other items
}
/// The non-virtual EDA_SHAPE members that call the virtuals, like `EdaTextExt`: defined and
/// blanket-implemented in eda_shape/mod.rs, forwarding to generic functions in
/// eda_shape/{mod,geometry,shapes}.rs (signatures M0c, bodies M7 / M8):
/// make_effective_shapes(ctx, edge_only), hatching(ctx), shape_bounding_box(ctx),
/// shape_hit_test(..), shape_transform_to_polygon(..), ... Every member whose C++ body reads
/// a width (GetWidth / GetEffectiveWidth / GetHatchLine*) takes `ctx`; the ones without it
/// (shape_position, shape_center, shape_length, rectangle_size) must not call them.
pub trait EdaShapeExt: EdaShapeItem {}
```

`PcbShape::transform_fp(ctx)` is `PCB_SHAPE::transformFp()` for read-only code (mutators read
`MutEnv::parent`): `None` when the direct parent is a pad, else the footprint found by walking
the parent chain (`GetParentFootprint()`, through the table for a cell).

Methods shown without a body have the base-class body as their default in the real trait
(the stored value; for a footprint child that is the library-frame value, which is why every
PCB host overrides them). The host `impl` blocks follow the shim rule of 6.2, including its
derived-class rule: `PcbField`, `PcbDimension` and `PcbTablecell` have a shim for every
`EDA_TEXT` / `EDA_SHAPE` virtual that their class or its base overrides. Mutators:
`EdaShape` has plain inherent setters (the `EDA_SHAPE` bodies); `PcbShape` setters take
`&MutEnv`, call them and then `sync_lib_coords(env)`, as each C++ override does.
`ItemRef::as_eda_text()` / `as_eda_shape()` return `&dyn EdaTextItem` / `&dyn EdaShapeItem`
of the item itself (the real host), never of an embedded base struct.

### 6.3 `ItemRef` and `AnyItem`

```rust
/// Resolved shared reference, for `match` (= `switch( Type() )` + `static_cast`): one variant
/// per `ItemKind` holding `&'a T` (`Pad(&'a Pad)`, `Track(&'a Track)`, ... `Net(&'a NetinfoItem)`),
/// generated by `board_items!`. `AnyItemMut<'a>` is the `&mut` twin without `Net`.
#[derive(Clone, Copy)]
pub enum AnyItem<'a> { /* 23 variants */ }

/// The Rust spelling of a `BOARD_ITEM*` parameter: context + item (+ its handle, read from
/// `base.id`; for `AnyItem::Net` the `ItemKind::Net` form).
#[derive(Clone, Copy)]
pub struct ItemRef<'a> { pub ctx: &'a BoardCtx<'a>, item: AnyItem<'a>, hole_proxy: bool }

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
    pub fn as_eda_text(&self) -> Option<&'a dyn EdaTextItem>;    // EDA_TEXT*  (6.2a)
    pub fn as_eda_shape(&self) -> Option<&'a dyn EdaShapeItem>;  // EDA_SHAPE* (6.2a)
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
    pub fn item_description(&self, units: &UnitsProvider<'_>, full: bool) -> String;
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
impl AnyItemMut<'_> {
    /// The mutating virtuals of a leaf item; None for footprint, table, group, generator
    /// (they cascade through Board methods, 8.3). Used by `Board::move_item` etc.
    pub fn as_leaf_mut(&mut self) -> Option<&mut dyn BoardItemMut>;
}
```

The downcasts that also accept a derived kind (`as_shape()` on a textbox or cell, `as_text()` on
a field or dimension, `as_textbox()` on a cell, `as_group()` on a generator) return the
**embedded base struct**. It is for reading the base's data and calling its non-virtual
members; virtual behaviour (bounding box, shapes, hit tests, shown text, the mixin virtuals)
must go through the `ItemRef`, `as_dyn()`, `as_eda_text()` or `as_eda_shape()`, which dispatch
on the real type. Calling `PcbShape::bounding_box` on the box of a textbox is the C++
`PCB_SHAPE::GetBoundingBox` called non-virtually.

Typed per-class behaviour (`pad.flash_layer(ctx, layer)`, `via.layer_pair()`,
`zone.filled_polys_list(layer)`, `padstack.for_each_unique_layer(..)`) is inherent methods on
the structs, in the module that ports the C++ file. A virtual that a concrete class introduces
is such an inherent method on every struct of the family plus a dispatcher on the tagged form
(`Track::width()`, `Track::length()`, `Track::width_constraint(ctx, want_source)`). Rule C1: an inherent method that backs a
trait method has **exactly the trait's signature** (so it always takes `ctx`, see 6.2); any
other inherent method takes `ctx` iff the C++ body reaches beyond `self`. A method that must
name its own item (`this`) uses `self.base().id` or `ctx.view(AnyItem::Pad(self))`.
Per-layer padstack access mirrors C++: `pad.shape(layer)`,
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
| `this` passed to another subsystem | `self.base().id` / `ctx.view(AnyItem::Pad(self))` |
| `std::map<BOARD_ITEM*, X>` | `BTreeMap<ItemId, X>` (iterated, see 3.1) / `HashMap<ItemId, X>` (lookup only) |
| `std::set<BOARD_ITEM*, cmp>` | collect `Vec<ItemId>`, `sort_by(|a, b| cmp(ctx, *a, *b))` (stable sort is fine: set semantics, comparators total) |
| `std::set<T*>` / `std::sort` of pointers | `BTreeSet<ItemId>` / sort by `ItemId`, or the order an analysis doc names (3.1) |
| `for( PCB_TRACK* t : board->Tracks() )` | `for (id, t) in board.tracks()` |
| `for( PAD* p : fp->Pads() )` | `for (pad_id, pad) in board.footprint_pads(fp_id)` |
| `PAD dummy( *pad ); dummy.SetSize( l, s );` | `let mut d = pad.detached_copy(); d.set_size(&env, l, s); ctx.view(AnyItem::Pad(&d))` (`env = board.mut_env(pad_id)`) |
| a C++ setter that reads other items (`PCB_DIMENSION_BASE::Update`) | clone-edit-store: `let mut d = board[id].clone(); d.update(&ctx); board.modify_dimension(id, \|x, _\| *x = d)` |

## 7. Upward hooks (no crate cycle)

Defined in `kicad-pcb/src/ctx.rs`; `DrcConstraintT` (port of `drc_constraint_types.h`),
`DrcConstraint` (value part of `DRC_CONSTRAINT`: type, `MinOptMax<i32>`, disallow flags,
zone connection, rule name, implicit flag), the `DRCE_*` code table (code <-> settings key, needed
by `BoardDesignSettings::drc_severities` and the `.kicad_pro` reader) and `ErrorLoc` live in
`kicad-pcb/src/drc_api.rs`; `kicad-drc` re-exports them.

```rust
/// Implemented by kicad_drc::DrcEngine. `Sync` so that a `BoardCtx` can cross threads if the
/// optional `parallel` feature is ever added; all methods take `&self`.
pub trait RuleResolver: Sync {
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
pub trait ConnectivityQuery: Sync {
    /// CONNECTIVITY_DATA::IsConnectedOnLayer (PAD::FlashLayer, PCB_VIA::FlashLayer). The item
    /// passes itself (`ctx.view(AnyItem::Pad(self))`); an item without a handle (detached
    /// copy) has no entry in the item map, exactly like a C++ copy at another address: false.
    fn is_connected_on_layer(&self, ctx: &BoardCtx<'_>, item: ItemRef<'_>, layer: PcbLayerId,
                             types: &[KicadT]) -> bool;
    /// CONNECTIVITY_DATA::GetNodeCount / GetPadCount (BOARD::GetNodesCount, net statistics).
    fn node_count(&self, net: Option<NetCode>) -> u32;
    fn pad_count(&self, net: Option<NetCode>) -> u32;
}
```

* Re-entrancy: `eval_rules` calls item methods, which may call `cached_own_clearance`. Engine
  caches use `Mutex` (works on wasm32) and never hold a lock across an item call.
* Because a run holds `ctx.rules = Some(&engine)`, **everything a DRC run mutates must not be
  the engine**: `kicad-drc` splits the C++ `DRC_ENGINE` into the rule set + memo caches (the
  `RuleResolver`, shared, interior mutability only for caches) and a separate run state
  (violation sink, per-code error counts, progress) passed as `&mut` to providers.
* `DRC_ENGINE::InvalidateClearanceCache` (called from C++ setters) has no model-side
  counterpart: the engine stamps its own-clearance cache with `board.time_stamp()` and drops
  it when the stamp moved (every `add` / `remove` / `modify_*` moves it, 8.5). The model never
  calls into the engine from a mutator.
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
    // BOARD::Add for leaf items: list choice per kind, push_front / push_back per mode (fact 3),
    // parent, `base.id`, UUID bind, time stamp. Tracks on a non-copper layer and other C++
    // refusals return Err with the item.
    pub fn add_track(&mut self, track: Track, mode: AddMode) -> Result<TrackId, Track>;
    pub fn add_zone(&mut self, zone: Zone, mode: AddMode) -> ZoneId;
    pub fn add_shape(&mut self, s: PcbShape, mode: AddMode) -> ShapeId;   // ... one per board-level leaf kind
    pub fn add(&mut self, item: OwnedItem, mode: AddMode) -> Result<ItemId, OwnedItem>;   // polymorphic form
    pub fn add_net(&mut self, net: NetinfoItem) -> Option<NetCode>;        // NETINFO_LIST::AppendNet
    pub fn add_marker(&mut self, m: PcbMarker) -> MarkerId;

    // Containers: create (= `new FOOTPRINT( board )`), fill, attach (= `BOARD::Add`).
    /// Arena slot, parent = Board, the four mandatory fields of the FOOTPRINT constructor.
    /// Resolvable through `ctx` and `board[id]`; **not** in `footprints()` and not UUID-indexed.
    pub fn create_footprint(&mut self) -> FootprintId;
    /// FOOTPRINT::Add (front-insert unless mode == Append, fields always append). Children of
    /// an unattached footprint are not UUID-indexed (C++ `IsItemIndexedById( this )` test).
    pub fn footprint_add(&mut self, fp: FootprintId, item: OwnedItem, mode: AddMode) -> Result<ItemId, OwnedItem>;
    /// BOARD::Add( footprint ): order list per mode, UUID bind of the subtree in the C++
    /// `CacheItemById` + `CacheChildrenById` order, time stamp.
    pub fn attach_footprint(&mut self, fp: FootprintId, mode: AddMode);
    /// PCB_TABLE: `parent` is `Board` or `Item(footprint)`; cells get `ParentRef::Item(table)`.
    pub fn create_table(&mut self, table: PcbTable, parent: ParentRef) -> TableId;
    pub fn table_add_cell(&mut self, table: TableId, cell: PcbTablecell) -> TablecellId;   // AddCell
    pub fn attach_table(&mut self, table: TableId, mode: AddMode);    // BOARD::Add / FOOTPRINT::Add
    /// Environment for mutators of a value that is not added yet (the parser calls
    /// `text.rotate( &env, .. )` on a child before `footprint_add`).
    pub fn child_env(&self, parent: ParentRef) -> MutEnv;

    // BOARD::Remove / FOOTPRINT::Remove + delete: cascades to children, leaves its group,
    // unbinds UUIDs, time stamp. `false` for a stale handle.
    pub fn remove(&mut self, id: ItemId) -> bool;
    pub fn remove_many(&mut self, ids: &[ItemId]) -> usize;               // one retain pass per list
    pub fn remove_all(&mut self, types: &[KicadT]);                       // BOARD::RemoveAll
    pub fn add_to_group(&mut self, group: ItemId, member: ItemId);
    pub fn remove_from_group(&mut self, member: ItemId);
    pub fn rebind_item_uuid(&mut self, id: ItemId, new_uuid: Kiid);       // BOARD_ITEM::SetUuid
}
```

A leaf value built for a footprint or table gets `base.parent = ParentRef::Item(parent)`
*before* it is added (C++ passes the parent to the constructor), so its board-frame getters
already work through `ctx` while it is still a local. `remove` also discards an unattached
container (the C++ `unique_ptr` going out of scope); dropping a `Board` that holds one (parse
error) is fine; `check_invariants` reports one that is left over.

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
    /// Invalidate the item's caches, run `f`, invalidate them again, re-stamp `base.id`,
    /// `base.parent` and `base.group` (so `f` may assign a whole new value: the three are
    /// relations only `Board` writes; a changed UUID is re-bound in the index), bump the time
    /// stamp, invalidate the parent footprint's geometry caches, journal `Modified` if the
    /// journal is on. Child and member lists of containers are not protected: never replace
    /// a footprint, table or group wholesale.
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
}
```

A mutator whose C++ body reads other items or the board beyond what `MutEnv` carries
(`PCB_DIMENSION_BASE::Update` with text variables, generator updates) is ported as a `&mut self`
method taking `&BoardCtx` and applied with **clone-edit-store** (6.4): clone the item, edit the
clone against the unmodified board, store it with `modify_*`. No slot is ever "taken out".

Scope note: the tier 1/2 consumers (parser incl. legacy conversions, plot dummies, zone filler,
net propagation, autorouter clients) need the setters, `Move` / `Rotate` of texts and shapes
used by the parser, layer / net changes and the footprint cascades. `Flip` / `Mirror` of every
item type are ported with their class (placement clients and `FOOTPRINT::Flip` need them), but
they are the last thing a package does; a `port_pending!` there does not block tier 1.

### 8.3 Footprint cascades

`FOOTPRINT::SetPosition/SetOrientation/Flip/SetTransformScale/RescaleAroundPoint/
MoveAnchorPosition/SetLayerAndFlip/Move/Rotate` become `Board::footprint_set_position(id, pos)`
etc. in `footprint/transform.rs`; they work the same on a created-but-unattached footprint (the
parser applies `(at ..)` / `(transform ..)` while only the mandatory fields exist, like C++).
The body destructures `Board` into disjoint arena borrows,
updates `transform`, builds one `MutEnv` with the new `ParentFrame` and runs the C++ loop body on
each child list in C++ order (fields, pads, zones, points, drawings by kind, table cells), then
patches the footprint's geometry / courtyard caches exactly where C++ patches them instead of
dropping them. Groups and generators cascade the same way (`move_item` on a group moves members).

### 8.4 Derived-state writers (do not invalidate, do not bump the time stamp)

Results that another subsystem computes and the file stores: zone fills
(`set_filled_polys_list`, `set_fill_flag`, `set_is_filled`, `set_need_refill`,
`set_is_island` / `insulated_islands`, `build_hash_value`, `cache_triangulation`),
`zone_layer_overrides` on pads and vias, the footprint net-tie cache (`build_net_tie_cache`).
They are reached through `board.zone_fill_mut(id) -> &mut ZoneFill`,
`board.set_zone_layer_override(id, layer, value)`, `board.clear_zone_layer_overrides(id)`,
`board.build_net_tie_cache(fp)`. C++ does not bump `m_timeStamp` for these either; the zone
filler calls `board.increment_time_stamp()` where the C++ tool does. The footprint courtyard
cache is *not* in this class: C++ builds it lazily from the `const` `GetCourtyard()`, so it is
a class-1 `KeyedCache` (section 9) and `fp.build_courtyard_caches(ctx, handler)` takes `&self`.

### 8.5 Time stamp, change journal

```rust
pub enum ChangeKind { Added, Removed, Modified }
pub struct Change { pub kind: ChangeKind, pub id: ItemId, pub uuid: Kiid, pub kicad_t: KicadT,
                    pub net_before: Option<NetCode>, pub layers_before: Lset }
impl Board {
    pub fn time_stamp(&self) -> i32;                     // BOARD::GetTimeStamp
    pub fn increment_time_stamp(&mut self);              // also clears BoardCaches
    pub fn set_journal_enabled(&mut self, on: bool);     // default: off
    pub fn drain_changes(&mut self) -> Vec<Change>;
}
```

**The time stamp is the one invalidation signal every tier 1/2 consumer uses.** `add_*`,
`attach_*`, `remove*`, `modify*`, the `footprint_*` cascades and `design_settings_mut()` bump it;
derived-state writers (8.4) do not. C++ bumps it in fewer places (commit push, remove, DRC
init and run); bumping more often only empties caches earlier and cannot change a result.
Consumers that keep state next to the board (connectivity, DRC caches, the own-clearance
cache) record the stamp they were built at and **rebuild from scratch** when it moved; for
connectivity that is the C++ full `Build()`, which connectivity-zones.md R-6 shows equivalent
to the incremental path for every CLI output. `kicad-session` decides when to rebuild.

The journal is implemented by M1 (a `Vec<Change>` push in the same places) but **off by
default and no tier 1/2 package may depend on it**; it exists so that incremental connectivity
updates and an interactive router can be added later without touching the mutators. Undo
(`BoardCommit`, parking removed items so that a revert restores the same handles) is deferred
with it (section 17). Until then: a client that wants to try something speculatively works on
`board.clone()` (all handles stay valid in the clone) or removes what it added.

Scratch flags: `BoardItemBase::flags` exists for non-algorithmic code (`STRUCT_DELETED`,
`IS_NEW`, `MALFORMED_*`); algorithms marking items while reading (`SKIP_STRUCT`, `CANDIDATE`,
`m_arflag`, `PAD::m_subRatsnest`) use a `HashMap<ItemId, _>` / `BTreeSet<ItemId>` of their own.

## 9. Derived caches and invalidation

Cache cells (in `cache.rs`), both `Sync`, both `Clone` to **empty** (which is what lets item
structs derive `Clone` although they hold a `Mutex`), both `Debug`, both ignored by equality:

```rust
/// Re-validated from `&self` (C++ `mutable` + mutex + time stamp / dirty check). `T` stores the
/// inputs it was built from (time stamp, parent transform, text ...); `valid` compares them, so
/// a forgotten invalidation cannot produce a stale read. A cache whose only input is the item
/// itself uses `|_| true` and relies on `Invalidate`.
pub struct KeyedCache<T>(std::sync::Mutex<Option<Arc<T>>>);
impl<T> KeyedCache<T> {
    pub fn get_or_build(&self, valid: impl Fn(&T) -> bool, build: impl FnOnce() -> T) -> Arc<T>; // build runs unlocked
    pub fn peek(&self) -> Option<Arc<T>>;      pub fn store(&self, value: T);
    pub fn invalidate(&self);                  pub fn clear(&mut self);
    pub fn update(&mut self, f: impl FnOnce(&mut T)) where T: Clone;  // patch in place (FOOTPRINT::Flip on caches)
}
/// Plain interior-mutable scratch for C++ `mutable` members that are not "one value built
/// from inputs" (per-line text bbox map, the three time-stamped footprint boxes).
pub struct CacheCell<T: Default>(std::sync::Mutex<T>);
impl<T: Default> CacheCell<T> { pub fn with<R>(&self, f: impl FnOnce(&mut T) -> R) -> R;  pub fn clear(&mut self); }
```

Three classes, three rules:

1. **Derived from the item and its parent** (C++ `mutable` members): stored in the item.
   Pad shapes / polygons / mask shapes (`KeyedCache` validated against `PadCacheKey` = parent
   transform + flip + `max_error`, also cleared by `set_dirty()` where C++ calls `SetDirty()`), footprint bbox /
   text-excluded bbox / hull (keyed by board time stamp, as C++), the footprint courtyard cache
   (valid while the stored hashes match, as `GetCourtyard()`; the malformed flags live in the
   cache value), zone bbox (time stamp), text render / bbox / knockout caches (key = the inputs
   C++ compares), shape hatching, barcode cache. `EDA_TEXT::m_shown_text` stays an eagerly
   maintained plain field (as `cacheShownText`). Dimension `shapes` are a plain field rebuilt
   by `update()`.
2. **Results written by another subsystem and saved in the file**: section 8.4.
3. **Spanning items**: connectivity graph, ratsnest, DRC R-trees, `m_DRCZones`,
   `m_Intersects*Cache`, island maps, own-clearance cache, drill symbol cache. **Not in
   `Board`.** They live in the consumer (`kicad_connectivity::ConnectivityData`,
   `kicad_drc::DrcBoardCaches`), keyed by `ItemId` (iterated maps: `BTreeMap`), stamped with
   `board.time_stamp()` and rebuilt when it moved (8.5). Board-level caches used by
   model code itself (`m_maxClearanceValue`, `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`,
   `m_StackedMicroviaCache`, `m_footprintCourtyardIndex`, `m_LayerExpressionCache`, board
   outline) are `Board::caches: BoardCaches`, keyed by time stamp.

## 10. Board-level services

The complete list of board-level signatures (iteration, lookup, nets and netclasses, design
settings, stackup and layers, title block / variants / text variables, geometry, save-order
comparators; all `impl Board` unless stated, C++ origin `pcbnew/board.cpp`) is the **API index
in `board-model-types.md` section 18**; M0 stubs every entry. The rules that go with it:

* Iterators yield `(handle, &item)` in C++ container order (10.1); `*_ids()` snapshots exist
  for loops that mutate.
* `nets()` iterates by name (`for( net : GetNetInfo() )`), `nets_by_netcode()` by code;
  `find_or_add_net` assigns codes in first-use order like the parser.
* `design_settings()` / `design_settings_mut()` are the only access to the settings (the
  `_mut` form bumps the time stamp and marks the `LayerEnv` snapshot stale).
* Comparators are free functions `(ctx, a: ItemId, b: ItemId) -> Ordering` next to the type
  the C++ comparator belongs to.


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
| `Padstack::copper_props` (C++ `std::unordered_map<PCB_LAYER_ID, ..>`): CUSTOM-mode `for_each_unique_layer` / `unique_layers`, and the `begin()` fallbacks of `copper_props(layer)` / `effective_layer_for` | the node order of libstdc++ (reference platform, `PORTING.md` 9), reproduced by `padstack::CopperPropsMap`; rule below |

**The `m_copperProps` order rule.** The map is observable (DRC reports per unique layer, pad
compare, the first-entry fallbacks), so it is neither sorted nor "insertion order". libstdc++
keeps all nodes in one list; `std::hash<PCB_LAYER_ID>` is the layer id, the bucket is
`id % bucket_count`. (1) Insert: the new node goes in front of the first node of its bucket,
or to the **front of the whole list** when its bucket is empty; so with distinct buckets the
map iterates in reverse insertion order and `begin()` is the entry inserted last (after the
constructor and `(layer B.Cu)` / `(layer In1.Cu)` of a custom padstack: `In1.Cu`, `B.Cu`,
`F.Cu`). (2) Bucket count: 1 when empty, 13 from the first insertion, then 29 at the 14th
element and 59 at the 30th (`_Prime_rehash_policy`, load factor 1); a rehash re-inserts every
node in list order with rule (1), which reverses the order of the distinct buckets. (3) Copy
(`operator=`, copy construction, also the temporaries of `FlipLayers`) keeps the node order
and the bucket count; `clear()` keeps the bucket count; erase keeps the order of the rest.
(4) `FlipLayers` therefore re-inserts in the old iteration order, each through rule (1).
Nothing outside `padstack.rs` may depend on the order except through `Padstack` methods.
| anything C++ orders by pointer address | section 3.1 (analysis-doc order, else `ItemId` order); record it |

## 11. Module layout (`rust/crates/kicad-pcb/src/`)

One module per C++ file; where one C++ file is split between work packages the Rust side is a
directory whose files name the C++ function ranges they port (`pad/{mod,derive,shapes}.rs`,
`board/{mod,containers,index,journal,nets,layers,queries,outline,text_vars,markers,compare}.rs`).
The complete tree is `board-model-types.md` section 19; `lib.rs` and the `mod.rs` files are
written by M0 and then frozen (section 16).

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

**Parser** (`pcb_io_kicad_sexpr_parser.cpp`, spec `analysis/pcb-io.md`). Holds `&mut Board`.
Board-level leaf items are built as local values and added with `AddMode::BulkAppend`. A
footprint follows the C++ statement order literally: `create_footprint()` where C++ does
`make_unique<FOOTPRINT>( m_board )`; `(at)` / `(transform)` through `Board::footprint_*`; each
child is a local value with `parent = Item(fp)`, read and converted with a short-lived
`BoardCtx::bare(&board)` for getters and `board.child_env(..)` for the legacy
`Rotate` / `Move` / `OverrideLibCoords` calls, then `footprint_add(fp, child, Append)`; finally
`attach_footprint(fp, BulkAppend)` where C++ calls `m_board->Add`. Tables use
`create_table` / `table_add_cell` / `attach_table` the same way. Arena indices therefore follow
file order (3.1). Nets are created by name on first use (`find_or_add_net`) so codes match C++.
Group members, generator members and net-chain terminal pads are read as `Kiid`s and resolved
after the loop (`resolve_groups`, `resolve_net_chain_terminal_pads`) like C++. Post-load
fix-ups use `modify_*`.

**Writer** (`pcb_io_kicad_sexpr.cpp`). Takes `&BoardCtx` (bare). Builds `Vec<ItemId>` from the
order lists, sorts with the ported comparators, formats; footprints from their child lists.
Lib-frame values via `lib_*()` accessors, verbatim payloads (embedded files, image data, drill
symbol profile) re-emitted as stored.

**Item shapes** (`GetEffectiveShape`, `TransformShapeToPolygon`). Inherent methods in the
`*/shapes.rs` modules, exposed through `BoardItem`. Results `SharedShape` (`Arc<dyn Shape>`,
`Send + Sync`), pad results from `PadCaches`. Flashing asks `ctx.connectivity` and
`zone_layer_overrides`, mask expansion asks `ctx.rules`, exactly where C++ asks the board.

**Connectivity + ratsnest** (`pcbnew/connectivity/`). `CN_ITEM::m_parent` -> `ItemId` (zone
layers: `ZoneId` + layer + outline index); `CN_ITEM`s live in the algorithm's own vector and
are named by their index (`CnItemId`), which is also the sort key of `m_connected` (R-1);
clusters, per-net lists and RN graphs hold handles. `ConnectivityData::build(&mut self, ctx)`
walks `all_connected_items()` (ctx without connectivity, see 6.1); it records
`board.time_stamp()`. Tier 1/2 always rebuild in full after board edits (8.5). Net propagation
(`PropagateNets` -> `SetNetCode`) collects `(ItemId, NetCode)` pairs and the caller applies
them with `board.set_item_net` afterwards, then re-records the stamp (propagation does not
change topology). Implements `ConnectivityQuery`.

**DRC engine + providers** (`pcbnew/drc/`, spec `analysis/drc.md`). A run holds `&BoardCtx`
(with connectivity and `rules = Some(&engine)`) for its whole duration, after a `&mut Board`
preparation phase (`increment_time_stamp` where C++ does, `build_net_tie_cache`,
triangulation, `synchronize_nets_and_netclasses`); run state is separate from the engine
(section 7). `DRC_RTREE::ITEM_WITH_SHAPE` -> `{ parent: ItemId, parent_shape: SharedShape,
sub: Option<u32> }`: C++ keeps a raw pointer into the parent shape's subshapes, Rust keeps the
owner and the subshape index (needs the accessor `Shape::indexable_subshape(i) -> Option<&dyn
Shape>` in `kicad-geom`; `indexable_subshapes()` allocates and is not for hot loops). Visitors
resolve with `ctx.item(id)`. Pair caches key on `(ItemId, ItemId, PcbLayerId)`. Violations
carry `Kiid`s (`RcItem::ids`) and a `DrcRuleInfo` copy of the violating rule (marker severity
depends on it); the run collects them and the caller adds markers with `board.add_marker`
afterwards (the C++ handler does it through a commit). Dummy shapes are locals viewed with
`ctx.view`. The engine implements `RuleResolver`.

**Zone filler** (`zone_filler.cpp`). Port pass by pass: each read pass is
`fn pass_n(ctx: &BoardCtx, ..) -> PassNResult`, each write pass `apply_pass_n(&mut Board, ..)`
through the section 8.4 writers (zone-layer overrides, fills, flags, islands, hashes). Earlier
fills are visible to later passes because they are stored before the next read pass, exactly as
the C++ ordering requires; never reorder a write relative to the reads that follow it. The
C++ loops that write an override on the item they are reading (parallel, one item each) become
"collect `(id, layer, value)`, then apply". Dummy pads and vias for thermal spokes are
`detached_copy()` values viewed with `ctx.view`. Teardrop zones are added with `add_zone` and
removed with `remove_many`. The filler takes `&mut Board`, `&mut ConnectivityData` and the
`&dyn RuleResolver` as three separate arguments and builds a fresh `BoardCtx` per read pass.

**Gerber / drill plotting** (`plot_board_layers.cpp`, `plot_brditems_plotter.cpp`, `gendrill_*`,
spec `analysis/plot-fab.md`). Pure readers over a `BoardCtx` *with connectivity and rules* (the
CLI always has an engine: flashing of unconnected pad layers and rule-driven mask expansion
affect plots). Footprint graphics via `footprint_drawings` + `match AnyItem`; the "plot a
modified copy of the pad" idiom is `detached_copy()` + `ctx.view`. Gerber X2 attributes from
`item.netname()`, `pad.number`, the parent footprint's reference field. Drill files iterate
vias in `tracks()` and `get_pads()`, read padstack drills, `via_type`, `PadAttrib`.

**Settings** (`analysis/settings.md`). The project-settings packages own JSON parsing and write
into the plain-data structs here (`BoardDesignSettings`, `NetSettings`, severities, exclusions,
`BoardProject::text_vars`); the model defines data, defaults (C++ constructors) and logic.

**Session, CLI, wasm** (`kicad-session`, used by `kicad-cli` and `kicad-wasm`).
`Session { board, connectivity, drc }` owns the peers and ports the `BOARD_LOADER` glue
(`initializeLoadedBoard`: project settings, `board.project`, `synchronize_nets_and_netclasses`,
connectivity build, engine init). Every operation is a sequence of borrow phases: mutate the
board, rebuild what the time stamp says is stale, run with a `BoardCtx` that always carries
both hooks. `kicad-fab` / `kicad-plot` receive the context and depend on neither peer crate.

**External autorouter client.** Load -> enumerate (`tracks()`, `get_pads()`, `nets()`, zones,
`board_outline`) -> `add_track` / `remove` / `modify_track` (handles of untouched items stay
valid) -> `session.rebuild_connectivity()` -> refill zones -> DRC -> export. Speculative passes
work on `board.clone()` or remove what they added.

**Push-and-shove router (tier 3).** `PNS::ITEM::m_parent` -> `Option<ItemId>`;
`PNS_KICAD_IFACE::SyncWorld` walks the board; the rule resolver adapter calls the same
`RuleResolver`; `Commit()` needs the deferred `BoardCommit` (section 17).

## 14. wasm / JS

wasm-bindgen cannot export references into Rust collections, so every design needs an id at the
boundary; here the model's native id *is* that id. `kicad-wasm` wraps `kicad_session::Session`:

```ts
class Session {                                  // Id = number (ItemId::to_bits); stale ids throw
  static load(pcb: string, project?: string, rules?: string): Session;  save(): string;
  resolve(uuid: string): Id | undefined;  uuidOf(id: Id): string;  isAlive(id: Id): boolean;
  trackIds(): Float64Array;   // also padIds, zoneIds, footprintIds; kindOf(id), item(id): object (board frame)
  tracksPacked(): Int32Array; // [kind, x1, y1, x2, y2, width, layer, net] * n, C++ order; also viasPacked, padsPacked
  nets(): { code: number; name: string; netclass: string }[];
  addTrack(t: object): Id;    // also addArc, addVia; update(id, patch); remove(id): boolean
  snapshot(): Session;        // board.clone(): ids stay valid in the copy
  rebuildConnectivity(): void;  ratsnest(): Int32Array;  refillZones(ids?: Id[]): void;
  runDrc(options?: object): object[];   // violations carry UUIDs, positions, severities, texts
  plotGerber(layer: string, options?: object): string;  drillFiles(options?: object): object[];
}
```

Ids are valid for one `Session` (and its snapshots); UUID strings are what JS keeps across
`save()` / `load()`.

## 15. Rules for implementers (checklist)

1. Store `ItemId`s, pass `ItemRef` / `&BoardCtx`, persist `Kiid`. Never store `&Board`, `&T`
   or `ItemRef` in a struct that outlives one read pass.
2. Never iterate an arena for semantics; use the order lists / `Board` iterators.
3. Board-frame getters of footprint children take `ctx`; lib-frame values are `lib_*`. Never
   apply a footprint transform by hand outside `footprint/transform.rs` and the per-type
   derive functions.
4. Rule C1 (6.3): an inherent method backing a trait method has the trait's signature; other
   inherent methods take `ctx` iff the C++ body reaches the parent, the board, another item, the
   net object or a hook. Never put behaviour into an `impl Trait for T` block (6.2). An
   override that calls its base class calls the base's free function (`board_item::x(self, ..)`);
   virtual behaviour of a derived kind is never reached through its embedded base struct.
5. Mutate only through `Board` methods; derived results through the section 8.4 writers. Do not
   add `pub` to arena fields or order lists.
6. `rules == None` / `connectivity == None` must behave exactly like the C++ null branches; test
   each.
7. Keep C++ container order: `push_front` where C++ does, `Append`-only exceptions of
   `FOOTPRINT::Add` included.
8. `BTreeMap`/`BTreeSet` for anything iterated; pointer-address order follows section 3.1 and
   is recorded in `PORT_STATUS.md`.
9. Default = C++ constructor with nil UUID; new UUIDs only from `board.kiid_generator`.
10. Caches: per-item derived data in `KeyedCache` / `CacheCell` with every external input in
    the key; nothing spanning items inside `Board`; consumers revalidate by time stamp.
11. Algorithm scratch flags go to the algorithm's own maps, not `base.flags`.
12. `Board::check_invariants()` after every mutating test step.
13. Test frame-dependent behaviour with rotated, flipped and non-uniformly scaled footprints.
14. Replace `port_pending!` markers of your package; leave none on reachable paths.
15. Do not change a signature in this document or in `board-model-types.md` without the
    orchestrator; add new methods freely in your own modules.
16. A C++ copy constructor is `detached_copy()`, never a bare `clone()`. Containers are created,
    filled, attached; nothing in tier 1/2 uses the deferred items of section 17.

## 16. Work packages for the model

Line counts are C++ lines actually ported (UI, property registration, protobuf, `View*`,
`GetMsgPanelInfo`, `Serialize` excluded). Every package ports the matching `qa/tests/pcbnew`
tests, keeps `check_invariants` green and replaces its `port_pending!` markers. File ownership is
disjoint: inherent `impl` blocks for one type may sit in several modules, trait `impl` blocks
are delegation-only and belong to M0 (6.2). "M0" means M0a + M0b-e. "needs" names foundation
work that must have landed (`G*` = kicad-geom packages of PLAN.md, `TF*` = text-font.md).

| WP | Title | Rust modules | C++ sources | ~lines | depends on |
|---|---|---|---|---|---|
| **M0a** | Kernel and pattern (1 agent; reviewed before anything else starts) | `arena`, `ids`, `items_macro`, `cache`, `pending`, `item`, `traits` (all traits of 6.2 / 6.2a with full signatures), `ctx`, `item_ref`, `drc_api`, `board/mod.rs` (struct, `Index`, typed getters, order-list iterators, `check_invariants`); a **placeholder struct for every item and settings type** (base field + derives only) so that the item table compiles; and one family end to end as the pattern the others copy: `PcbTrack` / `PcbArc` / `PcbVia` / `Track` with all fields, constructor defaults, delegation shims, inherent stubs, `add_track`, `modify_track`, `tracks()`, a `BoardCtx` / `ItemRef` round-trip test, wasm32 build | `eda_item.h`, `board_item.h`, `board_connected_item.h`, `pcb_track.h`, `drc_constraint_types.h`, `drc_rule.h`, `drc_item.h` code list | 2500 (Rust) | kicad-core, kicad-geom type names (stubs per section 17) |
| **M0b-e** | Type definitions (4 agents in parallel, disjoint files) | every struct / enum of `board-model-types.md` with `Default` = C++ constructor values, the delegation shims for exactly the virtuals the C++ class overrides, an inherent `port_pending!` stub for every signature of this document. **b**: padstack, pad, footprint, zone, zone settings, teardrop parameters. **c**: eda_shape, pcb_shape, line ending, eda_text, pcb_text, field, textbox, table, tablecell, generated-table data (`pcb_generated_table.rs`, held by `PcbTable`), dimension, barcode. **d**: `Board::default`, design settings, stackup, plot params, netinfo, netclass, net settings, embedded files, component classes, project data, and the `impl Board` stubs of sections 8 and 18. **e**: group, generator, target, point, marker / `RcItem`, constraint, 3D body, grid item, drill map, reference image | the matching headers and constructors | 3000 (headers) | M0a |
| M1 | Containers, index, journal | `board/{containers,index,journal}.rs` | `board.cpp` Add/Remove/RemoveAll/FinalizeBulk*/ResolveItem/RepairDuplicateItemUuids/GetItemSet/collectOwnedItems/RunOnChildren/AllConnectedItems, `footprint.cpp` constructor (mandatory fields) / Add / Remove / RunOnChildren as `create_footprint` / `footprint_add` / `attach_footprint`, table create / add cell / attach, `child_env`, `board_item_index.cpp`, `common/eda_group.cpp`; insertion-order arena indices (3.1); time stamp and journal | 1700 | M0 |
| M2 | Item base | `board_item.rs`, `board_connected_item.rs`, `eda_item.rs`, `item_ref.rs` forwards | `board_item.cpp`, `board_connected_item.cpp`, `common/eda_item.cpp` (flags, custom properties), `BOARD_ITEM::ptr_cmp` | 1100 | M0 |
| M3 | Nets and netclasses | `netinfo.rs`, `netclass.rs`, `board/nets.rs` | `netinfo_item.cpp`, `netinfo_list.cpp`, `common/netclass.cpp`, `board.cpp` net functions | 1500 | M0 |
| M4 | Net settings logic | `net_settings.rs` | `common/project/net_settings.cpp` (non-JSON); the pattern matcher is `kicad_core::eda_pattern_match` (already ported there, do not port it again) | 1400 | M3; needs kicad-core `eda_pattern_match` |
| M5 | Design settings | `board_design_settings.rs`, `zone_settings.rs`, `teardrop_parameters.rs` | `board_design_settings.cpp` (defaults, getters, validation), `zone_settings.cpp`, `teardrop_parameters.cpp` | 2000 | M0; needs kicad-core JSON value type |
| M6 | Stackup, plot params | `board_stackup.rs`, `pcb_plot_params.rs` | `board_stackup.cpp`, `stackup_predefined_prms.cpp`, `pcb_plot_params.cpp` (data); title block and page info are already in kicad-core | 2100 | M0; needs kicad-core plot enums |
| M7 | EdaShape core | `eda_shape/{mod,geometry}.rs`, `line_ending.rs` | `common/eda_shape.cpp` setters, arc/ellipse/rect/poly/bezier maths, move/rotate/flip/mirror/scale, bbox, Compare; `line_ending.cpp` data | 2800 | M0; needs G2, G3, kicad-geom `Ellipse`, `TransformTrs` |
| M8 | EdaShape shapes | `eda_shape/shapes.rs`, `stroke_params.rs` | `eda_shape.cpp` makeEffectiveShapes, TransformShapeToPolygon, hatching, line endings; `stroke_params.cpp` | 2600 | M7; needs G3-G6 |
| M9 | PcbShape | `pcb_shape/*` | `pcb_shape.cpp` (lib-frame machinery, transforms, shapes, mask) | 2200 | M8 |
| M10 | Text | `eda_text.rs`, `pcb_text.rs`, `pcb_field.rs` | `common/eda_text.cpp`, `pcb_text.cpp`, `pcb_field.cpp` | 2800 | M0 (M2 at run time); needs TF1-TF3, G4 |
| M11 | Text boxes, tables | `pcb_textbox.rs`, `pcb_table.rs`, `pcb_tablecell.rs`, `pcb_generated_table.rs` (data, defaults schema) | `pcb_textbox.cpp`, `pcb_table.cpp`, `pcb_tablecell.cpp`, `generated_table_schema.cpp` | 2800 | M9, M10 |
| M12 | Dimensions | `pcb_dimension.rs` | `pcb_dimension.cpp` | 2550 | M10, M8 |
| M13 | Padstack | `padstack.rs`, `pad_utils.rs` | `padstack.cpp`, `pad_utils.cpp` | 2400 | M0 |
| M14 | Pad model | `pad/{mod,derive}.rs` | `pad.cpp`: frames, layers, attributes, FlashLayer, clearance/mask/paste/zone-connection overrides, flip/rotate/mirror, OnFootprint*, Compare, descriptions | 2200 | M13, M2 |
| M15 | Pad shapes | `pad/shapes.rs` | `pad.cpp`: BuildEffectiveShapes/Polygon, TransformShapeToPolygon, TransformHoleToPolygon, mask shapes, MergePrimitivesAsPolygon | 1800 | M14, M9; needs G4-G6 |
| M16 | Tracks and arcs | `pcb_track/{mod,arc}.rs` (track and arc shapes included) | `pcb_track.cpp`: PCB_TRACK, PCB_ARC, cmp_tracks, shapes | 1600 | M2 |
| M17 | Vias | `pcb_track/via.rs`, `pcb_track/shapes.rs` (via and hole shapes) | `pcb_track.cpp`: PCB_VIA layers, tenting/covering/plugging/capping/filling, backdrill, post-machining, FlashLayer, hole shapes, ValidateViaParameters | 2400 | M13, M16 |
| M18 | Zones | `zone/*`, `zone_utils.rs` | `zone.cpp`, `zone_utils.cpp` | 2700 | M2, M5; needs G4 (clipper), G6 |
| M19 | Footprint A | `footprint/{mod,transform}.rs` | `footprint.cpp`: fields, variants, attributes, the `Board::footprint_*` cascade family (working on unattached footprints too), FixUuids, net-tie / jumper groups, `build_net_tie_cache` | 2500 | M1, M14, M9, M10, M18 |
| M20 | Footprint B | `footprint/{geometry,compare,shapes}.rs`, `footprint_courtyard_index.rs` | `footprint.cpp`: bbox/hull/courtyard caches, net-tie cache, cmp_*, TransformPadsToPolySet, TransformFPShapesToPolySet, text vars, FixUpPadsForBoard | 2500 | M19, M15, M21; needs G6 |
| M21 | Board outline | `convert_shape_list_to_polygon.rs`, `board/outline.rs` | `convert_shape_list_to_polygon.cpp`, `pcb_board_outline.cpp` | 2100 | M9; needs G4, G7 (nanoflann) |
| M22 | Board queries | `board/{layers,queries,text_vars,markers,compare}.rs` | `board.cpp` rest: layers, properties, variants, text vars, GetPad*, bbox, max clearance, exclusions/markers, groups sanity, comparators; `board_bounding_box.cpp`, `layer_utils.cpp` | 2600 | M1, M3 (item `compare`s of M9-M12 at run time) |
| M23 | Groups, generators, small items | `pcb_group.rs`, `pcb_generator.rs`, `pcb_point.rs`, `pcb_target.rs`, `pcb_marker.rs`, `pcb_constraint.rs`, `pcb_3d_body.rs`, `pcb_grid_item.rs`, `pcb_drill_map.rs` | the matching `.cpp`, `marker_base.cpp`, `rc_item.cpp` (data), tuning-pattern property normalisation | 2800 | M2 |
| M24 | Images, embedded files, component classes | `pcb_reference_image.rs`, `embedded_files.rs`, `component_class.rs` | `pcb_reference_image.cpp`, `reference_image.cpp` (+ PNG/JPEG header probe), `embedded_files.cpp`, `component_classes/*.cpp` | 2200 | M0; needs kicad-core MMH3 / `Hash128`, `base64`, `ruzstd` |
| M25 | Barcode (tier 2) | `pcb_barcode.rs` | `pcb_barcode.cpp` + encoder decision | 1100 | M10; tier 2, data-only until then (section 17) |
| M26 | Generated tables refresh (tier 2) | `pcb_generated_table.rs` | `pcb_generated_table.cpp`, `pcb_drill_chart.cpp`, `pcb_stackup_table.cpp` | 1900 | M11, drill writer |

Order and parallelism:

```
wave 0a: M0a                                                         (1 agent, then review)
wave 0b: M0b M0c M0d M0e                                             (4 agents)
wave 1:  M1 M2 M3 M5 M6 M7 M10 M13 M24                               (9 agents)
wave 2:  M4 M8 M14 M16 M18 M22 M23                                   (7)
wave 3:  M9 M12 M17                                                  (3)
wave 4:  M11 M15 M21 M19                                             (4)
wave 5:  M20            tier 2: M25 M26
```

M0a also ships `test_support.rs`: fixture builders that set the `pub(crate)` frame fields
directly (a board with one rotated / flipped / scaled footprint and a child of each kind), so
that wave 1-3 packages can run their frame tests before M1 and M19 land. Tests customise the
fixture from their own module (`fx.add_child(value, |fp| &mut fp.pads)`, `fx.set_child(id,
value)`, or the `pub(crate)` arenas); nobody edits `test_support.rs`.

**Wave 0b rules (fixed by the M0a review).** The four type packages edit disjoint files:

* *Frozen for everybody from now on:* `lib.rs`, `items_macro.rs`, `ids.rs`, `arena.rs`,
  `cache.rs`, `item.rs`, `traits.rs`, `ctx.rs`, `item_ref.rs`, `pending.rs`, `drc_api.rs`,
  `test_support.rs`, `foundation_stub.rs`, `pcb_track/*`, `Cargo.toml`. A package that needs a
  change there (a missing virtual, a foundation type, a dependency) reports it to the
  orchestrator and continues with a `port_pending!`.
* *A package edits only the main module files of its own types* (list in the M0b-e row),
  plus the sibling files of those modules (`pad/derive.rs`, `footprint/transform.rs`, ...)
  for the inherent stubs. Structural fields of the placeholders keep their names (`base`,
  `text`, `shape`, `textbox`, `group`, `members`, `cells`, the footprint child lists,
  `transform`, `flipped`, `NetinfoItem::net_code`, `NetinfoList::nets`): kernel code reads
  them.
* *Types that cross package borders exist already and are used as they are:*
  `ZoneConnection`, `ZoneLayerProperties` (zone_settings.rs), `TeardropParameters` /
  `TeardropParametersList` and `ZoneSettings` (placeholders M0b fills),
  `ComponentClass` / `ComponentClassCache` / `EmbeddedFiles` (placeholders M0d fills), the
  `Dim*` enums (pcb_dimension.rs), every item placeholder. A test of constructor defaults
  asserts only fields whose type belongs to the package.
* *`impl Board` stubs are written by the package that owns the file they belong to:* M0b the
  `Board::footprint_*` cascade family (footprint/transform.rs), `build_net_tie_cache`
  (footprint/mod.rs), `zone_fill_mut` (zone/mod.rs), `set_zone_layer_override` /
  `clear_zone_layer_overrides` (pad/mod.rs, for pads and vias); M0d everything else of
  sections 8.1, 8.2, 8.5 and of the types document's section 18: `add_*` / `add` /
  `add_marker` / `remove*` / `add_to_group` / `remove_from_group` in board/containers.rs,
  `rebind_item_uuid` in board/index.rs, `add_net` and the net functions in board/nets.rs,
  `move_item` / `rotate_item` / `flip_item` / `mirror_item` / `set_item_layer` /
  `set_item_locked` in board_item.rs, `set_item_net` in board_connected_item.rs, the rest in
  board/{layers,queries,text_vars,markers,compare,outline}.rs. M0d also makes the three
  kernel hooks real, because they are field reads in C++: `Board::copper_layer_count`,
  `Board::flip_layer`, `Board::build_layer_env` (board/layers.rs) and
  `BoardDesignSettings::max_error`, and ends `Board::default()` with
  `layer_env = build_layer_env()`.
* *Shims.* In the existing `impl` blocks of a type, one line per virtual that its C++ class
  or a base below the trait's class overrides (6.2); each package reports the list it derived
  from the headers.

The save-order comparators are model code (M2 `board_item_ptr_cmp`, M16 `pcb_track_cmp_tracks`,
M20 `footprint_cmp_*`, M22 `board_cmp_*`, M23 `pcb_point_cmp_points`); pcb-io.md PCBIO-02
consumes them and does not port them.

Downstream crates and the `io/` packages **compile** against the skeleton as soon as wave 0b is
in, but they call setters and shape functions that are `port_pending!` until the owning package
lands, so schedule them by this table, not by "after M0":

| downstream package | needs at run time |
|---|---|
| PCBIO-01 / 02 (parser and writer core, board skeleton), PCBIO-09 groups | M1, M2, M3, M22, M23 |
| PCBIO-04 setup, stackup, plot params | M5, M6 |
| PCBIO-03 tracks, vias, graphic shapes | M7, M9, M16, M17 |
| PCBIO-05 text / PCBIO-10 dimensions | M10, M11 / M12 |
| PCBIO-06 footprints / PCBIO-07 pads / PCBIO-08 zones | M19 / M13, M14 / M18 |
| connectivity + ratsnest | M1-M3, M14, M16-M18, M9; pad and zone contact shapes: M15 |
| DRC engine (rules, implicit rules) | M3-M5, M14, M17, M18 |
| DRC providers, plotting, drill files | the shape packages M8, M9, M10-M12, M15, M17, M18, M20, M21 |
| zone filler | M15, M17, M18, M20, M21, connectivity, DRC engine |

Nine agents editing one crate at once will break each other's `cargo check` now and then
(PORTING.md section 7 applies: small complete increments, wait and retry). The kernel files
listed under "Wave 0b rules" stay frozen; the trait shim blocks are frozen after wave 0b; a
package that needs a new module file, a new virtual or a dependency asks the orchestrator.

## 17. Deviations, deferred machinery, resolved decisions

**Deviations** (each into `kicad-pcb/PORT_STATUS.md`): pointer-address order replaced per
section 3.1; group member order -> insertion order; an item removed and re-added gets a new
handle (C++ keeps the address); the board time stamp moves on every edit (C++: commit push,
remove, DRC init); listeners replaced by time-stamp revalidation; `BOARD` caches used only by
DRC moved to `kicad-drc`; generated tables are not refreshed before save in tier 1 (C++
`RefreshGeneratedTables`); barcode symbols and drill-map symbols are not drawn until their
tier-2 packages land; generator geometry is not regenerated; library footprints are parsed
into a holder board instead of with a null board.

**Deferred: not built in M0-M24, not to be used by any tier 1/2 package.** Each item was in
revision 1; none is needed for load / save / connectivity / DRC / refill / plot / autorouter
clients, and each can be added later without changing a signature above.

| deferred | why it can wait | when it comes back |
|---|---|---|
| `BoardCommit` (add / modify / remove / push / revert) and arena *parking* (a removed item restorable under its handle) | nothing headless reverts; DRC markers and filler results are written directly; clients use `board.clone()` | tier 3 interactive router (`PNS_KICAD_IFACE::Commit`), undo in a GUI host |
| change-journal consumers (`apply_changes` in connectivity, journal-driven cache updates) | full rebuild is equivalent (R-6) | incremental connectivity for interactive use |
| `with_item_taken` / a `Taken` slot state | clone-edit-store covers it | never, unless profiling says so |
| `SideTable<T, V>` | `HashMap<ItemId, V>` is enough | optimisation only |
| `OnceCache` | `KeyedCache` with an always-valid predicate | - |
| `FootprintTree` / `TableTree` behaviour (`footprint_take`, adding a tree, copy between boards) | the parser creates containers in place; the types exist as data only | placing library footprints, clipboard, footprint update from library |
| `Similarity`, `NormalizeForCompare`, `Duplicate`, `swapData` | editor and library-parity operations | with their first consumer |

**Decisions of the orchestrator (they close the open questions of revision 1).**

* `kicad-geom` owns the kimath ports `TransformTrs`, `EdaOrientation` (already there),
  `Ellipse<T>` / `ShapeEllipse`, `Vector3D`, `ShapeNull`, `ErrorLoc`. Until one lands, M0 may
  carry a local stand-in in `kicad-pcb/src/foundation_stub.rs` (one module, each item marked
  `// REMOVE when kicad-geom provides X`); `rg foundation_stub` must be empty before the crate
  is declared done.
* `kicad-geom` also owns the kimath hash helpers (`Hash128`, MMH3, MD5; package G6a).
* `kicad-core` owns `UnitsProvider`, `CaseInsensitiveMap`, the shared plot
  enums (`PlotFormat`, `DrillMarks`, `PlotTextMode`, `DxfOutlineMode`, `DxfUnits`) and the JSON
  module (`serde_json` 1.0 with `preserve_order` + `float_roundtrip`, re-exported as
  `kicad_core::json::Value`, plus the hand-written nlohmann-compatible formatter of
  THIRD_PARTY_EQUIVALENTS.md 3.3). `TitleBlock`, `PageInfo`, `StrokeParams`,
  `TextAttributes`, `MAX_CU_LAYERS` are the existing kicad-core types; nothing here redefines
  them.
* `trait Shape` is `Send + Sync` (it already is); `RuleResolver` and `ConnectivityQuery` are
  `Sync`; `Board` is `Sync`.
* Netclass pattern matching uses `kicad_core::eda_pattern_match` (port of
  `common/eda_pattern_match.cpp`): wildcard patterns are hand-ported, regex patterns use the
  pure-Rust `regress` engine, which was closest to wxRegEx (PCRE2) in the experiment of
  THIRD_PARTY_EQUIVALENTS.md 3.7 (2 of 55 realistic patterns differ, against 14 for `regex`).
  A pattern `regress` cannot compile is a non-match with a warning, a recorded deviation. The
  M4 implementer greps every `.kicad_pro` of the corpus for netclass patterns and lists any
  affected ones in `PORT_STATUS.md`.
* Barcodes stay data-only until tier 2: round-tripped, bounding-rectangle shape, plotting
  reports the gap (M25 decides the encoder).
* `kicad-session` (new small crate, depends on kicad-pcb, kicad-connectivity, kicad-drc,
  kicad-zone) owns `Session { board, connectivity, drc }` and the `BOARD_LOADER` glue;
  `kicad-cli` and `kicad-wasm` are thin layers over it.
