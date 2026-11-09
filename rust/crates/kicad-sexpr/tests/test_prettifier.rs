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

//! Port of `qa/tests/pcbnew/test_prettifier.cpp`, and of the formatting check of
//! `qa/tests/common/test_library_tables.cpp`.

use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::{DsnLexer, FormatMode, IoError, prettify, prettify_in_place};

/// The tokens of a board or footprint file: token and text, without positions.
fn token_stream(text: &str, source: &str) -> Result<Vec<(T, String)>, IoError> {
    let mut lexer = DsnLexer::<T>::new(text, source);
    let mut tokens = Vec::new();

    loop {
        let tok = lexer.next_tok()?;

        if tok == T::T_EOF {
            return Ok(tokens);
        }

        tokens.push((tok, lexer.cur_text().to_string()));
    }
}

/// `BOOST_FIXTURE_TEST_CASE( BoardAndFootprintPrettifier, PRETTIFIER_TEST_FIXTURE )`
///
/// The C++ test parses the original and the prettified text with
/// `PCB_IO_KICAD_SEXPR_PARSER` and requires equal boards.  The board parser is not part of
/// this crate (it is built on top of it in `kicad-pcb`), so "parses the same way" is checked
/// on the token streams instead: the lexer must see the same tokens with the same texts.  The
/// comparison with the golden file is as in C++.
#[test]
fn board_and_footprint_prettifier() {
    let cases = [
        "Reverb_BTDR-1V.kicad_mod",
        "Samtec_HLE-133-02-xx-DV-PE-LC_2x33_P2.54mm_Horizontal.kicad_mod",
        "group_and_image.kicad_pcb",
    ];

    for test_case in cases {
        let in_path = kicad_qa::pcbnew_data_dir()
            .join("prettifier")
            .join(test_case);

        let mut in_data = std::fs::read_to_string(&in_path)
            .unwrap_or_else(|e| panic!("{test_case}: cannot read {}: {e}", in_path.display()));

        let original =
            token_stream(&in_data, "input file").unwrap_or_else(|e| panic!("{test_case}: {e}"));
        assert!(!original.is_empty(), "{test_case}");

        prettify_in_place(&mut in_data, FormatMode::Normal);

        let prettified = token_stream(&in_data, "prettified file")
            .unwrap_or_else(|e| panic!("{test_case}: {e}"));

        // File should parse the same way
        assert!(
            original == prettified,
            "{test_case}: Formatted version of original board item does not parse the same way!"
        );

        // And the formatting should match the golden
        let (base, ext) = test_case
            .rsplit_once('.')
            .expect("file name with extension");

        let golden_path = kicad_qa::pcbnew_data_dir()
            .join("prettifier")
            .join(format!("{base}_formatted.{ext}"));

        let golden = std::fs::read_to_string(&golden_path)
            .unwrap_or_else(|e| panic!("{test_case}: cannot read {}: {e}", golden_path.display()));

        assert!(
            golden == in_data,
            "{test_case}: Formatting result doesn't match golden!"
        );
    }
}

/// The formatting part of `BOOST_AUTO_TEST_CASE( ParseFromFile )` in
/// `qa/tests/common/test_library_tables.cpp`.
///
/// The C++ test loads each library table, writes it with `LIBRARY_TABLE::Format` and requires
/// `Prettify( ..., FORMAT_MODE::LIBRARY_TABLE )` of that to equal the file.  `LIBRARY_TABLE` is
/// not part of this crate; what can be checked here is the consequence for the formatter: the
/// files of the cases with `check_formatted` are fixed points of the library table mode.
#[test]
fn library_tables_are_fixed_points_of_library_table_mode() {
    // { .filename, .check_formatted } of the cases that load without error
    let cases = [
        ("sym-lib-table", true),
        ("fp-lib-table", true),
        ("nested-symbols", true),
        ("nested-disabled", true),
        ("nested-hidden", true),
        ("cycle", true),
        ("sym-hand-edited", false),
    ];

    for (filename, check_formatted) in cases {
        let path = kicad_qa::qa_data_dir().join("libraries").join(filename);
        let in_data = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{filename}: cannot read {}: {e}", path.display()));

        let formatted = prettify(&in_data, FormatMode::LibraryTable);

        if check_formatted {
            assert!(
                formatted == in_data,
                "{filename}:\n--- original ---\n{in_data}\n--- formatted ---\n{formatted}"
            );
        } else {
            // The hand edited table is exactly the case that is not in canonical form.
            assert!(formatted != in_data, "{filename}");
        }

        // Formatting is stable either way.
        assert!(
            prettify(&formatted, FormatMode::LibraryTable) == formatted,
            "{filename}"
        );
    }
}
