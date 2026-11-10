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

//! The `REPORT( ... )` texts of `pcbnew/drc/drc_engine.cpp` (`EvalRules`,
//! `EvalZoneConnection`, `ProcessAssertions`; package DRC-34): the `REPORT` macro, the
//! `reportConstraintHeader` lambda of `EvalRules`, `PrintZoneConnection` of `pcbnew/zones.h`
//! and the `UNITS_PROVIDER` half of `DRC_ENGINE` the texts format their values with.
//!
//! The texts are the untranslated `_()` strings (the port never translates). They are what
//! the clearance resolution / constraint inspection tools show; no CLI report contains them.

use kicad_core::Severity;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::string_utils::escape_html;
use kicad_core::units_provider::UnitsProvider;
use kicad_pcb::ItemRef;
use kicad_pcb::zone_settings::ZoneConnection;

use super::DrcEngineConstraint;
use crate::constraint_types::DrcConstraintT;
use crate::reporter::Reporter;

/// The `REPORT( s )` macro of `drc_engine.cpp`: `if( aReporter ) aReporter->Report( s )`
/// (default severity `RPT_SEVERITY_UNDEFINED`). The text is only built when there is a
/// reporter ("all string manipulation MUST BE KEPT INSIDE the REPORT macro").
pub(crate) fn report(reporter: &mut Option<&mut dyn Reporter>, text: impl FnOnce() -> String) {
    if let Some(r) = reporter {
        r.report(&text(), Severity::Undefined);
    }
}

/// Port of `PrintZoneConnection` (`pcbnew/zones.h`).
pub fn print_zone_connection(connection: ZoneConnection) -> &'static str {
    match connection {
        ZoneConnection::Inherited => "inherited",
        ZoneConnection::None => "none",
        ZoneConnection::Thermal => "thermal reliefs",
        ZoneConnection::Full => "solid",
        ZoneConnection::ThtThermal => "thermal reliefs for PTH",
    }
}

/// The `UNITS_PROVIDER` of `DRC_ENGINE` (`pcbIUScale`, the engine's user units) as the report
/// texts use it.
pub(crate) struct ReportUnits(pub UnitsProvider<'static>);

impl ReportUnits {
    /// The engine's units provider in `units`.
    pub(crate) fn new(units: EdaUnits) -> Self {
        ReportUnits(UnitsProvider::new(&PCB_IU_SCALE, units))
    }

    /// `MessageTextFromValue( aValue )` (an `int` or `double` distance, with the unit label).
    pub(crate) fn value(&self, value: f64) -> String {
        self.0
            .message_text_from_value(value, true, EdaDataType::Distance)
    }

    /// `MessageTextFromValue( int )`.
    pub(crate) fn int(&self, value: i32) -> String {
        self.value(f64::from(value))
    }

    /// `MessageTextFromUnscaledValue( int )`.
    pub(crate) fn unscaled(&self, value: i32) -> String {
        self.0
            .message_text_from_unscaled_value(f64::from(value), true, EdaDataType::Distance)
    }

    /// `aItem->GetItemDescription( this, true )`.
    pub(crate) fn desc_raw(&self, item: &ItemRef<'_>) -> String {
        item.item_description(&self.0, true)
    }

    /// `EscapeHTML( aItem->GetItemDescription( this, true ) )`.
    pub(crate) fn desc(&self, item: &ItemRef<'_>) -> String {
        escape_html(&self.desc_raw(item))
    }
}

/// `wxT( "<i>" ) + _( "undefined" ) + wxT( "</i>" )`.
const UNDEFINED: &str = "<i>undefined</i>";

/// Port of the `reportConstraintHeader` lambda of `DRC_ENGINE::EvalRules`: a blank line, then
/// what the constraint `c` checks with its value.
pub(crate) fn report_constraint_header(
    reporter: &mut Option<&mut dyn Reporter>,
    units: &ReportUnits,
    c: &DrcEngineConstraint,
) {
    if reporter.is_none() {
        return;
    }

    report(reporter, String::new);

    let implicit = c.parent_rule.is_implicit();
    let name = || escape_html(&c.constraint.name());
    let v = &c.constraint.value;

    use DrcConstraintT::*;

    let text = match c.constraint.type_ {
        ClearanceConstraint
        | CourtyardClearanceConstraint
        | SilkClearanceConstraint
        | HoleClearanceConstraint
        | EdgeClearanceConstraint
        | PhysicalClearanceConstraint
        | PhysicalHoleClearanceConstraint => {
            format!("Checking {} clearance: {}.", name(), units.int(v.min()))
        }
        CreepageConstraint => format!("Checking {} creepage: {}.", name(), units.int(v.min())),
        MaxUncoupledConstraint => format!(
            "Checking {} max uncoupled length: {}.",
            name(),
            units.int(v.max())
        ),
        SkewConstraint => format!("Checking {} max skew: {}.", name(), units.int(v.max())),
        ThermalReliefGapConstraint => {
            format!("Checking {} gap: {}.", name(), units.int(v.min()))
        }
        ThermalSpokeWidthConstraint => format!(
            "Checking {} thermal spoke width: {}.",
            name(),
            units.int(v.opt())
        ),
        SolderMaskExpansionConstraint => format!(
            "Checking {} solder mask expansion: {}.",
            name(),
            units.int(v.opt())
        ),
        SolderPasteAbsMarginConstraint => format!(
            "Checking {} solder paste absolute clearance: {}.",
            name(),
            units.int(v.opt())
        ),
        SolderPasteRelMarginConstraint => format!(
            "Checking {} solder paste relative clearance: {}.",
            name(),
            units.int(v.opt())
        ),
        MinResolvedSpokesConstraint => format!(
            "Checking {} min spoke count: {}.",
            name(),
            units.unscaled(v.min())
        ),
        MicroviaStackDepthConstraint => format!(
            "Checking {} max microvia stack depth: {}.",
            name(),
            units.unscaled(v.max())
        ),
        // "%.3f" of a double that is an int / 1000 (never an exact decimal tie)
        HoleAspectRatioConstraint => format!(
            "Checking {} max hole aspect ratio: {:.3}.",
            name(),
            f64::from(v.max()) / 1000.0
        ),
        ZoneConnectionConstraint => format!(
            "Checking {} zone connection: {}.",
            name(),
            print_zone_connection(c.constraint.zone_connection)
        ),
        TrackWidthConstraint
        | AnnularWidthConstraint
        | ViaDiameterConstraint
        | HoleSizeConstraint
        | TextHeightConstraint
        | TextThicknessConstraint
        | DiffPairGapConstraint
        | LengthConstraint
        | NetChainLengthConstraint
        | NetChainStubLengthConstraint
        | NetChainReturnPathConstraint
        | ConnectionWidthConstraint
        | HoleToHoleConstraint => {
            if implicit {
                match c.constraint.type_ {
                    TrackWidthConstraint => {
                        if v.has_opt() {
                            format!(
                                "Checking {} track width: opt {}.",
                                name(),
                                units.int(v.opt())
                            )
                        } else if v.has_min() {
                            format!(
                                "Checking {} track width: min {}.",
                                name(),
                                units.int(v.min())
                            )
                        } else {
                            return;
                        }
                    }
                    AnnularWidthConstraint => format!(
                        "Checking {} annular width: min {}.",
                        name(),
                        units.int(v.opt())
                    ),
                    ViaDiameterConstraint => {
                        if v.has_opt() {
                            format!(
                                "Checking {} via diameter: opt {}.",
                                name(),
                                units.int(v.opt())
                            )
                        } else if v.has_min() {
                            format!(
                                "Checking {} via diameter: min {}.",
                                name(),
                                units.int(v.min())
                            )
                        } else {
                            return;
                        }
                    }
                    HoleSizeConstraint => {
                        if v.has_opt() {
                            format!("Checking {} hole size: opt {}.", name(), units.int(v.opt()))
                        } else if v.has_min() {
                            format!("Checking {} hole size: min {}.", name(), units.int(v.min()))
                        } else {
                            return;
                        }
                    }
                    TextHeightConstraint | TextThicknessConstraint | ConnectionWidthConstraint => {
                        format!("Checking {}: min {}.", name(), units.int(v.min()))
                    }
                    DiffPairGapConstraint => {
                        if v.has_opt() {
                            format!(
                                "Checking {} diff pair gap: opt {}.",
                                name(),
                                units.int(v.opt())
                            )
                        } else if v.has_min() {
                            format!("Checking {} clearance: min {}.", name(), units.int(v.min()))
                        } else {
                            return;
                        }
                    }
                    HoleToHoleConstraint => format!(
                        "Checking {} hole to hole: min {}.",
                        name(),
                        units.int(v.min())
                    ),
                    _ => format!("Checking {}.", name()),
                }
            } else {
                let or_undefined = |has: bool, value: i32| {
                    if has {
                        units.int(value)
                    } else {
                        UNDEFINED.to_string()
                    }
                };

                format!(
                    "Checking {}: min {}; opt {}; max {}.",
                    name(),
                    or_undefined(v.has_min(), v.min()),
                    or_undefined(v.has_opt(), v.opt()),
                    or_undefined(v.has_max(), v.max())
                )
            }
        }
        _ => format!("Checking {}.", name()),
    };

    report(reporter, || text);
}
