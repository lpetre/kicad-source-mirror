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
//! FAB-01 ported `drill_span.h`, `drill_operation.h` and `drill_enumerator.h` / `.cpp` here;
//! package M26 moved the model-level copy into kicad-pcb (the drill chart needs it). Since
//! GLUE-3 this module re-exports [`kicad_pcb::drill`] (with `DRILL_SPAN` being
//! [`kicad_pcb::pcb_drill_map::DrillSpan`]) under the paths the exporters use.

pub use kicad_pcb::drill::{drill_enumerator, drill_operation};

/// `pcbnew/drill/drill_span.h`: `DRILL_SPAN`, `DRILL_LAYER_PAIR`, `HOLE_ATTRIBUTE`.
pub mod drill_span {
    pub use kicad_pcb::drill::{DrillLayerPair, HoleAttribute};
    pub use kicad_pcb::pcb_drill_map::DrillSpan;
}

pub use kicad_pcb::drill::{
    DrillLayerPair, DrillOpKind, DrillOperation, DrillOperationId, DrillPostMachining, DrillQuery,
    DrillSource, DrillSpan, HoleAttribute, enumerate_drill_operations, enumerate_drill_spans,
    is_drill_slot,
};
