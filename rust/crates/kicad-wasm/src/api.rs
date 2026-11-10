// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! The target-independent half of the JavaScript API (new code): plain Rust functions with
//! plain Rust results. The `#[wasm_bindgen]` layer in `lib.rs` only converts arguments and
//! results, so the native test suite calls exactly the code the WebAssembly module runs
//! (`tests/native_reference.rs` records the native results that `web/test/smoke.mjs` compares
//! the wasm results with).

use kicad_core::text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
use kicad_font::callback_gal::to_vector2i;
use kicad_font::gr_text::get_pen_size_for_normal;
use kicad_font::{FontExt, Metrics, StrokeCollector, default_font};
use kicad_geom::{DEGREES_T, EdaAngle, ShapeLineChain, ShapePolySet, Vector2D, Vector2I};
use kicad_plot::{FillT, GerberPlotter, Plotter, RenderSettings};
use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::{DSN_NUMBER, DSN_STRING, DsnLexer, FormatMode, KeywordSet};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `GetMajorMinorPatchVersion()`.
pub fn version() -> String {
    kicad_session::build_version::major_minor_patch_version().to_string()
}

/// The version strings (`versionInfo()`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionInfo {
    /// `GetMajorMinorPatchVersion()`, e.g. `"10.99.0"`.
    pub version: String,
    /// `GetBuildVersion()`, e.g. `"10.99.0-unknown"`.
    pub build: String,
    /// `GetCommitHash()` of the C++ tree the port follows.
    pub commit: String,
}

/// `versionInfo()`.
pub fn version_info() -> VersionInfo {
    VersionInfo {
        version: version(),
        build: kicad_session::build_version::build_version().to_string(),
        commit: kicad_session::build_version::commit_hash().to_string(),
    }
}

/// `prettify(text)`: `KICAD_FORMAT::Prettify( text, FORMAT_MODE::NORMAL )`, the formatting
/// pass the board writer applies to its output.
pub fn prettify(text: &str) -> String {
    kicad_sexpr::prettify(text, FormatMode::Normal)
}

/// Token statistics of an s-expression file as `DSNLEXER` (with the `.kicad_pcb` keyword table)
/// splits it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenStats {
    /// All tokens up to EOF.
    pub tokens: u64,
    /// `(` tokens.
    pub lists: u64,
    /// `)` tokens.
    pub closes: u64,
    /// Symbols that are `.kicad_pcb` keywords.
    pub keywords: u64,
    /// Other symbols.
    pub symbols: u64,
    /// Numbers.
    pub numbers: u64,
    /// Quoted strings.
    pub strings: u64,
    /// Deepest list nesting.
    pub max_depth: u64,
    /// Line number of the last token (`CurLineNumber()`).
    pub lines: u64,
    /// The ten most frequent keywords (count descending, then name), `[name, count]`.
    pub top_keywords: Vec<(String, u64)>,
}

/// `tokenStats(text)`: lex `text` completely with `DSNLEXER` and count the tokens. A lexer
/// error (unterminated string, ...) returns the `PARSE_ERROR::What()` text.
pub fn token_stats(text: &str, source: &str) -> Result<TokenStats, String> {
    let mut lexer = DsnLexer::<T>::new(text, source);
    let mut stats = TokenStats::default();
    let mut depth: u64 = 0;
    let mut keywords: BTreeMap<&'static str, u64> = BTreeMap::new();

    loop {
        let tok = lexer.next_tok().map_err(|e| e.what().to_string())?;

        if tok == T::T_EOF {
            break;
        }

        stats.tokens += 1;
        stats.lines = lexer.cur_line_number().max(0) as u64;

        match tok {
            T::T_LEFT => {
                stats.lists += 1;
                depth += 1;
                stats.max_depth = stats.max_depth.max(depth);
            }
            T::T_RIGHT => {
                stats.closes += 1;
                depth = depth.saturating_sub(1);
            }
            _ => {
                let v = tok.to_i32();

                if v >= 0 {
                    stats.keywords += 1;
                    *keywords.entry(T::KEYWORDS[v as usize]).or_default() += 1;
                } else if v == DSN_NUMBER {
                    stats.numbers += 1;
                } else if v == DSN_STRING {
                    stats.strings += 1;
                } else {
                    // DSN_SYMBOL (and any other negative token) counts as a symbol.
                    stats.symbols += 1;
                }
            }
        }
    }

    let mut top: Vec<(String, u64)> = keywords
        .into_iter()
        .map(|(k, n)| (k.to_string(), n))
        .collect();
    top.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    top.truncate(10);
    stats.top_keywords = top;

    Ok(stats)
}

/// Result of `strokeText`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StrokeTextResult {
    /// SVG path data in board units (nm, y down): one `M x y L x y ...` run per stroke.
    pub path: String,
    /// The pen width (nm) to stroke the path with (`GetPenSizeForNormal( size )`).
    pub stroke_width: i32,
    /// Number of stroke segments.
    pub segments: u64,
}

/// `strokeText(text, sizeMm)`: draw `text` with the KiCad stroke font (`KIFONT::STROKE_FONT`
/// through `FONT::Draw`), left / bottom aligned at the origin, glyph size `size_mm` mm, and
/// return the strokes as SVG path data. Points are truncated to integers as `CALLBACK_GAL`
/// does before plotting.
pub fn stroke_text(text: &str, size_mm: f64) -> StrokeTextResult {
    let size = kicad_geom::ki_round(size_mm * 1_000_000.0);
    let mut attrs = TextAttributes::default();
    attrs.size = (size, size);
    attrs.halign = GrTextHAlignT::Left;
    attrs.valign = GrTextVAlignT::Bottom;
    attrs.stroke_width = get_pen_size_for_normal(size);

    let mut collector = StrokeCollector::new();
    default_font().draw(
        &mut collector,
        text,
        Vector2I::new(0, 0),
        &attrs,
        &Metrics::default(),
    );

    let mut path = String::new();
    let mut segments = 0;

    for polyline in collector.polylines() {
        for (i, p) in polyline.iter().enumerate() {
            let q = to_vector2i(*p);

            if !path.is_empty() {
                path.push(' ');
            }

            path.push_str(&format!(
                "{} {} {}",
                if i == 0 { 'M' } else { 'L' },
                q.x,
                q.y
            ));
        }

        segments += polyline.len().saturating_sub(1) as u64;
    }

    StrokeTextResult {
        path,
        stroke_width: attrs.stroke_width,
        segments,
    }
}

/// A polygon as JavaScript passes it: rings of `[x, y]` points, ring 0 the outline, the others
/// holes.
pub type JsPolygon = Vec<Vec<[i32; 2]>>;

fn to_poly_set(polygons: &[JsPolygon]) -> ShapePolySet {
    let mut set = ShapePolySet::new();

    for polygon in polygons {
        let mut outline_index = -1;

        for (i, ring) in polygon.iter().enumerate() {
            let pts: Vec<Vector2I> = ring.iter().map(|p| Vector2I::new(p[0], p[1])).collect();
            let chain = ShapeLineChain::from_points(&pts, true);

            if i == 0 {
                outline_index = set.add_outline(&chain);
            } else {
                set.add_hole(&chain, outline_index);
            }
        }
    }

    set
}

fn from_poly_set(set: &ShapePolySet) -> Vec<JsPolygon> {
    set.c_polygons()
        .iter()
        .map(|polygon| {
            polygon
                .iter()
                .map(|ring| ring.c_points().iter().map(|p| [p.x, p.y]).collect())
                .collect()
        })
        .collect()
}

/// `polygonBoolean(a, b, op)`: `SHAPE_POLY_SET::BooleanAdd / BooleanSubtract /
/// BooleanIntersection / BooleanXor` (vendored Clipper2) of two polygon lists. `op` is
/// `"union"`, `"difference"`, `"intersection"` or `"xor"`.
pub fn polygon_boolean(
    a: &[JsPolygon],
    b: &[JsPolygon],
    op: &str,
) -> Result<Vec<JsPolygon>, String> {
    let mut sa = to_poly_set(a);
    let sb = to_poly_set(b);

    match op {
        "union" => sa.boolean_add(&sb),
        "difference" => sa.boolean_subtract(&sb),
        "intersection" => sa.boolean_intersection(&sb),
        "xor" => sa.boolean_xor(&sb),
        _ => {
            return Err(format!(
                "unknown boolean operation '{op}' (union, difference, intersection, xor)"
            ));
        }
    }

    Ok(from_poly_set(&sa))
}

/// The plot script of [`plot_demo_gerber`] in the format of the kicad-plot Gerber oracle
/// (`crates/kicad-plot/tests/oracle/gerber_plotter_oracle.cpp`); `tests/oracle/` runs it on
/// the real `GERBER_PLOTTER` and the golden file pins the result.
pub const DEMO_GERBER_SCRIPT: &str = "\
BEGIN 0
CFG 1 0 0 6 0 0 0 2540
CREATOR PCBNEW
HDR %TF.FileFunction,Copper,L1,Top*%
HDR %TF.FilePolarity,Positive*%
RS 0
START
SEG 10000000 -10000000 30000000 -10000000 250000 -1
SEG 30000000 -10000000 30000000 -20000000 250000 -1
FC 10000000 -10000000 1600000 -1
FR 30000000 -20000000 1500000 1000000 0 -1
FO 20000000 -25000000 1200000 2000000 90 -1
CIRC 20000000 -15000000 3000000 N 150000
TARC 20000000 -15000000 0 180 4000000 200000 -1
POLY F 0 -1 4 5000000 -30000000 15000000 -30000000 15000000 -35000000 5000000 -35000000
END
";

/// `plotDemoGerber()`: a small Gerber X2 file drawn by the Rust `GERBER_PLOTTER` port (two
/// tracks, round / rectangular / oval pad flashes, a circle, an arc and a filled region): the
/// calls of [`DEMO_GERBER_SCRIPT`], made exactly as the oracle driver makes them.
/// `build_version` and `date` fill the `G04 Created by KiCad (PCBNEW <version>) date <date>`
/// line (the C++ reads both from the build and the clock).
pub fn plot_demo_gerber(build_version: &str, date: &str) -> String {
    let mut plotter = GerberPlotter::new();
    plotter.set_render_settings(Some(RenderSettings::default()));
    plotter.open_file("plot.gbr");
    plotter.set_build_version(build_version);
    plotter.set_creation_date(date);

    // CFG 1 0 0 6 0 0 0 2540
    plotter.set_viewport(Vector2I::new(0, 0), 2540.0, 1.0, false);
    plotter.set_gerber_coordinates_format(6, false);
    plotter.use_x2format(true);
    plotter.use_x2_net_attributes(false);
    plotter.disable_apert_macros(false);
    // CREATOR PCBNEW
    plotter.set_creator("PCBNEW");
    // HDR ...
    plotter.add_line_to_header("%TF.FileFunction,Copper,L1,Top*%");
    plotter.add_line_to_header("%TF.FilePolarity,Positive*%");
    // RS 0
    plotter
        .state_mut()
        .render_settings
        .as_mut()
        .expect("render settings were set above")
        .set_default_pen_width(0);
    // START
    plotter.start_plot("1");

    let pt = |x: i32, y: i32| Vector2I::new(x, y);
    let deg = |d: f64| EdaAngle::new(d, DEGREES_T);

    plotter.thick_segment(
        pt(10_000_000, -10_000_000),
        pt(30_000_000, -10_000_000),
        250_000,
        None,
    );
    plotter.thick_segment(
        pt(30_000_000, -10_000_000),
        pt(30_000_000, -20_000_000),
        250_000,
        None,
    );
    plotter.flash_pad_circle(pt(10_000_000, -10_000_000), 1_600_000, None);
    plotter.flash_pad_rect(
        pt(30_000_000, -20_000_000),
        pt(1_500_000, 1_000_000),
        deg(0.0),
        None,
    );
    plotter.flash_pad_oval(
        pt(20_000_000, -25_000_000),
        pt(1_200_000, 2_000_000),
        deg(90.0),
        None,
    );
    plotter.circle(
        pt(20_000_000, -15_000_000),
        3_000_000,
        FillT::NoFill,
        150_000,
    );
    plotter.thick_arc(
        Vector2D::new(20_000_000.0, -15_000_000.0),
        deg(0.0),
        deg(180.0),
        4_000_000.0,
        200_000,
        None,
    );
    plotter.plot_poly(
        &[
            pt(5_000_000, -30_000_000),
            pt(15_000_000, -30_000_000),
            pt(15_000_000, -35_000_000),
            pt(5_000_000, -35_000_000),
        ],
        FillT::FilledShape,
        0,
        None,
    );

    plotter.end_plot();
    plotter.output().to_string()
}

/// FNV-1a 64 of `bytes` (what the JavaScript tests use to compare large outputs).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}
