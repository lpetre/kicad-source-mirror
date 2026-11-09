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

//! Port of KiCad common/ basics: KIID, layer ids and LSET, units and number formatting, text attributes, colours
//!
//! Part of the Rust port of KiCad core features. See `rust/PORTING.md`.
//!
//! | Module | C++ origin |
//! |---|---|
//! | [`kiid`] | `include/kiid.h`, `common/kiid.cpp` |
//! | [`mt19937`] | `boost::mt19937` (used by `KIID` generation) |
//! | [`layer_ids`] | `include/layer_ids.h`, `common/layer_id.cpp` |
//! | [`layer_range`] | `include/layer_range.h` |
//! | [`lseq`] | `include/lseq.h`, `common/lseq.cpp` |
//! | [`base_set`] | `include/base_set.h` (and `thirdparty/dynamic_bitset`) |
//! | [`lset`] | `include/lset.h`, `common/lset.cpp` |
//! | [`legacy_layers`] | legacy layer numbering helpers of `pcbnew/pcb_plot_params.cpp` and `pcbnew/pcb_io/kicad_legacy` |
//! | [`lib_id`] | `include/lib_id.h`, `common/lib_id.cpp`, `include/name_validation.h` |
//! | [`typeinfo`] | `include/core/typeinfo.h` |
//! | [`report_severity`] | `include/widgets/report_severity.h`, `common/widgets/report_severity.cpp` |
//!
//! The most used types are re-exported at the crate root.

pub mod base_set;
pub mod kiid;
pub mod layer_ids;
pub mod layer_range;
pub mod legacy_layers;
pub mod lib_id;
pub mod lseq;
pub mod lset;
pub mod mt19937;
pub mod report_severity;
mod sha1;
pub mod typeinfo;

pub use base_set::BaseSet;
pub use kiid::{Kiid, KiidGenerator, KiidNilSetReset, KiidPath, NILUUID, Timestamp};
pub use layer_ids::{
    Flashing, MAX_CU_LAYERS, MAX_USER_DEFINED_LAYERS, PCB_LAYER_ID_COUNT, PcbLayerId,
    UNDEFINED_LAYER, UNSELECTED_LAYER,
};
pub use layer_range::LayerRange;
pub use lib_id::LibId;
pub use lseq::Lseq;
pub use lset::Lset;
pub use report_severity::Severity;
pub use typeinfo::KicadT;
