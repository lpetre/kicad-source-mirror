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

//! Differential test of the polygon triangulation (package G4b) against the original C++
//! kimath code.
//!
//! `tests/oracle/tri_oracle.cpp` links the static libraries of the reference KiCad build,
//! triangulates pseudo-random chains, polygons with holes and KiCad-like zone fills
//! (`POLYGON_TRIANGULATION`, `SHAPE_POLY_SET::CacheTriangulation`), refines meshes, queries
//! `POLY_YSTRIPES_INDEX`, runs the triangle-based collisions and `SEG::IntersectsLine`, and
//! prints one line per section (case count and FNV-1a hash over the records of all inputs and
//! results).  Its output is committed as
//! `tests/golden/tri_oracle.txt`; this test replays the same inputs through the port
//! (`tests/oracle/tri_sections.rs`) and compares the hashes bit for bit.
//!
//! - Regenerate the golden file: `tests/oracle/build_tri_oracle.sh`.
//! - Find a mismatch: `build_tri_oracle.sh --dump cpp.txt`, run this test with
//!   `KICAD_GEOM_ORACLE_DUMP=rust.txt`, diff the files.
//!
//! Sections named `libm:*` depend on libm and are compared only if the `libm_probe` section
//! matches (see `oracle_diff.rs`).

#[allow(dead_code)]
#[path = "oracle/sections.rs"]
mod sections;

#[path = "oracle/tri_sections.rs"]
mod tri_sections;

#[test]
fn oracle_tri_differential_test() {
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/tri_oracle.txt");
    let golden_text =
        std::fs::read_to_string(&golden_path).expect("tests/golden/tri_oracle.txt is missing");
    let golden: Vec<(String, u64, u64)> = golden_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next().unwrap().to_string();
            let count: u64 = it.next().unwrap().parse().unwrap();
            let hash = u64::from_str_radix(it.next().unwrap(), 16).unwrap();
            (name, count, hash)
        })
        .collect();

    let dump: Option<Box<dyn std::io::Write>> =
        std::env::var_os("KICAD_GEOM_ORACLE_DUMP").map(|p| {
            let file = std::fs::File::create(p).expect("cannot create the dump file");
            Box::new(std::io::BufWriter::new(file)) as Box<dyn std::io::Write>
        });

    let mut results = sections::Results {
        sections: Vec::new(),
        dump,
    };

    tri_sections::run_all(&mut results);

    assert_eq!(
        results
            .sections
            .iter()
            .map(|s| s.0.as_str())
            .collect::<Vec<_>>(),
        golden.iter().map(|s| s.0.as_str()).collect::<Vec<_>>(),
        "the sections of the Rust test and of the golden file differ"
    );

    // Is this the libm the golden file was made with?
    let probe_matches = results.sections[0] == golden[0];
    let skip_libm = std::env::var_os("KICAD_GEOM_ORACLE_SKIP_LIBM").is_some() || !probe_matches;

    if !probe_matches {
        println!(
            "note: this platform's libm gives different results than the one the golden file was produced \
             with; the libm-dependent sections are not compared"
        );
    }

    let mut failures: Vec<String> = Vec::new();
    let mut compared_cases: u64 = 0;

    for (ours, theirs) in results.sections.iter().zip(golden.iter()) {
        let libm_dependent = ours.0.starts_with("libm:") || ours.0 == "libm_probe";

        if libm_dependent && skip_libm {
            println!("skipped  {} ({} cases)", ours.0, ours.1);
            continue;
        }

        if ours == theirs {
            println!("ok       {} ({} cases)", ours.0, ours.1);
            compared_cases += ours.1;
        } else {
            failures.push(format!(
                "section {}: got {} cases, hash {:016x}; expected {} cases, hash {:016x}",
                ours.0, ours.1, ours.2, theirs.1, theirs.2
            ));
        }
    }

    println!("{compared_cases} cases compared with the C++ oracle");

    assert!(
        failures.is_empty(),
        "results differ from the C++ oracle (see the module documentation for how to find the first \
         differing case):\n{}",
        failures.join("\n")
    );
}
