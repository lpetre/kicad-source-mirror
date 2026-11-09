# Text, fonts and text-to-geometry - porting analysis

> **Precedence note (orchestrator).** Where the Rust layout proposed in this document conflicts
> with `rust/docs/design/board-model.md` (revision 2, Fable-reviewed), the design document wins.
> In particular: `TextAttributes` lives in `kicad-core` (not `kicad-font`); `EDA_TEXT` behaviour is reached through the `EdaTextItem` trait of board-model.md 6.2a, never by running an algorithm on the bare `EdaText` data struct; the stroke font, layout engine and markup parser (TF1-TF3) are already ported in `kicad-font`.

Analyst document for the port of KiCad's text engine: the stroke ("KiCad Font") and outline
(TrueType/OpenType) fonts in `common/font/`, the Newstroke glyph table, the markup parser
(`~{overbar}`, `_{sub}`, `^{super}`), `EDA_TEXT` layout (justification, multi-line, bounding
boxes, pen widths), the board text items (`PCB_TEXT`, `PCB_FIELD`, `PCB_TEXTBOX`, dimension
text) and the way plotters turn text into strokes and polygons. All line numbers refer to the
files in `<kicad>` as checked in (KiCad master, board file format `20261031`).

Read `rust/PORTING.md` first. Related analysis documents that this one does not repeat:
`plot-fab.md` (Gerber/SVG engines, section 4.4 "Text"), `settings.md` section 4.14 (text
variables: resolver chain, `ExpandTextVars`/`ResolveTextVars`, built-in variable list),
`pcb-io.md` (parsing/writing `(effects ...)`, `(render_cache ...)`, risk R7 "Outline fonts"),
`connectivity-zones.md` (zone knockouts of text), `drc.md` (consumers of text shapes),
`design/board-model.md` (where text items live: packages M10, M11, M12).

File abbreviations used in line references:

| Abbr. | File | Lines |
|---|---|---|
| `FH` | `include/font/font.h` | 312 |
| `F` | `common/font/font.cpp` | 704 |
| `SF` | `common/font/stroke_font.cpp` | 307 |
| `SFH` | `include/font/stroke_font.h` | 119 |
| `GL` | `common/font/glyph.cpp` | 180 |
| `GT` | `include/font/glyph_types.h` | 112 |
| `NS` | `common/newstroke_font.cpp` (data) | 65745 |
| `OF` | `common/font/outline_font.cpp` | 630 |
| `OFH` | `include/font/outline_font.h` | 248 |
| `OD` | `common/font/outline_decomposer.cpp` | 182 |
| `ODH` | `include/font/outline_decomposer.h` | 133 |
| `FC` | `common/font/fontconfig.cpp` | 507 |
| `TA` | `common/font/text_attributes.cpp` | 130 |
| `TAH` | `include/font/text_attributes.h` | 166 |
| `FM` | `include/font/font_metrics.h` | 65 |
| `MP` | `include/markup_parser.h` | 135 |
| `MPC` | `common/markup_parser.cpp` | 83 |
| `CG` | `common/callback_gal.cpp` / `include/callback_gal.h` | 77 / 88 |
| `GR` | `common/gr_text.cpp` / `include/gr_text.h` | 172 / 126 |
| `ET` | `common/eda_text.cpp` | 1467 |
| `ETH` | `include/eda_text.h` | 554 |
| `PT` | `pcbnew/pcb_text.cpp` | 952 |
| `PF` | `pcbnew/pcb_field.cpp` | 397 |
| `TB` | `pcbnew/pcb_textbox.cpp` | 1147 |
| `DIM` | `pcbnew/pcb_dimension.cpp` | 2546 |
| `P` | `common/plotters/plotter.cpp` | 667 |
| `G` | `common/plotters/GERBER_plotter.cpp` | 1919 |
| `B` | `pcbnew/plot_brditems_plotter.cpp` | 1656 |
| `PP` | `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` | - |
| `W` | `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` | - |

Contents:

1. Purpose and entry points
2. File inventory
3. Data structures
4. Control flow and algorithms (with every order-sensitive place)
5. What this subsystem needs from other subsystems
6. Output formats
7. Backward compatibility
8. Multi-threading
9. Existing tests and data, differential tests
10. Rust module layout and work packages
11. Risks and traps

---

## 1. Purpose and entry points

The subsystem converts a string plus text attributes into geometry:

* **Stroke glyphs** (polylines in `VECTOR2D`) for the built-in Newstroke font; consumers turn
  each polyline segment into a thick segment (`SHAPE_SEGMENT`, Gerber `D02`/`D01` with a round
  aperture, or an oval polygon via `TransformOvalToPolygon`).
* **Outline glyphs** (`SHAPE_POLY_SET` per glyph, with holes, plus a cached triangulation) for
  TrueType/OpenType fonts, rasterised through FreeType + HarfBuzz + fontconfig, or read back from
  the `(render_cache ...)` polygons stored in board files.
* **Measurements**: advance width, bounding boxes (`EDA_TEXT::GetTextBox`), interline pitch,
  line breaking of text boxes (`FONT::LinebreakText`).

Entry points used by the ported features (tier 1 unless stated):

| Caller | Function | Purpose |
|---|---|---|
| DRC (copper/silk/edge clearance, courtyard, disallow, ...) | `PCB_TEXT::GetEffectiveShape` (PT:706-718), `PCB_TEXTBOX::GetEffectiveShape` (TB:852-861), `PCB_DIMENSION_BASE::GetEffectiveShape` (DIM:870-880) -> `EDA_TEXT::GetEffectiveTextShape` (ET:1143-1220) | `SHAPE_COMPOUND` of segments (stroke) or triangles (outline); knockout text -> polygon |
| Zone filler (knockouts), solder-mask DRC, board/footprint polygon builders | `PCB_TEXT::TransformTextToPolySet` (PT:784-846), `PCB_TEXT::TransformShapeToPolygon` (PT:857-865), `PCB_TEXTBOX::TransformTextToPolySet` / `TransformShapeToPolygon` (TB:878-1000) | merged polygon of the text, optional clearance, knockout box |
| Gerber/SVG plotting | `BRDITEMS_PLOTTER::PlotText` (B:786-896) -> `PLOTTER::PlotText` (P:618-667) -> `FONT::Draw` | stroke segments / filled polygons |
| R-tree / bbox / hit tests | `PCB_TEXT::GetBoundingBox` (PT:413-425) -> `EDA_TEXT::GetTextBox` (ET:755-888) | text box (also used by DRC broad phase, footprint bbox) |
| Text box wrapping | `PCB_TEXTBOX::GetShownText` (TB:497-541) -> `FONT::LinebreakText` (F:612-704) | inserts `\n` |
| Board file reader | `parseEDA_TEXT` effects, `MigrateLegacyBoldStrokeWidth` (PP:899-900), `parseRenderCache` (PP:903-950), `finishItem` -> `ResolveFont` (PP:1137-1147) | attributes, outline-font caches |
| Board file writer | `EDA_TEXT::Format` (ET:1081-1140), `formatRenderCache` (W:573-598) | `(effects ...)`, `(render_cache ...)` |
| DRC text_dims (tier 2) | `drc_test_provider_text_dims.cpp:136` (`GetRenderCache`), `:204` (`GetEffectiveTextPenWidth`) | outline glyph sizes / stroke widths |

`kicad-cli` itself never calls the font module directly; everything goes through the items.

---

## 2. File inventory

| File | Lines | Role | Scope |
|---|---|---|---|
| `common/newstroke_font.cpp` | 65745 | `const char* const newstroke_font[]` (L150-65734): **65487** glyph strings for U+0020..U+FFEE (index = codepoint - 0x20), MIT-licensed Hershey derivative + CJK from Source Han Sans (OFL, header L1-142); `newstroke_font_bufsize` L65735. Only C escape used: `\\` (32950 occurrences); no string concatenation | T1 (data) |
| `include/newstroke_font.h` | 33 | externs | T1 |
| `common/font/stroke_font.cpp` | 307 | `STROKE_FONT`: decoding (L95-187), interline (L190-195), layout of one run `GetTextAsGlyphs` (L198-281) | T1 |
| `include/font/stroke_font.h` | 119 | super/subscript constants (L108-110) | T1 |
| `common/font/glyph.cpp` | 180 | `STROKE_GLYPH` build/transform/move (L29-124); `OUTLINE_GLYPH` bbox/triangulation (L127-180) | T1 (stroke), T2 (outline) |
| `include/font/glyph.h`, `glyph_types.h` | 51, 112 | `GLYPH` base (hover flag), `STROKE_GLYPH : vector<vector<VECTOR2D>>`, `OUTLINE_GLYPH : SHAPE_POLY_SET` | T1/T2 |
| `common/font/font.cpp` | 704 | `FONT` base: font registry (L133-176), line positions (L179-240), `Draw` (L243-267), markup layout `drawMarkup` (L273-407), `drawSingleLineText` (L410-439), `StringBoundaryLimits` (L442-469), `GetAlignedDrawPosition` (L472-485), `boundingBoxSingleLine` (L488-500), word/line breaking (L509-704); markup LRU cache (L59-125) | T1 |
| `include/font/font.h` | 312 | `TEXT_STYLE` flags, `ITALIC_TILT = 1/8`, virtual interface | T1 |
| `include/font/font_metrics.h` | 65 | `METRICS`: interline 1.68, overbar 1.23, underline -0.16 | T1 |
| `common/font/text_attributes.cpp` / `.h` | 130 / 166 | `TEXT_ATTRIBUTES` defaults (TA:22-38), `Compare` (TA:41-104), alignment enums (`GR_TEXT_H_ALIGN_T` -1/0/1/indeterminate) | T1 |
| `include/font/kicad_font_name.h` | 27 | `KICAD_FONT_NAME` = `"KiCad Font"` | T1 |
| `common/markup_parser.cpp`, `include/markup_parser.h` | 83, 135 | PEGTL grammar and parse tree | T1 |
| `common/callback_gal.cpp`, `include/callback_gal.h` | 77, 88 | adaptor that feeds glyphs to stroke / outline / triangle callbacks | T1 |
| `common/gr_text.cpp`, `include/gr_text.h` | 172, 126 | pen size helpers (GR:33-94), `BOLD_STROKE_MULTIPLIER = 1.6` (`gr_text.h:82`), `GetKnockoutTextMargin` (`gr_text.h:88-91`), `GRTextWidth`; `GRPrintText` is wxDC printing (out of scope) | T1 (helpers) |
| `common/eda_text.cpp` / `include/eda_text.h` | 1467 / 554 | `EDA_TEXT`: attributes, pen width, shown-text cache, render cache, bbox, line positions, effective shape, `Format`, `Compare` | T1 (protobuf `Serialize`/`Deserialize` L170-235, `Print`, hyperlink/URL helpers, `Levenshtein`, property registration L1349-1467: out of scope) |
| `pcbnew/pcb_text.cpp` | 952 | `PCB_TEXT`: shown text with variables, draw rotation / keep-upright, footprint-transform getters, knockout, polygonisation | T1 (protobuf, menus, `ViewGetLOD`, `ShowSyntaxHelp`, property desc: out) |
| `pcbnew/pcb_field.cpp` | 397 | `PCB_FIELD` (inherits `PCB_TEXT`): variant-aware shown text (PF:144-210) | T1 |
| `pcbnew/pcb_textbox.cpp` | 1147 | `PCB_TEXTBOX`: draw position from box corners and margins, wrapped shown text, polygonisation | T1 (shapes), T2 (rotated boxes) |
| `pcbnew/pcb_dimension.cpp` | 2546 | text parts only: `updateText` per type, `GetValueText` (DIM:491-530), shapes | T2 (the rest is board-model M12) |
| `common/plotters/plotter.cpp` | 667 | `PLOTTER::Text` (P:548-615), `PLOTTER::PlotText` (P:618-667) | T1 |
| `common/plotters/GERBER_plotter.cpp` | - | `GERBER_PLOTTER::Text/PlotText` (G:1807-1846): net attributes, then base | T1 |
| `pcbnew/plot_brditems_plotter.cpp` | 1656 | `BRDITEMS_PLOTTER::PlotText` (B:786-896); callers `PlotPadNumber` L84-153, `PlotFootprintTextItems` L401-487, `PlotBoardGraphicItem` L489-567, `PlotDimension` L569-622, `PlotFootprintGraphicItems` L678-784 | T1 |
| `common/font/outline_font.cpp` | 630 | FreeType face loading, HarfBuzz shaping, glyph contour assembly | T3 (pure-Rust engine), T2 (render cache only) |
| `common/font/outline_decomposer.cpp` | 182 | `FT_Outline_Decompose` callbacks, Bezier flattening, winding | T3 |
| `common/font/fontconfig.cpp` / `.h` | 507 / 147 | font lookup / substitution (`FindFont` L208-388), `ListFonts` (UI) | T3 (policy only; no fontconfig in wasm) |
| `common/font/version_info.cpp` | 62 | library version strings | out (UI) |
| `common/embedded_files.cpp` | 748 | embedded font files: `UpdateFontFiles` L650-661 writes temp files for fontconfig | T3 (font part) |
| `common/text_eval/*` | 4314 | `@{...}` expressions (`EXPRESSION_EVALUATOR`), also invoked from `PLOTTER::Text/PlotText` (P:566-570, P:629-633) and `GRTextWidth` | T3 (see settings.md 4.14) |
| `common/common.cpp` L60-360 | - | `ExpandTextVars`, `ResolveTextVars`, `FinalizeTextVarExpansion` | owned by settings.md package `common/text_vars.rs` |
| `thirdparty/pegtl` | - | PEGTL 3 (only `parse_tree` with selector is used) | replaced by a hand-written parser |

Out of scope for the whole document: GAL/OpenGL/Cairo painters (`pcb_painter.cpp` draws knockout
via `PCB_TEXT::GetKnockoutCache`, PT:721-754 - display only), wxDC printing, hyperlinks and hover,
PDF/PS/DXF native text, schematic text, drawing-sheet text, `version_info`.

---

## 3. Data structures

### 3.1 `TEXT_ATTRIBUTES` (TAH:121-150)

| Member | Default (TA:22-38) | Notes |
|---|---|---|
| `m_Font` | `nullptr` | null = "use default", resolves to the stroke font |
| `m_Halign`, `m_Valign` | `CENTER`, `CENTER` | enum values -1/0/1 are used arithmetically (`-GetHorizJustify()` flips) |
| `m_Angle` | `ANGLE_0` | **`EDA_ORIENTATION`** (`eda_angle.h:496-575`): always normalised to [0,360), `-0` -> `0` on assignment |
| `m_LineSpacing` | 1.0 | multiplier on interline |
| `m_StrokeWidth` | 0 | stored thickness; 0 = auto. Also used by `FONT::getLinePositions` as a *position fudge* (4.4) |
| `m_Italic`, `m_Bold`, `m_Underlined`, `m_Hover` | false | `m_Hover` UI only |
| `m_Color` | `COLOR4D::UNSPECIFIED` | written to file, no geometry |
| `m_Mirrored` | false | mirror around the text origin (X only) |
| `m_Multiline` | true | for `EDA_TEXT` the parser/constructors of board items set it; `FONT::Draw` splits on `\n` regardless |
| `m_Size` | `(0,0)` | `VECTOR2I` (x = width, y = height) |
| `m_KeepUpright` | false | |

`Compare` (TA:41-104) orders by font name (`wxString::Cmp`), size x, size y, stroke width, angle,
line spacing, halign, valign, italic, bold, underlined, colour (`COLOR4D::Compare`), mirrored,
multiline, keep-upright; integer differences are returned as `a - b` (sign only matters).
`std::hash<TEXT_ATTRIBUTES>` (TAH:155-164) is not needed (no output depends on it).

### 3.2 Fonts

* `KIFONT::FONT` (FH): `m_fontName`, `m_fontFileName`; statics `s_defaultFont` (the stroke font,
  created lazily by `getDefaultFont`, F:133-141) and `s_fontMap`
  (`std::map<tuple<name,bold,italic,forDrawingSheet>, FONT*>`, F:57) caching `GetFont` results.
  Lookup only; never iterated.
* `STROKE_FONT` (SFH:53-115): pointers to the process-global glyph table
  `g_defaultFontGlyphs` / `g_defaultFontGlyphBoundingBoxes` (SF:47-50), filled once.
  Constants: `m_superSubSize = 0.8`, `m_subscriptBaselineOffset = 0.15`,
  `m_superscriptBaselineOffset = -0.35` (SFH:108-110); file constants
  `STROKE_FONT_SCALE = 1/21`, `FONT_OFFSET = -8` (SF:41-44).
* `OUTLINE_FONT` (OFH): `m_face` (FT_Face), `m_faceSize = 16`, `m_fakeBold`, `m_fakeItal`,
  `m_forDrawingSheet`. Constants (OFH:199-243): `m_outlineFontSizeCompensation = 1.4`,
  `m_charSizeScaler = 64`, `m_subscriptSuperscriptSize = 0.64`, `m_underlineOffsetScaler = -0.16`,
  `m_subscriptVerticalOffset = -0.25`, `m_superscriptVerticalOffset = 0.45`;
  `faceSize() = (int)(16 * 64 * 1.4) = 1433`, `subscriptSize() = KiROUND(1433 * 0.64) = 917`.
  `GLYPH_RESOLUTION = 1152`, `GLYPH_SIZE_SCALER = 72.0 / 1152` (ODH:42-47).

### 3.3 Glyphs

* `STROKE_GLYPH` (GT:80-104): `std::vector<std::vector<VECTOR2D>>` (one inner vector per pen-down
  polyline), `m_penIsDown` (build state, **not** copied by the copy constructor, GL:29-37),
  `m_boundingBox` (`BOX2D`). Glyph-table entries are in "font units" (cap height ~1.0, y down,
  baseline y = 0); `Transform` produces board-IU `VECTOR2D` points.
* `OUTLINE_GLYPH` (GT:39-77): a `SHAPE_POLY_SET` (outlines with holes, integer points) plus
  triangulation cache. The render cache and `formatRenderCache` round-trip these.
* `GLYPH::m_isHover` - UI only.

### 3.4 Markup tree (`MARKUP::NODE`, MP:40-53)

PEGTL `parse_tree::basic_node`: `children` (ordered), node type (one of `anyString`,
`anyStringWithinBraces`, `url`, `subscript`, `superscript`, `overbar`, or root), and content
(`string()` = the exact matched UTF-8 bytes) present only on `anyString`, `anyStringWithinBraces`
and `url` nodes (selector, MP:107-113). Markup nodes carry no content and are **discarded when they
have no children** (`discard_empty`, `thirdparty/pegtl/pegtl/contrib/parse_tree.hpp:399-411`).
Rust: `enum MarkupKind { Root, AnyString, AnyStringWithinBraces, Url, Subscript, Superscript,
Overbar }` + `content: Option<String>` + `children: Vec<MarkupNode>`.

### 3.5 `EDA_TEXT` (ETH)

| Member | Meaning |
|---|---|
| `m_text` | raw text as in the file (escaped: `{slash}`, `{dblquote}`, ... see `EscapeString`) |
| `m_shown_text` | `UnescapeString( m_text )`, maintained eagerly by `cacheShownText` (ET:605-629) |
| `m_shown_text_has_text_var_refs` | shown text contains `${` or `@{` |
| `m_text_var_refs` | `ExtractTextVarReferences( m_text )` (reactive tracker; not needed for output) |
| `m_hyperlink` | `(href ...)` |
| `m_attributes` | `TEXT_ATTRIBUTES` (3.1) |
| `m_pos` | position (library frame for footprint children since `FIRST_FP_AFFINE_TRANSFORM` = 20260616) |
| `m_visible` | |
| `m_unresolvedFontName` | `(face ...)` as read, until `ResolveFont` |
| `m_render_cache` | `unique_ptr<EDA_TEXT_RENDER_CACHE_DATA>` (ETH:46-54): `text`, `font`, `angle`, `offset`, `mirrored`, `glyphs` - outline fonts only |
| `m_bbox_cache` | `std::map<int line, {pos, bbox}>` guarded by a mutex; key = line index (-1 = all lines), valid while `drawPos` is unchanged |
| `m_IuScale` | `pcbIUScale` for board items |

Every attribute setter clears the render cache and the bbox cache (ET:237-513). Constants
(ETH:57-84): `TEXT_MIN_SIZE_MM = 0.001`, `TEXT_MAX_SIZE_MM = 250.0`, `DEFAULT_SIZE_TEXT = 50` mils.

### 3.6 Board text items

* `PCB_TEXT : BOARD_ITEM, EDA_TEXT` - adds `m_libTextAngle` (`EDA_ORIENTATION`, lib frame),
  knockout flag (`BOARD_ITEM::m_isKnockout`), `m_knockout_cache` (painter only). Getters
  `GetTextPos`, `GetTextSize`, `GetTextThickness`, `GetTextAngle` are **virtual overrides that
  apply the parent footprint's `TRANSFORM_TRS`** (PT:463-583) - and `EDA_TEXT` internals call
  them virtually (`GetTextBox`, `GetEffectiveTextPenWidth`, `GetDrawPos`).
* `PCB_FIELD : PCB_TEXT` - `m_id` (`FIELD_T`), name; variant override of the value (PF:151-210).
* `PCB_TEXTBOX : PCB_SHAPE, EDA_TEXT` - box (rectangle or 4-point polygon when rotated by a
  non-cardinal angle), margins (`GetMarginLeft/Top/Right/Bottom`), `m_borderEnabled`,
  `m_libTextAngle`, `m_libShape`. `PCB_TABLECELL : PCB_TEXTBOX` (tables are tier 2).
* `PCB_DIMENSION_BASE : PCB_TEXT` (`pcb_dimension.h:116`) - the dimension *is* its text item;
  plus `m_shapes` (segments/circles of the graphic), value/prefix/suffix/units/precision.

---

## 4. Control flow and algorithms

### 4.1 Stroke font loading (SF:95-187)

Run once (mutex `g_defaultFontLoadMutex`) on first `FONT::GetFont()`:

For each glyph string `s` (index j):

1. First two chars: `glyphStartX = (s[0] - 'R') / 21`, `glyphEndX = (s[1] - 'R') / 21`,
   `glyphWidth = glyphEndX - glyphStartX` (all `double`; `STROKE_FONT_SCALE` multiplies, i.e.
   `(c - 'R') * (1.0/21.0)`, **not** a division - keep the multiplication for bit-exactness).
2. Remaining pairs `(a, b)`: `" R"` = pen up (`RaisePen`); otherwise a point
   `x = (double)(a - 'R') * STROKE_FONT_SCALE - glyphStartX`,
   `y = (double)(b - 'R' + FONT_OFFSET) * STROKE_FONT_SCALE` (`AddPoint`: starts a new polyline
   when the pen is up, GL:40-50). Characters are `char` (signed on x86; all data is ASCII).
3. `buildGlyphBoundingBox` (SF:77-92): `min = (0, 0)`, `max = (glyphWidth, 0)`, then
   `min.y`/`max.y` widened over all points (x is **not** widened - glyphs may overhang their box);
   box = `BOX2D( min, max - min )`.
4. `m_maxGlyphWidth` is only updated during the first load (unused by layout).

Glyph 0 is the space (`"JZ"`: width 16/21); `space_width` in layout = `front().GetWidth()`.

Rust: `OnceLock<StrokeFontData>`; the table can be taken with `include_str!` of
`common/newstroke_font.cpp` parsed at first use (only `\\` unescaping), or a generated data file
(about 2.37 MB unescaped). Either way the decoded `f64` values must be produced by the same
expression. Decision to be reviewed (11, T14).

### 4.2 Markup parsing (MP:55-114, MPC:28-49)

Grammar (PEGTL, ordered choice, backtracking):

```
markup                = subscript / superscript / overbar
anyString             = ( !markup !url utf8_any )+
escapeSequence        = '{' identifier '}'          identifier = [A-Za-z_][A-Za-z0-9_]*
anyStringWithinBraces = ( !markup escapeSequence / !markup [^}] )+     (utf8 code points)
url                   = ( "http://" / "https://" ) [^ ]+
braces<C>             = C '{' until( '}', anyStringWithinBraces / subscript / superscript / overbar )
superscript = braces<'^'>   subscript = braces<'_'>   overbar = braces<'~'>
anything              = anyString / url / subscript / superscript / overbar
grammar               = until( eof, anything )
```

`until(R, S)`: loop { if R matches -> success (R consumed); else S must match, else fail }.
Consequences (mostly covered by `test_markup_parser.cpp::Parse`):

* An unterminated `~{` fails `braces` and is consumed as plain text by `anyString` (`!markup`
  succeeds), e.g. `"A string ~{incomplete markup ~{overbar}"` -> `anyString("A string ~{incomplete
  markup ")`, `overbar[ "overbar" ]`.
* `~{}` produces a markup node without children, which is discarded: no glyph, **no bar**.
* `{IDENT}` inside braces is consumed as a unit, so `^{a{b}c}` is one run `"a{b}c"`; `{b c}` is not
  an identifier, so its `}` closes the markup.
* URLs are only recognised at top level and swallow everything up to the next space (including
  `}` and markup characters).
* Parsing of valid UTF-8 never fails (`Parse` returns `nullptr` only on a PEGTL exception or an
  invalid byte sequence; `TO_UTF8( wxString )` is always valid). With a null root
  `::drawMarkup` draws nothing and returns the start position.

Nodes appear in source order; child order defines glyph emission order (4.3) and therefore the
order of Gerber draws. Implement as a hand-written recursive-descent parser over `&str`
(char-wise), mirroring each rule including backtracking; no PEG crate is needed.

### 4.3 Markup layout: `::drawMarkup` (F:273-380)

`FONT::drawMarkup` (F:383-407) fetches the parse tree from the LRU cache `s_markupCache` (1024
entries, F:123; pure memoisation, no output effect) and calls the recursive `::drawMarkup( bbox,
glyphs, node, position, font, size, angle, mirror, origin, textStyle, metrics )`:

For a non-root node:

1. `textStyle |= SUBSCRIPT` if subscript, else `|= SUPERSCRIPT` if superscript; `drawOverbar` if
   overbar; `drawUnderline` if the *inherited* style has `UNDERLINE` (so every nested node draws
   its own underline).
2. If the node has content: `nextPosition = font->GetTextAsGlyphs( &bbox, glyphs, content, size,
   nextPosition, angle, mirror, origin, textStyle )`; `aBoundingBox->Merge( bbox )` (BOX2 `m_init`
   semantics: the first merge into an uninitialised box copies).
3. Recurse into children in order, threading `nextPosition`.

After the children (also for the root, but root never sets the flags):

* Underline bar: `barTrim = size.x * 0.1`; `barOffset = metrics.GetUnderlineVerticalPosition(
  size.y )` (= `size.y * -0.16`); `start = (pos.x + barTrim, pos.y - barOffset)`,
  `end = (next.x - barTrim, next.y - barOffset)` as `VECTOR2D`; a 2-point `STROKE_GLYPH` is
  pushed after `Transform( {1,1}, {0,0}, 0.0, angle, mirror, origin )` (its bounding box is a
  default, uninitialised `BOX2D`).
* Overbar: the same with `GetOverbarVerticalPosition( size.y )` (= `1.23 * size.y`).
  The bar uses the **full** `aSize` even inside sub/superscript, and `aPosition.y` is the line
  baseline because `GetTextAsGlyphs` returns `(cursor.x, aPosition.y)`.

Emission order per node: own content glyphs, children (recursively), underline, overbar.
Bars do not enlarge `aBoundingBox`.

### 4.4 Multi-line placement: `FONT::getLinePositions` (F:179-240) and `FONT::Draw` (F:243-267)

`Draw( gal, text, position, cursor=(0,0), attrs, metrics )`:

1. Return if `text` is empty.
2. `getLinePositions( text, position - cursor, ... )`:
   * `wxStringSplit( text, lines, '\n' )` (`string_utils.cpp:1549-1568`): an empty trailing part is
     dropped (`"a\n"` -> `["a"]`, `""` -> `[]`), inner empty parts are kept.
   * `int interline = GetInterline( size.y, metrics ) * m_LineSpacing` - **truncation** to int.
     Stroke: `size.y * 1.68 * 0.9583` (`LEGACY_FACTOR`, SF:190-195, a function-local `static double`);
     outline: `size.y * 1.68` (OF:175-180).
   * per line `extents[i] = boundingBoxSingleLine( line, pos_i ) - pos_i` where
     `pos_i = (x, y + i*interline)`; `boundingBoxSingleLine` (F:488-500) is `drawMarkup` with
     `bbox = nullptr`, `glyphs = nullptr`, angle 0, no mirror, style = italic only (no bold).
   * `int height`: line 0 adds `size.y * 1.17` (int += double, **truncation**), later lines add
     `interline`.
   * `offset = (0, size.y)`; for stroke fonts `offset.x += m_StrokeWidth / 1.52` and
     `offset.y -= m_StrokeWidth * 0.052` (both int compound assignments from double: truncation
     toward zero). `m_StrokeWidth` is the value in the attributes passed in - see 4.10 for
     which callers pass the stored (possibly 0) thickness and which pass the effective width.
   * valign: TOP none, CENTER `offset.y -= height / 2` (int division), BOTTOM `offset.y -= height`.
   * per line: `lineOffset = offset; lineOffset.y += i * interline`; halign LEFT keeps
     `offset.x`, CENTER `lineOffset.x = -lineSize.x / 2` (**replaces** the stroke fudge),
     RIGHT `lineOffset.x = -( lineSize.x + offset.x )`; position = `position + lineOffset`.
3. `gal->SetLineWidth( m_StrokeWidth )`, then for each line `drawSingleLineText( gal, nullptr,
   line, positions[i], size, angle, mirrored, origin = aPosition (not minus cursor), italic,
   underlined, ... )` (F:410-439): style = ITALIC|UNDERLINE (bold is never passed to glyph
   layout), `drawMarkup` into a local glyph list, then `gal->DrawGlyphs( glyphs )` (glyphs in
   order; `CALLBACK_GAL::DrawGlyph`, CG:29-77).

Rotation and mirroring are therefore applied per glyph point about the **text anchor**
(`aPosition`), never per line.

`StringBoundaryLimits( text, size, thickness, bold, italic, metrics )` (F:442-469): `drawMarkup`
into a default `BOX2I` with position (0,0), angle 0, style BOLD/ITALIC; stroke fonts then
`Inflate( KiROUND( thickness * 1.5 ) )`; returns the size. `GetAlignedDrawPosition` returns
`positions.front()` (or the anchor).

### 4.5 Stroke run layout: `STROKE_FONT::GetTextAsGlyphs` (SF:198-281)

Inputs: `aSize` (`VECTOR2I`), `aPosition`, angle, mirror, origin, style.

```
TAB_WIDTH = 4; INTER_CHAR = 0.2
cursor (VECTOR2I) = aPosition; glyphSize (VECTOR2D) = aSize
tilt = ITALIC ? 1/8 : 0.0; space_width = bbox[0].GetWidth(); char_count = 0
if SUB or SUPER:  glyphSize = glyphSize * 0.8
                  cursor.y += glyphSize.y * (SUB ? 0.15 : -0.35)       // int += double: truncation
for c in text (wxUniChar = full code point on Linux):
  '\t': char_count = (char_count / 4 + 1) * 4 - 1
        int new_cursor = aPosition.x + aSize.x * char_count + aSize.x * space_width   // int+int*int, then + double, truncated
        while aSize.x > 0 && new_cursor <= cursor.x: char_count += 4; new_cursor += aSize.x * 4
        cursor.x = new_cursor
  ' ' : cursor.x += KiROUND( glyphSize.x * space_width )
  else: dd = c - ' '; if dd < 0 || dd >= 65487: c = '?'  (dd = 31)
        if glyphs: push source.Transform( glyphSize, cursor, tilt, angle, mirror, origin )
        cursor.x += KiROUND( source.BoundingBox().GetEnd().x * glyphSize.x )  // VECTOR2D *= VECTOR2D, then KiROUND of x
  ++char_count
if bbox: SetOrigin( aPosition ); SetEnd( cursor.x - KiROUND( glyphSize.x * 0.2 ), cursor.y - glyphSize.y ); Normalize()
return ( cursor.x, aPosition.y )
```

Traps: tabs use the full `aSize` (not the super/sub `glyphSize`) and `aPosition.x` (the run
start, not the line start); characters < U+0020 other than tab (e.g. `\r`) render as `?`; code
points above U+FFEE render as `?`; the advance is rounded **per glyph** (accumulated rounding).
Bold never changes stroke geometry (only the pen width, 4.9).

### 4.6 Stroke glyph transform (GL:75-112)

`STROKE_GLYPH::Transform( glyphSize, offset, tilt, angle, mirror, origin )` copies the glyph and:

* bbox: `end = bbox.GetEnd() * glyphSize` (component-wise); `if tilt != 0: end.x -= end.y * tilt`;
  `SetEnd( end )`; `Offset( offset )`.
* each point: `point *= glyphSize` (component-wise); `if tilt != 0: point.x -= point.y * tilt`;
  `point += offset` (`VECTOR2I` -> `VECTOR2D`); `if mirror: point.x = origin.x - ( point.x -
  origin.x )`; `if !angle.IsZero(): RotatePoint( point, origin, angle )` - the `double`
  overload (`trigo.cpp:279-328`): normalise, exact cases for 0/90/180/270, otherwise
  `x' = y*sin + x*cos`, `y' = y*cos - x*sin` with `EDA_ANGLE::Sin/Cos` (exact values at multiples
  of 45 degrees, `eda_angle.h:177-215`).

Order matters: scale, shear, translate, mirror, rotate. `Move( offset )` (GL:115-124) only adds.

### 4.7 Glyph sink: `CALLBACK_GAL::DrawGlyph` (CG:29-77)

Three constructors (`callback_gal.h:29-66`): (stroke, triangle) -> `m_stroke = true,
m_triangulate = true`; (stroke, outline) -> `true, false`; (outline only) -> `false, false`.

* Stroke glyph, `m_stroke`: for each polyline, for each consecutive pair, call the stroke callback
  with **`VECTOR2I` arguments converted from `VECTOR2D` by the `VECTOR2` converting constructor
  (`vector2d.h:84-108`): clamp to the `int` range, then `static_cast` = truncation toward zero**
  (not `KiROUND`). Single-point polylines emit nothing.
* Stroke glyph, `!m_stroke` (only the outline-only constructor; not used by board paths):
  `TransformOvalToPolygon( poly, p0, p1, GetLineWidth(), GetLineWidth() / 180, ERROR_INSIDE )`
  per segment, callback with `poly.Outline( 0 )`.
* Outline glyph, `m_triangulate`: `OUTLINE_GLYPH::Triangulate` -> triangles (needs the
  triangulation port, `kicad-geom` G4b).
* Outline glyph, otherwise: copy, `Fracture()` if it has holes, one callback per outline.

In Rust this is a `trait GlyphSink { fn stroke(&mut self, a: Vector2I, b: Vector2I);
fn outline(&mut self, c: &ShapeLineChain); fn triangle(&mut self, a, b, c); }` plus a
`draw_glyphs(&[Glyph], mode, sink)` function reproducing the dispatch.

### 4.8 `FONT::LinebreakText` (F:612-704) and `wordbreakMarkup` (F:509-609)

Used by `PCB_TEXTBOX::GetShownText` (every time the shown text is requested).

1. `spaceWidth = GetTextAsGlyphs( " " ).x` with style BOLD/ITALIC (no effect on stroke advances).
2. Split on `\n` (`wxStringSplit`); the result string is rebuilt, `\n` re-inserted between
   source lines (`ii != count - 1`; note that a dropped empty trailing line is not restored).
3. Per line, `wordbreakMarkup` re-parses the line (no cache) and walks the tree:
   * a sub/sup/overbar node yields **one** word `"_{"`/`"^{"`/`"~{"` + own content + child words
     (children with `aInsideMarkup = true`, i.e. verbatim) + `"}"`, width = sum of the content
     and child widths measured with style = exactly `SUBSCRIPT` or `SUPERSCRIPT` (assignment, not
     `|=`; bold/italic dropped) `| OVERBAR`;
   * inside markup: plain node content verbatim, its width;
   * top level plain text: `wxStringTokenizer( text, " ", wxTOKEN_RET_DELIMS )` tokens (each token
     keeps its trailing delimiter; consecutive spaces give `" "` tokens - verify the exact wx 3.2
     semantics with the oracle harness, 11 T9); width measured on `token.Trim()` (trailing
     whitespace removed) unless that is empty;
   * then the node's children are visited with the current style.
4. Glue: a run is appended to the previous word unless the previous word ends with `' '`.
5. Greedy fill: `pendingSpaceWidth = pendingSpaces.Length() * spaceWidth`;
   `overflow = lineWidth + pendingSpaceWidth + wordWidth > aColumnWidth - aThickness`;
   on overflow with pending spaces -> `\n`, reset, `buryMode` (drop the pending spaces); a word
   that is exactly `" "` accumulates into `pendingSpaces`; a word ending in `' '` contributes
   `word.Left( len - 1 )` and sets `pendingSpaces = " "`. A single word wider than the column is
   never split.

### 4.9 Pen width and size rules

`EDA_TEXT::GetEffectiveTextPenWidth( aDefault = 0 )` (ET:428-449), using the *virtual*
`GetTextThickness()`, `GetTextWidth()`, `GetTextSize()`:

```
pen = GetTextThickness()
if pen <= 1:
    pen = aDefault
    if IsBold():          pen = GetPenSizeForBold( GetTextWidth() )      // KiROUND( w / 5.0 )
    else if pen <= 1:     pen = GetPenSizeForNormal( GetTextWidth() )    // KiROUND( w / 8.0 )
else if IsBold() && isStrokeFont():
    pen = KiROUND( pen * 1.6 )                                            // BOLD_STROKE_MULTIPLIER
pen = ClampTextPenSize( pen, GetTextSize() )   // min(|x|,|y|) -> min( pen, KiROUND( size * 0.25 ) ) (GR:69-90)
```

`isStrokeFont()` (ET:277-288): resolved font -> `IsStroke()`; else unresolved face name ->
`FONT::IsStroke( name )` (`"Default Font"` translated or `"KiCad Font"`); else true.

`GetKnockoutTextMargin( size, thickness ) = max( KiROUND( thickness / 2.0 ), KiROUND( size.y / 9.0 ) )`.

`EDA_TEXT::SetTextSize( size, enforce = true )` (ET:513-533) clamps each component to
`[mmToIU( 0.001 ), mmToIU( 250 )]` (skipped for `unityScale`). `SetTextWidth/Height` always clamp.
`PCB_TEXT::SetTextSize` first divides by the footprint scale (PT:486-496).

Footprint transform (PT:463-583, needs `TRANSFORM_TRS` from the board model):
`GetTextPos = xf.Apply( lib_pos )`; `GetTextSize = ( KiROUND( lib.x * |sx| ), KiROUND( lib.y *
|sy| ) )`; `GetTextThickness = KiROUND( lib_t * ( |sx| + |sy| ) * 0.5 )`; `GetTextAngle =
( m_libTextAngle + fp orientation ).Normalize()`.

### 4.10 Board text to geometry - the three paths

The same text is turned into geometry along three slightly different paths. **They do not
produce identical coordinates** and each must be ported exactly as is.

**(a) Effective shape for DRC** - `EDA_TEXT::GetEffectiveTextShape( triangulate = true, bbox =
empty, angle = 0 )` (ET:1143-1220):

* `font = GetDrawFont( nullptr )` (resolved font, else `FONT::GetFont( "", bold, italic )` = stroke);
  `penWidth = GetEffectiveTextPenWidth()`; `shownText = GetShownText( FOR_CANVAS )`;
  `drawPos = GetDrawPos()`; `attrs = GetAttributes()`, `attrs.m_Size = GetTextSize()`.
* `attrs.m_StrokeWidth` stays the **stored** thickness (0 for auto) -> affects the
  `/1.52` and `*0.052` fudges of 4.4.
* bbox given (tables, not tier 1): `drawPos = bbox.GetCenter()`, CENTER/CENTER, `m_Angle = aAngle`.
  Otherwise `m_Angle = GetDrawRotation()`, and for outline fonts `cache = GetRenderCache( font,
  shownText, (0,0) )`.
* Sink: stroke -> `SHAPE_SEGMENT( p1, p2, penWidth )`; triangulate -> `SHAPE_SIMPLE` per triangle
  (outline -> `SHAPE_LINE_CHAIN` clone when not triangulating).
* `cache ? DrawGlyphs( *cache ) : font->Draw( &gal, shownText, drawPos, attrs, metrics )`.

`PCB_TEXT::GetEffectiveShape` (PT:706-718): knockout -> `TransformTextToPolySet( poly, 0,
GetMaxError(), ERROR_INSIDE )` as `SHAPE_POLY_SET`; else `GetEffectiveTextShape()`.
`PCB_TEXTBOX::GetEffectiveShape` adds `PCB_SHAPE::GetEffectiveShape` when stroke width >= 0
(TB:852-861). Dimensions: `m_shapes` + text shape clone (DIM:870-880).

**(b) Polygonisation** - `PCB_TEXT::TransformTextToPolySet( buf, clearance, maxError, errorLoc )`
(PT:784-846):

* `attrs.m_Angle = GetDrawRotation()`, `attrs.m_Size = GetTextSize()`, `m_StrokeWidth` stored.
* Sink: stroke -> `TransformOvalToPolygon( textShape, p1, p2, penWidth, maxError, errorLoc )`
  (each oval appended as a new outline); triangle -> new 3-point outline.
* `GetRenderCache( font, shownText )` (outline fonts) else `font->Draw( ..., GetTextPos(), attrs,
  GetFontMetrics() )` - note `GetTextPos()` here vs `GetDrawPos()` in (a) (equal for `PCB_TEXT`).
* `textShape.Simplify()` (Clipper2 union).
* Knockout: `buildBoundingHull( &final, textShape, margin + clearance )` (PT:756-782: rotate a
  copy by `-GetDrawRotation()` about `GetDrawPos()`, `BBox( margin )`, 4 corners rotated back,
  appended as one outline) then `final.BooleanSubtract( textShape )`, appended.
  `margin = GetKnockoutTextMargin( attrs.m_Size, penWidth )`.
* Otherwise if `clearance > 0 || errorLoc == ERROR_OUTSIDE`: `clearance += maxError` for
  `ERROR_OUTSIDE`, `textShape.Inflate( clearance, ROUND_ALL_CORNERS, maxError )` (no simplify
  flag); append.

`PCB_TEXT::TransformShapeToPolygon` (PT:857-865): text polygons with clearance 0, then only the
rotated **bounding hull** (`buildBoundingHull( buf, poly, clearance )`) is appended - this is what
the zone filler uses for non-knockout text (zone_filler.cpp:2038-2060; knockout text: copy with
knockout off, `TransformTextToPolySet( holes, 0, maxError, ERROR_INSIDE )`).

`PCB_TEXTBOX::TransformTextToPolySet` (TB:878-945) differs: `attrs = GetAttributes()` **without**
overriding `m_Angle` or `m_Size` (raw attribute values), drawn at `GetDrawPos()`; knockout:
`TransformShapeToPolygon( final, layer, clearance, ... )`, then `final.Rotate( GetTextAngle(),
GetCenter() )` if the angle is non-zero, subtract text; otherwise
`Inflate( clearance, ROUND_ALL_CORNERS, maxError, true )` (simplify = true).
`PCB_TEXTBOX::TransformShapeToPolygon` (TB:955-1000): the filled box (rectangle corners from
`GetRectCorners()` or the polygon outline) plus, when the border is enabled and
`width = GetWidth() + 2 * clearance > 0`, an oval per side.

**(c) Plotting** - `BRDITEMS_PLOTTER::PlotText( text, layer, knockout, metrics, strikeout )`
(B:786-896):

* skip if the shown text is empty (except a knockout text box) or the layer is not plotted;
  copper layers get the `.AperFunction,NonConductor` attribute.
* `attrs = GetAttributes()`, `m_Size = GetTextSize()`, **`m_StrokeWidth =
  GetEffectiveTextPenWidth()`** (differs from (a)/(b) for auto thickness), `m_Angle =
  GetDrawRotation()`, `m_Multiline = false`; `SetCurrentLineWidth( m_StrokeWidth )`.
* knockout: `TransformTextToPolySet( final, 0, board maxError, ERROR_INSIDE )` (text or text box),
  `Fracture()`, `PlotPoly( outline, FILLED_SHAPE, 0 )` per outline.
* outline font and fonts **not** embedded: draw `*GetRenderCache( font, shownText )` with
  stroke -> `ThickSegment`, polygon -> `PlotPoly`.
* multiline allowed: split shown text on `\n`, positions from **`EDA_TEXT::GetLinePositions`**
  (ET:932-974: `offset.y = GetInterline()` = `KiROUND( interline * lineSpacing )`; first line
  moved by `(n-1) * offset.y / 2` (CENTER) or `(n-1) * offset.y` (BOTTOM); `RotatePoint( pos,
  GetDrawPos(), rot )` and `RotatePoint( offset, rot )` (int overloads, `KiROUND`)), then one
  `PLOTTER::PlotText( positions[i], color, line, attrs, font, ... )` per line - each line is a
  separate `FONT::Draw` with its own rotation origin, so multi-line coordinates differ from
  path (a), which lays out all lines in one `Draw` with truncated interline.
* single line: `PlotText( GetTextPos(), ... )`; strikeout (fab layers, DNP) as in B:819-837.

`PLOTTER::PlotText` (P:618-667): evaluate `@{...}` if present (tier 3), `SetColor`,
`SetCurrentLineWidth( penWidth )`, bold with pen 0 -> `GetPenSizeForBold( min( size ) )`,
negative pen -> absolute; stroke callback = `MoveTo( p1 ); LineTo( p2 ); PenFinish()`; polygon
callback = `PlotPoly( FILLED_SHAPE, 0 )`; no font -> `GetFont( renderSettings default font )`;
`font->Draw( &gal, text, pos, attributes, metrics )`. `PLOTTER::Text` (P:548-615, used by pad
numbers and SVG): the same but the stroke callback calls `SetCurrentLineWidth( penWidth )` per
segment and a negative `size.x` means mirrored. `GERBER_PLOTTER::Text/PlotText` (G:1807-1846)
first emit the net attributes of the metadata.

### 4.11 `EDA_TEXT::GetTextBox( settings, line = -1 )` (ET:755-888)

1. Cache hit when `m_bbox_cache[line].m_pos == GetDrawPos()`.
2. `text = GetShownText( FOR_CANVAS )`, `thickness = GetEffectiveTextPenWidth()`; multiline:
   split, `text = lines[line]` if `0 <= line < n`, else `lines[0]` (if any).
3. `extents = font->StringBoundaryLimits( text, size, thickness, bold, italic )`;
   `fudge = KiROUND( extents.y * 0.17 )`; stroke: `textsize.y += fudge`.
4. `interline = KiROUND( font->GetInterline( size.y ) * lineSpacing )`;
   `if multiline && line > 0 && line < n: pos.y -= line * interline` (sic: minus).
5. `overbarOffset = extents.y / 6` if `text` (the selected line) contains `"~{"`.
6. all lines (`line < 0`, `n > 1`): `textsize.x = max` over lines 1.. of their extents;
   `textsize.y += ( n - 1 ) * interline`. Then `textsize.y += overbarOffset`.
7. `italicOffset = italic ? KiROUND( size.y * ITALIC_TILT ) : 0`; halign LEFT: mirrored ->
   `x -= width - italicOffset`; CENTER: `x -= ( width - italicOffset ) / 2`; RIGHT: not mirrored ->
   `x -= width - italicOffset`.
8. valign TOP: `Offset( 0, -fudge )`; CENTER: `y -= height / 2`; BOTTOM: `y -= height`,
   `Offset( 0, fudge )`. `Normalize()`; store in cache.

The box is unrotated; `PCB_TEXT::GetBoundingBox` (PT:413-425) inflates by the knockout margin and
applies `GetBoundingBoxRotated( GetTextPos(), GetDrawRotation() )`.

### 4.12 Rotation, keep-upright, mirroring, shown text

* `PCB_TEXT::GetDrawRotation` (PT:222-241): footprint child with keep-upright -> fold into
  `]-90, 90]` by repeated +-180; else `Normalize()` ([0, 360)).
* `PCB_TEXT::KeepUpright` (PT:392-410, edit-time) and `Mirror`/`Flip` (PT:597-663) change stored
  data only (not on the load/plot/DRC path, but needed for footprint flip in the board model).
* `PCB_TEXT::GetShownText` (PT:180-213): `EDA_TEXT::GetShownText` (cached unescaped text); if it
  has variables and the context is not `RAW_VALUE`: `ResolveTextVars( text, resolver, depth )`
  with resolver `LAYER` -> `GetLayerName()`, then parent `FOOTPRINT::ResolveTextVar`, then
  `BOARD::ResolveTextVar`; then `FinalizeTextVarExpansion`. `PCB_FIELD` (PF:151-210) uses the
  board's current variant, takes the variant field value (no variable expansion) when present,
  and resolves with the variant-aware resolvers. Built-in variables and precedence:
  `settings.md` 4.14. `PCB_TEXTBOX::GetShownText` (TB:497-541) resolves the same way and then
  wraps with `LinebreakText( text, colWidth, GetTextSize(), GetEffectiveTextPenWidth(), bold,
  italic )` where `colWidth = |corners[1] - corners[0]|` (`EuclideanNorm`, rounded) minus the
  left+right margins (horizontal text) or top+bottom margins.
* `PCB_TEXTBOX::GetDrawPos` (TB:365-462): anchor = corner/edge midpoint/centre of
  `GetCornersInSequence( drawAngle )` chosen by the (mirror-flipped) justification, plus margins
  rotated by `GetDrawRotation()` (int `RotatePoint`). Integer midpoints `( a + b ) / 2`,
  centre `( c0 + c1 + c2 + c3 ) / 4` (truncating).

### 4.13 Dimension text (tier 2, DIM)

`PCB_DIMENSION_BASE::updateText` (DIM:443-465): value (`m_valueString` when overridden else
`GetValueText()`), unit suffix by `m_unitsFormat` (none, `GetText( units )`, ` (units)`),
prefix, suffix, `SetText`. `GetValueText` (DIM:491-530): precision table (>= 6 means "auto" by
units), `wxString::Printf( "%.<p>f", ToUserUnit( pcbIUScale, units, measured ) )` (glibc printf
rounding - use the ported C formatter), optional zero suppression using `localeconv()` (CLI: `C`
locale, `.`). Position/angle: `PCB_DIM_ALIGNED::updateText` (DIM:1243-1282: OUTSIDE offset by
`GetEffectiveTextPenWidth() + GetTextHeight()` perpendicular, INLINE at crossbar centre;
keep-aligned angle folded into `]-90, 90]`), `PCB_DIM_ORTHOGONAL::updateText` (DIM:1493-1525),
leader (DIM:1667-1690), radial (DIM:1876-1894), center (DIM:2066-2072). `updateGeometry` uses
`GetTextBox( nullptr ).Inflate( ... )` to cut the crossbar around the text (DIM:1211, 1459, 1687,
1927). `ClearRenderCache` re-runs `Update()` (DIM:468-482) - the Rust port must not recurse.

### 4.14 Outline fonts: what FreeType/HarfBuzz/fontconfig are asked to do (tier 3)

1. **Lookup** (`FONT::GetFont`, F:144-168): empty name or a name starting with `"KiCad Font"` ->
   stroke font. Otherwise `s_fontMap` hit or `OUTLINE_FONT::LoadFont` (OF:93-122); on failure the
   stroke font (cached under the key as well).
2. **fontconfig** (`FindFont`, FC:208-388): a name containing bold/heavy/black/thick/dark forces
   `aBold`; embedded font files are added with `FcConfigAppFontAddFile` (EMBEDDED_FILES `m_files`
   is a `std::map`, so in name order); pattern `FC_FAMILY = name` (+ `FC_WEIGHT_BOLD`,
   `FC_SLANT_ITALIC`), `FcConfigSubstitute` + `FcDefaultSubstitute`, `FcFontMatch`. Fontconfig
   **always returns some font** (closest match) - a missing family is silently substituted (on the
   oracle host `fc-match "Ubuntu Sans"` and `fc-match Lato` give `Inter-Regular.otf`, `Calibri`
   gives `Carlito`). The style string decides `has_bold`/`has_ital`; missing styles set
   `FF_MISSING_BOLD/ITAL/BOLD_ITAL` -> fake bold/italic. `famStrings` is an `unordered_map`, but
   the loop only decides OK vs SUBSTITUTE (log text), not geometry.
3. **Face** (`loadFace`, OF:125-144): `FT_New_Face( file, faceIndex )`; `SelectCharmap`
   (OF:147-172: Unicode unless it cannot map any of `A a 0 space`, then MS Symbol, else Unicode);
   `FT_Set_Char_Size( face, 0, 1433, 1152, 0 )`.
4. **Run layout** (`GetTextAsGlyphs`, OF:254-299): splits at tabs; tab stop
   `KiROUND( size.x * 2.4 )`, `position.x += tab - ( position.x - origin.x ) % tab`.
   `getTextAsGlyphsUnlocked` (OF:466-630) under the FreeType mutex:
   * `scaler = supersub ? 917 : 1433`; `FT_Set_Char_Size( face, 0, scaler, 1152, 0 )`.
   * shaping (`getHarfbuzzShape`, OF:348-382, cached by (face, UTF-8 text, scaler)):
     `hb_buffer_add_utf8`, `hb_buffer_guess_segment_properties`, `hb_ft_font_create_referenced`,
     `hb_shape( font, buf, nullptr, 0 )` (default features), glyph infos and positions copied.
   * `scaleFactor = ( size.x / 1433, -size.y / 1433 ) * 1.4` (always `faceSize()`, also for
     super/sub); `origin = aPosition`, super/sub: `origin.y += size.y * 0.64 * ( -offset * 1.4 )`.
   * per glyph: contours from the glyph cache (key: face, glyph id, scaleFactor, forDrawingSheet,
     fakeItal, fakeBold, mirror, supersub, angle) or `LoadGlyphContours` (OF:424-463): fake italic
     -> `FT_Set_Transform` with a 12-degree shear (`float` angle, `FT_Fixed` matrix); `FT_Load_Glyph(
     face, gid, FT_LOAD_NO_BITMAP )` (**hinting enabled**, default load target); non-outline or
     error -> a "tofu" box contour pair (OF:507-531); fake bold -> `FT_Outline_Embolden( 64 )`;
     `OUTLINE_DECOMPOSER::OutlineToSegments` (OD:150-170): points in 26.6 units * `72/1152`,
     conic/cubic segments flattened with `BEZIER_POLY::GetPoly( out, m_FontErrorSize = 2.0 )`
     (`bezier_curves.cpp`), repeated points dropped, each contour tagged with
     `FT_Outline_Get_Orientation` of the **whole outline** and its own shoelace winding
     (OD:173-182: +1 if `sum((x2-x1)(y2+y1)) > 0`).
   * point transform: `pt = ( v + cursor ) * scaleFactor + origin`, mirror about `aOrigin.x`,
     `RotatePoint( pt, aOrigin, angle )` (double), then `SHAPE_LINE_CHAIN::Append( int, int )`
     (`shape_line_chain.h:518`): the doubles are converted implicitly, i.e. **truncated**.
   * **hole fix-up**: contour filled iff (TrueType orientation and winding +1) or (PostScript and
     winding -1); holes are attached to the **first** outline whose `PointInside( hole[0] )`, else
     added as outlines ("lovely TTF fonts", OF:568-588).
   * triangulation: first instance per cache key triangulates, later instances reuse it as a hint
     (`CacheTriangulation( hintData )`).
   * `cursor += ( x_advance, y_advance ) * 72/1152` into a `VECTOR2I` (truncation).
   * bbox: merge `aPosition - ( 0, ascender * |scale.y| )` and `aPosition + ( cursor.x * scale.x,
     descender * |scale.y| )` (ascender/descender from `face->size->metrics`, truncated to int).
   * returns `( aPosition.x + cursor.x * scale.x, aPosition.y - cursor.y * scale.y )` (truncated).
   * `OUTLINE_FONT::GetInterline` = `size * 1.68` (no LEGACY_FACTOR).
5. **Whole text for caches**: `OUTLINE_FONT::GetLinesAsGlyphs` (OF:232-251) = `getLinePositions`
   + `drawMarkup` per line with origin = text position, used by `EDA_TEXT::GetRenderCache`.

**Pure-Rust assessment.** `ttf-parser` (outline access, metrics, name table) and `rustybuzz`
(HarfBuzz port, shaping) are pure Rust and wasm-friendly (MIT/Apache). They cannot reproduce
the C++ output bit for bit: FreeType applies TrueType bytecode / CFF hinting at ~358 ppem
(`FT_LOAD_NO_BITMAP` without `FT_LOAD_NO_HINTING`) and `hb-ft` takes hinted, 26.6-rounded advances
from FreeType; `FT_Outline_Embolden` and the 26.6 scaling would have to be ported by hand. There
are no system fonts in wasm, so only embedded fonts could be rendered at all; substitution on the
oracle host is host-specific. Verdict: tier 2 = use stored render caches; tier 3 = optional
`ttf-parser` + `rustybuzz` renderer for embedded fonts, documented as approximate (sub-micron
differences), never used where a valid render cache exists.

### 4.15 Render cache (tier 2)

* **Read** (`parseRenderCache`, PP:903-950): `(render_cache "text" angle (polygon (pts ...)) ...)`
  -> `SetupRenderCache( text, GetFont() /* still null */, angle, (0,0) )`; each `(polygon ...)`:
  first `(pts ...)` outline, later ones holes; `AddRenderCacheGlyph( poly )` triangulates
  immediately (ET:738-746).
* **Resolve** (`finishItem`, PP:1137-1147 -> `ResolveFont`, ET:480-502): `m_attributes.m_Font =
  GetFont( unresolved name, bold, italic, embedded font files )`, and **if the cache has glyphs
  its font is set to the resolved font** so it stays valid.
* **Validity** (`GetRenderCache`, ET:684-720, outline fonts only): rebuilt unless glyphs present
  and `font`, `text == shown text`, `angle == GetDrawRotation()`, `offset`, `mirrored` all equal.
  Position is not part of the key: `EDA_TEXT::Offset` (ET:576-596) moves cached glyphs.
* **Board-level text loses its cache at load**: `parsePCB_TEXT` calls `SetVisible( true )` for
  non-footprint text (PP:4654-4656), and `EDA_TEXT::SetVisible` clears the render cache
  (ET:348-352). Footprint text keeps it unless a later setter / `OnFootprintTransformed`
  (PT:635-640, called from `FOOTPRINT::SetPosition` etc., `footprint.cpp:3708-3740`) clears it.
* **Write** (`formatRenderCache`, W:573-598, only when the resolved font is outline):
  `GetRenderCache` (regenerates if stale), quoted shown text, `FormatAngle( GetDrawRotation() )`,
  then outline-only `CALLBACK_GAL` -> one `(polygon (pts ...))` per fractured outline.
* **Consumers**: (a) `GetEffectiveTextShape`, (b) `TransformTextToPolySet`, (c) plotting when the
  font is outline and fonts are not embedded (B:855-872), DRC text_dims (`:136`).

Rust tier-2 rule: an outline-font text with a cache whose key matches is fully reproducible
(polygons verbatim; triangulation via `kicad-geom`). Everything else (board-level outline text,
texts whose shown text changed, embedded-font plotting) needs the font engine: report an
explicit "outline font not available" gap and fall back to the stroke font only behind a
documented, non-default option (C++ falls back to the stroke font only when fontconfig fails).

### 4.16 Order-sensitive places (summary)

| Place | Container / order | Effect on output | Port |
|---|---|---|---|
| Glyph emission (4.3, 4.5) | `std::vector`, node order then bars | order of Gerber `D02/D01`, SVG paths, polygons fed to `Simplify` | same order |
| Line order (4.4, 4.10c) | `wxArrayString` | order of draws | same |
| `TransformTextToPolySet` polygons | appended in glyph/segment order, then `Simplify` (Clipper2) | vertex order/start of resulting outlines | vendored clipper2-rs |
| Hole assignment (4.14) | first matching outline | polygon structure | same loop |
| `s_fontMap`, `s_markupCache`, HarfBuzz/glyph caches | `std::map` / LRU / `unordered_map` | none (memoisation), except triangulation hint reuse (structure identical) | plain maps or no cache |
| `famStrings` (FC:274-339) | `unordered_map` | log message only | n/a |
| `EMBEDDED_FILES::m_files` | `std::map` (sorted by name) | order fonts are offered to fontconfig (tie-breaks) | sorted |
| `m_bbox_cache` | `std::map<int,...>` | none | |
| Plot loops over items | see plot-fab.md | item order in files | |
| Thread scheduling | DRC/zone workers call text shape code concurrently | none (pure functions; caches are memoisation) | sequential |

No `std::sort` is involved in this subsystem.

---

## 5. What this subsystem needs from other subsystems

**kicad-geom** (all must exist with C++-exact numerics):

* `Vector2I`/`Vector2D` incl. the converting constructor semantics (double -> int: clamp then
  truncate; int -> double), component-wise `*`/`*=`, `+=` with mixed types, `EuclideanNorm`,
  `Resize`; `ki_round`.
* `Box2I`/`Box2D` with the `m_init` flag semantics: `SetOrigin`, `SetEnd`, `SetSize`, `SetX/Y`,
  `Offset`, `Merge( box )`, `Merge( point )`, `Normalize`, `Inflate( d )`, `Inflate( dx, dy )`,
  `GetInflated`, `GetEnd`, `GetCenter`, `GetBoundingBoxRotated( center, angle )`, `Contains`,
  `Intersects( box, rotation )`.
* `EdaAngle` (`Normalize`, `Normalized`, `IsZero`, `IsHorizontal`, `Sin`/`Cos` with exact
  multiples of 45, `Reflect`, `AsDegrees`), `EdaOrientation`.
* `trigo`: `RotatePoint` int overloads (`KiROUND`) and double overloads; `GetRotated`.
* `ShapeLineChain` (`Append( x, y )`, `SetClosed`, `PointInside`, `Clone`, `SegmentCount`,
  `GetSegment`), `ShapePolySet` (`NewOutline`, `Append`, `AddOutline`, `AddHole`, `Simplify`,
  `Inflate( amount, CORNER_STRATEGY::ROUND_ALL_CORNERS, maxError, simplify )`, `BooleanSubtract`,
  `Fracture`, `Rotate( angle, center )`, `Move`, `BBox( clearance )`, `HasHoles`, `OutlineCount`,
  `CacheTriangulation`, `TriangulatedPolygon`, hint-based triangulation), `ShapeSegment`,
  `ShapeSimple`, `ShapeCompound` (`AddShape`, `Clone`).
* `TransformOvalToPolygon( buf, a, b, width, maxError, errorLoc )`,
  `TransformCircleToPolygon` (dimensions); `ERROR_LOC`; `ARC_LOW_DEF`, `ARC_HIGH_DEF`.
* `BezierPoly::get_poly_d( out, max_error )` (tier 3, `libs/kimath/src/bezier_curves.cpp`).

**kicad-core**: `pcbIUScale` (`mmToIU`, `MilsToIU`), `EscapeString`/`UnescapeString`
(`string_utils.cpp:399-...`), `wxStringSplit` equivalent, UTF-8 helpers, `FormatInternalUnits`,
`FormatDouble2Str`, `FormatAngle`, C-printf formatter (dimension values), `EDA_UNIT_UTILS::GetText`
/ `ToUserUnit`, `COLOR4D` (+ `Compare`), text-variable expansion (`common/text_vars.rs` of the
settings plan: `ExpandTextVars`, `ResolveTextVars`, `FinalizeTextVarExpansion`,
`m_ResolveTextRecursionDepth = 6`), `ADVANCED_CFG::m_FontErrorSize = 2.0` (constant).

**Board model (kicad-pcb)**: `BOARD_ITEM::GetLayer`, `GetLayerName`, `IsKnockout`,
`IsSideSpecific`, `GetMaxError` (board `m_MaxError`, default `ARC_HIGH_DEF`), `GetFontMetrics`
(always `METRICS::Default()`, `board_item.cpp:174-177`), `GetParentFootprint`, `GetBoard`;
`FOOTPRINT::GetTransform()` (`TRANSFORM_TRS::Apply`, `InverseApply`, `GetScaleX/Y`),
`GetOrientation`, `ResolveTextVar( token, [variant,] depth )`, `GetVariant`,
`GetFieldValueForVariant`; `BOARD::ResolveTextVar`, `GetCurrentVariant`, `GetEmbeddedFiles()`
(`GetAreFontsEmbedded`, font files), `GetDesignSettings().m_MaxError`; `PCB_SHAPE`
(text box geometry: `GetStart/End`, `GetRectCorners`, `GetPolyShape`, `GetStroke`,
`GetEffectiveShape`, `GetCenter`, `GetWidth`), `EDA_SHAPE::GetCornersInSequence`.

**Plotter (kicad-plot)**: `SetColor`, `SetCurrentLineWidth`, `MoveTo/LineTo/PenFinish`,
`ThickSegment`, `PlotPoly( chain, FILLED_SHAPE, 0, data )`, `RenderSettings()->GetDefaultFont()`,
Gerber net-attribute formatting.

**DRC / zone**: none needed by this subsystem; they are consumers (section 1).

**Settings**: none beyond the above (`BOARD_DESIGN_SETTINGS` text defaults are only used by
`StyleFromSettings`, an edit-time function).

---

## 6. Output formats

The subsystem has no file format of its own. It shapes:

* Gerber: stroke text = one `D02`+`D01` pair per glyph segment with the current round aperture of
  diameter = effective pen width (plot-fab.md 4.4; `PLOTTER::PlotText` selects the width once per
  call, `PLOTTER::Text` per segment); knockout and outline text = `G36`/`G37` regions
  (`PlotPoly` filled). Coordinates come from the truncated `VECTOR2I` stroke points (4.7).
* Board file (owned by pcb-io): `EDA_TEXT::Format` (ET:1081-1140) writes
  `(effects (font [(face "..")] (size H W) [(line_spacing x)] [(thickness t)] [(bold yes)]
  [(italic yes)] [(color r g b a)]) [(justify [left|right] [top|bottom] [mirror])] [(href "..")])`
  - note size is height then width; `(render_cache ...)` as in 4.15.

---

## 7. Backward compatibility

| Behaviour | Where | Rule |
|---|---|---|
| Bold stroke width baked into thickness in old files | PP:899-900, ET:333-345 | file version `< 20260826`: bold stroke text with thickness > 1 -> `max( 2, KiROUND( t / 1.6 ) )`; `isStrokeFont` uses the unresolved face name here (fonts are resolved later in `finishItem`) |
| Missing text size | PP:891-897 | `1.524 mm` square (legacy 60 mils) |
| Hidden board text | PP:4654-4656 | forced visible (also drops the render cache, 4.15) |
| Hidden footprint text | PP:4646-4651 | converted to a hidden user field |
| `%V` / `%R` in old texts | pcb-io.md | -> `${VALUE}` / `${REFERENCE}` |
| 6.0 positioning fudges | F:200, F:211-212, SF:192, ET:791 | `1.17`, `/1.52`, `*0.052`, `0.9583`, `0.17`: must stay |
| Footprint children in board frame (files `< 20260616`) | pcb-io.md / board-model.md | conversion to lib frame before text getters apply the transform |
| Font name `"KiCad Font"` prefix | F:149 | any face starting with it is the stroke font |
| Render cache from a host with different fonts | 4.15 | used verbatim while valid; regenerated on save otherwise |

---

## 8. Multi-threading

* DRC providers and the zone filler call `GetEffectiveShape` / `TransformTextToPolySet` from
  thread-pool workers. Shared state is guarded: `g_defaultFontLoadMutex` (font table init),
  `s_defaultFontMutex`, `s_markupCacheMutex` (the whole markup layout runs under it!),
  `m_bbox_cacheMutex`, `OUTLINE_FONT::m_freeTypeMutex` (shaping + glyph load), PCB_TEXT knockout
  cache. `m_render_cache` is `mutable` and unguarded (benign in practice).
* Results do not depend on scheduling: every computation is a pure function of the item; caches
  only memoise. The only cache whose *content* depends on which call came first is the outline
  glyph triangulation hint (4.14), and it reproduces the same triangle topology.
* Sequential Rust port: plain functions, `OnceLock` for the stroke table, no global caches needed
  (optional memoisation behind `&mut` caches keyed exactly as C++). `EDA_TEXT`'s render/bbox
  caches become `KeyedCache`s per board-model.md section on caches.

---

## 9. Existing tests and data, differential tests

### 9.1 C++ unit tests to port

| File | Cases | What it asserts | Package |
|---|---|---|---|
| `qa/tests/common/test_kicad_stroke_font.cpp` (95) | `TabCheck` (L40-78): 5 pairs of tab-separated strings at size 1000 reach the same x; `ZeroSizeTab` (L81-92): size 0 with a tab returns x = 0 (no infinite loop) | tab stops | TF3 |
| `qa/tests/common/test_markup_parser.cpp` (186) | `Parse` (L67-129): 9 inputs -> tree dumps (incomplete markup, mixed runs); `OverbarMultipleSpacesWidth` (L135-183): space widths > 0, `LinebreakText` preserves `~{     }`, `_{     }`, `^{     }` etc. (issue 22913) | grammar, word breaking | TF2, TF3 |
| `qa/tests/common/test_text_attributes.cpp` (147) | `Compare`: ordering by font (non-null > null), halign, valign, angle, line spacing, ... and hash inequality | `TEXT_ATTRIBUTES::Compare` (port the ordering; hash checks may be dropped with a note) | TF3 |
| `qa/tests/common/test_eda_text.cpp` (237) | `Compare` (L42-75); `BoldFlagLeavesStoredThicknessAlone`; `EffectiveWidthAppliesBoldMultiplierForStroke` (10 -> 16); `EffectiveWidthAutoBoldMatchesLegacy`; `MigrateLegacyBoldStrokeWidthRecoversBase` (32 -> 20 with effective 32; 2 stays 2 with effective 3); `AutoThicknessOffFreezesBaseNotEffective`; 5 `TextVarReferences_*` cases (L165-235, reactive tracker; port only if `ExtractTextVarReferences` is ported) | pen width rules, migration | TF4 |
| `qa/tests/pcbnew/test_pcb_textbox.cpp` (295) | `GetMinSize*` (6 cases, L70-176), rotation round trips (L179-246), `EmptyKnockoutPlotsTheBox` (L248-292: an empty knockout text box plots exactly one polygon = the box) | text box geometry and knockout plotting | TF6, TF8 |
| `qa/tests/pcbnew/test_plot_footprint_scaled_text.cpp` (165) | footprint scaled x2 -> plotted silk bbox doubles (12 % tolerance), for the reference field and a footprint text | footprint-transform getters in plotting | TF5, TF8 |
| `qa/tests/pcbnew/test_issue25074_odb_knockout_textbox.cpp` (92) + `qa/data/pcbnew/issue25074/knockout_textbox.kicad_pcb` | ODB++ export (out of scope); reuse the board for Gerber/DRC differential tests | knockout text box | TF6 |
| `qa/tests/pcbnew/test_text_variable_resolution.cpp` (164) | title block > project vars > board properties | resolver chain | settings / TF5 |
| `qa/tests/common/test_expand_text_vars.cpp` (649) | expansion engine | settings package |

### 9.2 Differential oracle (required by PORTING.md 9)

**Unit-level harness** (`crates/kicad-font/tests/oracle/`): a C++ driver linked against
`/home/user/kicad-oracle/build/common/gal/libkigal.so` (fonts, `CALLBACK_GAL`),
`common/libkicommon.so` (markup parser, newstroke table, string utils) and `libkimath.a`, using
the stub-wx approach of `kicad-geom/tests/oracle`. Sections to emit (one line per value, `%.17g`
for doubles):

1. Glyph table: for every glyph index, the bbox and all decoded points (or a hash per glyph plus
   full dumps of ~500 sampled glyphs incl. all ASCII, Latin-1, Greek, CJK samples).
2. `GetTextAsGlyphs` over a corpus (ASCII words, tabs at various positions, `\r`, non-BMP, empty,
   sizes 0 / 1 / 1000 / 1270000 / non-square / negative, italic, sub/super) -> returned point,
   bbox, glyph points.
3. Markup trees for a fuzz corpus (random strings over `~_^{}` letters, spaces, `http://`).
4. `FONT::Draw` through a recording `CALLBACK_GAL`: the `VECTOR2I` stroke segments for
   combinations of halign x valign x angle (0, 45, 90, 180, 270, 33.3) x mirror x italic x
   underline x stroke width (0, 1, 150000) x multi-line x line spacing.
5. `StringBoundaryLimits`, `LinebreakText` (column widths sweep), `GetInterline`.

**Item-level harness** (`crates/kicad-pcb/tests/oracle/`, needs `libpcbcommon.a`/pcbnew objects,
or via `kicad-cli`): `GetTextBox`, `GetBoundingBox`, `GetEffectiveTextShape` segments,
`TransformTextToPolySet` polygons (vertex lists after `Simplify`) for texts parsed from board
files. If linking pcbnew objects is too heavy, compare through DRC and Gerber outputs below.

**CLI-level** (`docs/ORACLE.md`), stroke-font boards only:

* `kicad-cli pcb export gerbers -l F.SilkS,B.SilkS,F.Fab,B.Fab,F.Cu` on silkscreen-heavy boards:
  `demos/vme-wren`, `demos/multichannel/multichannel_mixer.kicad_pcb`, `demos/pic_programmer`,
  `demos/video`, `demos/interf_u` (dimensions), `demos/constraints`, `qa/data/pcbnew/issue5093.kicad_pcb`
  (markup), `qa/data/pcbnew/issue25074/knockout_textbox.kicad_pcb`,
  `qa/data/pcbnew/odbpp/knockout_textbox_unicode.kicad_pcb`, `qa/data/pcbnew/api_kitchen_sink.kicad_pcb`
  (knockout text, text boxes, fields), `qa/data/pcbnew/issue24442.kicad_pcb` (text boxes).
  Byte-compare after the usual date/version normalisation.
* `kicad-cli pcb drc --format json --severity-all` on the same boards (silk overlap, silk over
  copper, edge clearance with text, text_height/thickness).
* `kicad-cli pcb export svg` for a few of them (SVG text is the same stroke path).
* Outline-font boards (render caches: `demos/royalblue54L_feather`, `demos/tiny_tapeout`,
  `demos/jetson-agx-thor-baseboard`, `qa/data/pcbnew/issue14549.kicad_pcb`,
  `qa/data/pcbnew/issue22475/issue22475.kicad_pcb`): compare footprint text from caches; expect and
  document mismatches for board-level outline text (re-rendered with host substitute fonts).

---

## 10. Rust module layout and work packages

### 10.1 Layout

```
crates/kicad-font/src/
  lib.rs                 re-exports; Font enum { Stroke(&'static StrokeFont), Outline(OutlineFontRef) }
  newstroke.rs           glyph table source + decoding                 (newstroke_font.cpp, stroke_font.cpp L95-187)
  stroke_font.rs         StrokeFont: interline, get_text_as_glyphs      (stroke_font.cpp, stroke_font.h)
  glyph.rs               StrokeGlyph, OutlineGlyph, Glyph enum          (glyph.cpp, glyph.h, glyph_types.h)
  markup.rs              MarkupNode + parser                            (markup_parser.cpp/.h)
  font.rs                layout: line positions, draw_markup, draw, string_boundary_limits,
                         wordbreak, linebreak_text                      (font.cpp, font.h)
  font_metrics.rs        Metrics                                        (font_metrics.h)
  text_attributes.rs     TextAttributes, HAlign/VAlign, compare         (text_attributes.cpp/.h)
  glyph_sink.rs          GlyphSink trait + draw_glyphs dispatch         (callback_gal.cpp/.h)
  gr_text.rs             pen size helpers, knockout margin, constants   (gr_text.cpp L33-94, gr_text.h)
  registry.rs            FontRegistry: get_font(name, bold, italic, embedded) -> Font   (font.cpp L133-176)
  outline/ (feature "outline", tier 3)
    outline_font.rs      ttf-parser + rustybuzz port of outline_font.cpp
    decomposer.rs        outline_decomposer.cpp
    font_match.rs        family/style matching replacing fontconfig (fontconfig.cpp FindFont policy)
crates/kicad-pcb/src/ (board-model packages M10-M12, text parts)
  eda_text.rs            EdaText data, pen width, text box, line positions, effective shape, render cache
  pcb_text.rs pcb_field.rs pcb_textbox.rs pcb_dimension.rs   (shape functions described in 4.10-4.13)
crates/kicad-fab/src/plot_brditems/text.rs   BRDITEMS_PLOTTER::PlotText (plot-fab.md packages)
crates/kicad-plot/src/plotter.rs             PLOTTER::Text / PlotText
```

The font registry replaces the C++ statics: the board (or a `TextContext`) owns a
`FontRegistry` built from the board's embedded font files; `EdaText` stores a resolved
`FontId`/`Font` instead of a raw pointer. Without the `outline` feature every non-stroke face
resolves to `Font::Outline(Unavailable { name })`, which is usable only through a valid render
cache.

### 10.2 Work packages

Ordered so that a thin slice (stroke text in Gerber silkscreen and text clearance in DRC) works
after TF1-TF5 + TF8.

| Id | Title | C++ files (line ranges) | Est. C++ lines | Depends on | Tier | Validating tests |
|---|---|---|---|---|---|---|
| TF1 | Newstroke table and stroke glyphs | `newstroke_font.cpp` (data L150-65735, read via `include_str!` or generated), `stroke_font.cpp` L40-187, L284-307, `stroke_font.h`, `glyph.cpp` L29-124, `glyph.h`, `glyph_types.h` L80-104, `kicad_font_name.h` | 450 (+ data) | kicad-geom G1/G2 (vectors, boxes, angles, RotatePoint) | 1 | oracle section 1; glyph count 65487; space width 16/21 |
| TF2 | Markup parser | `markup_parser.h`, `markup_parser.cpp`, PEGTL semantics of `until`/`plus`/`not_at`/selector (`parse_tree.hpp` L360-420) | 220 | none | 1 | `test_markup_parser.cpp::Parse`; oracle section 3 (fuzz) |
| TF3 | Font layout core | `font.cpp` L44-704, `font.h`, `font_metrics.h`, `stroke_font.cpp` L190-281, `text_attributes.cpp`, `text_attributes.h`, `callback_gal.cpp`, `callback_gal.h`, `gr_text.cpp` L33-94, `gr_text.h` L60-95 | 1350 | TF1, TF2, kicad-geom (TransformOvalToPolygon only for the outline-only sink mode) | 1 | `test_kicad_stroke_font.cpp`, `test_markup_parser.cpp::OverbarMultipleSpacesWidth`, `test_text_attributes.cpp`; oracle sections 2, 4, 5 |
| TF4 | EdaText geometry (part of board-model M10) | `eda_text.cpp` L70-95, L237-760 (setters, pen width, sizes, Offset, cacheShownText, GetDrawFont, render-cache API), L755-1015 (GetTextBox, hit tests, GetLinePositions), L1081-1252 (Format, GetEffectiveTextShape, Compare), `eda_text.h` | 1300 | TF3, board-model M0, kicad-core text vars, kicad-geom shapes | 1 | `test_eda_text.cpp`; item oracle (`GetTextBox`, effective shape) |
| TF5 | PcbText / PcbField shapes (part of M10) | `pcb_text.cpp` L180-241, L367-666, L706-866; `pcb_field.cpp` L127-212 | 850 | TF4, M19 (footprint transform, ResolveTextVar), M22 (board text vars), kicad-geom G4a (Simplify, Inflate, BooleanSubtract) | 1 | `test_text_variable_resolution.cpp`; DRC JSON and silk Gerber diffs on stroke-font boards |
| TF6 | PcbTextbox text (part of M11) | `pcb_textbox.cpp` L174-465, L497-541, L580-720, L852-1018 | 900 | TF4, TF5 (shared helpers), TF3 (LinebreakText), M9 (PcbShape) | 1 (rectangles), 2 (rotated polygons) | `test_pcb_textbox.cpp`; `issue25074/knockout_textbox.kicad_pcb`, `issue24442.kicad_pcb` Gerber + DRC |
| TF7 | Dimension text (part of M12) | `pcb_dimension.cpp` L443-532, L601-660, L870-880, L935-965, L1028-1056, L1243-1282, L1493-1525, L1667-1690, L1876-1894, L2066-2072 | 450 | TF5, M12 geometry, kicad-core C formatter / units | 2 | `demos/interf_u`, `api_kitchen_sink` Gerber diffs |
| TF8 | Text plotting | `plotter.cpp` L548-667, `GERBER_plotter.cpp` L1807-1846, `plot_brditems_plotter.cpp` L84-153, L401-567, L569-622 (text part), L678-896 | 700 | TF3-TF6, kicad-plot engine (plot-fab packages) | 1 | `test_plot_footprint_scaled_text.cpp`, `test_pcb_textbox.cpp::EmptyKnockoutPlotsTheBox`; CLI Gerber diffs (9.2) |
| TF9 | Render cache and outline-font fallback | `eda_text.cpp` L472-503, L670-746; `pcb_io_kicad_sexpr_parser.cpp` L899-950, L1137-1147, L4640-4660; `pcb_io_kicad_sexpr.cpp` L573-598; `glyph.cpp` L127-180; `plot_brditems_plotter.cpp` L855-872; `font.cpp` L144-176 | 400 | TF4, pcb-io reader/writer, kicad-geom G4b (triangulation, hint reuse) | 2 | re-save byte equality of render caches (`demos/royalblue54L_feather`, `tiny_tapeout`, `jetson`); Gerber diffs of footprint outline text |
| TF10 | Outline font engine (optional) | `outline_font.cpp` (all), `outline_font.h`, `outline_decomposer.cpp`, `outline_decomposer.h`, `fontconfig.cpp` L208-388 (policy), `embedded_files.cpp` L650-661 (font list), `bezier_curves.cpp` (GetPoly double) | 1700 | TF3, TF9, ttf-parser, rustybuzz (to be approved in THIRD_PARTY_EQUIVALENTS.md) | 3 | tolerance-based comparison against oracle render caches (no bit-exactness claim) |
| TF11 | Font oracle harness | new: `crates/kicad-font/tests/oracle/{build_oracle.sh,font_oracle.cpp}` + goldens; drives sections 9.2.1-5 | 0 (new C++ ~500) | TF1 (to start), extended by TF2/TF3 | 1 | itself; replayed by Rust tests |

Notes on boundaries:

* TF4-TF7 are the text halves of board-model packages M10/M11/M12 (`design/board-model.md`);
  the orchestrator should assign them to the same implementers or split M10-M12 along these
  lines. The line ranges above are the text-specific parts.
* TF8 overlaps with the plot-fab packages for `plot_brditems_plotter.cpp`; take only the text
  functions here.
* Thin slice: TF1 -> TF2 -> TF3 (with TF11 sections 1-4) -> TF4 -> TF5 -> TF8 gives stroke text in
  silkscreen Gerbers and correct `GetEffectiveShape` for DRC.

---

## 11. Risks and traps

* **T1 - truncation, not rounding, of stroke points.** `CALLBACK_GAL` stroke callbacks receive
  `VECTOR2D` points converted to `VECTOR2I` by truncation (4.7). Using `ki_round` there shifts
  every Gerber coordinate by up to 1 nm. Same for `cursor.y += double`, `height += size * 1.17`,
  `offset.x += sw / 1.52`, `offset.y -= sw * 0.052`, `int interline = ... * spacing`, and tab
  positions (`int new_cursor = ... + double`).
* **T2 - per-glyph advance rounding.** `cursor.x += KiROUND( width * size.x )` per glyph and
  `KiROUND( glyphSize.x * space_width )` per space; do not sum in `f64`.
* **T3 - the three geometry paths differ** (4.10): DRC/polygon paths pass the *stored* thickness
  (0 = auto) into `TEXT_ATTRIBUTES::m_StrokeWidth`, plotting passes the *effective* width; DRC lays
  out multi-line text in one `Draw` with truncated interline, plotting draws line by line with
  `KiROUND` interline and rotated positions. Port each path literally; never share a "cleaned up"
  helper that changes one of them.
* **T4 - text box attributes.** `PCB_TEXTBOX::TransformTextToPolySet` uses the raw attributes
  (lib size, stored angle) unlike `PCB_TEXT`; `Inflate` there passes `simplify = true`.
* **T5 - virtual getters.** `EDA_TEXT` code calls `GetTextSize`, `GetTextThickness`,
  `GetTextPos`, `GetDrawPos`, `GetDrawRotation`, `GetShownText` virtually; in Rust pass a context
  (`&TextCtx` with footprint transform, board, layer name) or the item enum, and make sure every
  `EDA_TEXT` helper uses the overridden value exactly where C++ does, but uses
  `m_attributes` directly where C++ does (`GetAttributes()`, `IsBold`, alignment).
* **T6 - rotation order and exact trigonometry.** Glyph points: scale, shear, translate, mirror,
  rotate around the text anchor with the double `RotatePoint` and `EDA_ANGLE::Sin/Cos` special
  cases; line positions in the plot path use the int `RotatePoint` (`KiROUND`).
* **T7 - `EDA_ORIENTATION` normalisation.** `m_Angle` is always in [0, 360) and `-0` becomes `0`;
  `PCB_TEXT::GetDrawRotation` may return negative angles (keep-upright) which are then stored into
  `attrs.m_Angle` and normalised again. Sin/cos of the normalised value must be used.
* **T8 - markup edge cases.** Empty markup draws nothing (no bar); unterminated markup is literal
  text; `{IDENT}` escape inside braces; URLs at top level eat `}`; bars use the full text size;
  underline is drawn once per nested node.
* **T9 - wx semantics in LinebreakText.** `wxStringTokenizer( " ", wxTOKEN_RET_DELIMS )` token
  boundaries, `wxString::Trim()` whitespace set, `EndsWith`, `Length()` in code points. Verify
  against the oracle harness rather than reading wx docs.
* **T10 - code points.** wxString on Linux is UTF-32: iterate `char`s; anything below U+0020
  except tab, or above U+FFEE, renders as `?`. Byte length must never be used for counts.
* **T11 - render caches.** Board-level outline text loses its cache at load and is re-rendered
  with whatever font fontconfig substitutes on the host (`Inter` on the oracle host for
  `Ubuntu Sans` and `Lato`): Gerbers and DRC of such boards are host-dependent and cannot be
  matched without the same font files. Footprint text caches survive load only while no setter /
  `OnFootprintTransformed` runs after `parseRenderCache` - verify the footprint parse order with
  the oracle (`pcb upgrade` re-save of the outline-font demos must reproduce the caches).
* **T12 - fake bold/italic and hinting.** FreeType hinting, `hb-ft` advances and
  `FT_Outline_Embolden` make a pure-Rust outline engine approximate. Never claim bit-exactness for
  TF10; prefer render caches.
* **T13 - triangulation for outline glyphs.** `GetEffectiveTextShape` triangulates outline glyphs
  (`SHAPE_SIMPLE` triangles for DRC) and `TransformTextToPolySet` builds polygons from those
  triangles before `Simplify`; the triangulation port (G4b) must be exact, including the hint
  reuse path (`CacheTriangulation( hintData )`).
* **T14 - glyph table source.** `include_str!` of `../../../../common/newstroke_font.cpp` keeps a
  single source of truth but ties the crate to the C++ tree; a generated data file duplicates
  2.4 MB in `rust/`. Either way the wasm binary grows by ~2.4 MB; consider decoding lazily per
  glyph (`OnceLock` per glyph or decode on demand) if start-up time matters. Decision for review.
* **T15 - overflow.** `aSize.x * char_count` (tabs) and `aSize.x * 4` loops are `int` maths in
  C++; with overflow checks on, use `i64` intermediates only where the C++ cannot overflow for
  valid inputs (sizes <= 250 mm), or `wrapping_*` with a note; document the choice.
* **T16 - `@{...}` expressions.** `PLOTTER::PlotText` evaluates `@{` in plotted text and
  `ResolveTextVars` evaluates after each pass; until text_eval is ported (tier 3) leave such text
  unevaluated and record the gap (affects only texts containing `@{`).
* **T17 - dimension recursion.** `PCB_DIMENSION_BASE::ClearRenderCache` calls `Update()`, which
  calls `SetText` -> `cacheShownText` -> `ClearRenderCache`; C++ breaks the loop with
  `m_inClearRenderCache`. Model this explicitly.
* **T18 - `isStrokeFont` before font resolution.** During parsing the font is null; the legacy
  bold migration keys off the unresolved face name. Resolution order: parse item ->
  `MigrateLegacyBoldStrokeWidth` -> `finishItem` -> `ResolveFont`.
