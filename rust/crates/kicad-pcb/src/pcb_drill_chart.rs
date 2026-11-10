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

//! Port of `pcbnew/pcb_drill_chart.h` / `.cpp` (`PCB_DRILL_CHART`, `DRILL_CHART_CONTENT`,
//! `DRILL_PROFILE_PENDING`): what a drill chart reports. The chart's members are
//! `GeneratedTableKind::DrillChart` (package M0c); its constructor and `ApplyTemplate` are
//! `GeneratedTableData::new_drill_chart` / [`apply_template`]; the rebuild machinery is
//! `PCB_GENERATED_TABLE`'s ([`crate::pcb_generated_table`]).

use std::collections::{BTreeMap, BTreeSet};

use crate::ctx::BoardCtx;
use crate::drill::{
    DrillChartGroup, DrillChartModel, DrillChartRowSpec, DrillChartTemplate, DrillMarkMode,
    DrillOpKind, DrillSymbolAssignment, DrillSymbolProfile, assign_drill_symbols,
    enumerate_drill_spans, resolve_drill_symbols,
};
use crate::pcb_generated_table::{
    DrillChartColumnId, DrillChartFilter, GeneratedTableContent, GeneratedTableData,
    GeneratedTableKind, GeneratedTableRow, GeneratedTableUnits, format_generated_table_length,
};
use kicad_core::printf::format_f;

/// `NO_VALUE`: an em dash.
const NO_VALUE: &str = "\u{2014}";

/// Port of `PCB_DRILL_CHART::ApplyTemplate( aTemplate )`: copy a template's formatting. Called
/// when a chart is placed, never afterwards.
pub fn apply_template(data: &mut GeneratedTableData, template: &DrillChartTemplate) {
    data.columns = template.columns.clone();
    data.units = template.units;
    data.precision = template.precision;

    if let GeneratedTableKind::DrillChart { show_totals, .. } = &mut data.kind {
        *show_totals = template.show_totals;
    }
}

/// Port of `PCB_DRILL_CHART::columnId( aCol )`: the typed id a column at this index holds
/// (`None` for an id the schema does not know, which `Validate` keeps out of a loaded chart).
fn column_id(data: &GeneratedTableData, col: usize) -> Option<DrillChartColumnId> {
    use DrillChartColumnId as C;

    const ALL: [DrillChartColumnId; 12] = [
        C::Symbol,
        C::DrillDiameter,
        C::SlotSize,
        C::Plating,
        C::OperationCount,
        C::SiteCount,
        C::LayerSpan,
        C::Operation,
        C::Protection,
        C::BackdrillStub,
        C::AspectRatio,
        C::Description,
    ];

    ALL.into_iter()
        .find(|id| *id as i32 == data.columns[col].id)
}

/// Port of `PCB_DRILL_CHART::buildGroups( aBoard, aProfile )`: every span on the board, as
/// chart rows grouped the way `profile` says (what shares a row is the profile's grouping to
/// decide), filtered by the chart's filter.
fn build_groups(
    ctx: &BoardCtx<'_>,
    filter: &DrillChartFilter,
    profile: &DrillSymbolProfile,
) -> Vec<DrillChartGroup> {
    let spec = DrillChartRowSpec { filter: *filter };

    let mut model = DrillChartModel::new(profile);
    model.build_with_spec(ctx, &enumerate_drill_spans(ctx, false), &spec);

    model.groups().to_vec()
}

/// `spanText`: the board's own layer names, so a chart matches the stackup the fabricator was
/// given.
fn span_text(ctx: &BoardCtx<'_>, group: &DrillChartGroup) -> String {
    format!(
        "{} / {}",
        ctx.board.layer_name(group.top_layer),
        ctx.board.layer_name(group.bottom_layer)
    )
}

/// `operationText`.
fn operation_text(group: &DrillChartGroup) -> &'static str {
    if group.kind != DrillOpKind::PrimaryDrill {
        return "Backdrill";
    }

    if group.is_slot {
        return "Slot";
    }

    "Drill"
}

/// `protectionText`: the protection kinds joined by commas (`wxJoin( parts, ',' )`), an em dash
/// for none.
fn protection_text(group: &DrillChartGroup) -> String {
    let mut parts: Vec<&str> = Vec::new();

    if group.filled {
        parts.push("filled");
    }

    if group.capped {
        parts.push("capped");
    }

    if group.top_plugged || group.bottom_plugged {
        parts.push("plugged");
    }

    if group.top_covered || group.bottom_covered {
        parts.push("covered");
    }

    if group.top_tented || group.bottom_tented {
        parts.push("tented");
    }

    if parts.is_empty() {
        NO_VALUE.to_string()
    } else {
        parts.join(",")
    }
}

/// `symbolText`: a letter, the drill size, or nothing for a shape (the painter draws it).
fn symbol_text(group: &DrillChartGroup, units: GeneratedTableUnits, precision: i32) -> String {
    match group.symbol.mark_mode {
        DrillMarkMode::Letter => group.symbol.letter.clone(),
        DrillMarkMode::SizeText => format_generated_table_length(group.diameter, units, precision),
        // The shape itself is drawn by the painter. The cell carries no text
        DrillMarkMode::Shape => String::new(),
    }
}

/// Port of `PCB_DRILL_CHART::generate( aBoard, aRefresh )`. `refresh` is the refresh's pending
/// drill symbol profile (`aRefresh->Pending<DRILL_PROFILE_PENDING>().m_Profile`).
pub fn generate(
    ctx: &BoardCtx<'_>,
    data: &GeneratedTableData,
    refresh: Option<&mut DrillSymbolProfile>,
) -> GeneratedTableContent {
    let GeneratedTableKind::DrillChart {
        filter,
        show_totals,
        ..
    } = &data.kind
    else {
        panic!("PCB_DRILL_CHART::generate on another table kind");
    };

    let units = data.units;
    let precision = data.precision;

    // A refresh can carry grouping the board has not been given yet, as the properties dialog
    // does
    let mut groups = match refresh {
        Some(profile) => {
            let mut groups = build_groups(ctx, filter, profile);

            // The refresh's copy, so a cancelled placement leaves nothing behind and a batch
            // rebuild accumulates instead of keeping only the last chart's
            assign_drill_symbols(&mut groups, profile);
            groups
        }
        None => {
            let profile = DrillSymbolProfile::from_board(ctx.board.design_settings());
            let mut groups = build_groups(ctx, filter, &profile);

            // Whole board, not this chart's filtered subset, or a filtered chart prints a
            // different symbol from the map for the same hole
            let resolved: BTreeMap<String, DrillSymbolAssignment> = resolve_drill_symbols(ctx);

            for group in groups.iter_mut() {
                if let Some(symbol) = resolved.get(&group.symbol_key) {
                    group.symbol = symbol.clone();
                }
            }

            groups
        }
    };

    let mut content = GeneratedTableContent::default();
    content.rows.reserve(groups.len());
    content.row_shapes.reserve(groups.len());

    let cols = data.columns.len();

    for col in 0..cols {
        if column_id(data, col) == Some(DrillChartColumnId::Symbol) {
            content.symbol_column = col as i32;
        }
    }

    let length = |value: i32| format_generated_table_length(value, units, precision);

    for group in groups.iter_mut() {
        let mut row = GeneratedTableRow {
            key: group.key.clone(),
            cells: Vec::with_capacity(cols),
        };

        if group.symbol.mark_mode == DrillMarkMode::Shape {
            content.row_shapes.push(Some(group.symbol.shape_index));
        } else {
            content.row_shapes.push(None);
        }

        for col in 0..cols {
            let text = match column_id(data, col) {
                Some(DrillChartColumnId::Symbol) => symbol_text(group, units, precision),
                Some(DrillChartColumnId::DrillDiameter) => length(group.diameter),
                Some(DrillChartColumnId::SlotSize) => {
                    if group.is_slot {
                        format!("{} x {}", length(group.size_xy.x), length(group.size_xy.y))
                    } else {
                        NO_VALUE.to_string()
                    }
                }
                Some(DrillChartColumnId::Plating) => {
                    if group.not_plated { "No" } else { "Yes" }.to_string()
                }
                Some(DrillChartColumnId::OperationCount) => format!("{}", group.operation_count),
                Some(DrillChartColumnId::SiteCount) => format!("{}", group.site_count),
                Some(DrillChartColumnId::LayerSpan) => span_text(ctx, group),
                Some(DrillChartColumnId::Operation) => operation_text(group).to_string(),
                Some(DrillChartColumnId::Protection) => protection_text(group),
                Some(DrillChartColumnId::BackdrillStub) => match group.stub_length {
                    Some(stub) => length(stub),
                    None => NO_VALUE.to_string(),
                },
                Some(DrillChartColumnId::AspectRatio) => {
                    // Depth over diameter, the number a fabricator uses to judge plating
                    // difficulty. Needs a real stackup, so it stays an em dash without one
                    let depth = ctx
                        .board
                        .get_stackup_or_default()
                        .get_layer_distance(group.top_layer, group.bottom_layer);

                    let diameter = group.diameter;

                    if depth > 0 && diameter > 0 {
                        format!("{}:1", format_f(depth as f64 / diameter as f64, 1))
                    } else {
                        NO_VALUE.to_string()
                    }
                }
                Some(DrillChartColumnId::Description) => group.symbol.description.clone(),
                // Unreachable for a validated column set (the C++ switch has no default and
                // leaves the cell empty)
                None => String::new(),
            };

            row.cells.push(text);
        }

        content.rows.push(row);
    }

    if *show_totals {
        let mut operations = 0;

        // Sites are counted over distinct coordinates, not summed per group. A backdrilled via
        // contributes several operations at one location and is still one site
        let mut sites: BTreeSet<(i32, i32)> = BTreeSet::new();

        for group in &groups {
            operations += group.operation_count;

            for site in &group.sites {
                sites.insert((site.x, site.y));
            }
        }

        content
            .trailer
            .push(vec![format!("{operations} OPS / {} SITES", sites.len())]);
    }

    content
}

/// Port of `PCB_DRILL_CHART::onColumnsReversed()`: the chart is not stale after a flip, so no
/// rebuild would move the marks to the new column.
pub fn on_columns_reversed(data: &mut GeneratedTableData) {
    let cols = data.columns.len() as i32;

    if let GeneratedTableKind::DrillChart { symbol_column, .. } = &mut data.kind {
        if *symbol_column >= 0 {
            *symbol_column = cols - 1 - *symbol_column;
        }
    }
}

/// Port of `PCB_DRILL_CHART::GetItemDescription`: `"Drill Chart (%d rows)"`.
pub fn item_description(rows: i32) -> String {
    format!("Drill Chart ({rows} rows)")
}
