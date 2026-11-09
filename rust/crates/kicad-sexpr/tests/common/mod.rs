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

//! Helpers shared by the integration tests of kicad-sexpr.
//!
//! The text escaping, the FNV-1a hash and the token dump are the same as in the C++ oracle
//! harness (`tests/oracle/oracle.cpp`), so that outputs can be compared byte for byte.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use kicad_sexpr::keywords::{
    drawingsheet_t, drcrule_t, dsn, embedded_files_t, lib_table_t, nl_t, pcb_keys_t,
    pcbplotparams_t, strokeparams_t, tfield_t,
};
use kicad_sexpr::{DSN_EOF, DsnLexer, IoError, KeywordSet};

/// Escape every byte outside of 0x20..=0x7e, and the backslash, as `\xHH`.
pub fn esc(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());

    for &c in bytes {
        if !(0x20..=0x7e).contains(&c) || c == b'\\' {
            out.push_str(&format!("\\x{c:02x}"));
        } else {
            out.push(char::from(c));
        }
    }

    out
}

/// Undo [`esc`].
pub fn unesc(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 3 < bytes.len() && bytes[i + 1] == b'x' {
            let hex = std::str::from_utf8(&bytes[i + 2..i + 4]).expect("hex digits");
            out.push(u8::from_str_radix(hex, 16).expect("hex digits"));
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    out
}

/// [`unesc`] for text that must be valid UTF-8.
pub fn unesc_str(text: &str) -> String {
    String::from_utf8(unesc(text)).expect("escaped text is valid UTF-8")
}

/// 64 bit FNV-1a hash.
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for &c in data {
        hash ^= u64::from(c);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    hash
}

/// `<crate>/tests/golden/<name>`.
///
/// The environment variable `KICAD_SEXPR_GOLDEN_DIR` selects another directory.  This is for
/// running the comparisons of `oracle_goldens.rs` on further inputs, e.g. randomly generated
/// ones: put `*.txt` inputs there, make the `*.out` files with the oracle harness
/// (`tests/oracle`), and run the tests with the variable set.
pub fn golden_path(name: &str) -> PathBuf {
    match std::env::var_os("KICAD_SEXPR_GOLDEN_DIR") {
        Some(dir) => PathBuf::from(dir).join(name),
        None => Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("golden")
            .join(name),
    }
}

/// The content of a golden file.
pub fn golden(name: &str) -> String {
    let path = golden_path(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The keyword table of a keyword set by the name of its module, or "none".
pub fn keyword_table(name: &str) -> &'static [&'static str] {
    match name {
        "pcb_keys_t" => pcb_keys_t::T::KEYWORDS,
        "pcbplotparams_t" => pcbplotparams_t::T::KEYWORDS,
        "strokeparams_t" => strokeparams_t::T::KEYWORDS,
        "embedded_files_t" => embedded_files_t::T::KEYWORDS,
        "drcrule_t" => drcrule_t::T::KEYWORDS,
        "lib_table_t" => lib_table_t::T::KEYWORDS,
        "drawingsheet_t" => drawingsheet_t::T::KEYWORDS,
        "nl_t" => nl_t::T::KEYWORDS,
        "tfield_t" => tfield_t::T::KEYWORDS,
        "dsn" => dsn::T::KEYWORDS,
        "none" => &[],
        other => panic!("unknown keyword set {other}"),
    }
}

/// The keyword set the oracle uses for a corpus file (`keywordsForFile` in oracle.cpp).
pub fn keywords_for_file(path: &str) -> &'static str {
    if path.ends_with(".kicad_pcb") || path.ends_with(".kicad_mod") {
        "pcb_keys_t"
    } else if path.ends_with(".kicad_dru") {
        "drcrule_t"
    } else if path.ends_with("-lib-table") {
        "lib_table_t"
    } else if path.ends_with(".kicad_wks") {
        "drawingsheet_t"
    } else {
        "none"
    }
}

/// One line of a token dump (`tokenLine` in oracle.cpp).
pub fn token_line(lexer: &DsnLexer<i32>, tok: i32) -> String {
    format!(
        "tok\t{}\t{}\t{}\t{}\t{}",
        tok,
        lexer.cur_offset(),
        lexer.cur_line_number(),
        esc(lexer.cur_bytes()),
        esc(lexer.cur_separator().as_bytes())
    )
}

/// One line describing an error (`errorLine` in oracle.cpp).
pub fn error_line(error: &IoError) -> String {
    match error.as_parse_error() {
        Some(pe) => format!(
            "parse_error\t{}\t{}\t{}\t{}\t{}",
            esc(pe.what().as_bytes()),
            esc(pe.parse_problem().as_bytes()),
            pe.line_number,
            pe.byte_index,
            esc(pe.input_line.as_bytes())
        ),
        None => format!("io_error\t{}", esc(error.what().as_bytes())),
    }
}

/// Dump all the tokens of a text (`lexDump` in oracle.cpp); returns the dump and the number of
/// tokens including the final EOF token.
pub fn lex_dump(keywords: &str, knows_bar: bool, text: &str, source: &str) -> (String, usize) {
    let mut lexer = DsnLexer::<i32>::with_keywords(keyword_table(keywords), text, source);
    let mut out = String::new();
    let mut count = 0;

    lexer.set_knows_bar(knows_bar);

    loop {
        match lexer.next_tok() {
            Ok(tok) => {
                out.push_str(&token_line(&lexer, tok));
                out.push('\n');
                count += 1;

                if tok == DSN_EOF {
                    break;
                }
            }
            Err(e) => {
                out.push_str(&error_line(&e));
                out.push('\n');
                break;
            }
        }
    }

    (out, count)
}

/// Compare a text produced by the port with the golden text produced by the C++ oracle, line
/// by line, with a readable report of the first differences.
pub fn assert_same_lines(what: &str, got: &str, expected: &str) {
    if got == expected {
        return;
    }

    let got_lines: Vec<&str> = got.split('\n').collect();
    let expected_lines: Vec<&str> = expected.split('\n').collect();
    let mut report = String::new();
    let mut differences = 0;

    for i in 0..got_lines.len().max(expected_lines.len()) {
        let g = got_lines.get(i).copied();
        let e = expected_lines.get(i).copied();

        if g != e {
            differences += 1;

            if differences <= 10 {
                report.push_str(&format!(
                    "line {}:\n   rust: {}\n    c++: {}\n",
                    i + 1,
                    g.unwrap_or("<missing>"),
                    e.unwrap_or("<missing>")
                ));
            }
        }
    }

    panic!("{what}: {differences} line(s) differ from the C++ oracle\n{report}");
}
