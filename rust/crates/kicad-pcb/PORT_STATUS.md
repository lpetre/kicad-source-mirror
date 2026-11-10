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
| `COMPONENT_CLASS_MANAGER` (static / effective / combined / dynamic classes, netlist update, ticker) | `component_class.rs` | ported except `SyncDynamicComponentClassAssignments` (stub, tier 2: needs the DRC rule parser and `COMPONENT_CLASS_ASSIGNMENT_DATA`) |
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

* `ComponentClassManager::sync_dynamic_component_class_assignments`: `port_pending!` (tier 2,
  needs the DRC rule parser / `CompileAssignmentRule` and project component-class settings).
* `FOOTPRINT::GetComponentClass` (footprint/mod.rs, not this package) can now be written with
  `ComponentClassManager::get_component_class`, but needs `&mut` access to the manager and the
  DRC rule matcher.

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
