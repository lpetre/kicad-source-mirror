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

//! Port of `pcbnew/pad_utils.h` and `pcbnew/pad_utils.cpp` (namespace `PAD_UTILS`):
//! rounding-radius helpers, drill removal, attribute names and the pad consistency checks
//! (package M13).

use kicad_core::PcbLayerId;
use kicad_core::units_provider::UnitsProvider;

use crate::ctx::BoardCtx;
use crate::pad::Pad;
use crate::padstack::{PadAttrib, Padstack};

/// `PAD_UTILS::PAD_CHECK_CODE`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum PadCheckCode {
    PadSizeNotPositive,
    PthHoleTooSmall,
    PthHoleLeavesNoCopper,
    PthHoleNotInsideCopper,
    HoleNotInsidePad,
    NegativeLocalClearance,
    NegativeMaskClearanceTooLarge,
    NegativePasteMarginTooLarge,
    RoundrectRadiusNegative,
    RoundrectRadiusTooLarge,
    ChamferNegative,
    ChamferTooLarge,
    TrapezoidDeltaTooLarge,
    CustomShapeNotSinglePolygon,
    ThNoHole,
    PthNoOuterCopper,
    UnexpectedHole,
    MultipleCopperLayers,
    MaskWrongSide,
    PasteWrongSide,
    PadNoOuterCopper,
    ConnHasPaste,
    FiducialNotPlated,
    TestpointNotPlated,
    HeatsinkNotPlated,
    CastellatedNotPth,
    BgaNotSmd,
    MechanicalNotPth,
    PressfitNotPth,
    NonComponentNoCopper,
    NonComponentTechOnInner,
    NonComponentInnerNeedsCustomStackup,
}

/// `PAD_UTILS::PAD_CHECK_RESULT`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadCheckResult {
    pub code: PadCheckCode,
    pub message: String,
}

/// `PAD_UTILS::PadHasMeaningfulRoundingRadius( aPad, aLayer )`.
pub fn pad_has_meaningful_rounding_radius(
    ctx: &BoardCtx<'_>,
    pad: &Pad,
    layer: PcbLayerId,
) -> bool {
    crate::port_pending!("PAD_UTILS::PadHasMeaningfulRoundingRadius", ctx, pad, layer)
}

/// `PAD_UTILS::GetDefaultIpcRoundingRatio( aPad, aLayer )`.
pub fn default_ipc_rounding_ratio(ctx: &BoardCtx<'_>, pad: &Pad, layer: PcbLayerId) -> f64 {
    crate::port_pending!("PAD_UTILS::GetDefaultIpcRoundingRatio", ctx, pad, layer)
}

/// `PAD_UTILS::RemoveAllDrillsFromPadstack( aPadstack )`.
pub fn remove_all_drills_from_padstack(padstack: &mut Padstack) {
    crate::port_pending!("PAD_UTILS::RemoveAllDrillsFromPadstack", padstack)
}

/// `PAD_UTILS::GetPadAttributeString( aAttrib )` (translated UI string, untranslated here).
pub fn pad_attribute_string(attrib: PadAttrib) -> String {
    crate::port_pending!("PAD_UTILS::GetPadAttributeString", attrib)
}

/// `PAD_UTILS::CheckPad( aPad, aUnitsProvider, aForPadProperties, aErrorHandler )`.
pub fn check_pad(
    ctx: &BoardCtx<'_>,
    pad: &Pad,
    units: &UnitsProvider<'_>,
    for_pad_properties: bool,
    error_handler: &mut dyn FnMut(&PadCheckResult),
) {
    crate::port_pending!(
        "PAD_UTILS::CheckPad",
        ctx,
        pad,
        units,
        for_pad_properties,
        error_handler
    )
}
