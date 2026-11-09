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

//! The s-expression lexer: port of `DSNLEXER` (`include/dsnlexer.h`, `common/dsnlexer.cpp`),
//! of the line readers it reads from (`LINE_READER` and `STRING_LINE_READER` of
//! `include/richio.h`, `common/richio.cpp`) and of the keyword table generator
//! (`cmake/BuildSteps/TokenList2DsnLexer.cmake`).

pub mod c_numeric;
pub mod dsnlexer;
pub mod keywords;
pub mod line_reader;
