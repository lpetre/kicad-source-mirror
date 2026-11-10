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

//! Port of KiCad's copper zone filler (`pcbnew/zone_filler.cpp`, `zone_filler.h`).
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and
//! `rust/docs/analysis/connectivity-zones.md` (sections 4.8 - 4.14).
//!
//! * [`filler`]: `ZONE_FILLER` state and `Fill()` (the driver: flashing passes, the fill DAG run
//!   sequentially, island removal, out-of-board islands, the second flashing pass);
//! * [`index`]: the item indexes the filler builds once per fill;
//! * [`refill`]: the iterative refill and `refillZoneFromCache`;
//! * [`copper_fill`]: `fillSingleZone` and the per-zone copper computation (package ZF-03).

/// Panics with "kicad-zone: port pending: <what>" for C++ code whose package has not landed
/// yet (the crate-local twin of `kicad_pcb::port_pending!`).
#[macro_export]
macro_rules! port_pending {
    ($cxx:literal $(, $arg:expr)* $(,)?) => {{
        $( let _ = &$arg; )*
        panic!(concat!("kicad-zone: port pending: ", $cxx))
    }};
}

pub mod copper_fill;
pub mod filler;
pub mod index;
pub mod refill;

pub use filler::ZoneFiller;
