// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the net class / net chain / variant / embedded-file parts of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-11): `parseVariants`
//! (P:538-591), `parseNET_CHAINS_SECTION` and `parseNETCLASS` (P:3647-3883), and the
//! `embedded_fonts` / `embedded_files` branches of `parseBOARD_unchecked` (P:1547-1591) and of
//! `parseFOOTPRINT_unchecked` (P:7142-7175), which hand the line reader to
//! `EMBEDDED_FILES_PARSER` ([`parse_embedded`]) and turn its parse errors into warnings.
//!
//! The legacy net classes read here are transferred to the project afterwards (they replace
//! the project's classes and patterns): `project/board_project.rs`, `m_LegacyNetclassesLoaded`.

use std::sync::Arc;

use kicad_core::Kiid;
use kicad_core::string_utils::convert_to_new_overbar_notation;
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::embedded_files_t::{self, EmbeddedFilesLexer};
use kicad_sexpr::keywords::pcb_keys_t::{PcbLexer, T};

use super::PcbParser;
use crate::embedded_files::{EmbeddedFiles, parse_embedded};
use crate::ids::NetCode;
use crate::netclass::Netclass;

impl PcbParser {
    /// Port of `parseVariants`: call with `CurTok() == T_variants`.
    ///
    /// ```text
    /// (variants
    ///   (variant (name "VariantA") (description "Description A"))
    ///   (variant (name "VariantB") (description "Description B"))
    /// )
    /// ```
    pub(crate) fn parse_variants(&mut self) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            if token == T::T_variant {
                let mut variant_name = String::new();
                let mut description = String::new();

                token = self.lexer.next_tok()?;

                while token != T::T_RIGHT {
                    if token == T::T_LEFT {
                        token = self.lexer.next_tok()?;
                    }

                    match token {
                        T::T_name => {
                            self.lexer.need_symbol()?;
                            variant_name = self.lexer.from_utf8();
                            self.lexer.need_right()?;
                        }
                        T::T_description => {
                            self.lexer.need_symbol()?;
                            description = self.lexer.from_utf8();
                            self.lexer.need_right()?;
                        }
                        _ => return Err(self.lexer.expecting_str("name or description")),
                    }

                    token = self.lexer.next_tok()?;
                }

                if !variant_name.is_empty() {
                    self.board.add_variant(&variant_name);

                    if !description.is_empty() {
                        self.board
                            .set_variant_description(&variant_name, &description);
                    }
                }
            } else {
                return Err(self.lexer.expecting(T::T_variant));
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of `parseNETCLASS`: a legacy `(net_class <name> <description> ...)` of a pre-V6
    /// board, stored in the board's `NET_SETTINGS` (the default class is replaced when the
    /// name is the default class's name); the caller sets `m_LegacyNetclassesLoaded`.
    pub(crate) fn parse_netclass(&mut self) -> Result<(), IoError> {
        // wxCHECK_RET( CurTok() == T_net_class, ... )
        if self.lexer.cur_tok() != T::T_net_class {
            debug_assert!(
                false,
                "Cannot parse {} as net class.",
                self.lexer.cur_text()
            );
            return Ok(());
        }

        let mut nc = Netclass::new("", true);

        // Read netclass name (can be a name or just a number like track width)
        self.lexer.need_symbol_or_number()?;
        nc.set_name(&self.lexer.from_utf8());
        self.lexer.need_symbol()?;
        nc.description = self.lexer.from_utf8();

        let mut token = self.lexer.next_tok()?;

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                T::T_clearance => {
                    nc.clearance = Some(self.parse_netclass_units(T::T_clearance)?);
                }
                T::T_trace_width => {
                    nc.track_width = Some(self.parse_netclass_units(T::T_trace_width)?);
                }
                T::T_via_dia => {
                    nc.via_dia = Some(self.parse_netclass_units(T::T_via_dia)?);
                }
                T::T_via_drill => {
                    nc.via_drill = Some(self.parse_netclass_units(T::T_via_drill)?);
                }
                T::T_uvia_dia => {
                    nc.uvia_dia = Some(self.parse_netclass_units(T::T_uvia_dia)?);
                }
                T::T_uvia_drill => {
                    nc.uvia_drill = Some(self.parse_netclass_units(T::T_uvia_drill)?);
                }
                T::T_diff_pair_width => {
                    nc.diff_pair_width = Some(self.parse_netclass_units(T::T_diff_pair_width)?);
                }
                T::T_diff_pair_gap => {
                    nc.diff_pair_gap = Some(self.parse_netclass_units(T::T_diff_pair_gap)?);
                }
                T::T_add_net => {
                    self.lexer.need_symbol_or_number()?;

                    let mut net_name = self.lexer.from_utf8();

                    // Convert overbar syntax from `~...~` to `~{...}`.  These were left out of
                    // the first merge so the version is a bit later.
                    if self.required_version < 20210606 {
                        net_name = convert_to_new_overbar_notation(&self.lexer.from_utf8());
                    }

                    self.board
                        .design_settings
                        .net_settings
                        .set_netclass_pattern_assignment(&net_name, &nc.name);
                }
                _ => {
                    return Err(self.lexer.expecting_str(
                        "clearance, trace_width, via_dia, via_drill, uvia_dia, uvia_drill, \
                         diff_pair_width, diff_pair_gap or add_net",
                    ));
                }
            }

            self.lexer.need_right()?;
            token = self.lexer.next_tok()?;
        }

        let net_settings = &mut self.board.design_settings.net_settings;

        if net_settings.has_netclass(&nc.name) {
            // Must have been a name conflict, this is a bad board file.
            // User may have done a hand edit to the file.
            return Err(IoError::new(format!(
                "Duplicate NETCLASS name '{}' in file '{}' at line {}, offset {}.",
                nc.name,
                self.lexer.cur_source(),
                self.lexer.cur_line_number(),
                self.lexer.cur_offset()
            )));
        } else if nc.name == net_settings.default_netclass().name {
            net_settings.set_default_netclass(Arc::new(nc));
        } else {
            let name = nc.name.clone();
            net_settings.set_netclass(&name, Arc::new(nc));
        }

        Ok(())
    }

    /// `parseBoardUnits( T aToken )` of the net class fields (`EDA_DATA_TYPE::DISTANCE`).
    fn parse_netclass_units(&mut self, token: T) -> Result<i32, IoError> {
        self.parse_board_units_tok(token, kicad_core::EdaDataType::Distance)
    }

    /// Port of `parseNET_CHAINS_SECTION`: the `(net_chains ...)` aggregation block providing
    /// chain names, member nets and optional terminal pads.
    ///
    /// Tokens inside the section: `(net_chain (name <str>) (members (net <name>) ...)
    /// (terminal_pad <uuid>) (terminal_pad <uuid>))`; legacy files use net codes and the
    /// `(member (net <code>))` wrapper.
    pub(crate) fn parse_net_chains_section(&mut self) -> Result<(), IoError> {
        let mut token = self.lexer.next_tok()?;

        while token != T::T_EOF {
            if token == T::T_RIGHT {
                break; // end of (net_chains ...)
            } else if token == T::T_LEFT {
                token = self.lexer.next_tok()?;
            }

            if token != T::T_net_chain {
                self.skip_current()?;
                token = self.lexer.next_tok()?;
                continue;
            }

            let mut name = String::new();
            let mut net_codes: Vec<i32> = Vec::new();
            let mut net_names: Vec<String> = Vec::new();
            // `KIID padUuids[2]`: default-constructed, i.e. two fresh ids are drawn.
            let mut pad_uuids = [
                Kiid::new(&mut self.board.kiid_generator),
                Kiid::new(&mut self.board.kiid_generator),
            ];
            let mut pad_count = 0usize;

            let mut t = self.lexer.next_tok()?;

            while t != T::T_EOF {
                if t == T::T_RIGHT {
                    break; // end chain
                } else if t == T::T_LEFT {
                    t = self.lexer.next_tok()?;
                }

                match t {
                    T::T_name => {
                        self.lexer.need_symbol_or_number()?;
                        name = self.lexer.from_utf8();
                        self.lexer.need_right()?;
                    }
                    T::T_members => {
                        self.parse_net_chain_members(&mut net_codes, &mut net_names)?;
                    }
                    T::T_terminal_pad => {
                        if pad_count < 2 {
                            self.lexer.need_symbol_or_number()?;
                            pad_uuids[pad_count] = Kiid::from_string_or_random(
                                &self.lexer.from_utf8(),
                                &mut self.board.kiid_generator,
                            );
                            pad_count += 1;
                            self.lexer.need_right()?;
                        } else {
                            self.skip_current()?;
                        }
                    }
                    _ => self.skip_current()?,
                }

                t = self.lexer.next_tok()?;
            }

            // The nets are identified by their code; the C++ keeps NETINFO_ITEM pointers.
            let mut resolved_nets: Vec<NetCode> = Vec::new();

            for &code in &net_codes {
                let mut internal_code = code;

                if code >= 0
                    && (code as usize) < self.net_codes.len()
                    && self.net_codes[code as usize] != 0
                {
                    internal_code = self.net_codes[code as usize];
                }

                if let Some(net) = self.board.find_net(NetCode(internal_code)) {
                    resolved_nets.push(net.net_code());
                }
            }

            for net_name in &net_names {
                if let Some(net) = self.board.find_net_by_name(net_name) {
                    resolved_nets.push(net.net_code());
                }
            }

            // FindPadByUuid does not depend on the nets: look the pads up once.
            let pads: Vec<_> = pad_uuids[..pad_count]
                .iter()
                .map(|uuid| self.board.find_pad_by_uuid(uuid))
                .collect();

            for code in resolved_nets {
                // `FindNet( 0 )` of a board without nets is the static orphaned item, which
                // the C++ would modify; it is not part of the board and is left alone here.
                let Some(net) = self.board.net_info.get_net_item_mut(code) else {
                    continue;
                };

                net.set_net_chain(&name);

                for i in 0..pad_count.min(2) {
                    net.set_terminal_pad_uuid(i, pad_uuids[i]);

                    // SetTerminalPad: the pointer only, the UUID was set just above.
                    if let Some(pad) = pads[i] {
                        net.terminal_pads[i] = Some(pad);
                    }
                }
            }

            token = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// The `T_members` case of `parseNET_CHAINS_SECTION`: `(net <name>)` (current) or
    /// `(net <code>)` (legacy) entries, optionally wrapped in `(member ...)`.
    fn parse_net_chain_members(
        &mut self,
        net_codes: &mut Vec<i32>,
        net_names: &mut Vec<String>,
    ) -> Result<(), IoError> {
        let mut mt = self.lexer.next_tok()?;

        while mt != T::T_RIGHT {
            // Deviation: the C++ loop never ends at the end of a truncated file (`skipCurrent`
            // returns at EOF and `NextTok` keeps returning EOF); fail instead of hanging.
            if mt == T::T_EOF {
                return Err(self.lexer.expecting(T::T_RIGHT));
            }

            if mt == T::T_LEFT {
                mt = self.lexer.next_tok()?;
            }

            if mt == T::T_net {
                // Accept either a net code (legacy) or a net name (current).
                let tok = self.lexer.next_tok()?;

                if tok == T::T_NUMBER {
                    net_codes.push(self.lexer.parse_int());
                } else {
                    net_names.push(self.lexer.from_utf8());
                }

                self.lexer.need_right()?;
            } else if mt == T::T_member {
                // legacy style (member (net <code>)) wrapper
                let mut inner = self.lexer.next_tok()?;

                if inner == T::T_LEFT {
                    inner = self.lexer.next_tok()?;
                }

                if inner == T::T_net {
                    let tok2 = self.lexer.next_tok()?;

                    if tok2 == T::T_NUMBER {
                        net_codes.push(self.lexer.parse_int());
                    } else {
                        net_names.push(self.lexer.from_utf8());
                    }

                    self.lexer.need_right()?;
                }

                self.lexer.need_right()?;
            } else {
                self.skip_current()?;
            }

            mt = self.lexer.next_tok()?;
        }

        Ok(())
    }

    /// Port of the `T_embedded_fonts` branch of `parseBOARD_unchecked`.
    pub(crate) fn parse_board_embedded_fonts(&mut self) -> Result<(), IoError> {
        let mut embed_fonts = self.parse_bool()?;

        // An append must not clear the destination's flag; saving with it off deletes the
        // fonts the destination already embedded
        if self.append_to_existing {
            embed_fonts = embed_fonts || self.board.embedded_files.are_fonts_embedded();
        }

        self.board
            .embedded_files
            .set_are_fonts_embedded(embed_fonts);
        self.lexer.need_right()
    }

    /// Port of the `T_embedded_files` branch of `parseBOARD_unchecked`: a `PARSE_ERROR` of
    /// the embedded files parser becomes a parse warning and the rest of the section is
    /// skipped.
    pub(crate) fn parse_board_embedded_files(&mut self) -> Result<(), IoError> {
        parse_embedded_files_section(
            &mut self.lexer,
            &mut self.board.embedded_files,
            &mut self.report.warnings,
        )
    }

    /// Port of the `T_embedded_fonts` branch of `parseFOOTPRINT_unchecked`:
    /// `footprint->GetEmbeddedFiles()->SetAreFontsEmbedded( parseBool() )` (no append rule).
    /// For PCBIO-06: pass the footprint's `embedded_files`.
    pub(crate) fn parse_footprint_embedded_fonts(
        &mut self,
        files: &mut EmbeddedFiles,
    ) -> Result<(), IoError> {
        files.set_are_fonts_embedded(self.parse_bool()?);
        self.lexer.need_right()
    }

    /// Port of the `T_embedded_files` branch of `parseFOOTPRINT_unchecked` (the same
    /// warning recovery as the board's). For PCBIO-06: pass the footprint's `embedded_files`.
    pub(crate) fn parse_footprint_embedded_files(
        &mut self,
        files: &mut EmbeddedFiles,
    ) -> Result<(), IoError> {
        parse_embedded_files_section(&mut self.lexer, files, &mut self.report.warnings)
    }
}

/// The `T_embedded_files` branches of `parseBOARD_unchecked` / `parseFOOTPRINT_unchecked`:
///
/// ```text
/// EMBEDDED_FILES_PARSER embeddedFilesParser( reader );
/// embeddedFilesParser.SyncLineReaderWith( *this );
/// try { embeddedFilesParser.ParseEmbedded( files ); }
/// catch( const PARSE_ERROR& e ) { m_parseWarnings.push_back( e.What() ); skip to ')' }
/// SyncLineReaderWith( embeddedFilesParser );
/// ```
///
/// Any other `IO_ERROR` propagates (and is not synchronised back, as the C++ unwinds).
fn parse_embedded_files_section(
    lexer: &mut PcbLexer,
    files: &mut EmbeddedFiles,
    warnings: &mut Vec<String>,
) -> Result<(), IoError> {
    use embedded_files_t::T as Et;

    lexer.with_sub_lexer(|sub: &mut EmbeddedFilesLexer| -> Result<(), IoError> {
        let Err(e) = parse_embedded(sub, files) else {
            return Ok(());
        };

        if e.as_parse_error().is_none() {
            return Err(e);
        }

        warnings.push(e.what().to_string());

        // ParseEmbedded may have stopped mid-section. Skip remaining
        // tokens so the board parser doesn't see them at the top level.
        let mut depth = 0;

        loop {
            let tok = sub.next_tok()?;

            if tok == Et::T_EOF {
                break;
            }

            if tok == Et::T_LEFT {
                depth += 1;
            } else if tok == Et::T_RIGHT {
                depth -= 1;

                if depth < 0 {
                    break;
                }
            }
        }

        Ok(())
    })
}

#[cfg(test)]
mod tests {
    //! PCBIO-11 tests: the differential oracle (`tests/oracle/pcbio11_golden.sh`), unit tests
    //! of the token loops and the I/O parts of `qa/tests/pcbnew/net_chains/
    //! test_net_chain_terminal_pads.cpp` and `test_variant.cpp`.

    use std::path::Path;

    use kicad_sexpr::{FormatMode, OutputFormatter, PrettifiedOutputFormatter};

    use crate::board::{AddMode, Board};
    use crate::embedded_files::{EmbeddedFileType, EmbeddedFiles};
    use crate::ids::NetCode;
    use crate::io::writer::PcbWriter;
    use crate::io::{FormatOptions, ParseOptions, format_board, parse_board, parse_board_report};
    use crate::pcb_track::{PcbTrack, Track};

    fn crate_dir() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    fn staging() -> ParseOptions {
        ParseOptions {
            allow_pending: true,
        }
    }

    fn fnv1a64(bytes: &[u8]) -> String {
        let mut h: u64 = 0xcbf29ce484222325;

        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x100000001b3);
        }

        format!("{h:016x}")
    }

    /// The top-level blocks of a saved board, as `pcbio11_golden.sh` extracts them:
    /// `(keyword, block text)` for `variants`, `net_chains`, `embedded_fonts`, `embedded_files`.
    fn blocks(text: &str) -> Vec<(&'static str, String)> {
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut out = Vec::new();

        for kw in ["variants", "net_chains", "embedded_fonts", "embedded_files"] {
            let open = format!("\t({kw}");

            for (i, l) in lines.iter().enumerate() {
                if !l.starts_with(&open) {
                    continue;
                }

                let mut j = i;

                if !l.trim_end_matches('\n').ends_with(')') {
                    while lines[j] != "\t)\n" {
                        j += 1;
                    }
                }

                out.push((kw, lines[i..=j].concat()));
            }
        }

        out
    }

    fn board(text: &str) -> Board {
        parse_board(text, "", &ParseOptions::default()).unwrap()
    }

    /// The parts of `format( const BOARD* )` this package covers: `formatVariants`, the net chains, the
    /// `(embedded_fonts ..)` flag and the consolidated `(embedded_files ..)`.
    fn write_sections(board: &Board) -> String {
        let mut out = PrettifiedOutputFormatter::new(FormatMode::Normal, false);
        {
            let mut w = PcbWriter::new(&mut out, board, &FormatOptions::default());
            w.out.print(format_args!("(kicad_pcb")).unwrap();
            w.format_variants(board).unwrap();
            w.format_net_chains(board).unwrap();

            let mut files = EmbeddedFiles::default();

            for file in board.embedded_files.embedded_file_map().values() {
                // FormatBoardToFormatter: ClearEmbeddedFonts() without embedded fonts.
                if !board.embedded_files.are_fonts_embedded()
                    && file.type_ == EmbeddedFileType::Font
                {
                    continue;
                }

                files.add_file(file.clone());
            }

            // format( const BOARD* ): the footprints' files are consolidated into the same
            // map, in the writer's footprint order (`std::set` with `BOARD_ITEM::ptr_cmp`).
            let ctx = crate::ctx::BoardCtx::bare(board);
            let sorted_footprints = crate::io::writer::sorted_set(
                board.footprints().map(|(id, _)| crate::ids::ItemId::from(id)),
                |a, b| crate::board_item::board_item_ptr_cmp(&ctx, *a, *b),
            );

            for item in sorted_footprints {
                let Some(fp) = item.typed().and_then(|id| board.footprint(id)) else {
                    continue;
                };

                for file in fp.embedded_files.embedded_file_map().values() {
                    files.add_file(file.clone());
                }
            }

            let fonts = if board.embedded_files.are_fonts_embedded() {
                "yes"
            } else {
                "no"
            };
            w.out
                .print(format_args!("(embedded_fonts {fonts})"))
                .unwrap();

            if !files.is_empty() {
                files.write_embedded_files(&mut *w.out, true).unwrap();
            }

            w.out.print(format_args!(")")).unwrap();
        }
        out.finish().unwrap();
        out.into_string()
    }

    #[test]
    fn variants_registry() {
        let b = board(
            "(kicad_pcb (version 20260206) (generator x)\n(variants (variant (name \"A\") \
             (description \"d\")) (variant (name \"a\") (description \"e\")) (variant \
             (description \"x\")) (variant (name \"default\")) (variant (name \"B\"))))",
        );
        // The default variant name is the empty string's placeholder, not "default": the
        // reference re-save of chains.kicad_pcb keeps a variant named "Default".
        assert_eq!(
            b.variant_names,
            vec!["A".to_string(), "default".to_string(), "B".to_string()]
        );
        assert_eq!(b.get_variant_description("A"), "e");

        let err = parse_board(
            "(kicad_pcb (version 20260206) (generator x)\n(variants (variant (bad 1))))",
            "",
            &ParseOptions::default(),
        )
        .err()
        .unwrap();
        assert!(
            err.what().starts_with("Expecting name or description. Got"),
            "{}",
            err.what()
        );

        let err = parse_board(
            "(kicad_pcb (version 20260206) (generator x)\n(variants (foo)))",
            "",
            &ParseOptions::default(),
        )
        .err()
        .unwrap();
        assert!(
            err.what().starts_with("Expecting ''variant''"),
            "{}",
            err.what()
        );
    }

    #[test]
    fn net_chains_resolution() {
        let b = board(
            "(kicad_pcb (version 20250926) (generator x)\n(net 0 \"\") (net 5 \"A\") (net 9 \"B\") \
             (net 2 \"C\")\n(net_chains (net_chain (name \"S\") (members (net 9) (member \
             (net 2)) (net \"C\") (net 77)) (terminal_pad \"00000000-0000-0000-0000-00000000000a\") \
             (terminal_pad \"00000000-0000-0000-0000-00000000000b\") (terminal_pad \"x\")) \
             (other (thing)) (net_chain (name 7) (members (net \"A\")))))",
        );
        // File codes are mapped through m_netCodes: 9 -> "B", 2 -> "C".
        let by_name = |n: &str| b.find_net_by_name(n).unwrap();
        assert_eq!(by_name("B").net_chain(), "S");
        assert_eq!(by_name("C").net_chain(), "S");
        assert_eq!(by_name("A").net_chain(), "7");
        assert_eq!(
            by_name("B").terminal_pad_uuid(1).as_string(),
            "00000000-0000-0000-0000-00000000000b"
        );
        // No such pad on the board: the UUID is kept, the pointer stays empty.
        assert!(by_name("B").terminal_pad(0).is_none());
        assert_eq!(b.find_net(NetCode(0)).unwrap().net_chain(), "");
    }

    #[test]
    fn legacy_netclass_errors() {
        let text = std::fs::read_to_string(
            crate_dir().join("tests/data/pcbio11/legacy_netclass_dup_bad.kicad_pcb"),
        )
        .unwrap();
        let err = parse_board(&text, "<file>", &ParseOptions::default())
            .err()
            .unwrap();
        let log = std::fs::read_to_string(
            crate_dir().join("tests/golden/pcbio11/legacy_netclass_dup_bad.log.txt"),
        )
        .unwrap();
        assert_eq!(format!("Failed to load board: {}\n", err.what()), log);

        let err = parse_board(
            "(kicad_pcb (version 20200512) (host x y)\n(net_class N \"\" (track 1)))",
            "",
            &ParseOptions::default(),
        )
        .err()
        .unwrap();
        assert!(
            err.what().starts_with(
                "Expecting clearance, trace_width, via_dia, via_drill, uvia_dia, uvia_drill, \
                 diff_pair_width, diff_pair_gap or add_net."
            ),
            "{}",
            err.what()
        );
    }

    /// Byte level against the reference re-save (`tests/golden/pcbio11/<name>.kicad_pcb`): the
    /// hand-made boards contain nothing but what this package parses and writes, so the whole
    /// re-saved file must be identical; the embedded file warning must be the reference text.
    #[test]
    fn oracle_resave() {
        for name in ["chains", "embedded", "embedded_bad", "legacy_netclass"] {
            let input = std::fs::read_to_string(
                crate_dir().join(format!("tests/data/pcbio11/{name}.kicad_pcb")),
            )
            .unwrap();
            let golden = std::fs::read_to_string(
                crate_dir().join(format!("tests/golden/pcbio11/{name}.kicad_pcb")),
            )
            .unwrap();
            let log = std::fs::read_to_string(
                crate_dir().join(format!("tests/golden/pcbio11/{name}.log.txt")),
            )
            .unwrap();

            let (b, report) =
                parse_board_report(&input, "<file>", &ParseOptions::default()).unwrap();
            let warnings: String = report
                .warnings
                .iter()
                .map(|w| format!("Warning: {w}\n"))
                .collect();
            assert_eq!(warnings, log, "{name}");

            let saved = format_board(&b, &FormatOptions::default()).unwrap();
            assert_eq!(blocks(&saved), blocks(&golden), "{name}");
            assert_eq!(saved, golden, "{name}");

            // Every embedded file that survived decodes with a valid checksum.
            for file in b.embedded_files.embedded_file_map().values() {
                assert!(
                    crate::embedded_files::EmbeddedFile::clone(file).validate(),
                    "{name}"
                );
            }
        }
    }

    /// Item level against `pcb-dump board --no-init --sections board` (net classes, pattern
    /// assignments, nets with their class and chain, effective net classes) of the hand-made
    /// boards and of every corpus board with legacy `(net_class ..)` sections.
    #[cfg(feature = "dump")]
    #[test]
    fn oracle_dump_netclass() {
        use crate::dump::{DumpOptions, compare::compare, dump_board, scan_file_uuids};

        let keep = |l: &&str| {
            l.starts_with("file|")
                || l.contains("ds.netclass")
                || l.contains("netclass_pattern")
                || l.contains("effective_netclass")
                || l.split('|').nth(2).is_some_and(|k| {
                    k.strip_prefix("net.")
                        .and_then(|r| r.split_once('.'))
                        .is_some_and(|(code, _)| code.chars().all(|c| c.is_ascii_digit()))
                })
        };

        let mut cases: Vec<(String, String, String)> = Vec::new();

        for name in ["chains", "embedded", "embedded_bad", "legacy_netclass"] {
            cases.push((
                format!("{name}.kicad_pcb"),
                crate_dir()
                    .join(format!("tests/data/pcbio11/{name}.kicad_pcb"))
                    .to_string_lossy()
                    .into_owned(),
                std::fs::read_to_string(
                    crate_dir().join(format!("tests/golden/pcbio11/{name}.dump.txt")),
                )
                .unwrap(),
            ));
        }

        let corpus =
            std::fs::read_to_string(crate_dir().join("tests/golden/pcbio11_corpus_netclass.txt"))
                .unwrap();

        for part in corpus.split("path|").skip(1) {
            let (path, golden) = part.split_once('\n').unwrap();
            let file = kicad_qa::kicad_source_root().join(path);
            let name = file.file_name().unwrap().to_string_lossy().into_owned();
            cases.push((
                name,
                file.to_string_lossy().into_owned(),
                golden.to_string(),
            ));
        }

        let mut checked = 0;

        for (name, path, golden) in &cases {
            let text = std::fs::read_to_string(path).unwrap();
            let Ok(b) = parse_board(&text, "", &staging()) else {
                panic!("{name}: parse error");
            };
            let opts = DumpOptions {
                sections: ["board".to_string()].into_iter().collect(),
                hash: false,
                file_uuids: Some(scan_file_uuids(text.as_bytes())),
            };
            let rust = format!("file|{name}|\n{}", dump_board(&b, &opts));
            let rust: Vec<&str> = rust.lines().filter(keep).collect();
            let cmp = compare(golden, &rust.join("\n"));
            let failures: Vec<_> = cmp.lines.iter().filter(|l| l.0.is_failure()).collect();
            assert!(failures.is_empty(), "{name}:\n{}", cmp.report(20));
            checked += cmp.lines.len();
        }

        assert!(checked > 1000, "{checked}");
    }

    /// Corpus byte level (`tests/golden/pcbio11_corpus_sections.txt`): the `(variants ..)`,
    /// `(net_chains ..)`, `(embedded_fonts ..)` and `(embedded_files ..)` blocks of the
    /// reference re-save of every corpus board that has one of them. A board whose footprints
    /// carry embedded files is checked once footprints are parsed (PCBIO-06): their files are
    /// consolidated into the board's section. Also: every parsed embedded file validates.
    #[test]
    fn oracle_corpus_sections() {
        let golden =
            std::fs::read_to_string(crate_dir().join("tests/golden/pcbio11_corpus_sections.txt"))
                .unwrap();
        let root = kicad_qa::kicad_source_root();
        let mut boards: Vec<(&str, Vec<(&str, &str, &str)>)> = Vec::new();

        for line in golden.lines().filter(|l| !l.starts_with('#')) {
            let f: Vec<&str> = line.split('|').collect();
            let row = (
                f[1],
                f.get(2).copied().unwrap_or(""),
                f.get(3).copied().unwrap_or(""),
            );

            match boards.last_mut() {
                Some((path, rows)) if *path == f[0] => rows.push(row),
                _ => boards.push((f[0], vec![row])),
            }
        }

        let (mut identical, mut pending, mut files) = (0, 0, 0);

        for (path, rows) in &boards {
            let text = std::fs::read_to_string(root.join(path)).unwrap();
            // A model stub hit while parsing (`port_pending!` panic) leaves the board pending.
            let parsed = std::panic::catch_unwind(|| parse_board_report(&text, path, &staging()));
            let Ok(parsed) = parsed else {
                pending += rows.len();
                continue;
            };
            let (mut b, report) = parsed.unwrap();
            assert!(report.warnings.is_empty(), "{path}: {:?}", report.warnings);

            // The reference re-save runs BOARD_LOADER::Load, whose BuildListOfNets()
            // (board_loader.cpp) keeps the terminal pad UUIDs but clears the resolved pad
            // pointers (NETINFO_LIST::buildListOfNets): the saved chains have no terminal_pad
            // (issue25065).
            b.build_list_of_nets();

            for file in b.embedded_files.embedded_file_map().values() {
                assert!(
                    crate::embedded_files::EmbeddedFile::clone(file).validate(),
                    "{path}"
                );
                files += 1;
            }

            let saved = write_sections(&b);
            let ours = blocks(&saved);

            for (kw, lines, hash) in rows {
                if *kw == "none" {
                    continue;
                }

                let block = ours.iter().find(|(k, _)| k == kw).map(|(_, t)| t.as_str());
                let got = block.map(|t| {
                    (
                        format!("lines={}", t.split_inclusive('\n').count()),
                        fnv1a64(t.as_bytes()),
                    )
                });
                assert_eq!(
                    got,
                    Some((lines.to_string(), hash.to_string())),
                    "{path} {kw}:\n{}",
                    block.unwrap_or("")
                );
                identical += 1;
            }
        }

        eprintln!(
            "pcbio11 corpus sections: identical {identical}, pending {pending}, files {files}"
        );
        assert!(identical >= 20, "identical {identical}, pending {pending}");
        assert!(files > 0);
    }

    /// `test_net_chain_terminal_pads.cpp` `SingleNetSignalNamePersists` and
    /// `SignalsSurviveBuildListOfNets` (save, load, chain names kept; the second one also
    /// through `BuildListOfNets`). `SignalTerminalPadsRoundTrip` needs footprints with pads
    /// (PCBIO-06).
    #[test]
    fn net_chain_names_round_trip() {
        let names = [
            "Net-(R1-Pad1)",
            "Net-(R1-Pad2)",
            "Net-(R2-Pad2)",
            "Net-(R3-Pad2)",
        ];
        let mut b = Board::new();
        let mut x = 0;

        for name in names {
            let code = b.find_or_add_net(name, None);
            b.net_info
                .get_net_item_mut(code)
                .unwrap()
                .set_net_chain(if name == names[0] {
                    "CLK_REF"
                } else {
                    "Signal1"
                });

            // Add trivial tracks so that each net is considered used and serialized
            let mut t = PcbTrack::new(&mut b.kiid_generator);
            t.start = kicad_geom::Vector2I::new(x, 0);
            t.end = kicad_geom::Vector2I::new(x + 1_000_000, 0);
            let id = b.add_track(Track::Trace(t), AddMode::BulkAppend).unwrap();
            b.set_item_net(id.into(), code, false);
            x += 2_000_000;
        }

        let text = format_board(&b, &FormatOptions::default()).unwrap();
        let mut loaded = board(&text);
        assert_eq!(
            loaded.find_net_by_name(names[0]).unwrap().net_chain(),
            "CLK_REF"
        );

        // Simulate GUI post-load rebuild that previously cleared signals
        loaded.build_list_of_nets();
        let seen: Vec<&str> = loaded
            .nets()
            .filter(|n| n.net_chain() == "Signal1")
            .map(|n| n.netname())
            .collect();
        assert_eq!(seen, names[1..].to_vec());
    }

    /// `test_variant.cpp` `VariantTestProjectLoad`, board part: the variant registry of
    /// `variant_test.kicad_pcb` (the footprint variants are PCBIO-06).
    #[test]
    fn variant_test_project_load() {
        let path = kicad_qa::pcbnew_data_dir().join("variant_test/variant_test.kicad_pcb");
        let text = std::fs::read_to_string(path).unwrap();
        let b = parse_board(&text, "", &staging()).unwrap();
        assert!(b.has_variant("Variant A"));
    }
}
