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

//! Port of `qa/tests/common/test_richio.cpp`: test suite `RichIO` for RICHIO and related
//! formatting utilities.

use kicad_sexpr::{
    FormatMode, IoError, LineReader, OutputFormatter, StringFormatter, StringLineReader,
    format_stream_data, prettify_in_place,
};

/// Read all the lines of `buf`, as the C++ tests do inside `BOOST_CHECK_NO_THROW`.
fn consume_all_lines(buf: &str) -> Result<(), IoError> {
    let mut reader = StringLineReader::new(buf, "test");

    while reader.read_line()?.is_some() {
        // just consume
    }

    Ok(())
}

/// `BOOST_AUTO_TEST_CASE( PrettifyLargeImageData )`
///
/// Verify that Prettify produces well-formed output for large (data ...) blocks such as
/// base64-encoded images, and that STRING_LINE_READER can read the result.
///
/// Regression test for https://gitlab.com/kicad/code/kicad/-/issues/23162
#[test]
fn prettify_large_image_data() -> Result<(), IoError> {
    let mut fmt = StringFormatter::new();

    fmt.print(format_args!(
        "(kicad_sch (version 20231120) (generator \"eeschema\")"
    ))?;
    fmt.print(format_args!("(image (at 0 0)"))?;

    let image_size: usize = 2 * 1024 * 1024;
    let fake_image = vec![0x42u8; image_size];

    format_stream_data(&mut fmt, &fake_image)?;

    fmt.print(format_args!(")"))?; // close image
    fmt.print(format_args!(")"))?; // close kicad_sch

    let mut buf = fmt.string().to_string();

    prettify_in_place(&mut buf, FormatMode::Normal);

    // BOOST_CHECK_NO_THROW
    assert_eq!(consume_all_lines(&buf), Ok(()));

    Ok(())
}

/// `BOOST_AUTO_TEST_CASE( PrettifyLongQuotedString )`
///
/// Verify that STRING_LINE_READER can handle prettified output containing a very long quoted
/// string (e.g. a property value that exceeds the old 1 MB limit).
///
/// Regression test for https://gitlab.com/kicad/code/kicad/-/issues/23162
#[test]
fn prettify_long_quoted_string() -> Result<(), IoError> {
    let long_len: usize = 1_100_000;
    let long_value = "A".repeat(long_len);

    let mut fmt = StringFormatter::new();

    fmt.print(format_args!("(kicad_sch (version 20231120)"))?;
    fmt.print(format_args!(
        "(property \"Description\" {} (at 0 0 0))",
        fmt.quotes(&long_value)
    ))?;
    fmt.print(format_args!(")"))?;

    let mut buf = fmt.string().to_string();

    prettify_in_place(&mut buf, FormatMode::Normal);

    // BOOST_CHECK_NO_THROW
    assert_eq!(consume_all_lines(&buf), Ok(()));

    Ok(())
}

/// `BOOST_AUTO_TEST_CASE( IndentWritesTwoSpacesPerLevel )`
///
/// Indent writes two spaces per nesting level and nothing else.  Callers that want only the
/// indentation, such as DSN::WIRE::Format re-indenting a closing paren, depend on the exact
/// width.
#[test]
fn indent_writes_two_spaces_per_level() -> Result<(), IoError> {
    for nest_level in [0, 1, 2, 7] {
        let mut fmt = StringFormatter::new();

        let written = fmt.indent(nest_level)?;

        assert_eq!(fmt.string(), " ".repeat(2 * nest_level as usize));
        assert_eq!(written, 2 * nest_level);
    }

    Ok(())
}

/// `BOOST_AUTO_TEST_CASE( NestedPrintIndentsItsOutput )`
///
/// The nesting Print() overload prepends that same indentation to its formatted output.
#[test]
fn nested_print_indents_its_output() -> Result<(), IoError> {
    let mut fmt = StringFormatter::new();

    let written = fmt.print_nest(3, format_args!("(net {})", 42))?;

    assert_eq!(fmt.string(), "      (net 42)");
    assert_eq!(written, 14);

    Ok(())
}
