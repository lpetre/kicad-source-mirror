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

//! Port of the parts of `include/reporter.h` / `common/reporter.cpp` that the DRC uses:
//! the `REPORTER` interface and `WX_STRING_REPORTER`.
//!
//! (`REPORTER` is a `common/` class; it lives here until a shared home in `kicad-core`
//! exists, see this crate's `PORT_STATUS.md`.)

use kicad_core::Severity;

/// Port of `REPORTER`: a pure virtual class used to derive message reporter objects.
///
/// Only the members used by the DRC are ported (`Report( text, severity )`, `HasMessage`,
/// `HasMessageOfSeverity`, `Clear`). The base class bookkeeping (`m_reportedSeverityMask`)
/// is part of the trait contract: implementations OR every reported severity into a mask.
pub trait Reporter {
    /// Port of `REPORTER::Report( const wxString& aText, SEVERITY aSeverity )`.
    fn report(&mut self, text: &str, severity: Severity);

    /// Port of `REPORTER::HasMessage`: true if anything has been reported.
    fn has_message(&self) -> bool;

    /// Port of `REPORTER::HasMessageOfSeverity( int aSeverityMask )`.
    fn has_message_of_severity(&self, severity_mask: i32) -> bool;
}

/// Port of `WX_STRING_REPORTER`: a wrapper for reporting to a `wxString` object.
///
/// Additionally keeps every message with its severity (C++ only keeps the text), which tests
/// and the rule dump use.
#[derive(Clone, Debug, Default)]
pub struct WxStringReporter {
    /// `REPORTER::m_reportedSeverityMask`.
    reported_severity_mask: i32,
    /// `m_string`: every message followed by `"\n"`.
    string: String,
    /// The reported messages with their severities, in order.
    messages: Vec<(String, Severity)>,
}

impl WxStringReporter {
    /// `WX_STRING_REPORTER()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Port of `WX_STRING_REPORTER::GetMessages`.
    pub fn messages_text(&self) -> &str {
        &self.string
    }

    /// The reported messages with their severities, in order.
    pub fn messages(&self) -> &[(String, Severity)] {
        &self.messages
    }

    /// Port of `WX_STRING_REPORTER::Clear`.
    pub fn clear(&mut self) {
        self.reported_severity_mask = 0;
        self.string.clear();
        self.messages.clear();
    }
}

impl Reporter for WxStringReporter {
    /// Port of `WX_STRING_REPORTER::Report`.
    fn report(&mut self, text: &str, severity: Severity) {
        self.reported_severity_mask |= severity as i32;
        self.string.push_str(text);
        self.string.push('\n');
        self.messages.push((text.to_string(), severity));
    }

    fn has_message(&self) -> bool {
        self.reported_severity_mask != 0
    }

    fn has_message_of_severity(&self, severity_mask: i32) -> bool {
        (self.reported_severity_mask & severity_mask) != 0
    }
}

/// Reborrow an optional reporter for a shorter time (`Option::as_deref_mut` does not shorten
/// the trait object's lifetime bound, which `&mut` keeps invariant).
pub fn reborrow<'s>(reporter: &'s mut Option<&mut dyn Reporter>) -> Option<&'s mut dyn Reporter> {
    match reporter {
        Some(r) => Some(&mut **r),
        None => None,
    }
}
