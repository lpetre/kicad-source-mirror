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

//! Port of `qa/tests/common/test_lset.cpp` (Boost test suite `LSETTests`).

use kicad_core::base_set::BaseSet;
use kicad_core::layer_ids::{PCB_LAYER_ID_COUNT, PcbLayerId};
use kicad_core::lseq::Lseq;
use kicad_core::lset::Lset;

use PcbLayerId as L;

// Initialize an empty LSET
#[test]
fn lset_constructor_empty() {
    let set = Lset::new();
    assert_eq!(set.count(), 0);
}

// Initialize LSET from another BASE_SET
#[test]
fn lset_constructor_from_base_set() {
    let mut base = BaseSet::new(PCB_LAYER_ID_COUNT as usize);
    base.set(L::FCu);
    base.set(L::In1Cu);

    let set = Lset::from_base_set(base);
    assert_eq!(set.count(), 2);
    assert!(set.test(L::FCu));
    assert!(set.test(L::In1Cu));
}

// Initialize LSET from a specific PCB_LAYER_ID
#[test]
fn lset_constructor_from_layer() {
    let set = Lset::from([L::FCu]);
    assert_eq!(set.count(), 1);
    assert!(set.test(L::FCu));
}

// Initialize LSET from an initializer list
#[test]
fn lset_constructor_from_list() {
    let set = Lset::from([L::FCu, L::In1Cu, L::In2Cu]);
    assert_eq!(set.count(), 3);
    assert!(set.test(L::FCu));
    assert!(set.test(L::In1Cu));
    assert!(set.test(L::In2Cu));
}

// Initialize LSET from LSEQ
#[test]
fn lset_constructor_from_sequence() {
    let seq = Lseq::from([L::FCu, L::In1Cu, L::In2Cu]);
    let set = Lset::from_lseq(&seq);
    assert_eq!(set.count(), 3);
    assert!(set.test(L::FCu));
    assert!(set.test(L::In1Cu));
    assert!(set.test(L::In2Cu));
}

// Test Containment Check
#[test]
fn lset_contains() {
    let set = Lset::from([L::FCu, L::In1Cu, L::In2Cu]);
    assert!(set.contains(L::FCu));
    assert!(set.contains(L::In1Cu));
    assert!(!set.contains(L::In30Cu));
}

#[test]
fn lset_contains_all_with_different_sizes() {
    let mut base = BaseSet::new(64);
    base.set(L::FCu);

    let short_set = Lset::from_base_set(base);
    let mut full_set = Lset::from([L::FCu]);

    assert!(short_set.contains_all(&full_set));
    assert!(full_set.contains_all(&short_set));

    full_set.set(L::User14);

    assert!(!short_set.contains_all(&full_set));
    assert!(full_set.contains_all(&short_set));
}

// Test Sequence Generation
#[test]
fn lset_sequence_generation() {
    let set = Lset::from([L::FCu, L::In1Cu, L::In2Cu]);

    let sequence = set.seq();
    assert_eq!(sequence.len(), 3);
    assert_eq!(sequence[0], L::FCu);
    assert_eq!(sequence[1], L::In1Cu);
    assert_eq!(sequence[2], L::In2Cu);
}

// Test Hex and Binary Formatting
#[test]
fn lset_formatting() {
    let set = Lset::from([L::FCu, L::In1Cu, L::In2Cu]);

    let hex_string = set.fmt_hex();
    let expected_hex_string = "00000000_00000000_00000000_00000051"; // depends on bit ordering

    assert_eq!(hex_string, expected_hex_string);

    let bin_string = set.fmt_bin();
    let expected_bin_string = concat!(
        "0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|",
        "0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0000_0000|0101_0001"
    ); // depends on bit ordering

    assert_eq!(bin_string, expected_bin_string);
}

// Test ExtractLayer and Flip
#[test]
fn lset_manipulations() {
    let mut set = Lset::from([L::FCu, L::In1Cu, L::In2Cu]);

    // Test ExtractLayer: should extract the layer set or undefined if more than one
    let mut extracted_layer = set.extract_layer();
    assert_eq!(extracted_layer, L::UndefinedLayer);

    // Test Flip: should swap front and back layers
    set.flip_standard_layers(4);
    assert!(set.contains(L::BCu));
    assert!(set.contains(L::In1Cu)); // Internal layers remain unchanged

    // Test setting a single layer
    set = Lset::from([L::FCu]);
    extracted_layer = set.extract_layer();
    assert_eq!(extracted_layer, L::FCu);
    set.flip_standard_layers(0);
    extracted_layer = set.extract_layer();
    assert_eq!(extracted_layer, L::BCu);
}

// Test Static Mask Methods
#[test]
fn lset_static_masks() {
    let internal_cu_mask = Lset::internal_cu_mask().clone();
    assert!(internal_cu_mask.contains(L::In1Cu));
    assert!(internal_cu_mask.contains(L::In30Cu));
    assert!(!internal_cu_mask.contains(L::FCu));
    assert!(!internal_cu_mask.contains(L::BCu));
}

// Auxiliary function to compare LSEQ objects
fn compare_lseq(seq1: &Lseq, seq2: &Lseq) -> bool {
    if seq1.len() != seq2.len() {
        return false;
    }

    for i in 0..seq1.len() {
        if seq1[i] != seq2[i] {
            return false;
        }
    }

    true
}

// Test LSET::Seq base case
#[test]
fn lset_seq_base_case() {
    let lset = Lset::from([L::FCu, L::BCu, L::In1Cu, L::In2Cu]);
    let expected = Lseq::from([L::FCu, L::BCu, L::In1Cu, L::In2Cu]);

    let result = lset.seq();
    assert!(compare_lseq(&result, &expected));
}

// Test LSET::Seq empty case
#[test]
fn lset_seq_empty_case() {
    let lset = Lset::new();
    let expected = Lseq::new();

    let result = lset.seq();
    assert!(compare_lseq(&result, &expected));
}

// Test LSET::SeqStackupTop2Bottom base case
#[test]
fn lset_seq_stackup_top2bottom_base_case() {
    let lset = Lset::from([
        L::FCu,
        L::BCu,
        L::In1Cu,
        L::In2Cu,
        L::FSilkS,
        L::BSilkS,
        L::EdgeCuts,
        L::Margin,
        L::DwgsUser,
    ]);
    let expected = Lseq::from([
        L::EdgeCuts,
        L::Margin,
        L::DwgsUser,
        L::FSilkS,
        L::FCu,
        L::In1Cu,
        L::In2Cu,
        L::BCu,
        L::BSilkS,
    ]);

    let result = lset.seq_stackup_top2bottom(L::UndefinedLayer);
    assert!(compare_lseq(&result, &expected), "{result:?}");
}

// Test LSET::SeqStackupTop2Bottom prioritizing selected layer
#[test]
fn lset_seq_stackup_top2bottom_with_selection() {
    let lset = Lset::from([
        L::FCu,
        L::BCu,
        L::In1Cu,
        L::In2Cu,
        L::FSilkS,
        L::BSilkS,
        L::EdgeCuts,
        L::Margin,
        L::DwgsUser,
    ]);
    let expected = Lseq::from([
        L::FSilkS,
        L::EdgeCuts,
        L::Margin,
        L::DwgsUser,
        L::FCu,
        L::In1Cu,
        L::In2Cu,
        L::BCu,
        L::BSilkS,
    ]);

    let result = lset.seq_stackup_top2bottom(L::FSilkS);
    assert!(compare_lseq(&result, &expected), "{result:?}");
}

// Test LSET::SeqStackupForPlotting base case
#[test]
fn lset_seq_stackup_for_plotting_base_case() {
    let lset = Lset::from([
        L::FCu,
        L::BCu,
        L::In1Cu,
        L::In2Cu,
        L::FSilkS,
        L::BSilkS,
        L::EdgeCuts,
        L::Margin,
    ]);
    let expected = Lseq::from([
        L::BCu,
        L::BSilkS,
        L::In2Cu,
        L::In1Cu,
        L::FCu,
        L::FSilkS,
        L::Margin,
        L::EdgeCuts,
    ]);

    let result = lset.seq_stackup_for_plotting();
    assert!(compare_lseq(&result, &expected), "{result:?}");
}
