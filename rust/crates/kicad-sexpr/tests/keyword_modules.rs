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

//! The token enums in `src/lexer/keywords/` against the `*.keywords` files of the C++ tree.
//!
//! Run with `KICAD_SEXPR_REGENERATE_KEYWORDS=1` to rewrite the modules after a keywords file
//! changed (C++ regenerates its lexers at build time).

mod common;

use std::path::Path;

use common::keyword_table;
use kicad_sexpr::keywords::{
    KeywordsModuleSpec, generate_keywords_module, parse_token_list, token_list_copyright_lines,
};

/// (module / C++ enum namespace in lower case, C++ enum namespace, keywords file)
const KEYWORD_SETS: [(&str, &str, &str); 10] = [
    ("pcb_keys_t", "PCB_KEYS_T", "common/pcb.keywords"),
    (
        "pcbplotparams_t",
        "PCBPLOTPARAMS_T",
        "common/pcb_plot_params.keywords",
    ),
    (
        "strokeparams_t",
        "STROKEPARAMS_T",
        "common/stroke_params.keywords",
    ),
    (
        "embedded_files_t",
        "EMBEDDED_FILES_T",
        "common/embedded_files.keywords",
    ),
    ("drcrule_t", "DRCRULE_T", "common/drc_rules.keywords"),
    ("lib_table_t", "LIB_TABLE_T", "common/lib_table.keywords"),
    (
        "drawingsheet_t",
        "DRAWINGSHEET_T",
        "common/drawing_sheet/drawing_sheet.keywords",
    ),
    ("nl_t", "NL_T", "common/netlist.keywords"),
    (
        "tfield_t",
        "TFIELD_T",
        "common/template_fieldnames.keywords",
    ),
    (
        "dsn",
        "DSN",
        "pcbnew/specctra_import_export/specctra.keywords",
    ),
];

fn read_keywords_file(input_file: &str) -> String {
    let path = kicad_qa::kicad_source_root().join(input_file);

    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The shipped tables are what the port of `TokenList2DsnLexer.cmake` makes of the current
/// keywords files: same keywords, same token numbers.
#[test]
fn keyword_tables_match_keywords_files() {
    for (module, _, input_file) in KEYWORD_SETS {
        let tokens = parse_token_list(&read_keywords_file(input_file), input_file)
            .unwrap_or_else(|e| panic!("{e}"));

        assert!(
            tokens == keyword_table(module),
            "kicad_sexpr::keywords::{module} is out of date with {input_file}; regenerate with \
             KICAD_SEXPR_REGENERATE_KEYWORDS=1 cargo test -p kicad-sexpr --test keyword_modules"
        );
    }
}

/// The module sources are exactly what the generator writes.
#[test]
fn keyword_modules_are_in_sync() {
    let regenerate = std::env::var_os("KICAD_SEXPR_REGENERATE_KEYWORDS").is_some();

    for (module, enum_namespace, input_file) in KEYWORD_SETS {
        let text = read_keywords_file(input_file);
        let tokens = parse_token_list(&text, input_file).unwrap_or_else(|e| panic!("{e}"));
        let copyright_lines = token_list_copyright_lines(&text);

        let source = generate_keywords_module(
            &KeywordsModuleSpec {
                enum_namespace,
                input_file,
                crate_path: "crate",
                copyright_lines: &copyright_lines,
            },
            &tokens,
        );

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("lexer")
            .join("keywords")
            .join(format!("{module}.rs"));

        if regenerate {
            std::fs::write(&path, &source)
                .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
            continue;
        }

        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        assert!(
            committed == source,
            "{} differs from the generator output; regenerate with \
             KICAD_SEXPR_REGENERATE_KEYWORDS=1 cargo test -p kicad-sexpr --test keyword_modules",
            path.display()
        );
    }
}
