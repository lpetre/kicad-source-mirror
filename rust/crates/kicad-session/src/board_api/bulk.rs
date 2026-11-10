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

//! Flat integer record forms of the hot router calls (package LIVE-1; no C++ counterpart):
//! tracks, arcs and vias in and out as `i32` records, handles as `u64` arrays, so that a
//! WebAssembly host moves a whole batch with one call and no per-item objects. The records
//! map one to one onto [`NewTrack`] / [`NewArc`] / [`NewVia`] with every optional field at
//! its default; layers are `PCB_LAYER_ID` values (`F_Cu` = 0, `B_Cu` = 2, `In1_Cu` = 4, ...).
//!
//! | kind | stride | fields |
//! |---|---|---|
//! | track | 7 | `x1, y1, x2, y2, width, layer, net` |
//! | arc | 9 | `x1, y1, x2, y2, width, layer, net, mid x, mid y` |
//! | via | 8 | `x, y, diameter, drill, top layer, bottom layer, net, type` (0 through, 1 blind, 2 buried, 3 micro) |

use kicad_core::PcbLayerId;
use kicad_pcb::ItemId;
use kicad_pcb::pcb_track::{Track, Viatype};
use serde::{Deserialize, Serialize};

use super::commit::Transaction;
use super::edit::{NewArc, NewItem, NewTrack, NewVia};
use super::{layer_name, pt};
use crate::error::SessionError;
use crate::session::Session;

/// Values per track record.
pub const TRACK_STRIDE: usize = 7;
/// Values per arc record.
pub const ARC_STRIDE: usize = 9;
/// Values per via record.
pub const VIA_STRIDE: usize = 8;

const VIA_TYPES: [&str; 4] = ["through", "blind", "buried", "micro"];

fn via_type_code(t: Viatype) -> i32 {
    match t {
        Viatype::Blind => 1,
        Viatype::Buried => 2,
        Viatype::Microvia => 3,
        _ => 0,
    }
}

fn records(values: &[i32], stride: usize, what: &str) -> Result<usize, SessionError> {
    if values.len() % stride != 0 {
        return Err(SessionError::invalid_argument(format!(
            "{what} records: {} values is not a multiple of {stride}",
            values.len()
        )));
    }

    Ok(values.len() / stride)
}

fn layer_of(value: i32) -> Result<String, SessionError> {
    if !(0..kicad_core::layer_ids::PCB_LAYER_ID_COUNT as i32).contains(&value) {
        return Err(SessionError::invalid_argument(format!(
            "invalid layer index {value}"
        )));
    }

    Ok(layer_name(PcbLayerId::from(value)))
}

impl Transaction {
    /// Stages one new track segment per record of `values` (stride [`TRACK_STRIDE`]).
    pub fn stage_track_records(&mut self, values: &[i32]) -> Result<usize, SessionError> {
        let n = records(values, TRACK_STRIDE, "track")?;

        for r in values.chunks_exact(TRACK_STRIDE) {
            self.stage_add(NewItem::Track(NewTrack {
                start: [r[0], r[1]],
                end: [r[2], r[3]],
                width: Some(r[4]),
                layer: layer_of(r[5])?,
                net: r[6],
                ..NewTrack::default()
            }));
        }

        Ok(n)
    }

    /// Stages one new arc per record of `values` (stride [`ARC_STRIDE`]).
    pub fn stage_arc_records(&mut self, values: &[i32]) -> Result<usize, SessionError> {
        let n = records(values, ARC_STRIDE, "arc")?;

        for r in values.chunks_exact(ARC_STRIDE) {
            self.stage_add(NewItem::Arc(NewArc {
                start: [r[0], r[1]],
                end: [r[2], r[3]],
                width: Some(r[4]),
                layer: layer_of(r[5])?,
                net: r[6],
                mid: [r[7], r[8]],
                ..NewArc::default()
            }));
        }

        Ok(n)
    }

    /// Stages one new via per record of `values` (stride [`VIA_STRIDE`]).
    pub fn stage_via_records(&mut self, values: &[i32]) -> Result<usize, SessionError> {
        let n = records(values, VIA_STRIDE, "via")?;

        for r in values.chunks_exact(VIA_STRIDE) {
            let Some(vt) = usize::try_from(r[7]).ok().and_then(|t| VIA_TYPES.get(t)) else {
                return Err(SessionError::invalid_argument(format!(
                    "invalid via type {}",
                    r[7]
                )));
            };

            self.stage_add(NewItem::Via(NewVia {
                at: [r[0], r[1]],
                diameter: Some(r[2]),
                drill: Some(r[3]),
                top: Some(layer_of(r[4])?),
                bottom: Some(layer_of(r[5])?),
                net: r[6],
                via_type: Some((*vt).to_string()),
                ..NewVia::default()
            }));
        }

        Ok(n)
    }

    /// Stages the removal of every handle of `ids`.
    pub fn stage_removals(&mut self, ids: &[u64]) -> &mut Self {
        for &id in ids {
            self.stage_remove(id);
        }

        self
    }
}

/// The routing of the board as flat records, in board order (`BOARD::Tracks()`), each kind
/// with its handles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteRecords {
    /// Handles of the track segments.
    pub track_ids: Vec<u64>,
    /// Track records (stride [`TRACK_STRIDE`]).
    pub tracks: Vec<i32>,
    /// Handles of the arcs.
    pub arc_ids: Vec<u64>,
    /// Arc records (stride [`ARC_STRIDE`]).
    pub arcs: Vec<i32>,
    /// Handles of the vias.
    pub via_ids: Vec<u64>,
    /// Via records (stride [`VIA_STRIDE`]; the diameter is `GetWidth( F_Cu )`).
    pub vias: Vec<i32>,
}

impl Session {
    /// Every track, arc and via as flat records (the read half of the bulk forms).
    pub fn route_records(&self) -> RouteRecords {
        let mut r = RouteRecords::default();

        for (id, track) in self.board.tracks() {
            let t = track.as_track();
            let common = |out: &mut Vec<i32>| {
                let [x1, y1] = pt(t.start);
                let [x2, y2] = pt(t.end);
                out.extend([x1, y1, x2, y2, t.width, i32::from(t.base.item.layer)]);
                out.push(t.base.net.0);
            };

            match track {
                Track::Trace(_) => {
                    r.track_ids.push(ItemId::from(id).to_bits());
                    common(&mut r.tracks);
                }
                Track::Arc(arc) => {
                    r.arc_ids.push(ItemId::from(id).to_bits());
                    common(&mut r.arcs);
                    r.arcs.extend(pt(arc.mid));
                }
                Track::Via(via) => {
                    r.via_ids.push(ItemId::from(id).to_bits());
                    r.vias.extend([
                        t.start.x,
                        t.start.y,
                        via.width(PcbLayerId::FCu),
                        via.drill(),
                        i32::from(via.top_layer()),
                        i32::from(via.bottom_layer()),
                        t.base.net.0,
                        via_type_code(via.via_type()),
                    ]);
                }
            }
        }

        r
    }
}
