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

//! The drill model of `pcbnew/drill/` at board-model level (package M26): which holes a board
//! has per drill span, how they group into chart rows and which symbol each group carries.
//!
//! `drill_span.h`'s `DRILL_SPAN` is [`crate::pcb_drill_map::DrillSpan`]; the drill writers
//! (`kicad-fab`) build on this module. `DRILL_CHART_FILTER`, `DRILL_CHART_COLUMN_ID` and
//! `DrillChartSchema()` of `drill_chart_template.h` are in [`crate::pcb_generated_table`].

pub mod drill_chart_model;
pub mod drill_chart_template;
pub mod drill_enumerator;
pub mod drill_operation;
pub mod drill_symbol_assigner;
pub mod drill_symbol_profile;

pub use drill_chart_model::{DrillChartGroup, DrillChartModel, DrillChartRowSpec, DrillChartTotals};
pub use drill_chart_template::DrillChartTemplate;
pub use drill_enumerator::{enumerate_drill_operations, enumerate_drill_spans, is_drill_slot};
pub use drill_operation::{
    DrillLayerPair, DrillOpKind, DrillOperation, DrillOperationId, DrillPostMachining,
    DrillQuery, DrillSource, DrillSpan, HoleAttribute,
};
pub use drill_symbol_assigner::{
    DrillSymbolCache, DrillSymbolEntry, assign_drill_symbols, drill_symbol_cache,
    resolve_drill_symbols, resolve_drill_symbols_by_item,
};
pub use drill_symbol_profile::{
    DrillGroupKey, DrillMarkMode, DrillMarkPolicy, DrillSymbolAssignment, DrillSymbolProfile,
};
