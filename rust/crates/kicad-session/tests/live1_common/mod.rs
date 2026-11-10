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

//! Package LIVE-1 test support: the board list, seeded random edit batches (as a script the
//! C++ driver `tests/oracle/live1_oracle.cpp` replays), the batch as a [`Transaction`], and
//! the connectivity facts compared between the incremental path, a full rebuild and the
//! reference. The fact lines are printed in the same format by the C++ driver
//! (`fact_lines` there): every line is `section|...`, sorted, so that the comparison is a
//! set comparison where the reference's order depends on heap addresses.

#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use kicad_connectivity::data::ConnectivityData;
use kicad_connectivity::items::{CnItemId, CnList};
use kicad_core::lset::Lset;
use kicad_core::{KicadT, PcbLayerId};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::{Board, ItemId};
use kicad_session::Session;
use kicad_session::board_api::commit::Transaction;
use kicad_session::board_api::edit::{ItemChange, NewItem, NewTrack, NewVia};

/// The 30 boards of the incremental test (the first 10 are the oracle boards).
pub fn boards() -> Vec<String> {
    let list = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/live1_boards.txt");
    std::fs::read_to_string(list)
        .expect("live1_boards.txt")
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect()
}

pub fn board_path(rel: &str) -> PathBuf {
    kicad_qa::kicad_source_root().join(rel)
}

/// Loads a board through the QA path the C++ driver uses (`KI_TEST::ReadBoardFromFileOrStream`
/// then `BuildConnectivity`): parse, then a full connectivity build.
pub fn load(rel: &str) -> Option<Session> {
    let path = board_path(rel);
    let text = std::fs::read_to_string(&path).ok()?;
    let board = kicad_session::session::parse_board_text(&text, &path.to_string_lossy()).ok()?;
    let mut s = Session::new_empty(&path.to_string_lossy());
    s.board = board;
    s.rebuild_connectivity();
    Some(s)
}

// ------------------------------------------------------------------------------------------
// Seeded batches
// ------------------------------------------------------------------------------------------

/// SplitMix64.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }

    pub fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    pub fn offset(&mut self, max: i32) -> i32 {
        (self.next() % (2 * max as u64 + 1)) as i32 - max
    }

    pub fn uuid(&mut self) -> String {
        let a = self.next();
        let b = self.next();
        format!(
            "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
            a >> 32,
            (a >> 16) & 0xffff,
            a & 0xfff,
            b >> 52 & 0xfff,
            b & 0xffff_ffff_ffff
        )
    }
}

/// One scripted edit. The text form (one line, space separated) is what the C++ driver reads.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// `R <uuid>`
    Remove(String),
    /// `T <uuid> x1 y1 x2 y2 width layer net`
    Track {
        uuid: String,
        s: [i32; 2],
        e: [i32; 2],
        w: i32,
        layer: i32,
        net: i32,
    },
    /// `V <uuid> x y diameter drill net free`
    Via {
        uuid: String,
        at: [i32; 2],
        d: i32,
        drill: i32,
        net: i32,
        free: bool,
    },
    /// `M <uuid> x y` (`SetPosition` of a via, `SetStart` of a track)
    Move { uuid: String, at: [i32; 2] },
    /// `N <uuid> net`
    Net { uuid: String, net: i32 },
}

impl Op {
    pub fn line(&self) -> String {
        match self {
            Op::Remove(u) => format!("R {u}"),
            Op::Track {
                uuid,
                s,
                e,
                w,
                layer,
                net,
            } => {
                format!(
                    "T {uuid} {} {} {} {} {w} {layer} {net}",
                    s[0], s[1], e[0], e[1]
                )
            }
            Op::Via {
                uuid,
                at,
                d,
                drill,
                net,
                free,
            } => {
                format!(
                    "V {uuid} {} {} {d} {drill} {net} {}",
                    at[0], at[1], *free as i32
                )
            }
            Op::Move { uuid, at } => format!("M {uuid} {} {}", at[0], at[1]),
            Op::Net { uuid, net } => format!("N {uuid} {net}"),
        }
    }
}

/// A route item as the generator sees it.
#[derive(Clone, Debug)]
struct Routed {
    uuid: String,
    via: bool,
    start: [i32; 2],
    end: [i32; 2],
    net: i32,
}

/// The state the generator draws from (kept up to date with its own batches).
pub struct Gen {
    rng: Rng,
    routed: Vec<Routed>,
    /// Pad positions with their nets.
    pads: Vec<([i32; 2], i32)>,
    /// Points inside copper zones.
    zone_points: Vec<[i32; 2]>,
    nets: Vec<i32>,
    layers: Vec<i32>,
    widths: Vec<i32>,
}

impl Gen {
    pub fn new(s: &Session, seed: u64) -> Gen {
        let routed = s
            .route_items()
            .into_iter()
            .map(|r| Routed {
                uuid: r.uuid,
                via: r.kind == "via",
                start: r.start,
                end: r.end,
                net: r.net,
            })
            .collect::<Vec<_>>();
        let pads = s
            .pads(false)
            .into_iter()
            .map(|p| (p.position, p.net))
            .collect();
        let zone_points = s
            .zones_info(false)
            .into_iter()
            .filter(|z| !z.rule_area)
            .flat_map(|z| z.outline.into_iter().take(1))
            .filter(|p| p.outline.len() >= 3)
            .map(|p| {
                // the mean of three consecutive corners: inside for convex corners
                let o = &p.outline;
                let x = (o[0][0] as i64 + o[1][0] as i64 + o[2][0] as i64) / 3;
                let y = (o[0][1] as i64 + o[1][1] as i64 + o[2][1] as i64) / 3;
                [x as i32, y as i32]
            })
            .collect();
        let nets = s.net_details().into_iter().map(|n| n.code).collect();
        let layers = s
            .board_info()
            .copper_layers
            .into_iter()
            .map(|l| l.layer_id)
            .collect();
        let mut widths: Vec<i32> = Vec::new();
        for r in s.route_items() {
            if r.kind != "via" && !widths.contains(&r.width) {
                widths.push(r.width);
            }
        }
        if widths.is_empty() {
            widths.push(250_000);
        }

        Gen {
            rng: Rng::new(seed),
            routed,
            pads,
            zone_points,
            nets,
            layers,
            widths,
        }
    }

    fn point(&mut self) -> ([i32; 2], i32) {
        let k = self.rng.below(10);

        if k < 4 && !self.pads.is_empty() {
            self.pads[self.rng.below(self.pads.len())]
        } else if k < 8 && !self.routed.is_empty() {
            let r = &self.routed[self.rng.below(self.routed.len())];
            (if self.rng.chance(50) { r.start } else { r.end }, r.net)
        } else if !self.zone_points.is_empty() {
            (self.zone_points[self.rng.below(self.zone_points.len())], 0)
        } else if !self.pads.is_empty() {
            self.pads[self.rng.below(self.pads.len())]
        } else {
            (
                [self.rng.offset(50_000_000), self.rng.offset(50_000_000)],
                0,
            )
        }
    }

    fn net_for(&mut self, own: i32, other: i32) -> i32 {
        match self.rng.below(20) {
            0..=13 => own,
            14..=16 => other,
            17..=18 if !self.nets.is_empty() => self.nets[self.rng.below(self.nets.len())],
            _ => 0,
        }
    }

    /// One random batch: removals, moves, net changes and additions in random order.
    pub fn batch(&mut self) -> Vec<Op> {
        let mut ops = Vec::new();
        let mut touched = BTreeSet::new();

        for _ in 0..self.rng.below(5) {
            if self.routed.is_empty() {
                break;
            }
            let i = self.rng.below(self.routed.len());
            let r = self.routed.swap_remove(i);
            touched.insert(r.uuid.clone());
            ops.push(Op::Remove(r.uuid));
        }

        for _ in 0..self.rng.below(3) {
            let vias: Vec<usize> = (0..self.routed.len())
                .filter(|&i| self.routed[i].via)
                .collect();
            if vias.is_empty() {
                break;
            }
            let i = vias[self.rng.below(vias.len())];
            if touched.contains(&self.routed[i].uuid) {
                continue;
            }
            let at = if self.rng.chance(50) {
                self.point().0
            } else {
                let p = self.routed[i].start;
                [
                    p[0] + self.rng.offset(400_000),
                    p[1] + self.rng.offset(400_000),
                ]
            };
            self.routed[i].start = at;
            self.routed[i].end = at;
            touched.insert(self.routed[i].uuid.clone());
            ops.push(Op::Move {
                uuid: self.routed[i].uuid.clone(),
                at,
            });
        }

        for _ in 0..self.rng.below(3) {
            if self.routed.is_empty() || self.nets.is_empty() {
                break;
            }
            let i = self.rng.below(self.routed.len());
            if touched.contains(&self.routed[i].uuid) {
                continue;
            }
            let net = self.nets[self.rng.below(self.nets.len())];
            self.routed[i].net = net;
            touched.insert(self.routed[i].uuid.clone());
            ops.push(Op::Net {
                uuid: self.routed[i].uuid.clone(),
                net,
            });
        }

        for _ in 0..1 + self.rng.below(6) {
            let (a, na) = self.point();
            let (b, nb) = if self.rng.chance(60) {
                self.point()
            } else {
                (
                    [
                        a[0] + self.rng.offset(2_000_000),
                        a[1] + self.rng.offset(2_000_000),
                    ],
                    na,
                )
            };
            let net = self.net_for(na, nb);
            let layer = self.layers[self.rng.below(self.layers.len())];
            let w = self.widths[self.rng.below(self.widths.len())];
            let uuid = self.rng.uuid();
            self.routed.push(Routed {
                uuid: uuid.clone(),
                via: false,
                start: a,
                end: b,
                net,
            });
            ops.push(Op::Track {
                uuid,
                s: a,
                e: b,
                w,
                layer,
                net,
            });
        }

        for _ in 0..self.rng.below(3) {
            let (at, na) = self.point();
            let net = self.net_for(na, 0);
            let free = self.rng.chance(20);
            let uuid = self.rng.uuid();
            self.routed.push(Routed {
                uuid: uuid.clone(),
                via: true,
                start: at,
                end: at,
                net,
            });
            ops.push(Op::Via {
                uuid,
                at,
                d: 600_000,
                drill: 300_000,
                net,
                free,
            });
        }

        // a random staging order (C++ m_entries order)
        for i in (1..ops.len()).rev() {
            let j = self.rng.below(i + 1);
            ops.swap(i, j);
        }

        ops
    }
}

/// UUID string -> handle of every item a script can name.
pub fn uuid_handles(s: &Session) -> HashMap<String, u64> {
    s.route_items()
        .into_iter()
        .map(|r| (r.uuid, r.id))
        .collect()
}

/// The batch as a [`Transaction`] (handles resolved through `handles`).
pub fn transaction(ops: &[Op], handles: &HashMap<String, u64>) -> Transaction {
    let mut txn = Transaction::begin();
    let layer = |l: i32| kicad_session::board_api::layer_name(PcbLayerId::from(l));

    for op in ops {
        match op {
            Op::Remove(u) => {
                txn.stage_remove(handles[u]);
            }
            Op::Move { uuid, at } => {
                txn.stage_change(ItemChange {
                    id: handles[uuid],
                    start: Some(*at),
                    ..ItemChange::default()
                });
            }
            Op::Net { uuid, net } => {
                txn.stage_change(ItemChange {
                    id: handles[uuid],
                    net: Some(*net),
                    ..ItemChange::default()
                });
            }
            Op::Track {
                uuid,
                s,
                e,
                w,
                layer: l,
                net,
            } => {
                txn.stage_add(NewItem::Track(NewTrack {
                    start: *s,
                    end: *e,
                    width: Some(*w),
                    layer: layer(*l),
                    net: *net,
                    uuid: Some(uuid.clone()),
                    ..NewTrack::default()
                }));
            }
            Op::Via {
                uuid,
                at,
                d,
                drill,
                net,
                free,
            } => {
                txn.stage_add(NewItem::Via(NewVia {
                    at: *at,
                    diameter: Some(*d),
                    drill: Some(*drill),
                    net: *net,
                    free: *free,
                    uuid: Some(uuid.clone()),
                    ..NewVia::default()
                }));
            }
        }
    }

    txn
}

// ------------------------------------------------------------------------------------------
// Facts
// ------------------------------------------------------------------------------------------

fn uuid_of(ctx: &BoardCtx<'_>, id: ItemId) -> String {
    ctx.item(id)
        .map_or_else(|| "null".into(), |it| it.uuid().as_string())
}

/// The reference name of a connectivity item: the parent's UUID, plus
/// `/<layer>/<outline>` for a zone layer item.
fn cn_ref(ctx: &BoardCtx<'_>, list: &CnList, id: CnItemId) -> String {
    let item = list.item(id);

    if !item.valid() {
        return "invalid".into();
    }

    let mut r = uuid_of(ctx, item.parent());

    if let Some(zl) = item.zone_layer() {
        r.push_str(&format!(
            "/{}/{}",
            i32::from(item.board_layer()),
            zl.subpoly_index()
        ));
    }

    r
}

fn anchor_text(
    ctx: &BoardCtx<'_>,
    list: &CnList,
    a: &kicad_connectivity::items::CnAnchor,
) -> String {
    let r = a
        .item()
        .map_or_else(|| "null".into(), |i| cn_ref(ctx, list, i));
    format!("{},{}@{}", a.pos().x, a.pos().y, r)
}

/// An edge end: the anchor position only. Which of several coincident anchors of a cluster
/// ends an edge depends on the build history (list order: review-connectivity.md, ratsnest
/// ends under ties); the node sets keep the items.
fn end_text(a: &kicad_connectivity::items::CnAnchor) -> String {
    format!("{},{}", a.pos().x, a.pos().y)
}

/// An edge end as the reference driver prints it: the position, `/z` for a zone layer anchor.
fn flagged_end(list: &CnList, a: &kicad_connectivity::items::CnAnchor) -> String {
    let zone = a
        .item()
        .is_some_and(|i| list.item(i).zone_layer().is_some());
    format!("{}{}", end_text(a), if zone { "/z" } else { "" })
}

/// One ratsnest edge of an `rn_edges` line.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeRec {
    pub ends: String,
    pub weight: u32,
    pub zone: bool,
    pub tied: bool,
}

/// The edges of the edge list of an `rn_edges` line (`a>b:w`, `!` = tie record of the port).
pub fn parse_edges(list: &str) -> Vec<EdgeRec> {
    list.split(' ')
        .filter(|e| !e.is_empty())
        .map(|e| {
            let tied = e.ends_with('!');
            let e = e.trim_end_matches('!');
            let (ends, w) = e.rsplit_once(':').unwrap();
            EdgeRec {
                zone: ends.contains("/z"),
                ends: ends.to_string(),
                weight: w.parse().unwrap(),
                tied,
            }
        })
        .collect()
}

/// Whether two ratsnest edge sets of one net are the same up to the history dependence of
/// the edge ends (review-connectivity.md F-1, sites P7 - P9): the weights are equal as a
/// multiset; the ends of an edge are compared only where they cannot depend on list or heap
/// order: weight above 1 (not between coincident anchors of different clusters), no zone
/// layer end (`optimizeZoneAnchor` / zone-to-zone candidates), no tie record. Each side's
/// exact edges must be on the other side, or be matched by an edge of the same weight whose
/// ends are history dependent there.
pub fn edges_equivalent(a: &[EdgeRec], b: &[EdgeRec]) -> bool {
    let weights = |v: &[EdgeRec]| {
        let mut w: Vec<u32> = v.iter().map(|e| e.weight).collect();
        w.sort();
        w
    };

    if weights(a) != weights(b) {
        return false;
    }

    let eligible = |e: &EdgeRec| e.weight > 1 && !e.zone && !e.tied;
    let within = |x: &[EdgeRec], y: &[EdgeRec]| {
        let mut pool: Vec<&EdgeRec> = y.iter().collect();
        x.iter().filter(|e| eligible(e)).all(|e| {
            // the same edge, else an edge of the same weight whose ends are history dependent
            let hit = pool
                .iter()
                .position(|p| eligible(p) && p.ends == e.ends)
                .or_else(|| {
                    pool.iter()
                        .position(|p| !eligible(p) && p.weight == e.weight)
                });
            match hit {
                Some(i) => {
                    pool.swap_remove(i);
                    true
                }
                None => false,
            }
        })
    };

    within(a, b) && within(b, a)
}

/// Splits fact lines into the `rn_edges` edge sets per net and the other lines.
pub fn split_edges(lines: &[String]) -> (BTreeMap<i32, Vec<EdgeRec>>, Vec<String>) {
    let mut edges = BTreeMap::new();
    let mut rest = Vec::new();
    for l in lines {
        if let Some(r) = l.strip_prefix("rn_edges|") {
            let (net, list) = r.split_once('|').unwrap();
            edges.insert(net.parse().unwrap(), parse_edges(list));
        } else {
            rest.push(l.clone());
        }
    }
    (edges, rest)
}

/// The nets whose edge sets are not [`edges_equivalent`].
pub fn edge_differences(
    a: &BTreeMap<i32, Vec<EdgeRec>>,
    b: &BTreeMap<i32, Vec<EdgeRec>>,
) -> Vec<i32> {
    let nets: BTreeSet<i32> = a.keys().chain(b.keys()).copied().collect();
    nets.into_iter()
        .filter(|n| {
            !edges_equivalent(
                a.get(n).map_or(&[][..], Vec::as_slice),
                b.get(n).map_or(&[][..], Vec::as_slice),
            )
        })
        .collect()
}

const NON_ZONE: [KicadT; 4] = [
    KicadT::PcbTraceT,
    KicadT::PcbArcT,
    KicadT::PcbViaT,
    KicadT::PcbPadT,
];

/// The connectivity facts of the oracle (printed identically by the C++ driver), sorted.
/// Mutates `conn` (`GetClusters` re-runs the cluster search): pass a copy where the live
/// state must stay untouched.
pub fn fact_lines(board: &Board, conn: &mut ConnectivityData) -> Vec<String> {
    let ctx = BoardCtx::bare(board);
    let mut out = Vec::new();

    // net of every connected item
    for (id, t) in board.tracks() {
        out.push(format!(
            "net|{}|{}",
            uuid_of(&ctx, id.into()),
            t.as_track().base.net.0
        ));
    }

    let mut pads = Vec::new();
    for (fp, _) in board.footprints() {
        for (pad, _) in board.footprint_pads(fp) {
            pads.push(ItemId::from(pad));
        }
    }

    for &pad in &pads {
        let net = ctx.item(pad).and_then(|p| p.net_code()).map_or(0, |n| n.0);
        out.push(format!("net|{}|{}", uuid_of(&ctx, pad), net));
    }

    // cluster partition (CSM_RATSNEST), as sets
    let clusters = conn.algo_mut().get_clusters(&ctx).to_vec();
    {
        let algo = conn.algo();
        let list = algo.item_list();

        for c in clusters {
            let cl = algo.cluster(c);
            let mut members: Vec<String> =
                cl.items().iter().map(|&i| cn_ref(&ctx, list, i)).collect();
            members.sort();
            out.push(format!("cluster|{}|{}", cl.origin_net(), members.join(" ")));
        }
    }

    // ratsnest per net: nodes and edges (endpoint pairs normalised) with weights
    for net in 1..conn.net_count() {
        let list = conn.algo().item_list();
        let Some(rn) = conn.ratsnest_for_net(net) else {
            continue;
        };

        if rn.node_count() == 0 {
            continue;
        }

        let mut nodes: Vec<String> = rn
            .nodes()
            .map(|r| anchor_text(&ctx, list, kicad_connectivity::edge::anchor_of(list, r)))
            .collect();
        nodes.sort();
        out.push(format!("rn_nodes|{net}|{}", nodes.join(" ")));

        let mut edges: Vec<String> = rn
            .unsorted_edges()
            .iter()
            .map(|e| {
                let a = e
                    .source_anchor(list)
                    .map_or("none".into(), |a| flagged_end(list, a));
                let b = e
                    .target_anchor(list)
                    .map_or("none".into(), |a| flagged_end(list, a));
                let (a, b) = if a <= b { (a, b) } else { (b, a) };
                let tied = if e.is_tied() { "!" } else { "" };
                format!("{a}>{b}:{}{tied}", e.weight())
            })
            .collect();
        edges.sort();
        out.push(format!("rn_edges|{net}|{}", edges.join(" ")));
    }

    out.push(format!(
        "unconnected|{}|{}",
        conn.unconnected_count(false),
        conn.unconnected_count(true)
    ));

    // dangling ends and flashing
    let cu = Lset::all_cu_mask_n(ctx.copper_layer_count()).cu_stack();

    for (id, t) in board.tracks() {
        let item: ItemId = id.into();
        let (dangling, pos) = conn.test_track_endpoint_dangling(&ctx, item, false, None);
        let pos = pos.map_or("unset".into(), |p| format!("{},{}", p.x, p.y));
        out.push(format!(
            "dangling|{}|{}|{pos}",
            uuid_of(&ctx, item),
            dangling as i32
        ));

        if matches!(t, kicad_pcb::pcb_track::Track::Via(_)) {
            let it = ctx.item(item).unwrap();
            for &l in cu.iter().filter(|&&l| it.is_on_layer(l)) {
                let on = conn.is_connected_on_layer(&ctx, it, i32::from(l), &NON_ZONE);
                out.push(format!(
                    "conn_on_layer|{}|{}|{}",
                    uuid_of(&ctx, item),
                    i32::from(l),
                    on as i32
                ));
            }
        }
    }

    for &pad in &pads {
        let it = ctx.item(pad).unwrap();
        for &l in cu.iter().filter(|&&l| it.is_on_layer(l)) {
            let on = conn.is_connected_on_layer(&ctx, it, i32::from(l), &NON_ZONE);
            out.push(format!(
                "conn_on_layer|{}|{}|{}",
                uuid_of(&ctx, pad),
                i32::from(l),
                on as i32
            ));
        }
    }

    out.sort();
    out
}

/// The ends of an edge as candidate sets: the own position, or for an end with a tie record
/// (`CnEdgeTie`, review-connectivity.md F-1) the sorted candidate positions in braces.
pub fn tie_ends(e: &kicad_connectivity::edge::CnEdge, list: &CnList) -> (String, String) {
    let own = |a: Option<&kicad_connectivity::items::CnAnchor>| a.map_or("none".into(), end_text);
    let set = |c: &[kicad_connectivity::edge::CnEdgeNode], own: String| {
        if c.is_empty() {
            return own;
        }
        let mut p: Vec<String> = c.iter().map(|n| end_text(n.resolve(list))).collect();
        p.sort();
        p.dedup();
        format!("{{{}}}", p.join(";"))
    };
    let a = own(e.source_anchor(list));
    let b = own(e.target_anchor(list));

    match e.tie() {
        Some(t) => (set(&t.source_candidates, a), set(&t.target_candidates, b)),
        None => (a, b),
    }
}

/// Port-only facts (no reference counterpart in the driver): the isolated-island map of every
/// copper zone. Mutates `conn`.
pub fn port_only_lines(board: &Board, conn: &mut ConnectivityData) -> Vec<String> {
    let ctx = BoardCtx::bare(board);
    let mut out = Vec::new();

    let mut map: kicad_connectivity::algo::IsolatedIslandsMap = BTreeMap::new();
    for (zid, z) in board.zones() {
        if z.is_rule_area() {
            continue;
        }
        let layers = z.layer_set(&ctx) & Lset::all_cu_mask();
        let e = map.entry(zid).or_default();
        for l in layers.seq() {
            e.insert(l, Default::default());
        }
    }
    conn.fill_isolated_islands_map(&ctx, &mut map, false);
    for (zid, layers) in &map {
        for (l, isl) in layers {
            let mut iso = isl.isolated_outlines.clone();
            iso.sort();
            let mut single = isl.single_connection_outlines.clone();
            single.sort();
            out.push(format!(
                "island|{}/{}|{:?}|{:?}",
                uuid_of(&ctx, (*zid).into()),
                i32::from(*l),
                iso,
                single
            ));
        }
    }

    out
}

/// The first differences of two sorted line sets (`-` only in `a`, `+` only in `b`).
pub fn diff(a: &[String], b: &[String], max: usize) -> Vec<String> {
    let sa: BTreeSet<&String> = a.iter().collect();
    let sb: BTreeSet<&String> = b.iter().collect();
    let mut d: Vec<String> = sa.difference(&sb).map(|l| format!("- {l}")).collect();
    d.extend(sb.difference(&sa).map(|l| format!("+ {l}")));
    if a.len() != b.len() && d.is_empty() {
        d.push(format!("multiplicity differs: {} vs {}", a.len(), b.len()));
    }
    d.truncate(max);
    d
}

/// 64-bit FNV-1a.
pub fn fnv(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// Per-section hashes of fact lines: `section -> fnv( lines joined by '\n' )`.
pub fn section_hashes(lines: &[String]) -> BTreeMap<String, String> {
    let mut by: BTreeMap<String, String> = BTreeMap::new();
    for l in lines {
        let sec = l.split('|').next().unwrap_or("").to_string();
        let e = by.entry(sec).or_default();
        e.push_str(l);
        e.push('\n');
    }
    by.into_iter()
        .map(|(k, v)| (k, format!("{:016x}", fnv(&v))))
        .collect()
}

/// The golden directory of the reference replay.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/live1")
}

/// The file stem of a board path.
pub fn stem(rel: &str) -> String {
    Path::new(rel)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// The script of the reference replay of a board: `batches` seeded batches, each followed by
/// a `C` (commit) line.
pub fn script(s: &Session, seed: u64, batches: usize) -> String {
    let mut g = Gen::new(s, seed);
    let mut out = String::new();
    for _ in 0..batches {
        for op in g.batch() {
            out.push_str(&op.line());
            out.push('\n');
        }
        out.push_str("C\n");
    }
    out
}

/// Parses one script line back into an [`Op`] (`None` for `C`).
pub fn parse_op(line: &str) -> Option<Op> {
    let f: Vec<&str> = line.split_whitespace().collect();
    let n = |i: usize| f[i].parse::<i32>().unwrap();
    let u = || f[1].to_string();
    Some(match f[0] {
        "R" => Op::Remove(u()),
        "T" => Op::Track {
            uuid: u(),
            s: [n(2), n(3)],
            e: [n(4), n(5)],
            w: n(6),
            layer: n(7),
            net: n(8),
        },
        "V" => Op::Via {
            uuid: u(),
            at: [n(2), n(3)],
            d: n(4),
            drill: n(5),
            net: n(6),
            free: n(7) != 0,
        },
        "M" => Op::Move {
            uuid: u(),
            at: [n(2), n(3)],
        },
        "N" => Op::Net {
            uuid: u(),
            net: n(2),
        },
        _ => return None,
    })
}
