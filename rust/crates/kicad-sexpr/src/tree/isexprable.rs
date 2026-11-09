// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2016 Mark Roszko <mark.roszko@gmail.com>
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

//! Port of `libs/sexpr/include/sexpr/isexprable.h`.

use crate::tree::sexpr::{Sexpr, SexprList};

/// Port of `SEXPR::ISEXPRABLE`: something that can be written to and read from an s-expression
/// tree.
pub trait ISexprable {
    /// Port of `ISEXPRABLE::SerializeSEXPR`.  The default returns `None` (C++ `nullptr`).
    fn serialize_sexpr(&self) -> Option<Sexpr> {
        None
    }

    /// Port of `ISEXPRABLE::DeserializeSEXPR`.  The default does nothing.
    ///
    /// C++ declares the parameter as `SEXPR&`; the library only ever passes the list a value
    /// is being extracted from (`operator>>( SEXPR_LIST&, ISEXPRABLE& )`), so the port takes
    /// the list.
    fn deserialize_sexpr(&mut self, _sexp: &mut SexprList) {}
}
