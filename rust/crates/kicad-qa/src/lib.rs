//! Test-support helpers for the KiCad Rust port.
//!
//! The Rust workspace lives inside the KiCad source tree (`<kicad>/rust`), so tests can use
//! the original C++ QA data (`<kicad>/qa/data`) and demo projects (`<kicad>/demos`) directly.

use std::path::{Path, PathBuf};

pub mod board_file_utils;
pub mod io_corpus;
pub mod sexpr_text;
pub mod uuid_canon;

/// Root of the KiCad source tree (the directory containing `qa/`, `demos/`, `pcbnew/`, ...).
pub fn kicad_source_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <kicad>/rust/crates/kicad-qa
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("kicad-qa must live in <kicad>/rust/crates/kicad-qa")
        .to_path_buf()
}

/// `<kicad>/qa/data`, the data directory used by the C++ unit tests
/// (the C++ tests call this `KI_TEST::GetTestDataRootDir()` / `KICAD_TEST_DATA_DIR`).
pub fn qa_data_dir() -> PathBuf {
    kicad_source_root().join("qa").join("data")
}

/// `<kicad>/qa/data/pcbnew`, where `KI_TEST::LoadBoard()` looks boards up by relative name.
pub fn pcbnew_data_dir() -> PathBuf {
    qa_data_dir().join("pcbnew")
}

/// `<kicad>/demos`.
pub fn demos_dir() -> PathBuf {
    kicad_source_root().join("demos")
}

/// Path of the reference `kicad-cli` built from the C++ tree, if one is available.
///
/// Differential tests should call this and silently skip when it returns `None`.
/// Honours the `KICAD_ORACLE_CLI` environment variable, then falls back to the
/// conventional location `/home/user/kicad-oracle/bin/kicad-cli`.
pub fn oracle_cli() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("KICAD_ORACLE_CLI") {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    let p = PathBuf::from("/home/user/kicad-oracle/bin/kicad-cli");
    p.is_file().then_some(p)
}

/// Recursively collect files below `dir` with the given extension (without the dot), sorted.
pub fn files_with_extension(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|e| e.to_str()) == Some(ext) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_dirs_exist() {
        assert!(qa_data_dir().is_dir());
        assert!(pcbnew_data_dir().is_dir());
        assert!(demos_dir().is_dir());
    }
}
