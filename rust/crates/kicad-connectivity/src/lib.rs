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

//! Port of KiCad's board connectivity algorithm and ratsnest (`pcbnew/connectivity/`,
//! `pcbnew/ratsnest/ratsnest_data.*`).
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md` and
//! `rust/docs/analysis/connectivity-zones.md` (module layout: section 10.1). One module per
//! C++ file:
//!
//! | Module | C++ |
//! |---|---|
//! | [`items`] | `connectivity_items.h/.cpp` (`CN_ANCHOR`, `CN_ITEM`, `CN_ZONE_LAYER`, `CN_LIST`, `CN_CLUSTER`) |
//! | [`rtree`] | `connectivity_rtree.h` (`CN_RTREE`) |
//! | [`algo`] | `connectivity_algo.h/.cpp` (`CN_CONNECTIVITY_ALGO`, `CN_VISITOR`) |
//! | [`edge`] | `CN_EDGE` (`connectivity_algo.h`) |
//! | [`ratsnest`] | `ratsnest_data.h/.cpp` (`RN_NET`) |
//! | [`data`] | `connectivity_data.h/.cpp` (`CONNECTIVITY_DATA`) |
//! | [`from_to`] | `from_to_cache.h/.cpp` (`FROM_TO_CACHE`) |

pub mod algo;
pub mod data;
pub mod edge;
pub mod from_to;
pub mod items;
pub mod ratsnest;
pub mod rtree;

pub use data::ConnectivityData;
pub use items::{
    CnAnchor, CnAnchorRef, CnCluster, CnClusterId, CnItem, CnItemId, CnList, CnZoneLayer,
};
pub use rtree::CnRtree;
