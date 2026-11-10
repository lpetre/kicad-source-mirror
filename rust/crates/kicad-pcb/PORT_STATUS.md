# kicad-pcb port status

Board data model of `pcbnew` (design: `rust/docs/design/board-model.md`, data model:
`rust/docs/design/board-model-types.md`), later also the `.kicad_pcb` reader / writer (`io/`)
and project settings (`project/`).

State: the kernel (**M0a**) and the four type packages of wave 0b (**M0b** - **M0e**) have
landed and passed the integration review (design revision 2.2); every item and settings type
has its data, constructor defaults and delegation shims, behaviour is filled in by the later
packages, each of which keeps its own section below. The kernel files (`lib.rs`,
`items_macro.rs`, `ids.rs`, `arena.rs`, `cache.rs`, `item.rs`, `traits.rs`, `ctx.rs`,
`item_ref.rs`, `pending.rs`, `drc_api.rs`, `test_support.rs`, `foundation_stub.rs`,
`pcb_track/*`, `Cargo.toml`) are frozen (`board-model.md` 16): a change goes through the
orchestrator.

`tools/check_item_shims.py` (run from `<kicad>/rust`) compares the delegation shims of every
item type with the `override`s of its C++ class, its bases and the subclasses folded into the
Rust type (`PCB_GENERATOR` subclasses in `pcbnew/generators/*.h`, the `PCB_DIM_*` classes, the
generated tables), and checks that `Track` dispatches every trait method. It prints `ok` for
all 26 types and must keep doing so.

Sections: crate-wide deviations and known gaps, then one section per work package.

## Deviations (crate-wide; package-specific ones are in the package sections)

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

## Known gaps (crate-wide)

- Every `port_pending!` (run `rg port_pending! crates/kicad-pcb`): a behaviour stub that names
  its C++ function and panics; each belongs to the package named in its file's module comment.
- `foundation_stub.rs`: `FontRef`, `ShapeEllipse` stand-ins.
- `BOARD_ITEM::Mirror( const SEG& )`, `HitTest( const SHAPE_LINE_CHAIN&, .. )`, `IsType`,
  `GetCoverageArea`, `PCB_TRACK::GetDelay` are not in the traits (editor tools / tier 2).
- The `dump` module (replay of `pcb-dump`, `docs/PCB_DUMP_FORMAT.md`) is a placeholder; until
  it exists the constructor defaults are checked against `pcb-dump defaults` by
  `tests/test_defaults_oracle.rs` (spot checks) and the per-module `constructor_*` tests.

## Package M0a: kernel and pattern

### C++ to Rust map

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
| non-virtual `EDA_TEXT` / `EDA_SHAPE` members that call virtuals | `eda_text.rs` (`EdaTextExt`), `eda_shape/mod.rs` (`EdaShapeExt`): generic functions + blanket extension traits | signatures (M0c); bodies M10 / M7 / M8 |
| `PCB_TABLECELL::GetParentGroup` | `pcb_tablecell.rs` `PcbTablecell::parent_group` | done |
| `GetBoard()` / `m_DRCEngine` / `GetConnectivity()` | `ctx.rs` (`BoardCtx`, `RuleResolver`, `ConnectivityQuery`, `MutEnv`, `ParentFrame`, `LayerEnv`) | done |
| `BOARD_ITEM*` parameter, `switch( Type() )` | `item_ref.rs` (`ItemRef`, `AnyItem`, `AnyItemMut` with `as_leaf_mut`, `OwnedItem`, `FootprintTree`, `TableTree`) | done; `layer_name`, `fp_relative_position`, `netname` stubs (M2) |
| `drc_constraint_types.h`, `drc_rule.h` (value part), `drc_item.h` codes + `drc_item.cpp` settings keys | `drc_api.rs` | done |
| `BOARD` struct, `Index`, typed getters, order-list iterators, `GetTimeStamp`, `IncrementTimeStamp`, `GetDesignSettings` | `board/mod.rs` | done (`Board::default` = the `BOARD` constructor, completed by M0d) |
| `BOARD::Add` (tracks) | `board/containers.rs` `add_track` | done; `create_footprint`, `footprint_add`, `attach_footprint`, `create_table`, `table_add_cell`, `attach_table` stubs (M1); `child_env` done |
| `BOARD_ITEM_INDEX::BindItem`, `ResolveItem` | `board/index.rs` (`bind_uuid`, `rebind_uuid`, `resolve_item`) | partial (M1) |
| `BOARD_LISTENER` | `board/journal.rs` (types, switch, `drain_changes`) | partial: recording is M1 |
| `BOARD::FindNet`, `GetCopperLayerCount`, `FlipLayer`, `LAYER`, `LAYER_T` | `board/nets.rs`, `board/layers.rs` | see M0d (layers) and M3 (nets) |
| `PCB_TRACK`, `PCB_ARC`, `PCB_VIA` data, constructors, enums | `pcb_track/mod.rs` | done |
| `PCB_TRACK` / `PCB_ARC` / `PCB_VIA` virtual overrides | shims in `pcb_track/mod.rs`, inherent methods in `pcb_track/{mod,arc,via,shapes}.rs` | trivial bodies done (positions, Move, IsOnCopperLayer, HasHole, HasDrilledHole, `PCB_TRACK::GetLength`), the rest stubs (M16, M17) |
| virtuals introduced by `PCB_TRACK` (`GetWidth`, `SetWidth`, `GetLength`, `GetWidthConstraint`) | inherent methods + `Track::width` / `set_width` / `length` / `width_constraint` | dispatch done; `PCB_ARC::GetLength`, the via overrides and `GetWidthConstraint` stubs (M16, M17); `GetDelay` belongs to the length / delay calculation (tier 2) |
| `ZONE_CONNECTION`, `ZONE_LAYER_PROPERTIES` | `zone_settings.rs` | done |
| `DIM_UNITS_FORMAT`, `DIM_PRECISION`, `DIM_TEXT_POSITION`, `DIM_UNITS_MODE`, `DIM_ARROW_DIRECTION`, `DIM_TEXT_BORDER` | `pcb_dimension.rs` | done |
| `COMPONENT_CLASS_CACHE_PROXY` (mutable part) | `component_class.rs` `ComponentClassCache` | done |
| every other item / settings type | its module of section 19 | see the package sections M0b - M0e |

### Tests

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

## Package M0b: padstack, pad, footprint, zone, zone settings, teardrop parameters

| C++ | Rust | State |
|---|---|---|
| `PADSTACK` + `SHAPE_PROPS` / `COPPER_LAYER_PROPS` / `MASK_LAYER_PROPS` / `DRILL_PROPS` / `POST_MACHINING_PROPS`, `PAD_SHAPE`, `PAD_DRILL_SHAPE`, `PAD_DRILL_POST_MACHINING_MODE`, `BACKDRILL_MODE`, `PAD_ATTRIB`, `PAD_PROP`, `UNCONNECTED_LAYER_MODE`, `CUSTOM_SHAPE_ZONE_MODE`, `PADSTACK::TYPE` / `MODE` | `padstack.rs` | data, constructor, `for_new_via` (PCB_VIA constructor statements), trivial accessors done; mode logic (`CopperLayer`, `EffectiveLayerFor`, `ForEachUniqueLayer`, flip, compare, knockouts, backdrill, primitives, `operator=`) stubs (M13) |
| `PAD_UTILS` | `pad_utils.rs` | `PadCheckCode` / `PadCheckResult` done, functions stubs (M13) |
| `PAD`, `PAD_SIM_ELECTRICAL_TYPE`, PAD_DRAW_CACHE_DATA | `pad/mod.rs` (data, shims, accessors, `set_dirty`, `PTHMask` & co.), `pad/derive.rs` (M14 stubs), `pad/shapes.rs` (M15 stubs) | data, constructor, 27 shims done |
| `Board::set_zone_layer_override` / `clear_zone_layer_overrides` (pads and vias) | `pad/mod.rs` | stubs (M14 / M17) |
| `FOOTPRINT`, `FP_3DMODEL`, `FOOTPRINT_VARIANT`, `FP_UNIT_INFO`, `JUMPER_GROUP(_SET)`, cache data structs, `FOOTPRINT_ATTR_T`, `FOOTPRINT_TYPE`, `FOOTPRINT_STACKUP` | `footprint/mod.rs` | data, constructor (without the mandatory fields: `create_footprint`, M1), 12 shims, trivial getters (position, orientation, layer, `FitsEnabledLayers`, attributes, variants, net-tie cache read) done; rest stubs (M19) |
| `Board::footprint_*` cascades, `build_net_tie_cache` | `footprint/transform.rs`, `footprint/mod.rs` | stubs (M19) |
| footprint bbox / hull / courtyard / hit tests, `cmp_*`, `Transform*ToPolySet`, `FOOTPRINT_COURTYARD_INDEX` | `footprint/{geometry,compare,shapes}.rs`, `footprint_courtyard_index.rs` | stubs (M20) |
| `ZONE`, `ISOLATED_ISLANDS`, fill state | `zone/mod.rs`, `zone/shapes.rs` | data, constructor, 23 shims, `ZoneFill` writers (except hash / triangulation), `Board::zone_fill_mut`, `IsIsland`, `IsConnected` done; rest stubs (M18) |
| `MergeZonesWithSameOutline`, `AutoAssignZonePriorities` | `zone_utils.rs` | stubs (M18) |
| `ZONE_SETTINGS`, `ZONE_FILL_MODE`, `THIEVING_*`, `ZONE_BORDER_DISPLAY_STYLE`, `ISLAND_REMOVAL_MODE`, `PLACEMENT_SOURCE_T`, `REPEAT_LAYOUT_OPTIONS`, `CORNER_SMOOTHING`, `zones.h` constants | `zone_settings.rs` | data, constructor, `operator==` done; `ExportSetting`, `operator<<` stubs (M5) |
| `TEARDROP_PARAMETERS(_LIST)`, `TARGET_TD`, `TEARDROP_TYPE`, canonical target names | `teardrop_parameters.rs` | done |

Tests: constructor defaults of every type above against the C++ constructors (`padstack`,
`pad`, `footprint`, `zone`, `zone_settings`, `teardrop_parameters` test modules), fixture tests
with `check_invariants` (pad replace / add, footprint frame reads, zone fill writers do not move
the time stamp).

Deviations / notes:

- `PADSTACK::m_copperProps` is an `unordered_map` whose order is observable (CUSTOM-mode
  `ForEachUniqueLayer`, the `begin()` fallbacks of `CopperLayer` / `EffectiveLayerFor`): the
  port is `padstack::CopperPropsMap`, which reproduces the libstdc++ node order
  (`board-model.md` 10.1; the M0b `BTreeMap` was replaced by package M13).
- `Pad::default()` fills `zone_layer_overrides` for the 32 copper layers of the no-board
  `BoardCopperLayerCount()`; a pad constructed under a board in C++ gets the board's copper
  count. `GetZoneLayerOverride` returns `ZLO_NONE` for a missing layer, so reads are equal.
- `Zone::default()` writes the values `ZONE_SETTINGS().ExportSetting( *this, false )` stores
  directly (ExportSetting itself is M5); `Zone::new_with_settings` (board defaults) is a stub.
- `ZoneSettings` equality is manual (`std::equal` over the first map's range for the layer
  properties), not a derive.
- `RECT_NO_CHAMFER` is written as the literal 0 (`RECT_CHAMFER_POSITIONS` is not in kicad-geom).
- `OUTLINE_ERROR_HANDLER` of `BuildCourtyardCaches` is
  `convert_shape_list_to_polygon::OutlineErrorHandler` (one definition for the crate).

## Package M0c: shapes, text, tables, dimensions, barcode

Types of `board-model-types.md` sections 6, 7, 8 and the barcode of section 10, transcribed
and checked against the headers of this tree; `Default` = the C++ constructors (nil UUID),
`new(&mut KiidGenerator, ..)` draws the UUID. `check_item_shims.py` prints `ok` for
`PcbShape`, `PcbText`, `PcbField`, `PcbTextbox`, `PcbTablecell`, `PcbTable`, `PcbDimension`,
`PcbBarcode`.

| C++ | Rust | State |
|---|---|---|
| `SHAPE_T`, `FILL_T`, `ARC_MID`, `EDA_SHAPE_HATCH_CACHE_DATA`, `EDA_SHAPE` data + constructor | `eda_shape/mod.rs` (`ShapeT`, `FillT`, `ArcMid`, `EdaShapeHatchCacheData`, `EdaShape::new`) | done; trivial getters / `SetStart` / `SetEnd` real, other setters stubs (M7) |
| `EDA_SHAPE` virtual base bodies, non-virtual members calling virtuals | generic functions + `EdaShapeExt` (`make_effective_shapes`, `hatching`, `hatch_lines`, `shape_position`, `shape_center`, `shape_bounding_box`, `shape_hit_test[_rect]`, `shape_transform_to_polygon`, `shape_length`, `rectangle_size`) | signatures; `GetWidth` (with `ctx`), `IsProxyItem`, `IsFilledForHitTesting` real, rest stubs (M7 / M8) |
| `LINE_ENDING_STYLE`, `LINE_ENDING` | `line_ending.rs` | data done; geometry and tokens stubs (M7) |
| `PCB_SHAPE` data, constructor, overrides; lib-frame machinery and `EDA_SHAPE` setter overrides | `pcb_shape/mod.rs`, `pcb_shape/lib_frame.rs` | data done; `GetLayer`, `HasLineStroke`, `IsProxyItem`, `GetWidth` (`transformFp`, `fpScaleLinear`), `GetStroke`, `GetPosition`/`GetCenter` forwards, lib accessors real; rest stubs (M9) |
| `EDA_TEXT` data, `EDA_TEXT_RENDER_CACHE_DATA`, virtual base bodies, non-virtual members | `eda_text.rs` (`EdaText`, `EdaTextRenderCacheData`, `EdaTextExt`: `text_box`, `effective_text_pen_width`, `effective_text_shape`, `line_positions`, `interline`, `compare_text`, `auto_thickness`, `text_width`, `text_height`, `text_style_name`) | data done; stored-value getters real, setters / layout stubs (M10) |
| `PCB_TEXT`, `PCB_FIELD`, `FIELD_T` | `pcb_text.rs` (`PcbText::for_footprint` = the `FOOTPRINT*` constructor), `pcb_field.rs` | data done; `GetPosition`/`SetPosition`/`Move` forwards real, rest stubs (M10) |
| `PCB_TEXTBOX`, `PCB_TABLECELL`, `PCB_TABLE` | `pcb_textbox.rs`, `pcb_tablecell.rs`, `pcb_table.rs` | data done; trivial accessors real, rest stubs (M11) |
| `PCB_GENERATED_TABLE`, `PCB_DRILL_CHART`, `PCB_STACKUP_TABLE` members, `GENERATED_TABLE_COLUMN`, `DRILL_CHART_FILTER` | `pcb_generated_table.rs` | data done; templates / schemas / refresh stubs (M11, M26) |
| `PCB_DIMENSION_BASE` + `PCB_DIM_ALIGNED/ORTHOGONAL/RADIAL/LEADER/CENTER` | `pcb_dimension.rs` (`PcbDimension::of_type`, `DimensionKind`, `DimAligned`, `DimOrthogonalDir`) | data done; stubs (M12) |
| `PCB_BARCODE`, `BARCODE_T`, `BARCODE_ECC_T`, `PCB_BARCODE_CACHE` | `pcb_barcode.rs` | data done; stubs (M25) |

Derived classes (`PcbField`, `PcbDimension`, `PcbTextbox`, `PcbTablecell`) have a shim for every
virtual of their class or its bases; an inherited body calls the embedded base only where the
base body reaches no virtual the derived class overrides (each such method says so in its doc
comment), the others are stubs named `BASE::Method (DERIVED)`. Virtuals introduced by a class
are inherent on each struct of the family: `corners`, `sync_lib_coords`, `set_start`, `set_end`
(shape family), `text_hit_test[_rect]`, `text_type_description` (text family), `start`, `end`,
`update_geometry`, `update_text` (dimension), `set_autosized_row_heights` (table).

Tests: `constructor_values` in each module (constructor defaults against the C++ constructors,
fixture `set_child` / `add_child` / `add_cell` + `check_invariants`).

Deviations / notes:

- `PcbShapeLib` has no `PartialEq` (`ShapePolySet` has none); design said `Clone, Debug` anyway.
- `EdaShape::default()` has no C++ counterpart (`EDA_SHAPE` has no default constructor): it is
  `EDA_SHAPE( UNDEFINED, 0, NO_FILL )`.
- `PcbField::default()` / `new()` leave the user-field ordinal 0; the C++ constructor asks the
  parent footprint's `GetNextFieldOrdinal()` (the caller does it, M19).
- `PcbTablecell::default()` is a cell of an unmirrored table; `new(.., mirrored)` takes the
  parent's side (C++ reads the parent in the constructor). The `INT_MAX / 2` rectangle sets the
  lib-frame end too (what `SetEnd`'s `syncLibCoords` does without a parent footprint).
- `PcbBarcode::text` has `ParentRef::None` until the barcode is in a board.
- `GeneratedTableData::hidden` (`PCB_GENERATED_TABLE::m_hidden`) is not in the design but is
  persisted (`AllColumns()` in the writer); added. `PCB_DRILL_CHART::m_laidOutShowTotals` and
  `m_builtGeneration` are transient and dropped.

Known gaps:

- (closed) `EdaShapeItem::width`, `effective_width`, `hatch_line_width`, `hatch_line_spacing`
  take `ctx`; `PcbShape::width` is the ported `PCB_SHAPE::GetWidth` (tested on the scaled
  fixture footprint for a shape, a textbox and a table cell).
- The `EDA_SHAPE` setter overrides exist on `PcbShape` (all) and on `PcbTextbox` / `PcbTablecell`
  (`set_start`, `set_end`, `set_shape`, `set_top/left/right/bottom`, `set_stroke`,
  `set_poly_shape`); the remaining ones for textboxes follow with M11.

## Package M0d: board, settings, nets, project data

| C++ | Rust | State |
|---|---|---|
| `BOARD::BOARD()` (member initialisers, `LAYER` table, `recalcOpposites`, default net class) | `board/mod.rs` `Board::default`, `board/layers.rs` `init_layers` / `recalc_opposites` | done; ends with `layer_env = build_layer_env()` |
| `BOARD::GetCopperLayerCount`, `FlipLayer`, `SetCopperLayerCount`, `GetEnabledLayers`, `GetStandardLayerName`, `LAYER`, `LAYER_T` (+ `ShowType` / `ParseType`) | `board/layers.rs` | done (field reads); `build_layer_env` done; the other layer queries stubs (M22) |
| `BOARD_DESIGN_SETTINGS` (all members, constructor incl. the 78 DRC severity defaults, `DRC_EXCLUSION`, `VIA_DIMENSION`, `DIFF_PAIR_DIMENSION`, `VIA_STACK_PRESET`, `TEXT_ITEM_INFO`, `board_design_settings_defaults.h`) | `board_design_settings.rs` | data + defaults done; `max_error`, `Set/GetCopperLayerCount`, `SetUserDefinedLayerCount`, `GetSeverity`, `Ignore`, `GetLayerClass` and the per-class getters done; the derived getters stubs (M5) |
| `BOARD_STACKUP`, `BOARD_STACKUP_ITEM` (per-type constructor), `DIELECTRIC_PRMS` | `board_stackup.rs` | data + defaults + sub-layer accessors done; algorithms stubs (M6) |
| `PCB_PLOT_PARAMS` | `pcb_plot_params.rs` | data + defaults, `SetGerberPrecision`, `SetSvgPrecision` done; `IsSameAs` stub (M6) |
| `NETINFO_ITEM`, `NETINFO_LIST` | `netinfo.rs` | data + defaults + shims (`GetLayer`, `GetPosition`, `GetBoundingBox`) done; list logic stubs (M3) |
| `NETCLASS` | `netclass.rs` | data, constructor, `DEFAULT_*`, accessors done; name / composite logic stubs (M3) |
| `NET_SETTINGS` | `net_settings.rs` | data + constructor done; effective-netclass logic stubs (M4) |
| `EMBEDDED_FILES` | `embedded_files.rs` | data + defaults done; codec stubs (M24) |
| `COMPONENT_CLASS`, `COMPONENT_CLASS_MANAGER`, `COMPONENT_CLASS_ASSIGNMENT_RULE` (data) | `component_class.rs` | data + defaults done; logic stubs (M24) |
| `impl Board` stubs of `board-model.md` 8.1 / 8.2 / 8.5 and `board-model-types.md` 18 | `board/{containers,index,nets,layers,queries,text_vars,markers,compare,outline}.rs`, `board_item.rs` (`move_item` ... `set_item_locked`), `board_connected_item.rs` (`set_item_net`) | signatures (stubs) for M1, M2, M3, M21, M22 |

Tests: `board::layers::tests` (BOARD constructor, copper count -> flip table -> `MutEnv`,
user-layer pairing), `board_design_settings::tests` (constructor defaults, DRC severities),
`board_stackup::tests`, `pcb_plot_params::tests`, `netinfo::tests`, `netclass::tests`,
`net_settings::tests`, `embedded_files::tests`, `component_class::tests`. No C++ test
corresponds (constructor values, checked against the C++ constructors and
`docs/analysis/settings.md` 3.6 / 3.7).

Deviations / representation choices:

- `NetSettings` pattern assignments hold the compiled matcher, as in C++
  (`NetclassPatternAssignment = (EdaCombinedMatcher, String)`; the kicad-core matcher is
  `Clone + Send + Sync`). `add_single_pattern_assignment`,
  `add_single_chain_pattern_assignment`, the clear / set functions and `clear_all_caches` are
  ported; the bus expansion and the effective-netclass logic are M4.
- A root `Netclass` has an **empty** constituent list (C++ lists `this`; a `NETCLASS` always
  has at least one constituent). A constituent `ComponentClass` has `is_constituent` (C++
  lists `this`), which keeps it apart from the "none" class (no constituents) whatever its
  name; `IsEmpty` is ported. Note for M24: C++ effective classes point at the constituent
  objects, whose usage context changes later; `constituent_classes` holds `Arc` copies, so
  read the usage context through the manager's map.
- The three meander settings are their `tuning_pattern_settings` JSON objects; the defaults
  are the objects the C++ writer produces for `PNS::MEANDER_SETTINGS()` with the constructor
  spacings.
- `PcbPlotParams::layer` (`m_layer`, uninitialised in C++) defaults to `F_Cu`, the value the
  reference build reads (`pcb-dump defaults`).
- Net 0 of a new board keeps the `Default` class of the `NET_SETTINGS` constructor (priority
  `INT_MAX`), as in C++, while `net_settings.default_netclass` is the board's class
  (priority -1).
- `OutlineErrorHandler` (`OUTLINE_ERROR_HANDLER`) is the trait of
  `convert_shape_list_to_polygon.rs` (items as handles; every closure with the four
  parameters implements it), used by `get_board_polygon_outlines` and
  `Footprint::build_courtyard_caches`.

`Board::default` derives its `LayerEnv` snapshot from the settings (`build_layer_env`);
`LayerEnv::default_board()` no longer exists.

## Package M0e: groups, generators and the small item types

Types of `board-model-types.md` sections 9 and 10, with `Default` = the C++ constructor values
(nil UUID), `new(&mut KiidGenerator, ..)`, delegation shims (`check_item_shims.py` prints `ok`
for all ten) and inherent methods (trivial accessors real, the rest `port_pending!` for M23 /
M24).

| C++ | Rust | State |
|---|---|---|
| `PCB_GROUP`, `EDA_GROUP` data and accessors | `pcb_group.rs` `PcbGroup` | types, defaults, shims; constant overrides real (`GetLayer`, `IsLayerAgnostic`, `IsOnCopperLayer`, `HitTest`), member walks stubs (M23) |
| `PCB_GENERATOR` (+ subclass data as a property map) | `pcb_generator.rs` `PcbGenerator`, `GeneratorProp` | types, defaults, shims; `normalize_properties` and the subclass-dependent overrides stubs (M23) |
| `PCB_TARGET` | `pcb_target.rs` | types, defaults, shims, position / move real |
| `PCB_POINT`, `PCB_POINT::cmp_points` | `pcb_point.rs` `PcbPoint`, `pcb_point_cmp_points` | types, defaults, shims; frame-dependent setters and the comparator stubs (M23) |
| `PCB_MARKER`, `MARKER_BASE`, `RC_ITEM` / `DRC_ITEM` data | `pcb_marker.rs` `PcbMarker`, `MarkerT`, `RcItem` | types, constructor (marker type from the error code) ported, shims; `GetSeverity`, glyph, hit tests stubs (M23) |
| `PCB_CONSTRAINT`, `CONSTRAINT_MEMBER`, enums, token functions | `pcb_constraint.rs` | types, defaults, shims, constant overrides real, token tables ported; `ConstraintsAreDuplicate`, `RemapKIIDs`, shape, description stubs (M23) |
| `PCB_3D_BODY` + `EXTRUDED_3D_BODY` | `pcb_3d_body.rs` | types, defaults (incl. `GetDefaultColor`), shims, layer accessors real; `Flip` stub (M23) |
| `PCB_GRID_ITEM`, `PCB_GRID_AFFECTS` | `pcb_grid_item.rs` | types, defaults, shims, clamping setters and trivial overrides real; geometry stubs (M23) |
| `PCB_DRILL_MAP`, `DRILL_SPAN` | `pcb_drill_map.rs` `PcbDrillMap`, `DrillSpan` | types, defaults, shims; `DRILL_SPAN` ordering and the drill geometry stubs (M23) |
| `PCB_REFERENCE_IMAGE` + `REFERENCE_IMAGE` + `BITMAP_BASE` data | `pcb_reference_image.rs` | types, defaults, shims; size / position checks stubs (M24) |

Tests: constructor defaults of every type (each citing its constructor), the trivial overrides
through the traits on fixture footprint children with `check_invariants`; ported from
`qa/tests/pcbnew/constraints/test_pcb_constraint.cpp`: `BasicProperties`,
`GeometrySurfaceIsSafe` (transforms only), `MembersAndValue`, `VertexMemberIdentity`.

Differences between the design and the C++ headers (header followed where they differ):

- `PCB_GRID_ITEM::GetClass()` is `"PCB_GRIDITEM"` (no underscore before ITEM).
- `PCB_3D_BODY`: the extrusion members live in `EXTRUDED_3D_BODY m_extruded`; the design
  flattens them, and `layer` is `m_extruded.m_layer` (what `GetLayer` / `SetLayer` use), not
  `BOARD_ITEM::m_layer` (`base.layer`, `UNDEFINED_LAYER`).
- `DrcRuleInfo` is the kernel's (`drc_api.rs`), not redefined in `pcb_marker.rs`.
- `PCB_GENERATOR` has no default layer argument; `Default` uses `F_Cu`, the default of every
  registered subclass; `new` takes the layer.

Deviations / known gaps of this package:

- Group member order is insertion order (C++ `unordered_set`), also for
  `group_member_ids`.
- `PCB_DRILL_MAP( aParent )` with a board takes the board drill-symbol profile's size; the
  profile is kept verbatim (`Option<Sexpr>`), so `Default` / `new` give the no-board value
  (1.5 mm). The parser sets the size from the file.
- `PCB_POINT::SetParent` recomputes the baked position: `Board` must call
  `on_footprint_transformed` after adding a point to a footprint (M1 / M19).
- The generator subclasses (`pcbnew/generators/*.h`) are one Rust struct; their overrides
  (`GetFriendlyName`, `GetPosition`, `GetLayer`, `GetBoundingBox`, `HitTest`,
  `GetItemDescription`) are shims whose inherent method dispatches on `generator_type`.
  `friendly_name` is real, the others are stubs (M23).
- Not ported (editor / UI): `PCB_GROUP::TopLevelGroup` / `WithinScope`, `GRID_GEOMETRY`,
  `FindActiveGridAt`, constraint labels / glyphs / bitmaps, `EXTRUDED_3D_BODY::PackColorKey`,
  marker colours and zoom.

## Wave 0b integration review

Changes of the review (design revision 2.2), beyond what the package sections above say:

| What | Where |
|---|---|
| `EdaShapeItem::{width, effective_width, hatch_line_width, hatch_line_spacing}` take `ctx`; `PCB_SHAPE::GetWidth`, `transformFp`, `fpScaleLinear` ported | `traits.rs`, `eda_shape/mod.rs`, `pcb_shape/mod.rs`, `pcb_textbox.rs`, `pcb_tablecell.rs` |
| `EdaShapeItem::{top_left, bot_right}` take `ctx` (`PCB_TEXTBOX::GetTopLeft` / `GetBotRight` read `GetDrawRotation()`, i.e. the parent footprint's orientation; not reported by M0c) | the same files |
| `EdaPatternMatch: Send + Sync` + `clone_box`, `EdaCombinedMatcher: Clone` | `kicad-core` `eda_pattern_match.rs` |
| compiled matchers in `NetSettings`; pattern-assignment helpers ported | `net_settings.rs` |
| class families in the shim checker; `PcbGenerator::friendly_name` | `tools/check_item_shims.py`, `pcb_generator.rs` |
| `LayerEnv::default_board()` removed | `ctx.rs`, `board/mod.rs` |
| one `OutlineErrorHandler` | `convert_shape_list_to_polygon.rs`, `board/outline.rs`, `footprint/geometry.rs` |
| `ComponentClass::is_constituent`, `IsEmpty` | `component_class.rs` |
| `PcbPlotParams::layer` default `F_Cu` | `pcb_plot_params.rs` |

Checked against the C++ and `pcb-dump defaults` and kept: net 0 holding the
`NET_SETTINGS`-constructor class while the board's default class has priority -1; the 32
`zone_layer_overrides` entries of a pad without board; textbox margins 1002500; dimension
extension height 586420; `GeneratedTableData::hidden`; `PcbGenerator::default` on `F_Cu`;
`EXTRUDED_3D_BODY` flattened into `Pcb3dBody`.

Tests added: `pcb_shape::tests::width_scales_with_the_parent_footprint`,
`net_settings::tests::pattern_assignments_hold_compiled_matchers`,
`pcb_generator::tests::friendly_name_of_the_subclasses`,
`component_class::tests::constituent_shapes`,
`convert_shape_list_to_polygon::tests::closures_are_outline_error_handlers`,
`tests/test_defaults_oracle.rs` (constructor defaults of 20 types against `pcb-dump
defaults`), kicad-core `combined_matcher_is_clone_send_sync`.

## Package M3: nets and netclasses

| C++ | Rust | State |
|---|---|---|
| `NETCLASS::GetName`, `GetHumanReadableName`, `NameEquals`, `ContainsNetclassWithName`, `operator==`, `EqualsByPersistedFields`, `ResetParameters`, `Get/SetConstituentNetclasses`, `SetConstituentOwners` | `netclass.rs` (`get_name`, `get_human_readable_name`, `name_equals`, `contains_netclass_with_name`, `same_constituents`, `equals_by_persisted_fields`, `reset_parameters`, `constituent_netclasses`, `set_constituent_netclasses`) | ported; an empty `constituents` list is C++ `{ this }` |
| `wxString::Matches` (wx 3.2, non-regex branch) | `netclass::wx_string_matches` | ported by hand, wx quirks kept (`*?` skips the `?`, single-level backtrack) |
| `NETINFO_ITEM::SetNetname`, `SetNetClass`, `Clear`, `HasAutoGeneratedNetname`, `SetTerminal`, terminal-pad / chain accessors | `netinfo.rs` | ported; `set_netclass_or_default` / `clear_with_default` are the board forms |
| `NETINFO_LIST::AppendNet`, `RemoveNet`, `RemoveUnusedNets`, `clear`, `buildListOfNets` (list part), `getFreeNetCode`, `OrphanedItem` | `netinfo.rs` | ported; `append_net` also stamps `base.id = ItemId::net(code)` |
| `BOARD::FindNet` (code, name), `FindOrAddNet`, `Add` / `Remove` (net), `GetNetCount`, `RemoveUnusedNets`, `BuildListOfNets`, `SanitizeNetcodes`, `MapNets`, `MatchDpSuffix`, `DpCoupledNet`, `GetNodesCount`, `SetAreasNetCodesFromNetNames`, `SynchronizeNetsAndNetClasses`, `GetNetClassAssignmentCandidates`, `TracksInNet`, parser `ResolveTerminalPads` loop | `board/nets.rs` | ported |
| `BOARD::ApplyNetChainNetclasses` | `board/nets.rs` | partial: early returns and the assignment list ported; `port_pending!` where it calls `NET_SETTINGS::ClearChainPatternAssignments` / `SetChainPatternAssignment` (M4, no Rust signature yet). Reached only for boards whose project has net-chain classes |
| `NETINFO_ITEM::GetBoundingBox` | `netinfo.rs` | stub: GUI only, needs `CONNECTIVITY_DATA::GetNetItems` (not in `ConnectivityQuery`) |
| `NETINFO_ITEM::GetMsgPanelInfo`, `Matches`, `NETINFO_LIST::RebuildDisplayNetnames`, `RenameNets`, `NETCLASS::Serialize` / `Deserialize` | - | skipped: GUI / editor / protobuf API (`settings.md` 2.3) |

Run-time dependencies on other packages' stubs: `remove_net`, `sanitize_netcodes`, `map_nets`
(M1 `all_connected_items`; M2 `set_item_net`, `ItemRef::netname`), `set_areas_net_codes_from_net_names`
(M2 `set_item_net`, zone `is_on_copper_layer`), `synchronize_nets_and_netclasses` with a project
(M4 `clear_all_caches`, `get_effective_netclass`), `resolve_net_chain_terminal_pads` (M22
`find_pad_by_uuid`).

Tests: `netclass::tests::name_equals_matches_get_name` (`qa/tests/common/test_netclass.cpp`,
`NameEqualsMatchesGetName`); new tests for human-readable names, wildcard containment and the
`wxString::Matches` quirks, `ResetParameters` / equality, `SetNetname` and auto-generated names
(expected names from `pcb-dump` on `issue11814.kicad_pcb`, net 30), `AppendNet` code assignment
and code reuse after `RemoveNet`, `RemoveUnusedNets` / `clear` / `buildListOfNets`,
`MatchDpSuffix`, and the board-level find / add / couple / candidates / `BuildListOfNets`
without a project / orphaned item.

Deviations:
- `NetinfoItem::set_netclass( None )` and `clear()` (no board at hand) store `None`, which every
  reader resolves to the board's default class (`board-model.md` 10); C++ stores the default
  object itself. Board code uses `set_netclass_or_default` / `clear_with_default`, which store
  the object like C++.
- `NetinfoList::build_list_of_nets` is the list part only; `Board::build_list_of_nets` is the
  full port (default class, synchronisation, zone net codes).
- `Board::remove_unused_nets` unbinds the UUIDs of the dropped nets (C++ leaves dangling index
  entries); `add_net` / `remove_net` / `synchronize_nets_and_netclasses` bump the time stamp
  (C++: listeners / `Remove` only).
- `BOARD::Remove( net )` and `MapNets` write the item's net code directly (`SetNet`): the
  `ZONE::SetNet` override for copper-thieving zones is not applied (no `set_net` virtual).

## Package M2: item base (BOARD_ITEM, BOARD_CONNECTED_ITEM, EDA_ITEM)

| C++ | Rust | State |
|---|---|---|
| `BOARD_ITEM::IsOnCopperLayer`, `IsSideSpecific`, `IsLocked`, `GetStroke`, `LayerMaskDescribe`, `GetEffectiveShape`, `TransformShapeToPolygon` / `PolySet`, `IsGroupableType`, `GetMaxError`, `GetFlippedLayer`, `BoardLayerCount` / `BoardCopperLayerCount` / `BoardLayerSet`, `GetLayerName` (both), `GetParentFootprint`, `GetFPRelativePosition`, `SetFPRelativePosition`, `MIRROR_AXIS`, `GetBoard() != nullptr` (`has_board`) | `board_item.rs` (generic base bodies) | ported |
| `BOARD_ITEM::ptr_cmp` | `board_item::board_item_ptr_cmp` | ported (pointer tie-break = `ItemId` order, 3.1) |
| `EDA_ITEM::GetItemDescription`, `GetFriendlyName` / `GetTypeDesc` (`EDA_ITEM_DESC` map) | `board_item.rs`, `eda_item::type_desc` | ported (English) |
| `Move` / `Rotate` / `Flip` / `Mirror` / `SetLayer` / `SetLocked` on a stored item; `PCB_GROUP` and `PCB_GENERATOR` (base class) cascades, `groupHasFootprint`, `PCB_TABLE::Move` / `SetLayer` | `Board::move_item` .. `set_item_locked` in `board_item.rs` | ported; table Rotate / Flip / Mirror stubs (see below) |
| `BOARD_CONNECTED_ITEM::GetEffectiveNetClass`, `GetNetClassName`, `GetOwnClearance`, `GetNetname`, `GetNetnameMsg`, `GetShortNetname`, `GetDisplayNetname`, `SetNetCode` | `board_connected_item.rs` (`net`, `effective_netclass`, ..., `Board::set_item_net`) | ported |
| `EDA_ITEM` flags helpers, custom properties (`Set/Get/Remove/Clear/HasCustomProperty`, `SetCustomProperties`) | `eda_item.rs` (inherent methods of `BoardItemBase`) | ported; the property-manager parts (`RemoveConflictingCustomProperties`, inspectables) skipped (editor) |

Tests: `board_item::tests` (IsLocked rules incl. parent group / FPHOLDER / no board,
IsOnCopperLayer, IsSideSpecific, boardless LayerMaskDescribe, base texts and stroke, ptr_cmp
order incl. duplicate UUIDs, GetParentFootprint / GetFPRelativePosition on the frame fixture,
leaf move and group SetLayer through `Board`, null branches of the netclass / clearance /
netname getters), `eda_item::tests` (custom properties after
`test_pcb_dynamic_properties.cpp` `CustomPropertiesExposedAsDynamic`, edit flags). No C++
test targets these base bodies directly. Oracle comparison not run (needs the parser).

Deviations:
- `GetEffectiveShape` base returns a `ShapeNull` (C++: null `shared_ptr`).
- Items store a net code, not a `NETINFO_ITEM*`: code 0 resolves to the board's net 0 where
  C++ may still point at the static `OrphanedItem()` (whose netclass is a fresh
  `NETCLASS( "Default" )`); an item without a board takes `Netclass::fallback()`.
- `GetDisplayNetname` does not rebuild dirty display names (C++ const-mutates the list).
- `SetNetCode`: no `wxASSERT`, `aNoAssert` ignored; no `InvalidateClearanceCache`.
- `SetCustomProperty` case folding uses Rust one-to-one lower-case mapping (`wxTolower`).
- `PCB_GENERATOR` subclass overrides of Move / Rotate / Flip / Mirror (`PCB_VIA_STACK`,
  `PCB_VIA_STITCH`, `PCB_GENERATOR_POLY` outline) are not applied by the cascades (the
  generator is a property map; tier 2, M23).

Stubs left: `Board::rotate_item` / `flip_item` / `mirror_item` for tables (`PCB_TABLE::Rotate`
needs `PCB_TABLE::Normalize`, Flip / Mirror reorder cells and columns: table package);
`board_connected_item::own_clearance` with rules for an item without a handle (needs an
`ItemRef` of a generic `self`; change request filed). Frozen `item_ref.rs` stubs
`layer_name`, `fp_relative_position`, `netname` can now forward to
`board_item::layer_name`, `board_item::fp_relative_position`,
`board_connected_item::netname` (change request).

## Package M1: containers, UUID index, journal, create / attach

| C++ | Rust | State |
|---|---|---|
| `BOARD::Add` (every leaf kind, footprint, table) | `board/containers.rs` `add_*`, `add`, `add_marker`, `attach_footprint`, `attach_table` | ported (list per kind, `push_front` unless `APPEND` / `BULK_APPEND`, `ClearEditFlags`, `SetParent`, `CacheItemById` + `CacheChildrenById`) |
| `BOARD::Remove`, `FOOTPRINT::Remove`, delete, `PCB_TABLE::DeleteMarkedCells` | `remove`, `remove_many` (one retain pass per list) | ported; nets forward to `remove_net` (M3) |
| `BOARD::RemoveAll` | `remove_all` | ported (`PCB_NETINFO_T`: `NetinfoList::clear` = `detachAll` + delete) |
| `BOARD::FinalizeBulkAdd` / `FinalizeBulkRemove` | journal entries recorded by `add_*` / `remove_many` / `remove_all` | listeners -> journal; `noteDrillModelChange` belongs to the drill writer's caches |
| `FOOTPRINT::FOOTPRINT( BOARD* )` (mandatory fields) | `create_footprint` | ported (fields REFERENCE F_SilkS visible, VALUE F_Fab visible, DATASHEET / DESCRIPTION F_Fab hidden; reaches `EDA_TEXT::SetVisible`, M10) |
| `FOOTPRINT::Add` | `footprint_add` | ported (front unless exactly `APPEND`, fields `push_back`, `CacheItemSubtreeById` iff the footprint is indexed, `InvalidateGeometryCaches`) |
| `PCB_TABLE` constructor / `AddCell` | `create_table`, `table_add_cell` | ported (`AddCell` reaches `PCB_SHAPE::SetLayer`, M9) |
| `BOARD::RunOnChildren`, `FOOTPRINT` / `PCB_TABLE` / `PCB_GROUP::RunOnChildren` by handle | `run_on_children`, `item_run_on_children`, `direct_children` | ported (the `ItemRef` forms in footprint/pcb_table/pcb_group stay with M19 / M11 / M23) |
| `BOARD::AllConnectedItems`, `GetPads`, `collectOwnedItems`, `GetItemSet` | `all_connected_items`, `get_pads`, `collect_owned_items`, `get_item_set` | ported |
| `EDA_GROUP::AddItem` / `RemoveItem` / `ContainsItem` | `add_to_group`, `remove_from_group`, `group_contains_item` | ported (two-sided) |
| `BOARD_ITEM_INDEX` (`BindItem`, `Unbind`, `UnbindItem`, `Clear`, `Rebind`, `Bind/UnbindSubtree`, `Bind/UnbindChildren`) | `board/index.rs` (`bind_item`, `unbind_uuid`, `unbind_item`, `clear_item_by_id_cache`, `rebind_item_uuid_index`, `bind_subtree` ...) | ported; one forward map (see deviations) |
| `BOARD::ResolveItem`, `IsItemIndexedById`, `RebindItemUuid`, `BOARD_ITEM::SetUuid`, `RepairDuplicateItemUuids` | `resolve_item`, `is_item_indexed_by_id`, `rebind_item_uuid_index`, `rebind_item_uuid`, `repair_duplicate_item_uuids` | ported |
| `BOARD_LISTENER` | `board/journal.rs` (`Added` / `Removed` / `Modified` with the values before the edit) | ported, off by default |

Tests (`board::containers::tests`): ports of `qa/tests/pcbnew/test_board_item_index.cpp`
`ResolveItemIdentityCachePurgedOnDestruction`, `AttachedItemUuidRewriteDropsOldAlias` +
`ResolvingNewUuidPurgesStaleOldAlias`, `RebindItemUuidUpdatesCacheAtomically`,
`AttachedChildSetUuidRebindsCache`, `RepairDuplicateItemUuidsKeepsEarlierTraversalWinner`,
`RemovingResolvedChildEvictsCacheOnFootprintHolder`, `TableCellResolvesToTheCellNotItsTable`,
`FootprintTableCellResolvesByItsOwnId`; new tests for the `ADD_MODE` order rules of board and
footprint lists, unattached-footprint indexing, cascade removal of the frame fixture, two-sided
group membership and its refusals, `RemoveAll`, traversal orders and the journal.
`create_footprint_has_the_four_mandatory_fields` is `#[ignore]` until `EDA_TEXT::SetVisible`
(M10) lands. Not ported: `UncachedItemSurvivesBoardDestruction`,
`IndexedItemSurvivesBoardDestruction`, `UnbindingThroughAnotherBoardKeepsTheOwner`,
`UnbindingForeignSubtreesDoesNotScanTheIndex` (pointer lifetime / cross-board ownership that
handles cannot express), `SwapItemDataReindexesFootprintChildren` (SwapItemData is deferred),
`CacheItemByIdCanonicalizesRewrittenUuid` (a UUID cannot change behind the index outside the
crate: `modify_*` re-binds).

Deviations:

- The index is one `HashMap<Kiid, ItemId>`; the C++ reverse map (item -> UUID) is "the entry of
  the item's current UUID, if it names the item", which is equivalent because a UUID can only
  change through `modify_*` / `rebind_item_uuid`, both re-binding. A stale alias can only be
  produced by crate-internal raw writes; `resolve_item` ignores it like `BOARD_ITEM_INDEX::Find`.
- `resolve_item` takes `&self` and does not cache the hit of its linear scan (C++ does); only
  the cost differs. The board's own UUID and `DELETED_BOARD_ITEM` have no handle (`None`).
- `remove` deletes (C++ `Remove` leaves ownership to the caller); removing a group clears the
  back reference of its surviving members (`EDA_GROUP::RemoveAll`, which C++ leaves to
  `BOARD_COMMIT`), and `remove_all` does the same for removed groups. Removing a footprint child
  also leaves its group (C++: the commit does it).
- `add_to_group` uses the member's own `base.group` where C++ calls the virtual
  `GetParentGroup()` (differs only for a table cell that is itself a group member).
- The journal records every `add_*` (bulk modes included, where C++ defers to
  `FinalizeBulkAdd`) and children added to an attached footprint (C++: commit); entries read
  the item's layer set and so reach the type packages' `GetLayerSet`.
- `create_footprint`, `create_table`, `table_add_cell`, `footprint_add`, group edits and
  `rebind_item_uuid` bump the time stamp (C++ does not; harmless, `board-model.md` 8.5).
- The mandatory fields of `create_footprint` keep the default library position instead of an
  explicit `SetTextPos( parent->GetPosition() )` (identical for a new footprint at the origin).

Change made outside the package files: `board/mod.rs` (M0a), the two journal hooks only
(`mut_env_refreshed` notes the before-values, `after_modify` records `Modified`; the
`port_pending!` there was the journal recording of this package).

## Package M13: padstack and pad utilities

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `PADSTACK::CopperLayer` (const and mutable), `ForEachUniqueLayer`, `UniqueLayers`, `EffectiveLayerFor`, `RelevantShapeLayers`, `HasExplicitDefinitionForLayer` | `padstack.rs` (`copper_props`, `copper_props_mut`, ...) | ported |
| `m_copperProps` (`std::unordered_map`) | `padstack::CopperPropsMap` | ported: libstdc++ g++ 13 node order (bucket = id % count, insert at bucket begin / list front, `_Prime_rehash_policy` 13 / 29 / 59, copies keep order and bucket count, `clear()` keeps the bucket count), `board-model.md` 10.1 |
| `PADSTACK::operator=`, `operator==`, `Compare`, `Similarity`, `FlipLayers`, `GetMaxHoleSize`, `PostMachiningKnockout`, `MachiningKnockout`, `IsTented` / `IsCovered` / `IsPlugged` | `assign_from`, `eq_padstack`, `compare`, `similarity`, `flip_layers`, `max_hole_size`, `post_machining_knockout`, `machining_knockout`, `is_*` | ported |
| per-layer getters / setters (`SetSize`, `RoundRectRadius`, `SetRoundRectRadius`, `SolderMaskMargin` & co., `DefaultThermalSpokeAngleForShape`, `ThermalSpokeAngle`, ...), mutable `Offset` / `ChamferPositions` | `padstack.rs` | ported (`offset_mut`, `chamfer_positions_mut` added) |
| `AddPrimitive`, `AppendPrimitives`, `ReplacePrimitives`, `ClearPrimitives` | `padstack.rs` | ported |
| backdrill accessors (`findBackdrillDrill`, `backdrillWriteSlot`, `clearBackdrillSide`, `Get/SetBackdrillMode`, `...Size`, `...EndLayer`) | `padstack.rs` | ported |
| `SHAPE_PROPS` / `COPPER_LAYER_PROPS` / `MASK_LAYER_PROPS` / `DRILL_PROPS` / `POST_MACHINING_PROPS` `Compare`, `COPPER_LAYER_PROPS::operator==` / `Similarity` | `padstack.rs` | ported; `COPPER_LAYER_PROPS::Compare` of two equal-length non-empty primitive lists calls the `port_pending!("EDA_SHAPE::Compare")` marker (no `EDA_SHAPE::Compare` port or stub exists yet, M7) |
| `PADSTACK::Serialize` / `Deserialize` / `unpackCopperLayer` | - | skipped: protobuf API |
| `PAD_UTILS::GetDefaultIpcRoundingRatio`, `PadHasMeaningfulRoundingRadius`, `RemoveAllDrillsFromPadstack`, `GetPadAttributeString`, `CheckPad` (+ `checkPadLayer`) | `pad_utils.rs` | ported (messages untranslated); `CheckPad` reaches M14 / M15 pad stubs (`GetSize`, `GetBoundingBox`, `TransformShapeToPolygon`, `GetEffectiveHoleShape`, `MergePrimitivesAsPolygon`, layer set) at run time |

### Tests

`qa/tests/pcbnew/test_backdrill_properties.cpp`: all 10 cases. `test_padstack_clone.cpp`:
`InnerLayerKeepsInheritedSize`, `NewLayerOwnsItsPrimitives`, `CustomLayerEditKeepsInheritedSize`
(padstack part; `HalfTurnedPadComparison` needs `FOOTPRINT::FootprintNeedsUpdate`, M20).
`test_pad_setters.cpp` `SetAttributeClearsAllDrilling` (padstack part). `CopperPropsMap` order
against golden orders printed by a g++ 13 `std::unordered_map` driver (insert, rehash to 29 / 59,
copy + `clear()` + re-insert); the CUSTOM pad 05e66fff-... of `padstacks_complex.kicad_pcb`
against the oracle (`unique_layers=[B.Cu,In2.Cu,In1.Cu,F.Cu]`, `round_rect_radius`, inherited
anchor / spoke angle). `pcb-dump defaults`: all 153 `padstack.*` lines of `PAD` / `PCB_VIA` match
the oracle. `test_pad_check.cpp` (two NON_COMPONENT cases) not ported: needs the M14 pad getters
and a footprint fixture; port with M14.

### Deviations / notes

- Supersedes the M0b note: `m_copperProps` is no longer a `BTreeMap`; iteration follows libstdc++.
- The C++ `operator==` checks `(bool) m_customName != (bool) m_customName` (a self-comparison),
  so a custom name on one side only is not a difference; ported as is (`Compare` does see it).
- `abs( double )` in the `Compare` functions is taken as `std::abs( double )` (fabs).
- Primitives are values: `CopperLayer()`'s "new layer owns its primitives" copies with
  `detached_copy()` and keeps their parent (the C++ `SetParent( m_parent )` sets the same owner);
  `Similarity` treats two non-empty primitive lists as different (C++ compares `shared_ptr`s).
- `VECTOR2I * 1.1` in `SetBackdrillMode` is computed as `Vector2D` and converted with the
  clamping converting constructor (`cast`), as C++.

## Package PCBIO-01: board file parser core and board skeleton

### C++ to Rust map

| C++ (`pcbnew/pcb_io/kicad_sexpr/`) | Rust (`src/io/`) | state |
|---|---|---|
| `pcb_io_kicad_sexpr.h` version constants (`SEXPR_BOARD_FILE_VERSION`, `BOARD_FILE_HOST_VERSION`, `LEGACY_*`, `FIRST_*`, `SEXPR_FOOTPRINT_LIB_FILE_VERSION`) | `mod.rs` | ported (`CTL_*` flags: PCBIO-02) |
| `PCB_IO_KICAD_SEXPR::DoLoad` (new load: parse, `FUTURE_FORMAT_ERROR` conversion, "This file does not contain a PCB.") | `mod.rs` `parse_board`, `parse_board_report` | ported (append / preserved stackup: PCBIO-16) |
| `.kicad_mod` entry (`FP_CACHE::Load` -> `ParseFootprintLib`) | `mod.rs` `parse_footprint_file`, `parser/fp_lib.rs` | stub (PCBIO-14) |
| parser class, `GROUP_INFO` / `GENERATOR_INFO` / `CONSTRAINT_INFO`, `init`, `skipCurrent`, `pushValueIntoMap`, `getNetCode`, `parseBoardUnits` (3 forms), `parseInt`, `parseHex`, `parseBool`, `parseOptBool`, `parseMaybeAbsentBool`, `parseFrontBackOptBool`, `parseNet`, `GetRequiredVersion`, `parseXY`, `parseOutlinePoints`, `parseMargins`, `parseBoardProperty`, `parseCustomProperty` (2 forms), `lookUpLayer`, `lookUpLayerSet`, `parseBoardItemLayer`, `parseBoardItemLayersAsMask`, `parseLayersForCuItemWithSoldermask`, `CurStrToKIID` | `parser/mod.rs` `PcbParser` | ported |
| `IsValidBoardHeader`, `Parse`, `finishItem`, `parseBOARD`, `parseBOARD_unchecked` (loop and post-processing), `parseHeader`, `parseGeneralSection`, `parsePAGE_INFO`, `parseTITLE_BLOCK`, `parseLayer`, `createOldLayerMapping`, `parseLayers`, `parseNETINFO_ITEM` | `parser/board.rs` | ported |
| every other `parseXXX` | `parser/{tracks,shapes,setup,text,footprint,pad,zone,group,dimension,netclass_embedded,misc_items,generator,fp_lib}.rs` | staging stubs with the final signatures (PCBIO-03 .. 14) |
| `checkpoint()` (progress reporter, wall clock), query-user callback, undefined-layer rescue dialog | - | skipped: GUI (headless branch ported) |
| `remapAppendedLayers`, `m_appendToExisting` / `m_preserveDestinationStackup` branches | `parser/board.rs` (flags plumbed, always false) | skipped: PCBIO-16 |

Staging: a stub called with `ParseOptions::allow_pending == false` fails with the `IO_ERROR`
"port pending: <token> (PCBIO-nn)"; with `true` it skips the balanced list (`skipCurrent`) and
records `(token, line, package)` in `ParseReport::skipped`. Item parsers return `Ok(None)` while
they are stubs; `parseBOARD` adds what they return with `AddMode::BulkAppend` (footprints and
tables: `attach_footprint` / `attach_table` of a created container).

### Tests

`src/io/parser/tests.rs`: helper semantics (`board_units` incl. the scientific notation of
`ScientificNotationLoading`, `maybe_absent_bool`, `bools`, `kiids`, `required_version`,
`layer_lookups`, `outline_points`, `misc_helpers`, `init_maps`, `staging_stubs`); error texts
of the reference build (`error_messages`, `layer_errors`, `footprint_is_not_a_board`); the
corpus (`corpus_boards`: all 371 boards of `qa/data/pcbnew` + `demos` below 20 MB with
`allow_pending`, only `plugins/fakeboard.kicad_pcb` fails, with the oracle's message;
`corpus_large`, `#[ignore]`: the four boards of 20 MB and more); `test_kicad_sexpr.cpp`
`ScientificNotationLoading` (full check once PCBIO-03 and `GetArcMid` land); the differential
oracle `oracle_board_section`: the skeleton fields of `pcb-dump board --no-init` (file version,
generator, copper count, enabled layers, layer names and types, page, title block, properties,
net names) of 63 corpus boards (the smallest of each file format version) and 4 hand-made boards
(`tests/data/pcbio01/`), golden `tests/golden/pcbio01_board_section.txt` written by
`tests/oracle/pcbio01_board_golden.sh`. Tests that reach a `port_pending!` model stub report it
as pending instead of failing (`model()` in the test file); mismatches fail.

### Deviations / notes

- `GetRequiredVersion` formats the date as the reference build's C locale `%x` does
  (`MM/DD/YY`, checked: 29990101 -> "01/01/99"); a host in another locale would print
  differently. Days per month use the Gregorian calendar (`wxDateTime` default).
- `parseLayer` / `parseNET*` read names with `CurText()` / `FromUTF8()`; a name that is not
  valid UTF-8 gets the lexer's lossy text (the C++ `From_UTF8` falls back to the current locale).
- `parse_board` takes `&str`: callers convert file bytes (`from_utf8_lossy` in the dump hook).
- `FinalizeBulkAdd` (listeners, drill model generation), `m_LegacyVisibleLayers` (GUI state,
  never written back), `RefreshDrillSymbolLayers` (view-only layer / placement state) have no
  Rust counterpart: no field on `Board`.
- `parseNet` on a local item: `SetNetCode` uses the item's `is_on_copper_layer` and its parent
  chain (`GetBoard()`); `SetNet` is passed in by the caller (`parse_net_with`) because `ZONE`
  overrides it.
- `pushValueIntoMap` with a negative file net code is ignored (the C++ would `resize` to a huge
  size and abort).
- Empty input fails with "Unexpected ''end of input''"; not oracle-checked (the reference QA
  stream load path does not terminate on an empty file).

### Known gaps

- Model calls that are still stubs when this package landed (the parse panics with their name
  until they land; the tests report them as pending): `BOARD::SetLayerDescr`,
  `BOARD::SetEnabledLayers` / `BOARD_DESIGN_SETTINGS::SetEnabledLayers`, `BOARD::GetLayerName`,
  `BOARD::GetLayerType`, `EMBEDDED_FILES::UpdateFontFiles`, `BOARD::FixupEmbeddedData`,
  `FOOTPRINT::RunOnChildren`, `PCB_BARCODE::AssembleBarcode`.
- No model API yet (marked with `port_pending!` at the call site, reached only when such items
  exist): `EDA_TEXT::ResolveFont`, `ZONE::SetLayerSetAndRemoveUnusedFills`.
- `resolveGroups` / `resolveConstraints` are PCBIO-09 / PCBIO-13 stubs: exact (no-op) while no
  declaration was recorded, i.e. while their declaring parsers are stubs too.

## Package DUMP-RS: Rust side of the reference dump

Files: `src/dump/{mod,format,board,items,settings,defaults,compare,tests}.rs` (crate feature
`dump`), `src/bin/pcb_dump_rs.rs` (bin `pcb-dump-rs`), `tests/pcb_dump_defaults.rs`,
`tests/golden/pcb_dump_defaults.txt`, `rust/tools/pcb-dump/compare.py`.

- Mirrors `rust/tools/pcb-dump/*.cpp` file by file and field by field (same keys, field names,
  order and value formatting, `--hash` = FNV-1a). Every field is evaluated under
  `catch_unwind` with a silent, reference-counted panic hook: `port_pending!` gives
  `pending|SECTION|KEY|FIELD=CXX::Name`, other panics `rust-error|...=message`. A failed
  *condition* (a pending `IsOnLayer` deciding which fields exist) reports a field pattern
  (`*.F.Cu`, `shape.*`, `a|b`), a pending constructor reports `FIELD=*` for the whole class.
- C++ getters with logic that have **no Rust method yet** are reported as pending with the
  suffix `(no Rust API)` (e.g. `PCB_TRACK::IsNull`, `EDA_SHAPE::GetArcMid`,
  `PAD::GetBoundingBox (aLayer)`, most `PCB_VIA` getters, `PNS::MEANDER_SETTINGS`, which the
  model keeps as JSON). The package that adds such a method replaces the `no_api(..)` call in
  `src/dump/*.rs` by the real call (`rg 'no_api\(' crates/kicad-pcb/src/dump`). Plain C++
  member getters are read from the public Rust fields.
- Constructions in `defaults` follow `dump_defaults.cpp`: items with a default board as parent
  (values, not added); pads, fields and `PCB_TEXT.in_footprint` with a footprint stored in the
  board arena but not listed (fixture convention of `test_support.rs`); the table cell with a
  table stored the same way. `FOOTPRINT` uses `Board::create_footprint`, `ZONE`
  `Zone::new_with_settings` (both pending now: whole class pending);
  `PCB_GENERATOR.PCB_TUNING_PATTERN` has no Rust constructor (pending); the marker's
  `DRC_ITEM::Create( DRCE_CLEARANCE )` is built as an `RcItem` with code and settings key.
- `dump_board(&Board, &DumpOptions)` covers all sections (item, bbox, track, pad, shape, text,
  dim, zone, footprint, misc, board, order) and is unit-tested on the `test_support.rs`
  fixture boards. `pcb-dump-rs board FILE` goes through `dump::load_board_for_dump`, which
  returns "parser not ported" until the parser package fills it in. Order lists without a Rust
  comparator (`PCB_TRACK::cmp_tracks`, `PCB_GROUP::ptr_cmp`) are pending.
- Quirk kept from the C++ tool: `EMIT( "x." + std::to_string( i++ ), .. )` evaluates the field
  twice, so `default_fp_text`, `netclass_pattern` and footprint `pad` / `drawing` / `zone`
  indices are 1, 3, 5, ...
- Defaults scoreboard (2026-11-10): 3013 match, 0 mismatch, 669 pending, 0 rust-error,
  0 missing, 0 extra, 5 ref-error (fields the C++ itself reports as `error|`).
- A user field's ordinal is set by the dump from a local port of
  `FOOTPRINT::GetNextFieldOrdinal` (`dump/defaults.rs`), because `PcbField::new` leaves it to
  the caller and `Footprint` has no such method yet.
- Wrong constructor default found and fixed: `BOARD::m_fileFormatVersionAtLoad` was
  20160815, C++ `LEGACY_BOARD_FILE_VERSION` is 2 (`cmake/config.h.cmake`)
  (`src/board/mod.rs`, test in `src/board/layers.rs`).
- Dump-side interpretations (documented in the code): a `None` netclass parent is the class
  itself (`NETCLASS::ResetParents`), `EDA_TEXT::GetFontName` reads `attributes.font`, enum
  names are the Rust variant names in UPPER_SNAKE case with explicit C++ prefixes where the
  C++ enum is not an `enum class` (`GR_TEXT_H_ALIGN_`, `BS_EDGE_CONNECTOR_`, `BS_ITEM_TYPE_`,
  `LT_`, `RPT_SEVERITY_`,
  the `DRCE_*` table).

## Package M7: EdaShape core

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `EDA_SHAPE` inline setters / getters of `eda_shape.h` (`SetStartX/Y`, `SetEndX/Y`, `SetCenterX/Y`, `SetRadius`, `SetTop/Left/Right/Bottom`, `SetBezierC1/C2`, `SetEllipse*`, `SetFilled`, `GetStartEnding` / `GetEndEnding`, `SetPolyShape`) | `eda_shape/{mod,geometry}.rs`, inherent on `EdaShape` | ported |
| `SHAPE_T_asString`, `ShowShape`, `IsClosed`, `SetFillMode`, `SetWidth`, `SetLineStyle` / `GetLineStyle`, `SetCornerRadius`, `SetRectangle*`, `GetRectangleWidth/Height`, `GetRectCorners` | inherent on `EdaShape`; `rectangle_size(host)` | ported |
| arc maths: `SetArcGeometry`, `SetArcOrSegment`, `SetCachedArcData`, `SetCenter`, `SetArcAngleAndEnd`, `GetArcMid`, `CalcArcAngles`, `GetRadius`, `GetArcAngle`, `GetSegmentAngle`, `IsClockwiseArc` | inherent on `EdaShape` | ported |
| polygon / bezier / ellipse: `GetPolyPoints`, `GetPointCount`, `IsPolyShapeValid`, `SetPolyPoints`, `RebuildBezierToSegmentsPointsList`, `buildBezierToSegmentsPointsList`, `buildShapeEllipse`, `recalcEllipseArcEndpoints` | inherent on `EdaShape` | ported |
| `getPosition`, `setPosition`, `getCenter`, `GetLength` | `EdaShape::{position, set_position, center, length}`; `shape_position` / `shape_center` / `shape_length(host)` | ported |
| `move`, `scale`, `rotate`, `flip`, `mirror( SEG )` | `EdaShape::{move_, scale, rotate, flip, mirror}` (`max_error` = host `getMaxError()`) | ported |
| `getBoundingBox`, `computeArcBBox` | `EdaShape::bounding_box_core` + `shape_bounding_box(host, ctx)` (adds the host width), `compute_arc_bbox` | ported |
| `GetTopLeft`, `GetBotRight`, `GetCornersInSequence` (base bodies) | `top_left`, `bot_right`, `corners_in_sequence` | ported (see deviations) |
| `hitTest( VECTOR2I )`, `hitTest( BOX2I )` | `geometry::hit_test_point` / `hit_test_rect` via `shape_hit_test(_rect)` | ported except the line-ending branch (M8) |
| `Compare`, `operator==`, `Similarity`, `SwapShape`, `alg::longest_common_subset` | `EdaShape::compare`, `impl PartialEq for EdaShape`, `similarity`, `swap_shape` | ported |
| `LINE_ENDING::GetEffectiveSize`, `GetShortenDepth`, `GetCurveOrientationDepth`, `GetShapes`, `StyleToToken`, `TokenToStyle`, `StyleToChoiceIndex`, `s_defaultChoiceOrder` | `line_ending.rs` | ported |
| `hitTest( SHAPE_LINE_CHAIN )`, `LINE_ENDING::Draw` / `Plot` / `Format`, `Serialize`, edit state (`beginEdit` ..), `getFriendlyName`, `ShapeGetMsgPanelInfo`, `UI_FILL_MODE` props | - | skipped: GAL / API / UI; lasso hit test and `Format` belong to the consumers (M8 effective shapes, writer) |

### Tests

`eda_shape::tests` (from qa/tests/common/test_eda_shape.cpp): `SetAngleAndEnd`,
`SetArcGeometry`, `SetArcGeometryKeepsMidWhenEndsSwap`, `GetPolyPointsPreservesOrderAcrossOutlines`,
`EllipseBasicAccessors` / `EllipseArcIsOpenCurve` / `EllipsePerimeterForCircleCase`;
qa/tests/pcbnew/test_shape_corner_radius.cpp `PCBShapeCornerRadius` (on the `EDA_SHAPE` body);
oracle values (`pcb-dump board --sections shape,bbox`) for issue2512 (legacy arc, rect, poly) and
issue18839 (arc): start/end/centre/mid/radius/angles/clockwise/length/bbox/rect size, bit exact;
plus transforms / Compare / hit-test checks. `line_ending::tests` (from
qa/tests/common/test_line_ending.cpp): `GetEffectiveSizeUsesConfiguredAndAutoAxes`,
`GetShapesProducesStyleGeometry`, `OpenArrowOrientationUsesVisualLength`,
`GetShortenDepthUsesEndpointPolicy`. The line-ending shortening / effective-shape tests of
test_eda_shape.cpp belong to M8.

### Deviations / notes

- Transforms that call `getMaxError()` (`scale`, `rotate`, `flip`, `mirror`) take it as a
  `max_error` argument from the host (a `PCB_SHAPE` reads its board through `MutEnv`).
- `getCenter()` of POLY / RECTANGLE / BEZIER uses the bounding box without the width inflation:
  the centre of a box inflated symmetrically by `d >= 0` and normalized equals the centre of the
  normalized box, so the result is identical and no context is needed.
- `corners_in_sequence` (frozen trait signature without `ctx`) inflates by the stored stroke width
  (`EDA_SHAPE::GetWidth`); `PCB_SHAPE::GetWidth` differs only inside a scaled footprint (change
  request: give `EdaShapeItem::corners_in_sequence` a `ctx`).
- `move` drops the hatching cache instead of translating it first (dirty means dropped in this
  port; the translated cache is never read before regeneration outside interactive moves).
- `UNIMPLEMENTED_FOR` branches return 0 / false / (0,0) silently; `getPosition` of an empty
  polygon returns (0,0) (C++ throws), `GetLength` of a polygon without outline returns 0 (C++ UB).
- `hitTest` on a shape with a line ending panics with the M8 port-pending message
  (`MakeEffectiveShapesWithLineEndings`); `GetHatching()` in hit tests reads M8's
  `hatching_data` stub.
- Out-of-range `double -> int` conversions of the hit-test distance (`maxdist` passed to `int`
  parameters) use Rust's saturating `as`; only reachable with accuracies near `INT_MAX`.

## Package M16: tracks and arcs (PCB_TRACK, PCB_ARC)

| C++ (`pcbnew/pcb_track.cpp`, `pcb_track.h`) | Rust | State |
|---|---|---|
| `PCB_TRACK` accessors (`Get/SetStart/End[X/Y]`, `GetEndPoint`, `Get/SetWidth`, `HasSolderMask`, `Get/SetLocalSolderMaskMargin`), `IsNull`, `IsPointOnEnds` (+ `STARTPOINT` / `ENDPOINT`), `ApproxCollinear`, `GetLength` | `pcb_track/mod.rs` (`PcbTrack`, `Track::is_null`) | ported |
| `GetSolderMaskExpansion`, `GetWidthConstraint` | `PcbTrack::` / `PcbArc::solder_mask_expansion`, `width_constraint` (shared bodies `solder_mask_expansion_of`, `width_constraint_of`) | ported; the rule resolver gets the stored item (`this`), or a copy for an item without a handle |
| `IsOnLayer`, `GetLayerSet`, `SetLayerSet`, `GetBoundingBox`, `GetEffectiveShape`, `TransformShapeToPolygon` (segment and arc branches), `HitTest` (point, rect), `Rotate`, `Flip`, `Mirror` (both), `GetItemDescription`, `GetFriendlyName`, `operator==`, `Similarity` | `PcbTrack` / `PcbArc` inherent methods (`eq_item`, `similarity`, `mirror_seg`) | ported |
| `PCB_ARC` (`SHAPE_ARC` constructor, `CreateFromThreePoints`, `Get/SetMid`, `GetPosition` / `GetCenter`, `GetRadius`, `GetAngle`, `GetArcAngleStart/End`, `IsCCW`, `IsDegenerated`, `GetLength`, `Move`, `Rotate`, `Flip`, `Mirror`, `HitTest`s, `GetEffectiveShape`) | `pcb_track/arc.rs` | ported |
| `PCB_TRACK::cmp_tracks` | `pcb_track::pcb_track_cmp_tracks(ctx, TrackId, TrackId) -> Ordering` | ported; pointer tie-break = `TrackId` order (`board-model.md` 3.1) |
| `GetDelay`, `GetCoverageArea`, `HitTest( SHAPE_LINE_CHAIN )`, `Visit`, `View*`, `GetMsgPanelInfo*`, `GetMenuImage`, `Serialize` / `Deserialize`, `GetFocusPosition`, `swapData`, `CopyFrom`, `Clone` | - | skipped: tier 2 length / delay calculation, UI / selection, API; `Clone` / `CopyFrom` are `clone()` / `detached_copy()` |

Tests (`pcb_track::tests`): a reference-dump golden of 40 segments and arcs (bbox, length,
effective shape, position, layer set, solder mask expansion, IsNull, arc centre / radius /
angles / IsCCW / IsDegenerated, and `TransformShapeToPolygon` for three (clearance, maxError,
errorLoc) combos compared by FNV-1a hash), the `test_board_item.cpp` rows for `PCB_TRACK` /
`PCB_ARC` (`Move`, `Rotate`, `FlipLeftRight`, `FlipUpDown`, `BoardlessItemsUseStandardLayerMetadata`),
the null-board and rule-resolver branches of the mask expansion / width constraint, save
order, `IsPointOnEnds`, `ApproxCollinear`, hit tests, texts, `CreateFromThreePoints`,
`operator==` / `Similarity`. The golden comes from `pcb-dump` on `tracks_arcs_vias.kicad_pcb`
upgraded to format 20241229 with `(pad_to_mask_clearance 0.07)` and eleven crafted tracks /
arcs (solder mask openings with positive, negative and no local margin, zero length, a
degenerated arc, half circles, a nearly straight arc); the parser does not load boards yet
(`BOARD::SetLayerDescr` pending), so the test rebuilds the tracks from the dumped members.
Not compared against the reference: hit tests, `GetItemDescription` (no dump field; checked
against the C++ bodies by hand), the rule-resolver branches (need the DRC engine).

Deviations:
- `GetItemDescription` / `GetFriendlyName` are English (`_()` untranslated), as in M2.
- `GetSolderMaskExpansion` is a non-virtual `PCB_TRACK` member that `PCB_VIA` hides; there is
  no `Track` dispatcher for it (call the variant's method).
- `PCB_TRACK::GetFriendlyName` as written (`switch( Type() )`, incl. "Via") is
  `Track::track_friendly_name`; the per-type inherent methods answer for their own type.

Stubs left: none in `pcb_track/{mod,arc}.rs`. `Track` has no `eq_item` / `similarity`
dispatcher until M17 provides the `PCB_VIA` bodies.

## Package M22: board queries (layers, properties, variants, text variables, bounding boxes, markers, comparators)

### C++ to Rust map

| C++ (`pcbnew/board.cpp` unless stated) | Rust | state |
|---|---|---|
| `SetLayerDescr`, `GetLayerID`, `GetLayerName`, `SetLayerName`, `GetLayerType`, `SetLayerType`, `IsLayerEnabled`, `SetEnabledLayers`, `Get/SetUserDefinedLayerCount`, `GetCopperLayerStackMaxId`, `LayerDepth`, `IsFrontLayer` / `IsBackLayer` (board), `GetStackupOrDefault` | `board/layers.rs` | ported; every `m_layers` edit re-runs `recalcOpposites()`, marks the `LayerEnv` stale and bumps the time stamp |
| `pcbnew/layer_utils.cpp` (`AccumulateNames`, `GetAllFootprintLayers`, `GetUsedFootprintLayers`, `MinimalCopperLayerCount`, `GetOrphanedFootprintLayers`) | `board::layers::{accumulate_names, get_all_footprint_layers, ...}` | ported |
| `FindFootprintByReference`, `FindFootprintByPath`, `FindPadByUuid`, `GetPad( pos, layers )`, `GetPad( padList, pos, layers )`, `GetSortedPadListByXthenYCoord`, `GetZoneList`, `GetMaxClearanceValue`, `GetVisibleLayers` (see deviations), `IsFootprintHolder`, `ComputeBoundingBox`, `GetBoardEdgesBoundingBox`, `GroupsSanityCheck(Internal)` | `board/queries.rs` (`get_pad_at`, `get_pad_in_sorted_list`, `visible_layers`, ...) | ported |
| `GetTrackLength` | `queries.rs` | stub: needs `CONNECTIVITY_DATA::GetConnectedItems` and `LENGTH_DELAY_CALCULATION` |
| `pcbnew/board_bounding_box.cpp` (`BOARD_BOUNDING_BOX`) | - | skipped: a view-only `EDA_ITEM` wrapper of a box |
| `HasVariant`, `AddVariant`, `DeleteVariant`, `RenameVariant`, `SetCurrentVariant`, `GetCurrentVariant`, `Get/SetVariantDescription`, `SynchronizeProperties`, `GetContextualTextVars`, both `ResolveTextVar` overloads (`resolve_text_var`, `resolve_text_var_current`), `ConvertCrossReferencesToKIIDs`, `ConvertKIIDsToCrossReferences`, `FixupEmbeddedData`; `TITLE_BLOCK::TextVarResolver` with a project and `PROJECT::TextVarResolver` (board side) | `board/text_vars.rs` | ported, except the `DRILL_OPERATIONS` / `DRILL_SITES` / `DRILL_GROUPS` branch (stub: `DrillSymbolCache`, tier-2 drill model) |
| `CopyVariant`, `GetVariantNamesForUI` | - | skipped: editor / UI only |
| `ResolveDRCExclusions`, `RecordDRCExclusions` | `markers.rs` `resolve_drc_exclusions_with` / `record_drc_exclusions_with` (conversions passed in) | ported; the fixed-signature `resolve_drc_exclusions` / `record_drc_exclusions` stay stubs: `DRC_EXCLUSION::FromMarker` / `ToProto` / `PCB_MARKER::FromProto` are `kicad-drc` (DRC-11), above this crate |
| `UpdateRatsnestExclusions` | `markers.rs` `ratsnest_exclusions` (the marker half) | partial: the edge visibility part belongs to the connectivity crate |
| both `DeleteMARKERs` | `delete_all_markers`, `delete_markers` | ported |
| `BOARD::cmp_items`, `BOARD::cmp_drawings` | `board::compare::{board_cmp_items, board_cmp_drawings}` | ported; pointer tie-break = `ItemId` order (`board-model.md` 3.1) |

Inlined footprint functions (M19 / M20 have no Rust counterpart; each is a few statements,
ported in place and named in the doc comment): `FOOTPRINT::FindPadByUuid`,
`FOOTPRINT::GetPad( pos, mask )`, `FOOTPRINT::DeleteVariant`, `FOOTPRINT::RenameVariant`.
Called through local `port_pending!` helpers until their packages land:
`FOOTPRINT::ResolveTextVar` (cross references), `EDA_SHAPE::Compare`, `PCB_TABLE::Compare`,
`PCB_BARCODE::Compare` (`cmp_drawings`).

### Tests

`board::{layers,queries,text_vars,markers,compare}::m22_tests` (26): ports of
`test_text_variable_resolution.cpp` (`ProjectTextVarsTakePrecedence`,
`ProjectTextVarsUpdateDynamically`, `TitleBlockVarsTakePrecedence`, `BoardPropertiesFallback`;
`PcbTextResolvesProjectVars` needs `PCB_TEXT::GetShownText`), the board cases of
`test_variant.cpp` (`BoardVariantRegistry`, `VariantCaseInsensitivity`, `BoardVariantTextVars`,
`VariantDeletionClearsRegistry`, `RenameVariantPreservesData`), `group_saveload.cpp`
`InvalidGroups`; new tests for the layer table, `layer_utils`, the sorted-pad binary search,
bounding boxes of tracks, the clearance cache, exclusions with stand-in conversions,
`cmp_items`. Oracle: `board::layers::m22_oracle` replays `tests/golden/m22_board_layers.txt`
(`pcb-dump board --sections board` on `tests/data/m22/m22_layers.kicad_pcb`, a crafted 4-layer
board with power / mixed / front / back / auxiliary layers and user names, and on
`via_stack_track_width`, `teardrop_spike`, `drc_missing_tuning_profile`): 243 `layer.<L>.name`,
`layer.<L>.type` and `copper_layer_count` values, all equal. `pcb-dump-rs` vs `pcb-dump` on 7
boards: every layer line equal (the Rust dump prints `SIGNAL` for `LT_SIGNAL`, a formatter
difference of `dump/`); the four `ComputeBoundingBox` values match on the empty board and
cannot be compared on the others yet because the parser skips every item (PCBIO-03.. pending)
and most item `GetBoundingBox` are stubs.

### Deviations / notes

- `GetVisibleLayers`: `BoardProject` carries no `.kicad_prl` visibility; every layer is visible
  (the `PROJECT_LOCAL_SETTINGS` default). `ComputeBoundingBox` therefore uses every layer.
- `PROJECTNAME` resolves to the board file name without extension (`BoardProject` has no
  project name; `kicad-cli` binds `<board>.kicad_pro`). `VCSHASH` / `VCSSHORTHASH` resolve to
  "" (no git in library code). `CURRENT_DATE` / `CURRENT_TIME_*` use
  `TitleBlockClock::default()` (empty) - no wall clock and no clock field on `Board`.
- `FILEPATH` is the stored file name verbatim (`wxFileName::GetFullPath` without
  normalisation).
- `GroupsSanityCheckInternal` walks groups in `ItemId` order instead of the
  `unordered_set<EDA_GROUP*>` hash order; the result text is the same, with `repair` the group
  removed from a cycle may differ.
- `GetMaxClearanceValue` is cached until the next time-stamp bump regardless of the rule
  resolver of later calls, as the C++ cache ignores a later-attached engine.
- `LayerDepth` with a non-copper layer (C++ throws from `LAYER_RANGE`) is a `debug_assert!`
  and returns 0.
- `BumpDrillModelGeneration()` calls (layer renames, counts) have no counterpart (tier 2).

## Package M5: design settings, zone settings, teardrop parameters (model logic)

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `BOARD_DESIGN_SETTINGS::SetEnabledLayers`, `IsLayerEnabled`, `SetBoardThickness`, `Set/GetAuxOrigin`, `Set/GetGridOrigin`, `Get/SetDefaultZoneSettings`, `GetTeadropParamsList`, `GetCurrentNetClassName`, `GetStackupDescriptor` | `board_design_settings.rs` | done |
| `GetBiggestClearanceValue` (`m_DRCEngine->QueryWorstConstraint` = `RuleResolver::query_worst_constraint`), `GetSmallestClearanceValue`, `GetDRCEpsilon`, `GetHolePlatingThickness`, `MAXIMUM_CLEARANCE`, `MINIMUM/MAXIMUM_ERROR_SIZE_MM`, `MINIMUM/MAXIMUM_LINE_WIDTH_MM` | `board_design_settings.rs` | done |
| track width / via size / diff pair lists: `Set/Get*Index`, `GetNext*Index` (`nextPredefinedIndex`), `GetCurrentTrackWidth`, `GetCurrentViaSize`, `GetCurrentViaDrill`, `GetCurrentDiffPairWidth/Gap/ViaGap`, `UseNetClassTrack/Via/DiffPair`, custom size setters / getters, `UseCustomTrackViaSize`, `UseCustomDiffPairDimensions`, `Get/SetViaStackIndex` | `board_design_settings.rs` | done |
| `ValidateDesignRules`, `GetDesignRuleWarnings`, `VALIDATION_ERROR` | `board_design_settings.rs` | done (untranslated message texts) |
| copy constructor / `operator=` (`initFromOther`) | `BoardDesignSettings::detached_copy` / `init_from_other` | done (BOM members dropped) |
| `BOARD_DESIGN_SETTINGS::operator==` | - | not ported: needs `NET_SETTINGS::operator==` (no port yet); only dialogs use it |
| `ZONE_SETTINGS::operator<<( const ZONE& )` | `ZoneSettings::import_from_zone` (zone not on a board: `IsLocked()` false), `import_from_zone_ctx` (board zone) | done; runs once M18 lands (`Zone::local_clearance` stub) |
| `ZONE_SETTINGS::ExportSetting` | `ZoneSettings::export_setting` (detached zone, null-board env, net 0), `export_setting_env` (returns the net code instead of calling `SetNetCode`), `Board::export_zone_setting` (board zone: `modify_zone` + `set_item_net`) | done; runs once M18 lands (`Zone::set_fill_mode`, `set_min_thickness`, `set_corner_radius`, `set_layer_set`, `set_border_display_style` stubs) |
| `ZONE_SETTINGS::SetupLayersList`, `LAYER_PROPERTIES_GRID_TABLE` | - | skipped: GUI |
| `teardrop_parameters.cpp/.h` | `teardrop_parameters.rs` | done (M0b; nothing left for M5) |

### Tests

`board_design_settings::tests`: ported from `qa/tests/pcbnew/test_board_design_settings.cpp`:
`BiggestClearanceIncludesPhysicalHoleClearance` (engine replaced by a `RuleResolver` answering
`QueryWorstConstraint`), `PredefinedTrackWidthCyclingMonotonic`,
`PredefinedViaSizeCyclingMonotonic`, `PredefinedDiffPairCyclingMonotonic`,
`TrackWidthSwitchAdvancesFromConnectedWidth`; the JSON round-trip cases of that file belong to
the JSON package. Own tests: derived getters (values from `pcb-dump defaults`),
`SetEnabledLayers` counts, `ValidateDesignRules` / `GetDesignRuleWarnings` messages, copy
constructor vs assignment.

Oracle: `pcb-dump defaults` vs `pcb-dump-rs defaults`: all 344 `BOARD_DESIGN_SETTINGS|`,
`ZONE_SETTINGS|`, `TEARDROP_*|` lines and all 283 `BOARD|ds.*` lines the Rust dump emits are
identical (incl. `biggest_clearance`, `smallest_clearance`, `drc_epsilon`,
`hole_plating_thickness`, `enabled_layers`, default zone settings); the 63 `meander.*` lines
are not emitted by the Rust dump (the tuning settings are JSON objects, see the M0d section).

### Deviations / notes

- `GetDRCEpsilon` / `GetHolePlatingThickness` return the built-in `ADVANCED_CFG` defaults
  (0.0005 mm, 0.020 mm): library crates read no advanced config file.
- `ValidateDesignRules` checks the via list with a private port of the two-argument form of
  `PCB_VIA::ValidateViaParameters` (`validate_via_diameter_and_drill`; the layer and
  secondary / tertiary checks are no-ops for the defaulted arguments). The full function
  belongs to `pcb_track/via.rs`.
- `Board::export_zone_setting` applies `SetNetCode` after `SetBorderDisplayStyle` (C++:
  before); the border hatch does not depend on the net.
- `import_from_zone` reads `Zone::layer_set` directly: `ZONE::GetLayerSet()` returns
  `m_layerSet` (the trait method takes `ctx` only by rule C1).
- `init_from_other` clones `NetSettings` (C++ shares the `shared_ptr`).

Stubs left: none in M5's files.

## Package M10: text (EDA_TEXT, PCB_TEXT, PCB_FIELD)

| C++ | Rust | State |
|---|---|---|
| `EDA_TEXT` setters (`SetText`, `cacheShownText`, `SetVisible`, `SetMirrored`, `SetMultilineAllowed`, `SetKeepUpright`, `SetHoriz/VertJustify`, `SetBold[Flag]`, `SetItalic[Flag]`, `SetLineSpacing`, `SetFont`, `SetAttributes`, `SetHyperlink`, `ResolveFont`, `SetupRenderCache`, `AddRenderCacheGlyph`, `IsDefaultFormatting`, `isStrokeFont`, `GetDrawFont`) | `eda_text.rs` inherent methods of `EdaText` (`draw_font_lookup`, `is_stroke_font`, `font_ref`) | ported |
| `EDA_TEXT` virtual base bodies (`GetShownText`, `SetTextPos`, `SetTextSize`, `SetTextThickness`, `SetTextAngle`, `Offset`) and `SetAutoThickness`, `MigrateLegacyBoldStrokeWidth`, `TextHitTest` (both), `GetRenderCache` | generic functions in `eda_text.rs` | ported |
| `GetTextBox`, `GetEffectiveTextPenWidth`, `GetEffectiveTextShape`, `GetLinePositions`, `GetInterline`, `Compare`, `GetTextStyleName` | `eda_text.rs` (`EdaTextExt`) | ported |
| `PCB_TEXT` overrides (`GetTextPos/Size/Thickness/Angle`, setters, `Offset`, `GetDrawRotation`, `GetShownText`, `GetBoundingBox`, `TextHitTest`, `GetEffectiveShape`, `TransformTextToPolySet`, `buildBoundingHull`, `TransformShapeToPolygon`, `GetKnockoutCache`, `Rotate`, `Mirror`, `Flip`, `KeepUpright`, `OnFootprintTransformed/Rescaled`, `StyleFromSettings`, `GetItemDescription`, `SetLibText*`) | `pcb_text.rs`; bodies reaching `GetShownText` are generic over the host (`PcbTextHost`) and shared with `PcbField` | ported |
| `PCB_FIELD` (`IsMandatory`, `GetOrdinal`, `GetName`, `GetShownText` both, `GetTextTypeDescription`, `GetItemDescription`), `GetDefaultFieldName` / `GetUserFieldName` (untranslated) | `pcb_field.rs` | ported |
| `KIUI::EllipsizeMenuText` | `pcb_text::ellipsize_menu_text` | ported |

Tests: `eda_text::tests` (`qa/tests/common/test_eda_text.cpp`: `Compare`,
`BoldFlagLeavesStoredThicknessAlone`, `EffectiveWidthAppliesBoldMultiplierForStroke`,
`EffectiveWidthAutoBoldMatchesLegacy`, `MigrateLegacyBoldStrokeWidthRecoversBase`,
`AutoThicknessOffFreezesBaseNotEffective`; the hash checks and the `TextVarReferences_*` cases
are not ported: no `std::hash<EDA_TEXT>`, no reactive reference tracker), plus shown-text
caching and size clamping. Oracle (`pcb-dump board --sections text,bbox`, values in the
tests): `pcb_text::tests::oracle_multiline_board_text` / `oracle_mirrored_board_text`
(`qa/data/pcbnew/incorrect_text_mirroring_drc.kicad_pcb`), and
`oracle_footprint_texts_scaled_rotated` (two fields and a user text in a footprint rotated 30
degrees and scaled 1.5 x 0.75, keep-upright fold, mirror, left/right justification): text
pos/size/thickness, draw rotation, `GetTextBox`, `GetBoundingBox`, interline,
`GetEffectiveTextShape` segments, `TransformTextToPolySet` polygons - all bit-exact.
`footprint_text_mutators_round_trip` (frame fixture), `pcb_field::tests` (names, ordinals,
shown text, field geometry through the field's own `GetShownText`).

Deviations:
- Outline fonts: `m_Font` is a name; geometry is laid out with the stroke font
  (`kicad-font` D1). `GetRenderCache` returns the stored glyphs only when the cache is valid
  (font = `FontRef { name, bold, italic }`, text, draw rotation, offset, mirroring); where C++
  would regenerate it the stroke font is drawn instead. `isStrokeFont` follows the requested
  face (outline faces get no bold multiplier), `GetTextBox`'s stroke fudge follows the layout
  font. The parser must set `render_cache.font` with `EdaText::font_ref()` (or call
  `resolve_font`, which does).
- `EDA_TEXT::SetTextPos` base body: the current position is computed from `MutEnv::parent`
  (= `PCB_TEXT::GetTextPos`); exact for texts, fields, dimensions and barcode text. Text boxes
  and cells (no `GetTextPos` override) would see their `m_pos` mapped through the footprint
  transform; their board code does not use `m_pos`.
- `PCB_TEXT::Flip`: `IsSideSpecific()` uses the layer-set test only (`MutEnv` carries no
  `LAYER_T`; a user layer typed front/back is missed).
- `PCB_TEXT::StyleFromSettings`: the trait method has no `MutEnv`, so `SetTextSize` /
  `SetTextThickness` run without the parent scale, and `BOARD::IsBackLayer` is
  `IsBackLayer( layer )`.
- `GetKnockoutCache`: `aMaxError` is `GetMaxError()` (the painter passes its own value).
- `IsDefaultFormatting`: `GetAutoThickness` reads the stored width.
- `GetShownText` overrides: `ResolveTextVars` is inlined (`resolve_text_vars_shared_depth`) so
  that the resolver sees the pass depth as the C++ lambda (captures `aDepth` by reference);
  no expression evaluator (text_eval not ported). Texts are untranslated.
- `PCB_FIELD::GetItemDescription` without a parent footprint uses an empty reference (C++
  dereferences null). `-INDETERMINATE` justification stays indeterminate.
- `Compare` returns -1/0/1 for the string comparisons (C++ `wxString::Cmp`: only the sign is
  specified); position differences wrap like x86 `int`.

Stubs left (depend on M19): `FOOTPRINT::ResolveTextVar` (`pcb_text::footprint_resolve_text_var`)
and `FOOTPRINT::GetFieldValueForVariant` (`pcb_field.rs`) - neither is declared on `Footprint`
yet; reached only by footprint texts with `${...}` and by fields of a footprint with a variant
override.

## Package M23: groups, generators and small items

| C++ | Rust | State |
|---|---|---|
| `PCB_GROUP::GetBoundingBox` (footprints with text, +0.25 mm), `GetPosition`, `GetLayerSet`, `IsOnLayer`, `GetEffectiveShape` (compound), `RunOnChildren`, `GetItemDescription`, `operator==`, `GetBoardItems`; `EDA_GROUP::GetGroupMemberIds` | `pcb_group.rs` | ported |
| `PCB_GROUP::TopLevelGroup`, `WithinScope` (`getClosestGroup`, `getNestedGroup`) | `pcb_group::{top_level_group, within_scope}` | ported (handles; `None` scope = no group) |
| `PCB_GENERATOR::GetLayer` / `GetLayerSet` / `GetPosition` / `GetBoundingBox` / `HitTest` (point, rect) / `GetItemDescription` + the overrides of `PCB_TUNING_PATTERN`, `PCB_VIA_STACK`, `PCB_VIA_STITCH` | `pcb_generator.rs` (dispatch on `generator_type`) | ported; `PCB_TUNING_PATTERN::getOutline` ported (`TuningPatternData::outline`) |
| `SetProperties` + `GetProperties` of `PCB_GENERATOR` and the three subclasses, `STRING_ANY_MAP` (`get_to`, `get_to_iu`, `get_opt`, `set`, `set_iu`), `MEANDER_SETTINGS` constructor / target setters, token helpers | `PcbGenerator::normalize_properties`, `TuningPatternData`, `ViaStackData`, `ViaStitchData` (typed views read from the map) | ported |
| `GENERATORS_MGR::CreateFromType`, `LayerFollowsMembers` | `PcbGenerator::create_from_type`, `layer_follows_members` | ported (name = display name, layer F_Cu) |
| `PCB_POINT` (`SetPosition`, `recomputePosition`, `OnFootprintTransformed`, `Rotate`, `Flip`, `Mirror` both, `GetBoundingBox`, `GetEffectiveShape`, hit tests, description, `operator==`), `PCB_POINT::cmp_points` | `pcb_point.rs` | ported |
| `PCB_TARGET` (rotate, flip, bbox, shape, `TransformShapeToPolygon`, hit tests, description, `operator==`) | `pcb_target.rs` | ported (`TransformShapeToPolygon` reaches `PCB_SHAPE` setters, M9) |
| `PCB_MARKER::GetSeverity`, `MARKER_BASE::IsTreatedAsExcluded`, `ShapeToPolygon`, `GetBoundingBoxMarker`, `HitTestMarker` (point, rect), `PCB_MARKER` bbox / shape / hit tests / description; `RC_ITEM::GetErrorMessage`, `SetErrorDetail` | `pcb_marker.rs` | ported; `MarkerScale()` is the constructor's `SCALING_FACTOR` (162500) |
| `PCB_CONSTRAINT::GetEffectiveShape`, `GetItemDescription`, `RemapKIIDs`, `operator==`; `ConstraintsAreDuplicate`, `ConstraintTypeLabel` | `pcb_constraint.rs` | ported |
| `PCB_3D_BODY::Flip` | `pcb_3d_body.rs` | ported |
| `PCB_GRID_ITEM` (`Rotate`, `Flip`, `Mirror` both, `AsGridGeometry`, `buildOutlineWorld`, bbox, hit tests, description) | `pcb_grid_item.rs` | ported (`GRID_GEOMETRY` is kicad-geom's) |
| `DRILL_SPAN::TopLayer` / `BottomLayer` / `Pair` / `operator<`; `PCB_DRILL_MAP::SetSymbolSize`, `Rotate`, `Flip`, rect hit test, description | `pcb_drill_map.rs` | ported |
| `PCB_DRILL_MAP::GetSymbolExtent`, `GetBoundingBox`, `HitTest( point )` | `pcb_drill_map.rs` | **stub**: need `DRILL_SYMBOL_PROFILE::GetSymbolWidth` and the board's `DRILL_SYMBOL_CACHE` (tier-2 drill symbol package; the profile is kept verbatim); `symbol_extent` also lacks a `ctx` (change request) |

Verification:
- `pcb_generator::tests::normalize_properties_matches_the_oracle`: golden
  `tests/golden/m23_generator_properties.txt` (script `m23_generator_properties.sh` + `.py`):
  every `(generated ...)` block of 15 corpus boards before and after the reference
  `kicad-cli pcb upgrade --force`; 35 generators (26 tuning patterns of every mode and vintage,
  4 via stacks, 5 via stitches; one empty pattern is dropped by the writer) match key for key,
  numbers as `{:.10g}` text. It reproduces the C++ reading of the uninitialised `val` /
  `int_val` locals in `PCB_TUNING_PATTERN::SetProperties` (a missing `target_delay` takes the
  value of `target_length_max`: `tuning_generators_load_save.kicad_pcb`).
- `pcb_generator::tests::m23_items_board_matches_pcb_dump`: `tests/data/m23/m23_items.kicad_pcb`
  (nested groups, targets, a point, two tuning patterns incl. a single-sided chamfered one, a
  via stitch) rebuilt in the model; bounding boxes, positions, layers and layer sets equal
  `pcb-dump board --sections item,bbox,misc` of the reference build (incl. the tuning outline
  boxes 29650000,18400000,10700001,3200000 and 49591470,29650000,10758531,3429088). The parser
  part of groups / generators (PCBIO-09 / -13) was not ready, so pcb-dump-rs could not be used.
- `wxAny::GetAs` conversions were measured with a driver against the reference
  `libwx_baseu-3.2` (number to integer truncates after a `long long` range check, no number to
  bool, ...): `pcb_generator::tests::wx_any_conversions`.
- Ported C++ tests: `qa/tests/pcbnew/constraints/test_pcb_constraint.cpp`
  `CloneIsAFaithfulCopy`, `EqualityAndSimilarity` (equality part), `DuplicateDetection`,
  `VertexIndexInDuplicateDetection`. Not ported here: `test_group_mirror.cpp` (the cascade is
  `Board::mirror_item`, M2, and needs `PCB_SHAPE::Mirror`, M9), `group_saveload.cpp`,
  `test_group_load_save.cpp`, `test_generator_load_save.cpp`, `test_via_stack_saveload.cpp`
  (parser / writer packages).
- Unit tests for targets, points (rotated / flipped / scaled footprint frame, `cmp_points`),
  marker severity precedence and glyph, grid items, drill spans, 3D body flip.

Deviations / notes:
- The generator subclasses keep no typed state: `TuningPatternData` etc. are re-read from the
  (normalised) property map on every call, which is lossless (IU values are stored as
  `iu / 1e6` and read back with `KiROUND`).
- `PCB_TUNING_PATTERN::SetProperties` reads `long long val` / `int int_val` uninitialised when
  the first key is missing; Rust starts them at 0.
- `wxAny` string conversions (`string -> bool` accepts yes/true/1, no/false/0) beyond the
  measured cases are best effort; the parser only produces numbers, bools, strings, points,
  chains and cells.
- Group member order (C++ `unordered_set`) is insertion order: only the order of the compound
  from `GetEffectiveShape` and of `RunOnChildren` can differ.
- `MARKER_BASE::m_scalingFactor` is the constant `SCALING_FACTOR` (GUI zoom not ported).

Known gaps:
- The generator subclass overrides of `Move` / `Rotate` / `Flip` / `Mirror` / `SetLayer`
  (tuning pattern end point and base lines, `IN_EDIT`, members' layer; via stack hops and
  layers; via stitch moves only its outline) are not applied by the `Board::*_item` cascades
  of M2 (change request); the typed views make them a few lines each.
- Template items (`GetTemplateItems` / `SetTemplateItem`) are carried as data only.
- `Similarity` (deferred by the design, section 17).

## Package M4: net settings logic (effective netclasses, pattern assignments)

### C++ to Rust map

| C++ (`common/project/net_settings.cpp`, `include/project/net_settings.h`) | Rust (`net_settings.rs`) | state |
|---|---|---|
| `operator==` / `!=` | `impl PartialEq for NetSettings` | ported |
| `Set/GetDefaultNetclass`, `HasNetclass`, `SetNetclass(es)`, `GetNetclasses`, `GetCompositeNetclasses`, `ClearNetclasses` | same names, snake_case | ported |
| label assignments (`Get/Clear/Set/Append/HasNetclassLabelAssignment(s)`) | same names | ported |
| `SetNetclassPatternAssignment(s)`, `addSinglePatternAssignment`, `Get/ClearNetclassPatternAssignments` | same names | ported |
| `SetChainPatternAssignment`, `addSingleChainPatternAssignment`, `ClearChainPatternAssignments`, `HasChainPatternAssignments`, `GetChainPatternAssignments` | `set_chain_pattern_assignment`, ..., `chain_pattern_assignments` | ported |
| net colour assignments, `RenameNetPathPrefix`, `RenameNets` | same names | ported |
| inline chain-class accessors (`Set/Get/ClearNetChainClass(es)`, `...NetChainNetClass(es)`) | same names | ported |
| `HasEffectiveNetClass`, `GetCachedEffectiveNetClass`, `GetEffectiveNetClass`, `GetNetClassByName`, `ClearCacheForNet`, `ClearAllCaches` | same names | ported |
| `RecomputeEffectiveNetclasses`, `makeEffectiveNetclass`, `addMissingDefaults` | same names (last two private) | ported |
| `ParseBusVector`, `ParseBusGroup`, `ForEachBusMember`, `isEscaped`, `isSuperSubOverbar` | same names | ported |
| `CopyFrom`, constructor parameter registration, migrations | - | JSON package (next) |

### Tests

* `qa/tests/common/test_net_settings.cpp`: all in-memory cases ported as `net_settings::tests::*`
  (equality, chain patterns, rename, shared bus parser, `CompositeOwnsConstituents`). Not
  ported here (they need the JSON layer, next package): `NetChainClassesJsonRoundTrip*`,
  `NetChainClassesJsonDropsEmptyValues`, `ChainNetclassesJsonRoundTrip`,
  `RemovedChainNetclassIsDroppedFromProjectFile`, `CopyFrom*`.
* Differential oracle: `tests/oracle/net_settings_oracle.cpp` links the reference
  `libkicommon.so` and runs a command script (`tests/oracle/net_settings_golden.sh` builds and
  runs it). `oracle_scenarios` replays `tests/data/net_settings/scenarios.txt` (247 output
  lines: effective classes with every field, parent and constituent, composite naming and
  priority ties, implicit classes, default tuning profile / colours / unset default fields,
  chain sources, bus nets and `ClearCacheForNet` propagation, label management, renames,
  ~70 `ParseBusVector` / `ParseBusGroup` / `ForEachBusMember` edge cases); 
  `oracle_corpus_scenarios` replays `corpus_scenarios.txt`, generated by
  `tests/oracle/net_settings_corpus.py` from every corpus `.kicad_pro` with netclass
  patterns (3743 lines: regex compilability of all 1913 distinct patterns and the effective
  netclass name of 1811 nets of the 19 boards next to such projects). Both match the C++ line
  for line. Corpus patterns `regress` handles differently from wxRegEx at compile time: none
  (61 patterns fail to compile as regex in both, e.g. `+3.3V`, `*clk`; they match as wildcards).

### Deviations / notes

* `GetEffectiveNetClass` collects the resolved classes in a `std::unordered_set` of
  `shared_ptr` (hash order = pointer order) before the `std::sort`; here they are kept in
  first-insertion order, de-duplicated by `Arc::ptr_eq`, and sorted with
  `kicad_geom::stdsort::std_sort`. The order can only differ for two distinct classes with the
  same priority and the same name (a `netclasses` key different from the class's own name).
* `RecomputeEffectiveNetclasses` rebuilds each composite in place in C++. A shared
  `Arc<Netclass>` is immutable, so the rebuilt composite is a new `Arc` that replaces the old
  one in `composite_netclasses` and in every `effective_netclass_cache` entry holding it;
  `NetinfoItem::netclass` keeps the old value until the next
  `synchronize_nets_and_netclasses`. The constituents are the stored `Arc`s, so an edit made
  by replacing a class (the only edit possible on an `Arc`) is not picked up, unlike a C++
  in-place edit. Only the UI and the Altium importer call it.
* `GetCachedEffectiveNetClass` (`std::map::at`, throws when missing) returns `None`;
  `GetNetClassByName` always returns `Some` (default class when unknown).
* `ParseBusVector` / `ParseBusGroup` keep the C++ out-parameters as `Option<&mut ..>` so that
  partial writes match (`ParseBusGroup` writes the name before the member list can fail);
  `prefix_end` is a character index like the C++ `size_t` string position.

### Known gaps

* End-to-end check against `pcb-dump board --sections board` (`effective_netclass.*` lines)
  waits for the `.kicad_pro` JSON loader (next package): without it the Rust board has no
  netclasses / patterns to resolve.

## Package PCBIO-02: board file writer core and board skeleton

Files: `src/io/writer/{mod,board,tests}.rs` (+ the staging stubs `src/io/writer/{tracks,shapes,
setup,text,footprint,pad,zone,group,dimension,netclass_embedded,misc_items,generator,fp_lib}.rs`
owned by PCBIO-03 .. 14), the writer entry in `src/io/mod.rs`, `tests/data/pcbio02/`,
`tests/golden/pcbio02/`, `tests/golden/pcbio02_corpus.txt`,
`tests/oracle/pcbio02_resave_golden.sh`, `tests/oracle/pcbio02_corpus_golden.sh`.

### C++ to Rust map

| C++ (`pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp`, `.h`, `include/ctl_flags.h`) | Rust (`src/io/writer/`) | state |
|---|---|---|
| `CTL_*` flags, `CTL_FOR_CLIPBOARD` / `CTL_FOR_LIBRARY` / `CTL_FOR_BOARD` | `mod.rs` constants | ported |
| `PCB_IO_KICAD_SEXPR` writer state (`m_out`, `m_board`, `m_ctl`), constructor + `init()` | `mod.rs` `PcbWriter` | ported |
| `SaveBoard` (`PRETTIFIED_FILE_OUTPUTFORMATTER` incl. `ADVANCED_CFG::m_CompactSave`, `Finish()`) | `mod.rs` `format_board`, `format_board_report` (text returned instead of the file write) | ported (`GroupsSanityCheck` not called: result used only with a GUI callback, no side effect without `repair`) |
| `FormatBoardToFormatter` | `mod.rs` `format_board_to_formatter`, `board.rs` | ported (font embedding: see deviations) |
| `Format( const BOARD_ITEM* )` dispatch | `mod.rs` `PcbWriter::format_item` | ported |
| `formatInternalUnits` (3 overloads), `unbakeSize`, `unbakeSizeUniform`, `unbakeLinear` | `mod.rs` `format_internal_units_dt`, `format_iu`, `format_iu_pt`, `format_internal_units_pt_fp`, `unbake_size`, `unbake_size_uniform`, `unbake_linear` | ported |
| `formatLayer`, `formatLayers`, `formatPolyPts` | `mod.rs` | ported |
| `formatGeneral`, `formatBoardLayers`, `formatProperties`, `formatVariants`, `formatHeader`, `format( const BOARD* )` (section order, sorted containers, embedded files consolidation, `(embedded_fonts ..)`) | `board.rs` | ported |
| `std::set<T*, CMP>( begin, end )` | `mod.rs` `sorted_set` (stable sort + drop of later equivalent elements = the set's content) | ported |
| save-order comparators | model: `board_item_ptr_cmp`, `board_cmp_drawings`, `pcb_track_cmp_tracks`, `pcb_point_cmp_points` | called, not ported here |
| `formatSetup`, `formatDrillSymbolProfile` | `setup.rs` | stub (PCBIO-04) |
| net-chain section of `format( const BOARD* )` (W:962-1038) | `netclass_embedded.rs` `format_net_chains` | stub (PCBIO-11); writes nothing, like the C++, when no net has a chain name or terminal pad |
| every other `format(...)`, `formatRenderCache`, `formatTeardropParameters`, `formatInitialComments`, `FormatFootprintLibFile` | `tracks.rs`, `shapes.rs`, `text.rs`, `footprint.rs`, `pad.rs`, `zone.rs`, `group.rs`, `dimension.rs`, `misc_items.rs`, `generator.rs`, `fp_lib.rs` | staging stubs with the final signatures (PCBIO-03 .. 14) |
| `Parse( clipboard )`, `FP_CACHE`, `writers/pcb_writer_v9/v10` | - | skipped: GUI / file system / downgrade (`pcb-io.md` 2.2) |

Staging: a stub called with `FormatOptions::allow_pending == false` fails with the `IO_ERROR`
"port pending: <item kind> (PCBIO-nn)"; with `true` it writes nothing and records
`(item, package)` in `FormatReport::skipped`.

### Tests

`src/io/writer/tests.rs`:

- Differential oracle `oracle_resave`: 8 hand-made boards (`tests/data/pcbio02/`: current
  format with the default layer set; title block with escapes / UTF-8 / all comment slots and
  unsorted properties with a user paper size; 6 copper layers with user names and every layer
  type, user layers with front/back/user/auxiliary types incl. `User.10`/`User.11`; a 2022 file
  with old layer numbering and legacy `(net ..)` declarations; a version 4 file with `host` and
  `(page ..)`; variants; `(embedded_fonts yes)`; a 2024 file with a portrait user paper size)
  re-saved by the reference `kicad-cli pcb upgrade --force` (`tests/oracle/pcbio02_resave_golden.sh`).
  Parse + format of every input reproduces the oracle bytes exactly, and parse + format of
  the oracle output reproduces the oracle's second re-save (`v4_host_o2.kicad_pcb`, where the
  reference build changes the legacy layer names once more), except the `(setup ..)` section
  (PCBIO-04), which is removed from the expected text. Variants and the embedded-fonts flag
  are read by PCBIO-11 parser stubs; the test sets them by hand from the input
  (`apply_pending_parser_state`).
- Corpus oracle `oracle_corpus_skeleton`: every board of `qa/data/pcbnew` + `demos` below 2 MB
  re-saved by the reference build (`tests/oracle/pcbio02_corpus_golden.sh`, FNV-1a hashes in
  `tests/golden/pcbio02_corpus.txt`); parse + format must give the same three skeleton regions
  (A: start to the end of `(layers ..)`; B: the `(property ..)` / `(variants ..)` blocks
  after the setup section; C: the `(embedded_fonts ..)` line). Result: 342 boards, 1 rejected
  by the reference build (`plugins/fakeboard.kicad_pcb`), 341 compared, 337 identical in all
  three regions; the other 4 differ only where a pending parser feeds the region and the test
  checks that the parse skipped that construct: `issue14549` (legacy teardrop zones set
  `legacy_teardrops yes` in `parseZONE`, PCBIO-08), `issue24735`, `odbpp/variant_field_textvar`,
  `variant_test` (`(variants ..)`, PCBIO-11). Any other difference fails the test.
- `test_prettifier.cpp` `BoardAndFootprintPrettifier` (`board_and_footprint_prettifier`):
  golden check of `Prettify` for all three cases; for `group_and_image.kicad_pcb` the input
  and its prettified text parse to boards whose formatted output is identical (stand-in for
  `BOARD::operator==`); the two `.kicad_mod` cases report the footprint parser (PCBIO-14) as
  pending.
- Helpers: `format_layers_wildcards`, `format_layer_knockout`, `format_poly_pts_arcs`,
  `unbake_without_footprint`, `sorted_set_is_std_set`, `ctl_flags`, `raw_token_stream` (the
  stage-1 token stream before Prettify), `default_board_skeleton`, `staging_stubs` (strict
  mode: "port pending: setup (PCBIO-04)" - PCBIO-04 must update it; "port pending:
  kicad_footprint_lib (PCBIO-14)").

### Deviations / notes

- `FormatBoardToFormatter` mutates the board (`EmbedFonts()` / `ClearEmbeddedFonts()`); the
  port takes `&Board`: with fonts not embedded, the board's font files are left out of the
  consolidated `(embedded_files ..)` (the C++ removes them from the board), with fonts embedded
  `EmbedFonts()` is a no-op that keeps the font files already embedded (no font files in the
  port, `pcb-io.md` R7). The caller's board keeps its font files.
- The `std::set` tie-break on pointer addresses is `ItemId` order (the model comparators).
- `GetMajorMinorVersion()` is `FormatOptions::generator_version`, default `"10.99"`
  (confirmed by the oracle).

### Known gaps

- `formatLayers`, `formatPolyPts`, `formatLayer` and the un-bake helpers are checked against
  expectations derived from the C++ only; their oracle check comes with their callers (pads,
  tracks, shapes, zones: PCBIO-03 / 07 / 08).
- The `(setup ..)` section (PCBIO-04), net chains (PCBIO-11) and every item are staging stubs;
  the oracle comparisons skip exactly the setup section and use boards without items.

## Package M24: reference images, embedded files, component classes

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `PCB_REFERENCE_IMAGE` (geometry, shape, hit tests, flip, mirror) | `pcb_reference_image.rs` | ported |
| `REFERENCE_IMAGE` (`SetPosition`, `scaleBy`, `SetImageScale/Width/Height`, `boundingSize`, `GetBoundingBox`, `Rotate`, `Flip`, `Mirror( SEG )`, `SetOpacity`, `ReadImageFile( buffer )`, `PackToBytes`) | `pcb_reference_image.rs` | ported |
| `BITMAP_BASE` data part (`GetSize`, `GetScalingFactor`, `updatePPI`, `GetLegacyPPI`, `SaveImageData`) | `pcb_reference_image.rs` | ported; pixels never decoded |
| `wxImage::LoadFile` (wx 3.2 PNG / JPEG handlers: libpng IHDR/pHYs, libjpeg-turbo SOF/JFIF) | `probe_image_header` | header probe only |
| `EMBEDDED_FILES`, `EMBEDDED_FILE` (`Validate`, `Validate_SHA256`, `GetLink`, `AddFile` x2, `RemoveFile`, `AssignSharedFrom`, deep copy, `ClearEmbeddedFonts`, `UpdateFontFiles`, `WriteEmbeddedFiles`, `CompressAndEncode`, `DecompressAndDecode`, `ComputeFileHash`) | `embedded_files.rs` | ported |
| `EMBEDDED_FILES_PARSER::ParseEmbedded` | `embedded_files::parse_embedded` | ported (for PCBIO-11) |
| `ZSTD_getFrameContentSize` / `ZSTD_getFrameHeader` (libzstd) | `embedded_files.rs` | hand port; decoding by `ruzstd` |
| `picosha2::hash256_hex_string` | `embedded_files::sha256_hex` | local shim (FIPS vectors) |
| `wxBase64Encode/Decode` (strict) | `wx_base64_encode/decode` | `base64` crate, wx-strict config |
| `COMPONENT_CLASS` | `component_class.rs` | ported |
| `COMPONENT_CLASS_MANAGER` (static / effective / combined / dynamic classes, netlist update, ticker) | `component_class.rs` | ported (`SyncDynamicComponentClassAssignments`: package CC-1) |
| `COMPONENT_CLASS_CACHE_PROXY::GetComponentClass/RecomputeComponentClass` | `ComponentClassManager::get_component_class/recompute_component_class` | ported; rule matching passed in (`COMPONENT_CLASS_ASSIGNMENT_RULE::Matches` is kicad-drc) |
| `FinishNetlistUpdate` footprint pointer clearing | `Board::finish_component_class_netlist_update` | ported |

### Tests

Ported: qa/tests/common/test_embedded_file_compress.cpp (all 11 cases; `ComputeFileHash_*` on
bytes, `ComputeFileHash_NonExistentFile` n/a without filesystem), qa/tests/pcbnew/
test_component_classes.cpp `FinishNetlistUpdateClearsDeletedClassPointers`,
`FinishNetlistUpdatePreservesActiveClassPointers` (`ComponentClasses` needs dynamic rules:
tier 2), qa/tests/pcbnew/test_reference_image_load.cpp `ReferenceImageFlipLayer`,
`ReferenceImagePpiScaleMigration` (model part), `ReferenceImageRotateTransformOrigin`,
`ReferenceImageStoresRotation`, qa/tests/common/test_bitmap_base.cpp `Empty`,
`PPIFromNonIntegerPixelsPerCm`, PPI/size part of `BasicProps` (pixel tests n/a).

Differential oracle `tests/oracle/m24_oracle.cpp` (+ `m24_build_oracle.sh`; compiles
`common/reference_image.cpp`, `common/bitmap_base.cpp` unchanged, links libkicommon.so and wx):
* `tests/golden/m24_images.txt` (988 lines): 32 synthetic PNG/JPEG headers (pHYs units, bad CRC,
  late / duplicate / short pHYs, unknown critical chunk, width 0, truncated, JFIF units 0-3, no
  JFIF) and all 8 reference images of the 6 corpus boards: read result, PPI, legacy PPI, sizes,
  scale / width / height / rotate / flip / mirror maths and overflow refusals. Replay
  `pcb_reference_image::tests::m24_oracle_images`: identical.
* `tests/golden/m24_embed.txt` (303 lines): every `(embedded_files ...)` section of the 52 corpus
  files (qa/data, demos) parsed and decoded by the C++; replay
  `embedded_files::tests::m24_oracle_embedded_files`: identical. Stored checksums: 216 current
  MMH3, 87 legacy `addDataV1`, all verified.
* `m24_oracle_cpp_decodes_port_frames`: frames compressed by the port are decoded by the C++
  `DecompressAndDecode` (skips without the oracle binary).

### Deviations / notes

* Pixels are never decoded: `BITMAP_BASE::Mirror` (called by `REFERENCE_IMAGE::Flip/Mirror`)
  mirrors the raster and drops the original bytes in C++, so KiCad re-encodes a flipped image on
  save; the port keeps the original bytes (geometry, orientation, origin offset are exact).
* `probe_image_header` reads PNG and JPEG only (what KiCad stores); other wx formats (BMP, GIF,
  TIFF, ...) fail to read. Corrupt pixel data after a valid header is accepted (C++ fails).
  A PNG truncated after a valid IHDR is accepted, as the reference build does.
* `REFERENCE_IMAGE::Flip`'s `MIRROR` and `scaleBy`'s `m_pos + m_transformOriginOffset` wrap like
  the reference build (signed overflow) instead of panicking.
* `CompressAndEncode` uses the `ruzstd` encoder (level "Fastest"), not libzstd level 15: newly
  embedded files get other bytes (same data, same checksum). ruzstd writes no content size, so
  the frame header is rewritten with one (C++ rejects frames without it). Stored frames are
  never re-compressed.
* `DecompressAndDecode`: a corrupt frame is OUT_OF_MEMORY like C++, but ruzstd does not verify
  the frame's XXH64 checksum or the declared content size (the MMH3 check that follows catches
  corrupt data); invalid base64 returns OUT_OF_MEMORY directly (C++ reads past its buffer).
* `UpdateFontFiles` lists font *names*, not temporary file paths (no filesystem); `AddFile(
  wxFileName )` / `ComputeFileHash` take bytes (`add_file_from_data`, `compute_file_hash`).
* Component classes are `Arc` values: usage contexts live in the manager's maps
  (`Arc::make_mut`); C++ pointer identity is (`is_constituent`, name). `RebuildRequiredCaches`
  is a no-op (the courtyard cache is lazy and keyed). A missing map entry where C++ would create
  a null entry with `operator[]` (and crash) is skipped.

### Known gaps / stubs left

* (closed by package CC-1: `SyncDynamicComponentClassAssignments`, `FOOTPRINT::GetComponentClass`
  with assignment rules.)

## Package M14: pad model (PAD without shape generation)

### C++ to Rust map

| C++ (`pcbnew/pad.cpp`, `pad.h`) | Rust | State |
|---|---|---|
| anonymous namespace: `scaleLength`, `scaleInChildFrame`, `derivePad{Board,Lib}{Size,Drill,Offset}` | `pad/derive.rs` private fns | ported |
| `GetPosition` / `SetPosition`, `Get/SetFPRelativePosition` (BOARD_ITEM bodies), `Get/SetOrientation`, `Get/SetFPRelativeOrientation`, `GetOrientationDegrees` | `position`, `set_position`, `fp_relative_position`, `set_fp_relative_position`, `orientation`, `set_orientation`, `fp_relative_orientation`, `set_fp_relative_orientation`, `orientation_degrees` | ported (readers through the parent footprint via `ctx`, writers through `env.parent`) |
| `Get/SetSize`, `SetLibSize`, `SetSizeX/YProp`, `Get/SetOffset`, `SetLibOffset`, `ShapePos`, `Get/SetPrimaryDrillSize` (+ X / Y), `SetLibDrillSize`, `GetPrimitiveLibScale` | same names (`drill_size`, `set_drill_size`, `set_drill_size_x/y`, `set_size_x/y_prop`) | ported |
| drill / slot accessors (primary shape / start / end / filled / capped, secondary and tertiary drill setters, `Get*Knockout`, `IsBackdrilledOrPostMachined`) | `drill_shape`, `set_drill_shape`, ... | ported (`HasCopperRemoved` needs `GetBoundingRadius`: M15) |
| per-layer forwards (`SetShape`, anchor shape, delta, round-rect radius / ratio incl. front variants, chamfer ratio / positions, custom-shape-in-zone, unconnected-layer mode, `GetRemoveUnconnected`, `GetKeepTopBottom`, `ConditionallyFlashed`) | same names | ported |
| `GetLayer`, `GetPrincipalLayer`, `GetLayerSet`, `SetLayerSet`, `IsOnLayer`, `IsOnCopperLayer`, `IsAperturePad`, `CanHaveNumber`, `IsNPTHWithNoCopper`, `IsFlipped`, `IsNoConnectPad`, `IsFreePad`, `SameLogicalPadAs`, `SharesNetTieGroup` | same names (`is_free_pad_ctx`) | ported; `is_free_pad()` (no ctx) stays a marker, see gaps |
| `FlashLayer` (all overloads), `CanFlashLayer` | `flash_layer`, `flash_layer_permitted`, `flash_layer_set` | ported; connectivity through `ctx.connectivity` (`None` = empty CONNECTIVITY_DATA: not connected), zone-layer overrides read from the pad |
| `IsLocked`, `HasHole`, `HasDrilledHole`, `SetAttribute`, `SetProperty`, pin / die / sim setters, `SetPadstack`, `ImportSettingsFrom` | same names | ported (`ImportSettingsFrom` takes the pad's `env` and the master's `ctx`, clone-edit-store) |
| `GetLocalClearance` (both), `GetClearanceOverrides`, `GetOwnClearance`, `GetSolderMaskExpansion`, `GetSolderPasteMargin`, `GetZoneConnectionOverrides`, `GetLocalSpokeWidthOverride`, `GetLocalThermalGapOverride` (both), `GetThermalSpokeAngle`, local override getters / setters | same names | ported (rules branch through `ctx.rules`, fallback chain pad -> footprint -> board) |
| `Move`, `Rotate`, `Flip`, `FlipPrimitives`, `Mirror` (both), `mirrorChamferPositions`, `SwapShapePositions`, `OnFootprintTransformed`, `OnFootprintRescaled` (+ `rebakePrimitiveToFootprint`) | `move_by`, `rotate`, `flip`, `flip_primitives`, `mirror`, `mirror_seg`, `swap_shape_positions` (returns the two positions), `on_footprint_*` | ported; primitive `Flip` / `Mirror` / `RebakeWithScale` call the PcbShape methods (M9) |
| `Compare`, `operator==`, `Similarity`, `GetItemDescription`, `ShowPadShape`, `ShowLegacyPadShape`, `ShowPadAttr` | `compare`, `eq_item`, `similarity`, `item_description`, `show_*` | ported (English texts) |
| `Board::set_zone_layer_override` / `clear_zone_layer_overrides` | `pad/mod.rs` | pad branch ported; via branch `port_pending!` (`PCB_VIA::Set/ClearZoneLayerOverrides`, M17 has no accessor yet) |
| `GetBoundingBox` (both), `HitTest`, `GetEffectiveShape` & co. | `pad/shapes.rs` | M15 (needs shapes) |
| `Serialize` / `Deserialize`, `GetMsgPanelInfo`, `ViewGetLOD` & co., property setters for the UI, `Recombine`, `SetFrontShape` | - | skipped: API / UI / editor tools |

### Tests

- `pad::derive::tests::oracle_pad_frames`: differential test against the reference `pcb-dump`
  (`tests/oracle/m14_pad_golden.sh` on `tests/oracle/m14_pad_frames.kicad_pcb`, golden
  `tests/golden/m14_pad_frames.txt`): 15 pads in three footprints (rotated 30 degrees and
  scaled 1.5 x 0.75; flipped, rotated -90 and scaled 2 x 1; identity), NORMAL / FRONT_INNER_BACK
  / CUSTOM padstacks, PTH / SMD / CONN / NPTH / aperture pads, pad, footprint and board
  overrides. Each pad is rebuilt from its dumped members (library frame) and about 820 derived
  values are compared: position, orientation, primitive lib scale, per-layer board-frame size /
  offset, drill size, principal layer, flags, every override and fallback chain, mask expansion
  and paste margin on both sides, `IsOnLayer` / `FlashLayer` / `CanFlashLayer` per copper layer,
  `HasHole`, `IsOnCopperLayer`, `GetFPRelativePosition`. All match.
- `qa/tests/pcbnew/test_pad_setters.cpp`: all 4 cases. `test_pad_swap_positions.cpp`: all 5.
  `test_pad_flashing.cpp`: the two "do not force inner layer flashing" cases without
  connectivity (`ctx.connectivity == None`; the real-connectivity variant needs
  kicad-connectivity), `TechLayersPullFromAppropriateSide` needs effective shapes (M15).
- New tests: frame setters round trip on the rotated / flipped / scaled fixture, flip / mirror
  of lib position, offset, chamfers and layers, `SetAttribute` trimming, `Compare` /
  `operator==` / `Similarity`, the zone-layer-override writers (no time-stamp bump, C++ copper
  range), item descriptions.
- Not ported yet: `test_pad_check.cpp` (CheckPad reaches M15 shapes), the pad parts of
  `test_footprint_transform_sync.cpp` (they drive `FOOTPRINT::SetTransform` / `Flip` cascades,
  M19) and `test_footprint_pad_fixup.cpp` (M20).

### Deviations / notes

- `SetLayerSet` / `SetAttribute` / `SetNetCode`: `InvalidateClearanceCache` has no counterpart
  (time-stamp revalidation, `board-model.md` 7). `SetAttribute( NPTH )` sets net 0 directly
  (`SetNetCode( UNCONNECTED )` always ends on net 0).
- Custom-shape primitives are mutated with a `MutEnv` whose `parent` is `None` (C++
  `PCB_SHAPE::transformFp()` is null for pad children); the board facts are the pad's.
- `GetClearanceOverrides` / `GetZoneConnectionOverrides` fall back to the footprint's fields
  with the source text "footprint <reference>" (`FOOTPRINT::GetLocalClearance( aSource )` and
  `GetZoneConnectionOverrides` are header inlines without a Rust method); the reference is read
  only when a source is asked for.
- `operator==( BOARD_ITEM )`: a `BOARD` parent has no UUID in Rust; two pads whose parent is the
  board compare their parents as equal. `Similarity` treats a missing parent as a nil UUID
  (C++ dereferences it).
- `SharesNetTieGroup` reproduces `std::map::operator[]` (a pad number missing from the map is
  group 0).

### Known gaps / stubs left

- `Pad::is_free_pad()` (no `ctx`): `port_pending!` marker; `IsFreePad` reads the net name, so
  the port is `is_free_pad_ctx( ctx )` (signature change requested).
- `Board::set_zone_layer_override` / `clear_zone_layer_overrides` for vias: `port_pending!`
  until `PcbVia` has `set_zone_layer_override` / `clear_zone_layer_overrides` (M17).
- Run-time dependencies on other packages' stubs: `FOOTPRINT::GetReference` (descriptions,
  override sources; M19), `MapPadNumbersToNetTieGroups` (M19), `PCB_SHAPE::Flip` / `Mirror` /
  `RebakeWithScale` (custom primitives; M9).

## Package M6: board stackup, stackup predefined parameters, plot params (model logic)

Supersedes the "stubs (M6)" notes of the M0d rows for `board_stackup.rs` / `pcb_plot_params.rs`.

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `BOARD_STACKUP_ITEM` getters / setters, `AddDielectricPrms`, `RemoveDielectricPrms`, `Has*Value`, `Is*Editable`, `FormatEpsilonR`, `FormatLossTangent`, `FormatDielectricLayerName`, `IsEnabled` / `SetEnabled`, `Get/SetBrdLayerId`, `Get/SetTypeName`, `Get/SetLayerName`, `Get/SetDielectricLayerId` | `board_stackup.rs` `impl BoardStackupItem` | ported |
| `BOARD_STACKUP` copy constructor / `operator=` / `operator==` (and the item / `DIELECTRIC_PRMS` ones) | derived `Clone` / `PartialEq` (deep copies; field-wise equality = the C++ member lists) | ported |
| `BOARD_STACKUP::StackupAllowedBrdLayers`, `RemoveAll`, `GetStackupLayer` (+ `stackup_layer_mut`), `BuildDefaultStackupList`, `BuildBoardThicknessFromStackup`, `SynchronizeWithBoard`, `GetLayerDistance`, `GetDrillDepth` | `board_stackup.rs` `impl BoardStackup` | ported |
| `IsPrmSpecified` | `board_stackup::is_prm_specified` | ported |
| `stackup_predefined_prms.cpp/.h`: `copperFinishType`, `gbrjobColors`, `dielectricColors`, `FAB_LAYER_COLOR` (`GetColor`, `GetColorAsString`), `GetStandardCopperFinishes`, `GetStandardColors`, `GetColorUserDefinedListIdx`, `GetDefaultUserColor`, `GetStandardColor`, `GetStandardColorName`, `IsCustomColorIdx`, `IsColorNameNormalized`, `DEFAULT_SOLDERMASK_OPACITY` | `board_stackup.rs` (end of file) | ported |
| `BOARD_STACKUP::FormatBoardStackup` | - | PCBIO-04 (writer) |
| `BOARD_STACKUP::Serialize` / `Deserialize` (protobuf API) | - | skipped: IPC API out of scope |
| `PCB_PLOT_PARAMS::IsSameAs`, `SetGerberPrecision`, `SetSvgPrecision`, `Get/SetLayerSelection`, `SetPlotOnAllLayersSequence`, `GetLegacyPlotViaOnMaskLayer`, `GetGerberPrecision`, `GetSvgPrecision` | `pcb_plot_params.rs` | ported (the other trivial `Get*` / `Set*` of the header are the `pub` fields) |
| `PCB_PLOT_PARAMS::Format` / `Parse`, `PCB_PLOT_PARAMS_PARSER`, `remapLegacyLayerLSET` | PCBIO-04 (`kicad_core::legacy_layers::remap_legacy_layer_lset` exists) | not this package |

### Tests and verification

* Differential oracle `tests/oracle/m6_stackup_oracle.cpp` (built and run by
  `tests/oracle/m6_build_oracle.sh` against the reference build, reusing the pcb-dump harness
  objects), golden `tests/golden/m6_stackup_oracle.txt` (2604 lines). Rust replay:
  `board_stackup::tests::m6_oracle` and `pcb_plot_params::tests::m6_oracle_plot_params`. It
  covers `BuildDefaultStackupList` with settings for 1, 2, 4, 6, 8 and 32 copper layers (odd
  thicknesses, a 32-layer board too thin for its copper (negative dielectric), mask / paste /
  silk subsets, appending to a non-empty list) and without settings for 0, 2, 4 and 6 layers;
  `BuildBoardThicknessFromStackup`; `GetLayerDistance` and `GetDrillDepth` for all copper
  pairs (6 for 32-layer stackups) of default and customised stackups (sublayers, odd copper
  thickness, disabled items); `SynchronizeWithBoard` to the same, fewer and more layers;
  `Add/RemoveDielectricPrms`; per-type flags; `FormatEpsilonR` / `FormatLossTangent`;
  `IsPrmSpecified`, `IsColorNameNormalized`, the colour / finish tables; Gerber / SVG precision
  clamping; the full `IsSameAs` matrix (49 single-field changes x 6 plot formats, both
  directions). All lines match bit for bit (doubles compared as IEEE bits).
* `pcb-dump defaults` vs `pcb-dump-rs defaults` (`BOARD_STACKUP`, `BOARD_STACKUP.default_list`,
  `PCB_PLOT_PARAMS`): 194 lines, identical except the item `type` enum spelling of the Rust
  dump (`SILKSCREEN` vs `BS_ITEM_TYPE_SILKSCREEN`; dump code, reported).
* `pcb-dump board --sections board` on copies of 4 corpus boards without `(stackup ...)`:
  `stackup_or_default.*` identical (same spelling caveat). `ds.stackup.*` and `plot.*` differ
  because the parser side (PCBIO-04) does not yet read `(stackup ...)` / `(pcbplotparams ...)`
  nor build the loader's default stackup.
* `copy_semantics`: model part of `qa/tests/pcbnew/test_fab_stackup.cpp`
  (`FabStackupSnapshotsRealBoard`; the FAB snapshot itself is not model code).

### Deviations / notes

* `BuildDefaultStackupList` and `SynchronizeWithBoard` take the settings by shared reference;
  the C++ calls them on `aSettings->GetStackupDescriptor()` itself (aliasing). Rust callers
  pass a copy of the settings or work on a copy of the stackup.
* `AddDielectricPrms( idx )` with `idx > count` is undefined in C++; ignored here.
* `FormatDielectricLayerName` returns the untranslated `"Dielectric %d"`;
  `GetStandardCopperFinishes( aTranslate )` takes no flag (no translations in the port).
* `GetDefaultUserColor` / `GetStandardColor` / `GetStandardColorName` return `Option` (C++
  indexes an empty vector for types without a list).
* `wxString::CmpNoCase` (in `IsPrmSpecified`, `IsColorNameNormalized`) is a per-character
  lower-case comparison; a character whose Rust lower case has several characters compares by
  the first one (matches glibc `towlower` for U+0130).

## Package SET-3: net settings JSON (`net_settings` in `.kicad_pro`) and migrations

### C++ to Rust map

| C++ (`common/project/net_settings.cpp`) | Rust (`src/project/net_settings_json.rs`) | state |
|---|---|---|
| `netSettingsSchemaVersion` | `NET_SETTINGS_SCHEMA_VERSION` (5) | ported |
| `NET_SETTINGS` ctor: `NESTED_SETTINGS( "net_settings", 5, parent, path, false )`, the six `PARAM_LAMBDA<nlohmann::json>` (`classes`, `net_colors`, `net_chain_classes`, `net_chain_netclasses`, `netclass_assignments`, `netclass_patterns`, with `SetClearUnknownKeys()` on the four maps), `registerMigration` x5 | `NetSettingsJson::new( path )` (a `kicad_core::json_settings::NestedSettings<NetSettings>`; `ClearUnknownKeys` wrapper parameter) | ported |
| `saveNetclass`, `readNetClass`, `getInPcbUnits`, `getInSchUnits` | `save_netclass`, `read_netclass`, `get_in_pcb_units`, `get_in_sch_units` | ported |
| `migrateSchema0to1` .. `migrateSchema4to5` | `migrate_schema_0_to_1` .. `migrate_schema_4_to_5` | ported |
| `CopyFrom` | `NetSettingsJson::copy_from( target, other_json, other )` | ported |
| `NESTED_SETTINGS::LoadFromFile` / `SaveToFile`, `Load`, `Store`, `FormatAsString`, `Internals` on the net settings | `NetSettingsJson::{load_from_parent, save_to_parent, load, store, format_as_string, internals(_mut)}`; `load_net_settings( parent )` convenience | ported |

`NetSettings` stays plain data; the engine (`NetSettingsJson`) is separate and every call takes
the `NetSettings` it reads or writes. Load does not clear caches (as in C++).

### Tests (`tests/net_settings_json.rs`)

* `qa/tests/common/test_net_settings.cpp`: `NetChainClassesJsonRoundTrip`,
  `NetChainClassesJsonRoundTripEmpty`, `NetChainClassesJsonDropsEmptyValues`,
  `ChainNetclassesJsonRoundTrip`, `RemovedChainNetclassIsDroppedFromProjectFile` (file kept in
  memory: `flushToStore` + `formatFileContents`, reopen = `LoadFromBuffer` + nested load),
  `CopyFromProducesEqualInstance`, `CopyFromDropsPreExistingSinkState`,
  `CopyFromClearsStaleDerivedCaches`, `CopyFromClearsStaleChainDerivedPatterns`. Not ported:
  `CopyFromSelfIsNoop` (the borrow rules make `copy_from( self )` impossible).
  `qa/tests/common/test_project_file.cpp` has no net-settings case; its old-version data is
  covered by the corpus oracle below.
* Differential oracle `oracle_documents`: `tests/oracle/net_settings_json_oracle.cpp` (built and
  run by `tests/oracle/net_settings_json_golden.sh`) loads documents with the real
  `NET_SETTINGS` of `libkicommon.so` under a parameterless `JSON_SETTINGS` parent of schema 4
  (`LoadFromBuffer`, nested `LoadFromFile`), prints the load results, every field of every
  class (incl. Default), patterns, labels, colours and chain maps, then stores back
  (`flushToStore`: nested `SaveToFile` + `Store`) and prints the bytes `formatFileContents`
  writes, and a second store. Golden `tests/golden/net_settings_json.txt` (5782 lines): 20
  hand-written documents in `tests/data/net_settings_json` (every migration incl. aborted
  ones, `null`/array class entries, empty assignment objects becoming `null`, non-string
  values aborting a reader midway, NUL characters, out-of-range numbers, bus patterns,
  duplicates, unknown keys, comments, future / missing / non-integer versions, absent / `null`
  / non-object `net_settings`, parse error) with full output, and all 389 `.kicad_pro` under
  `qa/` and `demos/` (net settings versions 0..5 and no `meta`) with the written file as
  length + FNV-1a hash. 409 documents, 673 classes, 2149 patterns, 1167 label assignments:
  **all identical**, including the written bytes of every document (3 corpus files are
  rewritten byte-identically by both; the rest differ from their input only through the
  migrations and the parent's `meta.version`, identically in C++ and Rust).
* `pcb_dump_netclasses`: `tests/oracle/net_settings_json_pcbdump.py` runs
  `pcb-dump board --sections board` (QA load path, board bound to its project) on copies of
  the 261 corpus projects with a sibling `.kicad_pcb` and extracts the netclass table, pattern
  and label assignments and the effective netclass of every net
  (`tests/golden/net_settings_json_pcbdump.txt`). The Rust test loads the `.kicad_pro` alone
  with this module and resolves the effective class of the dumped net names with M4's
  `get_effective_netclass`. 255 projects identical (432 classes with every field, 3186
  pattern/label assignments, 10 667 nets). Not comparable yet (both need package SET-5):
  4 boards with legacy `(net_class ...)` sections (their classes replace the project's in
  `BOARD::SetProject`) and the effective classes of the 2 projects with
  `net_chain_netclasses` (`BOARD::ApplyNetChainNetclasses` adds chain patterns from the
  board's net chains); their class tables and assignments are compared.

### Deviations / notes

* A double that does not fit `int` in `mmToIU` / `MilsToIU` (UB in C++) is converted as the
  reference build does (`f64_to_i32_x86`, `INT_MIN`); the oracle documents include such values.
* `wxString` conversions from JSON stop at the first NUL (`c_str()`), for values and map keys,
  as in C++ (covered by the oracle).
* `JSON_SETTINGS`' constructor stores `meta.filename` (`net_settings.json`) in a fresh nested
  document; the port's nested document starts empty. Only visible in `FormatAsString` of a
  never-loaded instance (the C++ tests only inspect other keys); `LoadFromFile` clears it.
* Iteration over JSON objects in the readers and in migration 3 -> 4 is in key byte order
  (`std::map`), so a reader aborted by a bad value keeps the same entries as in C++.

### Known gaps

* `NET_SETTINGS` of the board-private settings and the project binding (`BOARD::SetProject`,
  legacy netclass transfer, `ApplyNetChainNetclasses`) belong to SET-5.
* `COLOR4D` JSON values go through `Color4D::set_from_wx_string` with the UTF-8 string;
  wxWidgets converts the `std::string` with the C locale first (non-ASCII colour strings are
  not exercised).

## Package M17: vias (PCB_VIA) and via / hole shapes

| C++ (`pcbnew/pcb_track.cpp`, `pcb_track.h`) | Rust | State |
|---|---|---|
| copy constructor / `operator=` | `PcbVia::detached_copy` (inherent, shadows `DetachedCopy`): `PCB_TRACK` members (`m_width`, `m_hasSolderMask`, `m_solderMaskMargin`) reset to constructor values, zone layer overrides kept, no handle | ported (see deviations) |
| `operator==`, `Similarity`, `sameZoneLayerOverrides` | `eq_item`, `similarity` | ported |
| `GetWidth` / `SetWidth` (both overloads), `GetFrontWidth`, `SetPadstackMode`, `Padstack`, `SetPadstack` | `width(layer)`, `set_width(layer, w)`, `width_no_layer` / `set_width_no_layer` (`debug_assert` like `wxCHECK_MSG` / `wxFAIL_MSG`), ... | ported |
| `GetWidthConstraint`, `GetDrillConstraint`, `SetSizeFromRules`, `GetMinAnnulus` | same names (snake case); `DRC_CONSTRAINT::Merge` as a private helper | ported; `SetSizeFromRules` evaluates the rules on a handle-less copy (C++: the `ROUTER_TRANSIENT` item) |
| primary / secondary / tertiary drill, backdrill (`Get/Set{Top,Bottom}Backdrill{Size,Layer}`, mode), post-machining (mode, size, depth, angle), filled / capped accessors, `SetDrill`, `GetDrill`, `SetDrillDefault`, `GetDrillValue` | `via.rs` | ported |
| `IsBackdrilledOrPostMachined`, `HasCopperRemoved`, `GetBackdrillOrPostMachiningKnockout`, `GetPostMachiningKnockout` | `via.rs` (ctx) | ported |
| tenting / covering / plugging / capping / filling: `Set*Mode`, `Get*Mode`, `Resolve*Mode`, `IsTented`, `GetSolderMaskExpansion` | `via.rs` | ported |
| `GetLayer`, `SetLayer`, `GetLayerSet`, `SetLayerSet`, `IsOnLayer` (mask layers of untented sides), `HasValidLayerPair`, `FitsEnabledLayers`, `SetViaType`, `SetLayerPair`, `SetTopLayer`, `SetBottomLayer`, `LayerPair`, `TopLayer`, `BottomLayer`, `SanitizeLayers`, `IsMicroVia`, `IsBlindVia`, `IsBuriedVia`, `IsGhostLayer` | `via.rs` (mutators take `&MutEnv` for `BoardCopperLayerCount` / `GetFlippedLayer`) | ported |
| `ValidateViaParameters` (+ `VIA_PARAMETER_ERROR`) | `PcbVia::validate_via_parameters`, `ViaParameterError`, `ViaParameterField`, `geometry_min_size()` | ported (messages untranslated) |
| `FlashLayer` (int and LSET), `ConditionallyFlashed`, `Get/SetRemoveUnconnected`, `Get/SetKeepStartEnd`, `ClearZoneLayerOverrides`, `Get/SetZoneLayerOverride`, `GetOutermostConnectedLayers`, `Get/SetIsFree` | `flash_layer`, `flash_layer_set`, ... | ported; connectivity through `ctx.connectivity` (`None` = empty CONNECTIVITY_DATA), `this` = the stored item, a handle-less value is unknown to connectivity |
| `CollectMicroviaColumns` | `PcbVia::collect_microvia_columns(ctx) -> Vec<Vec<TrackId>>` | ported; pointer sets -> `BTreeSet<TrackId>` (membership only) |
| `GetBoundingBox` (both), `HitTest` (point, rect), `Rotate`, `Mirror`, `Flip`, `GetItemDescription`, `GetFriendlyName`, `LayerMaskDescribe` | `via.rs` (`bounding_box_layer` for the per-layer overload) | ported |
| `GetEffectiveShape`, `GetEffectiveHoleShape`, `TransformShapeToPolygon` (via branch) | `shapes.rs` | ported |
| `IsType` (locate types), `GetPrincipalLayer` (declared, never defined), `GetCoverageArea`, `ViewGetLayers`, `ViewGetLOD`, `GetMsgPanelInfo`, `GetMenuImage`, `swapData`, `Serialize` / `Deserialize`, property registration | - | skipped: UI / API / undo |

Tests (`pcb_track::via::tests`, 8): a reference-dump golden (`tests/golden/m17_vias.txt`,
regenerated by `tests/golden/m17_vias.py`: `pcb-dump board --sections item,bbox,track` on a
crafted 4-layer board with 17 vias: through, blind, buried, micro, explicit tenting /
covering / plugging / capping / filling, backdrills on both sides, front and back
post-machining, the four unconnected-layer modes incl. a forced zone layer connection,
FRONT_INNER_BACK and CUSTOM padstacks incl. a ghost F.Cu, a bottom-up blind via, a free via
off the mm grid); the vias are rebuilt from the dumped padstack members (the parser does not
load vias yet) and about 1150 dumped derived values are asserted bit for bit (layer, layer set,
position, both bounding boxes per layer, modes, drill value, backdrill / post-machining
accessors, per copper layer `IsOnLayer` / width / `FlashLayer` / effective shape default and
flashed, hole shape, `TransformShapeToPolygon` for three combos by hash). Ports of
`test_via_validation.cpp` (all 8 cases), `test_board_item.cpp`
`BoardlessItemsUseStandardLayerMetadata` (via part), `test_pad_tenting.cpp` via cases (model
half), `test_issue25621_via_flashing_contact_layer.cpp` (via side, with a connectivity
stand-in holding the facts the C++ test `BOOST_REQUIRE`s). Hand-checked against the C++
bodies, not oracle-compared: mask-layer effective shapes, hit tests, transforms, texts,
`SanitizeLayers` backdrill clean-up, rule-resolver branches (need the DRC engine),
`CollectMicroviaColumns` on real stacks (only the empty case is tested; DRC via-stack tests
will exercise it).

Deviations:
- `detached_copy` on a `Track` value is still the generic clone + no handle (keeps the
  `PCB_TRACK` members of a via); `PcbVia::detached_copy` is exact. Not observable through any
  via method (none reads those members).
- `BOARD::InvalidateClearanceCache` after `SetLayerPair` / `SetTopLayer` / `SetBottomLayer` has
  no model counterpart (`board-model.md` 7).
- A non-copper end layer in `LAYER_RANGE` (C++ throws) gives an empty range plus a
  `debug_assert`.
- `IsTented` with a layer of neither side, `GetWidth()` / `SetWidth()` without a layer and
  `IsGhostLayer` with an unsupported layer `debug_assert` (C++ `wxCHECK` / `wxFAIL`) and take
  the C++ fallback.

Stubs left in `pcb_track/{via,shapes}.rs`: none. `Board::set_zone_layer_override` /
`clear_zone_layer_overrides` (via branch) stay `port_pending!` in `pad/mod.rs` (M14's file):
change request to call `PcbVia::set_zone_layer_override` / `clear_zone_layer_overrides`.

## Package M8: EdaShape shapes (effective shapes, polygons, hatching, line endings, stroke)

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `EDA_SHAPE::makeEffectiveShapes( aEdgeOnly, aLineChainOnly, aHittesting )` (every `SHAPE_T`) | `eda_shape::shapes::make_effective_shapes_list` (caller-owned `Vec<Box<dyn Shape>>`); `eda_shape::make_effective_shapes` (`Vec<SharedShape>`, M0c signature) | ported |
| `MakeEffectiveShapes`, `MakeEffectiveShapesForHitTesting`, `MakeEffectiveShapesForStroking`, `MakeEffectiveShapesWithLineEndings`, `makeShortenedBodyShapes`, `ShortenedBezierPolyline`, `TransformWithLineEndingsToPolygon` | generic fns in `eda_shape/shapes.rs`, method syntax through `shapes::EdaShapeShapesExt` (blanket) | ported |
| `MakeLineEndingEffectiveShapes`, `TransformLineEndingsToPolygon`, `GetLineEndingsBoundingBox`, `GetEndingTangents`, `GetLineEndingEndpoints`, `ShortenSegmentForEndings`, `ShortenArcForEndings`, `ShortenedBezierCurve`, `ShortenPolyForEndings`, `ShortenBodyPolyPoints`, `GetShortenedBodyPolyPoints`, statics `lineEndingClosedChain`, `hasLineEnding`, `addLineEndingEffectiveShapes`, `addLineEndingPolygon`, `bezierSpeedAt`, `bezierLength`, `findBezierTAtLength` | inherent on `EdaShape` / private fns in `shapes.rs` | ported |
| `TransformShapeToPolygon` (every `SHAPE_T`, clearance, error, `ERROR_LOC`, `ignoreLineWidth`, `includeFill`) | `shapes::transform_shape_to_polygon`, `eda_shape::shape_transform_to_polygon` | ported |
| `UpdateHatching`, `GetHatching`, `GetHatchLines`, `hatching()`, `hatchLines()` | `shapes::build_hatching`, `eda_shape::hatching_data` (cache `hatching_cache`), `EdaShapeExt::{hatching, hatch_lines}` | ported |
| `hitTest( VECTOR2I )` / `hitTest( BOX2I )` line-ending branches (left by M7) | `geometry::hit_test_point` / `hit_test_rect` | ported |
| `STROKE_PARAMS::Stroke` (rect, simple, line chain, segment, arc, ellipse) | `stroke_params::stroke` with `StrokePatternLengths` (the three `RENDER_SETTINGS` lengths, evaluated by the caller) | ported |
| `STROKE_PARAMS::GetLineStyleToken`, `Format`, `operator<`, parser | `kicad_core::stroke_params` (already there) | - |
| `PCB_SHAPE::UpdateHatching` (forces dirty), `getHatchingKnockouts`, `GetEffectiveShape`, `GetBoundingBox` | package M9 (they call the functions above) | - |
| `LINE_ENDING::Draw` / `Plot`, `KIGFX::DrawDashedLine` (qa/tests/common/test_dashed_line.cpp) | - | skipped: GAL / plotter drawing, not model geometry |

### Tests

- `eda_shape::shapes::tests` (qa/tests/common/test_eda_shape.cpp): `EllipseMakeEffectiveShapesNonEmpty`,
  `ShortenSegmentForEndingsTrimsStartAndEnd`, `SegmentOpenArrowLeavesBody`,
  `SegmentShorteningRejectsOverrun`, `SegmentEndingTangentsPointOutward`,
  `OpenArrowBezierTangentsUseDepth`, `LineEndingEndpointsUseShapeOwnedSources`,
  `LineEndingEndpointsRejectInvalidPolys`, `EffectiveShapesIncludeClosedEnding`,
  `EffectiveShapesIncludeOpenArrowLegs`, `BBoxIncludesCenteredEnding`,
  `BBoxUsesRotatedEndingGeometry`, `PolygonConversionAppendsEndings`,
  `EffectiveShapesShortenArrowBody`, `StrokingShapesShortenArrowBodyOnly`,
  `StrokingShapesRejectConsumedSegmentBody`, `EffectiveShapesHitEndpointSquare` (+ line-ending
  hit tests), `PolygonConversionShortensArrowBody`, `ShortenArcForEndingsKeepsPositiveSweep`,
  `ShortenArcForEndingsKeepsNegativeSweep`, `ArcShorteningRejectsOverrun`,
  `BezierNoEndingsReturnsSource`, `BezierCurveTrimsBothEnds`, `BezierCurveRejectsOverrun`,
  `ShortenedBezierPolylineUsesShortenedCurve`, `BezierPolylineRejectsOverrun`,
  `StrokingShortenedBezierDashes`, `PolyShorteningTrimsEnds`,
  `ShortenPolyForEndingsLeavesOpenArrowBody`, `BodyPolyPointsShortenFirstOpenOutline`; plus
  `effective_shape_variants` (edge-only / line-chain-only / hit-testing variants).
- `stroke_params::tests`: `BezierDashedLineHasGaps`, `BezierDottedLineIsEvenlySpaced`
  (qa/tests/pcbnew/test_issue25110_bezier_line_style.cpp), zero-width chain guard, and
  `stroke_matches_oracle`: C++ driver `tests/oracle/m8_stroke_oracle.cpp`
  (`m8_stroke_build_oracle.sh`, links `libcommon.a` of the reference build), golden
  `tests/golden/m8_stroke.txt`: 11 shapes (segments, chain with a duplicate vertex, simple,
  rect, rounded rect, arcs ccw / cw / ring, ellipse, ellipse arc) x 5 styles x 2 widths = 110
  cases, every piece hashed: 110/110 identical.
- `tests/m8_shapes_oracle.rs` (feature `dump`): reference `pcb-dump board --sections shape` of
  `tests/oracle/m8_shapes.kicad_pcb` (32 shapes: segments, rects plain / reversed / rounded /
  filled / zero width, the three hatch kinds, circles, arcs, filled / unfilled / hatched polys,
  beziers, ellipses, ellipse arcs; line endings of every style incl. stroke widths), golden
  `tests/golden/m8_shapes.txt` (`m8_shapes_golden.sh`). Shapes are rebuilt from the dumped
  members (parser not loading shapes yet) on a board-level `PCB_SHAPE`-like host;
  `shape.<L>` (`SHAPE_COMPOUND( MakeEffectiveShapesWithLineEndings( GetEffectiveWidth() ) )`),
  the three `tpoly` combos and `hatching`: 136/136 bit identical.
- `test_ellipse_roundtrip.cpp` is file I/O (parser / writer packages); its shape maths are
  covered by the ellipse cases of the dump oracle.

### Deviations / notes

- Hatching cache: the C++ keeps its previous (stale) cache when `UpdateHatching` returns early
  while dirty (not hatched, zero spacing, open shape, empty polygon); in this port every edit
  that dirties the cache drops it, so such an early return yields an empty hatching (what the
  C++ returns for a shape whose cache was never built). `isMoving()` (interactive edit
  state) is not ported: always false.
- `ROUNDRECT::TransformToPolygon` of a degenerate rounded rectangle leaves the set empty and
  the C++ then reads `Outline( 0 )` (UB); the port uses an empty chain.
- `STROKE_PARAMS::Stroke` with `LINE_STYLE::DEFAULT`: the C++ throws `std::out_of_range`
  (`lineTypeNames.at`); the port strokes it like `SOLID` (six width-long elements).
- In/out parameters of the shortening helpers stay `&mut`; pure out-parameters became return
  values (`Option<(start, end)>`, `(start_tangent, end_tangent)`, `Option<Box2I>`,
  `Option<Vec<Vector2I>>`).
- Kept C++ quirks: `makeShortenedBodyShapes` / `TransformWithLineEndingsToPolygon` pass the
  stroke (resp. polygon) width as the ending size for ellipse arcs; `Stroke` of a `SH_RECT`
  strokes every corner arc twice.
- The M7 note "hitTest on a shape with a line ending panics with the M8 port-pending message"
  no longer applies.

### Known gaps

- None in M8's files (no `port_pending!` left). `PcbShape::shape_max_error`,
  `hatching_knockouts`, `effective_shape` and `bounding_box` (M9) are still stubs, so the
  functions above are verified on test hosts; a real `PcbShape` goes through them once M9
  lands.

## Package SET-4: board design settings JSON (`board.design_settings` in `.kicad_pro`)

### C++ to Rust map

| C++ | Rust (`src/project/board_design_settings_json.rs`) | State |
|---|---|---|
| `BOARD_DESIGN_SETTINGS` constructor: all 90 `m_params` registrations (BOM, `rules.*`, `defaults.*`, `defaults.zones.*`, lambdas `rule_severities`, `drc_exclusions`, `track_widths`, `via_dimensions`, `via_stack_presets`, `diff_pair_dimensions`, `teardrop_options`, `teardrop_parameters`, `tuning_pattern_settings`, zone lambdas) | `build_params` (registration order) | ported |
| `registerMigration` 0->1 (`migrateSchema0to1`), 1->2 (inline lambda), 2->3 (`migrateSchema2to3`) | `register_migrations`, `migrate_schema_0_to_1`, `migrate_schema_2_to_3` | ported |
| `BOARD_DESIGN_SETTINGS::LoadFromFile` (nested load + legacy post-step) | `BoardDesignSettingsJson::load_from_file` (`Err` = the exception that escapes C++) | ported |
| `JSON_SETTINGS::Load` / `Store`, `NESTED_SETTINGS::SaveToFile` for this object | `load`, `store`, `save_to_file` | ported (generic part in kicad-core) |
| `BOARD::SetProject`'s `bds.SetParent( &projectFile, !m_LegacyDesignSettingsLoaded )` | `load_board_design_settings` (caller skips it for legacy boards) | settings side only |
| `FIELDS_TABLE_BOM_SETTINGS`, `BOM_FIELD`, `BOM_PRESET`, `BOM_FMT_PRESET` (`to_json`/`from_json`, `DefaultEditing`, `CSV`) | `FieldsTableBomSettings`, `BomField`, `BomPreset`, `BomFmtPreset` | ported (JSON only) |
| `PNS::MEANDER_SETTINGS` JSON (`make_settings` / `read_settings`) | private `MeanderSettings` (the model keeps the `make_settings` objects) | ported |
| `DRC_ITEM::GetItemsWithSeverities()` order | `ITEMS_WITH_SEVERITIES` | ported (headings included: a `""` key sets code 0 as in C++) |
| `DRC_EXCLUSION` `from_json` / `FromLegacyStrings` | trait `DrcExclusionCodec` (implemented by the caller; `kicad-drc` has the functions), `RawDrcExclusionCodec` fallback | interface |

API: `BoardDesignSettingsJson::new(path, codec)`; `load_from_file(Some(&mut project_internals), parent_is_project_file, &mut bds) -> Result<bool, JsonError>`; `save_to_file(&mut project_internals, &mut bds, force) -> bool`; then `JsonSettings::format_file_contents`-style output of the parent (`original` merged with `doc`, `dump(2)` sorted) is byte-exact. The BOM members live in `BoardDesignSettingsJson::bom` (not in `BoardDesignSettings`).

### Tests

- Differential oracle `tests/oracle/bds_json_oracle.cpp` + `bds_json_golden.sh` (real `SETTINGS_MANAGER::LoadProject`, `BOARD_DESIGN_SETTINGS::SetParent` + `LoadFromFile`, every touched member printed, then `SaveToFile` + `PROJECT_FILE::SerializeToString` and the `board.design_settings` subtree of the written file):
  - `tests/golden/bds_json_scenarios.txt` (21 hand-written projects in `tests/data/bds_json`: migrations 0->1 (inch/mm/float units), 1->2 (truncation 0.0638 -> 63799, overflow -> INT_MIN, missing `rules`), 2->3 (legacy strings, a throwing entry aborting the chain), missing / wrong-typed / future / non-object versions, every range bound, wrong types per parameter class, all lambdas incl. aborting setters, `hole_near_hole`, the `""` key, legacy courtyard flags, `legacy` fillets incl. the escaping exception, BOM presets, a `stored=error` from `update()`): replayed line by line by `tests::oracle_scenarios` - all identical except the documented deviation SET-4-2 (one line).
  - `tests/golden/bds_json_corpus.txt` (all 386 `.kicad_pro` of `qa/data` and `demos`, hashed; 2 are rejected by the C++ project loader): `tests::oracle_corpus` - 384/384 identical, including the stored subtree byte for byte; for the 6 projects with non-empty legacy `drc_exclusions` the exclusion lines are not compared (they need the kicad-drc codec, see Known gaps).
- `tests/oracle/bds_json_ds_compare.py`: `pcb-dump board --sections board` (QA load path, project bound) vs the Rust parser + this loader (`tests::pcb_dump_ds_lines`, `--features dump --ignored`) on the 253 boards with a sibling project (large demos excluded): every `ds.*` field the parameters write (minima, severities, size lists, via stack presets, teardrops, layer classes, dimensions, style flags, default zone settings, max error, mask clearance) identical; only `ds.drc_exclusions` differs on the 6 legacy-exclusion projects (same cause).
- Ported from `qa/tests/pcbnew/test_board_design_settings.cpp`: `NegativeSilkClearanceRoundTrip`, `SeededBoardValueSurvivesRoundTrip`, `OneMalformedPresetDoesNotDropTheRest`, `AnAbsurdPresetPitchIsClampedOnLoad`. From `qa/tests/common/test_project_file.cpp` only the BDS part of `NoRewriteWhenUnchanged` applies (`tests::no_rewrite_when_unchanged`); the other cases are project-file / sheet / manager behaviour (SET-5/SET-13).

### Deviations

- SET-4-1: without a DRC codec (`RawDrcExclusionCodec`) an exclusion object is kept verbatim with its compact sorted dump as sort key (no protobuf canonicalisation / ordering) and legacy marker strings are dropped. Callers that have `kicad-drc` must pass a codec built on `kicad_drc::exclusion::{from_json, from_legacy_strings}`.
- SET-4-2: `defaults.zones.pad_connection` / `remove_islands` values without an enumerator (`static_cast` of an arbitrary int in C++) leave the member unchanged; C++ stores and writes the raw int back. No real project has such values.
- The migration 1->2 member assignments are recorded by the migrator and applied right after the nested load (the C++ lambda assigns them during the migration); no parameter reads them in between, so the result is the same.

### Known gaps

- `DrcExclusionCodec` for `kicad-drc` is not implemented here (crate dependency direction); the 6 legacy-exclusion projects are verified only without their exclusions.
- The board binding itself (`BOARD::SetProject`, legacy net classes) is SET-5.

## Package PCBIO-09: board file groups and deferred resolution

Files: `src/io/parser/group.rs`, `src/io/writer/group.rs`, `tests/data/pcbio09/`,
`tests/golden/pcbio09/`, `tests/golden/pcbio09_corpus_groups.txt`,
`tests/oracle/pcbio09_golden.sh`.

### C++ to Rust map

| C++ | Rust | state |
|---|---|---|
| `parseGROUP_members` | `parser/group.rs` `parse_group_members` | ported |
| `parseGROUP` (name, bare `locked`, `uuid` / legacy `id`, `lib_id` with `{slash}` and both errors, `(locked B)`, `members`, `custom_property`) | `parse_group` | ported |
| `resolveGroups` (footprint lookup map, `BOARD::ResolveItem`, pass 1 create + `Add( INSERT )`, pass 2 members with the same-parent-footprint rule, append-mode `m_resetKIIDMap`, generator layer from the first member track, `GroupsSanityCheck( true )`) | `resolve_groups` | ported |
| generator branch of pass 1 (`CreateFromType` + "Cannot create generated object of type '%s'", `SetLayer`, `SetProperties`, `SetTemplateItem`) | `generator_from_info` (hook for PCBIO-13, which fills `generator_infos`) | ported, unexercised until `parseGENERATOR` lands |
| `format( const PCB_GROUP* )` (member validation against the board index, empty groups skipped, `locked`, `lib_id` with manual quotes, sorted member uuids, custom properties) | `writer/group.rs` `format_group` | ported |

### Tests

`src/io/parser/group.rs` `tests`:
- `oracle_resave_groups` (byte level): two hand-made boards (current format: nested groups,
  forward references, duplicate / unknown / self / ancestor members, members moving between
  groups, `{slash}` lib id, lib id without nickname, custom properties, UTF-8 and escaped
  names, both lock forms; a 20231212 file with `(id ..)` and an ancestor cycle) re-saved by
  `kicad-cli pcb upgrade --force`: the `(group ..)` blocks are identical for the input and for
  the re-save of the reference output (`_o2`).
- `oracle_dump_groups` (item level, feature `dump`): `pcb-dump board --no-init --sections
  misc,item` of the same boards: all lines match except `locked_flag` (see known gaps).
- `parse_errors`: the reference texts of the bad `lib_id`, unknown token and bad header.
- `oracle_corpus_groups`: the 86 `(group ..)` blocks (75 board, 11 footprint level) of the 28
  corpus boards containing groups, re-saved by the reference build: a block must be identical
  when the parse skipped nothing, otherwise its header must be identical and its member count
  at most the reference one (members pending on staging stubs are unresolved: they simply are
  not on the board and the test tightens by itself as the item packages land).
- Ports: `test_group_load_save.cpp` `GroupsLoadSave` (both files; member count bounded while a
  graphic parser of the file is a stub), `group_saveload.cpp` `HealthyGroups`,
  `SingleMemberGroupsSaved` (full round trip once the text writer PCBIO-05 lands; until then
  the group-to-group memberships are checked through the staging switches), plus
  `cycles_in_file_are_refused`. `InvalidGroups` is the M22 test; `DeepClone*` are model tests
  (no `DeepClone` in the port).

Verification status (2026-11-10, re-checked after a restart): the hand-made goldens and five
corpus entries of `pcbio09_corpus_groups.txt` were regenerated with the reference build and are
unchanged. `oracle_corpus_groups`: 2 blocks identical, 17 with identical header and members
pending, 66 pending (20 boards stop on the `EDA_SHAPE::Compare` model stub in the writer, the
large demos are skipped). Item level on the corpus (`run.sh --dump NAME --sections misc,item`,
groups_load_save, issue18839, issue22316, issue22983, issue24192, issue24264,
multichannel_nested, group_and_image): every board-level `PCB_GROUP_T` exists with the
reference UUID; every `group=` field of a parsed item matches except members of generators
(`(generated ..)`, PCBIO-13 pending: 648 lines on multichannel_nested); remaining group field
differences are `position` / `layerset` / `side_specific` computed from members that are
footprints (PCBIO-06 pending) and the pending `GetFPRelativePosition` model stub. Members that
are not parsed yet (staging skip with `allow_pending`) are simply unresolved (`ResolveItem`
finds nothing, as for a missing member in C++) and resolve once their parser lands.

### Deviations / notes

- `IsItemIndexedById( const BOARD_ITEM* )` is "the index entry of the item's UUID is this
  handle" (Rust handles cannot dangle).
- `LIB_ID::Parse` returns a byte offset into the item name; the error message picks the
  character at that index of the full name as the C++ does (`name[bad_pos]`, characters).
- The fresh ids the C++ draws (`GROUP_INFO()`, `new PCB_GROUP`, `new T` for generators) are
  drawn from `board.kiid_generator` at the same points.
- A generator declared inside a footprint is refused by `footprint_add` and dropped (the C++
  `FOOTPRINT::Add` has no case for it and leaks it).
- Group member order is insertion order (C++ `unordered_set`); only the generator layer pick
  ("first member track") could depend on it, and the C++ comment says it is only meaningful
  when all members share a layer.

### Known gaps

- pcb-dump-rs prints `locked_flag` as the raw `m_isLocked`; the C++ dump prints the
  non-virtual `BOARD_ITEM::IsLocked()` (parent group included): change request to DUMP-RS,
  the test skips that field.
- Footprint-level groups are parsed only once `parseFOOTPRINT` (PCBIO-06) calls `parse_group`.
- Generator resolution is unexercised until PCBIO-13 pushes `GeneratorInfo`s (it must also draw
  the `GENERATOR_INFO()` id, as `parse_group` does).

## Package M15: pad shapes (effective shapes, polygons, holes, mask shapes, hit tests)

### C++ to Rust map

| C++ (`pcbnew/pad.cpp`) | Rust (`pad/shapes.rs`) | State |
|---|---|---|
| `BuildEffectiveShapes`, `buildEffectiveShape` (every `PAD_SHAPE`, custom anchor + primitives, shape order inside the compound) | `build_effective_shapes`, `build_effective_shape` (private) | ported |
| `BuildEffectivePolygon` (per-layer polygons, bounding radius for `ERROR_OUTSIDE`) | `build_effective_polygon` (private) | ported |
| `m_shapesDirty` / `m_polyDirty[]` / `m_maskShapes` caches | `Pad::caches` (`KeyedCache`, key = parent transform + flip + `GetMaxError()`) | ported (`board-model.md` 9) |
| `GetEffectiveShape` (Edge.Cuts, unflashed PTH, mask shape, machining hole for physical / silk clearance), `getMaskShape` | `effective_shape`, `mask_shape` | ported |
| `GetEffectiveHoleShape`, `GetEffectivePolygon`, `GetBoundingRadius`, `HasCopperRemoved` | `effective_hole_shape`, `effective_polygon`, `bounding_radius`, `has_copper_removed` | ported |
| `GetBoundingBox()`, `GetBoundingBox( aLayer )` | `bounding_box`, `bounding_box_layer` | ported (the layer variant does not store into the draw cache: the value it would store is never read) |
| `TransformShapeToPolygon`, `TransformHoleToPolygon`, `MergePrimitivesAsPolygon` | same names | ported (Clipper calls through `ShapePolySet`) |
| `HitTest` (point, point + layer, rect, polyline) | `hit_test`, `hit_test_layer`, `hit_test_rect`, `hit_test_poly` | ported (`hit_test_poly` has no trait method; inherent only) |
| `addPadPrimitivesToPolygon`, `GetBestAnchorPosition` | - | not present in this C++ version |
| `FlipPrimitives` | `pad/derive.rs` | M14 |

### Tests

- `pad::shapes::tests::oracle_pad_shapes`: differential test against the reference `pcb-dump`
  (`tests/oracle/m15_pad_shapes_golden.py`, golden `tests/golden/m15_pad_shapes.txt`,
  reproduced byte for byte on re-generation) on `tests/oracle/m15_pad_shapes.kicad_pcb` (28 pads:
  every pad shape, custom pads with circle / rectangle anchors and every primitive kind incl. a
  proxy, NORMAL / FRONT_INNER_BACK / CUSTOM padstacks, mask margins of both signs, offsets,
  oblong drills; footprints identity, rotated 30 + scaled 1.5 x 0.75, flipped + rotated -90 +
  scaled 2 x 1, rotated 45) and qa boards padstacks_complex, padstacks, custom_pads, issue4139,
  issue6443. 135 pads are rebuilt from the dumped members and 2820 values compared (FNV hashes
  of the full vertex lists): effective shape per layer (default and forced flashing), effective
  polygons inside / outside, hole shape, `TransformHoleToPolygon` and `TransformShapeToPolygon`
  for three clearance / error / ERROR_LOC combos, all-layer and per-layer bounding boxes,
  bounding radius, mask expansion. All match. 2 pads skipped (circle primitives of a
  non-uniformly scaled footprint become ellipses, whose parameters the dump does not print).
- `qa/tests/pcbnew/test_pad_flashing.cpp` `TechLayersPullFromAppropriateSide`;
  `test_plot_pad_mask.cpp` `RoundRectMaskMatchesInflatedOutline`.
- New: `caches_follow_dirty_and_key` (cache sharing, rebuild on transform change, mask cache per
  expansion, the four hit tests).
- Not ported: `test_fab_pad_geometry.cpp` (loads boards and plots: parser + plotter),
  `test_clip_line_to_pad.cpp` (`LENGTH_DELAY_CALCULATION::OptimiseTraceInPad`, not model
  logic), `test_custom_pad_primitive_roundtrip.cpp` (file round trips: parser / writer;
  the scaling logic is M14's), `test_pad_check.cpp` (`CheckPad`, DRC / editor side).

### Deviations / notes

- `GetEffectiveShape` with a layer missing from the cache: C++ returns `nullptr` after
  `wxCHECK`; the port returns a `SHAPE_NULL` (debug assertion). `GetEffectivePolygon` with a
  missing layer: C++ `at()` throws; the port returns an empty set (unreachable via
  `EffectiveLayerFor`).
- `buildEffectiveShape` of a chamfered rect with a degenerate (empty) outline: C++ reads
  `COutline( 0 )` (UB); the port adds no body.
- The hit tests are not covered by the oracle (pcb-dump prints no hit tests); they are
  compositions of the oracle-verified polygons / radius / hole and `kicad_geom` collision code,
  checked by unit cases only.

### Known gaps / stubs left

- None in `pad/shapes.rs`. Run-time dependencies: custom primitives go through `PCB_SHAPE`
  (M9) / `EDA_SHAPE` (M8) shape and polygon code.
- `Padstack` `ShapeProps::default` writes a literal `0` for `chamfered_rect_positions`
  (`padstack.rs`, not this package's file): equal to `RECT_NO_CHAMFER`; cosmetic change
  requested.

## Package PCBIO-17: corpus differential harness of the board reader / writer

Test support, no product code. Driver: `rust/tools/io-corpus/run.sh` (README there).

### Files

| What | Where | C++ origin |
|---|---|---|
| corpus list, skip list, oracle cache (O1 / O2, key = FNV-1a 64 + size of the input + oracle binary mtimes, 2 jobs max, nice'd, private `KICAD_ORACLE_HOME`) | `crates/kicad-qa/src/io_corpus.rs` | new (`pcb-io.md` 9.3 D2) |
| UUID canonicaliser (mask random UUIDs, keep UUIDs of the original and legacy-timestamp UUIDs; optional sorting of masked sibling runs) | `crates/kicad-qa/src/uuid_canon.rs` | new (`pcb-io.md` 9.3 D3) |
| s-expression text helpers (list scanner, first difference with keyword path, top-level strip) | `crates/kicad-qa/src/sexpr_text.rs` | new |
| `GetPcbnewTestDataDir`, QA data paths | `crates/kicad-qa/src/board_file_utils.rs` | `qa/pcbnew_utils/board_file_utils.cpp` (file-system part) |
| `ReadBoardFromFile`, `RequireBoardFromPcbnewQAData`, `DumpBoardToFile`, footprint read / dump | `tests/io_corpus/board_file_utils.rs` | `qa/pcbnew_utils/board_file_utils.cpp` |
| `rust_resave` (parse + format with / without staging), status classification | `tests/io_corpus/resave.rs` | new |
| scoreboard / per-board report rendering | `tests/io_corpus/scoreboard.rs` | new |
| tests `corpus_scoreboard` (ignored, oracle), `golden_staging`, `strict_<stem>` x15 (ignored, naming the pending packages), `board_file_utils_helpers` | `tests/io_corpus.rs` | new |
| golden set: O1 (+ O2 when not a fixed point) of 15 boards < 16 KB, source versions 20200119 .. 20260206, `expected.txt` | `tests/golden/io_corpus/` | oracle output |

### Verification

Oracle cache over 375 corpus boards (370 run, 5 skipped: vme-wren x2, VCU118, jetson-agx-thor at
>= 10 MB, `plugins/fakeboard.kicad_pcb` known bad); 287 fixed points. Re-run fully cached:
about 50 s (mostly the cargo build). Scoreboard of 2026-11-10: identical 34, identical-modulo-
pending 159, differs 20 (all non-fixed points whose Rust re-save equals O1 modulo pending, i.e.
the O1 -> O2 change is the oracle's post-load net renumbering / track reordering, PCBIO-15),
model-pending 157 (`EDA_SHAPE::Compare` 149, `PCB_SHAPE::SetStart (PCB_TEXTBOX)` 8), no parse /
format errors or panics. `run.sh --board` and `run.sh --dump` exercised on `custom_fields` and
`issue18839`.

### Deviations / notes

* `rust_resave` is parse + format only; the oracle's `BOARD_LOADER::initializeLoadedBoard` steps
  are not reproduced (PCBIO-15), differences coming from them are reported as `differs`
  (flagged `= O1`), never masked.
* Boards whose O1 holds render caches / embedded fonts carry a caveat column (host fonts).

### Known gaps

* D3 (legacy input vs O1 after `canonicalise_uuids_sorted`) is provided by `kicad_qa::uuid_canon`
  but not yet run by the scoreboard (uninformative until the tier-1 packages land).
* D1 (footprint libraries, `fp upgrade`) is not part of this harness.
* The oracle-rejects / port-rejects check of `pcb-io.md` 9.2 covers only the skip-listed
  `fakeboard`; `qa/data/fuzz` is outside the corpus (`qa/data/pcbnew` + `demos`).

## Package M18: zones (ZONE model, zone utilities)

| C++ (`pcbnew/zone.cpp`, `zone.h`, `zone_utils.cpp`) | Rust | State |
|---|---|---|
| constructors (`ZONE( aParent )` with / without board, footprint rule areas), member initialisers | `Zone::default`, `Zone::new`, `Zone::new_with_settings` (`ExportSetting( *this, false )`, then `m_needRefill = false`; the footprint `SetIsRuleArea( true )` is overwritten by the settings like in C++) | ported |
| layer set (`SetLayer`, `SetLayerSet` with fill-container upkeep, copper-thieving clamp, `SetLayerSetAndRemoveUnusedFills`, `GetLayer`, `GetFirstLayer`, `IsOnLayer`, `IsOnCopperLayer`) | `zone/mod.rs` | ported |
| priority (`Get/SetAssignedPriority`, `HigherPriority`, `SameNet`), net (`SetNet` / `SetNetCode` copper-thieving override) | `assigned_priority`, `higher_priority`, `same_net`, `filter_net_code` (callers apply it before `SetNetCode`, `board-model.md` 8) | ported |
| outline access: `Outline()`, `GetLibraryOutline`, `GetBoardOutline` (footprint frame), `GetNumCorners`, `GetCornerPosition`, `RemoveAllContours`, `NewHole`, `AppendCorner`, `AddPolygon` (both), `RemoveCutout`, `MoveEdge` | `library_outline`, `board_outline(ctx)`, `outline_mut(env)`, ... | ported |
| border hatch: `SetBorderDisplayStyle`, `HatchBorder`, `UnHatchBorder`, `GetHatchLines`, `GetDefaultHatchPitch` | `zone/mod.rs` | ported (oracle) |
| `BuildSmoothedPoly` (chamfer / fillet, same-net neighbour aprons, board-outline clip), `TransformSmoothedOutlineToPolygon`, `TransformSolidAreasShapesToPolygon`, `TransformShapeToPolygon`, `GetEffectiveShape` | `zone/shapes.rs` | ported (oracle) |
| fill containers: `Get/SetFilledPolysList`, `MoveFilledPolysList`, `HasFilledPolysForLayer`, `GetFilledLayers`, `IsFilled` / `SetIsFilled`, `NeedRefill` / `SetNeedRefill`, fill flags, `IsIsland` / `SetIsIsland`, `UnFill`, `BuildHashValue` / `GetHashValue`, `CacheTriangulation`, `CacheOutlineTriangulation` | `Zone` getters + `ZoneFill` writers (`Board::zone_fill_mut`, derived state) | ported |
| `GetBoundingBox` (time-stamp keyed cache, `board-model.md` 6), `CacheBoundingBox`, `computeBoundingBox`, `OnFootprintTransformed` | `zone/shapes.rs`, `cache_bounding_box` | ported |
| hit tests: point, `HitTestForCorner`, `HitTestForEdge`, rect, polygon, `HitTestFilledArea`, `HitTestCutout` | `zone/shapes.rs` | ported (oracle) |
| `GetLocalClearance()` (0 for a rule area) / `GetLocalClearance( aSource )` (nullopt for a rule area), `GetThermalReliefGap( aPad )`, spoke width, pad connection, keepout / placement / teardrop flags | `local_clearance`, `local_clearance_with_source`, `thermal_relief_gap_for_pad`, ... | ported |
| `Move`, `Rotate`, `Flip`, `Mirror` (both), `OnFootprintRescaled`, `SetPosition` (no-op) | `zone/mod.rs` | ported (oracle) |
| `GetInteractingZones`, `CalculateFilledArea`, `CalculateOutlineArea`, `GetItemDescription`, `GetFriendlyName`, `operator==` | `interacting_zones`, `calculate_*_area`, `item_description`, `friendly_name`, `eq_item` | ported (oracle) |
| `MergeZonesWithSameOutline`, `AutoAssignZonePriorities` (overlap pairs, constraints, Kahn graph, same-net union-find) | `zone_utils.rs` | ported (oracle + unit tests) |
| `GetCoverageArea`, `Similarity`, `ViewGetLayers`, `ViewGetLOD`, `GetMsgPanelInfo`, `GetMenuImage`, `swapData`, `Clone( aLayer )`, `Serialize` / `Deserialize`, property registration | - | skipped: UI / API / undo (not in the model API) |
| `FOOTPRINT::cmp_zones` (save order) | `footprint/compare.rs` | not this package's file: stays `port_pending!` there (footprint package) |

Tests (21 in `zone::tests`, 1 oracle replay in `zone::oracle_tests`; 17 of them are C++ test ports): ports of
`qa/tests/pcbnew/test_zone.cpp` `SingleLayer`, `MultipleLayers`, `RescuedLayers`,
`CircleZoneCutoutRoundTrip`, `EmptyZoneGetPosition`, `ZoneMerge*` (4), `AutoPriority_*` (7);
`test_board_item.cpp` `BoardlessItemsUseStandardLayerMetadata` (zone part, interacting zones);
local-clearance overloads and copper thieving / layer clamp. Not ported: `RuleAreaCoverageArea*`
/ `FilledZoneCoverageUsesFilledPolygons` (`GetCoverageArea`, selection heuristics, not in the
model), `RuleAreaOutlineDrawnAboveCopper` (painter), `LibraryParityDetects*`
(`FOOTPRINT::FootprintNeedsUpdate`, footprint package), `SimilarZoneGetsUniqueName` (drawing
tool).

Differential oracle: `tests/oracle/m18_zone_oracle.cpp` (links the pcb-dump objects of the
reference build; `m18_zone_golden.sh`, under the C++ build lock) on
`tests/oracle/m18_zones.kicad_pcb`: 14 zones (fillet / chamfer smoothing, overlapping same-net
zones, priorities, a two-outline multi-layer zone with a hole, keepouts, a teardrop area, a
mask-layer zone, hatch styles edge / full, footprint zones in a rotated + non-uniformly scaled
footprint and in a rotated back-side footprint). The golden `tests/golden/m18_zones.txt`
(1005 lines) was regenerated on 2026-11-10 and is byte-identical to the committed one. The Rust
replay rebuilds the zones from the dumped members (the zone parser, PCBIO-08, is not in yet) and
compares more than 750 derived values as text: layers, bbox, board outline, hatch lines, smoothed
polygons and aprons (with and without board outline), `TransformSmoothedOutlineToPolygon`,
interacting zones, every hit test, priorities, equality, Move / Rotate / Flip / Mirror results,
fill hash / area / `TransformShapeToPolygon` / effective shape / `UnFill`,
`MergeZonesWithSameOutline` and `AutoAssignZonePriorities`. All match. pcb-dump vs pcb-dump-rs
on corpus boards cannot run yet: loading a board with zones hits the parser's
`port_pending!("ZONE::SetLayerSetAndRemoveUnusedFills")` (change request below).

Deviations:
- `ZoneFill::cache_triangulation` triangulates through `Arc::make_mut`: a fill shared with
  another holder (connectivity) is copied first, while C++ triangulates the shared object in
  place. The triangulation is a cache, so no result differs.
- `AutoAssignZonePriorities`: the pair constraints, computed on the thread pool in C++, are
  computed sequentially in pair order (C++ collects them in submission order too). The C++
  `unordered_map<ZONE*, ..>` containers become maps keyed by the eligible-zone index and are only
  looked up, never iterated. The progress reporter argument is dropped.
- `GetThermalReliefGap( aPad )` reads the pad's F.Cu thermal gap (`PAD::GetLocalThermalGapOverride()`
  is `m_padStack.ThermalGap()`, default layer F.Cu).
- Texts (`GetItemDescription`, `GetFriendlyName`, sources "zone") are untranslated English.

Known gaps: none in the package files (no `port_pending!` left in `zone/*` or `zone_utils.rs`).
Change requests: `io/parser/board.rs` should call `Zone::set_layer_set_and_remove_unused_fills(
zone.layer_set & enabled_layers)` (now ported) for every zone instead of `port_pending!`;
`footprint/compare.rs` `footprint_cmp_zones` (footprint package) can now use
`assigned_priority`, `layer_set`, `library_outline`.

## Package M9: PcbShape (`pcbnew/pcb_shape.cpp`, `pcb_shape.h`)

| C++ | Rust | State |
|---|---|---|
| library frame: `m_lib*` copies, `syncLibCoords`, `RebakeFromLib`, `RebakeWithScale`, `rebakeFromTransform`, `OnFootprintRescaled`, `GetLibraryArcMid` / `BezierC1` / `C2` / `PolyShape`, `SetLibStrokeWidth` | `pcb_shape/lib_frame.rs` | ported |
| `EDA_SHAPE` setter overrides (`SetStart`, `SetEnd`, `SetCenter`, `SetCornerRadius`, `SetArcGeometry`, `SetBezierC1/C2`, `SetPolyShape`, ellipse setters, `SetWidth`, `SetStroke`) | `lib_frame.rs` (`set_*`, taking `&MutEnv`) | ported |
| `Move`, `Rotate`, `Flip`, `Mirror` (both), `Scale`, `Normalize`, `NormalizeForCompare`, `OnFootprintTransformed` | `mod.rs` | ported |
| `SetLayer`, `GetLayerSet`, `SetLayerSet`, `IsOnLayer`, `HasSolderMask`, local mask margin, `GetSolderMaskExpansion`, `IsConnected`, net handling | `mod.rs`, `lib_frame.rs` | ported |
| `GetEffectiveShape`, `TransformShapeToPolygon`, `TransformShapeToPolySet`, `GetBoundingBox`, `HitTest` (both), `GetCorners`, `GetFocusPosition`, `GetConnectionPoints` | `mod.rs`, `shapes.rs`, `lib_frame.rs` | ported (bodies are M8) |
| `SetIsProxyItem`, `IsProxyItem`, `getHatchingKnockouts`, `UpdateHatching`, `GetItemDescription`, friendly name | `mod.rs`, `shapes.rs` | ported |
| `cmp_drawings` | `shapes.rs::pcb_shape_cmp_drawings` | ported; equal UUIDs tie-break on the item id (design 3.1, stands for the C++ pointer) |
| `operator==`, `Similarity` | `shapes.rs::eq_item`, `similarity` | ported (`m_forceVisible` is not in the model) |
| `GetCoverageArea`, `ViewGetLOD`, `ViewGetLayers`, `GetMsgPanelInfo`, `GetMenuImage`, `Serialize` / `Deserialize` (API), `swapData` | - | skipped: UI / IPC API / undo only |

Tests: `pcb_shape/tests.rs`, 26 unit tests (shape cases of
`qa/tests/pcbnew/test_footprint_transform_sync.cpp`: rescale / rotate / non-uniform scale of
ellipses, rectangles, circles, arcs, polygons, beziers; overrides: layers / mask / net, mask
expansion, effective shape, description, proxy, flip, cmp_drawings) and the reference-dump
replay `m9_shapes_match_oracle` (feature `dump`): `tests/golden/m9_shapes.txt`, produced by
`tests/oracle/m9_shapes_golden.sh` with `pcb-dump board --sections item,bbox,shape,footprint`
on `tests/oracle/m9_shapes.kicad_pcb` (board-level copper shapes with mask / margin / net;
eleven shape kinds in six footprints: rotated, flipped, non-uniform scaled + rotated, uniform
scaled + rotated, scaled, flipped + non-uniform scaled). 2384 values compared, 0 mismatches,
0 pending. `test_graphics_load_save` is a parser / writer round trip (PCBIO packages);
`test_graphic_trim` / `extend` are editor tools: not ported.

Notes:
- `GetEffectiveShape` on a mask layer passes the widened width to
  `MakeEffectiveShapesWithLineEndings`, but (as in the C++) only the line endings use it: the
  body shapes come from `GetEffectiveWidth()`. A plain segment keeps its width on the mask
  layer; the oracle's `shape.F.Mask` lines confirm this.
- Stubs left: none in `pcb_shape/`.

## Package M12: dimensions (PCB_DIMENSION_BASE and the PCB_DIM_* classes)

Supersedes the "stubs (M12)" note of the M0c row: no `port_pending!` is left in
`pcb_dimension.rs`.

| C++ | Rust | State |
|---|---|---|
| `PCB_DIMENSION_BASE`: `GetStart/End`, `SetStart/End` (lib frame inside a footprint), `GetValueText` (unit helpers, precisions incl. `V_V*`, zero suppression), `updateText` (prefix / value / units / suffix, override text), `drawAnArrow`, `GetUnitsMode` / `SetUnitsMode` / `UpdateUnits`, `GetBoundingBox`, `GetEffectiveShape`, `TransformShapeToPolygon`, `HitTest` (point, rect), `GetItemDescription`, `Move`, `Rotate`, `Flip`, `Mirror`, `OnFootprintRescaled/Transformed`, `StyleFromSettings`, `SetPosition` / `GetPosition`, the `PCB_TEXT` overrides (shims) | `pcb_dimension.rs` (`PcbDimension`) | ported |
| `PCB_DIM_ALIGNED`: `updateGeometry` (crossbar, extension lines, text knockout via `segPolyIntersection` / `segCircleIntersection` / `CollectKnockedOutSegments`), `updateText` (text position modes, keep-text-aligned), `UpdateHeight`, `GetAngle`, `Mirror` | `aligned_update_geometry`, `aligned_update_text`, `update_height`, `angle`, `mirror` | ported |
| `PCB_DIM_ORTHOGONAL`: `updateGeometry`, `updateText`, `Rotate` (quarter-turn orientation / height), `Mirror` | `orthogonal_update_*`, `rotate`, `mirror` | ported |
| `PCB_DIM_RADIAL`: `updateGeometry`, `updateText`, `GetKnee` | `radial_update_*`, `knee` | ported |
| `PCB_DIM_LEADER`: `updateGeometry` (text frame NONE / RECTANGLE / CIRCLE) | `leader_update_geometry` | ported |
| `PCB_DIM_CENTER`: `updateGeometry`, `updateText` | `center_update_*` | ported |
| `GetValueMode`, `ChangeValueMode`, `GetValueFieldText`, `ChangeValueFieldText` (`constraint_builder.cpp`: `DimensionValueMode`, `FindDimensionLengthConstraint`) | - | skipped: constraint solver package (value mode DRIVING needs the resolved `PCB_CONSTRAINT`s); no signature in the model |
| `HitTest( SHAPE_LINE_CHAIN, aContained )`, `GetMsgPanelInfo`, `ViewBBox`, `ViewGetLOD`, `Serialize` / `Deserialize`, the property registrations, `ChangeTextAngleDegrees` / `ChangeKeepTextAligned` (property setters) | - | skipped: GUI / API only, no signature in the model |

Tests (`pcb_dimension::tests`):
- `dimension_zero_suppression_preserves_integer_magnitude`
  (`qa/tests/pcbnew/test_dimension_zero_suppression.cpp`), `constructor_values`, `units_mode`.
- `board_item_move_rotate_flip_are_identities` (`qa/tests/pcbnew/test_board_item.cpp` `Move`,
  `Rotate`, `FlipLeftRight`, `FlipUpDown` for the five dimension classes, 81 position /
  reference pairs each) and `dimension_mirrors_across_axis` (the dimension part of
  `test_group_mirror.cpp` `GroupWithDimensionStillMirrors`).
- Oracle `oracle_dimensions_update` (default run) and `oracle_dimensions_dump`
  (`--features dump`), golden `tests/golden/m12_dimensions.txt` made by
  `tests/golden/m12_dimensions.py` with the reference `pcb-dump board --sections
  item,bbox,dim,text` on copies of `issue20249/dimension`, `issue24853`, `api_kitchen_sink`,
  `issue24192`, `demos/constraints` and `tests/golden/m12_dimensions.kicad_pcb` (31 dimensions:
  all five classes, both arrow directions, the three text position modes, all unit formats and
  units modes, zero suppression, `V_*` precisions, the three leader frames, mirrored bold text on
  B.SilkS, `${LAYER}` suffix, an aligned and a radial dimension in a footprint rotated 30 degrees
  and scaled 1.5 x 0.75). Every dimension is rebuilt from its dumped stored members and
  `update()`d; the default test compares 616 fields (value text, measured value, units mode,
  text, text position / angle, crossbar, `GetAngle`, `GetKnee`, every cached shape,
  `GetTextBox`, `GetBoundingBox`, `GetPosition`); the dump test compares all 2012 reference
  lines incl. `GetEffectiveShape`, `GetEffectiveTextShape`, text polygons and the
  `TransformShapeToPolygon` polygons (three clearance / error combos) - all bit-exact. Only
  `value_mode` stays pending (above).

Deviations:
- The `Update()` at the end of the context-free mutators (`Move`, `Rotate`, `Mirror`, `Flip`,
  `OnFootprintTransformed`, `StyleFromSettings`) runs against an empty board for board-level
  dimensions (exact unless the text holds variables needing the real board) and is skipped for
  footprint children (no footprint in `MutEnv` lookups); the caller refreshes those with
  `update(ctx)` (clone-edit-store, design 8.2). `StyleFromSettings` resolves `AUTOMATIC` units
  to millimetres (no `GetBoard()->GetUserUnits()` without a context).
- `Mirror` / `Flip`: `IsSideSpecific()` uses the layer-set test only (as `PCB_TEXT::Flip`, M10).

Known gaps:
- Not oracle-verified: `Move`, `Rotate`, `Flip`, `Mirror`, `HitTest` (no reference tool
  exercises them; covered by the ported identity tests and a review against the C++), and
  dimensions inside a *flipped* footprint (the oracle footprint is rotated and scaled, not
  mirrored). The footprint-cascade tests of `test_footprint_transform_sync.cpp`
  (`DimensionFollowsFootprintScale`, `DimensionAngleLibFrameFollowsFootprintRotate`,
  `DimensionLibCoordsSurviveFootprintRotate`, the double-flip case) wait for
  `FOOTPRINT::SetOrientation` / `Flip` / `SetTransformScale` (M19, still `port_pending!`).

## Package PCBIO-04: board file setup section (stackup, legacy design rules, defaults, plot params)

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `PCB_IO_KICAD_SEXPR_PARSER::parseSetup` (every token incl. the legacy pre-V6 design-rule tokens setting `m_LegacyDesignSettingsLoaded`, `user_trace_width` / `user_via` / `user_diff_pair` lists with the append rules, tenting / covering / plugging / capping / filling, aux / grid origin, the default stackup built when the file has none) | `src/io/parser/setup.rs` `parse_setup` | ported |
| `parseBoardStackup` (incl. the 128-item cap of issue 23625 and the duplicated-stackup stop of issue 24133, pre-20210824 colours without alpha, sub-layers, locked thickness, dielectric models, unknown tokens skipped) | `setup.rs` `parse_board_stackup` | ported |
| `parseDefaults`, `parseDefaultTextDims` | `setup.rs` | ported |
| `parseDrillSymbolProfile` | `setup.rs` + `setup_drill_profile.rs` | ported (data part of `DRILL_SYMBOL_PROFILE`, see deviations) |
| `PCB_PLOT_PARAMS::Parse`, `PCB_PLOT_PARAMS_PARSER` (`pcbplotparams_t` table, legacy layer-mask remap before 20240819, every deprecated token, clamping) | `src/io/parser/setup_plot_params.rs` | ported |
| `PCB_IO_KICAD_SEXPR::formatSetup`, `formatDrillSymbolProfile` | `src/io/writer/setup.rs` | ported |
| `BOARD_STACKUP::FormatBoardStackup`, `PCB_PLOT_PARAMS::Format` | `src/io/writer/setup.rs` (`format_board_stackup`, `format_plot_params`) | ported |
| `parseZoneDefaults`, `formatZoneLayerProperties` | PCBIO-08 (`zone.rs`), called from here | - |

### Tests and verification

`src/io/parser/setup_tests.rs` (11 tests):

* Ported C++ tests (`qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`):
  `Issue23625_CorruptedStackupCapped`, `Issue24133_DuplicatedStackupNotInflated` (data from
  `qa/data/pcbnew/plugins/kicad_sexpr/`). The QA tree has no dedicated plot-params test (only
  `test_downgrade_oracle.cpp`, downgrade writers out of scope); the reader is covered by
  `plot_params_reader` (expectations from the C++) and the oracles below.
* Byte-level oracle `oracle_resave_setup`: the 5 hand-made boards of `tests/data/pcbio04/`
  (legacy v5 with every legacy token and `viasonmask`, V6 stackup with alpha-less colours and
  sub-layers, current board with dielectric models / drill symbol profile / unknown tokens,
  2024 and legacy tenting forms) re-saved by the reference `kicad-cli pcb upgrade --force`
  (`tests/oracle/pcbio04_resave_golden.sh`, goldens `tests/golden/pcbio04/`): parse + format
  reproduces the `(setup ..)` section byte for byte, from the input and from the oracle
  output. `oracle_resave_setup_pcbio02_boards`: the same on the 8 PCBIO-02 boards.
* Corpus oracle `oracle_corpus_setup` (`tests/oracle/pcbio04_setup_golden.sh`, FNV-1a hashes in
  `tests/golden/pcbio04_setup_corpus.txt`): every board of `qa/data/pcbnew` + `demos` below
  2 MB: 342 boards, 1 rejected by the reference build, **341 compared, 341 identical** (207 of
  them compared on the board text cut after the setup section because a later item reaches a
  pending model stub). Any difference fails the test.
* Error texts (`setup_errors`), `m_LegacyDesignSettingsLoaded` (`legacy_design_settings_flag`),
  legacy lists and default netclass values, the default stackup on load, the drill symbol
  profile model form.
* Item level (`tests/oracle/pcbio04_dump_compare.sh`, not committed output): `pcb-dump board
  --no-init --sections board` vs `pcb-dump-rs board --sections board` on copies of 9 boards
  (versions 20171130, 20210108, 20240108 x2, 20240225, 20241229 x2, 20260206, 20261031; the 5
  PCBIO-04 boards, ecc83-pp_v2, pic_programmer, issue18142, complex_hierarchy): all 6434
  `ds.*` / `plot.*` / `stackup_or_default.*` reference lines match except the 63 `ds.meander.*`
  lines per board (`PNS::MEANDER_SETTINGS`, no Rust API, reported pending). The remaining
  mismatches of those dumps (bbox, item count, net order of pic_programmer) come from item
  parsers of other packages.

### Deviations

* `T_visible_elements`: the value is parsed and dropped; `BOARD::m_LegacyVisibleItems` is GUI
  state (only `files.cpp` migrates it into the project local settings) and has no Rust field.
* The drill symbol profile is kept in the model as a canonical s-expression
  (`BoardDesignSettings::drill_symbol_profile`, `None` for the default profile) until the
  tier-2 drill package ports `DRILL_SYMBOL_PROFILE`; `setup_drill_profile.rs` holds the data
  part and is compiled into both the reader and the writer module (`#[path]`,
  `clippy::duplicate_mod` allowed: the reader's module is private to `io::parser`).
* `parseBoardStackup` adds the item to the stackup before reading its parameters; the port adds
  it after reading (equivalent: an error aborts the load).
* `wxCHECK_RET( CurTok() == T_setup )` is a `debug_assert!` (the callers guarantee it).
* Out-of-range `dimension_units` / `dimension_precision` integers (undefined `static_cast` to an
  enum class, never written by KiCad) map to the last enumerator.

### Known gaps

* `parseZoneDefaults` (PCBIO-08) is called, not ported here.
* `src/io/writer/tests.rs` (`staging_stubs`, `oracle_resave`) still expects the setup section to
  be a staging stub and fails now that it is written; owned by PCBIO-02 (change requested).

## Package PCBIO-03: board file tracks, vias, arcs, graphic shapes, teardrop parameters

Files: `src/io/parser/tracks.rs`, `src/io/parser/shapes.rs`, `src/io/writer/tracks.rs`,
`src/io/writer/shapes.rs`, `tests/pcbio03_tracks_shapes.rs`, `tests/data/pcbio03/`,
`tests/golden/pcbio03/`, `tests/golden/pcbio03_corpus.txt`, `tests/oracle/pcbio03_golden.sh`.

### C++ to Rust map

| C++ (`pcb_io_kicad_sexpr_parser.cpp` P:, `pcb_io_kicad_sexpr.cpp` W:) | Rust | State |
|---|---|---|
| `parseTEARDROP_PARAMETERS` (P:707-772) | `parser/tracks.rs` `parse_teardrop_parameters` | ported |
| `parseLineEnding`, `parsePCB_SHAPE` (P:3886-4456, every `gr_*` / `fp_*` kind, legacy arc / width / fill forms, ellipses, library frame since 20260616) | `parser/shapes.rs` `parse_line_ending`, `parse_pcb_shape` | ported |
| `STROKE_PARAMS_PARSER` hookup (`SyncLineReaderWith` pair) | `parser/shapes.rs` `parse_stroke` (`DsnLexer::with_sub_lexer`) | ported |
| `parseARC` (incl. collinear arc loaded as a track), `parsePCB_TRACK`, `parsePCB_VIA` (every token and version branch), `parseViastack` (P:8951-9607) | `parser/tracks.rs` | ported |
| `parsePostMachining` (called from `parsePCB_VIA`) | `parser/pad.rs` | PCBIO-07 (staging stub) |
| `isDefaultTeardropParameters`, `formatTeardropParameters` (W:871-905) | `writer/tracks.rs` | ported |
| `format( const PCB_TRACK* )` incl. the `formatPostMachining` lambda (W:3241-3496) | `writer/tracks.rs` `format_track` | ported |
| `format( const PCB_SHAPE* )` (W:1183-1331), `LINE_ENDING::Format` | `writer/shapes.rs` `format_shape`, `format_line_ending` | ported |
| `STROKE_PARAMS::Format` | `kicad_core::stroke_params` | (kicad-core) |

### Tests and verification

* C++ tests ported (`tests/pcbio03_tracks_shapes.rs`): `CoincidentDrawingsSurviveSave`,
  `CollinearTrackArcLoadsAsTrack` (the 48 MB VCU118 variant is `#[ignore]`),
  `test_issue25650_collinear_arc`, `test_graphics_load_save`, `test_line_ending_load_save` (as a
  board round trip; the footprint-library path is PCBIO-06 / 14), `test_ellipse_roundtrip`
  (board cases).
* Byte level, hand-made boards (`tests/data/pcbio03/`: tracks / vias with every via token,
  padstack modes, back / tertiary drill, teardrops, tenting / covering / plugging / capping /
  filling, zone layer connections, two format versions; every shape kind, fill mode, stroke
  type, line ending, a 2021 file with legacy arcs, `width`, implicit fills, numeric nets):
  parse + format of the input and of the reference re-save (O1) reproduce O1's track and
  graphic blocks byte for byte (`oracle_resave_handmade`).
* Byte level, corpus (`oracle_corpus_blocks`, reference `kicad-cli pcb upgrade --force`
  re-saves of every corpus board via the io-corpus cache): 256 (board, kind) pairs identical
  (same blocks, same order) on 170 boards, 4 same set in a different track order (net codes
  follow footprint pads, PCBIO-06), 1 explained by the pending post-machining parser
  (`padstacks.kicad_pcb`), 272 pending on model stubs (mostly `EDA_SHAPE::Compare` in
  `board/compare.rs`, the drawing sort of the writer), 0 differ.
* Shape formatter alone (`io::writer::shapes::tests::oracle_shapes_corpus`, multiset of `gr_*`
  blocks, independent of the drawing sort): 271 boards identical + 5 identical with UUIDs
  masked (9157 blocks), 8 pending (`PCB_TEXTBOX` stub), 0 differ.
* Item level: `pcb-dump board --no-init --sections item,track,shape` vs `pcb-dump-rs` through
  `compare.py` on copies of 44 boards (versions 20200119 .. 20261031, the 4 hand-made ones):
  0 mismatches on track / via / arc / board-shape members; the only mismatches are
  `padstacks.kicad_pcb` front post-machining (PCBIO-07) and the group membership of
  `via_stacks.kicad_pcb` (its groups hold generators, pending). Missing lines are footprints
  and their children (PCBIO-06), zones and dimensions; pending lines are
  `ItemRef::fp_relative_position` / `netname` stubs in `item_ref.rs`. The dump's `shape`
  section now emits the `EDA_SHAPE` getters that exist (line style, centre, mid, radius, arc
  angles, clockwise, rect corners, segment angle, point count).

### Deviations / notes

* A value of `(uuid ..)` that is not a UUID gets a random UUID in C++ (`KIID( string )`); the
  port's generator is deterministic. The corpus tests compare such boards (5 ipc2581 boards)
  with the UUIDs masked.
* `wxCHECK_MSG( CurTok() == ... )` at the top of the item parsers is a `debug_assert!` plus the
  same early `None` return.

### Known gaps

* `parsePostMachining` (PCBIO-07): boards with `front_post_machining` / `back_post_machining`
  on vias lose them while it is a staging stub.
* The board-level drawing order needs `EDA_SHAPE::Compare` wired in `board/compare.rs`
  (`eda_shape_compare` is still a stub although `EdaShape::compare` exists).
* `src/io/parser/tests.rs` `staging_stubs` (PCBIO-01) still uses `(segment ..)` as its example
  of a staging stub and fails now that segments are parsed (change requested).

## Package PCBIO-05: board file text (effects, render cache, gr_text / fp_text, text boxes)

Files: `src/io/parser/text.rs`, `src/io/writer/text.rs`, `tests/pcbio05_text.rs`,
`tests/data/pcbio05/`, `tests/golden/pcbio05/`, `tests/oracle/pcbio05_golden.sh`,
`tests/oracle/pcbio05_blocks.py`. Integration edit in `src/io/parser/board.rs` (`finish_item`:
the `ResolveFont` stub replaced by a call to `resolve_child_font`, see below).

### C++ to Rust map

| C++ (`pcb_io_kicad_sexpr_parser.cpp`, `pcb_io_kicad_sexpr.cpp`, `eda_text.cpp`) | Rust | state |
|---|---|---|
| `parseEDA_TEXT` (font face / size / line_spacing / thickness / bold / italic, justify, bare and `(hide B)`, overbar conversion < 20210606, 60 mil default size, `MigrateLegacyBoldStrokeWidth` < 20260826) | `parser/text.rs` `parse_eda_text` | ported |
| `parseRenderCache` | `parse_render_cache` (`setup_render_cache` / `add_render_cache_glyph`) | ported |
| `parsePCB_TEXT` (`gr_text` / `fp_text reference / value / user`, bare `locked`, `%V` / `%R`, hidden footprint text -> `PCB_FIELD( USER )` with `GetUserFieldName`, hidden board text forced visible) | `parse_pcb_text` (board / dimension), `parse_pcb_text_item` (footprint: `ParsedText::Field`) | ported (footprint side unexercised until PCBIO-06) |
| `parsePCB_TEXT_effects` (`at` with legacy `unlocked`, `layer` + `knockout`, `hide`, `(locked B)`, `(unlocked B)`, effects, render cache, custom properties, legacy FP transform < `FIRST_FP_AFFINE_TRANSFORM`, `SetLibTextSize/Thickness`) | `parse_pcb_text_effects` | ported |
| `parsePCB_TEXTBOX`, `parseTextBoxContent` (table-cell `span` included), `bakeTextBoxLib` | `parse_pcb_textbox`, `parse_text_box_content_with`, `bake_text_box_lib` | ported; blocked at run time by the `PCB_TEXTBOX` model stubs (M11) |
| `finishItem`: `dynamic_cast<EDA_TEXT*>( child )->ResolveFont( embeddedFonts )` | `resolve_child_font` (called from `board.rs` `finish_item`) | ported |
| `format( const PCB_TEXT* )` (incl. the headerless `PCB_FIELD` body and dimension text as `gr_text`) | `writer/text.rs` `format_text`, `format_field`, `format_pcb_text_with` | ported |
| `format( const PCB_TEXTBOX* )` (incl. `table_cell`) | `format_textbox`, `format_textbox_with` | ported |
| `formatRenderCache` | `format_render_cache`, `format_render_cache_of` | ported (glyphs see deviations) |
| `EDA_TEXT::Format` | `format_eda_text` | ported |

### Tests and verification

- `tests/pcbio05_text.rs` `golden_text_blocks` (byte level, default run): 10 boards in
  `tests/data/pcbio05` (hand-made: KiCad 5 legacy texts without size / with bare `hide` /
  legacy `bold` / old overbars / `%R %V` / numeric text; 20221018 texts with knockout, bare
  `locked`, auto thickness, line spacing, an outline font with a loaded render cache; 20250324
  texts with `(locked yes)`, `(hide yes)`, custom properties, stroke and outline faces; two
  text-box boards; and texts-only extracts of the five corpus boards with outline fonts and
  render caches: RoyalBlue54L-Feather, tinytapeout-demo, jetson-agx-thor-baseboard, issue14549,
  issue22475) re-saved by `kicad-cli pcb upgrade --force` (`pcbio05_golden.sh`). Every
  `gr_text` block of the 8 text boards (448 blocks, 156 with outline fonts) is byte for byte
  identical to the oracle's, render-cache header included (glyph polygons removed from the
  goldens, see deviations). The 2 text-box boards stop at `PCB_SHAPE::SetStart (PCB_TEXTBOX)`
  (M11) and are reported, not compared.
- `corpus_text_blocks` (ignored, needs the io-corpus oracle cache): rust re-save of `O1` vs
  `O2` for every corpus board with board-level text: 77 boards / 167 blocks compared, all
  identical; 75 boards stop at `EDA_SHAPE::Compare` (writer sort, model pending) and 8 at the
  text-box model stub.
- Item level: `tools/io-corpus/run.sh --dump <data board> --sections item,text` on the 10 data
  boards: 0 mismatches in `item` and in every text attribute / shown text / stroke-font
  geometry field; the only mismatches are the glyph geometry fields (`text_shape`,
  `effective_shape`, `text_box`, `interline`, `text_polys`, knockout `tpoly`) of the 156
  outline-font texts (outline font engine, `kicad-font` D1). Text boxes: missing on the Rust
  side (M11 stub).
- Ports: `FootprintUserFieldKeepsAutoThickness` (parse half, `parser/text.rs`
  `footprint_user_field_keeps_auto_thickness`; the save / reload round trip needs PCBIO-06),
  `test_pcb_textbox.cpp` `NonCardinalRotationKeepsLibShapeRectangle` as an I/O test
  (`non_cardinal_rotation_keeps_lib_shape_rectangle`: rotated box still written with
  `start` / `end`; skipped while M11 is pending). The other `test_pcb_textbox` cases are model
  / plot tests (M11, plotting). Unit tests: `board_text_drops_render_cache`,
  `footprint_text_drops_render_cache`.

### Deviations / notes

- `formatRenderCache` regenerates the glyphs with `OUTLINE_FONT::GetLinesAsGlyphs` whenever
  the stored cache does not match; every loaded cache is cleared during the load (`SetVisible`,
  `SetTextSize`, `SetTextAngle`), so the C++ always regenerates from the host fonts. The port
  has no outline font engine: it writes the `(render_cache "<shown text>" <angle>)` header
  with no `(polygon ..)` lists. The goldens drop the oracle's polygons for that reason.
- `href` is written by `EDA_TEXT::Format` for text boxes but not parsed by `parseEDA_TEXT`
  (the reference build refuses such a file): same in the port.

### Known gaps

- Outline-font glyphs (render cache polygons, text geometry): `kicad-font` outline engine.
- Text boxes are untested end to end until the `PCB_TEXTBOX` setters (M11) land: rerun
  `golden_text_blocks` (the two `textboxes*` boards then compare) and
  `non_cardinal_rotation_keeps_lib_shape_rectangle`.
- Footprint text / fields (`fp_text`, `property`) are exercised by PCBIO-06.

## Package SET-5: project file, project binding (BOARD::SetProject), the two load sequences

Files: `src/project/{project,project_file,project_local_settings,board_project,board_loader}.rs`
(mod lines in `src/project/mod.rs`), `tests/set5_project.rs`,
`tests/oracle/set5_dump_compare.sh` + `set5_board_compare.py`; minimal edits in
`src/dump/mod.rs` (`DumpLoadMode`, `load_board_for_dump_mode`) and `src/bin/pcb_dump_rs.rs`
(`--loader`); additive fields in `src/board/mod.rs` (`BoardProject.project_name`,
`visible_layers`, `full_name`, `board_drawing_sheet_file`, `document`, `file_synced`;
`Board::text_var_clock`, host-settable clock for `CURRENT_DATE` / `CURRENT_TIME_*`).

### C++ to Rust map

| C++ | Rust | state |
|---|---|---|
| `PROJECT_FILE` ctor (ported params: `text_variables`, `pcbnew.page_layout_descr_file`), migrations 1->2, 2->3 (layer presets: version only), 3->4 (`schematic.bus_aliases`), `LoadFromFile` (+ nested `NET_SETTINGS`) | `project_file.rs` `ProjectFile::load_from_str` | ported (load side) |
| `PROJECT_LOCAL_SETTINGS` (`board.visible_layers`, `board.prototype_zone_fills`) | `project_local_settings.rs` | partial (only what the board reads) |
| `PROJECT::setProjectFullName`, `GetProjectFullName/Name/Path/Directory`, `IsNullProject`, `GetTextVars`, `ApplyTextVars`, `AbsolutePath`, `TextVarResolver`; `SETTINGS_MANAGER::LoadProject` (in-memory) | `project.rs` `Project`, `load_project(pro, prl, name)`, `BoardProject::text_var_resolver` | ported (git hash: ""; env / cwd expansion in `AbsolutePath` not ported) |
| `BOARD::SetProject` (legacy design settings guard, legacy `(net_class)` transfer, net settings ownership), `ClearProject`, `GetDesignRulesPath`, `SynchronizeComponentClasses`, `UpdateUserUnits( board, nullptr )` | `board_project.rs` | ported (`aReferenceOnly` not ported) |
| `BOARD_LOADER::initializeLoadedBoard`, `KI_TEST::LoadBoard` (post-parse order) | `board_loader.rs` `LoadPath::{Cli,Qa}.steps()`, `run_model_step`, `initialize_loaded_board` | ported; session steps (`InitDrcEngine`, `ResolveDrcExclusions`, `BuildConnectivity`) are returned as `StepOutcome::Session` for PCBIO-15 |

Step order (documented in `board_loader.rs`): CLI = BindProject, InitDrcEngine*,
ResolveDrcExclusions*, BuildConnectivity*, BuildListOfNets, SynchronizeNetsAndNetClasses(true),
SynchronizeComponentClasses (warning on failure), SynchronizeTuningProfileProperties,
UpdateUserUnits. QA = BindProject (only with a `.kicad_pro` / `.pro`), InitDrcEngine*,
BuildListOfNets, BuildConnectivity*, SynchronizeTuningProfileProperties,
SynchronizeComponentClasses (only with a project). `*` = session step.

### Tests

- Unit tests in the five files (24): names / paths / `AbsolutePath`, `ApplyTextVars`, text
  variables (partial load on a bad value), root migrations and versions, nested net settings,
  `.kicad_prl` hex parsing corner cases, `SetProject` (design + net settings, legacy design
  settings guard, legacy net classes replace the project's, rebinding), `ClearProject`, step
  order, QA without project binds nothing, CLI binds the empty project.
- `tests/set5_project.rs`: `test_project_file.cpp` `LoadProjectByAbsolutePathIsStable` (naming
  part) and `AliasMigrationPreservesNonemptyDefinitions` (load side); `test_board_loader.cpp`
  `BoardLoaderWithInit` (model side, `reference_images_load_save`) and `BoardLoaderWithoutInit`;
  corpus smoke test of every `.kicad_pro` in `qa/data` and `demos` (`settings.md` 9.3 (7)).
  Not applicable: the sheet / manager / save cases of `test_project_file.cpp` (schematic,
  SETTINGS_MANAGER, writer SET-13), `CollinearTrackArcLoadsAsTrack` (parser, 48 MB board).
- Oracle: `tests/oracle/set5_dump_compare.sh` (reference `pcb-dump board` = QA path and
  `--loader` = kicad-cli path vs `pcb-dump-rs` on the same path) over every corpus board with a
  sibling `.kicad_pro` (<= 6 MB, large demos excluded). Compared per board: `has_project`, all
  `ds.*` (except `ds.meander.*`), `title_block.*`, `composite_netclass.*`, and per net *name*
  the stored and the effective net class. Result: see "Oracle result" below.

### Oracle result

Corpus (256 boards with a `.kicad_pro`; 251 per path after the size / large-demo filter),
identical numbers on both paths (QA and `--loader`), run of 2026-11-10 before another
package's in-progress zone change made the Rust parser panic on most boards (a rerun the same
day: 148 match, 2 mismatch (drc exclusions, below), 101 not loadable by the Rust parser):

- 234 boards match completely (120 179 compared values per path; 1 142 pending: nets whose
  items the Rust parser still skips; `ds.biggest_clearance` is a session value - it needs the
  DRC engine's `QueryWorstConstraint` - and is not compared).
- 5 boards not loadable by the Rust parser today (`port_pending!` items outside
  `allow_pending`, e.g. `PCB_SHAPE::SetStart (PCB_TEXTBOX)`).
- 12 mismatching boards, none in settings code:
  - 5 (`issue14008`, `issue17429`, `skew_group_matched_drc`, `skew_within_diff_pairs_drc`,
    `solder_mask_bridge_test`): `ds.drc_exclusions` count - `RawDrcExclusionCodec` drops legacy
    marker strings (SET-4 deviation SET-4-1; needs the `kicad-drc` codec).
  - 4 legacy `(net_class ...)` boards (`issue18839`, `line_ending_drc_{fail,pass}`,
    `line_ending_zone_flood`): `parseNETCLASS` is a PCBIO-11 staging stub, so the board's
    classes are empty when `SetProject` transfers them (Default class, `smallest_clearance`,
    and on the CLI path `custom_track_width` follow).
  - 2 net-chain boards (`net_chain_bridging_tuning_profiles`,
    `test_drc_net_chain_tuning_profiles`): `BOARD::ApplyNetChainNetclasses` reaches the M4 stub
    `NET_SETTINGS::ClearChainPatternAssignments / SetChainPatternAssignment`, so the
    `BuildListOfNets` / `SynchronizeNetsAndNetClasses` steps stop there (chain composites
    missing, CLI `custom_via_size` not reset).
  - `issue23851`: composite classes `PowerLow,Default` / `Power,Default` missing because the
    nets that create them (pad nets) are not loaded yet.

Synthetic cases (second part of the script): the PCBIO-04 boards `legacy_v5` (pre-V6 setup),
`stackup_v6`, `current_misc` with four nets, each without a project and with
`tests/data/set5/design.kicad_pro` (rules, classes, patterns, a two-class label assignment,
text variables): 12 / 12 runs match (464-623 values each), incl. QA without a project
(`has_project=false`, no class sync) vs CLI with an empty one, and the legacy guard
(`board.design_settings` ignored for the pre-V6 board).

### Deviations

- The project is moved into the board (`Board::set_project` takes it, `clear_project` gives it
  back); C++ shares it by pointer, so its `m_project == aProject` early return has no
  counterpart (a bound project is released first).
- After `ClearProject` C++ leaves `bds.m_NetSettings` null; the board keeps a default
  `NetSettings`.
- If `BOARD_DESIGN_SETTINGS::LoadFromFile` throws inside `SetProject`, the project stays bound
  and the error is returned (as the C++ exception escapes); the project's net settings are
  dropped (C++ keeps them in the settings manager).
- A `.kicad_pro` that does not parse counts as no file (C++ retries it as a legacy INI, which
  reads nothing). The legacy `.pro` migration is not ported: the QA path binds an empty project
  for a lone `.pro`.
- `CURRENT_DATE` / `CURRENT_TIME_*` resolve from `Board::text_var_clock` (host-set, empty by
  default); `VCSHASH` / `VCSSHORTHASH` resolve to "".

### Known gaps

- Not loaded from the project: `board.layer_presets` / `viewports` / `layer_pairs`,
  `board.ipc2581.*`, `pcbnew.last_paths.*`, `COMPONENT_CLASS_SETTINGS` (SET-10),
  `TUNING_PROFILES` (SET-11), `.kicad_prl` `visible_items` (SET-9); no writer (SET-13).
- `SynchronizeComponentClasses` reaches the `port_pending!`
  `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments` (SET-10) on every bound board;
  `pcb-dump-rs` reports it as a pending step and continues.
- Text variables are verified by unit tests only (the dump has no text-variable field).

## Package PCBIO-10: board file dimensions (parse + write)

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `PCB_IO_KICAD_SEXPR_PARSER::parseDIMENSION` (P:5766-6231): bare `locked`, the skipped legacy value, the legacy `(width ..)` aligned form with `feature1` / `feature2` / `crossbar` (`UpdateHeight`, backwards) / `arrow1a..2b`, units read from the legacy text (`FetchUnitsFromString`, auto units otherwise), `(type ..)` for all five classes, `layer`, `tstamp` / `uuid`, `gr_text` (the `isStyleKnown` save / restore of keep-aligned and text position), `pts`, `height`, `leader_length`, `orientation`, `(format ..)` (prefix, suffix, units, units_format, precision, override_value, suppress_zeroes, with the C++ clamps), `(style ..)` (thickness, arrow_direction via `ChangeArrowDirection`, arrow_length, text_position_mode, extension_height, extension_offset, keep_text_aligned, text_frame), `(locked yes)`, `custom_property`, the closing `Update()`, all error texts | `src/io/parser/dimension.rs` `parse_dimension` (+ `parse_dimension_format`, `parse_dimension_style`) | ported |
| `PCB_IO_KICAD_SEXPR::format( const PCB_DIMENSION_BASE* )` (W:1067-1180) | `src/io/writer/dimension.rs` `format_dimension` (text through PCBIO-05's `format_pcb_text_with(.., is_dimension = true)`) | ported |

### Tests and verification

`tests/pcbio10_dimensions.rs`:

* `golden_dimension_blocks` (byte level, default run): the 36 boards of `tests/data/pcbio10/`
  (hand-made `dims_v5` (legacy 20171130 form, units from mm / in / mils / µm / unit-less
  texts, missing crossbar), `dims_v6` (6.0 order with the text before the style, all classes,
  prefix / suffix / suppress_zeroes, inward arrows, automatic units, leader frames),
  `dims_v7` (bare `locked`, radial, `V_*` precisions, mirrored text, zero-length dimension),
  `dims_current` (current format, `(locked yes)`, out-of-range units mode 4 and orientation 7,
  override value with a text variable, knockout text, custom property, center) and the
  dimensions-only extracts of all 32 corpus boards under 10 MB with board-level dimensions,
  taken from the ORIGINAL files) re-saved by the reference `kicad-cli pcb upgrade --force`
  (`tests/oracle/pcbio10_golden.sh` + `pcbio10_blocks.py`, goldens `tests/golden/pcbio10/`):
  parse + format reproduces **170 / 170 dimension blocks byte for byte** (fresh UUIDs of
  dimensions without one masked, `kicad_qa::uuid_canon`).
* `corpus_dimension_blocks` (ignored, needs the io-corpus oracle cache): `O2` vs the port's
  re-save of `O1`: 5 boards / 24 dimension blocks compared, all identical; 27 boards stop at
  the pending `ZONE::SetLayerSetAndRemoveUnusedFills` stub (their board-level dimensions are
  covered by the extracts above).
* Item level: `pcb-dump board --no-init --sections item,dim,text` vs `pcb-dump-rs` on copies of
  the 36 boards: 13968 lines match, **0 mismatches**, 170 pending (`dim|value_mode`, no Rust
  API, M12).
* Ported C++ test: `MalformedDimensionTextThrowsCleanly` (`test_kicad_sexpr.cpp`) as
  `malformed_dimension_text_throws_cleanly`; `dimension_parse_errors` checks the
  `Expecting(..)` texts. `DimensionZeroSuppressionPreservesIntegerMagnitude` was ported by M12.

### Deviations

* `PCB_DIMENSION_BASE::ClearRenderCache()` runs `Update()`, and every `EDA_TEXT` setter that
  `parsePCB_TEXT` calls on the dimension reaches it (the last is `SetVisible( true )`). The
  model's text setters act on the embedded `PcbText` and do not dispatch to the dimension, so
  the parser runs that last `Update()` itself right after the text is parsed. The
  intermediate `Update()`s are not reproduced; they only rewrite derived state (text, text
  position / angle in the non-manual modes, shapes) that the later setters and the final
  `Update()` overwrite. Observable effect reproduced: a legacy dimension reads its units from
  the regenerated `"<value> in"` text, so legacy dimensions always load in inches (as the
  reference does), whatever units their file text names.
* Values the C++ stores by `static_cast` outside the enumerators cannot be held by the Rust
  enums: `units_format 3` is read as 0 (renders identically, re-saved as 0), `precision`
  outside 0..9 is clamped into 0..9, `text_position_mode 3` is read as 2 (manual). `units 4`
  is exact (ignored, as `SetUnitsMode`'s switch does).
* An unknown `(type ..)` (null dereference in the C++) fails with "Cannot parse unknown
  dimension type ..."; the `wxCHECK_MSG` returns (`extension_height` on a non-aligned,
  `text_frame` on a non-leader dimension) return `Ok(None)` as the C++ returns `nullptr`
  (the board parser continues from the same token position). A `crossbar` on a non-aligned
  dimension is a no-op (C++: `static_cast` to the wrong class).

### Known gaps

* Footprint dimensions (`parseDIMENSION( footprint )`, P:6895) are parsed by this code but the
  footprint parser (PCBIO-06) is still a staging stub: not oracle-verified yet; the corpus
  test will cover them once it lands.

## Package PCBIO-08: board file zones (parse + write)

| C++ | Rust | State |
|---|---|---|
| `parseZONE` (P:9610-10480): every token and version branch (net by code / name with the `ZONE::SetNet` / `SetNetCode` thieving overrides, `net_name` repair, layer / layers, property, tstamp / uuid, hatch, priority, connect_pads, min_thickness, `filled_areas_thickness`, fill with every mode and option incl. thieving `>= 20260513`, placement incl. link, keepout, polygon with arcs, filled_polygon per layer with island flags, legacy `fill_segments`, name, attr teardrop, locked, custom_property), the post-parse fix-ups (non-copper / rule-area net 0, border hatch, stroked fill `< 20250210` inflated with `InflateWithLinkedHoles`, `fill_segments` -> `TransformOvalToPolygon` + `BooleanAdd`, legacy teardrops `< 20230517`, `m_linkedZones`) | `io/parser/zone.rs` | ported |
| `parseZoneDefaults`, `parseZoneLayerProperty` (P:3450-3501) | `io/parser/zone.rs` | ported |
| `parseBOARD` "Clear unused zone data" (`SetLayerSetAndRemoveUnusedFills( GetLayerSet() & enabled )`, P:1735-1744) | `io/parser/board.rs` (call site only) | ported |
| `format( const ZONE* )`, `format( const ZONE_LAYER_PROPERTIES&, int, PCB_LAYER_ID )` (W:3499-3790) | `io/writer/zone.rs` | ported |

### Tests

`tests/pcbio08_zones.rs`: ports of `test_kicad_sexpr.cpp` `Issue19775_ZoneLayerWildcards`,
`Issue23125_EmptyZoneDiscarded`, `CopperThievingZone_RoundTrip`,
`CopperThievingZone_AllPatternsRoundTrip`, `CopperThievingZone_RejectedInOldFileVersion`,
`CopperThievingZone_RejectsMalformedGeometry`; differential oracle `oracle_resave_handmade`: the
hand-made boards `tests/data/pcbio08/zones_modern.kicad_pcb` (every modern token: arcs in
outlines, hatch / thieving fills, smoothing, island area, keepout + placement rule areas with a
link and anchor, teardrop area, per-layer properties, zone defaults, a non-copper zone, a zone
with 2 corners) and `zones_legacy.kicad_pcb` (2020 format: legacy net codes, `net_name` repair
creating a net, stroked fill inflation, `(mode segment)` + `fill_segments`, a non-copper zone
with a net, old keepout) reproduce the zone blocks and zone defaults of the reference
`kicad-cli pcb upgrade --force` byte for byte (golden `tests/golden/pcbio08/`, the modern board
is identical as a whole file); `oracle_corpus_zones`: the zone blocks of the reference re-save of
every corpus board with zones (`tests/golden/pcbio08_corpus.txt`, both written by
`tests/oracle/pcbio08_golden.sh`), replayed from the original files (boards up to 4 MB).

Verification at landing (other packages pending: footprints skipped by staging): item level,
`pcb-dump board --no-init --sections zone` vs `pcb-dump-rs` on 182 corpus boards with zones:
100999 matches, 0 mismatches (missing / extra lines only for footprint zones and for ordinal keys
shifted by the skipped footprints; 3 boards stopped by a text-box model stub); `--sections item`
zone keys: 43845 matches, 67 mismatches, all `net` codes of boards whose footprints (and so the
net creation order) are skipped. Byte level: corpus replay 77 boards identical, 1 identical up to
order (teardrop areas sort by their random UUIDs), 1 identical up to a random UUID (a zone
without UUID in the file), 0 differ, 85 stopped in other packages' code (mostly the writer's
`EDA_SHAPE::Compare` stub); O1 -> Rust vs O2: 79 boards, 76 identical, 3 identical up to order.

### Deviations / notes

- Zones without a UUID in the file (teardrop areas never write one) get a random UUID, which is
  also a writer sort key (`docs/analysis/pcb-io.md` R1): their order among equal-layer zones and
  the written UUID of a non-teardrop zone differ from the oracle run.
- `parseNet` for a zone is a zone-local copy of `parse_net_with` so that both the legacy
  net-code path (`ZONE::SetNetCode`: thieving -> 0) and the name path (`ZONE::SetNet`: thieving
  -> no net, -1) apply the overrides.
- `SetHatchSmoothingLevel( parseDouble() )` and the writer's `formatInternalUnits(
  GetMinIslandArea() / IU_PER_MM )` convert double to int with `f64_to_i32_x86`.
- The writer returns an `IoError` "map::at" where the C++ `GetFilledPolysList` would throw for a
  zone layer without a fill entry (unreachable for parsed zones).
- The C++ "unknown zone corner smoothing type" writer error is unreachable (closed enum).

### Known gaps

- Footprint zones (`parseFOOTPRINT`, `< 20260616` board-frame outline un-baking) are PCBIO-06;
  only board-level zones are verified so far.
- The dump (`src/dump/items.rs`) still reports `ZONE::GetNumCorners` / `ZONE::IsTeardropArea` as
  pending although the model has them (change request to the dump owner).

## Package PCBIO-11: board file legacy net classes, net chains, variants, embedded files

Files: `src/io/parser/netclass_embedded.rs`, `src/io/writer/netclass_embedded.rs`,
`tests/data/pcbio11/`, `tests/golden/pcbio11/`, `tests/golden/pcbio11_corpus_netclass.txt`,
`tests/golden/pcbio11_corpus_sections.txt`, `tests/oracle/pcbio11_golden.sh`.

### C++ to Rust map

| C++ | Rust | state |
|---|---|---|
| `parseVariants` (P:538-591) | `parser/netclass_embedded.rs` `parse_variants` | ported |
| `parseNETCLASS` (P:3793-3883: name / description, the eight size tokens, `add_net` with the pre-20210606 overbar conversion into `SetNetclassPatternAssignment`, duplicate-name `IO_ERROR`, default class replacement) | `parse_netclass` | ported |
| `parseNET_CHAINS_SECTION` (P:3647-3791: names, `(net <name>)`, legacy `(net <code>)` through `m_netCodes`, `(member (net ..))` wrapper, two terminal pads, `FindPadByUuid`) | `parse_net_chains_section`, `parse_net_chain_members` | ported |
| `T_embedded_fonts` / `T_embedded_files` of `parseBOARD_unchecked` (P:1547-1591, append rule, `PARSE_ERROR` -> warning + skip to the closing `)`) | `parse_board_embedded_fonts`, `parse_board_embedded_files`, `parse_embedded_files_section` | ported |
| `T_embedded_fonts` / `T_embedded_files` of `parseFOOTPRINT_unchecked` (P:7142-7175) | `parse_footprint_embedded_fonts`, `parse_footprint_embedded_files` (take the footprint's `EmbeddedFiles`) | ported, called once PCBIO-06 lands |
| net-chain aggregation of `format( const BOARD* )` (W:962-1038: `std::map` keyed by `ValueStringCompare`, synthetic names, persistence rule) | `writer/netclass_embedded.rs` `format_net_chains` | ported |
| `formatVariants`, embedded files consolidation (W:828-850, 1040-1063) | `writer/board.rs` (PCBIO-02) | already ported |
| legacy class transfer to the project (`m_LegacyNetclassesLoaded`) | `project/board_project.rs` (SET-5) | already ported |

### Tests (all in `src/io/parser/netclass_embedded.rs`)

- `oracle_resave` (byte level): four hand-made boards (variants incl. case-insensitive duplicate
  / nameless / `Default`; net chains by name, legacy code, `(member ..)`, unknown tokens, chain
  names the `ValueStringCompare` map merges: `bus`/`BUS`, `1K5`/`1.5K`, numeric `42`; embedded
  files with fonts flag; a bad checksum mid-section; a pre-V6 board with legacy net classes)
  parsed and re-saved by the port: the **whole file** is byte-identical to the reference
  `kicad-cli pcb upgrade --force` output (4/4), and the warning text equals the reference one.
- `oracle_dump_netclass` (item level, feature `dump`): `pcb-dump board --no-init --sections
  board`, lines `ds.netclass.*`, `netclass_pattern.*`, `net.<code>.*` (class, chain),
  `effective_netclass.*`, of the 4 hand-made boards and the 7 corpus boards with legacy
  `(net_class ..)` (issue4139, issue4257, issue18839, line_ending_* x3, cli ZoneFill-4.0.7):
  all match (over 1000 lines; ZoneFill-4.0.7 has 26 `add_net` over several classes).
- `oracle_corpus_sections` (byte level): the `(variants ..)`, `(net_chains ..)`,
  `(embedded_fonts ..)`, `(embedded_files ..)` blocks of the reference re-save of the 18 corpus
  boards (< 20 MB, vme-wren excluded) with one of these sections: 31 blocks identical, 5 pending (a model stub
  `PCB_BARCODE::SetPosition` stops the parse of api_kitchen_sink; issue25265 / issue25303 carry
  footprint-level embedded files, consolidated on save, PCBIO-06 pending). Every parsed embedded
  file (15) validates its checksum, no parse warnings.
- `legacy_netclass_errors`: the reference duplicate-class `IO_ERROR` text and the `Expecting`
  text; `variants_registry`, `net_chains_resolution`: token loops.
- Ports: `test_net_chain_terminal_pads.cpp` `SingleNetSignalNamePersists` and
  `SignalsSurviveBuildListOfNets` (`net_chain_names_round_trip`), `test_variant.cpp`
  `VariantTestProjectLoad` (board registry part: `variant_test_project_load`).

### Deviations / notes

- `parseNET_CHAINS_SECTION` `(members ..)` loop: the C++ never terminates at the end of a
  truncated file (`skipCurrent` returns at EOF, `NextTok` keeps returning EOF); the port fails
  with "Expecting ')'" instead of hanging.
- `FindNet( 0 )` of a board without nets returns the static orphaned item, which the C++ would
  modify; the port leaves it alone (it is not part of the board).
- `KIID padUuids[2]` draws two fresh ids per chain from `board.kiid_generator`, as the C++
  default constructor does.

### Known gaps

- Terminal pads resolve only once footprints / pads are parsed (PCBIO-06); until then the UUID
  is kept and the pad pointer stays empty (as for a missing pad in C++), so
  `SignalTerminalPadsRoundTrip` is not ported yet.
- Footprint-level embedded files: PCBIO-06 must call `parse_footprint_embedded_fonts` /
  `parse_footprint_embedded_files` with the footprint's `embedded_files`; the corpus test then
  checks issue25265 / issue25303 automatically.

## Package M11: text boxes and tables (PCB_TEXTBOX, PCB_TABLE, PCB_TABLECELL)

### C++ to Rust map

| C++ | Rust | state |
|---|---|---|
| `PCB_TEXTBOX` overrides (`GetTopLeft/BotRight`, `GetCorners`, `GetCornersInSequence`, `SetTop/Left/Right/Bottom`, `SetShape`, `GetMinSize`, `GetLegacyTextMargin`, `GetDrawPos( aIsFlipped )`, `GetTextAngle/Size/Thickness` and setters, `GetShownText` (resolver + `LinebreakText`), `Move`, `Rotate`, `Mirror` (both), `Flip`, `OnFootprintTransformed/Rescaled`, `syncLibCoords`, `GetBoundingBox`, `HitTest` (point, rect), `GetItemDescription`, `GetEffectiveShape`, `GetPolygonalBoxShape`, `TransformTextToPolySet`, `TransformShapeToPolygon`, `StyleFromSettings`, `Is/SetBorderEnabled`, `Get/SetBorderWidth`) | `pcb_textbox.rs` (`PcbTextbox` methods; read-only bodies as generic functions over `TextboxHost`, shared with cells) | ported |
| `PCB_SHAPE` bodies inherited by text boxes that reach a textbox override (`SetStart/End/Center/CornerRadius/ArcGeometry/ArcAngleAndEnd/BezierC1/C2/Ellipse*/PolyShape` with the textbox `syncLibCoords`, `Normalize`, `GetEffectiveShape`, `GetSolderMaskExpansion` with the box as `this`, `TransformShapeToPolySet`, `getHatchingKnockouts`) | `pcb_textbox.rs` | ported |
| `PCB_TABLECELL` (`GetRow/GetColumn/GetAddr`, `GetUnwrappedShownText`, `GetShownText`, `GetRowHeight/GetColumnWidth`, `GetItemDescription`, the textbox bodies over the cell, setter forwards) | `pcb_tablecell.rs` | ported |
| `PCB_TABLE` read side (`GetPosition`, `GetEnd`, `GetBoundingBox`, `DrawBorders` as a callback, `GetEffectiveShape`, `HitTest`, `RunOnChildren`, `TransformShapeToPolygon`, `TransformShapeToPolySet` / `TransformGraphicItemsToPolySet` without render settings, `GetItemDescription`, `Compare`, `GetBorderStyle/GetSeparatorsStyle`, `setAutosizedRowHeights`, the sizing part of `Autosize`) | `pcb_table.rs` (`PcbTable`) | ported |
| `PCB_TABLE::Normalize`, `Autosize`, `Rotate`, `Flip` (`mirrorCellColumns`), `Mirror`, `Move`, `SetLayer` (cells are arena items) | `pcb_table.rs` `impl Board`: `table_normalize`, `table_autosize`, `table_rotate`, `table_flip`, `table_mirror`; `Board::rotate_item / flip_item / mirror_item` (board_item.rs) dispatch tables there | ported |
| `GENERATED_TABLE_SCHEMA` (`Find`, `FindToken`, `Token`, `DefaultColumn`, `Defaults`, `Validate`), `DrillChartSchema()`, `StackupTableSchema()`, `DRILL_CHART_COLUMN_ID`, `STACKUP_TABLE_COLUMN_ID`, units / align tokens, `GENERATED_TABLE_MAX_*`, `PCB_DRILL_CHART` / `PCB_STACKUP_TABLE` constructors, `AllColumns`, `RestoreColumns`, `SetPrecision` | `pcb_generated_table.rs` | ported |
| `PCB_GENERATED_TABLE::setAutosizedRowHeights` override | `PcbTable::set_autosized_row_heights` | stub (M26: needs `IsDataRow` / `metricRowHeight` of the refresh) |
| `FormatGeneratedTableLength`, `DocumentationLayers`, templates I/O, refresh | - | skipped here: tier 2 (M26) |
| `PCB_TABLECELL::SetColumnWidth / SetRowHeight`, `FontChoices`, `GetMsgPanelInfo`, `Similarity`, `operator==`, `Serialize` (API) | - | not ported (property editor / UI / API) |

### Tests

* Ported from `qa/tests/pcbnew/test_pcb_textbox.cpp`: `GetMinSize*` (7 cases), `ThirtyDegreeIncrementsToCardinalKeepsVisualRotated`, `NonCardinalRotationDoesNotCrash`, `CardinalRotationsRoundTrip`, `NonCardinalRotationKeepsLibShapeRectangle` (`EmptyKnockoutPlotsTheBox` is plotting: plot packages). From `test_footprint_transform_sync.cpp`: `TableRotates90Cleanly`; the footprint-flip table cases (`TableFlip*`) need `FOOTPRINT::Flip` (M19, pending) and are covered by the oracle below instead (table `Flip` on a footprint child).
* New: `grid_getters_and_stable_normalize` (Normalize stability, `GetEnd`, bbox, borders, row / column / addr), `footprint_child_sync_inverts_rebake`.
* Differential oracle `tests/oracle/m11_oracle.cpp` + `m11_golden.sh` (links the pcb-dump objects of the reference build) -> `tests/golden/m11_boxes_tables.txt` (9403 values): tables (3 x 2, with and without a column span, board level and in a footprint rotated 30 degrees and scaled (1.5, 0.75)) and text boxes (board / footprint) under none / Rotate 30 / Rotate 90 / Flip LR / Flip TB / Mirror LR / Mirror TB / Rotate+Flip / Move / Autosize (tables) / SetTextAngle 45 / Rotate+Normalize (text boxes); per item start / end / lib frame / angles / draw position (also flipped) / corners / bbox / shown text (line breaks, `${ROW}` `${COL}` `${ADDR}`) / min size / effective shape / text polygons / box polygons; per table position / end / bbox / widths / heights / cell order / borders / effective shape / polygons. Replay: `cargo test -p kicad-pcb --features dump --lib m11_matches_oracle`: all 9403 values equal.
* pcb-dump vs pcb-dump-rs (sections item, bbox, text, shape, misc) on scratch copies: `ipc2581/textbox-border`, `issue25074/knockout_textbox`, `odbpp/knockout_textbox_unicode`, `issue24442`, `incorrect_text_mirroring_drc`: 0 mismatch / 0 pending; `issue24525` (board tables incl. a 30 degree rotated one; barcode block removed from the scratch copy, M25 pending): 910 match, 0 mismatch, 6 pending (`table.end` / `border_style` / `separators_style` still `no_api` in `dump/defaults.rs`: change request). `embedded_table_rotated_legacy_v10` (footprint tables) does not load in Rust yet (footprint parser / transforms, M19 / PCBIO).

### Deviations / notes

* `EdaShapeItem::corners_in_sequence` has no context, so for a polygon box (a text box or cell in a footprint whose rotation is not cardinal, after `Flip`) the `EDA_SHAPE` body inflates by the stored width (M7 deviation). Every M11 body that has a context (corners, draw position, line breaking, table borders and effective shape) uses `corners_in_sequence_ctx`, which reads the virtual `GetWidth()` like C++; only callers of the context-free trait method see the M7 deviation.
* Mutators read the parent frame from `MutEnv` (`GetParentFootprint()` / `GetOrientation()` = `ParentFrame::transform`). `IsSideSpecific()` in `PCB_TEXTBOX::Flip` uses the layer-set part only (as `PcbText::flip`): the board layer-type part needs layer types that `MutEnv` does not carry.
* `PCB_TEXTBOX::StyleFromSettings` (no environment, as `PcbText`): size / thickness stored as given (footprint scale not stripped), the parent footprint is the direct parent (a footprint table's cell is not seen as in a footprint), the side is `IsBackLayer( GetLayer() )`.
* `PcbTextbox::normalize()` without environment follows `PcbShape::normalize`: exact for board-level boxes, a footprint child / cell keeps its library frame. Table code and the oracle use `normalize_env`.
* `PCB_TABLE::Normalize` reads widths / heights with `operator[]`, which stores 0 for a missing column / row: the Rust port does the same on copies and stores them with `modify_table`.
* `PCB_TABLE::Autosize` is split: `PcbTable::autosize(&mut self, ctx)` sizes (run on a clone), `Board::table_autosize` stores and normalizes. `(int)` of the width sum uses `f64_to_i32_x86`.
* A table without cells: `GetEffectiveShape` returns an empty compound and `Flip` (board level) does nothing, where the C++ dereferences null.
* Cell `GetRow()` / `GetColumn()` return -1 for a cell that is not stored or has no parent table (C++: null dereference).

### Known gaps / stubs left

* `PcbTable::set_autosized_row_heights` for a generated table (`PCB_GENERATED_TABLE` override): `port_pending!` (M26).
* `PCB_TABLE::Compare` is `PcbTable::compare(ctx, a, b)`; `board/compare.rs::pcb_table_compare` (M22) still calls its own stub (change request).

## Package M19: footprint A (fields, variants, attributes, transform cascades, net ties)

### C++ to Rust map (`pcbnew/footprint.cpp` / `footprint.h` unless stated)

| C++ | Rust | State |
|---|---|---|
| `SetPosition`, `SetOrientation`, `Flip`, `SetTransformScale`, `RescaleAroundPoint`, `MoveAnchorPosition`, `SetLayerAndFlip`, `Move`, `Rotate`, `SetLayer` | `footprint/transform.rs` `Board::footprint_*` | ported: C++ child order per cascade, hooks (`OnFootprintTransformed`, `OnFootprintRescaled`, `Flip`, `Move`, `Rotate`, `KeepUpright`) through `as_leaf_mut` without `modify` bookkeeping, cache patching as C++; works on created-but-unattached footprints |
| `PCB_TABLE::OnFootprintTransformed` / `OnFootprintRescaled` / `Move` inside the cascades | `transform.rs` (cell loops) | ported inline (the three bodies are one cell loop each); table `Flip` goes through `Board::flip_item` |
| `GetField( FIELD_T )` const, `GetField( name )`, `HasField`, `GetFields`, `GetNextFieldOrdinal`, `GetReference`, `GetValue`, `IsAnnotated` | `Footprint::{field, field_by_name, has_field, fields_sorted, next_field_ordinal, reference, value, is_annotated}` | ported (`GetFields` sorts with `kicad_geom::stdsort`) |
| variants: `GetVariant`, `SetVariant`, `AddVariant`, `DeleteVariant`, `RenameVariant`, `HasVariant`, `Get{DNP,ExcludedFromBOM,ExcludedFromSim,ExcludedFromPosFiles}ForVariant`, `GetFieldValueForVariant` | `Footprint::*` | ported |
| attributes (`Set/Is` board-only, DNP, exclusions, placed), `GetFootprintType`, `SetFootprintType`, `GetTypeName`, `GetLikelyAttribute`, `HasThroughHolePads`, `SetStackupMode`, `SetStackupLayers`, `GetLocalClearance( aSource )`, `GetZoneConnectionOverrides`, `Add3DModel`, `IsConflicting`, `GetSide`, `IsWithinSchematicSheet` | `Footprint::*` | ported |
| `GetContextualTextVars`, both `ResolveTextVar` | `contextual_text_vars`, `resolve_text_var`, `resolve_text_var_for_variant` | ported except `${PROPERTY.<name>}` (PROPERTY_MANAGER, not ported: unresolved) |
| `IsOnLayer`, `RunOnChildren` (`ItemRef` form), `GetItemDescription`, `DisambiguateItemDescription`, `GetFPIDAsString` | `Footprint::*` | ported |
| `FindPadByNumber`, `FindPadByUuid`, `GetPad( pos, mask )`, `GetPads`, `GetPadCount`, `GetUniquePadNumbers`, `GetUniquePadCount`, `GetNumberedPadCount`, `GetNextPadNumber` | `Footprint::*` | ported; `UTIL::GetRefDesPrefix` as `footprint::get_ref_des_prefix` |
| `IsNetTie`, `MapPadNumbersToNetTieGroups`, `GetNetTiePads`, `GetNetTieCache`, `BuildNetTieCache` | `Footprint::*`, `Board::build_net_tie_cache` | ported |
| `JUMPER_GROUP` / `JUMPER_GROUP_SET` (`common/jumper_group.cpp`) | `JumperGroup::{make, contains, names}`, `JumperGroupSet::{add, add_names, clear, all, is_empty, find_containing}`, `Footprint::jumper_pad_group` | ported |
| `GetComponentClass`, `GetComponentClassAsString`, `InvalidateComponentClassCache`, `GetStaticComponentClass` | `Footprint::component_class` (`&self`), `Board::footprint_component_class` (manager updated in place) | ported; see deviations |
| `FixUuids` | `Board::footprint_fix_uuids` | ported |
| `IsLibNameValid`, `StringLibNameInvalidChars` | `Footprint::is_lib_name_valid`, `footprint::string_lib_name_invalid_chars` | ported |
| non-const `GetField( FIELD_T )` (creates a missing mandatory field), `UpdateFields`, `ApplyDefaultSettings`, `AutoPositionFields`, `IncrementReference`, `ClearAllNets`, `GetArea`, `GetCoverageArea`, `CoverageRatio`, copy / move / `swapData`, `Duplicate*`, `Matches`, `Visit`, `Serialize`, `GetMsgPanelInfo`, `View*`, `GetSearchTerms` | - | not ported (editor / UI / geometry of M20, or need `BoardCommit`); `UpdateFields`' tests (`test_footprint.cpp`) therefore not ported |

### Tests

`footprint::transform::tests` (16): ports of `test_footprint_transform_sync.cpp`
`DefaultIsIdentity`, `SetPositionUpdatesTransform`, `SetOrientationUpdatesTransform`,
`MoveUpdatesTransform`, `RotateUpdatesTransform`, `SetLayerUpdatesFlipped`,
`PadEffectiveShapeFollowsFootprintScale`, `PadBBoxFollowsFootprintMove` +
`PadBBoxFollowsFootprintRotation`, `RescaleAroundPointMovesAnchorByExpectedDelta` +
`RescaleAroundPointComposesScale` (footprint placed by hand instead of loading issue18),
`MoveAnchorMovesPointsLikeOtherChildren`, `FlipNonCardinalKeepsPadAndShapeCoincident` (+ a
double flip), `BoardlessFootprintFlip` (pad, shape, point, fields; on a board-owned footprint);
new: cascades on an unattached footprint, cache patching / re-stamping, and the oracle replay
`m19_oracle_cascades`. `footprint::tests` (11): `test_variant.cpp` `FootprintVariantBasics`,
`FootprintDNPForVariant`, `FootprintBOMExclusionForVariant`, `FootprintSimExclusionForVariant`,
`FootprintPosFileExclusionForVariant`, `VariantCaseInsensitivity`, `EmptyVariantReturnsBase`,
`UnknownVariantReturnsBase`, `FootprintVariantTextVarResolution`, the footprint side of
`RenameVariantPreservesData` / `VariantDeletionClearsRegistry`; new tests for fields, pad
queries, net-tie groups / cache, jumper groups, children / layers / UUIDs / helpers, component
class. Not ported: `test_footprint_net_tie.cpp` (needs the DRC engine), `test_footprint.cpp`
(`UpdateFields`, bbox / hull: M20), `test_footprint_realignment.cpp` (needs the footprint
library loader), `test_fields_table_presence.cpp` (fields-table UI model, nothing of it is in
the footprint model), the load/save and undo cases of `test_footprint_transform_sync.cpp`.

Oracle: `tests/data/m19/m19_cascade.kicad_pcb` (crafted from `net_tie_hole_clearance`: a
footprint rotated 30 degrees and scaled 1.5 x 0.75 on F.Cu with two fields, a segment, a circle
(an ellipse on the board), an SMD and a PTH pad, and its back-side copy) dumped with
`pcb-dump board --sections footprint,pad,shape,text`; 110 selected values in
`tests/golden/m19_cascade.txt`. The test rebuilds each footprint from its library-frame values
at the identity placement and places it with `footprint_set_position`,
`footprint_set_orientation`, `footprint_set_transform_scale` (and `footprint_flip` for the copy):
every value (pad positions, library positions, orientations, layer sets, scaled sizes and
drills, shape board / library coordinates and widths, field positions, sizes, thickness, angle,
mirroring and layers, footprint transform, side, likely attribute) is identical. The back-side
file was mirrored by hand (the reference tool cannot flip); its field angle (180) follows
`PCB_TEXT::Flip` (M10), so that one value checks consistency, not an independent C++ flip.

### Deviations / notes

- Every cascade ends with a time-stamp bump (`board-model.md` 8.5). The footprint's
  time-stamped geometry caches that C++ keeps valid by patching them (`SetPosition`,
  `MoveAnchorPosition`, `SetOrientation`, `Flip` move / rotate / mirror them) are re-stamped
  with the new stamp when they were valid before, so patched values are used where C++ uses
  them; the ones C++ zeroes stay zero. The journal records nothing for a cascade.
- The child hooks run without `modify` (no per-child time stamp, no invalidation), as the C++
  calls them; children's own caches are keyed by their inputs. A footprint table's `Flip` uses
  `Board::flip_item`, which goes through `modify` and so drops the footprint's geometry caches
  that C++ patches afterwards (results equal, only rebuilt).
- `GetComponentClass` from `&self` recomputes against a clone of the manager when the ticker
  moved: the effective class the C++ registers in the manager on the way is not kept there
  (`Board::footprint_component_class` keeps it). With dynamic assignment rules it is
  `port_pending!` (the rule matcher is kicad-drc).
- `FixUuids` uses `rebind_item_uuid` (direct write for an unindexed UUID, as
  `ResetUuidDirect`; re-bind if the footprint is indexed) and bumps the time stamp.
- `GetReference` / `GetValue` of a footprint without the mandatory field return "" (C++
  dereferences null).
- `MapPadNumbersToNetTieGroups` matches `\` and `,` on characters (C++ casts `wxUniChar` to
  `unsigned char`, which only differs for code points that map to those bytes in the locale).
- `GetNetTiePads` of a pad whose number is not in the map uses group 0 (`std::map::operator[]`).
- `GetNextPadNumber` wraps on `int` overflow (C++ UB).

### Stubs left / known gaps

- `${PROPERTY.<name>}` in `ResolveTextVar`: not ported (no property manager), unresolved.
- `component_class` with assignment rules: `port_pending!` (kicad-drc rule matcher).
- Run-time dependencies on other packages' stubs: `PCB_TABLE::Flip` (`flip_item`, M11),
  `PCB_BARCODE::Flip` / `Move` / `Rotate` (M25) inside `Flip`, `MoveAnchorPosition`,
  `SetOrientation` of a footprint holding a barcode; `PCB_TABLE` / barcode bounding boxes etc.
  are not reached by the cascades.

## Package PCBIO-12: board file reference images, barcodes, targets, points, grid items, tables, generated tables, drill maps (parse + write)

Files: `src/io/parser/misc_items.rs`, `src/io/writer/misc_items.rs`, `tests/pcbio12_misc_items.rs`,
`tests/data/pcbio12/`, `tests/golden/pcbio12/`, `tests/oracle/pcbio12_golden.sh`,
`tests/oracle/pcbio12_resave.cpp` + `pcbio12_build_oracle.sh` (pure parse + write driver).

### C++ to Rust map

| C++ (`pcb_io_kicad_sexpr_parser.cpp`, `pcb_io_kicad_sexpr.cpp`) | Rust | state |
|---|---|---|
| `parsePCB_REFERENCE_IMAGE` (base64 `data`, `Failed to read image data.`, `isnormal` scale, PPI re-scale < 20260623) | `parse_pcb_reference_image` | ported, verified |
| `parsePCB_BARCODE` (`at` with optional angle = inline `SetOrientation`, `SetKind` / `SetErrorCorrection` Micro-QR rule, `SetTextSize` pen size, `parseMaybeAbsentBool` locked) | `parse_pcb_barcode` | ported; blocked at run time by `PCB_BARCODE::SetPosition` (model M25) |
| `parsePCB_TABLECELL`, `parseTableBodyToken`, `parseTableBody`, `parsePCB_TABLE` | `parse_pcb_tablecell` (cell mirrored like the C++ constructor), `parse_table_body_token`, `parse_table_body`, `parse_pcb_table` | ported, verified |
| `parseGeneratedTable`, `parseGeneratedTableExtra` (columns replacing defaults, `m_Id = -1`, `Validate`, any-shown, documentation-layer check, drill chart filter / totals / row shapes, row keys) | `parse_generated_table`, `parse_generated_table_extra` | ported, verified (pure parse + write oracle) |
| `parseDrillSpanBody`, `parsePCB_DRILL_MAP` | `parse_drill_span_body`, `parse_pcb_drill_map` | ported, verified |
| `parsePCB_POINT`, `parsePCB_TARGET`, `parsePCB_GRID_ITEM` (polar / cartesian `spacing` / `extent`, `affects`) | `parse_pcb_point`, `parse_pcb_target`, `parse_pcb_grid_item` | ported, verified |
| `DocumentationLayers()` (`board_tables/generated_table_schema.cpp`) | `parser::misc_items::documentation_layers` | ported (belongs to the table model; change request) |
| `format( PCB_REFERENCE_IMAGE / PCB_POINT / PCB_TARGET / PCB_GRID_ITEM )` | `format_reference_image`, `format_point`, `format_target`, `format_grid_item` | ported, verified |
| `format( PCB_BARCODE )` | `format_barcode` | ported; blocked by `PCB_BARCODE::GetPosition` / `GetAngle` (M25) |
| `formatTableData`, `format( PCB_TABLE )`, `format( PCB_GENERATED_TABLE )`, `formatGeneratedTableExtras`, `format( PCB_DRILL_MAP )`, static `formatDrillSpan` | `format_table_data`, `format_table`, `format_generated_table`, `format_generated_table_extras`, `format_drill_map`, `format_drill_span` | ported, verified |

### Tests and verification

- Byte level, `kicad-cli pcb upgrade --force` (`oracle_resave_blocks`): 6 hand-made boards in
  `tests/data/pcbio12` (images incl. rotate / scale / opacity / lock / custom property, targets
  incl. `tstamp` and width 0, points, cartesian / polar / clamped grid items, drill maps incl.
  backdrill / npth spans and an unknown token, two tables incl. the rotated table of issue24525,
  a pre-20260623 image board for the PPI migration, barcodes) and the corpus boards
  reference_images_load_save, group_and_image, issue7567: every top-level block of these kinds
  in the Rust re-save equals the reference re-save (O1), and the Rust re-save of O1 equals the
  reference re-save of O1 (O2 / O1): 48 blocks, 0 differences (random uuids of uuid-less input
  items masked).
- Byte level, pure parse + write (`oracle_parse_write_blocks`): kicad-cli's loader rebuilds the
  cells of generated tables (`RefreshGeneratedTables`), so `tests/oracle/pcbio12_resave.cpp`
  (reference `ReadBoardFromFileOrStream` + `PCB_IO_KICAD_SEXPR::SaveBoard`, linked like
  pcb-dump) writes `<name>_pw.kicad_pcb`: 21 blocks identical, including a drill chart
  (precision, renamed column, row keys, filter, totals, row shapes) and a stack-up table (units,
  hidden column, a repeated and an unknown column dropped by `Validate`, unknown token skipped).
- Corpus (`oracle_corpus_blocks`, io-corpus cache): reference_images_load_save,
  group_and_image(_formatted), issue7567, stonehenge identical; issue24525 and api_kitchen_sink
  pending on the barcode model.
- Item level: `pcb-dump board --no-init --sections item,misc,text` vs pcb-dump-rs on copies of
  the data boards and the three corpus boards: 0 mismatches (misc_items 302 match,
  tables 479 match + 6 pending dump-rs fields, reference_images 216, group_and_image 257,
  issue7567 136 match; its missing lines are footprints, PCBIO-06).
- Ported C++ tests: `test_reference_image_load.cpp` `ReferenceImageLoading` (load part),
  `ReferenceImagePpiScaleMigration` (file part), `test_custom_property_roundtrip.cpp`
  `EveryBoardItemKeepsItsCustomProperty` (image, target, point, grid item, drill map),
  `test_drill_chart.cpp` `MapSymbolSizeSurvivesBoardFileRoundTrip` (file part); error texts of
  the strict places (`strict_errors`). Not ported: `test_barcode_load_save.cpp` (barcode model
  M25), the other `test_drill_chart.cpp` cases (refresh / hit tests: M26 / M23).

### Deviations / notes

- `PCB_DRILL_MAP( aParent )` takes the symbol size of the board's drill symbol profile; the
  profile is kept verbatim (tier-2 drill symbol package), so a map without `(size ..)` gets the
  default profile's size. The writer always writes `(size ..)`.
- The barcode setters that end in `AssembleBarcode()` only refill the keyed symbol cache; the
  port calls `assemble_barcode` once at the end like the parser's final call.
- `parsePCB_BARCODE`'s `(at x y)` without an angle needs a second `)` (C++ `NextTok` +
  `NeedRIGHT`); reproduced.

### Known gaps

- Barcodes end to end: `PCB_BARCODE::SetPosition` / `GetPosition` / `GetAngle` / `Rotate` /
  `SetLayer` / `AssembleBarcode` are `port_pending!` (M25); `barcodes.kicad_pcb` goldens are
  committed (both oracles) and compare once they land.

## Package GLUE-1: finished ports wired into their call sites

Owned changes (no trait or public signature changed):

| C++ | Rust | state |
|---|---|---|
| `BOARD::cmp_drawings` shape / textbox branch (`shape->Compare( other )`) | `board/compare.rs` `eda_shape_compare` -> `EdaShape::compare` | ported |
| `PADSTACK::COPPER_LAYER_PROPS::Compare` custom shapes loop | `padstack.rs` `CopperLayerProps::compare` | ported |
| `BOARD_ITEM::GetLayerName`, `GetFPRelativePosition`, `BOARD_CONNECTED_ITEM::GetNetname` | `item_ref.rs` forwarders (`""` for non-connected kinds) | ported |
| `BOARD::ApplyNetChainNetclasses` | `board/nets.rs` | ported |
| `std::set<T, CMP>( begin, end )` | `io/writer/mod.rs` `sorted_set` (also used by the dump's `order` section) | ported: libstdc++ `_Rb_tree` (hinted `_M_insert_unique_( end() )`, `_M_get_insert_unique_pos`, `_Rb_tree_insert_and_rebalance`) on an index arena |
| pcb-dump getters | `dump/{items,board,defaults,settings}.rs`: pad (`IsFlipped`, `IsNPTHWithNoCopper`, `IsFreePad`, local zone connection / spoke width / thermal gap, `GetThermalGap`, `GetDelta`, `GetRoundRectCornerRadius`, `GetBoundingBox( aLayer )`), textbox border width and `TransformTextToPolySet`, dimension units mode / aligned angle / radial knee, zone teardrop area / corner count, reference image bbox, `BOARD::GetVisibleLayers`, the six JSON-backed `MEANDER_SETTINGS` members | wired |

Why `sorted_set` changed: the former stable sort + dedup equals `std::set` only for a strict weak
ordering. `EDA_SHAPE::Compare` (EPSILON = 2 IU) is not transitive, so the C++ order depends on
the shape of the red-black tree: `reverse_via.kicad_pcb` (Bezier curves whose start x differ by
1..22 nm) had 6 drawings in a different order. With the tree port the drawing order of that
board equals `pcb-dump --sections order` exactly (4116 items).

Verification:
* PCBIO-03 corpus oracle (`tests/pcbio03_tracks_shapes.rs` `oracle_corpus_blocks`), before ->
  after: identical 256 -> 510, same set 4 -> 12 (all `tracks`, net codes follow footprint pads,
  PCBIO-06), pending 272 -> 3 (`PCB_BARCODE::SetPosition`, M25), explained 1 -> 1, differ 0 -> 0.
* Defaults scoreboard (`tests/pcb_dump_defaults.rs`): match 3447 -> 3533, mismatch 0 -> 0,
  pending 235 -> 149.
* Item level, `pcb-dump board --no-init` vs `pcb-dump-rs` through `compare.py` on 6 small boards
  with text boxes, dimensions, keepout zones (`textbox-border`, `knockout_textbox`,
  `issue20249/dimension`, `issue24853`, `keepout_no_clearance`, `issue24442`): 5990 match,
  0 mismatch, 272 pending (meander members below, `GetValueMode`).
* Pad getters can only be checked on the defaults dump until footprints load (PCBIO-06).

Deviations / known gaps:
* `meander.*`: only `min_amplitude`, `max_amplitude`, `spacing`, `corner_style`,
  `corner_radius_percentage`, `single_sided` exist in the model (the JSON of
  `tuning_pattern_settings`, read back as `read_settings` does); the other 15 members of
  `PNS::MEANDER_SETTINGS` are not modelled and stay pending.
* Left pending (target not ported): `FOOTPRINT::ResolveTextVar`, `GetFieldValueForVariant`
  (M19 / M20), `BOARD::GetTrackLength`, `BOARD::DrillSymbolCache`, `PCB_DIMENSION_BASE::GetValueMode`,
  table / barcode / footprint getters without a Rust method, `PAD::IsFreePad()` without ctx
  (signature change needed; the dump uses `is_free_pad_ctx`).
* Staging-stub tests now use `(footprint ..)` (PCBIO-06) as the remaining stub; `oracle_resave`
  compares whole files (the setup section is ported).

## Package M21: board outline (convert_shape_list_to_polygon, GetBoardPolygonOutlines)

| C++ | Rust | state |
|---|---|---|
| `convert_shape_list_to_polygon.h/.cpp`: `close_enough`, `closer_to_first`, `free_end`, `isCopperOutside`, `PCB_SHAPE_ENDPOINTS_ADAPTOR` + nanoflann `KDTree`, `processClosedShape`, `processShapeSegment`, `buildContourHierarchy`, `addOutlinesToPolygon`, `addHolesToPolygon`, `checkSelfIntersections`, `findNeighbours` / `closerEndpoint`, `findCrossingContours`, `buildChainedClosedContour`, `doConvertOutlineToPolygon` | `convert_shape_list_to_polygon.rs` (private fns of the same names, `do_convert_outline_to_polygon` `pub(crate)`) | ported |
| `ConvertOutlineToPolygon`, `TestBoardOutlinesGraphicItems`, `BuildBoardPolygonOutlines`, `BuildFootprintPolygonOutlines`, `buildBoardBoundingBoxPoly`, `projectPointOnSegment`, `findEndSegments` | `convert_outline_to_polygon`, `test_board_outlines_graphic_items`, `build_board_polygon_outlines`, `build_footprint_polygon_outlines`, `build_board_bounding_box_poly`, `project_point_on_segment`, `find_end_segments` (all `pub`, `ctx` first, shape lists as `&[ShapeId]`, `aPolygons` / `aOutlines` as `&mut ShapePolySet`) | ported |
| `PCB_TYPE_COLLECTOR::Collect( board/footprint, { PCB_SHAPE_T } )` | `collect_board_shapes` (footprint drawings in footprint order, then board drawings: `BOARD::Visit` order), `collect_footprint_shapes` | ported |
| `BOARD::GetBoardPolygonOutlines` (NPTH subtraction, per-island simplify with arcs) | `Board::get_board_polygon_outlines` (`board/outline.rs`) | ported |
| `BOARD::UpdateBoardOutline` + `PCB_BOARD_OUTLINE::GetOutline()` | `Board::board_outline(ctx)`, `BoardCaches::board_outline` (built on first use after a time stamp bump: outlines without inference, fractured on success) | ported |
| `PCB_BOARD_OUTLINE` item class (view layers, Similarity, operator==, Clone) | - | skipped: the outline is a cache, not an item (board-model-types.md 15) |
| `GetBoardEdgesBoundingBox`, `ComputeBoundingBox` | `board/queries.rs` (M0d / M22) | used as they are |

Tests: `convert_shape_list_to_polygon::tests`: `degenerate_arc_outline_marker_issue24241` and
`invalid_outline_bezier_slot_issue24078` (ports of `test_drc_invalid_outline_issue24241.cpp` /
`_issue24078.cpp`, outline-building part: the handler calls of `TestBoardOutlinesGraphicItems` +
`BuildBoardPolygonOutlines` with the DRC parameters; 24078 skips itself until footprints parse,
PCBIO-06), `oracle_corpus` (differential), `closures_are_outline_error_handlers`.

Verification: `tests/oracle/m21_outline_oracle.cpp` (built by `m21_outline_golden.sh` on the
pcb-dump objects) runs, on scratch copies of every corpus board under 500 KB (qa/data/pcbnew,
demos) plus 13 malformed-outline fixtures (`tests/oracle/m21_fixtures.py`: gaps, sub-epsilon
gaps, arc closure, nested holes of every closed kind, bow tie, T junction, overlapping closed
shapes, degenerate items, bezier slots, ellipses / ellipse arcs, disjoint outlines, duplicate
segments, no edges, empty board), `TestBoardOutlinesGraphicItems`, `BuildBoardPolygonOutlines`
(DRC parameters), `GetBoardPolygonOutlines` (plain, inferred, arcs, NPTH), 
`BuildFootprintPolygonOutlines`, `ConvertOutlineToPolygon` on each footprint's F.CrtYd shapes
(courtyard parameters, `aAllowDisjoint = false`) and `UpdateBoardOutline`, recording every
error-handler call (message, item UUIDs, position, order) and a hash of each polygon (vertex
for vertex, contour order, closed flags, arc counts). Golden `tests/golden/m21_outlines.txt`
(333 boards). Result: 103 boards load in the Rust parser today (no footprints / barcodes /
generators), all 103 identical, with two documented pointer-order lines (below). Also
`pcb-dump board --no-init --sections board` vs `pcb-dump-rs` (`outline`, `outline_inferred`,
`outline_arcs` and their `.ok`): identical on all 95 footprint-free corpus/fixture boards the
Rust parser loads (boards with footprints differ only because the footprints are not parsed
yet). Not verifiable yet: footprint Edge.Cuts handling (`isCopperOutside`, footprint holes,
`BuildFootprintPolygonOutlines` with a footprint, NPTH subtraction, courtyards) - covered by the
golden for 230 boards that need PCBIO-06 / PCBIO-12 / PCBIO-13; `oracle_corpus` picks them up
automatically once they parse.

Deviations:

- `TestBoardOutlinesGraphicItems` iterates `std::set<PCB_SHAPE*> openShapes` (heap-address
  order) to seed the slot chaining and to build the nanoflann point list; the port uses
  `ShapeId` order (board-model.md 3.1 rule 2). It decides which shape owns a chained slot
  contour (item B of a "(self-intersecting)" overlap report) and the contour's start point. The
  oracle shows the reference allocator does not hand out ascending addresses: 2 of the 13
  fixtures (`m21_overlap`, `m21_bezier_slot`) report a different owner (and, for the bezier
  slot, a different marker position); listed in `POINTER_ORDER_DEVIATIONS`. None of the 90
  compared corpus boards is affected.
- `processShapeSegment` calls `PCB_SHAPE::RebuildBezierToSegmentsPointsList( aErrorMax )`,
  which rewrites the board item's `m_bezierPoints` (the board stays modified after the call,
  e.g. after DRC with 0.05 mm). The port does not mutate the board: it uses a locally built
  point list and, within the same top-level call, gives the error reports
  (`GetEffectiveShape()` of a bezier) a copy rebuilt with that error (`OutlineScratch`). Effects
  across calls (a later `GetEffectiveShape()` / length of the bezier) are not reproduced.
- `SKIP_STRUCT` flags live in `OutlineScratch` (algorithm-local set), not in `base.flags`.
- `BuildFootprintPolygonOutlines`, one-segment case: the C++ dereferences possibly empty
  `OPT_VECTOR2I`s in the corner branches (undefined behaviour for a segment parallel to two bbox
  sides that falls through); the port uses (0, 0).
- `Board::get_board_polygon_outlines` returns a fresh set (C++ appends to `aOutlines`; every
  caller passes an empty one).

Known gaps: wxLogTrace output of `traceBoardOutline` is dropped. No stubs left in the two files.

## Package PCBIO-13: board file generators and geometric constraints

Files: `src/io/parser/generator.rs`, `src/io/writer/generator.rs`, `tests/pcbio13_generators.rs`,
`tests/data/pcbio13/`, `tests/golden/pcbio13/`, `tests/golden/pcbio13_corpus.txt`,
`tests/oracle/pcbio13_golden.sh`.

### C++ to Rust map

| C++ (`pcb_io_kicad_sexpr_parser.cpp` / `pcb_io_kicad_sexpr.cpp`) | Rust | state |
|---|---|---|
| `parseGENERATOR` (`uuid` / legacy `id` header, type, name, `locked`, layer, members, templates, custom properties, typed property map: `yes`/`no`, number, quoted string, `(xy ..)`, `(pts ..)`, `(cells (ij ..))`, `emplace` = first value of a key wins; ghost tuning patterns / member-less via stacks dropped) | `parser/generator.rs` `parse_generator`, `parse_generator_property` | ported |
| `parseGENERATOR_templates` | `parse_generator_templates` | ported |
| `parseCONSTRAINT` (type, uuid, members with the optional VERTEX index, value in mm for length types, driving, custom properties) | `parse_constraint` | ported |
| `resolveConstraints` (footprint lookup map, `BOARD::ResolveItem`, append-mode remap with `find`, dangling members kept, other-footprint members dropped, `Add( INSERT )` to the recorded parent) | `resolve_constraints` | ported |
| generator creation (`CreateFromType`, `SetLayer`, `SetProperties`, `SetTemplateItem`) | PCBIO-09 `generator_from_info` (unchanged) | ported (now exercised) |
| `format( const PCB_GENERATOR* )` (ghost skip, `(type ..)` bare, name / layer quoted, `locked`, properties in key order with `{:.10g}` numbers, `FormatBool`, `(xy ..)`, `formatPolyPts`, `(cells (ij ..))`, quoted strings; templates; sorted member uuids; custom properties) | `writer/generator.rs` `format_generator` | ported except the template item itself (see known gaps) |
| `format( const PCB_CONSTRAINT* )` (member-less skip, VERTEX index, `FormatDouble2Str` value, lengths divided by `IU_PER_MM`, `(driving no)`) | `format_constraint` | ported |

### Tests and verification (2026-11-10)

- Byte level, corpus (`oracle_corpus_generators`, golden `pcbio13_corpus.txt` from the
  io-corpus re-saves of the reference `kicad-cli pcb upgrade --force`): 23 corpus boards with
  `(generated` or `(constraint`; 22 checked (incl. the demos cm5_minima and openair-max; the
  only top-level constraints are `demos/constraints`, the orientation_drc match is a rule),
  all `(generated ..)` / `(constraint ..)` blocks identical (68 generators on 20 boards, 87
  constraints); on issue25265, issue25303 and
  stitch_fp_rulearea identical except the template via (pending, below). api_kitchen_sink
  (footprint-level constraints only) stops on the `PCB_BARCODE::SetPosition` model stub.
  `qa/data/pcbnew/vme-wren.kicad_pcb` (29 MB, not in the io-corpus cache) checked once by hand
  with `oracle_large_board` (`--ignored`): 89 blocks identical. Not run: the large demos
  jetson-agx-thor and demos/vme-wren (machine rules).
- Byte level, hand-made board `tests/data/pcbio13/generators.kicad_pcb` (via stack with hops,
  quoted / escaped strings, duplicate key, wrong layer normalised by `start_layer`; ghost via
  stack; locked via stitch with cells, chain with an arc, unknown key, template via; constraints
  of every anchor, vertex with and without index, dangling member, values for length / angle /
  tiny length, `(driving no)`, custom property, member-less constraint): the Rust re-save of the
  input and of the reference re-save reproduce the reference blocks (`oracle_resave_handmade`).
- Parse errors (`parse_errors`): 10 malformed generator / constraint bodies give the reference
  load error texts byte for byte (golden `tests/golden/pcbio13/parse_errors.txt`, recorded from
  `kicad-cli`), incl. "Cannot create generated object of type '%s'".
- Item level (`tools/io-corpus/run.sh --dump NAME --sections item,misc`, pcb-dump `--no-init`
  vs pcb-dump-rs): via_stacks, both tuning_generators_load_save files, the 4 drc_via_stack
  boards, demos/constraints: 0 mismatch / 0 missing; issue25265, issue25303,
  stitch_fp_rulearea, drc_skew, issue22536, issue18045, issue23550, issue17971,
  diff_pair_uncoupled_tuning_drc, multichannel_nested: every generator line and every
  generator membership matches; the remaining mismatches are `net` codes and group members /
  positions that are footprints (PCBIO-06 pending), missing lines are footprint items.
- Ports: `test_generator_load_save.cpp` `GeneratorLoading` (both files, lock / name / 47
  members) + save / reload; `test_via_stack_saveload.cpp` `GoldenFileStillLoads`,
  `PropertiesAndMembersRoundTrip` (on the golden board: the via stack setters are editor code),
  `MemberlessStackIsNotSaved` (load here, save in the hand-made oracle);
  `constraints/test_constraint_serialization.cpp` `RoundTrip`, `VertexIndexRoundTrip`,
  `VertexIndexValidation`, `MissingShapeMemberPreservedAsError`. Not ported:
  `AppendRemapsConstraintMembers` (append mode is PCBIO-16; `resolve_constraints` has the
  remap branch).

### Deviations / notes

- `STRING_ANY_MAP::emplace` keeps the first value of a repeated key (`entry().or_insert`).
- A declaration interrupted by a parse error stays recorded, as in C++ (harmless: the parse
  fails).
- The ids the C++ draws (`GENERATOR_INFO()`, `CONSTRAINT_INFO()`, `new PCB_CONSTRAINT`) are
  drawn from `board.kiid_generator` at the same points.

### Known gaps

- Generator template items (the via stitch's via template) are parsed and kept, but not
  written: `format_track` (PCBIO-03) takes an arena handle, the template is owned by the
  generator. The writer prints `(templates (template (name "via")` and skips the via with
  "port pending: generator template (PCBIO-13)" (`allow_pending`) until `writer/tracks.rs`
  gains a `&Track` entry point (change request); then `format_template_item` becomes one call.
- A via stitch without a parsed template: C++ always has a default `PCB_VIA` template and
  writes it; the model keeps no default template (M23 gap). No corpus board has one.
- Footprint-level constraints are parsed once `parseFOOTPRINT` (PCBIO-06) calls
  `parse_constraint`; `resolve_constraints` already handles footprint parents.

## Package PROP-1: board item property tables

`src/properties/` (+ `src/dump/props.rs`, the `props-table` command of `pcb-dump-rs` and the
opt-in `props` dump section): the `PROPERTY_MANAGER` model the custom-rule language reads
(`A.Net_Class`, `A.Pad_Type == 'SMD'`). API for DRC-21: `property_manager()` (built once),
`class_of( ItemRef )` (= `TYPE_HASH( *item )`), `get_property( class, name )` (case-insensitive;
the rule language replaces `_` by a space first), `properties( class )`, `all_classes()`,
`is_of_type`, `derived_classes`, `is_available_for` / `is_writeable_for`, `Property::get( item )`
-> `PropertyValue` (enum values carry their `EnumId`; `enum_as_string()` is the `wxAny`
conversion `GetValue` uses), `Property::has_choices()`, `EnumId::map()` (`ENUM_MAP`).
`PCBEXPR_VAR_REF::ClassifyProperty` is ported privately in `dump/props.rs` (DRC-21 owns it).

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `include/properties/property_mgr.h`, `common/properties/property_mgr.cpp` (`RegisterType`, `AddProperty`, `ReplaceProperty`, `AddTypeCast`, `InheritsAfter`, `Mask`, `OverrideAvailability`, `OverrideWriteability`, `IsAvailableFor`, `IsWriteableFor`, `IsOfType`, `Rebuild`, `CLASS_DESC::rebuild` / `collectPropsRecur`, `GetProperty`, `GetProperties`, `GetDisplayOrder`, `GetGroupDisplayOrder`, `GetAllClasses`, `TypeCast` existence) | `properties/mod.rs` | ported (listeners, commit handler, dynamic properties: skipped, UI) |
| `include/properties/property.h` (`PROPERTY_BASE`, `PROPERTY`, `PROPERTY_ENUM`, `ENUM_MAP`, `DECLARE_ENUM_TO_WXANY` conversion, `TYPE_HASH` / `TYPE_NAME`) | `properties/mod.rs`, `properties/enums.rs` | ported; setters, validators, choices functions, indeterminate functions recorded as flags only (tier 3 / UI) |
| `EDA_ITEM_DESC`, `EDA_SHAPE_DESC` (+ `GetFillModeProp`), `EDA_TEXT_DESC` (+ `GetFontProp`, `GetTextThicknessProperty`) | `eda_item.rs`, `eda_shape.rs`, `eda_text.rs` | ported |
| `BOARD_ITEM_DESC` (+ `GetParentAsString`), `BOARD_CONNECTED_ITEM_DESC` | `board_item.rs` | ported; `Parent` of a board-level item is pending (`BOARD::m_Uuid`, generated at construction, not in the Rust model) |
| `PAD_DESC`, `TRACK_VIA_DESC`, `ZONE_DESC`, `FOOTPRINT_DESC`, `PCB_SHAPE_DESC`, `PCB_TEXT_DESC`, `PCB_TEXTBOX_DESC`, `PCB_FIELD_DESC` | `pad.rs`, `pcb_track.rs`, `zone.rs`, `footprint.rs`, `pcb_shape.rs`, `pcb_text.rs` | ported (`isPadEditMode`: footprint-holder boards only, pending `PAD::IsEntered`) |
| `DIMENSION_DESC` and the five concrete dimension descriptors (+ `DimensionHasValueMode`) | `pcb_dimension.rs` | ported; `Value Mode` / `Value` pending (`DimensionValueMode`, `GetValueFieldText`: constraint builder not in the Rust model) |
| `PCB_TABLE_DESC`, `PCB_TABLECELL_DESC` (+ `IsGeneratedTableCell`), `PCB_GENERATED_TABLE_DESC`, `PCB_DRILL_CHART_DESC`, `PCB_STACKUP_TABLE_DESC` | `pcb_table.rs` | ported; generated-table `Row Height`, `Font Name`, `Font Size` pending (`GetRowHeightProp`, `GetFontNameProp`, `GetFontSizeProp` not in the Rust model) |
| `PCB_GROUP_DESC`, `PCB_BARCODE_DESC`, `PCB_REFERENCE_IMAGE_DESC`, `PCB_3D_BODY_DESC`, `PCB_DRILL_MAP_DESC`, `PCB_GRIDITEM_DESC`, `PCB_MARKER_DESC`, `PCB_POINT_DESC`, `PCB_TARGET_DESC`, `PCB_CONSTRAINT_DESC` | `misc.rs` | ported; barcode `Orientation` calls the pending `PCB_BARCODE::GetAngle`; drill-map `Hole Span` pending for a single span (`EnumerateDrillSpans`) |
| `PCB_TUNING_PATTERN_DESC`, `PCB_VIA_STACK_DESC`, `PCB_VIA_STITCH_DESC` | `generators.rs` | table ported; the getters are pending (the generator subclasses' typed members live in the `PcbGenerator` property map, no Rust getters) |
| `PCB_GENERATOR_DESC` | - | compiled out in C++ (`#ifdef GENERATOR_ORDER`); `PCB_GENERATOR` exists only as a base, as in the reference build |

### Ported C++ tests

`qa/tests/common/test_property.cpp`: `NotexistingProperties`, `TypeCasting`, `EnumGlob`
(choices, enum to string conversion), `Availability` (through the board tables), `Alias`,
`Rename`, `AlternativeEnum`, `DuplicateProperty` on a private manager whose classes stand in for
the test's `A`..`E` (`properties/tests.rs`); `SetGet`, `VirtualMethods`, `IncorrectType`
skipped (setters / `wxAny` typing, not ported). `qa/tests/pcbnew/test_pcb_dynamic_properties.cpp`
skipped (dynamic properties, see gaps).

### Verification

* `pcb-dump props-table` vs `pcb-dump-rs props-table`: 1286 / 1286 lines identical (43 classes,
  all properties with order, owner, base, type, display, coordinate type, group, choices, flags,
  `ClassifyProperty` kind and `GetProperty` result; 44 enum maps). Committed hashed:
  `tests/golden/props_table.txt`, test `tests/props_oracle.rs`.
* `--sections props` on copies of 140 corpus boards (`qa/data/pcbnew`), about 670 000 matching
  values: 113 boards without any mismatch, 0 mismatches caused by this package; every remaining mismatch is on an item whose other sections (`item`, `track`,
  `shape`, `text`, `zone`, `pad`, `footprint`, `dim`) mismatch too or that is keyed by an
  ordinal shifted by items the Rust loader does not build yet (checked per key). 10 boards are
  committed hashed (`tests/golden/props_values.txt`, generator `tests/oracle/props_golden.sh`).

### Deviations

- `GetProperties( type )` is exposed in display order with each property once; the C++ vector
  is sorted by heap address and holds a property twice when a class reaches a base along two
  paths (`all_properties_count` gives the C++ size).
- `GetProperty` takes the first match in collection order (C++: address order); identical for
  every lookup of the reference table, including the duplicate `Text` of the dimension classes.
- `GetAllClasses()` is in `ClassId` order (C++: `unordered_map` order).
- `TYPE_NAME` is reproduced as the Itanium mangled name (`3PAD`), as in the reference build.
- Choice labels are the untranslated `_HKI` strings; the few `_()` group names
  (`Border Properties`, `Margins`, `Table Properties`, `Table`, `Table Generator`,
  `Drill Chart Properties`, `Drill Map Properties`) are the English source text, as the
  reference environment shows them.

### Known gaps

- Setters, validators, choice functions (font lists, pin types), indeterminate functions,
  listeners, `TranslateChoices`: not ported (tier 3 / UI); recorded as flags.
- Dynamic properties (`EDA_ITEM::GetDynamicProperties`: custom properties, `FOOTPRINT`
  mandatory-field properties): not ported; the rule language does not reach them
  (`PCBEXPR_UCODE::CreateVarRef` uses `GetProperty( TYPE_HASH, name )`).
- Pending getters (see the map above) print `pending|props|...` in the dump.
- The variant overrides of `PCBEXPR_VAR_REF::GetValue` for footprint attributes and the
  "number in a string" conversion belong to DRC-21 (`pcbexpr_evaluator.cpp`).

## Package M20: footprint B (geometry, courtyard and hull caches, comparators, polygons)

| C++ | Rust | State |
|---|---|---|
| `GetBoundingBox()`, `GetBoundingBox( aIncludeText )` (cache per board time stamp), `GetCenter`, `GetLayerBoundingBox`, `TextOnly`, `GetArea` | `footprint/geometry.rs` `Footprint::{bounding_box, bounding_box_with_text, center, layer_bounding_box, text_only, area}` | ported |
| `GetFpPadsLocalBbox` | `Footprint::fp_pads_local_bbox` | ported; the C++ footprint copy is placed in a **clone of the board** (a footprint cannot exist outside its board): correct, but costs a board clone per call (only the placement-file writer calls it) |
| `GetBoundingHull()` (cached), `GetBoundingHull( aLayer )` | `bounding_hull`, `bounding_hull_on_layer` | ported |
| `GetEffectiveShape` | `effective_shape` | ported |
| `HitTest( pos )`, `HitTestAccurate`, `HitTest( rect )`, both `HitTestOnLayer` | `hit_test`, `hit_test_accurate`, `hit_test_rect`, `hit_test_on_layer`, `hit_test_rect_on_layer` | ported |
| `HitTest( SHAPE_LINE_CHAIN )` | - | skipped: lasso selection (editor), no `hit_test_poly` virtual in the trait |
| `BuildCourtyardCaches`, `GetCourtyard`, `GetCachedCourtyard`, MALFORMED_F/B_COURTYARD flags | `build_courtyard_caches`, `courtyard`, `cached_courtyard`, `courtyard_flags` | ported (flags live in the cache value, see deviations) |
| `FixUpPadsForBoard` | `Board::footprint_fix_up_pads_for_board` | ported; the parser must not call it for a `.kicad_mod` holder board (C++ passes a null board there) |
| `TransformPadsToPolySet`, `TransformFPShapesToPolySet` | `footprint/shapes.rs` | ported (`PAD dummy( *pad )` = `detached_copy()` + `set_size` through the parent's `MutEnv`) |
| `cmp_drawings`, `cmp_pads`, `cmp_zones` (and `cmp_points_opt`) | `footprint/compare.rs` `footprint_cmp_{drawings,pads,zones}` | ported; pointer tie-break = `ItemId` order (`board-model.md` 3.1) |
| `operator==( const FOOTPRINT& )` | `Footprint::eq_item` | partial: pads (`PAD::operator==( const PAD& )` body inline), shapes, zones, groups, points compared; other children (texts, text boxes, dimensions, tables, barcodes, images, 3D bodies) and `PCB_FIELD::operator==` are `port_pending!` (no Rust `operator==` for those types yet) |
| `FOOTPRINT_COURTYARD_INDEX`, `BOARD::GetFootprintCourtyardIndex` | `footprint_courtyard_index.rs` (`FootprintCourtyardIndex::{new, query_overlapping}`, `Board::footprint_courtyard_index`, cached in `BoardCaches` until the next time stamp bump) | ported |
| `GetCoverageArea`, `CoverageRatio` | - | skipped: selection disambiguation (GUI, needs `GENERAL_COLLECTOR`) |

### Deviations

- `MALFORMED_F/B_COURTYARD` are stored in the courtyard cache value (`&self` build): they read
  as cleared once the cache is dropped (`Board::modify_*` of the footprint or a child), where
  C++ keeps the last flags on the item until the next `BuildCourtyardCaches`. Every C++ reader
  (DRC courtyard tests, placement writer) builds the caches right before reading the flags.
- `BOARD::IsElementVisible( LAYER_FP_VALUES / LAYER_FP_REFERENCES / LAYER_FP_TEXT )` in
  `GetBoundingBox` is always true: the `.kicad_prl` `visible_items` are not modelled (the C++
  default `GAL_SET::DefaultVisible()` has all three visible). `IsLayerVisible` uses
  `Board::visible_layers()` (every layer, same reason).
- The early return of `BuildCourtyardCaches` (no courtyard shapes) leaves the hashes at their
  defaults on a fresh cache value instead of the previous build's: `GetCourtyard` may rebuild
  more often; results are identical.

### Tests

`footprint::geometry::tests` (10): ports of `test_footprint.cpp` `FootprintCourtyardAndHull`
(move / rotate / flip through the `Board::footprint_*` cascades) and the first half of
`FootprintBoundingBoxWithoutMandatoryFields` (hidden mandatory fields; removing them needs the
non-const `Remove` path), `test_footprint_pad_fixup.cpp` `PthPadExpandsToAllCopperLayers`,
`drc/test_footprint_courtyard_chaining.cpp` `ClosesFromEverySeedShape` and
`ClosesInArbitraryShapeOrder` (Fisher-Yates over `Mt19937( 42 )` instead of libstdc++
`std::shuffle`: other permutations, same property), `drc/test_footprint_courtyard_index.cpp`
`MatchesBruteForceScan`; new: malformed-courtyard flags with an error handler, `cmp_pads` order.

Oracle (`tests/oracle/m20_footprint_golden.sh` -> `tests/golden/m20_footprints.txt`):
`tests/data/m20/m20_footprints.kicad_pcb`, four footprints: A rotated 30 degrees and scaled
1.5 x 0.75 with a closed F.CrtYd outline (segments and an arc, two stroke widths), a Cmts.User
line, a user text; B its flipped copy with an open (malformed) B.CrtYd outline; C fields only
(hidden value: the no-draw-items paths and the hull fallback square); D rotated 90 degrees with
a private-layer line, a circle courtyard and a roundrect pad. `pcb-dump --sections
bbox,footprint,order`. The Rust tests rebuild the footprints from library-frame values (the
footprint parser, PCBIO-06, is pending) and place them with the cascades:
`m20_oracle_footprint_geometry` (feature `dump`) compares all 48 footprint values (`bbox`,
with / without text, `fp_pads_local_bbox`, bounding hull, F/B copper layer boxes, F/B
courtyards, F/B copper hulls, area) as formatted strings: **48 of 48 equal**;
`m20_oracle_save_order` sorts the footprint pad / drawing lists with the ported comparators:
6 of 6 lists in the C++ save order. Not yet verified: footprint zones (fills), text boxes,
barcodes and dimensions in footprints, `HitTest*`, `GetEffectiveShape`, the
`Transform*ToPolySet` functions (no dump section prints them) and the 20-board corpus
comparison through `pcb-dump-rs` (needs PCBIO-06 / 07; `pcb-dump-rs` still prints `no_api`
for `fp_pads_local_bbox` and `area`, now available).

## Package M25a: barcode model without symbol encoding (PCB_BARCODE)

Port of `pcbnew/pcb_barcode.cpp` / `pcb_barcode.h` except the zint encoding itself, all in
`src/pcb_barcode.rs`.

| C++ | Rust | state |
|---|---|---|
| `GetPosition`, `SetPosition`, `GetAngle`, `SetOrientation` / `GetOrientation`, `Move`, `Rotate`, `Flip`, `OnFootprintRescaled` (no-op), `SetLayer` (+ `m_text`), `StyleFromSettings` | `position`, `set_position`, `angle`, `set_orientation` / `orientation`, `move_by`, `rotate`, `flip`, `on_footprint_rescaled`, `set_layer`, `style_from_settings` (+ `position_env` / `angle_env` for mutators) | ported |
| `SetText` / `GetText` / `SetBarcodeText` / `GetShownText`, `SetTextSize` / `GetTextSize`, `SetShowText` / `GetShowText`, `SetIsKnockout`, `SetKind` / `SetBarcodeKind`, `SetErrorCorrection` / `SetBarcodeErrorCorrection` (Micro QR: no H), `KeepSquare`, `SetBarcodeWidth` / `Height`, `SetMarginX` / `Y` (>= 1 mm), `GetPolyShape` / `GetSymbolPoly` / `GetTextPoly` / `GetLastError` | same names snake_case (`poly_shape`, `symbol_poly`, `text_poly`, `last_error` take `ctx`) | ported |
| `AssembleBarcode`, `computeCacheKey`, `ComputeBarcode` (around the encoder), `rescaleSymbolPoly`, `ComputeTextPoly` | `assemble_barcode` (`KeyedCache` keyed by an FNV-1a hash of the C++ key inputs), `compute_barcode`, `rescale_symbol_poly`, `compute_text_poly` | ported; symbol = `encode_symbol` (none yet) |
| the zint call (`ZBarcode_Create` / settings / `ZBarcode_Encode` / `ZBarcode_Buffer_Vector`) | `encode_symbol(kind, text, ecc) -> Option<ModuleMatrix>` (always `None`), `zint_options` (the ported `switch( m_kind )` + ECI choice) | gap (see below) |
| `GetBoundingBox`, `ViewBBox`, `HitTest` (point, rect), `GetBoundingHull`, `TransformShapeToPolygon`, `GetEffectiveShape` | `bounding_box`, `hit_test`, `hit_test_rect`, `bounding_hull`, `transform_shape_to_polygon`, `effective_shape` | ported |
| `GetItemDescription`, `Compare`, `Similarity`, `operator==` | `item_description`, `PcbBarcode::compare(ctx, a, b)`, `similarity`, `eq_item` | ported |
| `GetMsgPanelInfo`, `ViewGetLOD`, `GetMenuImage`, `Serialize` / `Deserialize`, property registration | - | skipped: GUI / API |

Tests (`pcb_barcode::tests`): ports of `qa/tests/pcbnew/test_barcode_load_save.cpp`
`BarcodeWriteRead`, `BarcodePositioningAlignment`, `BarcodeTextVariableExpansion`,
`BarcodeCode39UndefinedVariable` (complete; the symbol comparisons hold trivially without an
encoder), `BarcodeUndefinedVariable` (texts only: `OutlineCount() > 0` needs a QR encoder),
`BarcodeFootprintWriteRead` (through a board file instead of a `.kicad_mod`); oracle tests
`assemble_matches_oracle_without_symbol` (bbox + `GetPolyShape` of 7 barcodes whose symbol is
empty in C++ too: knockout box, back mirror, rotation, rotated / back / non-uniformly scaled
footprints, degenerate size), `frames_match_oracle`, `resave_matches_oracle` (byte level vs
`kicad-cli pcb upgrade --force`), plus mutator / setter / `Compare` / zint-option tests.

Verification (pcb-dump `board --no-init --sections item,misc,bbox` vs pcb-dump-rs, scratch
copies): a crafted board with 11 barcodes (3 footprints: rotated 30, back side -60, transform
rotate 12.5 scale 1.5 x 0.75; board barcodes front / back / copper / zero size): item 494/494,
bbox 25 match, misc 85 match; every mismatch is the encoded symbol (bbox of the 4 non-empty
barcodes and of their footprints) or a dump-side gap (below). `api_kitchen_sink` and
`issue24525` barcodes: all item / misc fields match, `bbox` differs (symbol). io-corpus:
`api_kitchen_sink` now loads and is identical modulo pending pads (barcode blocks identical).

Fields that depend on the encoded symbol (differ for non-empty texts until an encoder lands):
`GetBoundingBox` / `ViewBBox`, `GetPolyShape`, `GetSymbolPoly`, `GetTextPoly` (empty whenever
the symbol is), `GetLastError` (zint's `errtxt`), `GetEffectiveShape`,
`TransformShapeToPolygon`, `HitTest`, `GetBoundingHull`, and through them the footprint
bounding boxes / hulls and everything plotted or DRC-checked from a barcode.

What a faithful encoder needs (zint 2.15.0 is bundled in `thirdparty/zint`, BSD-3-Clause):
`ZBarcode_Create` defaults (`scale` 1, `option_1` -1, no border / whitespace, default heights),
`input_mode = UNICODE_MODE`, `show_hrt = 0`, then per kind:
* `CODE_39` -> `BARCODE_CODE39` (8): `code.c` `code39()` (no check digit, `option_2` 0);
* `CODE_128` -> `BARCODE_CODE128` (20): `code128.c` (automatic code-set selection, as zint);
* `QR_CODE` -> `BARCODE_QRCODE` (58), `option_1` = ECC 1..4: `qr.c` (mode optimisation, mask
  selection, version choice), `reedsol.c`; non-ASCII text sets `eci = 26` (`eci.c`, UTF-8);
* `MICRO_QR_CODE` -> `BARCODE_MICROQR` (97), `option_1` = ECC 1..3: `qr.c` micro path;
* `DATA_MATRIX` -> `BARCODE_DATAMATRIX` (71): `dmatrix.c` (ECC200, automatic size and
  encodation), `reedsol.c`; non-ASCII text sets `eci = 26`.
Common: `library.c` (`ZBarcode_Encode` input checks and `errtxt` texts), `common.c`, and
`vector.c` `ZBarcode_Buffer_Vector( symbol, 0 )` (the rectangle list with quiet zones and the
float coordinates `ComputeBarcode` rounds with `KiROUND( rect->x * scale )`). The encoder
fills `ModuleMatrix { scale, rectangles, hexagons }` in zint's list order; everything after
that is already ported.

Deviations:
- No symbol encoder: `encode_symbol` returns `None`, so a non-empty barcode assembles like a C++
  barcode zint rejects (empty symbol and text polygon; with knockout the box around the
  *origin* inflated by the margins, mirrored / rotated about the position, as in C++);
  `last_error` is `ENCODER_NOT_AVAILABLE` (the international-characters text for non-ASCII).
- Cache: rebuilt lazily when the C++ key changes (and cleared on every `modify_*` and by the
  setters that call `AssembleBarcode()` in C++). C++ can keep a stale cache when a non-key
  input changes (board max error, user layer type, footprint scale); the port rebuilds from
  the current state, i.e. like a freshly loaded C++ board.
- `m_text`'s parent is the barcode in C++; the inline `PcbText` usually has `ParentRef::None`,
  so the text is viewed with the barcode's parent (same `GetParentFootprint` / `GetBoard`).
- `StyleFromSettings`: the trait method has no `MutEnv`, the text size is set without the
  parent frame (as `PcbText::style_from_settings`).

Known gaps / change requests (other packages' files):
- Writer (`io/writer/misc_items.rs` `format_barcode`) writes the stored lib height for
  `(text_height ..)`; C++ writes `GetTextSize()` = `m_text.GetTextHeight()`, the virtual
  `GetTextSize().y` in the board frame: use `b.text_size(&ctx)`. Differs in non-uniformly
  scaled footprints (1.6 vs 1.2 mm in the test board).
- Parser (`io/parser/misc_items.rs`): `SetTextSize` computes the pen width from
  `m_text.GetTextHeight()` (board frame), the port from the lib height; call
  `barcode.set_text_size(&env, h)`, `barcode.set_kind`, `barcode.set_error_correction`,
  `barcode.set_orientation(&env, deg)` instead of the inlined copies.
- `board/compare.rs` `pcb_barcode_compare`: replace the stub with
  `PcbBarcode::compare(ctx, a, b)` (needs the two `AnyItem::Barcode` values and `ctx`).
- `dump/defaults.rs`: `barcode.show_text` -> `b.show_text()`, `barcode.text_size` ->
  `b.text_size(ctx)`, `barcode.poly` / `symbol_poly` / `text_poly` / `last_error` ->
  `b.poly_shape(ctx)` etc.; `barcode.kind` prints `CODE39` / `CODE128` where C++ prints the
  ENUM_MAP names `CODE_39` / `CODE_128`.

## Package PCBIO-06: board file footprints (parse + write)

Files: `src/io/parser/footprint.rs`, `src/io/writer/footprint.rs`, `tests/pcbio06_footprints.rs`,
`tests/data/pcbio06/`, `tests/golden/pcbio06/`, `tests/oracle/pcbio06_golden.sh`,
`tests/oracle/pcbio06_strip.py`.

### C++ to Rust map

| C++ (`pcb_io_kicad_sexpr_parser.cpp` P:, `pcb_io_kicad_sexpr.cpp` W:) | Rust | State |
|---|---|---|
| `parseFOOTPRINT` (P:6234, `FUTURE_FORMAT_ERROR` conversion) | `parser/footprint.rs` `parse_footprint` | ported |
| `parseFOOTPRINT_unchecked` (P:6250-7254: every token incl. `version` / `generator*` with the version check, `locked`, `placed`, `layer`, `tedit`, `tstamp` / `uuid`, `at`, `transform`, `descr`, `tags`, `height`, `property` with the < 20230620 legacy keys, `ki_fp_filters`, `Footprint`, existing / new fields and `StyleFromSettings`, `path`, `sheetname`, `sheetfile`, `units`, `autoplace_cost*`, `private_layers` (< 20220427), `net_tie_pad_groups`, `duplicate_pad_numbers_are_jumpers`, `jumper_pad_groups`, the four overrides (<= 20240201 zero = unset), `zone_connect`, `thermal_*`, `attr`, `fp_text` (legacy reference / value overwrite, hidden user text as field), `fp_text_box`, `table`, `fp_*` shapes, `image`, `barcode`, `dimension`, `pad`, `model` (file and extruded), `zone` (< 20260616 board frame), `group`, `constraint`, `point` (< 20260616 board frame), `embedded_fonts`, `embedded_files`, `component_classes`, `variant`, `custom_property`; post-processing: `FixUpPadsForBoard`, < 20200826 attributes, `LEGACY_NET_TIES` keyword groups, `SetFPID`) | `parse_footprint_unchecked`, `parse_footprint_property`, `parse_footprint_units`, `parse_extruded_body`, `add_footprint_zone` | ported |
| `parse3DModel` (P:954, legacy inch `at` with the `25.4f` float factor) | `parse_3d_model` | ported |
| `parseFootprintStackup` (P:7257) | `parse_footprint_stackup` | ported |
| `parseFootprintVariant` (P:594) | `parse_footprint_variant` | ported |
| `formatInitialComments` (W:1452) | `writer/footprint.rs` `format_initial_comments` | ported |
| `format( const FOOTPRINT*, bool aInLibFile )` (W:1466-1884: header, version for stand-alone footprints, flags, transform, fields as properties, component classes, filters, path, sheet, units, overrides, attributes, stackup, private layers, net ties, jumper groups, children in `std::set` order, variants (only values differing from the base), embedded fonts / files, extruded bodies, 3D models, custom properties) | `format_footprint` | ported; child order through `footprint_cmp_{drawings,pads,zones}` (M20), `pcb_point_cmp_points`, `board_item_ptr_cmp` |

### Tests and verification

- Byte level, `tests/pcbio06_footprints.rs` `golden_footprints` (default run, strict: no staging):
  23 boards in `tests/data/pcbio06`: two hand-made (`handmade_current`: every footprint token of
  the current format incl. scale transform, stackup, units, jumper / net-tie groups, variants,
  extruded bodies, 3D models with opacity / legacy `at`, component classes, custom properties,
  footprint zone and points, a property without effects; `handmade_v6`: a 20211014 file with
  bare `locked`, `tedit` / `tstamp`, `(at x y angle)`, the legacy `Sheetfile` / `Sheetname` /
  `ki_description` / `ki_keywords` properties, `Footprint` property, `autoplace_cost*`,
  `solder_paste_ratio`, zero overrides, `thermal_*`, Edge.Cuts in private layers, a hidden
  `fp_text user`, a board-frame footprint zone, a hidden inch-offset model, a footprint without
  attributes) and 21 corpus boards of the versions 20200119 .. 20261022 reduced by
  `pcbio06_strip.py` (no pads, at most one drawing / zone per footprint, so no footprint child
  sort is needed), re-saved by the reference `kicad-cli pcb upgrade --force`
  (`pcbio06_golden.sh`). Parse + format of every input reproduces the oracle's re-save O1, and
  parse + format of O1 reproduces O2 (O1 for fixed points): 46 of 46 re-saves byte-identical
  (UUIDs the C++ draws at random for UUID-less legacy items masked with
  `kicad_qa::uuid_canon`; footprint blocks ordered by such UUIDs compared sorted).
- Item level: `pcb-dump board --no-init --sections item,footprint,text,shape,zone` vs
  `pcb-dump-rs` (`tools/io-corpus/run.sh --dump`) on the 23 data boards and
  `tests/data/m19/m19_cascade.kicad_pcb`: 0 mismatches in footprint members and in the item /
  text / shape values of their children, except (a) footprint zones on 5 boards: `outline` /
  `hatch_lines` frames swapped by the Rust dump (`dump/items.rs` prints `board_outline` where
  the C++ prints `Outline()`, and the raw lib-frame hatch lines where the C++ prints
  `GetHatchLines()`; change requested), (b) `strip_microwave` `lib.poly` of two `fp_poly`
  (the Rust library polygon keeps a duplicated closing vertex, `parsePCB_SHAPE`, PCBIO-03;
  output bytes unaffected), (c) pad-derived values (`m19_cascade` has pads: PCBIO-07).
  On full corpus boards the remaining differences are pad-derived only (pads, hulls, areas,
  `#ordinal` keys shifted by the missing pads).
- Corpus scoreboard (`tools/io-corpus/run.sh`): before this package 92 identical / 198
  identical-modulo-pending / 78 differs / 2 model-pending with `footprint (PCBIO-06)` pending on
  276 boards (first full-text difference `footprint` on all of them); after (370 compared):
  98 identical / 21 identical-modulo-pending / 4 differs (first differences in `arc`,
  `segment`, `zone > priority`: not footprint code) / 247 model-pending (245 stop at the
  `FOOTPRINT::cmp_drawings` stub of M20, landing in parallel; 2 at `PCB_BARCODE::SetPosition`),
  no footprint construct pending (pads, PCBIO-07, on 257 boards), no parse error. Rerun once
  M20 / PCBIO-07 land.
- Ported C++ tests: `test_footprint_load_save.cpp` `FootprintLoadSave`,
  `FootprintFieldLockRoundTrip`, `FootprintUserFieldKeepsAutoThickness`,
  `LegacyFootprintPropertyTakesBoardDefaults` (the QA-board round trips run their save half
  once the footprint child sort is ported; the load halves run now);
  `test_extruded_body_load_save.cpp` `FullRoundTrip`, `DefaultsRoundTrip`,
  `NoExtrudedBodyRoundTrip` as board round trips (the `.kicad_mod` path is PCBIO-14), plus
  `extruded_body_without_height_is_not_saved`, `parsed_footprints_are_attached`. Not ported:
  `test_footprint_rescue_layer.cpp` (`.kicad_mod` files, PCBIO-14, and the
  `LAYER_UTILS::GetOrphanedFootprintLayers` dialog helper).

### Deviations / notes

- `(zone_connect N)` with N outside -1..3: the C++ cast keeps the integer (written back as
  is); the Rust `ZoneConnection` cannot hold it, it reads as `INHERITED` (not written).
- `SetTransientComponentClassNames` (read only by the paste tool) has no counterpart: the
  Rust footprint keeps no transient class names.
- `FOOTPRINT::Add` of a point: `PCB_POINT::SetParent` recomputes the board position; the
  parser calls `on_footprint_transformed` after `footprint_add` for that (idempotent if the
  model's `footprint_add` does it one day).
- `(model (type extruded) ...)` edits a copy of the footprint's first body and stores it back
  with `modify_body3d` (the C++ edits it in place; same final state, UUID included).
- `fmt::format( "(opacity {:.4f})" )` is ported as Rust `{:.4}` (both correctly rounded).

### Known gaps

- The footprint child sort (`FOOTPRINT::cmp_drawings` / `cmp_pads` / `cmp_zones`, M20) was a
  `port_pending!` stub when this package was verified: footprints with two or more drawings,
  pads or zones cannot be written until it lands; the goldens avoid that case.
- Pads (`parsePAD` / `format( PAD* )`, PCBIO-07) are staging stubs: footprints lose their pads
  with `allow_pending`, and the `LEGACY_NET_TIES` pad group of a `net tie` footprint is empty.

## Integration step 2 (INT-2a): kernel change requests

Design revision 2.3 (`docs/design/board-model.md`); the triage of all change requests is
`docs/design/integration-2.md`.

| C++ | Rust | state |
|---|---|---|
| `EDA_SHAPE::GetCornersInSequence` (reads the virtual `GetWidth()` through `getBoundingBox()`) | `EdaShapeItem::corners_in_sequence(ctx, angle)`, `eda_shape::corners_in_sequence` | ported exactly; the M7 deviation (stored width) and the M11 private copies in `pcb_textbox.rs` are gone |
| `BOARD_ITEM::StyleFromSettings` family | `BoardItemMut::style_from_settings(env, settings, check_side)`; `PcbText`, `PcbField`, `PcbDimension`, `PcbShape`, `PcbTextbox`, `PcbTablecell` | ported exactly (frame-converting `SetTextSize` / `SetTextThickness`, `GetParentFootprint()`, `BOARD::IsBackLayer`); supersedes the M10 / M11 / M12 "no MutEnv" deviations. `PcbBarcode` keeps its M25a body (shim ignores `env`) |
| `BOARD_ITEM::IsSideSpecific` in `PCB_TEXT::Flip`, `PCB_TEXTBOX::Flip`, `PCB_DIMENSION_BASE::Mirror` | `MutEnv::is_side_specific`, `LayerEnv::{layer_types, layer_type, is_front_layer, is_back_layer}` | ported exactly (user layers typed `LT_FRONT` / `LT_BACK`); supersedes the "layer-set test only" deviations of M10 / M11 / M12 |
| `BOARD_CONNECTED_ITEM::SetNet`, `ZONE::SetNet`, `ZONE::SetNetCode` | `BoardConnectedItem::set_net`, `board_connected_item::set_net`, `Zone::set_net`, `Board::set_item_netinfo`, the zone clamp in `Board::set_item_net` | ported; `BOARD::Remove( net )` and `MapNets` dispatch the virtual |
| `this` of a detached value in `BOARD_CONNECTED_ITEM::GetOwnClearance` | `AsAnyItem`, `board_item::with_this` | ported (the `port_pending!` for a detached item is gone) |
| `CONNECTIVITY_DATA::GetNetItems` as a hook, `NETINFO_ITEM::GetBoundingBox` | `ConnectivityQuery::net_items`, `NetinfoItem::bounding_box` | ported; pointer order -> `ItemId` order (3.1). Not compared against the reference (GUI only, see below) |
| `DRC_CONSTRAINT::GetName`, `GetRuleName`, `Merge` | `DrcConstraint::{name, rule_name, merge}` (`drc_api.rs`) | ported (moved from `kicad-drc` / the private copies of `pcb_track`) |
| `EDA_ITEM_FLAGS STARTPOINT`, `ENDPOINT` | `ItemFlags::{STARTPOINT, ENDPOINT}` | ported |
| `PAD::IsFreePad` | `Pad::is_free_pad(ctx)` (`is_free_pad_ctx`: forwarding wrapper, to be removed) | ported |
| `PCB_TRACK` family `operator==`, `Similarity`, copy constructor on a `PCB_TRACK*` | `Track::{eq_item, similarity, detached_copy}` | ported |
| `PCB_VIA::ValidateViaParameters` in `ValidateDesignRules` | `board_design_settings.rs` calls `PcbVia::validate_via_parameters` | ported (private copy removed) |
| `BOARD_DESIGN_SETTINGS::operator==` | `impl PartialEq for BoardDesignSettings` | ported without the BOM members (not modelled) |
| `PCB_DRILL_MAP::GetSymbolExtent`, `DRILL_SYMBOL_PROFILE::GetSymbolWidth` | `PcbDrillMap::symbol_extent(ctx)`, `BoardDesignSettings::drill_symbol_width` | ported |
| `FOOTPRINT::ResolveTextVar`, `GetFieldValueForVariant`, `FindPadByUuid`, `GetPad`, `DeleteVariant`, `RenameVariant`, `GetNextFieldOrdinal` call sites | `pcb_text.rs`, `pcb_field.rs` (`CmpNoCase` for the default variant), `board/text_vars.rs`, `board/queries.rs`, `dump/defaults.rs` | wired to the M19 methods |
| `PCB_TABLE::Compare` in `BOARD::cmp_drawings` | `board/compare.rs` -> `PcbTable::compare` | wired |
| `format( const PCB_TRACK* )` for a generator template | `io/writer/tracks.rs` `format_track_item(&Track)`, `io/writer/generator.rs` `format_template_item` | ported; supersedes the PCBIO-13 gap "template via not written" |
| pcb-dump getters | `dump/items.rs` (footprint side, likely attribute, exclude from sim, pad count, through-hole pads, component class, area), `dump/board.rs` (`fp_pads_local_bbox`, `locked_flag` through the non-virtual `BOARD_ITEM::IsLocked`), `dump/defaults.rs` (table end / border style / separators style) | wired |

Verification:

* `tests/oracle/int2a_oracle.cpp` (driver linking the reference pcbnew, `int2a_golden.sh`),
  golden `tests/golden/int2a_mutators.txt`, replay `integration2_tests::int2a_matches_oracle`
  (feature `dump`): 354 values, all equal. Cases: `IsSideSpecific` / `Flip` / `Mirror` /
  `StyleFromSettings` of text, text box and aligned dimension on eight layers of a board whose
  `User.1` is `LT_FRONT` and `User.2` is `LT_BACK`; `StyleFromSettings` of text, text box and
  the reference field inside a footprint at (10 mm, 20 mm), rotated 30 degrees, scaled
  (1.5, 0.75); `SetNetCode` / `SetNet` on a copper-thieving and a solid zone, `SetNetCode` of
  a track (known and unknown net), `BOARD::Remove( net )`; `GetSymbolExtent` with and without
  a board; `BOARD_DESIGN_SETTINGS::operator==` after eight single-member edits.
* Ported C++ tests: `ThievingZone_RejectsNetCodeViaBasePointer`,
  `ThievingZone_RejectsSetNetViaBasePointer` (`test_board_connected_items.cpp`) as
  `integration2_tests::thieving_zone_rejects_nets_via_base_pointer`.
* `corners_in_sequence(ctx)`: `pcb_table::oracle_tests::m11_matches_oracle` (unchanged golden).
* Generator template via: `tests/pcbio13_generators.rs` now compares whole blocks (the
  stripped-hash fallback is removed): 3 corpus boards and 1 hand-made board with templates
  are byte-identical to the reference re-save.
* `PcbShape::connection_points` wired into `kicad-connectivity`: its `cn01` / `cn04` goldens
  pass unchanged.
* Defaults scoreboard (`pcb-dump defaults` vs `pcb-dump-rs defaults`, `compare.py`): 3533 ->
  3558 match, 149 -> 124 pending, 0 mismatch. Board dumps (`--no-init`) of issue24853,
  keepout_no_clearance, issue20249/dimension, issue24442, tuning_generators_load_save and
  zone_thieving_track_bisection: identical scoreboards before and after (998 / 1028 / 1024 /
  1068 / 2754 / 899 match, 0 mismatch).

Not verified against the reference: `NetinfoItem::bounding_box` (needs a built connectivity;
no pcb-dump line); `Track::detached_copy` of a via (the via body is M17's, verified there).

Known gaps (owners in `integration-2.md`): generator subclass arms of the polymorphic
mutators and the via stitch default template; `Update()` after text setters of a dimension;
dynamic component classes; `PcbBarcode::style_from_settings` without `env`;
`Pad::is_free_pad_ctx` still present; `PCB_DRILL_MAP::GetBoundingBox` / `HitTest` and
`BOARD::GetTrackLength` pending their tier-2 dependencies.

## Package PCBIO-07: board file pads and padstacks (parse + write)

| C++ | Rust | State |
|---|---|---|
| `parsePAD` (P:7320-8030), every token and version branch (`die_delay` length scale `<= 20250926`, zero margins = inherit `<= 20240201`, custom-pad circle-anchor spoke angle 90 `<= 20211014`, overbar net names `< 20210606`), legacy `(net N "name")` with name check / orphaning, zero-size clamp + warning, `CanHaveNumber` | `io/parser/pad.rs` `parse_pad`, `parse_pad_net`, `parse_pad_extra_drill`, `parse_chamfer_list` | ported |
| `parsePAD_primitives`, `parsePAD_option`, `parsePostMachining`, `parsePadstack` (FRONT_INNER_BACK / CUSTOM layers) | `io/parser/pad.rs` | ported |
| `PAD::AddPrimitive` (pad.cpp: lib override + `rebakePrimitiveToFootprint`) | `io/parser/pad.rs` `pad_add_primitive` (private, see known gaps) | ported for the parser |
| `PAD::SetRemoveUnconnected` / `SetKeepTopBottom` (inline, pad.h) | `io/parser/pad.rs` free functions over `set_unconnected_layer_mode` | ported |
| `format( const PAD* )` (W:1972-2549) with `shapeName`, `formatCornerProperties`, `anchorShape`, `formatPrimitives`, `formatPadLayer` | `io/writer/pad.rs` | ported |
| `formatPostMachining` lambda | `io/writer/tracks.rs` `format_post_machining`, made `pub(crate)` (one-word edit, shared with the via writer) | ported (PCBIO-03) |

Ported tests (`io/writer/pad.rs` tests): `test_pad_roundtrip.cpp` `CopperOnlyPad`;
`test_pad_tenting.cpp` `DefaultTentingNotSerialized`, `ExplicitTentingSerialized`,
`NpthDefaultTentingNotSerialized`; `test_custom_pad_primitive_roundtrip.cpp`
`BezierAndPolyOnTransformedFootprint`, `SimplePadPrimitivesOnTransformedFootprint`,
`ScaledFootprintRoundTripStable`, `PadPositionAndOrientationRoundTripStable`,
`PadDeltaRoundTripStable`, `PadOffsetRoundTripStable`, `RotatedPadSizeRoundTripStable`.
(`FootprintPointRoundTripStable` and `PadSizeScalesAlongFootprintAxis...` test points / the
pad model, not the pad format; `test_footprint_pad_fixup.cpp` is in M20, the
`test_backdrill_properties.cpp` legacy read cases in M13; `test_complex_padstack_export.cpp`
tests the Gencad / Specctra / Hyperlynx exporters, out of scope.)

Oracle (`tests/pcbio07_pads.rs`, goldens `tests/oracle/pcbio07_golden.sh` with
`pcbio07_strip.py`): 4 hand-made boards (every pad token, all shapes, custom primitives incl.
`gr_bbox` / `gr_vector`, both complex padstack modes, backdrill / tertiary / post machining,
teardrops, tenting both syntaxes, rotated / flipped / non-uniformly scaled footprints, versions
20210108, 20240108, 20250926, 20261031) and 19 corpus boards reduced to footprints with pads:
46 re-saves (input -> O1 and O1 -> O2) byte-identical with `kicad-cli pcb upgrade --force`,
including the oracle's own size drift on transformed footprints. Corpus (`tools/io-corpus/run.sh`):
before 98 identical + 21 identical-modulo-pending of 370, 257 boards waiting for pads; after
359 identical (all 287 fixed points), 11 differ (zone net / priority after the oracle's
post-load steps, outline-font render caches; none in pad code), no pending construct.
Item level (`run.sh --dump --sections item,pad[,bbox]`, 61 boards incl. padstacks*, custom_pads,
m14 / m15 fixtures, one per format version from 20200119 to 20260101): zero mismatches except
the back-side mask / paste values listed under known gaps.

Deviations:
* `(zone_connect N)` with a value outside the `ZONE_CONNECTION` enum reads as `INHERITED` (the
  C++ cast keeps the integer; same rule as PCBIO-06).

Known gaps:
* Pad primitives keep `ParentRef::None` after the parse (C++ `AddPrimitive` sets the pad as
  parent; the local pad has no handle until `footprint_add`). The writer reads the primitives'
  `lib` members directly, which is what `GetLibrary*()` returns for a pad parent, so the output
  is exact; context-free model getters (`lib_bezier_c1`, `lib_poly_shape`) and
  `rebake_from_lib` / `sync_lib_coords` of such a primitive behave as for a detached shape.
  Needs `footprint_add` (or the T_pad case of `parseFOOTPRINT`) to re-parent them (change
  request); `PAD::AddPrimitive` itself belongs in the pad model.
* `Pad::set_local_solder_mask_margin` / `set_local_solder_paste_margin` /
  `set_local_solder_paste_margin_ratio` (M14) set the front outer layers only; the C++ sets
  F and B. Item-level mismatches on the back side (`padstack.back_outer.*`,
  `solder_mask_expansion.B.Mask`, `solder_paste_margin.B.Paste`, `shape.B.Mask`) on every board
  with pad margin overrides; the written file is unaffected (the writer reads the front).

## Package CC-1: component class synchronisation (dynamic assignment rules)

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `COMPONENT_CLASS_MANAGER::SyncDynamicComponentClassAssignments` | `ComponentClassManager::sync_dynamic_component_class_assignments` | ported |
| `COMPONENT_CLASS_MANAGER::CompileAssignmentRule` | `ComponentClassManager::compile_assignment_rule` + the `ComponentClassRuleCompiler` hook (kicad-drc `component_class_rules`) | ported |
| `COMPONENT_CLASS_ASSIGNMENT_RULE` (`GetComponentClass`, `Matches`) | `ComponentClassAssignmentRule` (condition: `ComponentClassRuleCondition` hook, implemented by kicad-drc's `DrcRuleCondition`) | ported |
| `COMPONENT_CLASS_ASSIGNMENT_DATA` (`AddCondition`, `GetConditionName`, `GetConditionType`, `GetAssignmentInDRCLanguage`) | `component_class.rs` `ComponentClassAssignmentData` | ported |
| `COMPONENT_CLASS_SETTINGS` (parameters, `loadAssignment`, `saveAssignment`, nested load) | `ComponentClassSettings::load_from_parent` | ported (load; the getters exist for `Store`, nothing saves the project yet) |
| `BOARD::SynchronizeComponentClasses` | `Board::synchronize_component_classes{,_for_sheets}` (project/board_project.rs) | ported |
| `FOOTPRINT::GetComponentClass` dynamic path | `Footprint::component_class` (footprint/mod.rs) | ported (rules matched with the compiled conditions) |
| `wxSplit( s, ',' )` (escape `\\`), `wxString::Trim`, `ToInt` | `wx_split`, `wx_trim_both`, `wx_to_int` (private) | ported; `wxSplit`'s escaped-escape handling measured on the reference build |

### Tests and verification

* `tests/oracle/cc1_oracle.cpp` (+ `build_cc1_oracle.sh`, cases `cc1_cases.txt`; links
  libkicommon.so): golden `tests/golden/cc1_component_class.txt` (579 lines):
  `GetAssignmentInDRCLanguage` of 399 hand-made assignments (every condition type, trimming,
  rotations incl. int / long overflow, `wxSplit` on all 363 strings of up to 5 characters over
  `a`, `\\`, `,`), and the `COMPONENT_CLASS_SETTINGS` loaded by `PROJECT_FILE::LoadFromFile`
  from 16 synthetic project texts (wrong types, exceptions part-way, null / array / primitive
  `conditions`, key order, NUL characters) and the 5 corpus projects with assignments. Replay
  `tests/cc1_oracle.rs`: identical.
* `component_class::tests::sync_without_assignments_needs_no_compiler`, `sync_dynamic_assignments`
  (usage-context transitions, sheet classes, all-or-nothing install).
* Board level (kicad-session `tests/component_classes.rs`): see kicad-session PORT_STATUS.md.

### Deviations / notes

* The settings are read from the bound project document (`BoardProject::document`) when
  `SynchronizeComponentClasses` runs, not kept as a member of the project file (the project
  reader has no `COMPONENT_CLASS_SETTINGS` member; same data at load time).
* Without a project the default settings are used (C++ dereferences a null project).
* The rule compiler is a hook on the manager (`rule_compiler`, kicad-drc installs it): compiling a
  rule without one panics with a clear message (programming error; the loaders install it).
  With no assignments and sheet classes off no hook is needed.
* The C++ `unordered_set`s of `SyncDynamicComponentClassAssignments` are `BTreeSet`s: their order
  only decides the order of `m_assignmentRules`, on which no result depends.
* `loadAssignment` where nlohmann has undefined behaviour (`operator[]` of a missing key on a
  const object) is treated like a thrown exception.
* `Footprint::component_class(&self)` still computes against a copy of the manager (M24 note);
  the rule conditions are evaluated without holding the footprint's cache lock.

### Known gaps

* Matching uses the DRC-22 expression functions: `getField`, `memberOfSheet`, `intersectsArea`
  (and the others) are still "not yet ported" in kicad-drc, so rules using them do not match yet
  (5 of 14 footprints of `component_classes.kicad_pcb`; the corresponding tests are `#[ignore]`d).


## Package FIX-PAD-1: back-side pad margins, pad primitive parents, PAD::AddPrimitive in the model

| C++ | Rust | State |
|---|---|---|
| `PAD::SetLocalSolderMaskMargin` / `SetLocalSolderPasteMargin` / `SetLocalSolderPasteMarginRatio` (pad.h: `F_Mask` + `B_Mask`, `F_Paste` + `B_Paste`) | `pad/derive.rs` setters | ported (both outer sides; getters read the `F_Cu` default = front) |
| `PAD::AddPrimitive`, `rebakePrimitiveToFootprint` | `Pad::add_primitive( env, layer, shape )`, private `rebake_primitive_to_footprint` | ported; the parser's `pad_add_primitive` (io/parser/pad.rs) now calls it |
| `PAD::AddPrimitivePoly` (both overloads), `PAD::DeletePrimitivesList` | `add_primitive_poly`, `add_primitive_poly_points`, `delete_primitives_list` | ported |
| `SetParent( this )` of `AddPrimitive` / the `PADSTACK` copy for a pad that gets its handle | `Pad::reparent_primitives`, called by `Board::footprint_add` (Pad arm) | ported |

Audit of the other two-sided accessors (pad.h / pad.cpp / padstack.cpp): `PADSTACK::SolderMaskMargin`
& co. dispatch front / back by layer side; `IsTented` / `IsCovered` / `IsPlugged` by side;
capping / filling are single drill props; zone connection / thermal / clearance setters write
`CopperLayer( F_Cu )` as in C++; `FlipLayers` swaps the mask props and post machining; copies go
through `assign_from` (re-parenting). No other defect found.

Readers that depended on the parent: `lib_bezier_c1` / `c2`, `lib_poly_shape` now return the
lib members (C++ `GetLibrary*` for a pad parent); `rebake_from_lib` is a no-op and
`sync_lib_coords` uses the pad branch; `GetMaxError()` of a primitive (bezier / rounded
rectangle / ellipse effective shapes of custom pads) is now the board's instead of
`ARC_HIGH_DEF`.

Verification: `tests/fixpad1_pads.rs` (feature `dump`), golden `tests/golden/fixpad1/` from
`tests/oracle/fixpad1_golden.sh` (reference pcb-dump, QA load path, hashed): back-side SMD and
PTH pads with mask / paste / ratio overrides, custom pad (bezier, rounded rect, arc) in a
rotated, non-uniformly scaled footprint, project `max_error` 0.05 mm: all item / pad / bbox lines
match except `own_clearance.*` (DRC-engine values of the C++ QA loader, a session step).
`run.sh --dump --sections item,pad,bbox` on the PCBIO-07 boards and the corpus boards with
back-side pads, custom pads or margin overrides: see the package report.

Known gaps:
* `PCB_SHAPE::Flip` / `syncLibCoords` of a pad primitive use the pad's `GetPrimitiveLibScale()`
  in C++; `MutEnv` does not carry it, so the Rust treats it as (1, 1) (the M9 deviation in
  `pcb_shape`, unchanged; only matters for primitives of pads in scaled footprints that are
  flipped or edited after load).

## Package GLUE-2: second wiring pass (change requests since GLUE-1)

Triage of every request: `docs/design/integration-2.md`, section "GLUE-2".

| C++ | Rust | state |
|---|---|---|
| `parsePCB_BARCODE`: `SetOrientation`, `SetText`, `SetTextSize`, `SetKind`, `SetErrorCorrection`, `SetShowText`, `SetIsKnockout` | `io/parser/misc_items.rs` calls the M25a setters | wired; the inlined copies (and the parser's `barcode_set_orientation`) are gone. The text pen width now uses the board-frame height (`SetTextSize` reads `m_text.GetTextHeight()`) |
| `format( PCB_BARCODE* )` `(text_height ..)` = `GetTextSize()` | `io/writer/misc_items.rs` -> `PcbBarcode::text_size(ctx)` | wired (board frame) |
| `PCB_BARCODE::Compare` in `BOARD::cmp_drawings` | `board/compare.rs` -> `PcbBarcode::compare` | wired (the `port_pending!` is gone) |
| `PCB_BARCODE::StyleFromSettings` | `PcbBarcode::style_from_settings(env, ..)` | the trait's `env` reaches `SetTextSize`; supersedes the M25a / INT-2a "shim ignores env" note |
| `BOARD_CONNECTED_ITEM::GetNetClassName` | `board_connected_item::net_class_name` -> `Netclass::get_name()` | fixed (was the composite's `name` field, "Effective for net: .."); only caller `FOOTPRINT::ResolveTextVar` `NET_CLASS(n)` |
| `BOARD::IsLayerVisible`, `BOARD::GetVisibleLayers` | `Board::is_layer_visible`, `Board::visible_layers` | ported; `visible_layers` now reads the project's `m_VisibleLayers` (was always all layers) |
| `FOOTPRINT::Add` -> `PCB_POINT::SetParent` (`recomputePosition`) | `Board::footprint_add` (Point arm) | ported; the parser's `modify_point(.. on_footprint_transformed)` work-around is gone |
| `PCB_VIA_STACK::IsSpanValid` (static) | `ViaStackData::is_span_valid(Option<&Board>, start, end)` | ported (kicad-drc keeps its local copy until its owner switches) |
| `ZONE` fill of a detached zone | `Zone::fill_mut` | added (DRC-27) |
| `PCB_TYPE_COLLECTOR` walk of `BuildBoardPolygonOutlines` | `convert_shape_list_to_polygon::collect_board_shapes` | now `pub` (DRC-23) |
| `FOOTPRINT::GetNextFieldOrdinal` in `parseFOOTPRINT` text conversion | `io/parser/text.rs` -> `Footprint::next_field_ordinal` | wired (local copy removed) |
| CLEAN-1 switches | `netclass.rs` / kicad-drc `implicit_condition.rs` -> `kicad_core::string_utils::wx_string_matches`; `eda_text.rs` -> `kicad_font::FontRef` (stand-in removed from `foundation_stub.rs`); `pcb_reference_image.rs` -> `kicad_geom::mirror_point` (wraps) | done |
| `Pad::is_free_pad_ctx` | removed; kicad-drc `providers/solder_mask.rs` calls `Pad::is_free_pad(ctx)` | done |
| kicad-plot `GERBER_PLOTTER::SetViewport` `wxASSERT( aMirror == false )` | `debug_assert!` removed (wxASSERT only logs) | done |
| pcb-dump: zone `outline` (`*Outline()`), `hatch_lines` (`GetHatchLines()`), barcode `show_text`, `text_size`, `kind` (magic_enum `CODE_39` ..), `poly` / `symbol_poly` / `text_poly` / `last_error` | `dump/items.rs`, `dump/defaults.rs` | wired; the polygons only for an empty text (otherwise pending on the zint encoder, ZINT-1) |
| pcb-dump with a caller context | `dump::dump_board_ctx(ctx, opts)`; `ds.biggest_clearance` uses `ctx.rules` | added (for kicad-session's `pcb-dump-session`) |

Tests updated (stale since footprints / pads landed): `io::parser::tests::staging_stubs` and
`io::writer::tests::staging_stubs` (the remaining stub is PCBIO-14's `kicad_footprint_lib`);
`netclass_embedded::oracle_corpus_sections` (footprint files merged as `format( BOARD* )` does;
the reference re-save runs `BuildListOfNets()`, which clears the resolved terminal pads, so the
replay calls `build_list_of_nets()`: issue25065); `pcbio12_misc_items::oracle_corpus_blocks`
compares api_kitchen_sink without its drill chart: the reference's '0.30 mm' cells come from
`RefreshGeneratedTables` in `board_loader.cpp` (the stored cells are '0.30'), not from a
formatting bug.

Verification:

* `tests/glue2.rs` (golden `tests/golden/glue2/scaled_fp_items.kicad_pcb`, reference
  `kicad-cli pcb upgrade --force` of `tests/data/glue2/scaled_fp_items.kicad_pcb`: two barcodes
  and a point in a footprint rotated 30 degrees, scaled (1.5, 0.75)): footprint block
  byte-identical (UUIDs of the reference's new Datasheet / Description fields masked);
  barcode `text_size` / `show_text` / kind / ECC and the point position equal `pcb-dump`;
  `${U1:NET_CLASS(n)}` with a project assigning two classes gives `L,M,Default|M,Default` as
  `pcb-dump` (`tests/data/glue2/net_class_text.kicad_{pcb,pro}`).
* `pcb-dump --no-init` vs `pcb-dump-rs` on the same board: 919 match, 9 mismatch, all from the
  QR barcode's empty symbol (no zint encoder, ZINT-1). The issue24525
  `ComputeBoundingBox( false )` difference has the same cause (its QR barcode's bbox is
  `0,0,0,0` without an encoder); nothing to fix in the bbox code.
* Zone section (`--sections zone`) of the 4 PCBIO-06 boards with zones: 0 mismatches.
* Defaults scoreboard: 3558 / 124 / 0 (INT-2a) -> 3564 match, 118 pending, 0 mismatch.
* io-corpus scoreboard (`tools/io-corpus/run.sh`): 359 identical / 11 differs / 5 skipped
  before and after.
* `(zone_connect 7)` in a footprint: the reference re-save writes `(zone_connect 7)` back (raw
  cast in `parseFOOTPRINT` / `parsePAD`); the port reads INHERITED and drops it. Not changed
  here (deferred, see Known gaps).

Known gaps (owners in `integration-2.md`, GLUE-2): `ZONE_CONNECTION` out-of-range values (an
i32-backed type: pads, footprints, zone filler); `BOARD::m_Uuid` and the board as a resolvable
report item; `fp_poly` library polygon keeps the duplicated closing vertex (the C++ re-derives
`m_libPoly` through `transformPolyVertices`, whose `SetClosed` merges it, at a later
`syncLibCoords` not yet located; output bytes unaffected); `BOARD::DrillSymbolLayers`, public
`expand_project_text_vars`.

## Review INT-2b: port decisions of the crate (reviewer pass)

Findings, verdicts per package and the evidence: `rust/docs/design/review-2.md`. Files of
this package: `tests/int2b_review.rs`, `tests/golden/int2b_review.txt`,
`tests/oracle/int2b_golden.sh`, `rust/docs/packages/FIX-BEZ-1.md`. Fixes made in finished
packages' files:

| C++ | Rust | state |
|---|---|---|
| `PAD::SetLocalSolderMaskMargin`, `SetLocalSolderPasteMargin`, `SetLocalSolderPasteMarginRatio` (pad.h: both outer sides) | `pad/derive.rs` | fixed (R2-03): the back side was not written |
| `bakeTextBoxLib` -> `OnFootprintTransformed` for the cells of a table inside a footprint (`GetParentFootprint()` through the table) | `io/parser/text.rs` `bake_text_box_lib` | fixed (R2-04) |
| `BOARD::ResolveTextVar` `PROJECTNAME`, `GetProject()->TextVarResolver` | `board/text_vars.rs` calls `BoardProject::text_var_resolver` with `Board::text_var_clock` | fixed (R2-01) |
| `PROJECT_GIT_UTILS::GetCurrentHash` outside a repository | `project/project.rs`: "no hash" | fixed (R2-02) |
| fresh `KIID()` never equals a UUID of the file (entropy-seeded in C++) | `kicad_core::KiidGenerator::reserve_all`; `io/parser/mod.rs` reserves every UUID-shaped token of the file in `PcbParser::new` | port addition (R2-05) |

Entries of earlier sections that no longer hold:

- M22 "`PROJECTNAME` resolves to the board file name ... `VCSHASH` / `VCSSHORTHASH` resolve to
  "" ... no clock field on `Board`": `PROJECTNAME` is `BoardProject::project_name`, the hash
  variables resolve to "no hash" (the reference value for a project outside a repository),
  the clock is `Board::text_var_clock` (empty until a host sets it).
- SET-5 "`VCSHASH` / `VCSSHORTHASH` resolve to """: "no hash".
- M22 / M20 "every layer is visible": `Board::visible_layers()` reads the project's
  `.kicad_prl` mask (changed by a concurrent package; checked against `pcb-dump --loader`).
- PCBIO-03 / PCBIO-08 "a generated UUID ... the port's generator is deterministic": still
  deterministic, but never equal to a UUID of the file (R2-05).

Verification:

- `tests/int2b_review.rs` `int2b_matches_oracle` (feature `dump`): 414 reference values
  (`pcb-dump board --no-init --hash`, `tests/oracle/int2b_golden.sh`) of four corpus boards:
  pad mask / paste margins, expansions and mask shapes of both sides, positions and boxes of
  the cells of a footprint table.
- `fresh_uuids_never_collide_with_file_uuids` (fails with "duplicate UUID 5cbb91d0-..."
  without the reservation); kicad-core `reserved_ids_are_skipped`.
- `board::text_vars::tests::built_in_variables` with the reference values of a `pcb-dump`
  run (review-2.md section 6).
- `d3_legacy_scoreboard` (ignored, needs the oracle cache): parse + format of the original
  corpus files against the reference re-save, 361 of 370 identical, the other 9 explained.
- Item-level corpus run (`pcb-dump` against `pcb-dump-rs`, 347 boards up to 1.5 MB, about 7.3
  million values): after the fixes the only mismatch classes left are the dump artifact
  `lib.poly` (R2-08), the barcode symbol, outline fonts, generated tables and UUID-less items.

Deviations:

- `KiidGenerator::reserve_all` and the parser's reservation have no C++ counterpart: the C++
  generator is seeded from entropy. With nothing reserved the sequence is the C++ one for the
  seed. A host that replaces `board.kiid_generator` after a load drops the reservations (use
  `seed_generator` to reseed and keep them, or an entropy-seeded generator).

Known gaps: the open findings of review-2.md (R2-07 .. R2-16).

## Package PCBIO-14: footprint library files (.kicad_mod parse + write, fp upgrade)

Supersedes the "`.kicad_mod` entry ... stub (PCBIO-14)" row of the PCBIO-01 table and the
`fp_lib.rs` staging stubs of PCBIO-01 / PCBIO-02.

### C++ to Rust map

| C++ | Rust | state |
|---|---|---|
| `PCB_IO_KICAD_SEXPR_PARSER::ParseFootprintLib` (bare `footprint` / `module` file, `kicad_footprint_lib` header with `version` / `generator` / `generator_version`, `checkVersion`, the `parseFp` lambda: no nickname, unlocked, `SetFileFormatVersionAtLoad`, `finishItem`, insert-first-wins `ptr_map` by name) | `src/io/parser/fp_lib.rs` `PcbParser::parse_footprint_lib` (returns `BTreeMap<String, FootprintId>`) | ported |
| per-file body of `FP_CACHE::Load` (parser with null board, `tmp.empty()` -> empty `IO_ERROR`, first entry of the map) | `src/io/mod.rs` `parse_footprint_file` (holder board, `BoardUse::FpHolder`, `board_present == false`, footprint attached) | ported; the rename to the file base name is the caller's (`run_fp_upgrade`) |
| `PCB_IO_KICAD_SEXPR::FormatFootprintLibFile` (font embed / clear, initial comments, header, `format( fp, true )`) | `src/io/writer/fp_lib.rs` `PcbWriter::format_footprint_lib_file` | ported (`EmbedFonts()`: no font files, R7, as the board writer) |
| `FootprintSave` normalisation (orientation 0, flip of a back footprint + orientation 0, leave group, `ClearAllNets`) | `src/io/writer/fp_lib.rs` `Board::footprint_save_normalise` | ported in place on a board the caller owns (no `Clone`, no `SetParent( nullptr )`) |
| `FP_CACHE::Save` / `Load` file handling, `FootprintSave` cache and file naming, `FootprintEnumerate` | kicad-session `run_fp_upgrade`, kicad-cli `command_upgrade.rs` | host side (directory listing and writing in kicad-cli) |
| `PCBNEW_JOBS_HANDLER::JobExportFpUpgrade` (KiCad library branch: load errors -> "Unable to load library", version check, `SetPath`, save errors -> "Unable to save library", "Footprint library was not updated") | kicad-session `jobs_handler.rs` `run_fp_upgrade` | ported (`ConvertLibrary` of legacy / foreign libraries: not ported, kicad-cli reports it) |

### Tests and verification (2026-11-10)

* D1 (`docs/analysis/pcb-io.md` 9.3) against the reference `kicad-cli fp upgrade --force -o OUT IN`,
  every `.kicad_mod` of `qa/data` and `demos` (426 files, each in its own scratch library):
  **393 byte-identical, 33 identical after masking the fresh UUIDs** the oracle draws for items
  without one (legacy files; no sibling reordering occurred), 0 differing, 0 failing. Also the
  messages and exit codes of: not updated (no `--force`, current file), in-place upgrade, a
  broken file in the library ("Unable to load library", 2), output path conflict (4), missing
  library / no output path (4): identical.
* Committed: `tests/oracle/pcbio14_golden.sh` (+ `pcbio14_mask_hash.py`) produces three byte
  goldens `tests/golden/pcbio14/*.kicad_mod` and the masked FNV-1a hash of the oracle output of
  every corpus file `tests/golden/pcbio14_d1_corpus.txt`; `tests/pcbio14_fp_lib.rs`
  `d1_byte_goldens` / `d1_corpus` replay them without the oracle (all 426 match).
* Ported C++ tests (`tests/pcbio14_fp_lib.rs`): `FpLibLoadSave` (test_fp_lib_load_save.cpp, both
  cases incl. roundtrip), `LegacyModuleFileImports`, `LibraryFootprintKeepsGroupsAndLoadsUnlocked`
  (footprint built as text instead of with the model, saved with `CTL_FOR_LIBRARY`),
  `ParseWarningDoesNotFailTheLibrary` (parser half here, library half in kicad-session
  `tests/pcbio14_fp_upgrade.rs`), `FootprintSave_OmitsNetsOnAllBoardConnectedItems` (board as
  text; checks the file has no `(net ` and every connected child is orphaned).
  `MissingLibraryThrowsOnEveryEnumerate`: skipped (filesystem cache, host side).
  The `.kicad_mod` cases of `BoardAndFootprintPrettifier` (writer/tests.rs) and the stub
  assertions of `parser/tests.rs` / `writer/tests.rs` `staging_stubs` now exercise the port.

### Deviations / notes

* **Null board** (the recorded risk of the design review). The C++ library parser and writer run
  with `GetBoard() == nullptr`. Parse: the holder board (`FpHolder`, default-constructed) is
  present in the arena, `board_present == false` reproduces the parser's own `m_board` tests;
  model defaults touched through `has_board` during the parse are shown equal empirically by the
  426-file D1 corpus (zones, pads, padstacks, text, shapes, groups, embedded files all occur).
  Write: two writer sites test `GetBoard()` on items of a footprint (`format( ZONE )` masks the
  layers with the enabled layers, `format( PAD )` lists a custom padstack by the board's copper
  count); with the holder attached the zone site differed (the 6 corpus files with a `*.Cu`
  footprint zone were written with `F.Cu B.Cu` only) and the padstack site would (no corpus file
  has a custom padstack in a footprint file). `format_footprint_lib_file` therefore writes a holder's
  footprint from a copy of the holder in which the footprint has no parent (`ParentRef::None`),
  so `has_board` is false for it and its children, exactly as for the boardless C++ footprint.
  Footprints of a non-holder board are written as they are (the C++ GUI export path, a clone
  keeping its parent).
* Multi-footprint `kicad_footprint_lib` files: as in C++ only the first footprint by name is kept;
  the others stay unattached in the holder's arena (the C++ map deletes them).
* `parse_footprint_file` returns an `IoError` with an empty message for a file without
  footprint (`THROW_IO_ERROR( wxEmptyString )`, caught by `FP_CACHE::Load`).

### Known gaps

* `PCB_IO_MGR::ConvertLibrary` (legacy / foreign libraries for `fp upgrade -o`), font embedding.

## Package STAB-PCB: board model against the reference on the whole corpus

Scoreboard driver: `rust/tools/pcb-dump/scoreboard.sh` (+ `scoreboard.py`): every corpus board
under 1 MB (qa/data/pcbnew, qa/data/cli, demos; 344 boards), scratch copy, `pcb-dump board
--no-init --hash --sections item,bbox,track,pad,shape,text,dim,zone,footprint,misc,board,order`
(cached gzip under `rust/target/dump-corpus/ref/`, keyed by board and binary hash) against
`pcb-dump-rs board --hash`, compare.py's classification, one line per board and section, the
summary in `tests/golden/dump_scoreboard.txt`. Regression cases: `tests/stab_pcb.rs` (feature
`dump`), goldens `tests/golden/stabpcb/` from `tests/oracle/stabpcb_golden.sh`
(`stabpcb_oracle.cpp`, linked against the pcb-dump objects: flips every footprint and prints
the pad primitives' board / library frame plus the pad and shape sections; and the reference
`kicad-cli pcb upgrade` of the same board).

Scoreboard (values): before 6133270 match / 257 mismatch / 18459 pending / 649 extra; after
6136472 / 64 / 15450 / 0 (0 rust-error, 0 missing). Every remaining mismatch is explained:

| class | values / boards | cause | status |
|---|---|---|---|
| `shape|lib.start`, `lib.end` of polygon cells (embedded_table_rotated_legacy_v10) | 16 / 1 | R2-09: `m_libStart` / `m_libEnd` of a cell whose library shape is POLY; never read for a POLY lib shape (`OnFootprintTransformed` / `rebakeFromLib` use `m_libPoly`) | expected (R2-09) |
| `text|*` / `bbox` / `board|bbox` of issue14549 | 38 / 1 | outline font (host font vs stroke font, R2-06) | expected |
| `bbox` / `board|bbox*` of barcodes (api_kitchen_sink, issue24525) | 6 / 2 | no zint symbol encoder (ZINT-1) | expected |
| `order|*` of ZoneFill-4.0.7, issue25580, issue4139 | 5 / 3 | items without a UUID in the file get generated UUIDs, a sort key (R2-10) | expected |
| pending `board|ds.meander.*` | 15436 / 343 | meander settings not modelled | expected |
| pending `misc|barcode.*`, `bbox` | 14 / 2 | ZINT-1 | expected |

Fixed here (C++ / Rust / state):

| C++ | Rust | state |
|---|---|---|
| `PCB_SHAPE::Flip` / `syncLibCoords` of a pad primitive: `PAD::GetPrimitiveLibScale()` through `GetParent()` | `MutEnv::pad_primitive_scale` (new field, `(1, 1)` outside pad primitives), set by `Pad::primitive_env` (current fp transform and relative orientation); read by `PcbShape::flip` and `sync_frame` | fixed (was `(1, 1)`, the M9 / FIX-PAD-1 deviation); `flipped_scaled_custom_pads_match_oracle`: 572 reference values after a flip of a non-uniformly scaled and a uniformly scaled footprint with bezier / rounded-rect / arc / segment / circle / polygon primitives |
| `(ZONE_CONNECTION) parseInt(..)` of pad and footprint `(zone_connect N)` keeps the integer; `format()` writes `static_cast<int>` | `CopperLayerProps::zone_connection: Option<i32>`, `Footprint::zone_connection: i32` (raw); `Pad::local_zone_connection_raw` / `set_local_zone_connection_raw`, `Footprint::local_zone_connection_raw` / `set_local_zone_connection[_raw]`, `Padstack::zone_connection_raw` / `set_zone_connection[_raw]`, `Pad::zone_connection_overrides_raw`; the enum accessors (`local_zone_connection`, `zone_connection_overrides`, `Padstack::zone_connection`) read through `ZoneConnection::from_raw` | fixed; `raw_zone_connect_values_survive_a_resave` (7 / 9 / -4 as the reference re-save). `from_raw` maps an out-of-range value to `FULL`: every C++ reader (zone filler switches with `FULL` in / as `default:`, `EvalRules` / `EvalZoneConnection` compare with `INHERITED` / `THT_THERMAL`, the zone connection test with `THERMAL`) treats it exactly like `FULL`. Writer, property getters and the dump use the raw value (`?N` as magic_enum prints it) |
| `PCB_DIMENSION_BASE` as a `PCB_TEXT`: `TransformTextToPolySet` | `PcbDimension::transform_text_to_poly_set` (public; the dimension is the host) | added (kicad-fab request); the dump's `text_polys` of dimensions uses it (corpus: all match) |
| `PCB_DIMENSION_BASE::GetValueMode`, `DimensionValueMode`, `FindDimensionLengthConstraint`, `CollectResolvedConstraints`, `DIM_VALUE_MODE` | `PcbDimension::value_mode`, `pcb_constraint::{dimension_value_mode, find_dimension_length_constraint, collect_resolved_constraints, DimValueMode}`; "Value Mode" property getter and the "Value" writeable predicate | ported (supersedes the M12 "skipped" row for `GetValueMode`); `dim|value_mode` 95 corpus values match. `GetValueFieldText` / `ChangeValue*` stay out of scope |
| dump: `lib.poly` = `GetLibraryPolyShape()` | `dump/items.rs` -> `library_poly_shape(ctx)` | fixed (R2-08: 192 values on 27 boards were a dump artifact) |
| dump: `fp.*.groups` order with `PCB_GROUP::ptr_cmp` (= the inherited `BOARD_ITEM::ptr_cmp`) | `dump/settings.rs` -> `board_item_ptr_cmp` | wired (2914 pending values now match) |
| dump: `ForEachBoardItem` / `DumpMisc` handle `PCB_TABLE_T` only (not generated tables) | `dump/format.rs`, `dump/defaults.rs` guards on `type_() == PcbTableT` | fixed (api_kitchen_sink: 649 extra values, item count) |

Reports checked against the reference, no model change:

* `PcbTablecell::corners` / `draw_pos` in a footprint (kicad-fab): already right since R2-04;
  every value of the cells of embedded_table_rotated_legacy_v10 matches (`--no-init` and
  `--loader`), and kicad-fab's `plot_driver_corpus` (KICAD_FAB_CORPUS_REPORT=1) now matches
  every file of that board: its two KNOWN_DIFFERENCES entries are stale (change request).
* Render cache of outline-font text: C++ does not keep the stored cache, for `fp_text` either
  (`SetLibTextSize` in `parsePCB_TEXT_effects` clears it, board text `SetVisible( true )`).
  Crafted check: kicad-fab's `plot_text/text.kicad_pcb` with the `fp_text "Outline"` cache
  replaced by a marker triangle; reference `kicad-cli pcb export gerbers -l User.3` plots the
  regenerated DejaVu Sans glyphs (identical to the original cache because the host has the
  font), no triangle. The kicad-fab difference is the missing outline font engine (R2-06).
* `fp_poly` library polygon with the closing vertex: nothing in C++ merges `m_libPoly` either
  (`OverrideLibPoly( GetPolyShape() )` in `parsePCB_SHAPE`, then `RebakeFromLib`, whose
  `transformPolyVertices` merges it only in the *board* polygon via `SetClosed`); the 1084 /
  1083 point counts came from the dump printing `m_libPoly` instead of `GetLibraryPolyShape()`
  (R2-08, fixed in the dump). Readers (`FOOTPRINT::cmp_drawings`, library parity) use
  `library_poly_shape`.

Entries of earlier sections that no longer hold: M9 / FIX-PAD-1 "pad primitives use (1, 1)
instead of `GetPrimitiveLibScale()`"; PCBIO-06 / PCBIO-07 / R2-14 / GLUE-2 "`zone_connect` out
of range reads as INHERITED" (the dimension / pad_connection parts of R2-14 still hold); M12
"`GetValueMode` skipped"; DUMP-RS "`PCB_GROUP::ptr_cmp` pending"; GLUE-2 known gap "fp_poly
library polygon keeps the duplicated closing vertex" (same in C++, see above).

io corpus (`tools/io-corpus/run.sh --update-golden`): 359 identical / 11 differ / 5 skipped
(unchanged); every golden-set board is identical now, so the 15 strict tests of
`tests/io_corpus.rs` run (their `#[ignore]` is gone). The 11: render caches of outline-font
text (tinytapeout-demo, issue22475: nothing else differs from O2 once the caches are masked;
RoyalBlue54L-Feather, issue14549: also zone nets) and zone net / priority / teardrop polygons
from the reference's post-load steps (complex_hierarchy, issue11814, issue14559, issue21482,
issue25580, teardrop_issue_JPC2, teardrop_spike). Each confirmed at session level: `kicad-rs pcb
upgrade --force` of the repository board equals O1 for complex_hierarchy, issue11814,
issue21482, and equals it modulo the generated UUIDs of UUID-less items (random in the
reference) and render caches for issue14559, RoyalBlue54L-Feather, issue14549; issue25580 and
the two teardrop boards are in kicad-session's `upgrade_corpus` golden (identical /
normalised). Demos over 1 MB went through the same byte comparison (identical except the two
render-cache demos); jetson, vme-wren (both copies) stay skipped as very large.

`pcb_dimension::tests::oracle_dimensions_dump` skips `value_mode`: the M12 fixture re-creates
the dimensions without the board's constraints (the field was pending before).

Deviations: none new. Known gaps: R2-09 cell `lib.start` / `lib.end` (unread);
`PCB_DIMENSION_BASE::GetValueFieldText`; out-of-range `zone_connect` in DRC report texts
(`PrintZoneConnection`) and library parity comparisons depend on callers switching to the raw
accessors (change requests).

## Package FONT-RC-1: outline-font text without a font engine (stored render cache as stand-in)

The reference never uses the `(render_cache ...)` read from a board file: the load path clears
it (`SetVisible( true )` for board text, `SetLibTextSize` for footprint text) and every reader
(`formatRenderCache`, plotting, `TransformTextToPolySet`, `GetEffectiveTextShape`, the TrueType
branch of the DRC text thickness check) regenerates the glyphs through `GetRenderCache` with
FreeType / HarfBuzz from the font installed on the host. The port has no outline font engine
(kicad-font D1). Decision (R2-06, orchestrator): the glyphs stored in the file stand in for the
regenerated ones while the text is unchanged. This is a documented deviation from the C++
control flow, not from its results on a machine that has the font.

| C++ | Rust | state |
|---|---|---|
| `parseRenderCache` (`SetupRenderCache`, `AddRenderCacheGlyph`) | `parse_render_cache`, then `EdaText::keep_file_render_cache` (not in C++) | ported + stand-in copy |
| (not in C++) | `EdaText::file_render_cache` (`EdaTextFileRenderCache`: the loaded cache + key), `eda_text::seal_file_render_cache` (end of `parse_pcb_text_effects`: board / footprint / dimension text and fields; end of `bake_text_box_lib`: text boxes, table cells) | new |
| `EDA_TEXT::GetRenderCache` | `eda_text::render_cache`: the live cache as before, else `file_render_cache_stand_in` where C++ would regenerate | ported + stand-in |
| (not in C++) | `eda_text::outline_font_fallback`, `outline_font_fallbacks( ctx )` (`OutlineFontFallback::message()`: "text uses font X which is not available; drawn with the stroke font"): visible outline texts without a valid stand-in, `RunOnChildren` order | new |
| `formatRenderCache` | unchanged: writes the stand-in glyphs (fractured as `CALLBACK_GAL` does), header from the current text as before | ported |

Validity of the stand-in: sealed, glyphs not empty; resolved text (`GetShownText( FOR_CANVAS )`),
angle (draw rotation, compared normalised to [0, 360): older files record -90 where the draw
rotation is 270), offset and mirroring as in `GetRenderCache`; plus face name, size,
justification, line spacing, italic, bold and multi-line as recorded at seal time (everything
whose setter clears the cache and that `OUTLINE_FONT::GetLinesAsGlyphs` reads; not the stroke
width, colour, keep-upright). A changed draw position translates the glyphs (what
`EDA_TEXT::Offset` does to a live cache), so footprint moves keep the stand-in; a rotation does
not. Readers (writer, `PCB_TEXT::TransformTextToPolySet`, `PCB_TEXTBOX`, `GetEffectiveTextShape`,
kicad-fab `PlotText`, kicad-drc `checkTextThickness`) all go through `render_cache`, so no reader
changed. `GetTextBox` / `StringBoundaryLimits` use the face's ascender, descender and HarfBuzz
advances, not the glyph polygons: the text bounding box stays on the stroke-font fallback (known
gap below).

Without a valid stand-in the writer writes the header only (`(render_cache "text" angle)`), as
before: the reference always produces glyphs (fontconfig substitutes a font for a missing one:
Calibri -> Carlito Bold, Ubuntu Sans -> Inter Bold), so "what the reference writes when it
cannot produce glyphs" does not occur on the oracle machine. The header is unchanged and matches
the reference's header on all corpus caches.

Verification:

* (a) Crafted board `tests/data/font_rc1/frc.kicad_pcb` (from `frc_input.kicad_pcb`, re-saved by
  the reference; fonts the oracle machine has: DejaVu Sans / Serif / Sans Mono, Liberation
  Sans / Serif / Mono; board texts incl. rotated bold italic, multi-line justified, mirrored on
  B.SilkS, a text variable, footprint fields and text in a 90 degree footprint, a rotated text
  box; 10 caches). The reference re-save of it is byte-identical to it (idempotent).
  - re-save: `kicad-rs pcb upgrade --force` byte-identical to the reference
    (`font_rc1_stand_in::resave_reproduces_reference`, parse + format; also checked at session
    level with the CLI);
  - Gerber: `pcb export gerbers -l F.Cu,B.Cu,F.SilkS,B.SilkS,F.Fab,User.1,User.2,User.3`: all 8
    layer files and the job file identical to the reference (CreationDate masked);
    kicad-fab `plot_text::crafted_text_matches_cli` now matches User.2 / User.3 too (75 files
    matched, 0 known differences, was 10 known);
  - DRC (`frc.kicad_dru`: text_height min 1 mm, text_thickness min 0.15 / 0.2 / 0.25 mm): the
    text_height and text_thickness violations identical to the reference (11 / 11). At
    text_thickness min 0.1 mm the port reports one more (`Liberation Bold`): see known gaps.
* (b) Marker triangle (`marker_triangle_cache_is_used_and_written`): a cache replaced by a
  triangle no font produces is exactly what `GetEffectiveTextShape` returns (outline and
  triangulated) and what the writer writes back; the plot path reads the same `render_cache`.
  Edited text (`edited_text_has_no_stand_in`): no stand-in, reported by `outline_font_fallbacks`,
  header-only cache written. Unit tests in `io/parser/text.rs` (move translates, size / angle /
  justification edits invalidate, footprint text legacy and new format).
* (c) Corpus boards with outline fonts (parser-level load, visible outline texts with a valid
  stand-in / total): issue22475 1/1, issue14549 6/6 (one cache recorded with angle -90, see
  above), RoyalBlue54L-Feather 72/72, tinytapeout-demo 4/4, jetson-agx-thor-baseboard 72/72; no
  fallback on any corpus board. Their reference outputs come from substitute fonts, so they are
  not compared byte for byte: kicad-session `upgrade_corpus` keeps masking render-cache glyph
  polygons. kicad-drc `drc24_courtyard_text::text_providers_match_reference_corpus` now compares
  issue22475 (before: pending) and the port does not report the `text_thickness` the reference
  reports on its `Franklin Gothic Medium` text, which the reference measures on fontconfig's
  substitute (change request: expected difference).

Deviations: the stand-in itself (above); the stand-in angle comparison is normalised.

Known gaps:

* Stored glyphs are fractured: `formatRenderCache` writes each outline of a glyph as its own
  `(polygon ...)` with holes bridged (`Fracture`), and `parseRenderCache` makes one glyph per
  polygon. The DRC thickness check (deflate, compare outline / hole counts) therefore sees slit
  outlines where the font gives outlines with holes; on the crafted board this flags one text at
  min 0.1 mm that the reference does not (`Unfracture()` of the stored glyph would match the
  reference there, but changes point order, so it would break the byte-identical re-save and
  plot if applied to the stand-in; not done, decision pending).
* Bounding box (`GetTextBox`) of outline text: stroke-font metrics (needs the face's metrics).
* Embedded fonts (out of scope; kicad-fab plots stroke text when fonts are embedded). What an
  outline engine package needs: FreeType outline decomposition (`OUTLINE_DECOMPOSER`, contour
  orientation, hole assignment by `PointInside`), HarfBuzz shaping (`getHarfbuzzShape`:
  advances, cluster order), the font lookup order (`FONT::GetFont`: embedded files of the board
  first, then fontconfig with its substitution rules), fake bold / italic, the outline font
  metrics for `GetTextBox`, and the oracle machine's installed fonts (`fc-list`: DejaVu,
  Liberation, Carlito, Caladea, Inter, JetBrains Mono, Free*, Noto, IPA, ...) to reproduce the
  reference's substitutes.

## Package ZINT-1: barcode symbol encoder wiring

`pcb_barcode.rs` `encode_symbol` now calls `kicad_zint::encode` (the port of the bundled
zint 2.15.0, crate `kicad-zint`, see its PORT_STATUS.md) with the settings of `zint_options`
and maps the vector rectangles / hexagons into `ModuleMatrix`; `compute_barcode` sets
`last_error` like `ComputeBarcode` (the international-characters text when `ZBarcode_Encode`
fails on non-ASCII text, zint's `errtxt` otherwise). `ENCODER_NOT_AVAILABLE` is removed; this
supersedes the M25a deviation "No symbol encoder" and the gap "`encode_symbol` always `None`".
`BarcodeUndefinedVariable` now checks `GetSymbolPoly().OutlineCount() > 0` as in C++.

Verification: `barcodes_match_reference_dump` (feature `dump`): 600 values (barcode polygons,
`last_error`, all bounding boxes of five boards with 36 barcodes) equal to `pcb-dump`.


## Package M26: generated tables (drill chart, stackup table) and their refresh

### C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `drill/drill_operation.h`, `HOLE_ATTRIBUTE` / `DRILL_LAYER_PAIR` of `drill_span.h` | `drill/drill_operation.rs` | ported (FAB-01's port moved to the model level; `DRILL_SPAN` is `pcb_drill_map::DrillSpan`, now `Eq` + `Ord` = `operator<`) |
| `drill/drill_enumerator.cpp` (`EnumerateDrillOperations`, `EnumerateDrillSpans`, `IsDrillSlot`) | `drill/drill_enumerator.rs` | ported (copy of FAB-01's `kicad-fab/src/drill/drill_enumerator.rs` with the M17 via methods instead of its `via_access` placeholders) |
| `drill/drill_symbol_profile.*` (profile, `GroupKeyString`, `Fingerprint`, token functions) | `drill/drill_symbol_profile.rs` | ported; `from_board` / `commit_to_board` read and write the board's canonical s-expression form through `io/parser/setup_drill_profile.rs` (compiled a third time as `model_form`) |
| `drill/drill_symbol_assigner.*` (`AssignDrillSymbols`, `ResolveDrillSymbols`, `DRILL_SYMBOL_ENTRY`, `ResolveDrillSymbolsByItem`), `BOARD::DrillSymbolCache` / `DRILL_SYMBOL_CACHE` | `drill/drill_symbol_assigner.rs` | ported; the cache is computed afresh per call (no generation / fingerprint reuse; same values) |
| `drill/drill_chart_model.*` | `drill/drill_chart_model.rs` | ported |
| `drill/drill_chart_template.*` (`DRILL_CHART_TEMPLATE`, `MakeDefault`) | `drill/drill_chart_template.rs` | ported; `SaveToFile` / `LoadFromFile` (template JSON files of the editor) **not ported** |
| `pcb_generated_table.cpp`: `IsDataRow`, `dataRows`, `IsStale`, `RebuildCells`, `migrateRows`, `setAutosizedRowHeights`, `metricRowHeight`, `minRowHeight`, `nearestDataCell`, `styleFromColumn`, `copyCellStyle`, row-height / font property getters and setters, `Flip` override | `pcb_generated_table.rs` (`PcbTable::is_data_row` .., `Board::generated_rebuild_cells`, `generated_table_autosize`, `generated_after_flip` ..) | ported; `SetColumns` / `moveShownCells` / `placeCells` / `styleCreatedCells` / `ApplySettings` / `CopyFrom` / `Similarity` / `Visit` / msg panel / property registration (editor paths) **not ported** |
| `board_tables/generated_table_schema.cpp` free functions (`DocumentationLayers`, `FormatGeneratedTableLength`, `ToGeneratedTableUnits`) | `pcb_generated_table.rs` | ported (the parser keeps its own `documentation_layers`) |
| `board_tables/generated_table_refresh.*` (`GENERATED_TABLE_REFRESH`, `RefreshGeneratedTables`) | `board_tables/generated_table_refresh.rs`, `pcb_generated_table::GeneratedTableRefresh` | ported (`OnItemsChanged` has no listener to notify) |
| `board_tables/stackup_table_model.cpp` (`BuildStackupTableRows`), `pcb_stackup_table.cpp` (`generate`, `styleNewCell`) | `pcb_stackup_table.rs` | ported |
| `pcb_drill_chart.cpp` (`generate`, `buildGroups`, `onRebuilt`, `onColumnsReversed`, `ApplyTemplate`, text helpers) | `pcb_drill_chart.rs` (+ `onRebuilt` at the end of `generated_rebuild_cells`) | ported; protobuf `Serialize` / `Deserialize` not ported (no API layer) |
| `board_tables/board_characteristics_table.cpp` | - | **not ported**: a one-shot plain `PCB_TABLE` the editor places, never refreshed; needs `ComputeBoardStatistics` (unported) |
| `PCB_DRILL_MAP::GetBoundingBox`, `HitTest( point )`, `GetBoardOutlines` (without its cache), `GetSpanChoice` / `SetSpanChoice` | `pcb_drill_map.rs` | ported (the M23 stubs are gone); `RefreshDrillSymbolLayers` / `DrillSymbolLayers` / `ExpandBoundingBoxForDrillSymbols` not needed by the refresh, not ported |
| `PCB_TEXTBOX::GetDrawPos` | `pcb_textbox.rs` | fix: the corner sum of the box centre wraps like the x86-64 C++ `int` sum (a new cell's half-`INT_MAX` box laid out before the table moves home) |

Call sites: `kicad-session` `board_loader::refresh_generated_tables` (now the real refresh;
`BOARD_LOADER::SaveBoard`), `jobs_handler::run_export_gerbers` and `run_pcb_upgrade` (a rebuilt
table marks the board as changed). The SVG job's refresh belongs to `PCB_PLOTTER::Plot` (FAB).

### Tests and verification

* `tests/m26_generated_tables.rs` + `tests/oracle/m26_generated_tables.{sh,py}` (golden
  `tests/golden/m26_generated_tables.txt`, cases `tests/data/m26/`): 13 boards, the reference
  `kicad-cli pcb upgrade --force` vs parse + `refresh_generated_tables` + write. 7 crafted boards
  (corpus boards with through / blind / micro vias, backdrills, slots, NPTH, press-fit, 2 / 4 / 6
  copper layers with and without stackup, a letters and a size-text profile) each with 3 drill
  charts (default columns; all 12 columns in inches, a hidden and a min-width column; a back-layer
  mils chart without totals) and 2 stackup tables, created without cells; 4 second-generation
  boards (the refreshed tables of one board on other boards: rows migrate by key, by position
  without row keys, and from restyled donor cells); `api_kitchen_sink` and
  `downgrade/golden/current/modern_metadata`. Every generated block (and the committed drill
  symbol profile) hashes like the reference's after normalising the UUIDs of new cells, with
  widths, heights and texts in clear in the golden; each output refreshed again equals the
  reference's second upgrade (12 fixed points; `m26_gen3_styled` moves column widths by 1 nm on
  the second upgrade in the reference too, reproduced). Measured once: all 26 full output files
  (O1 and O2) byte-identical after that normalisation.
* Session level (scratch run of `run_pcb_upgrade` over the same 12 inputs, 2026-11-10): all
  identical, `api_kitchen_sink` byte-identical (it was "not loadable" in the kicad-session
  upgrade corpus). Gerber job corpus (`kicad-session/tests/gerbers_job.rs`): the 2 runs that were
  pending on `RefreshGeneratedTables` (`modern_metadata`) now refresh and stop at
  `BRDITEMS_PLOTTER::PlotChartSymbols (FAB-13)`.
* `tests/pcbio12_misc_items.rs`: the generated-table masks are gone; `oracle_resave_blocks` and
  `oracle_corpus_blocks` refresh before writing (the kicad-cli path), new cell UUIDs masked;
  the parse + write test (`oracle_parse_write_blocks`) stays without refresh.
* Ported C++ tests (`tests/m26_drill_chart.rs`, `qa/tests/pcbnew/test_drill_chart.cpp`):
  `GroupsHolesBySizeAndPlating`, `RebuildKeepsCellIdentity`, `AChartFollowsTheBoardWithoutBeingAsked`
  (row counts), `MapIsHitAtItsMarksAndNowhereElse`, `MapIsHitOnItsOutlineWhenTheBoardHasNoHoles`,
  `MapBoardOutlineFollowsItsOffset`, `SpanChoiceRoundTripsThroughTheBoardSpans`,
  `CellFormattingSurvivesARebuild`, `CellFormattingFollowsItsGroupWhenAGroupIsAdded` / `Removed`,
  `RowsKeepSeparateCellsWhenARebuildReordersThem`, `FreshChartIsBuiltAtTheOrigin`,
  `RebuildKeepsChartPosition`, `EverySymbolResolvesWithoutAChart` (boards written as files; the
  module comment lists the cases left out). No `test_generated_table*` / `test_stackup_table*`
  exist in the tree.
* Unit tests: symbol assignment per policy (shapes exhausted, letters past Z, frozen
  assignments), `GroupKeyString` with every key, profile board-form round trip and fingerprint,
  token tables, `FormatGeneratedTableLength`, `DocumentationLayers`.

### Deviations

* `PCB_DRILL_CHART::IsStale` compares `m_builtGeneration` with `BOARD::GetDrillModelGeneration()`;
  the port keeps neither and treats a drill chart as always stale. The C++ chart is stale after
  every load (built generation 0, board generation >= 1), so the first refresh of a session is
  the same; a second refresh without a drill change rebuilds the chart again (the C++ skips it)
  and counts it in the return value. Rebuilding a fresh chart reproduces its cells (12 of 13
  oracle boards; the 13th moves by 1 nm in the C++ as well).
* `m_laidOutShowTotals` is not kept (set only between a `SetShowTotals` edit and the next rebuild).
* `DRILL_SYMBOL_PROFILE::m_uuid` is not kept (not in the board file, not in `operator==`).
* New cells take UUIDs from the board's `KiidGenerator` (random in the C++).

### Known gaps

* Not wired (change requests): `Board::table_autosize` / `PcbTable::set_autosized_row_heights`
  still reach the M26 `port_pending!` for a generated table (the refresh uses
  `Board::generated_table_autosize`); `Board::table_flip` / `table_mirror` do not call
  `Board::generated_after_flip` (ported, not oracle-verified).
* `kicad-fab/src/drill/` duplicates `drill/drill_operation.rs` / `drill_enumerator.rs` (and
  `DrillSpan`); consolidation: kicad-fab re-exports `kicad_pcb::drill`.
* Drill symbol and chart symbol plotting (`plot_brditems_plotter.cpp` L1515-1656) is FAB-13: it
  needs `crate::drill::drill_symbol_cache` (`by_item`: per hole the assignment, span, position,
  kind, diameter, size, orientation, slot flag), `PcbDrillMap::board_outlines` / `span` /
  `all_spans` / `offset` / `symbol_size`, the profile's `symbol_width`, and for charts
  `GeneratedTableKind::DrillChart { row_shapes, symbol_column }` with the cell geometry.
* Editor-only parts listed in the map (column editing, templates as files, board
  characteristics table, protobuf).

## Package SET-11 (done by WIRE-1): `TUNING_PROFILES` load

| C++ | Rust | State |
|---|---|---|
| `common/project/tuning_profiles.cpp` `TUNING_PROFILES` (constructor parameter `tuning_profiles_impedance_geometric`, `readUserDefinedProfileConfigurationLine`, `readViaOverrideConfigurationLine`, `migrateSchema0to1`, `migrateSchema1to2`, `GetTuningProfile`), `include/project/tuning_profiles.h` (`TUNING_PROFILE`, `DELAY_PROFILE_TRACK_PROPAGATION_ENTRY`, `DELAY_PROFILE_VIA_OVERRIDE_ENTRY`) | `project/tuning_profiles_json.rs` (`load_tuning_profiles( parent document )`) | ported, load side only (writer: SET-13) |

* Additive: `ProjectFile` is unchanged; the nested object is read from the project document
  (`BoardProject::document` / `ProjectFile::internals`) by the caller (kicad-session's
  `board_loader::tuning_profiles`, which maps it onto kicad-drc's `TuningProfiles` for
  `InitEngine` and `RunTests`).
* A profile entry whose conversion throws in C++ (`entry["x"]` of the wrong type) ends the
  load with the profiles read before it (`JSON_SETTINGS::Load` catches the exception); a
  missing key (undefined behaviour on a `const json&` in C++) is treated the same way.
* Verification: through the DRC result. With the loaded profiles the 6 corpus boards whose
  reference reports have no `missing_tuning_profile` entries for their profiles now equal the
  reference exactly (`kicad-session/tests/drc12_corpus.rs`: `drc_missing_tuning_profile`,
  `test_drc_net_chain_tuning_profiles`, `issue23868`, `issue25065`,
  `net_chain_bridging_tuning_profiles`, `time_calculations`), and `fw16_MCIO8i` lost those
  entries too (one unrelated difference left). pcb-dump has no tuning-profile section.
  Unit tests: migration 0 -> 2, entry filtering, the throw-ends-the-load rule.

## Package DRC-LP: footprint library tables (`src/libraries/`)

| C++ | Rust | State |
|---|---|---|
| `LIBRARY_TABLE_GRAMMAR` (PEGTL) + `LIBRARY_TABLE_PARSER::Parse` / `ParseBuffer` and its actions | `libraries/library_table.rs` `parse_library_table` | ported as a backtracking parser of the same language (no leading white space, `(lib` rows with `name` / `type` / `uri` / `options` / `descr`, `(hidden)` / `(disabled)`, quoted or bare values, the unescaping of the `QUOTED_TEXT` action, optional `(version)`); errors "Syntax error at line %zu, column %zu" |
| `LIBRARY_TABLE` (file / buffer constructors, `initFromIR`, `addRowFromIR`, `HasRow`, `Row`, `ParseOptions`), `LIBRARY_TABLE_ROW` | `library_table.rs` | ported (no `Format` / `Save`) |
| `LIBRARY_MANAGER::loadTables`, `loadNestedTables`, `LoadGlobalTables`, `LoadProjectTables`, `Rows`, `GetRow`, `GetFullURI`, `ExpandURI` (footprint tables) | `libraries/library_manager.rs` `LibraryManager` | ported; PCM auto-add / auto-remove and `ApplyLibOverrides` (read-only tables with kicad.json overrides) not ported; the row cache is transparent |
| `KIwxExpandEnvVars` / `ExpandEnvVarSubstitutions` (Unix), `ENV_VAR::IsVersionedEnvVar`, `GetVersionedEnvVarName`, the predefined variables, `wxFileName::MakeAbsolute` | `library_manager.rs` `LibraryEnv` | ported (project `TextVarResolver` first, then the environment, then the `KICAD*_` fallbacks; the `${DRC_WARNING}` pass-through with its `operator<<( int )` bracket) |
| `LIBRARY_MANAGER_ADAPTER::AsyncLoad` + `BlockUntilLoaded` (sequential), `loadQueuedRow`, `loadIfNeeded`, `loadFromScope`, `fetchIfLoaded`, `HasLibrary`, `IsLibraryLoaded`; `FOOTPRINT_LIBRARY_ADAPTER::createPlugin`, `enumerateLibrary`, `LoadFootprint`; `PCB_IO_KICAD_SEXPR::FootprintEnumerate` / `FP_CACHE::Load` | `libraries/footprint_library_adapter.rs` `FootprintLibraryAdapter` | ported for KiCad `.pretty` libraries (every `*.kicad_mod` parsed at load, one failure fails the library) |
| file access | `libraries/mod.rs` `LibrarySource` trait, `MemoryLibrarySource` | host-supplied (kicad-cli: file system; wasm: in memory) |

### Deviations

- Footprints of a loaded library are parsed again on `load_footprint` instead of keeping
  every parsed footprint (`PreloadedFootprints`); the parse is deterministic.
- A `.kicad_mod` file with invalid UTF-8 is read lossily.
- Rows of other plugin types fail to load ("not ported"), see kicad-drc DRC-LP known gaps.
- Nested-table cycles that `loadNestedTables` does not cut (two tables behind the same raw
  URI) make the C++ `Rows()` recurse without end; the port stops at depth 64.
- `m_childTables` is shared by the three table types in C++ (keyed by raw URI); the port
  holds only footprint tables.

Tests: unit tests in the three files (grammar incl. legacy forms and errors, options,
expansion, `MakeAbsolute`, adapter load states incl. disabled / missing / corrupt / unknown
type / nested). Verified against the reference through the DRC (kicad-drc PORT_STATUS DRC-LP).

## STAB-DRC-2: the board's own UUID

| C++ | Rust | State |
|---|---|---|
| `BOARD::m_Uuid` (`EDA_ITEM` base) | `Board::uuid()` (`board/mod.rs`) | ported |

Deviation: the C++ value is a random KIID drawn from the global generator when the board is
constructed (different per run). The port draws it from a generator of its own
(`BOARD_UUID_SEED`), so the item generator `Board::kiid_generator` is not consumed (every
golden that depends on its sequence is unchanged), and reserves it there like the UUIDs of a
loaded file. `Board::resolve_item` still has no handle for it (no `ItemId`); the DRC report
resolver (`kicad-drc` `BoardRcItemResolver`) answers it after every item, as
`BOARD::ResolveItem` does. Test: `board::tests::board_uuid_is_reserved_and_does_not_consume_the_item_generator`.
