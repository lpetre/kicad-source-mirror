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

//! Port of `PCB_IO_KICAD_SEXPR_PARSER::ParseFootprintLib` of
//! `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr_parser.cpp` (package PCBIO-14): the footprints
//! of a `.kicad_mod` file (bare `(footprint ...)` / `(module ...)` or
//! `(kicad_footprint_lib (version N) ...)`), parsed into the holder board.
//!
//! The C++ parser runs with a null board here; the port parses into a holder board
//! (`BoardUse::FpHolder`, `board-model.md` 2) with `board_present == false`, see
//! [`crate::io::parse_footprint_file`].

use std::collections::BTreeMap;

use kicad_core::LibId;
use kicad_sexpr::IoError;
use kicad_sexpr::keywords::pcb_keys_t::T;

use super::{ParsedItem, PcbParser};
use crate::ids::FootprintId;
use crate::io::SEXPR_FOOTPRINT_LIB_FILE_VERSION;

impl PcbParser {
    /// Port of `ParseFootprintLib( aFootprints )`: the footprints of a footprint library file,
    /// keyed by their library item name like the C++ `boost::ptr_map<wxString,
    /// FP_CACHE_ENTRY>` (ordered by name; the first footprint of a name wins, a later one of
    /// the same name stays unattached in the board arena, as the C++ map deletes it).
    ///
    /// Each footprint is created in [`board`](Self::board) and not attached; its id has no
    /// library nickname, it is unlocked, carries the file format version and is finished
    /// (`finishItem`).
    ///
    /// Starts by reading the initial comment lines (`ReadCommentLines`), i.e. it is called on
    /// a fresh parser.
    pub(crate) fn parse_footprint_lib(
        &mut self,
    ) -> Result<BTreeMap<String, FootprintId>, IoError> {
        let mut footprints: BTreeMap<String, FootprintId> = BTreeMap::new();

        let mut initial_comments = self.lexer.read_comment_lines()?;

        let mut token = self.lexer.cur_tok();

        while token != T::T_RIGHT {
            if token != T::T_LEFT {
                return Err(self.lexer.expecting(T::T_LEFT));
            }

            token = self.lexer.next_tok()?;

            match token {
                // legacy token `module`; `footprint`: this is a footprint file prior to the
                // header addition.
                T::T_module | T::T_footprint => {
                    // There is only a single footprint so the closing ')' has already been
                    // parsed.
                    self.parse_lib_fp(&mut initial_comments, &mut footprints)?;
                    return Ok(footprints);
                }
                T::T_kicad_footprint_lib => {
                    let mut has_version = false;

                    token = self.lexer.next_tok()?;

                    while token != T::T_RIGHT {
                        if token != T::T_LEFT {
                            return Err(self.lexer.expecting(T::T_LEFT));
                        }

                        token = self.lexer.next_tok()?;

                        match token {
                            T::T_version => {
                                if has_version {
                                    return Err(self.lexer.unexpected(token));
                                }

                                has_version = true;

                                let version_number = self.parse_int_expected("version")?;

                                self.required_version = version_number;
                                self.too_recent =
                                    version_number > SEXPR_FOOTPRINT_LIB_FILE_VERSION;
                                // Bar token is known from this version
                                self.lexer.set_knows_bar(self.required_version >= 20240706);
                                self.lexer.need_right()?;
                            }
                            T::T_generator => {
                                self.lexer.need_symbol()?;
                                self.lexer.need_right()?;
                            }
                            T::T_generator_version => {
                                self.lexer.need_symbol()?;
                                self.generator_version = self.lexer.from_utf8();
                                self.lexer.need_right()?;
                            }
                            T::T_footprint => {
                                self.check_lib_version(has_version)?;
                                self.parse_lib_fp(&mut initial_comments, &mut footprints)?;
                            }
                            _ => {
                                self.check_lib_version(has_version)?;
                                return Err(self.lexer.expecting_str(
                                    "footprint, generator, generator_version, or version",
                                ));
                            }
                        }

                        token = self.lexer.next_tok()?;
                    }

                    self.check_lib_version(has_version)?;

                    return Ok(footprints);
                }
                _ => return Err(self.lexer.expecting_str("footprint or kicad_footprint_lib")),
            }
        }

        Ok(footprints)
    }

    /// The `checkVersion` lambda of `ParseFootprintLib`: version gates in the footprint parser
    /// need the version before they run.
    fn check_lib_version(&self, has_version: bool) -> Result<(), IoError> {
        if !has_version {
            return Err(self.lexer.expecting_str("version"));
        }

        if self.too_recent {
            return Err(kicad_sexpr::FutureFormatError::new(
                &self.required_version.to_string(),
                &self.generator_version,
            )
            .into());
        }

        Ok(())
    }

    /// The `parseFp` lambda of `ParseFootprintLib`: parse one footprint (the initial comments
    /// go to the first one, `initial_comments.release()`), strip the library nickname, unlock
    /// it, stamp the file format version, finish it and insert it by name.
    fn parse_lib_fp(
        &mut self,
        initial_comments: &mut Option<Vec<String>>,
        footprints: &mut BTreeMap<String, FootprintId>,
    ) -> Result<(), IoError> {
        let Some(fp) = self.parse_footprint(initial_comments.take())? else {
            return Ok(());
        };

        let required_version = self.required_version;
        let mut fp_name = String::new();

        self.board.modify_footprint(fp, |footprint, _| {
            fp_name = footprint.fpid().lib_item_name().to_string();
            footprint.fpid = LibId::from_parts("", &fp_name);

            // Locking a footprint has no meaning outside of a board
            footprint.base.is_locked = false;
            footprint.file_format_version_at_load = required_version;
        });

        self.finish_item(ParsedItem::Footprint(fp))?;

        // aFootprints.insert( fpName, ... ): an existing key keeps its entry.
        footprints.entry(fp_name).or_insert(fp);

        Ok(())
    }
}
