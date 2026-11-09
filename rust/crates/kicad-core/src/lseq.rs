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

//! Port of `include/lseq.h` and `common/lseq.cpp` (`LSEQ`).

use std::ops::{Deref, DerefMut};

use crate::layer_ids::PcbLayerId;

/// Port of `LSEQ` (a `std::vector<PCB_LAYER_ID>`): a sequence (and therefore also a set) of
/// `PCB_LAYER_ID`s.  A sequence provides a certain order.
///
/// The type dereferences to its `Vec<PcbLayerId>`, like the C++ class derives from
/// `std::vector<PCB_LAYER_ID>`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Lseq(pub Vec<PcbLayerId>);

impl Lseq {
    /// Port of `LSEQ::LSEQ()`: an empty sequence.
    pub fn new() -> Self {
        Lseq(Vec::new())
    }

    /// Port of `LSEQ::TestLayers`: the signed distance from `rhs` to `lhs` in this sequence,
    /// i.e. (position of `lhs`) - (position of `rhs`).  A layer that is not part of the
    /// sequence counts as positioned at the end of the sequence.
    pub fn test_layers(&self, rhs: PcbLayerId, lhs: PcbLayerId) -> i32 {
        if rhs == lhs {
            return 0;
        }

        let position = |layer: PcbLayerId| -> i32 {
            self.0
                .iter()
                .position(|&l| l == layer)
                .unwrap_or(self.0.len()) as i32
        };

        position(lhs) - position(rhs)
    }
}

impl Deref for Lseq {
    type Target = Vec<PcbLayerId>;

    fn deref(&self) -> &Vec<PcbLayerId> {
        &self.0
    }
}

impl DerefMut for Lseq {
    fn deref_mut(&mut self) -> &mut Vec<PcbLayerId> {
        &mut self.0
    }
}

impl From<Vec<PcbLayerId>> for Lseq {
    fn from(layers: Vec<PcbLayerId>) -> Self {
        Lseq(layers)
    }
}

impl From<&[PcbLayerId]> for Lseq {
    fn from(layers: &[PcbLayerId]) -> Self {
        Lseq(layers.to_vec())
    }
}

impl<const N: usize> From<[PcbLayerId; N]> for Lseq {
    /// Port of `LSEQ::LSEQ( std::initializer_list<PCB_LAYER_ID> )`.
    fn from(layers: [PcbLayerId; N]) -> Self {
        Lseq(layers.to_vec())
    }
}

impl From<Lseq> for Vec<PcbLayerId> {
    fn from(seq: Lseq) -> Self {
        seq.0
    }
}

impl FromIterator<PcbLayerId> for Lseq {
    /// Port of `LSEQ::LSEQ( InputIterator aStart, InputIterator aEnd )`.
    fn from_iter<I: IntoIterator<Item = PcbLayerId>>(iter: I) -> Self {
        Lseq(iter.into_iter().collect())
    }
}

impl IntoIterator for Lseq {
    type Item = PcbLayerId;
    type IntoIter = std::vec::IntoIter<PcbLayerId>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Lseq {
    type Item = &'a PcbLayerId;
    type IntoIter = std::slice::Iter<'a, PcbLayerId>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl PartialEq<[PcbLayerId]> for Lseq {
    fn eq(&self, other: &[PcbLayerId]) -> bool {
        self.0.as_slice() == other
    }
}

impl PartialEq<Vec<PcbLayerId>> for Lseq {
    fn eq(&self, other: &Vec<PcbLayerId>) -> bool {
        &self.0 == other
    }
}

impl<const N: usize> PartialEq<[PcbLayerId; N]> for Lseq {
    fn eq(&self, other: &[PcbLayerId; N]) -> bool {
        self.0.as_slice() == other.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layers_distance() {
        let seq = Lseq::from([PcbLayerId::FCu, PcbLayerId::In1Cu, PcbLayerId::BCu]);

        assert_eq!(seq.test_layers(PcbLayerId::FCu, PcbLayerId::FCu), 0);
        assert_eq!(seq.test_layers(PcbLayerId::FCu, PcbLayerId::BCu), 2);
        assert_eq!(seq.test_layers(PcbLayerId::BCu, PcbLayerId::In1Cu), -1);

        // A missing layer sits at end().
        assert_eq!(seq.test_layers(PcbLayerId::FCu, PcbLayerId::FMask), 3);
        assert_eq!(seq.test_layers(PcbLayerId::FMask, PcbLayerId::FCu), -3);
        assert_eq!(seq.test_layers(PcbLayerId::FMask, PcbLayerId::BMask), 0);
    }

    #[test]
    fn conversions() {
        let seq: Lseq = [PcbLayerId::FCu, PcbLayerId::BCu].into_iter().collect();
        assert_eq!(seq.len(), 2);
        assert_eq!(seq, [PcbLayerId::FCu, PcbLayerId::BCu]);
        assert_eq!(seq, vec![PcbLayerId::FCu, PcbLayerId::BCu]);
        assert_eq!(seq[1], PcbLayerId::BCu);

        let mut seq = seq;
        seq.push(PcbLayerId::FMask);
        assert_eq!(Vec::from(seq).len(), 3);
    }
}
