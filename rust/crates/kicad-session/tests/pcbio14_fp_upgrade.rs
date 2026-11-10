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

//! `run_fp_upgrade` (port of `PCBNEW_JOBS_HANDLER::JobExportFpUpgrade` over `FP_CACHE::Load` /
//! `Save`, package PCBIO-14): messages, exit codes and output files as the reference
//! `kicad-cli fp upgrade` gives them (checked by hand against the oracle, see PORT_STATUS.md of
//! kicad-pcb), and the library half of `ParseWarningDoesNotFailTheLibrary`
//! (`qa/tests/pcbnew/pcb_io/kicad_sexpr/test_kicad_sexpr.cpp`).

use kicad_session::jobs::JobFpUpgrade;
use kicad_session::{FootprintLibrary, Reporter, exit_codes, run_fp_upgrade};

fn source(name: &str) -> String {
    let path = kicad_qa::qa_data_dir()
        .join("libraries/Resistor_SMD.pretty")
        .join(format!("{name}.kicad_mod"));
    std::fs::read_to_string(path).unwrap()
}

fn job(force: bool, output: &str) -> JobFpUpgrade {
    let mut job = JobFpUpgrade::new();
    job.library_path = "in.pretty".into();
    job.output_library_path = output.into();
    job.force = force;
    job
}

/// `ParseWarningDoesNotFailTheLibrary`: a parse warning leaves the footprint usable, a file
/// that does not parse fails the whole library.
#[test]
fn parse_warning_does_not_fail_the_library() {
    let zero_pad = source("R_0402_1005Metric").replacen("(size 0.54 0.64)", "(size 0 0)", 1);
    let mut library: FootprintLibrary = vec![
        ("R_0402_1005Metric.kicad_mod".into(), zero_pad),
        (
            "R_0603_1608Metric.kicad_mod".into(),
            source("R_0603_1608Metric"),
        ),
    ];

    let mut reporter = Reporter::new();
    let outcome = run_fp_upgrade(&library, &mut job(true, "out.pretty"), &mut reporter).unwrap();
    assert_eq!(outcome.exit_code, exit_codes::SUCCESS);
    assert_eq!(reporter.text(), "");
    let names: Vec<&str> = outcome.files.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "out.pretty/R_0402_1005Metric.kicad_mod",
            "out.pretty/R_0603_1608Metric.kicad_mod"
        ]
    );

    // A file that does not parse still fails the library, as it always has
    library.push(("broken.kicad_mod".into(), "(footprint".into()));
    let mut reporter = Reporter::new();
    let outcome = run_fp_upgrade(&library, &mut job(true, "out.pretty"), &mut reporter).unwrap();
    assert_eq!(outcome.exit_code, exit_codes::ERR_UNKNOWN);
    assert!(outcome.files.is_empty());
    assert_eq!(reporter.text(), "Unable to load library\n");
}

/// Without `--force` a library already in the current format is not saved; an older one is
/// saved in place (no output path), named by the file (the footprint takes the file base name).
#[test]
fn saves_only_when_needed() {
    let old: FootprintLibrary = vec![("renamed.kicad_mod".into(), source("R_0402_1005Metric"))];
    let mut reporter = Reporter::new();
    let outcome = run_fp_upgrade(&old, &mut job(false, ""), &mut reporter).unwrap();
    assert_eq!(outcome.exit_code, exit_codes::SUCCESS);
    assert_eq!(outcome.files.len(), 1);
    assert_eq!(outcome.files[0].name, "in.pretty/renamed.kicad_mod");
    let text = String::from_utf8(outcome.files[0].contents.clone()).unwrap();
    assert!(text.starts_with(
        "(kicad_footprint_lib\n\t(version 20261031)\n\t(generator \"kicad_footprint_editor\")\n\t(generator_version \"10.99\")\n\t(footprint \"renamed\"\n"
    ), "{text}");

    let current: FootprintLibrary = vec![("renamed.kicad_mod".into(), text)];
    let mut reporter = Reporter::new();
    let outcome = run_fp_upgrade(&current, &mut job(false, ""), &mut reporter).unwrap();
    assert_eq!(outcome.exit_code, exit_codes::SUCCESS);
    assert!(outcome.files.is_empty());
    assert_eq!(reporter.text(), "Footprint library was not updated\n");
}
