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
