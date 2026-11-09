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

//! The DRC test providers (`pcbnew/drc/drc_test_provider_*.cpp`), one module per C++ file, in
//! the C++ registration (run) order. `drc_test_provider_schematic_parity.cpp` is not ported.

pub mod annular_width;
pub mod disallow;
pub mod creepage;
pub mod connectivity;
pub mod connection_width;
pub mod copper_clearance;
pub mod physical_clearance;
pub mod courtyard_clearance;
pub mod edge_clearance;
pub mod footprint_checks;
pub mod hole_to_hole;
pub mod hole_size;
pub mod library_parity;
pub mod misc;
pub mod net_chain_tuning_profiles;
pub mod text_dims;
pub mod track_angle;
pub mod track_width;
pub mod track_segment_length;
pub mod zone_connections;
pub mod via_diameter;
pub mod via_stack;
pub mod via_stitch;
pub mod solder_mask;
pub mod silk_clearance;
pub mod matched_length;
pub mod diff_pair_coupling;
pub mod sliver_checker;
pub mod text_mirroring;
