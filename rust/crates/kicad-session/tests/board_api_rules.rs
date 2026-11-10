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

//! WASM-API-1: `Session::constraint` against the reference `DRC_ENGINE::EvalRules`
//! (`pcb-dump --sections evalrules`; golden `tests/golden/board_api_rules.txt`, generator
//! `tests/oracle/gen_board_api_rules_golden.sh`), and the net-only queries / router sizes on
//! a board with net classes.

mod common;

use kicad_core::{Kiid, PcbLayerId};
use kicad_core::lset::Lset;
use kicad_session::board_api::rules::{
    ALL_CONSTRAINT_TYPES, ConstraintQuery, ConstraintResult, ItemSel, constraint_kind,
};
use kicad_session::{LoadInputs, Session};

fn load(rel: &str) -> Session {
    let path = kicad_qa::kicad_source_root().join(rel);
    let text = std::fs::read_to_string(&path).expect("board");
    let pro = std::fs::read_to_string(path.with_extension("kicad_pro")).ok();
    let dru = std::fs::read_to_string(path.with_extension("kicad_dru")).ok();
    let name = path.to_string_lossy().into_owned();
    let inputs = LoadInputs {
        text: &text,
        source_name: &name,
        project_json: pro.as_deref(),
        rules_text: dru.as_deref(),
        ..LoadInputs::default()
    };
    common::load_staged(kicad_pcb::project::LoadPath::Cli, &inputs)
        .expect("load")
        .0
}

/// `constraintText` of dump_evalrules.cpp without `zc` (the API does not return it).
fn text(r: &ConstraintResult) -> String {
    let t = ALL_CONSTRAINT_TYPES
        .iter()
        .position(|t| kicad_session::board_api::enum_name(*t) == r.constraint_type)
        .expect("type") as i32;
    let o = |v: Option<i32>| v.map_or("none".to_string(), |v| v.to_string());
    format!(
        "t={t};v={}/{}/{};n={};mr={};im={};d={};s={}",
        o(r.min),
        o(r.opt),
        o(r.max),
        kicad_pcb::dump::format::v_str(&r.name),
        r.min_rule
            .as_ref()
            .map_or("-".to_string(), |n| kicad_pcb::dump::format::v_str(n)),
        i32::from(r.implicit_min),
        r.disallow,
        r.severity_code
    )
}

fn strip_zc(v: &str) -> String {
    v.split(';')
        .filter(|f| !f.starts_with("zc="))
        .collect::<Vec<_>>()
        .join(";")
}

#[test]
fn constraints_match_eval_rules() {
    let golden = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/board_api_rules.txt"),
    )
    .expect("golden");
    let mut session: Option<Session> = None;
    let mut compared = 0;
    let mut mismatches = Vec::new();

    for line in golden.lines() {
        if let Some(rest) = line.strip_prefix("file|") {
            session = Some(load(rest.trim_end_matches('|')));
            continue;
        }

        let s = session.as_mut().expect("file line first");
        let mut parts = line.splitn(3, '|');
        let (_, key, rest) = (parts.next(), parts.next().unwrap(), parts.next().unwrap());
        let (field, value) = rest.split_once('=').unwrap();
        let (t, layer) = field[1..].split_once('@').unwrap();
        let t: usize = t.parse().unwrap();
        let layer: i32 = layer.parse().unwrap();
        let ids: Vec<u64> = key
            .split(':')
            .skip(1)
            .map(|u| {
                s.board
                    .resolve_item(&Kiid::parse(u).expect("uuid"))
                    .expect("item")
                    .to_bits()
            })
            .collect();
        let sel = |id: u64| ItemSel {
            id: Some(id),
            ..ItemSel::default()
        };
        let q = ConstraintQuery {
            kind: constraint_kind(ALL_CONSTRAINT_TYPES[t]),
            a: sel(ids[0]),
            b: ids.get(1).map(|&id| sel(id)),
            layer: if layer < 0 {
                String::new()
            } else {
                Lset::name(PcbLayerId(layer))
            },
        };
        let r = s.constraint(&q).expect("constraint");
        compared += 1;

        if text(&r) != strip_zc(value) {
            mismatches.push(format!("{key} {field}: {} != {value}", text(&r)));
        }
    }

    eprintln!("compared {compared} constraints");
    assert!(compared > 500);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
fn net_queries_and_router_sizes() {
    let mut s = load("qa/data/pcbnew/multinetclasses_drc.kicad_pcb");
    let nets = s.net_details();
    let net = nets.iter().find(|n| n.code > 0).expect("a net").code;
    let q = ConstraintQuery {
        kind: "track_width".into(),
        a: ItemSel {
            net: Some(net),
            ..ItemSel::default()
        },
        b: None,
        layer: "F.Cu".into(),
    };
    let r = s.constraint(&q).unwrap();
    let sizes = s.router_sizes(net, "F.Cu").unwrap();
    assert_eq!(r.constraint_type, "TRACK_WIDTH_CONSTRAINT");
    let bds = s.board.design_settings();

    // ImportSizes: max( m_TrackMinWidth, PinnedOpt ) with UseNetClassTrack(), else
    // max( m_TrackMinWidth, GetCurrentTrackWidth() ).
    let expected = if bds.use_net_class_track() {
        // PinnedOpt(): the optimum clamped to min .. max
        r.min
            .unwrap_or(0)
            .max(r.max.unwrap_or(i32::MAX).min(r.opt.unwrap()))
    } else {
        bds.get_current_track_width()
    };
    assert_eq!(sizes.track_width, expected.max(bds.track_min_width), "{r:?} {sizes:?}");
    assert!(s.constraint(&ConstraintQuery { kind: "nope".into(), ..q.clone() }).is_err());
}
