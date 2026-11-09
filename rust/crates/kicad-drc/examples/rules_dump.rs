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

//! Rust twin of `pcb-dump rules [--component-classes] FILE...` (see
//! `rust/docs/PCB_DUMP_FORMAT.md` section 7), with the stand-in expression binding of the
//! tests. Run: `cargo run -p kicad-drc --example rules_dump -- FILE...` and diff against the
//! C++ output.

#[path = "../tests/common/mod.rs"]
mod common;

fn main() {
    let mut component_classes = false;
    let mut files = Vec::new();

    for arg in std::env::args().skip(1) {
        if arg == "--component-classes" {
            component_classes = true;
        } else {
            files.push(arg);
        }
    }

    for file in files {
        let name = std::path::Path::new(&file)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        println!("file|{name}|");

        match std::fs::read(&file) {
            Ok(bytes) => {
                // wxString::FromUTF8: empty for invalid UTF-8
                let text = String::from_utf8(bytes).unwrap_or_default();

                for line in common::dump_rules(&text, &name, component_classes) {
                    println!("{line}");
                }
            }
            Err(_) => println!("error|rules|{name}|cannot read file"),
        }
    }
}
