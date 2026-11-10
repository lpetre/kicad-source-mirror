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

//! Test support shared by the golden tests (new code, no C++ origin): where the board and the
//! goldens of a golden name live. STAB-CN added the corpus boards with footprints of
//! `tests/oracle/stab_cn_boards.txt` (goldens by `tests/oracle/gen_stab_cn.sh` under
//! `tests/golden/stab_cn/`).

#![allow(dead_code)]

use std::path::{Path, PathBuf};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `(golden name, path relative to <kicad>/qa/data without extension)` of the STAB-CN boards,
/// in list order.
pub fn stab_cn_entries() -> Vec<(String, String)> {
    let path = manifest_dir().join("tests/oracle/stab_cn_boards.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));

    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|rel| {
            let name = rel.rsplit('/').next().unwrap().to_string();
            (name, rel.to_string())
        })
        .collect()
}

/// The golden names of the STAB-CN boards.
pub fn stab_cn_boards() -> Vec<String> {
    stab_cn_entries().into_iter().map(|(n, _)| n).collect()
}

fn stab_cn_rel(name: &str) -> Option<String> {
    stab_cn_entries()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| r)
}

/// Whether `name` is a STAB-CN board.
pub fn is_stab_cn(name: &str) -> bool {
    stab_cn_rel(name).is_some()
}

/// The board file of a golden: the synthetic boards of `gen_cn04.py` (`rev_*`, `cn04_*`) in
/// `tests/golden/cn04/`, the STAB-CN boards under `<kicad>/qa/data`, else a board of
/// `qa/data/pcbnew`.
pub fn board_path(name: &str) -> PathBuf {
    if name.starts_with("rev_") || name.starts_with("cn04_") {
        manifest_dir()
            .join("tests/golden/cn04")
            .join(format!("{name}.kicad_pcb"))
    } else if let Some(rel) = stab_cn_rel(name) {
        kicad_qa::qa_data_dir().join(format!("{rel}.kicad_pcb"))
    } else {
        kicad_qa::pcbnew_data_dir().join(format!("{name}.kicad_pcb"))
    }
}

/// The `connectivity` golden of a board, relative to `tests/golden` without `.txt`.
pub fn golden_name(name: &str) -> String {
    if is_stab_cn(name) {
        format!("stab_cn/{name}")
    } else {
        name.to_string()
    }
}

/// The `flash.<layer>` golden of a corpus board, relative to `tests/golden` without `.txt`.
pub fn flash_golden_name(name: &str) -> String {
    if is_stab_cn(name) {
        format!("stab_cn/{name}.flash")
    } else {
        format!("cn04_flash/{name}")
    }
}

/// The pcb-dump item keys of a board (`InitItemKeys`, `kicad_pcb::dump::format`): the UUID
/// when it is unique among the walked items and present in the file text, else
/// `#<ordinal>` in the dump's walk order.
pub fn dump_item_keys(
    board: &kicad_pcb::Board,
    file_text: &str,
) -> std::collections::HashMap<kicad_pcb::ids::ItemId, String> {
    use kicad_pcb::dump::format::{board_items, scan_file_uuids};

    let in_file = scan_file_uuids(file_text.as_bytes());
    let items = board_items(board);
    let ctx = kicad_pcb::ctx::BoardCtx::bare(board);
    let uuid_of = |id| ctx.item(id).map(|i| i.uuid().as_std_string());
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for &id in &items {
        if let Some(u) = uuid_of(id) {
            *counts.entry(u).or_default() += 1;
        }
    }

    let nil = kicad_core::kiid::NILUUID.as_std_string();
    let mut keys = std::collections::HashMap::new();

    for (ordinal, &id) in items.iter().enumerate() {
        let Some(u) = uuid_of(id) else { continue };
        let key = if u == nil || counts[&u] > 1 || !in_file.contains(&u) {
            format!("#{ordinal}")
        } else {
            u
        };
        keys.insert(id, key);
    }

    keys
}
