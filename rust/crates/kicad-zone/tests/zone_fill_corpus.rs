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

//! Zone-fill corpus harness (connectivity-zones.md 9.2): for every board cached by
//! `rust/tools/zone-corpus/run.sh` under `rust/target/zone-corpus/<name>/` (`in/`: the input
//! copy, `ref/`: the same board after `kicad-cli pcb drc --refill-zones --save-board`), load
//! the input like kicad-cli (`BOARD_LOADER::Load`), refill every board zone like
//! `ZONE_FILLER_TOOL::FillAllZones` (time stamp bump, `ZONE_FILLER::Fill( board->Zones() )`)
//! and compare each zone layer's `filled_polygon`s with the reference vertex for vertex:
//! identical / same area but different vertices / different (with the first differing
//! outline and vertex), plus the `island` flags.
//!
//! The test skips silently without cached references. It reports and does not fail (boards
//! still depend on pending packages: ZF-03 copper fill, footprints, teardrops) unless
//! `ZONE_CORPUS_STRICT=1`. The report is also written to `target/zone-corpus/report.txt`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kicad_connectivity::data::ConnectivityData;
use kicad_core::PcbLayerId;
use kicad_drc::engine::DrcEngine;
use kicad_drc::engine::implicit_rules::TuningProfiles;
use kicad_drc::rule_parser::default_layer_choices;
use kicad_geom::ShapePolySet;
use kicad_pcb::Board;
use kicad_pcb::ids::ZoneId;
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::project::board_design_settings_json::DrcExclusionCodec;
use kicad_pcb::project::board_design_settings_json::RawDrcExclusionCodec;
use kicad_pcb::project::{LoadPath, LoadStep, StepOutcome, load_project, run_model_step};
use kicad_zone::ZoneFiller;

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/zone-corpus")
}

/// The single `.kicad_pcb` of a directory.
fn board_in(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .find(|p| p.extension().is_some_and(|e| e == "kicad_pcb"))
}

fn panic_text(payload: Box<dyn std::any::Any + Send>) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".to_string())
}

/// Teardrop regeneration (`TEARDROP_MANAGER::UpdateTeardrops`, package TD-01) would change the
/// board before the fill: boards with enabled teardrop parameters or teardrop zones are
/// reported, not compared, unless they use legacy teardrops.
fn needs_teardrops(text: &str) -> bool {
    if text.contains("(legacy_teardrops yes)") {
        return false;
    }

    if text.contains("(attr teardrop") {
        return true;
    }

    text.match_indices("(teardrops").any(|(i, _)| {
        let end = (i + 600).min(text.len());
        text.get(i..end)
            .is_some_and(|s| s.contains("(enabled yes)"))
    })
}

/// `BOARD_LOADER::Load` as kicad-cli runs it, with the DRC engine and connectivity of the
/// session steps.
#[allow(clippy::type_complexity)]
fn load_cli(path: &Path) -> Result<(Board, DrcEngine, ConnectivityData, Vec<String>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let name = path.to_str().unwrap();
    let mut board =
        parse_board(&text, name, &ParseOptions::default()).map_err(|e| format!("{e:?}"))?;
    board.file_name = name.to_string();

    let pro_path = path.with_extension("kicad_pro");
    let pro = std::fs::read_to_string(&pro_path).ok();
    let project = load_project(pro.as_deref(), None, pro_path.to_str().unwrap());
    let rules_path = path.with_extension("kicad_dru");
    let rules = std::fs::read_to_string(&rules_path).ok();

    let mut engine = DrcEngine::new();
    let mut connectivity = ConnectivityData::new();
    let mut skipped_steps = Vec::new();

    // initialize_loaded_board() step by step, so that a model step still pending in another
    // package (e.g. component classes with no footprints to act on) is recorded and skipped
    // instead of aborting the comparison.
    let codec: Rc<dyn DrcExclusionCodec> = Rc::new(RawDrcExclusionCodec);
    let mut project = Some(project);

    for &step in LoadPath::Cli.steps() {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            run_model_step(&mut board, LoadPath::Cli, step, &mut project, &codec)
        }));

        match outcome {
            Ok(Ok(StepOutcome::Session)) => match step {
                LoadStep::InitDrcEngine => {
                    // exceptions swallowed, like the C++ loader
                    let _ = engine.init_engine(
                        &mut board,
                        rules.as_deref().map(|t| (t, rules_path.to_str().unwrap())),
                        default_layer_choices(),
                        &TuningProfiles::default(),
                        None,
                    );
                }
                LoadStep::BuildConnectivity => {
                    connectivity.build_connectivity(&mut board);
                }
                _ => {}
            },
            Ok(Ok(_)) => {}
            Ok(Err(e)) => return Err(format!("{e:?}")),
            Err(p) => skipped_steps.push(format!("{step:?} ({})", panic_text(p))),
        }
    }

    Ok((board, engine, connectivity, skipped_steps))
}

#[derive(Default)]
struct Score {
    identical: usize,
    same_area: usize,
    different: usize,
}

/// Compares one zone layer; `None` if identical, else the description of the difference.
fn compare_layer(
    reference: &ShapePolySet,
    ref_islands: &[bool],
    port: &ShapePolySet,
    port_islands: &[bool],
) -> Option<(bool, String)> {
    let same_area = (reference.area() - port.area()).abs() <= 1e-9 * reference.area().abs();

    if reference.outline_count() != port.outline_count() {
        return Some((
            same_area,
            format!(
                "outline count {} vs {} (area {} vs {})",
                reference.outline_count(),
                port.outline_count(),
                reference.area(),
                port.area()
            ),
        ));
    }

    for ii in 0..reference.outline_count() {
        let a = reference.c_outline(ii);
        let b = port.c_outline(ii);

        if a.point_count() != b.point_count() {
            return Some((
                same_area,
                format!(
                    "outline {ii}: {} vs {} points",
                    a.point_count(),
                    b.point_count()
                ),
            ));
        }

        for jj in 0..a.point_count() {
            if a.c_point(jj) != b.c_point(jj) {
                return Some((
                    same_area,
                    format!(
                        "outline {ii} point {jj}: {:?} vs {:?}",
                        a.c_point(jj),
                        b.c_point(jj)
                    ),
                ));
            }
        }

        if ref_islands[ii as usize] != port_islands[ii as usize] {
            return Some((false, format!("outline {ii}: island flag differs")));
        }
    }

    None
}

fn islands(board: &Board, zone: ZoneId, layer: PcbLayerId, count: i32) -> Vec<bool> {
    let z = board.zone(zone).unwrap();
    (0..count).map(|i| z.is_island(layer, i)).collect()
}

/// Refills one cached board and compares it with the reference; one report line.
fn run_board(dir: &Path, score: &mut Score) -> String {
    let (Some(input), Some(reference)) = (board_in(&dir.join("in")), board_in(&dir.join("ref")))
    else {
        return "no reference".to_string();
    };

    let text = std::fs::read_to_string(&input).unwrap_or_default();

    if needs_teardrops(&text) {
        return "skipped: teardrop regeneration (TD-01) pending".to_string();
    }

    let loaded = catch_unwind(AssertUnwindSafe(|| load_cli(&input)));
    let (mut board, engine, mut connectivity, skipped_steps) = match loaded {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => return format!("load error: {e}"),
        Err(p) => return format!("load panic: {}", panic_text(p)),
    };

    let ref_text = std::fs::read_to_string(&reference).unwrap_or_default();
    let ref_board = match parse_board(&ref_text, "ref.kicad_pcb", &ParseOptions::default()) {
        Ok(b) => b,
        Err(e) => return format!("reference parse error: {e:?}"),
    };

    // ZONE_FILLER_TOOL::FillAllZones: IncrementTimeStamp, then Fill( board->Zones() ).
    board.increment_time_stamp();
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    let filled = catch_unwind(AssertUnwindSafe(|| {
        let mut filler = ZoneFiller::new(&board);
        filler.fill(&mut board, &mut connectivity, &engine, &zones);
    }));

    let skipped = if skipped_steps.is_empty() {
        String::new()
    } else {
        format!(" [load steps skipped: {}]", skipped_steps.join(", "))
    };

    if let Err(p) = filled {
        return format!("fill panic: {}; {}{skipped}", panic_text(p), smoke(&input));
    }

    let ref_zones: BTreeMap<String, ZoneId> = ref_board
        .zones()
        .map(|(id, z)| (z.base.item.uuid.as_string(), id))
        .collect();

    let mut local = Score::default();
    let mut first_diff = String::new();

    for &zone_id in &zones {
        let zone = board.zone(zone_id).unwrap();

        if zone.is_rule_area() {
            continue;
        }

        let uuid = zone.base.item.uuid.as_string();
        let Some(&ref_id) = ref_zones.get(&uuid) else {
            local.different += 1;
            continue;
        };

        for layer in zone.layer_set.iter() {
            let empty = ShapePolySet::new();
            let port = zone.get_fill(layer).unwrap_or(&empty);
            let ref_zone = ref_board.zone(ref_id).unwrap();
            let refp = ref_zone.get_fill(layer).unwrap_or(&empty);

            let diff = compare_layer(
                refp,
                &islands(&ref_board, ref_id, layer, refp.outline_count()),
                port,
                &islands(&board, zone_id, layer, port.outline_count()),
            );

            match diff {
                None => local.identical += 1,
                Some((same_area, what)) => {
                    if same_area {
                        local.same_area += 1;
                    } else {
                        local.different += 1;
                    }

                    if first_diff.is_empty() {
                        first_diff = format!("; first: zone {uuid} {layer:?}: {what}");
                    }
                }
            }
        }
    }

    score.identical += local.identical;
    score.same_area += local.same_area;
    score.different += local.different;

    format!(
        "zone layers identical {}, same area {}, different {}{}{skipped}",
        local.identical, local.same_area, local.different, first_diff
    )
}

/// Driver smoke run while the copper fill is pending: the same refill with the test stand-in
/// fill (`set_trivial_copper_fill`), which runs every driver pass (flashing, DAG, islands,
/// refill, out-of-board islands) on the real board.
fn smoke(input: &Path) -> String {
    let Ok(Ok((mut board, engine, mut connectivity, _))) =
        catch_unwind(AssertUnwindSafe(|| load_cli(input)))
    else {
        return "driver smoke: load failed".to_string();
    };

    board.increment_time_stamp();
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();

    let run = catch_unwind(AssertUnwindSafe(|| {
        let mut filler = ZoneFiller::new(&board);
        filler.set_trivial_copper_fill(true);
        filler.fill(&mut board, &mut connectivity, &engine, &zones);
        filler.fill_order().len()
    }));

    match run {
        Ok(items) => format!("driver smoke (trivial fill): ok, {items} fill items"),
        Err(p) => format!("driver smoke (trivial fill): panic: {}", panic_text(p)),
    }
}

#[test]
fn zone_fill_corpus() {
    let Ok(entries) = std::fs::read_dir(corpus_dir()) else {
        return; // no cached references: skip silently
    };

    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("ref").is_dir())
        .collect();
    dirs.sort();

    let only = std::env::var("ZONE_CORPUS_ONLY").ok();
    let mut score = Score::default();
    let mut report = String::new();

    // Port-pending panics are expected while ZF-03 is open; keep the output readable.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    for dir in &dirs {
        let name = dir.file_name().unwrap().to_string_lossy().to_string();

        if only.as_deref().is_some_and(|o| !name.contains(o)) {
            continue;
        }

        let line = run_board(dir, &mut score);
        let _ = writeln!(report, "{name}: {line}");
    }

    std::panic::set_hook(hook);

    let _ = writeln!(
        report,
        "TOTAL zone layers identical {}, same area {}, different {}",
        score.identical, score.same_area, score.different
    );

    println!("{report}");
    let _ = std::fs::write(corpus_dir().join("report.txt"), &report);

    if std::env::var("ZONE_CORPUS_STRICT").is_ok_and(|v| v == "1") {
        assert_eq!(score.same_area + score.different, 0, "{report}");
    }
}
