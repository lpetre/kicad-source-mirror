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
//! PCBIO-03: `format( const PCB_SHAPE* )` (W:1183-1331), plus `LINE_ENDING::Format`
//! (`common/line_ending.cpp`), the line-ending formatter the shape writer calls. The stroke
//! formatter is `STROKE_PARAMS::Format` (`kicad_core::stroke_params`).

use kicad_core::eda_units::format_angle;
use kicad_core::layer_ids::is_external_copper_layer;
use kicad_core::stroke_params::StrokeParams;
use kicad_core::PCB_IU_SCALE;
use kicad_sexpr::{IoError, OutputFormatter};
use kicad_sexpr::{format_bool, format_custom_properties, format_uuid};

use super::{
    CTL_OMIT_PAD_NETS, PcbWriter, format_internal_units_pt_fp, format_iu, format_iu_pt,
    unbake_linear,
};
use crate::board_connected_item::netname;
use crate::eda_shape::{FillT, ShapeT};
use crate::ids::ShapeId;
use crate::line_ending::{LineEnding, LineEndingStyle};
use crate::traits::{BoardConnectedItem, BoardItem};

/// Port of `LINE_ENDING::Format( OUTPUTFORMATTER* aOut, const EDA_IU_SCALE& aIuScale,
/// const char* aToken )`: `(<token> <style> [(length ..)] [(width ..)] [(stroke (width ..)
/// (type ..))])`, nothing for `NONE`.
pub(crate) fn format_line_ending(
    out: &mut dyn OutputFormatter,
    ending: &LineEnding,
    token: &str,
) -> Result<(), IoError> {
    if ending.style() == LineEndingStyle::None {
        return Ok(());
    }

    let mut result = format!("({token} {}", LineEnding::style_to_token(ending.style()));

    if ending.length() > 0 {
        result += &format!(" (length {})", format_iu(ending.length()));
    }

    if ending.width() > 0 {
        result += &format!(" (width {})", format_iu(ending.width()));
    }

    let stroke = ending.stroke();

    if stroke.width() > 0 {
        result += &format!(
            " (stroke (width {}) (type {}))",
            format_iu(stroke.width()),
            StrokeParams::get_line_style_token(stroke.line_style())
        );
    }

    result.push(')');

    out.print(format_args!("{result}"))?;
    Ok(())
}

impl PcbWriter<'_> {
    /// Port of `format( const PCB_SHAPE* aShape )`: `(gr_line ..)`, `(gr_rect ..)`, ...
    /// (`fp_*` inside a footprint). Library-frame values are written for lines, rectangles,
    /// circles, arcs and ellipses; polygon and bezier points are un-baked from the board frame.
    pub(crate) fn format_shape(&mut self, shape_id: ShapeId) -> Result<(), IoError> {
        let Some(shape) = self.src.shape(shape_id) else {
            return Ok(());
        };

        let ctx = self.ctx;
        let parent_fp = crate::board_item::parent_footprint(shape, &ctx);
        let prefix = if parent_fp.is_some() { "fp" } else { "gr" };

        match shape.lib_shape() {
            ShapeT::Segment => {
                self.out.print(format_args!(
                    "({prefix}_line (start {}) (end {})",
                    format_iu_pt(shape.lib_start()),
                    format_iu_pt(shape.lib_end())
                ))?;
            }
            ShapeT::Rectangle => {
                self.out.print(format_args!(
                    "({prefix}_rect (start {}) (end {})",
                    format_iu_pt(shape.lib_start()),
                    format_iu_pt(shape.lib_end())
                ))?;

                if shape.shape.corner_radius() > 0 {
                    self.out.print(format_args!(
                        " (radius {})",
                        format_iu(shape.lib_corner_radius())
                    ))?;
                }
            }
            ShapeT::Circle => {
                self.out.print(format_args!(
                    "({prefix}_circle (center {}) (end {})",
                    format_iu_pt(shape.lib_start()),
                    format_iu_pt(shape.lib_end())
                ))?;
            }
            ShapeT::Arc => {
                self.out.print(format_args!(
                    "({prefix}_arc (start {}) (mid {}) (end {})",
                    format_iu_pt(shape.lib_start()),
                    format_iu_pt(shape.lib_arc_mid()),
                    format_iu_pt(shape.lib_end())
                ))?;
            }
            ShapeT::Poly => {
                if shape.shape.is_poly_shape_valid() {
                    let outline = shape.shape.poly_shape().c_outline(0);

                    self.out.print(format_args!("({prefix}_poly"))?;
                    self.format_poly_pts(outline, parent_fp)?;
                } else {
                    return Ok(());
                }
            }
            ShapeT::Bezier => {
                self.out.print(format_args!(
                    "({prefix}_curve (pts (xy {}) (xy {}) (xy {}) (xy {}))",
                    format_internal_units_pt_fp(shape.shape.start(), parent_fp),
                    format_internal_units_pt_fp(shape.shape.bezier_c1(), parent_fp),
                    format_internal_units_pt_fp(shape.shape.bezier_c2(), parent_fp),
                    format_internal_units_pt_fp(shape.shape.end(), parent_fp)
                ))?;
            }
            ShapeT::Ellipse => {
                self.out.print(format_args!(
                    "({prefix}_ellipse (center {}) (major_radius {}) (minor_radius {}) \
                     (rotation_angle {})",
                    format_iu_pt(shape.lib_ellipse_center()),
                    format_iu(shape.lib_ellipse_major_radius()),
                    format_iu(shape.lib_ellipse_minor_radius()),
                    format_angle(shape.lib_ellipse_rotation().as_degrees())
                ))?;
            }
            ShapeT::EllipseArc => {
                self.out.print(format_args!(
                    "({prefix}_ellipse_arc (center {}) (major_radius {}) (minor_radius {}) \
                     (rotation_angle {}) (start_angle {}) (end_angle {})",
                    format_iu_pt(shape.lib_ellipse_center()),
                    format_iu(shape.lib_ellipse_major_radius()),
                    format_iu(shape.lib_ellipse_minor_radius()),
                    format_angle(shape.lib_ellipse_rotation().as_degrees()),
                    format_angle(shape.lib_ellipse_start_angle().as_degrees()),
                    format_angle(shape.lib_ellipse_end_angle().as_degrees())
                ))?;
            }
            ShapeT::Undefined => {
                // UNIMPLEMENTED_FOR( aShape->SHAPE_T_asString() )
                debug_assert!(
                    false,
                    "format(PCB_SHAPE): unimplemented for {}",
                    shape.shape.shape_t_as_string()
                );
                return Ok(());
            }
        }

        {
            let mut stroke = shape.stroke(&ctx);
            stroke.set_width(unbake_linear(stroke.width(), parent_fp));
            stroke.format(self.out, &PCB_IU_SCALE)?;
        }

        format_line_ending(self.out, &shape.shape.start_ending(), "start_shape")?;
        format_line_ending(self.out, &shape.shape.end_ending(), "end_shape")?;

        // The filled flag represents if a solid fill is present on circles, rectangles and
        // polygons
        if matches!(
            shape.shape.shape(),
            ShapeT::Poly | ShapeT::Rectangle | ShapeT::Circle | ShapeT::Ellipse
        ) {
            match shape.shape.fill_mode() {
                FillT::Hatch => {
                    self.out.print(format_args!("(fill hatch)"))?;
                }
                FillT::ReverseHatch => {
                    self.out.print(format_args!("(fill reverse_hatch)"))?;
                }
                FillT::CrossHatch => {
                    self.out.print(format_args!("(fill cross_hatch)"))?;
                }
                FillT::FilledShape => format_bool(self.out, "fill", true)?,
                _ => format_bool(self.out, "fill", false)?,
            }
        }

        if BoardItem::is_locked(shape, &ctx) {
            format_bool(self.out, "locked", true)?;
        }

        let layer_set = BoardItem::layer_set(shape, &ctx);
        let layer = BoardItem::layer(shape, &ctx);

        if layer_set.count() > 1 {
            self.format_layers(layer_set, false, false)?;
        } else {
            self.format_layer(layer, false)?;
        }

        if shape.has_solder_mask() && is_external_copper_layer(layer) {
            if let Some(margin) = shape.local_solder_mask_margin() {
                self.out
                    .print(format_args!("(solder_mask_margin {})", format_iu(margin)))?;
            }
        }

        if (self.ctl & CTL_OMIT_PAD_NETS) == 0 && shape.net_code().0 > 0 {
            let name = self.out.quotew(netname(shape, &ctx));
            self.out.print(format_args!("(net {name})"))?;
        }

        let base = &shape.base.item;
        format_uuid(self.out, &base.uuid.as_string())?;
        format_custom_properties(self.out, &base.custom_properties)?;
        self.out.print(format_args!(")"))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    //! Differential oracle of `format( const PCB_SHAPE* )` alone: the shapes of a parsed board
    //! formatted one by one (file order, `m_board` set) must give the reference re-save's
    //! `gr_*` blocks as a multiset. The board-level writer sorts drawings with
    //! `BOARD::cmp_drawings` (model package M22), whose `EDA_SHAPE::Compare` call is still a
    //! model stub; this test checks the shape formatter independently of that sort. The block
    //! order is checked by `tests/pcbio03_tracks_shapes.rs` once the comparator lands.

    use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
    use std::path::Path;

    use kicad_sexpr::{FormatMode, PrettifiedOutputFormatter};

    use super::*;
    use crate::board::Board;
    use crate::io::{FormatOptions, ParseOptions, parse_board_report};
    use crate::item_ref::AnyItem;

    const PENDING: &str = "kicad-pcb: port pending: ";

    /// `port_pending!` panics become `Err(name)`.
    fn model<R>(f: impl FnOnce() -> R) -> Result<R, String> {
        match catch_unwind(AssertUnwindSafe(f)) {
            Ok(r) => Ok(r),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| payload.downcast_ref::<String>().cloned());

                match msg {
                    Some(m) if m.starts_with(PENDING) => Err(m[PENDING.len()..].to_string()),
                    _ => resume_unwind(payload),
                }
            }
        }
    }

    /// The board's drawings that are shapes, each formatted with `format_shape`, inside a
    /// prettified `(kicad_pcb ..)` so that every block sits at depth 1 as in a board file.
    fn format_shapes_alone(board: &Board) -> String {
        let options = FormatOptions::default();
        let mut formatter = PrettifiedOutputFormatter::new(FormatMode::Normal, false);

        {
            let mut writer = PcbWriter::new(&mut formatter, board, &options);
            writer.board = Some(board);
            writer.out.print(format_args!("(kicad_pcb")).unwrap();

            let ids: Vec<ShapeId> = board
                .drawings()
                .filter(|(_, item)| matches!(item, AnyItem::Shape(_)))
                .filter_map(|(id, _)| id.typed())
                .collect();

            for id in ids {
                writer.format_shape(id).unwrap();
            }

            writer.out.print(format_args!(")")).unwrap();
        }

        formatter.finish().unwrap();
        formatter.into_string()
    }

    /// The top-level `gr_*` blocks (mirrors `tests/oracle/pcbio03_golden.sh`).
    fn graphic_blocks(text: &str) -> Vec<String> {
        const GRAPHICS: [&str; 8] = [
            "gr_line",
            "gr_rect",
            "gr_circle",
            "gr_arc",
            "gr_poly",
            "gr_curve",
            "gr_ellipse",
            "gr_ellipse_arc",
        ];
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        let mut out = Vec::new();
        let mut i = 0;

        while i < lines.len() {
            let l = lines[i];

            if !(l.starts_with("\t(") && !l.starts_with("\t\t")) {
                i += 1;
                continue;
            }

            let word = l[2..]
                .split([' ', '\n'])
                .next()
                .unwrap_or("")
                .trim_end_matches(')');
            let mut j = i;

            if !l.trim_end_matches('\n').ends_with(')')
                || l.matches('(').count() != l.matches(')').count()
            {
                while j < lines.len() && lines[j] != "\t)\n" {
                    j += 1;
                }
            }

            if GRAPHICS.contains(&word) {
                out.push(lines[i..=j.min(lines.len() - 1)].concat());
            }

            i = j + 1;
        }

        out
    }

    fn parse(text: &str, name: &str) -> Board {
        parse_board_report(text, name, &ParseOptions {
            allow_pending: true,
        })
        .unwrap_or_else(|e| panic!("{name}: {}", e.what()))
        .0
    }

    fn sorted(mut v: Vec<String>) -> Vec<String> {
        v.sort();
        v
    }

    /// The hand-made boards `tests/data/pcbio03/shapes*.kicad_pcb` (every shape kind, fill
    /// mode, stroke type, line ending; a 2021 file with legacy arcs, `width`, implicit fills,
    /// numeric nets) and their reference re-saves: input and O1 both format to O1's blocks.
    #[test]
    fn oracle_shapes_handmade() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");

        for name in ["shapes.kicad_pcb", "shapes_legacy.kicad_pcb"] {
            let input = std::fs::read_to_string(dir.join("data/pcbio03").join(name)).unwrap();
            let o1 = std::fs::read_to_string(dir.join("golden/pcbio03").join(name)).unwrap();
            let want = sorted(graphic_blocks(&o1));
            assert!(!want.is_empty());

            for (what, text) in [("input", &input), ("O1", &o1)] {
                let got = sorted(graphic_blocks(&format_shapes_alone(&parse(text, name))));

                for (g, w) in got.iter().zip(want.iter()) {
                    assert_eq!(g, w, "{name} ({what})");
                }

                assert_eq!(got.len(), want.len(), "{name} ({what})");
            }
        }
    }

    /// The `graphics` lines of `tests/golden/pcbio03_corpus.txt`: the multiset of `gr_*`
    /// blocks of each corpus board, parsed from the original file, against the reference
    /// re-save (sorted-block hash). Boards whose parse reaches a model stub are pending.
    #[test]
    fn oracle_shapes_corpus() {
        let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let golden =
            std::fs::read_to_string(crate_dir.join("tests/golden/pcbio03_corpus.txt")).unwrap();
        let root = kicad_qa::kicad_source_root();
        let (mut identical, mut pending, mut blocks_checked) = (0, 0, 0);
        let mut failures = Vec::new();

        for line in golden.lines().filter(|l| l.contains("|graphics|")) {
            let f: Vec<&str> = line.split('|').collect();
            let (path, count) = (f[0], f[2].parse::<usize>().unwrap());

            if count == 0 {
                continue;
            }

            let text = String::from_utf8_lossy(&std::fs::read(root.join(path)).unwrap())
                .into_owned();

            match model(|| {
                parse_board_report(&text, path, &ParseOptions {
                    allow_pending: true,
                })
                .map(|(board, _)| sorted(graphic_blocks(&format_shapes_alone(&board))))
            }) {
                Err(stub) => {
                    pending += 1;
                    println!("{path}: pending on model stub {stub}");
                }
                Ok(Err(e)) => failures.push(format!("{path}: {}", e.what())),
                Ok(Ok(got)) => {
                    let hash = format!(
                        "{:016x}",
                        kicad_qa::io_corpus::fnv1a64(got.concat().as_bytes())
                    );

                    if got.len() == count && hash == f[4] {
                        identical += 1;
                        blocks_checked += count;
                    } else {
                        failures.push(format!("{path}: {} blocks, expected {count}", got.len()));
                    }
                }
            }
        }

        println!(
            "pcbio03 shapes corpus: {identical} boards identical ({blocks_checked} blocks), \
             {pending} pending, {} differ",
            failures.len()
        );

        for f in &failures {
            println!("DIFFERS {f}");
        }

        assert!(failures.is_empty(), "{} boards differ", failures.len());
    }
}
