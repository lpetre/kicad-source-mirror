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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-14: `FormatFootprintLibFile` (W:354-371) and the pure normalisation steps of
//! `FootprintSave` (W:4176-4202).
//!
//! The footprint is written exactly as `format( aFootprint, true )` (the footprint writer of
//! PCBIO-06); what differs from a board footprint comes from the control flags the caller
//! passes (`CTL_FOR_LIBRARY` for `FP_CACHE::Save`: no library nickname, no footprint uuid, no
//! `(transform ...)`, no `(path ...)`, no pad / item nets, embedded files with their data) and
//! from `m_board == nullptr` ([`PcbWriter::board`] is `None` here).

use kicad_core::PcbLayerId;
use kicad_geom::{ANGLE_0, FlipDirection};
use kicad_sexpr::IoError;

use super::{FormatOptions, PcbWriter};
use crate::board::Board;
use crate::embedded_files::EmbeddedFileType;
use crate::ids::{FootprintId, ItemId, NetCode};
use crate::io::SEXPR_FOOTPRINT_LIB_FILE_VERSION;
use crate::item::{ParentRef, RecurseMode};

impl PcbWriter<'_> {
    /// Port of `FormatFootprintLibFile( FOOTPRINT* aFootprint )`: the initial comments, then
    /// `(kicad_footprint_lib (version ..) (generator "kicad_footprint_editor")
    /// (generator_version "..")`, the footprint (`aInLibFile = true`) and `)`.
    ///
    /// The C++ first mutates the footprint: `EmbedFonts()` when it embeds its fonts (needs the
    /// font files, `pcb-io.md` R7: a no-op here, the embedded font files already present are
    /// kept, as for the board writer), else `ClearEmbeddedFonts()`. The port keeps the source
    /// board immutable: when there is a font file to clear, the footprint is written from a
    /// copy of the board with the fonts cleared.
    ///
    /// The footprints the C++ writes into library files have no board (`FP_CACHE` entries are
    /// parsed with a null board, `FootprintSave` detaches its clone, the QA helpers save what
    /// `FootprintLoad` returned), so `GetBoard()` is null for the footprint and every child
    /// while they are formatted (e.g. a zone's layers are not masked with the enabled layers, a
    /// custom padstack lists only its explicit layers). The port's stand-in for such a
    /// footprint is the one of a footprint holder board (`BoardUse::FpHolder`,
    /// [`crate::io::parse_footprint_file`]): for a holder, the footprint is written from a copy
    /// in which it has no parent (`ParentRef::None`), so that `has_board` is false below it.
    pub(crate) fn format_footprint_lib_file(
        &mut self,
        footprint: FootprintId,
    ) -> Result<(), IoError> {
        let src = self.src;

        let clear_fonts = src.footprint(footprint).is_some_and(|fp| {
            !fp.embedded_files.are_fonts_embedded()
                && fp
                    .embedded_files
                    .embedded_file_map()
                    .values()
                    .any(|file| file.type_ == EmbeddedFileType::Font)
        });

        let detach = src.is_footprint_holder() && src.footprint(footprint).is_some();

        if clear_fonts || detach {
            let mut copy: Board = src.clone();

            if let Some(fp) = copy.footprints.get_mut(footprint) {
                if clear_fonts {
                    fp.embedded_files.clear_embedded_fonts();
                }

                if detach {
                    fp.base.parent = ParentRef::None;
                }
            }

            let options = FormatOptions {
                ctl: self.ctl,
                allow_pending: self.allow_pending,
                generator_version: self.generator_version.clone(),
                ..FormatOptions::default()
            };

            let skipped = {
                let mut inner = PcbWriter::new(&mut *self.out, &copy, &options);
                inner.format_footprint_lib_body(footprint)?;
                inner.into_report().skipped
            };

            self.report.skipped.extend(skipped);
            return Ok(());
        }

        self.format_footprint_lib_body(footprint)
    }

    /// The printing part of `FormatFootprintLibFile` (after the font step).
    fn format_footprint_lib_body(&mut self, footprint: FootprintId) -> Result<(), IoError> {
        self.format_initial_comments(footprint)?;

        // The generator version is printed raw between quotes (`"%s"`, no quoting).
        self.out.print(format_args!(
            "(kicad_footprint_lib (version {}) (generator \"kicad_footprint_editor\") \
             (generator_version \"{}\")\n",
            SEXPR_FOOTPRINT_LIB_FILE_VERSION, self.generator_version
        ))?;
        self.format_footprint(footprint, true)?;
        self.out.print(format_args!(")\n"))?;

        Ok(())
    }
}

impl Board {
    /// The pure normalisation steps of `PCB_IO_KICAD_SEXPR::FootprintSave` on the footprint `fp`
    /// (W:4176-4202), applied **in place**: the C++ works on a clone (`aFootprint->Clone()`), the
    /// port on a board the caller owns for the save (a copy of the board, or the holder board of a
    /// library footprint).
    ///
    /// * the orientation becomes zero;
    /// * a footprint not on `F_Cu` is flipped about its position with `flip_direction` (the C++
    ///   takes `PCBNEW_SETTINGS::m_FlipDirection`, or `TOP_BOTTOM` without settings) and its
    ///   orientation set to zero again (a left-right flip also turns the footprint half a turn);
    /// * it leaves its group (`SetParentGroup( nullptr )`; here `remove_from_group`, which also
    ///   drops it from the group's member list of that board);
    /// * `ClearAllNets()`: every connected descendant takes the orphaned net.
    ///
    /// `SetParent( nullptr )` has no counterpart: a footprint never exists outside a board
    /// (`board-model.md` 2), the writer leaves `m_board` null for library files anyway. The file
    /// system part (cache validation, file naming, `FP_CACHE::Save`) is the host's.
    pub fn footprint_save_normalise(&mut self, fp: FootprintId, flip_direction: FlipDirection) {
        let board = self;
        if board.footprint(fp).is_none() {
            return;
        }

        // It's orientation should be zero and it should be on the front layer.
        board.footprint_set_orientation(fp, ANGLE_0);

        let (layer, position) = {
            let ctx = crate::ctx::BoardCtx::bare(board);
            let footprint = &board[fp];
            (footprint.layer(&ctx), footprint.position(&ctx))
        };

        if layer != PcbLayerId::FCu {
            board.footprint_flip(fp, position, flip_direction);

            // A left-right flip also turns the footprint half a turn
            board.footprint_set_orientation(fp, ANGLE_0);
        }

        // Detach it from the board and its group
        board.remove_from_group(fp.into());

        // Force every connected descendant to the board-independent ORPHANED net before
        // serialization (FOOTPRINT::ClearAllNets).
        let mut children: Vec<ItemId> = Vec::new();
        {
            let ctx = crate::ctx::BoardCtx::bare(board);
            board[fp].run_on_children(&ctx, RecurseMode::Recurse, &mut |child| {
                children.extend(child.id())
            });
        }

        for child in children {
            let connected = board
                .get(child)
                .is_some_and(|item| item.as_connected().is_some());

            if connected {
                board.set_item_net(child, NetCode::ORPHANED, true);
            }
        }
    }
}
