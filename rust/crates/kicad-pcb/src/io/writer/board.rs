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

//! Port of the board skeleton of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp`
//! (package PCBIO-02): `FormatBoardToFormatter`, `formatGeneral`, `formatBoardLayers`,
//! `formatProperties`, `formatVariants`, `formatHeader` and `format( const BOARD* )` with its
//! section order, the sorted containers and the consolidated embedded files.
//!
//! The save-order comparators are model code (`board_item_ptr_cmp`, `board_cmp_drawings`,
//! `pcb_track_cmp_tracks`, `pcb_point_cmp_points`); the `std::set`s built from them are
//! [`sorted_set`].

use kicad_core::layer_ids::is_copper_layer;
use kicad_core::{Lset, PcbLayerId};
use kicad_sexpr::{IoError, format_bool};

use super::{CTL_FOR_BOARD, PcbWriter, format_iu, sorted_set};
use crate::board::Board;
use crate::board::LayerT;
use crate::board::compare::board_cmp_drawings;
use crate::board_item::board_item_ptr_cmp;
use crate::ctx::BoardCtx;
use crate::embedded_files::{EmbeddedFileType, EmbeddedFiles};
use crate::ids::{ItemId, TrackId};
use crate::io::SEXPR_BOARD_FILE_VERSION;
use crate::pcb_point::pcb_point_cmp_points;
use crate::pcb_track::pcb_track_cmp_tracks;

impl PcbWriter<'_> {
    /// Port of `PCB_IO_KICAD_SEXPR::FormatBoardToFormatter` (after the caller made the
    /// formatter): `(kicad_pcb (version N) (generator "pcbnew") (generator_version "X") ...)`.
    ///
    /// The C++ mutates the board first: `EmbedFonts()` when fonts are embedded (needs font
    /// files, `pcb-io.md` R7: a no-op here, the embedded font files already present are kept),
    /// else `GetEmbeddedFiles()->ClearEmbeddedFonts()`, which the port applies to the output
    /// only ([`PcbWriter::board_fonts_cleared`]).
    pub(crate) fn format_board_to_formatter(&mut self, board: &Board) -> Result<(), IoError> {
        // init()
        self.board = None;

        self.board = Some(self.src); // after init()
        debug_assert!(std::ptr::eq(self.src, board));

        // If the user wants fonts embedded, make sure that they are added to the board.
        // Otherwise, remove any fonts that were previously embedded.
        if board.embedded_files.are_fonts_embedded() {
            // m_board->EmbedFonts(): no font files in the port (R7), keep what is embedded.
        } else {
            self.board_fonts_cleared = true;
        }

        let version = self.out.quotew(&self.generator_version);
        self.out.print(format_args!(
            "(kicad_pcb (version {}) (generator \"pcbnew\") (generator_version {})",
            SEXPR_BOARD_FILE_VERSION, version
        ))?;

        self.format_board(board)?;

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatGeneral`: `(general (thickness ..) (legacy_teardrops
    /// ..))`, then the page settings and the title block.
    pub(crate) fn format_general(&mut self, board: &Board) -> Result<(), IoError> {
        let bds = board.design_settings();

        self.out.print(format_args!("(general"))?;

        self.out.print(format_args!(
            "(thickness {})",
            format_iu(bds.board_thickness())
        ))?;

        format_bool(&mut *self.out, "legacy_teardrops", board.legacy_teardrops)?;

        self.out.print(format_args!(")"))?;

        board.paper.format(&mut *self.out)?;
        board.titles.format(&mut *self.out)?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatBoardLayers`: the enabled copper layers from front to
    /// back, then the enabled non-copper layers in `TechAndUserUIOrder()`.
    pub(crate) fn format_board_layers(&mut self, board: &Board) -> Result<(), IoError> {
        self.out.print(format_args!("(layers"))?;

        // Save only the used copper layers from front to back.

        for &layer in board.enabled_layers().cu_stack().iter() {
            let canonical = Lset::name(layer);
            let user_name = board.layer_name(layer);
            let quoted_name = self.out.quotew(&canonical);
            let quoted_user = if canonical == user_name {
                String::new()
            } else {
                self.out.quotew(&user_name)
            };

            self.out.print(format_args!(
                "({} {} {} {})",
                layer.0,
                quoted_name,
                board.layer_type(layer).show_type(),
                quoted_user
            ))?;
        }

        // Save used non-copper layers in the order they are defined.
        let seq = board.enabled_layers().tech_and_user_ui_order();

        for &layer in seq.iter() {
            let mut print_type = false;

            // User layers (layer id >= User_1) have a qualifier
            // default is "user", but other qualifiers exist
            if layer.0 >= PcbLayerId::User1.0 {
                if is_copper_layer(layer) {
                    print_type = true;
                }

                if board.layer_type(layer) == LayerT::Front
                    || board.layer_type(layer) == LayerT::Back
                {
                    print_type = true;
                }
            }

            let canonical = Lset::name(layer);
            let user_name = board.layer_name(layer);
            let quoted_name = self.out.quotew(&canonical);
            let quoted_user = if user_name == canonical {
                String::new()
            } else {
                self.out.quotew(&user_name)
            };

            self.out.print(format_args!(
                "({} {} {} {})",
                layer.0,
                quoted_name,
                if print_type {
                    board.layer_type(layer).show_type()
                } else {
                    "user"
                },
                quoted_user
            ))?;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatProperties`: `(property "key" "value")` per board
    /// property, in key order (`std::map`).
    pub(crate) fn format_properties(&mut self, board: &Board) -> Result<(), IoError> {
        for (key, value) in &board.properties {
            let key = self.out.quotew(key);
            let value = self.out.quotew(value);
            self.out.print(format_args!("(property {key} {value})"))?;
        }

        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatVariants`: the board variant registry, in
    /// `GetVariantNames()` order; nothing without variants.
    pub(crate) fn format_variants(&mut self, board: &Board) -> Result<(), IoError> {
        let variant_names = &board.variant_names;

        if variant_names.is_empty() {
            return Ok(());
        }

        self.out.print(format_args!("(variants"))?;

        for variant_name in variant_names {
            let quoted = self.out.quotew(variant_name);
            self.out.print(format_args!("(variant (name {quoted})"))?;

            let description = board.get_variant_description(variant_name);

            if !description.is_empty() {
                let quoted = self.out.quotew(&description);
                self.out.print(format_args!("(description {quoted})"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatHeader`: writes everything that comes before the
    /// board items, like settings and layers etc.
    pub(crate) fn format_header(&mut self, board: &Board) -> Result<(), IoError> {
        self.format_general(board)?;

        // Layers list.
        self.format_board_layers(board)?;

        // Setup
        self.format_setup(board)?;

        // Properties
        self.format_properties(board)?;

        // Variants
        self.format_variants(board)?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::format( const BOARD* aBoard )`: header, footprints,
    /// drawings, points, tracks, zones, groups, generators, constraints (each in the order of
    /// its `std::set`), net chains, `(embedded_fonts ..)` and the consolidated embedded files.
    pub(crate) fn format_board(&mut self, board: &Board) -> Result<(), IoError> {
        let ctx = BoardCtx::bare(board);

        let sorted_footprints: Vec<ItemId> = sorted_set(
            board.footprints().map(|(id, _)| ItemId::from(id)),
            |a, b| board_item_ptr_cmp(&ctx, *a, *b),
        );
        let sorted_drawings: Vec<ItemId> =
            sorted_set(board.drawings().map(|(id, _)| id), |a, b| {
                board_cmp_drawings(&ctx, *a, *b)
            });
        let sorted_tracks: Vec<TrackId> = sorted_set(board.tracks().map(|(id, _)| id), |a, b| {
            pcb_track_cmp_tracks(&ctx, *a, *b)
        });
        let sorted_points: Vec<ItemId> = sorted_set(
            board.board_points().map(|(id, _)| ItemId::from(id)),
            |a, b| pcb_point_cmp_points(&ctx, *a, *b),
        );
        let sorted_zones: Vec<ItemId> =
            sorted_set(board.zones().map(|(id, _)| ItemId::from(id)), |a, b| {
                board_item_ptr_cmp(&ctx, *a, *b)
            });
        let sorted_groups: Vec<ItemId> =
            sorted_set(board.groups().map(|(id, _)| ItemId::from(id)), |a, b| {
                board_item_ptr_cmp(&ctx, *a, *b)
            });
        let sorted_generators: Vec<ItemId> = sorted_set(
            board.generators().map(|(id, _)| ItemId::from(id)),
            |a, b| board_item_ptr_cmp(&ctx, *a, *b),
        );
        let sorted_constraints: Vec<ItemId> = sorted_set(
            board.constraints().map(|(id, _)| ItemId::from(id)),
            |a, b| board_item_ptr_cmp(&ctx, *a, *b),
        );

        self.format_header(board)?;

        // Save the footprints.
        for &footprint in &sorted_footprints {
            self.format_item(footprint)?;
        }

        // Save the graphical items on the board (not owned by a footprint)
        for &item in &sorted_drawings {
            self.format_item(item)?;
        }

        // Save the points
        for &point in &sorted_points {
            self.format_item(point)?;
        }

        // Do not save PCB_MARKERs, they can be regenerated easily.

        // Save the tracks and vias.
        for &track in &sorted_tracks {
            self.format_item(ItemId::from(track))?;
        }

        // Save the polygon (which are the newer technology) zones.
        for &zone in &sorted_zones {
            self.format_item(zone)?;
        }

        // Save the groups
        for &group in &sorted_groups {
            self.format_item(group)?;
        }

        // Save the generators
        for &generator in &sorted_generators {
            self.format_item(generator)?;
        }

        // Save the geometric constraints last, after every item they may reference.
        for &constraint in &sorted_constraints {
            self.format_item(constraint)?;
        }

        // After writing all items, write the aggregated net chains section (if any)
        self.format_net_chains(board)?;

        // Save any embedded files
        // Consolidate the embedded models in footprints into a single map
        // to avoid duplicating the same model in the board file.
        let mut files_to_write = EmbeddedFiles::default();

        for file in board.embedded_files.embedded_file_map().values() {
            // ClearEmbeddedFonts() of FormatBoardToFormatter, applied here.
            if self.board_fonts_cleared && file.type_ == EmbeddedFileType::Font {
                continue;
            }

            files_to_write.add_file(file.clone());
        }

        for &item in &sorted_footprints {
            let Some(fp) = item.typed().and_then(|id| board.footprint(id)) else {
                continue;
            };

            for file in fp.embedded_files.embedded_file_map().values() {
                files_to_write.add_file(file.clone());
            }
        }

        self.out.print(format_args!(
            "(embedded_fonts {})",
            if board.embedded_files.are_fonts_embedded() {
                "yes"
            } else {
                "no"
            }
        ))?;

        if !files_to_write.is_empty() {
            files_to_write.write_embedded_files(&mut *self.out, (self.ctl & CTL_FOR_BOARD) != 0)?;
        }

        // Remove the files so that they are not freed in the DTOR: the shared entries are
        // reference counted here.
        Ok(())
    }
}
