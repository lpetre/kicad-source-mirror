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

//! Test support (new code): loading through the session's load sequences while some model
//! steps still reach `port_pending!` stubs of other packages.

#![allow(dead_code)]

use std::panic::{AssertUnwindSafe, catch_unwind};

use kicad_pcb::project::{LoadPath, LoadStep};
use kicad_session::board_loader::load_guarded;
use kicad_session::{LoadInputs, Session, SessionError};

/// The `port_pending!` stubs a load step may reach today, with the step. A step stopped by one
/// of these is skipped and reported; any other panic fails the test. Remove an entry when its
/// package lands.
pub const KNOWN_PENDING_STEPS: &[(LoadStep, &str)] = &[];

/// The text of a panic payload.
pub fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(non-string panic)".to_string())
}

/// Silences the panic messages of the known pending stubs (the default hook otherwise).
pub fn quiet_pending_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();

    ONCE.call_once(|| {
        let default = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !panic_text(info.payload()).contains("port pending") {
                default(info);
            }
        }));
    });
}

/// [`Session::load_board_with`] (`LoadPath::Cli`) or [`Session::load_board_qa`], with the steps
/// that reach a [`KNOWN_PENDING_STEPS`] stub skipped. Returns the session and the skipped
/// steps; another panic of a step is re-raised.
pub fn load_staged(
    path: LoadPath,
    inputs: &LoadInputs<'_>,
) -> Result<(Session, Vec<String>), SessionError> {
    quiet_pending_panics();
    let mut skipped = Vec::new();
    let mut guard = |step: LoadStep, run: &mut dyn FnMut()| {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(run)) {
            let text = panic_text(payload.as_ref());

            if KNOWN_PENDING_STEPS
                .iter()
                .any(|(s, f)| *s == step && text.ends_with(f))
            {
                skipped.push(format!("{step:?}: {text}"));
            } else {
                std::panic::resume_unwind(payload);
            }
        }
    };

    let loaded = load_guarded(path, inputs, Some(&mut guard))?;
    Ok((Session::from_loaded(loaded, inputs), skipped))
}
