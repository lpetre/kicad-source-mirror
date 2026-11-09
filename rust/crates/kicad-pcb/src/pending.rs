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

//! The `port_pending!` marker (new code, `board-model.md` section 12).
//!
//! Every behaviour stub of the board model is written as
//! `port_pending!("CLASS::Method", arg1, arg2)`: it names the C++ function the owning work
//! package still has to port and panics with that name when reached. The optional arguments are
//! the stub's parameters; they are only borrowed, so that a stub keeps the parameter names of
//! the final signature without `unused_variables` warnings. `rg port_pending! crates/kicad-pcb`
//! must be empty before the crate is declared done.

/// Marks a not yet ported C++ body; panics with the C++ name when reached.
///
/// ```should_panic
/// fn get_layer_set(layer: i32) -> u64 {
///     kicad_pcb::port_pending!("PCB_TRACK::GetLayerSet", layer)
/// }
/// get_layer_set(0);
/// ```
#[macro_export]
macro_rules! port_pending {
    ($cxx:literal $(, $arg:expr)* $(,)?) => {{
        $( let _ = &$arg; )*
        panic!(concat!("kicad-pcb: port pending: ", $cxx))
    }};
}
