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

//! Port of the marker / DRC-exclusion functions of `pcbnew/board.cpp`
//! (`ResolveDRCExclusions`, `RecordDRCExclusions`, `UpdateRatsnestExclusions`, both
//! `DeleteMARKERs`). Package M22.

use std::collections::BTreeSet;

use kicad_core::{Kiid, NILUUID, Severity};

use crate::board::Board;
use crate::board_design_settings::DrcExclusion;
use crate::ctx::BoardCtx;
use crate::ids::{ItemId, MarkerId};
use crate::pcb_marker::{MarkerT, PcbMarker};

impl Board {
    /// `BOARD::ResolveDRCExclusions( aCreateMarkers )`: the markers created for exclusions
    /// that match no existing marker.
    ///
    /// Stub: the exclusion identity (`DRC_EXCLUSION::FromMarker`, `FromProto` / `ToProto` and
    /// `PCB_MARKER::FromProto`) belongs to `kicad-drc` (`drc_exclusion.cpp`, DRC-11), above this
    /// crate. The board logic is [`Board::resolve_drc_exclusions_with`]; `kicad-drc` /
    /// `kicad-session` call that with its conversions and add the returned markers.
    pub fn resolve_drc_exclusions(&mut self, create_markers: bool) -> Vec<MarkerId> {
        crate::port_pending!(
            "BOARD::ResolveDRCExclusions (DRC_EXCLUSION in kicad-drc)",
            create_markers
        )
    }

    /// `BOARD::ResolveDRCExclusions( aCreateMarkers )` with the `kicad-drc` conversions passed
    /// in: `from_marker` is `DRC_EXCLUSION::FromMarker`, `marker_from_exclusion` is
    /// `PCB_MARKER::FromProto( exclusion.ToProto().marker() )` (`None` for a null marker).
    ///
    /// Every existing marker matching a recorded exclusion is excluded with the exclusion's
    /// comment; the design settings keep only the matched exclusions, plus (with
    /// `create_markers`) those that become new markers. New markers whose items do not all
    /// exist on the board are dropped. The returned markers are not added to the board (the
    /// C++ callers `Add()` them).
    pub fn resolve_drc_exclusions_with(
        &mut self,
        create_markers: bool,
        from_marker: &dyn Fn(&PcbMarker) -> DrcExclusion,
        marker_from_exclusion: &dyn Fn(&DrcExclusion) -> Option<PcbMarker>,
    ) -> Vec<PcbMarker> {
        let exclusions: BTreeSet<DrcExclusion> =
            std::mem::take(&mut self.design_settings_mut().drc_exclusions);

        for id in self.marker_list.clone() {
            let Some(marker) = self.markers.get_mut(id) else {
                continue;
            };

            let lookup = from_marker(marker);

            if let Some(it) = exclusions.get(&lookup) {
                marker.set_excluded(true, &it.comment);
                self.design_settings.drc_exclusions.insert(it.clone());
            }
        }

        let mut new_markers: Vec<PcbMarker> = Vec::new();

        if create_markers {
            for exclusion in &exclusions {
                if self.design_settings.drc_exclusions.contains(exclusion) {
                    continue;
                }

                let Some(mut marker) = marker_from_exclusion(exclusion) else {
                    continue;
                };

                let ids: Vec<Kiid> = marker.rc_item().ids().to_vec();

                if ids
                    .iter()
                    .any(|uuid| *uuid != NILUUID && self.resolve_item(uuid).is_none())
                {
                    continue;
                }

                marker.set_excluded(true, &exclusion.comment);
                new_markers.push(marker);
                self.design_settings
                    .drc_exclusions
                    .insert(exclusion.clone());
            }
        }

        new_markers
    }

    /// `BOARD::RecordDRCExclusions`.
    ///
    /// Stub for the same reason as [`Board::resolve_drc_exclusions`]; the board logic is
    /// [`Board::record_drc_exclusions_with`].
    pub fn record_drc_exclusions(&mut self) {
        crate::port_pending!("BOARD::RecordDRCExclusions (DRC_EXCLUSION in kicad-drc)")
    }

    /// `BOARD::RecordDRCExclusions` with `from_marker` = `DRC_EXCLUSION::FromMarker`: the
    /// design settings' exclusions become those of the excluded markers. The C++ copy into
    /// `PROJECT_FILE::m_BoardSettings` is the host's job (the project file is not modelled);
    /// every Rust marker carries an `RC_ITEM`, so the C++ null check has no counterpart.
    pub fn record_drc_exclusions_with(&mut self, from_marker: &dyn Fn(&PcbMarker) -> DrcExclusion) {
        let mut recorded: BTreeSet<DrcExclusion> = BTreeSet::new();

        for (_, marker) in self.markers() {
            if marker.is_excluded() {
                recorded.insert(from_marker(marker));
            }
        }

        self.design_settings_mut().drc_exclusions = recorded;
    }

    /// The board half of `BOARD::UpdateRatsnestExclusions`: the (main, aux) UUID pairs, both
    /// ways round, of the excluded ratsnest markers. The connectivity crate hides the matching
    /// unconnected edges (`RunOnUnconnectedEdges`).
    pub fn ratsnest_exclusions(&self) -> BTreeSet<(Kiid, Kiid)> {
        let mut exclusions = BTreeSet::new();

        for (_, marker) in self.markers() {
            if marker.marker_type() == MarkerT::Ratsnest && marker.is_excluded() {
                let rc_item = marker.rc_item();
                exclusions.insert((rc_item.main_item_id(), rc_item.aux_item_id()));
                exclusions.insert((rc_item.aux_item_id(), rc_item.main_item_id()));
            }
        }

        exclusions
    }

    /// `BOARD::DeleteMARKERs()`: every marker.
    pub fn delete_all_markers(&mut self) {
        let doomed: Vec<ItemId> = self.marker_list.iter().map(|&id| id.into()).collect();
        self.remove_many(&doomed);
        self.increment_time_stamp();
    }

    /// `BOARD::DeleteMARKERs( aWarningsAndErrors, aExclusions )`: the markers whose severity
    /// is `RPT_SEVERITY_EXCLUSION` (with `exclusions`) or anything else (with
    /// `warnings_and_errors`); the others keep their order.
    pub fn delete_markers(&mut self, warnings_and_errors: bool, exclusions: bool) {
        let doomed: Vec<ItemId> = {
            let ctx = BoardCtx::bare(self);

            self.markers()
                .filter(|(_, marker)| {
                    let excluded = marker.severity(&ctx) == Severity::Exclusion;
                    (excluded && exclusions) || (!excluded && warnings_and_errors)
                })
                .map(|(id, _)| id.into())
                .collect()
        };

        self.remove_many(&doomed);
        self.increment_time_stamp();
    }
}

/// Package M22 tests: the board logic of the exclusion functions, with a stand-in for the
/// `kicad-drc` identity (`DRC_EXCLUSION::FromMarker`: here the error code and the item ids).
#[cfg(test)]
mod m22_tests {
    use std::sync::Arc;

    use kicad_core::Kiid;
    use kicad_geom::Vector2I;

    use super::*;
    use crate::pcb_marker::RcItem;

    fn key_of(rc: &RcItem) -> String {
        format!("{}:{:?}", rc.error_code, rc.ids)
    }

    fn from_marker(marker: &PcbMarker) -> DrcExclusion {
        DrcExclusion {
            sort_key: key_of(marker.rc_item()),
            comment: String::new(),
            json: Default::default(),
        }
    }

    fn marker(board: &mut Board, code: i32, ids: Vec<Kiid>) -> PcbMarker {
        let rc = RcItem {
            error_code: code,
            ids,
            ..RcItem::default()
        };
        PcbMarker::new(
            &mut board.kiid_generator,
            Arc::new(rc),
            Vector2I::new(0, 0),
            0,
        )
    }

    #[test]
    fn resolve_and_record_exclusions() {
        let mut board = Board::default();
        let existing = marker(&mut board, 5, vec![]);
        let existing_key = key_of(existing.rc_item());
        let m = board.add_marker(existing);

        let exclusion = |key: &str, comment: &str| DrcExclusion {
            sort_key: key.to_string(),
            comment: comment.to_string(),
            json: Default::default(),
        };
        let dangling = Kiid::parse("00000000-0000-0000-0000-000000000123").unwrap();
        let ds = &mut board.design_settings_mut().drc_exclusions;
        ds.insert(exclusion(&existing_key, "known"));
        ds.insert(exclusion("7:[]", "new"));
        ds.insert(exclusion("dangling", "gone"));

        let to_marker = |ex: &DrcExclusion| -> Option<PcbMarker> {
            let mut generator = kicad_core::KiidGenerator::default();
            let rc = match ex.sort_key.as_str() {
                "7:[]" => RcItem {
                    error_code: 7,
                    ..RcItem::default()
                },
                "dangling" => RcItem {
                    error_code: 9,
                    ids: vec![dangling],
                    ..RcItem::default()
                },
                _ => return None,
            };
            Some(PcbMarker::new(
                &mut generator,
                Arc::new(rc),
                Vector2I::new(0, 0),
                0,
            ))
        };

        let created = board.resolve_drc_exclusions_with(true, &from_marker, &to_marker);
        assert!(board[m].is_excluded());
        assert_eq!(board[m].comment(), "known");
        assert_eq!(created.len(), 1);
        assert_eq!(created[0].rc_item().error_code, 7);
        assert!(created[0].is_excluded());
        assert_eq!(created[0].comment(), "new");
        // The dangling exclusion is dropped from the settings.
        let keys: Vec<&str> = board
            .design_settings()
            .drc_exclusions
            .iter()
            .map(|e| e.sort_key.as_str())
            .collect();
        assert_eq!(keys, vec![existing_key.as_str(), "7:[]"]);

        // Without creating markers only the matched exclusion survives.
        let mut copy = board.clone();
        assert!(
            copy.resolve_drc_exclusions_with(false, &from_marker, &to_marker)
                .is_empty()
        );
        assert_eq!(copy.design_settings().drc_exclusions.len(), 1);

        // Record: the excluded markers only.
        for marker in created {
            board.add_marker(marker);
        }
        board.record_drc_exclusions_with(&from_marker);
        assert_eq!(board.design_settings().drc_exclusions.len(), 2);

        let ratsnest = board.ratsnest_exclusions();
        assert!(ratsnest.is_empty());

        board.delete_all_markers();
        assert_eq!(board.markers().count(), 0);
        board.check_invariants().unwrap();
    }

    #[test]
    fn ratsnest_exclusion_pairs() {
        let mut board = Board::default();
        let (a, b) = (Kiid::from_name("a"), Kiid::from_name("b"));
        let mut m = marker(&mut board, 1, vec![a, b]);
        m.set_marker_type(MarkerT::Ratsnest);
        m.set_excluded(true, "");
        board.add_marker(m);
        let mut not_excluded = marker(&mut board, 1, vec![b, b]);
        not_excluded.set_marker_type(MarkerT::Ratsnest);
        board.add_marker(not_excluded);

        let pairs: Vec<(Kiid, Kiid)> = board.ratsnest_exclusions().into_iter().collect();
        let mut want = vec![(a, b), (b, a)];
        want.sort();
        assert_eq!(pairs, want);
    }
}
