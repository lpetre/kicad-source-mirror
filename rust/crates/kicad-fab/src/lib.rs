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

//! Port of KiCad's fabrication outputs: board layer plotting, drill files, Gerber job file.
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md`,
//! `rust/docs/analysis/plot-fab.md` (module layout: section 10.1) and `PORT_STATUS.md`.
//!
//! Nothing in this crate touches the file system, the clock or the environment: writers
//! produce [`OutputFile`] values and report through a [`Reporter`]; the host (`kicad-cli`,
//! wasm bindings) writes the files and prints the messages.

pub mod drill;
pub mod exporters;
pub mod jobs;
pub mod jobs_handler;
pub mod line_ending_plot;
pub mod pcb_plotter;
pub mod pcbplot;
pub mod plot_board_layers;
pub mod plot_brditems_plotter;
pub mod plot_brditems_shapes;
pub mod plot_brditems_text;
pub mod plot_layer_poly;
pub mod wx_file_name;

use kicad_core::report_severity::Severity;

/// One file produced by a writer (the port's replacement for writing to disk). `name` is the
/// path the C++ code would have written to (directory part as given by the caller).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFile {
    pub name: String,
    pub contents: Vec<u8>,
}

/// Errors of the fabrication outputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FabError {
    /// The code path needs a part of the port that does not exist yet (names the C++ function
    /// and the work package that ports it).
    NotPorted(&'static str),
    /// An option value the port does not support (e.g. a PDF drill map), with a message.
    Unsupported(String),
}

impl std::fmt::Display for FabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FabError::NotPorted(what) => write!(f, "not ported yet: {what}"),
            FabError::Unsupported(msg) => write!(f, "unsupported: {msg}"),
        }
    }
}

impl std::error::Error for FabError {}

/// Port of the part of `REPORTER` (`include/reporter.h`) the fabrication writers use:
/// `Report( aText, aSeverity )`. A C++ `REPORTER* = nullptr` is an `Option<&mut dyn Reporter>`
/// of `None`.
pub trait Reporter {
    /// `REPORTER::Report`.
    fn report(&mut self, text: &str, severity: Severity);
}

/// A reporter that keeps every message in order (useful for hosts and tests).
#[derive(Clone, Debug, Default)]
pub struct VecReporter {
    pub messages: Vec<(String, Severity)>,
}

impl Reporter for VecReporter {
    fn report(&mut self, text: &str, severity: Severity) {
        self.messages.push((text.to_string(), severity));
    }
}
