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

//! Work package DRC-33: the creepage provider (`drc_test_provider_creepage.cpp`), the
//! creepage engine (`drc_creepage_engine.cpp`) and part 2 of `drc_creepage_utils.cpp`,
//! verified against the reference `kicad-cli` reports of the corpus, plus the ported QA tests
//! (`qa/tests/pcbnew/drc/test_drc_creepage*.cpp`, `test_creepage_engine_*.cpp`,
//! `qa/tests/pcbnew/test_creepage_groove_order.cpp`).

mod drc_harness;

/// The corpus boards with a `creepage` rule (`grep -l creepage qa/data/pcbnew/*/*.kicad_dru`).
const CREEPAGE_BOARDS: &[&str] = &[
    "qa/data/pcbnew/creepage/creepage.kicad_pcb",
    "qa/data/pcbnew/creepage/creepage_malformed_edge.kicad_pcb",
    "qa/data/pcbnew/creepage_slots/creepage_slots.kicad_pcb",
    "qa/data/pcbnew/issue20480/issue20480.kicad_pcb",
    "qa/data/pcbnew/issue21482/issue21482.kicad_pcb",
    "qa/data/pcbnew/issue23389/issue23389.kicad_pcb",
    "qa/data/pcbnew/issue23578/issue23578.kicad_pcb",
    "qa/data/pcbnew/issue23658/issue23658.kicad_pcb",
    "qa/data/pcbnew/issue24078/issue24078.kicad_pcb",
    "qa/data/pcbnew/issue24286/issue24286.kicad_pcb",
    "qa/data/pcbnew/issue24523/issue24523.kicad_pcb",
    "qa/data/pcbnew/issue24543/issue24543.kicad_pcb",
    "qa/data/pcbnew/issue24544/issue24544.kicad_pcb",
    "qa/data/pcbnew/issue24597/issue24597.kicad_pcb",
    "qa/data/pcbnew/issue24853/issue24853.kicad_pcb",
    "qa/data/pcbnew/issue24875/issue24875.kicad_pcb",
    "qa/data/pcbnew/issue24875_capstraight/issue24875_capstraight.kicad_pcb",
    "qa/data/pcbnew/issue24875_fullround/issue24875_fullround.kicad_pcb",
    "qa/data/pcbnew/issue24885/issue24885.kicad_pcb",
    "qa/data/pcbnew/issue25439/issue25439.kicad_pcb",
];

/// Corpus replay of the `creepage` error code against the reference reports
/// (`tests/golden/drc_corpus.txt`), through the real run path with the creepage provider only.
#[test]
fn drc33_creepage_corpus() {
    let mut total = drc_harness::Scoreboard::default();

    for board in CREEPAGE_BOARDS {
        let score = drc_harness::compare_corpus_flow(&["creepage"], board, &["creepage"]);

        for (t, s) in score.per_type {
            let e = total.per_type.entry(t).or_default();
            e.matched += s.matched;
            e.missing += s.missing;
            e.extra += s.extra;
        }

        total.compared.extend(score.compared);
        total.pending.extend(score.pending);
        total.differences.extend(score.differences);
        total.partial_differences.extend(score.partial_differences);
    }

    total.print("DRC-33 creepage");
    total.assert_no_mismatch();
}

#[test]
#[ignore = "diagnostic"]
fn drc33_debug_one() {
    let board = std::env::var("DRC33_BOARD").unwrap_or_default();
    let path = kicad_qa::kicad_source_root().join(&board);
    let mut loaded = drc_harness::load_board(&path).unwrap();
    let found = drc_harness::run_flow_providers(
        &mut loaded,
        kicad_core::eda_units::EdaUnits::Mm,
        false,
        &["creepage"],
    );
    eprintln!("{} violations", found.len());
}
