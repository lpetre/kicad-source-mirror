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

//! Differential test of the spatial code against the original C++: libstdc++ `std::sort`,
//! `std::partial_sort`, `std::nth_element`, heaps and `std::priority_queue`
//! ([`kicad_geom::stdsort`]), `KIRTREE::PACKED_RTREE` / `DYNAMIC_RTREE`
//! ([`kicad_geom::geometry::rtree`]), delaunator-cpp and nanoflann
//! ([`kicad_geom::thirdparty`]).
//!
//! `tests/oracle/spatial_oracle.cpp` links the original headers / sources, drives them with a
//! deterministic pseudo-random generator and prints one line per section: the number of
//! cases and an FNV-1a hash over every result (final permutations including the order of
//! equal elements, r-tree iteration and query visit orders, early termination, nearest
//! neighbours, triangles / half-edges / hull, kd-tree results in their order, double bit
//! patterns).  Its output is committed as `tests/golden/spatial_oracle.txt`; this test replays
//! the same cases through the port (`tests/oracle/spatial_sections.rs`) and compares.
//!
//! No libm function is involved, so the comparison also holds on wasm32.
//!
//! To find the first differing case, run `tests/oracle/build_spatial_oracle.sh --dump
//! cpp_dump.txt`, set `KICAD_SPATIAL_ORACLE_DUMP=rust_dump.txt` when running this test and
//! diff the two files (one `section index hash` line per case).

#[path = "oracle/spatial_sections.rs"]
mod spatial_sections;

#[test]
fn spatial_oracle_differential_test() {
    let golden_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/spatial_oracle.txt");
    let golden_text =
        std::fs::read_to_string(&golden_path).expect("tests/golden/spatial_oracle.txt is missing");

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
        std::env::var_os("KICAD_SPATIAL_ORACLE_DUMP").map(|p| {
            let file = std::fs::File::create(p).expect("cannot create the dump file");
            Box::new(std::io::BufWriter::new(file)) as Box<dyn std::io::Write>
        });

    let mut results = spatial_sections::Results {
        sections: Vec::new(),
        dump,
    };

    spatial_sections::run_all(&mut results);

    let mut failures: Vec<String> = Vec::new();

    for (ours, theirs) in results.sections.iter().zip(golden.iter()) {
        if ours == theirs {
            println!("ok       {} ({} cases)", ours.0, ours.1);
        } else {
            failures.push(format!(
                "section {}: got {} cases, hash {:016x}; expected {} cases, hash {:016x}",
                ours.0, ours.1, ours.2, theirs.1, theirs.2
            ));
        }
    }

    assert_eq!(
        results.sections.len(),
        golden.len(),
        "the sections of the Rust test and of the golden file differ"
    );

    assert!(
        failures.is_empty(),
        "results differ from the C++ oracle:\n{}",
        failures.join("\n")
    );
}
