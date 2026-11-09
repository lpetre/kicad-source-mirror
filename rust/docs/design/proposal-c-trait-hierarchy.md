# Proposal C - board model as a mirrored class hierarchy (base structs + object-safe traits)

Design proposal for the data model of the `kicad-pcb` crate. Assigned angle: **maximum structural
fidelity to the C++ class hierarchy**, so that the ~100k lines of consumer code (parser/writer,
connectivity, DRC, zone filler, plotters, router) translate with the least mental effort:
`BOARD_ITEM*` becomes `&dyn BoardItem`, a C++ override becomes a trait method of the same name, a
C++ member becomes a field of the same name, and the board owns its items in the same containers
in the same order.

Everything below was checked against this tree (board file format 20261031). C++ file names are
relative to the repository root; `P`/`W` are the parser and writer as in
`rust/docs/analysis/pcb-io.md`. The approach is pushed as far as it goes; section 2.11 lists every
place where it hurts and what is done about it.

---------------------------------------------------------------------------------------------------

## 0. Summary

```
Board                                             (BOARD : BOARD_ITEM_CONTAINER, EMBEDDED_FILES)
 |- items: BoardItems
 |    |- footprints : ItemDeque<Footprint>        std::deque<FOOTPRINT*>     m_footprints
 |    |     |- fields     : ItemDeque<PcbField>         std::deque<PCB_FIELD*>
 |    |     |- pads       : ItemDeque<Pad>              std::deque<PAD*>
 |    |     |- drawings   : ItemDeque<dyn BoardItem>    std::deque<BOARD_ITEM*>
 |    |     |- zones, groups, constraints, points, bodies (typed deques)
 |    |- tracks     : ItemDeque<dyn Track>        std::deque<PCB_TRACK*>     (trace, arc, via)
 |    |- drawings   : ItemDeque<dyn BoardItem>    std::deque<BOARD_ITEM*>    (shape, text, textbox,
 |    |                                             table, dimension, barcode, image, target, ...)
 |    |- zones      : ItemDeque<Zone>             std::vector<ZONE*>
 |    |- groups, generators (dyn Generator), points, markers, constraints
 |- index: BoardItemIndex                         BOARD_ITEM_INDEX  (Kiid -> ItemPtr -> location)
 |- env: BoardEnv                                 everything that is not an item:
 |    layers, BoardDesignSettings { NetSettings, BoardStackup, .. }, NetinfoList, properties,
 |    title block, page, plot params, variants, embedded files, component classes, project data
 |- hooks: connectivity (Box<dyn ConnectivityHook>), DRC engine (Arc<dyn DrcEngineHook>)
 |- time_stamp, change journal, board-level caches
```

| Topic | Decision |
|---|---|
| Classes | Every C++ class with subclasses becomes a data struct `XxxBase` (its members) plus a trait (its virtual interface, base implementations as provided methods). Every concrete class is a struct whose first field is the base struct of its C++ parent. No concrete struct is ever embedded in another one. |
| Polymorphism | Object-safe traits `EdaItem` -> `BoardItem` -> `BoardConnectedItem`, family traits `Track`, `ShapeItem`, `TextItem`, `TextboxItem`, `Dimension`, `GroupItem`, `Generator`, `TableItem`, mixin traits `EdaShape`, `EdaText`, `EdaGroup`. `&dyn BoardItem` is the Rust spelling of `BOARD_ITEM*` in a parameter. |
| Downcasts | `item.type_()` (= `Type()`), `item.as_item_ref()` returning the `ItemRef<'_>` enum (= `switch( Type() )` + `static_cast`), `item.as_pad()`, `item.as_track()`, `item.as_connected()` ... (= `dynamic_cast`). |
| Ownership | The C++ ownership tree, unchanged: the board owns `Box`ed items in deques, a footprint owns its children, a table owns its cells, a padstack owns its primitives. `ItemDeque<T>` reproduces `push_back` / `push_front` / `erase` order. |
| Identity | `Kiid` (file identity) and `ItemPtr` (the Rust spelling of a *stored* `BOARD_ITEM*`: a `Copy` board-unique serial, resolved in O(log n), ordered by creation). Read-only passes may also store `&'a dyn BoardItem` directly. |
| Parent pointers | `m_parent` becomes `ParentLink` in the item base. A child of a footprint holds an `Arc<FpLink>`: an immutable snapshot of what C++ reads through `GetParentFootprint()` in parameter-free getters and setters (the footprint's `TRANSFORM_TRS`, side, uuid, `ItemPtr`). The footprint replaces the snapshot in the `OnFootprintTransformed()` hook that C++ already calls on every child. A bare `&Pad` (or a cloned dummy pad) can therefore answer `position()`, `size( layer )`, `orientation()`, `drill_size()`. |
| Upward calls | `GetBoard()` becomes a `ctx: BoardCtx<'_>` argument (a `Copy` wrapper of `Option<&Board>`, `None` = "no board", with the C++ null-board defaults built in). Mutators that need board data take `env: EnvCtx<'_>` (the non-item half of the board), which the borrow checker allows next to `&mut item`. |
| Nets | Items store the net code (`m_netinfo` -> `net_code: i32`); `NetinfoList` owns `NetinfoItem`s keyed by code and by name; each net holds its effective `Arc<Netclass>`. |
| Model calls into higher crates | `PAD::FlashLayer` -> connectivity and `GetOwnClearance` / `GetSolderMaskExpansion` -> DRC engine go through two hook traits defined in `kicad-pcb` and stored in the board, like the two `std::shared_ptr` members in C++. |
| Mutation | `Board::add` / `remove` / `item_mut` / `modify( ptr, closure )`, split borrow `Board::split_mut()` -> (`ItemsMut`, `EnvCtx`), "compute with `&Board`, apply with `&mut Board`" for the zone filler and connectivity, `BoardCommit` for router/autorouter clients. A pull-based change journal replaces `BOARD_LISTENER`. |
| Caches | Per-item lazy caches as in C++ (`mutable` + dirty flag + mutex -> `OnceLock`, reset by the `&mut` setters); `EDA_ITEM::m_flags` is an `AtomicU32` so "mark" algorithms run on `&Board`. DRC's board-wide caches move to `kicad-drc`, keyed by `ItemPtr`. |
| Rare item types | Every item type is a typed struct implementing `BoardItem`; none is kept as an opaque s-expression (section 3.15 explains why opaque pass-through cannot be byte exact, and what "data level" means for plotting and DRC). |
| wasm | Items are addressed by `Kiid` string (durable) or `ItemPtr` as a JS number (fast); no borrowed Rust object crosses the boundary. |

### 0.1 Porting card (the whole proposal on one screen)

| C++ | Rust in this proposal |
|---|---|
| `BOARD_ITEM* item` parameter / local | `item: &dyn BoardItem` (`&mut dyn BoardItem` when modified) |
| `BOARD_ITEM*` stored in a map / vector / struct that survives board edits | `ItemPtr`; `board.item( ptr )` |
| `BOARD_ITEM*` stored in a structure that lives inside one read-only pass (R-tree, cluster) | `&'a dyn BoardItem` (the structure gets a lifetime) or `ItemPtr` |
| `a == b` on pointers | `a.ptr() == b.ptr()` (`same_item( a, b )`) |
| `item->Type()` / `switch( item->Type() )` | `item.type_()` / `match item.as_item_ref() { ItemRef::Pad( pad ) => .. }` |
| `static_cast<PAD*>( item )` after a type check | `item.as_pad().expect( ".." )` or the `match` arm above |
| `dynamic_cast<PCB_TRACK*>( item )` (class family) | `item.as_track()` -> `Option<&dyn Track>` |
| `dynamic_cast<BOARD_CONNECTED_ITEM*>( item )`, `item->IsConnected()` | `item.as_connected()` -> `Option<&dyn BoardConnectedItem>` |
| `dynamic_cast<EDA_TEXT*>( item )` | `item.as_eda_text()` -> `Option<&dyn EdaText>` |
| `item->GetBoard()` | the `ctx` (or `board`) the function received; `ctx.board()` is `Option<&Board>` |
| `if( const BOARD* b = GetBoard() ) x = b->Foo(); else x = DEFAULT;` | `ctx.foo()` (default built in) or `if let Some( b ) = ctx.board() { .. }` |
| `item->GetParentFootprint()` | `item.parent_footprint( ctx )` -> `Option<&Footprint>`; identity only: `item.parent_footprint_ptr()` |
| `fp->GetTransform()` inside a child getter | `self.base().fp_transform()` (snapshot, no ctx) |
| `item->GetParentGroup()` | `item.parent_group()` -> `Option<ItemPtr>`; `ctx.group( ptr )` |
| `item->GetEffectiveShape( layer )` | `item.effective_shape( ctx, layer, Flashing::Default, DrcConstraintT::NullConstraint )` |
| `item->GetNetCode()` | `item.net_code()` |
| `item->GetNet()`, `GetNetname()`, `GetEffectiveNetClass()` | `item.net( ctx )`, `item.netname( ctx )`, `item.effective_net_class( ctx )` |
| `board->Add( item, ADD_MODE::APPEND )` | `board.add( Box::new( item ), AddMode::Append )` -> `ItemPtr` |
| `board->Remove( item ); delete item;` | `board.remove( ptr, RemoveMode::Normal )` -> `Option<Box<dyn BoardItem>>` (dropping it deletes) |
| `PAD dummy( *pad ); dummy.SetSize(..)` | `let mut dummy = pad.clone(); dummy.set_size(..)` (the clone keeps the parent snapshot, like the C++ copy keeps `m_parent`) |
| `item->Clone()` | `item.clone_item()` -> `Box<dyn BoardItem>` |
| `*a == *b` (virtual `operator==`) | `a.eq_item( b )` |
| `item->SetFlags( SKIP_STRUCT )` on a const-ish item | `item.set_flags( SKIP_STRUCT )` (takes `&self`) |
| `virtual T Foo() const` / `override` | trait method `fn foo( &self ) -> T` in the trait of the class that first declares it |
| non-virtual method of a base class | provided method of the family trait (dispatches correctly on every subclass) |
| `mutable` cache + dirty flag + mutex | `OnceLock<..>` field; `set_dirty( &mut self )` replaces it with a fresh `OnceLock` |

---------------------------------------------------------------------------------------------------

## 1. Facts about the C++ model that drive the design

1. **The hierarchy.** `EDA_ITEM` (uuid, type, flags, parent, group, custom properties) ->
   `BOARD_ITEM` (layer, knockout, locked) -> `BOARD_ITEM_CONTAINER` (`BOARD`, `FOOTPRINT`,
   `PCB_TABLE`) and `BOARD_CONNECTED_ITEM` (net, teardrop parameters). Multi-level chains:
   `PCB_TRACK` -> `PCB_ARC`, `PCB_VIA`; `PCB_SHAPE` (also `EDA_SHAPE`) -> `PCB_TEXTBOX` (also
   `EDA_TEXT`) -> `PCB_TABLECELL`; `PCB_TEXT` (also `EDA_TEXT`) -> `PCB_FIELD`,
   `PCB_DIMENSION_BASE` -> `PCB_DIM_ALIGNED` -> `PCB_DIM_ORTHOGONAL`, `PCB_DIM_RADIAL`,
   `PCB_DIM_LEADER`, `PCB_DIM_CENTER`; `PCB_GROUP` (also `EDA_GROUP`) -> `PCB_GENERATOR` ->
   `PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_GENERATOR_POLY` -> `PCB_VIA_STITCH`; `PCB_TABLE`
   -> `PCB_GENERATED_TABLE` -> `PCB_DRILL_CHART`, `PCB_STACKUP_TABLE`.
2. **How consumers use it** (occurrence counts in this tree; drc = `pcbnew/drc`, conn =
   `pcbnew/connectivity`, zone = `zone_filler.cpp` + teardrops, plot = plot/drill/gerber exporters,
   router = `pcbnew/router`, io = `pcb_io/kicad_sexpr`, model = the item `.cpp` files themselves):

   | Idiom | drc | conn | zone | plot | router | io | model |
   |---|---:|---:|---:|---:|---:|---:|---:|
   | `->Type()` | 186 | 50 | 37 | 17 | 62 | 21 | 126 |
   | `static_cast<` | 208 | 82 | 63 | 38 | 392 | 80 | 219 |
   | `dynamic_cast<` | 49 | 12 | 1 | 5 | 27 | 27 | 72 |
   | `GetBoard()` | 64 | 6 | 1 | 0 | 16 | 4 | 143 |
   | `GetParentFootprint()` | 29 | 6 | 0 | 5 | 8 | 10 | 108 |
   | `GetParent()` | 15 | 43 | 0 | 1 | 2 | 1 | 20 |
   | `GetLayerSet()` | 68 | 9 | 21 | 18 | 4 | 11 | 52 |
   | `GetNetCode()` | 52 | 31 | 30 | 1 | 15 | 8 | 27 |
   | `GetEffectiveShape(` | 40 | 5 | 7 | 1 | 1 | 0 | 21 |
   | `GetBoundingBox()` | 32 | 1 | 36 | 2 | 2 | 0 | 53 |
   | containers keyed by item pointers (`std::map<X*,..>` etc.) | 54 | 29 | 34 | 0 | 69 | 33 | 14 |

   Consumers are dominated by type switches, casts and a handful of virtuals. The upward walks
   (`GetBoard()`, `GetParentFootprint()`) are concentrated in the model's own code (251 of 390
   occurrences), i.e. inside the crate this document designs. That is what makes a trait hierarchy
   viable: consumers mostly need `&dyn BoardItem` plus a context they already hold (`m_board`).
3. **Footprint children live in the footprint's library frame** (format 20260616). `FOOTPRINT` has
   `TRANSFORM_TRS m_transform` (`libs/kimath/include/geometry/transform_trs.h`: translate, rotate,
   scale x/y). `PAD::GetPosition()` is `fp->GetTransform().Apply( m_libPos )`
   (`pcbnew/pad.cpp:255`); `GetSize`, `GetOffset`, `GetPrimaryDrillSize`, `GetOrientation`,
   `PCB_TEXT::GetTextPos/Size/Thickness/Angle`, `ZONE::GetBoardOutline`, `PCB_BARCODE`,
   `PCB_DIMENSION_BASE` derive board-frame values on read. `PCB_SHAPE` and `PCB_POINT` keep both a
   library copy (`m_libStart`, ...) and a baked board copy, re-baked by `OnFootprintTransformed()`.
   Every footprint mutator (`SetPosition`, `SetOrientation`, `Flip`, `SetTransformScale`,
   `pcbnew/footprint.cpp:3590-3850`) already calls `OnFootprintTransformed()` /
   `OnFootprintRescaled()` on every child. This existing hook is what the parent snapshot of
   section 2.5 rides on.
4. **Children reach further up for non-geometric data**: `IsLocked()` (parent group, board use),
   `GetMaxError()`, `BoardCopperLayerCount()` (32 without a board, `board_item.cpp:219`),
   `GetLayerName()`, text variables (`PCB_TEXT::GetShownText` -> footprint -> board -> project),
   `PAD::GetClearanceOverrides` -> footprint, net-tie groups, `SetNetCode` -> `board->FindNet`.
   About 100 of these are guarded `if( const BOARD* board = GetBoard() )` with a default branch.
5. **The model calls upward into other subsystems**: `PAD::FlashLayer`, `PCB_VIA::FlashLayer` ->
   `board->GetConnectivity()->IsConnectedOnLayer( this, layer, types )` (`pad.cpp:697-768`);
   `GetOwnClearance`, `PAD/PCB_TRACK/PCB_SHAPE::GetSolderMaskExpansion`, `PCB_TRACK::GetWidthConstraint`,
   `ZONE::GetThermalReliefGap( pad )` -> `bds.m_DRCEngine->EvalRules(..)` (`pad.cpp:1977`, 16
   call sites in model files). `kicad-connectivity` and `kicad-drc` sit above `kicad-pcb`.
6. **Containers and order**: `m_tracks`, `m_footprints`, `m_drawings`, `m_groups`, `m_generators`,
   `m_points`, `m_constraints`, `FOOTPRINT::m_pads/m_drawings/m_fields` are `std::deque<T*>`;
   `m_zones`, `m_markers` are `std::vector`. `BOARD::Add` (`board.cpp:1520`) does `push_front` for
   `ADD_MODE::INSERT` (the default) on tracks, footprints and drawings, `push_back` for `APPEND` /
   `BULK_APPEND` and for every other container; `Remove` is `std::erase`. DRC
   (`DRC_TEST_PROVIDER::forEachGeometryItem`, `drc_test_provider.cpp:150-360`), connectivity and
   the zone filler iterate in container order; the writer re-sorts everything into `std::set`s
   whose last tie-break is the object address.
7. **Identity**: `m_Uuid`, `BOARD_ITEM_INDEX` (`KIID` <-> pointer maps behind
   `BOARD::ResolveItem`, duplicates tolerated: the later binding wins). Pointers are used as map
   keys all over the consumers (table above); `std::map<ZONE*, ..>` is even iterated
   (`m_ZoneIsolatedIslandsMap`).
8. **Consumers make detached copies** that must keep working: `PAD dummy( *pad )` in
   `plot_board_layers.cpp` and `zone_filler.cpp`, `PCB_SHAPE ptAShape( nullptr, SHAPE_T::SEGMENT )`
   in `drc_test_provider.cpp`, `ZONE` copies in the filler. The copy keeps `m_parent`.
9. **Writes back into the model**: zone filler (`SetFilledPolysList`, `SetFillFlag`, `SetIsIsland`,
   `SetIsFilled`, `SetNeedRefill`, `BuildHashValue`, `CacheTriangulation`, `CacheBoundingBox`,
   `PAD/PCB_VIA::SetZoneLayerOverride`), connectivity (`SetNetCode` during net propagation), DRC
   (`BuildCourtyardCaches`, `BuildNetTieCache`, flags `MALFORMED_*_COURTYARD`, markers), router
   commits (add/remove/modify tracks and vias).
10. **Flags are scratch space**: `SKIP_STRUCT`, `CANDIDATE`, `IS_LINKED`, `HOLE_PROXY`,
    `STRUCT_DELETED`, `ROUTER_TRANSIENT` are set and cleared by algorithms on items they do not
    otherwise modify.

---------------------------------------------------------------------------------------------------

## 2. Ownership and identity architecture

### 2.1 Class mapping rules

These five rules generate every type in section 3.

* **M1 - data.** A C++ class `X` that has subclasses becomes `struct XBase` holding exactly the
  data members of `X`, with the base struct of `X`'s own C++ parent as its *first field*:
  `EdaItemBase`, `BoardItemBase { eda: EdaItemBase, .. }`, `BoardConnectedItemBase { item:
  BoardItemBase, .. }`, `PcbTrackBase { conn: BoardConnectedItemBase, .. }`, `PcbShapeBase`,
  `PcbTextBase`, `PcbTextboxBase`, `PcbDimensionBase`, `PcbDimAlignedBase`, `PcbGroupBase`,
  `PcbGeneratorBase`, `PcbTableBase`, `PcbGeneratedTableBase`, and the mixins `EdaShapeBase`,
  `EdaTextBase`, `EdaGroupBase`.
* **M2 - concrete classes.** A concrete C++ class is a struct of the same name (`PcbVia`, `Pad`,
  `Zone`, ...) whose first field is the base struct of its C++ parent. A class that is both
  concrete and a base (`PCB_TRACK`, `PCB_SHAPE`, `PCB_TEXT`, `PCB_TEXTBOX`, `PCB_GROUP`,
  `PCB_TABLE`, `PCB_DIM_ALIGNED`) gets both: `PcbTrackBase` (data, shared with `PcbArc` and
  `PcbVia`) and the thin concrete `PcbTrack { track: PcbTrackBase }`. **No concrete struct is ever
  a field of another item struct**, so there is no way to call a base-class override on a derived
  object by accident.
* **M3 - interface.** The virtual interface of a base class is a trait: `EdaItem`, `BoardItem:
  EdaItem`, `BoardConnectedItem: BoardItem`, `Track: BoardConnectedItem`, `ShapeItem:
  BoardConnectedItem + EdaShape`, `TextItem: BoardItem + EdaText`, `TextboxItem: ShapeItem +
  EdaText`, `Dimension: TextItem`, `GroupItem: BoardItem + EdaGroup`, `Generator: GroupItem`,
  `TableItem: BoardItem`. Each trait has one required accessor to its base struct (`fn track(
  &self ) -> &PcbTrackBase`), the C++ virtuals as methods, and the C++ non-virtual methods of the
  class as *provided* methods written against the accessor. Because provided methods are
  dispatched on the dynamic type, a base method that calls a virtual (`PCB_TRACK::GetLength()`
  calling `GetWidth()`) behaves as in C++.
* **M4 - inherited implementations.** Rust cannot inherit an *overridden* method from an
  intermediate base (`PCB_ARC` uses `PCB_TRACK::GetLayerSet`, which overrides
  `BOARD_ITEM::GetLayerSet`). The base implementation is therefore a generic free function in the
  base's module, `pcb_track::layer_set<T: Track + ?Sized>( this: &T ) -> Lset`, and each concrete
  type's `impl BoardItem` names the implementation it uses. The table is written with one macro
  per type and is reviewed against the `override` list of the C++ header:

  ```rust
  impl_virtuals! { PcbArc =>
      BoardItem {
          layer_set        = pcb_track::layer_set,          // inherited from PCB_TRACK
          is_on_layer      = pcb_track::is_on_layer,
          bounding_box     = pcb_track::bounding_box,
          effective_shape  = pcb_arc::effective_shape,      // PCB_ARC override
          hit_test         = pcb_arc::hit_test,
          rotate = pcb_arc::rotate, mirror = pcb_arc::mirror, flip = pcb_arc::flip,
      }
      Track { length = pcb_arc::length }                    // GetWidth inherited: trait default
  }
  ```

  Methods not listed fall back to the trait's provided body, which is the `BOARD_ITEM` /
  `EDA_ITEM` implementation. This is the vtable written out by hand: explicit, greppable,
  checkable.
* **M5 - mixins.** `EDA_SHAPE`, `EDA_TEXT`, `EDA_GROUP`, `EMBEDDED_FILES`, `MARKER_BASE` are data
  structs held by value (`shape: EdaShapeBase`) plus, where the mixin has virtuals that the board
  class overrides (`EDA_SHAPE::GetWidth`, `getMaxError`, `EDA_TEXT::GetTextPos`, `GetShownText`,
  ...), a trait (`EdaShape`, `EdaText`, `EdaGroup`) with the mixin's methods as provided methods.

### 2.2 Ownership

The ownership tree is the C++ one. `Board` owns `Box<Footprint>`, `Box<dyn Track>`,
`Box<dyn BoardItem>` (drawings), `Box<Zone>`, ...; `Footprint` owns its fields, pads, drawings,
zones, groups, constraints, points and bodies; `PcbTableBase` owns `Vec<Box<PcbTablecell>>`;
`CopperLayerProps` owns the custom-shape primitives (`Vec<Arc<PcbShape>>`, a `shared_ptr` vector in
C++); `PcbBarcode` owns its `PcbText` by value; generators own their template items. Groups and
generators do **not** own their members (C++: "no ownership implied"): they hold `ItemPtr`s.
There is no `Rc`, no `RefCell`, no raw pointer and no lifetime parameter in any item type.

Items are boxed because (a) the heterogeneous containers need it, (b) `push_front` and `erase`
then move 8 or 16 bytes per element, exactly like the C++ pointer deques, and (c) a `Box` gives
every item a stable address for the whole time it is attached, so read-only passes can keep
`&'a dyn BoardItem` in their own structures.

### 2.3 `ItemDeque<T>` (new code; replaces `std::deque<T*>` / `std::vector<T*>`)

```rust
/// Ordered owning container of board items. Port of the `std::deque<T*>` / `std::vector<T*>`
/// members of BOARD and FOOTPRINT together with the three ways the C++ modifies them
/// (`push_back`, `push_front`, `std::erase`). `T` may be a trait object.
pub struct ItemDeque<T: BoardItem + ?Sized> {
    items: VecDeque<Box<T>>,
    /// Order keys handed out so far; `items` is always sorted by `base().order_key`.
    next_back: OrderKey,      // starts at 0 and grows
    next_front: OrderKey,     // starts at -1 and shrinks
}

/// Position key of an item inside its `ItemDeque`: monotone along the deque, never reused.
pub type OrderKey = i64;

impl<T: BoardItem + ?Sized> ItemDeque<T> {
    pub fn push_back( &mut self, item: Box<T> ) -> &mut T;          // ADD_MODE::APPEND, BULK_APPEND
    pub fn push_front( &mut self, item: Box<T> ) -> &mut T;         // ADD_MODE::INSERT
    pub fn remove( &mut self, key: OrderKey ) -> Option<Box<T>>;    // std::erase( list, item )
    pub fn get( &self, key: OrderKey ) -> Option<&T>;               // binary search, O(log n)
    pub fn get_mut( &mut self, key: OrderKey ) -> Option<&mut T>;
    pub fn iter( &self ) -> impl DoubleEndedIterator<Item = &T> + ExactSizeIterator + Clone;
    pub fn iter_mut( &mut self ) -> impl DoubleEndedIterator<Item = &mut T> + ExactSizeIterator;
    pub fn len( &self ) -> usize;
    pub fn is_empty( &self ) -> bool;
    pub fn front( &self ) -> Option<&T>;
    pub fn back( &self ) -> Option<&T>;
    pub fn at( &self, index: usize ) -> Option<&T>;                 // m_zones[ii], GetArea( index )
    pub fn clear( &mut self ) -> Vec<Box<T>>;                       // RemoveAll
    pub fn retain_or_take( &mut self, keep: impl FnMut( &T ) -> bool ) -> Vec<Box<T>>;  // bulk erase
}
```

`push_back` stores `next_back` into the item's `order_key` and increments it; `push_front` stores
`next_front` and decrements it. The deque is therefore always sorted by key without ever
renumbering, and an item is found again by binary search. No positional index has to be maintained
when another item is removed or pushed at the front: removing one track costs one memmove of
pointers, exactly as in C++, and nothing else.

### 2.4 Identity: `Kiid`, `ItemPtr`, `BoardItemIndex`

Three things are called "the item" in C++ and all three are kept:

* **`Kiid`** (`m_Uuid`): what the file stores, what DRC reports print, what groups / constraints /
  net-chain terminals are written with. Not unique by construction (duplicates in files survive,
  see `CoincidentDrawingsSurviveSave` and `BOARD::RepairDuplicateItemUuids`).
* **`ItemPtr`**: the port of a *stored* pointer and of pointer identity.

  ```rust
  /// Non-owning reference to an item that is attached to a board. The Rust spelling of a stored
  /// `BOARD_ITEM*`, of `this` used as a map key, and of pointer comparison.
  /// Board-unique serial number, assigned when the item is bound to the board's index, never
  /// reused. `Ord` is creation (binding) order: the deterministic replacement for every place
  /// where C++ orders or tie-breaks by object address.
  #[derive( Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default )]
  pub struct ItemPtr( u32 );

  impl ItemPtr {
      pub const NULL: ItemPtr = ItemPtr( 0 );      // nullptr; also the value of detached items
      pub fn is_null( self ) -> bool;
      pub fn as_u32( self ) -> u32;                // wasm boundary
      pub fn from_u32( raw: u32 ) -> ItemPtr;
  }
  ```

  An item gets its `ItemPtr` in `Board::add` (and its children in the same call, C++
  `BindSubtree`), loses it in `Board::remove`. A detached item has `ItemPtr::NULL`, which is the
  port of `BOARD_ITEM::IsIndexedInBoard() == false`.
* **`&'a dyn BoardItem`**: inside one pass that only reads the board (a DRC run after the
  preparation phase, a connectivity build, plotting) the address of a boxed item is stable, so
  consumer structures may hold the reference itself, as the C++ holds the pointer: `struct
  ItemWithShape<'a> { parent: &'a dyn BoardItem, shape: .. }` in the DRC R-tree. This has zero
  lookup cost. Structures that survive `&mut Board` (persistent connectivity data, the router
  world, caches) hold `ItemPtr`.

```rust
/// Port of BOARD_ITEM_INDEX (pcbnew/board_item_index.h): the forward map (Kiid -> item) and the
/// reverse knowledge (item -> where it lives).
pub struct BoardItemIndex {
    /// m_itemsById. A later bind with the same Kiid evicts the earlier entry, as in C++.
    items_by_id: HashMap<Kiid, ItemPtr>,           // lookup only, never iterated
    /// Indexed by `ItemPtr::as_u32()`. `None` = unbound (removed) serial.
    locations: Vec<Option<Location>>,
    next_serial: u32,
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub( crate ) struct Location {
    pub list: ListKind,       // which container
    pub owner: ItemPtr,       // NULL for board-level lists, else the footprint / table that owns it
    pub key: OrderKey,        // position key inside that container
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub( crate ) enum ListKind {
    Footprints, Tracks, Drawings, Zones, Groups, Generators, Points, Markers, Constraints,
    FpFields, FpPads, FpDrawings, FpZones, FpGroups, FpConstraints, FpPoints, FpBodies,
    TableCells,
}

impl BoardItemIndex {
    pub fn find( &self, id: &Kiid ) -> Option<ItemPtr>;                    // Find
    pub( crate ) fn bind_item( &mut self, uuid: Kiid, loc: Location ) -> ItemPtr;   // BindItem
    pub( crate ) fn unbind_item( &mut self, ptr: ItemPtr, uuid: &Kiid );   // UnbindItem
    pub( crate ) fn rebind( &mut self, ptr: ItemPtr, old: &Kiid, new: Kiid );       // Rebind
    pub fn is_indexed( &self, id: &Kiid ) -> bool;
    pub fn clear( &mut self );
}
```

Resolution `Board::item( ptr )`: `locations[ptr]` (O(1)), then the container's binary search by
`key`; for children, the owner is resolved first (depth at most 3: footprint -> table -> cell).
Cost O(log n), about 50-100 ns; hot loops avoid it by holding `&'a dyn BoardItem`.

Deviations (to record in `PORT_STATUS.md`): pointer-order tie-breaks become `ItemPtr` order
(creation order); an item removed and added again gets a new `ItemPtr` (C++ keeps the address;
the `BoardCommit` of section 2.9 hides this from undo); `ResolveItem`'s linear-scan fallback for
unindexed items is not needed because every attached item is indexed.

### 2.5 Parent links: `ParentLink` and the `FpLink` snapshot

```rust
/// Port of EDA_ITEM::m_parent. Says what the parent is and carries the data of the enclosing
/// footprint that C++ reads through `GetParentFootprint()` inside parameter-free accessors.
#[derive( Clone, Debug, Default, PartialEq )]
pub enum ParentLink {
    /// m_parent == nullptr.
    #[default]
    Detached,
    /// Direct child of the BOARD (which may be a footprint holder).
    Board,
    /// Direct child of a FOOTPRINT: pad, field, graphic, zone, group, point, constraint, body.
    Footprint( Arc<FpLink> ),
    /// Child of another item: PCB_TABLECELL of a PCB_TABLE, the PCB_TEXT of a PCB_BARCODE,
    /// a template item of a generator. `fp` is the enclosing footprint, if any
    /// (C++ `findParent( PCB_FOOTPRINT_T )` walks through the direct parent).
    Item { parent: ItemPtr, kind: KicadT, fp: Option<Arc<FpLink>> },
    /// Custom-shape primitive owned by a PADSTACK (m_parent is the PAD). `scale` is
    /// `PAD::GetPrimitiveLibScale()`; `PCB_SHAPE::transformFp()` is null for these.
    Pad { pad: ItemPtr, scale: ( f64, f64 ), fp: Option<Arc<FpLink>> },
}

/// Immutable snapshot of the parent footprint, shared by all children of one footprint.
/// Replaced (never mutated) by the footprint whenever one of the four values changes.
#[derive( Clone, Debug, PartialEq )]
pub struct FpLink {
    pub ptr: ItemPtr,              // the FOOTPRINT's ItemPtr (NULL while it is detached)
    pub uuid: Kiid,                // the FOOTPRINT's m_Uuid (identity of detached footprints)
    pub transform: TransformTrs,   // copy of FOOTPRINT::m_transform
    pub layer: PcbLayerId,         // FOOTPRINT::GetLayer(): F_Cu or B_Cu (IsFlipped)
}

impl ParentLink {
    pub fn fp( &self ) -> Option<&FpLink>;                 // enclosing footprint, any depth
    pub fn fp_ptr( &self ) -> ItemPtr;                     // NULL if none
    pub fn direct_parent( &self ) -> ( KicadT, ItemPtr );  // ( PcbT, NULL ) for Board
    pub fn is_detached( &self ) -> bool;
}
```

Why a snapshot instead of "ask the board for my parent every time":

* It keeps the **signatures of the typed geometry accessors identical to C++**: `pad.position()`,
  `pad.size( layer )`, `pad.set_position( p )`, `shape.start()`, `text.text_pos()`,
  `zone.board_outline()` take no context. These are the calls the writer, the drill and Gerber
  generators and the connectivity anchors are made of.
* **Dummies work.** `let mut dummy = pad.clone()` shares the `Arc<FpLink>`, exactly as the C++
  copy constructor keeps `m_parent`, so the zone filler's and the plotter's dummy pads keep their
  board-frame geometry.
* **A footprint can be edited without the board**: `fp.set_orientation( a )` needs nothing but
  the footprint, so `&mut Footprint` obtained from the board is enough.

How it is kept right (invariant I-FP: for every child `c` of footprint `f`:
`c.parent.fp().transform == f.transform && .layer == f.layer && .uuid == f.uuid && .ptr == f.ptr`):

* `Footprint::transform`, `layer` are private; the only writers are the ports of `SetPosition`,
  `SetOrientation`, `SetTransformScale`, `RescaleAroundPoint`, `Flip`, `SetLayer`,
  `SetLayerAndFlip`, `Move`, `Rotate`. Each of them ends, as in C++, with the loop over all
  children calling the hook, whose Rust signature carries the new snapshot:
  `fn on_footprint_transformed( &mut self, link: &Arc<FpLink> )` /
  `fn on_footprint_rescaled( &mut self, link: &Arc<FpLink>, ratio_x, ratio_y, linear_factor,
  anchor, parent_rotate )`. The provided body stores the link; overrides (`Pad`: `set_dirty`,
  `PcbShape`: `rebake_from_lib`, `PcbPoint`, `PcbTextbox`, `PcbTable`: forward to cells) call it
  first. One `Arc` allocation per footprint edit, one pointer copy per child.
* `Footprint::add( item, mode )` sets the child's link (C++ `aBoardItem->SetParent( this )`);
  binding/unbinding the footprint in the board index re-issues the link because `ptr` changes;
  `Board::rebind_item_uuid` does the same for `uuid`.
* `Footprint::check_links()` (debug builds and tests) verifies I-FP; `Board::check_invariants()`
  runs it for every footprint.

What is *not* in the snapshot: everything else C++ reads through the parent (reference, local
clearance / mask / paste overrides, net-tie groups, locked state, attributes, private layers,
siblings). Those go through `item.parent_footprint( ctx )`, which resolves `FpLink::ptr` in the
board. They occur in methods that need the board anyway.

### 2.6 Context: `BoardCtx`, `EnvCtx`, and the two hooks

```rust
/// What C++ reaches through `GetBoard()` from a const method. `Copy`, one word.
/// `BoardCtx::NONE` is `GetBoard() == nullptr` (library footprints, detached dummies).
#[derive( Clone, Copy )]
pub struct BoardCtx<'a> { board: Option<&'a Board> }

/// What C++ reaches through `GetBoard()` from a mutator: the board *without its items*, so it
/// can be borrowed next to `&mut item`. `Copy`, one word.
#[derive( Clone, Copy )]
pub struct EnvCtx<'a> { env: Option<&'a BoardEnv> }

impl<'a> BoardCtx<'a> {
    pub const NONE: BoardCtx<'static>;
    pub fn board( self ) -> Option<&'a Board>;                 // if( const BOARD* board = GetBoard() )
    pub fn env( self ) -> EnvCtx<'a>;
    // --- other items (the reason BoardCtx is more than EnvCtx) ---
    pub fn item( self, ptr: ItemPtr ) -> Option<&'a dyn BoardItem>;
    pub fn footprint( self, ptr: ItemPtr ) -> Option<&'a Footprint>;
    pub fn group( self, ptr: ItemPtr ) -> Option<&'a dyn GroupItem>;
    pub fn connectivity( self ) -> Option<&'a dyn ConnectivityHook>;   // GetBoard()->GetConnectivity()
    pub fn drc_engine( self ) -> Option<&'a dyn DrcEngineHook>;        // bds.m_DRCEngine
    // --- everything EnvCtx offers, forwarded ---
}

impl<'a> EnvCtx<'a> {
    pub const NONE: EnvCtx<'static>;
    pub fn env( self ) -> Option<&'a BoardEnv>;
    pub fn design_settings( self ) -> Option<&'a BoardDesignSettings>;
    // The C++ null-board defaults live here, once:
    pub fn max_error( self ) -> i32;                 // BOARD_ITEM::GetMaxError: m_MaxError or ARC_HIGH_DEF
    pub fn copper_layer_count( self ) -> i32;        // BoardCopperLayerCount: board value or 32
    pub fn layer_count( self ) -> i32;               // BoardLayerCount
    pub fn layer_set( self ) -> Lset;                // BoardLayerSet: enabled layers or AllLayersMask
    pub fn layer_name( self, layer: PcbLayerId ) -> String;      // board name or LayerName( layer )
    pub fn flip_layer( self, layer: PcbLayerId ) -> PcbLayerId;  // BOARD::FlipLayer or ::FlipLayer
    pub fn is_footprint_holder( self ) -> bool;      // false without a board
    pub fn find_net( self, code: i32 ) -> Option<&'a NetinfoItem>;
    pub fn find_net_by_name( self, name: &str ) -> Option<&'a NetinfoItem>;
    pub fn default_netclass( self ) -> &'a Netclass; // net settings default or Netclass::fallback()
    pub fn time_stamp( self ) -> i32;
    pub fn resolve_text_var( self, token: &mut String, depth: i32 ) -> bool;   // BOARD::ResolveTextVar
}

impl Board {
    pub fn ctx( &self ) -> BoardCtx<'_>;
    pub fn env_ctx( &self ) -> EnvCtx<'_>;
    /// Split borrow: mutable access to the items, shared access to everything else.
    pub fn split_mut( &mut self ) -> ( ItemsMut<'_>, EnvCtx<'_> );
}
```

**Context rule (C1).** A method takes `ctx: BoardCtx<'_>` as its first argument if and only if its
C++ body, in any override, dereferences the board, another item, a group, the net object or a
hook. It takes `env: EnvCtx<'_>` instead if it is a mutator (`&mut self`) and only needs
board-level data. Otherwise it takes nothing. Consequences, all visible in section 4:

* `BoardItem` virtuals **with** `ctx`: `bounding_box`, `center`, `hit_test*`, `effective_shape`,
  `effective_hole_shape`, `transform_shape_to_polygon`, `transform_shape_to_poly_set`, `is_locked`,
  `is_tented`, `item_description`, `similarity`, `parent_footprint`; connected items: `net`,
  `netname`, `effective_net_class`, `own_clearance`, `clearance_overrides`. (Text needs it for
  text variables, pads and vias for flashing and mask expansion, groups for their members.)
* `BoardItem` virtuals **without**: `type_`, `uuid`, `ptr`, `flags`, `layer`, `layer_set`,
  `is_on_layer`, `is_on_copper_layer`, `position`, `set_position`, `is_connected`, `has_hole`,
  `is_knockout`, `net_code`, `local_clearance`, `move_`, `rotate`, `mirror`, and every typed
  geometry accessor.
* Mutators with `env`: `flip` (board-specific layer pairing), `set_net_code` (`board->FindNet`),
  `PcbVia::sanitize_layers`, `Footprint::fix_up_pads_for_board`, `Zone::set_layer_set...`.

One deliberate exception (deviation D-GRP): `PCB_GROUP::GetLayerSet / IsOnLayer / GetPosition`
walk the members. Giving `layer_set()` a context for the sake of groups would tax the 180 call
sites that ask tracks, pads and zones. The trait versions therefore answer for the group itself
(empty set, `false`, origin) and `GroupItem` offers `members_layer_set( ctx )`,
`members_bounding_box( ctx )`, `members_position( ctx )`. The only in-scope caller that sorts
groups by layer set is the writer (`BOARD_ITEM::ptr_cmp` on `m_groups`, W:910); it uses
`group_ptr_cmp( ctx, a, b )`. No DRC / connectivity / zone / plot code asks a group for its layers
(checked: the only group uses there are `GetParentGroup()` type tests).

**Hooks.** The two upward calls of fact 5 are trait objects stored in the board, mirroring
`std::shared_ptr<CONNECTIVITY_DATA> BOARD::m_connectivity` and
`std::shared_ptr<DRC_ENGINE> BOARD_DESIGN_SETTINGS::m_DRCEngine`:

```rust
/// Implemented by kicad-connectivity's CONNECTIVITY_DATA port. Only what the model itself calls.
pub trait ConnectivityHook: Any + Send + Sync {
    /// CONNECTIVITY_DATA::IsConnectedOnLayer (used by PAD::FlashLayer, PCB_VIA::FlashLayer).
    fn is_connected_on_layer( &self, board: &Board, item: &dyn BoardConnectedItem,
                              layer: PcbLayerId, types: &[KicadT] ) -> bool;
    /// CONNECTIVITY_DATA::GetNodeCount (BOARD::GetNodesCount).
    fn node_count( &self, net: i32 ) -> u32;
    fn as_any( &self ) -> &dyn Any;
    fn as_any_mut( &mut self ) -> &mut dyn Any;
    fn into_any( self: Box<Self> ) -> Box<dyn Any>;
}

/// Implemented by kicad-drc's DRC_ENGINE port. The engine holds no reference to the board;
/// the board is passed to every call. Internal caches use interior mutability.
pub trait DrcEngineHook: Any + Send + Sync {
    fn eval_rules( &self, board: &Board, constraint: DrcConstraintT, a: &dyn BoardItem,
                   b: Option<&dyn BoardItem>, layer: PcbLayerId ) -> DrcConstraint;
    fn has_rules_for_constraint_type( &self, constraint: DrcConstraintT ) -> bool;
    fn cached_own_clearance( &self, board: &Board, item: &dyn BoardItem, layer: PcbLayerId )
            -> ( i32, Option<String> );
    fn query_worst_constraint( &self, constraint: DrcConstraintT ) -> Option<DrcConstraint>;
    fn invalidate_clearance_cache( &self, uuid: &Kiid );
    fn as_any( &self ) -> &dyn Any;
}

impl Board {
    pub fn set_connectivity( &mut self, hook: Option<Box<dyn ConnectivityHook>> );
    pub fn connectivity( &self ) -> Option<&dyn ConnectivityHook>;
    /// Takes the hook out, runs `f( hook, &board )`, puts it back. This is how
    /// kicad-connectivity rebuilds or updates itself against the board that stores it.
    /// While `f` runs the board has no connectivity (FlashLayer answers as for "no board").
    pub fn with_connectivity_mut<T: ConnectivityHook, R>(
            &mut self, f: impl FnOnce( &mut T, &Board ) -> R ) -> Option<R>;
    pub fn set_drc_engine( &mut self, engine: Option<Arc<dyn DrcEngineHook>> );
}
```

`DrcConstraintT` (the `DRC_CONSTRAINT_T` enum, third parameter of `GetEffectiveShape`) and the
plain-data `DrcConstraint { type_, value: MinOptMax<i32>, disallow_flags, zone_connection, name,
implicit_min, .. }` are defined in `kicad-pcb::drc_hook` and re-exported by `kicad-drc`.

### 2.7 Polymorphic access

`ItemRef<'a>` is the closed enumeration of the instantiable board item classes; it is what
`switch( item->Type() )` followed by `static_cast` becomes.

```rust
#[derive( Clone, Copy )]
pub enum ItemRef<'a> {
    Footprint( &'a Footprint ),             // PCB_FOOTPRINT_T
    Pad( &'a Pad ),                         // PCB_PAD_T
    Shape( &'a PcbShape ),                  // PCB_SHAPE_T
    ReferenceImage( &'a PcbReferenceImage ),// PCB_REFERENCE_IMAGE_T
    Field( &'a PcbField ),                  // PCB_FIELD_T
    Generator( &'a dyn Generator ),         // PCB_GENERATOR_T (tuning pattern, via stack, via stitch)
    Text( &'a PcbText ),                    // PCB_TEXT_T
    Textbox( &'a PcbTextbox ),              // PCB_TEXTBOX_T
    Table( &'a PcbTable ),                  // PCB_TABLE_T
    Tablecell( &'a PcbTablecell ),          // PCB_TABLECELL_T
    Track( &'a PcbTrack ),                  // PCB_TRACE_T
    Via( &'a PcbVia ),                      // PCB_VIA_T
    Arc( &'a PcbArc ),                      // PCB_ARC_T
    Marker( &'a PcbMarker ),                // PCB_MARKER_T
    Barcode( &'a PcbBarcode ),              // PCB_BARCODE_T
    DimAligned( &'a PcbDimAligned ),        // PCB_DIM_ALIGNED_T
    DimLeader( &'a PcbDimLeader ),          // PCB_DIM_LEADER_T
    DimCenter( &'a PcbDimCenter ),          // PCB_DIM_CENTER_T
    DimRadial( &'a PcbDimRadial ),          // PCB_DIM_RADIAL_T
    DimOrthogonal( &'a PcbDimOrthogonal ),  // PCB_DIM_ORTHOGONAL_T
    Target( &'a PcbTarget ),                // PCB_TARGET_T
    Zone( &'a Zone ),                       // PCB_ZONE_T
    Group( &'a PcbGroup ),                  // PCB_GROUP_T
    BoardOutline( &'a PcbBoardOutline ),    // PCB_BOARD_OUTLINE_T
    Point( &'a PcbPoint ),                  // PCB_POINT_T
    Constraint( &'a PcbConstraint ),        // PCB_CONSTRAINT_T
    GridItem( &'a PcbGridItem ),            // PCB_GRID_ITEM_T
    DrillChart( &'a PcbDrillChart ),        // PCB_DRILL_CHART_T
    DrillMap( &'a PcbDrillMap ),            // PCB_DRILL_MAP_T
    Body3d( &'a Pcb3dBody ),                // PCB_3D_BODY_T
    StackupTable( &'a PcbStackupTable ),    // PCB_STACKUP_TABLE_T
}
// `ItemMut<'a>` is the same enum with `&'a mut`.
```

Downcast helpers are inherent methods of the trait objects (`impl dyn BoardItem + '_ { .. }`), so
they are available on every `&dyn BoardItem`, `&dyn Track`, ...:

| C++ | Rust | Matches |
|---|---|---|
| `dynamic_cast<PAD*>` / `Type() == PCB_PAD_T` | `as_pad()`, `as_zone()`, `as_footprint()`, `as_via()`, `as_arc()`, `as_trace()`, `as_pcb_shape()`, `as_pcb_text()`, `as_field()`, `as_pcb_textbox()`, `as_tablecell()`, `as_barcode()`, `as_marker()`, ... -> `Option<&Concrete>` | exactly that `KICAD_T` |
| `dynamic_cast<PCB_TRACK*>` | `as_track()` -> `Option<&dyn Track>` | trace, arc, via |
| `dynamic_cast<PCB_SHAPE*>` | `as_shape()` -> `Option<&dyn ShapeItem>` | shape, textbox, table cell |
| `dynamic_cast<PCB_TEXT*>` | `as_text()` -> `Option<&dyn TextItem>` | text, field, dimensions |
| `dynamic_cast<PCB_TEXTBOX*>` | `as_textbox()` -> `Option<&dyn TextboxItem>` | textbox, table cell |
| `dynamic_cast<PCB_DIMENSION_BASE*>`, `BaseType( t ) == PCB_DIMENSION_T` | `as_dimension()` -> `Option<&dyn Dimension>` | five dimension classes |
| `dynamic_cast<PCB_TABLE*>`, `BaseType( t ) == PCB_TABLE_T` | `as_table()` -> `Option<&dyn TableItem>` | table, drill chart, stackup table |
| `dynamic_cast<PCB_GROUP*>` / `EDA_GROUP*` | `as_group()` -> `Option<&dyn GroupItem>` | group, generators |
| `dynamic_cast<PCB_GENERATOR*>` | `as_generator()` -> `Option<&dyn Generator>` | generators |
| `dynamic_cast<BOARD_CONNECTED_ITEM*>` | `as_connected()` -> `Option<&dyn BoardConnectedItem>` | pad, trace, arc, via, zone, shape family (`ClassOf`, `board_connected_item.h:60`) |
| `dynamic_cast<EDA_TEXT*>` / `EDA_SHAPE*` | `as_eda_text()` / `as_eda_shape()` | by mixin |
| `dynamic_cast<BOARD_ITEM_CONTAINER*>` | `as_container()` -> `Option<&dyn BoardItemContainer>` | footprint, tables |

Every helper has a `_mut` twin. They are implemented once, on top of `as_item_ref()` /
`as_item_mut()`, which each concrete type gets from the plumbing macro. Upcasts (`&dyn Track` to
`&dyn BoardItem`, `Box<dyn Track>` to `Box<dyn BoardItem>`) are the built-in trait-object
upcasting coercions (stable since Rust 1.86; see open question Q1).

### 2.8 What a concrete type looks like

```rust
/// Port of PCB_VIA (pcbnew/pcb_track.h).
#[derive( Clone, Debug )]
pub struct PcbVia {
    pub track: PcbTrackBase,            // PCB_TRACK members (start, end, width, mask) + connected base
    pub( crate ) via_type: ViaType,     // m_viaType
    pub( crate ) padstack: Padstack,    // m_padStack
    pub( crate ) is_free: bool,         // m_isFree
    pub( crate ) zone_layer_overrides: [ZoneLayerOverride; MAX_CU_LAYERS],  // m_zoneLayerOverrides
}

board_item_plumbing! { PcbVia, ItemRef::Via, class = "PCB_VIA", base = track.conn.item, conn = track.conn }
//  generates: EdaItem::{eda, eda_mut, as_any, as_any_mut, into_any, class_name},
//             BoardItem::{base, base_mut, as_item_ref, as_item_mut, clone_item, into_board_item},
//             BoardConnectedItem::{conn, conn_mut}, the as_connected() overrides.

impl_virtuals! { PcbVia =>
    EdaItem { is_type = pcb_via::is_type, position = pcb_via::position, set_position = pcb_via::set_position }
    BoardItem {
        layer = pcb_via::layer, set_layer = pcb_via::set_layer,
        layer_set = pcb_via::layer_set, set_layer_set = pcb_via::set_layer_set,
        is_on_layer = pcb_via::is_on_layer, is_on_copper_layer = pcb_via::is_on_copper_layer,
        has_hole = pcb_via::has_hole, has_drilled_hole = pcb_via::has_drilled_hole,
        is_tented = pcb_via::is_tented, fits_enabled_layers = pcb_via::fits_enabled_layers,
        bounding_box = pcb_via::bounding_box, hit_test = pcb_via::hit_test,
        effective_shape = pcb_via::effective_shape, effective_hole_shape = pcb_via::effective_hole_shape,
        transform_shape_to_polygon = pcb_track::transform_shape_to_polygon,   // inherited from PCB_TRACK
        flip = pcb_via::flip, rotate = pcb_track::rotate, mirror = pcb_track::mirror, move_ = pcb_track::move_,
        item_description = pcb_via::item_description, eq_item = pcb_via::eq_item, similarity = pcb_via::similarity,
    }
    Track { track = self.track, width = pcb_via::width, set_width = pcb_via::set_width,
            width_constraint = pcb_via::width_constraint }
}

/// Port of PCB_VIA's own (non-virtual) interface: inherent methods, same names as C++.
impl PcbVia {
    pub fn new() -> PcbVia;                                    // PCB_VIA( nullptr )
    pub fn via_type( &self ) -> ViaType;
    pub fn set_via_type( &mut self, env: EnvCtx<'_>, via_type: ViaType );   // calls SanitizeLayers
    pub fn padstack( &self ) -> &Padstack;
    pub fn padstack_mut( &mut self ) -> &mut Padstack;
    pub fn width_on( &self, layer: PcbLayerId ) -> i32;        // GetWidth( PCB_LAYER_ID )
    pub fn drill( &self ) -> i32;
    pub fn drill_value( &self, ctx: BoardCtx<'_> ) -> i32;     // consults the effective netclass
    pub fn flash_layer( &self, ctx: BoardCtx<'_>, layer: PcbLayerId ) -> bool;
    pub fn layer_pair( &self ) -> ( PcbLayerId, PcbLayerId );  // LayerPair( &top, &bottom )
    pub fn top_layer( &self ) -> PcbLayerId;
    pub fn bottom_layer( &self ) -> PcbLayerId;
    pub fn is_micro_via( &self ) -> bool;   pub fn is_blind_via( &self ) -> bool;   pub fn is_buried_via( &self ) -> bool;
    pub fn zone_layer_override( &self, layer: PcbLayerId ) -> ZoneLayerOverride;
    pub fn set_zone_layer_override( &mut self, layer: PcbLayerId, value: ZoneLayerOverride );
    // ...
}
```

Typical consumer code, ported line by line (DRC `forEachGeometryItem`,
`drc_test_provider.cpp:173-193`):

```rust
for item in brd.tracks() {                                            // for( PCB_TRACK* item : brd->Tracks() )
    if ( item.layer_set() & a_layers ).any() {                        //   if( (item->GetLayerSet() & aLayers).any() )
        if type_mask[KicadT::PcbTraceT] && item.type_() == KicadT::PcbTraceT {
            a_func( item );                                           //       aFunc( item );
            n += 1;
        } else if type_mask[KicadT::PcbViaT] && item.type_() == KicadT::PcbViaT {
            a_func( item );
            n += 1;
        } // ...
    }
}
```

and `CONNECTIVITY_DATA::IsConnectedOnLayer` (`connectivity_data.cpp:454-469`):

```rust
let connected_item: &dyn BoardItem = connected.parent( board );       // BOARD_ITEM* connectedItem = connected->Parent();
if same_item( connected_item, a_item ) { continue; }                  // if( connectedItem == aItem ) continue;
if !parent_footprint.is_null()                                        // if( parentFootprint && connectedItem
        && connected_item.parent_footprint_ptr() == parent_footprint  //         && connectedItem->GetParentFootprint() == parentFootprint )
{ continue; }
if let ( Some( this_pad ), Some( other_pad ) ) = ( a_item.as_pad(), connected_item.as_pad() ) {
    // const PAD* thisPad = static_cast<const PAD*>( aItem ); ...
}
```

### 2.9 Mutation under the borrow checker

The rule of the model: **`&Board` to read, `&mut Board` to change, never both.** Five mechanisms
cover every pattern found in the consumers.

**(1) Structural changes go through the board** (port of `BOARD::Add` / `Remove`, same type switch,
same container choice per `KICAD_T`, same `ADD_MODE` semantics):

```rust
impl Board {
    /// BOARD::Add. Chooses the container from `item.type_()`, sets the parent link, binds the
    /// item and its children in the index, bumps the time stamp, appends `BoardChange::Added`.
    /// Gives the item back if the board refuses it (track on a non-copper layer, table cell,
    /// duplicate net): the C++ wxFAIL / early-return branches.
    pub fn add( &mut self, item: Box<dyn BoardItem>, mode: AddMode ) -> Result<ItemPtr, Box<dyn BoardItem>>;
    /// BOARD::Remove. Detaches from its parent group, unbinds (children too), sets
    /// STRUCT_DELETED, bumps the time stamp, appends `BoardChange::Removed`. Returns ownership:
    /// in C++ the caller owns the item after Remove().
    pub fn remove( &mut self, ptr: ItemPtr, mode: RemoveMode ) -> Option<Box<dyn BoardItem>>;
    pub fn delete( &mut self, ptr: ItemPtr );                         // BOARD_ITEM_CONTAINER::Delete
    pub fn remove_all( &mut self, types: &[KicadT] ) -> Vec<Box<dyn BoardItem>>;
    /// Children of an attached footprint (FOOTPRINT::Add / Remove on a board member).
    pub fn footprint_add( &mut self, fp: ItemPtr, item: Box<dyn BoardItem>, mode: AddMode ) -> Result<ItemPtr, Box<dyn BoardItem>>;
    pub fn footprint_remove( &mut self, fp: ItemPtr, child: ItemPtr ) -> Option<Box<dyn BoardItem>>;
    /// Typed convenience for clients (autorouter): no boxing or downcasting at the call site.
    pub fn add_track( &mut self, track: PcbTrack, mode: AddMode ) -> ItemPtr;
    pub fn add_arc( &mut self, arc: PcbArc, mode: AddMode ) -> ItemPtr;
    pub fn add_via( &mut self, via: PcbVia, mode: AddMode ) -> ItemPtr;
    pub fn add_zone( &mut self, zone: Zone ) -> ItemPtr;
    pub fn add_footprint( &mut self, fp: Footprint, mode: AddMode ) -> ItemPtr;
}
```

`Footprint::add` / `remove` exist too (the parser builds detached footprints with them); on an
attached footprint they must be reached through `Board::footprint_add` or `Board::modify`, which
keep the index in step.

**(2) In-place edits.**

```rust
impl Board {
    pub fn item( &self, ptr: ItemPtr ) -> Option<&dyn BoardItem>;
    /// Marks the item changed (time stamp, journal, item caches are the item's own business)
    /// and hands out `&mut`. Contract: no structural change (children added/removed) and no
    /// uuid change through this reference; use `modify` for those.
    pub fn item_mut( &mut self, ptr: ItemPtr ) -> Option<&mut dyn BoardItem>;
    /// Port of "commit.Modify( item ); ...edit...; commit.Push()" for one item: runs `f` with the
    /// item and the environment, then re-binds the item's subtree in the index (new children get
    /// an ItemPtr, removed ones lose it), re-issues FpLinks if needed, bumps the time stamp and
    /// appends `BoardChange::Changed`.
    pub fn modify<R>( &mut self, ptr: ItemPtr, f: impl FnOnce( &mut dyn BoardItem, EnvCtx<'_> ) -> R ) -> Option<R>;
    // typed twins: track_mut, via_mut, zone_mut, footprint_mut, pad_mut, modify_footprint, modify_zone, ...
    pub fn rebind_item_uuid( &mut self, ptr: ItemPtr, new_id: Kiid );   // BOARD::RebindItemUuid / SetUuid
}
```

**(3) Whole-board loops that mutate items and read settings** use the split borrow:

```rust
let ( mut items, env ) = board.split_mut();
for fp in items.footprints_mut() {                 // for( FOOTPRINT* fp : m_board->Footprints() )
    fp.fix_up_pads_for_board( env );               //     fp->FixUpPadsForBoard( m_board );
}
```

`ItemsMut<'_>` offers `footprints_mut()`, `tracks_mut()`, `zones_mut()`, `drawings_mut()`,
`item_mut( ptr )`, plus the shared iterators; it cannot add or remove (that needs the index).

**(4) Compute, then apply** - for algorithms that read many items to decide what to write. The
C++ already has this shape wherever it uses worker threads. The Rust port makes the hand-over
explicit:

```rust
// zone filler, one pass (zone_filler.cpp: fill, then commit):
let fills: Vec<( ItemPtr, PcbLayerId, ShapePolySet )> = compute_fills( &board, &zones_to_fill );   // &Board
for ( zone, layer, polys ) in fills {                                                              // &mut Board
    let z = board.zone_mut( zone ).unwrap();
    z.set_filled_polys_list( layer, polys );       // zone->SetFilledPolysList( layer, fillPolys );
    z.set_fill_flag( layer, true );                // zone->SetFillFlag( layer, true );
}
```

Net propagation in connectivity (`propagateConnections`) collects `( ItemPtr, net code )` pairs and
applies them; DRC collects markers and adds them at the end (it already does: `ReportViolation`
goes through a handler).

**(5) `BoardCommit`** (port of the non-GUI core of `BOARD_COMMIT` / `COMMIT`,
`pcbnew/board_commit.cpp`, `common/commit.cpp`) for router commits and external clients:

```rust
pub struct BoardCommit { /* staged Add / Remove / Modify entries with their undo copies */ }
impl BoardCommit {
    pub fn add( &mut self, item: Box<dyn BoardItem> ) -> StagedId;            // COMMIT::Add
    pub fn remove( &mut self, ptr: ItemPtr );                                 // COMMIT::Remove
    pub fn modify( &mut self, board: &Board, ptr: ItemPtr );                  // COMMIT::Modify: clone_item() copy
    pub fn modified( &mut self, ptr: ItemPtr, copy: Box<dyn BoardItem> );     // COMMIT::Modified
    /// BOARD_COMMIT::Push without the view/tool plumbing: applies the staged changes and returns
    /// what happened so that connectivity / ratsnest / DRC can update incrementally.
    pub fn push( self, board: &mut Board ) -> CommitResult;   // { added: Vec<ItemPtr>, removed: Vec<( ItemPtr, Kiid )>, changed: Vec<ItemPtr>, undo: UndoEntry }
    pub fn revert( self, board: &mut Board );                                 // BOARD_COMMIT::Revert
}
```

**Change journal** (replaces `BOARD_LISTENER` and the direct `m_connectivity->Add/Remove/Update`
calls inside `BOARD::Add/Remove`, which would be an upward call):

```rust
#[derive( Clone, Debug )]
pub struct BoardChange { pub kind: ChangeKind, pub ptr: ItemPtr, pub uuid: Kiid, pub type_: KicadT }
pub enum ChangeKind { Added, Removed, Changed }
impl Board {
    pub fn take_changes( &mut self ) -> Vec<BoardChange>;     // drained by connectivity / wasm glue
    pub fn set_change_tracking( &mut self, on: bool );        // off while loading (aSkipConnectivity)
}
```

### 2.10 Derived caches and invalidation

| C++ | Rust | Invalidated by |
|---|---|---|
| `EDA_ITEM::m_flags` | `ItemFlags( AtomicU32 )`, `Relaxed` ordering, methods take `&self` | the algorithm that set them |
| `PAD::m_drawCache` (effective shapes, hole shape, bbox), `m_polyDirty[2]`, `m_shapesDirty`, `m_maskShapes` | `OnceLock<PadShapeCache>`, `[OnceLock<..>; 2]`, `Mutex<BTreeMap<PcbLayerId, ( i32, Arc<ShapeCompound> )>>` | `Pad::set_dirty( &mut self )` (called by the same setters as in C++ and by `on_footprint_transformed`) |
| `FOOTPRINT::m_geometry_cache` (bbox, text-excluded bbox, hull, each with a board time stamp), `m_courtyard_cache` (+ hashes), `m_netTieCache` | `Mutex<FootprintGeometryCache>` (time-stamp validated exactly like C++), `OnceLock<FootprintCourtyardCache>`, `BTreeMap<ItemPtr, BTreeSet<i32>>` | time stamp comparison; `invalidate_geometry_caches()`; `build_courtyard_caches( &mut self, .. )` / `build_net_tie_cache( &mut self )` are explicit preparation steps as in C++ |
| `ZONE::m_bboxCache` + `m_bboxCacheTimeStamp`, `BOARD::m_ZoneBBoxCache` | `Mutex<( Box2I, i32 )>` in the zone; the board-level map is dropped | time stamp |
| `ZONE::m_FilledPolysList` (`shared_ptr<SHAPE_POLY_SET>` per layer), triangulation inside | `BTreeMap<PcbLayerId, Arc<ShapePolySet>>`; `cache_triangulation( &mut self, layer )` uses `Arc::make_mut` | filler writes |
| `EDA_TEXT::m_render_cache`, `m_bbox_cache`, `m_shown_text` | `Option<Box<EdaTextRenderCache>>` (file data, not derived: must survive, R7 of the I/O analysis), `Mutex<BTreeMap<i32, BboxCacheEntry>>`, plain `String` | the `EDA_TEXT` setters (`ClearRenderCache`, `ClearBoundingBoxCache`) |
| `EDA_SHAPE::m_hatchingCache` + `m_hatchingDirty` | `OnceLock<EdaShapeHatchCache>` | shape setters |
| `PCB_TEXT::m_knockout_cache`, `PCB_BARCODE::m_cache`, `PCB_DRILL_MAP::m_outlineCache` | `Mutex<Option<..>>` keyed by the same cache key as C++ | key mismatch |
| `BOARD::m_timeStamp` | `time_stamp: i32`, incremented by every `&mut` structural method and by `Board::increment_time_stamp()` | - |
| `BOARD::m_maxClearanceValue` | `OnceLock<i32>` in `BoardCaches`, replaced on time stamp increment | time stamp |
| `BOARD::m_CopperItemRTreeCache`, `m_DRCZones`, `m_Intersects*Cache`, `m_ZoneIsolatedIslandsMap`, `m_SolderMaskBridges`, ... (public DRC scratch on the board, cleared by `IncrementTimeStamp`) | moved out: `kicad_drc::DrcBoardCaches`, created per run, keyed by `ItemPtr` (pointer-keyed, iterated maps become `BTreeMap<ItemPtr, ..>`: deterministic) | dropped with the run |
| connectivity data | owned by `kicad-connectivity`, stored in the board hook slot, updated from the change journal | journal |

All cache cells are `Send + Sync`, so `Board: Send + Sync` and an optional `parallel` feature can
share `&Board` across threads later. `Clone` of an item clones its caches' current content
(`OnceLock<T>: Clone`), as the C++ copy constructors do or do not; where C++ resets a cache on
copy the `Clone` impl does the same.

Preparation phases (need `&mut Board`, run before a read-only pass, all exist in C++ as explicit
calls): `Board::cache_triangulation()`, `Footprint::build_courtyard_caches`,
`Footprint::build_net_tie_cache`, `Zone::cache_bounding_box`, `Board::build_list_of_nets`,
`Board::synchronize_nets_and_net_classes`.

### 2.11 Where this approach hurts, and the mitigation for each

| Pain | Why it is inherent | Mitigation | Residual cost |
|---|---|---|---|
| **Parent pointers** | Safe Rust has no back pointer from a boxed child to its owner. | Geometry: `Arc<FpLink>` snapshot, maintained by the hook C++ already calls (2.5). Everything else: `parent_footprint( ctx )` through `ItemPtr`. | Invariant I-FP must hold (private fields + checker). Non-geometric parent data needs `ctx`. |
| **`GetBoard()` inside item methods** (143 in model files) | Same. | `BoardCtx` / `EnvCtx` argument by rule C1; null-board defaults centralised in `EnvCtx`. | Every ctx-taking call site has one more argument; default arguments are gone anyway. |
| **`&mut item` + board data at once** | Exclusive borrow of the board. | `EnvCtx` + `split_mut()`; `modify( ptr, closure )`; compute-then-apply. | Mutators cannot look at *other items*; the few that do (`ZONE` priority resolution, group edits) are `Board` methods. |
| **Shared mutable state** (`shared_ptr<NETCLASS>`, `shared_ptr<SHAPE_POLY_SET>`, `shared_ptr<PCB_SHAPE>` primitives, `shared_ptr<CONNECTIVITY_DATA>`, `shared_ptr<DRC_ENGINE>`) | `Arc<T>` is immutable. | Netclasses are rebuilt, not mutated, by `synchronize_nets_and_net_classes`; fills and primitives use `Arc::make_mut` (copy on write; uniquely owned in practice); connectivity uses take-and-put-back; the DRC engine is immutable after `init_engine` except for `Mutex` caches. | A netclass edit is visible to items only after the next synchronize (in C++ too: `NETINFO_ITEM::m_netClass` is assigned there). |
| **Inherited overrides** (M4) | Traits have no intermediate-base inheritance. | Generic free functions + `impl_virtuals!` table per type. | About 30-60 mechanical lines per concrete type in the six multi-level chains. A wrong table entry is a silent behaviour bug: the table is reviewed against the header's `override` list and exercised by the conformance test of WP MODEL-21 (every type x every virtual, compared with the oracle where observable). |
| **Template methods** (non-virtual base method calling a virtual) | A method on a base *struct* would dispatch statically. | Rule M3: such methods are provided trait methods or generic free functions over `T: Trait + ?Sized`; base structs have only field-level inherent methods. | Reviewer checklist item; the "no concrete struct inside another" rule (M2) removes the other half of the problem. |
| **`Clone`** | `Box<dyn BoardItem>` is not `Clone`. | `clone_item()` (C++ `Clone()`), generated by the plumbing macro from the concrete `Clone`. `BoardItemBase::clone` resets `ptr` and `order_key` ("clones start detached from any board index", `board_item.h:114`) and copies flags by value. | `Board` itself is not `Clone` (C++: deleted copy constructor); `Board::deep_copy()` is offered as an explicit, hook-less copy for snapshots. |
| **`PartialEq`** | Not derivable for trait objects, and the C++ `operator==` overloads are hand-written and ignore fields. | `eq_item( &self, other: &dyn BoardItem ) -> bool` per type, ported statement by statement; `PartialEq` is *not* derived on item structs, so nobody compares with different semantics by accident. | - |
| **serde** | No derive for `dyn`. | Not needed: the file format has its own writer; wasm uses hand-written DTOs (section 6). Settings structs (`BoardDesignSettings`, `Netclass`) are plain data and may derive serde for the `.kicad_pro` JSON. | - |
| **Object safety** | No generic methods, no `Self` returns, no `impl Trait` arguments in the item traits. | Callbacks are `&mut dyn FnMut(..)`; `clone_item` returns a box; units come as `&dyn UnitsProvider`; iteration over children is `run_on_children( &mut dyn FnMut( &dyn BoardItem ) )`. | Slightly noisier closures. |
| **Downcast cost and exhaustiveness** | `dyn` has no `match`. | `ItemRef` enum from one virtual call; `match` on it is exhaustive, so adding an item type breaks the build everywhere a decision is needed. | One indirect call per downcast (C++ `Type()` is a field read: `type_()` is a provided method reading `eda().struct_type`, one accessor call). |
| **Dynamic dispatch and heap** | Every item is a `Box`, every virtual an indirect call. | Same as C++ (`new`, vtable). Hot typed loops take the concrete type after one `as_item_ref()`. | No regression against C++; less cache-friendly than an arena. |
| **Two kinds of non-owning reference** (`ItemPtr`, `&'a dyn`) | Stored pointers must survive mutation or not. | One rule: pass-scoped structures may borrow, long-lived ones use `ItemPtr`. | A consumer that picks the borrow cannot be kept across an edit (compile error, not a bug). |
| **Duplicate `Kiid`s, pointer-order tie-breaks** | Files may contain duplicates; C++ falls back to addresses. | Identity is `ItemPtr`, not `Kiid`; tie-breaks use `ItemPtr` order. | Documented deviation, only reachable with corrupt files. |
| **Partial moves / self-borrows inside a footprint** (iterate pads while mutating the footprint's fields) | One `&mut Footprint`. | Children are separate fields: Rust splits borrows by field (`for pad in fp.pads.iter_mut() { .. fp.transform .. }` is fine inside the module). | Outside the module use the accessor pair `fp.split_children_mut()`. |

---------------------------------------------------------------------------------------------------

## 3. Type definitions

Conventions used in all code blocks:

* Field name = C++ member name without `m_`, snake_case; the C++ member is repeated in a comment
  only where the mapping is not obvious. `wxString` -> `String`; `std::optional<T>` -> `Option<T>`;
  `std::map` / `std::set` -> `BTreeMap` / `BTreeSet` (iteration order reaches output);
  `std::unordered_*` that is only looked up -> `HashMap` / `HashSet`; `std::shared_ptr<T>` ->
  `Arc<T>`; `int` -> `i32`; `long long` -> `i64`; `double` -> `f64`.
* Foundation types: `Vector2I`, `Vector2D`, `Vector3D`, `Box2I`, `EdaAngle`, `EdaOrientation`,
  `Seg`, `ShapeLineChain`, `ShapePolySet`, `ShapeSegment`, `ShapeCompound`, `ShapeArc`,
  `dyn Shape`, `TransformTrs`, `FlipDirection`, `ErrorLoc` from `kicad-geom`; `Kiid`, `KiidPath`,
  `PcbLayerId`, `Lset`, `Lseq`, `LibId`, `KicadT`, `Flashing`, `Color4D`, `StrokeParams`,
  `LineStyle`, `TextAttributes`, `Severity`, `EdaUnits`, `MinOptMax<T>`, `Hash128` from
  `kicad-core`; `Sexpr` from `kicad-sexpr`; `Font` handle from `kicad-font`.
* Visibility: fields with invariants are `pub( crate )` with C++-named accessors; plain data
  fields are `pub`. All item structs derive `Clone, Debug`; none derives `PartialEq` (2.11).
* One module per C++ file (`pad.rs` ports `pad.h` + `pad.cpp`), in `crates/kicad-pcb/src/`.

### 3.1 Kernel (`eda_item.rs`, `board_item.rs`, `board_connected_item.rs`, `item_ptr.rs`)

```rust
/// EDA_ITEM_FLAGS m_flags. Interior mutability: algorithms mark items through `&self`.
#[derive( Debug, Default )]
pub struct ItemFlags( AtomicU32 );
impl Clone for ItemFlags { /* copies the current value */ }
impl ItemFlags {
    pub fn get( &self ) -> u32;                       // GetFlags
    pub fn set( &self, mask: u32 );                   // SetFlags
    pub fn clear( &self, mask: u32 );                 // ClearFlags
    pub fn has( &self, flag: u32 ) -> bool;           // HasFlag
}
// include/eda_item_flags.h, same bit values:
pub const IS_CHANGED: u32 = 1 << 0;   pub const IS_LINKED: u32 = 1 << 1;   pub const IN_EDIT: u32 = 1 << 2;
pub const IS_MOVING: u32 = 1 << 3;    pub const IS_NEW: u32 = 1 << 4;      pub const IS_BROKEN: u32 = 1 << 5;
pub const IS_DELETED: u32 = 1 << 7;   pub const STARTPOINT: u32 = 1 << 9;  pub const ENDPOINT: u32 = 1 << 10;
pub const SELECTED: u32 = 1 << 11;    pub const SELECTED_BY_DRAG: u32 = 1 << 12;
pub const STRUCT_DELETED: u32 = 1 << 13;   pub const CANDIDATE: u32 = 1 << 14;   pub const SKIP_STRUCT: u32 = 1 << 15;
pub const IS_PASTED: u32 = 1 << 17;   pub const COURTYARD_CONFLICT: u32 = 1 << 19;
pub const MALFORMED_F_COURTYARD: u32 = 1 << 20;   pub const MALFORMED_B_COURTYARD: u32 = 1 << 21;
pub const MALFORMED_COURTYARDS: u32 = MALFORMED_F_COURTYARD | MALFORMED_B_COURTYARD;
pub const ROUTER_TRANSIENT: u32 = 1 << 22;   pub const CONNECTIVITY_CANDIDATE: u32 = 1 << 23;
pub const HOLE_PROXY: u32 = 1 << 24;  pub const UR_TRANSIENT: u32 = 1 << 28;

/// Data members of EDA_ITEM (include/eda_item.h:598-622).
#[derive( Clone, Debug )]
pub struct EdaItemBase {
    pub( crate ) uuid: Kiid,                 // const KIID m_Uuid (changed only via the board, 2.9)
    pub( crate ) struct_type: KicadT,        // m_structType
    pub flags: ItemFlags,                    // m_flags
    pub( crate ) parent: ParentLink,         // m_parent
    pub( crate ) group: Option<ItemPtr>,     // EDA_GROUP* m_group (no ownership)
    pub custom_properties: BTreeMap<String, String>,   // m_customProperties, written as (custom_property k v)
}
// Dropped: m_rolloverPos, m_isRollover, m_forceVisible, m_netHighlighted (GUI),
// m_customPropsStorageCache (property-grid cache), KIGFX::VIEW_ITEM base, SERIALIZABLE base.

/// Data members of BOARD_ITEM (include/board_item.h:620-629).
#[derive( Debug )]
pub struct BoardItemBase {
    pub eda: EdaItemBase,
    pub( crate ) layer: PcbLayerId,          // m_layer (default F_Cu)
    pub( crate ) is_knockout: bool,          // m_isKnockout
    pub( crate ) is_locked: bool,            // m_isLocked
    pub( crate ) ptr: ItemPtr,               // replaces m_owningIndex and the object address
    pub( crate ) order_key: OrderKey,        // position key in the owning ItemDeque (2.3)
}
impl Clone for BoardItemBase { /* copies data and parent link; ptr = NULL, order_key = 0 */ }
impl BoardItemBase {
    pub fn new( type_: KicadT, layer: PcbLayerId ) -> Self;       // BOARD_ITEM( nullptr, idtype, aLayer ), fresh Kiid
    pub fn with_uuid( type_: KicadT, layer: PcbLayerId, uuid: Kiid ) -> Self;   // deterministic ids (parser)
    pub fn fp( &self ) -> Option<&FpLink>;                        // enclosing footprint snapshot
    pub fn fp_transform( &self ) -> Option<&TransformTrs>;        // GetParentFootprint()->GetTransform()
    pub fn set_uuid_direct( &mut self, uuid: Kiid );              // SetUuidDirect: detached / load-time only
}

/// Data members of BOARD_CONNECTED_ITEM (pcbnew/board_connected_item.h:238-250).
#[derive( Clone, Debug )]
pub struct BoardConnectedItemBase {
    pub item: BoardItemBase,
    /// NETINFO_ITEM* m_netinfo as its net code: >0 board net, 0 = NETINFO_LIST::UNCONNECTED and
    /// the orphaned singleton (the constructor default), -1 = m_netinfo == nullptr.
    pub( crate ) net_code: i32,
    pub teardrop_params: TeardropParameters, // m_teardropParams
    pub local_ratsnest_visible: bool,        // m_localRatsnestVisible (default true)
}

/// TEARDROP_PARAMETERS (pcbnew/teardrop/teardrop_parameters.h). Defaults in comments.
#[derive( Clone, Debug, PartialEq )]
pub struct TeardropParameters {
    pub td_max_len: i32,                     // mmToIU( 1.0 )
    pub td_max_width: i32,                   // mmToIU( 2.0 )
    pub best_length_ratio: f64,              // 0.5
    pub best_width_ratio: f64,               // 1.0
    pub widthto_size_filter_ratio: f64,      // 0.9   (m_WidthtoSizeFilterRatio)
    pub curved_edges: bool,                  // false
    pub enabled: bool,                       // false
    pub allow_use_two_tracks: bool,          // true
    pub td_on_pads_in_zones: bool,           // false
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum AddMode { Insert, Append, BulkAppend, BulkInsert }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum RemoveMode { Normal, Bulk }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum RecurseMode { Recurse, NoRecurse }
/// ZONE_LAYER_OVERRIDE (include/board_item.h:73).
#[derive( Clone, Copy, Debug, PartialEq, Eq, Default )]
pub enum ZoneLayerOverride { #[default] ZloNone, ZloForceFlashed, ZloForceNoZoneConnection }
```

### 3.2 `Board` (`board.rs`)

```rust
/// Port of BOARD (pcbnew/board.h). Not `Clone` (C++ deletes the copy constructor); see `deep_copy`.
pub struct Board {
    pub( crate ) items: BoardItems,
    pub( crate ) index: BoardItemIndex,          // mutable BOARD_ITEM_INDEX m_itemIndex
    pub( crate ) env: BoardEnv,
    pub( crate ) hooks: BoardHooks,
    pub( crate ) time_stamp: i32,                // std::atomic<int> m_timeStamp (modification counter)
    pub( crate ) changes: Vec<BoardChange>,      // replaces m_listeners
    pub( crate ) track_changes: bool,
    pub( crate ) caches: BoardCaches,
}

/// The item containers, in the C++ declaration order (board.h:1898-1908).
pub struct BoardItems {
    pub( crate ) markers: ItemDeque<PcbMarker>,            // MARKERS m_markers (vector)
    pub( crate ) drawings: ItemDeque<dyn BoardItem>,       // DRAWINGS m_drawings
    pub( crate ) footprints: ItemDeque<Footprint>,         // FOOTPRINTS m_footprints
    pub( crate ) tracks: ItemDeque<dyn Track>,             // TRACKS m_tracks
    pub( crate ) groups: ItemDeque<PcbGroup>,              // GROUPS m_groups
    pub( crate ) constraints: ItemDeque<PcbConstraint>,    // CONSTRAINTS m_constraints
    pub( crate ) zones: ItemDeque<Zone>,                   // ZONES m_zones (vector)
    pub( crate ) generators: ItemDeque<dyn Generator>,     // GENERATORS m_generators
    pub( crate ) board_outline: Option<Box<PcbBoardOutline>>,   // PCB_BOARD_OUTLINE* m_boardOutline (derived)
    pub( crate ) points: ItemDeque<PcbPoint>,              // PCB_POINTS m_points
}

/// Everything in BOARD that is not an item. Borrowable next to `&mut` items (EnvCtx).
pub struct BoardEnv {
    pub board_use: BoardUse,                               // m_boardUse
    pub outlines_chaining_epsilon: i32,                    // m_outlinesChainingEpsilon (mmToIU( 0.01 ))
    pub file_name: String,                                 // m_fileName
    pub generator: String,                                 // m_generator (tag read from the file)
    pub file_format_version_at_load: i32,                  // m_fileFormatVersionAtLoad
    pub( crate ) layers: BTreeMap<i32, Layer>,             // std::map<int, LAYER> m_layers
    pub( crate ) design_settings: BoardDesignSettings,     // std::shared_ptr<BOARD_DESIGN_SETTINGS> m_designSettings
    pub( crate ) net_info: NetinfoList,                    // NETINFO_LIST m_NetInfo
    pub net_chain_colors: BTreeMap<String, Color4D>,       // m_netChainColors
    pub properties: BTreeMap<String, String>,              // m_properties (board text variables)
    pub paper: PageInfo,                                   // m_paper
    pub title_block: TitleBlock,                           // m_titles
    pub plot_options: PcbPlotParams,                       // m_plotOptions
    pub project: Option<Arc<ProjectData>>,                 // PROJECT* m_project (what the port needs of it)
    pub user_units: EdaUnits,                              // m_userUnits
    pub current_variant: String,                           // m_currentVariant
    pub variant_names: Vec<String>,                        // m_variantNames
    pub variant_descriptions: BTreeMap<String, String>,    // m_variantDescriptions
    pub legacy_teardrops: bool,                            // m_legacyTeardrops
    pub embedded_files: EmbeddedFiles,                     // EMBEDDED_FILES base class (+ BOARD::m_embedFonts,
                                                           //   which shadows EMBEDDED_FILES::m_embedFonts: one flag kept)
    pub component_class_manager: ComponentClassManager,    // m_componentClassManager
    pub drill_symbol_layers: Lset,                         // m_drillSymbolLayers (RefreshDrillSymbolLayers)
    pub drill_model_generation: u64,                       // m_drillModelGeneration
    pub board_outline_generation: u64,                     // m_boardOutlineGeneration
    // public legacy-load flags of BOARD:
    pub legacy_visible_layers: Lset,                       // m_LegacyVisibleLayers
    pub legacy_visible_items: Option<Vec<u64>>,            // GAL_SET m_LegacyVisibleItems as raw words (never written)
    pub legacy_design_settings_loaded: bool,               // m_LegacyDesignSettingsLoaded
    pub legacy_copper_edge_clearance_loaded: bool,         // m_LegacyCopperEdgeClearanceLoaded
    pub legacy_netclasses_loaded: bool,                    // m_LegacyNetclassesLoaded
}

pub struct BoardHooks {
    pub( crate ) connectivity: Option<Box<dyn ConnectivityHook>>,   // m_connectivity
    pub( crate ) drc_engine: Option<Arc<dyn DrcEngineHook>>,        // BOARD_DESIGN_SETTINGS::m_DRCEngine
}

pub struct BoardCaches {
    pub( crate ) max_clearance_value: OnceLock<i32>,       // mutable std::optional<int> m_maxClearanceValue
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum BoardUse { Normal, FpHolder }                     // BOARD_USE

/// LAYER_T / LAYER (board.h:241-290).
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum LayerT { Undefined = -1, Signal = 0, Power, Mixed, Jumper, Aux, Front, Back }
#[derive( Clone, Debug, PartialEq )]
pub struct Layer {
    pub name: String,          // m_name: canonical name
    pub user_name: String,     // m_userName
    pub type_: LayerT,         // m_type
    pub visible: bool,         // m_visible
    pub number: i32,           // m_number: PCB_LAYER_ID
    pub opposite: i32,         // m_opposite
}

/// The part of PROJECT (+ PROJECT_FILE, PROJECT_LOCAL_SETTINGS) the ported features read.
#[derive( Clone, Debug, Default )]
pub struct ProjectData {
    pub project_name: String,
    pub project_path: String,                              // only used for text variables / rule file names
    pub text_variables: BTreeMap<String, String>,          // PROJECT::GetTextVars
    pub prototype_zone_fill: bool,                         // PROJECT_LOCAL_SETTINGS::m_PrototypeZoneFill
    pub tuning_profiles: Option<Sexpr>,                    // tier 3: kept as parsed data until ported
}
```

Members of `BOARD` deliberately dropped: `m_highLight`, `m_highLightPrevious` (GUI),
`m_listeners` (journal instead), `m_historyLifetime`, `m_embeddedFilesDelegate` (design blocks),
`m_textVarAdapter` (expression adapter, built on demand by `kicad-drc`), `m_lengthDelayCalc`
(owned by the length-calculation port above the model), `m_drillSymbolPlacements`,
`m_drillSymbolCache` (+ mutex; drill-map rendering, tier 3), `m_CachesMutex` and all public DRC
caches (`m_Intersects*Cache`, `m_EnclosedByArea*`, `m_ItemFieldCache`, `m_LayerExpressionCache`,
`m_CopperZoneRTreeCache`, `m_CopperItemRTreeCache`, `m_ZoneBBoxCache`, `m_StackedMicroviaCache`,
`m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache`, `m_footprintCourtyardIndex`, `m_DRCZones`,
`m_DRCCopperZones`, `m_DRCCopperZonesByLayer`, `m_DRCMaxClearance`, `m_DRCMaxPhysicalClearance`,
`m_SolderMaskBridges`, `m_ZoneIsolatedIslandsMap`: all move to `kicad_drc::DrcBoardCaches`, see
`rust/docs/analysis/drc.md` section 3.8), `PROJECT::_ELEM` base.

### 3.3 Nets and netclasses (`netinfo.rs`, `netclass.rs`, `net_settings.rs`)

```rust
/// NETINFO_ITEM (pcbnew/netinfo.h). Not a BoardItem in the port (in C++ it derives from
/// BOARD_ITEM only to travel through BOARD::Add / commits); `Board::add_net` replaces that.
#[derive( Clone, Debug )]
pub struct NetinfoItem {
    pub( crate ) uuid: Kiid,                       // EDA_ITEM::m_Uuid
    pub( crate ) net_code: i32,                    // m_netCode
    pub( crate ) netname: String,                  // m_netname (full name)
    pub( crate ) short_netname: String,            // m_shortNetname
    pub( crate ) display_netname: String,          // m_displayNetname
    pub net_chain: String,                         // m_netChain
    pub terminal_pads: [Option<ItemPtr>; 2],       // PAD* m_terminalPads[2]
    pub terminal_pad_uuids: [Kiid; 2],             // m_terminalPadUuids[2]
    pub( crate ) net_class: Option<Arc<Netclass>>, // std::shared_ptr<NETCLASS> m_netClass (effective)
    pub is_current: bool,                          // m_isCurrent
}

/// NETINFO_LIST. Owns the nets; both views in the C++ order.
#[derive( Clone, Debug )]
pub struct NetinfoList {
    pub( crate ) nets: Vec<NetinfoItem>,                   // storage; slot index is private
    pub( crate ) net_names: BTreeMap<String, usize>,       // NETNAMES_MAP m_netNames (iteration = by name)
    pub( crate ) net_codes: BTreeMap<i32, usize>,          // NETCODES_MAP m_netCodes
    pub( crate ) new_net_code: i32,                        // m_newNetCode
    pub display_netnames_dirty: bool,                      // m_DisplayNetnamesDirty
}
impl NetinfoList {
    pub const UNCONNECTED: i32 = 0;
    pub const ORPHANED: i32 = -1;
    pub fn net_item( &self, code: i32 ) -> Option<&NetinfoItem>;            // GetNetItem( int )
    pub fn net_item_by_name( &self, name: &str ) -> Option<&NetinfoItem>;   // GetNetItem( wxString )
    pub fn net_count( &self ) -> usize;
    pub fn iter( &self ) -> impl Iterator<Item = &NetinfoItem>;             // begin()..end(): by NAME
    pub fn nets_by_netcode( &self ) -> impl Iterator<Item = &NetinfoItem>;  // NetsByNetcode()
    pub( crate ) fn append_net( &mut self, net: NetinfoItem ) -> Result<i32, i32>;  // AppendNet: Err( existing code ) on duplicate name
    pub( crate ) fn remove_net( &mut self, code: i32 ) -> Option<NetinfoItem>;
    pub( crate ) fn free_net_code( &mut self ) -> i32;                      // getFreeNetCode
    pub( crate ) fn build_list_of_nets( &mut self, .. );                    // buildListOfNets
}

/// NETCLASS (include/netclass.h).
#[derive( Clone, Debug )]
pub struct Netclass {
    pub( crate ) is_default: bool,
    /// m_constituents / m_constituentOwners: the netclasses an aggregate (effective) class is made of.
    pub( crate ) constituents: Vec<Arc<Netclass>>,
    pub( crate ) name: String,
    pub( crate ) priority: i32,
    pub( crate ) description: String,
    pub( crate ) clearance: Option<i32>,
    pub( crate ) track_width: Option<i32>,
    pub( crate ) via_dia: Option<i32>,
    pub( crate ) via_drill: Option<i32>,
    pub( crate ) uvia_dia: Option<i32>,
    pub( crate ) uvia_drill: Option<i32>,
    pub( crate ) diff_pair_width: Option<i32>,
    pub( crate ) diff_pair_gap: Option<i32>,
    pub( crate ) diff_pair_via_gap: Option<i32>,
    pub( crate ) wire_width: Option<i32>,          // schematic values are kept: the project file round-trips them
    pub( crate ) bus_width: Option<i32>,
    pub( crate ) schematic_color: Color4D,
    pub( crate ) line_style: Option<i32>,
    pub( crate ) pcb_color: Color4D,
    pub( crate ) tuning_profile: String,
    /// The fifteen `NETCLASS* m_xxxParent` members: which netclass supplied each value of an
    /// aggregate. `None` = this class itself (`ResetParents()` state). Stored by name; the DRC
    /// only uses the parent for its name (`GetClearanceParent()->GetName()`).
    pub( crate ) parents: NetclassParents,
}
#[derive( Clone, Debug, Default )]
pub struct NetclassParents {
    pub clearance: Option<String>, pub track_width: Option<String>, pub via_diameter: Option<String>,
    pub via_drill: Option<String>, pub uvia_dia: Option<String>, pub uvia_drill: Option<String>,
    pub diff_pair_width: Option<String>, pub diff_pair_gap: Option<String>, pub diff_pair_via_gap: Option<String>,
    pub pcb_color: Option<String>, pub wire_width: Option<String>, pub bus_width: Option<String>,
    pub schematic_color: Option<String>, pub line_style: Option<String>, pub tuning_profile: Option<String>,
}

/// NET_SETTINGS (include/project/net_settings.h).
#[derive( Clone, Debug )]
pub struct NetSettings {
    pub( crate ) default_net_class: Arc<Netclass>,                             // m_defaultNetClass
    pub( crate ) net_classes: BTreeMap<String, Arc<Netclass>>,                 // m_netClasses
    pub( crate ) net_class_label_assignments: BTreeMap<String, BTreeSet<String>>,   // m_netClassLabelAssignments
    /// m_netClassPatternAssignments: ( matcher, netclass name ) in assignment order.
    pub( crate ) net_class_pattern_assignments: Vec<( EdaCombinedMatcher, String )>,
    pub( crate ) net_class_chain_pattern_assignments: BTreeMap<NetChainSource, Vec<( EdaCombinedMatcher, String )>>,
    pub( crate ) composite_net_classes: BTreeMap<String, Arc<Netclass>>,       // m_compositeNetClasses
    pub( crate ) implicit_net_classes: BTreeMap<String, Arc<Netclass>>,        // m_impicitNetClasses
    /// m_effectiveNetclassCache: net name -> effective class. C++ fills it lazily from const
    /// methods; here `Mutex<BTreeMap<..>>` so that `effective_netclass( &self, name )` keeps its signature.
    pub( crate ) effective_netclass_cache: Mutex<BTreeMap<String, Arc<Netclass>>>,
    pub( crate ) netclass_bus_members: BTreeMap<String, BTreeSet<String>>,
    pub( crate ) net_color_assignments: BTreeMap<String, Color4D>,
    pub( crate ) net_chain_classes: BTreeMap<String, String>,
    pub( crate ) net_chain_net_classes: BTreeMap<String, String>,
}
```

`EdaCombinedMatcher` is the port of `EDA_COMBINED_MATCHER( pattern, CTX_NETCLASS )`
(`common/eda_pattern_match.cpp`: wildcard, regex and anchored variants); it lives in `kicad-core`.

### 3.4 Design settings and friends (`board_design_settings.rs`, `zone_settings.rs`, `board_stackup.rs`, `pcb_plot_params.rs`, `drill_symbol_profile.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord )]
pub struct ViaDimension { pub diameter: i32, pub drill: i32 }            // VIA_DIMENSION
#[derive( Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default )]
pub struct DiffPairDimension { pub width: i32, pub gap: i32, pub via_gap: i32 }   // DIFF_PAIR_DIMENSION
#[derive( Clone, Debug, PartialEq )]
pub struct ViaStackPreset {                                              // VIA_STACK_PRESET
    pub name: String, pub start_layer: PcbLayerId, pub end_layer: PcbLayerId, pub staggered: bool,
    pub via_size: i32, pub via_drill: i32, pub use_netclass: bool, pub filled: bool, pub capped: bool, pub pitch: i32,
}
#[derive( Clone, Debug, PartialEq )]
pub struct TextItemInfo { pub text: String, pub visible: bool, pub layer: PcbLayerId }   // TEXT_ITEM_INFO
pub const LAYER_CLASS_SILK: usize = 0;   pub const LAYER_CLASS_COPPER: usize = 1;   pub const LAYER_CLASS_EDGES: usize = 2;
pub const LAYER_CLASS_COURTYARD: usize = 3;   pub const LAYER_CLASS_FAB: usize = 4;   pub const LAYER_CLASS_OTHERS: usize = 5;
pub const LAYER_CLASS_COUNT: usize = 6;

/// TEARDROP_PARAMETERS_LIST.
#[derive( Clone, Debug, PartialEq )]
pub struct TeardropParametersList {
    pub target_vias: bool, pub target_pth_pads: bool, pub target_smd_pads: bool, pub use_round_shapes_only: bool,
    pub params_list: [TeardropParameters; 3],        // TARGET_ROUND, TARGET_RECT, TARGET_TRACK
}

/// PNS::MEANDER_SETTINGS (pcbnew/router/pns_meander.h): data only, stored in the design settings
/// and in tuning patterns.
#[derive( Clone, Debug, PartialEq )]
pub struct MeanderSettings {
    pub min_amplitude: i32, pub max_amplitude: i32, pub spacing: i32, pub step: i32, pub len_pad_to_die: i32,
    pub signal_extra_length: i64, pub signal_extra_delay: i64,
    pub target_length: MinOptMax<i64>, pub target_length_delay: MinOptMax<i64>,
    pub target_signal_length: MinOptMax<i64>, pub target_signal_length_delay: MinOptMax<i64>,
    pub target_skew: MinOptMax<i32>, pub target_skew_delay: MinOptMax<i32>,
    pub override_custom_rules: bool, pub corner_style: MeanderStyle, pub corner_radius_percentage: i32,
    pub single_sided: bool, pub initial_side: MeanderSide, pub length_tolerance: i32,
}
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum MeanderStyle { Round = 1, Chamfer }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum MeanderSide { Left = -1, Default = 0, Right = 1 }

/// BOARD_DESIGN_SETTINGS (include/board_design_settings.h). The NESTED_SETTINGS / JSON plumbing
/// is not part of the struct; the `.kicad_pro` loader fills it (project work package).
#[derive( Clone, Debug )]
pub struct BoardDesignSettings {
    pub track_width_list: Vec<i32>,                       // m_TrackWidthList (index 0 = netclass value)
    pub vias_dimensions_list: Vec<ViaDimension>,          // m_ViasDimensionsList
    pub via_stack_presets: Vec<ViaStackPreset>,           // m_ViaStackPresets
    pub diff_pair_dimensions_list: Vec<DiffPairDimension>,// m_DiffPairDimensionsList
    pub teardrop_params_list: TeardropParametersList,     // m_TeardropParamsList
    pub single_track_meander_settings: MeanderSettings,   // m_SingleTrackMeanderSettings
    pub diff_pair_meander_settings: MeanderSettings,      // m_DiffPairMeanderSettings
    pub skew_meander_settings: MeanderSettings,           // m_SkewMeanderSettings
    pub current_via_type: ViaType,                        // m_CurrentViaType
    pub use_connected_track_width: bool,                  // m_UseConnectedTrackWidth
    pub temp_override_track_width: bool,                  // m_TempOverrideTrackWidth
    pub min_clearance: i32,                               // m_MinClearance
    pub min_groove_width: i32,                            // m_MinGrooveWidth
    pub min_conn: i32,                                    // m_MinConn
    pub track_min_width: i32,                             // m_TrackMinWidth
    pub vias_min_annular_width: i32,                      // m_ViasMinAnnularWidth
    pub vias_min_size: i32,                               // m_ViasMinSize
    pub min_through_drill: i32,                           // m_MinThroughDrill
    pub micro_vias_min_size: i32,                         // m_MicroViasMinSize
    pub micro_vias_min_drill: i32,                        // m_MicroViasMinDrill
    pub copper_edge_clearance: i32,                       // m_CopperEdgeClearance
    pub hole_clearance: i32,                              // m_HoleClearance
    pub hole_to_hole_min: i32,                            // m_HoleToHoleMin
    pub silk_clearance: i32,                              // m_SilkClearance
    pub min_resolved_spokes: i32,                         // m_MinResolvedSpokes
    pub min_silk_text_height: i32,                        // m_MinSilkTextHeight
    pub min_silk_text_thickness: i32,                     // m_MinSilkTextThickness
    // m_DRCEngine lives in Board::hooks (2.6)
    pub drc_severities: BTreeMap<i32, Severity>,          // m_DRCSeverities (DRC error code -> severity)
    pub drc_exclusions: BTreeSet<DrcExclusion>,           // m_DrcExclusions (ordered by GetSortKey())
    pub zone_keep_external_fillets: bool,                 // m_ZoneKeepExternalFillets
    pub max_error: i32,                                   // m_MaxError
    pub solder_mask_expansion: i32,                       // m_SolderMaskExpansion
    pub solder_mask_min_width: i32,                       // m_SolderMaskMinWidth
    pub solder_mask_to_copper_clearance: i32,             // m_SolderMaskToCopperClearance
    pub solder_paste_margin: i32,                         // m_SolderPasteMargin
    pub solder_paste_margin_ratio: f64,                   // m_SolderPasteMarginRatio
    pub allow_soldermask_bridges_in_fps: bool,            // m_AllowSoldermaskBridgesInFPs
    pub tent_vias_front: bool, pub tent_vias_back: bool,  // m_TentViasFront / Back
    pub cover_vias_front: bool, pub cover_vias_back: bool,
    pub plug_vias_front: bool, pub plug_vias_back: bool,
    pub cap_vias: bool, pub fill_vias: bool,
    pub net_settings: NetSettings,                        // std::shared_ptr<NET_SETTINGS> m_NetSettings
    pub default_fp_text_items: Vec<TextItemInfo>,         // m_DefaultFPTextItems
    pub user_layer_names: BTreeMap<String, String>,       // m_UserLayerNames
    pub zone_layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,   // m_ZoneLayerProperties
    pub line_thickness: [i32; LAYER_CLASS_COUNT],         // m_LineThickness[]
    pub text_size: [Vector2I; LAYER_CLASS_COUNT],         // m_TextSize[]
    pub text_thickness: [i32; LAYER_CLASS_COUNT],         // m_TextThickness[]
    pub text_italic: [bool; LAYER_CLASS_COUNT],           // m_TextItalic[]
    pub text_upright: [bool; LAYER_CLASS_COUNT],          // m_TextUpright[]
    pub dimension_units_mode: DimUnitsMode,               // m_DimensionUnitsMode
    pub dimension_precision: DimPrecision,                // m_DimensionPrecision
    pub dimension_units_format: DimUnitsFormat,           // m_DimensionUnitsFormat
    pub dimension_suppress_zeroes: bool,
    pub dimension_text_position: DimTextPosition,
    pub dimension_keep_text_aligned: bool,
    pub dimension_arrow_length: i32,
    pub dimension_extension_offset: i32,
    pub style_fp_fields: bool, pub style_fp_text: bool, pub style_fp_shapes: bool,
    pub style_fp_dimensions: bool, pub style_fp_barcodes: bool,
    pub has_stackup: bool,                                // m_HasStackup
    pub use_height_for_length_calcs: bool,                // m_UseHeightForLengthCalcs
    // private members of the C++ class:
    pub( crate ) aux_origin: Vector2I,                    // m_auxOrigin
    pub( crate ) grid_origin: Vector2I,                   // m_gridOrigin
    pub( crate ) track_width_index: i32,
    pub( crate ) via_size_index: i32,
    pub( crate ) diff_pair_index: i32,
    pub( crate ) via_stack_index: i32,
    pub( crate ) use_custom_track_via: bool,
    pub( crate ) custom_track_width: i32,
    pub( crate ) custom_via_size: ViaDimension,
    pub( crate ) use_custom_diff_pair: bool,
    pub( crate ) custom_diff_pair: DiffPairDimension,
    pub( crate ) copper_layer_count: i32,                 // m_copperLayerCount
    pub( crate ) user_defined_layer_count: i32,           // m_userDefinedLayerCount
    pub( crate ) enabled_layers: Lset,                    // m_enabledLayers
    pub( crate ) board_thickness: i32,                    // m_boardThickness
    pub( crate ) current_net_class_name: String,
    pub( crate ) stackup: BoardStackup,                   // m_stackup
    pub( crate ) drill_symbol_profile: DrillSymbolProfile,// m_drillSymbolProfile
    pub( crate ) default_zone_settings: ZoneSettings,     // m_defaultZoneSettings
}

/// DRC_EXCLUSION (pcbnew/drc/drc_exclusion.h) as data: the serialized marker key and comment.
#[derive( Clone, Debug, PartialEq, Eq, PartialOrd, Ord )]
pub struct DrcExclusion { pub sort_key: String, pub serialized: String, pub comment: String }

// ---- zone_settings.h ----
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ZoneConnection { Inherited = -1, None = 0, Thermal, Full, ThtThermal }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ZoneFillMode { Polygons = 0, HatchPattern = 1, CopperThieving = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ThievingPattern { Dots = 0, Squares = 1, Hatch = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ZoneBorderDisplayStyle { NoHatch, DiagonalFull, DiagonalEdge, InvisibleBorder }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum IslandRemovalMode { Always = 0, Never, Area }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PlacementSourceT { Sheetname = 0, ComponentClass, GroupPlacement, DesignBlock }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum CornerSmoothing { NoSmoothing = 0, Chamfer, Fillet }   // ZONE_SETTINGS::CORNER_SMOOTHING
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum TeardropType { TdNone = 0, TdUnspecified, TdViapad, TdTrackend }
#[derive( Clone, Debug, PartialEq, Default )]
pub struct ZoneLayerProperties { pub hatching_offset: Option<Vector2I> }
#[derive( Clone, Debug, PartialEq )]
pub struct ThievingSettings {
    pub pattern: ThievingPattern, pub element_size: i32, pub gap: i32, pub line_width: i32,
    pub stagger: bool, pub orientation: EdaOrientation,
}
#[derive( Clone, Debug, PartialEq )]
pub struct RepeatLayoutOptions {
    pub copy_routing: bool, pub connected_routing_only: bool, pub copy_placement: bool, pub copy_other_items: bool,
    pub group_items: bool, pub include_locked_items: bool, pub anchor: Kiid,
}

/// ZONE_SETTINGS (pcbnew/zone_settings.h).
#[derive( Clone, Debug, PartialEq )]
pub struct ZoneSettings {
    pub zone_priority: u32,
    pub fill_mode: ZoneFillMode,
    pub zone_clearance: i32,
    pub zone_min_thickness: i32,
    pub hatch_thickness: i32,
    pub hatch_gap: i32,
    pub hatch_orientation: EdaAngle,
    pub hatch_smoothing_level: i32,
    pub hatch_smoothing_value: f64,
    pub hatch_hole_min_area: f64,
    pub hatch_border_algorithm: i32,
    pub thieving_settings: ThievingSettings,
    pub netcode: i32,
    pub name: String,
    pub layers: Lset,
    pub zone_border_display_style: ZoneBorderDisplayStyle,
    pub border_hatch_pitch: i32,
    pub thermal_relief_gap: i64,                 // long
    pub thermal_relief_spoke_width: i64,         // long
    pub locked: bool,
    pub teardrop_type: TeardropType,
    pub layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,
    pub( crate ) corner_smoothing_type: CornerSmoothing,
    pub( crate ) corner_radius: u32,
    pub( crate ) pad_connection: ZoneConnection,
    pub( crate ) is_rule_area: bool,
    pub( crate ) placement_area_enabled: bool,
    pub( crate ) placement_area_source_type: PlacementSourceT,
    pub( crate ) placement_area_source: String,
    pub( crate ) keepout_do_not_allow_zone_fills: bool,
    pub( crate ) keepout_do_not_allow_vias: bool,
    pub( crate ) keepout_do_not_allow_tracks: bool,
    pub( crate ) keepout_do_not_allow_pads: bool,
    pub( crate ) keepout_do_not_allow_footprints: bool,
    pub( crate ) remove_islands: IslandRemovalMode,
    pub( crate ) min_island_area: i64,
}

// ---- board_stackup.h ----
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum BoardStackupItemType { Undefined, Copper, Dielectric, Solderpaste, Soldermask, Silkscreen }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum BsEdgeConnectorConstraints { None, InUse, Bevelled }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum DielectricModel { Constant, DjordjevicSarkar }
#[derive( Clone, Debug, PartialEq )]
pub struct DielectricPrms {
    pub material: String, pub thickness: i32, pub thickness_locked: bool, pub epsilon_r: f64,
    pub loss_tangent: f64, pub spec_freq: f64, pub dielectric_model: DielectricModel, pub color: String,
}
#[derive( Clone, Debug, PartialEq )]
pub struct BoardStackupItem {
    pub( crate ) type_: BoardStackupItemType,
    pub( crate ) layer_name: String,
    pub( crate ) type_name: String,
    pub( crate ) layer_id: PcbLayerId,
    pub( crate ) dielectric_layer_id: i32,
    pub( crate ) dielectric_prms_list: Vec<DielectricPrms>,
    pub( crate ) enabled: bool,
}
#[derive( Clone, Debug, PartialEq )]
pub struct BoardStackup {
    pub finish_type: String,
    pub has_dielectric_constrains: bool,
    pub has_thickness_constrains: bool,
    pub edge_connector_constraints: BsEdgeConnectorConstraints,
    pub edge_plating: bool,
    pub( crate ) list: Vec<BoardStackupItem>,        // std::vector<BOARD_STACKUP_ITEM*> m_list (owned)
}

// ---- drill/drill_symbol_profile.h ----
#[derive( Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord )]
pub enum DrillGroupKey { SizeGrp, Slot, Plating, Span, Operation, HoleFunction, Protection, PostMachining }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DrillMarkMode { Shape, Letter, SizeText }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DrillMarkPolicy { Shapes, Letters, ShapesThenLetters, SizeText }
#[derive( Clone, Debug, PartialEq )]
pub struct DrillSymbolAssignment { pub mark_mode: DrillMarkMode, pub shape_index: i32, pub letter: String, pub description: String }
#[derive( Clone, Debug, PartialEq )]
pub struct DrillSymbolProfile {
    pub uuid: Kiid, pub name: String, pub group_by: BTreeSet<DrillGroupKey>, pub mark_policy: DrillMarkPolicy,
    pub symbol_size: i32, pub symbol_width: i32, pub freeze_assignments: bool,
    pub assignments: BTreeMap<String, DrillSymbolAssignment>,
}

// ---- pcb_plot_params.h ---- (enums of include/plotters/plotter.h that the struct stores are defined here
// and re-exported by kicad-plot, which sits beside kicad-pcb in the crate graph)
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PlotFormat { Undefined = -1, Hpgl = 0, Gerber, Post, Dxf, Pdf, Svg, Png }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DrillMarks { NoDrillShape = 0, SmallDrillShape = 1, FullDrillShape = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PlotTextMode { Stroke, Native, Phantom, Default }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DxfOutlineMode { Sketch, Filled }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DxfUnits { Inch = 0, Mm = 1 }

/// PCB_PLOT_PARAMS (pcbnew/pcb_plot_params.h).
#[derive( Clone, Debug, PartialEq )]
pub struct PcbPlotParams {
    pub pdf_front_fp_property_popups: bool,      // m_PDFFrontFPPropertyPopups
    pub pdf_back_fp_property_popups: bool,
    pub pdf_metadata: bool,
    pub pdf_single: bool,
    pub background_color: Color4D,
    pub( crate ) format: PlotFormat,
    pub( crate ) layer_selection: Lset,
    pub( crate ) plot_on_all_layers_sequence: Lseq,
    pub( crate ) skip_npth_pads: bool,
    pub( crate ) plot_pad_numbers: bool,
    pub( crate ) drill_marks: DrillMarks,
    pub( crate ) text_mode: PlotTextMode,
    pub( crate ) dxf_plot_mode: DxfOutlineMode,
    pub( crate ) dxf_units: DxfUnits,
    pub( crate ) dxf_polygon_mode: bool,
    pub( crate ) a4_output: bool,
    pub( crate ) auto_scale: bool,
    pub( crate ) scale: f64,
    pub( crate ) mirror: bool,
    pub( crate ) negative: bool,
    pub( crate ) black_and_white: bool,
    pub( crate ) plot_drawing_sheet: bool,
    pub( crate ) plot_via_on_mask_layer: Option<bool>,     // deprecated, legacy read only
    pub( crate ) subtract_mask_from_silk: bool,
    pub( crate ) use_gerber_protel_extensions: bool,
    pub( crate ) use_gerber_x2_format: bool,
    pub( crate ) gerber_disable_apert_macros: bool,
    pub( crate ) include_gerber_netlist_info: bool,
    pub( crate ) create_gerber_job_file: bool,
    pub( crate ) gerber_precision: i32,
    pub( crate ) svg_precision: u32,
    pub( crate ) svg_fit_page_to_board: bool,
    pub( crate ) png_dpi: i32,
    pub( crate ) png_antialias: bool,
    pub( crate ) use_aux_origin: bool,
    pub( crate ) output_directory: String,
    pub( crate ) scale_selection: i32,
    pub( crate ) plot_reference: bool,
    pub( crate ) plot_value: bool,
    pub( crate ) plot_fp_text: bool,
    pub( crate ) sketch_pads_on_fab_layers: bool,
    pub( crate ) sketch_pad_line_width: i32,
    pub( crate ) hide_dnp_fps_on_fab_layers: bool,
    pub( crate ) sketch_dnp_fps_on_fab_layers: bool,
    pub( crate ) crossout_dnp_fps_on_fab_layers: bool,
    pub( crate ) fine_scale_adjust_x: f64,
    pub( crate ) fine_scale_adjust_y: f64,
    pub( crate ) width_adjust: i32,
    pub( crate ) dashed_line_dash_ratio: f64,
    pub( crate ) dashed_line_gap_ratio: f64,
    pub( crate ) dxf_export_as_multi_layered_file: bool,
    pub( crate ) layers_to_export: Vec<( PcbLayerId, String )>,
    pub( crate ) layer: PcbLayerId,
}
// Dropped: COLOR_SETTINGS* m_colors, m_default_colors (GUI colour themes; plotting to Gerber is
// monochrome. kicad-plot receives colours as explicit parameters for SVG).
```

`TitleBlock { tb_texts: Vec<String> }` (title, date, revision, company, comment 1..9) and
`PageInfo { type_: PageSizeType, size: Vector2D /* mils */, portrait: bool, description: String }`
port `include/title_block.h` and `include/page_info.h` and live in `kicad-core` (the I/O analysis
lists their formatters there).

### 3.5 `Footprint` (`footprint.rs`)

```rust
pub const FP_THROUGH_HOLE: i32 = 0x0001;   pub const FP_SMD: i32 = 0x0002;   pub const FP_EXCLUDE_FROM_POS_FILES: i32 = 0x0004;
pub const FP_EXCLUDE_FROM_BOM: i32 = 0x0008;   pub const FP_BOARD_ONLY: i32 = 0x0010;   pub const FP_JUST_ADDED: i32 = 0x0020;
pub const FP_DNP: i32 = 0x0040;   pub const FP_EXCLUDE_FROM_SIM: i32 = 0x0080;          // FOOTPRINT_ATTR_T
pub const FP_IS_PLACED: i32 = 0x02;   pub const FP_TO_PLACE: i32 = 0x04;                 // m_fpStatus bits
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum FootprintType { Unspecified, ThroughHole, Smd }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum FootprintStackup { ExpandInnerLayers, CustomLayers }

#[derive( Clone, Debug, PartialEq )]
pub struct Fp3dModel {                       // FP_3DMODEL
    pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D,
    pub opacity: f64, pub filename: String, pub show: bool,
}
#[derive( Clone, Debug, PartialEq, Default )]
pub struct FootprintVariant {                // FOOTPRINT_VARIANT
    pub name: String, pub dnp: bool, pub excluded_from_bom: bool, pub excluded_from_sim: bool,
    pub excluded_from_pos_files: bool, pub fields: BTreeMap<String, String>,
}
#[derive( Clone, Debug, PartialEq )]
pub struct FpUnitInfo { pub unit_name: String, pub pins: Vec<String> }      // FOOTPRINT::FP_UNIT_INFO
#[derive( Clone, Debug, PartialEq, Default )]
pub struct JumperGroupSet { pub groups: Vec<BTreeSet<String>> }             // JUMPER_GROUP_SET / JUMPER_GROUP
#[derive( Clone, Debug, Default )]
pub struct FootprintGeometryCache {          // FOOTPRINT_GEOMETRY_CACHE_DATA
    pub bounding_box: Box2I, pub bounding_box_timestamp: i32,
    pub text_excluded_bbox: Box2I, pub text_excluded_bbox_timestamp: i32,
    pub hull: ShapePolySet, pub hull_timestamp: i32,
}
#[derive( Clone, Debug, Default )]
pub struct FootprintCourtyardCache {         // FOOTPRINT_COURTYARD_CACHE_DATA
    pub front: ShapePolySet, pub back: ShapePolySet, pub front_hash: Hash128, pub back_hash: Hash128,
}

/// Port of FOOTPRINT : BOARD_ITEM_CONTAINER, EMBEDDED_FILES (pcbnew/footprint.h:1404-1470).
#[derive( Debug )]
pub struct Footprint {
    pub item: BoardItemBase,                               // m_layer = F_Cu or B_Cu (the side)
    pub embedded_files: EmbeddedFiles,                     // EMBEDDED_FILES base class
    pub( crate ) fields: ItemDeque<PcbField>,              // m_fields
    pub( crate ) drawings: ItemDeque<dyn BoardItem>,       // m_drawings
    pub( crate ) pads: ItemDeque<Pad>,                     // m_pads
    pub( crate ) zones: ItemDeque<Zone>,                   // m_zones
    pub( crate ) groups: ItemDeque<PcbGroup>,              // m_groups
    pub( crate ) constraints: ItemDeque<PcbConstraint>,    // m_constraints
    pub( crate ) points: ItemDeque<PcbPoint>,              // m_points
    pub( crate ) bodies: ItemDeque<Pcb3dBody>,             // m_bodies
    pub( crate ) transform: TransformTrs,                  // m_transform (private: invariant I-FP)
    pub( crate ) flipped: bool,                            // m_flipped
    pub( crate ) link: Arc<FpLink>,                        // new: the snapshot handed to the children (2.5)
    pub fpid: LibId,                                       // m_fpid
    pub attributes: i32,                                   // m_attributes (FP_* bits)
    pub fp_status: i32,                                    // m_fpStatus
    pub file_format_version_at_load: i32,
    pub( crate ) variants: CaseInsensitiveMap<FootprintVariant>,     // m_variants (case-insensitive key order)
    pub( crate ) geometry_cache: Mutex<FootprintGeometryCache>,      // m_geometry_cache + mutex
    pub net_tie_pad_groups: Vec<String>,                   // m_netTiePadGroups
    pub( crate ) net_tie_cache: BTreeMap<ItemPtr, BTreeSet<i32>>,    // std::map<const BOARD_ITEM*, std::set<int>>
    pub jumper_pad_groups: JumperGroupSet,                 // m_jumperPadGroups
    pub duplicate_pad_numbers_are_jumpers: bool,
    pub allow_missing_courtyard: bool,
    pub allow_solder_mask_bridges: bool,
    pub zone_connection: ZoneConnection,                   // m_zoneConnection
    pub clearance: Option<i32>,                            // m_clearance
    pub solder_mask_margin: Option<i32>,
    pub solder_paste_margin: Option<i32>,
    pub solder_paste_margin_ratio: Option<f64>,
    pub( crate ) stackup_layers: Lset,                     // m_stackupLayers
    pub( crate ) stackup_mode: FootprintStackup,           // m_stackupMode
    pub private_layers: Lset,                              // m_privateLayers
    pub lib_description: String,                           // m_libDescription
    pub keywords: String,
    pub path: KiidPath,                                    // m_path
    pub sheetname: String,
    pub sheetfile: String,
    pub filters: String,
    pub last_edit_time: i64,                               // timestamp_t m_lastEditTime (passed in, never the clock)
    pub arflag: i32,                                       // m_arflag
    pub link_uuid: Kiid,                                   // KIID m_link (renamed: `link` is the FpLink)
    pub models_3d: Vec<Fp3dModel>,                         // m_3D_Drawings
    pub height: i32,                                       // m_height
    pub initial_comments: Option<Vec<String>>,             // wxArrayString* m_initial_comments
    pub( crate ) courtyard_cache: OnceLock<FootprintCourtyardCache>, // m_courtyard_cache + mutex
    pub( crate ) transient_component_class_names: BTreeSet<String>,  // unordered_set in C++: sorted here
    pub( crate ) component_class_cache_proxy: ComponentClassCacheProxy,
    pub unit_info: Vec<FpUnitInfo>,                        // m_unitInfo
}
// Dropped: m_fieldPropsStorageCache, m_searchTerms, LIB_TREE_ITEM base (library browser).
// `impl Clone for Footprint` ports the copy constructor: children are cloned, re-parented to a fresh
// FpLink with the same uuid, group membership is re-mapped (footprint.cpp:215-330).
```

`Footprint::new()` adds the four mandatory fields (Reference on F.SilkS visible, Value on F.Fab
visible, Datasheet and Description on F.Fab hidden) exactly as `footprint.cpp:181-192`.

### 3.6 `Padstack` and `Pad` (`padstack.rs`, `pad.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadShape { Circle, Rectangle, Oval, Trapezoid, Roundrect, ChamferedRect, Custom }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadDrillShape { Undefined, Circle, Oblong }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadDrillPostMachiningMode { Unknown, NotPostMachined, Counterbore, Countersink }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum BackdrillMode { NoBackdrill, BackdrillBottom, BackdrillTop, BackdrillBoth }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadAttrib { Pth, Smd, Conn, Npth, NonComponent }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum PadProp { None, Bga, FiducialGlbl, FiducialLocal, Testpoint, Heatsink, Castellated, Mechanical, Pressfit }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum UnconnectedLayerMode { KeepAll, StartEndOnly, RemoveAll, RemoveExceptStartAndEnd }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum CustomShapeZoneMode { Outline, Convexhull }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadSimElectricalType { None = 0, Source, Sink }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadstackType { Normal, Via, Mounting }      // PADSTACK::TYPE
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PadstackMode { Normal, FrontInnerBack, Custom }   // PADSTACK::MODE
pub const RECT_NO_CHAMFER: i32 = 0;  pub const RECT_CHAMFER_TOP_LEFT: i32 = 1;  pub const RECT_CHAMFER_TOP_RIGHT: i32 = 2;
pub const RECT_CHAMFER_BOTTOM_LEFT: i32 = 4;  pub const RECT_CHAMFER_BOTTOM_RIGHT: i32 = 8;  pub const RECT_CHAMFER_ALL: i32 = 15;

#[derive( Clone, Debug )]
pub struct ShapeProps {                      // PADSTACK::SHAPE_PROPS
    pub shape: PadShape,
    pub anchor_shape: PadShape,
    pub size: Vector2I,
    pub offset: Vector2I,
    pub round_rect_radius_ratio: f64,
    pub chamfered_rect_ratio: f64,
    pub chamfered_rect_positions: i32,       // RECT_CHAMFER_* bits
    pub trapezoid_delta_size: Vector2I,
}
#[derive( Clone, Debug )]
pub struct CopperLayerProps {                // PADSTACK::COPPER_LAYER_PROPS
    pub shape: ShapeProps,
    pub zone_connection: Option<ZoneConnection>,
    pub thermal_spoke_width: Option<i32>,
    pub thermal_spoke_angle: Option<EdaOrientation>,
    pub thermal_gap: Option<i32>,
    pub clearance: Option<i32>,
    /// std::vector<std::shared_ptr<PCB_SHAPE>> custom_shapes, in the pad's own frame.
    pub custom_shapes: Vec<Arc<PcbShape>>,
}
#[derive( Clone, Debug, PartialEq, Default )]
pub struct MaskLayerProps {                  // PADSTACK::MASK_LAYER_PROPS
    pub solder_mask_margin: Option<i32>,
    pub solder_paste_margin: Option<i32>,
    pub solder_paste_margin_ratio: Option<f64>,
    pub has_solder_mask: Option<bool>,
    pub has_solder_paste: Option<bool>,
    pub has_covering: Option<bool>,
    pub has_plugging: Option<bool>,
}
#[derive( Clone, Debug, PartialEq )]
pub struct DrillProps {                      // PADSTACK::DRILL_PROPS
    pub size: Vector2I,
    pub shape: PadDrillShape,                // default Undefined
    pub start: PcbLayerId,                   // default UNDEFINED_LAYER
    pub end: PcbLayerId,
    pub is_filled: Option<bool>,
    pub is_capped: Option<bool>,
}
#[derive( Clone, Debug, PartialEq, Default )]
pub struct PostMachiningProps {              // PADSTACK::POST_MACHINING_PROPS
    pub mode: Option<PadDrillPostMachiningMode>, pub size: i32, pub depth: i32, pub angle: i32,
}

/// PADSTACK (pcbnew/padstack.h:596-624). Plain data owned by a Pad or a PcbVia.
#[derive( Clone, Debug )]
pub struct Padstack {
    /// Replaces `BOARD_ITEM* m_parent`: the only things C++ asks the parent are its type and,
    /// through it, the board (passed as ctx where needed: `flip_layers( env )`, `machining_knockout( ctx, layer )`).
    pub( crate ) owner_type: KicadT,                       // PCB_PAD_T or PCB_VIA_T
    pub( crate ) mode: PadstackMode,
    pub( crate ) layer_set: Lset,
    pub( crate ) custom_name: Option<String>,              // std::unique_ptr<wxString>
    pub( crate ) orientation: EdaOrientation,              // pads: unused in favour of Pad::lib_orientation
    /// std::unordered_map<PCB_LAYER_ID, COPPER_LAYER_PROPS>; keys F_Cu (= ALL_LAYERS), In1_Cu (= INNER_LAYERS), B_Cu, or any copper layer in CUSTOM mode.
    pub( crate ) copper_props: BTreeMap<PcbLayerId, CopperLayerProps>,
    pub( crate ) front_mask_props: MaskLayerProps,
    pub( crate ) back_mask_props: MaskLayerProps,
    pub( crate ) unconnected_layer_mode: UnconnectedLayerMode,
    pub( crate ) custom_shape_in_zone_mode: CustomShapeZoneMode,
    pub( crate ) drill: DrillProps,
    pub( crate ) secondary_drill: DrillProps,
    pub( crate ) tertiary_drill: DrillProps,
    pub( crate ) front_post_machining: PostMachiningProps,
    pub( crate ) back_post_machining: PostMachiningProps,
}
impl Padstack {
    pub const ALL_LAYERS: PcbLayerId = PcbLayerId::FCu;
    pub const INNER_LAYERS: PcbLayerId = PcbLayerId::In1Cu;
    // Same accessors as C++: copper_layer( layer ), shape( layer ), size( layer ), offset( layer ), drill(),
    // for_each_unique_layer( &mut dyn FnMut( PcbLayerId ) ), unique_layers(), effective_layer_for( layer ),
    // start_layer(), end_layer(), is_tented( side ), primitives( layer ), compare( a, b ) -> i32, ...
}

#[derive( Clone, Debug, Default )]
pub struct PadShapeCache {                   // PAD::PAD_DRAW_CACHE_DATA
    pub effective_bounding_box: Box2I,
    pub effective_shapes: BTreeMap<PcbLayerId, Arc<ShapeCompound>>,
    pub effective_hole_shape: Option<Arc<ShapeSegment>>,
    pub effective_bounding_radius: i32,      // PAD::m_effectiveBoundingRadius
}

/// Port of PAD : BOARD_CONNECTED_ITEM (pcbnew/pad.h:1124-1173).
#[derive( Clone, Debug )]
pub struct Pad {
    pub conn: BoardConnectedItemBase,
    pub number: String,                                    // m_number
    pub pin_function: String,                              // m_pinFunction
    pub pin_type: String,                                  // m_pinType
    pub( crate ) lib_pos: Vector2I,                        // m_libPos (footprint library frame)
    pub( crate ) lib_orientation: EdaOrientation,          // m_libOrientation
    pub( crate ) padstack: Padstack,                       // m_padStack (sizes, offsets, drill in library frame)
    pub( crate ) shape_cache: OnceLock<PadShapeCache>,     // m_drawCache + m_shapesDirty + m_dataMutex
    pub( crate ) poly_cache: [OnceLock<BTreeMap<PcbLayerId, Arc<ShapePolySet>>>; 2],   // m_effectivePolygons + m_polyDirty[2]
    pub( crate ) mask_shapes: Mutex<BTreeMap<PcbLayerId, ( i32, Arc<ShapeCompound> )>>,   // m_maskShapes + mutex
    pub sub_ratsnest: i32,                                 // m_subRatsnest
    pub( crate ) attribute: PadAttrib,                     // m_attribute (default Pth)
    pub( crate ) property: PadProp,                        // m_property
    pub length_pad_to_die: i32,                            // m_lengthPadToDie
    pub delay_pad_to_die: i32,                             // m_delayPadToDie
    pub sim_electrical_type: PadSimElectricalType,
    pub( crate ) zone_layer_overrides: BTreeMap<PcbLayerId, ZoneLayerOverride>,   // m_zoneLayerOverrides
}
// Dropped: m_lastGalZoomLevel (view).  `Mutex` fields are cloned by value in `impl Clone`.
```

Board-frame accessors are parameter-free thanks to the snapshot, e.g. (port of `pad.cpp:244-304`):

```rust
impl Pad {
    pub fn position( &self ) -> Vector2I {                             // PAD::GetPosition
        match self.conn.item.fp_transform() { Some( xf ) => xf.apply( self.lib_pos ), None => self.lib_pos }
    }
    pub fn set_position_pad( &mut self, pos: Vector2I ) {              // PAD::SetPosition
        self.lib_pos = match self.conn.item.fp_transform() { Some( xf ) => xf.inverse_apply( pos ), None => pos };
        self.set_dirty();
    }
    pub fn library_position( &self ) -> Vector2I;                      // GetLibraryPosition
    pub fn size( &self, layer: PcbLayerId ) -> Vector2I;               // derivePadBoardSize( lib size, shape, xform, rel. orientation )
    pub fn set_size( &mut self, layer: PcbLayerId, size: Vector2I );   pub fn set_lib_size( &mut self, layer: PcbLayerId, size: Vector2I );
    pub fn drill_size( &self ) -> Vector2I;                            pub fn set_lib_drill_size( &mut self, size: Vector2I );
    pub fn offset( &self, layer: PcbLayerId ) -> Vector2I;             pub fn set_lib_offset( &mut self, layer: PcbLayerId, offset: Vector2I );
    pub fn orientation( &self ) -> EdaAngle;                           pub fn fp_relative_orientation( &self ) -> EdaAngle;
    pub fn shape_pos( &self, layer: PcbLayerId ) -> Vector2I;
    pub fn flash_layer( &self, ctx: BoardCtx<'_>, layer: PcbLayerId, only_check_if_permitted: bool ) -> bool;   // connectivity hook
    pub fn solder_mask_expansion( &self, ctx: BoardCtx<'_>, layer: PcbLayerId ) -> i32;                           // DRC hook or local/footprint/board values
    pub fn solder_paste_margin( &self, ctx: BoardCtx<'_>, layer: PcbLayerId ) -> Vector2I;
    pub fn zone_connection_overrides( &self, ctx: BoardCtx<'_> ) -> ( ZoneConnection, Option<String> );
    pub fn effective_polygon( &self, ctx: BoardCtx<'_>, layer: PcbLayerId, error_loc: ErrorLoc ) -> Arc<ShapePolySet>;
    pub fn same_logical_pad_as( &self, other: &Pad ) -> bool;          // compares fp uuid/ptr from the links + number
    pub fn set_dirty( &mut self );
    // ... every other accessor of pad.h under its snake_case name
}
```

### 3.7 Tracks, arcs, vias (`pcb_track.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum EndpointT { Start = 0, End = 1 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ViaType { NotDefined = 0, Microvia = 1, Blind = 2, Buried = 3, Through = 4 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum TentingMode { FromBoard = 0, Tented = 1, NotTented = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum CoveringMode { FromBoard = 0, Covered = 1, NotCovered = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PluggingMode { FromBoard = 0, Plugged = 1, NotPlugged = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum CappingMode { FromBoard = 0, Capped = 1, NotCapped = 2 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum FillingMode { FromBoard = 0, Filled = 1, NotFilled = 2 }
pub const UNDEFINED_DRILL_DIAMETER: i32 = -1;

/// Data members of PCB_TRACK (pcbnew/pcb_track.h:299-306), shared by trace, arc and via.
#[derive( Clone, Debug )]
pub struct PcbTrackBase {
    pub conn: BoardConnectedItemBase,
    pub( crate ) start: Vector2I,                  // m_Start
    pub( crate ) end: Vector2I,                    // m_End
    pub( crate ) has_solder_mask: bool,            // m_hasSolderMask
    pub( crate ) solder_mask_margin: Option<i32>,  // m_solderMaskMargin
    pub( crate ) width: i32,                       // m_width (trace/arc thickness; not the via diameter)
}
#[derive( Clone, Debug )] pub struct PcbTrack { pub track: PcbTrackBase }                       // PCB_TRACE_T
#[derive( Clone, Debug )] pub struct PcbArc { pub track: PcbTrackBase, pub( crate ) mid: Vector2I }   // PCB_ARC_T, m_Mid
// PcbVia: section 2.8. C++ `std::array<std::atomic<ZONE_LAYER_OVERRIDE>, MAX_CU_LAYERS>` is a plain
// array: the filler writes it through `&mut` in its apply phase.
```

### 3.8 `Zone` (`zone.rs`)

```rust
/// Port of ZONE : BOARD_CONNECTED_ITEM (pcbnew/zone.h:945-1085).
#[derive( Debug )]
pub struct Zone {
    pub conn: BoardConnectedItemBase,
    /// SHAPE_POLY_SET* m_Poly. For a footprint zone this is the *library* outline (zone.cpp:947);
    /// `board_outline()` applies the FpLink transform, `library_outline()` returns it as is.
    pub( crate ) poly: ShapePolySet,
    pub( crate ) corner_smoothing_type: CornerSmoothing,
    pub( crate ) corner_radius: u32,
    pub( crate ) zone_name: String,
    pub( crate ) layer_set: Lset,                          // m_layerSet (+ m_layerSetMutex: dropped)
    pub( crate ) layer_properties: BTreeMap<PcbLayerId, ZoneLayerProperties>,
    pub( crate ) priority: u32,                            // m_priority
    pub( crate ) is_rule_area: bool,
    pub( crate ) placement_area_enabled: bool,
    pub( crate ) placement_area_source_type: PlacementSourceT,
    pub( crate ) placement_area_source: String,
    pub link_set: Kiid,                                    // m_linkSet
    pub repeat_options: RepeatLayoutOptions,               // m_repeatOptions
    pub( crate ) teardrop_type: TeardropType,
    pub( crate ) do_not_allow_zone_fills: bool,
    pub( crate ) do_not_allow_vias: bool,
    pub( crate ) do_not_allow_tracks: bool,
    pub( crate ) do_not_allow_pads: bool,
    pub( crate ) do_not_allow_footprints: bool,
    pub( crate ) pad_connection: ZoneConnection,           // m_PadConnection
    pub( crate ) zone_clearance: i32,                      // m_ZoneClearance
    pub( crate ) zone_min_thickness: i32,                  // m_ZoneMinThickness
    pub( crate ) fill_version: i32,                        // m_fillVersion
    pub( crate ) island_removal_mode: IslandRemovalMode,
    pub( crate ) min_island_area: i64,                     // long long
    pub( crate ) is_filled: bool,                          // m_isFilled
    pub( crate ) need_refill: bool,                        // std::atomic<bool> m_needRefill
    pub( crate ) thermal_relief_gap: i32,
    pub( crate ) thermal_relief_spoke_width: i32,
    pub( crate ) fill_mode: ZoneFillMode,
    pub( crate ) hatch_thickness: i32,
    pub( crate ) hatch_gap: i32,
    pub( crate ) hatch_orientation: EdaOrientation,
    pub( crate ) hatch_smoothing_level: i32,
    pub( crate ) hatch_smoothing_value: f64,
    pub( crate ) hatch_hole_min_area: f64,
    pub( crate ) hatch_border_algorithm: i32,
    pub( crate ) thieving_settings: ThievingSettings,
    pub local_flgs: i32,                                   // m_localFlgs (scratch for polygon code)
    /// std::map<PCB_LAYER_ID, std::shared_ptr<SHAPE_POLY_SET>> m_FilledPolysList (+ mutex: dropped)
    pub( crate ) filled_polys_list: BTreeMap<PcbLayerId, Arc<ShapePolySet>>,
    pub( crate ) fill_flags: Lset,                         // m_fillFlags (+ mutex: dropped)
    pub( crate ) filled_polys_hash: BTreeMap<PcbLayerId, Hash128>,
    pub( crate ) border_style: ZoneBorderDisplayStyle,
    pub( crate ) border_hatch_pitch: i32,
    pub( crate ) border_hatch_lines: Vec<Seg>,             // derived by HatchBorder(), not saved
    pub( crate ) insulated_islands: BTreeMap<PcbLayerId, BTreeSet<i32>>,
    pub( crate ) area: f64,                                // m_area
    pub( crate ) outlinearea: f64,                         // m_outlinearea
    pub( crate ) bbox_cache: Mutex<( Box2I, i32 )>,        // m_bboxCache + m_bboxCacheTimeStamp (-1 = invalid)
}
// Dropped: m_CornerSelection and the corner-editing API (interactive), GetLock() (sequential port).
// `impl Clone` ports ZONE( const ZONE& ): deep copy of fills (new Arc per layer), bbox cache reset.
```

### 3.9 `EdaShape` and `PcbShape` (`eda_shape.rs`, `pcb_shape.rs`, `line_ending.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum ShapeT { Undefined = -1, Segment = 0, Rectangle, Arc, Circle, Poly, Bezier, Ellipse, EllipseArc }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum FillT { NoFill = 1, FilledShape, FilledWithBgBodycolor, FilledWithColor, Hatch, ReverseHatch, CrossHatch }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum LineEndingStyle { None = 0, Arrow, Circle, Square, ArrowOpen }
#[derive( Clone, Debug, PartialEq )]
pub struct LineEnding { pub style: LineEndingStyle, pub length: i32, pub width: i32, pub stroke: StrokeParams }
#[derive( Clone, Copy, Debug, PartialEq, Default )]
pub struct ArcMid { pub mid: Vector2I, pub start: Vector2I, pub end: Vector2I, pub center: Vector2I }   // ARC_MID
#[derive( Clone, Debug, Default )]
pub struct EdaShapeHatchCache { pub hatching: ShapePolySet, pub hatch_lines: Vec<Seg> }

/// Data members of EDA_SHAPE (include/eda_shape.h:740-780).
#[derive( Clone, Debug )]
pub struct EdaShapeBase {
    pub( crate ) ends_swapped: bool,               // m_endsSwapped
    pub( crate ) shape: ShapeT,                    // m_shape
    pub( crate ) stroke: StrokeParams,             // m_stroke (library-frame width for footprint children)
    pub( crate ) start_ending: LineEnding,         // m_startEnding
    pub( crate ) end_ending: LineEnding,           // m_endEnding
    pub( crate ) fill: FillT,                      // m_fill
    pub( crate ) fill_color: Color4D,              // m_fillColor
    pub( crate ) hatching_cache: OnceLock<EdaShapeHatchCache>,   // m_hatchingCache + m_hatchingDirty
    pub( crate ) rectangle_height: i64,            // m_rectangleHeight
    pub( crate ) rectangle_width: i64,             // m_rectangleWidth
    pub( crate ) corner_radius: i32,               // m_cornerRadius
    pub( crate ) start: Vector2I,                  // m_start: line start or circle centre
    pub( crate ) end: Vector2I,                    // m_end: line end or circle 3 o'clock point
    pub( crate ) arc_center: Vector2I,             // m_arcCenter
    pub( crate ) arc_mid_data: ArcMid,             // m_arcMidData
    pub( crate ) bezier_c1: Vector2I,
    pub( crate ) bezier_c2: Vector2I,
    pub( crate ) bezier_points: Vec<Vector2I>,     // derived by RebuildBezierToSegmentsPointsList
    pub( crate ) ellipse: Ellipse<i32>,            // m_ellipse (ELLIPSE / ELLIPSE_ARC)
    pub( crate ) poly: ShapePolySet,               // mutable std::unique_ptr<SHAPE_POLY_SET> m_poly (empty until used)
    pub( crate ) proxy_item: bool,                 // m_proxyItem
}
// Dropped: m_editState and the interactive begin/continue/calc/endEdit API, m_blankEnding (a static default).

/// Data members of PCB_SHAPE : BOARD_CONNECTED_ITEM, EDA_SHAPE (pcbnew/pcb_shape.h:383-406).
#[derive( Clone, Debug )]
pub struct PcbShapeBase {
    pub conn: BoardConnectedItemBase,
    pub shape: EdaShapeBase,                       // the baked, board-frame geometry
    pub( crate ) has_solder_mask: bool,
    pub( crate ) solder_mask_margin: Option<i32>,
    // Library-frame copy (what the file stores for footprint children), kept in step by
    // sync_lib_coords() / rebake_from_lib() exactly as in pcb_shape.cpp:1090-1600.
    pub( crate ) lib_start: Vector2I,
    pub( crate ) lib_end: Vector2I,
    pub( crate ) lib_corner_radius: i32,
    pub( crate ) lib_arc_mid: Vector2I,
    pub( crate ) lib_bezier_c1: Vector2I,
    pub( crate ) lib_bezier_c2: Vector2I,
    pub( crate ) lib_poly: ShapePolySet,
    pub( crate ) lib_ellipse_center: Vector2I,
    pub( crate ) lib_ellipse_major_radius: i32,
    pub( crate ) lib_ellipse_minor_radius: i32,
    pub( crate ) lib_ellipse_rotation: EdaAngle,
    pub( crate ) lib_ellipse_start_angle: EdaAngle,
    pub( crate ) lib_ellipse_end_angle: EdaAngle,
    pub( crate ) lib_shape: ShapeT,
}
#[derive( Clone, Debug )] pub struct PcbShape { pub shape: PcbShapeBase }        // PCB_SHAPE_T
```

`PcbShapeBase` reads its transform as `PCB_SHAPE::transformFp()` does: the `FpLink` transform for
`ParentLink::Footprint`, the pad scale for `ParentLink::Pad`, nothing otherwise.

### 3.10 Text family and tables (`eda_text.rs`, `pcb_text.rs`, `pcb_field.rs`, `pcb_textbox.rs`, `pcb_tablecell.rs`, `pcb_table.rs`, `pcb_generated_table.rs`)

```rust
/// EDA_TEXT_RENDER_CACHE_DATA: outline-font glyph polygons read from `(render_cache ..)`.
/// File data, re-emitted verbatim while the key (text, angle, offset, font) is unchanged.
#[derive( Clone, Debug )]
pub struct EdaTextRenderCache {
    pub text: String, pub angle: EdaAngle, pub offset: Vector2I, pub font_name: String,
    pub glyphs: Vec<ShapePolySet>,                 // std::vector<std::unique_ptr<KIFONT::GLYPH>> (outline glyphs)
}
#[derive( Clone, Debug )] pub struct BboxCacheEntry { pub pos: Vector2I, pub bbox: Box2I }

/// Data members of EDA_TEXT (include/eda_text.h:484-520).
#[derive( Debug )]
pub struct EdaTextBase {
    pub hyperlink: String,                         // m_hyperlink
    pub( crate ) text: String,                     // m_text
    pub( crate ) shown_text: String,               // m_shown_text (unescaped cache)
    pub( crate ) shown_text_has_text_var_refs: bool,
    pub( crate ) text_var_refs: Vec<TextVarRefKey>,// m_text_var_refs
    pub( crate ) render_cache: Option<Box<EdaTextRenderCache>>,   // m_render_cache
    pub( crate ) bbox_cache: Mutex<BTreeMap<i32, BboxCacheEntry>>,// m_bbox_cache + mutex
    pub( crate ) attributes: TextAttributes,       // m_attributes (font, h/v align, angle, line spacing, stroke
                                                   //   width, italic, bold, underlined, colour, mirrored,
                                                   //   multiline, size, keep upright); kicad-core type
    pub( crate ) unresolved_font_name: String,     // m_unresolvedFontName
    pub( crate ) pos: Vector2I,                    // m_pos (library frame for footprint children)
    pub( crate ) visible: bool,                    // m_visible
}
// Dropped: m_activeUrl (GUI), m_IuScale (always the PCB scale in this crate).

#[derive( Clone, Debug, Default )]
pub struct PcbTextKnockoutCache { pub text: String, pub font_name: String, pub attrs_key: u64, pub knockout: ShapePolySet }

/// Data members of PCB_TEXT : BOARD_ITEM, EDA_TEXT.
#[derive( Clone, Debug )]
pub struct PcbTextBase {
    pub item: BoardItemBase,
    pub text: EdaTextBase,
    pub( crate ) lib_text_angle: EdaOrientation,   // m_libTextAngle
    pub( crate ) knockout_cache: Mutex<Option<PcbTextKnockoutCache>>,   // m_knockout_cache
}
#[derive( Clone, Debug )] pub struct PcbText { pub text: PcbTextBase }       // PCB_TEXT_T

/// FIELD_T (include/template_fieldnames.h).
#[derive( Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord )]
pub enum FieldT { User, Reference, Value, Footprint, Datasheet, Description, IntersheetRefs, SheetName, SheetFilename, SheetUser, SheetRefSuffix }

/// PCB_FIELD : PCB_TEXT.
#[derive( Clone, Debug )]
pub struct PcbField {
    pub text: PcbTextBase,
    pub( crate ) id: FieldT,                       // m_id
    pub( crate ) ordinal: i32,                     // m_ordinal
    pub( crate ) name: String,                     // m_name
    pub private: bool,                             // m_private
}

/// Data members of PCB_TEXTBOX : PCB_SHAPE, EDA_TEXT.
#[derive( Clone, Debug )]
pub struct PcbTextboxBase {
    pub shape: PcbShapeBase,
    pub text: EdaTextBase,
    pub( crate ) border_enabled: bool,             // m_borderEnabled
    pub( crate ) margin_left: i32, pub( crate ) margin_top: i32,
    pub( crate ) margin_right: i32, pub( crate ) margin_bottom: i32,
    pub( crate ) lib_text_angle: EdaOrientation,   // m_libTextAngle
}
#[derive( Clone, Debug )] pub struct PcbTextbox { pub textbox: PcbTextboxBase }                 // PCB_TEXTBOX_T
#[derive( Clone, Debug )]
pub struct PcbTablecell { pub textbox: PcbTextboxBase, pub( crate ) col_span: i32, pub( crate ) row_span: i32 }   // PCB_TABLECELL_T

/// Data members of PCB_TABLE : BOARD_ITEM_CONTAINER (pcbnew/pcb_table.h:352-364).
#[derive( Clone, Debug )]
pub struct PcbTableBase {
    pub item: BoardItemBase,
    pub( crate ) stroke_external: bool,
    pub( crate ) stroke_header_separator: bool,    // m_StrokeHeaderSeparator
    pub( crate ) border_stroke: StrokeParams,
    pub( crate ) stroke_rows: bool,
    pub( crate ) stroke_columns: bool,
    pub( crate ) separators_stroke: StrokeParams,
    pub( crate ) col_count: i32,
    pub( crate ) col_widths: BTreeMap<i32, i32>,
    pub( crate ) row_heights: BTreeMap<i32, i32>,
    pub( crate ) cells: Vec<Box<PcbTablecell>>,    // std::vector<PCB_TABLECELL*> m_cells (owned, row-major)
}
#[derive( Clone, Debug )] pub struct PcbTable { pub table: PcbTableBase }                       // PCB_TABLE_T

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum GeneratedTableAlign { Left, Center, Right }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum GeneratedTableUnits { Mm, Inch, Mils }
#[derive( Clone, Debug, PartialEq )]
pub struct GeneratedTableColumn { pub id: i32, pub heading: String, pub align: GeneratedTableAlign, pub width: i32, pub visible: bool }
#[derive( Clone, Debug, PartialEq )] pub struct HiddenColumn { pub column: GeneratedTableColumn, pub position: i32 }
#[derive( Clone, Debug, PartialEq )]
pub struct DrillChartFilter { pub plated: bool, pub non_plated: bool, pub vias: bool, pub slots: bool, pub backdrills: bool, pub castellated: bool }

/// Data members of PCB_GENERATED_TABLE : PCB_TABLE.
#[derive( Clone, Debug )]
pub struct PcbGeneratedTableBase {
    pub table: PcbTableBase,
    pub( crate ) columns: Vec<GeneratedTableColumn>,
    pub( crate ) units: GeneratedTableUnits,
    pub( crate ) precision: i32,
    pub( crate ) row_keys: BTreeMap<i32, String>,
    pub( crate ) hidden: Vec<HiddenColumn>,
}
#[derive( Clone, Debug )]
pub struct PcbDrillChart {                                                                       // PCB_DRILL_CHART_T
    pub gen_table: PcbGeneratedTableBase,
    pub( crate ) filter: DrillChartFilter,
    pub( crate ) show_totals: bool,
    pub( crate ) laid_out_show_totals: Option<bool>,
    pub( crate ) row_shapes: BTreeMap<i32, i32>,
    pub( crate ) symbol_column: i32,
}
#[derive( Clone, Debug )] pub struct PcbStackupTable { pub gen_table: PcbGeneratedTableBase }   // PCB_STACKUP_TABLE_T
```

### 3.11 Dimensions (`pcb_dimension.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimUnitsFormat { NoSuffix, BareSuffix, ParenSuffix }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum DimPrecision { X = 0, XX, XXx, XXxx, XXxxx, XXxxxx, VVv, VVvv, VVvvv, VVvvvv }   // X, X_X, .. X_XXXXX, V_VV .. V_VVVVV
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimTextPosition { Outside, Inline, Manual }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimUnitsMode { Inch, Mils, Mm, Automatic }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimArrowDirection { Inward, Outward }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimTextBorder { None, Rectangle, Circle, Roundrect }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum DimOrthogonalDir { Horizontal, Vertical }      // PCB_DIM_ORTHOGONAL::DIR

/// Data members of PCB_DIMENSION_BASE : PCB_TEXT (pcbnew/pcb_dimension.h:410-445).
#[derive( Clone, Debug )]
pub struct PcbDimensionBase {
    pub text: PcbTextBase,
    pub( crate ) override_text_enabled: bool,      // m_overrideTextEnabled
    pub( crate ) value_string: String,             // m_valueString
    pub( crate ) prefix: String,
    pub( crate ) suffix: String,
    pub( crate ) units: EdaUnits,
    pub( crate ) auto_units: bool,
    pub( crate ) units_format: DimUnitsFormat,
    pub( crate ) arrow_direction: DimArrowDirection,
    pub( crate ) precision: DimPrecision,
    pub( crate ) suppress_zeroes: bool,
    pub( crate ) line_thickness: i32,
    pub( crate ) arrow_length: i32,
    pub( crate ) extension_offset: i32,
    pub( crate ) text_position: DimTextPosition,
    pub( crate ) keep_text_aligned: bool,
    pub( crate ) measured_value: i32,
    pub( crate ) start: Vector2I,                  // FP-relative in a footprint, board absolute otherwise
    pub( crate ) end: Vector2I,
    pub( crate ) shapes: Vec<Arc<dyn Shape>>,      // m_shapes: derived by updateGeometry()
    pub( crate ) busy: bool,                       // m_busy / m_inClearRenderCache re-entrancy guards
}
#[derive( Clone, Debug )]
pub struct PcbDimAlignedBase {                     // data members of PCB_DIM_ALIGNED
    pub dim: PcbDimensionBase,
    pub( crate ) height: i32,                      // m_height
    pub( crate ) extension_height: i32,            // m_extensionHeight
    pub( crate ) cross_bar_start: Vector2I,        // m_crossBarStart (derived)
    pub( crate ) cross_bar_end: Vector2I,
}
#[derive( Clone, Debug )] pub struct PcbDimAligned { pub aligned: PcbDimAlignedBase }
#[derive( Clone, Debug )] pub struct PcbDimOrthogonal { pub aligned: PcbDimAlignedBase, pub( crate ) orientation: DimOrthogonalDir }
#[derive( Clone, Debug )] pub struct PcbDimRadial { pub dim: PcbDimensionBase, pub( crate ) leader_length: i32 }
#[derive( Clone, Debug )] pub struct PcbDimLeader { pub dim: PcbDimensionBase, pub( crate ) text_border: DimTextBorder }
#[derive( Clone, Debug )] pub struct PcbDimCenter { pub dim: PcbDimensionBase }
```

`Dimension::update( &mut self, env: EnvCtx<'_> )` is the port of `PCB_DIMENSION_BASE::Update()`
(`updateGeometry()` + `updateText()`, virtual per class); the parser calls it at the end of
`parseDIMENSION`, because the recomputed text and geometry are written back (I/O analysis 4.7).

### 3.12 Groups and generators (`eda_group.rs`, `pcb_group.rs`, `pcb_generator.rs`, `generators/*.rs`)

```rust
/// Data members of EDA_GROUP (common/eda_group.h).
#[derive( Clone, Debug, Default )]
pub struct EdaGroupBase {
    /// std::unordered_set<EDA_ITEM*> m_items (no ownership). BTreeSet: iteration in ItemPtr order
    /// replaces the unspecified C++ hash order (deterministic; the writer sorts by uuid anyway).
    pub( crate ) items: BTreeSet<ItemPtr>,
    pub name: String,                              // m_name
    pub design_block_lib_id: LibId,                // m_designBlockLibId
}
#[derive( Clone, Debug )] pub struct PcbGroupBase { pub item: BoardItemBase, pub group: EdaGroupBase }
#[derive( Clone, Debug )] pub struct PcbGroup { pub group: PcbGroupBase }                    // PCB_GROUP_T

/// Data members of PCB_GENERATOR : PCB_GROUP.
#[derive( Clone, Debug )]
pub struct PcbGeneratorBase {
    pub group: PcbGroupBase,                       // BoardItemBase::layer is the generator's layer
    pub( crate ) generator_type: String,           // m_generatorType: "tuning_pattern", "via_stack", "via_stitch"
    pub( crate ) origin: Vector2I,                 // m_origin
    pub( crate ) is_dirty: bool,
    pub( crate ) update_order: i32,
}
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum LengthTuningMode { Single, DiffPair, DiffPairSkew }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum TuningStatus { TooShort = 0, TooLong, Tuned }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ViaStackStyle { Stacked = 0, Staggered = 1 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PcbViaStitchLayout { Plain, Staggered, Poisson }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PcbViaStitchMode { Stitch, Guard }

#[derive( Clone, Debug )]
pub struct PcbTuningPattern {                      // pcbnew/generators/pcb_tuning_pattern.h
    pub generator: PcbGeneratorBase,
    pub( crate ) end: Vector2I,
    pub( crate ) settings: MeanderSettings,
    pub( crate ) base_line: Option<ShapeLineChain>,
    pub( crate ) base_line_coupled: Option<ShapeLineChain>,
    pub( crate ) track_width: i32,
    pub( crate ) assembled_line_width: i32,
    pub( crate ) diff_pair_gap: i32,
    pub( crate ) tuning_mode: LengthTuningMode,
    pub( crate ) last_net_name: String,
    pub( crate ) tuning_info: String,
    pub( crate ) tuning_length: i64,
    pub( crate ) tuning_status: TuningStatus,
    pub( crate ) update_side_from_end: bool,
    pub( crate ) has_rule_target: bool,
}
#[derive( Clone, Debug )]
pub struct PcbViaStack {                           // pcbnew/generators/pcb_via_stack.h
    pub generator: PcbGeneratorBase,
    pub( crate ) start_layer: PcbLayerId, pub( crate ) end_layer: PcbLayerId,
    pub( crate ) style: ViaStackStyle, pub( crate ) pitch: i32,
    pub( crate ) filled: bool, pub( crate ) capped: bool,
    pub( crate ) via_size: i32, pub( crate ) via_drill: i32, pub( crate ) use_netclass: bool,
    pub( crate ) net_code: i32, pub( crate ) preset_name: String,
    pub( crate ) hops: Option<ShapeLineChain>,
}
#[derive( Clone, Debug )]
pub struct PcbViaStitch {                          // pcbnew/generators/pcb_via_stitch.h (PCB_GENERATOR_POLY)
    pub generator: PcbGeneratorBase,
    pub( crate ) outline: ShapePolySet,            // PCB_GENERATOR_POLY::m_outline
    pub( crate ) pitch: i32, pub( crate ) layout: PcbViaStitchLayout, pub( crate ) mode: PcbViaStitchMode,
    pub( crate ) seed: u32,
    pub( crate ) via_template: Option<Box<PcbVia>>,            // std::unique_ptr<PCB_VIA> m_viaTemplate (owned)
    pub( crate ) last_net_name: String, pub( crate ) net_code: i32,
    pub( crate ) last_guarded_net_name: String, pub( crate ) guarded_net_code: i32,
    pub( crate ) excluded_cells: BTreeSet<Vector2I>,
    pub( crate ) excluded_positions: BTreeSet<Vector2I>,
    pub( crate ) child_grid_config: GridConfig,                // libs/kimath grid_geometry.h, plain data
    pub( crate ) origin_offset: Vector2I,
}
// Dropped in all three: status-popup / frame pointers, cached bridging values, m_pendingRemovals,
// m_childContextMenu, m_lastUpdateChangedVias (interactive editing state).

/// Generator property bag: std::map<std::string, wxAny> (include/string_any_map.h), key order.
#[derive( Clone, Debug, PartialEq, Default )]
pub struct StringAnyMap { pub map: BTreeMap<String, AnyValue>, pub iu_scale: f64 }
#[derive( Clone, Debug, PartialEq )]
pub enum AnyValue { Bool( bool ), Int( i32 ), Double( f64 ), String( String ), Vector2I( Vector2I ),
                    LineChain( ShapeLineChain ), PolySet( ShapePolySet ), Points( Vec<Vector2I> ) }
```

Generators are modelled at **data level**: typed members, `Generator::properties()` /
`set_properties()` (ports of `GetProperties` / `SetProperties`, needed for a byte-exact
`(generated ..)` block, I/O risk R9), membership, move/rotate/flip. Regeneration (`Update`,
`EditStart` ...) needs the router and is tier 3.

### 3.13 Remaining item types

```rust
/// PCB_REFERENCE_IMAGE : BOARD_ITEM with REFERENCE_IMAGE / BITMAP_BASE flattened.
#[derive( Clone, Debug )]
pub struct PcbReferenceImage {
    pub item: BoardItemBase,
    pub( crate ) pos: Vector2I,                    // REFERENCE_IMAGE::m_pos (centre)
    pub( crate ) transform_origin_offset: Vector2I,
    pub( crate ) orientation: EdaOrientation,
    pub( crate ) opacity: f64,
    pub( crate ) scale: f64,                       // BITMAP_BASE::m_scale
    pub( crate ) image_data: Arc<[u8]>,            // m_imageData: the original PNG/JPEG bytes, re-emitted verbatim
    pub( crate ) image_type: ImageType,            // m_imageType { Png, Jpeg, Other }
    pub( crate ) pixel_size: Vector2I,             // from the image header (wxImage width/height)
    pub( crate ) pixel_size_iu: f64,               // m_pixelSizeIu
    pub( crate ) ppi: i32,                         // m_ppi
    pub( crate ) legacy_ppi: i32,                  // what bitmap_base.cpp derives for the < 20260623 scale migration
    pub( crate ) image_id: Kiid,                   // m_imageId
}
// Dropped: decoded wxImage / wxBitmap, mirror flags (OpenGL only), preview state.

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum BarcodeT { Code39 = 0, Code128 = 1, DataMatrix = 2, QrCode = 3, MicroQrCode = 4 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum BarcodeEccT { L = 1, M = 2, Q = 3, H = 4 }
#[derive( Clone, Debug, Default )]
pub struct PcbBarcodeCache { pub key_hash: u64, pub poly: ShapePolySet, pub symbol_poly: ShapePolySet,
                             pub text_poly: ShapePolySet, pub bbox: Box2I, pub last_error: String }
/// PCB_BARCODE : BOARD_ITEM (pcbnew/pcb_barcode.h:440-455).
#[derive( Clone, Debug )]
pub struct PcbBarcode {
    pub item: BoardItemBase,
    pub( crate ) width: i32, pub( crate ) height: i32,
    pub( crate ) lib_pos: Vector2I,                // FP-relative in a footprint
    pub( crate ) margin: Vector2I,
    pub( crate ) text: PcbText,                    // PCB_TEXT m_text, owned by value (ParentLink::Item to the barcode)
    pub( crate ) kind: BarcodeT,
    pub( crate ) lib_angle: EdaOrientation,
    pub( crate ) error_correction: BarcodeEccT,
    pub( crate ) cache: Mutex<Option<PcbBarcodeCache>>,
}

/// PCB_POINT : BOARD_ITEM.
#[derive( Clone, Debug )]
pub struct PcbPoint { pub item: BoardItemBase, pub( crate ) lib_pos: Vector2I, pub( crate ) pos: Vector2I, pub( crate ) size: i32 }

/// PCB_TARGET : BOARD_ITEM.
#[derive( Clone, Debug )]
pub struct PcbTarget { pub item: BoardItemBase, pub( crate ) shape: i32 /* bit 0: 0 = +, 1 = X */, pub( crate ) size: i32,
                       pub( crate ) line_width: i32, pub( crate ) pos: Vector2I }

/// RC_ITEM (include/rc_item.h) as data; DRC_ITEM adds the rule and the violating test in kicad-drc.
#[derive( Clone, Debug )]
pub struct RcItem {
    pub error_code: i32, pub error_message: String, pub error_title: String, pub settings_key: String,
    pub ids: Vec<Kiid>,                            // m_ids: main, aux, aux2, aux3 item uuids
    pub rule_name: Option<String>,                 // DRC_ITEM::m_violatingRule->m_Name
}
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum MarkerT { Unspec, Erc, Drc, DrawingSheet, Ratsnest, Parity, Simulation }      // MARKER_BASE::MARKER_T
/// PCB_MARKER : BOARD_ITEM, MARKER_BASE.
#[derive( Clone, Debug )]
pub struct PcbMarker {
    pub item: BoardItemBase,
    pub pos: Vector2I,                             // MARKER_BASE::m_Pos
    pub( crate ) marker_type: MarkerT,
    pub( crate ) excluded: bool,
    pub( crate ) comment: String,
    pub( crate ) rc_item: Arc<RcItem>,             // std::shared_ptr<RC_ITEM> m_rcItem
    pub( crate ) scaling_factor: i32,
    pub( crate ) shape_bounding_box: Box2I,
    pub( crate ) path_shapes: Vec<PcbShape>,       // m_pathShapes
    pub( crate ) path_start: Vector2I, pub( crate ) path_end: Vector2I, pub( crate ) path_length: i32,
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum ExtrusionMaterial { Plastic = 0, Matte, Metal, Copper }
#[derive( Clone, Debug, PartialEq )]
pub struct Extruded3dBody {                        // EXTRUDED_3D_BODY
    pub height: i32, pub standoff: i32, pub layer: PcbLayerId, pub color: Color4D, pub material: ExtrusionMaterial,
    pub show: bool, pub scale: Vector3D, pub rotation: Vector3D, pub offset: Vector3D,
}
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum Body3dKind { Extruded = 0, Model }
/// PCB_3D_BODY : BOARD_ITEM (child of a footprint).
#[derive( Clone, Debug )]
pub struct Pcb3dBody { pub item: BoardItemBase, pub( crate ) kind: Body3dKind, pub( crate ) extruded: Extruded3dBody }

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum PcbGridType { Cartesian = 0, Polar = 1 }
#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub struct PcbGridAffects { pub cursor: bool, pub routing: bool, pub placement: bool }
/// PCB_GRID_ITEM : BOARD_ITEM (no physical presence).
#[derive( Clone, Debug )]
pub struct PcbGridItem {
    pub item: BoardItemBase,
    pub( crate ) type_: PcbGridType, pub( crate ) pos: Vector2I,
    pub( crate ) extent: Vector2I, pub( crate ) spacing: Vector2I,
    pub( crate ) orientation: EdaOrientation, pub( crate ) phi_extent: EdaAngle, pub( crate ) phi_spacing: EdaAngle,
    pub( crate ) color: Color4D, pub( crate ) priority: u32, pub( crate ) tick_interval: u32, pub( crate ) affects: PcbGridAffects,
}

#[derive( Clone, Debug, PartialEq )]
pub struct DrillSpan { pub start_layer: PcbLayerId, pub end_layer: PcbLayerId, pub is_backdrill: bool, pub is_non_plated_file: bool }
/// PCB_DRILL_MAP : BOARD_ITEM (configuration; the symbols are drawn at the holes).
#[derive( Clone, Debug )]
pub struct PcbDrillMap {
    pub item: BoardItemBase,
    pub( crate ) offset: Vector2I,
    pub( crate ) symbol_size: i32, pub( crate ) span: DrillSpan, pub( crate ) all_spans: bool,
    pub( crate ) outline_slots: bool, pub( crate ) guide_cross: bool,
}   // outline cache dropped with drill-symbol rendering (tier 3)

#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum PcbConstraintType { Undefined = 0, Coincident, Horizontal, Vertical, Parallel, Perpendicular, Collinear, Symmetric,
    EqualLength, EqualRadius, PointOnLine, Midpoint, FixedPosition, FixedLength, Concentric, FixedRadius,
    AngularDimension, Tangent, ArcAngle }
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum ConstraintAnchor { Whole = 0, Start, End, Mid, Center, Radius, Vertex }
#[derive( Clone, Debug, PartialEq )]
pub struct ConstraintMember { pub item: Kiid, pub anchor: ConstraintAnchor, pub index: i32 }   // by KIID in C++ too
/// PCB_CONSTRAINT : BOARD_ITEM (layer agnostic).
#[derive( Clone, Debug )]
pub struct PcbConstraint {
    pub item: BoardItemBase,
    pub( crate ) type_: PcbConstraintType, pub( crate ) members: Vec<ConstraintMember>,
    pub( crate ) value: Option<f64>, pub( crate ) driving: bool,
}

/// PCB_BOARD_OUTLINE : BOARD_ITEM. Derived from Edge.Cuts by Board::update_board_outline(); never saved.
#[derive( Clone, Debug )]
pub struct PcbBoardOutline { pub item: BoardItemBase, pub( crate ) outlines: ShapePolySet }
```

### 3.14 Component classes and embedded files (`component_class.rs`, `embedded_files.rs`)

```rust
#[derive( Clone, Copy, Debug, PartialEq, Eq )]
pub enum ComponentClassUsage { Static, Dynamic, StaticAndDynamic, Effective }
#[derive( Clone, Debug )]
pub struct ComponentClass {
    pub( crate ) name: String,
    pub( crate ) constituent_classes: Vec<Arc<ComponentClass>>,      // std::vector<COMPONENT_CLASS*>
    pub( crate ) usage_context: ComponentClassUsage,
}
/// COMPONENT_CLASS_MANAGER. Maps are BTreeMap (unordered in C++; nothing ordered may depend on them).
#[derive( Clone, Debug )]
pub struct ComponentClassManager {
    pub( crate ) none_class: Arc<ComponentClass>,
    pub( crate ) constituent_classes: BTreeMap<String, Arc<ComponentClass>>,
    pub( crate ) effective_classes: BTreeMap<String, Arc<ComponentClass>>,
    pub( crate ) static_class_names_cache: BTreeSet<String>,
    pub( crate ) assignment_rules: Vec<ComponentClassAssignmentRule>,   // tier 2 (dynamic classes need the expression engine)
    pub( crate ) ticker: i64,
}
/// COMPONENT_CLASS_CACHE_PROXY: per-footprint static class + cached effective class.
#[derive( Clone, Debug, Default )]
pub struct ComponentClassCacheProxy {
    pub( crate ) static_component_class: Option<Arc<ComponentClass>>,
    pub( crate ) final_component_class: Mutex<Option<( i64, Arc<ComponentClass> )>>,   // ( ticker, class )
}

#[derive( Clone, Copy, Debug, PartialEq, Eq )] pub enum EmbeddedFileType { Font, Model, Worksheet, Datasheet, Other }
#[derive( Clone, Debug, PartialEq )]
pub struct EmbeddedFile {
    pub name: String, pub type_: EmbeddedFileType, pub is_valid: bool,
    pub compressed_encoded_data: String,           // base64( zstd ), written back verbatim
    pub decompressed_data: Vec<u8>,                // filled on demand (zstd decoder)
    pub data_hash: String,
}
#[derive( Clone, Debug, Default )]
pub struct EmbeddedFiles {
    pub( crate ) files: BTreeMap<String, Arc<EmbeddedFile>>,   // m_files (by name)
    pub( crate ) font_files: Vec<String>,                      // m_fontFiles
    pub( crate ) embed_fonts: bool,                            // m_embedFonts
}   // m_fileAddedCallback dropped
```

### 3.15 Modelling level per type; why nothing is an opaque s-expression

| Level | Types | Meaning |
|---|---|---|
| **Full** (data + all non-GUI behaviour) | `Board`, `Footprint`, `Pad`, `Padstack`, `PcbTrack`, `PcbArc`, `PcbVia`, `Zone`, `PcbShape`, `PcbText`, `PcbField`, `PcbTextbox`, `PcbTable`, `PcbTablecell`, the five dimensions, `PcbGroup`, `PcbMarker`, `PcbTarget`, `PcbPoint`, `NetinfoItem`, `Netclass`, `NetSettings`, `BoardDesignSettings`, `ZoneSettings`, `BoardStackup`, `PcbPlotParams`, `EmbeddedFiles`, `PcbBoardOutline` | Read, write byte-exact, shapes, DRC, plotting. |
| **Data** (all fields typed, read/write byte-exact, trivial or conservative behaviour) | `PcbTuningPattern`, `PcbViaStack`, `PcbViaStitch` (members are ordinary tracks/vias and are fully handled; no regeneration), `PcbConstraint` (no solver), `PcbGridItem` (no physical presence), `Pcb3dBody` (3D only), `PcbReferenceImage` (bytes kept; bbox from the image header; not plotted to Gerber in C++ either), `PcbDrillMap` (configuration; drill symbols not rendered), `PcbDrillChart` / `PcbStackupTable` (cells are ordinary `PcbTablecell`s read from the file and are plotted and checked like any table; `RefreshGeneratedTables` is not ported), `PcbBarcode` (text part rendered; the symbol polygon needs a port of the encoders KiCad takes from zint: until then `effective_shape` is the bounding rectangle and the plotter emits a warning), `ComponentClassManager` (static classes; dynamic rules tier 2) | Consequences: re-save is byte exact; Gerber output differs from the oracle only for barcodes (missing symbol), drill maps (missing symbols) and stale generated tables; DRC treats them through the generic `BoardItem` interface (bounding box, layer set), so clearance results involving a barcode symbol are conservative. All recorded in `PORT_STATUS.md`. |
| **Opaque** | none | see below |

Opaque pass-through of unmodelled constructs (keeping the parsed `Sexpr` and printing it back) was
considered and rejected for board items, for three reasons found in the I/O analysis: (1) the C++
parser is strict - an unknown token is a `PARSE_ERROR` - so a valid file never contains anything
the model does not know; (2) the writer re-formats every number and re-quotes every string, so an
opaque subtree is byte exact only if the input was already normalised by the same KiCad version;
(3) every item takes part in the writer's ordering (`Type()`, layer set, uuid, per-type compare)
and in uuid indexing, so the "opaque" item would need typed fields anyway. `Sexpr` is used in
exactly one place, `ProjectData::tuning_profiles` (tier 3 data nobody reads yet). As a *staging*
aid, WP MODEL-17 may land `PcbGridItem`, `PcbDrillMap` and the generated-table configuration with
their bodies held as `Sexpr` until their parser packages (PCBIO-12) are done; boards containing
them are then on the corpus skip list.

### 3.16 Members and interfaces dropped from every class

`KIGFX::VIEW_ITEM` (`ViewGetLayers`, `ViewBBox`, `ViewGetLOD`), `GetMsgPanelInfo`, `GetMenuImage`,
`GetFriendlyName`, `Matches` / `Replace` (find dialog), `Visit` / `IterateForward` (collectors;
replaced by iterators and `run_on_children`), `SERIALIZABLE` (`Serialize` / `Deserialize`
protobuf), the `PROPERTY_MANAGER` descriptor structs at the end of each `.cpp` (the property table
needed by custom-rule expressions is a separate tier-2 package on top of the model),
`swapData` / `SwapItemData` (undo by swap; `BoardCommit` restores clones instead), `Duplicate`
(GUI; `clone_item` + fresh uuid), `GetCoverageArea` (selection ranking), `Show` (debug dump),
`HIGH_LIGHT_INFO`. Each crate's `PORT_STATUS.md` lists them as `skipped: GUI`.

---------------------------------------------------------------------------------------------------

## 4. Shared behaviour API

All traits are object safe. "provided" = has a default body, which is the C++ base-class
implementation; "required" = generated by `board_item_plumbing!` or written per type. Method names
are the C++ names in snake_case with `Get` dropped (PORTING.md section 2).

### 4.1 `EdaItem` (port of the `EDA_ITEM` interface)

```rust
pub trait EdaItem: Any + Send + Sync + std::fmt::Debug {
    // ---- required plumbing ----
    fn eda( &self ) -> &EdaItemBase;
    fn eda_mut( &mut self ) -> &mut EdaItemBase;
    fn as_any( &self ) -> &dyn Any;
    fn as_any_mut( &mut self ) -> &mut dyn Any;
    fn into_any( self: Box<Self> ) -> Box<dyn Any>;
    fn class_name( &self ) -> &'static str;                               // GetClass(): "PCB_VIA", "PAD", ...

    // ---- provided: non-virtual EDA_ITEM members ----
    fn type_( &self ) -> KicadT { self.eda().struct_type }                // Type()
    fn uuid( &self ) -> &Kiid { &self.eda().uuid }                        // m_Uuid
    fn parent( &self ) -> &ParentLink { &self.eda().parent }              // GetParent()
    fn parent_group( &self ) -> Option<ItemPtr> { self.eda().group }      // GetParentGroup()
    fn set_parent_group( &mut self, group: Option<ItemPtr> ) { self.eda_mut().group = group; }
    fn flags( &self ) -> u32 { self.eda().flags.get() }                   // GetFlags()
    fn set_flags( &self, mask: u32 ) { self.eda().flags.set( mask ) }     // SetFlags()   (note: &self)
    fn clear_flags( &self, mask: u32 ) { self.eda().flags.clear( mask ) }
    fn has_flag( &self, flag: u32 ) -> bool { self.eda().flags.has( flag ) }
    fn clear_edit_flags( &self );   fn clear_temp_flags( &self );
    fn custom_properties( &self ) -> &BTreeMap<String, String> { &self.eda().custom_properties }

    // ---- virtuals ----
    fn is_type( &self, scan_types: &[KicadT] ) -> bool;                   // provided: BaseType match + PCB_LOCATE_* rules
    fn position( &self ) -> Vector2I { Vector2I::new( 0, 0 ) }            // GetPosition()
    fn set_position( &mut self, _pos: Vector2I ) {}                       // SetPosition()
    fn focus_position( &self ) -> Vector2I { self.position() }            // GetFocusPosition()
    fn sort_position( &self ) -> Vector2I { self.position() }             // GetSortPosition()
}
```

### 4.2 `BoardItem` (port of the `BOARD_ITEM` interface)

```rust
pub trait BoardItem: EdaItem {
    // ---- required plumbing ----
    fn base( &self ) -> &BoardItemBase;
    fn base_mut( &mut self ) -> &mut BoardItemBase;
    fn as_item_ref( &self ) -> ItemRef<'_>;                               // typed view (2.7)
    fn as_item_mut( &mut self ) -> ItemMut<'_>;
    fn clone_item( &self ) -> Box<dyn BoardItem>;                         // Clone()
    fn eq_item( &self, other: &dyn BoardItem ) -> bool;                   // virtual operator==
    fn similarity( &self, ctx: BoardCtx<'_>, other: &dyn BoardItem ) -> f64;   // Similarity()

    // ---- identity and structure (provided) ----
    fn ptr( &self ) -> ItemPtr { self.base().ptr }                        // `this` as identity / map key
    fn is_indexed_in_board( &self ) -> bool { self.base().attached }      // IsIndexedInBoard()
    fn parent_footprint_ptr( &self ) -> ItemPtr { self.parent().fp_ptr() }
    fn parent_footprint<'a>( &self, ctx: BoardCtx<'a> ) -> Option<&'a Footprint>;       // GetParentFootprint()
    fn is_groupable_type( &self ) -> bool;                                // IsGroupableType()
    fn fp_relative_position( &self ) -> Vector2I;                         // GetFPRelativePosition(): InverseApply( position )
    fn set_fp_relative_position( &mut self, pos: Vector2I );

    // ---- class tests (virtuals; overridden by the plumbing of the classes concerned) ----
    fn is_connected( &self ) -> bool { false }                            // IsConnected()
    fn as_connected( &self ) -> Option<&dyn BoardConnectedItem> { None }
    fn as_connected_mut( &mut self ) -> Option<&mut dyn BoardConnectedItem> { None }
    fn as_eda_text( &self ) -> Option<&dyn EdaText> { None }
    fn as_eda_shape( &self ) -> Option<&dyn EdaShape> { None }
    fn as_container( &self ) -> Option<&dyn BoardItemContainer> { None }

    // ---- layers ----
    fn layer( &self ) -> PcbLayerId { self.base().layer }                 // GetLayer()
    fn set_layer( &mut self, layer: PcbLayerId ) { self.base_mut().layer = layer; }
    fn layer_set( &self ) -> Lset;                                        // GetLayerSet(): { m_layer } or empty
    fn set_layer_set( &mut self, layers: &Lset );                         // SetLayerSet()
    fn is_on_layer( &self, layer: PcbLayerId ) -> bool { self.base().layer == layer }
    fn is_on_copper_layer( &self ) -> bool;                               // provided: single-layer test or scan of layer_set()
    fn is_side_specific( &self ) -> bool;
    fn is_layer_agnostic( &self ) -> bool { false }
    fn fits_enabled_layers( &self, enabled_layers: &Lset, copper_layer_count: i32 ) -> bool;
    fn flipped_layer( &self, env: EnvCtx<'_>, layer: PcbLayerId ) -> PcbLayerId { env.flip_layer( layer ) }
    fn layer_name( &self, ctx: BoardCtx<'_> ) -> String { ctx.env().layer_name( self.layer() ) }
    fn layer_mask_describe( &self, ctx: BoardCtx<'_> ) -> String;

    // ---- state ----
    fn is_locked( &self, ctx: BoardCtx<'_> ) -> bool;                     // IsLocked(): parent group, board use, m_isLocked
    fn set_locked( &mut self, locked: bool ) { self.base_mut().is_locked = locked; }
    fn is_knockout( &self ) -> bool { self.base().is_knockout }
    fn set_is_knockout( &mut self, knockout: bool ) { self.base_mut().is_knockout = knockout; }
    fn has_hole( &self ) -> bool { false }
    fn has_drilled_hole( &self ) -> bool { false }
    fn is_tented( &self, _ctx: BoardCtx<'_>, _layer: PcbLayerId ) -> bool { false }
    fn has_line_stroke( &self ) -> bool { false }
    fn stroke( &self ) -> StrokeParams;                                   // GetStroke(): wxFAIL default
    fn set_stroke( &mut self, stroke: &StrokeParams );
    fn max_error( &self, ctx: BoardCtx<'_> ) -> i32 { ctx.env().max_error() }           // GetMaxError()
    fn board_copper_layer_count( &self, ctx: BoardCtx<'_> ) -> i32 { ctx.env().copper_layer_count() }
    fn board_layer_set( &self, ctx: BoardCtx<'_> ) -> Lset { ctx.env().layer_set() }

    // ---- geometry ----
    fn center( &self, ctx: BoardCtx<'_> ) -> Vector2I { self.bounding_box( ctx ).centre() }   // GetCenter()
    fn bounding_box( &self, ctx: BoardCtx<'_> ) -> Box2I;                 // GetBoundingBox()
    fn hit_test( &self, ctx: BoardCtx<'_>, position: Vector2I, accuracy: i32 ) -> bool;
    fn hit_test_rect( &self, ctx: BoardCtx<'_>, rect: &Box2I, contained: bool, accuracy: i32 ) -> bool;
    fn hit_test_poly( &self, ctx: BoardCtx<'_>, poly: &ShapeLineChain, contained: bool ) -> bool;
    /// GetEffectiveShape( aLayer = UNDEFINED_LAYER, aFlash = FLASHING::DEFAULT, aUsage = NULL_CONSTRAINT ).
    fn effective_shape( &self, ctx: BoardCtx<'_>, layer: PcbLayerId, flash: Flashing, usage: DrcConstraintT )
            -> Arc<dyn Shape>;
    /// GetEffectiveHoleShape(); `None` where C++ returns a null pointer after wxFAIL.
    fn effective_hole_shape( &self, ctx: BoardCtx<'_>, layer: PcbLayerId, usage: DrcConstraintT )
            -> Option<Arc<ShapeSegment>>;
    fn transform_shape_to_polygon( &self, ctx: BoardCtx<'_>, buffer: &mut ShapePolySet, layer: PcbLayerId,
                                   clearance: i32, error: i32, error_loc: ErrorLoc, ignore_line_width: bool );
    fn transform_shape_to_poly_set( &self, ctx: BoardCtx<'_>, buffer: &mut ShapePolySet, layer: PcbLayerId,
                                    clearance: i32, error: i32, error_loc: ErrorLoc );

    // ---- transformations (mutators; `flip` needs the board's layer pairing) ----
    fn move_( &mut self, move_vector: Vector2I );                         // Move()
    fn rotate( &mut self, rot_centre: Vector2I, angle: EdaAngle );
    fn flip( &mut self, env: EnvCtx<'_>, centre: Vector2I, flip_direction: FlipDirection );
    fn mirror( &mut self, centre: Vector2I, flip_direction: FlipDirection );
    fn mirror_axis( &mut self, axis: &Seg );                              // Mirror( const SEG& )
    fn normalize( &mut self ) {}
    fn normalize_for_compare( &mut self ) { self.normalize() }
    fn on_footprint_transformed( &mut self, link: &Arc<FpLink> );         // provided: store the link (2.5)
    fn on_footprint_rescaled( &mut self, link: &Arc<FpLink>, ratio_x: f64, ratio_y: f64, linear_factor: f64,
                              anchor: Vector2I, parent_rotate: EdaAngle );
    fn style_from_settings( &mut self, _settings: &BoardDesignSettings, _check_side: bool ) {}
    fn remap_kiids( &mut self, _id_map: &BTreeMap<Kiid, Kiid> ) {}

    // ---- children ----
    fn run_on_children( &self, _f: &mut dyn FnMut( &dyn BoardItem ), _mode: RecurseMode ) {}
    fn run_on_children_mut( &mut self, _f: &mut dyn FnMut( &mut dyn BoardItem ), _mode: RecurseMode ) {}

    // ---- reports ----
    /// GetItemDescription( UNITS_PROVIDER*, bool aFull ): the text printed for every item in DRC reports.
    fn item_description( &self, ctx: BoardCtx<'_>, units: &dyn UnitsProvider, full: bool ) -> String;
    fn parent_as_string( &self, ctx: BoardCtx<'_> ) -> String;            // GetParentAsString()
}

/// BOARD_ITEM_CONTAINER (FOOTPRINT, PCB_TABLE; the BOARD itself is not a BoardItem in the port).
pub trait BoardItemContainer: BoardItem {
    fn add( &mut self, item: Box<dyn BoardItem>, mode: AddMode ) -> Result<ItemPtr, Box<dyn BoardItem>>;
    fn remove( &mut self, ptr: ItemPtr, mode: RemoveMode ) -> Option<Box<dyn BoardItem>>;
}

/// BOARD_ITEM::ptr_cmp: Type(), GetLayerSet().Seq() as vectors, m_Uuid, then identity.
pub fn ptr_cmp( a: &dyn BoardItem, b: &dyn BoardItem ) -> std::cmp::Ordering;
pub fn same_item( a: &dyn BoardItem, b: &dyn BoardItem ) -> bool { a.ptr() == b.ptr() }
```

### 4.3 `BoardConnectedItem`

```rust
pub trait BoardConnectedItem: BoardItem {
    fn conn( &self ) -> &BoardConnectedItemBase;                          // required
    fn conn_mut( &mut self ) -> &mut BoardConnectedItemBase;

    fn net_code( &self ) -> i32 { self.conn().net_code }                  // GetNetCode()
    fn net<'a>( &self, ctx: BoardCtx<'a> ) -> Option<&'a NetinfoItem>;    // GetNet(): board net, None if code < 0 or no board
    fn set_net( &mut self, net: Option<&NetinfoItem> );                   // virtual SetNet()
    /// virtual SetNetCode( aNetCode, aNoAssert ): 0 if not on copper; unknown code -> -1; no board -> 0.
    fn set_net_code( &mut self, env: EnvCtx<'_>, net_code: i32, no_assert: bool ) -> bool;
    fn netname( &self, ctx: BoardCtx<'_> ) -> String;                     // GetNetname()
    fn netname_msg( &self, ctx: BoardCtx<'_> ) -> String;
    fn short_netname( &self, ctx: BoardCtx<'_> ) -> String;
    fn display_netname( &self, ctx: BoardCtx<'_> ) -> String;
    /// virtual GetEffectiveNetClass(): the net's class, else the board default, else the static fallback.
    fn effective_net_class<'a>( &self, ctx: BoardCtx<'a> ) -> &'a Netclass;
    fn net_class_name( &self, ctx: BoardCtx<'_> ) -> String { self.effective_net_class( ctx ).name().to_string() }
    /// virtual GetOwnClearance( aLayer, wxString* aSource ): DRC hook `cached_own_clearance`, 0 without engine.
    fn own_clearance( &self, ctx: BoardCtx<'_>, layer: PcbLayerId ) -> ( i32, Option<String> );
    /// virtual GetClearanceOverrides( wxString* aSource ): pads (own value, else the footprint's).
    fn clearance_overrides( &self, _ctx: BoardCtx<'_> ) -> Option<( i32, String )> { None }
    fn local_clearance( &self ) -> Option<i32> { None }                   // virtual GetLocalClearance()
    fn local_clearance_with_source( &self, _ctx: BoardCtx<'_> ) -> Option<( i32, String )> { None }
    fn teardrop_params( &self ) -> &TeardropParameters { &self.conn().teardrop_params }
    fn teardrop_params_mut( &mut self ) -> &mut TeardropParameters { &mut self.conn_mut().teardrop_params }
    fn local_ratsnest_visible( &self ) -> bool { self.conn().local_ratsnest_visible }
}
```

### 4.4 Family and mixin traits (accessor + the methods consumers call; full lists follow the headers)

```rust
/// PCB_TRACK family: PcbTrack, PcbArc, PcbVia.
pub trait Track: BoardConnectedItem {
    fn track( &self ) -> &PcbTrackBase;                                   // required
    fn track_mut( &mut self ) -> &mut PcbTrackBase;
    fn start( &self ) -> Vector2I { self.track().start }                  fn set_start( &mut self, p: Vector2I ) { self.track_mut().start = p; }
    fn end( &self ) -> Vector2I { self.track().end }                      fn set_end( &mut self, p: Vector2I ) { self.track_mut().end = p; }
    fn end_point( &self, which: EndpointT ) -> Vector2I;
    fn width( &self ) -> i32 { self.track().width }                       // virtual GetWidth() (via: front-layer diameter)
    fn set_width( &mut self, width: i32 ) { self.track_mut().width = width; }           // virtual SetWidth()
    fn length( &self ) -> f64;                                            // virtual GetLength()
    fn delay( &self, ctx: BoardCtx<'_> ) -> f64;                          // virtual GetDelay()
    fn width_constraint( &self, ctx: BoardCtx<'_> ) -> ( MinOptMax<i32>, Option<String> );   // virtual, DRC hook
    fn has_solder_mask( &self ) -> bool;                                  fn local_solder_mask_margin( &self ) -> Option<i32>;
    fn solder_mask_expansion( &self, ctx: BoardCtx<'_> ) -> i32;
    fn is_point_on_ends( &self, point: Vector2I, min_dist: i32 ) -> u32;  // STARTPOINT | ENDPOINT flags
    fn is_null( &self ) -> bool;
    fn approx_collinear( &self, other: &dyn Track ) -> bool;
}
/// PCB_TRACK::cmp_tracks: net code, layer, type (trace < via < arc as KICAD_T), uuid, identity.
pub fn cmp_tracks( a: &dyn Track, b: &dyn Track ) -> std::cmp::Ordering;

/// EDA_SHAPE interface. Provided methods are the ports of the EDA_SHAPE member functions; the
/// ones marked (v) are virtual in C++ and overridden by the PCB_SHAPE family.
pub trait EdaShape {
    fn eda_shape( &self ) -> &EdaShapeBase;                               // required
    fn eda_shape_mut( &mut self ) -> &mut EdaShapeBase;
    fn shape( &self ) -> ShapeT;            fn start( &self ) -> Vector2I;       fn end( &self ) -> Vector2I;
    fn set_start( &mut self, p: Vector2I ); /* (v): PCB_SHAPE calls sync_lib_coords */   fn set_end( &mut self, p: Vector2I ); // (v)
    fn width( &self ) -> i32;               // (v) PCB_SHAPE: library width scaled by the footprint
    fn effective_width( &self ) -> i32;     fn stroke_params( &self ) -> &StrokeParams;
    fn is_solid_fill( &self ) -> bool;      fn fill_mode( &self ) -> FillT;      fn is_closed( &self ) -> bool;
    fn arc_mid( &self ) -> Vector2I;        fn arc_angle( &self ) -> EdaAngle;   fn radius( &self ) -> i32;      fn center_shape( &self ) -> Vector2I;
    fn rect_corners( &self ) -> Vector<Vector2I>;   fn corner_radius( &self ) -> i32;
    fn poly_shape( &self ) -> &ShapePolySet;        fn bezier_points( &self ) -> &[Vector2I];
    fn is_proxy_item( &self ) -> bool;      // (v)
    fn shape_max_error( &self ) -> i32;     // (v) getMaxError(): 100 for EDA_SHAPE; PCB_SHAPE asks the board (ctx stored by caller, see below)
    fn hatching_knockouts( &self, ctx: BoardCtx<'_> ) -> ShapePolySet;    // (v) getHatchingKnockouts()
    fn make_effective_shapes( &self, max_error: i32, edge_only: bool, line_chain_only: bool, hittesting: bool ) -> Vec<Box<dyn Shape>>;
    fn shape_transform_to_polygon( &self, buffer: &mut ShapePolySet, clearance: i32, error: i32, error_loc: ErrorLoc,
                                   ignore_line_width: bool, include_fill: bool );
    fn shape_bounding_box( &self ) -> Box2I;
    fn shape_compare( &self, other: &dyn EdaShape ) -> i32;               // EDA_SHAPE::Compare
}
/// PCB_SHAPE family: PcbShape, PcbTextbox, PcbTablecell.
pub trait ShapeItem: BoardConnectedItem + EdaShape {
    fn pcb_shape( &self ) -> &PcbShapeBase;                               // required
    fn pcb_shape_mut( &mut self ) -> &mut PcbShapeBase;
    fn library_start( &self ) -> Vector2I;  fn library_end( &self ) -> Vector2I;  fn library_shape( &self ) -> ShapeT;   // + the other GetLibrary* accessors
    fn rebake_from_lib( &mut self );        fn override_lib_coords( &mut self, start: Vector2I, end: Vector2I, arc_mid: Vector2I );
    fn connection_points( &self ) -> Vec<Vector2I>;
    fn has_solder_mask( &self ) -> bool;    fn solder_mask_expansion( &self, ctx: BoardCtx<'_> ) -> i32;
    fn compare( &self, other: &dyn ShapeItem ) -> i32;                    // PCB_SHAPE::Compare (writer ordering)
}

/// EDA_TEXT interface.
pub trait EdaText {
    fn eda_text( &self ) -> &EdaTextBase;                                 // required
    fn eda_text_mut( &mut self ) -> &mut EdaTextBase;
    fn text( &self ) -> &str;               fn set_text( &mut self, text: &str );          // (v)
    /// (v) GetShownText( RESOLUTION_CONTEXT, aDepth ): PCB_TEXT expands ${..} through footprint, board, project.
    fn shown_text( &self, ctx: BoardCtx<'_>, context: ResolutionContext, depth: i32 ) -> String;
    fn text_pos( &self ) -> Vector2I;       fn set_text_pos( &mut self, p: Vector2I );     // (v) PCB_TEXT: library <-> board via FpLink
    fn text_size( &self ) -> Vector2I;      fn set_text_size( &mut self, size: Vector2I, enforce_min: bool );   // (v)
    fn text_thickness( &self ) -> i32;      fn set_text_thickness( &mut self, w: i32 );    // (v)
    fn text_angle( &self ) -> EdaAngle;     fn set_text_angle( &mut self, a: EdaAngle );   // (v)
    fn effective_text_pen_width( &self, default_pen_width: i32 ) -> i32;
    fn is_visible( &self ) -> bool;         fn set_visible( &mut self, visible: bool );    // (v)
    fn is_bold( &self ) -> bool;  fn is_italic( &self ) -> bool;  fn is_mirrored( &self ) -> bool;  fn is_keep_upright( &self ) -> bool;
    fn horiz_justify( &self ) -> GrTextHAlignT;   fn vert_justify( &self ) -> GrTextVAlignT;
    fn attributes( &self ) -> &TextAttributes;    fn font( &self ) -> Option<&Font>;
    fn draw_rotation( &self ) -> EdaAngle;  fn draw_pos( &self ) -> Vector2I;              // (v)
    fn text_box( &self, ctx: BoardCtx<'_>, line: i32 ) -> Box2I;                           // GetTextBox
    fn effective_text_shape( &self, ctx: BoardCtx<'_>, triangulate: bool, bbox: &Box2I, corner_angle: EdaAngle ) -> Arc<ShapeCompound>;
    fn text_compare( &self, other: &dyn EdaText ) -> i32;                                  // EDA_TEXT::Compare
}
pub trait TextItem: BoardItem + EdaText {            // PCB_TEXT family: PcbText, PcbField, dimensions
    fn pcb_text( &self ) -> &PcbTextBase;            fn pcb_text_mut( &mut self ) -> &mut PcbTextBase;
    fn lib_text_angle( &self ) -> EdaAngle;          fn set_lib_text_pos( &mut self, p: Vector2I );   fn set_lib_text_size( &mut self, s: Vector2I );
    fn keep_upright( &mut self );                    fn text_type_description( &self ) -> String;   // (v)
    fn transform_text_to_poly_set( &self, ctx: BoardCtx<'_>, buffer: &mut ShapePolySet, clearance: i32, max_error: i32, error_loc: ErrorLoc );
    fn knockout_cache( &self, ctx: BoardCtx<'_>, max_error: i32 ) -> ShapePolySet;
}
pub trait TextboxItem: ShapeItem + EdaText {         // PCB_TEXTBOX family: PcbTextbox, PcbTablecell
    fn textbox( &self ) -> &PcbTextboxBase;          fn textbox_mut( &mut self ) -> &mut PcbTextboxBase;
    fn is_border_enabled( &self ) -> bool;           fn margins( &self ) -> ( i32, i32, i32, i32 );
    fn corners_in_sequence( &self, angle: EdaAngle ) -> Vec<Vector2I>;   fn draw_pos_textbox( &self ) -> Vector2I;
}
pub trait Dimension: TextItem {                      // PCB_DIMENSION_BASE family
    fn dim( &self ) -> &PcbDimensionBase;            fn dim_mut( &mut self ) -> &mut PcbDimensionBase;
    fn dim_start( &self ) -> Vector2I;               fn dim_end( &self ) -> Vector2I;               // board frame (v)
    fn library_start( &self ) -> Vector2I;           fn library_end( &self ) -> Vector2I;
    fn update( &mut self, env: EnvCtx<'_> );         // Update() = update_geometry() + update_text()
    fn update_geometry( &mut self );                 // pure virtual updateGeometry()
    fn update_text( &mut self, env: EnvCtx<'_> );    // virtual updateText()
    fn value_text( &self, env: EnvCtx<'_> ) -> String;   fn measured_value( &self ) -> i32;
    fn shapes( &self ) -> &[Arc<dyn Shape>];         fn line_thickness( &self ) -> i32;
}
pub trait EdaGroup {
    fn eda_group( &self ) -> &EdaGroupBase;          fn eda_group_mut( &mut self ) -> &mut EdaGroupBase;
    fn items( &self ) -> &BTreeSet<ItemPtr>;         fn contains_item( &self, ptr: ItemPtr ) -> bool;
    fn name( &self ) -> &str;                        fn group_member_ids( &self, ctx: BoardCtx<'_> ) -> Vec<Kiid>;
}
pub trait GroupItem: BoardItem + EdaGroup {          // PCB_GROUP family
    fn members_layer_set( &self, ctx: BoardCtx<'_> ) -> Lset;        // PCB_GROUP::GetLayerSet (deviation D-GRP)
    fn members_bounding_box( &self, ctx: BoardCtx<'_> ) -> Box2I;    fn members_position( &self, ctx: BoardCtx<'_> ) -> Vector2I;
    fn is_on_layer_members( &self, ctx: BoardCtx<'_>, layer: PcbLayerId ) -> bool;
}
pub trait Generator: GroupItem {                     // PCB_GENERATOR family
    fn generator( &self ) -> &PcbGeneratorBase;      fn generator_mut( &mut self ) -> &mut PcbGeneratorBase;
    fn generator_type( &self ) -> &str;              fn layer_follows_members( &self ) -> bool { true }
    fn properties( &self ) -> StringAnyMap;          fn set_properties( &mut self, props: &StringAnyMap );
    fn template_items( &self ) -> Vec<( String, &dyn BoardItem )>;   fn set_template_item( &mut self, name: &str, item: Box<dyn BoardItem> );
}
pub trait TableItem: BoardItemContainer {            // PCB_TABLE family
    fn table( &self ) -> &PcbTableBase;              fn table_mut( &mut self ) -> &mut PcbTableBase;
    fn cells( &self ) -> &[Box<PcbTablecell>];       fn col_count( &self ) -> i32;   fn row_count( &self ) -> i32;
    fn col_width( &self, col: i32 ) -> i32;          fn row_height( &self, row: i32 ) -> i32;
    fn table_compare( &self, other: &dyn TableItem ) -> i32;        // PCB_TABLE::Compare
}
```

Note on `EdaShape::shape_max_error` and the other mixin virtuals that reach the board in C++
(`PCB_SHAPE::getMaxError()` -> `GetBoard()`): mixin methods that need it take the value as a
parameter (`make_effective_shapes( max_error, .. )`); the `BoardItem` method that calls them
passes `ctx.env().max_error()`. No mixin method stores a context.

### 4.5 What the plumbing macro generates per concrete type

`board_item_plumbing!( Type, ItemRef::Variant, class = "..", base = path, [conn = path], [text = path], [shape = path] )`
emits `EdaItem::{eda, eda_mut, as_any, as_any_mut, into_any, class_name}`,
`BoardItem::{base, base_mut, as_item_ref, as_item_mut, clone_item}`, the `as_connected` /
`as_eda_text` / `as_eda_shape` / `as_container` overrides and the family accessors
(`Track::track`, `ShapeItem::pcb_shape`, ...). `impl_virtuals!` (2.1, M4) emits the remaining trait
methods as one-line forwards. The two macros are the only "magic" in the crate; both are
`macro_rules!`, expand to plain trait impls, and are part of the kernel work package.

---------------------------------------------------------------------------------------------------

## 5. Board-level services

```rust
impl Board {
    // ---- construction ----
    pub fn new() -> Board;                                   // BOARD(): default layers (2 Cu), settings, net 0
    pub fn deep_copy( &self ) -> Board;                      // explicit copy without hooks / journal

    // ---- containers, in C++ order (5.1) ----
    pub fn tracks( &self ) -> impl DoubleEndedIterator<Item = &dyn Track> + ExactSizeIterator + Clone;   // Tracks()
    pub fn footprints( &self ) -> impl DoubleEndedIterator<Item = &Footprint> + ExactSizeIterator + Clone;
    pub fn drawings( &self ) -> impl DoubleEndedIterator<Item = &dyn BoardItem> + ExactSizeIterator + Clone;
    pub fn zones( &self ) -> impl DoubleEndedIterator<Item = &Zone> + ExactSizeIterator + Clone;
    pub fn generators( &self ) -> impl Iterator<Item = &dyn Generator>;
    pub fn groups( &self ) -> impl Iterator<Item = &PcbGroup>;
    pub fn points( &self ) -> impl Iterator<Item = &PcbPoint>;
    pub fn markers( &self ) -> impl Iterator<Item = &PcbMarker>;
    pub fn constraints( &self ) -> impl Iterator<Item = &PcbConstraint>;
    pub fn board_outline( &self ) -> Option<&PcbBoardOutline>;
    pub fn area( &self, index: usize ) -> Option<&Zone>;     // GetArea( index )
    pub fn first_footprint( &self ) -> Option<&Footprint>;
    pub fn is_empty( &self ) -> bool;

    // ---- derived iterations ----
    pub fn all_connected_items( &self ) -> Vec<&dyn BoardConnectedItem>;   // AllConnectedItems(), C++ order
    pub fn run_on_children( &self, f: &mut dyn FnMut( &dyn BoardItem ), mode: RecurseMode );
    pub fn item_set( &self ) -> Vec<&dyn BoardItem>;         // GetItemSet(): sorted by uuid (CompareByUuid)
    pub fn pads( &self ) -> Vec<&Pad>;                       // GetPads()
    pub fn zone_list( &self, include_zones_in_footprints: bool ) -> Vec<&Zone>;   // GetZoneList()
    pub fn tracks_in_net( &self, net_code: i32 ) -> Vec<&dyn Track>;

    // ---- lookup ----
    pub fn item( &self, ptr: ItemPtr ) -> Option<&dyn BoardItem>;
    pub fn resolve_item( &self, id: &Kiid ) -> Option<&dyn BoardItem>;     // ResolveItem( id, true )
    pub fn resolve_ptr( &self, id: &Kiid ) -> Option<ItemPtr>;
    pub fn is_item_indexed_by_id( &self, id: &Kiid ) -> bool;
    pub fn footprint( &self, ptr: ItemPtr ) -> Option<&Footprint>;         // + track(), via(), zone(), pad(), group()
    pub fn find_footprint_by_reference( &self, reference: &str ) -> Option<&Footprint>;
    pub fn find_footprint_by_path( &self, path: &KiidPath ) -> Option<&Footprint>;
    pub fn find_pad_by_uuid( &self, id: &Kiid ) -> Option<&Pad>;
    pub fn pad_at( &self, position: Vector2I, layer_mask: &Lset ) -> Option<&Pad>;      // GetPad( pos, mask )
    pub fn repair_duplicate_item_uuids( &mut self ) -> i32;
    pub fn groups_sanity_check( &mut self, repair: bool ) -> String;

    // ---- nets and netclasses ----
    pub fn net_info( &self ) -> &NetinfoList;                              // GetNetInfo(): iterates by NAME
    pub fn find_net( &self, net_code: i32 ) -> Option<&NetinfoItem>;       // FindNet( int )
    pub fn find_net_by_name( &self, name: &str ) -> Option<&NetinfoItem>;  // FindNet( wxString )
    pub fn find_or_add_net( &mut self, name: &str, net_code: i32 ) -> i32; // FindOrAddNet -> code
    pub fn add_net( &mut self, net: NetinfoItem ) -> i32;                  // Add( NETINFO_ITEM ): AppendNet code rules
    pub fn remove_net( &mut self, net_code: i32 );                         // items of the net go to net 0
    pub fn net_count( &self ) -> usize;
    pub fn build_list_of_nets( &mut self );
    pub fn synchronize_nets_and_net_classes( &mut self, reset_track_and_via_sizes: bool );
    pub fn set_areas_net_codes_from_net_names( &mut self ) -> i32;
    pub fn sanitize_netcodes( &mut self );
    pub fn match_dp_suffix( &self, net_name: &str ) -> ( i32, String );    // MatchDpSuffix
    pub fn dp_coupled_net( &self, net_code: i32 ) -> Option<&NetinfoItem>;
    pub fn net_class_assignment_candidates( &self ) -> BTreeSet<String>;

    // ---- settings ----
    pub fn design_settings( &self ) -> &BoardDesignSettings;
    pub fn design_settings_mut( &mut self ) -> &mut BoardDesignSettings;
    pub fn stackup_or_default( &self ) -> BoardStackup;
    pub fn max_clearance_value( &self ) -> i32;                            // cached per time stamp
    pub fn plot_options( &self ) -> &PcbPlotParams;   pub fn page_settings( &self ) -> &PageInfo;
    pub fn title_block( &self ) -> &TitleBlock;       pub fn properties( &self ) -> &BTreeMap<String, String>;
    pub fn set_project( &mut self, project: Option<Arc<ProjectData>>, loaded: Option<&ProjectBoardSettings> );   // BOARD::SetProject glue
    pub fn embedded_files( &self ) -> &EmbeddedFiles; pub fn fixup_embedded_data( &mut self );
    pub fn component_class_manager( &self ) -> &ComponentClassManager;

    // ---- layers ----
    pub fn copper_layer_count( &self ) -> i32;        pub fn set_copper_layer_count( &mut self, count: i32 );
    pub fn enabled_layers( &self ) -> &Lset;          pub fn set_enabled_layers( &mut self, mask: &Lset );
    pub fn is_layer_enabled( &self, layer: PcbLayerId ) -> bool;
    pub fn is_layer_visible( &self, layer: PcbLayerId ) -> bool;           // enabled (no GUI visibility state)
    pub fn layer_name( &self, layer: PcbLayerId ) -> String;               // user name or canonical
    pub fn set_layer_name( &mut self, layer: PcbLayerId, name: &str ) -> bool;
    pub fn layer_id( &self, name: &str ) -> PcbLayerId;                    // GetLayerID
    pub fn layer_type( &self, layer: PcbLayerId ) -> LayerT;   pub fn set_layer_descr( &mut self, index: PcbLayerId, layer: &Layer ) -> bool;
    pub fn flip_layer( &self, layer: PcbLayerId ) -> PcbLayerId;           pub fn layer_depth( &self, start: PcbLayerId, end: PcbLayerId ) -> i32;
    pub fn is_front_layer( &self, layer: PcbLayerId ) -> bool;             pub fn is_back_layer( &self, layer: PcbLayerId ) -> bool;
    pub fn copper_layer_stack_max_id( &self ) -> PcbLayerId;

    // ---- variants and text variables ----
    pub fn variant_names( &self ) -> &[String];       pub fn add_variant( &mut self, name: &str );
    pub fn variant_description( &self, name: &str ) -> String;   pub fn current_variant( &self ) -> &str;
    pub fn resolve_text_var( &self, token: &mut String, depth: i32 ) -> bool;
    pub fn convert_cross_references_to_kiids( &self, source: &str ) -> String;
    pub fn convert_kiids_to_cross_references( &self, source: &str ) -> String;

    // ---- geometry ----
    pub fn compute_bounding_box( &self, board_edges_only: bool, physical_layers_only: bool ) -> Box2I;
    pub fn board_edges_bounding_box( &self ) -> Box2I;
    pub fn board_polygon_outlines( &self, infer_outline_if_necessary: bool, error_handler: Option<&mut dyn FnMut( &str, Option<&dyn BoardItem>, Option<&dyn BoardItem>, Vector2I )>,
                                   allow_use_arcs_in_polygons: bool, include_npth_as_outlines: bool ) -> Option<ShapePolySet>;
    pub fn update_board_outline( &mut self );
    pub fn convert_brd_layer_to_polygonal_contours( &self, layer: PcbLayerId, outlines: &mut ShapePolySet );
    pub fn cache_triangulation( &mut self, zones: &[ItemPtr] );            // BOARD::CacheTriangulation

    // ---- change tracking ----
    pub fn time_stamp( &self ) -> i32;                pub fn increment_time_stamp( &mut self );
    pub fn take_changes( &mut self ) -> Vec<BoardChange>;
    pub fn check_invariants( &self ) -> Result<(), String>;                // tests / debug: index, order keys, FpLinks, groups
}

impl BoardDesignSettings {
    pub fn severity( &self, drc_error_code: i32 ) -> Severity;             // GetSeverity
    pub fn ignore( &self, drc_error_code: i32 ) -> bool;                   // Ignore
    pub fn biggest_clearance_value( &self, engine: Option<&dyn DrcEngineHook> ) -> i32;
    pub fn line_thickness( &self, layer: PcbLayerId ) -> i32;   pub fn text_size( &self, layer: PcbLayerId ) -> Vector2I;
    pub fn text_thickness( &self, layer: PcbLayerId ) -> i32;   pub fn layer_class( &self, layer: PcbLayerId ) -> usize;
    pub fn drc_epsilon( &self ) -> i32;               pub fn hole_plating_thickness( &self ) -> i32;
    pub fn board_thickness( &self ) -> i32;           pub fn stackup_descriptor( &self ) -> &BoardStackup;
    pub fn aux_origin( &self ) -> Vector2I;           pub fn grid_origin( &self ) -> Vector2I;
    pub fn current_track_width( &self ) -> i32;       pub fn current_via_size( &self ) -> i32;   pub fn current_via_drill( &self ) -> i32;
}

impl NetSettings {
    pub fn default_netclass( &self ) -> &Arc<Netclass>;
    pub fn netclasses( &self ) -> &BTreeMap<String, Arc<Netclass>>;
    pub fn composite_netclasses( &self ) -> &BTreeMap<String, Arc<Netclass>>;
    pub fn has_netclass( &self, name: &str ) -> bool;                      pub fn set_netclass( &mut self, name: &str, netclass: Arc<Netclass> );
    pub fn set_netclass_pattern_assignment( &mut self, pattern: &str, netclass: &str );
    /// GetEffectiveNetClass( netName ): label assignments, then pattern assignments in order, then
    /// chain assignments; several matches build (and cache) a composite class ordered by priority.
    pub fn effective_netclass( &self, net_name: &str ) -> Arc<Netclass>;
    pub fn cached_effective_netclass( &self, net_name: &str ) -> Option<Arc<Netclass>>;
    pub fn clear_all_caches( &self );
}
```

Effective netclass resolution, end to end: `Board::synchronize_nets_and_net_classes` (port of
`board.cpp`, same name) walks `net_info()` and stores `net_settings.effective_netclass( name )`
into each `NetinfoItem::net_class`; `BoardConnectedItem::effective_net_class( ctx )` is then one
lookup by net code plus a deref, with the two C++ fallbacks. DRC severities and exclusions are
plain fields of `BoardDesignSettings`. The stackup is `design_settings().stackup_descriptor()` when
`has_stackup`, else `stackup_or_default()` builds the default list from the enabled layers.

### 5.1 Iteration orders the C++ code relies on (normative)

| Service | Order |
|---|---|
| `tracks()`, `footprints()`, `drawings()`, `Footprint::pads()`, `graphical_items()` | deque order: `Insert` pushes at the front, `Append` / `BulkAppend` at the back; a loaded board is in file order |
| `zones()`, `groups()`, `generators()`, `points()`, `markers()`, `constraints()`, `Footprint::fields()` | always appended (board); footprint zones / groups / points / constraints: `Insert` puts at the front |
| `all_connected_items()` | tracks; per footprint: pads, zones, connected graphics; board zones; connected board drawings (`board.cpp:3757`) |
| `Board::run_on_children` | tracks, zones, markers, groups, constraints, points, footprints (+ children), drawings (+ children) (`board.cpp:837`) |
| `Footprint::run_on_children` | fields, pads, zones, groups, bodies, constraints, points, drawings (+ children) (`footprint.cpp:3342`) |
| DRC `for_each_geometry_item` (in `kicad-drc`) | tracks, drawings (tables then their cells), zones, per footprint: fields, pads, graphics, zones |
| `net_info().iter()` | by net name (`std::map<wxString, ..>`); `nets_by_netcode()` by code |
| `item_set()` | by uuid, ties by `ItemPtr` |
| writer | `ptr_cmp`, `cmp_drawings`, `cmp_tracks`, `Footprint::cmp_pads / cmp_drawings / cmp_zones`, `cmp_points`, all ending in uuid then `ItemPtr` |
| any map keyed by an item | `BTreeMap<ItemPtr, _>`: creation order |
