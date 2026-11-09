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

//! Port of `qa/tests/common/test_base_set.cpp` (Boost test suite `BaseSetTests`).

use kicad_core::base_set::BaseSet;

#[test]
fn construction_and_size() {
    let mut bs = BaseSet::new(10);
    assert_eq!(bs.size(), 10);
    assert_eq!(bs.count(), 0);

    bs.resize(20);
    assert_eq!(bs.size(), 20);
    assert_eq!(bs.count(), 0);
}

#[test]
fn bit_setting_and_resetting() {
    let mut bs = BaseSet::new(10);
    bs.set(2);
    assert!(bs.test(2));
    assert_eq!(bs.count(), 1);

    bs.reset(2);
    assert!(!bs.test(2));
    assert_eq!(bs.count(), 0);
}

#[test]
fn set_out_of_range() {
    let mut bs = BaseSet::new(10);
    assert_eq!(bs.size(), 10);
    assert_eq!(bs.count(), 0);

    bs.set(10);
    assert_eq!(bs.size(), 11);
    assert_eq!(bs.count(), 1);

    bs.reset(10);
    assert_eq!(bs.size(), 11);
    assert_eq!(bs.count(), 0);

    bs.reset(20);
    assert_eq!(bs.size(), 21);
    assert_eq!(bs.count(), 0);
}

#[test]
fn iterating_set_bits() {
    let mut bs = BaseSet::new(10);
    bs.set(2);
    bs.set(4);

    let mut it = bs.set_bits();
    assert_eq!(it.next(), Some(2));
    assert_eq!(it.next(), Some(4));
    assert!(it.next().is_none());

    // Custom reverse iterator test
    let mut reverse_set_bits: Vec<usize> = Vec::new();

    for rit in bs.set_bits_rev() {
        reverse_set_bits.push(rit);
    }

    assert_eq!(reverse_set_bits.len(), 2);
    assert_eq!(reverse_set_bits[0], 4);
    assert_eq!(reverse_set_bits[1], 2);
}

// Test equality operator
#[test]
fn base_set_equality_operator() {
    let mut set1 = BaseSet::new(10);
    let mut set2 = BaseSet::new(10);
    let mut set3 = BaseSet::new(15);

    set1.set(2);
    set1.set(4);
    set2.set(2);
    set2.set(4);
    set3.set(2);

    assert!(set1 == set2);
    assert!(!(set1 == set3));
    assert!(!(set2 == set3));
}

// Test less-than operator
#[test]
fn base_set_comparison_operator() {
    let mut set1 = BaseSet::new(10);
    let mut set2 = BaseSet::new(10);
    let mut set3 = BaseSet::new(15);

    set1.set(2);
    set1.set(5);
    set2.set(2);
    set3.set(2);

    assert!(set3 < set1); // Although set3 is larger, set1 has a 1 at position 5, so set3 is less
    assert!(set2 < set3); // set2 and set3 both have the same values set but set3 has more positions available
    assert!(!(set1 < set3));
    assert!(!(set1 < set2)); // Although sizes are equal, elements in set2 are subsets of set1
}

// Test compare function
#[test]
fn base_set_compare_function() {
    let mut set1 = BaseSet::new(10);
    let mut set2 = BaseSet::new(10);
    let mut set3 = BaseSet::new(10);
    let mut set4 = BaseSet::new(15);

    set1.set(2);
    set1.set(4);

    set2.set(2);
    set2.set(4);

    set3.set(2);

    set4.set(2);

    assert_eq!(set1.compare(&set2), 0); // set1 and set2 are equal
    assert_eq!(set1.compare(&set3), 1); // set1 is greater than set3
    assert_eq!(set3.compare(&set1), -1); // set3 is less than set1
    assert_eq!(set3.compare(&set4), -1); // set3 is less than set4
    assert_eq!(set4.compare(&set3), 1); // set4 is greater than set3
}

// Test boolean operator&=
#[test]
fn base_set_and_assignment() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs1.set(3);
    bs2.set(2);
    bs2.set(3);

    bs1 &= &bs2;
    assert_eq!(bs1.test(1), false);
    assert_eq!(bs1.test(2), false);
    assert_eq!(bs1.test(3), true);
}

// Test boolean operator|=
#[test]
fn base_set_or_assignment() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs2.set(2);
    bs2.set(3);

    bs1 |= &bs2;
    assert_eq!(bs1.test(1), true);
    assert_eq!(bs1.test(2), true);
    assert_eq!(bs1.test(3), true);
}

// Test boolean operator^=
#[test]
fn base_set_xor_assignment() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs1.set(3);
    bs2.set(2);
    bs2.set(3);

    bs1 ^= &bs2;
    assert_eq!(bs1.test(1), true);
    assert_eq!(bs1.test(2), true);
    assert_eq!(bs1.test(3), false);
}

// Test boolean operator~
#[test]
fn base_set_not_operator() {
    let mut bs1 = BaseSet::new(4);
    bs1.set(1);
    bs1.set(3);
    let bs2 = !&bs1;

    assert_eq!(bs2.test(0), true);
    assert_eq!(bs2.test(1), false);
    assert_eq!(bs2.test(2), true);
    assert_eq!(bs2.test(3), false);
}

// Test non-member operator&
#[test]
fn base_set_and_operator() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs1.set(3);
    bs2.set(2);
    bs2.set(3);

    let result = &bs1 & &bs2;
    assert_eq!(result.test(1), false);
    assert_eq!(result.test(2), false);
    assert_eq!(result.test(3), true);
}

// Test non-member operator|
#[test]
fn base_set_or_operator() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs2.set(2);
    bs2.set(3);

    let result = &bs1 | &bs2;
    assert_eq!(result.test(1), true);
    assert_eq!(result.test(2), true);
    assert_eq!(result.test(3), true);
}

// Test non-member operator^
#[test]
fn base_set_xor_operator() {
    let mut bs1 = BaseSet::new(10);
    let mut bs2 = BaseSet::new(10);
    bs1.set(1);
    bs1.set(3);
    bs2.set(2);
    bs2.set(3);

    let result = &bs1 ^ &bs2;
    assert_eq!(result.test(1), true);
    assert_eq!(result.test(2), true);
    assert_eq!(result.test(3), false);
}

// Test std::hash specialization
#[test]
fn base_set_hash() {
    let mut bs1 = BaseSet::new(10);
    bs1.set(1);
    bs1.set(3);

    let hash = bs1.hash_value();

    let mut bs2 = BaseSet::new(10);
    bs2.set(1);
    bs2.set(3);

    assert_eq!(hash, bs2.hash_value());

    bs2.set(2);
    assert_ne!(hash, bs2.hash_value());
}
