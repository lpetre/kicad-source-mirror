// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2020 Ian McInerney <ian.s.mcinerney@ieee.org>
// Copyright (C) 2007-2014 Jean-Pierre Charras, jp.charras at wanadoo.fr
// Copyright The KiCad Developers, see AUTHORS.TXT for contributors.
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
// UUID string parsing, formatting, hashing and random generation follow Boost.Uuid
// (boost/uuid/string_generator.hpp, uuid_io.hpp, uuid.hpp, random_generator.hpp,
// basic_name_generator.hpp):
//   Copyright 2006, 2010 Andy Tompkins.
//   Copyright 2017 James E. King III
//   Distributed under the Boost Software License, Version 1.0. (See
//   accompanying file LICENSE_1_0.txt or copy at
//   https://www.boost.org/LICENSE_1_0.txt)

//! Port of `include/kiid.h` and `common/kiid.cpp` (`KIID`, `KIID_PATH`,
//! `KIID_NIL_SET_RESET`), including the parts of Boost.Uuid they rely on.
//!
//! # UUID generation
//!
//! The C++ code keeps thread-local global state: a `boost::mt19937`, a
//! `boost::uuids::basic_random_generator<boost::mt19937>` seeded from operating system
//! entropy, and the "create nil UUIDs" flag.  A library crate that must run in WebAssembly
//! can have neither hidden global state nor access to OS entropy, so all of it lives in an
//! explicit [`KiidGenerator`] that the caller creates and passes to the functions that need
//! fresh ids:
//!
//! | C++ | Rust |
//! |---|---|
//! | `KIID()` | [`Kiid::new`]`( &mut generator )` |
//! | `KIID( aString )` | [`Kiid::from_string_or_random`]`( s, &mut generator )`, or the pure [`Kiid::parse`] |
//! | `KIID::SeedGenerator( seed )` | [`KiidGenerator::seeded`] / [`KiidGenerator::seed_generator`] |
//! | default (OS entropy) seeding | [`KiidGenerator::from_entropy`] with a caller supplied entropy source |
//! | `KIID::CreateNilUuids( flag )` | [`KiidGenerator::create_nil_uuids`] |
//! | `KIID_NIL_SET_RESET` | [`KiidNilSetReset`] |
//!
//! A generator created with [`KiidGenerator::seeded`] reproduces the id sequence of the C++
//! code after `KIID::SeedGenerator` with the same seed.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Deref, DerefMut};
use std::str::FromStr;

use crate::mt19937::Mt19937;
use crate::sha1::sha1;

/// Port of `timestamp_t`: our type to represent unique IDs for all kinds of elements;
/// historically simply the timestamp when they were created.
pub type Timestamp = u32;

/// Port of `KIID`: a universally unique identifier (the 16 bytes of a
/// `boost::uuids::uuid`).
///
/// The ordering is that of `boost::uuids::uuid::operator<`, a lexicographic comparison of the
/// 16 bytes; it is what KiCad uses to sort items by id.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Kiid {
    uuid: [u8; 16],
}

/// Port of the global `niluuid`: the nil reference.
pub const NILUUID: Kiid = Kiid::nil();

/// Port of `NilUuid()`.
pub const fn nil_uuid() -> Kiid {
    Kiid::nil()
}

/// The error returned when a string is not a valid `KIID` (see [`Kiid::parse`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KiidParseError;

impl fmt::Display for KiidParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // The message of the exception boost::uuids::string_generator throws.
        f.write_str("invalid uuid string")
    }
}

impl std::error::Error for KiidParseError {}

impl Kiid {
    /// Port of the default constructor `KIID::KIID()`: a new random id drawn from
    /// `generator`, or the nil id while the generator is in "create nil UUIDs" mode.
    pub fn new(generator: &mut KiidGenerator) -> Kiid {
        generator.generate()
    }

    /// Port of `KIID::KIID( int null )` (which asserts `null == 0`): the nil id.
    pub const fn nil() -> Kiid {
        Kiid { uuid: [0; 16] }
    }

    /// The deterministic part of `KIID::KIID( const std::string& aString )`.
    ///
    /// A non-empty string of at most 8 hexadecimal digits is a legacy timestamp, which is
    /// stored in the last 4 bytes of the id.  Anything else must be a UUID string in one of
    /// the forms accepted by `boost::uuids::string_generator`:
    ///
    /// ```text
    /// 0123456789abcdef0123456789abcdef
    /// 01234567-89ab-cdef-0123-456789abcdef
    /// {01234567-89ab-cdef-0123-456789abcdef}
    /// {0123456789abcdef0123456789abcdef}
    /// ```
    ///
    /// Returns `None` where the C++ constructor falls back to assigning a new random id (see
    /// [`Kiid::from_string_or_random`]).
    pub fn parse(string: &str) -> Option<Kiid> {
        let bytes = string.as_bytes();

        if !bytes.is_empty() && bytes.len() <= 8 && bytes.iter().all(u8::is_ascii_hexdigit) {
            let mut uuid = [0u8; 16];

            // A legacy-timestamp-based UUID has only the last 4 octets filled in.
            // Convert them individually to avoid stepping in the little-endian/big-endian
            // doo-doo.
            for i in 0..4usize {
                let start = bytes.len() as i32 - 8 + i as i32 * 2;
                let end = start + 2;

                let start = start.max(0);
                let len = (end - start).max(0);

                let octet = &bytes[start as usize..(start + len) as usize];

                // strtol( octet.data(), nullptr, 16 ) of at most two hex digits
                uuid[i + 12] = octet
                    .iter()
                    .fold(0u8, |acc, &c| (acc << 4) | hex_value(c).unwrap_or(0));
            }

            Some(Kiid { uuid })
        } else {
            parse_uuid_string(bytes).map(|uuid| Kiid { uuid })
        }
    }

    /// Port of `KIID::KIID( const std::string& aString )` (and of the `const char*` and
    /// `wxString` overloads, which convert to a UTF-8 `std::string` first).
    ///
    /// When the string is neither a legacy timestamp nor a valid UUID string, the best we can
    /// do is assign a new random one, drawn from `generator`.  As in C++ that fallback id is
    /// random even while the generator is in "create nil UUIDs" mode.
    pub fn from_string_or_random(string: &str, generator: &mut KiidGenerator) -> Kiid {
        match Self::parse(string) {
            Some(id) => id,
            // Failed to parse string representation; best we can do is assign a new
            // random one.
            None => generator.random_uuid(),
        }
    }

    /// Port of `KIID::KIID( timestamp_t aTimestamp )`: an id holding a legacy timestamp in
    /// its last 4 bytes.
    pub const fn from_legacy_timestamp(timestamp: Timestamp) -> Kiid {
        let mut uuid = [0u8; 16];

        uuid[12] = (timestamp >> 24) as u8;
        uuid[13] = (timestamp >> 16) as u8;
        uuid[14] = (timestamp >> 8) as u8;
        uuid[15] = timestamp as u8;

        Kiid { uuid }
    }

    /// Port of `KIID::Hash` (`boost::uuids::hash_value` of Boost up to 1.85, with a 64 bit
    /// `size_t`).
    ///
    /// The value is only meant for in-memory hashing; newer Boost versions compute a
    /// different value.
    pub fn hash_value(&self) -> u64 {
        let mut seed: u64 = 0;

        for &byte in &self.uuid {
            seed ^= u64::from(byte)
                .wrapping_add(0x9e37_79b9)
                .wrapping_add(seed << 6)
                .wrapping_add(seed >> 2);
        }

        seed
    }

    /// Port of `KIID::FromName`: return a `KIID` derived from a name, the same name always
    /// gives the same `KIID` (a version 5, SHA-1 name based UUID).
    pub fn from_name(name: &str) -> Kiid {
        // Arbitrary but fixed namespace, the same name must map to the same KIID forever
        // "8b8b58e2-3d21-4a24-9dcf-42e0f14001a2"
        const NS_UUID: [u8; 16] = [
            0x8b, 0x8b, 0x58, 0xe2, 0x3d, 0x21, 0x4a, 0x24, 0x9d, 0xcf, 0x42, 0xe0, 0xf1, 0x40,
            0x01, 0xa2,
        ];

        // boost::uuids::name_generator_sha1: hash the namespace bytes followed by the name
        // bytes, and use the first 16 bytes of the digest.
        let digest = sha1(&[&NS_UUID, name.as_bytes()]);

        let mut uuid = [0u8; 16];
        uuid.copy_from_slice(&digest[..16]);

        // set variant: must be 0b10xxxxxx
        uuid[8] &= 0xBF;
        uuid[8] |= 0x80;

        // set version
        const HASHVER: u8 = 5; // boost::uuids::detail::sha1::get_version()
        uuid[6] &= 0x0F; // clear out the relevant bits
        uuid[6] |= HASHVER << 4; // and apply them

        Kiid { uuid }
    }

    /// Port of `KIID::FromBytes`.
    pub const fn from_bytes(bytes: [u8; 16]) -> Kiid {
        Kiid { uuid: bytes }
    }

    /// Port of `KIID::IsLegacyTimestamp`: true when bytes 8 to 11 are all zero (which is also
    /// the case for the nil id).
    pub const fn is_legacy_timestamp(&self) -> bool {
        self.uuid[8] == 0 && self.uuid[9] == 0 && self.uuid[10] == 0 && self.uuid[11] == 0
    }

    /// Port of `KIID::AsLegacyTimestamp`: the last 4 bytes as a big-endian number.
    pub const fn as_legacy_timestamp(&self) -> Timestamp {
        let mut ret: Timestamp = 0;

        ret |= (self.uuid[12] as u32) << 24;
        ret |= (self.uuid[13] as u32) << 16;
        ret |= (self.uuid[14] as u32) << 8;
        ret |= self.uuid[15] as u32;

        ret
    }

    /// Port of `KIID::AsString` (`boost::uuids::to_string`): the canonical lower case form
    /// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
    pub fn as_string(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";

        let mut result = String::with_capacity(36);

        for (i, &byte) in self.uuid.iter().enumerate() {
            result.push(HEX[usize::from(byte >> 4)] as char);
            result.push(HEX[usize::from(byte & 0x0F)] as char);

            if i == 3 || i == 5 || i == 7 || i == 9 {
                result.push('-');
            }
        }

        result
    }

    /// Port of `KIID::AsStdString`; identical to [`Kiid::as_string`].
    pub fn as_std_string(&self) -> String {
        self.as_string()
    }

    /// Port of `KIID::AsLegacyTimestampString`: the legacy timestamp as 8 upper case
    /// hexadecimal digits.
    pub fn as_legacy_timestamp_string(&self) -> String {
        format!("{:08X}", self.as_legacy_timestamp())
    }

    /// Port of `KIID::AsBytes`.
    pub const fn as_bytes(&self) -> [u8; 16] {
        self.uuid
    }

    /// Port of `KIID::SniffTest`: returns true if a string has the correct formatting to be a
    /// `KIID` (the length of a canonical UUID string, and only hexadecimal digits and dashes).
    pub fn sniff_test(candidate: &str) -> bool {
        // niluuid.AsString().Length()
        const NILUUID_STR_LENGTH: usize = 36;

        // Any non-ASCII character fails the character test below, so comparing the byte
        // length is equivalent to comparing the number of characters.
        if candidate.len() != NILUUID_STR_LENGTH {
            return false;
        }

        candidate
            .bytes()
            .all(|c| c.is_ascii_hexdigit() || c == b'-')
    }

    /// Port of `KIID::ConvertTimestampToUuid`: change an existing time stamp based UUID into
    /// a true UUID.
    ///
    /// If this is not a time stamp based UUID, then no change is made.
    pub fn convert_timestamp_to_uuid(&mut self, generator: &mut KiidGenerator) {
        if !self.is_legacy_timestamp() {
            return;
        }

        *self = generator.random_uuid();
    }

    /// Port of `KIID::Combine`: creates a deterministic `KIID` from two input `KIID`s by
    /// XORing their underlying UUIDs.
    ///
    /// This is useful for generating stable IDs for derived objects (like teardrops) that are
    /// created from the combination of two parent objects.
    pub fn combine(first: &Kiid, second: &Kiid) -> Kiid {
        let mut result = Kiid::nil();

        for i in 0..16 {
            result.uuid[i] = first.uuid[i] ^ second.uuid[i];
        }

        result
    }

    /// Port of `KIID::FromDeterministicString`: build a deterministic UUID from an arbitrary
    /// name string.
    ///
    /// The same input always yields the same UUID, and distinct inputs almost always yield
    /// distinct UUIDs.  This is useful for keying objects that have a stable name but no
    /// native UUID (e.g. library symbols / footprints, which are file-name keyed rather than
    /// UUID keyed) so that two callsites derive the identical id for the same name.
    ///
    /// The output bytes are part of the diff/merge engine's on-disk contract; the hashing
    /// here must not change once persisted artifacts depend on it.
    ///
    /// C++ hashes over `wxChar` code units, so for non-ASCII input the result differs between
    /// platforms with 16-bit vs 32-bit `wxChar` (Windows vs Linux/macOS).  The port hashes
    /// Unicode code points, i.e. it follows the 32-bit `wxChar` platforms.
    pub fn from_deterministic_string(name: &str) -> Kiid {
        // Dual FNV-1a accumulators fold the name into two 64-bit values, which are
        // then laid out as a v4-shaped UUID string and parsed back into a KIID so
        // boost's string_generator produces identical bytes on every call. Do not
        // alter the constants or bit layout: this output is embedded in persisted
        // diff/merge artifacts and must stay byte-identical.
        let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
        let mut h2: u64 = 0x8422_2325_cbf2_9ce4;

        for c in name.chars() {
            let c = u64::from(u32::from(c));

            h1 ^= c;
            h1 = h1.wrapping_mul(0x0000_0100_0000_01b3);
            h2 ^= c.wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h2 = (h2 << 13) | (h2 >> 51);
        }

        // "%08x-%04x-%04x-%04x-%012llx" of
        //   h1 >> 32, ( h1 >> 16 ) & 0xffff, h1 & 0xffff, h2 & 0xffff,
        //   ( h2 >> 16 ) & 0xffffffffffff
        // parsed back as a UUID string, i.e. these 16 bytes:
        let mut uuid = [0u8; 16];

        uuid[0..8].copy_from_slice(&h1.to_be_bytes());
        uuid[8..10].copy_from_slice(&((h2 & 0xffff) as u16).to_be_bytes());
        uuid[10..16].copy_from_slice(&(h2 >> 16).to_be_bytes()[2..8]);

        Kiid { uuid }
    }

    /// Port of `KIID::Increment`: generates a deterministic replacement for a given ID.
    ///
    /// NB: destroys uniform distribution!  But it's the only thing we have when a
    /// deterministic replacement for a duplicate ID is required.
    pub fn increment(&mut self) {
        // This obviously destroys uniform distribution, but it can be useful when a
        // deterministic replacement for a duplicate ID is required.

        for i in (0..16).rev() {
            self.uuid[i] = self.uuid[i].wrapping_add(1);

            if self.uuid[i] != 0 {
                break;
            }
        }
    }
}

impl fmt::Display for Kiid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl fmt::Debug for Kiid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Kiid({})", self.as_string())
    }
}

impl FromStr for Kiid {
    type Err = KiidParseError;

    /// See [`Kiid::parse`].
    fn from_str(s: &str) -> Result<Kiid, KiidParseError> {
        Kiid::parse(s).ok_or(KiidParseError)
    }
}

impl From<[u8; 16]> for Kiid {
    fn from(bytes: [u8; 16]) -> Kiid {
        Kiid::from_bytes(bytes)
    }
}

impl From<Kiid> for [u8; 16] {
    fn from(id: Kiid) -> [u8; 16] {
        id.as_bytes()
    }
}

/// Value of a hexadecimal digit (`boost::uuids::string_generator::get_value`).
fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Port of `boost::uuids::string_generator::operator()`; `None` where it throws
/// `std::runtime_error( "invalid uuid string" )`.
fn parse_uuid_string(s: &[u8]) -> Option<[u8; 16]> {
    let mut chars = s.iter().copied();

    // check open brace
    let mut c = chars.next()?;
    let has_open_brace = c == b'{';

    if has_open_brace {
        c = chars.next()?;
    }

    let mut has_dashes = false;

    let mut u = [0u8; 16];

    for i in 0..16 {
        if i != 0 {
            c = chars.next()?;
        }

        if i == 4 {
            has_dashes = c == b'-';

            if has_dashes {
                c = chars.next()?;
            }
        }
        // if there are dashes, they must be in every slot
        else if (i == 6 || i == 8 || i == 10) && has_dashes {
            if c == b'-' {
                c = chars.next()?;
            } else {
                return None;
            }
        }

        u[i] = hex_value(c)?;

        c = chars.next()?;
        u[i] <<= 4;
        u[i] |= hex_value(c)?;
    }

    // check close brace
    if has_open_brace {
        c = chars.next()?;

        if c != b'}' {
            return None;
        }
    }

    // check end of string - any additional data is an invalid uuid
    if chars.next().is_some() {
        return None;
    }

    Some(u)
}

/// The source of new `KIID`s: the port of the thread-local generator state of
/// `common/kiid.cpp` (`rng`, `randomGenerator` and `g_createNilUuids`).
///
/// Random ids are produced like `boost::uuids::basic_random_generator<boost::mt19937>` does:
/// four consecutive 32 bit outputs of the Mersenne Twister are stored little-endian into the
/// 16 bytes, then the RFC 4122 variant and the version 4 are set.  (This holds for Boost up to
/// 1.85 on both LP64 and LLP64 platforms, where the bytes are taken from
/// `uniform_int<unsigned long>` values, and for Boost 1.86 and later on little-endian
/// platforms.)
///
/// * [`KiidGenerator::seeded`] is deterministic and reproduces the sequence of the C++ code
///   after `KIID::SeedGenerator( seed )`.
/// * [`KiidGenerator::from_entropy`] corresponds to the default C++ behaviour, where the
///   Mersenne Twister state is seeded from operating system entropy.  The entropy comes from
///   a caller supplied source because this crate has no access to the operating system.
///
/// [`Default`] gives `KiidGenerator::seeded( 5489 )` (the default seed of `boost::mt19937`):
/// deterministic, as the porting rules require of anything C++ takes from the environment.
/// Beware that ids produced by two generators with the same seed collide; use
/// [`KiidGenerator::from_entropy`] whenever generated ids end up in files that may later be
/// combined with other files.
#[derive(Clone, Debug)]
pub struct KiidGenerator {
    rng: Mt19937,
    /// When true, always create nil uuids for performance, when valid ones aren't needed.
    create_nil_uuids: bool,
}

impl KiidGenerator {
    /// Number of entropy bytes [`KiidGenerator::from_entropy`] requests from its source.
    pub const ENTROPY_BYTES: usize = Mt19937::STATE_SIZE * 4;

    /// A deterministic generator: the port of `KIID::SeedGenerator( aSeed )` applied to a
    /// fresh generator.
    pub fn seeded(seed: u32) -> Self {
        KiidGenerator {
            rng: Mt19937::from_seed(seed),
            create_nil_uuids: false,
        }
    }

    /// A non-deterministic generator.
    ///
    /// `fill` is called once with a buffer of [`KiidGenerator::ENTROPY_BYTES`] bytes that it
    /// must fill with random bytes (for instance from `getrandom` in a native program or
    /// `crypto.getRandomValues` in a browser).  The bytes seed the complete state of the
    /// Mersenne Twister, like the default constructor of
    /// `boost::uuids::basic_random_generator<boost::mt19937>` does with bytes from the
    /// operating system.
    pub fn from_entropy(mut fill: impl FnMut(&mut [u8])) -> Self {
        let mut bytes = [0u8; Self::ENTROPY_BYTES];
        fill(&mut bytes);

        let mut words = [0u32; Mt19937::STATE_SIZE];

        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }

        Self::from_seed_words(&words)
    }

    /// A generator whose Mersenne Twister state is seeded from the given 624 words
    /// (`boost::mt19937::seed( SeedSeq& )`).
    pub fn from_seed_words(words: &[u32; Mt19937::STATE_SIZE]) -> Self {
        KiidGenerator {
            rng: Mt19937::from_seed_words(words),
            create_nil_uuids: false,
        }
    }

    /// Port of `KIID::SeedGenerator`: re-initialize the UUID generator with a given seed (for
    /// testing or QA purposes).
    ///
    /// WARNING: Resetting the UUID generator in the middle of a run will potentially have
    /// harmful effects on file integrity.
    pub fn seed_generator(&mut self, seed: u32) {
        self.rng.seed(seed);
    }

    /// Port of `KIID::CreateNilUuids`: a performance optimization which disables/enables the
    /// generation of pseudo-random UUIDs by [`KiidGenerator::generate`] / [`Kiid::new`].
    pub fn create_nil_uuids(&mut self, nil: bool) {
        self.create_nil_uuids = nil;
    }

    /// True while [`KiidGenerator::generate`] returns nil ids (`g_createNilUuids`).
    pub fn is_creating_nil_uuids(&self) -> bool {
        self.create_nil_uuids
    }

    /// Port of the default constructor `KIID::KIID()`: the nil id in "create nil UUIDs" mode,
    /// a random id otherwise.
    pub fn generate(&mut self) -> Kiid {
        if self.create_nil_uuids {
            Kiid::nil()
        } else {
            self.random_uuid()
        }
    }

    /// Port of `randomGenerator()`
    /// (`boost::uuids::basic_random_generator<boost::mt19937>::operator()`): a random
    /// (version 4) UUID, regardless of the "create nil UUIDs" mode.
    pub fn random_uuid(&mut self) -> Kiid {
        let mut u = [0u8; 16];

        for chunk in u.chunks_exact_mut(4) {
            chunk.copy_from_slice(&self.rng.next_u32().to_le_bytes());
        }

        // set variant
        // must be 0b10xxxxxx
        u[8] &= 0xBF;
        u[8] |= 0x80;

        // set version
        // must be 0b0100xxxx
        u[6] &= 0x4F; //0b01001111
        u[6] |= 0x40; //0b01000000

        Kiid { uuid: u }
    }
}

impl Default for KiidGenerator {
    /// `KiidGenerator::seeded( 5489 )`, see the type documentation for the caveats.
    fn default() -> Self {
        KiidGenerator::seeded(Mt19937::DEFAULT_SEED)
    }
}

/// Port of `KIID_NIL_SET_RESET`: RAII class to safely set/reset nil KIIDs for use in
/// footprint/symbol loading.
///
/// Creating the guard switches the generator to "create nil UUIDs" mode; dropping it switches
/// the mode off again (unconditionally, like the C++ destructor).  The guard dereferences to
/// the generator so that it can be used while the guard is alive.
pub struct KiidNilSetReset<'a> {
    generator: &'a mut KiidGenerator,
}

impl<'a> KiidNilSetReset<'a> {
    /// Port of `KIID_NIL_SET_RESET::KIID_NIL_SET_RESET`.
    pub fn new(generator: &'a mut KiidGenerator) -> Self {
        generator.create_nil_uuids(true);
        KiidNilSetReset { generator }
    }
}

impl Drop for KiidNilSetReset<'_> {
    fn drop(&mut self) {
        self.generator.create_nil_uuids(false);
    }
}

impl Deref for KiidNilSetReset<'_> {
    type Target = KiidGenerator;

    fn deref(&self) -> &KiidGenerator {
        self.generator
    }
}

impl DerefMut for KiidNilSetReset<'_> {
    fn deref_mut(&mut self) -> &mut KiidGenerator {
        self.generator
    }
}

/// Port of `KIID_PATH` (a `std::vector<KIID>`): the path of sheet ids leading to an item.
///
/// The type dereferences to its `Vec<Kiid>`, like the C++ class derives from
/// `std::vector<KIID>`.  The ordering is the C++ one: a shorter path sorts before a longer
/// one, paths of equal length are compared element by element.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct KiidPath(pub Vec<Kiid>);

impl KiidPath {
    /// Port of `KIID_PATH::KIID_PATH()`: an empty path.
    pub fn new() -> Self {
        KiidPath(Vec::new())
    }

    /// The deterministic variant of `KIID_PATH::KIID_PATH( const wxString& aString )`: parse
    /// a path of the form `/<kiid>/<kiid>...`.
    ///
    /// Returns `None` when a path step is not a valid `KIID` string, where the C++
    /// constructor silently substitutes a random id (see
    /// [`KiidPath::from_string_or_random`]).
    pub fn parse(string: &str) -> Option<KiidPath> {
        let mut path = KiidPath::new();

        for path_step in wx_split(string, '/', '\\') {
            if !path_step.is_empty() {
                path.push(Kiid::parse(&path_step)?);
            }
        }

        Some(path)
    }

    /// Port of `KIID_PATH::KIID_PATH( const wxString& aString )`: parse a path of the form
    /// `/<kiid>/<kiid>...`.  Path steps that are not valid `KIID` strings become random ids
    /// drawn from `generator`, like in `KIID::KIID( const wxString& )`.
    pub fn from_string_or_random(string: &str, generator: &mut KiidGenerator) -> KiidPath {
        let mut path = KiidPath::new();

        for path_step in wx_split(string, '/', '\\') {
            if !path_step.is_empty() {
                path.push(Kiid::from_string_or_random(&path_step, generator));
            }
        }

        path
    }

    /// Port of `KIID_PATH::MakeRelativeTo`: remove the leading `path` from this path.
    ///
    /// Returns false when this path is not contained within `path`.  As in C++ this path is
    /// left empty when `path` is longer than this path, and unchanged when it differs.
    pub fn make_relative_to(&mut self, path: &KiidPath) -> bool {
        let copy = std::mem::take(&mut self.0);

        if path.len() > copy.len() {
            return false; // this path is not contained within aPath
        }

        for i in 0..path.len() {
            if copy[i] != path[i] {
                self.0 = copy;
                return false; // this path is not contained within aPath
            }
        }

        self.0.extend_from_slice(&copy[path.len()..]);

        true
    }

    /// Port of `KIID_PATH::EndsWith`: test if `path` matches from the last path towards the
    /// first path.
    ///
    /// This is useful for testing for existing schematic symbol and sheet instances when
    /// copying or adding a new sheet that is lower in the hierarchy than the current path.
    ///
    /// Returns true if this path ends with `path` or false if it does not.
    pub fn ends_with(&self, path: &KiidPath) -> bool {
        if path.len() > self.len() {
            return false; // this path can not end aPath
        }

        self.0.ends_with(&path.0)
    }

    /// Port of `KIID_PATH::IsContainedWithin`: true when this path starts with
    /// `parent_path`.
    pub fn is_contained_within(&self, parent_path: &KiidPath) -> bool {
        self.len() >= parent_path.len() && self.0.starts_with(&parent_path.0)
    }

    /// Port of `KIID_PATH::AsString`: `/<kiid>/<kiid>...` (the empty string for an empty
    /// path).
    pub fn as_string(&self) -> String {
        let mut path = String::new();

        for path_step in &self.0 {
            path.push('/');
            path.push_str(&path_step.as_string());
        }

        path
    }

    /// Port of `std::hash<KIID_PATH>`.
    pub fn hash_value(&self) -> u64 {
        let mut seed: u64 = 0;

        for kiid in &self.0 {
            seed ^= kiid
                .hash_value()
                .wrapping_add(0x9e37_79b9)
                .wrapping_add(seed << 6)
                .wrapping_add(seed >> 2);
        }

        seed
    }
}

impl Deref for KiidPath {
    type Target = Vec<Kiid>;

    fn deref(&self) -> &Vec<Kiid> {
        &self.0
    }
}

impl DerefMut for KiidPath {
    fn deref_mut(&mut self) -> &mut Vec<Kiid> {
        &mut self.0
    }
}

impl PartialOrd for KiidPath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for KiidPath {
    /// Port of `KIID_PATH::operator<` and `operator>`: the sizes are compared first, then the
    /// elements.
    fn cmp(&self, other: &Self) -> Ordering {
        self.len()
            .cmp(&other.len())
            .then_with(|| self.0.cmp(&other.0))
    }
}

impl AddAssign<&KiidPath> for KiidPath {
    /// Port of `KIID_PATH::operator+=`.
    fn add_assign(&mut self, rhs: &KiidPath) {
        self.0.extend_from_slice(&rhs.0);
    }
}

impl Add<&KiidPath> for KiidPath {
    type Output = KiidPath;

    /// Port of `operator+( KIID_PATH aLhs, const KIID_PATH& aRhs )`.
    fn add(mut self, rhs: &KiidPath) -> KiidPath {
        self += rhs;
        self
    }
}

impl fmt::Display for KiidPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.as_string())
    }
}

impl From<Vec<Kiid>> for KiidPath {
    fn from(path: Vec<Kiid>) -> Self {
        KiidPath(path)
    }
}

impl FromIterator<Kiid> for KiidPath {
    fn from_iter<I: IntoIterator<Item = Kiid>>(iter: I) -> Self {
        KiidPath(iter.into_iter().collect())
    }
}

impl IntoIterator for KiidPath {
    type Item = Kiid;
    type IntoIter = std::vec::IntoIter<Kiid>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a KiidPath {
    type Item = &'a Kiid;
    type IntoIter = std::slice::Iter<'a, Kiid>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Port of wxWidgets' `wxSplit( str, sep, escape )` as used by `KIID_PATH`.
///
/// Splits `string` at each `sep`.  A separator preceded by the `escape` character is not a
/// separator: the escape character is replaced by the separator instead.  An empty string
/// gives no tokens; otherwise the last token is always added, even when empty.
fn wx_split(string: &str, sep: char, escape: char) -> Vec<String> {
    let mut ret = Vec::new();
    let mut curr = String::new();
    let mut prev = '\0';

    for ch in string.chars() {
        if ch == sep {
            if prev == escape {
                // remove the escape character and don't consider this
                // occurrence of 'sep' as a real separator
                curr.pop();
                curr.push(sep);
            } else {
                // real separator
                ret.push(std::mem::take(&mut curr));
            }
        } else {
            // normal character
            curr.push(ch);
        }

        prev = ch;
    }

    // add the last token, which we always have unless the string is empty
    if !string.is_empty() {
        ret.push(curr);
    }

    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wx_split_semantics() {
        assert_eq!(wx_split("", '/', '\\'), Vec::<String>::new());
        assert_eq!(wx_split("/", '/', '\\'), vec!["", ""]);
        assert_eq!(wx_split("/a/b", '/', '\\'), vec!["", "a", "b"]);
        assert_eq!(wx_split("a/b/", '/', '\\'), vec!["a", "b", ""]);
        assert_eq!(wx_split("a\\/b/c", '/', '\\'), vec!["a/b", "c"]);
        assert_eq!(wx_split("a\\b", '/', '\\'), vec!["a\\b"]);
    }

    #[test]
    fn string_generator_forms() {
        let expected = [
            0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab,
            0xcd, 0xef,
        ];

        // Accepted by boost::uuids::string_generator.
        for s in [
            "01234567-89ab-cdef-0123-456789abcdef",
            "0123456789abcdef0123456789abcdef",
            "{01234567-89ab-cdef-0123-456789abcdef}",
            "{0123456789abcdef0123456789abcdef}",
            "01234567-89AB-CDEF-0123-456789ABCDEF",
        ] {
            assert_eq!(parse_uuid_string(s.as_bytes()), Some(expected), "{s}");
        }

        // Rejected (boost throws).
        for s in [
            "01234567-89abcdef0123456789abcdef",
            "0123456789ab-cdef-0123-456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef0",
            "01234567-89ab-cdef-0123-456789abcde",
            "{01234567-89ab-cdef-0123-456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef}",
            "g1234567-89ab-cdef-0123-456789abcdef",
            "",
            "012345678",
            "01234567-89ab-cdef-0123456789abcdef",
            "01234567-89ab-cdef-0123-4567-89abcdef",
            " 01234567-89ab-cdef-0123-456789abcdef",
            "01234567-89ab-cdef-0123-456789abcdef ",
            "0123456789abcdef-0123456789abcdef",
            "01234567--89ab-cdef-0123-456789abcdef",
        ] {
            assert_eq!(parse_uuid_string(s.as_bytes()), None, "{s}");
        }
    }
}
