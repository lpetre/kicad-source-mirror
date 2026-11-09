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
//
// The bit container semantics follow thirdparty/dynamic_bitset (sul::dynamic_bitset),
// Copyright (c) 2019 Maxime Pinard, distributed under the MIT license.

//! Port of `include/base_set.h` (`BASE_SET`) together with the parts of
//! `thirdparty/dynamic_bitset/dynamic_bitset.h` (`sul::dynamic_bitset<uint64_t>`) that
//! `BASE_SET` inherits and KiCad uses.
//!
//! A [`BaseSet`] is a dynamically sized bit set.  Bit 0 is the least significant bit of the
//! first 64 bit block, exactly as in `sul::dynamic_bitset<uint64_t>`; unused bits of the last
//! block are always kept at zero ("sanitized"), which the comparison and hashing code relies on.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Index, Not};

/// Number of bits of one storage block (`sul::dynamic_bitset<uint64_t>::bits_per_block`).
pub const BITS_PER_BLOCK: usize = 64;

/// Number of blocks stored inline (without heap allocation).  Two blocks hold the 128 bits of
/// an `LSET`, by far the most common size.  This is purely a storage optimisation: the C++
/// class always uses a `std::vector<uint64_t>`.
const INLINE_BLOCKS: usize = 2;

/// Port of `dynamic_bitset::blocks_required`.
#[inline]
const fn blocks_required(nbits: usize) -> usize {
    nbits / BITS_PER_BLOCK + if nbits % BITS_PER_BLOCK > 0 { 1 } else { 0 }
}

/// Port of `dynamic_bitset::block_index`.
#[inline]
const fn block_index(pos: usize) -> usize {
    pos / BITS_PER_BLOCK
}

/// Port of `dynamic_bitset::bit_mask`.
#[inline]
const fn bit_mask(pos: usize) -> u64 {
    1u64 << (pos % BITS_PER_BLOCK)
}

/// A position inside a [`BaseSet`].
///
/// C++ converts `PCB_LAYER_ID`, `int` and `size_t` arguments implicitly to the `size_t`
/// position taken by `BASE_SET::set()`, `test()`, ...  This trait plays the role of that
/// implicit conversion.  A position that has no valid index (a negative layer id such as
/// `UNDEFINED_LAYER`) converts to `None`; C++ would turn it into a huge `size_t` and run into
/// undefined behaviour / an allocation failure, the port ignores such positions instead (see
/// `PORT_STATUS.md`).
pub trait BitPos: Copy {
    /// The bit index, or `None` when the value cannot be an index.
    fn bit_pos(self) -> Option<usize>;
}

impl BitPos for usize {
    #[inline]
    fn bit_pos(self) -> Option<usize> {
        Some(self)
    }
}

impl BitPos for i32 {
    #[inline]
    fn bit_pos(self) -> Option<usize> {
        usize::try_from(self).ok()
    }
}

impl BitPos for u32 {
    #[inline]
    fn bit_pos(self) -> Option<usize> {
        Some(self as usize)
    }
}

/// Block storage: up to [`INLINE_BLOCKS`] blocks inline, otherwise on the heap.
#[derive(Clone)]
enum Storage {
    /// Unused inline blocks are always zero.
    Inline([u64; INLINE_BLOCKS]),
    Heap(Vec<u64>),
}

/// Port of `BASE_SET`, a `sul::dynamic_bitset<uint64_t>` whose single-bit mutators grow the
/// set on demand.
///
/// Naming of the overloaded C++ members:
///
/// | C++ | Rust |
/// |---|---|
/// | `set( pos )` / `set( pos, value )` / `set()` | [`set`](Self::set) / [`set_value`](Self::set_value) / [`set_all`](Self::set_all) |
/// | `reset( pos )` / `reset()` | [`reset`](Self::reset) / [`reset_all`](Self::reset_all) |
/// | `flip( pos )` / `flip()` | [`flip`](Self::flip) / [`flip_all`](Self::flip_all) |
/// | `begin()` .. `end()` (all bits as `bool`) | [`iter`](Self::iter) |
/// | `set_bits_begin()` .. `set_bits_end()` | [`set_bits`](Self::set_bits) |
/// | `set_bits_rbegin()` .. `set_bits_rend()` | [`set_bits_rev`](Self::set_bits_rev) |
/// | `std::hash<BASE_SET>` | [`hash_value`](Self::hash_value) |
///
/// Equality is that of `sul::dynamic_bitset::operator==`: two sets are equal when they have
/// the same size *and* the same bits.  The ordering ([`Ord`]) is that of `BASE_SET::compare`
/// and `BASE_SET::operator<`: a lexicographic comparison of the bits starting at bit 0.
#[derive(Clone)]
pub struct BaseSet {
    storage: Storage,
    bits_number: usize,
}

impl BaseSet {
    /// Default size of a `BASE_SET` (`BASE_SET( size_t size = 64 )`).
    pub const DEFAULT_SIZE: usize = 64;

    /// Port of `BASE_SET::BASE_SET( size_t size )`: a set of `size` cleared bits.
    pub fn new(size: usize) -> Self {
        let nblocks = blocks_required(size);
        let storage = if nblocks <= INLINE_BLOCKS {
            Storage::Inline([0; INLINE_BLOCKS])
        } else {
            Storage::Heap(vec![0; nblocks])
        };

        BaseSet {
            storage,
            bits_number: size,
        }
    }

    /// The blocks in use (`m_blocks`).
    #[inline]
    fn blocks(&self) -> &[u64] {
        match &self.storage {
            Storage::Inline(a) => &a[..blocks_required(self.bits_number)],
            Storage::Heap(v) => v.as_slice(),
        }
    }

    /// The blocks in use, mutable.
    #[inline]
    fn blocks_mut(&mut self) -> &mut [u64] {
        match &mut self.storage {
            Storage::Inline(a) => &mut a[..blocks_required(self.bits_number)],
            Storage::Heap(v) => v.as_mut_slice(),
        }
    }

    /// Port of `dynamic_bitset::size`: the number of bits.
    #[inline]
    pub fn size(&self) -> usize {
        self.bits_number
    }

    /// Port of `dynamic_bitset::empty`: true when the set has no bits at all (size 0).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.bits_number == 0
    }

    /// Port of `dynamic_bitset::num_blocks`.
    #[inline]
    pub fn num_blocks(&self) -> usize {
        blocks_required(self.bits_number)
    }

    /// Port of `dynamic_bitset::sanitize`: clear the unused bits of the last block.
    fn sanitize(&mut self) {
        let shift = self.bits_number % BITS_PER_BLOCK;

        if shift > 0 {
            if let Some(last) = self.blocks_mut().last_mut() {
                *last &= !(u64::MAX << shift);
            }
        }
    }

    /// Port of `dynamic_bitset::resize( nbits )` (new bits are cleared).
    pub fn resize(&mut self, nbits: usize) {
        self.resize_with_value(nbits, false);
    }

    /// Port of `dynamic_bitset::resize( nbits, value )`.
    pub fn resize_with_value(&mut self, nbits: usize, value: bool) {
        if nbits == self.bits_number {
            return;
        }

        let old_num_blocks = self.num_blocks();
        let new_num_blocks = blocks_required(nbits);
        let init_value: u64 = if value { u64::MAX } else { 0 };

        if new_num_blocks != old_num_blocks {
            // m_blocks.resize( new_num_blocks, init_value )
            let keep = old_num_blocks.min(new_num_blocks);

            if new_num_blocks <= INLINE_BLOCKS {
                let mut inline = [0u64; INLINE_BLOCKS];
                inline[..keep].copy_from_slice(&self.blocks()[..keep]);

                for block in inline.iter_mut().take(new_num_blocks).skip(keep) {
                    *block = init_value;
                }

                self.storage = Storage::Inline(inline);
            } else {
                match &mut self.storage {
                    Storage::Heap(v) => v.resize(new_num_blocks, init_value),
                    Storage::Inline(a) => {
                        let mut v = Vec::with_capacity(new_num_blocks);
                        v.extend_from_slice(&a[..keep]);
                        v.resize(new_num_blocks, init_value);
                        self.storage = Storage::Heap(v);
                    }
                }
            }
        }

        if value && nbits > self.bits_number && old_num_blocks > 0 {
            // set value of the new bits in the old last block
            let extra_bits = self.bits_number % BITS_PER_BLOCK;

            if extra_bits > 0 {
                let block = match &mut self.storage {
                    Storage::Inline(a) => &mut a[old_num_blocks - 1],
                    Storage::Heap(v) => &mut v[old_num_blocks - 1],
                };

                *block |= init_value << extra_bits;
            }
        }

        self.bits_number = nbits;
        self.sanitize();
    }

    /// Port of `dynamic_bitset::clear`: remove all bits (size becomes 0).
    pub fn clear(&mut self) {
        self.storage = Storage::Inline([0; INLINE_BLOCKS]);
        self.bits_number = 0;
    }

    /// Port of `BASE_SET::set( size_t pos )`: set a bit, growing the set to `pos + 1` bits
    /// when `pos` is beyond the current size.
    pub fn set<P: BitPos>(&mut self, pos: P) -> &mut Self {
        self.set_value(pos, true)
    }

    /// Port of `BASE_SET::set( size_t pos, bool value )`: set a bit to a given value, growing
    /// the set to `pos + 1` bits when `pos` is beyond the current size.
    pub fn set_value<P: BitPos>(&mut self, pos: P, value: bool) -> &mut Self {
        let Some(pos) = pos.bit_pos() else {
            return self;
        };

        if pos >= self.size() {
            self.resize(pos + 1);
        }

        let block = &mut self.blocks_mut()[block_index(pos)];

        if value {
            *block |= bit_mask(pos);
        } else {
            *block &= !bit_mask(pos);
        }

        self
    }

    /// Port of `BASE_SET::set()`: set all bits to 1.
    pub fn set_all(&mut self) -> &mut Self {
        for block in self.blocks_mut() {
            *block = u64::MAX;
        }

        self.sanitize();
        self
    }

    /// Port of `BASE_SET::reset( size_t pos )`: clear a bit.  Like the C++ original this
    /// grows the set to `pos + 1` bits when `pos` is beyond the current size.
    pub fn reset<P: BitPos>(&mut self, pos: P) -> &mut Self {
        self.set_value(pos, false)
    }

    /// Port of `BASE_SET::reset()`: clear all bits.
    pub fn reset_all(&mut self) -> &mut Self {
        for block in self.blocks_mut() {
            *block = 0;
        }

        self
    }

    /// Port of `BASE_SET::flip( size_t pos )`: toggle a bit, growing the set to `pos + 1` bits
    /// when `pos` is beyond the current size.
    pub fn flip<P: BitPos>(&mut self, pos: P) -> &mut Self {
        let Some(pos) = pos.bit_pos() else {
            return self;
        };

        if pos >= self.size() {
            self.resize(pos + 1);
        }

        self.blocks_mut()[block_index(pos)] ^= bit_mask(pos);
        self
    }

    /// Port of `BASE_SET::flip()`: toggle all bits.
    pub fn flip_all(&mut self) -> &mut Self {
        for block in self.blocks_mut() {
            *block = !*block;
        }

        self.sanitize();
        self
    }

    /// Port of `dynamic_bitset::test( pos )`.
    ///
    /// The C++ function asserts `pos < size()` (undefined behaviour in release builds); the
    /// port returns `false` for positions outside of the set.
    #[inline]
    pub fn test<P: BitPos>(&self, pos: P) -> bool {
        match pos.bit_pos() {
            Some(pos) if pos < self.bits_number => {
                (self.blocks()[block_index(pos)] & bit_mask(pos)) != 0
            }
            _ => false,
        }
    }

    /// Port of `dynamic_bitset::test_set( pos, value )`: return the bit and set it to `value`.
    pub fn test_set<P: BitPos>(&mut self, pos: P, value: bool) -> bool {
        let result = self.test(pos);

        if result != value {
            self.set_value(pos, value);
        }

        result
    }

    /// Port of `dynamic_bitset::all`: true when every bit is set (also true for size 0).
    pub fn all(&self) -> bool {
        if self.is_empty() {
            return true;
        }

        let blocks = self.blocks();
        let extra_bits = self.bits_number % BITS_PER_BLOCK;

        if extra_bits == 0 {
            blocks.iter().all(|&b| b == u64::MAX)
        } else {
            let (last, full) = blocks.split_last().expect("non-empty set has blocks");

            full.iter().all(|&b| b == u64::MAX)
                && *last == (u64::MAX >> (BITS_PER_BLOCK - extra_bits))
        }
    }

    /// Port of `dynamic_bitset::any`: true when at least one bit is set.
    pub fn any(&self) -> bool {
        self.blocks().iter().any(|&b| b != 0)
    }

    /// Port of `dynamic_bitset::none`: true when no bit is set.
    pub fn none(&self) -> bool {
        !self.any()
    }

    /// Port of `dynamic_bitset::count`: the number of set bits.
    pub fn count(&self) -> usize {
        self.blocks().iter().map(|b| b.count_ones() as usize).sum()
    }

    /// Port of `dynamic_bitset::is_subset_of`.
    ///
    /// The C++ function asserts that both sets have the same size; the port treats the bits
    /// missing from the shorter set as cleared.
    pub fn is_subset_of(&self, bitset: &BaseSet) -> bool {
        let lhs = self.blocks();
        let rhs = bitset.blocks();

        lhs.iter()
            .enumerate()
            .all(|(i, &block)| (block & !rhs.get(i).copied().unwrap_or(0)) == 0)
    }

    /// Port of `dynamic_bitset::is_proper_subset_of`.
    ///
    /// The C++ function asserts that both sets have the same size; the port treats the bits
    /// missing from the shorter set as cleared.
    pub fn is_proper_subset_of(&self, bitset: &BaseSet) -> bool {
        let lhs = self.blocks();
        let rhs = bitset.blocks();
        let mut is_proper = false;

        for i in 0..lhs.len().max(rhs.len()) {
            let self_block = lhs.get(i).copied().unwrap_or(0);
            let other_block = rhs.get(i).copied().unwrap_or(0);

            if (self_block & !other_block) != 0 {
                return false;
            }

            if (!self_block & other_block) != 0 {
                is_proper = true;
            }
        }

        is_proper
    }

    /// Port of `dynamic_bitset::intersects`: true when the two sets share a set bit.
    pub fn intersects(&self, bitset: &BaseSet) -> bool {
        self.blocks()
            .iter()
            .zip(bitset.blocks())
            .any(|(&a, &b)| (a & b) != 0)
    }

    /// Port of `dynamic_bitset::find_first`: the position of the first set bit
    /// (`npos` becomes `None`).
    pub fn find_first(&self) -> Option<usize> {
        self.blocks()
            .iter()
            .enumerate()
            .find(|&(_, &block)| block != 0)
            .map(|(i, &block)| i * BITS_PER_BLOCK + block.trailing_zeros() as usize)
    }

    /// Port of `dynamic_bitset::find_next`: the position of the first set bit after `prev`
    /// (`npos` becomes `None`).
    pub fn find_next(&self, prev: usize) -> Option<usize> {
        if self.is_empty() || prev >= self.size() - 1 {
            return None;
        }

        let blocks = self.blocks();
        let first_bit = prev + 1;
        let first_block = block_index(first_bit);
        let first_bit_index = first_bit % BITS_PER_BLOCK;
        let first_block_shifted = blocks[first_block] >> first_bit_index;

        if first_block_shifted != 0 {
            return Some(first_bit + first_block_shifted.trailing_zeros() as usize);
        }

        blocks
            .iter()
            .enumerate()
            .skip(first_block + 1)
            .find(|&(_, &block)| block != 0)
            .map(|(i, &block)| i * BITS_PER_BLOCK + block.trailing_zeros() as usize)
    }

    /// Port of `dynamic_bitset::to_string` with the default `'0'` / `'1'` characters: one
    /// character per bit, most significant (highest position) first.
    pub fn to_bit_string(&self) -> String {
        (0..self.size())
            .rev()
            .map(|i| if self.test(i) { '1' } else { '0' })
            .collect()
    }

    /// Port of `BASE_SET::compare`.
    ///
    /// The bits are compared lexicographically starting at bit 0 (`false < true`); when one
    /// set is a prefix of the other the shorter one is the smaller
    /// (`alg::lexicographical_compare_three_way` over `begin()`..`end()`).
    /// Returns -1, 0 or 1.
    pub fn compare(&self, other: &BaseSet) -> i32 {
        // The C++ code walks the sets bit by bit.  Comparing whole blocks is equivalent: the
        // lowest differing bit decides when it lies inside both sets.  A differing bit beyond
        // the size of the shorter set means the common prefix is equal (unused bits are always
        // zero), in which case the sizes decide.
        let min_size = self.size().min(other.size());
        let lhs = self.blocks();
        let rhs = other.blocks();

        for (i, (&a, &b)) in lhs.iter().zip(rhs).enumerate() {
            let diff = a ^ b;

            if diff != 0 {
                let pos = i * BITS_PER_BLOCK + diff.trailing_zeros() as usize;

                if pos < min_size {
                    return if (a & (1u64 << diff.trailing_zeros())) != 0 {
                        1
                    } else {
                        -1
                    };
                }

                break;
            }
        }

        match self.size().cmp(&other.size()) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    /// Port of `BASE_SET::FmtBin`: a binary string showing the contents of this set, highest
    /// bit first, with `_` between nibbles and `|` between bytes.
    pub fn fmt_bin(&self) -> String {
        let mut ret = String::new();
        let bit_count = self.size();

        for bit in 0..bit_count {
            if bit != 0 {
                if bit % 8 == 0 {
                    ret.push('|');
                } else if bit % 4 == 0 {
                    ret.push('_');
                }
            }

            ret.push(if self.test(bit) { '1' } else { '0' });
        }

        // reverse of string
        ret.chars().rev().collect()
    }

    /// Port of `BASE_SET::FmtHex`: a hex string showing the contents of this set, highest
    /// nibble first, with `_` between groups of 8 nibbles.
    pub fn fmt_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";

        let mut ret = String::new();
        let nibble_count = self.size().div_ceil(4);

        for nibble in 0..nibble_count {
            let mut ndx = 0usize;

            // test 4 consecutive bits and set ndx to 0-15
            for nibble_bit in 0..4 {
                let nibble_pos = nibble_bit + nibble * 4;

                // make sure it's not extra bits that don't exist in the bitset but need to in
                // the hex format
                if nibble_pos >= self.size() {
                    break;
                }

                if self.test(nibble_pos) {
                    ndx |= 1 << nibble_bit;
                }
            }

            if nibble != 0 && nibble % 8 == 0 {
                ret.push('_');
            }

            ret.push(HEX[ndx] as char);
        }

        // reverse of string
        ret.chars().rev().collect()
    }

    /// Port of `BASE_SET::ParseHex( const std::string& )`: convert the output of
    /// [`fmt_hex`](Self::fmt_hex) and replace this set's values with those given in the input
    /// string.
    ///
    /// Parsing runs from the end of the string towards its start and stops at the first non
    /// hex ASCII byte, except that the `_` markers output by `fmt_hex` are not terminators.
    /// The size of the set is not changed.
    ///
    /// Returns the number of bytes consumed.  As in C++ a terminating non-hex byte counts as
    /// consumed, and the set is replaced whenever at least one byte was consumed.
    pub fn parse_hex(&mut self, s: &str) -> i32 {
        self.parse_hex_bytes(s.as_bytes())
    }

    /// Port of `BASE_SET::ParseHex( const char* aStart, int aCount )`, see
    /// [`parse_hex`](Self::parse_hex).
    pub fn parse_hex_bytes(&mut self, bytes: &[u8]) -> i32 {
        let mut tmp = BaseSet::new(self.size());

        // rstart walks from the last byte down to (and past) the first one.
        let count = bytes.len() as isize;
        let mut rstart: isize = count - 1;
        let bitcount = self.size();
        let mut nibble_ndx = 0usize;

        while rstart > -1 {
            let cc = bytes[rstart as usize];
            rstart -= 1;

            if cc == b'_' {
                continue;
            }

            let nibble: u32 = match cc {
                b'0'..=b'9' => u32::from(cc - b'0'),
                b'a'..=b'f' => u32::from(cc - b'a') + 10,
                b'A'..=b'F' => u32::from(cc - b'A') + 10,
                _ => break,
            };

            let mut bit = nibble_ndx * 4;
            let mut ndx = 0;

            while bit < bitcount && ndx < 4 {
                if (nibble & (1 << ndx)) != 0 {
                    tmp.set(bit);
                }

                bit += 1;
                ndx += 1;
            }

            if bit >= bitcount {
                break;
            }

            nibble_ndx += 1;
        }

        let byte_count = count - 1 - rstart;

        if byte_count > 0 {
            *self = tmp;
        }

        byte_count as i32
    }

    /// Port of the `BASE_SET::begin()` .. `end()` iteration: every bit of the set as a `bool`,
    /// starting at bit 0.
    pub fn iter(&self) -> BitsIter<'_> {
        BitsIter { set: self, pos: 0 }
    }

    /// Port of the `set_bits_begin()` .. `set_bits_end()` iteration
    /// (`BASE_SET::set_bits_iterator`): the positions of the set bits in ascending order.
    pub fn set_bits(&self) -> SetBitsIter<'_> {
        SetBitsIter {
            set: self,
            next: self.find_first(),
        }
    }

    /// Port of the `set_bits_rbegin()` .. `set_bits_rend()` iteration
    /// (`BASE_SET::set_bits_reverse_iterator`): the positions of the set bits in descending
    /// order.
    pub fn set_bits_rev(&self) -> SetBitsRevIter<'_> {
        SetBitsRevIter {
            set: self,
            index: self.size() as isize - 1,
        }
    }

    /// Port of `std::hash<BASE_SET>`: `hashVal = hashVal * 31 + bit` over all bits starting at
    /// bit 0, with `size_t` (64 bit) wrap-around.
    pub fn hash_value(&self) -> u64 {
        let mut hash_val: u64 = 0;

        for bit in self.iter() {
            hash_val = hash_val.wrapping_mul(31).wrapping_add(u64::from(bit));
        }

        hash_val
    }

    /// Shared implementation of `BASE_SET::operator&=`, `|=` and `^=`: the shorter operand is
    /// zero-extended to the size of the longer one, and `self` takes the larger size.
    fn combine_with(&mut self, other: &BaseSet, op: impl Fn(u64, u64) -> u64) {
        let my_size = self.size();
        let other_size = other.size();

        if my_size < other_size {
            self.resize(other_size);
        }

        let rhs = other.blocks();

        for (i, block) in self.blocks_mut().iter_mut().enumerate() {
            *block = op(*block, rhs.get(i).copied().unwrap_or(0));
        }
    }
}

impl Default for BaseSet {
    /// Port of the default argument of `BASE_SET( size_t size = 64 )`.
    fn default() -> Self {
        BaseSet::new(Self::DEFAULT_SIZE)
    }
}

impl PartialEq for BaseSet {
    /// Port of `sul::dynamic_bitset::operator==`: same number of bits and same blocks.
    fn eq(&self, other: &Self) -> bool {
        self.bits_number == other.bits_number && self.blocks() == other.blocks()
    }
}

impl Eq for BaseSet {}

impl Hash for BaseSet {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.bits_number.hash(state);
        self.blocks().hash(state);
    }
}

impl PartialOrd for BaseSet {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BaseSet {
    /// Port of `BASE_SET::operator<` (see [`BaseSet::compare`]).
    ///
    /// Note that in C++ only `operator<` is defined by `BASE_SET`; `a > b` silently resolves
    /// to the unrelated numeric comparison of `sul::dynamic_bitset`.  The port uses the
    /// `BASE_SET` ordering for all comparison operators.
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other).cmp(&0)
    }
}

impl fmt::Debug for BaseSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BaseSet[{}]({})", self.size(), self.fmt_hex())
    }
}

impl fmt::Display for BaseSet {
    /// Port of `sul::dynamic_bitset`'s `operator<<`: [`BaseSet::to_bit_string`].
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_bit_string())
    }
}

impl Index<usize> for BaseSet {
    type Output = bool;

    /// Port of the read-only `operator[]` (same as [`BaseSet::test`]).
    fn index(&self, pos: usize) -> &bool {
        if self.test(pos) { &true } else { &false }
    }
}

impl BitAndAssign<&BaseSet> for BaseSet {
    /// Port of `BASE_SET::operator&=`.
    fn bitand_assign(&mut self, rhs: &BaseSet) {
        self.combine_with(rhs, |a, b| a & b);
    }
}

impl BitOrAssign<&BaseSet> for BaseSet {
    /// Port of `BASE_SET::operator|=`.
    fn bitor_assign(&mut self, rhs: &BaseSet) {
        self.combine_with(rhs, |a, b| a | b);
    }
}

impl BitXorAssign<&BaseSet> for BaseSet {
    /// Port of `BASE_SET::operator^=`.
    fn bitxor_assign(&mut self, rhs: &BaseSet) {
        self.combine_with(rhs, |a, b| a ^ b);
    }
}

impl Not for &BaseSet {
    type Output = BaseSet;

    /// Port of `BASE_SET::operator~`.
    fn not(self) -> BaseSet {
        let mut result = self.clone();
        result.flip_all();
        result
    }
}

impl Not for BaseSet {
    type Output = BaseSet;

    /// Port of `BASE_SET::operator~`.
    fn not(mut self) -> BaseSet {
        self.flip_all();
        self
    }
}

/// Implements the by-value/by-reference combinations of a binary bit operator and of its
/// compound assignment for a set type that already implements `OpAssign<&T> for T`.
macro_rules! impl_bit_ops {
    ($ty:ty, $op:ident, $method:ident, $op_assign:ident, $method_assign:ident) => {
        impl $op_assign<$ty> for $ty {
            fn $method_assign(&mut self, rhs: $ty) {
                $op_assign::$method_assign(self, &rhs);
            }
        }

        impl $op<&$ty> for &$ty {
            type Output = $ty;

            fn $method(self, rhs: &$ty) -> $ty {
                let mut result = self.clone();
                $op_assign::$method_assign(&mut result, rhs);
                result
            }
        }

        impl $op<$ty> for &$ty {
            type Output = $ty;

            fn $method(self, rhs: $ty) -> $ty {
                let mut result = self.clone();
                $op_assign::$method_assign(&mut result, &rhs);
                result
            }
        }

        impl $op<&$ty> for $ty {
            type Output = $ty;

            fn $method(mut self, rhs: &$ty) -> $ty {
                $op_assign::$method_assign(&mut self, rhs);
                self
            }
        }

        impl $op<$ty> for $ty {
            type Output = $ty;

            fn $method(mut self, rhs: $ty) -> $ty {
                $op_assign::$method_assign(&mut self, &rhs);
                self
            }
        }
    };
}

pub(crate) use impl_bit_ops;

impl_bit_ops!(BaseSet, BitAnd, bitand, BitAndAssign, bitand_assign);
impl_bit_ops!(BaseSet, BitOr, bitor, BitOrAssign, bitor_assign);
impl_bit_ops!(BaseSet, BitXor, bitxor, BitXorAssign, bitxor_assign);

impl<'a> IntoIterator for &'a BaseSet {
    type Item = bool;
    type IntoIter = BitsIter<'a>;

    fn into_iter(self) -> BitsIter<'a> {
        self.iter()
    }
}

/// Iterator over all bits of a [`BaseSet`] (`BASE_SET::const_iterator`).
#[derive(Clone)]
pub struct BitsIter<'a> {
    set: &'a BaseSet,
    pos: usize,
}

impl Iterator for BitsIter<'_> {
    type Item = bool;

    fn next(&mut self) -> Option<bool> {
        if self.pos < self.set.size() {
            let bit = self.set.test(self.pos);
            self.pos += 1;
            Some(bit)
        } else {
            None
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.set.size() - self.pos;
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for BitsIter<'_> {}

/// Iterator over the positions of the set bits of a [`BaseSet`], ascending
/// (`BASE_SET::set_bits_iterator`).
#[derive(Clone)]
pub struct SetBitsIter<'a> {
    set: &'a BaseSet,
    next: Option<usize>,
}

impl Iterator for SetBitsIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        let current = self.next?;
        self.next = self.set.find_next(current);
        Some(current)
    }
}

/// Iterator over the positions of the set bits of a [`BaseSet`], descending
/// (`BASE_SET::set_bits_reverse_iterator`).
#[derive(Clone)]
pub struct SetBitsRevIter<'a> {
    set: &'a BaseSet,
    index: isize,
}

impl Iterator for SetBitsRevIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        // advance_to_previous_set_bit()
        while self.index >= 0 && !self.set.test(self.index as usize) {
            self.index -= 1;
        }

        if self.index < 0 {
            return None;
        }

        let current = self.index as usize;
        self.index -= 1;
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit-by-bit transcription of `alg::lexicographical_compare_three_way`, used to check the
    /// block-wise implementation of [`BaseSet::compare`].
    fn naive_compare(a: &BaseSet, b: &BaseSet) -> i32 {
        let mut it1 = a.iter();
        let mut it2 = b.iter();

        loop {
            match (it1.next(), it2.next()) {
                (Some(x), Some(y)) => {
                    if !x & y {
                        return -1;
                    }

                    if x & !y {
                        return 1;
                    }
                }
                (None, None) => return 0,
                (Some(_), None) => return 1,
                (None, Some(_)) => return -1,
            }
        }
    }

    /// Small deterministic generator for the randomized checks below.
    struct Lcg(u64);

    impl Lcg {
        fn next(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            self.0 >> 33
        }
    }

    fn random_set(rng: &mut Lcg, size: usize) -> BaseSet {
        let mut set = BaseSet::new(size);

        for i in 0..size {
            if rng.next() % 3 == 0 {
                set.set(i);
            }
        }

        set
    }

    #[test]
    fn compare_matches_bitwise_reference() {
        let mut rng = Lcg(42);
        let sizes = [0usize, 1, 5, 63, 64, 65, 127, 128, 129, 200, 256];

        for &sa in &sizes {
            for &sb in &sizes {
                for _ in 0..20 {
                    let a = random_set(&mut rng, sa);
                    let mut b = random_set(&mut rng, sb);

                    assert_eq!(a.compare(&b), naive_compare(&a, &b));

                    // Also compare against a set sharing a long common prefix.
                    b = a.clone();
                    b.resize(sb);
                    assert_eq!(a.compare(&b), naive_compare(&a, &b));
                    assert_eq!(b.compare(&a), naive_compare(&b, &a));
                }
            }
        }
    }

    #[test]
    fn inline_and_heap_storage_behave_identically() {
        let mut set = BaseSet::new(10);
        set.set(3usize);
        set.set(300usize);
        assert_eq!(set.size(), 301);
        assert_eq!(set.count(), 2);
        assert!(set.test(3usize));
        assert!(set.test(300usize));
        assert!(!set.test(301usize));

        set.resize(100);
        assert_eq!(set.size(), 100);
        assert_eq!(set.count(), 1);

        let mut same = BaseSet::new(100);
        same.set(3usize);
        assert_eq!(set, same);

        set.resize_with_value(260, true);
        assert_eq!(set.count(), 1 + 160);
        assert!(set.test(100usize));
        assert!(set.test(259usize));
        assert!(!set.test(99usize));

        set.resize_with_value(70, true);
        set.resize_with_value(128, true);
        assert_eq!(set.count(), 1 + 58);
        assert!(!set.all());
        set.set_all();
        assert!(set.all());
        assert_eq!(set.count(), 128);
    }

    #[test]
    fn find_and_iterate() {
        let mut set = BaseSet::new(200);

        for pos in [0usize, 63, 64, 130, 199] {
            set.set(pos);
        }

        assert_eq!(set.find_first(), Some(0));
        assert_eq!(set.find_next(0), Some(63));
        assert_eq!(set.find_next(63), Some(64));
        assert_eq!(set.find_next(64), Some(130));
        assert_eq!(set.find_next(130), Some(199));
        assert_eq!(set.find_next(199), None);
        assert_eq!(
            set.set_bits().collect::<Vec<_>>(),
            vec![0, 63, 64, 130, 199]
        );
        assert_eq!(
            set.set_bits_rev().collect::<Vec<_>>(),
            vec![199, 130, 64, 63, 0]
        );
        assert_eq!(set.iter().filter(|&b| b).count(), 5);
        assert_eq!(BaseSet::new(0).find_first(), None);
        assert_eq!(BaseSet::new(0).set_bits().count(), 0);
        assert_eq!(BaseSet::new(0).set_bits_rev().count(), 0);
    }

    #[test]
    fn subset_and_intersection() {
        let mut a = BaseSet::new(128);
        let mut b = BaseSet::new(128);
        a.set(1usize).set(100usize);
        b.set(1usize).set(100usize).set(5usize);

        assert!(a.is_subset_of(&b));
        assert!(!b.is_subset_of(&a));
        assert!(a.is_proper_subset_of(&b));
        assert!(!a.is_proper_subset_of(&a));
        assert!(a.intersects(&b));
        assert!(!a.intersects(&BaseSet::new(128)));
    }

    #[test]
    fn string_forms() {
        let mut set = BaseSet::new(10);
        set.set(1usize).set(9usize);

        assert_eq!(set.to_bit_string(), "1000000010");
        assert_eq!(set.to_string(), "1000000010");
        assert_eq!(set.fmt_bin(), "10|0000_0010");
        assert_eq!(set.fmt_hex(), "202");
        assert!(set[1]);
        assert!(!set[2]);

        let mut parsed = BaseSet::new(10);
        assert_eq!(parsed.parse_hex("202"), 3);
        assert_eq!(parsed, set);

        // Parsing stops once all bits of the set are filled.
        let mut small = BaseSet::new(8);
        assert_eq!(small.parse_hex("abcdef"), 2);
        assert_eq!(small.fmt_hex(), "ef");

        // A non-hex byte terminates the scan but counts as consumed, and the set is replaced.
        let mut junk = BaseSet::new(16);
        junk.set(0usize);
        assert_eq!(junk.parse_hex("zz"), 1);
        assert!(junk.none());

        let mut partial = BaseSet::new(16);
        assert_eq!(partial.parse_hex("0x1_f"), 4);
        assert_eq!(partial.fmt_hex(), "001f");

        // Nothing consumed: the set is left alone.
        let mut untouched = BaseSet::new(16);
        untouched.set(4usize);
        assert_eq!(untouched.parse_hex(""), 0);
        assert!(untouched.test(4usize));
    }

    #[test]
    fn negative_positions_are_ignored() {
        let mut set = BaseSet::new(8);
        set.set(-1i32);
        set.flip(-5i32);
        set.reset(-2i32);
        assert_eq!(set.size(), 8);
        assert!(set.none());
        assert!(!set.test(-1i32));
    }

    #[test]
    fn operators_with_different_sizes() {
        let mut a = BaseSet::new(10);
        let mut b = BaseSet::new(200);
        a.set(1usize).set(3usize);
        b.set(3usize).set(150usize);

        let and = &a & &b;
        assert_eq!(and.size(), 200);
        assert_eq!(and.set_bits().collect::<Vec<_>>(), vec![3]);

        let or = &b | &a;
        assert_eq!(or.size(), 200);
        assert_eq!(or.set_bits().collect::<Vec<_>>(), vec![1, 3, 150]);

        let xor = a.clone() ^ b.clone();
        assert_eq!(xor.size(), 200);
        assert_eq!(xor.set_bits().collect::<Vec<_>>(), vec![1, 150]);

        // The larger left-hand side keeps its size.
        let mut c = b.clone();
        c &= &a;
        assert_eq!(c.size(), 200);
        assert_eq!(c.set_bits().collect::<Vec<_>>(), vec![3]);

        let not = !&a;
        assert_eq!(not.size(), 10);
        assert_eq!(not.count(), 8);
    }
}
