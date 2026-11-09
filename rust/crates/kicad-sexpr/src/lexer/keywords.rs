// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2010 Wayne Stambaugh <stambaughw@verizon.net>
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

//! Port of `cmake/BuildSteps/TokenList2DsnLexer.cmake`: keyword tables for [`DsnLexer`].
//!
//! # How C++ gets its token enums
//!
//! For every `*.keywords` file (for example `common/pcb.keywords`) the CMake function
//! `make_lexer()` runs `TokenList2DsnLexer.cmake`, which
//!
//! 1. reads the file line by line, strips `# comments` and surrounding white space and ignores
//!    empty lines;
//! 2. requires every remaining line to match `[a-z][_0-9a-z]*` in full (a lower case letter
//!    followed by lower case letters, digits and underscores), failing the build with
//!    `Invalid token string "..." at line N in file <...>.` otherwise.  Consequently a keyword
//!    can never contain `'-'`, `'+'` or `'.'` nor start with a digit or an upper case letter:
//!    there is no name mangling scheme for such characters, the generator rejects them.  Text
//!    such as `F.Cu`, `+3V3` or `1.6` reaches a parser as `DSN_SYMBOL`/`DSN_NUMBER`/`DSN_STRING`
//!    and is compared as a string;
//! 3. sorts the tokens with CMake's `list( SORT )` (plain byte-wise string order, so digits
//!    sort before `'_'`, which sorts before letters) and fails with
//!    `Duplicate tokens found in file <...>.` if a token occurs twice;
//! 4. emits `namespace <enum> { enum T { T_NONE = DSN_NONE, ..., T_EOF = DSN_EOF, T_<first> = 0,
//!    T_<second>, ... }; }`: the twelve negative syntax tokens of `DSN_SYNTAX_T` followed by one
//!    enumerator `T_<keyword>` per keyword, numbered from 0 in sorted order;
//! 5. emits the table `KEYWORD <NAME>_LEXER::keywords[] = { TOKDEF( <first> ), ... }` (keyword
//!    text and token number, in the same order), a hash map from keyword text to token number
//!    and the class `<NAME>_LEXER : public DSNLEXER` whose `NextTok()` etc. return `<enum>::T`.
//!
//! # The Rust mapping
//!
//! - A token type is anything implementing [`KeywordSet`]: its [`KEYWORDS`](KeywordSet::KEYWORDS)
//!   table is the sorted keyword list (index = token number) and it converts from/to the `int`
//!   token values `DSNLEXER` works with.
//! - The [`keywords!`](crate::keywords!) macro turns a *sorted* list of `T_<keyword> = "<keyword>"`
//!   pairs into such an enum, numbered exactly like step 4, and checks rules 2 and 3 at compile
//!   time.  The enumerators keep their C++ spelling (`T_at`, `T_LEFT`, ...) because the keyword
//!   and syntax tokens of one enum differ only in case (`T_LEFT` is `'('`, `T_left` the keyword
//!   `left`), which an UpperCamelCase conversion would merge.
//! - `<NAME>_LEXER` is the type `DsnLexer<T>`.
//! - [`parse_token_list`] is steps 1-3 and [`generate_keywords_module`] writes the Rust source
//!   of a module holding the `keywords!` invocation for a token list (this is how the modules
//!   below were produced; the test `keyword_modules_are_in_sync` regenerates and compares them).
//!
//! # Provided keyword sets
//!
//! | module | C++ namespace / lexer class | keywords file |
//! |---|---|---|
//! | [`pcb_keys_t`] | `PCB_KEYS_T` / `PCB_LEXER` | `common/pcb.keywords` |
//! | [`pcbplotparams_t`] | `PCBPLOTPARAMS_T` / `PCB_PLOT_PARAMS_LEXER` | `common/pcb_plot_params.keywords` |
//! | [`strokeparams_t`] | `STROKEPARAMS_T` / `STROKE_PARAMS_LEXER` | `common/stroke_params.keywords` |
//! | [`embedded_files_t`] | `EMBEDDED_FILES_T` / `EMBEDDED_FILES_LEXER` | `common/embedded_files.keywords` |
//! | [`drcrule_t`] | `DRCRULE_T` / `DRC_RULES_LEXER` | `common/drc_rules.keywords` |
//! | [`lib_table_t`] | `LIB_TABLE_T` / `LIB_TABLE_LEXER` | `common/lib_table.keywords` |
//! | [`drawingsheet_t`] | `DRAWINGSHEET_T` / `DRAWING_SHEET_LEXER` | `common/drawing_sheet/drawing_sheet.keywords` |
//! | [`nl_t`] | `NL_T` / `NETLIST_LEXER` | `common/netlist.keywords` |
//! | [`tfield_t`] | `TFIELD_T` / `TEMPLATE_FIELDNAMES_LEXER` | `common/template_fieldnames.keywords` |
//! | [`dsn`] | `DSN` / `SPECCTRA_LEXER` | `pcbnew/specctra_import_export/specctra.keywords` |
//!
//! [`DsnLexer`]: crate::lexer::dsnlexer::DsnLexer

use std::fmt;

use crate::lexer::dsnlexer::{
    DSN_BAR, DSN_COMMENT, DSN_DASH, DSN_EOF, DSN_LEFT, DSN_NONE, DSN_NUMBER, DSN_QUOTE_DEF,
    DSN_RIGHT, DSN_STRING, DSN_STRING_QUOTE, DSN_SYMBOL,
};

pub mod drawingsheet_t;
pub mod drcrule_t;
pub mod dsn;
pub mod embedded_files_t;
pub mod lib_table_t;
pub mod nl_t;
pub mod pcb_keys_t;
pub mod pcbplotparams_t;
pub mod strokeparams_t;
pub mod tfield_t;

/// A token type for [`DsnLexer`](crate::lexer::dsnlexer::DsnLexer): the Rust counterpart of a
/// generated C++ `namespace <enum> { enum T { ... }; }` together with its keyword table.
///
/// Token values are the C++ `int` values: negative for the syntax tokens of
/// [`DsnSyntaxT`], and the index into [`KEYWORDS`](Self::KEYWORDS) for keywords.
pub trait KeywordSet: Copy + Eq + fmt::Debug + 'static {
    /// The keyword table (C++ `<NAME>_LEXER::keywords[]`): keyword `KEYWORDS[n]` is token `n`.
    ///
    /// Must be sorted in byte-wise ascending order without duplicates, as the CMake generator
    /// guarantees for the C++ tables.  The [`keywords!`](crate::keywords!) macro checks this at
    /// compile time.
    const KEYWORDS: &'static [&'static str];

    /// Convert a C++ `int` token value (the C++ cast `(T) tok`).  Values that are neither a
    /// syntax token nor a keyword index must map to the "none" token.
    fn from_i32(tok: i32) -> Self;

    /// The C++ `int` value of this token.
    fn to_i32(self) -> i32;
}

/// Raw `int` tokens, for code that works with the plain `DSNLEXER` interface: negative values
/// are the `DSN_*` constants, values `>= 0` index the keyword table the lexer was constructed
/// with (see `DsnLexer::<i32>::with_keywords`).
impl KeywordSet for i32 {
    const KEYWORDS: &'static [&'static str] = &[];

    fn from_i32(tok: i32) -> Self {
        tok
    }

    fn to_i32(self) -> i32 {
        self
    }
}

/// Port of `enum DSN_SYNTAX_T`: all the DSN lexer's tokens that are supported in lexing.  It is
/// up to the parser if it wants also to support them.
///
/// This is also the token type of a lexer without keyword table (C++
/// `DSNLEXER( const std::string& aSExpression, const wxString& aSource )`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(i32)]
pub enum DsnSyntaxT {
    /// `DSN_NONE`
    None = DSN_NONE,
    /// `DSN_BAR`, also called pipe `'|'`
    Bar = DSN_BAR,
    /// `DSN_COMMENT`
    Comment = DSN_COMMENT,
    /// `DSN_STRING_QUOTE`
    StringQuote = DSN_STRING_QUOTE,
    /// `DSN_QUOTE_DEF`
    QuoteDef = DSN_QUOTE_DEF,
    /// `DSN_DASH`
    Dash = DSN_DASH,
    /// `DSN_SYMBOL`
    Symbol = DSN_SYMBOL,
    /// `DSN_NUMBER`
    Number = DSN_NUMBER,
    /// `DSN_RIGHT`: right bracket, `')'`
    Right = DSN_RIGHT,
    /// `DSN_LEFT`: left bracket, `'('`
    Left = DSN_LEFT,
    /// `DSN_STRING`: a quoted string, stripped of the quotes
    String = DSN_STRING,
    /// `DSN_EOF`: special case for end of file
    Eof = DSN_EOF,
}

impl KeywordSet for DsnSyntaxT {
    const KEYWORDS: &'static [&'static str] = &[];

    fn from_i32(tok: i32) -> Self {
        match tok {
            DSN_BAR => DsnSyntaxT::Bar,
            DSN_COMMENT => DsnSyntaxT::Comment,
            DSN_STRING_QUOTE => DsnSyntaxT::StringQuote,
            DSN_QUOTE_DEF => DsnSyntaxT::QuoteDef,
            DSN_DASH => DsnSyntaxT::Dash,
            DSN_SYMBOL => DsnSyntaxT::Symbol,
            DSN_NUMBER => DsnSyntaxT::Number,
            DSN_RIGHT => DsnSyntaxT::Right,
            DSN_LEFT => DsnSyntaxT::Left,
            DSN_STRING => DsnSyntaxT::String,
            DSN_EOF => DsnSyntaxT::Eof,
            _ => DsnSyntaxT::None,
        }
    }

    fn to_i32(self) -> i32 {
        self as i32
    }
}

/// True if `token` matches the generator's validity rule `[a-z][_0-9a-z]*` in full.
pub const fn is_valid_keyword(token: &str) -> bool {
    let bytes = token.as_bytes();

    if bytes.is_empty() || !bytes[0].is_ascii_lowercase() {
        return false;
    }

    let mut i = 1;

    while i < bytes.len() {
        let c = bytes[i];

        if !(c == b'_' || c.is_ascii_digit() || c.is_ascii_lowercase()) {
            return false;
        }

        i += 1;
    }

    true
}

/// Byte-wise `a < b`, usable in constant evaluation.
const fn str_less(a: &str, b: &str) -> bool {
    let a = a.as_bytes();
    let b = b.as_bytes();
    let mut i = 0;

    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return a[i] < b[i];
        }

        i += 1;
    }

    a.len() < b.len()
}

/// True if `table` is sorted in strictly ascending byte-wise order (sorted, no duplicates),
/// i.e. laid out the way the CMake generator numbers the tokens.
pub const fn is_sorted_keyword_table(table: &[&str]) -> bool {
    let mut i = 1;

    while i < table.len() {
        if !str_less(table[i - 1], table[i]) {
            return false;
        }

        i += 1;
    }

    true
}

/// Compile-time check used by the [`keywords!`](crate::keywords!) macro: every keyword is valid
/// and the table is sorted without duplicates, so the token numbering equals that of
/// `TokenList2DsnLexer.cmake`.
pub const fn assert_valid_keyword_table(table: &[&str]) {
    let mut i = 0;

    while i < table.len() {
        assert!(
            is_valid_keyword(table[i]),
            "keywords!: invalid token string, keywords must match [a-z][_0-9a-z]*"
        );

        i += 1;
    }

    assert!(
        is_sorted_keyword_table(table),
        "keywords!: the keyword list must be sorted byte-wise and free of duplicates"
    );
}

/// Define a token enum with its keyword table, the Rust counterpart of what
/// `TokenList2DsnLexer.cmake` generates from a `*.keywords` file.
///
/// ```
/// kicad_sexpr::keywords! {
///     /// The tokens of my format.
///     pub enum T {
///         T_at = "at",
///         T_layer = "layer",
///         T_xy = "xy",
///     }
/// }
///
/// use kicad_sexpr::{DsnLexer, KeywordSet};
///
/// assert_eq!(T::T_LEFT as i32, -3);   // the DSN_SYNTAX_T tokens come first ...
/// assert_eq!(T::T_at as i32, 0);      // ... and the keywords are numbered from 0.
/// assert_eq!(T::KEYWORDS, ["at", "layer", "xy"]);
///
/// let mut lexer = DsnLexer::<T>::new("(at 1 2)", "example");
/// assert_eq!(lexer.next_tok().unwrap(), T::T_LEFT);
/// assert_eq!(lexer.next_tok().unwrap(), T::T_at);
/// ```
///
/// The generated enum is `#[repr(i32)]` and starts with the twelve syntax tokens `T_NONE`,
/// `T_BAR`, `T_COMMENT`, `T_STRING_QUOTE`, `T_QUOTE_DEF`, `T_DASH`, `T_SYMBOL`, `T_NUMBER`,
/// `T_RIGHT`, `T_LEFT`, `T_STRING` and `T_EOF` (values `DSN_NONE`..`DSN_EOF`), followed by the
/// listed enumerators numbered from 0.  It implements [`KeywordSet`](crate::KeywordSet) and has
/// a `token_name()` method (the generated C++ `<NAME>_LEXER::TokenName`).
///
/// The list must be sorted byte-wise without duplicates and every keyword must match
/// `[a-z][_0-9a-z]*`; this is verified at compile time.  Use
/// [`parse_token_list`](crate::lexer::keywords::parse_token_list) and
/// [`generate_keywords_module`](crate::lexer::keywords::generate_keywords_module) to produce
/// the list from a `*.keywords` file.
#[macro_export]
macro_rules! keywords {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $keyword:literal ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        #[repr(i32)]
        $vis enum $name {
            // these first few are negative special ones for syntax, and are
            // inherited from DSNLEXER.
            /// `DSN_NONE`
            T_NONE = $crate::lexer::dsnlexer::DSN_NONE,
            /// `DSN_BAR`, also called pipe: `'|'`
            T_BAR = $crate::lexer::dsnlexer::DSN_BAR,
            /// `DSN_COMMENT`
            T_COMMENT = $crate::lexer::dsnlexer::DSN_COMMENT,
            /// `DSN_STRING_QUOTE`
            T_STRING_QUOTE = $crate::lexer::dsnlexer::DSN_STRING_QUOTE,
            /// `DSN_QUOTE_DEF`
            T_QUOTE_DEF = $crate::lexer::dsnlexer::DSN_QUOTE_DEF,
            /// `DSN_DASH`
            T_DASH = $crate::lexer::dsnlexer::DSN_DASH,
            /// `DSN_SYMBOL`
            T_SYMBOL = $crate::lexer::dsnlexer::DSN_SYMBOL,
            /// `DSN_NUMBER`
            T_NUMBER = $crate::lexer::dsnlexer::DSN_NUMBER,
            /// `DSN_RIGHT`, right bracket: `')'`
            T_RIGHT = $crate::lexer::dsnlexer::DSN_RIGHT,
            /// `DSN_LEFT`, left bracket: `'('`
            T_LEFT = $crate::lexer::dsnlexer::DSN_LEFT,
            /// `DSN_STRING`, a quoted string, stripped of the quotes
            T_STRING = $crate::lexer::dsnlexer::DSN_STRING,
            /// `DSN_EOF`, special case for end of file
            T_EOF = $crate::lexer::dsnlexer::DSN_EOF,
            $( $(#[$vmeta])* $variant, )*
        }

        impl $name {
            /// Port of the generated `<NAME>_LEXER::TokenName`: the name of the token in ASCII
            /// form.
            #[allow(dead_code)]
            pub fn token_name(self) -> &'static str {
                let tok = self as i32;

                if tok < 0 {
                    $crate::lexer::dsnlexer::syntax(tok)
                } else {
                    match <$name as $crate::lexer::keywords::KeywordSet>::KEYWORDS
                        .get(tok as usize)
                    {
                        Some(name) => name,
                        None => "token too big",
                    }
                }
            }
        }

        impl $crate::lexer::keywords::KeywordSet for $name {
            const KEYWORDS: &'static [&'static str] = &[ $( $keyword ),* ];

            fn from_i32(tok: i32) -> Self {
                const ALL: &[$name] = &[ $( $name::$variant ),* ];

                match tok {
                    $crate::lexer::dsnlexer::DSN_BAR => $name::T_BAR,
                    $crate::lexer::dsnlexer::DSN_COMMENT => $name::T_COMMENT,
                    $crate::lexer::dsnlexer::DSN_STRING_QUOTE => $name::T_STRING_QUOTE,
                    $crate::lexer::dsnlexer::DSN_QUOTE_DEF => $name::T_QUOTE_DEF,
                    $crate::lexer::dsnlexer::DSN_DASH => $name::T_DASH,
                    $crate::lexer::dsnlexer::DSN_SYMBOL => $name::T_SYMBOL,
                    $crate::lexer::dsnlexer::DSN_NUMBER => $name::T_NUMBER,
                    $crate::lexer::dsnlexer::DSN_RIGHT => $name::T_RIGHT,
                    $crate::lexer::dsnlexer::DSN_LEFT => $name::T_LEFT,
                    $crate::lexer::dsnlexer::DSN_STRING => $name::T_STRING,
                    $crate::lexer::dsnlexer::DSN_EOF => $name::T_EOF,
                    _ => {
                        if tok >= 0 {
                            match ALL.get(tok as usize) {
                                Some(token) => *token,
                                None => $name::T_NONE,
                            }
                        } else {
                            $name::T_NONE
                        }
                    }
                }
            }

            fn to_i32(self) -> i32 {
                self as i32
            }
        }

        const _: () = $crate::lexer::keywords::assert_valid_keyword_table(
            <$name as $crate::lexer::keywords::KeywordSet>::KEYWORDS,
        );
    };
}

/// Why a `*.keywords` token list was rejected; the two fatal errors of
/// `TokenList2DsnLexer.cmake`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenListError {
    /// A line is not a valid token (`[a-z][_0-9a-z]*`).
    InvalidToken {
        /// The offending line with its comment removed (C++ CMake `tmpToken`).
        token: String,
        /// Number of the line among the non-empty lines of the file, starting at 1.
        line: usize,
        /// The name of the token list file.
        input_file: String,
    },
    /// A token occurs more than once.
    DuplicateTokens {
        /// The name of the token list file.
        input_file: String,
    },
}

impl fmt::Display for TokenListError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TokenListError::InvalidToken {
                token,
                line,
                input_file,
            } => write!(
                f,
                "TokenList2DsnLexer.cmake failure: Invalid token string \"{token}\" at line \
                 {line} in file <{input_file}>."
            ),
            TokenListError::DuplicateTokens { input_file } => write!(
                f,
                "TokenList2DsnLexer.cmake failure: Duplicate tokens found in file <{input_file}>."
            ),
        }
    }
}

impl std::error::Error for TokenListError {}

/// Port of the token list processing of `TokenList2DsnLexer.cmake`: read the text of a
/// `*.keywords` file and return its tokens validated, sorted and checked for duplicates.
/// The index of a token in the result is its token number (`T_<token>`).
///
/// The token list file format requires a single token per line.  `#` starts a comment that
/// runs to the end of the line; empty lines are ignored.
///
/// CMake reads the file with `file( STRINGS )` into a list, so empty lines are not counted in
/// the line number of the error message; that is reproduced.  CMake list splitting of lines
/// containing `';'` is not.
///
/// # Errors
///
/// See [`TokenListError`].
pub fn parse_token_list(text: &str, input_file: &str) -> Result<Vec<String>, TokenListError> {
    let mut tokens: Vec<String> = Vec::new();
    let mut line_count = 0;

    for line in text.split('\n') {
        // file( STRINGS ) ignores carriage returns and does not return empty lines.
        let line = line.trim_end_matches('\r');

        if line.is_empty() {
            continue;
        }

        line_count += 1;

        // strip any comment from # to end of line
        let tmp_token = match line.find('#') {
            Some(hash) => &line[..hash],
            None => line,
        };
        let token = tmp_token.trim();

        // Ignore empty lines.
        if !token.is_empty() {
            // Make sure token is valid.
            if is_valid_keyword(token) {
                tokens.push(token.to_string());
            } else {
                return Err(TokenListError::InvalidToken {
                    token: tmp_token.to_string(),
                    line: line_count,
                    input_file: input_file.to_string(),
                });
            }
        }
    }

    tokens.sort();

    // Check for duplicates.
    let tokens_before = tokens.len();
    tokens.dedup();

    if tokens_before != tokens.len() {
        return Err(TokenListError::DuplicateTokens {
            input_file: input_file.to_string(),
        });
    }

    Ok(tokens)
}

/// The `Copyright` comment lines of a `*.keywords` file (without the leading `#`), to be
/// carried over into the generated module.
pub fn token_list_copyright_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix('#'))
        .map(str::trim)
        .filter(|line| line.starts_with("Copyright"))
        .map(str::to_string)
        .collect()
}

/// `PCB_PLOT_PARAMS` -> `PcbPlotParams`.
fn upper_camel_case(name: &str) -> String {
    let mut out = String::with_capacity(name.len());

    for word in name.split('_').filter(|word| !word.is_empty()) {
        let mut chars = word.chars();

        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(&chars.as_str().to_lowercase());
        }
    }

    out
}

/// What [`generate_keywords_module`] needs to know besides the tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeywordsModuleSpec<'a> {
    /// The C++ namespace the enum `T` lives in (the `enum` parameter of `make_lexer()`), e.g.
    /// `PCB_KEYS_T`.
    pub enum_namespace: &'a str,
    /// Path of the token list file relative to the KiCad source root, e.g.
    /// `common/pcb.keywords`.  Its base name decides the name of the lexer class
    /// (`pcb.keywords` -> `PCB_LEXER` -> `PcbLexer`), as in the CMake script.
    pub input_file: &'a str,
    /// Path used to reach this crate from the generated code: `crate` inside `kicad-sexpr`,
    /// `kicad_sexpr` (or `::kicad_sexpr`) elsewhere.
    pub crate_path: &'a str,
    /// Extra copyright lines for the licence header, see [`token_list_copyright_lines`].
    pub copyright_lines: &'a [String],
}

/// Write the Rust source of a module that defines `enum T` (via [`keywords!`](crate::keywords!))
/// and the lexer type alias for a token list, the counterpart of the `<name>_lexer.h` /
/// `<name>_keywords.cpp` pair `TokenList2DsnLexer.cmake` writes.
///
/// `tokens` must be the result of [`parse_token_list`].
pub fn generate_keywords_module(spec: &KeywordsModuleSpec<'_>, tokens: &[String]) -> String {
    use std::fmt::Write as _;

    // the keywords filename without extension is important, it sets the classname into RESULT
    let file_name = spec
        .input_file
        .rsplit('/')
        .next()
        .unwrap_or(spec.input_file);
    let result = file_name.split('.').next().unwrap_or(file_name);
    let lexer_class = format!("{}_LEXER", result.to_uppercase());
    let lexer_alias = upper_camel_case(&lexer_class);

    let mut out = String::new();

    out.push_str(
        "// This program source code file is part of KiCad, a free EDA CAD application.\n//\n",
    );

    for line in spec.copyright_lines {
        // The KiCad Developers line is always written below.
        if !line.contains("KiCad Developers") && !line.contains("Kicad Developers") {
            let _ = writeln!(out, "// {line}");
        }
    }

    out.push_str(
        "// Copyright The KiCad Developers, see AUTHORS.txt for contributors.\n\
         //\n\
         // This program is free software: you can redistribute it and/or modify it\n\
         // under the terms of the GNU General Public License as published by the\n\
         // Free Software Foundation, either version 3 of the License, or (at your\n\
         // option) any later version.\n\
         //\n\
         // This program is distributed in the hope that it will be useful, but\n\
         // WITHOUT ANY WARRANTY; without even the implied warranty of\n\
         // MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU\n\
         // General Public License for more details.\n\
         //\n\
         // You should have received a copy of the GNU General Public License along\n\
         // with this program.  If not, see <http://www.gnu.org/licenses/>.\n\n",
    );

    let _ = writeln!(
        out,
        "//! Port of the generated `{result}_lexer.h` / `{result}_keywords.cpp`: the token enum\n\
         //! `{ns}::T` and the lexer `{lexer_class}`, made from `{input}`.\n\
         //!\n\
         //! Do not modify this file, it was automatically generated by\n\
         //! `kicad_sexpr::lexer::keywords::generate_keywords_module` (the port of the\n\
         //! TokenList2DsnLexer CMake script).  To regenerate after a change of the keywords file\n\
         //! run `KICAD_SEXPR_REGENERATE_KEYWORDS=1 cargo test -p kicad-sexpr --test keyword_modules`.\n",
        ns = spec.enum_namespace,
        input = spec.input_file,
    );

    let _ = writeln!(out, "{}::keywords! {{", spec.crate_path);
    let _ = writeln!(
        out,
        "    /// Port of `{ns}::T`: enum `T` contains all this lexer's tokens ({count} keywords of\n    \
         /// `{input}` after the syntax tokens inherited from `DSNLEXER`).",
        ns = spec.enum_namespace,
        count = tokens.len(),
        input = spec.input_file,
    );
    out.push_str("    pub enum T {\n");

    for token in tokens {
        let _ = writeln!(out, "        T_{token} = \"{token}\",");
    }

    out.push_str("    }\n}\n\n");

    let _ = writeln!(
        out,
        "/// Port of `{lexer_class}`: an automatically generated lexer class, based on the keywords\n\
         /// provided by `{input}`.\n\
         pub type {lexer_alias} = {krate}::lexer::dsnlexer::DsnLexer<T>;",
        input = spec.input_file,
        krate = spec.crate_path,
    );

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    crate::keywords! {
        /// Test tokens.
        enum TestT {
            T_arrow1a = "arrow1a",
            T_arrow_open = "arrow_open",
            T_at = "at",
            T_left = "left",
            T_xy = "xy",
        }
    }

    crate::keywords! {
        enum EmptyT {}
    }

    #[test]
    fn macro_numbers_tokens_like_cmake() {
        assert_eq!(TestT::T_NONE as i32, -12);
        assert_eq!(TestT::T_BAR as i32, -11);
        assert_eq!(TestT::T_COMMENT as i32, -10);
        assert_eq!(TestT::T_STRING_QUOTE as i32, -9);
        assert_eq!(TestT::T_QUOTE_DEF as i32, -8);
        assert_eq!(TestT::T_DASH as i32, -7);
        assert_eq!(TestT::T_SYMBOL as i32, -6);
        assert_eq!(TestT::T_NUMBER as i32, -5);
        assert_eq!(TestT::T_RIGHT as i32, -4);
        assert_eq!(TestT::T_LEFT as i32, -3);
        assert_eq!(TestT::T_STRING as i32, -2);
        assert_eq!(TestT::T_EOF as i32, -1);
        assert_eq!(TestT::T_arrow1a as i32, 0);
        assert_eq!(TestT::T_arrow_open as i32, 1);
        assert_eq!(TestT::T_at as i32, 2);
        assert_eq!(TestT::T_left as i32, 3);
        assert_eq!(TestT::T_xy as i32, 4);

        assert_eq!(
            TestT::KEYWORDS,
            ["arrow1a", "arrow_open", "at", "left", "xy"]
        );
        assert!(EmptyT::KEYWORDS.is_empty());
    }

    #[test]
    fn macro_conversions_round_trip() {
        for tok in -12..5 {
            assert_eq!(TestT::from_i32(tok).to_i32(), tok);
        }

        assert_eq!(TestT::from_i32(5), TestT::T_NONE);
        assert_eq!(TestT::from_i32(-13), TestT::T_NONE);
        assert_eq!(TestT::from_i32(i32::MAX), TestT::T_NONE);
        assert_eq!(EmptyT::from_i32(0), EmptyT::T_NONE);
        assert_eq!(EmptyT::from_i32(-3), EmptyT::T_LEFT);
    }

    #[test]
    fn macro_token_name() {
        assert_eq!(TestT::T_at.token_name(), "at");
        assert_eq!(TestT::T_left.token_name(), "left");
        assert_eq!(TestT::T_LEFT.token_name(), "(");
        assert_eq!(TestT::T_EOF.token_name(), "end of input");
        assert_eq!(TestT::T_COMMENT.token_name(), "???");
    }

    #[test]
    fn dsn_syntax_t_values() {
        assert_eq!(DsnSyntaxT::None as i32, -12);
        assert_eq!(DsnSyntaxT::Bar as i32, -11);
        assert_eq!(DsnSyntaxT::Comment as i32, -10);
        assert_eq!(DsnSyntaxT::StringQuote as i32, -9);
        assert_eq!(DsnSyntaxT::QuoteDef as i32, -8);
        assert_eq!(DsnSyntaxT::Dash as i32, -7);
        assert_eq!(DsnSyntaxT::Symbol as i32, -6);
        assert_eq!(DsnSyntaxT::Number as i32, -5);
        assert_eq!(DsnSyntaxT::Right as i32, -4);
        assert_eq!(DsnSyntaxT::Left as i32, -3);
        assert_eq!(DsnSyntaxT::String as i32, -2);
        assert_eq!(DsnSyntaxT::Eof as i32, -1);

        for tok in -12..0 {
            assert_eq!(DsnSyntaxT::from_i32(tok).to_i32(), tok);
        }

        assert_eq!(DsnSyntaxT::from_i32(0), DsnSyntaxT::None);
        assert_eq!(DsnSyntaxT::from_i32(-100), DsnSyntaxT::None);
    }

    #[test]
    fn keyword_validity() {
        // Valid tokens:    a a1 foo_1 foo_bar2
        for token in ["a", "a1", "foo_1", "foo_bar2", "bar_"] {
            assert!(is_valid_keyword(token), "{token}");
        }

        // Invalid tokens:  1 A _foo foO, and anything with '-', '+', '.' or spaces.
        for token in [
            "", "1", "A", "_foo", "foO", "f.cu", "a-b", "a+b", "1v8", "a b", "é",
        ] {
            assert!(!is_valid_keyword(token), "{token}");
        }
    }

    #[test]
    fn sorted_table_check() {
        assert!(is_sorted_keyword_table(&[]));
        assert!(is_sorted_keyword_table(&["a"]));
        assert!(is_sorted_keyword_table(&["a", "a1", "a_", "aa", "b"]));
        assert!(!is_sorted_keyword_table(&["a", "a"]));
        assert!(!is_sorted_keyword_table(&["b", "a"]));
        assert!(!is_sorted_keyword_table(&["a_", "a1"]));
    }

    #[test]
    fn token_list_is_stripped_sorted_and_numbered() {
        let text = "#\n# Copyright (C) 2012 CERN.\n#\n\nxy\n  at   # position\n\tarrow_open\n\
                    arrow1a\r\nleft\n\n";
        let tokens = parse_token_list(text, "test.keywords").unwrap();

        // '1' < '_' < letters
        assert_eq!(tokens, ["arrow1a", "arrow_open", "at", "left", "xy"]);
        assert_eq!(
            token_list_copyright_lines(text),
            ["Copyright (C) 2012 CERN."]
        );
    }

    #[test]
    fn token_list_errors() {
        let err = parse_token_list("at\n\n# c\nF.Cu # layer\n", "bad.keywords").unwrap_err();
        assert_eq!(
            err,
            TokenListError::InvalidToken {
                token: "F.Cu ".to_string(),
                line: 3,
                input_file: "bad.keywords".to_string()
            }
        );
        assert_eq!(
            err.to_string(),
            "TokenList2DsnLexer.cmake failure: Invalid token string \"F.Cu \" at line 3 in file \
             <bad.keywords>."
        );

        for bad in ["1v8", "a-b", "+3v3", "Abc", "_x", "a b"] {
            assert!(matches!(
                parse_token_list(bad, "f"),
                Err(TokenListError::InvalidToken { .. })
            ));
        }

        let err = parse_token_list("at\nxy\nat\n", "dup.keywords").unwrap_err();
        assert_eq!(
            err.to_string(),
            "TokenList2DsnLexer.cmake failure: Duplicate tokens found in file <dup.keywords>."
        );
    }

    #[test]
    fn lexer_class_names() {
        assert_eq!(upper_camel_case("PCB_LEXER"), "PcbLexer");
        assert_eq!(
            upper_camel_case("PCB_PLOT_PARAMS_LEXER"),
            "PcbPlotParamsLexer"
        );
        assert_eq!(upper_camel_case("SPECCTRA_LEXER"), "SpecctraLexer");
    }

    #[test]
    fn generated_module_layout() {
        let tokens = vec!["at".to_string(), "xy".to_string()];
        let copyright = vec![
            "Copyright (C) 2012 CERN.".to_string(),
            "Copyright (C) 2019-2023 KiCad Developers, see AUTHORS.txt for contributors."
                .to_string(),
        ];
        let source = generate_keywords_module(
            &KeywordsModuleSpec {
                enum_namespace: "PCB_KEYS_T",
                input_file: "common/pcb.keywords",
                crate_path: "crate",
                copyright_lines: &copyright,
            },
            &tokens,
        );

        assert!(source.starts_with(
            "// This program source code file is part of KiCad, a free EDA CAD application.\n\
             //\n\
             // Copyright (C) 2012 CERN.\n\
             // Copyright The KiCad Developers, see AUTHORS.txt for contributors.\n"
        ));
        assert!(source.contains(
            "crate::keywords! {\n    /// Port of `PCB_KEYS_T::T`: enum `T` contains all this \
             lexer's tokens (2 keywords of\n    /// `common/pcb.keywords` after the syntax \
             tokens inherited from `DSNLEXER`).\n    pub enum T {\n        T_at = \"at\",\n        \
             T_xy = \"xy\",\n    }\n}\n"
        ));
        assert!(source.ends_with("pub type PcbLexer = crate::lexer::dsnlexer::DsnLexer<T>;\n"));
    }
}
