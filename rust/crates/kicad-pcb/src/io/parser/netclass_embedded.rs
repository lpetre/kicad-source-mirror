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

        self.board.embedded_files.set_are_fonts_embedded(embed_fonts);
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
