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

//! The `m_drcEngine` side of `pcbnew/drc/drc_test_provider.cpp` as the providers see it during
//! a run: [`RunHost`] lets the provider base helpers ([`crate::test_provider::DrcRunHost`])
//! report into the engine's run state ([`DrcRunState`]), and the `REPORT_AUX` macro of
//! `drc_test_provider.h`.
//!
//! Shared by every provider package (added by DRC-06, the first one).

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use kicad_core::Severity;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::pcb_marker::{PcbMarker, RcItem};

use crate::engine::DrcRunState;
use crate::reporter::Reporter;
use crate::test_provider::DrcRunHost;

/// The run state of the engine viewed as the provider base's run host. Dereferences to the run
/// state, so a provider calls `IsErrorLimitExceeded`, `IsCancelled`, ... on it directly.
pub struct RunHost<'a, 'r>(pub &'a mut DrcRunState<'r>);

impl<'r> Deref for RunHost<'_, 'r> {
    type Target = DrcRunState<'r>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl DerefMut for RunHost<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
    }
}

impl DrcRunHost for RunHost<'_, '_> {
    /// `DRC_ENGINE::ReportViolation` (the C++ always passes a path generator, possibly the
    /// empty default lambda).
    fn report_violation(
        &mut self,
        item: Arc<RcItem>,
        violating_test: &str,
        marker_pos: Vector2I,
        marker_layer: i32,
        path_generator: &dyn Fn(&mut PcbMarker),
    ) {
        self.0.report_violation(
            &item,
            violating_test,
            marker_pos,
            marker_layer,
            Some(path_generator),
        );
    }

    fn report_progress(&mut self, progress: f64) -> bool {
        self.0.report_progress(progress)
    }

    fn report_phase(&mut self, message: &str) -> bool {
        self.0.report_phase(message)
    }

    fn log_reporter(&mut self) -> Option<&mut dyn Reporter> {
        self.0.log_reporter()
    }
}

/// Port of the `REPORT_AUX( s )` macro of `drc_test_provider.h`:
/// `if( getLogReporter() ) getLogReporter()->Report( s, RPT_SEVERITY_INFO )`.
pub fn report_aux(run: &mut DrcRunState<'_>, message: &str) {
    if let Some(reporter) = run.log_reporter() {
        reporter.report(message, Severity::Info);
    }
}
