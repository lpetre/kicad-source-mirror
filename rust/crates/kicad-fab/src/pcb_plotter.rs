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

//! Port of `pcbnew/pcb_plotter.h` / `pcb_plotter.cpp`: the `PCB_PLOTTER` helpers for layer
//! sequences and output file names (`BuildPlotFileName`, `getPlotSequence`,
//! `copperLayerShouldBeSkipped`; `pcb_plotter.cpp` L355-423).
//!
//! Work packages FAB-03 (this part), FAB-07 (`PlotJobToPlotOpts`), FAB-11 (`PCB_PLOTTER::Plot`,
//! the multi-file driver used by SVG) (`docs/analysis/plot-fab.md` 10.2).
//!
//! Out of scope: `PCB_PLOTTER::DialogLayerSelection` (GUI dialog support, analysis 2.2).

use kicad_core::{Lset, PcbLayerId};
use kicad_pcb::board::Board;

use crate::wx_file_name::WxFileName;

/// `wxFileName::GetForbiddenChars( wxPATH_DOS )` (`*?` plus `\/:"<>|`) followed by the two
/// characters `BuildPlotFileName` appends (`%.`).
const BUILD_PLOT_FILE_NAME_BAD_CHARS: &[char] =
    &['*', '?', '\\', '/', ':', '"', '<', '>', '|', '%', '.'];

/// `wxString::Trim()`: the characters `wxSafeIsspace` accepts (ASCII white space only).
fn is_wx_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '\u{0c}' | '\r')
}

/// Port of `PCB_PLOTTER::BuildPlotFileName`: complete a plot file name.
///
/// `a_filename` contains the base filename only (without path and extension) when calling
/// this function. It is expected to be a valid filename (this is usually the board
/// filename). The directory becomes `a_output_dir`, the extension `a_extension`; the suffix,
/// trimmed and with the characters not allowed in file names replaced by `_`, is appended to
/// the name as `-<suffix>` when something survives.
pub fn build_plot_file_name(
    a_filename: &mut WxFileName,
    a_output_dir: &str,
    a_suffix: &str,
    a_extension: &str,
) {
    a_filename.set_path(a_output_dir);

    // Set the file extension
    a_filename.set_ext(a_extension);

    // remove leading and trailing spaces if any from the suffix, if
    // something survives add it to the name;
    // also the suffix can contain some not allowed chars in filename (/ \ . : and some others),
    // so change them to underscore
    // Remember it can be called from a python script, so the illegal chars
    // have to be filtered here.
    let suffix: String = a_suffix
        .trim_end_matches(is_wx_space)
        .trim_start_matches(is_wx_space)
        .chars()
        .map(|c| {
            if BUILD_PLOT_FILE_NAME_BAD_CHARS.contains(&c) {
                '_'
            } else {
                c
            }
        })
        .collect();

    if !suffix.is_empty() {
        let name = format!("{}-{}", a_filename.name(), suffix);
        a_filename.set_name(&name);
    }
}

/// Port of `PCB_PLOTTER::getPlotSequence`: the layer to plot first, then the layers of
/// `a_plot_with_all_layers_seq` that are not already in the sequence (in their order).
pub fn get_plot_sequence(
    a_layer_to_plot: PcbLayerId,
    a_plot_with_all_layers_seq: &[PcbLayerId],
) -> Vec<PcbLayerId> {
    let mut plot_sequence = Vec::new();

    // Base layer always gets plotted first.
    plot_sequence.push(a_layer_to_plot);

    for &layer in a_plot_with_all_layers_seq {
        // Don't plot the same layer more than once;
        if plot_sequence.contains(&layer) {
            continue;
        }

        plot_sequence.push(layer);
    }

    plot_sequence
}

/// Port of `PCB_PLOTTER::copperLayerShouldBeSkipped`: true for a copper layer the board does
/// not enable.
pub fn copper_layer_should_be_skipped(a_board: &Board, a_layer_to_plot: PcbLayerId) -> bool {
    let mask: Lset = Lset::all_cu_mask() & &!&a_board.enabled_layers();
    mask.contains(a_layer_to_plot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(board: &str, dir: &str, suffix: &str, ext: &str) -> String {
        let mut fn_ = WxFileName::new(board);
        build_plot_file_name(&mut fn_, dir, suffix, ext);
        fn_.full_path()
    }

    #[test]
    fn plot_file_names() {
        assert_eq!(
            name("/a/demo.kicad_pcb", "out", "F.Cu", "gtl"),
            "out/demo-F_Cu.gtl"
        );
        assert_eq!(
            name("demo.kicad_pcb", "out/", " Inner Top \t", "g1"),
            "out/demo-Inner Top.g1"
        );
        assert_eq!(
            name("demo.kicad_pcb", "o", "My:Layer.x*?\\/\"<>|%", "gbr"),
            "o/demo-My_Layer_x_________.gbr"
        );
        assert_eq!(name("demo.kicad_pcb", "o", "  ", "gbrjob"), "o/demo.gbrjob");
        // wxString::Trim only removes ASCII white space
        assert_eq!(
            name("demo.kicad_pcb", "o", "\u{a0}x", "gbr"),
            "o/demo-\u{a0}x.gbr"
        );
    }

    #[test]
    fn plot_sequence() {
        let seq = get_plot_sequence(
            PcbLayerId::FCu,
            &[PcbLayerId::EdgeCuts, PcbLayerId::FCu, PcbLayerId::EdgeCuts],
        );
        assert_eq!(seq, vec![PcbLayerId::FCu, PcbLayerId::EdgeCuts]);
    }
}
