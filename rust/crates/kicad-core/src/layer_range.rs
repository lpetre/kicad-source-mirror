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

//! Port of `include/layer_range.h` (`LAYER_RANGE`).

use std::fmt;

use crate::layer_ids::PcbLayerId;

const F_CU: i32 = PcbLayerId::FCu.0;
const B_CU: i32 = PcbLayerId::BCu.0;
const IN1_CU: i32 = PcbLayerId::In1Cu.0;
const UNDEFINED_LAYER: i32 = PcbLayerId::UndefinedLayer.0;

/// The error raised by [`LayerRange::new`] for a non-copper start or stop layer
/// (`std::invalid_argument( "Only works for copper layers" )` in C++).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerRangeError;

impl fmt::Display for LayerRangeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Only works for copper layers")
    }
}

impl std::error::Error for LayerRangeError {}

/// Port of `LAYER_RANGE`: the copper layers between two copper layers of a board with a given
/// number of copper layers, in physical stack order.
///
/// Iterating the range (with [`LayerRange::iter`] or a `for` loop) walks the stack from the
/// start layer to the stop layer, both included, taking into account that `B_Cu` has a lower
/// numeric id than any inner layer but is physically below them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayerRange {
    start: PcbLayerId,
    stop: PcbLayerId,
    layer_count: i32,
}

impl LayerRange {
    /// Port of `LAYER_RANGE::LAYER_RANGE( PCB_LAYER_ID start, PCB_LAYER_ID stop, int layer_count )`.
    ///
    /// The layer count is clamped to a minimum of 2.  Fails when `start` or `stop` is not a
    /// copper layer (an odd id), where C++ throws `std::invalid_argument`.
    pub fn new(
        start: PcbLayerId,
        stop: PcbLayerId,
        layer_count: i32,
    ) -> Result<LayerRange, LayerRangeError> {
        if (start.0 & 1) != 0 || (stop.0 & 1) != 0 {
            return Err(LayerRangeError);
        }

        Ok(LayerRange {
            start,
            stop,
            layer_count: layer_count.max(2),
        })
    }

    /// The first layer of the range (`m_start`).
    pub fn start(&self) -> PcbLayerId {
        self.start
    }

    /// The last layer of the range (`m_stop`).
    pub fn stop(&self) -> PcbLayerId {
        self.stop
    }

    /// The (clamped) number of copper layers of the stack (`m_layer_count`).
    pub fn layer_count(&self) -> i32 {
        self.layer_count
    }

    /// Port of the `begin()` .. `end()` iteration of `LAYER_RANGE`.
    pub fn iter(&self) -> LayerRangeIter {
        // LAYER_RANGE::end(): an iterator positioned on the stop layer, advanced once.
        let mut end = LayerRangeIterator::new(self.stop.0, self.stop.0, self.layer_count);
        end.advance();

        LayerRangeIter {
            it: LayerRangeIterator::new(self.start.0, self.stop.0, self.layer_count),
            end: end.current,
        }
    }

    /// Port of the static `LAYER_RANGE::Contains( int aStart_layer, int aEnd_layer, int aTest_layer )`.
    pub fn contains_range(start_layer: i32, end_layer: i32, test_layer: i32) -> bool {
        let mut start_layer = start_layer;
        let mut end_layer = end_layer;
        let mut test_layer = test_layer;

        // B_Cu is the lowest copper layer for Z order copper layers
        // F_cu = top, B_Cu = bottom
        // So set the distance from top for B_Cu to INT_MAX
        if test_layer == B_CU {
            test_layer = i32::MAX;
        }

        if start_layer == B_CU {
            start_layer = i32::MAX;
        }

        if end_layer == B_CU {
            end_layer = i32::MAX;
        }

        if start_layer > end_layer {
            std::mem::swap(&mut start_layer, &mut end_layer);
        }

        test_layer >= start_layer && test_layer <= end_layer
    }

    /// Port of the member `LAYER_RANGE::Contains( int aTest_layer )`.
    pub fn contains(&self, test_layer: impl Into<i32>) -> bool {
        Self::contains_range(self.start.0, self.stop.0, test_layer.into())
    }

    /// Port of `LAYER_RANGE::size`.
    pub fn size(&self) -> usize {
        // Map a copper layer to its physical position in the stack so that size() matches the
        // iterator's traversal instead of the enum's numeric ordering (F_Cu=0, B_Cu=2, In1_Cu=4,
        // In2_Cu=6, ...).  F_Cu sits at position 0, each In<N>_Cu at position N, and B_Cu at the
        // bottom of whatever stackup the caller specified.
        let ordinal = |layer: PcbLayerId| -> i64 {
            if layer == PcbLayerId::FCu {
                return 0;
            }

            if layer == PcbLayerId::BCu {
                return i64::from(self.layer_count) - 1;
            }

            i64::from(layer.0 / 2 - 1)
        };

        let start = ordinal(self.start);
        let stop = ordinal(self.stop);

        ((start - stop).abs() + 1) as usize
    }
}

impl IntoIterator for LayerRange {
    type Item = PcbLayerId;
    type IntoIter = LayerRangeIter;

    fn into_iter(self) -> LayerRangeIter {
        self.iter()
    }
}

impl IntoIterator for &LayerRange {
    type Item = PcbLayerId;
    type IntoIter = LayerRangeIter;

    fn into_iter(self) -> LayerRangeIter {
        self.iter()
    }
}

/// Port of `LAYER_RANGE::LAYER_RANGE_ITERATOR`.
#[derive(Clone, Copy, Debug)]
struct LayerRangeIterator {
    current: i32,
    stop: i32,
    layer_count: i32,
    reverse: bool,
}

impl LayerRangeIterator {
    /// Port of the direction choice made by the `LAYER_RANGE_ITERATOR` constructor.
    fn direction(current: i32, stop: i32) -> bool {
        // B_Cu has a lower numeric id than any inner layer, but is physically below them.
        // When iterating from B_Cu toward an inner layer we must walk the stack in reverse
        // so the B_Cu special-case in next_layer() produces the correct physical order.
        if current == B_CU && stop != B_CU {
            true
        } else {
            !(stop == B_CU || stop >= current)
        }
    }

    /// Port of `LAYER_RANGE_ITERATOR::LAYER_RANGE_ITERATOR`.
    fn new(start: i32, stop: i32, layer_count: i32) -> Self {
        LayerRangeIterator {
            current: start,
            stop,
            layer_count,
            reverse: Self::direction(start, stop),
        }
    }

    /// The id of the innermost copper layer next to `B_Cu` for the layer count:
    /// `static_cast<int>( F_Cu ) + 2 * ( m_layer_count - 2 ) + 2`.
    fn last_inner(&self) -> i32 {
        F_CU.wrapping_add(2i32.wrapping_mul(self.layer_count - 2))
            .wrapping_add(2)
    }

    /// Port of `LAYER_RANGE_ITERATOR::next_layer`.
    ///
    /// Returns `None` in the situations where the C++ iterator can never reach its stop layer
    /// any more and would iterate forever (the stop layer is not part of the stack described
    /// by the layer count); see `PORT_STATUS.md`.
    fn next_layer(&self, layer: i32) -> Option<i32> {
        let next = if self.reverse {
            if layer == B_CU && self.layer_count == 2 {
                F_CU
            } else if layer == B_CU {
                self.last_inner()
            } else if layer == self.stop || layer == UNDEFINED_LAYER {
                UNDEFINED_LAYER
            } else if layer == IN1_CU {
                F_CU
            } else {
                if layer <= F_CU {
                    // Walked past the top of the stack without meeting the stop layer.
                    return None;
                }

                layer - 2
            }
        } else if layer == F_CU && self.layer_count == 2 {
            B_CU
        } else if layer == self.stop || layer == UNDEFINED_LAYER {
            UNDEFINED_LAYER
        } else if layer == B_CU {
            // Reached the bottom of the stack without meeting the stop layer.
            return None;
        } else if layer == self.last_inner() {
            B_CU
        } else if layer == F_CU {
            IN1_CU
        } else {
            if layer > self.stop && layer > self.last_inner() {
                // Walked past the bottom of the stack without meeting the stop layer.
                return None;
            }

            layer + 2
        };

        Some(next)
    }

    /// Port of `LAYER_RANGE_ITERATOR::operator++`.  Returns false when the iteration had to be
    /// cut short (see [`Self::next_layer`]).
    fn advance(&mut self) -> bool {
        match self.next_layer(self.current) {
            Some(next) => {
                self.current = next;
                true
            }
            None => false,
        }
    }
}

/// Iterator over the layers of a [`LayerRange`].
#[derive(Clone, Copy, Debug)]
pub struct LayerRangeIter {
    it: LayerRangeIterator,
    /// The layer value of the `end()` iterator; iteration stops when it is reached.
    end: i32,
}

impl Iterator for LayerRangeIter {
    type Item = PcbLayerId;

    fn next(&mut self) -> Option<PcbLayerId> {
        if self.it.current == self.end {
            return None;
        }

        let layer = PcbLayerId(self.it.current);

        if !self.it.advance() {
            self.it.current = self.end;
        }

        Some(layer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layers(start: PcbLayerId, stop: PcbLayerId, count: i32) -> Vec<PcbLayerId> {
        LayerRange::new(start, stop, count)
            .unwrap()
            .iter()
            .collect()
    }

    #[test]
    fn two_layer_quirks() {
        // The end iterator of a range stopping on F_Cu of a 2 layer board is B_Cu, so a range
        // starting on B_Cu is empty in C++.
        assert_eq!(layers(PcbLayerId::BCu, PcbLayerId::FCu, 2), vec![]);
        assert_eq!(
            layers(PcbLayerId::FCu, PcbLayerId::FCu, 2),
            vec![PcbLayerId::FCu]
        );
        assert_eq!(
            layers(PcbLayerId::BCu, PcbLayerId::BCu, 2),
            vec![PcbLayerId::BCu]
        );
    }

    #[test]
    fn unreachable_stop_terminates() {
        // In C++ these loop forever; the port stops at the end of the stack.
        assert_eq!(
            layers(PcbLayerId::FCu, PcbLayerId::In5Cu, 4),
            vec![
                PcbLayerId::FCu,
                PcbLayerId::In1Cu,
                PcbLayerId::In2Cu,
                PcbLayerId::BCu
            ]
        );
        assert_eq!(
            layers(PcbLayerId::BCu, PcbLayerId::In5Cu, 4),
            vec![
                PcbLayerId::BCu,
                PcbLayerId::In2Cu,
                PcbLayerId::In1Cu,
                PcbLayerId::FCu
            ]
        );
        assert_eq!(
            layers(PcbLayerId::In5Cu, PcbLayerId::BCu, 4),
            vec![PcbLayerId::In5Cu]
        );
        assert_eq!(
            layers(PcbLayerId::In3Cu, PcbLayerId::In4Cu, 4),
            vec![PcbLayerId::In3Cu, PcbLayerId::In4Cu]
        );
    }

    #[test]
    fn contains() {
        assert!(LayerRange::contains_range(F_CU, B_CU, IN1_CU));
        assert!(LayerRange::contains_range(B_CU, F_CU, B_CU));
        assert!(!LayerRange::contains_range(F_CU, IN1_CU, B_CU));
        assert!(LayerRange::contains_range(IN1_CU, B_CU, 60));
        assert!(!LayerRange::contains_range(8, B_CU, IN1_CU));

        let range = LayerRange::new(PcbLayerId::In2Cu, PcbLayerId::In4Cu, 8).unwrap();
        assert!(range.contains(PcbLayerId::In3Cu));
        assert!(!range.contains(PcbLayerId::BCu));
        assert_eq!(range.start(), PcbLayerId::In2Cu);
        assert_eq!(range.stop(), PcbLayerId::In4Cu);
        assert_eq!(range.layer_count(), 8);
    }

    #[test]
    fn error_message() {
        let err = LayerRange::new(PcbLayerId::FMask, PcbLayerId::BCu, 4).unwrap_err();
        assert_eq!(err.to_string(), "Only works for copper layers");
    }
}
