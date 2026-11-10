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

//! Unit tests of the board dump on the model's fixture boards (`test_support.rs`).

use super::{DumpOptions, dump_board};
use crate::ids::ItemKind;
use crate::item::ParentRef;
use crate::pcb_track::{PcbTrack, Track};
use crate::test_support::{board_with_footprint, rotated_flipped_scaled};

/// Every line is one of the documented kinds and data lines have `SECTION|KEY|FIELD=VALUE`.
fn check_well_formed(text: &str) {
    for line in text.lines() {
        let kind = line.split('|').next().unwrap_or("");
        let parts: Vec<&str> = line.splitn(4, '|').collect();

        match kind {
            "pending" | "rust-error" => {
                assert_eq!(parts.len(), 4, "{line}");
                assert!(parts[3].contains('='), "{line}");
            }
            _ => {
                assert_eq!(parts.len(), 3, "{line}");
                assert!(parts[2].contains('='), "{line}");
                assert!(
                    [
                        "item",
                        "bbox",
                        "track",
                        "pad",
                        "shape",
                        "text",
                        "dim",
                        "zone",
                        "footprint",
                        "misc",
                        "board",
                        "order"
                    ]
                    .contains(&kind),
                    "{line}"
                );
            }
        }
    }
}

fn sections_of(text: &str, key: &str) -> Vec<String> {
    let mut v: Vec<String> = text
        .lines()
        .filter_map(|l| {
            let p: Vec<&str> = l.split('|').collect();
            let (s, k) = if p[0] == "pending" || p[0] == "rust-error" {
                (p[1], p[2])
            } else {
                (p[0], p[1])
            };
            (k == key).then(|| s.to_string())
        })
        .collect();
    v.dedup();
    v
}

#[test]
fn fixture_board_dumps_every_item_and_section() {
    let mut fx = rotated_flipped_scaled();
    let track = fx
        .board
        .insert_item(Track::Trace(PcbTrack::default()), ParentRef::Board);
    fx.board.track_list.push_back(track);

    let text = dump_board(&fx.board, &DumpOptions::default());
    check_well_formed(&text);

    // The walk: footprint #0, then its children (fields, pads, drawings with the table's cell,
    // zones, groups, points), then the board track.
    let n = super::format::board_items(&fx.board).len();
    assert_eq!(
        n,
        fx.children.len() - 2 + 1 + 1,
        "constraint and 3D body are not walked"
    );

    for i in 0..n {
        let key = format!("#{i}");
        let s = sections_of(&text, &key);
        // `misc` prints nothing for most types.
        assert_eq!(s.first().map(String::as_str), Some("item"), "{key}: {s:?}");
        assert!(s.iter().any(|x| x == "bbox"), "{key}: {s:?}");
    }

    assert!(sections_of(&text, "#0").contains(&"footprint".to_string()));
    let pad = fx.child(ItemKind::Pad);
    let pad_key = super::format::board_items(&fx.board)
        .iter()
        .position(|&id| id == pad)
        .map(|i| format!("#{i}"))
        .expect("pad walked");
    assert!(sections_of(&text, &pad_key).contains(&"pad".to_string()));
    assert!(
        text.contains("item|#0|item.type=PCB_FOOTPRINT_T\n")
            || text.contains("item|#0|type=PCB_FOOTPRINT_T\n")
    );
    assert!(text.lines().any(|l| l.starts_with("board|board|")));
    assert!(
        text.lines()
            .any(|l| l.contains("|order|board|") || l.starts_with("order|board|"))
    );
    assert!(text.contains(&format!("item|#{}|type=PCB_TRACE_T\n", n - 1)));
}

#[test]
fn sections_filter_and_hash() {
    let fx = board_with_footprint(Default::default(), false);
    let opts = DumpOptions {
        sections: ["item".to_string()].into_iter().collect(),
        hash: true,
        file_uuids: None,
    };
    let text = dump_board(&fx.board, &opts);
    check_well_formed(&text);

    for line in text
        .lines()
        .filter(|l| !l.starts_with("pending") && !l.starts_with("rust-error"))
    {
        assert!(line.starts_with("item|"), "{line}");
        let v = line.split_once('=').expect("value").1;
        assert!(v.len() == 17 && v.starts_with('#'), "{line}");
    }
}

#[test]
fn file_uuids_give_uuid_keys() {
    let fx = board_with_footprint(Default::default(), false);
    let uuid = fx.board[fx.footprint].base.uuid.as_std_string();
    let opts = DumpOptions {
        file_uuids: Some(super::scan_file_uuids(
            format!("(uuid \"{uuid}\")").as_bytes(),
        )),
        ..DumpOptions::default()
    };
    let text = dump_board(&fx.board, &opts);
    assert!(text.contains(&format!(
        "item|{uuid}|uuid={}\n",
        super::format::v_str(&uuid).trim_matches('"')
    )));
    assert!(text.contains(&format!("item|{uuid}|uuid_in_file=true\n")));
}
