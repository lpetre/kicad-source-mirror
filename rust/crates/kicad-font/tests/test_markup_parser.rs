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

//! Port of `qa/tests/common/test_markup_parser.cpp` (suite `MarkupParser`).

use kicad_font::font::{Font, FontExt};
use kicad_font::markup_parser::{MarkupNode, MarkupParser};
use kicad_font::stroke_font::StrokeFont;
use kicad_geom::{ANGLE_0, Vector2I};

fn node_to_string(a_node: &MarkupNode, a_string_to_populate: &mut String) {
    a_string_to_populate.push_str(" {");

    if a_node.is_overbar() {
        a_string_to_populate.push_str("OVER");
    }
    if a_node.is_subscript() {
        a_string_to_populate.push_str("SUB");
    }
    if a_node.is_superscript() {
        a_string_to_populate.push_str("SUP");
    }

    if let Some(content) = a_node.content() {
        a_string_to_populate.push('\'');
        a_string_to_populate.push_str(content);
        a_string_to_populate.push('\'');
    }

    for c in &a_node.children {
        node_to_string(c, a_string_to_populate);
    }

    a_string_to_populate.push_str("} ");
}

/// Test the `MARKUP::MARKUP_PARSER::Parse` method.
#[test]
fn parse() {
    let cases: [(&str, &str); 9] = [
        ("A normal string", " { {'A normal string'} } "),
        (
            "_{A subscript String}",
            " { {SUB {'A subscript String'} } } ",
        ),
        (
            "^{A superscript String}",
            " { {SUP {'A superscript String'} } } ",
        ),
        (
            "~{An overbar String}",
            " { {OVER {'An overbar String'} } } ",
        ),
        ("~{An incomplete markup", " { {'~{An incomplete markup'} } "),
        (
            "A string ~{overbar}",
            " { {'A string '}  {OVER {'overbar'} } } ",
        ),
        (
            "A string ~{incomplete markup",
            " { {'A string ~{incomplete markup'} } ",
        ),
        (
            "A string ~{overbar} ~{incomplete markup",
            " { {'A string '}  {OVER {'overbar'} }  {' ~{incomplete markup'} } ",
        ),
        (
            "A string ~{incomplete markup ~{overbar}",
            " { {'A string ~{incomplete markup '}  {OVER {'overbar'} } } ",
        ),
    ];

    for (input, expected) in cases {
        let parser = MarkupParser::new(input);
        let root_node = parser.parse().expect(input);

        let mut result = String::new();
        node_to_string(&root_node, &mut result);

        assert_eq!(result, expected, "{input}");
    }
}

/// Verify that LinebreakText preserves multiple spaces inside overbar markup and that
/// the overbar word receives a nonzero width so line-wrapping accounts for it.
///
/// Regression test for https://gitlab.com/kicad/code/kicad/-/issues/22913
#[test]
fn overbar_multiple_spaces_width() {
    let font = StrokeFont::load_font("").unwrap();

    let glyph_size = Vector2I::new(1000, 1000);
    let origin = Vector2I::new(0, 0);

    // Measure the width of a single space for reference
    let space_width = font
        .text_as_glyphs(
            None, None, " ", glyph_size, origin, ANGLE_0, false, origin, 0,
        )
        .x;

    assert!(space_width > 0);

    // Measure the width of 5 spaces
    let five_space_width = font
        .text_as_glyphs(
            None, None, "     ", glyph_size, origin, ANGLE_0, false, origin, 0,
        )
        .x;

    assert!(five_space_width > space_width);

    // Verify that LinebreakText preserves overbar with multiple spaces
    let wide_column = 100000;

    let cases: [(&str, &str); 7] = [
        ("~{     }", "~{     }"),
        ("A ~{     }", "A ~{     }"),
        ("A ~{  B  }", "A ~{  B  }"),
        ("~{     } end", "~{     } end"),
        ("/~{     }", "/~{     }"),
        ("_{     }", "_{     }"),
        ("^{     }", "^{     }"),
    ];

    for (input, expected) in cases {
        let mut text = input.to_string();
        font.linebreak_text(&mut text, wide_column, glyph_size, 0, false, false);
        assert_eq!(text, expected, "{input}");
    }
}
