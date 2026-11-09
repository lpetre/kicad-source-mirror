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

//! The message sink of the job runners: the `REPORTER` the C++ jobs handler writes to
//! (`include/reporter.h`; in `kicad-cli` a `CLI_REPORTER`, `common/reporter.cpp`).
//!
//! The runners cannot print (no process access in library crates, `PORTING.md` 5), so they
//! record every `Report( aMsg, aSeverity )` call in order. `kicad-cli` replays them like
//! `CLI_REPORTER::Report`: `RPT_SEVERITY_ERROR` to stderr, everything else to stdout, debug
//! messages only when verbose. The message texts keep the C++ trailing `\n`.

use kicad_core::report_severity::Severity;
use serde::Serialize;

/// One `REPORTER::Report( aMsg, aSeverity )` call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReportLine {
    /// `aSeverity`.
    #[serde(serialize_with = "serialize_severity")]
    pub severity: Severity,
    /// `aMsg`, verbatim (usually ending in `\n`).
    pub text: String,
}

fn serialize_severity<S: serde::Serializer>(sev: &Severity, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(severity_name(*sev))
}

/// Lower-case name of a severity for the JavaScript side (`"error"`, `"warning"`, ...).
pub fn severity_name(sev: Severity) -> &'static str {
    match sev {
        Severity::Undefined => "undefined",
        Severity::Info => "info",
        Severity::Exclusion => "exclusion",
        Severity::Action => "action",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Ignore => "ignore",
        Severity::Debug => "debug",
    }
}

/// Ordered list of the messages a job reported (a `WX_STRING_REPORTER` that keeps the
/// severities).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Reporter {
    lines: Vec<ReportLine>,
}

impl Reporter {
    /// An empty reporter.
    pub fn new() -> Self {
        Reporter::default()
    }

    /// Port of `REPORTER::Report( aMsg, aSeverity )`.
    pub fn report(&mut self, text: impl Into<String>, severity: Severity) -> &mut Self {
        self.lines.push(ReportLine {
            severity,
            text: text.into(),
        });
        self
    }

    /// `REPORTER::Report( aMsg )` with the default `RPT_SEVERITY_UNDEFINED`.
    pub fn report_undefined(&mut self, text: impl Into<String>) -> &mut Self {
        self.report(text, Severity::Undefined)
    }

    /// The messages in report order.
    pub fn lines(&self) -> &[ReportLine] {
        &self.lines
    }

    /// Port of `REPORTER::HasMessageOfSeverity( RPT_SEVERITY_ERROR )`.
    pub fn has_error(&self) -> bool {
        self.lines.iter().any(|l| l.severity == Severity::Error)
    }

    /// All messages concatenated (the `WX_STRING_REPORTER::GetMessages()` text).
    pub fn text(&self) -> String {
        self.lines.iter().map(|l| l.text.as_str()).collect()
    }

    /// Take the recorded messages out.
    pub fn into_lines(self) -> Vec<ReportLine> {
        self.lines
    }
}
