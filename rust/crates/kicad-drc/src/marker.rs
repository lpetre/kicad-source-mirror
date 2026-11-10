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

//! Port of the DRC-side functions of `pcbnew/pcb_marker.cpp` (work package DRC-11):
//! `PCB_MARKER::ToProto`, `PCB_MARKER::FromProto` (the identity and storage form of an
//! exclusion) and `PCB_MARKER::FromLegacyString` (exclusions of project files before design
//! settings schema 3), with `wxSplit` and the `strtol` position parsing they use.
//!
//! The marker type itself (`PCB_MARKER`, `MARKER_BASE`: constructor, severity, exclusion flag)
//! is `kicad_pcb::pcb_marker::PcbMarker`. The kiapi messages are in [`proto`].

pub mod proto;

use std::sync::Arc;

use kicad_core::layer_ids::{LAYER_DRAWINGSHEET, PCB_LAYER_ID_COUNT, layer_name};
use kicad_core::{Kiid, KiidGenerator, NILUUID, PCB_IU_SCALE, PcbLayerId};
use kicad_geom::{Vector2I, ki_round};
use kicad_pcb::ctx::BoardCtx;
use kicad_pcb::drc_api::*;
use kicad_pcb::pcb_marker::{MarkerT, PcbMarker};

use crate::item;
use crate::rc_item::strip_markup;
use proto::{
    DrcMarkerMsg, ProtoVector2, board_layer_from_proto, board_layer_to_proto,
    drc_error_type_from_proto, drc_error_type_to_proto, marker_type_from_proto,
    marker_type_to_proto, rule_severity_to_proto,
};

/// Port of `PCB_MARKER::ToProto( kiapi::board::DrcMarker&, const PCB_MARKER& )`.
///
/// `board` is `aMarker.GetBoard()`: with a board the message carries the marker's severity
/// and `IsTreatedAsExcluded()`; without one, only an excluded marker sets them (exclusion).
pub fn to_proto(marker: &PcbMarker, board: Option<&BoardCtx<'_>>) -> DrcMarkerMsg {
    let drc = marker.rc_item();
    let mut msg = DrcMarkerMsg {
        error_type: drc_error_type_to_proto(drc.error_code()),
        // kiapi::common::PackVector2( *aMsg.mutable_position(), aMarker.GetPos() )
        position: Some(ProtoVector2 {
            x_nm: PCB_IU_SCALE.iu_to_nm(marker.pos().x),
            y_nm: PCB_IU_SCALE.iu_to_nm(marker.pos().y),
        }),
        ..DrcMarkerMsg::default()
    };

    for id in drc.ids() {
        if *id != NILUUID {
            msg.items.push(id.as_string());
        }
    }

    msg.marker_type = marker_type_to_proto(marker.marker_type());

    // Only store the layer for error types where it affects identity.
    let store_layer = matches!(
        drc.error_code(),
        DRCE_COPPER_SLIVER
            | DRCE_GENERIC_WARNING
            | DRCE_GENERIC_ERROR
            | DRCE_UNCONNECTED_ITEMS
            | DRCE_STARVED_THERMAL
    );

    if store_layer {
        let mut layer = marker.base.layer;

        if layer == PcbLayerId::UndefinedLayer {
            layer = PcbLayerId::FCu;
        }

        msg.layer = board_layer_to_proto(layer);
    }

    if let Some(rule) = drc.violating_rule() {
        msg.violating_rule = rule.name.clone();
    }

    msg.description = strip_markup(&drc.error_message());

    if let Some(ctx) = board {
        msg.severity = rule_severity_to_proto(marker.severity(ctx));
        msg.excluded = marker.is_treated_as_excluded(ctx);
    } else if marker.is_excluded() {
        msg.severity = rule_severity_to_proto(kicad_core::Severity::Exclusion);
        msg.excluded = true;
    }

    msg
}

/// Port of `PCB_MARKER::FromProto( const kiapi::board::DrcMarker& )`: `None` when the error
/// type has no `DRC_ITEM`. The new marker gets a fresh UUID from `kiid_generator`; item ids
/// that are not UUIDs get random ones (`KIID( const std::string& )`).
pub fn from_proto(msg: &DrcMarkerMsg, kiid_generator: &mut KiidGenerator) -> Option<PcbMarker> {
    let code = drc_error_type_from_proto(msg.error_type);

    let drc_item = item::create(code)?;

    let position = msg.position.unwrap_or_default();
    // kiapi::common::UnpackVector2: KiROUND( NmToIUD( nm ) )
    let pos = Vector2I::new(
        ki_round(PCB_IU_SCALE.nm_to_iu_d(position.x_nm)),
        ki_round(PCB_IU_SCALE.nm_to_iu_d(position.y_nm)),
    );
    let mut layer = board_layer_from_proto(msg.layer).0;

    // MarkerType will be set by the c'tor, based on the PCB_DRC_CODE and the layer.  Since the
    // LAYER_DRAWINGSHEET can't be represented in a PCB_LAYER_ID (it's a GAL_LAYER_ID), we have
    // to give that one a little bit of help.
    if marker_type_from_proto(msg.marker_type) == MarkerT::DrawingSheet {
        layer = LAYER_DRAWINGSHEET;
    }

    let mut marker = PcbMarker::new(kiid_generator, Arc::new(drc_item), pos, layer);

    let mut id = |index: usize| -> Kiid {
        match msg.items.get(index) {
            Some(value) => Kiid::from_string_or_random(value, kiid_generator),
            None => NILUUID,
        }
    };

    let (main_id, aux_id, aux2_id, aux3_id) = (id(0), id(1), id(2), id(3));
    Arc::make_mut(&mut marker.rc_item).set_items(main_id, aux_id, aux2_id, aux3_id);

    Some(marker)
}

/// Port of `wxSplit( str, sep, escape = '\\' )` (wxWidgets 3.2 `arrstr.cpp`): split at `sep`
/// unless it follows `escape`, in which case the escape character is replaced by the
/// separator. An empty string gives no tokens; otherwise the last token is always added.
pub fn wx_split(text: &str, sep: char, escape: char) -> Vec<String> {
    let mut ret = Vec::new();

    if escape == '\0' {
        // wxStringTokenize( str, sep, wxTOKEN_RET_EMPTY_ALL )
        if text.is_empty() {
            return ret;
        }

        return text.split(sep).map(str::to_string).collect();
    }

    let mut curr = String::new();
    let mut prev = '\0';

    for ch in text.chars() {
        if ch == sep {
            if prev == escape {
                // remove the escape character and don't consider this occurrence of 'sep' as
                // a real separator
                curr.pop();
                curr.push(sep);
            } else {
                // real separator
                ret.push(std::mem::take(&mut curr));
            }
        } else {
            // normal character
            curr.push(ch);
        }

        prev = ch;
    }

    // add the last token, which we always have unless the string is empty
    if !text.is_empty() {
        ret.push(curr);
    }

    ret
}

/// `(int) strtol( aText, nullptr, 10 )` (C locale): leading white space, an optional sign,
/// decimal digits up to the first other character; 0 without digits; `LONG_MAX` / `LONG_MIN`
/// on overflow; the `long` to `int` conversion keeps the low 32 bits (g++).
fn strtol10_as_int(text: &str) -> i32 {
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r') {
        i += 1;
    }

    let mut negative = false;

    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        negative = bytes[i] == b'-';
        i += 1;
    }

    let mut value: i64 = 0;
    let mut overflow = false;

    while i < bytes.len() && bytes[i].is_ascii_digit() {
        let d = i64::from(bytes[i] - b'0');

        if !overflow {
            match value.checked_mul(10).and_then(|v| {
                if negative {
                    v.checked_sub(d)
                } else {
                    v.checked_add(d)
                }
            }) {
                Some(v) => value = v,
                None => overflow = true,
            }
        }

        i += 1;
    }

    if overflow {
        value = if negative { i64::MIN } else { i64::MAX };
    }

    value as i32
}

/// Port of `PCB_MARKER::FromLegacyString( const wxString& aData )`: the pipe-separated
/// exclusion form `key|x|y|uuid1|uuid2[|...]` of design settings before schema 3. `None` for
/// truncated strings and unknown settings keys.
pub fn from_legacy_string(data: &str, kiid_generator: &mut KiidGenerator) -> Option<PcbMarker> {
    let get_marker_layer = |layer_name_text: &str| -> i32 {
        for layer in 0..PCB_LAYER_ID_COUNT {
            if layer_name(PcbLayerId(layer)) == layer_name_text {
                return layer;
            }
        }

        PcbLayerId::FCu.0
    };

    let props = wx_split(data, '|', '\\');

    // Every legacy marker has a settings key, position and at least two item fields.
    // Reject truncated settings rather than indexing past wxArrayString below.
    if props.len() < 5 {
        return None;
    }

    let mut marker_layer = PcbLayerId::FCu.0;
    let marker_pos = Vector2I::new(strtol10_as_int(&props[1]), strtol10_as_int(&props[2]));

    let mut drc_item = item::create_from_key(&props[0])?;
    let code = drc_item.error_code();
    let nil = NILUUID;

    if code == DRCE_COPPER_SLIVER || code == DRCE_GENERIC_WARNING || code == DRCE_GENERIC_ERROR {
        let a = Kiid::from_string_or_random(&props[3], kiid_generator);
        drc_item.set_items(a, nil, nil, nil);
        marker_layer = get_marker_layer(&props[4]);
    } else if code == DRCE_UNCONNECTED_ITEMS {
        // Pre-9.0.4 versions didn't have KIIDs as last two properties to allow sorting stability
        let (a, b) = if props.len() == 5 {
            (3, 4)
        } else if props.len() >= 7 {
            (5, 6)
        } else {
            return None;
        };

        let a = Kiid::from_string_or_random(&props[a], kiid_generator);
        let b = Kiid::from_string_or_random(&props[b], kiid_generator);
        drc_item.set_items(a, b, nil, nil);
    } else if code == DRCE_STARVED_THERMAL {
        let a = Kiid::from_string_or_random(&props[3], kiid_generator);
        let b = Kiid::from_string_or_random(&props[4], kiid_generator);
        drc_item.set_items(a, b, nil, nil);

        // Pre-7.0 versions didn't differentiate between layers
        if props.len() == 6 {
            marker_layer = get_marker_layer(&props[5]);
        }
    } else if code == DRCE_UNRESOLVED_VARIABLE && props[3].is_empty() && props[4].is_empty() {
        // Note: caller must load our item pointer with the drawing sheet proxy item
        marker_layer = LAYER_DRAWINGSHEET;
    } else {
        let a = Kiid::from_string_or_random(&props[3], kiid_generator);
        let b = Kiid::from_string_or_random(&props[4], kiid_generator);
        drc_item.set_items(a, b, nil, nil);
    }

    Some(PcbMarker::new(
        kiid_generator,
        Arc::new(drc_item),
        marker_pos,
        marker_layer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wx_split_cases() {
        assert!(wx_split("", '|', '\\').is_empty());
        assert_eq!(wx_split("a|b||c|", '|', '\\'), vec!["a", "b", "", "c", ""]);
        assert_eq!(wx_split("a\\|b|c", '|', '\\'), vec!["a|b", "c"]);
        assert_eq!(wx_split("a\\||c", '|', '\\'), vec!["a|", "c"]);
        assert_eq!(wx_split("|", '|', '\\'), vec!["", ""]);
    }

    #[test]
    fn strtol_cases() {
        assert_eq!(strtol10_as_int("113596405"), 113_596_405);
        assert_eq!(strtol10_as_int("  -42xyz"), -42);
        assert_eq!(strtol10_as_int("+7"), 7);
        assert_eq!(strtol10_as_int("abc"), 0);
        assert_eq!(strtol10_as_int("4294967297"), 1);
        assert_eq!(strtol10_as_int("99999999999999999999"), -1); // LONG_MAX -> low 32 bits
        assert_eq!(strtol10_as_int("-99999999999999999999"), 0); // LONG_MIN -> low 32 bits
    }

    #[test]
    fn legacy_strings() {
        let mut generator = KiidGenerator::default();
        let a = "1503dbdb-28b2-4a5a-8b2d-94a0f105210d";
        let nil = "00000000-0000-0000-0000-000000000000";

        let m = from_legacy_string(
            &format!("annular_width|113596405|160181800|{a}|{nil}"),
            &mut generator,
        )
        .unwrap();
        assert_eq!(m.rc_item().error_code, DRCE_ANNULAR_WIDTH);
        assert_eq!(m.pos(), Vector2I::new(113_596_405, 160_181_800));
        assert_eq!(m.rc_item().ids().len(), 4);
        assert_eq!(m.rc_item().main_item_id().as_string(), a);
        assert_eq!(m.marker_type(), MarkerT::Drc);
        let msg = to_proto(&m, None);
        assert_eq!(msg.items, vec![a.to_string()]);
        assert_eq!(msg.layer, 0);
        assert_eq!(msg.marker_type, 2);

        // Too short / unknown key.
        assert!(from_legacy_string("clearance|1|2|x", &mut generator).is_none());
        assert!(
            from_legacy_string(&format!("hole_near_hole|1|2|{a}|{a}"), &mut generator).is_none()
        );

        // Unconnected items: 5 fields, 6 fields (rejected), 7 fields.
        let m = from_legacy_string(&format!("unconnected_items|1|2|{a}|{nil}"), &mut generator)
            .unwrap();
        assert_eq!(m.marker_type(), MarkerT::Ratsnest);
        assert!(
            from_legacy_string(&format!("unconnected_items|1|2|{a}|{a}|x"), &mut generator)
                .is_none()
        );
        let m = from_legacy_string(
            &format!("unconnected_items|1|2|x|y|{nil}|{a}"),
            &mut generator,
        )
        .unwrap();
        assert_eq!(m.rc_item().aux_item_id().as_string(), a);
        let msg = to_proto(&m, None);
        assert_eq!(msg.layer, 3); // F_Cu stored for unconnected items

        // Copper sliver: one item and a layer name.
        let m = from_legacy_string(&format!("copper_sliver|1|2|{a}|B.Cu"), &mut generator).unwrap();
        assert_eq!(m.base.layer, PcbLayerId::BCu);
        assert_eq!(to_proto(&m, None).layer, 34);
        let m = from_legacy_string(&format!("copper_sliver|1|2|{a}|nope"), &mut generator).unwrap();
        assert_eq!(m.base.layer, PcbLayerId::FCu);

        // Starved thermal with a layer (display name).
        let m = from_legacy_string(
            &format!("starved_thermal|1|2|{a}|{a}|In1.Cu"),
            &mut generator,
        )
        .unwrap();
        assert_eq!(m.base.layer, PcbLayerId::In1Cu);

        // Drawing-sheet unresolved variable.
        let m = from_legacy_string("unresolved_variable|1|2||", &mut generator).unwrap();
        assert_eq!(m.marker_type(), MarkerT::DrawingSheet);
        assert!(m.rc_item().ids().is_empty());
    }

    #[test]
    fn proto_round_trip() {
        let mut generator = KiidGenerator::default();
        let a = "1503dbdb-28b2-4a5a-8b2d-94a0f105210d";
        let mut m = from_legacy_string(&format!("clearance|-5|7|{a}|{a}"), &mut generator).unwrap();
        m.set_excluded(true, "c");
        let msg = to_proto(&m, None);
        assert_eq!(msg.error_type, 5);
        assert_eq!(msg.position, Some(ProtoVector2 { x_nm: -5, y_nm: 7 }));
        assert_eq!(msg.description, "Clearance violation");
        assert!(msg.excluded);
        assert_eq!(msg.severity, 3);

        let back = from_proto(&msg, &mut generator).unwrap();
        assert_eq!(back.pos(), m.pos());
        assert_eq!(back.rc_item().ids().len(), 4);
        assert_eq!(back.rc_item().aux_item_id().as_string(), a);
        assert!(!back.is_excluded());

        // via_stitch_overlap has no DrcErrorType: the marker cannot be restored.
        let mut msg2 = msg.clone();
        msg2.error_type = drc_error_type_to_proto(DRCE_VIA_STITCH_OVERLAP);
        assert!(from_proto(&msg2, &mut generator).is_none());

        // Drawing-sheet marker type.
        let mut msg3 = msg.clone();
        msg3.marker_type = 3;
        let back = from_proto(&msg3, &mut generator).unwrap();
        assert_eq!(back.marker_type(), MarkerT::DrawingSheet);
    }
}
