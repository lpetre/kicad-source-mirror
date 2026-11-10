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

//! Port of `pcbnew/drc/drc_test_provider.h` / `drc_test_provider.cpp`: the provider base class
//! (`DRC_TEST_PROVIDER`), the standard item type lists, the canonical board walk
//! (`forEachGeometryItem`), the violation reporting helpers, the message formatting and the
//! provider registration order.
//!
//! The C++ base class reaches the engine through `m_drcEngine`. The port splits the engine
//! (`board-model.md` section 7): the rules are behind `BoardCtx::rules`, and everything a run
//! mutates (violations, error limits, progress, the log reporter) is behind the run host
//! [`DrcRunHost`], which the engine's run state implements (package DRC-03). A provider keeps
//! a [`DrcTestProviderBase`] for the members of the C++ base class.

use std::sync::{Arc, LazyLock};

use kicad_core::Severity;
use kicad_core::base_units::PCB_IU_SCALE;
use kicad_core::eda_units::{EdaDataType, EdaUnits};
use kicad_core::layer_ids::{Flashing, PcbLayerId};
use kicad_core::lset::Lset;
use kicad_core::typeinfo::KicadT;
use kicad_core::units_provider::UnitsProvider;
use kicad_geom::geometry::shape::Shape;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::pcb_marker::{PcbMarker, RcItem};
use kicad_pcb::pcb_shape::PcbShape;
use kicad_pcb::{AnyItem, BoardCtx, EdaTextItem, ItemRef};

use crate::constraint_types::DrcConstraintT;
use crate::reporter::Reporter;

/// The engine side of a provider run: the `DRC_ENGINE` members the provider base class calls.
/// Implemented by the engine's run state (DRC-03); tests use a recording implementation.
pub trait DrcRunHost {
    /// Port of `DRC_ENGINE::ReportViolation( aItem, aPos, aMarkerLayer, aPathGenerator )`.
    /// `violating_test` is the name of the provider (C++: `aItem->GetViolatingTest()`, used in
    /// the log line). `path_generator` is called with the marker the violation handler creates.
    fn report_violation(
        &mut self,
        item: Arc<RcItem>,
        violating_test: &str,
        marker_pos: Vector2I,
        marker_layer: i32,
        path_generator: &dyn Fn(&mut PcbMarker),
    );

    /// Port of `DRC_ENGINE::ReportProgress`: false when the run was cancelled. No progress
    /// reporter by default (always true).
    fn report_progress(&mut self, _progress: f64) -> bool {
        true
    }

    /// Port of `DRC_ENGINE::ReportPhase`: false when the run was cancelled. No progress
    /// reporter by default (always true).
    fn report_phase(&mut self, _message: &str) -> bool {
        true
    }

    /// Port of `DRC_ENGINE::GetLogReporter`.
    fn log_reporter(&mut self) -> Option<&mut dyn Reporter> {
        None
    }
}

/// The path generator of `reportViolation`'s default argument (`[]( PCB_MARKER* ){}`).
pub fn no_path(_marker: &mut PcbMarker) {}

/// Port of `DRC_TEST_PROVIDER::s_allBasicItems` (filled by `DRC_TEST_PROVIDER::Init()`): every
/// `KICAD_T` below `MAX_STRUCT_TYPE_ID` except `PCB_FOOTPRINT_T` and `PCB_GROUP_T`, in value
/// order (non-compound board geometry items).
pub fn all_basic_items() -> &'static [KicadT] {
    static ITEMS: LazyLock<Vec<KicadT>> = LazyLock::new(|| basic_items(false));
    &ITEMS
}

/// Port of `DRC_TEST_PROVIDER::s_allBasicItemsButZones`: [`all_basic_items`] without
/// `PCB_ZONE_T`.
pub fn all_basic_items_but_zones() -> &'static [KicadT] {
    static ITEMS: LazyLock<Vec<KicadT>> = LazyLock::new(|| basic_items(true));
    &ITEMS
}

/// The loop of `DRC_TEST_PROVIDER::Init()`.
fn basic_items(but_zones: bool) -> Vec<KicadT> {
    (0..KicadT::MaxStructTypeId as i32)
        .filter_map(KicadT::from_i32)
        .filter(|&t| t != KicadT::PcbFootprintT && t != KicadT::PcbGroupT)
        .filter(|&t| !but_zones || t != KicadT::PcbZoneT)
        .collect()
}

/// One registered test provider: the module of this crate that ports it and its
/// `GetName()`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderRegistration {
    /// The `drc_test_provider_<module>.cpp` file and the `providers::<module>` module.
    pub module: &'static str,
    /// `DRC_TEST_PROVIDER::GetName()` of the provider.
    pub name: &'static str,
}

/// `DRC_TEST_PROVIDER_REGISTRY::Instance().GetTestProviders()`: the providers in registration
/// order, which is the run order of `DRC_ENGINE::RunTests`. The C++ registers them from static
/// initialisers, so the order is the link order of `PCBNEW_DRC_SRCS`; this table was confirmed
/// against the reference build (`tests/golden/rtree_oracle.txt`, `registry|` lines).
/// `schematic_parity` is not ported (it needs a schematic).
pub const DRC_TEST_PROVIDERS: &[ProviderRegistration] = &[
    ProviderRegistration {
        module: "annular_width",
        name: "annular_width",
    },
    ProviderRegistration {
        module: "disallow",
        name: "disallow",
    },
    ProviderRegistration {
        module: "creepage",
        name: "creepage",
    },
    ProviderRegistration {
        module: "connectivity",
        name: "connectivity",
    },
    ProviderRegistration {
        module: "connection_width",
        name: "copper width",
    },
    ProviderRegistration {
        module: "copper_clearance",
        name: "clearance",
    },
    ProviderRegistration {
        module: "physical_clearance",
        name: "physical_clearance",
    },
    ProviderRegistration {
        module: "courtyard_clearance",
        name: "courtyard_clearance",
    },
    ProviderRegistration {
        module: "edge_clearance",
        name: "edge_clearance",
    },
    ProviderRegistration {
        module: "footprint_checks",
        name: "footprint checks",
    },
    ProviderRegistration {
        module: "hole_to_hole",
        name: "hole_to_hole_clearance",
    },
    ProviderRegistration {
        module: "hole_size",
        name: "hole_size",
    },
    ProviderRegistration {
        module: "library_parity",
        name: "library_parity",
    },
    ProviderRegistration {
        module: "schematic_parity",
        name: "schematic_parity",
    },
    ProviderRegistration {
        module: "misc",
        name: "miscellaneous",
    },
    ProviderRegistration {
        module: "net_chain_tuning_profiles",
        name: "net chain tuning profiles",
    },
    ProviderRegistration {
        module: "text_dims",
        name: "text_dimensions",
    },
    ProviderRegistration {
        module: "track_angle",
        name: "track angle",
    },
    ProviderRegistration {
        module: "track_width",
        name: "width",
    },
    ProviderRegistration {
        module: "track_segment_length",
        name: "segment_length",
    },
    ProviderRegistration {
        module: "zone_connections",
        name: "zone connections",
    },
    ProviderRegistration {
        module: "via_diameter",
        name: "diameter",
    },
    ProviderRegistration {
        module: "via_stack",
        name: "microvia_stack",
    },
    ProviderRegistration {
        module: "via_stitch",
        name: "via_stitch_overlap",
    },
    ProviderRegistration {
        module: "solder_mask",
        name: "solder_mask_issues",
    },
    ProviderRegistration {
        module: "silk_clearance",
        name: "silk_clearance",
    },
    ProviderRegistration {
        module: "matched_length",
        name: "length",
    },
    ProviderRegistration {
        module: "diff_pair_coupling",
        name: "diff_pair_coupling",
    },
    ProviderRegistration {
        module: "sliver_checker",
        name: "sliver checker",
    },
    ProviderRegistration {
        module: "text_mirroring",
        name: "text_mirroring",
    },
];

/// The name of the base class (`DRC_TEST_PROVIDER::GetName()`).
pub const NO_NAME_TEST: &str = "<no name test>";

/// Port of the data members of `DRC_TEST_PROVIDER` (and its `UNITS_PROVIDER` base) and of its
/// non-virtual helpers. `m_drcEngine` / `m_board` are not members: the helpers take the run
/// host and the board context as arguments.
#[derive(Clone, Debug)]
pub struct DrcTestProviderBase {
    /// `GetName()` of the provider (C++: a virtual).
    pub name: &'static str,
    /// `UNITS_PROVIDER::m_userUnits` (set by `RunTests( aUnits )`).
    pub user_units: EdaUnits,
    /// `m_isRuleDriven`
    pub is_rule_driven: bool,
}

impl Default for DrcTestProviderBase {
    /// `DRC_TEST_PROVIDER()`: `UNITS_PROVIDER( pcbIUScale, EDA_UNITS::MM )`.
    fn default() -> Self {
        DrcTestProviderBase {
            name: NO_NAME_TEST,
            user_units: EdaUnits::Mm,
            is_rule_driven: true,
        }
    }
}

impl DrcTestProviderBase {
    /// A base for the provider called `name`.
    pub fn new(name: &'static str) -> Self {
        DrcTestProviderBase {
            name,
            ..Self::default()
        }
    }

    /// `SetUserUnits()` (the first half of `DRC_TEST_PROVIDER::RunTests( aUnits )`).
    pub fn set_user_units(&mut self, units: EdaUnits) {
        self.user_units = units;
    }

    /// The `UNITS_PROVIDER` base.
    pub fn units_provider(&self) -> UnitsProvider<'static> {
        UnitsProvider::new(&PCB_IU_SCALE, self.user_units)
    }

    /// Port of `reportViolation`: sets the violating test and forwards to the engine.
    pub fn report_violation(
        &self,
        host: &mut dyn DrcRunHost,
        item: Arc<RcItem>,
        marker_pos: Vector2I,
        marker_layer: i32,
        path_generator: &dyn Fn(&mut PcbMarker),
    ) {
        host.report_violation(item, self.name, marker_pos, marker_layer, path_generator);
    }

    /// Port of `reportTwoPointGeometry`: the marker path is the one segment `pt_a` -> `pt_b`.
    pub fn report_two_point_geometry(
        &self,
        host: &mut dyn DrcRunHost,
        drc_item: Arc<RcItem>,
        marker_pos: Vector2I,
        pt_a: Vector2I,
        pt_b: Vector2I,
        layer: PcbLayerId,
    ) {
        // PCB_SHAPE ptAShape( nullptr, SHAPE_T::SEGMENT )
        let mut pt_a_shape = PcbShape::default();
        pt_a_shape.shape.set_start(pt_a);
        pt_a_shape.shape.set_end(pt_b);

        self.report_violation(
            host,
            drc_item,
            marker_pos,
            layer.0,
            &|marker: &mut PcbMarker| {
                marker.set_path(vec![pt_a_shape.clone()], pt_a, pt_b);
            },
        );
    }

    /// Port of `reportTwoShapeGeometry`: distance 0 gives a degenerate path at the marker;
    /// otherwise the path joins the nearest points of the two shapes, if they have some.
    #[allow(clippy::too_many_arguments)]
    pub fn report_two_shape_geometry(
        &self,
        host: &mut dyn DrcRunHost,
        drc_item: Arc<RcItem>,
        marker_pos: Vector2I,
        shape1: &dyn Shape,
        shape2: &dyn Shape,
        layer: PcbLayerId,
        distance: i32,
    ) {
        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();

        if distance == 0 {
            self.report_two_point_geometry(
                host, drc_item, marker_pos, marker_pos, marker_pos, layer,
            );
        } else if shape1.nearest_points(shape2, &mut pt_a, &mut pt_b) {
            self.report_two_point_geometry(host, drc_item, marker_pos, pt_a, pt_b, layer);
        } else {
            self.report_violation(host, drc_item, marker_pos, layer.0, &no_path);
        }
    }

    /// Port of `reportTwoItemGeometry`: [`Self::report_two_shape_geometry`] with the effective
    /// shapes of the two items on `layer`.
    #[allow(clippy::too_many_arguments)]
    pub fn report_two_item_geometry(
        &self,
        host: &mut dyn DrcRunHost,
        drc_item: Arc<RcItem>,
        marker_pos: Vector2I,
        item1: ItemRef<'_>,
        item2: ItemRef<'_>,
        layer: PcbLayerId,
        distance: i32,
    ) {
        let shape1 =
            item1.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);
        let shape2 =
            item2.effective_shape(layer, Flashing::Default, DrcConstraintT::NullConstraint);

        self.report_two_shape_geometry(
            host,
            drc_item,
            marker_pos,
            shape1.as_ref(),
            shape2.as_ref(),
            layer,
            distance,
        );
    }

    /// Port of `reportProgress( aCount, aSize, aDelta = 1 )`.
    pub fn report_progress(
        &self,
        host: &mut dyn DrcRunHost,
        count: usize,
        size: usize,
        delta: usize,
    ) -> bool {
        // size_t arithmetic: aSize - 1 wraps for an empty set, like the C++
        if count % delta == 0 || count == size.wrapping_sub(1) {
            if !host.report_progress(count as f64 / size as f64) {
                return false;
            }
        }

        true
    }

    /// Port of `reportPhase`: logs the message (`REPORT_AUX`) and forwards it.
    pub fn report_phase(&self, host: &mut dyn DrcRunHost, stage_name: &str) -> bool {
        if let Some(reporter) = host.log_reporter() {
            reporter.report(stage_name, Severity::Info);
        }

        host.report_phase(stage_name)
    }

    /// Port of `formatMsg( aFormatString, aSource, aConstraint, aActual, aDataType )`: both
    /// values with `MessageTextFromValue( value, true, type )`; if the two strings are equal both
    /// are formatted again with the more precise `StringFromValue( value, true, type )`.
    pub fn format_msg(
        &self,
        format_string: &str,
        source: &str,
        constraint: f64,
        actual: f64,
        data_type: EdaDataType,
    ) -> String {
        let (constraint_str, actual_str) = self.format_values(constraint, actual, data_type);
        wx_format_s(format_string, &[source, &constraint_str, &actual_str])
    }

    /// Port of `formatMsg( aFormatString, aSource, aConstraint, aActual, aAdditionalParam,
    /// aDataType )`.
    pub fn format_msg_extra(
        &self,
        format_string: &str,
        source: &str,
        constraint: f64,
        actual: f64,
        additional_param: &str,
        data_type: EdaDataType,
    ) -> String {
        let (constraint_str, actual_str) = self.format_values(constraint, actual, data_type);
        wx_format_s(
            format_string,
            &[source, &constraint_str, &actual_str, additional_param],
        )
    }

    /// Port of `formatMsg( aFormatString, aSource, const EDA_ANGLE& aConstraint,
    /// const EDA_ANGLE& aActual )` (angles in degrees).
    pub fn format_msg_angle(
        &self,
        format_string: &str,
        source: &str,
        constraint_degrees: f64,
        actual_degrees: f64,
    ) -> String {
        let units = self.units_provider();
        let mut constraint_str = units.message_text_from_angle(constraint_degrees, true);
        let mut actual_str = units.message_text_from_angle(actual_degrees, true);

        if constraint_str == actual_str {
            // Use more precise formatting if the message-text strings were equal.
            constraint_str = units.string_from_angle(constraint_degrees, true);
            actual_str = units.string_from_angle(actual_degrees, true);
        }

        wx_format_s(format_string, &[source, &constraint_str, &actual_str])
    }

    /// The value part shared by the two distance overloads of `formatMsg`.
    fn format_values(
        &self,
        constraint: f64,
        actual: f64,
        data_type: EdaDataType,
    ) -> (String, String) {
        let units = self.units_provider();
        let mut constraint_str = units.message_text_from_value(constraint, true, data_type);
        let mut actual_str = units.message_text_from_value(actual, true, data_type);

        if constraint_str == actual_str {
            // Use more precise formatting if the message-text strings were equal.
            constraint_str = units.string_from_value(constraint, true, data_type);
            actual_str = units.string_from_value(actual, true, data_type);
        }

        (constraint_str, actual_str)
    }
}

/// `wxString::Format` for format strings whose only conversions are `%s` (all `formatMsg`
/// format strings): each `%s` takes the next argument, `%%` is a literal `%`. A `%s` without
/// an argument left is kept literally.
pub fn wx_format_s(format_string: &str, args: &[&str]) -> String {
    let mut out = String::with_capacity(format_string.len() + 32);
    let mut args = args.iter();
    let mut chars = format_string.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            match chars.peek() {
                Some('s') => {
                    chars.next();

                    match args.next() {
                        Some(a) => out.push_str(a),
                        None => out.push_str("%s"),
                    }

                    continue;
                }
                Some('%') => {
                    chars.next();
                    out.push('%');
                    continue;
                }
                _ => {}
            }
        }

        out.push(c);
    }

    out
}

/// Port of `DRC_TEST_PROVIDER::isInvisibleText`: a `PCB_FIELD` that is not visible.
pub fn is_invisible_text(item: ItemRef<'_>) -> bool {
    match item.any() {
        AnyItem::Field(field) => !field.eda_text().is_visible(),
        _ => false,
    }
}

/// `typeMask[ aType ]`: the bitset of `forEachGeometryItem` (an empty type list means all
/// types).
struct TypeMask([bool; KicadT::MaxStructTypeId as usize]);

impl TypeMask {
    fn new(types: &[KicadT]) -> Self {
        let mut mask = [types.is_empty(); KicadT::MaxStructTypeId as usize];

        for &t in types {
            // std::bitset::operator[] on an out-of-range type is undefined in C++; the type
            // lists in use only hold item types.
            if let Some(bit) = mask.get_mut(t as i32 as usize) {
                *bit = true;
            }
        }

        TypeMask(mask)
    }

    fn get(&self, t: KicadT) -> bool {
        self.0.get(t as i32 as usize).copied().unwrap_or(false)
    }
}

/// Port of `DRC_TEST_PROVIDER::forEachGeometryItem( aTypes, aLayers, aFunc )`: the canonical
/// board walk. Returns the number of items handed to `func`.
///
/// Order: board tracks (the callback's return value is ignored here), board drawings
/// (dimensions, shapes, texts, textboxes, targets, tables then their cells, barcodes), board
/// zones, then per footprint: fields, pads (a pad with a hole regardless of the layers),
/// graphical items, zones and the footprint itself. From the drawings on, `func` returning
/// false ends the walk. An empty type list means all types.
pub fn for_each_geometry_item<'c>(
    ctx: &'c BoardCtx<'c>,
    types: &[KicadT],
    layers: &Lset,
    mut func: impl FnMut(ItemRef<'c>) -> bool,
) -> i32 {
    let brd = ctx.board;
    let type_mask = TypeMask::new(types);
    let mut n = 0;

    let on_layers = |item: &ItemRef<'c>| (&item.layer_set() & layers).any();

    for (_, track) in brd.tracks() {
        let item = ctx.view(AnyItem::Track(track));

        if on_layers(&item) {
            let t = item.type_();

            if (type_mask.get(KicadT::PcbTraceT) && t == KicadT::PcbTraceT)
                || (type_mask.get(KicadT::PcbViaT) && t == KicadT::PcbViaT)
                || (type_mask.get(KicadT::PcbArcT) && t == KicadT::PcbArcT)
            {
                func(item);
                n += 1;
            }
        }
    }

    // The drawing branches shared by the board and the footprints (`with_targets`: only the
    // board loop tests PCB_TARGET_T). Returns false when the walk ends.
    let visit_drawing = |item: ItemRef<'c>,
                         with_targets: bool,
                         func: &mut dyn FnMut(ItemRef<'c>) -> bool,
                         n: &mut i32|
     -> bool {
        if !on_layers(&item) {
            return true;
        }

        let t = item.type_();

        if (type_mask.get(KicadT::PcbDimensionT) && t.base_type() == KicadT::PcbDimensionT)
            || (type_mask.get(KicadT::PcbShapeT) && t == KicadT::PcbShapeT)
            || (type_mask.get(KicadT::PcbTextT) && t == KicadT::PcbTextT)
            || (type_mask.get(KicadT::PcbTextboxT) && t == KicadT::PcbTextboxT)
            || (with_targets && type_mask.get(KicadT::PcbTargetT) && t == KicadT::PcbTargetT)
        {
            if !func(item) {
                return false;
            }

            *n += 1;
        } else if t.base_type() == KicadT::PcbTableT {
            // A drill chart is a table for clearance purposes, so a PCB_TABLE_T mask
            // has to reach one or its artwork escapes DRC entirely
            if type_mask.get(KicadT::PcbTableT) {
                if !func(item) {
                    return false;
                }

                *n += 1;
            }

            if type_mask.get(KicadT::PcbTablecellT)
                && let Some(table_id) = item.id().and_then(|id| id.typed())
            {
                for (_, cell) in ctx.board.table_cells(table_id) {
                    if !func(ctx.view(AnyItem::Tablecell(cell))) {
                        return false;
                    }

                    *n += 1;
                }
            }
        } else if type_mask.get(KicadT::PcbBarcodeT) && t == KicadT::PcbBarcodeT {
            if !func(item) {
                return false;
            }

            *n += 1;
        }

        true
    };

    for (_, drawing) in brd.drawings() {
        if !visit_drawing(ctx.view(drawing), true, &mut func, &mut n) {
            return n;
        }
    }

    if type_mask.get(KicadT::PcbZoneT) {
        for (_, zone) in brd.zones() {
            let item = ctx.view(AnyItem::Zone(zone));

            if on_layers(&item) {
                if !func(item) {
                    return n;
                }

                n += 1;
            }
        }
    }

    for (fp_id, footprint) in brd.footprints() {
        if type_mask.get(KicadT::PcbFieldT) {
            for (_, field) in brd.footprint_fields(fp_id) {
                let item = ctx.view(AnyItem::Field(field));

                if on_layers(&item) {
                    if !func(item) {
                        return n;
                    }

                    n += 1;
                }
            }
        }

        if type_mask.get(KicadT::PcbPadT) {
            for (_, pad) in brd.footprint_pads(fp_id) {
                let item = ctx.view(AnyItem::Pad(pad));

                // Careful: if a pad has a hole then it pierces all layers
                if item.has_hole() || on_layers(&item) {
                    if !func(item) {
                        return n;
                    }

                    n += 1;
                }
            }
        }

        for (_, dwg) in brd.footprint_drawings(fp_id) {
            // The footprint loop has no PCB_TARGET_T branch.
            if !visit_drawing(ctx.view(dwg), false, &mut func, &mut n) {
                return n;
            }
        }

        if type_mask.get(KicadT::PcbZoneT) {
            for (_, zone) in brd.footprint_zones(fp_id) {
                let item = ctx.view(AnyItem::Zone(zone));

                if on_layers(&item) {
                    if !func(item) {
                        return n;
                    }

                    n += 1;
                }
            }
        }

        if type_mask.get(KicadT::PcbFootprintT) {
            if !func(ctx.view(AnyItem::Footprint(footprint))) {
                return n;
            }

            n += 1;
        }
    }

    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use kicad_geom::geometry::shape_segment::ShapeSegment;

    /// Records what the provider base hands to the engine.
    #[derive(Default)]
    struct Recorder {
        violations: Vec<(String, Vector2I, i32, Vec<Vector2I>)>,
        progress: Vec<f64>,
        phases: Vec<String>,
        log: crate::reporter::WxStringReporter,
    }

    impl DrcRunHost for Recorder {
        fn report_violation(
            &mut self,
            _item: Arc<RcItem>,
            violating_test: &str,
            marker_pos: Vector2I,
            marker_layer: i32,
            path_generator: &dyn Fn(&mut PcbMarker),
        ) {
            let mut marker = PcbMarker::default();
            path_generator(&mut marker);
            let mut path = Vec::new();

            if !marker.path().is_empty() {
                path.push(marker.path_start);
                path.push(marker.path_end);
                path.push(marker.path()[0].shape.start());
                path.push(marker.path()[0].shape.end());
            }

            self.violations
                .push((violating_test.to_string(), marker_pos, marker_layer, path));
        }

        fn report_progress(&mut self, progress: f64) -> bool {
            self.progress.push(progress);
            true
        }

        fn report_phase(&mut self, message: &str) -> bool {
            self.phases.push(message.to_string());
            true
        }

        fn log_reporter(&mut self) -> Option<&mut dyn Reporter> {
            Some(&mut self.log)
        }
    }

    #[test]
    fn wx_format_s_substitutes_in_order() {
        assert_eq!(
            wx_format_s("(%s clearance %s; actual %s)", &["a", "b", "c"]),
            "(a clearance b; actual c)"
        );
        assert_eq!(wx_format_s("100%% %s", &["x"]), "100% x");
        assert_eq!(wx_format_s("%s %s", &["x"]), "x %s");
        assert_eq!(wx_format_s("%d", &["x"]), "%d");
    }

    #[test]
    fn two_shape_geometry_paths() {
        let base = DrcTestProviderBase::new("clearance");
        let mut host = Recorder::default();
        let item = Arc::new(RcItem::default());
        let a = ShapeSegment::new(Vector2I::new(0, 0), Vector2I::new(100, 0), 10);
        let b = ShapeSegment::new(Vector2I::new(50, 40), Vector2I::new(50, 90), 10);
        let pos = Vector2I::new(7, 8);

        // distance 0: a degenerate path at the marker
        base.report_two_shape_geometry(&mut host, item.clone(), pos, &a, &b, PcbLayerId::FCu, 0);
        // otherwise the nearest points
        base.report_two_shape_geometry(&mut host, item, pos, &a, &b, PcbLayerId::BCu, 30);

        assert_eq!(host.violations[0].0, "clearance");
        assert_eq!(host.violations[0].2, PcbLayerId::FCu.0);
        assert_eq!(host.violations[0].3, vec![pos; 4]);

        let mut pt_a = Vector2I::default();
        let mut pt_b = Vector2I::default();
        assert!(a.nearest_points(&b, &mut pt_a, &mut pt_b));
        assert_eq!(host.violations[1].2, PcbLayerId::BCu.0);
        assert_eq!(host.violations[1].3, vec![pt_a, pt_b, pt_a, pt_b]);
    }

    #[test]
    fn progress_and_phase() {
        let base = DrcTestProviderBase::default();
        let mut host = Recorder::default();

        for i in 0..10 {
            assert!(base.report_progress(&mut host, i, 10, 4));
        }

        // every 4th, and the last one
        assert_eq!(host.progress, vec![0.0, 0.4, 0.8, 0.9]);

        assert!(base.report_phase(&mut host, "Checking track widths..."));
        assert_eq!(host.phases, vec!["Checking track widths...".to_string()]);
        assert!(host.log.has_message());
    }

    #[test]
    fn registration_table_names_every_provider_module() {
        assert_eq!(DRC_TEST_PROVIDERS.len(), 30);
        assert_eq!(DRC_TEST_PROVIDERS[0].name, "annular_width");
        assert_eq!(DRC_TEST_PROVIDERS[29].module, "text_mirroring");
    }
}
