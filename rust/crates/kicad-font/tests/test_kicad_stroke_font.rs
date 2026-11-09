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

//! Port of `qa/tests/common/test_kicad_stroke_font.cpp` (suite `KicadStrokeFont`).

use kicad_font::font::Font;
use kicad_font::stroke_font::StrokeFont;
use kicad_geom::{ANGLE_0, Box2I, Vector2I};

/// Test the tab spacing.
#[test]
fn tab_check() {
    let font = StrokeFont::load_font("").unwrap();

    let cases: [(&str, &str); 5] = [
        (
            "v34_STM23G491\t\tController STM32\t(column 3)",
            "v34_LPC1758\t\t\tController NXP\t\t(column 3)",
        ),
        (
            "1\td9c1892a\t\t\tMOLEX\t\t\t1053071202\t\t\t\t\t12\t\tCONNECTOR",
            "REF\tPART NUMBER\t\t\tMANUFACTURER\tMANUFACTURER PART NUMBER\tQTY\t\tCONNECTOR",
        ),
        (
            "5\tC-000208\t\t\tMCMASTER/CARR\t8054T13 BLUE\t\t\t\t3960MM\tCONNECTOR",
            "REF\tPART NUMBER\t\t\tMANUFACTURER\tMANUFACTURER PART NUMBER\tQTY\t\tCONNECTOR",
        ),
        ("0\t\t\t\tlinvA\t\t\tL", "3\t\t\t\tlloadA\t\t\tL"),
        ("6\t\t\t\tVpccA\t\t\tL", "14\t\t\t\t--\t\t\t\tL"),
    ];

    for (text1, text2) in cases {
        let mut glyphs = Vec::new();
        let mut bbox = Box2I::default();

        let output1 = font.text_as_glyphs(
            Some(&mut bbox),
            Some(&mut glyphs),
            text1,
            Vector2I::new(1000, 1000),
            Vector2I::new(0, 0),
            ANGLE_0,
            false,
            Vector2I::new(0, 0),
            0,
        );
        let output2 = font.text_as_glyphs(
            Some(&mut bbox),
            Some(&mut glyphs),
            text2,
            Vector2I::new(1000, 1000),
            Vector2I::new(0, 0),
            ANGLE_0,
            false,
            Vector2I::new(0, 0),
            0,
        );

        assert_eq!(
            output1.x, output2.x,
            "Incorrect tab size for \n\t'{text1}' and\n\t'{text2}'"
        );
    }
}

#[test]
fn zero_size_tab() {
    let font = StrokeFont::load_font("").unwrap();
    let mut glyphs = Vec::new();
    let mut bbox = Box2I::default();

    // Zero-size pin names are how a symbol hides one name, and they can hold tabs
    let end = font.text_as_glyphs(
        Some(&mut bbox),
        Some(&mut glyphs),
        "A\tB",
        Vector2I::new(0, 0),
        Vector2I::new(0, 0),
        ANGLE_0,
        false,
        Vector2I::new(0, 0),
        0,
    );

    assert_eq!(end.x, 0);
}

/// Not in the C++ suite: basic facts about the glyph table (analysis document, section 9).
#[test]
fn glyph_table_basics() {
    let font = StrokeFont::load_font("").unwrap();

    assert_eq!(font.glyph_count(), 65487);
    // "JZ": from -8/21 to +8/21
    assert_eq!(
        font.glyph_bounding_box(0).width(),
        8.0 * (1.0 / 21.0) - -8.0 * (1.0 / 21.0)
    );
    assert!(font.glyph(65487).is_none());
    assert!(StrokeFont::load_font("Some Font").is_none());
}
