// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2024 Alex Shvartzkop <dudesuchamazing@gmail.com>
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

//! Port of `libs/kimath/include/hash_128.h` (`HASH_128`).

/// Port of `HASH_128`: a storage class for a 128-bit hash value.
///
/// The C++ union of `Value8[16]`, `Value32[4]` and `Value64[2]` is stored as the two 64-bit
/// words; [`Hash128::value8`] / [`Hash128::value32`] give the little endian views the union
/// gives on the reference platform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash128 {
    /// `Value64`
    pub value64: [u64; 2],
}

impl Hash128 {
    /// A zero hash (the C++ default member initialiser).
    pub const fn new() -> Self {
        Hash128 { value64: [0, 0] }
    }

    /// Port of `HASH_128::Clear()`.
    pub fn clear(&mut self) {
        self.value64 = [0, 0];
    }

    /// The `Value8` view of the union (little endian).
    pub fn value8(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[..8].copy_from_slice(&self.value64[0].to_le_bytes());
        out[8..].copy_from_slice(&self.value64[1].to_le_bytes());
        out
    }

    /// The `Value32` view of the union (little endian).
    pub fn value32(&self) -> [u32; 4] {
        [
            self.value64[0] as u32,
            (self.value64[0] >> 32) as u32,
            self.value64[1] as u32,
            (self.value64[1] >> 32) as u32,
        ]
    }

    /// Port of `HASH_128::ToString()`: both words as 16 upper case hex digits each.
    pub fn to_string_hex(&self) -> String {
        format!("{:016X}{:016X}", self.value64[0], self.value64[1])
    }
}

impl core::fmt::Display for Hash128 {
    /// Same text as [`Hash128::to_string_hex`].
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.to_string_hex())
    }
}
