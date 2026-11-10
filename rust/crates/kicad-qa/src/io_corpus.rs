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

//! The board corpus of the reader / writer differential harness and its oracle cache (new
//! code, test support; `docs/analysis/pcb-io.md` 9.2 / 9.3 D2, package PCBIO-17).
//!
//! * [`io_corpus_boards`]: every `.kicad_pcb` under `qa/data/pcbnew` and `demos`, with the
//!   skip list (the very large boards, the known-bad file).
//! * [`ensure_oracle_cache`]: for each board, `O1` = the reference `kicad-cli pcb upgrade
//!   --force` of a copy of the board file alone (no project files) and `O2` = the same re-save
//!   of `O1`, cached under `<kicad>/rust/target/io-corpus/boards/` (never in the repository).
//!   The cache key is the FNV-1a 64 hash and size of the input plus the modification times of
//!   the oracle binaries, so a re-run only calls the oracle for new or changed boards or after
//!   an oracle rebuild. The oracle runs with a private `KICAD_ORACLE_HOME` per worker, at most
//!   two at a time, under `nice -n 15`.
//!
//! A board is a *fixed point* when `O1 == O2`; only those are byte-exact targets of
//! `rust_resave( O1 )` (D2 compares with `O2` either way: net codes and track order can change
//! once more after a legacy load).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::UNIX_EPOCH;

use crate::{demos_dir, files_with_extension, kicad_source_root, oracle_cli, pcbnew_data_dir};

/// Boards at least this large are skipped (the oracle needs minutes and the cache hundreds of
/// MB for them): `vme-wren` (two copies), the Allegro import `VCU118`, `jetson-agx-thor`.
pub const IO_CORPUS_MAX_BYTES: u64 = 10_000_000;

/// Files that are not valid boards (`pcb-io.md` 9.2); the oracle rejects them. The parser
/// tests check that the port rejects them too.
pub const IO_CORPUS_KNOWN_BAD: &[(&str, &str)] = &[(
    "qa/data/pcbnew/plugins/fakeboard.kicad_pcb",
    "known bad: \"A fake Board file for testing\", rejected by the oracle",
)];

/// Version of the cache layout; part of the key.
const CACHE_VERSION: u32 = 1;

/// One board of the corpus.
#[derive(Clone, Debug)]
pub struct CorpusBoard {
    /// Path relative to the KiCad source root, with `/` separators, e.g.
    /// `qa/data/pcbnew/issue18.kicad_pcb`.
    pub name: String,
    /// Absolute path.
    pub path: PathBuf,
    /// File size in bytes.
    pub size: u64,
    /// Why the board is skipped, if it is.
    pub skip: Option<String>,
}

impl CorpusBoard {
    /// The name with `/` replaced by `__` (the cache directory name).
    pub fn flat_name(&self) -> String {
        self.name.replace('/', "__")
    }

    /// The file name, e.g. `issue18.kicad_pcb`.
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The corpus: every `.kicad_pcb` below `qa/data/pcbnew` and `demos`, sorted by name.
pub fn io_corpus_boards() -> Vec<CorpusBoard> {
    let root = kicad_source_root();
    let mut files = files_with_extension(&pcbnew_data_dir(), "kicad_pcb");
    files.extend(files_with_extension(&demos_dir(), "kicad_pcb"));

    let mut boards: Vec<CorpusBoard> = files
        .into_iter()
        .map(|path| {
            let name = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let skip = if let Some((_, why)) = IO_CORPUS_KNOWN_BAD.iter().find(|(n, _)| *n == name)
            {
                Some((*why).to_string())
            } else if size >= IO_CORPUS_MAX_BYTES {
                Some(format!("very large ({} MB)", size / 1_000_000))
            } else {
                None
            };

            CorpusBoard {
                name,
                path,
                size,
                skip,
            }
        })
        .collect();

    boards.sort_by(|a, b| a.name.cmp(&b.name));
    boards
}

/// Find a board by its name, its file name or its file stem (`issue18`); the query must match
/// exactly one board.
///
/// # Errors
///
/// A message listing the candidates when the query matches none or several boards.
pub fn find_corpus_board<'b>(
    boards: &'b [CorpusBoard],
    query: &str,
) -> Result<&'b CorpusBoard, String> {
    let exact: Vec<&CorpusBoard> = boards.iter().filter(|b| b.name == query).collect();

    if exact.len() == 1 {
        return Ok(exact[0]);
    }

    let hits: Vec<&CorpusBoard> = boards
        .iter()
        .filter(|b| {
            let file = b.file_name();
            file == query
                || file.strip_suffix(".kicad_pcb") == Some(query)
                || b.name.ends_with(&format!("/{query}"))
        })
        .collect();

    match hits.len() {
        1 => Ok(hits[0]),
        0 => Err(format!("no corpus board matches '{query}'")),
        _ => Err(format!(
            "'{query}' matches several boards: {}",
            hits.iter()
                .map(|b| b.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// `<kicad>/rust/target/io-corpus`, or the directory named by `IO_CORPUS_DIR`.
pub fn io_corpus_dir() -> PathBuf {
    match std::env::var_os("IO_CORPUS_DIR") {
        Some(d) => PathBuf::from(d),
        None => kicad_source_root()
            .join("rust")
            .join("target")
            .join("io-corpus"),
    }
}

/// FNV-1a 64 of `bytes` (the cache key hash; the same function as `pcb-dump --hash`).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;

    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }

    h
}

/// The reference oracle and the identity of its build.
#[derive(Clone, Debug)]
pub struct OracleStamp {
    /// The `kicad-cli` wrapper to call ([`oracle_cli`]).
    pub cli: PathBuf,
    /// Modification times (ns since the epoch) of the wrapper, the real binary and the pcbnew
    /// kiface (`<build>/kicad/kicad-cli`, `<build>/pcbnew/_pcbnew.kiface`, build tree from
    /// `KICAD_ORACLE_BUILD` or `/home/user/kicad-oracle/build`), `-` for a missing file.
    pub stamp: String,
}

fn mtime_ns(path: &Path) -> String {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or_else(|| "-".to_string(), |d| d.as_nanos().to_string())
}

/// The oracle and its build stamp, or `None` when no oracle is available (callers skip
/// silently then).
pub fn oracle_stamp() -> Option<OracleStamp> {
    let cli = oracle_cli()?;
    let build = std::env::var_os("KICAD_ORACLE_BUILD").map_or_else(
        || PathBuf::from("/home/user/kicad-oracle/build"),
        PathBuf::from,
    );
    let stamp = format!(
        "{}:{}:{}",
        mtime_ns(&cli),
        mtime_ns(&build.join("kicad").join("kicad-cli")),
        mtime_ns(&build.join("pcbnew").join("_pcbnew.kiface"))
    );

    Some(OracleStamp { cli, stamp })
}

/// What the oracle made of a board.
#[derive(Clone, Debug)]
pub enum OracleResult {
    /// The oracle saved the board. `o2` is the same file as `o1` for a fixed point.
    Saved {
        o1: PathBuf,
        o2: PathBuf,
        fixed_point: bool,
    },
    /// The oracle did not save the board (load error, crash, time-out): its output.
    Failed { message: String },
}

/// A board of the oracle cache.
#[derive(Clone, Debug)]
pub struct CachedBoard {
    /// [`CorpusBoard::name`].
    pub name: String,
    pub result: OracleResult,
    /// False when the oracle was run for it in this call.
    pub from_cache: bool,
}

/// The key of a board: input hash and size, oracle stamp, cache layout version.
fn cache_key(input: &[u8], stamp: &OracleStamp) -> String {
    format!(
        "v{CACHE_VERSION} fnv={:016x} size={} oracle={}",
        fnv1a64(input),
        input.len(),
        stamp.stamp
    )
}

/// Read the cache entry in `dir` if its key is `key`.
fn read_entry(name: &str, dir: &Path, key: &str) -> Option<CachedBoard> {
    let meta = fs::read_to_string(dir.join("meta")).ok()?;
    let mut lines = meta.lines();

    if lines.next()? != key {
        return None;
    }

    let status = lines.next()?;
    let result = match status {
        "saved fixed" | "saved changed" => {
            let o1 = dir.join("o1.kicad_pcb");
            let fixed_point = status == "saved fixed";
            let o2 = if fixed_point {
                o1.clone()
            } else {
                dir.join("o2.kicad_pcb")
            };

            if !o1.is_file() || !o2.is_file() {
                return None;
            }

            OracleResult::Saved {
                o1,
                o2,
                fixed_point,
            }
        }
        "failed" => OracleResult::Failed {
            message: lines.collect::<Vec<_>>().join("\n"),
        },
        _ => return None,
    };

    Some(CachedBoard {
        name: name.to_string(),
        result,
        from_cache: true,
    })
}

/// Run `kicad-cli pcb upgrade --force` on a copy of `input` (named `file_name`) in the fresh
/// directory `work`. Returns the saved text or the oracle's output as the error.
fn oracle_resave(
    stamp: &OracleStamp,
    home: &Path,
    work: &Path,
    file_name: &str,
    input: &[u8],
) -> Result<Vec<u8>, String> {
    let _ = fs::remove_dir_all(work);
    fs::create_dir_all(work).map_err(|e| e.to_string())?;
    let board = work.join(file_name);
    fs::write(&board, input).map_err(|e| e.to_string())?;

    let run = |wrapped: bool| {
        let mut cmd = if wrapped {
            let mut c = Command::new("nice");
            c.args(["-n", "15", "timeout", "900"]).arg(&stamp.cli);
            c
        } else {
            Command::new(&stamp.cli)
        };

        cmd.args(["pcb", "upgrade", "--force"])
            .arg(&board)
            .current_dir(work)
            .env("KICAD_ORACLE_HOME", home)
            .output()
    };

    let output = match run(true) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => run(false),
        other => other,
    }
    .map_err(|e| format!("cannot run {}: {e}", stamp.cli.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if !output.status.success() || !stdout.contains("Successfully saved") {
        let mut message = format!("exit status {}", output.status);

        // The schema-file error is printed on every run.
        for line in stdout.lines().chain(stderr.lines()) {
            if !line.contains("api.v1.schema.json") && !line.trim().is_empty() {
                message.push('\n');
                message.push_str(line);
            }
        }

        let _ = fs::remove_dir_all(work);
        return Err(message);
    }

    let saved = fs::read(&board).map_err(|e| e.to_string());
    let _ = fs::remove_dir_all(work);
    saved
}

/// Produce (or reuse) the cache entry of one board.
fn build_entry(
    board: &CorpusBoard,
    stamp: &OracleStamp,
    root: &Path,
    worker: usize,
) -> io::Result<CachedBoard> {
    let input = fs::read(&board.path)?;
    let key = cache_key(&input, stamp);
    let dir = root.join("boards").join(board.flat_name());

    if let Some(entry) = read_entry(&board.name, &dir, &key) {
        return Ok(entry);
    }

    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir)?;
    let home = root.join(format!("oracle-home-{worker}"));
    let work = root.join(format!("work-{worker}"));
    let file_name = board.file_name();

    let (status, body) = match oracle_resave(stamp, &home, &work, &file_name, &input) {
        Err(message) => ("failed", message),
        Ok(o1) => {
            fs::write(dir.join("o1.kicad_pcb"), &o1)?;

            match oracle_resave(stamp, &home, &work, &file_name, &o1) {
                Err(message) => {
                    let _ = fs::remove_file(dir.join("o1.kicad_pcb"));
                    ("failed", format!("re-save of O1 failed: {message}"))
                }
                Ok(o2) if o2 == o1 => ("saved fixed", String::new()),
                Ok(o2) => {
                    fs::write(dir.join("o2.kicad_pcb"), &o2)?;
                    ("saved changed", String::new())
                }
            }
        }
    };

    // The meta file is written last: an interrupted run leaves no valid entry behind.
    fs::write(dir.join("meta"), format!("{key}\n{status}\n{body}"))?;

    let mut entry = read_entry(&board.name, &dir, &key)
        .ok_or_else(|| io::Error::other(format!("cache entry of {} unreadable", board.name)))?;
    entry.from_cache = false;
    Ok(entry)
}

/// Make sure the oracle cache holds `O1` / `O2` of every board in `boards` that is not
/// skipped, running the oracle (`jobs` at a time, at most 2) for missing or stale entries.
///
/// Returns `None` when no oracle is available; otherwise one entry per non-skipped board, in
/// the order of `boards`. `progress` is called after each board the oracle had to process
/// (done, total to process, board name).
///
/// # Errors
///
/// File-system errors of the cache directory.
pub fn ensure_oracle_cache(
    boards: &[&CorpusBoard],
    jobs: usize,
    progress: &(dyn Fn(usize, usize, &str) + Sync),
) -> Option<io::Result<Vec<CachedBoard>>> {
    let stamp = oracle_stamp()?;
    let root = io_corpus_dir();

    if let Err(e) = fs::create_dir_all(root.join("boards")) {
        return Some(Err(e));
    }

    let todo: Vec<&CorpusBoard> = boards
        .iter()
        .copied()
        .filter(|b| b.skip.is_none())
        .collect();
    let results: Mutex<Vec<Option<io::Result<CachedBoard>>>> =
        Mutex::new((0..todo.len()).map(|_| None).collect());
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let jobs = jobs.clamp(1, 2);

    std::thread::scope(|scope| {
        for worker in 0..jobs {
            let (todo, results, next, done, stamp, root) =
                (&todo, &results, &next, &done, &stamp, &root);

            scope.spawn(move || {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);

                    let Some(board) = todo.get(i) else {
                        break;
                    };

                    let entry = build_entry(board, stamp, root, worker);

                    if matches!(&entry, Ok(e) if !e.from_cache) {
                        let n = done.fetch_add(1, Ordering::SeqCst) + 1;
                        progress(n, todo.len(), &board.name);
                    }

                    results.lock().unwrap_or_else(|p| p.into_inner())[i] = Some(entry);
                }
            });
        }
    });

    let results = results.into_inner().unwrap_or_else(|p| p.into_inner());
    Some(
        results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(io::Error::other("worker did not finish"))))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_and_skip_list() {
        let boards = io_corpus_boards();
        assert!(boards.len() >= 370, "{} boards", boards.len());
        let bad = find_corpus_board(&boards, "fakeboard").unwrap();
        assert!(bad.skip.as_deref().unwrap().starts_with("known bad"));
        let wren = find_corpus_board(&boards, "demos/vme-wren/vme-wren.kicad_pcb").unwrap();
        assert!(wren.skip.is_some());
        let small = find_corpus_board(&boards, "issue18.kicad_pcb").unwrap();
        assert_eq!(small.name, "qa/data/pcbnew/issue18.kicad_pcb");
        assert!(small.skip.is_none());
        assert!(find_corpus_board(&boards, "vme-wren").is_err());
        assert_eq!(small.flat_name(), "qa__data__pcbnew__issue18.kicad_pcb");
    }

    #[test]
    fn fnv_matches_pcb_dump() {
        // FNV-1a 64 test vectors.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
