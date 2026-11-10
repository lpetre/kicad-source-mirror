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

//! Port of `pcbnew/drill/drill_symbol_assigner.h` / `.cpp` (`AssignDrillSymbols`,
//! `ResolveDrillSymbols`, `DRILL_SYMBOL_ENTRY`, `ResolveDrillSymbolsByItem`) and of the board's
//! `DRILL_SYMBOL_CACHE` (`pcbnew/board.h` / `board.cpp` `BOARD::DrillSymbolCache`).

use std::collections::{BTreeMap, BTreeSet};

use kicad_core::Kiid;
use kicad_geom::{Box2I, EdaAngle, Vector2I};

use crate::ctx::BoardCtx;

use super::drill_chart_model::{DrillChartGroup, DrillChartModel, DrillChartTotals};
use super::drill_enumerator::{enumerate_drill_operations, enumerate_drill_spans};
use super::drill_operation::{DrillOpKind, DrillQuery, DrillSpan};
use super::drill_symbol_profile::{
    DrillMarkMode, DrillMarkPolicy, DrillSymbolAssignment, DrillSymbolProfile,
};

/// Port of `AssignDrillSymbols( aGroups, aProfile )`: give every group a mark, keeping the
/// ones the profile already records.
///
/// With freeze on, an existing group never changes symbol because a new hole size appeared;
/// new groups take the lowest mark nobody is using. Fabrication drawings get re-approved when
/// symbols move, so churn here is expensive for the user. `profile` gains assignments for any
/// group that did not have one.
pub fn assign_drill_symbols(groups: &mut [DrillChartGroup], profile: &mut DrillSymbolProfile) {
    let shape_count = kicad_plot::drill_markers::curated_shape_count();

    let mut used_shapes: BTreeSet<i32> = BTreeSet::new();
    let mut used_letters: BTreeSet<String> = BTreeSet::new();

    if profile.freeze_assignments() {
        for group in groups.iter_mut() {
            if let Some(existing) = profile.assignment(&group.symbol_key) {
                group.symbol = existing.clone();

                if existing.mark_mode == DrillMarkMode::Shape {
                    used_shapes.insert(existing.shape_index);
                } else if existing.mark_mode == DrillMarkMode::Letter {
                    used_letters.insert(existing.letter.clone());
                }
            }
        }
    }

    let next_letter = |used_letters: &BTreeSet<String>| -> String {
        for i in 0..26 * 27 {
            let candidate = if i < 26 {
                char::from(b'A' + i as u8).to_string()
            } else {
                let mut s = String::new();
                s.push(char::from(b'A' + (i / 26) as u8 - 1));
                s.push(char::from(b'A' + (i % 26) as u8));
                s
            };

            if !used_letters.contains(&candidate) {
                return candidate;
            }
        }

        "?".to_string()
    };

    for group in groups.iter_mut() {
        if profile.freeze_assignments() && profile.assignment(&group.symbol_key).is_some() {
            continue;
        }

        let mut assignment = DrillSymbolAssignment::default();

        match profile.mark_policy() {
            DrillMarkPolicy::SizeText => {
                assignment.mark_mode = DrillMarkMode::SizeText;
            }
            DrillMarkPolicy::Letters => {
                assignment.mark_mode = DrillMarkMode::Letter;
                assignment.letter = next_letter(&used_letters);
                used_letters.insert(assignment.letter.clone());
            }
            DrillMarkPolicy::Shapes | DrillMarkPolicy::ShapesThenLetters => {
                let mut shape = -1;

                for i in 0..shape_count {
                    if !used_shapes.contains(&i) {
                        shape = i;
                        break;
                    }
                }

                if shape >= 0 {
                    assignment.mark_mode = DrillMarkMode::Shape;
                    assignment.shape_index = shape;
                    used_shapes.insert(shape);
                } else if profile.mark_policy() == DrillMarkPolicy::ShapesThenLetters {
                    assignment.mark_mode = DrillMarkMode::Letter;
                    assignment.letter = next_letter(&used_letters);
                    used_letters.insert(assignment.letter.clone());
                } else {
                    // Out of distinguishable shapes, so reuse from the start rather than draw
                    // nothing at all
                    assignment.mark_mode = DrillMarkMode::Shape;
                    assignment.shape_index = 0;
                }
            }
        }

        group.symbol = assignment.clone();
        profile.set_assignment(&group.symbol_key, assignment);
    }
}

/// Port of `ResolveDrillSymbols( aBoard )`: the symbol every hole should carry, without
/// touching the board.
///
/// A drill map has to draw the same marks whether or not a chart has ever been placed, so
/// consumers resolve through here rather than relying on the profile already holding
/// assignments. Assignment happens on a copy. Only a chart rebuild commits new ones.
pub fn resolve_drill_symbols(ctx: &BoardCtx<'_>) -> BTreeMap<String, DrillSymbolAssignment> {
    let mut profile = DrillSymbolProfile::from_board(ctx.board.design_settings());

    let mut groups: Vec<DrillChartGroup> = {
        let mut model = DrillChartModel::new(&profile);
        model.build(ctx, &enumerate_drill_spans(ctx, false));
        model.groups().to_vec()
    };

    assign_drill_symbols(&mut groups, &mut profile);

    let mut resolved = BTreeMap::new();

    for group in &groups {
        resolved.insert(group.symbol_key.clone(), group.symbol.clone());
    }

    resolved
}

/// `DRILL_SYMBOL_ENTRY`: one drawable mark, with the geometry the renderer needs to place it.
///
/// The diameter is carried because a size-text mark has to print the hole it labels.
#[derive(Clone, Debug, PartialEq)]
pub struct DrillSymbolEntry {
    pub symbol: DrillSymbolAssignment, // m_Symbol
    pub span: DrillSpan,               // m_Span
    /// m_Position: where the mark is drawn, which is the hole itself.
    pub position: Vector2I,
    pub kind: DrillOpKind,     // m_Kind
    pub diameter: i32,         // m_Diameter
    pub size_xy: Vector2I,     // m_SizeXY
    pub orientation: EdaAngle, // m_Orientation
    pub is_slot: bool,         // m_IsSlot
}

/// Port of `ResolveDrillSymbolsByItem( aBoard, aResolved )`: the same answer keyed by the
/// item that owns the holes (`std::map<KIID, ...>`). One item maps to several entries: a
/// backdrilled via owns its primary drill and both backdrills.
pub fn resolve_drill_symbols_by_item(
    ctx: &BoardCtx<'_>,
    resolved: &BTreeMap<String, DrillSymbolAssignment>,
) -> BTreeMap<Kiid, Vec<DrillSymbolEntry>> {
    let profile = DrillSymbolProfile::from_board(ctx.board.design_settings());

    let mut by_item: BTreeMap<Kiid, Vec<DrillSymbolEntry>> = BTreeMap::new();

    let mut query = DrillQuery {
        merge_pth_npth: true,
        ..DrillQuery::default()
    };

    for span in enumerate_drill_spans(ctx, false) {
        query.span = span;

        for op in enumerate_drill_operations(ctx, &query) {
            let Some(symbol) = resolved.get(&profile.group_key_string(&op)) else {
                continue;
            };

            by_item
                .entry(op.source_id)
                .or_default()
                .push(DrillSymbolEntry {
                    symbol: symbol.clone(),
                    span,
                    position: op.position,
                    kind: op.kind,
                    diameter: op.diameter,
                    size_xy: op.size_xy,
                    orientation: op.orientation,
                    is_slot: op.is_slot,
                });
        }
    }

    by_item
}

/// `DRILL_SYMBOL_CACHE` (`pcbnew/board.h`): everything the renderers need to draw drill
/// symbols, resolved once. The generation / profile fingerprint of the C++ cache are not
/// kept: [`drill_symbol_cache`] computes it afresh (same values, no reuse).
#[derive(Clone, Debug, Default)]
pub struct DrillSymbolCache {
    pub by_group: BTreeMap<String, DrillSymbolAssignment>, // m_ByGroup
    pub by_item: BTreeMap<Kiid, Vec<DrillSymbolEntry>>,    // m_ByItem
    /// m_Totals: counts for the `${DRILL_*}` text variables.
    pub totals: DrillChartTotals,
    /// m_HoleExtent: extent of every hole that carries a mark.
    pub hole_extent: Box2I,
}

/// Port of the rebuild of `BOARD::DrillSymbolCache()`: the resolved symbols by group and by
/// item, the totals of an unfiltered model and the hole extent.
pub fn drill_symbol_cache(ctx: &BoardCtx<'_>) -> DrillSymbolCache {
    let mut cache = DrillSymbolCache {
        by_group: resolve_drill_symbols(ctx),
        ..DrillSymbolCache::default()
    };
    cache.by_item = resolve_drill_symbols_by_item(ctx, &cache.by_group);

    let profile = DrillSymbolProfile::from_board(ctx.board.design_settings());
    let mut totals_model = DrillChartModel::new(&profile);
    totals_model.build(ctx, &enumerate_drill_spans(ctx, false));
    cache.totals = totals_model.totals();

    for entries in cache.by_item.values() {
        for entry in entries {
            cache.hole_extent.merge_point(entry.position);
        }
    }

    cache
}
