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

//! Round-trip tests of the lexer and of `prettify` on real files of the KiCad source tree
//! (`qa/data` and `demos`).
//!
//! - [`token_streams_are_sane_and_prettify_is_idempotent`] is a property test that needs
//!   nothing but the files.
//! - [`corpus_matches_cpp`] compares the port with the C++ original on every file listed in
//!   `tests/golden/corpus.out`, using hashes the oracle harness (`tests/oracle`) recorded.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use common::{fnv1a, golden, keywords_for_file, lex_dump};
use kicad_sexpr::keywords::pcb_keys_t::T;
use kicad_sexpr::{DsnLexer, DsnSyntaxT, FormatMode, KeywordSet, prettify};

/// Files above this size are left to the tests that are run on request (`--ignored`).
const BIG_FILE: u64 = 12 * 1024 * 1024;

/// One line of `corpus.out`.
struct Golden {
    path: String,
    input_hash: u64,
    token_count: usize,
    tokens_hash: u64,
    prettify_normal_hash: u64,
    prettify_compact_hash: u64,
    prettify_normal_is_identity: bool,
}

fn read_corpus_golden() -> Vec<Golden> {
    let hex = |text: &str| u64::from_str_radix(text, 16).expect("hash");

    golden("corpus.out")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            assert_eq!(fields.len(), 7, "malformed corpus.out line: {line}");

            Golden {
                path: fields[0].to_string(),
                input_hash: hex(fields[1]),
                token_count: fields[2].parse().expect("token count"),
                tokens_hash: hex(fields[3]),
                prettify_normal_hash: hex(fields[4]),
                prettify_compact_hash: hex(fields[5]),
                prettify_normal_is_identity: fields[6] == "1",
            }
        })
        .collect()
}

/// Lex a whole file and check the sanity of the token stream: no error, the parentheses are
/// balanced and never close more than was opened, and the end of the input is reached.
///
/// Returns the number of tokens and the format version if the file starts with
/// `(<name> (version N)`.
fn check_token_stream<K: KeywordSet>(path: &Path, text: &str) -> (usize, Option<i64>) {
    let mut lexer = DsnLexer::<K>::new(text, &path.display().to_string());
    let mut depth: i64 = 0;
    let mut count = 0;
    let mut version = None;
    let mut recent: Vec<(i32, String)> = Vec::new();

    loop {
        let tok = lexer
            .next_tok()
            .unwrap_or_else(|e| panic!("{}: lexer error: {e}", path.display()))
            .to_i32();

        count += 1;

        if count <= 5 {
            recent.push((tok, lexer.cur_text().to_string()));

            // ( name ( version N
            if count == 5
                && recent[0].0 == kicad_sexpr::DSN_LEFT
                && recent[2].0 == kicad_sexpr::DSN_LEFT
                && recent[3].1 == "version"
                && recent[4].0 == kicad_sexpr::DSN_NUMBER
            {
                version = recent[4].1.parse().ok();
            }
        }

        match tok {
            kicad_sexpr::DSN_LEFT => depth += 1,
            kicad_sexpr::DSN_RIGHT => {
                depth -= 1;
                assert!(
                    depth >= 0,
                    "{}: unbalanced ')' at line {}, offset {}",
                    path.display(),
                    lexer.cur_line_number(),
                    lexer.cur_offset()
                );
            }
            kicad_sexpr::DSN_EOF => break,
            _ => {}
        }
    }

    assert_eq!(depth, 0, "{}: {depth} unclosed '('", path.display());

    // The end of the input stays the end of the input.
    assert_eq!(
        lexer.next_tok().expect("EOF").to_i32(),
        kicad_sexpr::DSN_EOF
    );
    assert_eq!(lexer.cur_tok().to_i32(), kicad_sexpr::DSN_EOF);

    (count, version)
}

/// `prettify( prettify( x ) ) == prettify( x )` in both modes that board files are saved with;
/// returns `prettify( x ) == x` for the normal mode.
fn check_prettify(path: &Path, text: &str) -> bool {
    let normal = prettify(text, FormatMode::Normal);

    assert!(
        prettify(&normal, FormatMode::Normal) == normal,
        "{}: prettify is not idempotent (normal mode)",
        path.display()
    );

    let compact = prettify(text, FormatMode::CompactTextProperties);

    assert!(
        prettify(&compact, FormatMode::CompactTextProperties) == compact,
        "{}: prettify is not idempotent (compact mode)",
        path.display()
    );

    normal == text
}

/// Every `step`-th of the files below `dir` with the extension `ext` that are not big, in
/// sorted order.
fn sample(dir: &Path, ext: &str, step: usize) -> Vec<PathBuf> {
    kicad_qa::files_with_extension(dir, ext)
        .into_iter()
        .filter(|path| std::fs::metadata(path).is_ok_and(|meta| meta.len() <= BIG_FILE))
        .step_by(step)
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The first format version whose files KiCad writes through `Prettify` (KiCad 8).  Older
/// files were laid out by the writers themselves (two space indentation, several lists per
/// line) and are no fixed points.
const FIRST_PRETTIFIED_VERSION: i64 = 20240108;

#[test]
fn token_streams_are_sane_and_prettify_is_idempotent() {
    // All boards of the pcbnew test data and of the demos (a few hundred), except the very big
    // ones; chosen by sorted file name, so the choice is deterministic.
    let mut boards = sample(&kicad_qa::pcbnew_data_dir(), "kicad_pcb", 1);
    boards.extend(sample(&kicad_qa::demos_dir(), "kicad_pcb", 1));

    assert!(boards.len() >= 40, "only {} boards found", boards.len());

    // What the C++ Prettify does with each file of the corpus, for the fixed point check.
    let cpp: BTreeMap<PathBuf, Golden> = read_corpus_golden()
        .into_iter()
        .map(|g| (kicad_qa::kicad_source_root().join(&g.path), g))
        .collect();

    let mut newest_version = 0;
    let mut fixed_points: Vec<(PathBuf, i64, bool)> = Vec::new();

    for path in &boards {
        let text = read(path);

        let (count, version) = check_token_stream::<T>(path, &text);
        assert!(count > 1, "{}: no tokens", path.display());

        let is_fixed_point = check_prettify(path, &text);

        if let Some(version) = version {
            newest_version = newest_version.max(version);
            fixed_points.push((path.clone(), version, is_fixed_point));
        }
    }

    // KiCad's own saved files are fixed points of Prettify.
    let mut newest = 0;
    let mut modern = 0;
    let mut modern_fixed = 0;

    for (path, version, is_fixed_point) in &fixed_points {
        if *version == newest_version {
            newest += 1;
            assert!(
                is_fixed_point,
                "{}: has the newest format version {version} but prettify() changes it",
                path.display()
            );
        }

        if *version >= FIRST_PRETTIFIED_VERSION {
            modern += 1;

            if *is_fixed_point {
                modern_fixed += 1;
            } else if let Some(g) = cpp.get(path) {
                // Not a fixed point: then it must not be one for the C++ Prettify either (the
                // file was edited by hand or saved in the compact mode; see PORT_STATUS.md).
                // Otherwise the port is wrong.
                if g.input_hash == fnv1a(read(path).as_bytes()) {
                    assert!(
                        !g.prettify_normal_is_identity,
                        "{}: fixed point of the C++ Prettify, but not of the port",
                        path.display()
                    );
                }
            }
        }
    }

    assert!(newest >= 1);
    assert!(
        modern >= 40,
        "only {modern} boards of format version {FIRST_PRETTIFIED_VERSION}+"
    );

    // The bulk of the modern boards are files as KiCad saved them.
    assert!(
        modern_fixed * 10 >= modern * 8,
        "only {modern_fixed} of {modern} modern boards are fixed points of prettify()"
    );

    // Footprints, custom rules, symbol libraries and drawing sheets.
    let mut others = sample(&kicad_qa::qa_data_dir(), "kicad_mod", 8);
    let footprints = others.len();
    others.extend(sample(&kicad_qa::qa_data_dir(), "kicad_sym", 3));
    others.extend(sample(&kicad_qa::qa_data_dir(), "kicad_wks", 1));

    assert!(footprints >= 10, "only {footprints} footprints found");

    for path in &others {
        let text = read(path);

        if path.extension().is_some_and(|ext| ext == "kicad_mod") {
            check_token_stream::<T>(path, &text);
        } else {
            check_token_stream::<DsnSyntaxT>(path, &text);
        }

        check_prettify(path, &text);
    }

    // Custom rule files: comments, and the test data contains deliberately broken ones, so only
    // the lexer has to get through them.
    let rules = sample(&kicad_qa::qa_data_dir(), "kicad_dru", 1);

    assert!(rules.len() >= 10, "only {} rule files found", rules.len());

    for path in &rules {
        let text = read(path);
        let mut lexer =
            DsnLexer::<kicad_sexpr::keywords::drcrule_t::T>::new(text.as_str(), "rules");

        loop {
            match lexer.next_tok() {
                Ok(kicad_sexpr::keywords::drcrule_t::T::T_EOF) => break,
                Ok(_) => {}
                // An unterminated string is the one thing the lexer itself refuses.
                Err(e) => {
                    assert!(
                        e.problem().starts_with("Unterminated delimited string"),
                        "{}: {e}",
                        path.display()
                    );
                    break;
                }
            }
        }

        check_prettify(path, &text);
    }
}

/// Compare the port with the C++ original on the files of `corpus.out`: same token dump (token
/// values, offsets, line numbers, texts and separators) and same prettified text in the normal
/// and in the compact mode.
fn compare_with_cpp(select: impl Fn(u64) -> bool) -> (usize, usize) {
    let root = kicad_qa::kicad_source_root();
    let mut compared = 0;
    let mut skipped = 0;
    let mut failures: Vec<String> = Vec::new();

    for g in read_corpus_golden() {
        let path = root.join(&g.path);

        let Ok(bytes) = std::fs::read(&path) else {
            skipped += 1;
            continue;
        };

        if !select(bytes.len() as u64) {
            continue;
        }

        // A file that changed since the golden was made cannot be compared.
        if fnv1a(&bytes) != g.input_hash {
            skipped += 1;
            continue;
        }

        let Ok(text) = String::from_utf8(bytes) else {
            skipped += 1;
            continue;
        };

        compared += 1;

        let (dump, token_count) = lex_dump(keywords_for_file(&g.path), true, &text, &g.path);

        if token_count != g.token_count || fnv1a(dump.as_bytes()) != g.tokens_hash {
            failures.push(format!(
                "{}: token dump differs ({} tokens, C++ {})",
                g.path, token_count, g.token_count
            ));
        }

        let normal = prettify(&text, FormatMode::Normal);

        if fnv1a(normal.as_bytes()) != g.prettify_normal_hash {
            failures.push(format!("{}: prettify (normal) differs", g.path));
        }

        if (normal == text) != g.prettify_normal_is_identity {
            failures.push(format!("{}: prettify fixed point status differs", g.path));
        }

        let compact = prettify(&text, FormatMode::CompactTextProperties);

        if fnv1a(compact.as_bytes()) != g.prettify_compact_hash {
            failures.push(format!("{}: prettify (compact) differs", g.path));
        }
    }

    assert!(
        failures.is_empty(),
        "{} difference(s) to the C++ oracle:\n{}",
        failures.len(),
        failures.join("\n")
    );

    (compared, skipped)
}

#[test]
fn corpus_matches_cpp() {
    let (compared, skipped) = compare_with_cpp(|size| size <= BIG_FILE);

    // The golden was made from this very tree, so (nearly) every file must take part.
    assert!(
        compared >= 400,
        "only {compared} files compared, {skipped} skipped"
    );
    assert!(
        skipped * 20 <= compared,
        "{skipped} of {} files skipped",
        compared + skipped
    );
}

/// The same for the few very big boards (30 to 85 MB each); takes a while in a debug build.
/// Run with `cargo test -p kicad-sexpr --test corpus -- --ignored`.
#[test]
#[ignore = "lexes and formats about 230 MB of boards"]
fn big_corpus_matches_cpp() {
    let (compared, _) = compare_with_cpp(|size| size > BIG_FILE);

    assert!(compared >= 1);
}
