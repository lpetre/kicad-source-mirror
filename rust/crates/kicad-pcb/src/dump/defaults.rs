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

//! The `misc` item section and the `defaults` command (`dump_defaults.cpp`).

use std::sync::Arc;

use kicad_core::report_severity::Severity;
use kicad_core::{KicadT, PcbLayerId};
use kicad_geom::Vector2I;

use super::board::{SectionFn, dump_bbox, dump_item_base, is_dimension};
use super::format::{
    Out, attempt, emit, no_api, v_angle, v_bool, v_box, v_enum, v_f64, v_int, v_str, v_vec,
};
use super::{items, settings};
use crate::board::Board;
use crate::board_design_settings::BoardDesignSettings;
use crate::board_stackup::BoardStackup;
use crate::ctx::BoardCtx;
use crate::eda_shape::ShapeT;
use crate::footprint::Footprint;
use crate::ids::NetCode;
use crate::item::ParentRef;
use crate::item_ref::{AnyItem, ItemRef};
use crate::net_settings::NetSettings;
use crate::netclass::Netclass;
use crate::netinfo::NetinfoItem;
use crate::pcb_field::{FieldT, PcbField};
use crate::pcb_marker::{PcbMarker, RcItem};
use crate::pcb_plot_params::PcbPlotParams;
use crate::pcb_track::{PcbArc, PcbTrack, PcbVia, Track};
use crate::teardrop_parameters::TeardropParameters;
use crate::traits::BoardItem;
use crate::zone_settings::ZoneSettings;

/// The magic_enum names of `PCB_DRC_CODE` by value (first declared enumerator for aliases,
/// generated from `pcbnew/drc/drc_item.h`).
const DRCE_NAMES: [&str; 79] = [
    "?0",
    "DRCE_FIRST",
    "DRCE_SHORTING_ITEMS",
    "DRCE_ALLOWED_ITEMS",
    "DRCE_TEXT_ON_EDGECUTS",
    "DRCE_CLEARANCE",
    "DRCE_CREEPAGE",
    "DRCE_TRACKS_CROSSING",
    "DRCE_EDGE_CLEARANCE",
    "DRCE_ZONES_INTERSECT",
    "DRCE_ISOLATED_COPPER",
    "DRCE_STARVED_THERMAL",
    "DRCE_DANGLING_VIA",
    "DRCE_DANGLING_TRACK",
    "DRCE_DRILLED_HOLES_TOO_CLOSE",
    "DRCE_DRILLED_HOLES_COLOCATED",
    "DRCE_HOLE_CLEARANCE",
    "DRCE_TRACK_WIDTH",
    "DRCE_TRACK_ANGLE",
    "DRCE_TRACK_SEGMENT_LENGTH",
    "DRCE_ANNULAR_WIDTH",
    "DRCE_CONNECTION_WIDTH",
    "DRCE_DRILL_OUT_OF_RANGE",
    "DRCE_VIA_DIAMETER",
    "DRCE_PADSTACK",
    "DRCE_PADSTACK_INVALID",
    "DRCE_MICROVIA_DRILL_OUT_OF_RANGE",
    "DRCE_MALFORMED_MICROVIA_STACK_SPAN",
    "DRCE_MICROVIA_STACK_NOT_FILLED",
    "DRCE_MICROVIA_STACK_DEPTH",
    "DRCE_HOLE_ASPECT_RATIO",
    "DRCE_MICROVIA_CROSSES_CORE",
    "DRCE_OVERLAPPING_FOOTPRINTS",
    "DRCE_MISSING_COURTYARD",
    "DRCE_MALFORMED_COURTYARD",
    "DRCE_PTH_IN_COURTYARD",
    "DRCE_NPTH_IN_COURTYARD",
    "DRCE_DISABLED_LAYER_ITEM",
    "DRCE_INVALID_OUTLINE",
    "DRCE_MISSING_FOOTPRINT",
    "DRCE_DUPLICATE_FOOTPRINT",
    "DRCE_EXTRA_FOOTPRINT",
    "DRCE_NET_CONFLICT",
    "DRCE_SCHEMATIC_PARITY",
    "DRCE_FOOTPRINT_FILTERS",
    "DRCE_FOOTPRINT_TYPE_MISMATCH",
    "DRCE_LIB_FOOTPRINT_ISSUES",
    "DRCE_LIB_FOOTPRINT_MISMATCH",
    "DRCE_PAD_TH_WITH_NO_HOLE",
    "DRCE_FOOTPRINT",
    "DRCE_FOOTPRINT_SCALED_WITH_PADS",
    "DRCE_UNRESOLVED_VARIABLE",
    "DRCE_ASSERTION_FAILURE",
    "DRCE_GENERIC_WARNING",
    "DRCE_GENERIC_ERROR",
    "DRCE_COPPER_SLIVER",
    "DRCE_SOLDERMASK_BRIDGE",
    "DRCE_SILK_MASK_CLEARANCE",
    "DRCE_SILK_EDGE_CLEARANCE",
    "DRCE_SILK_CLEARANCE",
    "DRCE_TEXT_HEIGHT",
    "DRCE_TEXT_THICKNESS",
    "DRCE_LENGTH_OUT_OF_RANGE",
    "DRCE_NET_CHAIN_STUB_TOO_LONG",
    "DRCE_NET_CHAIN_RETURN_PATH_BREAK",
    "DRCE_NET_CHAIN_TUNING_PROFILES",
    "DRCE_SKEW_OUT_OF_RANGE",
    "DRCE_VIA_COUNT_OUT_OF_RANGE",
    "DRCE_DP_GAP_OUT_OF_RANGE",
    "DRCE_DP_UNCOUPLED_LENGTH_TOO_LONG",
    "DRCE_MIRRORED_TEXT_ON_FRONT_LAYER",
    "DRCE_UNMIRRORED_TEXT_ON_BACK_LAYER",
    "DRCE_MISSING_TUNING_PROFILE",
    "DRCE_TUNING_PROFILE_IMPLICIT_RULES",
    "DRCE_TRACK_ON_POST_MACHINED_LAYER",
    "DRCE_TRACK_NOT_CENTERED_ON_VIA",
    "DRCE_SCHEMATIC_FIELDS_PARITY",
    "DRCE_VIA_STITCH_OVERLAP",
    "DRCE_COPPER_OUTSIDE_BOARD",
];

/// `VEnum( (PCB_DRC_CODE) code )`.
pub(crate) fn drc_code_name(code: i32) -> String {
    usize::try_from(code)
        .ok()
        .and_then(|c| DRCE_NAMES.get(c))
        .map_or_else(|| format!("?{code}"), |s| (*s).to_string())
}

/// `VEnum( SEVERITY )`: `RPT_SEVERITY_*`.
pub(crate) fn severity_name(s: Severity) -> String {
    format!("RPT_SEVERITY_{}", v_enum(s))
}

/// `BOARD_DESIGN_SETTINGS::m_NetSettings` (a shared_ptr in C++, always present in Rust).
pub(crate) fn net_settings_of(b: &BoardDesignSettings) -> Option<&NetSettings> {
    Some(&b.net_settings)
}

// ---- misc section -------------------------------------------------------------------------

/// `DumpMisc`.
pub(crate) fn dump_misc(o: &mut Out, it: ItemRef<'_>) {
    let ctx = it.ctx;

    match it.any() {
        AnyItem::Target(t) => {
            emit!(o, "target.shape", v_int(t.shape()));
            emit!(o, "target.size", v_int(t.size()));
            emit!(o, "target.width", v_int(t.width()));
        }
        AnyItem::Point(p) => {
            emit!(o, "point.size", v_int(p.size()));
            emit!(o, "point.library_position", v_vec(p.lib_pos()));
        }
        AnyItem::Barcode(b) => {
            emit!(o, "barcode.text", v_str(b.text.text.text()));
            emit!(o, "barcode.kind", v_enum(b.kind));
            emit!(o, "barcode.ecc", v_enum(b.error_correction));
            emit!(o, "barcode.show_text", no_api("PCB_BARCODE::GetShowText"));
            emit!(o, "barcode.size", format!("{},{}", b.width, b.height));
            emit!(o, "barcode.text_size", no_api("PCB_BARCODE::GetTextSize"));
            emit!(o, "barcode.margin", v_vec(b.margin));
            emit!(o, "barcode.angle", v_angle(b.angle(ctx)));
            emit!(o, "barcode.library_pos", v_vec(b.lib_pos()));
            emit!(o, "barcode.library_angle", v_angle(b.lib_angle()));
            emit!(o, "barcode.poly", no_api("PCB_BARCODE::GetPolyShape"));
            emit!(
                o,
                "barcode.symbol_poly",
                no_api("PCB_BARCODE::GetSymbolPoly")
            );
            emit!(o, "barcode.text_poly", no_api("PCB_BARCODE::GetTextPoly"));
            emit!(o, "barcode.last_error", no_api("PCB_BARCODE::GetLastError"));
        }
        AnyItem::Image(r) => {
            emit!(o, "image.position", v_vec(r.pos));
            emit!(o, "image.size", v_vec(r.size()));
            emit!(
                o,
                "image.transform_origin_offset",
                v_vec(r.transform_origin_offset())
            );
            emit!(o, "image.orientation", v_angle(r.orientation()));
            emit!(o, "image.opacity", v_f64(r.opacity()));
            emit!(o, "image.scale", v_f64(r.image_scale()));
            emit!(o, "image.bbox", v_box(r.reference_image_bounding_box()));
        }
        AnyItem::Group(_) | AnyItem::Generator(_) => {
            let g = it.as_group().expect("group");
            emit!(o, "group.name", v_str(g.name()));
            let mut keys: Vec<String> = g.members.iter().map(|m| o.item_key(Some(*m))).collect();
            keys.sort();
            emit!(o, "group.members", format!("[{}]", keys.join(" ")));

            if let AnyItem::Generator(generator) = it.any() {
                emit!(o, "generator.type", v_str(generator.generator_type()));
            }
        }
        AnyItem::Table(t) => {
            emit!(o, "table.cols", v_int(t.col_count()));
            emit!(o, "table.rows", v_int(t.row_count()));
            emit!(o, "table.end", v_vec(t.end(ctx)));
            emit!(o, "table.border_width", v_int(t.border_stroke.width()));
            emit!(
                o,
                "table.border_style",
                items::v_line_style(t.border_style())
            );
            emit!(
                o,
                "table.separators_width",
                v_int(t.separators_stroke.width())
            );
            emit!(
                o,
                "table.separators_style",
                items::v_line_style(t.separators_style())
            );
            emit!(o, "table.stroke_external", v_bool(t.stroke_external));
            emit!(o, "table.stroke_header", v_bool(t.stroke_header_separator));
            emit!(o, "table.stroke_columns", v_bool(t.stroke_columns));
            emit!(o, "table.stroke_rows", v_bool(t.stroke_rows));

            if let Some(n) = o.cond("table.col_width.*", || t.col_count()) {
                for c in 0..n {
                    emit!(o, format!("table.col_width.{c}"), v_int(t.col_width(c)));
                }
            }

            if let Some(n) = o.cond("table.row_height.*", || t.row_count()) {
                for r in 0..n {
                    emit!(o, format!("table.row_height.{r}"), v_int(t.row_height(r)));
                }
            }
        }
        AnyItem::Marker(m) => {
            emit!(o, "marker.severity", severity_name(m.severity(ctx)));
            emit!(o, "marker.excluded", v_bool(m.is_excluded()));
            emit!(o, "marker.error_code", v_int(m.rc_item.error_code()));
            emit!(o, "marker.settings_key", v_str(m.rc_item.settings_key()));
        }
        _ => {}
    }
}

// ---- defaults -----------------------------------------------------------------------------

/// `dumpDefaultItem`: every applicable item section under key `class`, field prefix =
/// section name.
fn dump_default_item(o: &mut Out, class: &str, it: ItemRef<'_>) {
    o.scope("defaults", class, |o| {
        let t = it.type_();
        let run = |o: &mut Out, section: &str, f: SectionFn| {
            o.field_prefix = format!("{section}.");
            // The C++ guard reports with the bare section name (no prefix).
            let r = attempt(|| f(o, it));
            o.field_prefix.clear();

            if let Err(failure) = r {
                o.fail(section, failure);
            }
        };

        run(o, "item", dump_item_base);
        run(o, "bbox", dump_bbox);

        if matches!(t, KicadT::PcbTraceT | KicadT::PcbArcT | KicadT::PcbViaT) {
            run(o, "track", items::dump_track);
        }

        if t == KicadT::PcbPadT {
            run(o, "pad", items::dump_pad);
        }

        if matches!(
            t,
            KicadT::PcbShapeT | KicadT::PcbTextboxT | KicadT::PcbTablecellT
        ) {
            run(o, "shape", items::dump_shape);
        }

        if matches!(
            t,
            KicadT::PcbTextT | KicadT::PcbFieldT | KicadT::PcbTextboxT | KicadT::PcbTablecellT
        ) || is_dimension(t)
        {
            run(o, "text", items::dump_text);
        }

        if is_dimension(t) {
            run(o, "dim", items::dump_dimension);
        }

        if t == KicadT::PcbZoneT {
            run(o, "zone", items::dump_zone);
        }

        if t == KicadT::PcbFootprintT {
            run(o, "footprint", items::dump_footprint);
        }

        run(o, "misc", dump_misc);
    });
}

/// A class whose C++ constructor has no Rust counterpart yet (or whose Rust constructor is
/// pending): one `pending|defaults|CLASS|*=...` line covers all its fields.
fn class_pending(o: &mut Out, class: &str, failure: super::format::Failure) {
    o.scope("defaults", class, |o| o.fail("*", failure));
}

fn construct<T>(o: &mut Out, class: &str, f: impl FnOnce() -> T) -> Option<T> {
    match attempt(f) {
        Ok(v) => Some(v),
        Err(failure) => {
            class_pending(o, class, failure);
            None
        }
    }
}

/// `FOOTPRINT::GetNextFieldOrdinal()` (no Rust method yet; replace by it once it exists).
fn next_field_ordinal(board: &Board, fp: crate::ids::FootprintId) -> i32 {
    board
        .footprint_fields(fp)
        .fold(42, |ordinal, (_, f)| ordinal.max(f.ordinal + 1))
}

fn parented<T: BoardItem>(mut item: T, parent: ParentRef) -> T {
    item.base_mut().parent = parent;
    item
}

/// The C++ magic_enum order of `SHAPE_T` and `FIELD_T`.
const SHAPES: [ShapeT; 9] = [
    ShapeT::Undefined,
    ShapeT::Segment,
    ShapeT::Rectangle,
    ShapeT::Arc,
    ShapeT::Circle,
    ShapeT::Poly,
    ShapeT::Bezier,
    ShapeT::Ellipse,
    ShapeT::EllipseArc,
];

const FIELDS: [FieldT; 11] = [
    FieldT::User,
    FieldT::Reference,
    FieldT::Value,
    FieldT::Footprint,
    FieldT::Datasheet,
    FieldT::Description,
    FieldT::IntersheetRefs,
    FieldT::SheetName,
    FieldT::SheetFilename,
    FieldT::SheetUser,
    FieldT::SheetRefSuffix,
];

/// `DumpDefaults`: default-construct every class the C++ tool constructs. Items get a default
/// board as parent (not added to its lists); pads, fields and the footprint text get a
/// default footprint of that board (stored in the arena like the model's test fixtures,
/// `test_support.rs`, but not listed); the table cell gets a table stored the same way.
pub(crate) fn dump_defaults_into(o: &mut Out) {
    let mut board = Board::new();
    let fp = board.insert_item(Footprint::default(), ParentRef::Board);
    let fp_parent = ParentRef::Item(fp.into());
    let fp_layer = board[fp].base.layer;
    let table = board.insert_item(
        crate::pcb_table::PcbTable::with_line_width(100_000),
        ParentRef::Board,
    );
    // Values that need the board's KIID generator are built before the context borrows it.
    let fields: Vec<(FieldT, Option<PcbField>)> = FIELDS
        .iter()
        .map(|&id| {
            let f = attempt(|| {
                let mut f = PcbField::new(&mut board.kiid_generator, Some(fp_layer), id, "");

                // The C++ constructor numbers a user field with GetNextFieldOrdinal(); the Rust
                // constructor leaves that to the caller.
                if id == FieldT::User {
                    f.ordinal = next_field_ordinal(&board, fp);
                }

                f
            });
            (id, f.ok().map(|f| parented(f, fp_parent)))
        })
        .collect();
    let marker = attempt(|| {
        let rc = RcItem {
            error_code: crate::drc_api::DRCE_CLEARANCE,
            settings_key: crate::drc_api::drc_settings_key(crate::drc_api::DRCE_CLEARANCE)
                .unwrap_or_default()
                .to_string(),
            ..RcItem::default()
        };
        // PCB_MARKER( item, pos ) takes no parent.
        PcbMarker::new(
            &mut board.kiid_generator,
            Arc::new(rc),
            Vector2I::new(0, 0),
            PcbLayerId::FCu.as_i32(),
        )
    });
    // FOOTPRINT( &board ): the constructor with its mandatory fields (create_footprint).
    let footprint = attempt(|| board.create_footprint());

    let ctx = BoardCtx::bare(&board);
    let on_board = ParentRef::Board;

    {
        let track = Track::Trace(parented(PcbTrack::default(), on_board));
        dump_default_item(o, "PCB_TRACK", ctx.view(AnyItem::Track(&track)));
        let arc = Track::Arc(PcbArc {
            track: parented(PcbTrack::default(), on_board),
            ..PcbArc::default()
        });
        dump_default_item(o, "PCB_ARC", ctx.view(AnyItem::Track(&arc)));

        if let Some(via) = construct(o, "PCB_VIA", || {
            let mut v = PcbVia::default();
            v.track.base.item.parent = on_board;
            Track::Via(Box::new(v))
        }) {
            dump_default_item(o, "PCB_VIA", ctx.view(AnyItem::Track(&via)));
        }
    }

    if let Some(pad) = construct(o, "PAD", || parented(crate::pad::Pad::default(), fp_parent)) {
        dump_default_item(o, "PAD", ctx.view(AnyItem::Pad(&pad)));
    }

    match footprint {
        Ok(id) => {
            if let Some(it) = ctx.item(id.into()) {
                dump_default_item(o, "FOOTPRINT", it);
            }
        }
        Err(f) => class_pending(o, "FOOTPRINT", f),
    }

    // ZONE( &board ) takes the board's default zone settings.
    if let Some(zone) = construct(o, "ZONE", || {
        let mut kiids = kicad_core::kiid::KiidGenerator::default();
        parented(
            crate::zone::Zone::new_with_settings(
                &mut kiids,
                board.design_settings().default_zone_settings(),
                false,
            ),
            on_board,
        )
    }) {
        dump_default_item(o, "ZONE", ctx.view(AnyItem::Zone(&zone)));
    }

    for st in SHAPES {
        let class = format!("PCB_SHAPE.{}", v_enum(st));

        if let Some(shape) = construct(o, &class, || {
            parented(crate::pcb_shape::PcbShape::with_shape(st), on_board)
        }) {
            dump_default_item(o, &class, ctx.view(AnyItem::Shape(&shape)));
        }
    }

    if let Some(text) = construct(o, "PCB_TEXT", || {
        parented(crate::pcb_text::PcbText::default(), on_board)
    }) {
        dump_default_item(o, "PCB_TEXT", ctx.view(AnyItem::Text(&text)));
    }

    if let Some(text) = construct(o, "PCB_TEXT.in_footprint", || {
        parented(
            crate::pcb_text::PcbText::for_footprint(Some(fp_layer)),
            fp_parent,
        )
    }) {
        dump_default_item(o, "PCB_TEXT.in_footprint", ctx.view(AnyItem::Text(&text)));
    }

    for (id, field) in &fields {
        let class = format!("PCB_FIELD.{}", v_enum(*id));

        match field {
            Some(f) => dump_default_item(o, &class, ctx.view(AnyItem::Field(f))),
            None => o.scope("defaults", &class, |o| {
                o.fail(
                    "*",
                    super::format::Failure::Error("PCB_FIELD::PCB_FIELD panicked".into()),
                )
            }),
        }
    }

    if let Some(tb) = construct(o, "PCB_TEXTBOX", || {
        parented(crate::pcb_textbox::PcbTextbox::default(), on_board)
    }) {
        dump_default_item(o, "PCB_TEXTBOX", ctx.view(AnyItem::Textbox(&tb)));
    }

    if let Some(it) = ctx.item(table.into()) {
        dump_default_item(o, "PCB_TABLE", it);
    }

    if let Some(cell) = construct(o, "PCB_TABLECELL", || {
        parented(
            crate::pcb_tablecell::PcbTablecell::default(),
            ParentRef::Item(table.into()),
        )
    }) {
        dump_default_item(o, "PCB_TABLECELL", ctx.view(AnyItem::Tablecell(&cell)));
    }

    for (class, t) in [
        ("PCB_DIM_ALIGNED", KicadT::PcbDimAlignedT),
        ("PCB_DIM_ORTHOGONAL", KicadT::PcbDimOrthogonalT),
        ("PCB_DIM_RADIAL", KicadT::PcbDimRadialT),
        ("PCB_DIM_LEADER", KicadT::PcbDimLeaderT),
        ("PCB_DIM_CENTER", KicadT::PcbDimCenterT),
    ] {
        if let Some(d) = construct(o, class, || {
            parented(
                crate::pcb_dimension::PcbDimension::of_type(t).expect("dimension type"),
                on_board,
            )
        }) {
            dump_default_item(o, class, ctx.view(AnyItem::Dimension(&d)));
        }
    }

    if let Some(g) = construct(o, "PCB_GROUP", || {
        parented(crate::pcb_group::PcbGroup::default(), on_board)
    }) {
        dump_default_item(o, "PCB_GROUP", ctx.view(AnyItem::Group(&g)));
    }

    class_pending(
        o,
        "PCB_GENERATOR.PCB_TUNING_PATTERN",
        super::format::Failure::Pending(
            "PCB_TUNING_PATTERN::PCB_TUNING_PATTERN (no Rust API)".into(),
        ),
    );

    if let Some(t) = construct(o, "PCB_TARGET", || {
        parented(crate::pcb_target::PcbTarget::default(), on_board)
    }) {
        dump_default_item(o, "PCB_TARGET", ctx.view(AnyItem::Target(&t)));
    }

    if let Some(p) = construct(o, "PCB_POINT", || {
        parented(crate::pcb_point::PcbPoint::default(), on_board)
    }) {
        dump_default_item(o, "PCB_POINT", ctx.view(AnyItem::Point(&p)));
    }

    if let Some(b) = construct(o, "PCB_BARCODE", || {
        parented(crate::pcb_barcode::PcbBarcode::default(), on_board)
    }) {
        dump_default_item(o, "PCB_BARCODE", ctx.view(AnyItem::Barcode(&b)));
    }

    if let Some(r) = construct(o, "PCB_REFERENCE_IMAGE", || {
        parented(
            crate::pcb_reference_image::PcbReferenceImage::default(),
            on_board,
        )
    }) {
        dump_default_item(o, "PCB_REFERENCE_IMAGE", ctx.view(AnyItem::Image(&r)));
    }

    match &marker {
        Ok(m) => dump_default_item(o, "PCB_MARKER", ctx.view(AnyItem::Marker(m))),
        Err(_) => class_pending(
            o,
            "PCB_MARKER",
            super::format::Failure::Error("PCB_MARKER::PCB_MARKER panicked".into()),
        ),
    }

    o.scope("defaults", "NETINFO_ITEM", |o| {
        // NETINFO_ITEM( &board ): empty name, code -1.
        let net = NetinfoItem::with_name("", NetCode(-1));
        emit!(o, "netcode", v_int(net.net_code().0));
        emit!(o, "netname", v_str(net.netname()));
        emit!(o, "short_netname", v_str(net.short_netname()));
        emit!(
            o,
            "netclass",
            net.netclass()
                .map_or_else(|| "null".to_string(), |n| v_str(&n.get_name()))
        );
        emit!(o, "net_chain", v_str(net.net_chain()));
    });

    o.scope("defaults", "NETCLASS", |o| {
        if let Some(nc) = o.cond("*", || Netclass::new("Default", true)) {
            settings::dump_netclass(o, "", &nc);
        }
    });

    o.scope("defaults", "BOARD_DESIGN_SETTINGS", |o| {
        if let Some(bds) = o.cond("*", BoardDesignSettings::default) {
            settings::dump_design_settings(o, "", &bds);
        }
    });

    o.scope("defaults", "BOARD_STACKUP", |o| {
        settings::dump_stackup(o, "", &BoardStackup::default());
    });

    o.scope("defaults", "BOARD_STACKUP.default_list", |o| {
        let r = attempt(|| {
            let mut s = BoardStackup::default();
            s.build_default_stackup_list(Some(board.design_settings()), 2);
            s
        });

        match r {
            Ok(s) => settings::dump_stackup(o, "", &s),
            Err(f) => o.fail("*", f),
        }
    });

    o.scope("defaults", "PCB_PLOT_PARAMS", |o| {
        settings::dump_plot_params(o, "", &PcbPlotParams::default());
    });

    o.scope("defaults", "TEARDROP_PARAMETERS", |o| {
        settings::dump_teardrop_params(o, "", &TeardropParameters::default());
    });

    o.scope("defaults", "ZONE_SETTINGS", |o| {
        settings::dump_zone_settings(o, "", &ZoneSettings::default());
    });

    // The board's own items are not listed, so the walk is empty (as in C++).
    o.scope("defaults", "BOARD", |o| {
        super::format::init_item_keys(o, &board);
        settings::dump_board_section(o, &ctx);
    });
}
