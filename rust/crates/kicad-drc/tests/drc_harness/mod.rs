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

//! The shared differential DRC corpus harness of the provider packages (built by DRC-06).
//!
//! * Reference side: `tests/golden/drc_corpus.txt`, the `violations` of
//!   `kicad-cli pcb drc --format json --severity-all --units mm` for every corpus board
//!   (`tests/oracle/gen_drc_corpus.py`, raw reports cached under `rust/target/drc-corpus/`).
//! * Rust side: the board is loaded like `KI_TEST::LoadBoard` (parser, project, rule file,
//!   `InitEngine`), `RunTests` runs with the providers under test only, and the violations are
//!   turned into report entries as `PCBNEW_JOBS_HANDLER::JobExportDrc` does it (markers,
//!   project exclusions, `DRC_ITEMS_PROVIDER`, the JSON report of DRC-11).
//! * Both sides are filtered to the error types (settings keys) of the providers under test and
//!   compared: per type matched / missing / extra (multisets of the entry text), and the order
//!   of the filtered entries.
//!
//! Usage from a provider test (`mod drc_harness;`):
//!
//! ```ignore
//! let score = drc_harness::compare_corpus(&["annular_width"], "", &mut || {
//!     vec![Box::new(DrcTestProviderAnnularWidth::default())]
//! });
//! score.print("annular width");
//! score.assert_no_mismatch();
//! ```

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use kicad_core::Severity;
use kicad_core::eda_units::EdaUnits;
use kicad_core::json::{self, ObjectOrder};
use kicad_core::json_settings::JsonSettingsInternals;
use kicad_core::kiid::KiidGenerator;
use kicad_drc::engine::implicit_condition::EngineCondition;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use kicad_drc::exclusion::KiapiDrcExclusionCodec;
use kicad_drc::exclusion::{record_drc_exclusions, resolve_drc_exclusions};
use kicad_drc::item::DrcItemsProvider;
use kicad_drc::report::DrcReport;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_pcb::board::BoardProject;
use kicad_pcb::io::{ParseOptions, ParseReport, parse_board_report};
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker, RcItem};
use kicad_pcb::project::board_design_settings_json::load_board_design_settings;
use kicad_pcb::project::net_settings_json::load_net_settings;
use kicad_pcb::{Board, BoardCtx};

#[path = "../common/mod.rs"]
mod common;

/// One report entry, reduced to what the comparison uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `severity`
    pub severity: String,
    /// `type` (settings key)
    pub type_: String,
    /// `description`
    pub description: String,
    /// `items`: uuid, description, x, y (mm)
    pub items: Vec<(String, String, f64, f64)>,
    /// `excluded` (exclusions stored in the project).
    pub excluded: bool,
}

impl Entry {
    /// The comparison text of the entry (everything but the order).
    pub fn key(&self) -> String {
        let excluded = if self.excluded { " (excluded)" } else { "" };
        let mut s = format!(
            "{} | {}{excluded} | {}",
            self.type_, self.severity, self.description
        );

        for (uuid, desc, x, y) in &self.items {
            s += &format!(" | {uuid} {desc} @({x}, {y})");
        }

        s
    }
}

/// FNV-1a 64 (continuing from `h`).
pub fn fnv1a64(mut h: u64, data: &[u8]) -> u64 {
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001B3);
    }

    h
}

/// The FNV-1a 64 offset basis.
pub const FNV_OFFSET: u64 = 0xCBF29CE484222325;

/// The text an entry contributes to a summary hash (`hash_line` of `gen_drc_corpus.py`):
/// positions in IU.
pub fn hash_text(e: &Entry) -> String {
    let severity = if e.excluded {
        format!("{}/excluded", e.severity)
    } else {
        e.severity.clone()
    };
    let mut fields = vec![severity, e.type_.clone(), e.description.clone()];

    for (uuid, desc, x, y) in &e.items {
        fields.push(uuid.clone());
        fields.push(desc.clone());
        fields.push(format!("{}", (x * 1e6).round() as i64));
        fields.push(format!("{}", (y * 1e6).round() as i64));
    }

    let fields: Vec<String> = fields
        .into_iter()
        .map(|f| f.replace(['\t', '\n'], " "))
        .collect();
    fields.join("\t") + "\n"
}

/// Count and hash of a list of entries (report order).
pub fn summarize(entries: &[&Entry]) -> (usize, u64) {
    let mut h = FNV_OFFSET;

    for e in entries {
        h = fnv1a64(h, hash_text(e).as_bytes());
    }

    (entries.len(), h)
}

/// The reference side of one board.
#[derive(Clone, Debug, Default)]
pub struct BoardReference {
    /// The entries stored in full, in report order.
    pub entries: Vec<Entry>,
    /// Types stored as a summary only: count and hash.
    pub summaries: BTreeMap<String, (usize, u64)>,
}

/// The reference per board: `None` when the reference run failed.
pub fn golden() -> BTreeMap<String, Option<BoardReference>> {
    let text = include_str!("../golden/drc_corpus.txt");
    let mut boards: BTreeMap<String, Option<BoardReference>> = BTreeMap::new();

    for line in text.lines() {
        let f: Vec<&str> = line.split('\t').collect();

        match f.get(1).copied() {
            Some("load_error") => {
                boards.insert(f[0].to_string(), None);
                continue;
            }
            Some("none") => {
                boards.insert(f[0].to_string(), Some(BoardReference::default()));
                continue;
            }
            _ => {}
        }

        let board = boards
            .entry(f[0].to_string())
            .or_insert_with(|| Some(BoardReference::default()))
            .as_mut()
            .unwrap();

        if f[1] == "#" {
            board.summaries.insert(
                f[2].to_string(),
                (
                    f[3].parse().unwrap(),
                    u64::from_str_radix(f[4], 16).unwrap(),
                ),
            );
            continue;
        }

        let mut items = Vec::new();

        for it in f[5..].chunks(4) {
            items.push((
                it[0].to_string(),
                it[1].to_string(),
                it[2].parse().unwrap(),
                it[3].parse().unwrap(),
            ));
        }

        board.entries.push(Entry {
            severity: f[2].trim_end_matches("/excluded").to_string(),
            excluded: f[2].ends_with("/excluded"),
            type_: f[3].to_string(),
            description: f[4].to_string(),
            items,
        });
    }

    boards
}

/// The entries of `type_` in the cached raw reference report
/// (`rust/target/drc-corpus/<rel>.json`), if the cache exists (for diagnostics).
pub fn cached_entries(rel: &str, type_: &str) -> Option<Vec<Entry>> {
    let path = kicad_qa::kicad_source_root()
        .join("rust/target/drc-corpus")
        .join(format!("{rel}.json"));
    let doc = json::parse_bytes(&std::fs::read(path).ok()?, ObjectOrder::Sorted, true).ok()?;
    let mut out = Vec::new();

    for v in doc["violations"].as_array()? {
        if v["type"].as_str() != Some(type_) {
            continue;
        }

        out.push(Entry {
            severity: v["severity"].as_str()?.to_string(),
            excluded: v["excluded"].as_bool().unwrap_or(false),
            type_: type_.to_string(),
            description: v["description"].as_str()?.to_string(),
            items: v["items"]
                .as_array()?
                .iter()
                .map(|i| {
                    (
                        i["uuid"].as_str().unwrap_or("").to_string(),
                        i["description"].as_str().unwrap_or("").to_string(),
                        i["pos"]["x"].as_f64().unwrap_or(0.0),
                        i["pos"]["y"].as_f64().unwrap_or(0.0),
                    )
                })
                .collect(),
        });
    }

    Some(out)
}

/// Multiset difference of two entry lists: (matched, missing, extra) with the texts.
pub fn diff_entries(reference: &[Entry], rust: &[Entry]) -> (usize, Vec<String>, Vec<String>) {
    let mut remaining: Vec<Option<String>> = rust.iter().map(|e| Some(e.key())).collect();
    let mut matched = 0;
    let mut missing = Vec::new();

    for e in reference {
        let key = e.key();

        if let Some(slot) = remaining.iter_mut().find(|k| k.as_deref() == Some(&key)) {
            *slot = None;
            matched += 1;
        } else {
            missing.push(key);
        }
    }

    (matched, missing, remaining.into_iter().flatten().collect())
}

/// The part of `TUNING_PROFILES` that `loadImplicitRules` reads (see `evalrules_oracle.rs`).
pub fn read_tuning_profiles(doc: &json::Value) -> TuningProfiles {
    use kicad_core::{Lset, PcbLayerId};
    use kicad_drc::engine::implicit_rules::{
        DelayProfileTrackPropagationEntry, TuningProfile, TuningProfileType,
    };

    let mut profiles = Vec::new();
    let list = doc
        .pointer("/tuning_profiles/tuning_profiles_impedance_geometric")
        .and_then(|v| v.as_array());

    for entry in list.into_iter().flatten() {
        let int = |v: &json::Value, key: &str| v[key].as_i64().unwrap_or(0) as i32;
        let entries = entry["layer_entries"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|layer| DelayProfileTrackPropagationEntry {
                signal_layer: PcbLayerId::from(Lset::name_to_layer(
                    layer["signal_layer"].as_str().unwrap_or(""),
                )),
                width: int(layer, "width"),
                diff_pair_gap: int(layer, "diff_pair_gap"),
            })
            .collect();

        profiles.push(TuningProfile {
            profile_name: entry["profile_name"].as_str().unwrap_or("").to_string(),
            type_: if int(entry, "type") == 1 {
                TuningProfileType::Differential
            } else {
                TuningProfileType::Single
            },
            track_propagation_entries: entries,
        });
    }

    TuningProfiles::new(profiles)
}

/// A board loaded for a DRC run.
pub struct Loaded {
    pub board: Board,
    pub report: ParseReport,
    pub engine: DrcEngine,
}

/// The QA load path (`KI_TEST::LoadBoard`): parser, project file (design settings, net
/// settings, tuning profiles), rule file, `InitEngine`. Rule conditions compile with the DRC-20
/// stand-in binding until DRC-21 lands.
pub fn load_board(path: &Path) -> Result<Loaded, String> {
    let mut tuning_profiles = TuningProfiles::default();
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let name = path.file_name().unwrap().to_string_lossy().to_string();
    let options = ParseOptions {
        allow_pending: true,
    };
    let (mut board, report) =
        parse_board_report(&text, &name, &options).map_err(|e| e.to_string())?;

    let pro = path.with_extension("kicad_pro");

    if pro.exists() {
        let doc = json::parse_bytes(&std::fs::read(&pro).unwrap(), ObjectOrder::Sorted, true)
            .map_err(|e| format!("project: {e:?}"))?;
        let mut parent = JsonSettingsInternals {
            original: doc.clone(),
            doc,
        };

        if !board.legacy_design_settings_loaded {
            let _ = load_board_design_settings(
                &mut parent,
                board.design_settings_mut(),
                std::rc::Rc::new(KiapiDrcExclusionCodec::default()),
            );
        }

        let (net_settings, _) = load_net_settings(Some(&parent));
        board.design_settings_mut().net_settings = net_settings;
        board.project = Some(BoardProject::default());

        tuning_profiles = read_tuning_profiles(&parent.doc);
    } else if path.with_extension("pro").exists() {
        return Err("legacy .pro project (not readable by the port)".to_string());
    }

    let dru = path.with_extension("kicad_dru");
    let dru_text = dru
        .exists()
        .then(|| String::from_utf8_lossy(&std::fs::read(&dru).unwrap()).to_string());
    let dru_path = dru.to_string_lossy().to_string();

    let mut engine = DrcEngine::new();
    let result = engine.init_engine_with::<common::SyntaxBinder>(
        &mut board,
        dru_text.as_deref().map(|t| (t, dru_path.as_str())),
        default_layer_choices(),
        &tuning_profiles,
        None,
    );

    if result.is_err() {
        return Err("InitEngine failed".to_string());
    }

    Ok(Loaded {
        board,
        report,
        engine,
    })
}

/// True when the engine holds a user rule whose condition needs the expression binding
/// (DRC-21).
pub fn has_compiled_conditions(engine: &DrcEngine) -> bool {
    engine.constraint_map().values().flatten().any(|c| {
        !c.parent_rule.is_implicit() && matches!(c.condition, Some(EngineCondition::Compiled(_)))
    })
}

/// A violation as the violation handler receives it, with the marker the CLI's handler makes
/// of it (`new PCB_MARKER( aItem, aPos, aLayer )`, path generator applied).
#[derive(Clone)]
pub struct Reported {
    pub item: Arc<RcItem>,
    pub pos: kicad_geom::math::vector2::Vector2I,
    pub layer: i32,
    pub marker: PcbMarker,
}

/// `RunTests( units, reportAllTrackErrors, testFootprints = false )` with `providers` only
/// (the cache generator is not run: providers that need its caches are not supported yet).
/// Returns the violations in handler order; the board is not changed. Marker UUIDs (never part
/// of a report) come from a private generator: the board's default-seeded generator reproduces
/// the UUIDs of boards that were themselves written by a deterministically seeded KiCad (e.g.
/// `issue10697`), and a marker with an item's UUID would hijack the item resolution.
pub fn run_providers(
    loaded: &mut Loaded,
    units: EdaUnits,
    report_all_track_errors: bool,
    providers: &mut [&mut dyn DrcTestProviderRun],
) -> Vec<Reported> {
    let mut found = Vec::new();
    let mut generator = KiidGenerator::seeded(0xD5C0_0006);

    {
        let mut run = DrcRunState::new();
        run.set_violation_handler(Box::new(|item, pos, layer, path| {
            let mut marker = PcbMarker::new(&mut generator, item.clone(), pos, layer);

            if let Some(path) = path {
                path(&mut marker);
            }

            found.push(Reported {
                item: item.clone(),
                pos,
                layer,
                marker,
            });
        }));

        loaded.engine.prepare_run(
            &mut run,
            &mut loaded.board,
            units,
            report_all_track_errors,
            false,
        );

        let ctx = BoardCtx::bare(&loaded.board).with_rules(&loaded.engine);
        loaded
            .engine
            .run_providers(&mut run, &ctx, units, providers);
    }

    found
}

/// The `violations` of `kicad-cli pcb drc --format json --severity-all --units mm` after a run
/// of `providers` (`BOARD_LOADER::Load`: `ResolveDRCExclusions( true )`; then
/// `PCBNEW_JOBS_HANDLER::JobExportDrc`): `RecordDRCExclusions`,
/// `DeleteMARKERs( true, true )`, `RunTests`, the markers added in handler order,
/// `ResolveDRCExclusions( false )`, `DRC_ITEMS_PROVIDER::SetSeverities` and the JSON report of
/// DRC-11.
pub fn cli_report_entries(
    loaded: &mut Loaded,
    report_all_track_errors: bool,
    providers: &mut [&mut dyn DrcTestProviderRun],
) -> Vec<Entry> {
    // BOARD_LOADER::Load: the markers of the exclusions stored in the project.
    for marker in resolve_drc_exclusions(&mut loaded.board, true) {
        loaded.board.add_marker(marker);
    }

    record_drc_exclusions(&mut loaded.board);
    loaded.board.delete_markers(true, true);

    let found = run_providers(loaded, EdaUnits::Mm, report_all_track_errors, providers);

    for r in found {
        loaded.board.add_marker(r.marker);
    }

    resolve_drc_exclusions(&mut loaded.board, false);

    let severities = Severity::Error as i32 | Severity::Warning as i32 | Severity::Exclusion as i32;
    let ctx = BoardCtx::bare(&loaded.board).with_rules(&loaded.engine);
    let mut markers = DrcItemsProvider::new(MarkerT::Drc, MarkerT::DrawingSheet);
    let mut ratsnest = DrcItemsProvider::new(MarkerT::Ratsnest, MarkerT::Unspec);
    let mut parity = DrcItemsProvider::new(MarkerT::Parity, MarkerT::Unspec);
    markers.set_severities(&ctx, severities);
    ratsnest.set_severities(&ctx, severities);
    parity.set_severities(&ctx, severities);

    let report = DrcReport::new(&ctx, EdaUnits::Mm, &markers, &ratsnest, &parity);

    report
        .json_report("", "")
        .violations
        .into_iter()
        .map(|v| Entry {
            severity: v.severity,
            excluded: v.excluded,
            type_: v.type_,
            description: v.description,
            items: v
                .items
                .into_iter()
                .map(|i| (i.uuid, i.description, i.pos.x, i.pos.y))
                .collect(),
        })
        .collect()
}

/// Matched / missing / extra entries of one error type.
#[derive(Default, Debug, Clone)]
pub struct TypeScore {
    pub matched: usize,
    pub missing: usize,
    pub extra: usize,
}

/// The result of a corpus comparison.
#[derive(Default, Debug)]
pub struct Scoreboard {
    /// Boards compared (loaded completely, no rule conditions pending).
    pub compared: Vec<String>,
    /// Boards not compared, with the reason.
    pub pending: Vec<String>,
    /// Per type.
    pub per_type: BTreeMap<String, TypeScore>,
    /// Every difference: board, then the missing (`-`) / extra (`+`) entries or the order.
    pub differences: Vec<String>,
    /// Differences on boards the parser loaded only partially (not counted as failures).
    pub partial_differences: Vec<String>,
}

impl Scoreboard {
    /// Prints the scoreboard to stderr (`cargo test -- --nocapture` shows it).
    pub fn print(&self, title: &str) {
        eprintln!(
            "{title}: {} boards compared, {} pending",
            self.compared.len(),
            self.pending.len()
        );

        for (t, s) in &self.per_type {
            eprintln!(
                "  {t}: matched {} missing {} extra {}",
                s.matched, s.missing, s.extra
            );
        }

        for d in &self.differences {
            eprintln!("  DIFF {d}");
        }

        for d in &self.partial_differences {
            eprintln!("  diff on a partially loaded board: {d}");
        }

        for p in &self.pending {
            eprintln!("  pending: {p}");
        }
    }

    /// Fails the test on a difference on a completely loaded board.
    pub fn assert_no_mismatch(&self) {
        assert!(
            self.differences.is_empty(),
            "{} differences:\n{}",
            self.differences.len(),
            self.differences.join("\n")
        );
    }
}

/// Runs `panic`-prone code silently.
pub fn quiet<R>(f: impl FnOnce() -> R) -> R {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = f();
    std::panic::set_hook(hook);
    r
}

/// The message of a caught panic.
pub fn panic_text(e: &(dyn std::any::Any + Send)) -> String {
    e.downcast_ref::<String>()
        .cloned()
        .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
        .unwrap_or_default()
}

/// Compares the providers made by `make` on every golden board against the reference,
/// filtered to `types`. `filter` limits the boards (path substring; empty = all).
pub fn compare_corpus(
    types: &[&str],
    filter: &str,
    make: &mut dyn FnMut() -> Vec<Box<dyn DrcTestProviderRun>>,
) -> Scoreboard {
    let root = kicad_qa::kicad_source_root();
    let mut score = Scoreboard::default();

    for t in types {
        score.per_type.entry(t.to_string()).or_default();
    }

    for (rel, reference) in golden() {
        if !rel.contains(filter) {
            continue;
        }

        let Some(reference) = reference else {
            score.pending.push(format!("{rel}: reference run failed"));
            continue;
        };

        let path = root.join(&rel);
        let result = quiet(|| {
            catch_unwind(AssertUnwindSafe(
                || -> Result<(Vec<Entry>, bool), String> {
                    let mut loaded = load_board(&path)?;

                    if has_compiled_conditions(&loaded.engine) {
                        return Err("user rule conditions need DRC-21".to_string());
                    }

                    let mut providers = make();
                    let mut refs: Vec<&mut dyn DrcTestProviderRun> = Vec::new();

                    for p in providers.iter_mut() {
                        refs.push(&mut **p);
                    }
                    let entries = cli_report_entries(&mut loaded, false, &mut refs)
                        .into_iter()
                        .filter(|e| types.contains(&e.type_.as_str()))
                        .collect();

                    Ok((entries, loaded.report.skipped.is_empty()))
                },
            ))
        });

        let (rust, complete) = match result {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                score.pending.push(format!("{rel}: {e}"));
                continue;
            }
            Err(e) => {
                score
                    .pending
                    .push(format!("{rel}: panic: {}", panic_text(&*e)));
                continue;
            }
        };

        let mut diffs = Vec::new();
        let mut per_type: BTreeMap<String, TypeScore> = BTreeMap::new();

        // Types stored as summaries: count and hash of the entries in report order.
        for t in types {
            let Some(&(n, h)) = reference.summaries.get(*t) else {
                continue;
            };

            let mine: Vec<&Entry> = rust.iter().filter(|e| e.type_ == *t).collect();
            let s = per_type.entry(t.to_string()).or_default();

            if summarize(&mine) == (n, h) {
                s.matched += n;
                continue;
            }

            let owned: Vec<Entry> = mine.into_iter().cloned().collect();

            match cached_entries(&rel, t) {
                Some(cached) => {
                    let (m, missing, extra) = diff_entries(&cached, &owned);
                    s.matched += m;
                    s.missing += missing.len();
                    s.extra += extra.len();

                    for k in missing {
                        diffs.push(format!("{rel}: - {k}"));
                    }

                    for k in extra {
                        diffs.push(format!("{rel}: + {k}"));
                    }

                    if s.missing == 0 && s.extra == 0 {
                        diffs.push(format!("{rel}: {t}: same entries in a different order"));
                    }
                }
                None => {
                    s.missing += n;
                    s.extra += owned.len();
                    diffs.push(format!(
                        "{rel}: {t}: {} entries, reference {n} (hash differs; no cached report \
                         for details, run gen_drc_corpus.py)",
                        owned.len()
                    ));
                }
            }
        }

        // Types stored in full: multiset per type, then the order.
        let full_ref: Vec<Entry> = reference
            .entries
            .into_iter()
            .filter(|e| types.contains(&e.type_.as_str()))
            .collect();
        let full_rust: Vec<Entry> = rust
            .iter()
            .filter(|e| !reference.summaries.contains_key(&e.type_))
            .cloned()
            .collect();

        for t in types {
            if reference.summaries.contains_key(*t) {
                continue;
            }

            let r: Vec<Entry> = full_ref.iter().filter(|e| e.type_ == *t).cloned().collect();
            let m: Vec<Entry> = full_rust
                .iter()
                .filter(|e| e.type_ == *t)
                .cloned()
                .collect();
            let (matched, missing, extra) = diff_entries(&r, &m);
            let s = per_type.entry(t.to_string()).or_default();
            s.matched += matched;
            s.missing += missing.len();
            s.extra += extra.len();

            for k in missing {
                diffs.push(format!("{rel}: - {k}"));
            }

            for k in extra {
                diffs.push(format!("{rel}: + {k}"));
            }
        }

        if diffs.is_empty() && full_rust != full_ref {
            diffs.push(format!("{rel}: same entries in a different order"));
        }

        if complete {
            score.compared.push(rel.clone());

            for (t, s) in per_type {
                let total = score.per_type.entry(t).or_default();
                total.matched += s.matched;
                total.missing += s.missing;
                total.extra += s.extra;
            }

            score.differences.extend(diffs);
        } else {
            score.pending.push(format!(
                "{rel}: parser skipped items ({} diffs)",
                diffs.len()
            ));
            score.partial_differences.extend(diffs);
        }
    }

    score
}
