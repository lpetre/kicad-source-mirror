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

//! Port of `pcbnew/drill/drill_span.h`: `HOLE_ATTRIBUTE`, `DRILL_LAYER_PAIR`, `DRILL_SPAN`.

use std::cmp::Ordering;

use kicad_core::layer_ids::{PcbLayerId, is_copper_layer_lower_than};

/// `HOLE_ATTRIBUTE`: hole attribute, mainly to identify vias and pads and add this info as
/// comment in NC drill files.
///
/// The declaration order is a sort key (`cmpHoleSorting`), so the derived `Ord` is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(i32)]
pub enum HoleAttribute {
    /// `HOLE_UNKNOWN`: uninitialized type.
    #[default]
    HoleUnknown,
    /// `HOLE_VIA_THROUGH`: a via hole (always plated) from top to bottom.
    HoleViaThrough,
    /// `HOLE_VIA_BURIED`: a via hole (always plated) not through hole.
    HoleViaBuried,
    /// `HOLE_VIA_BACKDRILL`: a via hole created by a backdrill operation.
    HoleViaBackdrill,
    /// `HOLE_PAD`: a plated or not plated pad hole.
    HolePad,
    /// `HOLE_PAD_CASTELLATED`: a plated castelleted pad hole.
    HolePadCastellated,
    /// `HOLE_PAD_PRESSFIT`: a plated press-fit pad hole.
    HolePadPressfit,
    /// `HOLE_MECHANICAL`: a mechanical pad (provided, not used).
    HoleMechanical,
    /// `HOLE_PAD_BACKDRILL`: a pad hole created by a backdrill operation.
    HolePadBackdrill,
}

/// `DRILL_LAYER_PAIR` (`std::pair<PCB_LAYER_ID, PCB_LAYER_ID>`).
pub type DrillLayerPair = (PcbLayerId, PcbLayerId);

/// `DRILL_SPAN`: a drilled layer range, plus whether it is a backdrill span and whether it is
/// the pseudo span of the non-plated file.
///
/// `Eq` compares the stored layers (not top / bottom), so a span and its reverse stay distinct;
/// `Ord` is the C++ `operator<` (used by the `std::set` of `EnumerateDrillSpans`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DrillSpan {
    pub start_layer: PcbLayerId,  // m_StartLayer
    pub end_layer: PcbLayerId,    // m_EndLayer
    pub is_backdrill: bool,       // m_IsBackdrill
    pub is_non_plated_file: bool, // m_IsNonPlatedFile
}

impl Default for DrillSpan {
    /// `DRILL_SPAN()`: `F_Cu` to `B_Cu`, not backdrill, plated.
    fn default() -> Self {
        DrillSpan {
            start_layer: PcbLayerId::FCu,
            end_layer: PcbLayerId::BCu,
            is_backdrill: false,
            is_non_plated_file: false,
        }
    }
}

impl DrillSpan {
    /// `DRILL_SPAN( aStartLayer, aEndLayer, aIsBackdrill, aIsNonPlated )`.
    pub const fn new(
        start_layer: PcbLayerId,
        end_layer: PcbLayerId,
        is_backdrill: bool,
        is_non_plated: bool,
    ) -> Self {
        DrillSpan {
            start_layer,
            end_layer,
            is_backdrill,
            is_non_plated_file: is_non_plated,
        }
    }

    /// `DRILL_SPAN::TopLayer()`.
    pub fn top_layer(&self) -> PcbLayerId {
        // B_Cu (id=2) is numerically less than inner layers (id>=4), but is physically
        // at the bottom of the stack. Use IsCopperLayerLowerThan for correct ordering.
        if is_copper_layer_lower_than(self.start_layer, self.end_layer) {
            self.end_layer
        } else {
            self.start_layer
        }
    }

    /// `DRILL_SPAN::BottomLayer()`.
    pub fn bottom_layer(&self) -> PcbLayerId {
        if is_copper_layer_lower_than(self.start_layer, self.end_layer) {
            self.start_layer
        } else {
            self.end_layer
        }
    }

    /// `DRILL_SPAN::DrillStartLayer()`.
    pub fn drill_start_layer(&self) -> PcbLayerId {
        self.start_layer
    }

    /// `DRILL_SPAN::DrillEndLayer()`.
    pub fn drill_end_layer(&self) -> PcbLayerId {
        self.end_layer
    }

    /// `DRILL_SPAN::Pair()`: `( TopLayer(), BottomLayer() )`.
    pub fn pair(&self) -> DrillLayerPair {
        (self.top_layer(), self.bottom_layer())
    }
}

impl Ord for DrillSpan {
    /// `DRILL_SPAN::operator<`: by top layer id, bottom layer id (numeric ids, so `B_Cu` sorts
    /// before the inner layers), backdrill first, non-plated first, then the stored start and
    /// end layers. Two spans are equivalent exactly when all four members are equal, so this
    /// is consistent with `Eq`.
    fn cmp(&self, other: &Self) -> Ordering {
        if self.top_layer() != other.top_layer() {
            return self.top_layer().cmp(&other.top_layer());
        }

        if self.bottom_layer() != other.bottom_layer() {
            return self.bottom_layer().cmp(&other.bottom_layer());
        }

        if self.is_backdrill != other.is_backdrill {
            return if self.is_backdrill {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }

        if self.is_non_plated_file != other.is_non_plated_file {
            return if self.is_non_plated_file {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }

        if self.start_layer != other.start_layer {
            return self.start_layer.cmp(&other.start_layer);
        }

        self.end_layer.cmp(&other.end_layer)
    }
}

impl PartialOrd for DrillSpan {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    const IN1: PcbLayerId = PcbLayerId::In1Cu;
    const IN2: PcbLayerId = PcbLayerId::In2Cu;
    const F: PcbLayerId = PcbLayerId::FCu;
    const B: PcbLayerId = PcbLayerId::BCu;

    #[test]
    fn top_bottom_follow_the_physical_stack() {
        let s = DrillSpan::new(B, IN1, true, false);
        assert_eq!(s.top_layer(), IN1);
        assert_eq!(s.bottom_layer(), B);
        assert_eq!(s.pair(), (IN1, B));
        assert_eq!(s.drill_start_layer(), B);
        let s = DrillSpan::new(F, IN2, false, false);
        assert_eq!(s.pair(), (F, IN2));
        assert_eq!(DrillSpan::default().pair(), (F, B));
    }

    #[test]
    fn reversed_span_is_distinct_but_same_pair() {
        let a = DrillSpan::new(F, IN2, true, false);
        let b = DrillSpan::new(IN2, F, true, false);
        assert_ne!(a, b);
        assert_eq!(a.pair(), b.pair());
        // Same top and bottom: start layer decides (F_Cu = 0 < In2_Cu = 6).
        assert!(a < b);
    }

    #[test]
    fn set_order_matches_operator_less() {
        let mut set = BTreeSet::new();
        set.insert(DrillSpan::new(IN1, IN2, false, false));
        set.insert(DrillSpan::new(F, IN1, false, false));
        set.insert(DrillSpan::new(IN2, B, false, false));
        set.insert(DrillSpan::new(F, IN1, true, false));
        set.insert(DrillSpan::new(B, IN2, true, false));
        set.insert(DrillSpan::new(F, IN1, false, false)); // duplicate
        let v: Vec<_> = set.into_iter().collect();
        assert_eq!(
            v,
            vec![
                // top F_Cu (0): bottom In1 (4); backdrill first
                DrillSpan::new(F, IN1, true, false),
                DrillSpan::new(F, IN1, false, false),
                // top In1 (4), bottom In2 (6)
                DrillSpan::new(IN1, IN2, false, false),
                // top In2 (6), bottom B_Cu (2); backdrill first
                DrillSpan::new(B, IN2, true, false),
                DrillSpan::new(IN2, B, false, false),
            ]
        );
    }
}
