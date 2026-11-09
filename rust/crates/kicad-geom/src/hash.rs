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
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `libs/kimath/include/hash.h` (`hash_combine`, `hash_val`), together with the
//! libstdc++ `std::hash` specialisations they call ([`StdHash`]).
//!
//! `std::size_t` is 64 bits on the reference platform (x86-64 Linux, `PORTING.md` section 9),
//! so every hash here is a `u64` on all targets, including wasm32.  The values are those of
//! libstdc++ (GCC 13): `std::hash` of an integer is the integer converted to `size_t`, of a
//! floating point value and of a string it is `std::_Hash_bytes` (`libsupc++/hash_bytes.cc`)
//! with the seed `0xc70f6907`.
//!
//! The variadic C++ templates are the macros [`hash_combine!`](crate::hash_combine) and
//! [`hash_val!`](crate::hash_val); [`hash_combine`] is the single-value step.

/// The seed libstdc++ uses for `std::hash` of floating point values and strings
/// (`std::_Hash_impl::hash`).
pub const STD_HASH_SEED: u64 = 0xc70f_6907;

/// The seed of `hash_val`.
pub const HASH_VAL_SEED: u64 = 0xa82d_e1c0;

#[inline]
fn shift_mix(v: u64) -> u64 {
    v ^ (v >> 47)
}

/// Port of libstdc++ `std::_Hash_bytes( ptr, len, seed )` for a 64-bit `size_t`
/// (`libsupc++/hash_bytes.cc`, a variant of MurmurHash64A).
pub fn std_hash_bytes(a_data: &[u8], a_seed: u64) -> u64 {
    const MUL: u64 = (0xc6a4_a793_u64 << 32) + 0x5bd1_e995_u64;

    let len = a_data.len() as u64;
    let len_aligned = a_data.len() & !0x7;
    let mut hash: u64 = a_seed ^ len.wrapping_mul(MUL);

    for chunk in a_data[..len_aligned].chunks_exact(8) {
        let mut word = [0u8; 8];
        word.copy_from_slice(chunk);
        let data = shift_mix(u64::from_le_bytes(word).wrapping_mul(MUL)).wrapping_mul(MUL);
        hash ^= data;
        hash = hash.wrapping_mul(MUL);
    }

    let tail = &a_data[len_aligned..];

    if !tail.is_empty() {
        // load_bytes(): the remaining bytes as a little endian number
        let mut data: u64 = 0;

        for &b in tail.iter().rev() {
            data = (data << 8).wrapping_add(b as u64);
        }

        hash ^= data;
        hash = hash.wrapping_mul(MUL);
    }

    hash = shift_mix(hash).wrapping_mul(MUL);
    shift_mix(hash)
}

/// The libstdc++ `std::hash<T>` of a type, as used by `hash_combine`.
pub trait StdHash {
    /// `std::hash<T>()( *this )`.
    fn std_hash(&self) -> u64;
}

macro_rules! impl_std_hash_int {
    ($($t:ty),*) => {
        $(
            impl StdHash for $t {
                /// Integers hash to themselves, converted to `size_t` (sign extended).
                #[inline]
                #[allow(clippy::unnecessary_cast)]
                fn std_hash(&self) -> u64 {
                    *self as i128 as u64
                }
            }
        )*
    };
}

impl_std_hash_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl StdHash for bool {
    #[inline]
    fn std_hash(&self) -> u64 {
        *self as u64
    }
}

impl StdHash for char {
    /// `std::hash<wchar_t>` / `char32_t`: the code point.
    #[inline]
    fn std_hash(&self) -> u64 {
        *self as u64
    }
}

impl StdHash for f64 {
    /// `std::hash<double>`: 0 for both zeros, else `_Hash_bytes` of the representation.
    #[inline]
    fn std_hash(&self) -> u64 {
        if *self != 0.0 {
            std_hash_bytes(&self.to_le_bytes(), STD_HASH_SEED)
        } else {
            0
        }
    }
}

impl StdHash for f32 {
    /// `std::hash<float>`: 0 for both zeros, else `_Hash_bytes` of the representation.
    #[inline]
    fn std_hash(&self) -> u64 {
        if *self != 0.0 {
            std_hash_bytes(&self.to_le_bytes(), STD_HASH_SEED)
        } else {
            0
        }
    }
}

impl StdHash for str {
    /// `std::hash<std::string>` of the UTF-8 bytes.
    #[inline]
    fn std_hash(&self) -> u64 {
        std_hash_bytes(self.as_bytes(), STD_HASH_SEED)
    }
}

impl StdHash for String {
    #[inline]
    fn std_hash(&self) -> u64 {
        self.as_str().std_hash()
    }
}

impl StdHash for [u8] {
    /// `std::hash<std::string>` of a string holding these bytes.
    #[inline]
    fn std_hash(&self) -> u64 {
        std_hash_bytes(self, STD_HASH_SEED)
    }
}

impl StdHash for Vec<u8> {
    /// `std::hash<std::string>` of a string holding these bytes.
    #[inline]
    fn std_hash(&self) -> u64 {
        self.as_slice().std_hash()
    }
}

impl<T: StdHash + ?Sized> StdHash for &T {
    #[inline]
    fn std_hash(&self) -> u64 {
        (**self).std_hash()
    }
}

/// One step of `hash_combine( std::size_t& seed, const T& val, ... )`:
/// `seed ^= std::hash<T>()( val ) + 0x9e3779b9 + ( seed << 6 ) + ( seed >> 2 )`.
#[inline]
pub fn hash_combine<T: StdHash + ?Sized>(seed: &mut u64, val: &T) {
    *seed ^= val
        .std_hash()
        .wrapping_add(0x9e37_79b9)
        .wrapping_add(*seed << 6)
        .wrapping_add(*seed >> 2);
}

/// Port of the variadic `hash_combine( seed, args... )`: combines the values left to right.
///
/// `hash_combine!(seed, a, b, c)` where `seed` is a `u64` place.
#[macro_export]
macro_rules! hash_combine {
    ($seed:expr $(, $val:expr)* $(,)?) => {{
        $( $crate::hash::hash_combine(&mut $seed, &$val); )*
    }};
}

/// Port of `hash_val( args... )`: `hash_combine` of all values into the seed `0xa82de1c0`.
#[macro_export]
macro_rules! hash_val {
    ($($val:expr),* $(,)?) => {{
        let mut seed: u64 = $crate::hash::HASH_VAL_SEED;
        $( $crate::hash::hash_combine(&mut seed, &$val); )*
        seed
    }};
}
