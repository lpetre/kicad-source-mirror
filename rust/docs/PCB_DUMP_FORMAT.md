# pcb-dump output format

`pcb-dump` (source `rust/tools/pcb-dump/`, see `rust/docs/ORACLE.md` section "pcb-dump") prints
the board data model as computed by the real pcbnew code. This document specifies the format
precisely enough that a Rust implementation can produce byte-identical lines for the parts it
implements. The C++ source of truth for every field is the `EMIT( "field", ... )` call of the
same name in `rust/tools/pcb-dump/dump_*.cpp`; read it to see exactly which getter produces a
value.

## 1. Lines

Output is UTF-8, one record per line, `\n` terminated. There are five line kinds:

```
file|<basename>|                              start of one input board
SECTION|KEY|FIELD=VALUE                       a data line
error|SECTION|KEY|<message>                   a field / section / load that threw or crashed
warn|load|<basename>|<message>                e.g. wx assertions counted while loading
```

* `SECTION` is one of `item bbox track pad shape text dim zone footprint misc board order` for
  `pcb-dump board`, and `defaults` for `pcb-dump defaults`.
* `KEY` identifies the item (section 3); for `board` and `order` it is `board`; for `defaults`
  it is the class name (`PCB_TRACK`, `PCB_SHAPE.ARC`, `PCB_FIELD.REFERENCE`, `BOARD_DESIGN_SETTINGS`,
  ...).
* `FIELD` is a dotted name (`[A-Za-z0-9_.]`, plus layer names such as `F.Cu`, net codes, numeric
  indices). Everything after the first `=` is the value. Fields never contain `|` or `=`.
* With `--hash`, `VALUE` is replaced by `#` followed by 16 lowercase hex digits: the 64-bit
  **FNV-1a** hash of the UTF-8 bytes of the unhashed value (offset basis `0xcbf29ce484222325`,
  prime `0x100000001b3`, xor-then-multiply per byte, wrapping). Keys, field names, `error` and
  `warn` lines are never hashed. To debug a mismatch rerun without `--hash` and diff the line.
* `error` lines carry free text (C++ exception text, wx assertion text with C++ file/line) and
  are not part of the contract; a Rust side should treat them as "no value" and skip the field.

Within a board the order is: the `board` section, then for each item (section 3 order) the
item's sections in the order `item bbox track pad shape text dim zone footprint misc` (only the
applicable ones), then the `order` section. Within a section fields come in the fixed order of
the C++ code. A Rust harness should normally compare as a set of `SECTION|KEY|FIELD` lines
(map lookup), which makes it independent of order and lets it skip fields not yet ported.

`--sections a,b,c` restricts output to those sections.

## 2. Values

| Kind | Format | Examples |
|---|---|---|
| integer (`int`, `long long`, `unsigned`, ...) | decimal, `-` for negatives | `0`, `-1200000` |
| `double` | `printf("%.17g")`; `0.0` and `-0.0` both print `0` | `90`, `0.25`, `0.10000000000000001`, `1e-07`, `inf`, `nan` |
| `bool` | `true` / `false` | |
| string (`wxString`) | `"` + escaped UTF-8 + `"`; escapes: `\\` `\"` `\n` `\r` `\t`, other bytes < 0x20 and 0x7f as `\xHH` (lowercase), all other bytes verbatim | `"R1"`, `"a\"b"` |
| enum | the C++ enumerator name (magic_enum), unqualified; unknown values `?<int>` | `PTH`, `ROUNDRECT`, `PCB_PAD_T` |
| `std::optional<T>` | `none` or the value of `T` | `none`, `150000` |
| `VECTOR2I` | `x,y` | `110490000,78867000` |
| `VECTOR2D` / pairs of doubles | `x,y` with the double format | `1,1` |
| `BOX2I` | `x,y,w,h` = `GetPosition()` and `GetSize()` as stored (not normalised) | `84015000,73648750,27950000,10436500` |
| `EDA_ANGLE` | `AsDegrees()` with the double format | `90`, `-45` |
| layer | `LSET::Name()` canonical name; `UNDEFINED_LAYER`, `UNSELECTED_LAYER`, `?<n>` | `F.Cu`, `In1.Cu`, `Edge.Cuts` |
| layer list | `[` names joined by `,` `]` | `[F.Cu,B.Cu]` |
| `LSET` | 32 lowercase hex digits = 128 bits, most significant first; bit *n* is layer id *n* (`F_Cu`=0, `F_Mask`=1, `B_Cu`=2, ...) | `00000000000000000000000000000001` |
| `KIID` | `AsStdString()` (lowercase 8-4-4-4-12) | |
| point list | `[` points joined by one space `]` | `[0,0 10,0]` |

Composite (geometry) values, all integers in nm in KiCad's own vertex order:

```
chain   := C(<closed|open> w=<width> n=<pointcount> [<pt> <pt> ...]
             [ arcs=[<arc> <arc> ...] shapes=[<s> <s> ...]])        arcs part only if ArcCount()>0
arc     := A(<p0> <mid> <p1> w=<width>)                              SHAPE_ARC GetP0/GetArcMid/GetP1/GetWidth
s       := <first>  or  <first>/<second>                             SHAPE_LINE_CHAIN::CShapes() per point;
                                                                     -1 = plain point; /second only if >= 0
polyset := P(<outlinecount> {<chain>[ hole <chain>]...} ...)         outline then its holes, KiCad order
shape   := SH_RECT(p=<pt> size=<w,h> r=<radius> w=<width>)
         | SH_SEGMENT(a=<pt> b=<pt> w=<width>)
         | SH_LINE_CHAIN(<chain>)
         | SH_CIRCLE(c=<pt> r=<radius>)
         | SH_SIMPLE(<chain>)
         | SH_POLY_SET(<polyset>)
         | SH_COMPOUND(<n> <shape> <shape> ...)
         | SH_ARC(<arc>)
         | SH_ELLIPSE(c=<pt> major=<int> minor=<int> rot=<deg> start=<deg> end=<deg> arc=<bool>)
         | SH_NULL()
         | null                                                      null shared_ptr
```

Type names are `SHAPE_TYPE_asString()`. Hatch lines are `[<a>><b> ...]` (segment endpoints
joined by `>`). Lists of strings are `[` quoted strings joined by one space `]`. A few composite
fields join scalar values with `,` or `/` as documented next to their `EMIT` call (e.g.
`textbox.margins=left,top,right,bottom`, `ds.vias_dimensions_list=[dia/drill ...]`).

### Rust recipe for `%.17g`

`%.17g` = 17 significant digits, `%e` style if the decimal exponent is `< -4` or `>= 17`,
otherwise `%f` style, then trailing zeros (and a trailing `.`) removed. With `format!("{:.16e}", x)`
you get the digits and exponent; reformat accordingly. Exponents have a sign and at least two
digits (`1e-07`, `1.5e+20`). The dumper folds `-0.0` to `0`.

## 3. Items, keys and order

Items are visited in this order (container order, i.e. load order):

1. `BOARD::Footprints()`, each footprint followed by its children: `GetFields()` (in order),
   `Pads()`, `GraphicalItems()`, `Zones()`, `Groups()`, `Points()`;
2. `Drawings()` (a `PCB_TABLE` is followed by its `GetCells()`);
3. `Tracks()`; 4. `Zones()`; 5. `Generators()`; 6. `Groups()`; 7. `Points()`; 8. `Markers()`.

The visit index (0-based, over the whole list above) is the item's **ordinal**.

`KEY` is the item's UUID when it is (a) not nil, (b) unique among all visited items and (c)
present in the input file text; otherwise `#<ordinal>`. (c) is decided by scanning the file for
tokens made of hex digits and `-`: 36-character tokens of UUID shape count as UUIDs, hex tokens of
up to 8 digits count as legacy tstamps (`00000000-0000-0000-0000-0000XXXXXXXX`). Items that the
loader gave a generated UUID therefore get an ordinal key. The `item.uuid` field prints
`generated` for such items, and `item.uuid_in_file` says which case applies. (The tool seeds
`KIID`'s generator, so generated UUIDs are stable from run to run, but they are not part of the
contract.)

References to other items (`parent`, `group`, member lists, pad order, `order` lists) use the
same keys; `board` is the board, `null` is none, `unkeyed` an item outside the walk.

## 4. Sections

| Section | For | Content (see the C++ for the exact field list) |
|---|---|---|
| `item` | every item | type, uuid, parent, group, layer, layerset (hex + names), position, FP-relative position, locked (effective and own flag), copper/hole/knockout/side-specific flags; for connected items net code, net name, local ratsnest flag, local clearance; custom properties |
| `bbox` | every item | `GetBoundingBox()`; footprints: with/without text, `GetFpPadsLocalBbox`, bounding hull, F/B copper layer bboxes; pads and vias: per-copper-layer bbox; pads: bounding radius |
| `track` | track, arc, via | geometry, width, length, mask; arc centre/radius/angles; via type, layer span, drill, tenting/covering/plugging/capping/filling modes, backdrill, post-machining, full `padstack.*`, per copper layer: `on_layer`, `width`, `flash`, `shape` (`GetEffectiveShape`, default flashing) and `shape_flashed` (forced flash), `hole_shape`; `tpoly.*` (below) |
| `pad` | pads | number, pin function/type, attribute, property, positions and orientations (board frame and library frame), lib scale, drill, local overrides, effective zone connection and thermal values, mask expansion / paste margin per side, `padstack.*` (library frame, as stored), `layer.<L>.*` (board frame getters per unique padstack layer), per copper layer: `on_layer`, `flash` (`FlashLayer(l)`), `flash_permitted` (`FlashLayer(l, true)`, no connectivity), `own_clearance`, `shape`, `shape_flashed`, `effective_polygon` (`ERROR_INSIDE`) / `effective_polygon_out` (`ERROR_OUTSIDE`); `shape.<L>` for the non-copper layers; `hole_shape`, `hole_poly.*`, `tpoly.*` |
| `shape` | `PCB_SHAPE`, textbox, table cell | EDA_SHAPE members by shape kind (start/end/centre/mid/radius/angles/bezier/poly/rect/ellipse), stroke, fill, hatching, mask, library-frame data (`lib.*`), corners, per layer `shape.<L>` and `tpoly.*` |
| `text` | text, field, textbox, cell, dimensions | EDA_TEXT attributes, `text`, `shown_text` (`GetShownText(RESOLVED)`), text box, interline, draw pos/rotation, field id/name/ordinal, textbox margins/border, cell row/col/span, `text_shape` (`GetEffectiveTextShape(false)`: stroke segments), `text_polys.c0_e5000_in` (`TransformTextToPolySet(0, 5000, ERROR_INSIDE)`), `effective_shape`, knockout `tpoly.*` |
| `dim` | dimensions | dimension members, `shapes.<i>` (the generated shapes), aligned/orthogonal/radial/leader specifics, `tpoly.*` |
| `zone` | zones | settings, outline (`Outline()`), library outline, hatch lines, per layer `filled.<L>` (filled polygons as stored, `none` if no fill for the layer) and `islands.<L>` (one `0`/`1` per filled outline, `IsIsland`), effective shape |
| `footprint` | footprints | every member incl. TRS transform, attributes, local overrides, net-tie and jumper groups, fields, pad/drawing/zone order (`pad.<i>=<key> "<number>"`), 3D models, variants, courtyards F/B (`GetCourtyard`), hulls, area |
| `misc` | target, point, barcode, reference image, group, generator, table, marker | the class-specific members; group members as a sorted key list |
| `board` | the board | file version, generator, layers (enabled, names, types), page, title block, properties, all design settings (`ds.*`, incl. `ds.drc_severity.<DRCE_*>`, netclasses, stackup, default zone settings, teardrops, meanders), plot params (`plot.*`), nets (`net.<code>.*`), effective netclass per net, composite netclasses, `stackup_or_default.*`, four `ComputeBoundingBox` variants, `GetBoardPolygonOutlines` (plain, inferred, with arcs), item count |
| `order` | the board | the save order of `PCB_IO_KICAD_SEXPR::format()`: the same `std::set` comparators (`BOARD_ITEM::ptr_cmp`, `BOARD::cmp_drawings`, `PCB_TRACK::cmp_tracks`, `PCB_POINT::cmp_points`, `FOOTPRINT::cmp_pads/cmp_drawings/cmp_zones`, `PCB_GROUP::ptr_cmp`) as `[key key ...]`; `<list>.dropped=N` if the set dropped N "equal" items |

`tpoly.<L>.<combo>` is `TransformShapeToPolygon(buf, L, clearance, maxError, errorLoc)` on an
empty buffer, for the combos `c0_e5000_in` (0, 5000, `ERROR_INSIDE`), `c0_e1000_out`
(0, 1000, `ERROR_OUTSIDE`) and `c100000_e5000_out` (100000, 5000, `ERROR_OUTSIDE`). Pads use the
layer `F.Cu` if the pad is on it, else its principal layer; vias use `F.Cu`.

## 5. defaults

`pcb-dump defaults` default-constructs each class (items with a default `BOARD` as parent, pads
and fields with a default `FOOTPRINT` on that board as parent; items are not added to the board)
and prints the same item sections with the section name as field prefix:
`defaults|PCB_TRACK|track.width=200000`. Settings classes print their members directly:
`defaults|BOARD_DESIGN_SETTINGS|min_clearance=0`. `defaults|BOARD|...` is the `board` section of a
default board. Classes: `PCB_TRACK PCB_ARC PCB_VIA PAD FOOTPRINT ZONE PCB_SHAPE.<every SHAPE_T>
PCB_TEXT PCB_TEXT.in_footprint PCB_FIELD.<every FIELD_T> PCB_TEXTBOX PCB_TABLE PCB_TABLECELL
PCB_DIM_ALIGNED PCB_DIM_ORTHOGONAL PCB_DIM_RADIAL PCB_DIM_LEADER PCB_DIM_CENTER PCB_GROUP
PCB_GENERATOR.PCB_TUNING_PATTERN PCB_TARGET PCB_POINT PCB_BARCODE PCB_REFERENCE_IMAGE PCB_MARKER
NETINFO_ITEM NETCLASS BOARD_DESIGN_SETTINGS BOARD_STACKUP BOARD_STACKUP.default_list
PCB_PLOT_PARAMS TEARDROP_PARAMETERS ZONE_SETTINGS BOARD`.

## 6. Stability rules

* Two runs over the same input produce identical output (checked over the corpus).
* Fields are only ever added (within a section or as new sections); renaming a field or changing
  its value format requires regenerating every golden that contains it, so say so in the commit.
* Values are what the C++ returns, including its quirks; do not "fix" them in the dumper.

## 7. rules

`pcb-dump rules [--component-classes] FILE...` (source `dump_rules.cpp`) parses each `.kicad_dru`
file with the real `DRC_RULES_PARSER` (source name = the file's basename) twice: with a
collecting `REPORTER` (key `report`, the rule editor path: errors are reported and parsing goes on)
and without one (key `throw`, the DRC engine / kicad-cli path: the first error throws a
`PARSE_ERROR`). The text is passed as is (not through `DRC_ENGINE::loadRules`, which doubles every
line break and expands text variables). Layer names are matched against the start-up
`ENUM_MAP<PCB_LAYER_ID>` (canonical names only). `PROPERTY_MANAGER::Rebuild()` runs first.
With `--component-classes`, `ParseComponentClassAssignmentRules` is used instead of `Parse`.

```
file|<basename>|
rules|<mode>|status=ok | parse_error | io_error
rules|<mode>|error="<What()>"                    (parse_error / io_error only)
rules|<mode>|error.problem="<ParseProblem()>"    (parse_error only, also .line, .offset,
rules|<mode>|error.line=<n>                       .input_line = the source line as stored)
rules|<mode>|msg.<i>=<SEVERITY> "<text>"         every REPORTER::Report call, in order
rules|<mode>|count=<number of rules>
rules|<mode>|rule.<i>.name / .implicit / .unary / .layer_source / .layers (LSET hex) / .severity
rules|<mode>|rule.<i>.condition=none | "<expression>"   then .condition.pair (RequiresPairItems),
                                     .condition.item_b (ReferencesItemB), .condition.geometry
rules|<mode>|rule.<i>.constraints=<n>
rules|<mode>|rule.<i>.c.<j>.type (DRC_CONSTRAINT_T name) / .min / .opt / .max (none or IU)
             / .disallow (flag bits) / .zone_connection / .test (assertion, like .condition)
             / .implicit_min / .reference_layer / .reference_net
             / .option.skew_within_diff_pairs / .option.space_domain / .option.time_domain
             / .name (GetName()) / .severity (GetSeverity())
             / .parent_rule / .min_rule / .opt_rule / .max_rule (none | self | other)
rules|<mode>|ccrule.<i>.class="<name>" | ccrule.<i>=null     (--component-classes)
warn|rules|<basename>|wx assertions: <n>          assertions raised while parsing (counted, not thrown)
```

Severities print as the C++ enumerator names (`RPT_SEVERITY_ERROR`, ...). Fields come in the
order shown. Rust twin: `rust/crates/kicad-drc/tests/common/mod.rs` (`dump_rules`), runnable as
`cargo run -p kicad-drc --example rules_dump -- FILE...`; goldens in
`rust/crates/kicad-drc/tests/golden/rules_*.txt` (generator `tests/oracle/gen_rules_goldens.py`).
