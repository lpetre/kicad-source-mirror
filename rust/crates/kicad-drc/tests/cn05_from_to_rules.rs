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

//! CN-05: the rule function `fromTo()` (`fromToFunc`, `pcbnew/pcbexpr_functions.cpp`) with
//! the from-to cache of `kicad_connectivity::from_to`, against the reference `kicad-cli pcb
//! drc`: for each pair of `tests/golden/cn05_from_to_cli.txt` (`gen_cn05_from_to_cli.py`) the
//! tracks for which the condition `A.fromTo( from, to )` holds are exactly the tracks the
//! reference flags with a `track_width` rule under that condition on `demos/ecc83/ecc83-pp`.
//!
//! The condition is evaluated directly (`DRC_RULE_CONDITION::EvaluateFor`, the call
//! `EvalRules` makes for the width provider), on a connectivity built and a from-to cache
//! rebuilt as `DRC_ENGINE::RunTests` does.

use kicad_connectivity::data::ConnectivityData;
use kicad_core::KicadT;
use kicad_drc::rule_condition::DrcRuleCondition;
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::DrcConstraintT;
use kicad_pcb::io::{ParseOptions, parse_board};

#[test]
fn cn05_from_to_conditions_match_kicad_cli() {
    let golden = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cn05_from_to_cli.txt"),
    )
    .unwrap();
    let path = kicad_qa::demos_dir().join("ecc83/ecc83-pp.kicad_pcb");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut board = parse_board(&text, "", &ParseOptions::default()).unwrap();

    let mut data = ConnectivityData::new();
    assert!(data.build_connectivity(&mut board));
    data.rebuild_from_to_cache(&BoardCtx::bare(&board));

    let ctx = BoardCtx::bare(&board).with_connectivity(&data);
    let tracks: Vec<_> = board
        .tracks()
        .map(|(id, _)| ctx.item(id.into()).unwrap())
        .filter(|t| matches!(t.type_(), KicadT::PcbTraceT | KicadT::PcbArcT))
        .collect();

    let mut pairs = 0;

    for line in golden.lines().filter(|l| l.starts_with("pair|")) {
        let f: Vec<&str> = line.split('|').collect();
        let (from, to) = (f[1], f[2]);
        let expected: Vec<&str> = f[3].split(',').filter(|s| !s.is_empty()).collect();

        let mut condition = DrcRuleCondition::new(&format!("A.fromTo('{from}','{to}')"));
        assert!(condition.compile(None, 0, 0), "{from} -> {to}");

        let mut got: Vec<String> = tracks
            .iter()
            .filter(|t| {
                condition.evaluate_for(
                    Some(**t),
                    None,
                    DrcConstraintT::TrackWidthConstraint as i32,
                    t.layer(),
                    None,
                )
            })
            .map(|t| t.uuid().as_std_string())
            .collect();
        got.sort();

        assert_eq!(got, expected, "{from} -> {to}");
        pairs += 1;
    }

    assert_eq!(pairs, 7);
}
