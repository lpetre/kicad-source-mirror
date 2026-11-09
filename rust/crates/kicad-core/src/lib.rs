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
//! | [`base_units`] | `include/base_units.h` |
//! | [`minoptmax`] | `libs/core/include/core/minoptmax.h` |
//! | [`eda_units`] | `include/eda_units.h`, `common/eda_units.cpp` |
//! | [`string_utils`] | `include/string_utils.h`, `common/string_utils.cpp` |
//! | [`color4d`] | `include/gal/color4d.h`, `common/gal/color4d.cpp` |
//! | [`text_attributes`] | `include/font/text_attributes.h`, `common/font/text_attributes.cpp` |
//! | [`stroke_params`] | `include/stroke_params.h`, `include/stroke_params_parser.h`, `common/stroke_params.cpp` (data part) |
//! | [`title_block`] | `include/title_block.h`, `common/title_block.cpp` |
//! | [`page_info`] | `include/page_info.h`, `common/page_info.cpp` |
//! | [`json`] | nlohmann/json 3.11.3 parse / `dump()` / `get<T>()` / `json_pointer` as KiCad uses them |
//! | [`json_settings`] | `common/settings/json_settings.cpp`, `nested_settings.cpp` (document, `Get<T>`, migrations, nested load/store) |
//! | [`settings_parameters`] | `include/settings/parameters.h`, `common/settings/parameters.cpp` (`PARAM<T>` and friends) |
//! | [`eda_pattern_match`] | `include/eda_pattern_match.h`, `common/eda_pattern_match.cpp` (regex via `regress`, see [`eda_pattern_match::pcre`]) |
//! | [`text_vars`] | `ExpandTextVars` / `ResolveTextVars` / `FinalizeTextVarExpansion` / `NormalizeFilePathForTextVars` of `common/common.cpp` |
//! | [`units_provider`] | `include/units_provider.h` |
//! | [`case_insensitive_map`] | `include/case_insensitive_map.h` |
//! | [`printf`] | re-export of `kicad_sexpr::formatter::cformat`: printf / `{fmt}` exact `%f`, `%e`, `%g` |
//!
//! The most used types are re-exported at the crate root.

pub mod base_set;
pub mod base_units;
pub mod case_insensitive_map;
pub mod color4d;
pub mod eda_pattern_match;
pub mod eda_units;
pub mod json;
pub mod json_settings;
pub mod kiid;
pub mod layer_ids;
pub mod layer_range;
pub mod legacy_layers;
pub mod lib_id;
pub mod lseq;
pub mod lset;
pub mod minoptmax;
pub mod mt19937;
pub mod page_info;
pub mod plot_enums;
pub mod report_severity;
pub mod settings_parameters;
mod sha1;
pub mod string_utils;
pub mod stroke_params;
pub mod text_attributes;
pub mod text_vars;
pub mod title_block;
pub mod typeinfo;
pub mod units_provider;
mod wxstr;

/// printf / `{fmt}` exact double formatting (`%f`, `%e`, `%g`), shared with `kicad-sexpr`.
pub use kicad_sexpr::formatter::cformat as printf;

pub use base_units::{EdaIuScale, GERB_IU_SCALE, PCB_IU_SCALE, SCH_IU_SCALE};
pub use color4d::{Color4D, EdaColorT};
pub use eda_units::{EdaDataType, EdaUnits};
pub use minoptmax::MinOptMax;
pub use page_info::{PageInfo, PageSizeType};
pub use string_utils::EscapeContext;
pub use stroke_params::{LineStyle, StrokeParams};
pub use text_attributes::{GrTextHAlignT, GrTextVAlignT, TextAttributes};
pub use title_block::TitleBlock;

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
