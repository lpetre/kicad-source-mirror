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

//! `kicad-bench`: the measuring instrument of the performance phase (package PERF-1,
//! `docs/PERF.md`). Not a port of a C++ file: it drives the port through the same
//! `kicad-session` calls `kicad-rs` makes (`host::get_board`, `run_drc`,
//! `run_export_gerbers`, `run_export_drill`, `run_pcb_upgrade`) and times every stage.
//!
//! ```text
//! kicad-bench run --board FILE [--reps N] [--warmup W] [--pipelines LIST] [--commit HASH]
//!                 [--label NAME] [--no-mem]
//! kicad-bench mem --board FILE --pipeline NAME     (internal: one pipeline, peak RSS per stage)
//! ```
//!
//! `run` prints one TSV line per stage (see [`TSV_HEADER`]). Pipelines (each starts from a
//! fresh load, as each `kicad-cli` invocation does):
//!
//! * `drc`: load, `pcb drc` (`run_drc`), then a second, instrumented `RunTests` on the same
//!   session whose progress / log messages split it into the cache generator phases and the
//!   providers (`drc2.*`);
//! * `fab`: load, `pcb export gerbers` (all enabled layers, CLI defaults), `pcb export
//!   drill` (CLI defaults), `pcb upgrade --force` (the board writer);
//! * `refill`: load, `FillAllZones` (all zones), then `pcb drc` on the refilled board (the
//!   two halves of `pcb drc --refill-zones`);
//! * `fill` (not in the default list): load and `FillAllZones` only;
//! * `edit` (not in the default list): the router client loop of `board_api` (see
//!   `edit_loop`);
//! * `live` (not in the default list, package LIVE-1): all routing removed and re-added in
//!   ten transactions each, incremental (`live.commit`) and with a full connectivity rebuild
//!   per commit (`live.commit_full`, the pre-LIVE-1 cost), `live.checkpoint`, `live.rollback`
//!   (see `live_loop`).
//!
//! The load is `getBoard` split into its steps: `parse` (project file read + board parser, up
//! to the first post-parse step), `load.<LoadStep>` (each step of `initializeLoadedBoard`,
//! `load.BuildConnectivity` is the connectivity build), `load.session_init` (session, host
//! inputs, drawing sheet) and `load_total`.
//!
//! Peak RSS: before timing, `run` starts itself once per pipeline (`mem`), so every pipeline
//! runs in a fresh process; there the peak (`VmHWM`) is reset before each stage
//! (`/proc/self/clear_refs` = 5) and read after it. With the `count-alloc` feature a counting
//! global allocator reports allocation calls and bytes per stage.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::ExitCode;
use std::time::Instant;

use kicad_core::Severity;
use kicad_session::jobs::{JobExportPcbDrill, JobExportPcbGerbers, JobPcbDrc, JobPcbUpgrade};
use kicad_session::{HostClock, LoadInputs, Reporter, Session, SessionError};

#[cfg(feature = "count-alloc")]
#[global_allocator]
static GLOBAL: &stats_alloc::StatsAlloc<std::alloc::System> = &stats_alloc::INSTRUMENTED_SYSTEM;

/// The columns of the `run` output.
const TSV_HEADER: &str =
    "commit\tboard\tstage\tn\tmin_ms\tmedian_ms\tmax_ms\tpeak_rss_mb\tallocs\talloc_mb";

/// The fixed host inputs (date of reports and Gerber headers, UUID seed), so that every run
/// produces the same bytes.
const FIXED_DATE: &str = "2026-01-01T00:00:00";
const UUID_SEED: u32 = 1;

/// All pipelines, in run order.
const PIPELINES: &[&str] = &["drc", "fab", "refill"];

/// Allocation counters at one instant (zero without the `count-alloc` feature).
#[derive(Clone, Copy, Default)]
struct AllocSnap {
    calls: u64,
    bytes: u64,
}

#[cfg(feature = "count-alloc")]
fn alloc_snap() -> AllocSnap {
    let s = GLOBAL.stats();
    AllocSnap {
        calls: (s.allocations + s.reallocations) as u64,
        bytes: (s.bytes_allocated + s.bytes_reallocated.max(0) as usize) as u64,
    }
}

#[cfg(not(feature = "count-alloc"))]
fn alloc_snap() -> AllocSnap {
    AllocSnap::default()
}

/// `VmHWM` / `VmRSS` of this process in kB (0 when `/proc` is not readable).
fn proc_status_kb(key: &str) -> u64 {
    let Ok(text) = std::fs::read_to_string("/proc/self/status") else {
        return 0;
    };

    text.lines()
        .find_map(|l| l.strip_prefix(key))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .unwrap_or(0)
}

/// Resets the peak RSS (`VmHWM`) to the current RSS.
fn reset_hwm() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// One measured stage occurrence.
#[derive(Clone, Default)]
struct Sample {
    ms: f64,
    peak_kb: u64,
    allocs: u64,
    alloc_bytes: u64,
}

/// The samples of every stage, in order of first appearance.
#[derive(Default)]
struct Recorder {
    /// Reset and read the peak RSS around every stage (`mem` mode).
    mem: bool,
    /// Whether samples are kept (false during warm-up).
    keep: bool,
    /// Appended to every stage name (` [cold]` for the first repetition of the process).
    suffix: &'static str,
    stages: Vec<(String, Vec<Sample>)>,
}

/// The start of a stage.
struct Mark {
    t: Instant,
    a: AllocSnap,
}

impl Recorder {
    fn start(&self) -> Mark {
        if self.mem {
            reset_hwm();
        }

        Mark {
            t: Instant::now(),
            a: alloc_snap(),
        }
    }

    fn end(&mut self, name: &str, m: &Mark) {
        let ms = m.t.elapsed().as_secs_f64() * 1e3;
        let a = alloc_snap();
        let peak_kb = if self.mem {
            proc_status_kb("VmHWM:")
        } else {
            0
        };
        let s = Sample {
            ms,
            peak_kb,
            allocs: a.calls - m.a.calls,
            alloc_bytes: a.bytes - m.a.bytes,
        };
        self.push(name, s);
    }

    fn push(&mut self, name: &str, s: Sample) {
        if !self.keep {
            return;
        }

        let name = format!("{name}{}", self.suffix);

        match self.stages.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => v.push(s),
            None => self.stages.push((name, vec![s])),
        }
    }

    fn stage<T>(&mut self, name: &str, f: impl FnOnce() -> T) -> T {
        let m = self.start();
        let r = f();
        self.end(name, &m);
        r
    }
}

/// What the host reads for one board (`host::get_board` of kicad-rs).
struct BoardFiles {
    path: String,
    text: String,
    project_path: String,
    project: Option<String>,
    local_settings: Option<String>,
    rules: Option<String>,
}

fn with_extension(path: &str, ext: &str) -> String {
    std::path::Path::new(path)
        .with_extension(ext)
        .to_string_lossy()
        .into_owned()
}

impl BoardFiles {
    fn read(path: &str) -> Result<BoardFiles, String> {
        let abs = std::fs::canonicalize(path)
            .map_err(|e| format!("{path}: {e}"))?
            .to_string_lossy()
            .into_owned();
        let bytes = std::fs::read(&abs).map_err(|e| format!("{abs}: {e}"))?;
        let project_path = with_extension(&abs, "kicad_pro");

        Ok(BoardFiles {
            text: String::from_utf8_lossy(&bytes).into_owned(),
            project: std::fs::read_to_string(&project_path).ok(),
            local_settings: std::fs::read_to_string(with_extension(&project_path, "kicad_prl"))
                .ok(),
            rules: std::fs::read_to_string(with_extension(&abs, "kicad_dru")).ok(),
            project_path,
            path: abs,
        })
    }
}

fn clock() -> HostClock {
    HostClock::parse_iso(FIXED_DATE).expect("fixed date")
}

/// `host::get_board` of kicad-rs (`Session::load_board_with` + host inputs + drawing sheet),
/// with `BOARD_LOADER::Load` run step by step (`board_loader::load_guarded`) so that each
/// step is timed.
fn load(rec: &mut Recorder, files: &BoardFiles) -> Result<Session, SessionError> {
    use kicad_pcb::project::LoadPath;

    let inputs = LoadInputs {
        text: &files.text,
        source_name: &files.path,
        project_path: Some(&files.project_path),
        project_json: files.project.as_deref(),
        local_settings_json: files.local_settings.as_deref(),
        rules_text: files.rules.as_deref(),
        ..LoadInputs::default()
    };

    let total = rec.start();
    let parse = rec.start();
    let mut parse_done = false;
    let rec_cell = RefCell::new(&mut *rec);

    let loaded = {
        let mut guard = |step: kicad_pcb::project::LoadStep, run: &mut dyn FnMut()| {
            let mut r = rec_cell.borrow_mut();

            if !parse_done {
                r.end("parse", &parse);
                parse_done = true;
            }

            let m = r.start();
            run();
            r.end(&format!("load.{step:?}"), &m);
        };
        kicad_session::board_loader::load_guarded(LoadPath::Cli, &inputs, Some(&mut guard))
    };

    let rec = rec_cell.into_inner();
    let loaded = loaded?;

    let mut session = rec.stage("load.session_init", || {
        let mut session = Session::from_loaded(loaded, &inputs);
        session.apply_host_inputs(&clock(), UUID_SEED);
        session.host_read_file = Some(|path| std::fs::read(path).ok());
        let _ = session.load_project_drawing_sheet();
        session
    });

    // kicad-rs prints these on stderr; computing them is part of getBoard there
    let _ = rec.stage("load.font_warnings", || session.outline_font_warnings());
    rec.end("load_total", &total);
    session.log_warnings.clear();
    Ok(session)
}

/// FNV-1a 64 of the produced files, for the cross-check against kicad-rs.
fn fnv1a64(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= u64::from(b);
        *h = h.wrapping_mul(0x100_0000_01b3);
    }
}

/// Prints a one-line summary of a job outcome on stderr (first repetition only).
fn summarize(verbose: bool, what: &str, r: &Result<kicad_session::JobOutcome, SessionError>) {
    if !verbose {
        return;
    }

    match r {
        Ok(o) => {
            let mut h = 0xcbf2_9ce4_8422_2325_u64;
            let mut bytes = 0;

            // KICAD_BENCH_DUMP=<dir>: write the files of the first repetition there (by
            // base name) for the byte comparison with kicad-rs
            let dump = std::env::var("KICAD_BENCH_DUMP").ok();

            for f in &o.files {
                if let Some(dir) = &dump {
                    let base = std::path::Path::new(&f.name)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let _ = std::fs::create_dir_all(format!("{dir}/{what}"));
                    let _ = std::fs::write(format!("{dir}/{what}/{base}"), &f.contents);
                }

                fnv1a64(&mut h, f.name.as_bytes());
                fnv1a64(&mut h, &f.contents);
                bytes += f.contents.len();
            }

            eprintln!(
                "  {what}: exit {} files {} bytes {bytes} fnv {h:016x}",
                o.exit_code,
                o.files.len()
            );
        }
        Err(e) => eprintln!("  {what}: ERROR {e}"),
    }
}

/// `pcb drc` with the CLI defaults (report format, no parity, fixed date and seed).
fn drc_job(files: &BoardFiles) -> JobPcbDrc {
    let mut job = JobPcbDrc::new();
    job.rc.filename = files.path.clone();
    job.parity = false;
    job.rc.exit_code_violations = false;
    job.report_date = clock().iso8601();
    job.marker_uuid_seed = UUID_SEED;
    job
}

/// A DRC progress / log event: the time and the text.
type Events = RefCell<Vec<(Instant, u64, AllocSnap, String)>>;

struct PhaseLog<'a> {
    events: &'a Events,
    mem: bool,
}

impl PhaseLog<'_> {
    fn push(&self, text: String) {
        // Progress phases inside a provider are not interval boundaries (see drc_phases):
        // they must not read and reset the peak of their provider's interval.
        let boundary = text.starts_with("provider:")
            || !self
                .events
                .borrow()
                .iter()
                .any(|e| e.3.starts_with("provider:"));

        if !boundary {
            return;
        }

        let peak = if self.mem {
            let p = proc_status_kb("VmHWM:");
            reset_hwm();
            p
        } else {
            0
        };
        self.events
            .borrow_mut()
            .push((Instant::now(), peak, alloc_snap(), text));
    }
}

impl kicad_drc::engine::ProgressReporter for PhaseLog<'_> {
    fn advance_phase(&mut self, message: &str) {
        self.push(format!("phase:{message}"));
    }
}

impl kicad_drc::Reporter for PhaseLog<'_> {
    fn report(&mut self, text: &str, _severity: Severity) {
        if let Some(name) = text
            .strip_prefix("Run DRC provider: '")
            .and_then(|t| t.strip_suffix('\''))
        {
            self.push(format!("provider:{name}"));
        }
    }

    fn has_message(&self) -> bool {
        false
    }

    fn has_message_of_severity(&self, _severity_mask: i32) -> bool {
        false
    }
}

/// A second `RunTests` on the session (after `record_drc_exclusions` and `DeleteMARKERs`, as
/// the job does), split at the progress phases of the cache generator and at each provider.
fn drc_phases(rec: &mut Recorder, session: &mut Session, verbose: bool) {
    use kicad_drc::run_flow::{RunTestsOptions, run_tests};

    kicad_drc::exclusion::record_drc_exclusions(&mut session.board);
    session.board.delete_markers(true, true);

    let tuning_profiles = kicad_session::board_loader::tuning_profiles(&session.board);
    let options = RunTestsOptions {
        units: kicad_core::eda_units::EdaUnits::Mm,
        report_all_track_errors: false,
        test_footprints: false,
        tuning_profiles: &tuning_profiles,
        // no footprint library tables (the hermetic kicad-cli home has none either)
        footprint_libraries: None,
    };

    let events: Events = RefCell::new(Vec::new());
    let mut violations = 0usize;
    let start = rec.start();
    let start_peak_reset = Instant::now();

    {
        let mut progress = PhaseLog {
            events: &events,
            mem: rec.mem,
        };
        let mut log = PhaseLog {
            events: &events,
            mem: rec.mem,
        };
        let mut run = kicad_drc::engine::DrcRunState::new();
        run.set_progress_reporter(Some(&mut progress));
        run.set_log_reporter(Some(&mut log));
        run.set_violation_handler(Box::new(
            |_item: &std::sync::Arc<kicad_pcb::pcb_marker::RcItem>,
             _pos: kicad_geom::math::vector2::Vector2I,
             _layer: i32,
             _path: Option<&dyn Fn(&mut kicad_pcb::pcb_marker::PcbMarker)>| {
                violations += 1;
            },
        ));

        let engine = session.drc_engine.as_ref().expect("DRC engine");
        run_tests(
            engine,
            &mut run,
            &mut session.board,
            &mut session.connectivity,
            &options,
        );
        run.clear_violation_handler();
    }

    let end_peak = if rec.mem { proc_status_kb("VmHWM:") } else { 0 };
    let end_alloc = alloc_snap();
    let end = Instant::now();
    rec.end("drc2.run_tests", &start);

    // Intervals: [start, e0) is the engine's prepare_run, then each event up to the next one;
    // generator phases are named after their message, providers after their name.
    let events = events.into_inner();
    let mut prev_t = start_peak_reset;
    let mut prev_a = start.a;
    let mut prev_name = "drc2.prepare".to_string();
    let mut in_providers = false;

    for (t, peak, a, text) in
        events
            .iter()
            .cloned()
            .chain(std::iter::once((end, end_peak, end_alloc, String::new())))
    {
        if in_providers && text.starts_with("phase:") {
            // provider phases stay in their provider's interval (not recorded by PhaseLog)
            continue;
        }

        // The peak read at an event belongs to the interval it closes.
        rec.push(
            &prev_name,
            Sample {
                ms: (t - prev_t).as_secs_f64() * 1e3,
                peak_kb: peak,
                allocs: a.calls - prev_a.calls,
                alloc_bytes: a.bytes - prev_a.bytes,
            },
        );

        if let Some(name) = text.strip_prefix("provider:") {
            in_providers = true;
            prev_name = format!("drc2.provider.{name}");
        } else if let Some(msg) = text.strip_prefix("phase:") {
            prev_name = format!("drc2.gen.{}", msg.trim_end_matches('.'));
        }

        prev_t = t;
        prev_a = a;
    }

    if verbose {
        eprintln!("  drc2: {violations} violations");
    }
}

/// Tracks re-routed per edit batch of the `edit` pipeline, and batches per load.
const EDIT_TRACKS: usize = 200;
const EDIT_ROUNDS: usize = 3;

/// The inner loop of a router client (`board_api`, docs/PERF.md "Note for planners"): per
/// round, remove [`EDIT_TRACKS`] tracks and add them again with the same geometry, twice
/// from the same full-refill state: the LIVE-2 path (`commit`, `check_items` of the new
/// items, `refill_zones_affected` with the commit's damage) and the batch path
/// (`apply_edits`, `quick_drc` with the clearance and connectivity providers, full zone
/// refill), whose saved boards must be equal; then the teardrop update; `save` at the end.
fn edit_loop(rec: &mut Recorder, session: &mut Session, verbose: bool) -> Result<(), String> {
    use kicad_session::board_api::check::{CheckItemsOptions, QuickDrcOptions};
    use kicad_session::board_api::commit::Transaction;
    use kicad_session::board_api::edit::{EditBatch, NewItem, NewTrack};

    // the fills of the loaded file are not a full refill's: start from one
    let r = rec.stage("edit.initial_refill", || session.refill_zones());
    r.map_err(|e| format!("initial refill: {e}"))?;

    let tracks: Vec<_> = session
        .tracks()
        .into_iter()
        .filter(|t| t.kind == "track")
        .collect();

    for round in 0..EDIT_ROUNDS {
        // a different window of the (re-ordered) track list each round
        let chosen: Vec<_> = tracks
            .iter()
            .skip(round * EDIT_TRACKS)
            .take(EDIT_TRACKS)
            .collect();
        let current = session.tracks();
        let mut batch = EditBatch::default();

        for t in &chosen {
            // the handle of the same geometry in the current board (handles change on re-add)
            if let Some(c) = current.iter().find(|c| {
                c.kind == "track"
                    && c.start == t.start
                    && c.end == t.end
                    && c.layer == t.layer
                    && c.net == t.net
                    && !batch.remove.contains(&c.id)
            }) {
                batch.remove.push(c.id);
                batch.add.push(NewItem::Track(NewTrack {
                    start: t.start,
                    end: t.end,
                    width: Some(t.width),
                    layer: t.layer.clone(),
                    net: t.net,
                    ..NewTrack::default()
                }));
            }
        }

        // LIVE-2 path first, from the full-refill state of the round's start: commit, the
        // scoped check of the new items, the affected-only refill.
        let cp = session.checkpoint();
        let mut txn = Transaction::begin();
        txn.stage_batch(&batch);
        txn.append = true;
        let r = rec.stage("edit.commit", || session.commit(&txn));
        let report = r.map_err(|e| format!("commit: {e}"))?;

        let check = CheckItemsOptions::default();
        let r = rec.stage("edit.check_items", || {
            session.check_items(&report.edit.added, &check)
        });
        let scoped = r.map_err(|e| format!("check_items: {e}"))?;

        let mut damage = report.damage.clone();
        damage.merge(&scoped.damage);
        let r = rec.stage("edit.refill_affected", || {
            session.refill_zones_affected(&damage)
        });
        let refill = r.map_err(|e| format!("refill_affected: {e}"))?;
        let live_text = session.save().map_err(|e| format!("save: {e}"))?;

        // The batch path of WASM-API-1 from the same state.
        session.restore(cp);
        let r = rec.stage("edit.apply_edits", || session.apply_edits(&batch));
        r.map_err(|e| format!("apply_edits: {e}"))?;

        let options = QuickDrcOptions {
            providers: vec!["clearance".to_string(), "connectivity".to_string()],
            ..QuickDrcOptions::default()
        };
        let r = rec.stage("edit.quick_drc", || session.quick_drc(&options));
        let summary = r.map_err(|e| format!("quick_drc: {e}"))?;

        let r = rec.stage("edit.zone_refill", || session.refill_zones());
        r.map_err(|e| format!("edit zone_refill: {e}"))?;

        // the affected refill gives the full refill's board (package LIVE-2, verification c)
        if session.save().map_err(|e| format!("save: {e}"))? != live_text {
            return Err(format!(
                "round {round}: affected refill differs from the full refill"
            ));
        }

        // updateTeardrops leaves new, unfilled teardrops: time it, then go back to the
        // full-refill state for the next round.
        let cp = session.checkpoint();
        let r = rec.stage("edit.update_teardrops", || session.update_teardrops());
        r.map_err(|e| format!("update_teardrops: {e}"))?;
        session.restore(cp);

        if verbose && round == 0 {
            eprintln!(
                "  edit: {} tracks re-added, quick_drc: {} markers, {} unconnected; \
                 check_items: {} violations; affected refill: {} of {} zones refilled {:?}",
                batch.add.len(),
                summary.markers,
                summary.unconnected,
                scoped.violations.len(),
                refill.refilled.len(),
                session.board.zones().count(),
                refill.reasons
            );
        }
    }

    // Local edits (one net re-routed, the router's typical candidate): the net's tracks
    // removed and re-added, then the affected refill against the full refill from the same
    // state.
    let mut by_net: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();

    for t in session
        .tracks()
        .iter()
        .filter(|t| t.kind == "track" && t.net > 0)
    {
        *by_net.entry(t.net).or_default() += 1;
    }

    let nets: Vec<i32> = by_net
        .iter()
        .filter(|(_, n)| **n >= 2 && **n <= 40)
        .map(|(net, _)| *net)
        .take(EDIT_ROUNDS)
        .collect();

    for net in nets {
        let mut batch = EditBatch::default();

        for t in session
            .tracks()
            .iter()
            .filter(|t| t.kind == "track" && t.net == net)
        {
            batch.remove.push(t.id);
            batch.add.push(NewItem::Track(NewTrack {
                start: t.start,
                end: t.end,
                width: Some(t.width),
                layer: t.layer.clone(),
                net: t.net,
                ..NewTrack::default()
            }));
        }

        let cp = session.checkpoint();
        let mut txn = Transaction::begin();
        txn.stage_batch(&batch);
        txn.append = true;
        let r = rec.stage("edit.local_commit", || session.commit(&txn));
        let report = r.map_err(|e| format!("commit: {e}"))?;
        let r = rec.stage("edit.local_check_items", || {
            session.check_items(&report.edit.added, &CheckItemsOptions::default())
        });
        let scoped = r.map_err(|e| format!("check_items: {e}"))?;
        let mut damage = report.damage.clone();
        damage.merge(&scoped.damage);
        let r = rec.stage("edit.local_refill_affected", || {
            session.refill_zones_affected(&damage)
        });
        let refill = r.map_err(|e| format!("refill_affected: {e}"))?;
        let live_text = session.save().map_err(|e| format!("save: {e}"))?;

        session.restore(cp);
        session.commit(&txn).map_err(|e| format!("commit: {e}"))?;
        let r = rec.stage("edit.local_zone_refill", || session.refill_zones());
        r.map_err(|e| format!("zone_refill: {e}"))?;

        if session.save().map_err(|e| format!("save: {e}"))? != live_text {
            return Err(format!(
                "net {net}: affected refill differs from the full refill"
            ));
        }

        if verbose {
            eprintln!(
                "  edit local: net {net}, {} tracks; affected refill: {} of {} zones {:?}",
                batch.add.len(),
                refill.refilled.len(),
                session.board.zones().count(),
                refill.reasons
            );
        }
    }

    let r = rec.stage("edit.save", || session.save());
    r.map_err(|e| format!("save: {e}"))?;
    Ok(())
}

/// Package LIVE-1: the transaction layer on the whole routing. Remove all routing in ten
/// commits and add it back from flat records in ten commits (incremental connectivity,
/// `Session::commit`, stage `live.commit`), with a checkpoint before and a rollback after;
/// then the same twenty batches with a full `BuildConnectivity` after each commit, the cost
/// of the pre-LIVE-1 `apply_edits` (stage `live.commit_full`), and a second rollback.
fn live_loop(rec: &mut Recorder, session: &mut Session, verbose: bool) -> Result<(), String> {
    use kicad_session::board_api::commit::Transaction;

    const BATCHES: usize = 10;
    let rec_all = session.route_records();
    let ids: Vec<u64> = rec_all
        .track_ids
        .iter()
        .chain(&rec_all.arc_ids)
        .chain(&rec_all.via_ids)
        .copied()
        .collect();

    let chunk = |len: usize, stride: usize, i: usize| {
        let n = len / stride;
        let per = n.div_ceil(BATCHES);
        (i * per).min(n) * stride..((i + 1) * per).min(n) * stride
    };
    let mut batches = Vec::new();

    for i in 0..BATCHES {
        let mut t = Transaction::begin();
        t.stage_removals(&ids[chunk(ids.len(), 1, i)]);
        batches.push(t);
    }

    for i in 0..BATCHES {
        let mut t = Transaction::begin();
        t.append = true;
        let tr = &rec_all.tracks[chunk(rec_all.tracks.len(), 7, i)];
        let ar = &rec_all.arcs[chunk(rec_all.arcs.len(), 9, i)];
        let vr = &rec_all.vias[chunk(rec_all.vias.len(), 8, i)];
        t.stage_track_records(tr).map_err(|e| e.to_string())?;
        t.stage_arc_records(ar).map_err(|e| e.to_string())?;
        t.stage_via_records(vr).map_err(|e| e.to_string())?;
        batches.push(t);
    }

    let cp = rec.stage("live.checkpoint", || session.checkpoint());
    let mut unconnected = Vec::new();

    for t in &batches {
        let r = rec.stage("live.commit", || session.commit(t));
        unconnected.push(r.map_err(|e| format!("commit: {e}"))?.unconnected);
    }

    rec.stage("live.rollback", || session.rollback(&cp));

    for (k, t) in batches.iter().enumerate() {
        let r = rec.stage("live.commit_full", || {
            let r = session.commit(t);
            session.rebuild_connectivity();
            r
        });
        r.map_err(|e| format!("commit: {e}"))?;

        // the incremental counts equal those of the full rebuild
        if session.connectivity.unconnected_count(false) != unconnected[k] {
            return Err(format!("batch {k}: incremental unconnected count differs"));
        }
    }

    rec.stage("live.rollback", || session.rollback(&cp));

    if verbose {
        eprintln!(
            "  live: {} items in {} batches, unconnected after removal {}",
            ids.len(),
            batches.len(),
            unconnected[BATCHES - 1]
        );
    }

    Ok(())
}

/// Runs one pipeline on a freshly loaded session.
fn run_pipeline(
    rec: &mut Recorder,
    files: &BoardFiles,
    pipeline: &str,
    verbose: bool,
) -> Result<(), String> {
    let mut session = load(rec, files).map_err(|e| format!("load: {e}"))?;

    match pipeline {
        "drc" => {
            let mut job = drc_job(files);
            let mut reporter = Reporter::new();
            let r = rec.stage("drc", || {
                kicad_session::run_drc(&mut session, &mut job, &mut reporter)
            });
            summarize(verbose, "drc", &r);
            r.map_err(|e| format!("drc: {e}"))?;
            drc_phases(rec, &mut session, verbose);
        }
        "fab" => {
            // pcb export gerbers: CLI defaults (PCB_EXPORT_GERBER_COMMAND::populateJob)
            let mut gerber = JobExportPcbGerbers::new();
            {
                let g = &mut gerber.gerber;
                g.plot.filename = files.path.clone();
                g.plot.plot_footprint_values = true;
                g.plot.plot_ref_des = true;
                g.plot.plot_drawing_sheet = false;
                g.disable_aperture_macros = false;
                g.plot.subtract_solder_mask_from_silk = false;
                g.include_netlist_attributes = true;
                g.use_x2_format = true;
                g.plot.use_drill_origin = false;
                g.use_protel_file_extension = true;
                g.precision = 6;
                g.plot.check_zones_before_plot = false;
                g.plot.arg_layers = Some(String::new());
                g.plot.arg_common_layers = Some(String::new());
                g.plot.creation_date = clock().gbr_creation_date();
            }
            gerber.use_board_plot_params = false;
            let mut reporter = Reporter::new();
            let r = rec.stage("gerbers", || {
                kicad_session::run_export_gerbers(&mut session, &mut gerber, &mut reporter)
            });
            summarize(verbose, "gerbers", &r);
            r.map_err(|e| format!("gerbers: {e}"))?;

            // pcb export drill: CLI defaults (PCB_EXPORT_DRILL_COMMAND::doPerform)
            let mut drill = JobExportPcbDrill::new();
            drill.filename = files.path.clone();
            drill.drill_units = kicad_session::jobs::DrillUnits::Mm;
            drill.excellon_oval_drill_route = false;
            drill.excellon_combine_pth_npth = true;
            drill.creation_date = clock().gbr_creation_date();
            let mut reporter = Reporter::new();
            let r = rec.stage("drill", || {
                kicad_session::run_export_drill(&mut session, &mut drill, &mut reporter)
            });
            summarize(verbose, "drill", &r);
            r.map_err(|e| format!("drill: {e}"))?;

            // pcb upgrade --force: the board writer
            let mut upgrade = JobPcbUpgrade::new();
            upgrade.filename = files.path.clone();
            upgrade.force = true;
            let mut reporter = Reporter::new();
            let r = rec.stage("write", || {
                kicad_session::run_pcb_upgrade(&mut session, &mut upgrade, &mut reporter)
            });
            summarize(verbose, "write", &r);
            r.map_err(|e| format!("write: {e}"))?;
        }
        // `fill`: the zone refill alone (for boards whose DRC cannot run)
        "refill" | "fill" => {
            let r = rec.stage("zone_refill", || session.refill_zones());
            r.map_err(|e| format!("zone_refill: {e}"))?;

            if pipeline == "fill" {
                rec.stage("fill.drop", || drop(session));
                return Ok(());
            }

            let mut job = drc_job(files);
            let mut reporter = Reporter::new();
            let r = rec.stage("drc_after_refill", || {
                kicad_session::run_drc(&mut session, &mut job, &mut reporter)
            });
            summarize(verbose, "drc_after_refill", &r);
            r.map_err(|e| format!("drc_after_refill: {e}"))?;
        }
        "edit" => edit_loop(rec, &mut session, verbose)?,
        "live" => live_loop(rec, &mut session, verbose)?,
        other => return Err(format!("unknown pipeline '{other}'")),
    }

    // Dropping a large board is not free either; it is part of every kicad-cli run.
    rec.stage(&format!("{pipeline}.drop"), || drop(session));
    Ok(())
}

/// Runs a pipeline, turning a `port_pending!` panic into an error.
fn run_guarded(
    rec: &mut Recorder,
    files: &BoardFiles,
    pipeline: &str,
    verbose: bool,
) -> Result<(), String> {
    match catch_unwind(AssertUnwindSafe(|| {
        run_pipeline(rec, files, pipeline, verbose)
    })) {
        Ok(r) => r,
        Err(p) => Err(format!(
            "panic: {}",
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        )),
    }
}

struct Args {
    cmd: String,
    board: String,
    reps: usize,
    warmup: usize,
    pipelines: Vec<String>,
    commit: String,
    label: String,
    mem: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().ok_or("missing command (run | mem)")?;
    let mut a = Args {
        cmd,
        board: String::new(),
        reps: 7,
        warmup: 1,
        pipelines: PIPELINES.iter().map(|s| s.to_string()).collect(),
        commit: "-".to_string(),
        label: String::new(),
        mem: true,
    };

    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or(format!("{arg} needs a value"));

        match arg.as_str() {
            "--board" => a.board = val()?,
            "--reps" => a.reps = val()?.parse().map_err(|e| format!("--reps: {e}"))?,
            "--warmup" => a.warmup = val()?.parse().map_err(|e| format!("--warmup: {e}"))?,
            "--pipelines" | "--pipeline" => {
                a.pipelines = val()?.split(',').map(str::to_string).collect()
            }
            "--commit" => a.commit = val()?,
            "--label" => a.label = val()?,
            "--no-mem" => a.mem = false,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }

    if a.board.is_empty() {
        return Err("--board is required".to_string());
    }

    if a.label.is_empty() {
        a.label = std::path::Path::new(&a.board)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    Ok(a)
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(f64::total_cmp);
    let n = v.len();

    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// `mem`: one pipeline in this (fresh) process, peak RSS per stage on stdout
/// (`stage<TAB>peak_kb` lines).
fn cmd_mem(a: &Args, files: &BoardFiles) -> ExitCode {
    let mut rec = Recorder {
        mem: true,
        keep: true,
        suffix: "",
        stages: Vec::new(),
    };

    for p in &a.pipelines {
        if let Err(e) = run_guarded(&mut rec, files, p, false) {
            eprintln!("kicad-bench mem: {}: {p}: {e}", a.label);
        }
    }

    for (name, samples) in &rec.stages {
        let peak = samples.iter().map(|s| s.peak_kb).max().unwrap_or(0);
        println!("{name}\t{peak}");
    }

    ExitCode::SUCCESS
}

/// The peaks of every pipeline, each measured in a child process (`mem`).
fn child_peaks(a: &Args) -> Vec<(String, u64)> {
    let mut peaks: Vec<(String, u64)> = Vec::new();
    let Ok(exe) = std::env::current_exe() else {
        return peaks;
    };

    for p in &a.pipelines {
        let out = std::process::Command::new(&exe)
            .args(["mem", "--board", &a.board, "--pipeline", p])
            .output();

        let Ok(out) = out else { continue };

        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let Some((name, kb)) = line.split_once('\t') else {
                continue;
            };
            let kb: u64 = kb.parse().unwrap_or(0);

            match peaks.iter_mut().find(|(n, _)| n == name) {
                Some((_, v)) => *v = (*v).max(kb),
                None => peaks.push((name.to_string(), kb)),
            }
        }
    }

    peaks
}

fn cmd_run(a: &Args, files: &BoardFiles) -> ExitCode {
    let peaks = if a.mem { child_peaks(a) } else { Vec::new() };
    let mut rec = Recorder::default();
    let mut failed = Vec::new();

    for rep in 0..a.warmup + a.reps {
        // The first repetition of the process pays the one-time initialisations (lazy
        // statics, fonts, ...) that every kicad-rs invocation pays: kept as `<stage> [cold]`.
        rec.keep = rep >= a.warmup || rep == 0;
        rec.suffix = if rep < a.warmup { " [cold]" } else { "" };

        for p in &a.pipelines {
            if failed.contains(p) {
                continue;
            }

            if let Err(e) = run_guarded(&mut rec, files, p, rep == 0) {
                eprintln!("kicad-bench: {}: pipeline {p}: {e}", a.label);
                failed.push(p.clone());
            }
        }
    }

    println!("{TSV_HEADER}");

    for (name, samples) in &rec.stages {
        let mut ms: Vec<f64> = samples.iter().map(|s| s.ms).collect();
        let med = median(&mut ms);
        let peak = peaks
            .iter()
            .find(|(n, _)| n == name)
            .map_or("-".to_string(), |(_, kb)| {
                format!("{:.1}", *kb as f64 / 1024.0)
            });
        let (allocs, alloc_mb) = if cfg!(feature = "count-alloc") {
            let mut c: Vec<f64> = samples.iter().map(|s| s.allocs as f64).collect();
            let mut b: Vec<f64> = samples.iter().map(|s| s.alloc_bytes as f64).collect();
            (
                format!("{:.0}", median(&mut c)),
                format!("{:.1}", median(&mut b) / 1048576.0),
            )
        } else {
            ("-".to_string(), "-".to_string())
        };

        println!(
            "{}\t{}\t{name}\t{}\t{:.2}\t{med:.2}\t{:.2}\t{peak}\t{allocs}\t{alloc_mb}",
            a.commit,
            a.label,
            ms.len(),
            ms[0],
            ms[ms.len() - 1],
        );
    }

    for p in &failed {
        println!("{}\t{}\t{p}\tFAILED\t-\t-\t-\t-\t-\t-", a.commit, a.label);
    }

    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let a = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("kicad-bench: {e}");
            return ExitCode::from(2);
        }
    };

    // port_pending! panics are reported per pipeline, not on stderr each time
    std::panic::set_hook(Box::new(|_| {}));

    let files = match BoardFiles::read(&a.board) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("kicad-bench: {e}");
            return ExitCode::from(1);
        }
    };

    match a.cmd.as_str() {
        "run" => cmd_run(&a, &files),
        "mem" => cmd_mem(&a, &files),
        other => {
            eprintln!("kicad-bench: unknown command '{other}'");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_odd_and_even() {
        assert_eq!(median(&mut [3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&mut [4.0, 1.0, 2.0, 3.0]), 2.5);
    }

    #[test]
    fn proc_status_is_read() {
        // Linux only; on other systems the reader returns 0
        if std::path::Path::new("/proc/self/status").exists() {
            assert!(proc_status_kb("VmRSS:") > 0);
        }
    }
}
