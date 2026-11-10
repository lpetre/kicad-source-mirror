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


#[test]
fn tmp_dbg() {
    let dir = std::env::var("DBG_BOARD").unwrap();
    let (mut board, engine, mut connectivity, _sk) = load_cli(Path::new(&dir)).unwrap();
    let zones: Vec<ZoneId> = board.zones().map(|(id, _)| id).collect();
    {
        let ctx = kicad_pcb::ctx::BoardCtx::bare(&board).with_rules(&engine);
        for &a in &zones { for &b in &zones { if a==b {continue;}
            for layer in [PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::BCu] {
                let c = kicad_pcb::ctx::RuleResolver::eval_rules(&engine, &ctx, kicad_pcb::drc_api::DrcConstraintT::ClearanceConstraint, ctx.item(a.into()), ctx.item(b.into()), layer);
                eprintln!("{:?} {:?} {:?}: null={} min={}", a, b, layer, c.is_null(), c.value().min());
            }}}
    }
    board.increment_time_stamp();
    let mut filler = ZoneFiller::new(&board);
    filler.fill(&mut board, &mut connectivity, &engine, &zones);
}
