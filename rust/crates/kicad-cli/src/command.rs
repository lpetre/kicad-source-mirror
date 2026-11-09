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

//! Port of `kicad/cli/command.h` and `kicad/cli/command.cpp` (`CLI::COMMAND`): the common
//! arguments (`addCommonArgs`, `addDrawingSheetArg`, `addDefineArg`, `addVariantsArg`) as clap
//! argument builders, and the common part of `COMMAND::Perform` ([`CommonArgs::from_matches`]).

use std::collections::BTreeMap;

use clap::{Arg, ArgAction, ArgMatches};
use kicad_session::exit_codes;

use crate::argparse_compat;

/// `ARG_INPUT`
pub const ARG_INPUT: &str = "input";
/// `ARG_OUTPUT`
pub const ARG_OUTPUT: &str = "output";
/// `ARG_DRAWING_SHEET`
pub const ARG_DRAWING_SHEET: &str = "drawing-sheet";
/// `ARG_DEFINE_VAR_LONG`
pub const ARG_DEFINE_VAR_LONG: &str = "define-var";
/// `ARG_VARIANT`
pub const ARG_VARIANT: &str = "variant";

/// `COMMAND::IO_TYPE`
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IoType {
    /// `FILE`
    File,
    /// `DIRECTORY`
    Directory,
    /// `FILE_OR_DIRECTORY`
    FileOrDirectory,
}

/// An argparse `.flag()` argument (`--name`, default false).
pub fn flag(long: &'static str, help: &'static str) -> Arg {
    Arg::new(long).long(long).help(help).action(ArgAction::SetTrue)
}

/// An argparse `.flag()` argument with an extra long spelling (argparse's second name, e.g.
/// `--erd` for `--exclude-refdes`).
pub fn flag2(alias: &'static str, long: &'static str, help: &'static str) -> Arg {
    flag(long, help).visible_alias(alias)
}

/// An argparse string option with `.default_value( d )`: argparse gives such options
/// `nargs = 0..1`, so a bare `--name` keeps the default.
pub fn string_opt(long: &'static str, default: &'static str, help: &'static str) -> Arg {
    Arg::new(long)
        .long(long)
        .help(help)
        .num_args(0..=1)
        .default_value(default)
        .default_missing_value(default)
        .action(ArgAction::Set)
}

/// An argparse `.scan<'i', int>().default_value( d )` option.
pub fn int_opt(long: &'static str, default: &'static str, help: &'static str) -> Arg {
    string_opt(long, default, help).value_parser(argparse_compat::scan_int)
}

/// An argparse `.scan<'g', double>().default_value( d )` option.
pub fn double_opt(long: &'static str, default: &'static str, help: &'static str) -> Arg {
    string_opt(long, default, help).value_parser(argparse_compat::scan_double)
}

/// `COMMAND::addCommonArgs` input half.
pub fn input_arg(io: IoType) -> Arg {
    let (help, metavar) = match io {
        IoType::File => ("Input file", "INPUT_FILE"),
        IoType::Directory => ("Input directory", "INPUT_DIR"),
        IoType::FileOrDirectory => ("Input file or directory", "INPUT_FILE_OR_DIR"),
    };

    Arg::new(ARG_INPUT).help(help).value_name(metavar).required(true)
}

/// `COMMAND::addCommonArgs` output half: `-o, --output`, default `""`.
pub fn output_arg(io: IoType) -> Arg {
    let (help, metavar) = match io {
        IoType::File => ("Output file", "OUTPUT_FILE"),
        IoType::Directory => ("Output directory", "OUTPUT_DIR"),
        IoType::FileOrDirectory => ("Output file or directory", "OUTPUT_FILE_OR_DIR"),
    };

    string_opt(ARG_OUTPUT, "", help).short('o').value_name(metavar)
}

/// `COMMAND::addDrawingSheetArg`.
pub fn drawing_sheet_arg() -> Arg {
    string_opt(
        ARG_DRAWING_SHEET,
        "",
        "Path to drawing sheet, this overrides any existing project defined sheet when used",
    )
    .value_name("SHEET_PATH")
}

/// `COMMAND::addDefineArg`.
pub fn define_arg() -> Arg {
    Arg::new(ARG_DEFINE_VAR_LONG)
        .long(ARG_DEFINE_VAR_LONG)
        .short('D')
        .help(
            "Overrides or adds project variables, can be used multiple times to declare \
             multiple variables.\nUse in the format of '--define-var key=value' or '-D key=value'",
        )
        .value_name("KEY=VALUE")
        .action(ArgAction::Append)
        .num_args(1)
}

/// `COMMAND::addVariantsArg`.
pub fn variants_arg() -> Arg {
    Arg::new(ARG_VARIANT)
        .long(ARG_VARIANT)
        .help(
            "The variant name(s) to output, can be used multiple times to specify multiple \
             variants.\nWhen specifying multiple variants, use ${VARIANT} in the output path \
             to generate separate files for each variant.\nWhen no --variant argument is \
             provided, the default variant is output.",
        )
        .value_name("VAR")
        .action(ArgAction::Append)
        .num_args(1)
}

/// Port of `wxStringSplit( aText, aStrings, aSplitter )` (`common/string_utils.cpp`): a
/// trailing empty part is dropped.
pub fn wx_string_split(text: &str, splitter: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut tmp = String::new();

    for c in text.chars() {
        if c == splitter {
            out.push(std::mem::take(&mut tmp));
        } else {
            tmp.push(c);
        }
    }

    if !tmp.is_empty() {
        out.push(tmp);
    }

    out
}

/// The values `COMMAND::Perform` extracts before `doPerform` (`m_argInput`, `m_argOutput`,
/// `m_argDrawingSheet`, `m_argDefineVars`, `m_argVariantNames`).
#[derive(Clone, Debug, Default)]
pub struct CommonArgs {
    /// `m_argInput`
    pub input: String,
    /// `m_argOutput`
    pub output: String,
    /// `m_argDrawingSheet`
    pub drawing_sheet: String,
    /// `m_argDefineVars`
    pub define_vars: BTreeMap<String, String>,
    /// `m_argVariantNames`
    pub variant_names: Vec<String>,
}

/// Which common arguments a command registered (`m_hasInputArg`, ...).
#[derive(Clone, Copy, Debug, Default)]
pub struct CommonArgSet {
    /// `m_hasInputArg`
    pub input: bool,
    /// `m_hasOutputArg`
    pub output: bool,
    /// `m_hasDrawingSheetArg`
    pub drawing_sheet: bool,
    /// `m_hasDefineArg`
    pub define: bool,
    /// `m_hasVariantArg` (with `m_variantsRequireOutputToken` = true)
    pub variant: bool,
}

impl CommonArgs {
    /// Port of the argument part of `CLI::COMMAND::Perform`. `Err( exit code )` on a malformed
    /// `-D` (silently, like the C++) or several variants without `${VARIANT}` in the output.
    pub fn from_matches(m: &ArgMatches, set: CommonArgSet) -> Result<CommonArgs, i32> {
        let mut args = CommonArgs::default();
        let get = |name: &str| m.get_one::<String>(name).cloned().unwrap_or_default();

        if set.input {
            args.input = get(ARG_INPUT);
        }

        if set.output {
            args.output = get(ARG_OUTPUT);
        }

        if set.drawing_sheet {
            args.drawing_sheet = get(ARG_DRAWING_SHEET);
        }

        if set.define {
            if let Some(defines) = m.get_many::<String>(ARG_DEFINE_VAR_LONG) {
                for def in defines {
                    let bits = wx_string_split(def, '=');

                    if bits.len() == 2 {
                        args.define_vars.insert(bits[0].clone(), bits[1].clone());
                    } else {
                        return Err(exit_codes::ERR_ARGS);
                    }
                }
            }
        }

        if set.variant {
            if let Some(names) = m.get_many::<String>(ARG_VARIANT) {
                args.variant_names = names.cloned().collect();

                if args.variant_names.len() > 1
                    && set.output
                    && !args.output.is_empty()
                    && !args.output.contains("${VARIANT}")
                {
                    eprint!(
                        "When specifying multiple variants, the output path must contain \
                         ${{VARIANT}} to generate separate output files for each variant.\n"
                    );
                    return Err(exit_codes::ERR_ARGS);
                }
            }
        }

        Ok(args)
    }
}

/// `m_argParser.get<bool>( name )` for a flag.
pub fn get_flag(m: &ArgMatches, name: &str) -> bool {
    m.get_flag(name)
}

/// `m_argParser.get<std::string>( name )`.
pub fn get_string(m: &ArgMatches, name: &str) -> String {
    m.get_one::<String>(name).cloned().unwrap_or_default()
}

/// `m_argParser.get<int>( name )`.
pub fn get_int(m: &ArgMatches, name: &str) -> i32 {
    m.get_one::<i32>(name).copied().unwrap_or_default()
}

/// `m_argParser.get<double>( name )`.
pub fn get_double(m: &ArgMatches, name: &str) -> f64 {
    m.get_one::<f64>(name).copied().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_split_like_wx() {
        assert_eq!(wx_string_split("a=b", '='), vec!["a", "b"]);
        assert_eq!(wx_string_split("a=", '='), vec!["a"]);
        assert_eq!(wx_string_split("=b", '='), vec!["", "b"]);
        assert_eq!(wx_string_split("a=b=c", '='), vec!["a", "b", "c"]);
    }
}
