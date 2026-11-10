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

//! The drill model (`pcbnew/drill/`): which holes a board has, per drill span.
//!
//! Ported in work package FAB-01: `drill_span.h`, `drill_operation.h`, `drill_enumerator.h` /
//! `.cpp`. The drill chart / drill symbol files of that directory belong to FAB-13 (tier 3).

pub mod drill_enumerator;
pub mod drill_operation;
pub mod drill_span;

pub use drill_enumerator::{enumerate_drill_operations, enumerate_drill_spans, is_drill_slot};
pub use drill_operation::{
    DrillOpKind, DrillOperation, DrillOperationId, DrillPostMachining, DrillQuery, DrillSource,
};
pub use drill_span::{DrillLayerPair, DrillSpan, HoleAttribute};
