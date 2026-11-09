# kicad-font port status

Port of KiCad's built-in stroke font ("KiCad Font", Newstroke), the font-independent text
layout engine of `KIFONT::FONT`, the markup parser and the pure text pen-size helpers.
Work package F1. Builds for `wasm32-unknown-unknown`, no `unsafe`, no dependencies beyond
`kicad-geom` and `kicad-core`.

Verified bit for bit against the C++ (g++ 13 / libstdc++, the reference build of
`docs/ORACLE.md`) by a differential oracle: the whole glyph table (strings and decoded
points) and 8500 seeded cases, all matching (see "Verification").

## C++ to Rust map

| C++ | Rust | State |
|---|---|---|
| `common/newstroke_font.cpp`, `include/newstroke_font.h` (`newstroke_font[]`, `newstroke_font_bufsize`) | `src/newstroke_font.rs` (`NEWSTROKE_FONT`, `NEWSTROKE_FONT_BUFSIZE` = 65487) | ported: generated mechanically (see below), verified byte for byte |
| `include/font/glyph.h`, `glyph_types.h`, `common/font/glyph.cpp` | `src/glyph.rs` | |
| - `GLYPH` (`IsStroke`, `IsOutline`, `BoundingBox`, `IsHover`, `SetIsHover`) | `Glyph` enum (`Stroke`, `Outline`) with the same methods | ported |
| - `STROKE_GLYPH` (copy ctor, `AddPoint`, `RaisePen`, `Finalize`, `BoundingBox`, `SetBoundingBox`, `Transform`, `Move`) | `StrokeGlyph` (`Clone` = copy ctor: pen state and hover flag reset), `add_point`, `raise_pen`, `finalize`, `bounding_box`, `set_bounding_box`, `transform`, `move_` | ported |
| - `OUTLINE_GLYPH` (`BoundingBox`) | `OutlineGlyph { poly: ShapePolySet }`, `bounding_box` | partial: data + bounding box; `Triangulate`, `CacheTriangulation`, `GetTriangulationData` not ported (GAP-1) |
| `include/font/stroke_font.h`, `common/font/stroke_font.cpp` | `src/stroke_font.rs` | |
| - `STROKE_FONT_SCALE`, `FONT_OFFSET`, `buildGlyphBoundingBox`, `loadNewStrokeFont` | private consts, `build_glyph_bounding_box`, `decode_glyph`, `load_new_stroke_font`, `default_font_data()` (`OnceLock`) | ported |
| - `STROKE_FONT::LoadFont`, `GetInterline`, `GetSuperSubSizeMultiplier`, `GetSuperSubBaselineOffset`, `GetTextAsGlyphs`, `GetGlyphCount`, `GetGlyph`, `GetGlyphBoundingBox`, `m_superSubSize` & co | `StrokeFont::load_font`, `Font` impl (`interline`, `super_sub_size_multiplier`, `super_sub_baseline_offset`, `text_as_glyphs`), `glyph_count`, `glyph`, `glyph_bounding_box`, `SUPER_SUB_SIZE`, `SUBSCRIPT_BASELINE_OFFSET`, `SUPERSCRIPT_BASELINE_OFFSET` | ported |
| `include/font/font.h`, `common/font/font.cpp` | `src/font.rs` | |
| - `TEXT_STYLE`, `TEXT_STYLE_FLAGS`, `ITALIC_TILT`, `IsBold/IsItalic/IsSuperscript/IsSubscript` | `text_style::*`, `TextStyleFlags` (`u32`), `ITALIC_TILT`, `is_bold` ... | ported |
| - `FONT` virtuals | trait `Font` (required methods) | ported |
| - `FONT` non-virtual members: `Draw` (both overloads), `StringBoundaryLimits`, `GetAlignedDrawPosition`, `LinebreakText`, `getLinePositions`, `drawMarkup`, `drawSingleLineText`, `boundingBoxSingleLine`, `wordbreakMarkup`, `linesCount`, `NameAsToken` | trait `FontExt` (blanket impl for every `Font`, also `dyn Font`): `draw`, `draw_with_cursor`, `string_boundary_limits`, `aligned_draw_position`, `linebreak_text`, `line_positions`, `draw_markup`, `draw_single_line_text`, `bounding_box_single_line`, `wordbreak_markup`; free `lines_count`; `Font::name_as_token` | ported |
| - file-local `::drawMarkup`, `::wordbreakMarkup` | `draw_markup_node`, `wordbreak_markup_node` | ported (incl. URL hover / active URL) |
| - `FONT::GetFont`, `getDefaultFont`, `FONT::IsStroke( name )` | `get_font -> FontLookup`, `default_font()`, `is_stroke_font_name` | ported, outline fonts see D1 |
| - `MARKUP_CACHE` (LRU parse cache) | - | skipped: pure memoisation, every call parses |
| `include/font/font_metrics.h`, `METRICS::Default()` | `src/font_metrics.rs` (`Metrics`, `Metrics::DEFAULT`, `default_metrics()`) | ported |
| `include/font/kicad_font_name.h` | `font::KICAD_FONT_NAME` | ported |
| `include/markup_parser.h`, `common/markup_parser.cpp` (PEGTL grammar + selector) | `src/markup_parser.rs` (`MarkupParser`, `MarkupNode`, `MarkupNodeType`; `type_string`, `as_string`) | ported as a hand-written recursive-descent parser (D2) |
| `include/callback_gal.h`, `common/callback_gal.cpp`, `GAL::SetLineWidth/GetLineWidth/DrawGlyph/DrawGlyphs` | `src/callback_gal.rs`: trait `TextGal`, `CallbackGal`, `StrokeCollector`, `to_vector2i` | partial: stroke modes; GAP-2 |
| `common/gr_text.cpp` / `include/gr_text.h`: `GetPenSizeForBold/DemiBold/Normal` (int and `wxSize` overloads), `ClampTextPenSize` (3 overloads), `BOLD_STROKE_MULTIPLIER`, `GetKnockoutTextMargin`, `GRTextWidth` | `src/gr_text.rs`: `get_pen_size_for_bold[_size]`, `..._demi_bold[_size]`, `..._normal[_size]`, `clamp_text_pen_size`, `clamp_text_pen_size_f32`, `clamp_text_pen_size_vec`, `BOLD_STROKE_MULTIPLIER`, `get_knockout_text_margin`, `gr_text_width` | ported (`GRTextWidth`: GAP-3) |
| `include/eda_text.h`: `TEXT_MIN_SIZE_MM`, `TEXT_MAX_SIZE_MM`, `DEFAULT_SIZE_TEXT` | `gr_text::TEXT_MIN_SIZE_MM`, ... | ported |
| `GRPrintText` | - | skipped: wxDC printing |
| `TEXT_ATTRIBUTES`, `GR_TEXT_H/V_ALIGN_T` | in `kicad-core::text_attributes` (used as is) | (kicad-core) |
| `common/font/outline_font.cpp`, `outline_decomposer.cpp`, `fontconfig.cpp`, `version_info.cpp` | - | skipped: FreeType/HarfBuzz/fontconfig (D1) |

### Glyph table generation

`src/newstroke_font.rs` was produced by a throw-away script (`rust/target/font-gen/`, not
committed): it tokenises the initializer of `newstroke_font[]` (skipping `/* */` and `//`
comments, decoding the C escapes `\\` and `\"`, concatenating adjacent literals - there are
none), asserts that every string is printable ASCII of even length and writes one Rust literal
per glyph (the extra copyright/licence notices of the C++ file are carried over). Verification:
65487 strings; a dump of the C++ `newstroke_font[]` (oracle `strings` mode) is identical to
the Rust table; the oracle golden carries the FNV-1a hash of all strings and of the decoded
glyphs (64 blocks) which the Rust test recomputes.

## Ported tests

| C++ | Rust | Notes |
|---|---|---|
| `qa/tests/common/test_kicad_stroke_font.cpp` (`TabCheck`, `ZeroSizeTab`) | `tests/test_kicad_stroke_font.rs` (`tab_check`, `zero_size_tab`, + `glyph_table_basics`) | all |
| `qa/tests/common/test_markup_parser.cpp` (`Parse`, `OverbarMultipleSpacesWidth`) | `tests/test_markup_parser.rs` (`parse`, `overbar_multiple_spaces_width`) | all |
| - | `src/font.rs` unit tests (`get_font`, `lines_count`, wx tokenizer) | new |

Other qa files mentioning fonts (`test_eda_text.cpp`, `test_text_attributes.cpp`,
`test_pdf_unicode_plot.cpp`, `test_target3001_fonts.cpp`, text_eval, schematic importers)
test `EDA_TEXT` (kicad-pcb), `TEXT_ATTRIBUTES` (kicad-core) or plotters/outline fonts.

## Verification (differential oracle)

`tests/oracle/font_oracle.cpp` links the real `libkigal.so` (fonts, `CALLBACK_GAL`) and
`libkicommon.so` (markup parser, Newstroke table, `wxStringSplit`) plus `libkimath.a` /
`libcore.a`; the pure helpers of `gr_text.cpp` are compiled verbatim from the C++ source
(the rest of that file needs libcommon's wxDC code). Build/regenerate with
`tests/oracle/build_oracle.sh [outdir] regen`. `tests/test_font_oracle.rs` replays it with
an identical splitmix64 case generator and identical output text, compared per case through
FNV-1a (`tests/golden/font_oracle.txt`, 200 KB). Doubles are compared as bit patterns.

| Section | Cases | Content | Result |
|---|---|---|---|
| glyph table | 65487 glyphs | raw strings hash, decoded bboxes and points | match |
| M | 3000 | markup parse trees of random strings (ASCII, Latin-1, Greek, Cyrillic, CJK, non-BMP, U+FFEF.., controls, NUL, `~_^{}`, escapes, URLs, unbalanced markup) | match |
| T | 2000 | `GetTextAsGlyphs`: return value, bbox, every glyph (bbox, points), random style bits, sizes incl. 0/tiny/negative/non-square, tabs, angles, mirror | match |
| D | 3000 | multi-line marked-up text with random attributes (all justifications, angle, mirror, italic, bold, underline, stroke width 0/1/large, line spacing, cursor): `getLinePositions` lines/positions/extents, `Draw` through a recording `CALLBACK_GAL` (each glyph's points + the `VECTOR2I` stroke segments, final line width), `StringBoundaryLimits`, `GetAlignedDrawPosition`, `GetInterline`, `GRTextWidth`, `LinebreakText` (random column width) | match |
| H | 500 | pen size helpers, all `ClampTextPenSize` overloads, `GetKnockoutTextMargin` | match |

## Deviations

- **D1 - outline fonts.** `FONT::GetFont` with a non-stroke face name loads an
  `OUTLINE_FONT` through fontconfig + FreeType. Fontconfig always returns *some* font
  (`FcFontMatch` closest match, so a missing family is silently substituted by a host font,
  with fake bold/italic when the style is missing); `OUTLINE_FONT::LoadFont` only fails (and
  `GetFont` then falls back to the stroke font, caching that under the key) when fontconfig
  reports `FF_ERROR` or `FT_New_Face` fails. This port has no outline engine:
  `get_font` returns `FontLookup::OutlineUnavailable { name, bold, italic, .. }`, whose
  `font()` is the stroke font (KiCad's own fallback for a font that cannot be loaded). Callers
  must treat this as "outline font not available" (use the item's render cache where one is
  valid, otherwise report the substitution). `embedded_files` are accepted and ignored.
- **D2 - markup parser.** Hand-written instead of PEGTL; same tree for every input (verified on
  3000 fuzz cases plus the C++ test). `markup` results are memoised per position, which
  removes PEGTL's exponential re-parsing of nested unterminated markup without changing the
  result. `parse()` returns `None` only where PEGTL would fail, which cannot happen for valid
  UTF-8 (Rust `&str`).
- **D3 - integer overflow.** Where the C++ does plain `int` arithmetic that can overflow for
  absurd inputs (tab stops `aSize.x * char_count`, cursor advances, line offsets) the port uses
  `wrapping_*` (x86 behaviour of the g++ build) so it never panics; `int = double`
  conversions use Rust's saturating `as` where C++ is undefined out of range.
- **D4 - no global caches.** `s_fontMap` / `MARKUP_CACHE` are memoisation only; not ported.
  The decoded glyph table is a process-wide `OnceLock` (C++: mutex-guarded globals).
- **D5 - `_( "Default Font" )`** is compared untranslated (no i18n in the port).

## Known gaps

- **GAP-1** `OUTLINE_GLYPH` triangulation (`Triangulate`, `CacheTriangulation` incl. hint
  data, `GetTriangulationData`): needs the `kicad-geom` triangulation port.
- **GAP-2** `CALLBACK_GAL`: outline glyphs are handed unchanged to the outline-glyph callback
  (C++: triangulated, or copied + `Fracture()`d and passed outline by outline); the
  outline-only constructor (stroke glyphs turned into `TransformOvalToPolygon` ovals) is not
  ported. Both need `kicad-geom` shape functions; neither affects stroke-font text.
- **GAP-3** `GRTextWidth` (and any caller that would evaluate `@{...}`): text_eval
  (`EXPRESSION_EVALUATOR`) is not ported, text containing `@{` is measured unevaluated.
- Outline font engine (FreeType/HarfBuzz/fontconfig), `OUTLINE_FONT::GetTextAsGlyphs`,
  render caches: see D1 and `docs/analysis/text-font.md` 4.14/4.15 (TF9/TF10).
- `wasm32`: glyph rotation uses `kicad_geom` sin/cos (libm), see PORTING.md section 9.
