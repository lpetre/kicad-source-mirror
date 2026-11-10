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

//! Port of `qa/tests/pcbnew/drc/test_drc_prepared_candidates.cpp` (DRC_RTREE prepared-copper
//! queries, the padding credit, DRC_ZONE_INDEX and CollectOverlappingPairs).
//!
//! The C++ fixture loads QA boards with footprints and zones and runs the whole DRC to get
//! `m_DRCMaxClearance` and the board caches; neither the engine (DRC-03 / DRC-05) nor footprint
//! and zone loading exist yet. The test bodies are therefore run on the synthetic track / arc /
//! via boards of `tests/data/rtree` with a fixed maximum clearance, and the zone tests on
//! synthetic boxes; the assertions are the C++ ones. `DRCGraphicBoundsDoNotContainTheirEffectiveShape`
//! runs on the C++ board (`issue22102`) and reports itself as pending until the board model
//! loads it.

use std::path::Path;

use kicad_core::layer_ids::{Flashing, PcbLayerId, is_copper_layer};
use kicad_core::lset::Lset;
use kicad_drc::constraint_types::DrcConstraintT;
use kicad_drc::rtree::DrcRtree;
use kicad_drc::zone_index::{DrcZoneIndex, collect_overlapping_pairs};
use kicad_geom::math::box2::Box2I;
use kicad_geom::math::vector2::{Vector2I, Vector2L};
use kicad_pcb::io::{ParseOptions, parse_board};
use kicad_pcb::zone::Zone;
use kicad_pcb::{AddMode, AnyItem, Board, BoardCtx, ItemId, ItemRef};

/// Stand-in for `m_DRCMaxClearance` of the C++ fixture.
const MAX_CLEARANCE: i32 = 200000;

const BOARDS: [&str; 3] = [
    "rtree_small.kicad_pcb",
    "rtree_ties.kicad_pcb",
    "rtree_4layer.kicad_pcb",
];

fn load(name: &str) -> Board {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/rtree")
        .join(name);
    let text = std::fs::read_to_string(path).unwrap();
    parse_board(&text, name, &ParseOptions::default()).unwrap()
}

/// `insertCopperItems`: the board's tracks (and pads) on one layer, one padding.
fn insert_copper_items(
    tree: &mut DrcRtree,
    ctx: &BoardCtx<'_>,
    layer: PcbLayerId,
    padding: i32,
    except: Option<ItemId>,
    except_padding: i32,
) {
    for (_, track) in ctx.board.tracks() {
        let item = ctx.view(AnyItem::Track(track));

        if !item.is_on_layer(layer) {
            continue;
        }

        let p = if item.id() == except {
            except_padding
        } else {
            padding
        };
        tree.insert(item, layer, DrcConstraintT::ClearanceConstraint, p, false);
    }

    tree.build();
}

/// The cache generator's copper tree (`m_CopperItemRTreeCache`) of a track-only board.
fn copper_tree(ctx: &BoardCtx<'_>) -> DrcRtree {
    let copper = Lset::all_cu_mask_n(ctx.board.copper_layer_count());
    let mut tree = DrcRtree::new();

    for (_, track) in ctx.board.tracks() {
        let item = ctx.view(AnyItem::Track(track));
        (&item.layer_set() & &copper).run_on_layers(|layer| {
            tree.insert(
                item,
                layer,
                DrcConstraintT::ClearanceConstraint,
                MAX_CLEARANCE,
                false,
            );
        });
    }

    tree.build();
    tree
}

/// `copperReferences`: the first `limit` tracks (the boards have no pads).
fn copper_references<'a>(ctx: &'a BoardCtx<'a>, limit: usize) -> Vec<ItemRef<'a>> {
    ctx.board
        .tracks()
        .take(limit)
        .map(|(_, t)| ctx.view(AnyItem::Track(t)))
        .collect()
}

fn uuid_of(ctx: &BoardCtx<'_>, id: ItemId) -> String {
    ctx.item(id).unwrap().uuid().as_string()
}

fn copper_layers(item: &ItemRef<'_>, ctx: &BoardCtx<'_>) -> Vec<PcbLayerId> {
    let mut out = Vec::new();
    (&item.layer_set() & &Lset::all_cu_mask_n(ctx.board.copper_layer_count()))
        .run_on_layers(|l| out.push(l));
    out
}

/// `legacyCandidates`
fn legacy_candidates(
    tree: &DrcRtree,
    ctx: &BoardCtx<'_>,
    r: ItemRef<'_>,
    layer: PcbLayerId,
    clearance: i32,
    filter_log: Option<&mut Vec<String>>,
    reject: &str,
) -> Vec<String> {
    let mut hits = Vec::new();
    let mut log = filter_log;

    tree.query_colliding(
        r,
        layer,
        layer,
        Some(&mut |id: ItemId| {
            let uuid = uuid_of(ctx, id);

            if let Some(log) = log.as_mut() {
                log.push(uuid.clone());
            }

            uuid != reject
        }),
        Some(&mut |id: ItemId| {
            hits.push(uuid_of(ctx, id));
            true
        }),
        clearance,
    );

    hits
}

/// `preparedCandidates`
#[allow(clippy::too_many_arguments)]
fn prepared_candidates(
    tree: &DrcRtree,
    ctx: &BoardCtx<'_>,
    r: ItemRef<'_>,
    layer: PcbLayerId,
    clearance: i32,
    credit: bool,
    filter_log: Option<&mut Vec<String>>,
    reject: &str,
    check_parent_shapes: bool,
) -> Vec<String> {
    let ref_shape = r.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
    let mut hits = Vec::new();
    let mut log = filter_log;

    tree.query_colliding_prepared_copper(
        r,
        &ref_shape,
        layer,
        Some(&mut |id: ItemId| {
            let uuid = uuid_of(ctx, id);

            if let Some(log) = log.as_mut() {
                log.push(uuid.clone());
            }

            uuid != reject
        }),
        Some(
            &mut |id: ItemId, parent_shape: &kicad_pcb::item::SharedShape| {
                if check_parent_shapes {
                    // The substituted parent has to be what the provider used to build itself,
                    // which is the plain layer shape, not just something with the same bounds
                    let expected = ctx.item(id).unwrap().effective_shape(
                        layer,
                        Flashing::Default,
                        DrcConstraintT::NullConstraint,
                    );
                    assert_eq!(parent_shape.format(false), expected.format(false));
                }

                hits.push(uuid_of(ctx, id));
                true
            },
        ),
        clearance,
        credit,
    );

    hits
}

/// `isSubsequence`
fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut pos = 0;

    for entry in needle {
        while pos < haystack.len() && haystack[pos] != *entry {
            pos += 1;
        }

        if pos == haystack.len() {
            return false;
        }

        pos += 1;
    }

    true
}

#[test]
fn drc_prepared_copper_query_matches_legacy() {
    let mut total_hits = 0;
    let mut widest_query = 0;
    let mut negative_clearance_queries = 0;

    for name in BOARDS {
        let board = load(name);
        let ctx = BoardCtx::bare(&board);
        let tree = copper_tree(&ctx);
        let clearances = [
            -MAX_CLEARANCE,
            0,
            MAX_CLEARANCE / 2,
            MAX_CLEARANCE,
            MAX_CLEARANCE + 1,
        ];

        for r in copper_references(&ctx, 40) {
            for layer in copper_layers(&r, &ctx) {
                for clearance in clearances {
                    let legacy = legacy_candidates(&tree, &ctx, r, layer, clearance, None, "");
                    let prepared =
                        prepared_candidates(&tree, &ctx, r, layer, clearance, true, None, "", true);

                    assert_eq!(legacy, prepared);

                    total_hits += legacy.len();
                    widest_query = widest_query.max(legacy.len());

                    if clearance < 0 {
                        negative_clearance_queries += 1;
                    }
                }
            }
        }
    }

    // Several references have to actually collide, and at least one with more than one
    // candidate, or the collection comparisons above are vacuous
    assert!(total_hits > 0);
    assert!(widest_query > 1);
    assert!(negative_clearance_queries > 0);
}

#[test]
fn drc_prepared_copper_filter_invocation_parity() {
    let board = load("rtree_4layer.kicad_pcb");
    let ctx = BoardCtx::bare(&board);
    let tree = copper_tree(&ctx);

    let mut plain_invocations = 0;
    let mut credit_invocations = 0;
    let mut rejections = 0;

    for r in copper_references(&ctx, 40) {
        for layer in copper_layers(&r, &ctx) {
            // Reject whichever candidate the legacy query sees first, so the memoized filter
            // result is exercised on a parent that really is in the tree
            let mut probe = Vec::new();
            legacy_candidates(&tree, &ctx, r, layer, MAX_CLEARANCE, Some(&mut probe), "");

            let Some(reject) = probe.first().cloned() else {
                continue;
            };

            rejections += 1;

            let mut legacy_log = Vec::new();
            let mut plain_log = Vec::new();
            let mut credit_log = Vec::new();

            let legacy = legacy_candidates(
                &tree,
                &ctx,
                r,
                layer,
                MAX_CLEARANCE,
                Some(&mut legacy_log),
                &reject,
            );
            let plain = prepared_candidates(
                &tree,
                &ctx,
                r,
                layer,
                MAX_CLEARANCE,
                false,
                Some(&mut plain_log),
                &reject,
                false,
            );
            let credit = prepared_candidates(
                &tree,
                &ctx,
                r,
                layer,
                MAX_CLEARANCE,
                true,
                Some(&mut credit_log),
                &reject,
                false,
            );

            // Without credit the broad phase is the legacy one, so the filter has to be asked
            // the same questions in the same order
            assert_eq!(legacy_log, plain_log);
            assert_eq!(legacy, plain);

            // Credit only shrinks the query box, so it asks a subsequence of the same
            // questions and still finds every collision
            assert!(is_subsequence(&credit_log, &plain_log));
            assert_eq!(legacy, credit);

            // Memoization: a parent is never handed to the filter twice
            let mut sorted_credit = credit_log.clone();
            sorted_credit.sort();
            assert!(sorted_credit.windows(2).all(|w| w[0] != w[1]));

            plain_invocations += plain_log.len();
            credit_invocations += credit_log.len();
        }
    }

    assert!(rejections > 0);

    // Pruning pure filter invocations is the whole point of the credit, so an
    // equal-call-count requirement would be wrong; what has to hold is that it never asks more
    assert!(credit_invocations < plain_invocations);
}

#[test]
fn drc_prepared_copper_credit_uses_minimum_padding() {
    // The falsifying case needs two real copper items whose bounding boxes are disjoint but
    // which still collide within the board's worst clearance.
    let layer = PcbLayerId::FCu;
    let mut found = None;

    for name in BOARDS {
        let board = load(name);
        let ctx = BoardCtx::bare(&board);
        let mut uniform = DrcRtree::new();
        insert_copper_items(&mut uniform, &ctx, layer, MAX_CLEARANCE, None, 0);

        let items = copper_references(&ctx, 100000);

        'outer: for r in &items {
            if !r.is_on_layer(layer) {
                continue;
            }

            let reachable = legacy_candidates(&uniform, &ctx, *r, layer, MAX_CLEARANCE, None, "");

            for candidate in &items {
                if candidate == r || !candidate.is_on_layer(layer) {
                    continue;
                }

                if r.bounding_box().intersects(&candidate.bounding_box()) {
                    continue;
                }

                if !reachable.contains(&candidate.uuid().as_string()) {
                    continue;
                }

                found = Some((name, r.id().unwrap(), candidate.id().unwrap()));
                break 'outer;
            }
        }

        if found.is_some() {
            break;
        }
    }

    let (name, reference, far_candidate) = found.expect("a qualifying pair");
    let board = load(name);
    let ctx = BoardCtx::bare(&board);
    let r = ctx.item(reference).unwrap();

    // One item inserted with no padding drops the layer minimum to zero, which has to cost the
    // query its whole credit
    let mut mixed = DrcRtree::new();
    insert_copper_items(
        &mut mixed,
        &ctx,
        layer,
        MAX_CLEARANCE,
        Some(far_candidate),
        0,
    );

    let expected = legacy_candidates(&mixed, &ctx, r, layer, MAX_CLEARANCE, None, "");
    let credit = prepared_candidates(&mixed, &ctx, r, layer, MAX_CLEARANCE, true, None, "", false);

    assert_eq!(expected, credit);
    assert!(credit.contains(&uuid_of(&ctx, far_candidate)));

    // clear() drops the recorded padding with the entries; rebuilding on another layer must
    // not inherit either
    mixed.clear();
    assert!(
        prepared_candidates(&mixed, &ctx, r, layer, MAX_CLEARANCE, true, None, "", false)
            .is_empty()
    );

    insert_copper_items(
        &mut mixed,
        &ctx,
        PcbLayerId::BCu,
        MAX_CLEARANCE / 2,
        None,
        0,
    );

    for r in copper_references(&ctx, 40) {
        if !r.is_on_layer(PcbLayerId::BCu) {
            continue;
        }

        let legacy = legacy_candidates(&mixed, &ctx, r, PcbLayerId::BCu, MAX_CLEARANCE, None, "");
        let rebuilt = prepared_candidates(
            &mixed,
            &ctx,
            r,
            PcbLayerId::BCu,
            MAX_CLEARANCE,
            true,
            None,
            "",
            false,
        );

        assert_eq!(legacy, rebuilt);
    }
}

#[test]
fn drc_credit_call_sites_satisfy_containment() {
    // Tracks and pads are the only two call sites that opt into the padding credit, and the
    // credit is only sound where the item's bounding box contains the shape the query collides
    // against. (Pads: pending until the pad shapes are ported.)
    let mut examined = 0;

    for name in BOARDS {
        let board = load(name);
        let ctx = BoardCtx::bare(&board);

        for r in copper_references(&ctx, usize::MAX) {
            for layer in copper_layers(&r, &ctx) {
                let shape =
                    r.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
                examined += 1;
                assert!(
                    r.bounding_box().contains_box(&shape.bbox(0)),
                    "{name} {} does not contain its own shape",
                    r.uuid().as_string()
                );
            }
        }
    }

    // A clean verdict over zero objects is not a clean verdict
    assert!(examined > 0);
}

/// Pseudo-random boxes for the zone tests (the C++ uses the zones of fill_bad,
/// intersectingzones and issue5750).
fn boxes(seed: u32, n: usize) -> Vec<Box2I> {
    let mut state = seed.wrapping_mul(2654435761);
    let mut next = |m: i32| -> i32 {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        ((state >> 8) % m as u32) as i32
    };

    (0..n)
        .map(|_| {
            let (x, y) = (next(2000) - 500, next(2000) - 500);
            let (w, h) = (next(600) - 100, next(600) - 100);
            Box2I::new(Vector2I::new(x, y), Vector2L::new(w as i64, h as i64))
        })
        .collect()
}

#[test]
fn drc_zone_index_matches_ordered_linear_scan() {
    let mut exercised = 0;
    let mut multi_candidate_queries = 0;

    for seed in 1..4u32 {
        let mut board = Board::new();
        let zone_boxes = boxes(seed, 30);
        let ids: Vec<_> = zone_boxes
            .iter()
            .map(|_| board.add_zone(Zone::default(), AddMode::Append))
            .collect();
        let box_of = |id| zone_boxes[ids.iter().position(|&z| z == id).unwrap()];

        // Two layers: all zones on F.Cu, every other one on B.Cu
        let mut by_layer = std::collections::BTreeMap::new();
        by_layer.insert(PcbLayerId::FCu, ids.clone());
        by_layer.insert(
            PcbLayerId::BCu,
            ids.iter().step_by(2).copied().collect::<Vec<_>>(),
        );

        let mut index = DrcZoneIndex::new();
        index.build(&by_layer, box_of);

        for (layer, zones) in &by_layer {
            for &zone in zones {
                let mut query = box_of(zone);
                query.inflate(30);
                query.normalize();

                let expected: Vec<_> = zones
                    .iter()
                    .copied()
                    .filter(|&c| query.intersects(&box_of(c)))
                    .collect();
                let actual = index.query(*layer, &query);

                assert_eq!(expected, actual);
                exercised += 1;

                if actual.len() > 1 {
                    multi_candidate_queries += 1;
                }
            }
        }

        // A layer the board has no copper zones on must simply come back empty
        assert!(
            index
                .query(PcbLayerId::UndefinedLayer, &Box2I::default())
                .is_empty()
        );
        assert!(!index.has_layer(PcbLayerId::In1Cu));

        index.clear();
        assert!(index.query(PcbLayerId::FCu, &Box2I::default()).is_empty());
    }

    assert!(exercised > 0);
    assert!(multi_candidate_queries > 0);
}

#[test]
fn drc_zone_pair_enumeration_matches_nested_loop() {
    let mut total_pairs = 0;
    let mut empty_layers = 0;

    for seed in 1..6u32 {
        let fills = boxes(seed, 8 + 9 * seed as usize);
        // Every fifth zone has no fill on this layer; indices are a strided subset like the
        // provider's per-layer index lists
        let indices: Vec<usize> = (0..fills.len()).filter(|i| i % 4 != 1).collect();
        let fill_box = |i: usize| (i % 5 != 2).then_some(fills[i]);

        let mut expected = Vec::new();

        for ia in 0..indices.len() {
            let Some(a) = fill_box(indices[ia]) else {
                continue;
            };

            for &ib in &indices[ia + 1..] {
                match fill_box(ib) {
                    Some(b) if a.intersects(&b) => expected.push((indices[ia], ib)),
                    _ => {}
                }
            }
        }

        let pairs = collect_overlapping_pairs(&indices, fill_box);
        assert_eq!(expected, pairs, "seed {seed}");

        total_pairs += pairs.len();

        if pairs.is_empty() {
            empty_layers += 1;
        }

        for (a, b) in pairs {
            assert!(indices.contains(&a) && indices.contains(&b));
            assert!(fill_box(a).is_some() && fill_box(b).is_some());
        }
    }

    // A layer with no overlapping fills
    let pairs = collect_overlapping_pairs(&[0, 1], |i| {
        Some(Box2I::new(
            Vector2I::new(i as i32 * 1000, 0),
            Vector2L::new(10, 10),
        ))
    });

    if pairs.is_empty() {
        empty_layers += 1;
    }

    assert!(total_pairs > 0);
    assert!(empty_layers > 0);
}

/// `DRCGraphicBoundsDoNotContainTheirEffectiveShape`: padding credit is only sound where the
/// item's bounding box contains the shape the query will collide against. Copper graphics do
/// not satisfy that, which is why testGraphicClearances() opts out. Red if the two bounding
/// boxes are ever reconciled, at which point the credit could be turned back on there.
///
/// `issue22102` has footprints; while the board model cannot load it (or shape its graphics)
/// the test reports itself as pending and passes.
#[test]
fn drc_graphic_bounds_do_not_contain_their_effective_shape() {
    let name = "issue22102.kicad_pcb";
    let text = std::fs::read_to_string(kicad_qa::pcbnew_data_dir().join(name)).unwrap();

    let result = std::panic::catch_unwind(|| {
        let board = match parse_board(&text, name, &ParseOptions::default()) {
            Ok(board) => board,
            Err(e) => return Err(format!("{e}")),
        };
        let ctx = BoardCtx::bare(&board);
        let mut copper_graphics = 0usize;
        let mut uncontained = 0usize;

        for (_, drawing) in board.drawings() {
            let item = ctx.view(drawing);
            let layer = item.layer();

            if !is_copper_layer(layer) {
                continue;
            }

            let shape =
                item.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

            copper_graphics += 1;

            if !item.bounding_box().contains_box(&shape.bbox(0)) {
                uncontained += 1;
            }
        }

        Ok((copper_graphics, uncontained))
    });

    let pending = |msg: &str| {
        assert!(msg.contains("port pending"), "{name}: {msg}");
        eprintln!("{name}: pending ({msg})");
    };

    match result {
        Ok(Ok((copper_graphics, uncontained))) => {
            assert!(copper_graphics > 0);
            assert!(uncontained > 0);
        }
        Ok(Err(msg)) => pending(&msg),
        Err(e) => {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default();
            pending(&msg);
        }
    }
}
