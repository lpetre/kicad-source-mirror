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

//! Port of `qa/tests/common/test_layer_range.cpp` (Boost test suite `LayerRangeTests`).

use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_core::layer_range::{LayerRange, LayerRangeError};

use PcbLayerId as L;

fn range(start: PcbLayerId, stop: PcbLayerId, layer_count: i32) -> LayerRange {
    LayerRange::new(start, stop, layer_count).expect("copper layers")
}

#[test]
fn forward_iteration_two_layers() {
    let range = range(L::FCu, L::BCu, 2);
    let expected = vec![L::FCu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn forward_iteration_four_layers() {
    let range = range(L::FCu, L::BCu, 4);
    let expected = vec![L::FCu, L::In1Cu, L::In2Cu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn reverse_iteration_four_layers() {
    let range = range(L::BCu, L::FCu, 4);
    let expected = vec![L::BCu, L::In2Cu, L::In1Cu, L::FCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn partial_range_forward() {
    let range = range(L::In1Cu, L::BCu, 6);
    let expected = vec![L::In1Cu, L::In2Cu, L::In3Cu, L::In4Cu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn partial_range_reverse() {
    let range = range(L::In3Cu, L::FCu, 6);
    let expected = vec![L::In3Cu, L::In2Cu, L::In1Cu, L::FCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn from_back_to_inner() {
    // Backdrill-from-bottom walks physically upward from B_Cu through the inner stack.
    // Even though B_Cu's numeric id (2) is less than any inner layer's, the iterator must
    // reverse so that the stack is traversed in physical order.
    let range = range(L::BCu, L::In2Cu, 4);
    let expected = vec![L::BCu, L::In2Cu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn from_back_to_inner_six_layer() {
    let range = range(L::BCu, L::In2Cu, 6);
    let expected = vec![L::BCu, L::In4Cu, L::In3Cu, L::In2Cu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn size_matches_iteration() {
    // size() must match how many layers the iterator yields, even when B_Cu is an endpoint.
    struct Case {
        start: PcbLayerId,
        stop: PcbLayerId,
        layer_count: i32,
        expected: usize,
    }

    let case = |start, stop, layer_count, expected| Case {
        start,
        stop,
        layer_count,
        expected,
    };

    let cases = [
        case(L::FCu, L::BCu, 2, 2),
        case(L::FCu, L::BCu, 4, 4),
        case(L::FCu, L::In1Cu, 4, 2),
        case(L::In1Cu, L::BCu, 6, 5),
        case(L::In3Cu, L::FCu, 6, 4),
        case(L::BCu, L::In2Cu, 4, 2),
        case(L::BCu, L::In2Cu, 6, 4),
    ];

    for c in &cases {
        let range = range(c.start, c.stop, c.layer_count);
        let mut counted = 0usize;

        for _layer in range {
            counted += 1;
        }

        assert_eq!(range.size(), c.expected);
        assert_eq!(counted, c.expected);
    }
}

#[test]
fn invalid_layer_throws_exception() {
    assert_eq!(LayerRange::new(L::FMask, L::BCu, 4), Err(LayerRangeError));
    assert_eq!(LayerRange::new(L::FCu, L::BMask, 4), Err(LayerRangeError));
}

#[test]
fn single_layer_range() {
    let range = range(L::In2Cu, L::In2Cu, 6);
    let expected = vec![L::In2Cu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn zero_layer_count_clamped_to_two() {
    let range = range(L::FCu, L::BCu, 0);
    let expected = vec![L::FCu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn one_layer_count_clamped_to_two() {
    let range = range(L::FCu, L::BCu, 1);
    let expected = vec![L::FCu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn negative_layer_count_clamped_to_two() {
    let range = range(L::FCu, L::BCu, -5);
    let expected = vec![L::FCu, L::BCu];
    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}

#[test]
fn max_layer_count() {
    let range = range(L::FCu, L::BCu, PCB_LAYER_ID_COUNT);
    let mut expected = vec![L::FCu];

    for i in (L::In1Cu.0..2 * PCB_LAYER_ID_COUNT).step_by(2) {
        expected.push(PcbLayerId(i));
    }

    expected.push(L::BCu);

    let mut result = Vec::new();

    for layer in range {
        result.push(layer);
    }

    assert_eq!(result, expected);
}
