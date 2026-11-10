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

//! Port of `pcbnew/pcb_stackup_table.h` / `.cpp` (`PCB_STACKUP_TABLE`: `generate`,
//! `styleNewCell`) and of `pcbnew/board_tables/stackup_table_model.h` / `.cpp`
//! (`BuildStackupTableRows`; `StackupTableSchema()` and `STACKUP_TABLE_COLUMN_ID` are in
//! [`crate::pcb_generated_table`], package M11).

use kicad_core::base_units::UNITY_SCALE;
use kicad_core::eda_units::ui::string_from_value;
use kicad_core::layer_ids::is_valid_layer;
use kicad_core::string_utils::initial_caps;
use kicad_core::{EdaDataType, EdaUnits, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::Vector2I;

use crate::board::Board;
use crate::board_stackup::{BoardStackupItem, BoardStackupItemType, NOT_SPECIFIED_PRM};
use crate::ctx::{BoardCtx, MutEnv};
use crate::pcb_generated_table::{
    GeneratedTableColumn, GeneratedTableContent, GeneratedTableData, GeneratedTableRow,
    GeneratedTableUnits, StackupTableColumnId, format_generated_table_length,
};
use crate::pcb_tablecell::PcbTablecell;

/// Port of `PCB_STACKUP_TABLE::generate( aBoard, aRefresh )`: one row per stackup layer
/// (`BuildStackupTableRows`), no trailer.
pub fn generate(ctx: &BoardCtx<'_>, data: &GeneratedTableData) -> GeneratedTableContent {
    GeneratedTableContent {
        rows: build_stackup_table_rows(ctx.board, &data.columns, data.units, data.precision),
        ..GeneratedTableContent::default()
    }
}

/// Port of `PCB_STACKUP_TABLE::styleNewCell( aCell, aIsHeading )`: 1.5 mm text, 0.3 mm thick
/// for a heading, else 0.2 mm.
pub fn style_new_cell(cell: &mut PcbTablecell, env: &MutEnv, is_heading: bool) {
    cell.set_text_size(
        env,
        Vector2I::new(PCB_IU_SCALE.mm_to_iu(1.5), PCB_IU_SCALE.mm_to_iu(1.5)),
        true,
    );
    cell.set_text_thickness(
        env,
        PCB_IU_SCALE.mm_to_iu(if is_heading { 0.3 } else { 0.2 }),
    );
}

/// Port of `PCB_STACKUP_TABLE::GetItemDescription`: `"Stackup Table (%d rows)"`.
pub fn item_description(rows: i32) -> String {
    format!("Stackup Table ({rows} rows)")
}

/// The anonymous-namespace `layerName( aBoard, aItem, aSublayer )`.
fn layer_name(board: &Board, item: &BoardStackupItem, sublayer: usize) -> String {
    // Dielectric names are built like the gerber job file's, since the stackup's own are not
    // very good
    if item.type_() == BoardStackupItemType::Dielectric {
        let mut name = String::from("Dielectric");

        if item.sublayers_count() < 2 {
            name += &format!(" {}", item.dielectric_layer_id());
        } else {
            name += &format!(
                " {} ({}/{})",
                item.dielectric_layer_id(),
                sublayer + 1,
                item.sublayers_count()
            );
        }

        return name;
    }

    // The stackup keeps the name from when Board Setup last closed, so a later rename would be
    // missed
    if is_valid_layer(item.brd_layer_id()) {
        return board.layer_name(item.brd_layer_id());
    }

    item.layer_name().to_string()
}

/// The anonymous-namespace `cellText( aItem, aSublayer, aName, aColumnId, aUnits,
/// aPrecision )`.
fn cell_text(
    item: &BoardStackupItem,
    sublayer: usize,
    name: &str,
    column_id: i32,
    units: GeneratedTableUnits,
    precision: i32,
) -> String {
    use StackupTableColumnId as C;

    let unscaled = |value: f64| {
        string_from_value(
            &UNITY_SCALE,
            EdaUnits::Unscaled,
            value,
            false,
            EdaDataType::Distance,
        )
    };

    match column_id {
        x if x == C::LayerName as i32 => name.to_string(),
        x if x == C::Type as i32 => initial_caps(item.type_name()),
        x if x == C::Material as i32 => item.material(sublayer),
        x if x == C::Thickness as i32 => {
            // Layers like silkscreen, paste, etc. have no defined thickness, but that does not
            // mean that they are specified as exactly 0mm
            if !item.is_thickness_editable() {
                return NOT_SPECIFIED_PRM.to_string();
            }

            // The table's own units, so the drawing does not change with the editor's display
            // units
            format_generated_table_length(item.thickness(sublayer), units, precision)
        }
        x if x == C::Color as i32 => item.color(sublayer),
        x if x == C::EpsilonR as i32 => unscaled(item.epsilon_r(sublayer)),
        x if x == C::LossTangent as i32 => unscaled(item.loss_tangent(sublayer)),
        _ => String::new(),
    }
}

/// `canonicalName( aLayer )`: `BOARD::GetStandardLayerName`.
fn canonical_name(layer: PcbLayerId) -> String {
    Board::standard_layer_name(layer)
}

/// Port of `BuildStackupTableRows( aBoard, aColumns, aUnits, aPrecision )`: one row per stackup
/// layer, and per sublayer of a split dielectric, top to bottom.
///
/// Reads a synchronized copy of the stackup so a stale stored stackup is reported as the board
/// now is without being rewritten. Keys are `"L:<canonical layer name>"` for board layers and
/// `"D:<canonical name of the copper layer above>:<sublayer index>"` for dielectrics.
pub fn build_stackup_table_rows(
    board: &Board,
    columns: &[GeneratedTableColumn],
    units: GeneratedTableUnits,
    precision: i32,
) -> Vec<GeneratedTableRow> {
    // A copy, so generating the table never rewrites a stale stored stackup behind the user's
    // back
    let mut stackup = board.get_stackup_or_default();
    stackup.synchronize_with_board(board.design_settings());

    let mut rows = Vec::new();
    let mut copper_above = PcbLayerId::UndefinedLayer;

    for item in stackup.list() {
        if item.type_() == BoardStackupItemType::Copper {
            copper_above = item.brd_layer_id();
        }

        for sublayer in 0..item.sublayers_count() {
            // Canonical names, so a rename does not move a row's formatting onto a different
            // layer
            let key = if item.type_() == BoardStackupItemType::Dielectric {
                format!("D:{}:{}", canonical_name(copper_above), sublayer)
            } else {
                format!("L:{}", canonical_name(item.brd_layer_id()))
            };

            let name = layer_name(board, item, sublayer);

            let cells = columns
                .iter()
                .map(|column| cell_text(item, sublayer, &name, column.id, units, precision))
                .collect();

            rows.push(GeneratedTableRow { key, cells });
        }
    }

    rows
}
