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

//! Port of `pcbnew/exporters/gendrill_writer_base.cpp` L506-847 (`genDrillMapFile`) and
//! L996-1019 (`plotDrillMarks`): drill map drawings.
//!
//! Work package FAB-12 (`docs/analysis/plot-fab.md` 10.2, tier 2); not started. Only the entry
//! point that `GendrillWriterBase::create_map_files_set` (FAB-01) calls exists.

use kicad_core::plot_enums::PlotFormat;

use super::gendrill_writer_base::GendrillWriterBase;
use crate::FabError;

/// `GENDRILL_WRITER_BASE::genDrillMapFile( aFullFileName, aFormat )`: plot a map of drill
/// marks for the current hole list (built by `build_holes_list`); returns the file contents.
///
/// Not ported yet (FAB-12): always `Err(FabError::NotPorted)`.
pub fn gen_drill_map_file(
    writer: &GendrillWriterBase<'_>,
    full_file_name: &str,
    format: PlotFormat,
) -> Result<Vec<u8>, FabError> {
    let _ = (writer, full_file_name, format);
    Err(FabError::NotPorted("GENDRILL_WRITER_BASE::genDrillMapFile (FAB-12)"))
}
