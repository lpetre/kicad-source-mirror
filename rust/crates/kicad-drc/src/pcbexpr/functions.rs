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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `pcbnew/pcbexpr_functions.cpp`: the built-in functions of the PCB expression
//! language (`existsOnLayer`, `intersectsCourtyard`, `intersectsArea`, `enclosedByArea`,
//! `memberOfGroup`, `isMicroVia`, `inDiffPair`, `getField`, `hasNetclass`, ...) and their
//! registration (`PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions`).
//!
//! Each C++ function `void f( LIBEVAL::CONTEXT* aCtx, void* self )` becomes a closure
//! registered with [`FuncCallRef::new`]: `self` is the receiver variable (`A`, `B`, ...), the
//! arguments are popped from the context's stack, the result value is pushed, and the
//! expensive part is a deferred evaluation (`VALUE::SetDeferredEval`) that only runs when the
//! value is read.
//!
//! # Board-level run-time caches
//!
//! The C++ functions memoise in `BOARD` members (`m_IntersectsCourtyardCache`, the
//! `m_*ResultCache`s, `m_ItemFieldCache`, `m_LayerExpressionCache`, `m_ZonesByNameCache`,
//! `m_DeflatedZoneOutlineCache`, `m_StackedMicroviaCache`), all cleared by
//! `BOARD::IncrementTimeStamp()`. Some of them are semantically visible: the courtyard memo
//! `{ footprint, item }` omits the layer the item shape was taken on, the per-area memo
//! `{ area, item, layer }` omits the constraint (silk clearance widens the area's layer set),
//! and a memo hit does not repeat the error reports of the evaluation it replaces. The Rust
//! `Board` is borrowed immutably during rule evaluation and has no slot for them, so they live
//! in a [`PcbExprBoardCaches`] value that the caller installs for one board with
//! [`PcbExprCacheScope::enter`] (for the duration of a DRC run, a dump, a test). The caches
//! are dropped whenever the board's time stamp moved (= `IncrementTimeStamp`). Without an
//! installed scope every evaluation computes afresh (see `PORT_STATUS.md`, DRC-22).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use kicad_core::layer_ids::{is_back_layer, is_front_layer};
use kicad_core::lset::Lset;
use kicad_core::string_utils::wx_string_matches;
use kicad_core::{KicadT, Kiid, PcbLayerId};
use kicad_geom::geometry::approximation::ErrorLoc;
use kicad_geom::geometry::corner_strategy::CornerStrategy;
use kicad_geom::geometry::geometry_utils::ARC_LOW_DEF;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::geometry::shape_poly_set::ShapePolySet;
use kicad_libeval::{Context, FuncCallRef, ValueId};
use kicad_pcb::board::Board;
use kicad_pcb::ids::TrackId;
use kicad_pcb::item::{ItemFlags, SharedShape};
use kicad_pcb::padstack::PadAttrib;
use kicad_pcb::pcb_track::{PcbVia, Viatype};
use kicad_pcb::{ItemId, ItemRef};

use super::evaluator::{PcbExprBinder, PcbExprBuiltinFunctions, PcbExprUcode, PcbExprVarRef};
use crate::caches::{DrcPredicateCaches, ItemSelectorLayerCacheKey, MemoCache};
use crate::constraint_types::DrcConstraintT;
use crate::engine::diff_pair::match_dp_suffix;
use crate::rtree::DrcRtree;
use crate::rule_parser::{LayerChoices, board_layer_choices, default_layer_choices};

/// The evaluation context of the PCB binding.
type PcbContext<'e> = Context<'e, PcbExprUcode>;

/// `UNDEFINED_LAYER`.
const UNDEFINED_LAYER: PcbLayerId = PcbLayerId::UndefinedLayer;

/*
 * Board-level caches
 */

/// The run-time caches of `BOARD` that the expression functions read and write (`board.h`,
/// "Run-time caches"): the predicate memos of [`DrcPredicateCaches`] plus
/// `m_LayerExpressionCache`, `m_ZonesByNameCache`, `m_DeflatedZoneOutlineCache` and
/// `m_StackedMicroviaCache`. Keys use item handles instead of pointers.
pub struct PcbExprBoardCaches {
    /// Address of the board the caches belong to (identity only, never dereferenced).
    board: usize,
    /// `BOARD::GetTimeStamp()` the caches were filled at.
    stamp: Cell<i32>,
    /// `m_Intersects*Cache`, `m_EnclosedByAreaCache`, the `m_*ResultCache`s, `m_ItemFieldCache`.
    pub predicates: DrcPredicateCaches,
    /// `m_LayerExpressionCache`.
    layer_expressions: RefCell<BTreeMap<String, Lset>>,
    /// `m_ZonesByNameCache`.
    zones_by_name: RefCell<BTreeMap<String, Vec<ItemId>>>,
    /// `m_DeflatedZoneOutlineCache`.
    deflated_zone_outlines: RefCell<BTreeMap<ItemId, ShapePolySet>>,
    /// `m_StackedMicroviaCache` (`std::optional<std::set<const PCB_VIA*>>`).
    stacked_microvias: RefCell<Option<BTreeSet<TrackId>>>,
}

impl PcbExprBoardCaches {
    fn new(board: &Board) -> Self {
        PcbExprBoardCaches {
            board: board_address(board),
            stamp: Cell::new(board.time_stamp()),
            predicates: DrcPredicateCaches::new(),
            layer_expressions: RefCell::new(BTreeMap::new()),
            zones_by_name: RefCell::new(BTreeMap::new()),
            deflated_zone_outlines: RefCell::new(BTreeMap::new()),
            stacked_microvias: RefCell::new(None),
        }
    }

    /// The cache part of `BOARD::IncrementTimeStamp()`.
    fn clear(&self) {
        self.predicates.clear();
        self.layer_expressions.borrow_mut().clear();
        self.zones_by_name.borrow_mut().clear();
        self.deflated_zone_outlines.borrow_mut().clear();
        *self.stacked_microvias.borrow_mut() = None;
    }
}

fn board_address(board: &Board) -> usize {
    std::ptr::from_ref(board) as usize
}

thread_local! {
    /// The caches installed by the innermost [`PcbExprCacheScope`] of this thread.
    static ACTIVE_CACHES: RefCell<Option<Rc<PcbExprBoardCaches>>> = const { RefCell::new(None) };
}

/// Installs fresh [`PcbExprBoardCaches`] for one board on the current thread until it is
/// dropped (then the previously installed caches, if any, are active again). Evaluations of
/// items of other boards stay uncached.
pub struct PcbExprCacheScope {
    previous: Option<Rc<PcbExprBoardCaches>>,
}

impl PcbExprCacheScope {
    /// Install empty caches for `board` (the state after `BOARD::IncrementTimeStamp()`).
    pub fn enter(board: &Board) -> Self {
        let caches = Rc::new(PcbExprBoardCaches::new(board));
        let previous = ACTIVE_CACHES.with(|active| active.borrow_mut().replace(caches));

        PcbExprCacheScope { previous }
    }
}

impl Drop for PcbExprCacheScope {
    fn drop(&mut self) {
        let previous = self.previous.take();
        ACTIVE_CACHES.with(|active| *active.borrow_mut() = previous);
    }
}

/// The installed caches of `board`, emptied first if the board's time stamp moved since they
/// were filled; `None` without a scope for this board.
fn board_caches(board: &Board) -> Option<Rc<PcbExprBoardCaches>> {
    ACTIVE_CACHES.with(|active| {
        let active = active.borrow();
        let caches = active.as_ref()?;

        if caches.board != board_address(board) {
            return None;
        }

        if caches.stamp.get() != board.time_stamp() {
            caches.clear();
            caches.stamp.set(board.time_stamp());
        }

        Some(Rc::clone(caches))
    })
}

/*
 * Helpers
 */

/// `vref ? vref->GetObject( aCtx ) : nullptr`.
fn receiver<'e>(ctx: &PcbContext<'e>, vref: Option<&PcbExprVarRef>) -> Option<ItemRef<'e>> {
    vref.and_then(|v| v.object(ctx.env()))
}

/// `arg->AsString()` as an owned string.
fn arg_string(ctx: &PcbContext<'_>, arg: ValueId) -> String {
    ctx.value(arg).as_string(ctx).to_string()
}

/// `!arg || arg->AsString().IsEmpty()` (`Pop()` never returns null).
fn arg_is_empty(ctx: &PcbContext<'_>, arg: ValueId) -> bool {
    ctx.value(arg).as_string(ctx).is_empty()
}

/// `result = aCtx->AllocValue(); result->Set( 0.0 ); aCtx->Push( result );`
fn push_zero(ctx: &mut PcbContext<'_>) -> ValueId {
    let result = ctx.alloc_value();
    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);
    result
}

/// `if( aCtx->HasErrorCallback() ) aCtx->ReportError( msg );`
fn report(ctx: &PcbContext<'_>, msg: impl FnOnce() -> String) {
    if ctx.has_error_callback() {
        ctx.report_error(&msg());
    }
}

/// `( item->GetFlags() & ROUTER_TRANSIENT ) != 0`. An item without a handle (a value outside
/// the board, C++ a pointer to a stack object) cannot be a cache key and is treated the same
/// way: never memoised.
fn is_transient(item: &ItemRef<'_>) -> bool {
    item.base().flags.0 & ItemFlags::ROUTER_TRANSIENT != 0 || item.id().is_none()
}

/// `aItem->GetFlags() & HOLE_PROXY` (the disallow provider marks the hole of a pad / via).
fn is_hole_proxy(item: &ItemRef<'_>) -> bool {
    item.is_hole_proxy() || item.base().flags.0 & ItemFlags::HOLE_PROXY != 0
}

/// `dynamic_cast<FOOTPRINT*>`.
fn as_footprint_ref(item: Option<ItemRef<'_>>) -> Option<ItemRef<'_>> {
    item.filter(|i| i.as_footprint().is_some())
}

/// `dynamic_cast<ZONE*>`.
fn as_zone_ref(item: Option<ItemRef<'_>>) -> Option<ItemRef<'_>> {
    item.filter(|i| i.as_zone().is_some())
}

/// `ENUM_MAP<PCB_LAYER_ID>::Instance().Choices()`: canonical and board layer names after the
/// kicad-cli load path (same map as `PCBEXPR_LAYER_VALUE::EqualTo`, see DRC-21).
fn layer_map(ctx: &PcbContext<'_>) -> LayerChoices {
    match ctx.env().board() {
        Some(board) => board_layer_choices(board),
        None => default_layer_choices(),
    }
}

/// `SHAPE_POLY_SET::Collide( const SHAPE* aShape )` with the default arguments.
fn poly_collide(poly: &ShapePolySet, shape: &dyn Shape) -> bool {
    poly.collide_shape(shape, 0, None, None)
}

/// `ZONE::GetBoardOutline()` / `ZONE::Outline()` as the C++ picks them ("GetBoardOutline() is
/// expensive.  Only use it where we have to.").
fn zone_outline_for_collision(zone: &ItemRef<'_>) -> ShapePolySet {
    let z = zone.as_zone().expect("zone item");

    if zone.parent_footprint().is_some() {
        z.board_outline(zone.ctx)
    } else {
        z.outline().clone()
    }
}

/*
 * fromTo
 */

/// Port of `fromToFunc`.
///
/// The from-to cache is reached through the item's context
/// (`ConnectivityQuery::is_on_from_to_path`, `kicad_connectivity::from_to`). A context
/// without connectivity, or a connectivity stand-in without a from-to cache, takes the C++
/// branch for a non-existent cache (result 0), reporting its warning text through the error
/// callback instead of `wxLogWarning`.
fn from_to_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) -> bool {
    let item = receiver(ctx, vref);
    let result = ctx.alloc_value();
    let arg_to = ctx.pop();
    let arg_from = ctx.pop();

    ctx.value_mut(result).set_double(0.0);
    ctx.push(result);

    let Some(item) = item else {
        return false;
    };

    if arg_is_empty(ctx, arg_from) {
        report(ctx, || {
            format!(
                "Missing 'from' pad argument (footprint reference designator followed by \
                 hyphen and pad number) to {}.",
                "fromTo()"
            )
        });

        return false;
    }

    if arg_is_empty(ctx, arg_to) {
        report(ctx, || {
            format!(
                "Missing 'to' pad argument (footprint reference designator followed by hyphen \
                 and pad number) to {}.",
                "fromTo()"
            )
        });

        return false;
    }

    // auto ftCache = item->GetBoard()->GetConnectivity()->GetFromToCache();
    let from = arg_string(ctx, arg_from);
    let to = arg_string(ctx, arg_to);
    let on_path = item
        .ctx
        .connectivity
        .and_then(|conn| conn.is_on_from_to_path(item.ctx, item, &from, &to));

    let Some(on_path) = on_path else {
        // if( !ftCache ) { wxLogWarning( ... ); return true; }
        report(ctx, || {
            "Attempting to call fromTo() with non-existent from-to cache.".to_string()
        });

        return true;
    };

    if on_path {
        ctx.value_mut(result).set_double(1.0);
    }

    true
}

/*
 * existsOnLayer, isPlated
 */

/// Port of `existsOnLayerFunc`.
fn exists_on_layer_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let item = receiver(ctx, vref);
    let arg = ctx.pop();
    let result = push_zero(ctx);

    let Some(item) = item else {
        return;
    };

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!("Missing layer name argument to {}.", "existsOnLayer()")
        });

        return;
    }

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let layer_name = arg_string(ctx, arg);
        let layer_map = layer_map(ctx);

        if ctx.has_error_callback() {
            /*
             * Interpreted version
             */

            let mut any_match = false;

            for (text, layer) in &layer_map {
                if wx_string_matches(text, &layer_name) {
                    any_match = true;

                    if item.is_on_layer(*layer) {
                        return 1.0;
                    }
                }
            }

            if !any_match {
                ctx.report_error(&format!("Unrecognized layer '{layer_name}'"));
            }

            0.0
        } else {
            /*
             * Compiled version
             */

            let caches = board_caches(item.ctx.board);

            if let Some(caches) = &caches {
                if let Some(mask) = caches.layer_expressions.borrow().get(&layer_name) {
                    return if (&item.layer_set() & mask).any() {
                        1.0
                    } else {
                        0.0
                    };
                }
            }

            let mut mask = Lset::new();

            for (text, layer) in &layer_map {
                if wx_string_matches(text, &layer_name) {
                    mask.set(*layer);
                }
            }

            if let Some(caches) = &caches {
                caches
                    .layer_expressions
                    .borrow_mut()
                    .insert(layer_name, mask.clone());
            }

            if (&item.layer_set() & &mask).any() {
                1.0
            } else {
                0.0
            }
        }
    });
}

/// Port of `isPlatedFunc`.
#[allow(clippy::if_same_then_else)] // keeps the C++ statement structure
fn is_plated_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let result = push_zero(ctx);

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    if item
        .as_pad()
        .is_some_and(|pad| pad.attribute() == PadAttrib::Pth)
    {
        ctx.value_mut(result).set_double(1.0);
    } else if item.type_() == KicadT::PcbViaT {
        ctx.value_mut(result).set_double(1.0);
    }
}

/*
 * Courtyards
 */

/// Port of `collidesWithCourtyard`.
///
/// `item_shape` is the C++ `std::shared_ptr<SHAPE>& aItemShape`, which no caller ever
/// assigns (kept for fidelity).
fn collides_with_courtyard(
    item: &ItemRef<'_>,
    item_shape: &Option<SharedShape>,
    ctx: &PcbContext<'_>,
    footprint: &ItemRef<'_>,
    side: PcbLayerId,
) -> bool {
    let fp = footprint.as_footprint().expect("footprint item");
    let footprint_courtyard = fp.courtyard(footprint.ctx, side);

    if footprint_courtyard.outline_count() == 0 {
        return false;
    }

    // Broad phase before the polygon-level Collide, which dominates when a rule tests a
    // courtyard against many items (intersectsCourtyard('*') over a full board). A bbox miss
    // cannot collide, so the expensive shape build and Collide are skipped.
    if !Shape::bbox(&footprint_courtyard, 0).intersects(&item.bounding_box()) {
        return false;
    }

    if let Some(shape) = item_shape {
        poly_collide(&footprint_courtyard, shape.as_ref())
    } else if item.as_zone().is_some() {
        // Since rules are used for zone filling we can't rely on the filled shapes.  Use the
        // zone  outline instead.
        let zone_outline = zone_outline_for_collision(item);
        poly_collide(&footprint_courtyard, &zone_outline)
    } else {
        let shape = item.effective_shape(
            ctx.env().layer(),
            Default::default(),
            DrcConstraintT::NullConstraint,
        );
        poly_collide(&footprint_courtyard, shape.as_ref())
    }
}

/// Port of the static `testFootprintSelector`.
#[allow(clippy::if_same_then_else)] // keeps the C++ statement structure
fn test_footprint_selector(footprint: &ItemRef<'_>, selector: &str) -> bool {
    // NOTE: This code may want to be somewhat more generalized, but for now it's implemented
    // here to support functions like insersectsCourtyard where we want multiple ways to search
    // for the footprints in question.
    // If support for text variable replacement is added, it should happen before any other
    // logic here, so that people can use text variables to contain references or LIBIDs.
    // (see: https://gitlab.com/kicad/code/kicad/-/issues/11231)

    if selector.is_empty() {
        return false;
    }

    let fp = footprint.as_footprint().expect("footprint item");

    // First check if we have a known directive
    if selector.starts_with('$')
        && selector.ends_with('}')
        && kicad_core::wxstr::upper(selector).starts_with("${CLASS:")
    {
        // aSelector.Mid( 8, aSelector.Length() - 9 )
        let len = selector.chars().count();
        let name: String = selector
            .chars()
            .skip(8)
            .take(len.saturating_sub(9))
            .collect();

        if let Some(comp_class) = fp.component_class(footprint.ctx) {
            if comp_class.contains_class_name(&name) {
                return true;
            }
        }
    } else if wx_string_matches(&fp.reference(footprint.ctx), selector) {
        return true;
    } else if selector.contains(':') && wx_string_matches(&fp.fpid_as_string(), selector) {
        return true;
    }

    false
}

/// Port of the static `searchFootprintsNearItem`: "A"/"B" resolve to the items under test;
/// any other selector is matched against the footprints whose courtyard can actually reach
/// `item`, found via the spatial index rather than a full-board scan.  A footprint the index
/// skips would fail the same bbox test `collidesWithCourtyard()` applies, so the result
/// matches a linear scan.
fn search_footprints_near_item<'e>(
    arg: &str,
    ctx: &PcbContext<'e>,
    item: &ItemRef<'e>,
    func: &mut dyn FnMut(&ItemRef<'e>) -> bool,
) -> bool {
    if arg == "A" {
        return as_footprint_ref(ctx.env().item(0)).is_some_and(|fp| func(&fp));
    } else if arg == "B" {
        return as_footprint_ref(ctx.env().item(1)).is_some_and(|fp| func(&fp));
    }

    let mut found = false;
    let index = item.ctx.board.footprint_courtyard_index(item.ctx);

    index.query_overlapping(&item.bounding_box(), &mut |fp_id| {
        let Some(fp) = item.ctx.item(fp_id.into()) else {
            return true;
        };

        if test_footprint_selector(&fp, arg) && func(&fp) {
            found = true;
            return false;
        }

        true
    });

    found
}

/// The three courtyard predicates (`intersectsCourtyardFunc`, `intersectsFrontCourtyardFunc`,
/// `intersectsBackCourtyardFunc`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum CourtyardSide {
    Both,
    Front,
    Back,
}

impl CourtyardSide {
    fn function_name(self) -> &'static str {
        match self {
            CourtyardSide::Both => "intersectsCourtyard()",
            CourtyardSide::Front => "intersectsFrontCourtyard()",
            CourtyardSide::Back => "intersectsBackCourtyard()",
        }
    }

    /// `m_Intersects{,F,B}CourtyardCache`.
    fn pair_cache(self, caches: &DrcPredicateCaches) -> &MemoCache<(ItemId, ItemId), bool> {
        match self {
            CourtyardSide::Both => &caches.intersects_courtyard,
            CourtyardSide::Front => &caches.intersects_f_courtyard,
            CourtyardSide::Back => &caches.intersects_b_courtyard,
        }
    }

    /// `m_Intersects{,F,B}CourtyardResultCache`.
    fn result_cache(
        self,
        caches: &DrcPredicateCaches,
    ) -> &MemoCache<ItemSelectorLayerCacheKey, bool> {
        match self {
            CourtyardSide::Both => &caches.intersects_courtyard_result,
            CourtyardSide::Front => &caches.intersects_f_courtyard_result,
            CourtyardSide::Back => &caches.intersects_b_courtyard_result,
        }
    }
}

/// `MISSING_FP_ARG( f )`.
fn missing_fp_arg(f: &str) -> String {
    format!("Missing footprint argument (A, B, or reference designator) to {f}.")
}

/// Port of `intersectsCourtyardFunc`, `intersectsFrontCourtyardFunc` and
/// `intersectsBackCourtyardFunc` (one body, `side` selects the courtyard and the caches).
fn intersects_courtyard_impl(
    ctx: &mut PcbContext<'_>,
    vref: Option<&PcbExprVarRef>,
    side: CourtyardSide,
) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || missing_fp_arg(side.function_name()));
        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let transient = is_transient(&item);
        let selector = arg_string(ctx, arg);
        let caches = board_caches(item.ctx.board);

        // Whole-predicate memo: the same condition repeated across many rules resolves in
        // O(1) here instead of re-scanning every footprint.  "A"/"B" select the other item of
        // the current pair rather than a board-wide set, so they cannot be keyed by item alone
        // (and touch only one footprint anyway); skip the memo for those.
        let memoize = !transient && selector != "A" && selector != "B";

        let rkey = item.id().map(|id| ItemSelectorLayerCacheKey {
            item: id,
            selector: selector.clone(),
            layer: ctx.env().layer(),
            constraint: ctx.env().constraint(),
        });

        if memoize {
            if let (Some(caches), Some(rkey)) = (&caches, &rkey) {
                if let Some(whole) = side.result_cache(&caches.predicates).get(rkey) {
                    return if whole { 1.0 } else { 0.0 };
                }
            }
        }

        let item_shape: Option<SharedShape> = None;

        let res = search_footprints_near_item(&selector, ctx, &item, &mut |fp| {
            let key = fp.id().zip(item.id());

            if !transient {
                if let (Some(caches), Some(key)) = (&caches, key) {
                    if let Some(cached) = side.pair_cache(&caches.predicates).get(&key) {
                        return cached;
                    }
                }
            }

            let hit = match side {
                CourtyardSide::Both => {
                    collides_with_courtyard(&item, &item_shape, ctx, fp, PcbLayerId::FCu)
                        || collides_with_courtyard(&item, &item_shape, ctx, fp, PcbLayerId::BCu)
                }
                CourtyardSide::Front | CourtyardSide::Back => {
                    let flipped = fp.as_footprint().is_some_and(|f| f.is_flipped());
                    let front = (side == CourtyardSide::Front) != flipped;
                    let layer_id = if front {
                        PcbLayerId::FCu
                    } else {
                        PcbLayerId::BCu
                    };

                    collides_with_courtyard(&item, &item_shape, ctx, fp, layer_id)
                }
            };

            if !transient {
                if let (Some(caches), Some(key)) = (&caches, key) {
                    side.pair_cache(&caches.predicates).set(key, hit);
                }
            }

            hit
        });

        if memoize {
            if let (Some(caches), Some(rkey)) = (&caches, rkey) {
                side.result_cache(&caches.predicates).set(rkey, res);
            }
        }

        if res { 1.0 } else { 0.0 }
    });
}

/*
 * Rule areas
 */

/// Port of the static `getDeflatedZoneOutline`.
fn get_deflated_zone_outline(area: &ItemRef<'_>) -> ShapePolySet {
    let board = area.ctx.board;
    let caches = board_caches(board);

    // Check cache first
    if let (Some(caches), Some(id)) = (&caches, area.id()) {
        if let Some(outline) = caches.deflated_zone_outlines.borrow().get(&id) {
            return outline.clone();
        }
    }

    // Cache miss - compute deflated outline
    let zone = area.as_zone().expect("zone item");
    let mut area_outline = zone.board_outline(area.ctx);
    area_outline.clear_arcs();
    area_outline.deflate(
        board.design_settings().get_drc_epsilon(),
        CornerStrategy::AllowAcuteCorners,
        ARC_LOW_DEF,
    );

    // Store in cache
    if let (Some(caches), Some(id)) = (&caches, area.id()) {
        caches
            .deflated_zone_outlines
            .borrow_mut()
            .insert(id, area_outline.clone());
    }

    area_outline
}

/// Port of `collidesWithArea`.
///
/// `area_layers` is `aArea->GetLayerSet()` as the caller's `SCOPED_LAYERSET` left it (the
/// C++ temporarily changes the zone's layer set; the Rust board is immutable here).
fn collides_with_area(
    item: &ItemRef<'_>,
    layer: PcbLayerId,
    ctx: &PcbContext<'_>,
    area: &ItemRef<'_>,
    area_layers: &Lset,
    for_keepout: bool,
) -> bool {
    let area_bbox = area.bounding_box();

    // Get cached deflated outline. Collisions include touching, so we need to deflate outline
    // by enough to exclude it. This is particularly important for detecting copper fills as
    // they will be exactly touching along the entire exclusion border.
    let area_outline = get_deflated_zone_outline(area);

    if is_hole_proxy(item) {
        if item.type_() == KicadT::PcbPadT {
            return item
                .effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::NullConstraint)
                .is_some_and(|hole| poly_collide(&area_outline, hole.as_ref()));
        } else if item.type_() == KicadT::PcbViaT {
            let overlap = &item.layer_set() & area_layers;

            // Avoid buried vias that don't overlap the zone's layers
            if overlap.any() {
                let ctx_layer = ctx.env().layer();

                if ctx_layer == UNDEFINED_LAYER || overlap.contains(ctx_layer) {
                    return item
                        .effective_hole_shape(UNDEFINED_LAYER, DrcConstraintT::NullConstraint)
                        .is_some_and(|hole| poly_collide(&area_outline, hole.as_ref()));
                }
            }
        }

        return false;
    }

    if let Some(footprint) = item.as_footprint() {
        let flags = footprint.courtyard_flags(item.ctx);

        if flags.malformed_front || flags.malformed_back {
            report(ctx, || {
                "Footprint's courtyard is not a single, closed shape.".to_string()
            });

            return false;
        }

        if (area_layers & Lset::front_mask()).any() {
            let courtyard = footprint.courtyard(item.ctx, PcbLayerId::FCrtYd);

            if courtyard.outline_count() == 0 {
                report(ctx, || "Footprint has no front courtyard.".to_string());
            } else if poly_collide(&area_outline, courtyard.c_outline(0)) {
                return true;
            }
        }

        if (area_layers & Lset::back_mask()).any() {
            let courtyard = footprint.courtyard(item.ctx, PcbLayerId::BCrtYd);

            if courtyard.outline_count() == 0 {
                report(ctx, || "Footprint has no back courtyard.".to_string());
            } else if poly_collide(&area_outline, courtyard.c_outline(0)) {
                return true;
            }
        }

        false
    } else if item.type_() == KicadT::PcbZoneT {
        let zone = item.as_zone().expect("zone item");

        if for_keepout {
            if !zone.is_filled() {
                return false;
            }

            // board->GetCopperZoneRTree( zone ) is the same tree built by the cache generator
            // (DRC_CACHE_GENERATOR inserts every copper zone on each of its layers); the
            // fallback below builds the identical single-zone tree for this layer, so the
            // yes / no answer is the same either way.
            let mut rtree = DrcRtree::new();
            rtree.insert(*item, layer, DrcConstraintT::ClearanceConstraint, 0, false);
            rtree.build();

            rtree.query_colliding_box(&area_bbox, &area_outline, layer)
        } else {
            let zone_poly = zone_outline_for_collision(item);
            poly_collide(&area_outline, &zone_poly)
        }
    } else {
        if !area_layers.contains(layer) {
            return false;
        }

        let shape = item.effective_shape(layer, Default::default(), DrcConstraintT::NullConstraint);
        poly_collide(&area_outline, shape.as_ref())
    }
}

/// Port of `searchAreas`.
fn search_areas<'e>(
    arg: &str,
    ctx: &PcbContext<'e>,
    item: &ItemRef<'e>,
    func: &mut dyn FnMut(Option<ItemRef<'e>>) -> bool,
) -> bool {
    let board_ctx = item.ctx;
    let board = board_ctx.board;

    if arg == "A" {
        func(as_zone_ref(ctx.env().item(0)))
    } else if arg == "B" {
        func(as_zone_ref(ctx.env().item(1)))
    } else if Kiid::sniff_test(arg) {
        // Resolve through the board's item index: it re-binds stale entries and includes
        // both board zones and zones inside footprints. In the normal path, it gives an O(1)
        // lookup for the item by its UUID.
        let target = Kiid::parse(arg);

        match target.and_then(|t| board_ctx.resolve(&t)) {
            Some(zone) if zone.type_() == KicadT::PcbZoneT => func(Some(zone)),
            _ => false,
        }
    } else {
        // Match on zone name.  Use cached zone name lookup to avoid O(n) iteration through all
        // zones for each call.
        let caches = board_caches(board);
        let cached = caches
            .as_ref()
            .and_then(|c| c.zones_by_name.borrow().get(arg).cloned());

        let matching_zones = match cached {
            Some(zones) => zones,
            None => {
                let mut matching_zones: Vec<ItemId> = Vec::new();

                for (id, area) in board.zones() {
                    if wx_string_matches(area.zone_name(), arg) {
                        matching_zones.push(id.into());
                    }
                }

                for (fp_id, _) in board.footprints() {
                    for (id, area) in board.footprint_zones(fp_id) {
                        if wx_string_matches(area.zone_name(), arg) {
                            matching_zones.push(id.into());
                        }
                    }
                }

                // Store in cache for future lookups
                if let Some(caches) = &caches {
                    caches
                        .zones_by_name
                        .borrow_mut()
                        .insert(arg.to_string(), matching_zones.clone());
                }

                matching_zones
            }
        };

        for id in matching_zones {
            if func(board_ctx.item(id)) {
                return true;
            }
        }

        false
    }
}

/// `MISSING_AREA_ARG( f )`.
fn missing_area_arg(f: &str) -> String {
    format!("Missing rule-area argument (A, B, or rule-area name) to {f}.")
}

/// The deferred part of `doIntersectsAreaFunc`: does `item` intersect a rule area selected by
/// `selector` on the context's layer? Also the native form of the implicit keepout condition
/// `A.intersectsKeepout('<uuid>')` (see [`intersects_keepout`]).
fn intersects_area_eval<'e>(
    ctx: &PcbContext<'e>,
    item: ItemRef<'e>,
    selector: &str,
    for_keepout: bool,
) -> bool {
    let layer = ctx.env().layer();
    let constraint = ctx.env().constraint();
    let transient = is_transient(&item);
    let caches = board_caches(item.ctx.board);

    // See intersectsCourtyard: "A"/"B" are pair-relative and not memoizable here.
    let memoize = !transient && selector != "A" && selector != "B";

    let rkey = item.id().map(|id| ItemSelectorLayerCacheKey {
        item: id,
        selector: selector.to_string(),
        layer,
        constraint,
    });

    if memoize {
        if let (Some(c), Some(rkey)) = (&caches, &rkey) {
            if let Some(whole) = area_result_cache(c, for_keepout).get(rkey) {
                return whole;
            }
        }
    }

    let item_bbox = item.bounding_box();

    let res = search_areas(selector, ctx, &item, &mut |area| {
        let Some(area) = area else {
            return false;
        };

        if area == item || area.parent() == Some(item) {
            return false;
        }

        // SCOPED_LAYERSET scopedLayerSet( aArea );
        let mut area_layers = area.layer_set();

        if constraint == DrcConstraintT::SilkClearanceConstraint as i32 {
            // Silk clearance tests are run across layer pairs
            if (area_layers.contains(PcbLayerId::FSilkS) && is_front_layer(layer))
                || (area_layers.contains(PcbLayerId::BSilkS) && is_back_layer(layer))
            {
                area_layers.set(layer);
            }
        }

        let common_layers = &area_layers & &item.layer_set();

        if !common_layers.any() {
            return false;
        }

        if !area.bounding_box().intersects(&item_bbox) {
            return false;
        }

        let mut test_layers = Lset::new();

        if layer != UNDEFINED_LAYER {
            test_layers.set(layer);
        } else {
            test_layers = common_layers;
        }

        let is_transient = is_transient(&item);
        let mut layers_to_compute: Vec<PcbLayerId> = Vec::new();
        let pair_key = |l: PcbLayerId| area.id().zip(item.id()).map(|(a, i)| (a, i, l));

        if !is_transient {
            for test_layer in test_layers.ui_order().iter().copied() {
                let cached = match (&caches, pair_key(test_layer)) {
                    (Some(c), Some(key)) => area_pair_cache(c, for_keepout).get(&key),
                    _ => None,
                };

                match cached {
                    Some(cached) => {
                        if cached {
                            return true;
                        }
                    }
                    None => layers_to_compute.push(test_layer),
                }
            }
        } else {
            for test_layer in test_layers.ui_order().iter().copied() {
                layers_to_compute.push(test_layer);
            }
        }

        let mut any_collision = false;

        for test_layer in layers_to_compute {
            let collides =
                collides_with_area(&item, test_layer, ctx, &area, &area_layers, for_keepout);

            if !is_transient {
                if let (Some(c), Some(key)) = (&caches, pair_key(test_layer)) {
                    area_pair_cache(c, for_keepout).set(key, collides);
                }
            }

            if collides {
                any_collision = true;
            }
        }

        any_collision
    });

    if memoize {
        if let (Some(c), Some(rkey)) = (&caches, rkey) {
            area_result_cache(c, for_keepout).set(rkey, res);
        }
    }

    res
}

/// `aForKeepout ? board->m_IntersectsKeepoutResultCache : board->m_IntersectsAreaResultCache`.
fn area_result_cache(
    caches: &PcbExprBoardCaches,
    for_keepout: bool,
) -> &MemoCache<ItemSelectorLayerCacheKey, bool> {
    if for_keepout {
        &caches.predicates.intersects_keepout_result
    } else {
        &caches.predicates.intersects_area_result
    }
}

/// `aForKeepout ? board->m_IntersectsKeepoutCache : board->m_IntersectsAreaCache`.
fn area_pair_cache(
    caches: &PcbExprBoardCaches,
    for_keepout: bool,
) -> &MemoCache<(ItemId, ItemId, PcbLayerId), bool> {
    if for_keepout {
        &caches.predicates.intersects_keepout
    } else {
        &caches.predicates.intersects_area
    }
}

/// Port of the static `doIntersectsAreaFunc`.
fn do_intersects_area_func(
    ctx: &mut PcbContext<'_>,
    vref: Option<&PcbExprVarRef>,
    for_keepout: bool,
) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            if for_keepout {
                missing_area_arg("intersectsKeepout()")
            } else {
                missing_area_arg("intersectsArea()")
            }
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let selector = arg_string(ctx, arg);

        if intersects_area_eval(ctx, item, &selector, for_keepout) {
            1.0
        } else {
            0.0
        }
    });
}

/// Port of `intersectsKeepoutFunc`.
fn intersects_keepout_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    do_intersects_area_func(ctx, vref, true);
}

/// Port of `intersectsAreaFunc`.
fn intersects_area_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    do_intersects_area_func(ctx, vref, false);
}

/// The value of `A.intersectsKeepout( '<selector>' )` for the items `a` / `b`, the constraint
/// and the layer of a rule evaluation (`DRC_RULE_CONDITION::EvaluateFor` without a reporter):
/// the native form of the implicit keepout rule conditions. `false` without item `a`.
pub fn intersects_keepout(
    a: Option<ItemRef<'_>>,
    b: Option<ItemRef<'_>>,
    selector: &str,
    constraint: i32,
    layer: PcbLayerId,
) -> bool {
    let Some(item) = a else {
        return false;
    };

    let ctx: PcbContext<'_> = Context::new(PcbExprUcode::item_env(a, b, constraint, layer));

    if selector.is_empty() {
        return false;
    }

    intersects_area_eval(&ctx, item, selector, true)
}

/// Port of `enclosedByAreaFunc`.
fn enclosed_by_area_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || missing_area_arg("enclosedByArea()"));
        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let board = item.ctx.board;
        let max_error = board.design_settings().max_error();
        let layer = ctx.env().layer();
        let transient = is_transient(&item);
        let selector = arg_string(ctx, arg);
        let caches = board_caches(board);

        // See intersectsCourtyard: "A"/"B" are pair-relative and not memoizable here.
        let memoize = !transient && selector != "A" && selector != "B";

        let rkey = item.id().map(|id| ItemSelectorLayerCacheKey {
            item: id,
            selector: selector.clone(),
            layer,
            constraint: ctx.env().constraint(),
        });

        if memoize {
            if let (Some(c), Some(rkey)) = (&caches, &rkey) {
                if let Some(whole) = c.predicates.enclosed_by_area_result.get(rkey) {
                    return if whole { 1.0 } else { 0.0 };
                }
            }
        }

        let item_bbox = item.bounding_box();

        let res = search_areas(&selector, ctx, &item, &mut |area| {
            let Some(area) = area else {
                return false;
            };

            if area == item || area.parent() == Some(item) {
                return false;
            }

            if item.type_() != KicadT::PcbFootprintT
                && !(&area.layer_set() & &item.layer_set()).any()
            {
                return false;
            }

            if !area.bounding_box().intersects(&item_bbox) {
                return false;
            }

            let key = area.id().zip(item.id()).map(|(a, i)| (a, i, layer));

            if !is_transient(&item) {
                if let (Some(c), Some(key)) = (&caches, key) {
                    if let Some(cached) = c.predicates.enclosed_by_area.get(&key) {
                        return cached;
                    }
                }
            }

            let mut item_shape = ShapePolySet::default();

            if item.type_() == KicadT::PcbZoneT {
                item_shape = item.as_zone().expect("zone item").board_outline(item.ctx);
            } else if let Some(fp) = item.as_footprint() {
                for test_layer in area.layer_set().iter() {
                    fp.transform_pads_to_poly_set(
                        item.ctx,
                        &mut item_shape,
                        test_layer,
                        0,
                        max_error,
                        ErrorLoc::Outside,
                    );
                    fp.transform_fp_shapes_to_poly_set(
                        item.ctx,
                        &mut item_shape,
                        test_layer,
                        0,
                        max_error,
                        ErrorLoc::Outside,
                        true,
                        true,
                        false,
                    );
                }
            } else {
                item.transform_shape_to_polygon(
                    &mut item_shape,
                    layer,
                    0,
                    max_error,
                    ErrorLoc::Outside,
                    false,
                );
            }

            let enclosed_by_area = if item_shape.is_empty() {
                // If it's already empty then our test will have no meaning.
                false
            } else {
                // GetBoardOutline() is expensive.  Only use it where we have to.
                let area_outline = zone_outline_for_collision(&area);

                item_shape.clear_arcs();
                item_shape.boolean_subtract(&area_outline);

                item_shape.is_empty()
            };

            if !is_transient(&item) {
                if let (Some(c), Some(key)) = (&caches, key) {
                    c.predicates.enclosed_by_area.set(key, enclosed_by_area);
                }
            }

            enclosed_by_area
        });

        if memoize {
            if let (Some(c), Some(rkey)) = (&caches, rkey) {
                c.predicates.enclosed_by_area_result.set(rkey, res);
            }
        }

        if res { 1.0 } else { 0.0 }
    });
}

/*
 * Groups, sheets, footprints
 */

/// Port of `memberOfGroupFunc` (also `memberOf`).
fn member_of_group_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!("Missing group name argument to {}.", "memberOfGroup()")
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let mut group = item.parent_group();

        if group.is_none() {
            if let Some(parent) = item.parent().filter(|p| p.type_() == KicadT::PcbFootprintT) {
                group = parent.parent_group();
            }
        }

        let name = arg_string(ctx, arg);

        while let Some(g) = group {
            // EDA_GROUP: a group or a generator
            let group_name = g.as_group().map_or("", |grp| grp.name());

            if wx_string_matches(group_name, &name) {
                return 1.0;
            }

            group = g.parent_group();
        }

        0.0
    });
}

/// `MISSING_SHEET_ARG( f )`.
fn missing_sheet_arg(f: &str) -> String {
    format!("Missing sheet name argument to {f}.")
}

/// The footprint `memberOfSheet` / `memberOfSheetOrChildren` look at: the parent footprint,
/// or the item itself when it is one.
fn sheet_footprint<'e>(item: &ItemRef<'e>) -> Option<ItemRef<'e>> {
    item.parent_footprint()
        .or_else(|| (item.type_() == KicadT::PcbFootprintT).then_some(*item))
}

/// `wxString::RemoveLast()` after `EndsWith( "/" )`.
fn strip_trailing_slash(s: &mut String) {
    if s.ends_with('/') {
        s.pop();
    }
}

/// Port of `memberOfSheetFunc`.
fn member_of_sheet_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || missing_sheet_arg("memberOfSheet()"));
        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let Some(fp) = sheet_footprint(&item) else {
            return 0.0;
        };

        let mut sheet_name = fp.as_footprint().expect("footprint").sheetname.clone();
        let mut ref_name = arg_string(ctx, arg);

        strip_trailing_slash(&mut sheet_name);
        strip_trailing_slash(&mut ref_name);

        if wx_string_matches(&sheet_name, &ref_name) {
            return 1.0;
        }

        if (wx_string_matches(&ref_name, "/") || ref_name.is_empty()) && sheet_name.is_empty() {
            return 1.0;
        }

        0.0
    });
}

/// Port of `memberOfSheetOrChildrenFunc`.
fn member_of_sheet_or_children_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || missing_sheet_arg("memberOfSheetOrChildren()"));
        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let Some(fp) = sheet_footprint(&item) else {
            return 0.0;
        };

        let mut sheet_name = fp.as_footprint().expect("footprint").sheetname.clone();
        let mut ref_name = arg_string(ctx, arg);

        strip_trailing_slash(&mut sheet_name);
        strip_trailing_slash(&mut ref_name);

        let sheet_path = crate::marker::wx_split(&sheet_name, '/', '\\');
        let ref_path = crate::marker::wx_split(&ref_name, '/', '\\');

        if ref_path.len() > sheet_path.len() {
            return 0.0;
        }

        if (wx_string_matches(&ref_name, "/") || ref_name.is_empty()) && sheet_name.is_empty() {
            return 1.0;
        }

        for i in 0..ref_path.len() {
            if !wx_string_matches(&sheet_path[i], &ref_path[i]) {
                return 0.0;
            }
        }

        1.0
    });
}

/// Port of `memberOfFootprintFunc`.
fn member_of_footprint_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!(
                "Missing footprint argument (reference designator) to {}.",
                "memberOfFootprint()"
            )
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if let Some(parent_fp) = item.parent_footprint() {
            if test_footprint_selector(&parent_fp, &arg_string(ctx, arg)) {
                return 1.0;
            }
        }

        0.0
    });
}

/*
 * Vias
 */

/// The common body of `isMicroVia`, `isBlindVia`, `isBuriedVia` and `isBlindBuriedViaFunc`.
fn via_predicate(
    ctx: &mut PcbContext<'_>,
    vref: Option<&PcbExprVarRef>,
    test: fn(&PcbVia) -> bool,
) {
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    if item
        .filter(|i| i.type_() == KicadT::PcbViaT)
        .and_then(|i| i.as_via())
        .is_some_and(test)
    {
        ctx.value_mut(result).set_double(1.0);
    }
}

/// Port of `isMicroVia`.
fn is_micro_via(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    via_predicate(ctx, vref, PcbVia::is_micro_via);
}

/// Port of `isBlindVia`.
fn is_blind_via(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    via_predicate(ctx, vref, PcbVia::is_blind_via);
}

/// Port of `isBuriedVia`.
fn is_buried_via(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    via_predicate(ctx, vref, PcbVia::is_buried_via);
}

/// Port of `isBlindBuriedViaFunc`.
fn is_blind_buried_via_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    via_predicate(ctx, vref, |via| via.is_blind_via() || via.is_buried_via());
}

/// Port of `isStackedViaFunc`.
fn is_stacked_via_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    let Some(item) = item.filter(|i| i.type_() == KicadT::PcbViaT) else {
        return;
    };

    let Some(via) = item.as_via() else {
        return;
    };

    if !item.has_board() || via.via_type() != Viatype::Microvia {
        return;
    }

    let Some(via_id): Option<TrackId> = item.id().and_then(|id| id.typed()) else {
        return;
    };

    let caches = board_caches(item.ctx.board);

    if let Some(caches) = &caches {
        if let Some(stacked) = caches.stacked_microvias.borrow().as_ref() {
            if stacked.contains(&via_id) {
                ctx.value_mut(result).set_double(1.0);
            }

            return;
        }
    }

    let mut stacked: BTreeSet<TrackId> = BTreeSet::new();

    for column in PcbVia::collect_microvia_columns(item.ctx) {
        stacked.extend(column);
    }

    let hit = stacked.contains(&via_id);

    if let Some(caches) = &caches {
        *caches.stacked_microvias.borrow_mut() = Some(stacked);
    }

    if hit {
        ctx.value_mut(result).set_double(1.0);
    }
}

/*
 * Diff pairs and net chains
 */

/// Port of `isCoupledDiffPairFunc`.
fn is_coupled_diff_pair_func(ctx: &mut PcbContext<'_>, _vref: Option<&PcbExprVarRef>) {
    let a = ctx.env().item(0).filter(|i| i.as_connected().is_some());
    let b = ctx.env().item(1).filter(|i| i.as_connected().is_some());
    let result = push_zero(ctx);

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let Some(netinfo) = a.and_then(|a| a.net()) else {
            return 0.0;
        };

        let (polarity, coupled_net, _) = match_dp_suffix(netinfo.netname());

        if polarity == 0 {
            return 0.0;
        }

        let constraint = ctx.env().constraint();

        if constraint == DrcConstraintT::DiffPairGapConstraint as i32
            || constraint == DrcConstraintT::LengthConstraint as i32
            || constraint == DrcConstraintT::NetChainLengthConstraint as i32
            || constraint == DrcConstraintT::SkewConstraint as i32
        {
            // DRC engine evaluates these only in the context of a diffpair, but doesn't always
            // supply the second (B) item.
            if let Some(a) = a.filter(|a| a.has_board()) {
                if a.ctx.board.find_net_by_name(&coupled_net).is_some() {
                    return 1.0;
                }
            }
        }

        if b.is_some_and(|b| b.netname() == coupled_net) {
            return 1.0;
        }

        0.0
    });
}

/// Port of `inDiffPairFunc`.
fn in_diff_pair_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let argv = ctx.pop();
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    if arg_is_empty(ctx, argv) {
        report(ctx, || {
            format!("Missing diff-pair name argument to {}.", "inDiffPair()")
        });

        return;
    }

    let Some(item) = item.filter(|i| i.has_board()) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if item.is_connected() {
            let Some(netinfo) = item.net() else {
                return 0.0;
            };

            let ref_name = netinfo.netname();
            let arg = arg_string(ctx, argv);
            let (polarity, coupled_net, base_name) = match_dp_suffix(ref_name);

            if polarity != 0 && item.ctx.board.find_net_by_name(&coupled_net).is_some() {
                if wx_string_matches(&base_name, &arg) {
                    return 1.0;
                }

                if base_name.ends_with('_') {
                    // baseName.BeforeLast( '_' ): empty when there is no '_'
                    let before_last = base_name.rfind('_').map_or("", |pos| &base_name[..pos]);

                    if wx_string_matches(before_last, &arg) {
                        return 1.0;
                    }
                }
            }
        }

        0.0
    });
}

/// Port of `inNetChainFunc`.
fn in_net_chain_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let argv = ctx.pop();
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    if arg_is_empty(ctx, argv) {
        report(ctx, || {
            format!("Missing net-chain name argument to {}", "inNetChain()")
        });

        return;
    }

    let Some(item) = item.filter(|i| i.has_board()) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if !item.is_connected() {
            return 0.0;
        }

        let Some(netinfo) = item.net() else {
            return 0.0;
        };

        let chain_name = netinfo.net_chain();

        if chain_name.is_empty() {
            return 0.0;
        }

        if wx_string_matches(chain_name, &arg_string(ctx, argv)) {
            1.0
        } else {
            0.0
        }
    });
}

/// Port of `hasNetChainFunc`.
fn has_net_chain_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    let Some(item) = item.filter(|i| i.has_board()) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |_ctx| {
        if !item.is_connected() {
            return 0.0;
        }

        match item.net() {
            Some(netinfo) if !netinfo.net_chain().is_empty() => 1.0,
            _ => 0.0,
        }
    });
}

/// Port of `inNetChainClassFunc`.
fn in_net_chain_class_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let argv = ctx.pop();
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    if arg_is_empty(ctx, argv) {
        report(ctx, || {
            format!("Missing netclass name argument to {}", "inNetChainClass()")
        });

        return;
    }

    let Some(item) = item.filter(|i| i.has_board()) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if !item.is_connected() {
            return 0.0;
        }

        let Some(netinfo) = item.net() else {
            return 0.0;
        };

        let chain_name = netinfo.net_chain();

        if chain_name.is_empty() {
            return 0.0;
        }

        // The board's NET_SETTINGS always exists in the port (C++ `if( ns )`).
        let ns = &item.ctx.board.design_settings().net_settings;
        let class_name = ns.get_net_chain_class(chain_name);

        if class_name.is_empty() {
            return 0.0;
        }

        if wx_string_matches(&class_name, &arg_string(ctx, argv)) {
            1.0
        } else {
            0.0
        }
    });
}

/*
 * Fields, netclasses, component classes, custom properties
 */

/// Port of `getFieldFunc`.
fn get_field_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let item = receiver(ctx, vref);
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_string("");
    ctx.push(result);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!("Missing field name argument to {}.", "getField()")
        });

        return;
    }

    let Some(item) = item.filter(|i| i.has_board()) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_string(move |ctx| {
        let Some(fp) = item.as_footprint() else {
            return String::new();
        };

        let field_name = arg_string(ctx, arg);
        let board = item.has_board().then_some(item.ctx.board);

        if let Some(board) = board {
            let variant_name = board.get_current_variant();

            if let Some(variant) = fp.variant(variant_name) {
                if variant.has_field_value(&field_name) {
                    return variant.field_value(&field_name);
                }
            }
        }

        // Only base values go in the cache.
        let caches = board.and_then(board_caches);
        let key = item.id().map(|id| (id, field_name.clone()));

        if let (Some(c), Some(key)) = (&caches, &key) {
            if let Some(cached) = c.predicates.item_field.get(key) {
                return cached;
            }
        }

        let text = fp
            .field_by_name(item.ctx, &field_name)
            .and_then(|id| item.ctx.board.field(id))
            .map(|field| field.text.text.text().to_string())
            .unwrap_or_default();

        if let (Some(c), Some(key)) = (&caches, key) {
            c.predicates.item_field.set(key, text.clone());
        }

        text
    });
}

/// Port of `hasNetclassFunc`.
fn has_netclass_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!("Missing netclass name argument to {}.", "hasNetclass()")
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if !item.is_connected() {
            return 0.0;
        }

        match item.effective_netclass() {
            Some(netclass) if netclass.contains_netclass_with_name(&arg_string(ctx, arg)) => 1.0,
            _ => 0.0,
        }
    });
}

/// Port of `hasExactNetclassFunc`.
fn has_exact_netclass_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!(
                "Missing netclass name argument to {}.",
                "hasExactNetclass()"
            )
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if !item.is_connected() {
            return 0.0;
        }

        match item.effective_netclass() {
            Some(netclass) if netclass.name_equals(&arg_string(ctx, arg)) => 1.0,
            _ => 0.0,
        }
    });
}

/// Port of `hasComponentClassFunc`.
fn has_component_class_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let result = push_zero(ctx);

    if arg_is_empty(ctx, arg) {
        report(ctx, || {
            format!(
                "Missing component class name argument to {}.",
                "hasComponentClass()"
            )
        });

        return;
    }

    let Some(item) = receiver(ctx, vref) else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        let footprint = if item.type_() == KicadT::PcbFootprintT {
            Some(item)
        } else {
            item.parent_footprint()
        };

        let Some(footprint) = footprint else {
            return 0.0;
        };

        let comp_class = footprint
            .as_footprint()
            .and_then(|fp| fp.component_class(footprint.ctx));

        match comp_class {
            Some(cc) if cc.contains_class_name(&arg_string(ctx, arg)) => 1.0,
            _ => 0.0,
        }
    });
}

/// `item->GetCustomProperty( key, value )`, then the parent footprint's.
fn custom_property_of(item: &ItemRef<'_>, key: &str) -> Option<String> {
    if let Some(value) = item.base().custom_property(key) {
        return Some(value.to_string());
    }

    item.parent_footprint()
        .and_then(|fp| fp.base().custom_property(key).map(str::to_string))
}

/// Port of `customPropertyFunc`.
fn custom_property_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let item = receiver(ctx, vref);
    let result = ctx.alloc_value();

    ctx.value_mut(result).set_string("");
    ctx.push(result);

    // `if( !arg )`: never true, Pop() always returns a value.

    let Some(item) = item else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_string(move |ctx| {
        custom_property_of(&item, &arg_string(ctx, arg)).unwrap_or_default()
    });
}

/// Port of `hasCustomPropertyFunc`.
fn has_custom_property_func(ctx: &mut PcbContext<'_>, vref: Option<&PcbExprVarRef>) {
    let arg = ctx.pop();
    let item = receiver(ctx, vref);
    let result = push_zero(ctx);

    // `if( !arg )`: never true, Pop() always returns a value.

    let Some(item) = item else {
        return;
    };

    ctx.value_mut(result).set_deferred_eval_double(move |ctx| {
        if custom_property_of(&item, &arg_string(ctx, arg)).is_some() {
            1.0
        } else {
            0.0
        }
    });
}

/*
 * Registration
 */

/// A function body in the form `FuncCallRef` stores.
type Body = fn(&mut PcbContext<'_>, Option<&PcbExprVarRef>);

/// Port of `PCBEXPR_BUILTIN_FUNCTIONS::RegisterAllFunctions` (after its `m_funcs.clear()`).
pub fn register_all_functions(registry: &mut PcbExprBuiltinFunctions) {
    let mut register = |signature: &str, body: Body, geometry_dependent: bool| {
        registry.register_func(signature, FuncCallRef::new(body), geometry_dependent);
    };

    register("existsOnLayer('x')", exists_on_layer_func, false);

    register("isPlated()", is_plated_func, false);

    // Geometry-dependent functions depend on item position/shape rather than item properties.
    // The third argument marks them so that CreateFuncCall() can detect them automatically.
    register(
        "insideCourtyard('x') DEPRECATED",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Both),
        true,
    );
    register(
        "insideFrontCourtyard('x') DEPRECATED",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Front),
        true,
    );
    register(
        "insideBackCourtyard('x') DEPRECATED",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Back),
        true,
    );
    register(
        "intersectsCourtyard('x')",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Both),
        true,
    );
    register(
        "intersectsFrontCourtyard('x')",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Front),
        true,
    );
    register(
        "intersectsBackCourtyard('x')",
        |c, v| intersects_courtyard_impl(c, v, CourtyardSide::Back),
        true,
    );

    register("insideArea('x') DEPRECATED", intersects_keepout_func, true);
    register("intersectsArea('x')", intersects_area_func, true);
    register("intersectsKeepout('x')", intersects_keepout_func, true);
    register("enclosedByArea('x')", enclosed_by_area_func, true);

    register("isMicroVia()", is_micro_via, false);
    register("isBlindVia()", is_blind_via, false);
    register("isBuriedVia()", is_buried_via, false);
    register("isBlindBuriedVia()", is_blind_buried_via_func, false);
    register("isStackedVia()", is_stacked_via_func, false);

    register("memberOf('x') DEPRECATED", member_of_group_func, false);
    register("memberOfGroup('x')", member_of_group_func, false);
    register("memberOfFootprint('x')", member_of_footprint_func, false);
    register("memberOfSheet('x')", member_of_sheet_func, false);
    register(
        "memberOfSheetOrChildren('x')",
        member_of_sheet_or_children_func,
        false,
    );

    register(
        "fromTo('x','y')",
        |c, v| {
            from_to_func(c, v);
        },
        false,
    );
    register("isCoupledDiffPair()", is_coupled_diff_pair_func, false);
    register("inDiffPair('x')", in_diff_pair_func, false);
    register("inNetChain('x')", in_net_chain_func, false);
    register("hasNetChain()", has_net_chain_func, false);
    register("inNetChainClass('x')", in_net_chain_class_func, false);

    register("getField('x')", get_field_func, false);

    register("hasNetclass('x')", has_netclass_func, false);
    register("hasExactNetclass('x')", has_exact_netclass_func, false);
    register("hasComponentClass('x')", has_component_class_func, false);

    register("customProperty('x')", custom_property_func, false);
    register("hasCustomProperty('x')", has_custom_property_func, false);
}
