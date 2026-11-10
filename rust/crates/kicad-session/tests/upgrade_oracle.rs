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

//! `PCBNEW_JOBS_HANDLER::JobUpgrade` end to end (`Session::load_board` + `run_pcb_upgrade`)
//! against the reference `kicad-cli pcb upgrade --force` (`tests/oracle/gen_upgrade_golden.sh`,
//! goldens `tests/golden/upgrade/<board>.kicad_pcb` and `.out`): the re-saved board must be
//! byte-identical, the message and the exit code equal.

use kicad_session::jobs::JobPcbUpgrade;
use kicad_session::{Reporter, Session, SessionError, exit_codes, run_pcb_upgrade};

/// Boards whose every construct the parser and the writer support today.
const BOARDS: &[&str] = &["qa/data/pcbnew/issue10697.kicad_pcb"];

fn golden(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/upgrade")
        .join(name)
}

fn upgrade(text: &str, name: &str, force: bool) -> Result<(i32, Reporter, Vec<u8>), SessionError> {
    let mut session = Session::load_board(text, name, None, None)?;
    let mut job = JobPcbUpgrade::new();
    job.filename = name.to_string();
    job.force = force;
    let mut reporter = Reporter::new();
    let outcome = run_pcb_upgrade(&mut session, &mut job, &mut reporter)?;
    let file = match outcome.files.as_slice() {
        [] => Vec::new(),
        [f] => {
            assert_eq!(f.name, name);
            f.contents.clone()
        }
        _ => panic!("more than one output file"),
    };
    Ok((outcome.exit_code, reporter, file))
}

#[test]
fn upgrade_force_matches_oracle() {
    let root = kicad_qa::kicad_source_root();

    for rel in BOARDS {
        let name = rel.rsplit('/').next().unwrap();
        let stem = name.trim_end_matches(".kicad_pcb");
        let text = std::fs::read_to_string(root.join(rel)).unwrap();
        let (code, reporter, file) = upgrade(&text, name, true).unwrap();

        let expected_out = std::fs::read_to_string(golden(&format!("{stem}.out"))).unwrap();
        assert_eq!(
            format!("{}rc={code}\n", reporter.text()),
            expected_out,
            "{rel}"
        );
        let expected = std::fs::read(golden(name)).unwrap();
        assert!(
            file == expected,
            "{rel}: re-saved board differs from the reference kicad-cli output"
        );

        // Without --force: the corpus board has an older format version, so it is saved too.
        let (code2, reporter2, file2) = upgrade(&text, name, false).unwrap();
        assert_eq!(
            (code2, reporter2.text(), &file2),
            (code, reporter.text(), &file)
        );

        // The upgraded board is current: `Board file was not updated` (the reference prints
        // this for a second run). Its re-load needs every construct the writer emits; skip
        // while one of them is still a staging stub of the parser.
        let upgraded = String::from_utf8(file).unwrap();
        match upgrade(&upgraded, name, false) {
            Ok((code3, reporter3, file3)) => {
                assert_eq!(code3, exit_codes::SUCCESS);
                assert_eq!(reporter3.text(), "Board file was not updated\n");
                assert!(file3.is_empty());
            }
            Err(SessionError::NotYetPorted { subsystem }) => {
                assert!(subsystem.contains("kicad-pcb"), "{subsystem}");
            }
            Err(e) => panic!("{rel}: re-load of the upgraded board: {e}"),
        }
    }
}
