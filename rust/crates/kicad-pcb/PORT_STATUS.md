# kicad-pcb port status

Board data model of `pcbnew` (design: `rust/docs/design/board-model.md`, data model:
`rust/docs/design/board-model-types.md`), later also the `.kicad_pcb` reader / writer (`io/`)
and project settings (`project/`).

State after **work package M0a - kernel and pattern** and its gate review (design revision
2.1). The module tree of `board-model-types.md` section 19 is complete; every item and settings
type exists as a placeholder that the type packages M0b-e fill; the track family is the
finished pattern. The kernel files are frozen: `lib.rs`, `items_macro.rs`, `ids.rs`, `arena.rs`,
`cache.rs`, `item.rs`, `traits.rs`, `ctx.rs`, `item_ref.rs`, `pending.rs`, `drc_api.rs`,
`test_support.rs`, `foundation_stub.rs`, `pcb_track/*`, `Cargo.toml` (`board-model.md` 16,
"Wave 0b rules").

`tools/check_item_shims.py` (run from `<kicad>/rust`) compares the delegation shims of every
item type with the `override`s of its C++ class and bases, and checks that `Track` dispatches
every trait method. The track family passes; every other type lists its missing shims until its
type package lands. It must print `ok` for a type before that type's package is reported done.

## C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| (new) generational arena | `arena.rs` (`Arena<T>`: Live / Free / Retired slots) | done |
| `BOARD_ITEM*` stored / `KICAD_T` of an arena | `ids.rs` (`Id<T>`, `ItemId`, `ItemKind`, `NetCode`, `ArenaItem`) | done |
| (new) item table | `items_macro.rs` (`board_items!` + generators `gen_ids`, `gen_any_item`, `gen_board`) | done |
| C++ `mutable` cache members | `cache.rs` (`KeyedCache<T>`, `CacheCell<T>`) | done |
| (new) stub marker | `pending.rs` (`port_pending!`) | done |
| `EDA_ITEM` / `BOARD_ITEM` / `BOARD_CONNECTED_ITEM` data, `EDA_ITEM_FLAGS`, `m_parent` | `item.rs` | done |
| `EDA_ITEM` / `BOARD_ITEM` / `BOARD_CONNECTED_ITEM` / `EDA_TEXT` / `EDA_SHAPE` virtuals | `traits.rs` | done (signatures; every non-constant base body forwards to a generic function in `board_item.rs`, `board_connected_item.rs`, `eda_text.rs`, `eda_shape/mod.rs`) |
| `BOARD_ITEM::GetLayerSet` / `IsOnLayer` / `FitsEnabledLayers` / `SetLayer` / `SetLayerSet` (base bodies) | `board_item.rs` | done; the other base bodies there are stubs (M2) |
| non-virtual `EDA_TEXT` / `EDA_SHAPE` members that call virtuals | `eda_text.rs` (`EdaTextExt`), `eda_shape/mod.rs` (`EdaShapeExt`): generic functions + blanket extension traits | `text_box` signature; the rest M0c (signatures), M10 / M7 / M8 (bodies) |
| `PCB_TABLECELL::GetParentGroup` | `pcb_tablecell.rs` `PcbTablecell::parent_group` | done |
| `GetBoard()` / `m_DRCEngine` / `GetConnectivity()` | `ctx.rs` (`BoardCtx`, `RuleResolver`, `ConnectivityQuery`, `MutEnv`, `ParentFrame`, `LayerEnv`) | done |
| `BOARD_ITEM*` parameter, `switch( Type() )` | `item_ref.rs` (`ItemRef`, `AnyItem`, `AnyItemMut` with `as_leaf_mut`, `OwnedItem`, `FootprintTree`, `TableTree`) | done; `layer_name`, `fp_relative_position`, `netname` stubs (M2) |
| `drc_constraint_types.h`, `drc_rule.h` (value part), `drc_item.h` codes + `drc_item.cpp` settings keys | `drc_api.rs` | done |
| `BOARD` struct, `Index`, typed getters, order-list iterators, `GetTimeStamp`, `IncrementTimeStamp`, `GetDesignSettings` | `board/mod.rs` | done; `Default` scalar members only (settings part: M0d) |
| `BOARD::Add` (tracks) | `board/containers.rs` `add_track` | done; `create_footprint`, `footprint_add`, `attach_footprint`, `create_table`, `table_add_cell`, `attach_table` stubs (M1); `child_env` done |
| `BOARD_ITEM_INDEX::BindItem`, `ResolveItem` | `board/index.rs` (`bind_uuid`, `rebind_uuid`, `resolve_item`) | partial (M1) |
| `BOARD_LISTENER` | `board/journal.rs` (types, switch, `drain_changes`) | partial: recording is M1 |
| `BOARD::FindNet`, `GetCopperLayerCount`, `FlipLayer`, `LAYER`, `LAYER_T` | `board/nets.rs`, `board/layers.rs` | stubs (M3, M22), `LayerT` done, `Layer` placeholder |
| `PCB_TRACK`, `PCB_ARC`, `PCB_VIA` data, constructors, enums | `pcb_track/mod.rs` | done |
| `PCB_TRACK` / `PCB_ARC` / `PCB_VIA` virtual overrides | shims in `pcb_track/mod.rs`, inherent methods in `pcb_track/{mod,arc,via,shapes}.rs` | trivial bodies done (positions, Move, IsOnCopperLayer, HasHole, HasDrilledHole, `PCB_TRACK::GetLength`), the rest stubs (M16, M17) |
| virtuals introduced by `PCB_TRACK` (`GetWidth`, `SetWidth`, `GetLength`, `GetWidthConstraint`) | inherent methods + `Track::width` / `set_width` / `length` / `width_constraint` | dispatch done; `PCB_ARC::GetLength`, the via overrides and `GetWidthConstraint` stubs (M16, M17); `GetDelay` belongs to the length / delay calculation (tier 2) |
| `ZONE_CONNECTION`, `ZONE_LAYER_PROPERTIES` | `zone_settings.rs` | done |
| `DIM_UNITS_FORMAT`, `DIM_PRECISION`, `DIM_TEXT_POSITION`, `DIM_UNITS_MODE`, `DIM_ARROW_DIRECTION`, `DIM_TEXT_BORDER` | `pcb_dimension.rs` | done |
| `COMPONENT_CLASS_CACHE_PROXY` (mutable part) | `component_class.rs` `ComponentClassCache` | done (`ComponentClass` placeholder: M0d) |
| every other item / settings type | its module of section 19 | placeholder (M0b-e) |

## Tests

New code, no C++ test corresponds: `arena::tests`, `ids::tests`, `cache::tests`,
`drc_api::tests`, `board::tests` (BoardCtx / ItemRef round trip, id stamping, `detached_copy`,
ItemId order = insertion order, `add_track` modes and refusal, `modify_track` and the
polymorphic `modify` with `as_leaf_mut`, `modify_*` keeping id / parent / group and re-binding
a changed UUID, `check_invariants` positive and negative, net handles and the net check of
`check_invariants`, the table cell's parent group, mixin hosts of the derived items, `Track`
dispatch of the `PCB_TRACK` virtuals, time stamp wrap, frame fixture,
`Board: Clone + Send + Sync`, and a compile-only test that a textbox, which implements all
five traits, has no ambiguous method name). `test_support.rs` builds the rotated / flipped /
scaled footprint fixture (`add_child`, `add_cell`, `set_child`, `child`).

## Deviations

- Pointer-address order is replaced by `ItemId` order (`board-model.md` 3.1); arena indices
  follow insertion order while nothing was freed (tested).
- An item removed and re-added gets a new handle (C++ keeps the address).
- The board time stamp moves on every `add_*` / `modify_*` / `design_settings_mut()` (C++: commit
  push, remove, DRC init); `BOARD_CONNECTED_ITEM::SetLayer`'s `InvalidateClearanceCache` has no
  counterpart (the DRC engine revalidates by time stamp).
- `BOARD_ITEM::Move` / `Rotate` / `Flip` / `Mirror` / `EDA_ITEM::SetPosition` base bodies
  (assert / message box in C++) are no-ops.
- A `modify_*` closure that changes the UUID re-binds the UUID index (C++ `SetUuid` does the same
  through `BOARD_ITEM_INDEX::Rebind`). `modify_*` restores `base.id`, `base.parent` and
  `base.group` after the closure: group membership is a back reference only `Board` writes
  (C++ `SwapItemData` keeps each side's group for the same reason).
- The time stamp wraps at `i32::MAX` instead of overflowing (C++: signed overflow; the model
  bumps the stamp far more often).
- `RESOLUTION_CONTEXT` is `kicad_core::text_vars::ResolutionContext`; `UNITS_PROVIDER` is the
  struct `kicad_core::units_provider::UnitsProvider`.
- Group member order is insertion order (C++ `unordered_set`).

## Known gaps

- Every `port_pending!` (run `rg port_pending! crates/kicad-pcb`) and every `PLACEHOLDER` struct.
- `foundation_stub.rs`: `FontRef`, `ShapeEllipse` stand-ins.
- `tools/check_item_shims.py` reports the missing delegation shims of every placeholder type.
- `BOARD_ITEM::Mirror( const SEG& )`, `HitTest( const SHAPE_LINE_CHAIN&, .. )`, `IsType`,
  `GetCoverageArea`, `PCB_TRACK::GetDelay` are not in the traits (editor tools / tier 2).
- `LayerEnv::default_board()` enables all layers until the snapshot is derived from the default
  design settings (`Board::build_layer_env`, M22).
- `BoardDesignSettings::max_error()` returns the constructor value until M0d adds the field.
- The section-18 API index of `board-model-types.md` and the rest of the section 8 family are
  stubbed only where the kernel needs them (iteration, lookup, `find_net`,
  `copper_layer_count`, `flip_layer`, create / attach); the rest is written in wave 0b by the
  package that owns the file (`board-model.md` 16, "Wave 0b rules").
- `Board::build_layer_env` is a stub until M0d: `design_settings_mut()` followed by a
  `modify_*` panics with its `port_pending!` name until then.
