// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2014 SoftPLC Corporation, Dick Hollenbeck <dick@softplc.com>
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

//! Port of `include/lset.h` and `common/lset.cpp` (`LSET`).
//!
//! `ToLAYER_ID`, which C++ also defines in `lset.cpp`, lives in [`crate::layer_ids`];
//! `GAL_SET` (GUI visibility set) is not ported.

use std::fmt;
use std::ops::{
    BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Deref, DerefMut, Not,
};
use std::sync::OnceLock;

use crate::base_set::{BaseSet, impl_bit_ops};
use crate::layer_ids::{
    MAX_CU_LAYERS, PCB_LAYER_ID_COUNT, PcbLayerId, is_copper_layer,
};
use crate::layer_range::LayerRange;
use crate::lseq::Lseq;

const F_CU: usize = PcbLayerId::FCu.0 as usize;
const B_CU: usize = PcbLayerId::BCu.0 as usize;

/// Port of `LSET`: a set of `PCB_LAYER_ID`s.
///
/// It can be converted to numerous purpose `LSEQ`s using the various member functions, most
/// of which are based on `Seq()`.  The advantage of converting to [`Lseq`] using purposeful
/// code, is it removes any dependency on order/sequence inherent in this set.
///
/// `LSET` derives from `BASE_SET`; the port wraps a [`BaseSet`] and dereferences to it, so all
/// `BaseSet` methods (`set`, `reset`, `test`, `count`, `any`, `fmt_hex`, `parse_hex`, ...) are
/// available and accept a [`PcbLayerId`] wherever they take a bit position.
///
/// Iterating an `Lset` (`for layer in &lset`) yields the contained layers in ascending
/// `PCB_LAYER_ID` order, like the C++ range-for over an `LSET`.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Lset(BaseSet);

impl Lset {
    /// Port of `LSET::LSET()`: create an empty (cleared) set of `PCB_LAYER_ID_COUNT` bits.
    pub fn new() -> Self {
        // all bits are set to zero in BASE_SET()
        Lset(BaseSet::new(PCB_LAYER_ID_COUNT as usize))
    }

    /// Port of `LSET::LSET( const BASE_SET& aOther )`.  The size of `other` is kept.
    pub fn from_base_set(other: BaseSet) -> Self {
        Lset(other)
    }

    /// Port of `LSET::LSET( std::initializer_list<PCB_LAYER_ID> aList )` and of
    /// `LSET::LSET( const std::vector<PCB_LAYER_ID>& aList )`.  Negative layers are skipped.
    pub fn from_layers(list: &[PcbLayerId]) -> Self {
        let mut set = Lset::new();

        for &layer in list {
            if layer.0 >= 0 {
                set.0.set(layer);
            }
        }

        set
    }

    /// Port of `LSET::LSET( const LSEQ& aSeq )`.  Negative layers are skipped.
    pub fn from_lseq(seq: &Lseq) -> Self {
        Self::from_layers(seq)
    }

    /// Port of `LSET::LSET( const LAYER_RANGE& aRange )`.
    ///
    /// As in C++ (which does not delegate to `LSET()` here) the resulting set starts with the
    /// `BASE_SET` default size of 64 bits and grows when a layer beyond bit 63 is set.
    pub fn from_layer_range(range: &LayerRange) -> Self {
        let mut set = Lset(BaseSet::default());

        for layer in range {
            if layer.0 >= 0 {
                set.0.set(layer);
            }
        }

        set
    }

    /// The underlying `BASE_SET`.
    pub fn as_base_set(&self) -> &BaseSet {
        &self.0
    }

    /// The underlying `BASE_SET`, mutable.
    pub fn as_base_set_mut(&mut self) -> &mut BaseSet {
        &mut self.0
    }

    /// Convert into the underlying `BASE_SET`.
    pub fn into_base_set(self) -> BaseSet {
        self.0
    }

    /// Port of `LSET::Contains`: see if the layer set contains a PCB layer.
    pub fn contains(&self, layer: PcbLayerId) -> bool {
        // At the moment, LSET cannot store negative layers, but PCB_LAYER_ID can contain them
        if layer.0 < 0 {
            return false;
        }

        self.0.test(layer)
    }

    /// Port of `LSET::ContainsAll`: see if this layer set contains all layers in another set.
    /// The two sets may have different sizes.
    pub fn contains_all(&self, layers: &Lset) -> bool {
        // C++ zero-extends the shorter set first; BaseSet::is_subset_of() does that implicitly.
        layers.0.is_subset_of(&self.0)
    }

    /// Port of `LSET::Name`: return the fixed name association with `layer_id`.
    ///
    /// These names must not be translated or changed.  They are used as tokens in the board
    /// file format because the ordinal value of the `PCB_LAYER_ID` enum was not stable over
    /// time.  See [`crate::layer_ids::layer_name`] for what should be used to display the
    /// default name of a layer in the GUI.
    pub fn name(layer_id: PcbLayerId) -> String {
        // using a switch to explicitly show the mapping more clearly
        let txt = match layer_id {
            PcbLayerId::FCu => "F.Cu",
            PcbLayerId::BCu => "B.Cu",

            // Technicals
            PcbLayerId::BAdhes => "B.Adhes",
            PcbLayerId::FAdhes => "F.Adhes",
            PcbLayerId::BPaste => "B.Paste",
            PcbLayerId::FPaste => "F.Paste",
            PcbLayerId::BSilkS => "B.SilkS",
            PcbLayerId::FSilkS => "F.SilkS",
            PcbLayerId::BMask => "B.Mask",
            PcbLayerId::FMask => "F.Mask",

            // Users
            PcbLayerId::DwgsUser => "Dwgs.User",
            PcbLayerId::CmtsUser => "Cmts.User",
            PcbLayerId::Eco1User => "Eco1.User",
            PcbLayerId::Eco2User => "Eco2.User",
            PcbLayerId::EdgeCuts => "Edge.Cuts",
            PcbLayerId::Margin => "Margin",

            // Footprint
            PcbLayerId::FCrtYd => "F.CrtYd",
            PcbLayerId::BCrtYd => "B.CrtYd",
            PcbLayerId::FFab => "F.Fab",
            PcbLayerId::BFab => "B.Fab",

            // Rescue
            PcbLayerId::Rescue => "Rescue",

            _ => {
                return if layer_id.0 < 0 {
                    "UNDEFINED".to_string()
                } else if (layer_id.0 & 1) != 0 {
                    let offset = (layer_id.0 - PcbLayerId::Rescue.0) / 2;
                    format!("User.{offset}")
                } else {
                    let offset = (layer_id.0 - PcbLayerId::BCu.0) / 2;
                    format!("In{offset}.Cu")
                };
            }
        };

        txt.to_string()
    }

    /// Port of `LSET::NameToLayer`: return the layer number from a (canonical) layer name, or
    /// -1 when the name is not a layer name.
    pub fn name_to_layer(name: &str) -> i32 {
        let known = match name {
            "F.Cu" => Some(PcbLayerId::FCu),
            "B.Cu" => Some(PcbLayerId::BCu),
            "F.Adhes" => Some(PcbLayerId::FAdhes),
            "B.Adhes" => Some(PcbLayerId::BAdhes),
            "F.Paste" => Some(PcbLayerId::FPaste),
            "B.Paste" => Some(PcbLayerId::BPaste),
            "F.SilkS" => Some(PcbLayerId::FSilkS),
            "B.SilkS" => Some(PcbLayerId::BSilkS),
            "F.Mask" => Some(PcbLayerId::FMask),
            "B.Mask" => Some(PcbLayerId::BMask),
            "Dwgs.User" => Some(PcbLayerId::DwgsUser),
            "Cmts.User" => Some(PcbLayerId::CmtsUser),
            "Eco1.User" => Some(PcbLayerId::Eco1User),
            "Eco2.User" => Some(PcbLayerId::Eco2User),
            "Edge.Cuts" => Some(PcbLayerId::EdgeCuts),
            "Margin" => Some(PcbLayerId::Margin),
            "F.CrtYd" => Some(PcbLayerId::FCrtYd),
            "B.CrtYd" => Some(PcbLayerId::BCrtYd),
            "F.Fab" => Some(PcbLayerId::FFab),
            "B.Fab" => Some(PcbLayerId::BFab),
            "Rescue" => Some(PcbLayerId::Rescue),
            _ => None,
        };

        if let Some(layer) = known {
            return layer.0;
        }

        if let Some(rest) = name.strip_prefix("User.") {
            if let Some(offset) = wx_to_long(rest) {
                if offset > 0 {
                    // int + long arithmetic, truncated to the int return type.
                    return i64::from(PcbLayerId::User1.0)
                        .wrapping_add((offset - 1).wrapping_mul(2)) as i32;
                }
            }
        }

        if let Some(rest) = name.strip_prefix("In") {
            // str_num.RemoveLast( 3 ); // Removes .Cu
            //
            // wxString::RemoveLast( n ) is Truncate( length() - n ): when the string is shorter
            // than n the unsigned subtraction wraps around and nothing is removed.
            let char_count = rest.chars().count();
            let str_num: String = if char_count >= 3 {
                rest.chars().take(char_count - 3).collect()
            } else {
                rest.to_string()
            };

            if let Some(offset) = wx_to_long(&str_num) {
                if offset > 0 {
                    return i64::from(PcbLayerId::In1Cu.0)
                        .wrapping_add((offset - 1).wrapping_mul(2)) as i32;
                }
            }
        }

        -1
    }

    /// Port of `LSET::IsBetween`: return true if `layer` is between `start` and `end`,
    /// inclusive.
    ///
    /// This is meant to take into account the direction of the layers and the fact that `B_Cu`
    /// comes before `In*_Cu`.
    ///
    /// Bug compatibility: when the larger of the two bounds is `B_Cu`, the C++ code reassigns
    /// the upper bound to `std::numeric_limits<PCB_LAYER_ID>::max() & ~1`, intending "the
    /// largest possible positive even number".  `std::numeric_limits` is not specialised for
    /// enumeration types, so that expression is 0, and consequently no inner layer is
    /// reported as being between `F_Cu` and `B_Cu`.  The port reproduces this.
    pub fn is_between(start: PcbLayerId, end: PcbLayerId, layer: PcbLayerId) -> bool {
        if layer == start || layer == end {
            return true;
        }

        let lo = start.0.min(end.0);
        let mut hi = start.0.max(end.0);
        let layer = layer.0;

        if hi == PcbLayerId::BCu.0 {
            // Reassign the end layer to the largest possible positive even number
            // (std::numeric_limits<PCB_LAYER_ID>::max() & ~1, which evaluates to 0, see above)
            hi = 0;
        }

        (layer & 1) == 0 && (layer >= lo) && (layer <= hi)
    }

    /// Port of `LSET::InternalCuMask`: return a complete set of internal copper layers which
    /// is all Cu layers except `F_Cu` and `B_Cu`.
    pub fn internal_cu_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::In1Cu,
                PcbLayerId::In2Cu,
                PcbLayerId::In3Cu,
                PcbLayerId::In4Cu,
                PcbLayerId::In5Cu,
                PcbLayerId::In6Cu,
                PcbLayerId::In7Cu,
                PcbLayerId::In8Cu,
                PcbLayerId::In9Cu,
                PcbLayerId::In10Cu,
                PcbLayerId::In11Cu,
                PcbLayerId::In12Cu,
                PcbLayerId::In13Cu,
                PcbLayerId::In14Cu,
                PcbLayerId::In15Cu,
                PcbLayerId::In16Cu,
                PcbLayerId::In17Cu,
                PcbLayerId::In18Cu,
                PcbLayerId::In19Cu,
                PcbLayerId::In20Cu,
                PcbLayerId::In21Cu,
                PcbLayerId::In22Cu,
                PcbLayerId::In23Cu,
                PcbLayerId::In24Cu,
                PcbLayerId::In25Cu,
                PcbLayerId::In26Cu,
                PcbLayerId::In27Cu,
                PcbLayerId::In28Cu,
                PcbLayerId::In29Cu,
                PcbLayerId::In30Cu,
            ])
        })
    }

    /// Port of `LSET::FrontAssembly`: return a complete set of all top assembly layers
    /// (`F_SilkS`, `F_Mask`, `F_Fab`, `F_CrtYd`).
    pub fn front_assembly() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::FSilkS,
                PcbLayerId::FMask,
                PcbLayerId::FFab,
                PcbLayerId::FCrtYd,
            ])
        })
    }

    /// Port of `LSET::BackAssembly`: return a complete set of all bottom assembly layers
    /// (`B_SilkS`, `B_Mask`, `B_Fab`, `B_CrtYd`).
    pub fn back_assembly() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::BSilkS,
                PcbLayerId::BMask,
                PcbLayerId::BFab,
                PcbLayerId::BCrtYd,
            ])
        })
    }

    /// Port of `LSET::AllCuMask( int aCuLayerCount )`: return a mask holding the requested
    /// number of Cu `PCB_LAYER_ID`s.
    pub fn all_cu_mask_n(cu_layer_count: i32) -> Lset {
        if cu_layer_count == MAX_CU_LAYERS {
            return Self::all_cu_mask().clone();
        }

        all_cu_mask_impl(cu_layer_count)
    }

    /// Port of `LSET::AllCuMask()`: return `AllCuMask( MAX_CU_LAYERS )`.
    pub fn all_cu_mask() -> &'static Lset {
        static SAVED_MAX: OnceLock<Lset> = OnceLock::new();

        SAVED_MAX.get_or_init(|| all_cu_mask_impl(MAX_CU_LAYERS))
    }

    /// Port of `LSET::ExternalCuMask`: return a mask holding the Front and Bottom layers.
    pub fn external_cu_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| Lset::from_layers(&[PcbLayerId::FCu, PcbLayerId::BCu]))
    }

    /// Port of `LSET::AllNonCuMask`: return a mask holding all layer minus CU layers.
    pub fn all_non_cu_mask() -> Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(all_non_cu_mask_impl).clone()
    }

    /// Port of `LSET::AllLayersMask`.
    pub fn all_layers_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            let mut set = Lset::new();
            set.0.set_all();
            set
        })
    }

    /// Port of `LSET::FrontTechMask`: return a mask holding all technical layers (no CU layer)
    /// on front side.
    pub fn front_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::FSilkS,
                PcbLayerId::FMask,
                PcbLayerId::FAdhes,
                PcbLayerId::FPaste,
                PcbLayerId::FCrtYd,
                PcbLayerId::FFab,
            ])
        })
    }

    /// Port of `LSET::FrontBoardTechMask`: return a mask holding technical layers used in a
    /// board fabrication (no CU layer) on front side.
    pub fn front_board_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::FSilkS,
                PcbLayerId::FMask,
                PcbLayerId::FAdhes,
                PcbLayerId::FPaste,
            ])
        })
    }

    /// Port of `LSET::BackTechMask`: return a mask holding all technical layers (no CU layer)
    /// on back side.
    pub fn back_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::BSilkS,
                PcbLayerId::BMask,
                PcbLayerId::BAdhes,
                PcbLayerId::BPaste,
                PcbLayerId::BCrtYd,
                PcbLayerId::BFab,
            ])
        })
    }

    /// Port of `LSET::BackBoardTechMask`: return a mask holding technical layers used in a
    /// board fabrication (no CU layer) on Back side.
    pub fn back_board_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::BSilkS,
                PcbLayerId::BMask,
                PcbLayerId::BAdhes,
                PcbLayerId::BPaste,
            ])
        })
    }

    /// Port of `LSET::AllTechMask`: return a mask holding all technical layers (no CU layer)
    /// on both side.
    pub fn all_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| Self::back_tech_mask() | Self::front_tech_mask())
    }

    /// Port of `LSET::AllBoardTechMask`: return a mask holding board technical layers (no CU
    /// layer) on both side.
    pub fn all_board_tech_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| Self::back_board_tech_mask() | Self::front_board_tech_mask())
    }

    /// Port of `LSET::FrontMask`: return a mask holding all technical layers and the external
    /// CU layer on front side.
    pub fn front_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            let mut set = Self::front_tech_mask().clone();
            set.0.set(PcbLayerId::FCu);
            set
        })
    }

    /// Port of `LSET::BackMask`: return a mask holding all technical layers and the external
    /// CU layer on back side.
    pub fn back_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            let mut set = Self::back_tech_mask().clone();
            set.0.set(PcbLayerId::BCu);
            set
        })
    }

    /// Port of `LSET::SideSpecificMask`.
    pub fn side_specific_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Self::back_tech_mask() | Self::front_tech_mask() | Self::all_cu_mask()
        })
    }

    /// Port of `LSET::UserMask`.
    pub fn user_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| {
            Lset::from_layers(&[
                PcbLayerId::DwgsUser,
                PcbLayerId::CmtsUser,
                PcbLayerId::Eco1User,
                PcbLayerId::Eco2User,
                PcbLayerId::EdgeCuts,
                PcbLayerId::Margin,
            ])
        })
    }

    /// Port of `LSET::PhysicalLayersMask`: return a mask holding all layers which are
    /// physically realized.  Equivalent to the copper layers + the board tech mask.
    pub fn physical_layers_mask() -> &'static Lset {
        static SAVED: OnceLock<Lset> = OnceLock::new();

        SAVED.get_or_init(|| Self::all_board_tech_mask() | Self::all_cu_mask())
    }

    /// Port of `LSET::UserDefinedLayersMask`: return a mask with the requested number of user
    /// defined layers.  The C++ default argument is `MAX_USER_DEFINED_LAYERS`.
    pub fn user_defined_layers_mask(user_defined_layer_count: i32) -> Lset {
        let mut ret = Lset::new();
        let mut layer = PcbLayerId::User1.0 as usize;

        for _ulayer in 1..=user_defined_layer_count {
            if layer > ret.size() {
                break;
            }

            ret.0.set(layer);
            layer += 2;
        }

        ret
    }

    /// Port of `LSET::CuStack`: return a sequence of copper layers in starting from the
    /// front/top and extending to the back/bottom.
    pub fn cu_stack(&self) -> Lseq {
        let mut ret = Lseq::new();

        ret.reserve(32);

        for layer in self.copper_layers() {
            ret.push(layer);
        }

        ret
    }

    /// Port of `LSET::TechAndUserUIOrder`: return the technical and user layers in the order
    /// shown in layer widget.
    pub fn tech_and_user_ui_order(&self) -> Lseq {
        let mut ret = self.seq_ordered(&[
            PcbLayerId::FAdhes,
            PcbLayerId::BAdhes,
            PcbLayerId::FPaste,
            PcbLayerId::BPaste,
            PcbLayerId::FSilkS,
            PcbLayerId::BSilkS,
            PcbLayerId::FMask,
            PcbLayerId::BMask,
            PcbLayerId::DwgsUser,
            PcbLayerId::CmtsUser,
            PcbLayerId::Eco1User,
            PcbLayerId::Eco2User,
            PcbLayerId::EdgeCuts,
            PcbLayerId::Margin,
            PcbLayerId::FCrtYd,
            PcbLayerId::BCrtYd,
            PcbLayerId::FFab,
            PcbLayerId::BFab,
        ]);

        for layer in self.non_copper_layers() {
            if layer >= PcbLayerId::User1 {
                ret.push(layer);
            }
        }

        ret
    }

    /// Port of `LSET::UIOrder`: return the copper, technical and user layers in the order
    /// shown in layer widget.
    pub fn ui_order(&self) -> Lseq {
        let mut order = self.cu_stack();
        let techuser = self.tech_and_user_ui_order();

        order.extend(techuser);

        order
    }

    /// Port of `LSET::Seq( const LSEQ& aSequence )`: return an `LSEQ` from the union of this
    /// `LSET` and a desired sequence.
    ///
    /// The `LSEQ` elements will be in the same order as in `sequence`, and the result will
    /// only contain layers which are present in this set.
    pub fn seq_ordered(&self, sequence: &[PcbLayerId]) -> Lseq {
        let mut ret = Lseq::new();

        for &layer in sequence {
            if self.0.test(layer) {
                ret.push(layer);
            }
        }

        ret
    }

    /// Port of `LSET::Seq()`: return a `LSEQ` from this `LSET` in ascending `PCB_LAYER_ID`
    /// order.
    ///
    /// Each `LSEQ` element will be in the same sequence as in `PCB_LAYER_ID` and only present
    /// in the resultant `LSEQ` if present in this set.  Therefore the sequence is subject to
    /// change, use it only when enumeration and not order is important.
    pub fn seq(&self) -> Lseq {
        let mut ret = Lseq::new();

        ret.reserve(self.count());

        for i in self.0.set_bits() {
            ret.push(PcbLayerId(i as i32));
        }

        ret
    }

    /// Port of `LSET::SeqStackupTop2Bottom`: generate a sequence of layers that represent a
    /// top to bottom stack of this set of layers.
    ///
    /// `selected_layer` is the layer to put at the top of stack when defined (the C++ default
    /// argument is `UNDEFINED_LAYER`).
    pub fn seq_stackup_top2bottom(&self, selected_layer: PcbLayerId) -> Lseq {
        let base_sequence = self.seq_ordered(&[
            PcbLayerId::EdgeCuts,
            PcbLayerId::Margin,
            PcbLayerId::DwgsUser,
            PcbLayerId::CmtsUser,
            PcbLayerId::Eco1User,
            PcbLayerId::Eco2User,
        ]);

        let top_tech_sequence = self.seq_ordered(&[
            PcbLayerId::FFab,
            PcbLayerId::FSilkS,
            PcbLayerId::FPaste,
            PcbLayerId::FAdhes,
            PcbLayerId::FMask,
            PcbLayerId::FCrtYd,
        ]);

        let bottom_tech_sequence = self.seq_ordered(&[
            PcbLayerId::BCrtYd,
            PcbLayerId::BMask,
            PcbLayerId::BAdhes,
            PcbLayerId::BPaste,
            PcbLayerId::BSilkS,
            PcbLayerId::BFab,
        ]);

        let mut seq = self.seq_ordered(&base_sequence);

        for layer in self.non_copper_layers() {
            if layer >= PcbLayerId::User1 {
                seq.push(layer);
            }
        }

        seq.extend(top_tech_sequence);

        for layer in self.copper_layers() {
            seq.push(layer);
        }

        seq.extend(bottom_tech_sequence);

        if selected_layer != PcbLayerId::UndefinedLayer {
            if let Some(pos) = seq.iter().position(|&l| l == selected_layer) {
                seq.remove(pos);
                seq.insert(0, selected_layer);
            }
        }

        seq
    }

    /// Port of `LSET::SeqStackupForPlotting`: return the sequence that is typical for a
    /// bottom-to-top stack-up.
    ///
    /// For instance, to plot multiple layers in a single image, the top layers output last.
    pub fn seq_stackup_for_plotting(&self) -> Lseq {
        // bottom-to-top stack-up layers
        // Note that the bottom technical layers are flipped so that when plotting a bottom-side
        // view, they appear in the correct sequence.
        let bottom_tech_sequence = self.seq_ordered(&[
            PcbLayerId::BCu,
            PcbLayerId::BMask,
            PcbLayerId::BPaste,
            PcbLayerId::BSilkS,
            PcbLayerId::BAdhes,
            PcbLayerId::BCrtYd,
            PcbLayerId::BFab,
        ]);

        // Copper layers go here

        let top_tech_sequence = self.seq_ordered(&[
            PcbLayerId::FMask,
            PcbLayerId::FPaste,
            PcbLayerId::FSilkS,
            PcbLayerId::FAdhes,
            PcbLayerId::FCrtYd,
            PcbLayerId::FFab,
        ]);

        let user_sequence = self.seq_ordered(&[
            PcbLayerId::DwgsUser,
            PcbLayerId::CmtsUser,
            PcbLayerId::Eco1User,
            PcbLayerId::Eco2User,
        ]);

        // User layers go here

        let base_sequence = self.seq_ordered(&[PcbLayerId::Margin, PcbLayerId::EdgeCuts]);

        let mut seq = self.seq_ordered(&bottom_tech_sequence);

        let mut temp_layers: Vec<PcbLayerId> = Vec::new();

        // We are going to reverse the copper layers and then add them to the sequence
        // because the plotting order is bottom-to-top
        for layer in self.copper_layers() {
            // Skip B_Cu because it is already in the sequence (if it exists)
            if layer != PcbLayerId::BCu {
                temp_layers.push(layer);
            }
        }

        for &layer in temp_layers.iter().rev() {
            seq.push(layer);
        }

        seq.extend(top_tech_sequence);

        seq.extend(user_sequence);

        temp_layers.clear();

        for layer in self.non_copper_layers() {
            if layer >= PcbLayerId::User1 {
                temp_layers.push(layer);
            }
        }

        for &layer in temp_layers.iter().rev() {
            seq.push(layer);
        }

        seq.extend(base_sequence);

        seq
    }

    /// Port of `LSET::RunOnLayers`: execute a function on each layer of the `LSET`.
    pub fn run_on_layers(&self, mut function: impl FnMut(PcbLayerId)) {
        for ii in self.0.set_bits() {
            function(PcbLayerId(ii as i32));
        }
    }

    /// Port of `LSET::ExtractLayer`: find the first set `PCB_LAYER_ID`.
    ///
    /// Returns `UNDEFINED_LAYER` if more than one is set or `UNSELECTED_LAYER` if none is set.
    pub fn extract_layer(&self) -> PcbLayerId {
        let set_count = self.count();

        if set_count == 0 {
            return PcbLayerId::UnselectedLayer;
        } else if set_count > 1 {
            return PcbLayerId::UndefinedLayer;
        }

        match self.0.find_first() {
            Some(i) => PcbLayerId(i as i32),
            // set_count was verified as 1 above, what did you break?
            None => PcbLayerId::UndefinedLayer,
        }
    }

    /// Port of `LSET::FlipStandardLayers`: flip the layers in this set.
    ///
    /// BACK and FRONT copper layers, mask, paste, solder layers are swapped; internal layers
    /// are flipped only if the copper layers count is known.
    ///
    /// `copper_layers_count` is the number of copper layers; if 0 (in fact if < 4, the C++
    /// default argument is 0) internal layers will be not flipped because the layer count is
    /// not known.
    pub fn flip_standard_layers(&mut self, copper_layers_count: i32) -> &mut Self {
        let mut old_mask = self.clone();

        self.0.reset_all();

        // Mapping for Copper and Non-Copper layers, in the iteration order of the C++
        // std::map<PCB_LAYER_ID, PCB_LAYER_ID> (ascending key).
        const FLIP_MAP: [(PcbLayerId, PcbLayerId); 14] = [
            (PcbLayerId::FCu, PcbLayerId::BCu),
            (PcbLayerId::FMask, PcbLayerId::BMask),
            (PcbLayerId::BCu, PcbLayerId::FCu),
            (PcbLayerId::BMask, PcbLayerId::FMask),
            (PcbLayerId::FSilkS, PcbLayerId::BSilkS),
            (PcbLayerId::BSilkS, PcbLayerId::FSilkS),
            (PcbLayerId::FAdhes, PcbLayerId::BAdhes),
            (PcbLayerId::BAdhes, PcbLayerId::FAdhes),
            (PcbLayerId::FPaste, PcbLayerId::BPaste),
            (PcbLayerId::BPaste, PcbLayerId::FPaste),
            (PcbLayerId::BCrtYd, PcbLayerId::FCrtYd),
            (PcbLayerId::FCrtYd, PcbLayerId::BCrtYd),
            (PcbLayerId::BFab, PcbLayerId::FFab),
            (PcbLayerId::FFab, PcbLayerId::BFab),
        ];

        for (first, second) in FLIP_MAP {
            if old_mask.0.test(first) {
                self.0.set(second);
            }

            old_mask.0.set_value(first, false);
        }

        if copper_layers_count >= 4 {
            let internal_mask = &old_mask & Self::internal_cu_mask();
            let inner_layer_count = copper_layers_count - 2;

            for ii in 1..=inner_layer_count {
                // int arithmetic in C++; widen so absurd layer counts cannot overflow.
                let from = (i64::from(inner_layer_count) - i64::from(ii) + 1) * 2 + B_CU as i64;

                if from < internal_mask.size() as i64 && internal_mask.0.test(from as usize) {
                    self.0.set(ii as usize * 2 + B_CU);
                }
            }
        }

        old_mask.clear_copper_layers();

        // Copy across any remaining, non-side-specific layers
        for layer in &old_mask {
            self.0.set(layer);
        }

        self
    }

    /// Port of `LSET::LayerCount`: return the number of layers between `start` and `end`,
    /// inclusive.
    pub fn layer_count(start: PcbLayerId, end: PcbLayerId, copper_layer_count: i32) -> i32 {
        let mut start_i = start.0;
        let mut end_i = end.0;

        // Both layers need to be copper
        if !(is_copper_layer(start) && is_copper_layer(end)) {
            return copper_layer_count;
        }

        if start == PcbLayerId::BCu {
            std::mem::swap(&mut start_i, &mut end_i);
        }

        if start == end {
            return 1;
        }

        if start == PcbLayerId::FCu {
            if end == PcbLayerId::BCu {
                return copper_layer_count;
            } else {
                return (end_i - start_i) / 2 - 1;
            }
        } else if end == PcbLayerId::BCu {
            // Add 1 for the B_Cu layer
            return copper_layer_count.wrapping_sub(start_i / 2).wrapping_add(1);
        }

        (end_i - start_i) / 2
    }

    /// Port of `LSET::ClearCopperLayers`: clear the copper layers in this set.
    pub fn clear_copper_layers(&mut self) -> &mut Self {
        for ii in (0..self.size()).step_by(2) {
            self.0.reset(ii);
        }

        self
    }

    /// Port of `LSET::ClearNonCopperLayers`: clear the non-copper layers in this set.
    pub fn clear_non_copper_layers(&mut self) -> &mut Self {
        for ii in (1..self.size()).step_by(2) {
            self.0.reset(ii);
        }

        self
    }

    /// Port of `LSET::ClearUserDefinedLayers`: clear the user defined layers in this set.
    pub fn clear_user_defined_layers(&mut self) -> &mut Self {
        for ii in (PcbLayerId::User1.0 as usize..self.size()).step_by(2) {
            self.0.reset(ii);
        }

        self
    }

    /// Port of the `LSET::begin()` .. `end()` iteration (`all_set_layers_iterator`): the
    /// layers of the set in ascending `PCB_LAYER_ID` order.
    pub fn iter(&self) -> LsetIter<'_> {
        LsetIter {
            inner: self.0.set_bits(),
        }
    }

    /// Port of the `copper_layers_begin()` .. `copper_layers_end()` iteration
    /// (`LSET::copper_layers_iterator`): the copper layers of the set in stack order, `F_Cu`
    /// first, then the inner layers, `B_Cu` last.
    pub fn copper_layers(&self) -> CopperLayersIter<'_> {
        CopperLayersIter::new(&self.0, 0)
    }

    /// Port of the `non_copper_layers_begin()` .. `non_copper_layers_end()` iteration
    /// (`LSET::non_copper_layers_iterator`): the non-copper layers of the set in ascending
    /// `PCB_LAYER_ID` order.
    pub fn non_copper_layers(&self) -> NonCopperLayersIter<'_> {
        NonCopperLayersIter::new(&self.0, 0)
    }
}

/// Port of the free function `allCuMask` of `lset.cpp`.
fn all_cu_mask_impl(cu_layer_count: i32) -> Lset {
    let mut ret = Lset::new();

    // F_Cu and B_Cu are copper layers, so the range can always be built.
    if let Ok(range) = LayerRange::new(PcbLayerId::FCu, PcbLayerId::BCu, cu_layer_count) {
        for layer in range {
            ret.0.set(layer);
        }
    }

    ret
}

/// Port of the free function `allNonCuMask` of `lset.cpp`.
fn all_non_cu_mask_impl() -> Lset {
    let mut mask = Lset::new();
    mask.0.set_all();

    let copper: Vec<PcbLayerId> = mask.copper_layers().collect();

    for layer in copper {
        mask.0.reset(layer);
    }

    mask
}

/// Port of `wxString::ToLong( long* val, int base = 10 )` as used by `LSET::NameToLayer`.
///
/// Like `wcstol` this accepts leading (C locale) white space and an optional sign; the
/// conversion only succeeds when at least one digit was read, the whole string was consumed
/// and the value fits a (64 bit) `long`.  As `wxString::ToLong` works on the C string, an
/// embedded NUL character ends the text.
fn wx_to_long(s: &str) -> Option<i64> {
    let text = match s.find('\0') {
        Some(pos) => &s[..pos],
        None => s,
    };

    let mut chars = text
        .trim_start_matches([' ', '\t', '\n', '\u{b}', '\u{c}', '\r'])
        .chars()
        .peekable();

    let negative = match chars.peek() {
        Some('-') => {
            chars.next();
            true
        }
        Some('+') => {
            chars.next();
            false
        }
        _ => false,
    };

    let mut value: i64 = 0;
    let mut any_digit = false;

    for c in chars {
        let digit = i64::from(c.to_digit(10).filter(|_| c.is_ascii())?);

        // Accumulate as a negative number so that LONG_MIN is representable.
        value = value.checked_mul(10)?.checked_sub(digit)?;
        any_digit = true;
    }

    if !any_digit {
        return None;
    }

    if negative { Some(value) } else { value.checked_neg() }
}

impl Default for Lset {
    fn default() -> Self {
        Lset::new()
    }
}

impl Deref for Lset {
    type Target = BaseSet;

    fn deref(&self) -> &BaseSet {
        &self.0
    }
}

impl DerefMut for Lset {
    fn deref_mut(&mut self) -> &mut BaseSet {
        &mut self.0
    }
}

impl fmt::Debug for Lset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Lset")?;
        f.debug_list().entries(self.iter()).finish()
    }
}

impl From<BaseSet> for Lset {
    fn from(other: BaseSet) -> Self {
        Lset::from_base_set(other)
    }
}

impl From<Lset> for BaseSet {
    fn from(set: Lset) -> Self {
        set.0
    }
}

impl From<&[PcbLayerId]> for Lset {
    fn from(list: &[PcbLayerId]) -> Self {
        Lset::from_layers(list)
    }
}

impl<const N: usize> From<[PcbLayerId; N]> for Lset {
    fn from(list: [PcbLayerId; N]) -> Self {
        Lset::from_layers(&list)
    }
}

impl From<&Vec<PcbLayerId>> for Lset {
    fn from(list: &Vec<PcbLayerId>) -> Self {
        Lset::from_layers(list)
    }
}

impl From<&Lseq> for Lset {
    fn from(seq: &Lseq) -> Self {
        Lset::from_lseq(seq)
    }
}

impl From<&LayerRange> for Lset {
    fn from(range: &LayerRange) -> Self {
        Lset::from_layer_range(range)
    }
}

impl FromIterator<PcbLayerId> for Lset {
    fn from_iter<I: IntoIterator<Item = PcbLayerId>>(iter: I) -> Self {
        let mut set = Lset::new();

        for layer in iter {
            if layer.0 >= 0 {
                set.0.set(layer);
            }
        }

        set
    }
}

impl<'a> IntoIterator for &'a Lset {
    type Item = PcbLayerId;
    type IntoIter = LsetIter<'a>;

    fn into_iter(self) -> LsetIter<'a> {
        self.iter()
    }
}

impl BitAndAssign<&Lset> for Lset {
    fn bitand_assign(&mut self, rhs: &Lset) {
        self.0 &= &rhs.0;
    }
}

impl BitOrAssign<&Lset> for Lset {
    fn bitor_assign(&mut self, rhs: &Lset) {
        self.0 |= &rhs.0;
    }
}

impl BitXorAssign<&Lset> for Lset {
    fn bitxor_assign(&mut self, rhs: &Lset) {
        self.0 ^= &rhs.0;
    }
}

impl_bit_ops!(Lset, BitAnd, bitand, BitAndAssign, bitand_assign);
impl_bit_ops!(Lset, BitOr, bitor, BitOrAssign, bitor_assign);
impl_bit_ops!(Lset, BitXor, bitxor, BitXorAssign, bitxor_assign);

impl Not for &Lset {
    type Output = Lset;

    fn not(self) -> Lset {
        Lset(!&self.0)
    }
}

impl Not for Lset {
    type Output = Lset;

    fn not(self) -> Lset {
        Lset(!self.0)
    }
}

/// Iterator over the layers of an [`Lset`] in ascending order
/// (`LSET::all_set_layers_iterator`).
#[derive(Clone)]
pub struct LsetIter<'a> {
    inner: crate::base_set::SetBitsIter<'a>,
}

impl Iterator for LsetIter<'_> {
    type Item = PcbLayerId;

    fn next(&mut self) -> Option<PcbLayerId> {
        self.inner.next().map(|i| PcbLayerId(i as i32))
    }
}

/// Iterator over the copper layers of an [`Lset`] in stack order
/// (`LSET::copper_layers_iterator`).
#[derive(Clone)]
pub struct CopperLayersIter<'a> {
    base_set: &'a BaseSet,
    index: usize,
}

impl<'a> CopperLayersIter<'a> {
    /// Port of `LSET::copper_layers_iterator::copper_layers_iterator`.
    fn new(set: &'a BaseSet, index: usize) -> Self {
        let mut it = CopperLayersIter {
            base_set: set,
            index: (index + 1) & !1,
        };

        it.advance_to_next_set_copper_bit();
        it
    }

    /// Port of `LSET::copper_layers_iterator::next_copper_layer`.
    fn next_copper_layer(&mut self) {
        if self.index == F_CU {
            self.index += 4;
        } else if self.index == B_CU {
            self.index = self.base_set.size();
        } else {
            self.index += 2;

            if self.index >= self.base_set.size() {
                self.index = B_CU;
            }
        }
    }

    /// Port of `LSET::copper_layers_iterator::advance_to_next_set_copper_bit`.
    fn advance_to_next_set_copper_bit(&mut self) {
        while self.index < self.base_set.size() && !self.base_set.test(self.index) {
            self.next_copper_layer();
        }
    }
}

impl Iterator for CopperLayersIter<'_> {
    type Item = PcbLayerId;

    fn next(&mut self) -> Option<PcbLayerId> {
        // The C++ loop runs until the iterator equals copper_layers_end(), whose index is the
        // set size rounded up to an even number.  For the even sizes that occur in practice
        // that is "index == size"; for a set of odd size the C++ iteration never terminates
        // (see PORT_STATUS.md), the port stops once the index leaves the set.
        if self.index >= self.base_set.size() {
            return None;
        }

        let layer = PcbLayerId(self.index as i32);

        self.next_copper_layer();
        self.advance_to_next_set_copper_bit();

        Some(layer)
    }
}

/// Iterator over the non-copper layers of an [`Lset`] in ascending order
/// (`LSET::non_copper_layers_iterator`).
#[derive(Clone)]
pub struct NonCopperLayersIter<'a> {
    base_set: &'a BaseSet,
    index: usize,
}

impl<'a> NonCopperLayersIter<'a> {
    /// Port of `LSET::non_copper_layers_iterator::non_copper_layers_iterator`.
    fn new(set: &'a BaseSet, index: usize) -> Self {
        let mut it = NonCopperLayersIter {
            base_set: set,
            index,
        };

        it.advance_to_next_set_non_copper_bit();
        it
    }

    /// Port of `LSET::non_copper_layers_iterator::advance_to_next_set_non_copper_bit`.
    fn advance_to_next_set_non_copper_bit(&mut self) {
        while self.index < self.base_set.size()
            && (self.index % 2 != 1 || !self.base_set.test(self.index))
        {
            self.index += 1;
        }
    }
}

impl Iterator for NonCopperLayersIter<'_> {
    type Item = PcbLayerId;

    fn next(&mut self) -> Option<PcbLayerId> {
        if self.index >= self.base_set.size() {
            return None;
        }

        let layer = PcbLayerId(self.index as i32);

        self.index += 1;
        self.advance_to_next_set_non_copper_bit();

        Some(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wx_to_long_semantics() {
        assert_eq!(wx_to_long("12"), Some(12));
        assert_eq!(wx_to_long("  12"), Some(12));
        assert_eq!(wx_to_long("+7"), Some(7));
        assert_eq!(wx_to_long("-7"), Some(-7));
        assert_eq!(wx_to_long(""), None);
        assert_eq!(wx_to_long(" "), None);
        assert_eq!(wx_to_long("+"), None);
        assert_eq!(wx_to_long("12 "), None);
        assert_eq!(wx_to_long("1x"), None);
        assert_eq!(wx_to_long("0x10"), None);
        assert_eq!(wx_to_long("9223372036854775807"), Some(i64::MAX));
        assert_eq!(wx_to_long("-9223372036854775808"), Some(i64::MIN));
        assert_eq!(wx_to_long("9223372036854775808"), None);
        assert_eq!(wx_to_long("5\0junk"), Some(5));
        assert_eq!(wx_to_long("\u{664}"), None); // ARABIC-INDIC DIGIT FOUR is not a C digit
    }

    #[test]
    fn name_round_trip() {
        for id in 0..PCB_LAYER_ID_COUNT {
            let layer = PcbLayerId(id);
            assert_eq!(Lset::name_to_layer(&Lset::name(layer)), id, "{layer:?}");
        }

        assert_eq!(Lset::name(PcbLayerId::UndefinedLayer), "UNDEFINED");
        assert_eq!(Lset::name(PcbLayerId::In30Cu), "In30.Cu");
        assert_eq!(Lset::name(PcbLayerId::User45), "User.45");
        assert_eq!(Lset::name_to_layer("UNDEFINED"), -1);
        assert_eq!(Lset::name_to_layer("F.Silkscreen"), -1);
        assert_eq!(Lset::name_to_layer("User.0"), -1);
        assert_eq!(Lset::name_to_layer("In0.Cu"), -1);
        // wxString::RemoveLast( 3 ) leaves strings shorter than 3 characters alone.
        assert_eq!(Lset::name_to_layer("In1"), PcbLayerId::In1Cu.0);
        assert_eq!(Lset::name_to_layer("In12"), PcbLayerId::In12Cu.0);
        assert_eq!(Lset::name_to_layer("In123"), -1);
        assert_eq!(Lset::name_to_layer("Inner1.Cu"), -1);
    }

    #[test]
    fn is_between_bug_compatibility() {
        use PcbLayerId as L;

        assert!(Lset::is_between(L::FCu, L::BCu, L::FCu));
        assert!(Lset::is_between(L::FCu, L::BCu, L::BCu));
        // See the documentation of Lset::is_between().
        assert!(!Lset::is_between(L::FCu, L::BCu, L::In1Cu));
        assert!(Lset::is_between(L::In1Cu, L::In3Cu, L::In2Cu));
        assert!(Lset::is_between(L::In3Cu, L::In1Cu, L::In2Cu));
        assert!(!Lset::is_between(L::In1Cu, L::In3Cu, L::In4Cu));
        assert!(!Lset::is_between(L::In1Cu, L::In3Cu, L::FSilkS));
        assert!(Lset::is_between(L::FCu, L::In3Cu, L::BCu));
    }

    #[test]
    fn copper_iteration_order() {
        let all = Lset::all_layers_mask();
        let stack = all.cu_stack();

        assert_eq!(stack.len(), 64);
        assert_eq!(stack[0], PcbLayerId::FCu);
        assert_eq!(stack[1], PcbLayerId::In1Cu);
        assert_eq!(stack[62], PcbLayerId(126));
        assert_eq!(stack[63], PcbLayerId::BCu);

        // An odd sized set terminates (the C++ iterator would not).
        let mut odd = BaseSet::new(7);
        odd.set(0usize).set(2usize).set(6usize);
        let odd = Lset::from_base_set(odd);
        assert_eq!(
            odd.cu_stack(),
            [PcbLayerId::FCu, PcbLayerId(6), PcbLayerId::BCu]
        );
    }

    #[test]
    fn masks() {
        assert_eq!(Lset::all_cu_mask().count(), 32);
        assert_eq!(Lset::all_cu_mask_n(2), *Lset::external_cu_mask());
        assert_eq!(Lset::all_cu_mask_n(0), *Lset::external_cu_mask());
        assert_eq!(Lset::all_cu_mask_n(4).count(), 4);
        assert_eq!(Lset::all_cu_mask_n(MAX_CU_LAYERS), *Lset::all_cu_mask());
        assert_eq!(
            &(Lset::internal_cu_mask() | Lset::external_cu_mask()),
            Lset::all_cu_mask()
        );
        assert_eq!(Lset::all_non_cu_mask().count(), 64);
        assert!(!Lset::all_non_cu_mask().intersects(Lset::all_cu_mask()));
        assert_eq!(Lset::all_layers_mask().count(), 128);
        assert_eq!(Lset::user_defined_layers_mask(45).count(), 45);
        assert_eq!(Lset::user_defined_layers_mask(100).count(), 45);
        assert_eq!(Lset::user_defined_layers_mask(0).count(), 0);
        assert_eq!(
            Lset::user_defined_layers_mask(2),
            Lset::from([PcbLayerId::User1, PcbLayerId::User2])
        );
        assert_eq!(Lset::physical_layers_mask().count(), 32 + 8);
        assert_eq!(Lset::side_specific_mask().count(), 32 + 12);
        assert_eq!(Lset::front_mask().count(), 7);
        assert_eq!(Lset::back_mask().count(), 7);
        assert_eq!(Lset::all_tech_mask().count(), 12);
        assert_eq!(Lset::user_mask().count(), 6);
        assert_eq!(Lset::front_assembly().count(), 4);
        assert_eq!(Lset::back_assembly().count(), 4);
    }

    #[test]
    fn layer_range_constructor_keeps_base_set_default_size() {
        let range = LayerRange::new(PcbLayerId::FCu, PcbLayerId::BCu, 4).unwrap();
        let set = Lset::from_layer_range(&range);

        assert_eq!(set.size(), 64);
        assert_eq!(
            set.seq(),
            [
                PcbLayerId::FCu,
                PcbLayerId::BCu,
                PcbLayerId::In1Cu,
                PcbLayerId::In2Cu
            ]
        );
    }

    #[test]
    fn layer_count() {
        use PcbLayerId as L;

        assert_eq!(Lset::layer_count(L::FCu, L::BCu, 6), 6);
        assert_eq!(Lset::layer_count(L::FCu, L::FCu, 6), 1);
        assert_eq!(Lset::layer_count(L::FCu, L::In2Cu, 6), 2);
        assert_eq!(Lset::layer_count(L::In1Cu, L::BCu, 6), 5);
        assert_eq!(Lset::layer_count(L::In1Cu, L::In3Cu, 6), 2);
        assert_eq!(Lset::layer_count(L::FMask, L::BCu, 6), 6);
    }

    #[test]
    fn clear_helpers_and_debug() {
        let mut set = Lset::from([
            PcbLayerId::FCu,
            PcbLayerId::FMask,
            PcbLayerId::User1,
            PcbLayerId::In2Cu,
        ]);

        assert_eq!(format!("{set:?}"), "Lset[FCu, FMask, In2Cu, User1]");

        let mut a = set.clone();
        a.clear_copper_layers();
        assert_eq!(a, Lset::from([PcbLayerId::FMask, PcbLayerId::User1]));

        let mut b = set.clone();
        b.clear_non_copper_layers();
        assert_eq!(b, Lset::from([PcbLayerId::FCu, PcbLayerId::In2Cu]));

        set.clear_user_defined_layers();
        assert_eq!(
            set,
            Lset::from([PcbLayerId::FCu, PcbLayerId::FMask, PcbLayerId::In2Cu])
        );

        let mut collected = Vec::new();
        set.run_on_layers(|l| collected.push(l));
        assert_eq!(collected, set.seq().0);
    }
}
