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

//! DRC-31 unit tests ported from `qa/tests/pcbnew/drc/test_drc_chain_topology.cpp`,
//! `test_drc_chain_length_trunk.cpp` and the `ViaElectricalSpan` case of
//! `test_drc_lengths.cpp`.
//!
//! The C++ tests assign net chains and terminal pads in code (`SetNetChain`,
//! `SetTerminalPad`); the port has no mutable net access on a loaded board, so the same
//! assignment is written as a `(net_chains ...)` block into a copy of the board, which the
//! loader reads; the terminal pads are then resolved from their UUIDs (the load itself drops
//! the pad pointers, see `load_tagged`).

mod drc_harness;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use kicad_connectivity::data::ConnectivityData;
use kicad_core::layer_ids::PcbLayerId;
use kicad_core::typeinfo::KicadT;
use kicad_drc::chain_topology::{ChainTopology, ChainTopologyStatus};
use kicad_drc::length_delay_calculation::LengthDelayCalculation;
use kicad_pcb::{Board, BoardCtx, ItemId};

const MM: f64 = 1e6;

fn data(rel: &str) -> PathBuf {
    kicad_qa::kicad_source_root()
        .join("qa/data/pcbnew")
        .join(rel)
}

/// `BOOST_CHECK_CLOSE( a, b, pct )`.
fn close(a: f64, b: f64, pct: f64) -> bool {
    let d = (a - b).abs();
    d <= (pct / 100.0) * a.abs() && d <= (pct / 100.0) * b.abs()
}

/// How the terminal pads of the chain are chosen.
enum Terminals {
    /// `tagChain`: the first pad of the footprint `first`, the last pad of `last`.
    ByReference(&'static str, &'static str),
    /// `tagAndSetTerminals`: the first pad of the extreme-X footprints.
    ExtremeX,
    /// Only terminal 0: the first pad of the first footprint.
    FirstOnly,
}

/// Writes a copy of the board with every net whose name starts with `/NET_` in chain `chain`
/// and the terminals chosen by `terminals`, and loads it.
fn load_tagged(rel: &str, chain: &str, terminals: Terminals, dir: &Path) -> Board {
    let src = data(rel);
    let text = std::fs::read_to_string(&src).unwrap();
    let plain = drc_harness::load_board(&src).unwrap().board;
    let ctx = BoardCtx::bare(&plain);

    let pads_of = |fp| -> Vec<String> {
        plain
            .footprint_pads(fp)
            .map(|(id, _)| ctx.item(id.item_id()).unwrap().uuid().as_string())
            .collect()
    };

    let mut term = [None, None];

    match terminals {
        Terminals::ByReference(first, last) => {
            for (fp, f) in plain.footprints() {
                let pads = pads_of(fp);

                if f.reference(&ctx) == first && !pads.is_empty() {
                    term[0] = pads.first().cloned();
                }

                if f.reference(&ctx) == last && !pads.is_empty() {
                    term[1] = pads.last().cloned();
                }
            }
        }
        Terminals::ExtremeX => {
            let mut first: Option<(i32, Vec<String>)> = None;
            let mut last: Option<(i32, Vec<String>)> = None;

            for (fp, f) in plain.footprints() {
                let pads = pads_of(fp);

                if pads.is_empty() {
                    continue;
                }

                let x = f.position(&ctx).x;

                if first.as_ref().is_none_or(|(fx, _)| x < *fx) {
                    first = Some((x, pads.clone()));
                }

                if last.as_ref().is_none_or(|(lx, _)| x > *lx) {
                    last = Some((x, pads));
                }
            }

            term[0] = first.map(|(_, p)| p[0].clone());
            term[1] = last.map(|(_, p)| p[0].clone());
        }
        Terminals::FirstOnly => {
            let (fp, _) = plain.footprints().next().unwrap();
            term[0] = pads_of(fp).first().cloned();
        }
    }

    let members: Vec<String> = plain
        .nets()
        .filter(|n| n.netname().starts_with("/NET_"))
        .map(|n| format!("\t\t\t\t(net \"{}\")\n", n.netname()))
        .collect();

    let mut block =
        format!("\t(net_chains\n\t\t(net_chain\n\t\t\t(name \"{chain}\")\n\t\t\t(members\n");
    block += &members.concat();
    block += "\t\t\t)\n";

    for t in term.iter().flatten() {
        block += &format!("\t\t\t(terminal_pad \"{t}\")\n");
    }

    block += "\t\t)\n\t)\n";

    let end = text.trim_end().rfind(')').unwrap();
    let mut tagged = text.clone();
    tagged.insert_str(end, &block);

    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(src.file_name().unwrap());
    std::fs::write(&path, tagged).unwrap();

    let mut board = drc_harness::load_board(&path).unwrap().board;

    // BuildListOfNets() of the load clears the terminal pad pointers and keeps only their
    // UUIDs (NETINFO_LIST::buildListOfNets), so a loaded board has no terminal pads: resolve
    // them again from the UUIDs, as SetTerminalPad() does in the C++ test.
    board.resolve_net_chain_terminal_pads();
    board
}

/// `chainItems`: the tracks and pads of the board on the chain.
fn chain_items(board: &Board, chain: &str) -> BTreeSet<ItemId> {
    let ctx = BoardCtx::bare(board);
    let on_chain = |id: ItemId| {
        ctx.item(id)
            .and_then(|i| i.net())
            .is_some_and(|n| n.net_chain() == chain)
    };
    let mut items = BTreeSet::new();

    for (t, _) in board.tracks() {
        if on_chain(t.item_id()) {
            items.insert(t.item_id());
        }
    }

    for (fp, _) in board.footprints() {
        for (p, _) in board.footprint_pads(fp) {
            if on_chain(p.item_id()) {
                items.insert(p.item_id());
            }
        }
    }

    items
}

fn topology(board: &mut Board, chain: &str) -> ChainTopology {
    let mut conn = ConnectivityData::new();
    conn.build_connectivity(board);
    let board: &Board = board;
    let ctx = BoardCtx::bare(board).with_connectivity(&conn);
    let calc = LengthDelayCalculation::new(&ctx, Some(&conn));
    ChainTopology::new(&calc, chain, &chain_items(board, chain), 0.0)
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kicad-drc31-topo-{}-{name}", std::process::id()))
}

/// `TopologyTreeOnSimpleTrunk`: three-net trunk through two passives, 100 mm, no stubs.
#[test]
fn topology_tree_on_simple_trunk() {
    let mut board = load_tagged(
        "net_chains/chain_topology_trunk.kicad_pcb",
        "SIG",
        Terminals::ByReference("FP_START", "FP_END"),
        &tmp("trunk"),
    );
    let topo = topology(&mut board, "SIG");

    assert_eq!(topo.status(), ChainTopologyStatus::Ok);
    assert!(topo.is_valid());
    assert!(
        close(topo.trunk_length(), 100.0 * MM, 5.0),
        "{}",
        topo.trunk_length()
    );
    assert!(topo.stubs().is_empty());
}

/// `TopologyDetectsTStub`: single-net trunk plus a perpendicular 5 mm T-stub at the midpoint.
#[test]
fn topology_detects_t_stub() {
    let mut board = load_tagged(
        "net_chains/chain_topology_t_stub.kicad_pcb",
        "TSIG",
        Terminals::ByReference("FP_START", "FP_END"),
        &tmp("tstub"),
    );
    let topo = topology(&mut board, "TSIG");

    assert!(topo.is_valid(), "{:?}", topo.status());
    assert_eq!(topo.stubs().len(), 1);

    // Fixture trunk runs (32,49) to (82,49); the stub taps it at the midpoint
    let stub = &topo.stubs()[0];
    assert!((f64::from(stub.branch_point.x) - 57.0 * MM).abs() <= 100.0);
    assert!((f64::from(stub.branch_point.y) - 49.0 * MM).abs() <= 100.0);
    assert!(close(stub.length, 5.0 * MM, 5.0), "{}", stub.length);

    // Trunk length is 50 mm and the stub does not contribute.
    assert!(
        close(topo.trunk_length(), 50.0 * MM, 5.0),
        "{}",
        topo.trunk_length()
    );
}

/// `TopologyMissingTerminalPad`: only one terminal pad set.
#[test]
fn topology_missing_terminal_pad() {
    let mut board = load_tagged(
        "net_chains/chain_topology_missing_terminal.kicad_pcb",
        "MISSING",
        Terminals::FirstOnly,
        &tmp("missing"),
    );
    let topo = topology(&mut board, "MISSING");

    assert_eq!(topo.status(), ChainTopologyStatus::NoTerminalPads);
}

/// `TopologyDisconnected`: both terminals set, no track between them.
#[test]
fn topology_disconnected() {
    let mut board = load_tagged(
        "net_chains/chain_topology_disconnected.kicad_pcb",
        "DISC",
        Terminals::ByReference("FP_START", "FP_END"),
        &tmp("disc"),
    );
    let topo = topology(&mut board, "DISC");

    assert_eq!(topo.status(), ChainTopologyStatus::Disconnected);
}

/// `TopologyCycleDetected`: two parallel paths between the terminals.
#[test]
fn topology_cycle_detected() {
    let mut board = load_tagged(
        "net_chains/chain_topology_cycle.kicad_pcb",
        "LOOP",
        Terminals::ByReference("FP_START", "FP_END"),
        &tmp("cycle"),
    );
    let topo = topology(&mut board, "LOOP");

    assert_eq!(topo.status(), ChainTopologyStatus::CycleDetected);
}

/// `DaisyChainTrunkEqualsSumExplicit` (`test_drc_chain_length_trunk.cpp`): two-net daisy
/// through a single bridge; trunk 21.55 + 1.9 + 26.55 = 50 mm.
#[test]
fn daisy_chain_trunk_equals_sum_explicit() {
    let mut board = load_tagged(
        "net_chains/chain_length_daisy.kicad_pcb",
        "DSY",
        Terminals::ExtremeX,
        &tmp("daisy"),
    );
    let topo = topology(&mut board, "DSY");

    assert!(topo.is_valid(), "{:?}", topo.status());
    assert!(
        close(topo.trunk_length(), 50.0e6, 5.0),
        "{}",
        topo.trunk_length()
    );
}

/// `BranchedChainTrunkExcludesStubs` (`test_drc_chain_length_trunk.cpp`).
#[test]
fn branched_chain_trunk_excludes_stubs() {
    let mut board = load_tagged(
        "net_chains/chain_length_branched.kicad_pcb",
        "BR",
        Terminals::ExtremeX,
        &tmp("branched"),
    );
    let topo = topology(&mut board, "BR");

    assert!(topo.is_valid(), "{:?}", topo.status());
    assert!(
        close(topo.trunk_length(), 50.0e6, 5.0),
        "{}",
        topo.trunk_length()
    );
    assert_eq!(topo.stubs().len(), 3);
}

/// `ViaElectricalSpan` (`test_drc_lengths.cpp`, GitLab #22536): the electrical span of a via
/// comes from the layers where traces connect, not from its physical span.
#[test]
fn via_electrical_span() {
    let mut board = drc_harness::load_board(&data("issue22536.kicad_pcb"))
        .unwrap()
        .board;
    let mut conn = ConnectivityData::new();
    conn.build_connectivity(&mut board);
    let board: &Board = &board;
    let ctx = BoardCtx::bare(board).with_connectivity(&conn);
    let calc = LengthDelayCalculation::new(&ctx, Some(&conn));

    // Find vias on Net 3 (Net-(J1-Pin_3)) which has:
    // - TH vias with physical span F.Cu to B.Cu
    // - Traces on F.Cu and In1.Cu only
    // The electrical span should be F.Cu to In1.Cu, not F.Cu to B.Cu
    let net3 = board.find_net_by_name("Net-(J1-Pin_3)").expect("net 3");
    let mut via_count = 0;

    for (t, _) in board.tracks() {
        let item = ctx.item(t.item_id()).unwrap();

        if item.type_() != KicadT::PcbViaT || item.net_code() != Some(net3.net_code) {
            continue;
        }

        // Verify the via is physically a TH via spanning F.Cu to B.Cu
        let via = item.as_via().unwrap();
        assert_eq!(via.top_layer(), PcbLayerId::FCu);
        assert_eq!(via.bottom_layer(), PcbLayerId::BCu);

        // Calculate the electrical span using CalculateViaLayers
        let length_item = calc.length_calculation_item(item);
        let (start_layer, end_layer) = length_item.layers();

        assert_eq!(start_layer, PcbLayerId::FCu);
        assert_eq!(end_layer, PcbLayerId::In1Cu);

        via_count += 1;
    }

    assert_eq!(via_count, 2); // Net 3 has 2 vias
}
