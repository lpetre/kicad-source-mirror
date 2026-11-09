// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2007-2012 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
// Copyright (C) 2019 Jean-Pierre Charras, jp.charras@wanadoo.fr
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

//! Legacy layer numbering helpers used when reading old board files.
//!
//! KiCad has had three board layer numbering schemes:
//!
//! 1. The `.brd` scheme (before 2013): copper layers 0 (back) .. 15 (front), technical layers
//!    16 .. 28, stored as 32 bit masks.  Port of `PCB_IO_KICAD_LEGACY::leg_layer2new` and
//!    `leg_mask2new` from `pcbnew/pcb_io/kicad_legacy/pcb_io_kicad_legacy.cpp`:
//!    [`leg_layer2new`], [`leg_mask2new`].
//! 2. The pre-V9 `.kicad_pcb` scheme: `F_Cu` = 0, `In1_Cu` .. `In30_Cu` = 1 .. 30, `B_Cu` = 31,
//!    technical and user layers 32 .. 59, stored as 64 bit hex masks.  Port of
//!    `LEGACY_PCB_LAYER_ID`, `s_legacyLayerIdMap` and `remapLegacyLayerLSET` from
//!    `pcbnew/pcb_plot_params.cpp`: [`LEGACY_PCB_LAYER_ID_COUNT`],
//!    [`remap_legacy_layer_lset`].  The per-id conversion of this scheme is
//!    [`crate::layer_ids::board_layer_from_legacy_id`] (`BoardLayerFromLegacyId`).
//! 3. The current scheme, see [`crate::layer_ids::PcbLayerId`].

use crate::base_set::BaseSet;
use crate::layer_ids::{PcbLayerId, board_layer_from_legacy_id};
use crate::lset::Lset;

/// `LEGACY_PCB_LAYER_ID_COUNT`: the number of layer ids of the pre-V9 numbering.
///
/// These are the layer IDs from before 5e0abadb23425765e164f49ee2f893e94ddb97fc, and are
/// needed for mapping old PCB files to the new layer numbering.  (`LEGACY_Rescue` is 59; the
/// four ids 60 - 63 were reserved for future expansion within the 64 bit integer limit.)
pub const LEGACY_PCB_LAYER_ID_COUNT: usize = 60;

/// Port of `remapLegacyLayerLSET` (`pcbnew/pcb_plot_params.cpp`): translate a pre-V9 layer
/// set (for instance one read with `BASE_SET::ParseHex` from an old `layerselection` plot
/// parameter) into the current layer numbering.
///
/// `legacy_lset` is expected to hold [`LEGACY_PCB_LAYER_ID_COUNT`] bits; missing bits are
/// read as cleared.
pub fn remap_legacy_layer_lset(legacy_lset: &BaseSet) -> Lset {
    let mut new_layers = Lset::new();

    // s_legacyLayerIdMap maps every LEGACY_PCB_LAYER_ID (0 .. 59) exactly like
    // BoardLayerFromLegacyId() does.
    for legacy_layer in 0..LEGACY_PCB_LAYER_ID_COUNT {
        let new_layer = board_layer_from_legacy_id(legacy_layer as i32);
        new_layers.set_value(new_layer, legacy_lset.test(legacy_layer));
    }

    new_layers
}

/// `.brd` layer number of the back copper layer (`LAYER_N_BACK`).
pub const LAYER_N_BACK: i32 = 0;
/// `.brd` layer number of the front copper layer (`LAYER_N_FRONT`).
pub const LAYER_N_FRONT: i32 = 15;
/// First `.brd` non-copper layer number (`FIRST_NON_COPPER_LAYER`).
pub const FIRST_NON_COPPER_LAYER: i32 = 16;
/// `ADHESIVE_N_BACK`.
pub const ADHESIVE_N_BACK: i32 = 16;
/// `ADHESIVE_N_FRONT`.
pub const ADHESIVE_N_FRONT: i32 = 17;
/// `SOLDERPASTE_N_BACK`.
pub const SOLDERPASTE_N_BACK: i32 = 18;
/// `SOLDERPASTE_N_FRONT`.
pub const SOLDERPASTE_N_FRONT: i32 = 19;
/// `SILKSCREEN_N_BACK`.
pub const SILKSCREEN_N_BACK: i32 = 20;
/// `SILKSCREEN_N_FRONT`.
pub const SILKSCREEN_N_FRONT: i32 = 21;
/// `SOLDERMASK_N_BACK`.
pub const SOLDERMASK_N_BACK: i32 = 22;
/// `SOLDERMASK_N_FRONT`.
pub const SOLDERMASK_N_FRONT: i32 = 23;
/// `DRAW_N`.
pub const DRAW_N: i32 = 24;
/// `COMMENT_N`.
pub const COMMENT_N: i32 = 25;
/// `ECO1_N`.
pub const ECO1_N: i32 = 26;
/// `ECO2_N`.
pub const ECO2_N: i32 = 27;
/// `EDGE_N`.
pub const EDGE_N: i32 = 28;
/// Last `.brd` non-copper layer number (`LAST_NON_COPPER_LAYER`).
pub const LAST_NON_COPPER_LAYER: i32 = 28;
/// `.brd` bit mask of all copper layers (`ALL_CU_LAYERS`).
pub const ALL_CU_LAYERS: u32 = 0x0000_FFFF;

/// Port of `is_leg_copperlayer_valid`: return true if `legacy_layer_num` is a valid copper
/// layer legacy id, therefore top, bottom or inner activated layer.
pub fn is_leg_copperlayer_valid(cu_count: i32, legacy_layer_num: i32) -> bool {
    legacy_layer_num == LAYER_N_FRONT || legacy_layer_num < cu_count
}

/// Port of `PCB_IO_KICAD_LEGACY::leg_layer2new`: convert a `.brd` layer number into a
/// `PCB_LAYER_ID`, given the number of copper layers of the board.
pub fn leg_layer2new(cu_count: i32, layer_num: i32) -> PcbLayerId {
    let mut newid: i32;
    let old = layer_num as u32;

    // this is a speed critical function, be careful.

    if old <= LAYER_N_FRONT as u32 {
        // In .brd files, the layers are numbered from back to front
        // (the opposite of the .kicad_pcb files)
        if old == LAYER_N_FRONT as u32 {
            newid = PcbLayerId::FCu.0;
        } else if old == LAYER_N_BACK as u32 {
            newid = PcbLayerId::BCu.0;
        } else {
            newid = board_layer_from_legacy_id(cu_count.wrapping_sub(1).wrapping_sub(old as i32)).0;

            // This is of course incorrect, but at least it avoid crashing pcbnew:
            if newid < 0 {
                newid = 0;
            }
        }
    } else {
        newid = match old as i32 {
            ADHESIVE_N_BACK => PcbLayerId::BAdhes.0,
            ADHESIVE_N_FRONT => PcbLayerId::FAdhes.0,
            SOLDERPASTE_N_BACK => PcbLayerId::BPaste.0,
            SOLDERPASTE_N_FRONT => PcbLayerId::FPaste.0,
            SILKSCREEN_N_BACK => PcbLayerId::BSilkS.0,
            SILKSCREEN_N_FRONT => PcbLayerId::FSilkS.0,
            SOLDERMASK_N_BACK => PcbLayerId::BMask.0,
            SOLDERMASK_N_FRONT => PcbLayerId::FMask.0,
            DRAW_N => PcbLayerId::DwgsUser.0,
            COMMENT_N => PcbLayerId::CmtsUser.0,
            ECO1_N => PcbLayerId::Eco1User.0,
            ECO2_N => PcbLayerId::Eco2User.0,
            EDGE_N => PcbLayerId::EdgeCuts.0,
            // Remap all illegal non copper layers to comment layer
            _ => PcbLayerId::CmtsUser.0,
        };
    }

    PcbLayerId(newid)
}

/// Port of `PCB_IO_KICAD_LEGACY::leg_mask2new`: convert a `.brd` layer mask into an `LSET`,
/// given the number of copper layers of the board.
pub fn leg_mask2new(cu_count: i32, mask: u32) -> Lset {
    let mut ret = Lset::new();
    let mut mask = mask;

    if (mask & ALL_CU_LAYERS) == ALL_CU_LAYERS {
        ret = Lset::all_cu_mask().clone();

        mask &= !ALL_CU_LAYERS;
    }

    let mut i = 0;

    while mask != 0 {
        if (mask & 1) != 0 {
            ret.set(leg_layer2new(cu_count, i));
        }

        i += 1;
        mask >>= 1;
    }

    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_pre_v9_mask() {
        // 0x00010fc_ffffffff: all copper + the usual plot layers of an old board.
        let mut legacy = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);
        assert_eq!(legacy.parse_hex("00010fc_ffffffff"), 16);

        let new = remap_legacy_layer_lset(&legacy);

        let mut expected = Lset::all_cu_mask().clone();

        for layer in [
            PcbLayerId::BPaste,
            PcbLayerId::FPaste,
            PcbLayerId::BSilkS,
            PcbLayerId::FSilkS,
            PcbLayerId::BMask,
            PcbLayerId::FMask,
            PcbLayerId::EdgeCuts,
        ] {
            expected.set(layer);
        }

        assert_eq!(new, expected);
        assert_eq!(new.size(), 128);

        // Every legacy id maps to a distinct current id.
        let mut all = BaseSet::new(LEGACY_PCB_LAYER_ID_COUNT);
        all.set_all();
        assert_eq!(remap_legacy_layer_lset(&all).count(), 60);
    }

    #[test]
    fn brd_layers() {
        assert_eq!(leg_layer2new(2, LAYER_N_FRONT), PcbLayerId::FCu);
        assert_eq!(leg_layer2new(2, LAYER_N_BACK), PcbLayerId::BCu);
        // 4 layer board: .brd inner layers 1 and 2 are numbered from the back.
        assert_eq!(leg_layer2new(4, 1), PcbLayerId::In2Cu);
        assert_eq!(leg_layer2new(4, 2), PcbLayerId::In1Cu);
        // Inner layer beyond the stack: newid < 0 is clamped to F_Cu.
        assert_eq!(leg_layer2new(2, 5), PcbLayerId::FCu);
        assert_eq!(leg_layer2new(4, SILKSCREEN_N_FRONT), PcbLayerId::FSilkS);
        assert_eq!(leg_layer2new(4, EDGE_N), PcbLayerId::EdgeCuts);
        assert_eq!(leg_layer2new(4, 29), PcbLayerId::CmtsUser);
        assert_eq!(leg_layer2new(4, -1), PcbLayerId::CmtsUser);

        assert!(is_leg_copperlayer_valid(4, LAYER_N_FRONT));
        assert!(is_leg_copperlayer_valid(4, 3));
        assert!(!is_leg_copperlayer_valid(4, 4));

        let through_hole = leg_mask2new(2, 0x00C0_FFFF);
        let mut expected = Lset::all_cu_mask().clone();
        expected.set(PcbLayerId::BMask).set(PcbLayerId::FMask);
        assert_eq!(through_hole, expected);

        let smd = leg_mask2new(2, 0x0088_8000);
        assert_eq!(
            smd,
            Lset::from([PcbLayerId::FCu, PcbLayerId::FPaste, PcbLayerId::FMask])
        );
    }
}
