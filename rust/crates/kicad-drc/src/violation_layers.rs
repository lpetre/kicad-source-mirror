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

//! Port of `DRC_ITEM::GetViolationLayers` (`pcbnew/drc/drc_item.cpp`, package DRC-34): the
//! layer a violation focuses on when it is selected, and the layers it is visible on.

use kicad_core::Kiid;
use kicad_core::layer_ids::UNDEFINED_LAYER;
use kicad_core::{Lset, PcbLayerId};
use kicad_pcb::drc_api::{DRCE_EDGE_CLEARANCE, DRCE_INVALID_OUTLINE, DRCE_MALFORMED_COURTYARD};
use kicad_pcb::pcb_marker::{PcbMarker, RcItem};
use kicad_pcb::{BoardCtx, ItemRef};

use crate::item::{aux_item2_id, aux_item3_id};

/// The result of [`violation_layers`]: `aPrincipalLayer` and `aViolationLayers`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViolationLayers {
    /// `aPrincipalLayer` (`UNDEFINED_LAYER` when nothing decides it).
    pub principal_layer: PcbLayerId,
    /// `aViolationLayers`.
    pub violation_layers: Lset,
}

/// `BOARD::ResolveItem( aID, true )` as a handle.
fn resolve<'a>(ctx: &'a BoardCtx<'a>, id: &Kiid) -> Option<ItemRef<'a>> {
    ctx.board.resolve_item(id).and_then(|item| ctx.item(item))
}

/// The `getActiveLayers` lambda: the layer set of the item; for a pad only the layers it is
/// flashed on.
fn active_layers(item: &ItemRef<'_>) -> Lset {
    if let Some(pad) = item.as_pad() {
        let mut layers = Lset::new();

        for layer in item.layer_set().iter() {
            if pad.flash_layer(item.ctx, layer) {
                layers.set(layer);
            }
        }

        layers
    } else {
        item.layer_set()
    }
}

/// Port of `DRC_ITEM::GetViolationLayers( aBoard, aItem, aMarker, aPrincipalLayer,
/// aViolationLayers )`.
pub fn violation_layers<'a>(
    ctx: &'a BoardCtx<'a>,
    item: &RcItem,
    marker: Option<&PcbMarker>,
) -> ViolationLayers {
    let mut principal_layer = UNDEFINED_LAYER;
    let mut violation_layers = Lset::new();

    let a = resolve(ctx, &item.main_item_id());
    let b = resolve(ctx, &item.aux_item_id());
    let c = resolve(ctx, &aux_item2_id(item));
    let d = resolve(ctx, &aux_item3_id(item));

    let code = item.error_code();

    if code == DRCE_MALFORMED_COURTYARD {
        let flags = a
            .and_then(|a| a.as_footprint())
            .map(|fp| fp.courtyard_flags(ctx));

        if flags.is_some_and(|f| f.malformed_back && !f.malformed_front) {
            principal_layer = PcbLayerId::BCrtYd;
        } else {
            principal_layer = PcbLayerId::FCrtYd;
        }
    } else if code == DRCE_INVALID_OUTLINE || code == DRCE_EDGE_CLEARANCE {
        principal_layer = PcbLayerId::EdgeCuts;
    } else {
        // The marker's layer is set by the test provider
        if let Some(marker) = marker {
            let marker_layer = marker.base.layer;

            if marker_layer.0 > UNDEFINED_LAYER.0 {
                principal_layer = marker_layer;
            }
        }

        // Fall back to intersecting the layer sets of the contributing items
        if principal_layer.0 <= UNDEFINED_LAYER.0 {
            if a.is_some() || b.is_some() || c.is_some() || d.is_some() {
                violation_layers = Lset::all_layers_mask().clone();
            }

            for it in [a, b, c, d].into_iter().flatten() {
                let layers_list = active_layers(&it);
                violation_layers &= &layers_list;

                if principal_layer.0 <= UNDEFINED_LAYER.0 && layers_list.count() > 0 {
                    principal_layer = layers_list.seq().0[0];
                }
            }
        }
    }

    if violation_layers.count() > 0 {
        principal_layer = violation_layers.seq().0[0];
    } else if principal_layer.0 >= 0 {
        violation_layers.set(principal_layer);
    }

    ViolationLayers {
        principal_layer,
        violation_layers,
    }
}
