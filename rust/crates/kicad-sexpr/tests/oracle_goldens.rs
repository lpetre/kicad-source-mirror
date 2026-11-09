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

//! Differential tests against the original C++ code.
//!
//! `tests/oracle/oracle.cpp` is a small program that links KiCad's own `dsnlexer.cpp`,
//! `richio.cpp`, `exceptions.cpp`, `kicad_io_utils.cpp`, `libs/sexpr` and the lexers the CMake
//! generator makes from the `*.keywords` files.  `tests/oracle/generate_goldens.sh` ran it on the
//! inputs in `tests/golden/*.txt` and stored what it printed in `tests/golden/*.out`.  The tests
//! here feed the same inputs to the port and require the same output, byte for byte.
//!
//! There is no C++ unit test for `DSNLEXER` in `qa/`; this file takes that place.

mod common;

use common::{
    assert_same_lines, error_line, esc, golden, keyword_table, token_line, unesc, unesc_str,
};
use kicad_sexpr::lexer::c_numeric::{from_chars_double, strtol, strtoll};
use kicad_sexpr::{
    DSN_EOF, DsnLexer, FormatMode, FutureFormatError, IoError, OutputFormatter, Parser, Sexpr,
    SexprChildrenArg, SexprDouble, SexprException, SexprInteger, SexprList, SexprString,
    SexprSymbol, StringFormatter, StringLineReader, as_string, as_string_in, as_symbol,
    as_symbol_in, format_bool, format_custom_properties, format_e, format_f, format_g,
    format_opt_bool, format_stream_data, format_uuid, is_number_text, prettify_bytes,
};

/// The data lines of a golden input: everything but `#` comment lines (and, unless empty lines
/// are data, empty lines).
fn data_lines(text: &str, empty_lines_are_data: bool) -> impl Iterator<Item = &str> {
    text.lines()
        .filter(move |line| !line.starts_with('#') && (empty_lines_are_data || !line.is_empty()))
}

fn split_command(line: &str) -> (&str, &str) {
    match line.split_once(' ') {
        Some((cmd, arg)) => (cmd, arg),
        None => (line, ""),
    }
}

fn int_arg(arg: &str) -> i32 {
    arg.trim().parse().unwrap_or(0)
}

/// `esc( lexer->CurLine() )`: the C++ accessor returns a C string, which ends at a NUL.
fn c_str(text: &str) -> &str {
    text.split('\0').next().unwrap_or("")
}

/// The interpreter for `lexer_script.txt` (`modeScript` in oracle.cpp).
fn run_lexer_script(script: &str) -> String {
    let mut out = String::new();
    let mut main_lexer: Option<DsnLexer<i32>> = None;
    let mut sub_lexer: Option<DsnLexer<i32>> = None;
    let mut source = String::from("src");

    for line in data_lines(script, false) {
        out.push_str(&format!("> {line}\n"));

        let (cmd, arg) = split_command(line);

        if cmd == "source" {
            source = unesc_str(arg);
            continue;
        }

        if cmd == "lexer_repeat" {
            // lexer_repeat <keywords> <count> <text>: the text repeated <count> times
            let mut fields = arg.splitn(3, ' ');
            let kw = fields.next().expect("keywords");
            let count: usize = fields.next().expect("count").parse().expect("count");
            let piece = unesc_str(fields.next().expect("text"));

            sub_lexer = None;
            main_lexer = Some(DsnLexer::<i32>::with_keywords(
                keyword_table(kw),
                piece.repeat(count),
                &source,
            ));

            continue;
        }

        if cmd == "lexer" || cmd == "lexer_no_reader" {
            let (kw, text) = split_command(arg);

            sub_lexer = None;
            main_lexer = Some(if cmd == "lexer" {
                DsnLexer::<i32>::with_keywords(keyword_table(kw), unesc_str(text), &source)
            } else {
                DsnLexer::<i32>::with_keywords_from_line_reader(keyword_table(kw), None)
            });

            continue;
        }

        // Commands on both lexers.
        match cmd {
            "sub" => {
                // SUB_PARSER sub( reader ); sub.SyncLineReaderWith( *this );
                let main = main_lexer.as_mut().expect("lexer");
                let mut sub =
                    DsnLexer::<i32>::with_keywords_from_line_reader(keyword_table(arg), None);

                out.push_str(&format!(
                    "sync\t{}\n",
                    i32::from(sub.sync_line_reader_with(main))
                ));
                sub_lexer = Some(sub);
                continue;
            }
            "endsub" => {
                // SyncLineReaderWith( sub );
                let main = main_lexer.as_mut().expect("lexer");
                let mut sub = sub_lexer.take().expect("sub lexer");

                out.push_str(&format!(
                    "sync\t{}\n",
                    i32::from(main.sync_line_reader_with(&mut sub))
                ));
                continue;
            }
            "dropsub" => {
                let main = main_lexer.as_mut().expect("lexer");
                let mut sub = sub_lexer.take().expect("sub lexer");

                main.reclaim_line_reader_from(&mut sub);
                continue;
            }
            _ => {}
        }

        let lexer = match sub_lexer.as_mut() {
            Some(sub) => sub,
            None => main_lexer.as_mut().expect("lexer"),
        };

        let result: Result<(), IoError> = (|| {
            match cmd {
                "specctra" => lexer.set_specctra_mode(arg == "1"),
                "knowsbar" => lexer.set_knows_bar(arg == "1"),
                "comments" => {
                    let old = lexer.set_comments_are_tokens(arg == "1");
                    out.push_str(&format!("old\t{}\n", i32::from(old)));
                }
                "delim" => {
                    let old = lexer.set_string_delimiter(unesc(arg)[0]);
                    out.push_str(&format!("old\t{old}\n"));
                }
                "spaceinquoted" => {
                    let old = lexer.set_space_in_quoted_tokens(arg == "1");
                    out.push_str(&format!("old\t{}\n", i32::from(old)));
                }
                "next" => {
                    let tok = lexer.next_tok()?;
                    out.push_str(&format!("{}\n", token_line(lexer, tok)));
                }
                "nexttok" => {
                    let tok = lexer.next_tok()?;
                    out.push_str(&format!("token\t{tok}\n"));
                }
                "all" => {
                    for _ in 0..100_000 {
                        let tok = lexer.next_tok()?;
                        out.push_str(&format!("{}\n", token_line(lexer, tok)));

                        if tok == DSN_EOF {
                            break;
                        }
                    }
                }
                "needleft" => {
                    lexer.need_left()?;
                    out.push_str("ok\n");
                }
                "needright" => {
                    lexer.need_right()?;
                    out.push_str("ok\n");
                }
                "needbar" => {
                    lexer.need_bar()?;
                    out.push_str("ok\n");
                }
                "needsymbol" => {
                    let tok = lexer.need_symbol()?;
                    out.push_str(&format!("{}\n", token_line(lexer, tok)));
                }
                "needsymbolornumber" => {
                    let tok = lexer.need_symbol_or_number()?;
                    out.push_str(&format!("{}\n", token_line(lexer, tok)));
                }
                "neednumber" => {
                    let tok = lexer.need_number(&unesc_str(arg))?;
                    out.push_str(&format!("{}\n", token_line(lexer, tok)));
                }
                "expecting" => return Err(lexer.expecting(int_arg(arg))),
                "expectingstr" => return Err(lexer.expecting_str(&unesc_str(arg))),
                "unexpected" => return Err(lexer.unexpected(int_arg(arg))),
                "unexpectedstr" => return Err(lexer.unexpected_str(&unesc_str(arg))),
                "duplicate" => return Err(lexer.duplicate(int_arg(arg))),
                "state" => {
                    out.push_str(&format!(
                        "state\tcur={}\tprev={}\ttext={}\toffset={}\tline={}\tsource={}\t\
                         curline={}\tsep={}\n",
                        lexer.cur_tok(),
                        lexer.prev_tok(),
                        esc(lexer.cur_bytes()),
                        lexer.cur_offset(),
                        lexer.cur_line_number(),
                        esc(lexer.cur_source().as_bytes()),
                        esc(c_str(lexer.cur_line()).as_bytes()),
                        esc(lexer.cur_separator().as_bytes())
                    ));
                }
                "readcomments" => match lexer.read_comment_lines()? {
                    None => out.push_str("comments\tnone\n"),
                    Some(comments) => {
                        out.push_str(&format!("comments\t{}\n", comments.len()));

                        for comment in comments {
                            out.push_str(&format!("\t{}\n", esc(comment.as_bytes())));
                        }
                    }
                },
                "parsedouble" | "parsedoubleexp" | "parsedoubletok" => {
                    let value = match cmd {
                        "parsedouble" => lexer.parse_double()?,
                        "parsedoubleexp" => lexer.parse_double_expected(&unesc_str(arg))?,
                        _ => lexer.parse_double_tok(int_arg(arg))?,
                    };

                    out.push_str(&format!("double\t{:016x}\n", value.to_bits()));
                }
                "parseint" => out.push_str(&format!("int\t{}\n", lexer.parse_int())),
                "parseintexp" => {
                    let value = lexer.parse_int_expected(&unesc_str(arg))?;
                    out.push_str(&format!("int\t{value}\n"));
                }
                "parsehex" => {
                    let value = lexer.parse_hex()?;
                    out.push_str(&format!("long\t{value}\n"));
                }
                "tokentext" => {
                    out.push_str(&format!(
                        "text\t{}\n",
                        esc(lexer.token_text(int_arg(arg)).as_bytes())
                    ));
                }
                "tokenstring" => {
                    out.push_str(&format!(
                        "text\t{}\n",
                        esc(lexer.token_string(int_arg(arg)).as_bytes())
                    ));
                }
                "curstrastoken" => {
                    out.push_str(&format!("token\t{}\n", lexer.cur_str_as_token()));
                }
                "issymbol" => {
                    out.push_str(&format!(
                        "is_symbol\t{}\tis_number\t{}\n",
                        i32::from(DsnLexer::<i32>::is_symbol(int_arg(arg))),
                        i32::from(DsnLexer::<i32>::is_number(int_arg(arg)))
                    ));
                }
                "fromutf8" => {
                    out.push_str(&format!("text\t{}\n", esc(lexer.from_utf8().as_bytes())));
                }
                "push" => {
                    lexer.push_reader(Box::new(StringLineReader::new(unesc_str(arg), "pushed")));
                }
                "pop" => {
                    let popped = lexer.pop_reader();
                    let name = match &popped {
                        Some(reader) => esc(reader.source().as_bytes()),
                        None => "null".to_string(),
                    };

                    out.push_str(&format!("popped\t{name}\n"));
                }
                "init" => lexer.init_parser_state(),
                other => panic!("unknown command: {other}"),
            }

            Ok(())
        })();

        if let Err(e) = result {
            out.push_str(&format!("{}\n", error_line(&e)));
        }
    }

    out
}

#[test]
fn lexer_script_matches_cpp() {
    let got = run_lexer_script(&golden("lexer_script.txt"));

    assert_same_lines("lexer_script", &got, &golden("lexer_script.out"));
}

/// `DSNLEXER::isNumber`, `DSNLEXER::parseDouble`, `strtol` and `strtoll` (`modeNumbers`).
#[test]
fn numbers_match_cpp() {
    let mut got = String::new();

    for line in data_lines(&golden("numbers.txt"), false) {
        let text = unesc(line);

        let dbl = match from_chars_double(&text) {
            // The sign and payload of a NaN are of no interest.
            Ok((value, _)) if value.is_nan() => "nan".to_string(),
            Ok((value, _)) => format!("{:016x}", value.to_bits()),
            Err(_) => "error".to_string(),
        };

        got.push_str(&format!(
            "{}\tis_number={}\tdouble={}\tstrtol10={}\tstrtol16={}\tstrtoll0={}\n",
            line,
            i32::from(is_number_text(&text)),
            dbl,
            strtol(&text, 10).0,
            strtol(&text, 16).0,
            strtoll(&text, 0).0
        ));
    }

    assert_same_lines("numbers", &got, &golden("numbers.out"));
}

/// The `printf` and `{fmt}` float conversions (`modePrintf`).
#[test]
fn printf_conversions_match_cpp() {
    let mut got = String::new();

    for line in data_lines(&golden("printf.txt"), false) {
        let mut fields = line.split(' ');
        let kind = fields.next().expect("conversion");
        let precision: usize = fields
            .next()
            .expect("precision")
            .parse()
            .expect("precision");
        let bits = u64::from_str_radix(fields.next().expect("bits"), 16).expect("bits");
        let value = f64::from_bits(bits);

        let text = match kind {
            "g" => format_g(value, precision),
            "f" => format_f(value, precision),
            "e" => format_e(value, precision),
            other => panic!("unknown conversion {other}"),
        };

        // printf and {fmt} agree on every input, so one function serves both.
        got.push_str(&format!("{line}\tprintf={text}\tfmt={text}\n"));
    }

    assert_same_lines("printf", &got, &golden("printf.out"));
}

/// `OUTPUTFORMATTER::Quotes` and `OUTPUTFORMATTER::GetQuoteChar` (`modeQuotes`).
#[test]
fn quotes_match_cpp() {
    let formatter = StringFormatter::new();
    let mut got = String::new();

    for line in data_lines(&golden("quotes.txt"), true) {
        let text = unesc_str(line);

        got.push_str(&format!(
            "{}\tquotes={}\tquote_char={}\n",
            line,
            esc(formatter.quotes(&text).as_bytes()),
            esc(formatter.get_quote_char(&text).as_bytes())
        ));
    }

    assert_same_lines("quotes", &got, &golden("quotes.out"));
}

/// The `KICAD_FORMAT` helpers and `OUTPUTFORMATTER::Print` on fixed inputs (`modeFormat`).
#[test]
fn format_helpers_match_cpp() -> Result<(), IoError> {
    let mut got = String::new();

    {
        let mut out = StringFormatter::new();
        format_bool(&mut out, "locked", true)?;
        format_bool(&mut out, "hide", false)?;
        format_opt_bool(&mut out, "a", Some(true))?;
        format_opt_bool(&mut out, "b", Some(false))?;
        format_opt_bool(&mut out, "c", None)?;
        got.push_str(&format!("bool\t{}\n", esc(out.string().as_bytes())));
    }

    {
        let mut out = StringFormatter::new();
        format_uuid(&mut out, "0f1a2b3c-0000-4000-8000-00000000abcd")?;
        got.push_str(&format!("uuid\t{}\n", esc(out.string().as_bytes())));
    }

    {
        let mut out = StringFormatter::new();
        // The C++ item keeps its properties in a std::map, so they come out sorted by key.
        let mut props = std::collections::BTreeMap::new();
        format_custom_properties(&mut out, &props)?;
        props.insert("key", "value");
        props.insert("a \"b\"", "line1\nline2");
        format_custom_properties(&mut out, &props)?;
        got.push_str(&format!(
            "custom_properties\t{}\n",
            esc(out.string().as_bytes())
        ));
    }

    for size in [0usize, 1, 2, 3, 4, 56, 57, 58, 113, 114, 115, 300] {
        let mut out = StringFormatter::new();
        let data: Vec<u8> = (0..size).map(|i| (i * 37 + 11) as u8).collect();

        format_stream_data(&mut out, &data)?;
        got.push_str(&format!(
            "stream_data\t{}\t{}\n",
            size,
            esc(out.string().as_bytes())
        ));
    }

    {
        let mut out = StringFormatter::new();
        let a = out.print_nest(3, format_args!("(net {})", 42))?;
        let b = out.print(format_args!(
            "({} {} {})",
            "x",
            format_g(0.1, 6),
            format_f(2.5, 4)
        ))?;
        let c = out.print_nest(0, format_args!("{}", ""))?;
        let d = out.indent(2)?;
        got.push_str(&format!(
            "print\t{a}\t{b}\t{c}\t{d}\t{}\n",
            esc(out.string().as_bytes())
        ));
        out.strip_useless();
        got.push_str(&format!(
            "strip_useless\t{}\n",
            esc(out.string().as_bytes())
        ));
    }

    // The exception classes
    {
        let describe = |label: &str, error: &IoError| format!("{label}\t{}\n", error_line(error));

        got += &describe("io_error", &IoError::new("Line length exceeded"));
        got += &describe("io_cancelled", &IoError::cancelled());
        got += &describe("io_cancelled_msg", &IoError::cancelled_msg("stop"));

        let version_only = FutureFormatError::new("20250101", "");
        got += &describe("future_format", &version_only.into());

        let with_generator = FutureFormatError::new("20250101", "10.0");
        got += &describe("future_format_generator", &with_generator.clone().into());
        got += &format!(
            "future_format_fields\t{}\t{}\n",
            with_generator.required_version, with_generator.required_generator
        );

        let pe = IoError::parse("Unexpected 'foo'", "f.kicad_pcb", "the line\n", 3, 9);
        got += &describe("parse_error", &pe);

        let from_parse = pe
            .to_future_format_error("20250101", "")
            .expect("a PARSE_ERROR");
        got += &describe("future_format_from_parse", &from_parse.into());

        let from_parse_generator: IoError = pe
            .to_future_format_error("20250101", "10.0")
            .expect("a PARSE_ERROR")
            .into();
        got += &describe("future_format_from_parse_generator", &from_parse_generator);

        let again = from_parse_generator
            .to_future_format_error("1", "2")
            .expect("a PARSE_ERROR");
        got += &describe("future_format_from_future_format", &again.clone().into());
        got += &format!(
            "future_format_from_future_format_fields\t{}\t{}\n",
            again.required_version, again.required_generator
        );
    }

    assert_same_lines("format", &got, &golden("format.out"));
    Ok(())
}

/// `dumpTree` in oracle.cpp.
fn dump_tree(node: &Sexpr, depth: usize, out: &mut String) {
    let indent = " ".repeat(depth * 2);

    match node {
        Sexpr::List(list) => {
            out.push_str(&format!(
                "{}list line={} children={}\n",
                indent,
                node.line_number(),
                list.number_of_children()
            ));

            for child in list.children() {
                dump_tree(child, depth + 1, out);
            }
        }
        Sexpr::String(string) => {
            out.push_str(&format!(
                "{}string line={} {}\n",
                indent,
                node.line_number(),
                esc(string.value.as_bytes())
            ));
        }
        Sexpr::Symbol(symbol) => {
            out.push_str(&format!(
                "{}symbol line={} {}\n",
                indent,
                node.line_number(),
                esc(symbol.value.as_bytes())
            ));
        }
        Sexpr::Integer(integer) => {
            out.push_str(&format!(
                "{}integer line={} {}\n",
                indent,
                node.line_number(),
                integer.value
            ));
        }
        Sexpr::Double(double) => {
            out.push_str(&format!(
                "{}double line={} {:016x}\n",
                indent,
                node.line_number(),
                double.value.to_bits()
            ));
        }
    }
}

/// `SEXPR::PARSER` and `SEXPR::AsString` (`modeSexpr`).
#[test]
fn sexpr_parser_matches_cpp() {
    let mut got = String::new();

    for line in data_lines(&golden("sexpr.txt"), false) {
        got.push_str(&format!("> {line}\n"));

        let mut parser = Parser::new();

        match parser.parse(&unesc_str(line)) {
            Ok(None) => got.push_str("null\n"),
            Ok(Some(tree)) => {
                got.push_str(&format!(
                    "as_string\t{}\n",
                    esc(tree.as_string(0).as_bytes())
                ));
                dump_tree(&tree, 0, &mut got);
            }
            Err(SexprException::Parse(e)) => {
                got.push_str(&format!("parse_exception\t{}\n", e.what()));
            }
            Err(SexprException::InvalidType(e)) => {
                got.push_str(&format!("invalid_type_exception\t{}\n", e.what()));
            }
            Err(other) => panic!("unexpected exception {other:?}"),
        }
    }

    assert_same_lines("sexpr", &got, &golden("sexpr.out"));
}

/// `attempt` in oracle.cpp: the label, then the output of `f` or the exception it ends with.
fn attempt(out: &mut String, label: &str, f: impl FnOnce() -> Result<String, SexprException>) {
    let text = match f() {
        Ok(text) => text,
        Err(SexprException::InvalidType(e)) => format!("invalid_type_exception: {}", e.what()),
        Err(SexprException::Parse(e)) => format!("parse_exception: {}", e.what()),
        Err(SexprException::InvalidArgument(msg)) => format!("invalid_argument: {msg}"),
        Err(other) => panic!("{label}: unexpected {other:?}"),
    };

    out.push_str(&format!("{label}\t{text}\n"));
}

/// The programmatic interface of the SEXPR tree on fixed inputs (`modeSexprApi`).
#[test]
fn sexpr_api_matches_cpp() {
    // `std::cout << double`
    let num = |value: f64| format_g(value, 6);
    let mut got = String::new();

    // The variadic constructor / AddChildren
    {
        let mut list = SexprList::from_children([
            SexprChildrenArg::from(1),
            2i64.into(),
            3.5.into(),
            "str".into(),
            as_symbol("sym").into(),
            as_string("str2").into(),
            Sexpr::from(SexprSymbol::new("node")).into(),
        ]);

        got.push_str(&format!(
            "variadic\t{}\t{}\t{}\n",
            list.number_of_children(),
            list.line_number(),
            esc(list.as_string(0).as_bytes())
        ));

        list.add_children([SexprChildrenArg::from(-7), 0.25.into()]);
        got.push_str(&format!(
            "add_children\t{}\n",
            esc(list.as_string(0).as_bytes())
        ));
    }

    // The stream insertion operators
    let mut list = SexprList::new();

    {
        let mut sub = SexprList::with_line_number(7);
        sub.push(2).push(42.42).push("substring");

        list.push(as_symbol("name"))
            .push(as_string("text"))
            .push(5)
            .push(1_099_511_627_776_i64)
            .push(1.5)
            .push(2.5f32)
            .push(String::from("plain"))
            .push(sub)
            .push(Sexpr::from(SexprDouble::with_line_number(0.125, 3)))
            .push(9);

        got.push_str(&format!(
            "insert\t{}\t{}\n",
            list.number_of_children(),
            esc(list.as_string(0).as_bytes())
        ));
        got.push_str(&format!(
            "insert_level\t{}\n",
            esc(list.as_string(2).as_bytes())
        ));
        got.push_str(&format!(
            "sub_line\t{}\t{}\n",
            list.child(7).unwrap().line_number(),
            list.child(8).unwrap().line_number()
        ));
    }

    // The stream extraction operators
    {
        let mut symbol = String::new();
        let mut string = String::new();

        attempt(&mut got, "extract_string_as_symbol", || {
            list.read_in_string(as_string_in(&mut symbol))
                .map(|_| String::new())
        });
        attempt(&mut got, "extract_symbol", || {
            list.read_in_string(as_symbol_in(&mut symbol))?;
            Ok(symbol.clone())
        });
        attempt(&mut got, "extract_symbol_as_string", || {
            list.read_in_string(as_symbol_in(&mut string))
                .map(|_| String::new())
        });
        attempt(&mut got, "extract_int_from_string", || {
            list.read_i32().map(|v| v.to_string())
        });
        attempt(&mut got, "extract_long_from_string", || {
            list.read_i64().map(|v| v.to_string())
        });
        attempt(&mut got, "extract_float_from_string", || {
            list.read_f32().map(|v| num(f64::from(v)))
        });
        attempt(&mut got, "extract_double_from_string", || {
            list.read_f64().map(num)
        });
        attempt(&mut got, "extract_string", || {
            list.read_in_string(as_string_in(&mut string))?;
            Ok(string.clone())
        });
        attempt(&mut got, "extract_std_string_from_int", || {
            list.read_string()
        });
        attempt(&mut got, "extract_double_from_int", || {
            list.read_f64().map(num)
        });
        attempt(&mut got, "extract_int", || {
            list.read_i32().map(|v| v.to_string())
        });
        attempt(&mut got, "extract_long", || {
            list.read_i64().map(|v| v.to_string())
        });
        attempt(&mut got, "extract_int_from_double", || {
            list.read_i32().map(|v| v.to_string())
        });
        attempt(&mut got, "extract_double", || list.read_f64().map(num));
        attempt(&mut got, "extract_float", || {
            list.read_f32().map(|v| num(f64::from(v)))
        });
        attempt(&mut got, "extract_std_string", || list.read_string());
    }

    // Scan (without int32_t* arguments: doScan() writes a double through those)
    {
        let mut symbol = String::from("none");
        let mut string = String::from("none");
        let mut number = -1.0;
        let mut big = -1i64;

        let n = list.scan(&mut [
            "name".into(),
            "text".into(),
            (&mut number).into(),
            (&mut big).into(),
        ]);
        got.push_str(&format!("scan\t{n}\t{}\t{big}\n", num(number)));

        let n = list.scan(&mut [
            (&mut symbol).into(),
            (&mut string).into(),
            (&mut number).into(),
        ]);
        got.push_str(&format!(
            "scan_strings\t{n}\t{symbol}\t{string}\t{}\n",
            num(number)
        ));

        let n = list.scan(&mut ["name".into(), "other".into(), (&mut number).into()]);
        got.push_str(&format!("scan_mismatch\t{n}\n"));

        let n = list.scan(&mut ["name".into(), (&mut big).into()]);
        got.push_str(&format!("scan_wrong_type\t{n}\n"));

        // a string comparison against a number is skipped, a string target keeps its value
        string = String::from("kept");
        let n = list.scan(&mut [
            "name".into(),
            "text".into(),
            "five".into(),
            (&mut big).into(),
            (&mut string).into(),
        ]);
        got.push_str(&format!("scan_skipped\t{n}\t{big}\t{string}\n"));

        let expected = String::from("name");
        let n = list.scan(&mut [expected.clone().into(), String::from("text").into()]);
        got.push_str(&format!("scan_const_string\t{n}\n"));
    }

    // The accessors on the wrong kind of node
    {
        let mut integer = Sexpr::from(SexprInteger::new(7));
        let dbl = Sexpr::from(SexprDouble::new(2.5));
        let string = Sexpr::from(SexprString::new("s"));
        let symbol = Sexpr::from(SexprSymbol::with_line_number("sym", 12));

        got.push_str(&format!(
            "values\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            integer.long_integer().unwrap(),
            integer.integer().unwrap(),
            num(integer.double().unwrap()),
            num(f64::from(integer.float().unwrap())),
            num(dbl.double().unwrap()),
            num(f64::from(dbl.float().unwrap())),
            string.string().unwrap(),
            symbol.symbol().unwrap(),
            symbol.line_number(),
            integer.line_number()
        ));

        let nothing = |_| String::new();

        attempt(&mut got, "double_as_integer", || {
            dbl.long_integer().map(|_| String::new())
        });
        attempt(&mut got, "double_as_int", || {
            dbl.integer().map(|_| String::new())
        });
        attempt(&mut got, "string_as_double", || {
            string.double().map(|_| String::new())
        });
        attempt(&mut got, "string_as_float", || {
            string.float().map(|_| String::new())
        });
        attempt(&mut got, "symbol_as_string", || {
            symbol.string().map(|_| String::new())
        });
        attempt(&mut got, "string_as_symbol", || {
            string.symbol().map(|_| String::new())
        });
        attempt(&mut got, "symbol_line_as_symbol", || {
            Sexpr::from(SexprInteger::with_line_number(1, 42))
                .symbol()
                .map(|_| String::new())
        });
        attempt(&mut got, "integer_children", || {
            integer.children().map(|_| String::new())
        });
        attempt(&mut got, "integer_child", || {
            integer.child(0).map(|_| String::new())
        });
        attempt(&mut got, "integer_number_of_children", || {
            integer.number_of_children().map(|_| String::new())
        });
        attempt(&mut got, "integer_add_child", || {
            integer.add_child(SexprInteger::new(0)).map(nothing)
        });
        attempt(&mut got, "integer_list", || {
            integer.list().map(|_| String::new())
        });

        let wide = Sexpr::from(SexprInteger::new(4_294_967_301));
        got.push_str(&format!(
            "wide\t{}\t{}\t{}\n",
            wide.long_integer().unwrap(),
            wide.integer().unwrap(),
            wide.as_string(0)
        ));
    }

    assert_same_lines("sexpr_api", &got, &golden("sexpr_api.out"));
}

/// `KICAD_FORMAT::Prettify` in its three modes (`modePrettifyLines`).
#[test]
fn prettify_matches_cpp() {
    let mut got = String::new();

    for line in data_lines(&golden("prettify.txt"), true) {
        let text = unesc(line);

        got.push_str(&format!("> {line}\n"));

        for (name, mode) in [
            ("normal", FormatMode::Normal),
            ("compact", FormatMode::CompactTextProperties),
            ("libtable", FormatMode::LibraryTable),
        ] {
            got.push_str(&format!(
                "{}\t{}\n",
                name,
                esc(&prettify_bytes(&text, mode))
            ));
        }
    }

    assert_same_lines("prettify", &got, &golden("prettify.out"));
}

/// The keyword tables of the shipped token enums against the tables the real CMake generator
/// (`TokenList2DsnLexer.cmake`) produced for the C++ build (`modeKeywords`).
#[test]
fn keyword_tables_match_cmake_generator() {
    let mut got = String::new();

    for set in [
        "pcb_keys_t",
        "pcbplotparams_t",
        "strokeparams_t",
        "embedded_files_t",
        "drcrule_t",
        "lib_table_t",
        "drawingsheet_t",
        "nl_t",
        "tfield_t",
        "dsn",
    ] {
        got.push_str(&format!("# {set}\n"));

        for (token, name) in keyword_table(set).iter().enumerate() {
            got.push_str(&format!("{token}\t{name}\n"));
        }
    }

    assert_same_lines("keywords", &got, &golden("keywords.out"));
}
