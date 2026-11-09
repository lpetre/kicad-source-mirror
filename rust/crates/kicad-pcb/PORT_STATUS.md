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
- The generator subclasses override `GetFriendlyName` (and `IsSelectableAsConnected`, ...);
  `tools/check_item_shims.py` only reads `pcb_generator.h`, so `PcbGenerator` has no
  `friendly_name` shim and returns the `BOARD_ITEM` default until the checker knows the
  subclass headers.
- Not ported (editor / UI): `PCB_GROUP::TopLevelGroup` / `WithinScope`, `GRID_GEOMETRY`,
  `FindActiveGridAt`, constraint labels / glyphs / bitmaps, `EXTRUDED_3D_BODY::PackColorKey`,
  marker colours and zoom.

## M0d - board, settings, nets, project data

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

- `NetSettings` pattern assignments hold `NetclassPattern` (the pattern text) instead of an
  `EdaCombinedMatcher`: the kicad-core matcher is neither `Clone` nor `Send + Sync`, which
  `Board` needs; `NetclassPattern::matcher()` builds `EDA_COMBINED_MATCHER( p, CTX_NETCLASS )`.
- A root `Netclass` / constituent `ComponentClass` has an **empty** constituent list (C++
  lists `this`).
- The three meander settings are their `tuning_pattern_settings` JSON objects; the defaults
  are the objects the C++ writer produces for `PNS::MEANDER_SETTINGS()` with the constructor
  spacings.
- `PcbPlotParams::layer` (`m_layer`, uninitialised in C++) defaults to `UNDEFINED_LAYER`.
- Net 0 of a new board keeps the `Default` class of the `NET_SETTINGS` constructor (priority
  `INT_MAX`), as in C++, while `net_settings.default_netclass` is the board's class
  (priority -1).
- `OutlineErrorHandler` (`OUTLINE_ERROR_HANDLER`) is defined in `board/outline.rs` for the
  `get_board_polygon_outlines` signature; M21 may move it.

The M0a known gaps "`BoardDesignSettings::max_error()` returns the constructor value",
"`Board::build_layer_env` is a stub" and "`LayerEnv::default_board()` enables all layers"
are closed for boards (`Board::default` derives its snapshot from the settings);
`LayerEnv::default_board()` itself (frozen `ctx.rs`) is no longer used by `Board`.

## Package M0c: shapes, text, tables, dimensions, barcode

Types of `board-model-types.md` sections 6, 7, 8 and the barcode of section 10, transcribed
and checked against the headers of this tree; `Default` = the C++ constructors (nil UUID),
`new(&mut KiidGenerator, ..)` draws the UUID. `check_item_shims.py` prints `ok` for
`PcbShape`, `PcbText`, `PcbField`, `PcbTextbox`, `PcbTablecell`, `PcbTable`, `PcbDimension`,
`PcbBarcode`.

| C++ | Rust | State |
|---|---|---|
| `SHAPE_T`, `FILL_T`, `ARC_MID`, `EDA_SHAPE_HATCH_CACHE_DATA`, `EDA_SHAPE` data + constructor | `eda_shape/mod.rs` (`ShapeT`, `FillT`, `ArcMid`, `EdaShapeHatchCacheData`, `EdaShape::new`) | done; trivial getters / `SetStart` / `SetEnd` real, other setters stubs (M7) |
| `EDA_SHAPE` virtual base bodies, non-virtual members calling virtuals | generic functions + `EdaShapeExt` (`make_effective_shapes`, `hatching`, `hatch_lines`, `shape_position`, `shape_center`, `shape_bounding_box`, `shape_hit_test[_rect]`, `shape_transform_to_polygon`, `shape_length`, `rectangle_size`) | signatures; `GetWidth`, `IsProxyItem`, `IsFilledForHitTesting` real, rest stubs (M7 / M8) |
| `LINE_ENDING_STYLE`, `LINE_ENDING` | `line_ending.rs` | data done; geometry and tokens stubs (M7) |
| `PCB_SHAPE` data, constructor, overrides; lib-frame machinery and `EDA_SHAPE` setter overrides | `pcb_shape/mod.rs`, `pcb_shape/lib_frame.rs` | data done; `GetLayer`, `HasLineStroke`, `IsProxyItem`, `GetStroke`, `GetPosition`/`GetCenter` forwards, lib accessors real; rest stubs (M9) |
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

- `PCB_SHAPE::GetWidth` scales by the parent footprint (`fpScaleLinear( transformFp() )`) but
  `EdaShapeItem::width` takes no context: stub until the orchestrator decides (see the M0c
  report).
- The `EDA_SHAPE` setter overrides exist on `PcbShape` (all) and on `PcbTextbox` / `PcbTablecell`
  (`set_start`, `set_end`, `set_shape`, `set_top/left/right/bottom`, `set_stroke`,
  `set_poly_shape`); the remaining ones for textboxes follow with M11.

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

- `PADSTACK::m_copperProps` is an `unordered_map` in C++; the port uses a `BTreeMap` (layer
  order). This affects CUSTOM-mode `ForEachUniqueLayer` order and the `begin()` fallbacks of
  `CopperLayer` / `EffectiveLayerFor`; M13 must check the oracle there.
- `Pad::default()` fills `zone_layer_overrides` for the 32 copper layers of the no-board
  `BoardCopperLayerCount()`; a pad constructed under a board in C++ gets the board's copper
  count. `GetZoneLayerOverride` returns `ZLO_NONE` for a missing layer, so reads are equal.
- `Zone::default()` writes the values `ZONE_SETTINGS().ExportSetting( *this, false )` stores
  directly (ExportSetting itself is M5); `Zone::new_with_settings` (board defaults) is a stub.
- `ZoneSettings` equality is manual (`std::equal` over the first map's range for the layer
  properties), not a derive.
- `RECT_NO_CHAMFER` is written as the literal 0 (`RECT_CHAMFER_POSITIONS` is not in kicad-geom).
- `OUTLINE_ERROR_HANDLER` of `BuildCourtyardCaches` is the local alias
  `footprint::geometry::CourtyardErrorHandler` until M21 defines the board-outline handler.
