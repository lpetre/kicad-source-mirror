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

//! Placeholders for the `PCB_VIA` accessors the drill enumerator calls
//! (`pcbnew/pcb_track.h` / `.cpp`) and that the board model does not declare yet.
//!
//! The via model package M17 (`board-model.md` section 16) ports these as inherent `PcbVia`
//! methods; until then every function here panics like a `port_pending!` stub of the model.
//! Nothing is re-implemented here. When M17 lands, each call site in `drill_enumerator.rs`
//! is replaced by the model method of the same C++ name and this file is deleted (requested
//! from M17 in the FAB-01 result: `PcbVia::drill_value(ctx)`, `layer_pair(ctx)`,
//! `resolve_filling_mode(ctx)`, `resolve_capping_mode(ctx)`,
//! `resolve_{front,back}_{covering,plugging,tenting}_mode(ctx)`).

use kicad_core::layer_ids::PcbLayerId;
use kicad_pcb::BoardCtx;
use kicad_pcb::pcb_track::{
    CappingMode, CoveringMode, FillingMode, PcbVia, PluggingMode, TentingMode,
};

/// `PCB_VIA::GetDrillValue()` (netclass default when the via drill is undefined).
pub(crate) fn drill_value(ctx: &BoardCtx<'_>, via: &PcbVia) -> i32 {
    kicad_pcb::port_pending!("PCB_VIA::GetDrillValue (M17)", ctx, via)
}

/// `PCB_VIA::LayerPair( &top, &bottom )`.
pub(crate) fn layer_pair(ctx: &BoardCtx<'_>, via: &PcbVia) -> (PcbLayerId, PcbLayerId) {
    kicad_pcb::port_pending!("PCB_VIA::LayerPair (M17)", ctx, via)
}

/// `PCB_VIA::ResolveFillingMode()`.
pub(crate) fn resolve_filling_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> FillingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveFillingMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveCappingMode()`.
pub(crate) fn resolve_capping_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> CappingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveCappingMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveFrontCoveringMode()`.
pub(crate) fn resolve_front_covering_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> CoveringMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveFrontCoveringMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveBackCoveringMode()`.
pub(crate) fn resolve_back_covering_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> CoveringMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveBackCoveringMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveFrontPluggingMode()`.
pub(crate) fn resolve_front_plugging_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> PluggingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveFrontPluggingMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveBackPluggingMode()`.
pub(crate) fn resolve_back_plugging_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> PluggingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveBackPluggingMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveFrontTentingMode()`.
pub(crate) fn resolve_front_tenting_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> TentingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveFrontTentingMode (M17)", ctx, via)
}

/// `PCB_VIA::ResolveBackTentingMode()`.
pub(crate) fn resolve_back_tenting_mode(ctx: &BoardCtx<'_>, via: &PcbVia) -> TentingMode {
    kicad_pcb::port_pending!("PCB_VIA::ResolveBackTentingMode (M17)", ctx, via)
}
