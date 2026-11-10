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

//! The zone connections provider's second connectivity (`run_flow::LazyConnectivity`) is
//! built at its first query only. Both paths against the reference reports of the DRC corpus,
//! through the real run path (cache generator, from-to rebuild, the provider slot):
//!
//! * `issue24865`: pads with `remove_unused_layers` reach `FlashLayer` in the provider's
//!   `testZoneLayer`, so the connectivity IS built (one of the two corpus boards that build
//!   it, the other being `issue22826`; found with a temporary build counter over the whole
//!   corpus).
//! * `interf_u`: copper zones and two reference `starved_thermal` entries, but nothing
//!   queries the connectivity, so it is never built.

mod drc_harness;

use drc_harness::compare_corpus_flow;

/// The provider's error type.
const TYPES: &[&str] = &["starved_thermal"];

fn assert_board_matches(filter: &str) {
    let score = compare_corpus_flow(TYPES, filter, &["zone connections"]);
    score.print(filter);

    assert_eq!(
        score.compared.len(),
        1,
        "{filter}: not compared: {:?}",
        score.pending
    );
    score.assert_no_mismatch();
}

/// The lazy connectivity is built (flash-layer queries of `remove_unused_layers` pads).
#[test]
fn queried_connectivity_matches_reference() {
    assert_board_matches("issue24865/issue24865.kicad_pcb");
}

/// The lazy connectivity is never built (no query); the provider's entries are unchanged.
#[test]
fn unqueried_connectivity_matches_reference() {
    assert_board_matches("interf_u/interf_u.kicad_pcb");
}
