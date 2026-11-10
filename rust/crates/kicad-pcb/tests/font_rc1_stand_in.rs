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

//! Package FONT-RC-1 verification: the render cache stored in the file as the stand-in for the
//! outline font engine (`EDA_TEXT::GetRenderCache`, `formatRenderCache`).
//!
//! `tests/data/font_rc1/frc.kicad_pcb` is `frc_input.kicad_pcb` (hand-written: board texts,
//! footprint fields and text, a text box, in fonts the oracle machine has according to
//! `fc-list`: DejaVu Sans / Serif / Sans Mono, Liberation Sans / Serif / Mono) re-saved with the
//! reference `kicad-cli pcb upgrade --force`, i.e. with the caches the reference generated with
//! FreeType / HarfBuzz from those fonts. Re-saving that file with the reference reproduces it
//! byte for byte (measured), so the port's re-save must too.

use std::path::Path;

use kicad_pcb::board::Board;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::eda_text::{self, EdaTextExt};
use kicad_pcb::io::{FormatOptions, ParseOptions, format_board, parse_board};
use kicad_pcb::item::RecurseMode;
use kicad_core::text_vars::ResolutionContext;
use kicad_geom::{EdaAngle, Vector2I};

fn data(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/font_rc1")
            .join(name),
    )
    .unwrap()
}

fn load(text: &str) -> Board {
    parse_board(text, "frc.kicad_pcb", &ParseOptions::default()).expect("frc.kicad_pcb loads")
}

/// Shown text (`FOR_CANVAS`) of every text item, in `RunOnChildren` order.
fn texts(board: &Board) -> Vec<(kicad_pcb::ids::ItemId, String)> {
    let ctx = BoardCtx::bare(board);
    let mut ids = Vec::new();
    board.run_on_children(RecurseMode::Recurse, &mut |id| ids.push(id));
    ids.into_iter()
        .filter_map(|id| {
            let item = ctx.item(id)?;
            let host = item.as_eda_text()?;
            Some((id, host.shown_text(&ctx, ResolutionContext::ForCanvas, 0)))
        })
        .collect()
}

/// Replace the polygons of the `(render_cache "<text>" ..)` list of `text` by `polygons`.
fn replace_cache_polygons(file: &str, text: &str, polygons: &str) -> String {
    let head = format!("(render_cache \"{text}\" ");
    let start = file.find(&head).expect("cache present");
    let poly_start = start + file[start..].find("(polygon").unwrap();
    let mut depth = 0;
    let mut end = start;

    for (i, c) in file[start..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;

                if depth == 0 {
                    end = start + i;
                    break;
                }
            }
            _ => {}
        }
    }

    format!("{}{}{}", &file[..poly_start], polygons, &file[end..])
}

/// Re-save of the reference's own output: identical bytes (header and glyphs).
#[test]
fn resave_reproduces_reference() {
    let o1 = data("frc.kicad_pcb");
    let board = load(&o1);
    let out = format_board(&board, &FormatOptions::default()).unwrap();

    if let Some(d) = kicad_qa::sexpr_text::first_difference(&o1, &out, 3) {
        panic!(
            "differs from the reference at line {} ({}):\n{}",
            d.line,
            d.path_label(),
            d.excerpt
        );
    }
}

/// Every outline text of the board (10: board texts, footprint fields and text, the text box,
/// a text with a variable) has a valid stand-in after the whole load; none falls back.
#[test]
fn every_outline_text_has_a_stand_in() {
    let board = load(&data("frc.kicad_pcb"));
    let ctx = BoardCtx::bare(&board);
    let mut with_glyphs = 0;

    for (id, shown) in texts(&board) {
        let item = ctx.item(id).unwrap();
        let host = item.as_eda_text().unwrap();

        if host.eda_text().is_stroke_font() {
            continue;
        }

        assert!(host.eda_text().render_cache_data().is_none(), "{shown}: C++ clears it at load");
        let cache = eda_text::render_cache(host, &ctx, &shown, Vector2I::default());
        assert!(cache.is_some_and(|c| !c.glyphs.is_empty()), "{shown}: no stand-in");
        with_glyphs += 1;
    }

    assert_eq!(with_glyphs, 10);
    assert_eq!(eda_text::outline_font_fallbacks(&ctx), Vec::new());
}

/// Criterion (b) of the package: what the port measures and writes are exactly the stored
/// glyph polygons, proved with a marker triangle that no font produces.
#[test]
fn marker_triangle_cache_is_used_and_written() {
    let marker = "(polygon (pts (xy 129 139) (xy 131 139) (xy 130 141)))";
    let file = replace_cache_polygons(&data("frc.kicad_pcb"), "Ok ring", marker);
    let board = load(&file);
    let ctx = BoardCtx::bare(&board);
    let (id, _) = texts(&board)
        .into_iter()
        .find(|(_, t)| t == "Ok ring")
        .unwrap();
    let item = ctx.item(id).unwrap();
    let host = item.as_eda_text().unwrap();

    // GetEffectiveTextShape (outline, not triangulated): the triangle only.
    let shape = host.effective_text_shape(&ctx, false, None, EdaAngle::default());
    assert_eq!(shape.shapes().len(), 1);
    let bbox = shape.shapes()[0].bbox(0);
    assert_eq!(bbox.origin(), Vector2I::new(129_000_000, 139_000_000));
    assert_eq!((bbox.width(), bbox.height()), (2_000_000, 2_000_000));

    // Triangulated: the one triangle.
    let shape = host.effective_text_shape(&ctx, true, None, EdaAngle::default());
    assert_eq!(shape.shapes().len(), 1);

    // The writer writes the stored triangle back.
    let out = format_board(&board, &FormatOptions::default()).unwrap();
    assert!(
        out.contains("(render_cache \"Ok ring\" 0\n\t\t\t(polygon\n\t\t\t\t(pts\n\t\t\t\t\t(xy 129 139) (xy 131 139) (xy 130 141)\n"),
        "marker not written back"
    );
}

/// After an edit that changes what the font would produce, the stand-in is gone: the text falls
/// back to the stroke font, is reported, and only the cache header is written (no glyphs).
#[test]
fn edited_text_has_no_stand_in() {
    let mut board = load(&data("frc.kicad_pcb"));
    let (id, _) = texts(&board)
        .into_iter()
        .find(|(_, t)| t == "Ok ring")
        .unwrap();
    let text_id = id.typed::<kicad_pcb::pcb_text::PcbText>().unwrap();

    board.modify_text(text_id, |t, env| {
        t.text.set_text("Ok ring2");
        t.set_text_size(env, Vector2I::new(1_300_000, 1_300_000), true);
    });

    let ctx = BoardCtx::bare(&board);
    let fallbacks = eda_text::outline_font_fallbacks(&ctx);
    assert_eq!(fallbacks.len(), 1);
    assert_eq!(fallbacks[0].item, id);
    assert_eq!(
        fallbacks[0].message(),
        "text uses font Liberation Mono which is not available; drawn with the stroke font"
    );

    let out = format_board(&board, &FormatOptions::default()).unwrap();
    assert!(out.contains("(render_cache \"Ok ring2\" 0)"));
}
