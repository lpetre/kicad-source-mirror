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

//! `kicad-rs pcb upgrade --force` (`Session::load_board`, the full `BOARD_LOADER::Load`
//! sequence, + `run_pcb_upgrade`) against the reference `kicad-cli pcb upgrade --force` over
//! the board corpus. Packages PCBIO-15 (new code, test support) and FIX-RESAVE-1.
//!
//! Two comparisons:
//!
//! * `upgrade_corpus_scoreboard` (ignored: it needs the oracle and the io-corpus cache,
//!   `rust/tools/io-corpus`, `kicad_qa::io_corpus`): the input is the cached `O1` (the oracle's
//!   re-save of the board alone, without project or rule file), the expectation `O2` (the
//!   oracle's re-save of `O1`), over the whole corpus. Writes
//!   `rust/target/io-corpus/session_scoreboard.md`.
//!   `cargo test -p kicad-session --test upgrade_corpus -- --ignored --nocapture`.
//!   With `UPGRADE_CORPUS_UPDATE_GOLDEN=1` it also rewrites `tests/golden/upgrade_corpus.txt`.
//! * `upgrade_corpus_small` (default run, no oracle needed): the input is the board file of the
//!   repository itself (every corpus board under 300 KB), the expectation `O1`, recorded in the
//!   compact golden `tests/golden/upgrade_corpus.txt` as hashes. Each board must reach at least
//!   the status recorded there.
//!
//! Comparison rules (the reference is not reproducible byte for byte in two places, both
//! explained in `PORT_STATUS.md`, "FIX-RESAVE-1"):
//!
//! * render caches: `parsePCB_TEXT` ends with `SetVisible( true )` and footprint text with
//!   `SetLibTextSize`, both clear the cache read from the file, so `formatRenderCache` always
//!   regenerates the glyphs with the outline font engine from the fonts fontconfig finds on the
//!   oracle machine. The port has no outline font engine and writes the cache header only. The
//!   polygons of every `(render_cache ..)` list are masked.
//! * zone order: teardrop zones are saved without a UUID (`format( ZONE )`), so each load gives
//!   them a fresh random `KIID`, and `BOARD_ITEM::ptr_cmp` (the writer's zone set) orders zones
//!   of the same layer set by UUID: the place of a teardrop zone among the zones is random in
//!   the reference (two different outputs measured on 12 runs of the same input). Zones whose
//!   UUID is not deterministic are compared as a set; zones with a deterministic UUID keep their
//!   order.
//! * random UUIDs of items without one in a legacy input are masked
//!   (`kicad_qa::uuid_canon::canonicalise_uuids_sorted`).
//!
//! The parser runs strictly (no staging): a board with a construct whose parser package has
//! not landed is `not-loadable`. Steps reaching a known `port_pending!` stub are skipped
//! (`common::KNOWN_PENDING_STEPS`).

mod common;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;

use kicad_pcb::project::LoadPath;
use kicad_qa::io_corpus::{
    OracleResult, ensure_oracle_cache, fnv1a64, io_corpus_boards, io_corpus_dir,
};
use kicad_qa::sexpr_text::{SexprNode, first_difference, parse_lists};
use kicad_qa::uuid_canon::{UUID_PLACEHOLDER, canonicalise_uuids_sorted};
use kicad_session::jobs::JobPcbUpgrade;
use kicad_session::{LoadInputs, Reporter, SessionError, run_pcb_upgrade};

/// Boards of the default-run comparison: the repository file is smaller than this.
const SMALL_BOARD_BYTES: u64 = 300_000;

/// The committed golden of `upgrade_corpus_small`.
fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/golden/upgrade_corpus.txt")
}

/// `kicad-rs pcb upgrade --force` of `text` (a board file named `name`, no project).
fn upgrade(text: &str, name: &str) -> Result<String, String> {
    let inputs = LoadInputs {
        text,
        source_name: name,
        ..LoadInputs::default()
    };

    let (mut session, _) = common::load_staged(LoadPath::Cli, &inputs).map_err(|e| match e {
        SessionError::NotYetPorted { subsystem } => format!("not-loadable: {subsystem}"),
        other => other.to_string(),
    })?;
    let mut job = JobPcbUpgrade::new();
    job.filename = name.to_string();
    job.force = true;
    let mut reporter = Reporter::new();
    let outcome = run_pcb_upgrade(&mut session, &mut job, &mut reporter)
        .map_err(|e| format!("not-loadable: {e}"))?;

    match outcome.files.as_slice() {
        [f] => Ok(String::from_utf8_lossy(&f.contents).into_owned()),
        _ => Err(format!("no output: {}", reporter.text())),
    }
}

/// [`upgrade`] with `port_pending!` panics caught: `Err( (is_not_loadable, message) )`.
fn upgrade_caught(text: &str, name: &str) -> Result<String, (bool, String)> {
    match catch_unwind(AssertUnwindSafe(|| upgrade(text, name))) {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) if e.starts_with("not-loadable: ") => {
            Err((true, e["not-loadable: ".len()..].to_string()))
        }
        Ok(Err(e)) => Err((false, e)),
        Err(payload) => {
            let text = common::panic_text(payload.as_ref());

            if text.contains("port pending") {
                Err((true, text))
            } else {
                Err((false, format!("panic: {text}")))
            }
        }
    }
}

// ---- normalisation -----------------------------------------------------------------------

/// Replace every `(render_cache "text" angle (polygon ..) ..)` list by `(render_cache "text"
/// angle)` on one line (the header's blanks collapsed): the glyphs are regenerated by the
/// reference's outline font engine at save.
fn mask_render_caches(text: &str) -> String {
    fn collect(node: &SexprNode, out: &mut Vec<(usize, usize, usize)>) {
        if node.keyword == "render_cache" {
            let header_end = node.children.first().map_or(node.end, |c| c.start);
            out.push((node.start, header_end, node.end));
            return;
        }

        for c in &node.children {
            collect(c, out);
        }
    }

    let mut spans = Vec::new();

    for root in parse_lists(text) {
        collect(&root, &mut spans);
    }

    let mut out = String::with_capacity(text.len());
    let mut pos = 0;

    for (start, header_end, end) in spans {
        out.push_str(&text[pos..start]);
        let header = &text[start + 1..header_end];
        out.push('(');
        out.push_str(&header.split_whitespace().collect::<Vec<_>>().join(" "));
        out.push(')');
        pos = end + 1;
    }

    out.push_str(&text[pos..]);
    out
}

/// True when the zone block has a deterministic UUID: a `(uuid ..)` child that is not the
/// [`UUID_PLACEHOLDER`] of a masked random one.
fn zone_has_fixed_uuid(text: &str, zone: &SexprNode) -> bool {
    zone.children
        .iter()
        .any(|c| c.keyword == "uuid" && !c.text(text).contains(UUID_PLACEHOLDER))
}

/// Within the run of top-level `(zone ..)` blocks: keep the zones with a deterministic UUID in
/// their order and move the others (teardrop zones, zones with a masked random UUID) behind
/// them, sorted by their text. `BOARD_ITEM::ptr_cmp` orders zones of one layer set by UUID,
/// random for those.
fn canonicalise_zone_order(text: &str) -> String {
    let roots = parse_lists(text);
    let Some(root) = roots.first() else {
        return text.to_string();
    };
    let zones: Vec<&SexprNode> = root
        .children
        .iter()
        .filter(|c| c.keyword == "zone")
        .collect();

    if zones.len() < 2 {
        return text.to_string();
    }

    // The writer saves all zones together; only a contiguous run (blanks between) is handled.
    let contiguous = zones.windows(2).all(|w| {
        text[w[0].end + 1..w[1].start]
            .bytes()
            .all(|b| b.is_ascii_whitespace())
    });

    if !contiguous {
        return text.to_string();
    }

    let mut fixed: Vec<&str> = Vec::new();
    let mut random: Vec<&str> = Vec::new();

    for z in &zones {
        if zone_has_fixed_uuid(text, z) {
            fixed.push(z.text(text));
        } else {
            random.push(z.text(text));
        }
    }

    random.sort_unstable();
    let first = zones[0].start;
    let last = zones[zones.len() - 1].end;
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..first]);
    out.push_str(
        &fixed
            .into_iter()
            .chain(random)
            .collect::<Vec<_>>()
            .join("\n"),
    );
    out.push_str(&text[last + 1..]);
    out
}

/// The comparison form of a re-save of `original`: random UUIDs masked, render-cache glyphs
/// masked, zones of random identity in canonical order.
fn normalise(text: &str, original: &str) -> String {
    let canon = canonicalise_uuids_sorted(text, original);
    canonicalise_zone_order(&mask_render_caches(&canon))
}

/// Status of one board, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Status {
    /// Byte for byte equal to the reference.
    Identical,
    /// Equal after [`normalise`].
    Normalised,
    /// Different even then.
    Differs,
    /// A `port_pending!` stub or `NotYetPorted` stopped the load or save.
    NotLoadable,
    /// An error or a panic that is not a pending stub.
    Failed,
}

impl Status {
    fn name(self) -> &'static str {
        match self {
            Status::Identical => "identical",
            Status::Normalised => "normalised",
            Status::Differs => "differs",
            Status::NotLoadable => "not-loadable",
            Status::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Option<Status> {
        [
            Status::Identical,
            Status::Normalised,
            Status::Differs,
            Status::NotLoadable,
            Status::Failed,
        ]
        .into_iter()
        .find(|st| st.name() == s)
    }
}

/// Which of the normalisation rules a `Normalised` board needed (for the report).
fn masks_needed(expected: &str, actual: &str, original: &str) -> String {
    let ce = canonicalise_uuids_sorted(expected, original);
    let ca = canonicalise_uuids_sorted(actual, original);
    let mut needed = Vec::new();

    if ce != expected || ca != actual {
        needed.push("random-uuid");
    }

    if ce != ca {
        if canonicalise_zone_order(&ce) != canonicalise_zone_order(&ca) {
            needed.push("render-cache");
        }

        if mask_render_caches(&ce) != mask_render_caches(&ca) {
            needed.push("zone-order");
        }
    }

    needed.join("+")
}

/// One compared board.
struct Compared {
    status: Status,
    /// First difference (for `Differs`), the error (for `NotLoadable` / `Failed`), the masks
    /// needed (for `Normalised`).
    detail: String,
}

/// Compare the port's re-save of `input` with the reference's `expected`; `original` is the
/// file whose UUIDs are deterministic.
fn compare(input: &str, expected: &str, original: &str, name: &str) -> Compared {
    match upgrade_caught(input, name) {
        Ok(text) => {
            if text == expected {
                Compared {
                    status: Status::Identical,
                    detail: String::new(),
                }
            } else {
                let ne = normalise(expected, original);
                let na = normalise(&text, original);

                if ne == na {
                    Compared {
                        status: Status::Normalised,
                        detail: masks_needed(expected, &text, original),
                    }
                } else {
                    let detail = match first_difference(&ne, &na, 1) {
                        Some(d) => format!(
                            "line {} `{}`: expected `{}`, actual `{}`",
                            d.line,
                            d.path.join(" > "),
                            d.expected.unwrap_or_default().trim(),
                            d.actual.unwrap_or_default().trim()
                        ),
                        None => "differs".to_string(),
                    };
                    Compared {
                        status: Status::Differs,
                        detail,
                    }
                }
            }
        }
        Err((true, why)) => Compared {
            status: Status::NotLoadable,
            detail: why.split(" at line").next().unwrap_or(&why).to_string(),
        },
        Err((false, why)) => Compared {
            status: Status::Failed,
            detail: why,
        },
    }
}

fn hash_hex(text: &str) -> String {
    format!("{:016x}", fnv1a64(text.as_bytes()))
}

/// The scoreboard of `kicad-rs pcb upgrade --force` vs the reference over the corpus (input
/// `O1`, expected `O2`); with `UPGRADE_CORPUS_UPDATE_GOLDEN=1` also the golden of
/// [`upgrade_corpus_small`] (input the repository file, expected `O1`).
#[test]
#[ignore = "needs the reference kicad-cli and the io-corpus oracle cache"]
fn upgrade_corpus_scoreboard() {
    common::quiet_pending_panics();
    let boards = io_corpus_boards();
    let refs: Vec<_> = boards.iter().collect();
    let Some(cache) = ensure_oracle_cache(&refs, 1, &|_, _, _| {}) else {
        eprintln!("no oracle: skipped");
        return;
    };
    let cache = cache.expect("oracle cache");
    let update_golden = std::env::var_os("UPGRADE_CORPUS_UPDATE_GOLDEN").is_some();

    let mut by_status: BTreeMap<Status, Vec<(String, String)>> = BTreeMap::new();
    let mut golden = String::new();
    let _ = writeln!(
        golden,
        "# upgrade_corpus_small: kicad-rs pcb upgrade --force of the repository board vs O1 \
         (reference kicad-cli pcb upgrade --force).\n# Generated by `UPGRADE_CORPUS_UPDATE_GOLDEN=1 \
         cargo test -p kicad-session --test upgrade_corpus -- --ignored`.\n# board\tstatus\t\
         fnv1a64(O1)\tfnv1a64(normalised O1)"
    );

    for entry in &cache {
        let board = boards.iter().find(|b| b.name == entry.name).unwrap();
        let file_name = board.file_name();

        let OracleResult::Saved { o1, o2, .. } = &entry.result else {
            by_status
                .entry(Status::Failed)
                .or_default()
                .push((entry.name.clone(), "reference did not save".to_string()));
            continue;
        };
        let o1 = std::fs::read_to_string(o1).unwrap();
        let o2 = std::fs::read_to_string(o2).unwrap();

        let c = compare(&o1, &o2, &o1, &file_name);
        by_status
            .entry(c.status)
            .or_default()
            .push((entry.name.clone(), c.detail));

        if update_golden && board.size < SMALL_BOARD_BYTES {
            let original = std::fs::read_to_string(&board.path).unwrap();
            let small = compare(&original, &o1, &original, &file_name);
            let _ = writeln!(
                golden,
                "{}\t{}\t{}\t{}",
                entry.name,
                small.status.name(),
                hash_hex(&o1),
                hash_hex(&normalise(&o1, &original))
            );
        }
    }

    let mut md = String::new();
    let _ = writeln!(
        md,
        "# kicad-rs pcb upgrade --force vs kicad-cli (PCBIO-15, FIX-RESAVE-1)\n\nInput O1, \
         expected O2 (io-corpus oracle cache); strict parser. `normalised`: equal after masking \
         render-cache glyphs and canonicalising the order of zones without a deterministic \
         UUID (see the module comment of `tests/upgrade_corpus.rs`).\n"
    );
    let _ = writeln!(md, "| status | boards |\n|---|---:|");

    for (status, list) in &by_status {
        let _ = writeln!(md, "| {} | {} |", status.name(), list.len());
    }

    for (status, list) in &by_status {
        let _ = writeln!(md, "\n## {}\n", status.name());

        for (name, detail) in list {
            if detail.is_empty() {
                let _ = writeln!(md, "- `{name}`");
            } else {
                let _ = writeln!(md, "- `{name}`: {detail}");
            }
        }
    }

    let path = io_corpus_dir().join("session_scoreboard.md");
    let _ = std::fs::write(&path, &md);
    println!("{md}");
    println!("written to {}", path.display());

    if update_golden {
        std::fs::write(golden_path(), golden).unwrap();
        println!("golden written to {}", golden_path().display());
    }
}

/// `kicad-rs pcb upgrade --force` of every corpus board under 300 KB (the repository file,
/// alone) against the reference's re-save `O1`, through the hashes of the committed golden:
/// each board must reach at least the status recorded there. Improvements are printed (rerun
/// the scoreboard with `UPGRADE_CORPUS_UPDATE_GOLDEN=1` to record them).
#[test]
fn upgrade_corpus_small() {
    common::quiet_pending_panics();
    let golden = std::fs::read_to_string(golden_path()).expect("golden upgrade_corpus.txt");
    let boards = io_corpus_boards();
    let mut regressions = Vec::new();
    let mut improvements = Vec::new();
    let mut checked = 0;

    for line in golden
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        assert_eq!(f.len(), 4, "golden line `{line}`");
        let (name, want, o1_hash, o1_norm_hash) = (f[0], Status::parse(f[1]).unwrap(), f[2], f[3]);
        let Some(board) = boards.iter().find(|b| b.name == name) else {
            panic!("golden board `{name}` not in the corpus");
        };
        let original = std::fs::read_to_string(&board.path).unwrap();
        let got = match upgrade_caught(&original, &board.file_name()) {
            Ok(text) => {
                if hash_hex(&text) == o1_hash {
                    Status::Identical
                } else if hash_hex(&normalise(&text, &original)) == o1_norm_hash {
                    Status::Normalised
                } else {
                    Status::Differs
                }
            }
            Err((true, _)) => Status::NotLoadable,
            Err((false, _)) => Status::Failed,
        };
        checked += 1;

        if got > want {
            regressions.push(format!("{name}: {} (golden: {})", got.name(), want.name()));
        } else if got < want {
            improvements.push(format!("{name}: {} (golden: {})", got.name(), want.name()));
        }
    }

    for i in &improvements {
        println!("improved: {i}");
    }

    assert!(checked > 0, "empty golden");
    assert!(
        regressions.is_empty(),
        "{} of {checked} boards regressed:\n{}",
        regressions.len(),
        regressions.join("\n")
    );
}

/// The normalisation rules on hand-made texts.
#[test]
fn normalisation_rules() {
    let original = "(kicad_pcb\n)";
    // Render caches: glyphs masked, header blanks collapsed.
    let a = "(kicad_pcb\n\t(gr_text \"x\"\n\t\t(render_cache \"x\"\n\t\t\t0\n\t\t\t(polygon\n\t\t\t\t(pts \
             (xy 1 1))\n\t\t\t)\n\t\t)\n\t)\n)";
    let b = "(kicad_pcb\n\t(gr_text \"x\"\n\t\t(render_cache \"x\"\n\t\t\t0\n\t\t)\n\t)\n)";
    assert_eq!(normalise(a, original), normalise(b, original));
    assert_ne!(
        normalise(a, original),
        normalise(&b.replace("\"x\"\n\t\t\t0", "\"x\" 90"), original)
    );

    // Zones without a UUID (teardrops) form a set; zones with a UUID keep their order.
    let z1 = "\t(zone\n\t\t(priority 1)\n\t)";
    let z2 = "\t(zone\n\t\t(priority 2)\n\t)";
    let u1 = "\t(zone\n\t\t(uuid \"11111111-1111-1111-1111-111111111111\")\n\t)";
    let u2 = "\t(zone\n\t\t(uuid \"22222222-2222-2222-2222-222222222222\")\n\t)";
    let original = format!("(kicad_pcb\n{u1}\n{u2}\n)");
    let board = |zones: &[&str]| format!("(kicad_pcb\n{}\n\t(group)\n)", zones.join("\n"));
    assert_eq!(
        normalise(&board(&[z1, u1, z2, u2]), &original),
        normalise(&board(&[u1, z2, u2, z1]), &original)
    );
    assert_ne!(
        normalise(&board(&[z1, u1, z2, u2]), &original),
        normalise(&board(&[z1, u2, z2, u1]), &original)
    );
    assert_ne!(
        normalise(&board(&[z1, u1, z1, u2]), &original),
        normalise(&board(&[z1, u1, z2, u2]), &original)
    );
}
