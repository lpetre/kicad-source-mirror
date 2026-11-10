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

//! Differential test of `convertLayerArg` and the layer selection of `JobExportGerbers`
//! against the reference `kicad-cli` (`tests/oracle/gen_gerbers_layer_args.sh`, golden
//! `tests/golden/gerbers_layer_args.txt`): for each `--layers` argument the oracle's
//! `Invalid layer name` messages and the plotted layers, in plot order (`LSET::UIOrder`), must
//! be what the port computes.
//!
//! The board (`via_stacks.kicad_pcb`) is not loaded (the parser is pending): its enabled layers
//! are the ones the oracle plots for the empty layer list, and its user layer names equal the
//! GUI names, so the port runs with `user_layer_name = None`.

use kicad_core::layer_ids::layer_name;
use kicad_core::{Lset, PcbLayerId};
use kicad_session::jobs::JobExportPcbGerbers;
use kicad_session::jobs_handler::gerbers_layers_to_plot;
use kicad_session::reporter::Reporter;

/// `BuildPlotFileName` suffix of a layer: the GUI name with `.` replaced by `_`.
fn suffix(layer: PcbLayerId) -> String {
    layer_name(layer).replace('.', "_")
}

struct Case {
    arg: String,
    messages: Vec<String>,
    plotted: Vec<String>,
}

fn cases() -> Vec<Case> {
    let golden = include_str!("golden/gerbers_layer_args.txt");
    let mut out: Vec<Case> = Vec::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("CASE [") {
            let end = rest.rfind("] rc=").unwrap();
            assert_eq!(&rest[end + 5..], "0");
            out.push(Case {
                arg: rest[..end].to_string(),
                messages: Vec::new(),
                plotted: Vec::new(),
            });
        } else if let Some(p) = line.strip_prefix("P ") {
            let name = p.rsplit_once(' ').unwrap().0;
            out.last_mut().unwrap().plotted.push(name.to_string());
        } else {
            out.last_mut().unwrap().messages.push(format!("{line}\n"));
        }
    }

    out
}

#[test]
fn layer_arguments_match_kicad_cli() {
    let cases = cases();
    assert_eq!(cases[0].arg, "");

    // Enabled layers: what the oracle plots without a layer list.
    let all: Vec<PcbLayerId> = Lset::all_layers_mask().seq().0;
    let enabled: Lset = cases[0]
        .plotted
        .iter()
        .map(|s| {
            *all.iter()
                .find(|l| suffix(**l) == *s)
                .unwrap_or_else(|| panic!("{s}"))
        })
        .collect();

    for case in &cases {
        let mut job = JobExportPcbGerbers::new();
        job.gerber.plot.arg_layers = Some(case.arg.clone());
        job.gerber.plot.arg_common_layers = Some(String::new());
        let mut reporter = Reporter::new();
        let layers = gerbers_layers_to_plot(&mut job, &enabled, None, None, &mut reporter);
        let plotted: Vec<String> = layers.ui_order().0.into_iter().map(suffix).collect();
        let messages: Vec<String> = reporter.lines().iter().map(|l| l.text.clone()).collect();

        assert_eq!(messages, case.messages, "messages for [{}]", case.arg);
        assert_eq!(plotted, case.plotted, "layers for [{}]", case.arg);
    }

    assert_eq!(cases.len(), 22);
}
