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

//! Port of `pcbnew/drc/drc_exclusion.h` / `drc_exclusion.cpp` (`DRC_EXCLUSION`: a
//! `kiapi.board.DrcExclusion` message = canonical marker + comment) and of the DRC side of
//! `BOARD::ResolveDRCExclusions` / `BOARD::RecordDRCExclusions` (`pcbnew/board.cpp`), work
//! package DRC-11.
//!
//! The value type stored in the design settings is `kicad_pcb::board_design_settings::
//! DrcExclusion` (`sort_key`, `comment`, `json`); this module creates and reads it:
//!
//! * `sort_key` is `DRC_EXCLUSION::GetSortKey()`: the protobuf wire bytes of the canonical
//!   marker (without the layer for unconnected items). C++ compares these `std::string`s
//!   bytewise (`char_traits<char>::lt` compares as `unsigned char`); the Rust key maps every
//!   byte to the `char` with the same value (Latin-1), which keeps that order, because UTF-8
//!   order is code point order. Empty = no marker.
//! * `json` is the message as stored in the `.kicad_pro` (`to_json( json&, const
//!   DRC_EXCLUSION& )`); the full message is recovered from it.

use std::cell::RefCell;

use kicad_core::KiidGenerator;
use kicad_core::json::Value;
use kicad_pcb::Board;
use kicad_pcb::board_design_settings::DrcExclusion;
use kicad_pcb::pcb_marker::PcbMarker;

use crate::marker::proto::{DrcExclusionMsg, DrcMarkerMsg, parse_exclusion_json};
use crate::marker::{from_legacy_string, from_proto as marker_from_proto, to_proto as marker_to_proto};

/// `DRCET_UNCONNECTED_ITEMS`.
const DRCET_UNCONNECTED_ITEMS: i32 = 1;

/// Port of `canonicalizeExclusion`: the report-only fields are not part of an exclusion.
fn canonicalize_exclusion(message: &mut DrcExclusionMsg) {
    let Some(marker) = &mut message.marker else {
        return;
    };

    marker.violating_rule.clear();
    marker.description.clear();
    marker.severity = 0;
    marker.excluded = false;
}

/// The bytes of a C++ `std::string` sort key as an order-preserving Rust string (see the
/// module documentation).
fn bytes_to_key(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

/// Port of `DRC_EXCLUSION::GetSortKey()`.
pub fn sort_key_of(message: &DrcExclusionMsg) -> String {
    let Some(marker) = &message.marker else {
        return String::new();
    };

    // Unconnected-item markers may be reported on several layers
    if marker.error_type == DRCET_UNCONNECTED_ITEMS {
        let mut marker = marker.clone();
        marker.layer = 0;
        return bytes_to_key(&marker.serialize());
    }

    bytes_to_key(&marker.serialize())
}

/// The `DRC_EXCLUSION` value of a (canonical) message.
fn exclusion_from_message(message: DrcExclusionMsg) -> DrcExclusion {
    DrcExclusion {
        sort_key: sort_key_of(&message),
        comment: message.comment.clone(),
        json: message.to_json(),
    }
}

/// `DRC_EXCLUSION::ToProto()`: the message of an exclusion, recovered from its stored JSON.
pub fn to_proto(exclusion: &DrcExclusion) -> DrcExclusionMsg {
    parse_exclusion_json(&exclusion.json).unwrap_or_default()
}

/// Port of `DRC_EXCLUSION::FromMarker( const PCB_MARKER& )`: the canonical marker message and
/// the marker's comment.
pub fn from_marker(marker: &PcbMarker) -> DrcExclusion {
    // The severity / exclusion fields ToProto fills from the board are cleared again by the
    // canonicalisation, so the board is not needed.
    let mut message = DrcExclusionMsg {
        marker: Some(marker_to_proto(marker, None)),
        comment: String::new(),
    };

    canonicalize_exclusion(&mut message);
    message.comment = marker.comment().to_string();
    exclusion_from_message(message)
}

/// Port of `DRC_EXCLUSION::FromProto( const kiapi::board::DrcExclusion& )`.
pub fn from_proto(message: &DrcExclusionMsg) -> DrcExclusion {
    let mut message = message.clone();
    canonicalize_exclusion(&mut message);
    exclusion_from_message(message)
}

/// Port of `DRC_EXCLUSION::FromLegacyStrings( aMarkerData, aComment )`: an exclusion without a
/// marker (empty sort key) when the legacy string is rejected.
pub fn from_legacy_strings(
    marker_data: &str,
    comment: &str,
    kiid_generator: &mut KiidGenerator,
) -> DrcExclusion {
    let mut message = DrcExclusionMsg::default();

    if let Some(marker) = from_legacy_string(marker_data, kiid_generator) {
        message.marker = Some(marker_to_proto(&marker, None));
    }

    canonicalize_exclusion(&mut message);
    message.comment = comment.to_string();
    exclusion_from_message(message)
}

/// Port of `from_json( const nlohmann::json&, DRC_EXCLUSION& )` on a fresh `DRC_EXCLUSION`:
/// `JsonStringToMessage` (which leaves the message empty when it rejects the input), then the
/// canonicalisation.
pub fn from_json(json: &Value) -> DrcExclusion {
    let mut message = parse_exclusion_json(json).unwrap_or_default();
    canonicalize_exclusion(&mut message);
    exclusion_from_message(message)
}

/// Port of `to_json( nlohmann::json&, const DRC_EXCLUSION& )`.
pub fn to_json(exclusion: &DrcExclusion) -> Value {
    exclusion.json.clone()
}

/// `PCB_MARKER::FromProto( exclusion.ToProto().marker() )` as `BOARD::ResolveDRCExclusions`
/// calls it (a missing marker message is the default message, whose error type has no item).
pub fn marker_from_exclusion(
    exclusion: &DrcExclusion,
    kiid_generator: &mut KiidGenerator,
) -> Option<PcbMarker> {
    let marker = to_proto(exclusion).marker.unwrap_or_else(DrcMarkerMsg::default);
    marker_from_proto(&marker, kiid_generator)
}

/// Port of `BOARD::ResolveDRCExclusions( aCreateMarkers )`: marks the board markers that match
/// a recorded exclusion as excluded and returns the markers created for the other exclusions
/// whose items all exist (only with `create_markers`; the caller adds them to the board).
/// New marker UUIDs come from the board's generator.
pub fn resolve_drc_exclusions(board: &mut Board, create_markers: bool) -> Vec<PcbMarker> {
    let generator = RefCell::new(std::mem::take(&mut board.kiid_generator));

    let new_markers = board.resolve_drc_exclusions_with(create_markers, &from_marker, &|ex| {
        marker_from_exclusion(ex, &mut generator.borrow_mut())
    });

    board.kiid_generator = generator.into_inner();
    new_markers
}

/// Port of `BOARD::RecordDRCExclusions()` (the copy into the project file's board settings is
/// the host's job, see `Board::record_drc_exclusions_with`).
pub fn record_drc_exclusions(board: &mut Board) {
    board.record_drc_exclusions_with(&from_marker);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use kicad_core::json::{ObjectOrder, dump, parse};
    use kicad_core::{Kiid, PcbLayerId};
    use kicad_geom::Vector2I;
    use kicad_pcb::drc_api::*;

    fn marker(board: &mut Board, code: i32, ids: Vec<Kiid>, pos: Vector2I, layer: i32) -> PcbMarker {
        let mut item = crate::item::create(code).unwrap();
        item.set_item_ids(ids);
        PcbMarker::new(&mut board.kiid_generator, Arc::new(item), pos, layer)
    }

    #[test]
    fn identity_ignores_report_fields_and_comment() {
        let mut board = Board::default();
        let a = Kiid::from_name("a");
        let mut m1 = marker(&mut board, DRCE_CLEARANCE, vec![a], Vector2I::new(1, 2), 0);
        let mut m2 = m1.clone();
        Arc::make_mut(&mut m2.rc_item).set_error_detail("(x)");
        m2.set_excluded(true, "comment");
        m1.set_excluded(false, "");
        assert_eq!(from_marker(&m1), from_marker(&m2));
        assert_eq!(from_marker(&m2).comment, "comment");

        // The position and the item ids are part of the identity, the layer is not (except for
        // the layer-carrying codes).
        let m3 = marker(&mut board, DRCE_CLEARANCE, vec![a], Vector2I::new(1, 3), 0);
        assert_ne!(from_marker(&m1), from_marker(&m3));
        let m4 = marker(&mut board, DRCE_CLEARANCE, vec![a], Vector2I::new(1, 2), PcbLayerId::BCu.0);
        assert_eq!(from_marker(&m1), from_marker(&m4));
        let s1 = marker(&mut board, DRCE_COPPER_SLIVER, vec![a], Vector2I::new(1, 2), 0);
        let s2 = marker(&mut board, DRCE_COPPER_SLIVER, vec![a], Vector2I::new(1, 2), PcbLayerId::BCu.0);
        assert_ne!(from_marker(&s1), from_marker(&s2));
        // ... except for unconnected items, whose sort key drops the layer.
        let u1 = marker(&mut board, DRCE_UNCONNECTED_ITEMS, vec![a, a], Vector2I::new(1, 2), 0);
        let u2 = marker(&mut board, DRCE_UNCONNECTED_ITEMS, vec![a, a], Vector2I::new(1, 2), PcbLayerId::BCu.0);
        assert_eq!(from_marker(&u1), from_marker(&u2));
        assert_ne!(from_marker(&u1).json, from_marker(&u2).json);
    }

    #[test]
    fn json_storage_round_trip() {
        let mut board = Board::default();
        let a = Kiid::from_name("a");
        let mut m = marker(&mut board, DRCE_TRACK_WIDTH, vec![a], Vector2I::new(-1000, 2000), 0);
        m.set_excluded(true, "fine");
        let ex = from_marker(&m);
        let text = dump(&to_json(&ex), None, ObjectOrder::Sorted);
        assert_eq!(
            text,
            format!(
                "{{\"comment\":\"fine\",\"marker\":{{\"error_type\":\"DRCET_TRACK_WIDTH\",\"items\":[{{\"value\":\"{}\"}}],\"marker_type\":\"MT_DRC\",\"position\":{{\"x_nm\":\"-1000\",\"y_nm\":\"2000\"}}}}}}",
                a.as_string()
            )
        );

        let back = from_json(&parse(&text, ObjectOrder::Sorted).unwrap());
        assert_eq!(back, ex);
        assert_eq!(back.comment, "fine");
        assert_eq!(dump(&back.json, None, ObjectOrder::Sorted), text);

        // Rejected input: empty message, empty sort key.
        let bad = from_json(&parse("{\"marker\":{\"bogus\":1},\"comment\":\"x\"}", ObjectOrder::Sorted).unwrap());
        assert!(bad.sort_key.is_empty());
        assert!(bad.comment.is_empty());

        // Legacy string.
        let mut generator = KiidGenerator::default();
        let legacy = from_legacy_strings(
            &format!("track_width|-1000|2000|{}|00000000-0000-0000-0000-000000000000", a.as_string()),
            "fine",
            &mut generator,
        );
        assert_eq!(legacy, ex);
        assert_eq!(dump(&legacy.json, None, ObjectOrder::Sorted), text);
        let rejected = from_legacy_strings("track_width|1", "c", &mut generator);
        assert!(rejected.sort_key.is_empty());
        assert_eq!(dump(&rejected.json, None, ObjectOrder::Sorted), "{\"comment\":\"c\"}");
    }

    #[test]
    fn sort_key_order_is_byte_order() {
        // Keys whose bytes differ above 0x7f still sort like the C++ std::string.
        assert!(bytes_to_key(&[0x7f]) < bytes_to_key(&[0x80]));
        assert!(bytes_to_key(&[0x80, 0x00]) < bytes_to_key(&[0xff]));
        assert!(bytes_to_key(&[0x12]) < bytes_to_key(&[0x12, 0x00]));
    }

    #[test]
    fn resolve_and_record() {
        let mut board = Board::default();
        let a = Kiid::from_name("a");
        let existing = marker(&mut board, DRCE_CLEARANCE, vec![], Vector2I::new(5, 5), 0);
        let mut recorded = existing.clone();
        recorded.set_excluded(true, "known");
        let id = board.add_marker(existing);

        // An exclusion for an item that does not exist and one without items.
        let dangling = marker(&mut board, DRCE_TRACK_WIDTH, vec![a], Vector2I::new(0, 0), 0);
        let mut lonely = marker(&mut board, DRCE_INVALID_OUTLINE, vec![], Vector2I::new(9, 9), 0);
        lonely.set_excluded(true, "outline");

        let settings = &mut board.design_settings_mut().drc_exclusions;
        settings.insert(from_marker(&recorded));
        settings.insert(from_marker(&dangling));
        settings.insert(from_marker(&lonely));

        let created = resolve_drc_exclusions(&mut board, true);
        assert!(board[id].is_excluded());
        assert_eq!(board[id].comment(), "known");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].rc_item().error_code, DRCE_INVALID_OUTLINE);
        assert_eq!(created[0].pos(), Vector2I::new(9, 9));
        assert_eq!(created[0].comment(), "outline");
        assert_eq!(board.design_settings().drc_exclusions.len(), 2);

        for m in created {
            board.add_marker(m);
        }

        record_drc_exclusions(&mut board);
        assert_eq!(board.design_settings().drc_exclusions.len(), 2);
    }
}
