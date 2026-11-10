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

//! Port of `pcbnew/exporters/gendrill_writer_base.cpp` L850-993 (`GenDrillReportFile`) and
//! L1022-1070 (`printToolSummary`): the drill report text file. The report is base-class
//! functionality used by both drill writers; it is an inherent `impl` block of
//! [`GendrillWriterBase`] living in this module (work package FAB-02).
//!
//! Also defines [`HoleListSource`], the point where the writers obtain their hole lists
//! (`EnumerateDrillSpans` / `buildHolesList`): from the board ([`BoardHoleSource`], what the
//! C++ does), or from a recorded enumeration (hand-built hole lists, the differential tests).
//!
//! The file is not written: it becomes an [`OutputFile`] in [`GendrillWriterBase::outputs`].

use kicad_core::layer_ids::PcbLayerId;
use kicad_core::lset::Lset;
use kicad_core::printf::format_f;
use kicad_plot::gbr_metadata::GbrCreationDate;

use super::gendrill_writer_base::{
    GendrillWriterBase, ToolSummary, diameter_in_inches, diameter_in_mm, layer_name,
};
use crate::drill::{DrillSpan, HoleAttribute, enumerate_drill_spans};
use crate::wx_file_name::WxFileName;
use crate::{OutputFile, Reporter};

/// Where a drill writer gets its drill spans and hole lists from.
///
/// The C++ writers call `EnumerateDrillSpans( *m_pcb )` and `buildHolesList( span, npth )`
/// directly; the port routes both calls through this trait so that the writers can also run
/// on hand-built or recorded hole lists. [`BoardHoleSource`] is the C++ behaviour.
pub trait HoleListSource {
    /// `EnumerateDrillSpans( *m_pcb )` (pad backdrills not collected).
    fn drill_spans(&self, writer: &GendrillWriterBase<'_>) -> Vec<DrillSpan>;

    /// `buildHolesList( aSpan, aGenerateNPTH_list )`: fill `hole_list_buffer`,
    /// `hole_tool_references` and `tool_list_buffer` of `writer` for the span, honouring
    /// `writer.merge_pth_npth`.
    fn build_holes_list(
        &self,
        writer: &mut GendrillWriterBase<'_>,
        span: &DrillSpan,
        generate_npth_list: bool,
    );
}

/// The hole lists of the writer's board, exactly as the C++ obtains them.
#[derive(Clone, Copy, Debug, Default)]
pub struct BoardHoleSource;

impl HoleListSource for BoardHoleSource {
    fn drill_spans(&self, writer: &GendrillWriterBase<'_>) -> Vec<DrillSpan> {
        enumerate_drill_spans(&writer.pcb, false)
    }

    fn build_holes_list(
        &self,
        writer: &mut GendrillWriterBase<'_>,
        span: &DrillSpan,
        generate_npth_list: bool,
    ) {
        writer.build_holes_list(span, generate_npth_list);
    }
}

/// The virtual `getDrillFileName( aSpan, aNPTH, aMerge_PTH_NPTH )` the report calls: the base
/// version ([`GendrillWriterBase::drill_file_name`]) for the Excellon writer, the
/// `GERBER_WRITER` override for the Gerber writer.
pub type DrillFileNameFn<'f> = &'f dyn Fn(&GendrillWriterBase<'_>, &DrillSpan, bool, bool) -> String;

/// The `separator` line of `GenDrillReportFile`.
const SEPARATOR: &str = "    =============================================================\n";

/// `{fmt}`'s `display_width_of( cp )` (`thirdparty/fmt/include/fmt/format.h`): the number of
/// columns a code point counts for in width computations (2 for wide East Asian characters
/// and some emoji, else 1).
fn fmt_display_width_of(cp: u32) -> usize {
    1 + usize::from(
        cp >= 0x1100
            && (cp <= 0x115f // Hangul Jamo init. consonants
                || cp == 0x2329 // LEFT-POINTING ANGLE BRACKET
                || cp == 0x232a // RIGHT-POINTING ANGLE BRACKET
                // CJK ... Yi except IDEOGRAPHIC HALF FILL SPACE:
                || ((0x2e80..=0xa4cf).contains(&cp) && cp != 0x303f)
                || (0xac00..=0xd7a3).contains(&cp) // Hangul Syllables
                || (0xf900..=0xfaff).contains(&cp) // CJK Compatibility Ideographs
                || (0xfe10..=0xfe19).contains(&cp) // Vertical Forms
                || (0xfe30..=0xfe6f).contains(&cp) // CJK Compatibility Forms
                || (0xff00..=0xff60).contains(&cp) // Fullwidth Forms
                || (0xffe0..=0xffe6).contains(&cp) // Fullwidth Forms
                || (0x20000..=0x2fffd).contains(&cp) // CJK
                || (0x30000..=0x3fffd).contains(&cp)
                // Miscellaneous Symbols and Pictographs + Emoticons:
                || (0x1f300..=0x1f64f).contains(&cp)
                // Supplemental Symbols and Pictographs:
                || (0x1f900..=0x1f9ff).contains(&cp)),
    )
}

/// `fmt::format( "{:<N}", aText )` for a string: left aligned, padded with spaces to a display
/// width of `width` columns as `{fmt}` computes it.
pub(crate) fn fmt_left_aligned(text: &str, width: usize) -> String {
    let display_width: usize = text.chars().map(|c| fmt_display_width_of(c as u32)).sum();
    let mut out = text.to_string();

    for _ in display_width..width {
        out.push(' ');
    }

    out
}

/// `fmt::format( "{:W.Pf}", aValue )`: fixed notation with `precision` decimals, right
/// aligned in at least `width` columns.
pub(crate) fn fmt_fixed_width(value: f64, width: usize, precision: usize) -> String {
    let text = format_f(value, precision);
    let mut out = String::new();

    for _ in text.len()..width {
        out.push(' ');
    }

    out.push_str(&text);
    out
}

impl<'a> GendrillWriterBase<'a> {
    /// Port of `GENDRILL_WRITER_BASE::GenDrillReportFile( aFullFileName, aReporter )`: create
    /// a plain text report file giving a list of drill values and drill count for through
    /// holes, oblong holes, and for buried vias, drill values and drill count per layer pair.
    /// There is only one report for all drill files even when buried or blind vias exist.
    ///
    /// Here is a sample created by this function:
    /// ```text
    /// Drill report for F:/tmp/interf_u/interf_u.brd
    /// Created on 04/10/2012 20:48:38
    /// Selected Drill Unit: Imperial (inches)
    ///
    /// Drill report for plated through holes :
    /// T1  0,025"  0,64mm  (1 hole)
    /// T2  0,031"  0,79mm  (120 holes)
    /// T3  0,032"  0,81mm  (151 holes)  (with 1 slot)
    /// T4  0,040"  1,02mm  (43 holes)
    /// T5  0,079"  2,00mm  (1 hole)  (with 1 slot)
    /// T6  0,120"  3,05mm  (1 hole)  (with 1 slot)
    ///
    /// Total plated holes count 317
    ///
    ///
    /// Drill report for buried and blind vias :
    ///
    /// Drill report for holes from layer Soudure to layer Interne1 :
    ///
    /// Total plated holes count 0
    ///
    ///
    /// Drill report for holes from layer Interne1 to layer Interne2 :
    /// T1  0,025"  0,64mm  (3 holes)
    ///
    /// Total plated holes count 3
    ///
    ///
    /// Drill report for holes from layer Interne2 to layer Composant :
    /// T1  0,025"  0,64mm  (1 hole)
    ///
    /// Total plated holes count 1
    ///
    ///
    /// Drill report for unplated through holes :
    /// T1  0,120"  3,05mm  (1 hole)  (with 1 slot)
    ///
    /// Total unplated holes count 1
    /// ```
    ///
    /// `creation_date` replaces the clock (`GetISO8601CurrentDateTime()`); `drill_file_name`
    /// is the writer's (virtual) `getDrillFileName`; `source` provides the spans and hole
    /// lists (see [`HoleListSource`]). The report is appended to [`Self::outputs`] under
    /// `full_file_name`. The C++ failure paths (file cannot be created or written) do not
    /// exist without a file system, so this always returns `true` and `reporter` receives
    /// nothing.
    pub fn gen_drill_report_file(
        &mut self,
        full_file_name: &str,
        creation_date: &GbrCreationDate,
        drill_file_name: DrillFileNameFn<'_>,
        source: &dyn HoleListSource,
        reporter: Option<&mut dyn Reporter>,
    ) -> bool {
        // Only the C++ error paths report.
        let _ = reporter;

        let mut out = String::new();

        let board = self.pcb.board;
        let brd_filename = WxFileName::new(&board.file_name);

        let hole_sets = source.drill_spans(self);

        out.push_str(&format!("Drill report for {}\n", brd_filename.full_name()));
        out.push_str(&format!(
            "Created on {}\n\n",
            creation_date.format_iso_combined()
        ));

        // Output the cu layer stackup, so layer name references make sense.
        out.push_str("Copper Layer Stackup:\n");
        out.push_str(SEPARATOR);

        let mut conventional_layer_num = 1;

        for layer in Lset::all_cu_mask_n(board.copper_layer_count())
            .ui_order()
            .0
        {
            out.push_str(&format!(
                "    L{:<2}:  {} {}\n",
                conventional_layer_num,
                fmt_left_aligned(&board.layer_name(layer), 25),
                layer_name(layer) // generic layer name
            ));
            conventional_layer_num += 1;
        }

        out.push_str("\n\n");

        // output hole lists:
        // 1 - through holes
        // 2 - for partial holes only: by layer starting and ending pair
        // 3 - Non Plated through holes

        let mut build_npth_list = false; // First pass: build PTH list only

        // in this loop are plated only:
        for span in &hole_sets {
            source.build_holes_list(self, span, build_npth_list);

            let total_hole_count;

            if span.pair() == (PcbLayerId::FCu, PcbLayerId::BCu) && !span.is_backdrill {
                out.push_str(&format!(
                    "Drill file '{}' contains\n",
                    drill_file_name(self, span, false, self.merge_pth_npth)
                ));

                out.push_str("    plated through holes:\n");
                out.push_str(SEPARATOR);
                total_hole_count = self.print_tool_summary(&mut out, ToolSummary::Plated);
                out.push_str(&format!(
                    "    Total plated holes count {total_hole_count}\n"
                ));
            } else if span.is_backdrill {
                out.push_str(&format!(
                    "Drill file '{}' contains\n",
                    drill_file_name(self, span, false, self.merge_pth_npth)
                ));

                out.push_str(&format!(
                    "    backdrill span: '{}' to '{}':\n",
                    board.layer_name(span.drill_start_layer()),
                    board.layer_name(span.drill_end_layer())
                ));

                out.push_str(SEPARATOR);
                total_hole_count = self.print_tool_summary(&mut out, ToolSummary::Backdrill);
                out.push_str(&format!(
                    "    Total backdrilled holes count {total_hole_count}\n"
                ));
            } else {
                out.push_str(&format!(
                    "Drill file '{}' contains\n",
                    drill_file_name(self, span, false, self.merge_pth_npth)
                ));

                out.push_str(&format!(
                    "    holes connecting layer pair: '{} and {}' ({} vias):\n",
                    board.layer_name(span.pair().0),
                    board.layer_name(span.pair().1),
                    if span.pair().0 == PcbLayerId::FCu || span.pair().1 == PcbLayerId::BCu {
                        "blind"
                    } else {
                        "buried"
                    }
                ));

                out.push_str(SEPARATOR);
                total_hole_count = self.print_tool_summary(&mut out, ToolSummary::Plated);
                out.push_str(&format!(
                    "    Total plated holes count {total_hole_count}\n"
                ));
            }

            out.push_str("\n\n");
        }

        // NPTHoles. Generate the full list (pads+vias) if PTH and NPTH are merged,
        // or only the NPTH list (which never has vias)
        if !self.merge_pth_npth {
            build_npth_list = true;
        }

        let npth_span = DrillSpan::new(PcbLayerId::FCu, PcbLayerId::BCu, false, build_npth_list);

        source.build_holes_list(self, &npth_span, build_npth_list);

        // nothing wrong with an empty NPTH file in report.
        if self.merge_pth_npth {
            out.push_str("Not plated through holes are merged with plated holes\n");
        } else {
            out.push_str(&format!(
                "Drill file '{}' contains\n",
                drill_file_name(self, &npth_span, true, self.merge_pth_npth)
            ));
        }

        out.push_str("    unplated through holes:\n");
        out.push_str(SEPARATOR);
        let total_hole_count = self.print_tool_summary(&mut out, ToolSummary::Unplated);
        out.push_str(&format!(
            "    Total unplated holes count {total_hole_count}\n"
        ));

        self.outputs.push(OutputFile {
            name: full_file_name.to_string(),
            contents: out.into_bytes(),
        });

        true
    }

    /// Port of `GENDRILL_WRITER_BASE::printToolSummary( out, aSummary )`: print the drill
    /// tools of the class `summary` of the current tool list to `out` and return the hole
    /// count of these tools. The tool number is the index in the whole tool list.
    pub fn print_tool_summary(&self, out: &mut String, summary: ToolSummary) -> u32 {
        let mut total_hole_count: u32 = 0;

        for (ii, tool) in self.tool_list_buffer.iter().enumerate() {
            // Backdrills set the non-plated flag, so they must be classified before it is read
            let bucket = if tool.is_backdrill {
                ToolSummary::Backdrill
            } else if tool.hole_not_plated {
                ToolSummary::Unplated
            } else {
                ToolSummary::Plated
            };

            if bucket != summary {
                continue;
            }

            // List the tool number assigned to each drill in mm then in inches.
            let tool_number = ii + 1;
            out.push_str(&format!(
                "    T{}  {}mm  {}\"  ",
                tool_number,
                fmt_fixed_width(diameter_in_mm(f64::from(tool.diameter)), 2, 3),
                fmt_fixed_width(diameter_in_inches(f64::from(tool.diameter)), 2, 4)
            ));

            // Now list how many holes and ovals are associated with each drill.
            if tool.total_count == 1 && tool.oval_count == 0 {
                out.push_str("(1 hole");
            } else if tool.total_count == 1 {
                out.push_str("(1 hole)  (with 1 slot");
            } else if tool.oval_count == 0 {
                out.push_str(&format!("({} holes)", tool.total_count));
            } else if tool.oval_count == 1 {
                out.push_str(&format!("({} holes)  (with 1 slot", tool.total_count));
            } else {
                // tool.m_OvalCount > 1
                out.push_str(&format!(
                    "({} holes)  (with {} slots",
                    tool.total_count, tool.oval_count
                ));
            }

            if tool.hole_attribute == HoleAttribute::HolePadCastellated {
                out.push_str(", castellated");
            }

            if tool.hole_attribute == HoleAttribute::HolePadPressfit {
                out.push_str(", press-fit");
            }

            out.push_str(")\n");

            // C++ adds the int m_TotalCount to an unsigned total.
            total_hole_count = total_hole_count.wrapping_add(tool.total_count as u32);
        }

        out.push('\n');

        total_hole_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_width_helpers() {
        assert_eq!(fmt_left_aligned("F.Cu", 6), "F.Cu  ");
        assert_eq!(fmt_left_aligned("toolongname", 4), "toolongname");
        // A CJK character counts two columns, an accented Latin letter one.
        assert_eq!(fmt_left_aligned("\u{5e95}x", 5), "\u{5e95}x  ");
        assert_eq!(fmt_left_aligned("\u{e9}x", 4), "\u{e9}x  ");
        assert_eq!(fmt_fixed_width(0.3, 2, 3), "0.300");
        assert_eq!(fmt_fixed_width(0.0118, 2, 4), "0.0118");
        assert_eq!(fmt_fixed_width(1.0, 7, 1), "    1.0");
    }
}
