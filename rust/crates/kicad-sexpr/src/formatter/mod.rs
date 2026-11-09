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

//! The s-expression writer side: port of `OUTPUTFORMATTER` and `STRING_FORMATTER`
//! (`include/richio.h`, `common/richio.cpp`) and of namespace `KICAD_FORMAT`
//! (`include/io/kicad/kicad_io_utils.h`, `common/io/kicad/kicad_io_utils.cpp`), including
//! `Prettify`.

pub mod cformat;
pub mod kicad_io_utils;
pub mod richio;
