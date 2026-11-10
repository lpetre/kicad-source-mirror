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

//! Port of the writer half of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` and
//! `pcb_io_kicad_sexpr.h` (`PCB_IO_KICAD_SEXPR` as a formatter), plus the `CTL_*` flags of
//! `include/ctl_flags.h`: package PCBIO-02 owns this file and `board.rs`.
//!
//! This file holds the writer state ([`PcbWriter`]: `m_out`, `m_board`, `m_ctl`), the
//! `Format( const BOARD_ITEM* )` dispatch, the free formatting helpers (`formatInternalUnits`
//! overloads, `unbakeSize`, `unbakeSizeUniform`, `unbakeLinear`) and the member helpers
//! `formatLayer`, `formatLayers` and `formatPolyPts`. The entry points [`format_board`] /
//! [`format_board_report`] port `PCB_IO_KICAD_SEXPR::SaveBoard` (file output replaced by the
//! returned text).
//!
//! The writer emits the same token stream as the C++ (one Rust statement per `Print`), and the
//! layout of the file is decided only by `KICAD_FORMAT::Prettify`
//! (`kicad_sexpr::PrettifiedOutputFormatter`), exactly like `PRETTIFIED_FILE_OUTPUTFORMATTER`
//! (`docs/analysis/pcb-io.md` 6.1).
//!
//! One file per construct group, each owned by its work package (`pcb-io.md` 10.2):
//!
//! | file | package | C++ |
//! |---|---|---|
//! | `mod.rs`, `board.rs` | PCBIO-02 | `SaveBoard`, `FormatBoardToFormatter`, `Format`, helpers, `formatGeneral`, `formatBoardLayers`, `formatProperties`, `formatVariants`, `formatHeader`, `format( BOARD* )` |
//! | `tracks.rs`, `shapes.rs` | PCBIO-03 | tracks, arcs, vias, teardrop parameters / graphic shapes |
//! | `setup.rs` | PCBIO-04 | `formatSetup`, `formatDrillSymbolProfile` (stackup, plot parameters) |
//! | `text.rs` | PCBIO-05 | text, text box, render cache |
//! | `footprint.rs` | PCBIO-06 | footprint, initial comments |
//! | `pad.rs` | PCBIO-07 | pad |
//! | `zone.rs` | PCBIO-08 | zone, zone layer properties |
//! | `group.rs` | PCBIO-09 | group |
//! | `dimension.rs` | PCBIO-10 | dimension |
//! | `netclass_embedded.rs` | PCBIO-11 | net chains |
//! | `misc_items.rs` | PCBIO-12 | reference image, barcode, target, point, grid item, tables, generated tables, drill map |
//! | `generator.rs` | PCBIO-13 | generator, constraint |
//! | `fp_lib.rs` | PCBIO-14 | `FormatFootprintLibFile` |
//!
//! Until a package lands, its functions are **staging stubs** ([`PcbWriter::pending`]): with
//! `FormatOptions::allow_pending == false` they fail with "port pending: <item kind>
//! (PCBIO-nn)", with `allow_pending == true` they write nothing and record the item in
//! [`FormatReport::skipped`].

// The helper API and the staging stubs are consumed by the PCBIO packages as they land.
#![allow(dead_code)]

mod board;
mod dimension;
mod footprint;
mod fp_lib;
mod generator;
mod group;
mod misc_items;
mod netclass_embedded;
mod pad;
mod setup;
mod shapes;
mod text;
mod tracks;
mod zone;

#[cfg(test)]
mod tests;

use std::cmp::Ordering;

use kicad_core::eda_units::{format_internal_units, format_internal_units_point};
use kicad_core::{EdaDataType, Lset, MAX_CU_LAYERS, PCB_IU_SCALE, PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_geom::{ShapeLineChain, Vector2I, ki_round};
use kicad_sexpr::{FormatMode, IoError, OutputFormatter, PrettifiedOutputFormatter};

use crate::board::Board;
use crate::ctx::BoardCtx;
use crate::footprint::Footprint;
use crate::ids::{FootprintId, ItemId};

// ---- include/ctl_flags.h ------------------------------------------------------------------
//
// These flags are used to control the data items that must be disabled when creating mainly a
// netlist but also some other KiCad files. They allow skipping specified data in these files.

/// `CTL_OMIT_EXTRA`.
pub const CTL_OMIT_EXTRA: i32 = 1 << 0;
/// `CTL_OMIT_NETS`.
pub const CTL_OMIT_NETS: i32 = 1 << 1;
/// `CTL_OMIT_PAD_NETS`: omit pads net names (useless in library).
pub const CTL_OMIT_PAD_NETS: i32 = 1 << 1;
/// `CTL_OMIT_UUIDS`: omit component unique ids (useless in library).
pub const CTL_OMIT_UUIDS: i32 = 1 << 2;
/// `CTL_OMIT_FP_UUID`: don't prefix the footprint UUID to the sheet path.
pub const CTL_OMIT_FP_UUID: i32 = 1 << 3;
/// `CTL_OMIT_PATH`: omit component sheet time stamp (useless in library).
pub const CTL_OMIT_PATH: i32 = 1 << 4;
/// `CTL_OMIT_AT`: omit position and rotation (always saved with position 0,0 and rotation = 0
/// in library).
pub const CTL_OMIT_AT: i32 = 1 << 5;
/// `CTL_OMIT_LIBNAME`: omit lib alias when saving (used for board/not library).
pub const CTL_OMIT_LIBNAME: i32 = 1 << 7;
/// `CTL_OMIT_FOOTPRINT_VERSION`: omit the version string from the (footprint) sexpr group.
pub const CTL_OMIT_FOOTPRINT_VERSION: i32 = 1 << 8;
/// `CTL_OMIT_FILTERS`: omit the ki_fp_filters attribute in .kicad_xxx files.
pub const CTL_OMIT_FILTERS: i32 = 1 << 9;
/// `CTL_OMIT_INITIAL_COMMENTS`: omit #FOOTPRINT initial comments.
pub const CTL_OMIT_INITIAL_COMMENTS: i32 = 1 << 10;
/// `CTL_OMIT_COLOR`: omit the color attribute in .kicad_xxx files.
pub const CTL_OMIT_COLOR: i32 = 1 << 11;
/// `CTL_OMIT_HYPERLINK`: omit the hyperlink attribute in .kicad_xxx files.
pub const CTL_OMIT_HYPERLINK: i32 = 1 << 12;

// common combinations of the above (pcb_io_kicad_sexpr.h):

/// `CTL_FOR_CLIPBOARD`: format output for the clipboard instead of footprint library or BOARD.
pub const CTL_FOR_CLIPBOARD: i32 = CTL_OMIT_INITIAL_COMMENTS; // (CTL_OMIT_NETS)

/// `CTL_FOR_LIBRARY`: format output for a footprint library instead of clipboard or BOARD.
pub const CTL_FOR_LIBRARY: i32 =
    CTL_OMIT_PAD_NETS | CTL_OMIT_UUIDS | CTL_OMIT_PATH | CTL_OMIT_AT | CTL_OMIT_LIBNAME;

/// `CTL_FOR_BOARD`: the zero arg constructor when the plugin is used for `Load()` and `Save()`
/// of a BOARD file.
pub const CTL_FOR_BOARD: i32 = CTL_OMIT_INITIAL_COMMENTS | CTL_OMIT_FOOTPRINT_VERSION;

/// `GetMajorMinorVersion()` of the reference build (`KICAD_MAJOR_MINOR_VERSION`,
/// `cmake/KiCadVersion.cmake`: `10.99.0-unknown`), written as `(generator_version "10.99")`.
/// Confirmed against `kicad-cli pcb upgrade` of the oracle.
pub const KICAD_MAJOR_MINOR_VERSION: &str = "10.99";

/// Options of a save.
#[derive(Clone, Debug)]
pub struct FormatOptions {
    /// `m_ctl`, the control flags given to the `PCB_IO_KICAD_SEXPR` constructor
    /// (default `CTL_FOR_BOARD`).
    pub ctl: i32,
    /// The mode given to `PRETTIFIED_FILE_OUTPUTFORMATTER` (`KICAD_FORMAT::FORMAT_MODE::NORMAL`
    /// for boards).
    pub format_mode: FormatMode,
    /// `ADVANCED_CFG::m_CompactSave` (advanced config key `CompactSave`, default false): turns
    /// the `NORMAL` mode into `COMPACT_TEXT_PROPERTIES`.
    pub compact_save: bool,
    /// `GetMajorMinorVersion()`, written as `generator_version`.
    pub generator_version: String,
    /// Staging switch while the PCBIO packages land: when `true`, an item whose formatter is
    /// still a staging stub is not written and is recorded in [`FormatReport::skipped`] instead
    /// of failing the save with "port pending: ...".
    pub allow_pending: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        FormatOptions {
            ctl: CTL_FOR_BOARD,
            format_mode: FormatMode::Normal,
            compact_save: false,
            generator_version: KICAD_MAJOR_MINOR_VERSION.to_string(),
            allow_pending: false,
        }
    }
}

/// One item not written because its formatter is a staging stub
/// (`FormatOptions::allow_pending`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkippedFormat {
    /// What was skipped, e.g. `"segment"` or `"setup"`.
    pub item: String,
    /// The package that will port it, e.g. `"PCBIO-03"`.
    pub package: &'static str,
}

/// What a save reports next to the text.
#[derive(Clone, Debug, Default)]
pub struct FormatReport {
    /// Items skipped by staging stubs, in output order.
    pub skipped: Vec<SkippedFormat>,
}

/// The formatter state of `PCB_IO_KICAD_SEXPR` (`m_out`, `m_board`, `m_ctl`).
pub struct PcbWriter<'a> {
    /// `m_out`: everything is printed into it.
    pub(crate) out: &'a mut dyn OutputFormatter,
    /// The board that holds the items being formatted (for a library footprint: the holder
    /// board, `board-model.md` 2). Items are looked up here.
    pub(crate) src: &'a Board,
    /// A bare context over [`src`](Self::src) for model getters and the save-order comparators.
    pub(crate) ctx: BoardCtx<'a>,
    /// `m_board`: set by `FormatBoardToFormatter` after `init()`; `None` when formatting a
    /// library footprint (several branches depend on it).
    pub(crate) board: Option<&'a Board>,
    /// `m_ctl`.
    pub(crate) ctl: i32,
    /// `FormatOptions::allow_pending`.
    pub(crate) allow_pending: bool,
    /// `FormatOptions::generator_version`.
    pub(crate) generator_version: String,
    /// `FormatBoardToFormatter` calls `m_board->GetEmbeddedFiles()->ClearEmbeddedFonts()` when
    /// the board does not embed its fonts; the port keeps the board immutable and records here
    /// that the board's font files count as removed for the rest of the save.
    pub(crate) board_fonts_cleared: bool,
    /// Items skipped by staging stubs.
    pub(crate) report: FormatReport,
}

impl<'a> PcbWriter<'a> {
    /// Port of `PCB_IO_KICAD_SEXPR( int aControlFlags )` + `SetOutputFormatter`: a writer
    /// printing into `out`, whose items live in `src`. `m_board` starts null (`init()`).
    pub fn new(out: &'a mut dyn OutputFormatter, src: &'a Board, options: &FormatOptions) -> Self {
        PcbWriter {
            out,
            src,
            ctx: BoardCtx::bare(src),
            board: None,
            ctl: options.ctl,
            allow_pending: options.allow_pending,
            generator_version: options.generator_version.clone(),
            board_fonts_cleared: false,
            report: FormatReport::default(),
        }
    }

    /// The report of what staging stubs skipped so far.
    pub fn report(&self) -> &FormatReport {
        &self.report
    }

    /// Take the report out of the writer.
    pub fn into_report(self) -> FormatReport {
        self.report
    }

    /// Staging stub helper of the PCBIO packages: fails with "port pending: <item> (<package>)"
    /// unless `allow_pending`, in which case nothing is written and the item is recorded.
    pub(crate) fn pending(&mut self, item: &str, package: &'static str) -> Result<(), IoError> {
        if !self.allow_pending {
            return Err(IoError::new(format!("port pending: {item} ({package})")));
        }

        self.report.skipped.push(SkippedFormat {
            item: item.to_string(),
            package,
        });
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::Format( const BOARD_ITEM* aItem )`: output `item` in
    /// s-expression format (dispatch on `Type()`).
    ///
    /// # Errors
    ///
    /// An [`IoError`] on write error, or "port pending: ..." from a staging stub.
    pub fn format_item(&mut self, item: ItemId) -> Result<(), IoError> {
        use kicad_core::KicadT as K;

        let Some(item_ref) = self.ctx.item(item) else {
            debug_assert!(false, "Cannot format item {item:?}: not in the board");
            return Ok(());
        };

        match item_ref.type_() {
            K::PcbDimAlignedT
            | K::PcbDimCenterT
            | K::PcbDimRadialT
            | K::PcbDimOrthogonalT
            | K::PcbDimLeaderT => self.format_dimension(typed(item)?),

            K::PcbShapeT => self.format_shape(typed(item)?),

            K::PcbReferenceImageT => self.format_reference_image(typed(item)?),

            K::PcbPointT => self.format_point(typed(item)?),

            K::PcbTargetT => self.format_target(typed(item)?),

            K::PcbGridItemT => self.format_grid_item(typed(item)?),

            K::PcbFootprintT => self.format_footprint(typed(item)?, false),

            K::PcbPadT => self.format_pad(typed(item)?),

            // Handled in the footprint formatter when properties are formatted
            K::PcbFieldT => Ok(()),

            K::PcbTextT => self.format_text(typed(item)?),

            K::PcbTextboxT => self.format_textbox(typed(item)?),

            K::PcbBarcodeT => self.format_barcode(typed(item)?),

            K::PcbTableT => self.format_table(typed(item)?),

            K::PcbDrillChartT | K::PcbStackupTableT => self.format_generated_table(typed(item)?),

            K::PcbDrillMapT => self.format_drill_map(typed(item)?),

            K::PcbGroupT => self.format_group(typed(item)?),

            K::PcbGeneratorT => self.format_generator(typed(item)?),

            K::PcbConstraintT => self.format_constraint(typed(item)?),

            K::PcbTraceT | K::PcbArcT | K::PcbViaT => self.format_track(typed(item)?),

            K::PcbZoneT => self.format_zone(typed(item)?),

            other => {
                // wxFAIL_MSG( wxT( "Cannot format item " ) + aItem->GetClass() )
                debug_assert!(false, "Cannot format item {other:?}");
                Ok(())
            }
        }
    }

    // ---- helpers ------------------------------------------------------------------------------

    /// Port of `PCB_IO_KICAD_SEXPR::formatLayer( PCB_LAYER_ID aLayer, bool aIsKnockout )`:
    /// `(layer "name" [knockout])`.
    pub(crate) fn format_layer(
        &mut self,
        layer: PcbLayerId,
        is_knockout: bool,
    ) -> Result<(), IoError> {
        let name = self.out.quotew(&Lset::name(layer));
        self.out.print(format_args!(
            "(layer {} {})",
            name,
            if is_knockout { "knockout" } else { "" }
        ))?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatPolyPts( const SHAPE_LINE_CHAIN& outline,
    /// const FOOTPRINT* aParentFP )`: `(pts (xy ..)(arc (start ..) (mid ..) (end ..)) ...)`;
    /// with a parent footprint the points are un-baked into the library frame.
    pub(crate) fn format_poly_pts(
        &mut self,
        outline: &ShapeLineChain,
        parent_fp: Option<&Footprint>,
    ) -> Result<(), IoError> {
        self.out.print(format_args!("(pts"))?;

        let mut ii = 0;

        while ii < outline.point_count() {
            let ind = outline.arc_index(ii as usize);

            if ind < 0 {
                self.out.print(format_args!(
                    "(xy {})",
                    format_internal_units_pt_fp(outline.c_point(ii), parent_fp)
                ))?;
            } else {
                let arc = outline.arc(ind as usize);
                self.out.print(format_args!(
                    "(arc (start {}) (mid {}) (end {}))",
                    format_internal_units_pt_fp(arc.get_p0(), parent_fp),
                    format_internal_units_pt_fp(arc.get_arc_mid(), parent_fp),
                    format_internal_units_pt_fp(arc.get_p1(), parent_fp)
                ))?;

                loop {
                    ii += 1;

                    if !(ii < outline.point_count() && outline.arc_index(ii as usize) == ind) {
                        break;
                    }
                }

                ii -= 1;
            }

            ii += 1;
        }

        self.out.print(format_args!(")"))?;
        Ok(())
    }

    /// Port of `PCB_IO_KICAD_SEXPR::formatLayers( LSET aLayerMask, bool aEnumerateLayers,
    /// bool aIsZone )`: `(layers ...)` with the wildcards `*.Cu` (every copper layer of the
    /// board, or all 32 without a board), `F&B.Cu` (zones), `*.Adhes`, `*.Paste`, `*.SilkS`,
    /// `*.Mask`, `*.CrtYd`, `*.Fab`, then the remaining layers by ascending id.
    pub(crate) fn format_layers(
        &mut self,
        mut layer_mask: Lset,
        enumerate_layers: bool,
        is_zone: bool,
    ) -> Result<(), IoError> {
        let cu_all = Lset::all_cu_mask().clone();
        let fr_bk = Lset::from_layers(&[PcbLayerId::BCu, PcbLayerId::FCu]);
        let adhes = Lset::from_layers(&[PcbLayerId::BAdhes, PcbLayerId::FAdhes]);
        let paste = Lset::from_layers(&[PcbLayerId::BPaste, PcbLayerId::FPaste]);
        let silks = Lset::from_layers(&[PcbLayerId::BSilkS, PcbLayerId::FSilkS]);
        let mask = Lset::from_layers(&[PcbLayerId::BMask, PcbLayerId::FMask]);
        let crt_yd = Lset::from_layers(&[PcbLayerId::BCrtYd, PcbLayerId::FCrtYd]);
        let fab = Lset::from_layers(&[PcbLayerId::BFab, PcbLayerId::FFab]);

        let cu_board_mask = Lset::all_cu_mask_n(match self.board {
            Some(board) => board.copper_layer_count(),
            None => MAX_CU_LAYERS,
        });

        let mut output = String::new();

        if !enumerate_layers {
            // If all copper layers present on the board are enabled, then output the wildcard
            if (&layer_mask & &cu_board_mask) == cu_board_mask {
                output.push(' ');
                output += &self.out.quotew("*.Cu");

                // Clear all copper bits because pads might have internal layers that aren't
                // part of the board enabled, and we don't want to output those in the layers
                // listing if we already output the wildcard.
                layer_mask &= &!&cu_all;
            } else if (&layer_mask & &cu_board_mask) == fr_bk {
                output.push(' ');

                if is_zone {
                    output += &self.out.quotew("F&B.Cu");
                } else {
                    output += &self.out.quotew("*.Cu");
                }

                layer_mask &= &!&fr_bk;
            }

            for (pair, wildcard) in [
                (&adhes, "*.Adhes"),
                (&paste, "*.Paste"),
                (&silks, "*.SilkS"),
                (&mask, "*.Mask"),
                (&crt_yd, "*.CrtYd"),
                (&fab, "*.Fab"),
            ] {
                if (&layer_mask & pair) == *pair {
                    output.push(' ');
                    output += &self.out.quotew(wildcard);
                    layer_mask &= &!pair;
                }
            }
        }

        // output any individual layers not handled in wildcard combos above
        for layer in 0..PCB_LAYER_ID_COUNT {
            if layer_mask.contains(PcbLayerId(layer)) {
                output.push(' ');
                output += &self.out.quotew(&Lset::name(PcbLayerId(layer)));
            }
        }

        self.out.print(format_args!("(layers {output})"))?;
        Ok(())
    }

    /// The parent footprint of a footprint child, by handle (`GetParentFootprint()`).
    pub(crate) fn footprint(&self, id: FootprintId) -> Option<&'a Footprint> {
        self.src.footprint(id)
    }
}

/// The typed handle of `item`; the dispatch only calls it for the matching kind.
fn typed<T: crate::ids::ArenaItem>(item: ItemId) -> Result<crate::ids::Id<T>, IoError> {
    item.typed::<T>()
        .ok_or_else(|| IoError::new(format!("Cannot format item {item:?}: unexpected kind")))
}

/// Port of the static `formatInternalUnits( const int aValue, const EDA_DATA_TYPE aDataType =
/// EDA_DATA_TYPE::DISTANCE )` of the writer: `EDA_UNIT_UTILS::FormatInternalUnits( pcbIUScale,
/// .. )`.
pub(crate) fn format_internal_units_dt(value: i32, data_type: EdaDataType) -> String {
    format_internal_units(&PCB_IU_SCALE, value, data_type)
}

/// `formatInternalUnits( int )` with the default `EDA_DATA_TYPE::DISTANCE`.
pub(crate) fn format_iu(value: i32) -> String {
    format_internal_units_dt(value, EdaDataType::Distance)
}

/// Port of `formatInternalUnits( const VECTOR2I& aCoord )`: `"x y"`.
pub(crate) fn format_iu_pt(coord: Vector2I) -> String {
    format_internal_units_point(&PCB_IU_SCALE, coord.x, coord.y)
}

/// Port of `formatInternalUnits( const VECTOR2I& aCoord, const FOOTPRINT* aParentFP )`: with a
/// parent footprint the point is un-baked first (`GetTransform().InverseApply( aCoord )`).
pub(crate) fn format_internal_units_pt_fp(
    coord: Vector2I,
    parent_fp: Option<&Footprint>,
) -> String {
    if let Some(fp) = parent_fp {
        return format_iu_pt(fp.transform().inverse_apply(coord));
    }

    format_iu_pt(coord)
}

/// Port of the static `unbakeSize( const VECTOR2I& aSize, const FOOTPRINT* aParentFP )`:
/// divide by the footprint scale per axis.
pub(crate) fn unbake_size(size: Vector2I, parent_fp: Option<&Footprint>) -> Vector2I {
    let Some(fp) = parent_fp else {
        return size;
    };

    let xform = fp.transform();
    Vector2I::new(
        ki_round(f64::from(size.x) / xform.scale_x()),
        ki_round(f64::from(size.y) / xform.scale_y()),
    )
}

/// Port of the static `unbakeSizeUniform( const VECTOR2I& aSize, const FOOTPRINT* aParentFP )`:
/// divide by the mean footprint scale.
pub(crate) fn unbake_size_uniform(size: Vector2I, parent_fp: Option<&Footprint>) -> Vector2I {
    let Some(fp) = parent_fp else {
        return size;
    };

    let xform = fp.transform();
    let avg = (xform.scale_x() + xform.scale_y()) * 0.5;
    Vector2I::new(
        ki_round(f64::from(size.x) / avg),
        ki_round(f64::from(size.y) / avg),
    )
}

/// Port of the static `unbakeLinear( int aValue, const FOOTPRINT* aParentFP )`: divide by the
/// mean footprint scale.
pub(crate) fn unbake_linear(value: i32, parent_fp: Option<&Footprint>) -> i32 {
    let Some(fp) = parent_fp else {
        return value;
    };

    let xform = fp.transform();
    let avg = (xform.scale_x() + xform.scale_y()) * 0.5;
    ki_round(f64::from(value) / avg)
}

/// The content of a C++ `std::set<T*, CMP>` built from a container (`std::set( begin, end )`):
/// the items in comparator order, an item comparing equal to one inserted before it being
/// dropped. With a strict weak ordering a stable sort followed by dropping consecutive
/// equivalent items gives exactly that set.
pub(crate) fn sorted_set<T: Copy>(
    items: impl IntoIterator<Item = T>,
    mut cmp: impl FnMut(&T, &T) -> Ordering,
) -> Vec<T> {
    let mut v: Vec<T> = items.into_iter().collect();
    v.sort_by(|a, b| cmp(a, b));
    v.dedup_by(|later, kept| cmp(kept, later) == Ordering::Equal);
    v
}

// ---- entry points --------------------------------------------------------------------------

/// Port of `PCB_IO_KICAD_SEXPR::FormatBoardToFormatter`: print the whole board into `out`
/// (raw token stream; prettification is the formatter's business).
///
/// # Errors
///
/// An [`IoError`] on write error, or "port pending: ..." from a staging stub (unless
/// `options.allow_pending`).
pub fn format_board_to_formatter(
    out: &mut dyn OutputFormatter,
    board: &Board,
    options: &FormatOptions,
) -> Result<FormatReport, IoError> {
    let mut writer = PcbWriter::new(out, board, options);
    writer.format_board_to_formatter(board)?;
    Ok(writer.into_report())
}

/// Save a board as `.kicad_pcb` text: port of `PCB_IO_KICAD_SEXPR::SaveBoard` (the
/// `PRETTIFIED_FILE_OUTPUTFORMATTER`, `FormatBoardToFormatter`, `Finish()`); the text is
/// returned instead of being written to a file.
///
/// `GroupsSanityCheck()` is not called: its result is only used with a query-user callback
/// (GUI), and without `repair` it has no side effect.
///
/// # Errors
///
/// An [`IoError`] on write error, or "port pending: <item kind> (PCBIO-nn)" for items whose
/// formatter package has not landed yet (unless `options.allow_pending`).
pub fn format_board(board: &Board, options: &FormatOptions) -> Result<String, IoError> {
    format_board_report(board, options).map(|(text, _)| text)
}

/// [`format_board`] returning the [`FormatReport`] (items skipped by staging stubs with
/// `options.allow_pending`) next to the text.
pub fn format_board_report(
    board: &Board,
    options: &FormatOptions,
) -> Result<(String, FormatReport), IoError> {
    let mut formatter = PrettifiedOutputFormatter::new(options.format_mode, options.compact_save);
    let report = format_board_to_formatter(&mut formatter, board, options)?;
    formatter.finish()?;
    Ok((formatter.into_string(), report))
}

/// Port of `PCB_IO_KICAD_SEXPR::FormatFootprintLibFile` as a save of a `.kicad_mod` file: the
/// footprint `fp` of the holder board `holder` (`board-model.md` 2), prettified like
/// `FP_CACHE::Save`. Staging stub of PCBIO-14 (see `fp_lib.rs`).
///
/// # Errors
///
/// An [`IoError`] on write error, or "port pending: ...".
pub fn format_footprint_lib_file(
    holder: &Board,
    fp: FootprintId,
    options: &FormatOptions,
) -> Result<(String, FormatReport), IoError> {
    let mut formatter = PrettifiedOutputFormatter::new(options.format_mode, options.compact_save);
    let report = {
        let mut writer = PcbWriter::new(&mut formatter, holder, options);
        writer.format_footprint_lib_file(fp)?;
        writer.into_report()
    };
    formatter.finish()?;
    Ok((formatter.into_string(), report))
}
