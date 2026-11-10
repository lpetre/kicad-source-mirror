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

//! Port of `pcbnew/exporters/gendrill_gerber_writer.h` / `.cpp` (`GERBER_WRITER`: Gerber X2 drill
//! and IPC-4761 via protection files).
//!
//! Work package FAB-08 (`docs/analysis/plot-fab.md` 10.2); not started. It embeds
//! `super::gendrill_writer_base::GendrillWriterBase` and overrides `getDrillFileName`.
